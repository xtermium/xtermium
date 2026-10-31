//! Cloud sync (docs/sync.md): the encrypted sessions and trusted keys, and the recovery key, go
//! to a sync server and other devices' changes come back. The server only ever holds ciphertext
//! and the passphrase-sealed recovery key.
//!
//! When only one side changed, its copy replaces the other's whole (deletions included). When
//! both did, nothing is merged: the person picks this device's copy or the cloud's, seeing when
//! and where each was last changed. A new device joins with the recovery passphrase, which
//! unseals the account's data key.

use crate::config::{Known, Store};
use crate::vault::{self, Key};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

/// What a device has, decrypted.
#[derive(Clone)]
pub struct Device {
    pub key: Key,
    pub store: Store,
    pub known: Known,
    /// `recovery.key`'s text; None until a recovery passphrase is set.
    pub recovery: Option<String>,
    pub state: State,
    /// When this device's data last changed (Unix seconds), if known.
    pub changed: Option<u64>,
}

/// Where this device stands with the server (`sync.xtm`).
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct State {
    /// The server's revision this device last matched.
    pub revision: u64,
    /// Digest of what was in sync then; anything else means this device changed since.
    pub synced: String,
}

/// Which copy wins a conflict.
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Local,
    Remote,
}

/// One side of a conflict, for the person choosing.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Side {
    /// Last changed (Unix seconds).
    pub at: Option<u64>,
    pub device: String,
    /// "Windows", "Linux" or "macOS".
    pub os: String,
    /// Where the change was uploaded from, as the server saw it; empty for this device.
    pub ip: String,
    pub location: String,
    pub sessions: usize,
    pub tunnels: usize,
    pub keys: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct Conflict {
    pub local: Side,
    pub remote: Side,
}

#[derive(Serialize, Default, Debug)]
pub struct Outcome {
    /// Changes came from the server.
    pub pulled: bool,
    /// This device's changes went up.
    pub pushed: bool,
    /// Both sides changed: nothing happened yet; ask, then sync again with a [`Choice`].
    pub conflict: Option<Conflict>,
    /// The account's storage, as the server last said.
    pub quota: Option<Quota>,
}

/// The server holds a vault this device's key can't open: join it with the recovery passphrase.
#[derive(Debug)]
pub struct NeedsPassphrase;

impl std::fmt::Display for NeedsPassphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("This account's data was set up on another device: enter its recovery passphrase to join.")
    }
}

impl std::error::Error for NeedsPassphrase {}

#[derive(Serialize, Deserialize)]
struct Files {
    sessions: String,
    known: String,
    /// Sealed [`Meta`]: which device uploaded this and when. Absent from older uploads.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    meta: String,
}

/// Who uploaded a revision, sealed with the data: the server never sees it.
#[derive(Serialize, Deserialize, Default)]
struct Meta {
    device: String,
    os: String,
    at: u64,
}

/// What the server recorded about the upload that made the current revision.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Updated {
    at: Option<u64>,
    ip: String,
    location: String,
}

/// The account's storage: bytes held and bytes allowed. Servers that don't say give none.
#[derive(Serialize, Deserialize, Clone, Copy, Default, Debug, PartialEq)]
pub struct Quota {
    pub used: u64,
    pub total: u64,
}

#[derive(Deserialize)]
struct Remote {
    revision: u64,
    files: Files,
    recovery: String,
    #[serde(default)]
    updated: Updated,
    #[serde(default)]
    quota: Option<Quota>,
}

pub struct Api {
    url: String,
    token: String,
    http: reqwest::Client,
}

/// Biggest answer accepted (the API caps files at a few MB).
const MAX: usize = 32 << 20;

/// xtermium's sync server, the one the app talks to: Settings has no address to change.
/// `XTERMIUM_SYNC_URL` points a development build at another (as `XTERMIUM_HOME` moves the data).
pub const DEFAULT_URL: &str = "https://account.xtermium.com";

/// The server's address without a trailing slash ("": xtermium's): HTTPS unless it's this computer.
fn base(url: &str) -> Result<String> {
    let fixed = std::env::var("XTERMIUM_SYNC_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let base = url.trim().trim_end_matches('/');
    let base = if base.is_empty() { fixed.trim().trim_end_matches('/') } else { base };
    // By the parsed host: `http://localhost:@evil.example` is evil.example (`localhost:` is a
    // user name there), which a look at the text alone would take for this computer.
    let url = reqwest::Url::parse(base).map_err(|_| anyhow!("That isn't a web address."))?;
    let local = url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if url.scheme() != "https" && !local {
        bail!("The sync server address must start with https://");
    }
    Ok(base.into())
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()?)
}

/// The server's own words for a refusal (`{"error": "…"}`), if it gave any.
fn said(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v["error"].as_str().map(str::to_string).filter(|s| !s.is_empty())
}

/// A refusal the app acts on by itself rather than only showing (docs/sync.md "Status codes").
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trouble {
    /// 401 or 403 from the server itself (a JSON `error`): the token no longer works. Sign in
    /// again. The same statuses from Cloudflare, an HTML page from a bot block or challenge, are
    /// `Later`: the token is fine.
    SignedOut,
    /// 402: uploading needs Pro; the server's sentence, shown with a link to get it.
    NeedsPro { message: String },
    /// 429, 503, Cloudflare's "origin down" or bot block, or no connection at all: try again in
    /// `after` seconds, with backoff.
    Later { after: u64 },
}

impl std::fmt::Display for Trouble {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::SignedOut => write!(f, "Signed out: sign in again to sync."),
            Self::NeedsPro { message } => write!(f, "{message}"),
            Self::Later { .. } => write!(f, "The sync server can't be reached right now."),
        }
    }
}

impl std::error::Error for Trouble {}

/// Sends, and reads the answer up to `MAX`: the status, the body and `Retry-After` in seconds
/// (30 when the server didn't say). Not reaching the server at all is `Trouble::Later`.
async fn fetch(req: reqwest::RequestBuilder) -> Result<(u16, Vec<u8>, u64)> {
    let mut resp = req.send().await.map_err(|_| Trouble::Later { after: 30 })?;
    let status = resp.status().as_u16();
    let after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok()).unwrap_or(30);
    let mut body = vec![];
    while let Some(chunk) = resp.chunk().await.context("sync server")? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX {
            bail!("the sync server's answer is too big");
        }
    }
    Ok((status, body, after))
}

/// The statuses every signed-in call answers the same way, as the `Trouble` they are.
fn trouble(status: u16, after: u64, body: &[u8]) -> Option<Trouble> {
    Some(match status {
        // Cloudflare's bot blocks and challenges are 403 (or 401) too, as an HTML page: the server
        // always refuses in JSON, so only its own word signs the app out.
        401 | 403 if said(body).is_some() => Trouble::SignedOut,
        401 | 403 => Trouble::Later { after },
        402 => Trouble::NeedsPro { message: said(body).unwrap_or_else(|| "Syncing needs Pro.".into()) },
        // 502, 504 and 520–530 are Cloudflare's own answers while the server or its tunnel is down.
        429 | 502..=504 | 520..=530 => Trouble::Later { after },
        _ => return None,
    })
}

/// A JSON POST; `token` makes it a signed-in one.
async fn post(url: String, token: Option<&str>, body: serde_json::Value) -> Result<(u16, Vec<u8>, u64)> {
    let mut req = client()?.post(url).header("Content-Type", "application/json").body(body.to_string());
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    fetch(req).await
}

/// The id the server knows the desktop app by.
const CLIENT_ID: &str = "xtermium-desktop";

/// A sign-in in progress (`POST /auth/device/code`): the person approves `user_code` in the
/// browser at `verification_uri_complete` while the app polls with `device_code`.
#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Starts a sign-in. There is no password: the person signs in at Google, GitHub or Discord in
/// the browser and approves the code there.
pub async fn device_start(url: &str) -> Result<DeviceCode> {
    let (status, body, _) = post(format!("{}/auth/device/code", base(url)?), None, json!({ "client_id": CLIENT_ID })).await?;
    match status {
        200 => serde_json::from_slice(&body).context("the sync server's answer"),
        s => bail!("{}", said(&body).unwrap_or_else(|| format!("sync server: HTTP {s}"))),
    }
}

/// One poll of a sign-in (`POST /auth/device/token`).
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Poll {
    /// Not approved yet: ask again after the interval.
    Pending,
    /// Polling too fast: add 5 s to the interval.
    SlowDown,
    /// Approved: the bearer token for every other call. The code is spent.
    Token { token: String },
}

pub async fn device_poll(url: &str, device_code: &str) -> Result<Poll> {
    let body = json!({ "grant_type": "urn:ietf:params:oauth:grant-type:device_code", "device_code": device_code, "client_id": CLIENT_ID });
    let (status, body, _) = post(format!("{}/auth/device/token", base(url)?), None, body).await?;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    match (status, v["error"].as_str().unwrap_or("")) {
        (200, _) => Ok(Poll::Token { token: v["access_token"].as_str().filter(|t| !t.is_empty()).context("the sync server's answer has no token")?.into() }),
        (400, "authorization_pending") => Ok(Poll::Pending),
        (400, "slow_down") => Ok(Poll::SlowDown),
        (400, "expired_token") => bail!("The code expired: start again."),
        (400, "access_denied") => bail!("The sign-in was refused in the browser."),
        (400, "invalid_grant") => bail!("That code was already used or isn't known: start again."),
        (s, e) => bail!("sync server: HTTP {s} {e}"),
    }
}

/// The signed-in account (`GET /v1/account`) and whether it has Pro, which uploading needs.
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Pro {
    pub active: bool,
    pub plan: Option<String>,
    pub until: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Account {
    pub email: String,
    pub name: Option<String>,
    pub image: Option<String>,
    #[serde(default)]
    pub pro: Pro,
}

pub async fn account(url: &str, token: &str) -> Result<Account> {
    let (status, body, after) = fetch(client()?.get(format!("{}/v1/account", base(url)?)).bearer_auth(token)).await?;
    if let Some(t) = trouble(status, after, &body) {
        return Err(t.into());
    }
    match status {
        200 => serde_json::from_slice(&body).context("the sync server's answer"),
        s => bail!("sync server: HTTP {s}"),
    }
}

/// The address of Creem's customer portal (cancel, change the card, invoices).
pub async fn portal(url: &str, token: &str) -> Result<String> {
    let (status, body, after) = post(format!("{}/v1/account/portal", base(url)?), Some(token), json!({})).await?;
    if let Some(t) = trouble(status, after, &body) {
        return Err(t.into());
    }
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    match (status, v["url"].as_str()) {
        (200, Some(u)) if !u.is_empty() => Ok(u.into()),
        (s, _) => bail!("{}", said(&body).unwrap_or_else(|| format!("sync server: HTTP {s}"))),
    }
}

/// Ends the token's session on the server, or with `everywhere` every session of the account.
/// This device forgets the token either way.
pub async fn sign_out(url: &str, token: &str, everywhere: bool) -> Result<()> {
    let path = if everywhere { "/auth/revoke-sessions" } else { "/v1/account/logout" };
    post(format!("{}{path}", base(url)?), Some(token), json!({})).await?;
    Ok(())
}

impl Api {
    /// `url`: the server's address (empty: xtermium's); `token`: the signed-in account's.
    pub fn new(url: &str, token: &str) -> Result<Self> {
        let base = base(url)?;
        if token.trim().is_empty() {
            bail!("Sign in to sync.");
        }
        Ok(Self { url: format!("{base}/v1/vault"), token: token.trim().into(), http: client()? })
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<(u16, Vec<u8>)> {
        let (status, body, after) = fetch(req.bearer_auth(&self.token)).await?;
        if let Some(t) = trouble(status, after, &body) {
            return Err(t.into());
        }
        match status {
            413 => bail!("{}", said(&body).unwrap_or_else(|| "The sync account's storage is full: nothing more syncs until it shrinks.".into())),
            _ => Ok((status, body)),
        }
    }

    /// None when the account has no vault yet.
    async fn get(&self) -> Result<Option<Remote>> {
        let (status, body) = self.send(self.http.get(&self.url)).await?;
        match status {
            200 => Ok(Some(serde_json::from_slice(&body).context("the sync server's answer")?)),
            404 => Ok(None),
            s => bail!("sync server: HTTP {s}"),
        }
    }

    /// Ok(new revision and the storage left), or Err(current revision) when `base` is out of date.
    async fn put(&self, base: u64, dev: &Device) -> Result<Result<(u64, Option<Quota>), u64>> {
        let body = json!({ "base": base, "files": seal(dev), "recovery": dev.recovery.as_deref().unwrap_or("") });
        let (status, body) = self
            .send(self.http.put(&self.url).header("Content-Type", "application/json").body(body.to_string()))
            .await?;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let revision = v["revision"].as_u64().ok_or_else(|| anyhow!("the sync server's answer has no revision"));
        match status {
            200 => Ok(Ok((revision?, serde_json::from_value(v["quota"].clone()).ok()))),
            409 => Ok(Err(revision?)),
            s => bail!("sync server: HTTP {s}"),
        }
    }

    /// Removes everything the server stores for the account.
    pub async fn delete(&self) -> Result<()> {
        match self.send(self.http.delete(&self.url)).await?.0 {
            200 | 204 | 404 => Ok(()),
            s => bail!("sync server: HTTP {s}"),
        }
    }
}

impl State {
    pub fn load() -> Self {
        crate::config::read("sync", &[], |_| Ok(String::new()))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        crate::config::write("sync", &toml::to_string(self)?)
    }
}

/// This computer's name and OS, as shown to whoever resolves a conflict elsewhere.
pub fn this_device() -> (String, String) {
    let name = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|n| !n.is_empty()))
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|n| n.trim().to_string()))
        .unwrap_or_default();
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    (name, os.to_string())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn digest(store: &Store, known: &Known, recovery: Option<&str>) -> String {
    let mut h = Sha256::new();
    for part in [toml::to_string(store).unwrap_or_default(), toml::to_string(known).unwrap_or_default()] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    h.update(recovery.unwrap_or("").as_bytes());
    vault::hex(&h.finalize())
}

impl Device {
    fn digest(&self) -> String {
        digest(&self.store, &self.known, self.recovery.as_deref())
    }

    fn synced_at(&mut self, revision: u64) {
        self.state = State { revision, synced: self.digest() };
    }

    fn side(&self) -> Side {
        let (device, os) = this_device();
        Side {
            at: self.changed,
            device,
            os,
            sessions: self.store.sessions.len(),
            tunnels: self.store.tunnels.len(),
            keys: self.known.ssh.len() + self.known.rdp.len(),
            ..Default::default()
        }
    }

    /// Something worth keeping: replacing it with another copy would lose it.
    fn has_data(&self) -> bool {
        !(self.store.sessions.is_empty()
            && self.store.tunnels.is_empty()
            && self.known.ssh.is_empty()
            && self.known.rdp.is_empty())
    }
}

fn seal(dev: &Device) -> Files {
    let file = |name, text: String| B64.encode(vault::seal(&dev.key, name, text.as_bytes()));
    let (device, os) = this_device();
    Files {
        sessions: file("sessions", toml::to_string_pretty(&dev.store).unwrap_or_default()),
        known: file("known", toml::to_string(&dev.known).unwrap_or_default()),
        meta: file("meta", toml::to_string(&Meta { device, os, at: now() }).unwrap_or_default()),
    }
}

/// The account's copy, decrypted.
struct Opened {
    store: Store,
    known: Known,
    side: Side,
}

fn open(key: &Key, r: &Remote) -> Result<Opened> {
    let file =
        |name, b64: &str| -> Result<String> { Ok(String::from_utf8(vault::open(key, name, &B64.decode(b64)?)?)?) };
    let store: Store = toml::from_str(&file("sessions", &r.files.sessions)?)?;
    let known: Known = toml::from_str(&file("known", &r.files.known)?)?;
    let meta: Meta = file("meta", &r.files.meta).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
    let side = Side {
        at: r.updated.at.or((meta.at > 0).then_some(meta.at)),
        device: meta.device,
        os: meta.os,
        ip: r.updated.ip.clone(),
        location: r.updated.location.clone(),
        sessions: store.sessions.len(),
        tunnels: store.tunnels.len(),
        keys: known.ssh.len() + known.rdp.len(),
    };
    Ok(Opened { store, known, side })
}

/// Brings this device and the server level. When both changed, returns the conflict and changes
/// nothing, unless `choice` says which copy wins.
pub async fn sync(dev: &mut Device, api: &Api, choice: Option<Choice>) -> Result<Outcome> {
    let mut out = Outcome::default();
    // Another device can put between our get and put: then look again.
    for _ in 0..5 {
        let changed = dev.digest() != dev.state.synced;
        let base = match api.get().await? {
            None => {
                // The first device: its recovery key is how the others will join.
                if dev.recovery.is_none() {
                    bail!("Set a recovery passphrase first (Settings > Security): your other devices need it to join.");
                }
                0
            }
            Some(remote) => {
                out.quota = remote.quota;
                let theirs = open(&dev.key, &remote).map_err(|_| NeedsPassphrase)?;
                if digest(&theirs.store, &theirs.known, Some(&remote.recovery)) == dev.digest() {
                    dev.synced_at(remote.revision);
                    return Ok(out);
                }
                let moved = remote.revision != dev.state.revision;
                if moved && (!changed || choice == Some(Choice::Remote)) {
                    dev.store = theirs.store;
                    dev.known = theirs.known;
                    dev.recovery = Some(remote.recovery);
                    dev.synced_at(remote.revision);
                    out.pulled = true;
                    return Ok(out);
                }
                if moved && choice.is_none() {
                    out.conflict = Some(Conflict { local: dev.side(), remote: theirs.side });
                    return Ok(out);
                }
                remote.revision
            }
        };
        if let Ok((revision, quota)) = api.put(base, dev).await? {
            dev.synced_at(revision);
            out.pushed = true;
            out.quota = quota.or(out.quota);
            return Ok(out);
        }
    }
    bail!("The sync server kept changing during the sync; try again.")
}

/// Joins the account's vault from a device with its own key: the recovery passphrase unseals the
/// account's data key. If this device already has data of its own, the person chooses which copy
/// to keep (the conflict comes back first, with nothing changed). `dev.key` changes: every
/// local file has to be encrypted again under it.
pub async fn join(dev: &mut Device, api: &Api, passphrase: &str, choice: Option<Choice>) -> Result<Outcome> {
    let remote = api.get().await?.ok_or_else(|| anyhow!("There's nothing to join yet: this account has no data."))?;
    let r: vault::Recovery = toml::from_str(&remote.recovery).context("the account's recovery key")?;
    let pass = passphrase.to_string();
    let key = tokio::task::spawn_blocking(move || vault::unwrap(&pass, &r)).await??;
    let theirs = open(&key, &remote)?;
    let differs = digest(&theirs.store, &theirs.known, None) != digest(&dev.store, &dev.known, None);
    if dev.has_data() && differs && choice.is_none() {
        let conflict = Conflict { local: dev.side(), remote: theirs.side };
        return Ok(Outcome { conflict: Some(conflict), ..Default::default() });
    }
    dev.key = key;
    dev.recovery = Some(remote.recovery);
    if dev.has_data() && differs && choice == Some(Choice::Local) {
        // This device's data, now under the account's key, replaces the account's.
        dev.state = State { revision: remote.revision, synced: String::new() };
        return sync(dev, api, Some(Choice::Local)).await;
    }
    dev.store = theirs.store;
    dev.known = theirs.known;
    dev.synced_at(remote.revision);
    Ok(Outcome { pulled: true, quota: remote.quota, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Kind, Session};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncWriteExt, BufReader};

    #[test]
    fn cloudflare_origin_down_is_later() {
        for s in [502, 503, 504, 520, 522, 524, 530] {
            assert!(matches!(trouble(s, 30, b""), Some(Trouble::Later { after: 30 })), "{s}");
        }
        assert!(trouble(500, 30, b"").is_none());
    }

    #[test]
    fn only_the_server_signs_out() {
        let api = br#"{"error": "Sign in to sync."}"#;
        assert_eq!(trouble(401, 30, api), Some(Trouble::SignedOut));
        assert_eq!(trouble(403, 30, api), Some(Trouble::SignedOut));
        // Cloudflare's bot block or challenge: an HTML page, not the server's word. Try later.
        let cf = b"<!DOCTYPE html><html><title>Just a moment...</title></html>";
        assert_eq!(trouble(403, 30, cf), Some(Trouble::Later { after: 30 }));
        assert_eq!(trouble(401, 30, b""), Some(Trouble::Later { after: 30 }));
    }

    /// The API of docs/sync.md, in memory: (revision, files, recovery) or nothing.
    type Vault = Option<(u64, serde_json::Value, String)>;

    async fn server() -> (String, Arc<Mutex<Vault>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let vault: Arc<Mutex<Vault>> = Arc::default();
        let v = vault.clone();
        tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let v = v.clone();
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    while let Some(req) = crate::automation::read_request(&mut r).await {
                        // The sign-in: pending, then "slow down", then the token; the code is spent after that.
                        static POLLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                        let bearer = req.headers.get("authorization").cloned().unwrap_or_default();
                        let signed_in = matches!(bearer.as_str(), "Bearer t" | "Bearer lapsed" | "Bearer down");
                        let mut extra = "";
                        let (status, body) = if req.path == "/auth/device/code" {
                            let b: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
                            assert_eq!(b["client_id"], "xtermium-desktop");
                            let code = json!({ "device_code": "dc", "user_code": "ABCD2345", "verification_uri": "http://x/device",
                                "verification_uri_complete": "http://x/device?user_code=ABCD2345", "expires_in": 600, "interval": 5 });
                            ("200 OK", code.to_string())
                        } else if req.path == "/auth/device/token" {
                            let b: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
                            assert_eq!(b["device_code"], "dc");
                            match POLLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) {
                                0 => ("400 Bad Request", json!({ "error": "authorization_pending" }).to_string()),
                                1 => ("400 Bad Request", json!({ "error": "slow_down" }).to_string()),
                                2 => ("200 OK", json!({ "access_token": "t", "token_type": "Bearer" }).to_string()),
                                _ => ("400 Bad Request", json!({ "error": "invalid_grant" }).to_string()),
                            }
                        } else if !signed_in {
                            ("401 Unauthorized", json!({ "error": "Sign in to sync." }).to_string())
                        } else if bearer == "Bearer down" {
                            extra = "Retry-After: 7\r\n";
                            ("503 Service Unavailable", String::new())
                        } else if req.path == "/v1/account" {
                            let acc = json!({ "email": "a@example.com", "name": "A", "image": null,
                                "pro": { "active": bearer == "Bearer t", "plan": "monthly", "until": 1_770_000_000, "status": "active" } });
                            ("200 OK", acc.to_string())
                        } else if req.path == "/v1/account/portal" {
                            ("404 Not Found", json!({ "error": "No subscription yet." }).to_string())
                        } else if req.path == "/auth/revoke-sessions" {
                            ("200 OK", json!({ "status": true }).to_string())
                        } else if req.path == "/v1/account/logout" {
                            ("204 No Content", String::new())
                        } else if req.method == "PUT" && bearer == "Bearer lapsed" {
                            ("402 Payment Required", json!({ "error": "Pro ended: get it again to keep syncing." }).to_string())
                        } else {
                            let mut v = v.lock().unwrap();
                            match (req.method.as_str(), &*v) {
                                ("GET", None) => ("404 Not Found", String::new()),
                                ("GET", Some((rev, files, rec))) => {
                                    let updated =
                                        json!({ "at": 1_760_000_000, "ip": "203.0.113.7", "location": "Athens, GR" });
                                    let body =
                                        json!({ "revision": rev, "files": files, "recovery": rec, "updated": updated });
                                    ("200 OK", body.to_string())
                                }
                                ("PUT", cur) => {
                                    let b: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                                    let rev = cur.as_ref().map_or(0, |c| c.0);
                                    if b["base"].as_u64() != Some(rev) {
                                        ("409 Conflict", json!({ "revision": rev }).to_string())
                                    } else {
                                        *v =
                                            Some((rev + 1, b["files"].clone(), b["recovery"].as_str().unwrap().into()));
                                        ("200 OK", json!({ "revision": rev + 1 }).to_string())
                                    }
                                }
                                ("DELETE", _) => {
                                    *v = None;
                                    ("204 No Content", String::new())
                                }
                                _ => ("405 Method Not Allowed", String::new()),
                            }
                        };
                        let head = format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
                        if w.write_all(head.as_bytes()).await.is_err() || w.write_all(body.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (url, vault)
    }

    fn device(sessions: &[&str]) -> Device {
        let key = vault::random::<32>();
        let mut store = Store::default();
        for s in sessions {
            store.sessions.push(Session::new(*s, "", Kind::Shell { program: String::new(), dir: String::new() }));
        }
        Device { key, store, known: Known::default(), recovery: None, state: State::default(), changed: Some(42) }
    }

    fn names(d: &Device) -> Vec<&str> {
        let mut v: Vec<&str> = d.store.sessions.iter().map(|s| s.name.as_str()).collect();
        v.sort();
        v
    }

    fn add(d: &mut Device, name: &str) {
        d.store.sessions.push(Session::new(name, "", Kind::Shell { program: String::new(), dir: String::new() }));
    }

    #[tokio::test]
    async fn two_devices() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (url, vault) = server().await;
        let api = Api::new(&url, "t").unwrap();
        // Signing in: a code to approve in the browser, polled until the token comes; the code
        // is then spent. The account says who, and whether uploading is allowed.
        let code = device_start(&url).await.unwrap();
        assert_eq!((code.user_code.as_str(), code.interval, code.expires_in), ("ABCD2345", 5, 600));
        assert!(code.verification_uri_complete.ends_with("?user_code=ABCD2345"));
        assert_eq!(device_poll(&url, &code.device_code).await.unwrap(), Poll::Pending);
        assert_eq!(device_poll(&url, &code.device_code).await.unwrap(), Poll::SlowDown);
        assert_eq!(device_poll(&url, &code.device_code).await.unwrap(), Poll::Token { token: "t".into() });
        assert!(device_poll(&url, &code.device_code).await.unwrap_err().to_string().contains("start again"));
        let acc = account(&url, "t").await.unwrap();
        assert_eq!((acc.email.as_str(), acc.name.as_deref(), acc.pro.active, acc.pro.plan.as_deref()), ("a@example.com", Some("A"), true, Some("monthly")));
        assert!(!account(&url, "lapsed").await.unwrap().pro.active);
        assert_eq!(account(&url, "nope").await.unwrap_err().downcast::<Trouble>().unwrap(), Trouble::SignedOut);
        assert!(portal(&url, "t").await.unwrap_err().to_string().contains("No subscription"));
        sign_out(&url, "t", false).await.unwrap();
        sign_out(&url, "t", true).await.unwrap();
        assert!(base("").unwrap() == DEFAULT_URL && Api::new(&url, " ").is_err());
        assert!(Api::new("http://sync.example.com", "t").is_err(), "plain HTTP to another host");
        assert!(base("http://localhost:@evil.example").is_err(), "a user name that looks local");
        assert!(base("http://127.0.0.1.evil.example:80").is_err() && base("http://localhost:8080/").is_ok());
        let wrong = Api::new(&url, "wrong").unwrap();
        let e = sync(&mut device(&[]), &wrong, None).await.unwrap_err().downcast::<Trouble>().unwrap();
        assert_eq!(e, Trouble::SignedOut);
        // A server that's away says when to try again; an unreachable one, 30 s.
        let down = Api::new(&url, "down").unwrap();
        let e = sync(&mut device(&[]), &down, None).await.unwrap_err().downcast::<Trouble>().unwrap();
        assert_eq!(e, Trouble::Later { after: 7 });
        let gone = Api::new("http://127.0.0.1:1", "t").unwrap();
        let e = sync(&mut device(&[]), &gone, None).await.unwrap_err().downcast::<Trouble>().unwrap();
        assert_eq!(e, Trouble::Later { after: 30 });

        // The first device needs a recovery passphrase, then uploads.
        let mut a = device(&["a1"]);
        assert!(sync(&mut a, &api, None).await.unwrap_err().to_string().contains("recovery passphrase"));
        a.recovery = Some(toml::to_string(&vault::wrap("correct horse battery", &a.key).unwrap()).unwrap());
        let o = sync(&mut a, &api, None).await.unwrap();
        assert!(o.pushed && !o.pulled);
        assert_eq!(vault.lock().unwrap().as_ref().unwrap().0, 1);
        assert!(!sync(&mut a, &api, None).await.unwrap().pushed, "nothing new");

        // Without Pro, downloading still works but uploading is refused with the server's words.
        let lapsed = Api::new(&url, "lapsed").unwrap();
        let mut l = a.clone();
        assert!(!sync(&mut l, &lapsed, None).await.unwrap().pushed, "reading is still allowed");
        add(&mut l, "l1");
        let e = sync(&mut l, &lapsed, None).await.unwrap_err().downcast::<Trouble>().unwrap();
        assert_eq!(e, Trouble::NeedsPro { message: "Pro ended: get it again to keep syncing.".into() });

        // A second device with data of its own can't read the account's until it joins; joining
        // asks which copy to keep, showing who uploaded the account's and from where.
        let mut b = device(&["b1"]);
        let e = sync(&mut b, &api, None).await.unwrap_err();
        assert!(e.downcast_ref::<NeedsPassphrase>().is_some());
        assert!(join(&mut b, &api, "wrong passphrase", None).await.is_err());
        let c = join(&mut b, &api, "correct horse battery", None).await.unwrap().conflict.unwrap();
        assert_eq!((c.local.sessions, c.local.at), (1, Some(42)));
        assert_eq!(
            (c.remote.sessions, c.remote.ip.as_str(), c.remote.location.as_str()),
            (1, "203.0.113.7", "Athens, GR")
        );
        assert_eq!(c.remote.os, this_device().1, "the uploader's OS, sealed in the vault");
        assert_ne!(b.key, a.key, "nothing changes until the choice");
        let o = join(&mut b, &api, "correct horse battery", Some(Choice::Remote)).await.unwrap();
        assert!(o.pulled && !o.pushed);
        assert_eq!((b.key, names(&b)), (a.key, vec!["a1"]));

        // Only one side changed: no question, its copy wins, deletions included.
        add(&mut b, "b2");
        assert!(sync(&mut b, &api, None).await.unwrap().pushed);
        let o = sync(&mut a, &api, None).await.unwrap();
        assert!(o.pulled && o.conflict.is_none());
        assert_eq!(names(&a), ["a1", "b2"]);
        a.store.sessions.retain(|s| s.name != "b2");
        sync(&mut a, &api, None).await.unwrap();
        sync(&mut b, &api, None).await.unwrap();
        assert_eq!(names(&b), ["a1"]);

        // Both changed: asked, nothing changes, then the chosen copy wins whole.
        add(&mut a, "a3");
        add(&mut b, "b3");
        sync(&mut a, &api, None).await.unwrap();
        let before = vault.lock().unwrap().as_ref().unwrap().0;
        let o = sync(&mut b, &api, None).await.unwrap();
        assert!(o.conflict.is_some() && !o.pushed && !o.pulled);
        assert_eq!((names(&b), vault.lock().unwrap().as_ref().unwrap().0), (vec!["a1", "b3"], before));
        assert!(sync(&mut b, &api, Some(Choice::Local)).await.unwrap().pushed);
        assert!(sync(&mut a, &api, None).await.unwrap().pulled, "a hadn't changed since: no question");
        assert_eq!(names(&a), ["a1", "b3"]);

        add(&mut a, "a4");
        add(&mut b, "b4");
        sync(&mut a, &api, None).await.unwrap();
        assert!(sync(&mut b, &api, Some(Choice::Remote)).await.unwrap().pulled);
        assert_eq!(names(&b), ["a1", "a4", "b3"]);

        // A third device joins keeping its own data: it replaces the account's, under the
        // account's key, and the others take it without being asked.
        let mut c = device(&["c1"]);
        let o = join(&mut c, &api, "correct horse battery", Some(Choice::Local)).await.unwrap();
        assert!(o.pushed && c.key == a.key);
        assert!(sync(&mut b, &api, None).await.unwrap().pulled);
        assert_eq!(names(&b), ["c1"]);

        // A put on an out-of-date base is refused with the current revision.
        let rev = vault.lock().unwrap().as_ref().unwrap().0;
        assert_eq!(api.put(rev - 1, &b).await.unwrap(), Err(rev));
        assert_eq!(vault.lock().unwrap().as_ref().unwrap().0, rev);

        // Delete the cloud copy: the next sync starts it again.
        api.delete().await.unwrap();
        assert!(vault.lock().unwrap().is_none());
        assert!(sync(&mut b, &api, None).await.unwrap().pushed);
    }
}
