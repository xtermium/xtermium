//! Sessions, folders and tunnels (`sessions.xtm`), trusted server keys (`known.xtm`), and the
//! encrypted storage they and the settings live in.

use crate::vault::{self, Key};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// A list of sessions without the ones this version can't read (a kind it no longer has, like
/// the asciinema replays it once had): one of those mustn't cost the whole file.
fn readable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Session>, D::Error> {
    Ok(Vec::<serde_json::Value>::deserialize(d)?.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Store {
    #[serde(default, rename = "session", deserialize_with = "readable")]
    pub sessions: Vec<Session>,
    #[serde(default, rename = "tunnel")]
    pub tunnels: Vec<Tunnel>,
    /// Folders with something to remember (tags). Folders also exist implicitly through sessions.
    #[serde(default, rename = "folder")]
    pub folders: Vec<Folder>,
    /// Saved commands to type into a terminal (Snippets).
    #[serde(default, rename = "snippet")]
    pub snippets: Vec<Snippet>,
    /// Logins sessions can share (Credentials): change the password once, for all of them.
    #[serde(default, rename = "credential")]
    pub credentials: Vec<Credential>,
    /// Saved layouts (the tab bar's Layouts menu): sessions opened together, one per pane.
    #[serde(default, rename = "layout", skip_serializing_if = "Vec::is_empty")]
    pub layouts: Vec<Layout>,
    /// The other workspaces (Settings > Workspaces), each with sessions and folders of its own;
    /// the sessions and folders above are the default one's. Everything else here (shared logins,
    /// tunnels, snippets, layouts) is every workspace's. Synced and backed up with it.
    #[serde(default, rename = "workspace", skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<Workspace>,
}

/// Puts each of `from` into `to`, in place of the one `same` says it is, else at the end.
fn upsert<T: Clone>(to: &mut Vec<T>, from: &[T], same: impl Fn(&T, &T) -> bool) {
    for x in from {
        match to.iter_mut().find(|t| same(t, x)) {
            Some(t) => *t = x.clone(),
            None => to.push(x.clone()),
        }
    }
}

/// Moves each of `from` into `to` (`Store::hoist`): left out when the same is there, kept as
/// "<name> (<ws>)" when one of its name differs.
fn hoist_named<T: Serialize>(to: &mut Vec<T>, from: Vec<T>, ws: &str, name: fn(&mut T) -> &mut String) {
    for mut x in from {
        let n = name(&mut x).clone();
        let there = to.iter_mut().position(|e| *name(e) == n).map(|i| serde_json::to_value(&to[i]).ok());
        match there {
            Some(v) if v == serde_json::to_value(&x).ok() => {}
            Some(_) => {
                *name(&mut x) = format!("{n} ({ws})");
                to.push(x);
            }
            None => to.push(x),
        }
    }
}

/// A saved layout: its pane arrangement (ui/App.svelte `Layout`: single, columns, rows, grid,
/// tiles), the sessions on screen in tab-strip order with the pane each was in and the name its
/// tab was given, and the panes' sizes. Opening one takes the saved session of the same folder
/// and name if there is one, so later edits apply; else this copy.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Layout {
    pub name: String,
    #[serde(default)]
    pub arrangement: String,
    #[serde(default, rename = "pane", deserialize_with = "readable")]
    pub panes: Vec<Session>,
    /// The pane each of `panes` was in; empty (older layouts): one after another.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub at: Vec<u32>,
    /// The name each of `panes`' tabs was given in the tab strip; "" for none.
    #[serde(default, rename = "label", skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The panes' sizes as they were dragged; None: even.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Sizes>,
}

/// Pane sizes (ui/lib/actions.ts `Sizes`): each row's share of the height, and each pane's share
/// of its row's width, as fractions.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Sizes {
    pub rows: Vec<f64>,
    pub cols: Vec<Vec<f64>>,
}

/// A workspace besides the default one: its sessions and folders (the rest of `data` stays
/// empty: that is every workspace's, at the top of the store).
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Workspace {
    pub name: String,
    #[serde(default)]
    pub data: Store,
}

/// A login kept once and used by many sessions (an endpoint's or RDP/VNC/website session's
/// `credential` is its id). Stored encrypted with the sessions, and synced with them.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Credential {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// Private key path (SSH).
    #[serde(default)]
    pub key: String,
    /// Sessions using this login connect through this SSH server: as the jump host of an SSH
    /// session without one of its own, a forwarded port for RDP and VNC, a SOCKS proxy for websites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<Gateway>,
}

/// An SSH server to connect through (Credential::gateway).
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Gateway {
    pub host: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// Private key path. Empty = agent / default keys / password.
    #[serde(default)]
    pub key: String,
}

impl Gateway {
    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            key: self.key.clone(),
            credential: String::new(),
            password: Some(self.password.clone()).filter(|p| !p.is_empty()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Snippet {
    pub name: String,
    /// Typed as it is; `{{name}}` placeholders are asked for first. Line breaks press Enter.
    pub text: String,
    /// Press Enter after the text.
    #[serde(default = "yes")]
    pub run: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Folder {
    /// `/`-separated, like `Session::folder`.
    pub path: String,
    /// Search terms for the folder and everything in it.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Sidebar colour (`#rrggbb`), inherited by sessions without their own; empty = none.
    #[serde(default)]
    pub color: String,
    /// Sidebar icon: a built-in icon name or a `data:` image; empty = the folder icon.
    #[serde(default)]
    pub icon: String,
    /// A built-in icon stays still on hover instead of moving.
    #[serde(default, skip_serializing_if = "is_false")]
    pub still: bool,
    /// Place among its sibling folders, from 1, once dragged into one; 0 = by name, after those.
    #[serde(default)]
    pub order: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Session {
    pub name: String,
    /// Folder path in the tree, `/`-separated. Empty = root.
    #[serde(default)]
    pub folder: String,
    pub kind: Kind,
    #[serde(default)]
    pub style: Style,
    /// Sidebar colour (`#rrggbb`); empty = the folder's, else the kind's.
    #[serde(default)]
    pub color: String,
    /// Sidebar icon: a built-in icon name or a `data:` image; empty = the kind's icon.
    #[serde(default)]
    pub icon: String,
    /// A built-in icon stays still on hover instead of moving.
    #[serde(default, skip_serializing_if = "is_false")]
    pub still: bool,
    #[serde(default)]
    pub notes: String,
}

impl Session {
    pub fn new(name: impl Into<String>, folder: impl Into<String>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            folder: folder.into(),
            kind,
            style: Style::default(),
            color: String::new(),
            icon: String::new(),
            still: false,
            notes: String::new(),
        }
    }
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
// Sessions number in the hundreds at most: an unboxed SSH variant costs nothing worth boxing for.
#[allow(clippy::large_enum_variant)]
pub enum Kind {
    Ssh(Ssh),
    Shell {
        #[serde(default)]
        program: String,
        /// The folder it starts in, "" for the home folder ("Open in xtermium" from a folder).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        dir: String,
    },
    Serial {
        port: String,
        baud: u32,
    },
    Rdp {
        host: String,
        #[serde(default = "rdp_port")]
        port: u16,
        #[serde(default)]
        user: String,
        /// Picture quality against speed (0 quality, 1 balanced, 2 speed); None: the global
        /// setting (Settings > Defaults).
        #[serde(default)]
        quality: Option<u8>,
        /// A shared login's id: its user and password are used.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        credential: String,
        /// The file panel logs in to the host's shares (SMB) as the desktop's login, without
        /// asking; None: the global setting (Settings > Defaults).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        smb_login: Option<bool>,
        /// The file panel doesn't open the host's shared folders for this session at all.
        #[serde(default, skip_serializing_if = "is_false")]
        no_shares: bool,
    },
    Browser {
        url: String,
        /// A shared login's id: its SSH gateway, if it has one, carries the pages.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        credential: String,
        /// Its own browser profile (web.rs), where its cookies and sign-ins stay between runs:
        /// random hex, given when it's stored (clean.rs `profiles`).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        profile: String,
    },
    Telnet {
        host: String,
        #[serde(default = "telnet_port")]
        port: u16,
    },
    /// PowerShell remoting: the local PowerShell enters a session on the host over WinRM (term.rs
    /// `run_winrm`), with the password xtermium has or asks for. No `user`: the Windows login (domain).
    Winrm {
        host: String,
        #[serde(default = "winrm_port")]
        port: u16,
        #[serde(default)]
        user: String,
        /// WinRM over HTTPS (5986) instead of HTTP (5985).
        #[serde(default)]
        https: bool,
        /// HTTPS without checking the server's certificate: self-signed outside a domain.
        #[serde(default)]
        insecure: bool,
        /// A shared login's id: its user and password are used.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        credential: String,
        /// The shared login's password, filled in to connect (Store::resolve); never stored.
        #[serde(skip)]
        password: Option<String>,
    },
    /// An AI coding agent's command line, run locally in `dir` (empty: the home folder):
    /// `tool` is one of `AGENTS` (run with the flags for `mode`, `effort` and `model`, then
    /// `args`), or "custom" (`args` is the whole command).
    Agent {
        tool: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        dir: String,
        /// How much it asks, one of `AGENT_MODES`; "" leaves the tool's default.
        #[serde(default)]
        mode: String,
        /// Reasoning effort, one of `AGENT_EFFORTS`; "" leaves the tool's default.
        #[serde(default)]
        effort: String,
        /// The model, as the tool names it; "" leaves its default.
        #[serde(default)]
        model: String,
    },
    Vnc {
        host: String,
        #[serde(default = "vnc_port")]
        port: u16,
        /// A shared login's id: its password is used.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        credential: String,
    },
    /// A server's BMC console: the BMC's own HTML5 console in a website view (web.rs), signed in
    /// with the shared login. `vendor`: whose BMC (`KVM_VENDORS`), which decides how it signs in and
    /// where its console is (kvm.rs).
    Kvm {
        #[serde(default = "supermicro")]
        vendor: String,
        host: String,
        #[serde(default = "https_port")]
        port: u16,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        credential: String,
    },
}

fn https_port() -> u16 {
    443
}

/// The BMCs a console session can be for (ui/lib/api.ts `KVM_VENDORS`).
pub const KVM_VENDORS: &[&str] = &["supermicro", "idrac"];

fn supermicro() -> String {
    KVM_VENDORS[0].into()
}

/// The agent tools (ui/lib/api.ts `AGENTS`); `Kind::agent_program` names what each one runs.
pub const AGENTS: &[&str] = &["claude", "codex", "opencode", "copilot", "cursor"];
/// How much an agent asks: before edits and commands, before commands only, or never.
pub const AGENT_MODES: &[&str] = &["ask", "edit", "auto"];
/// Reasoning effort levels, as Claude Code and Codex spell them.
pub const AGENT_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// `f value` onto a command line, unless the value is empty.
fn flag(v: &mut Vec<String>, f: &str, value: &str) {
    if !value.is_empty() {
        v.extend([f.to_string(), value.to_string()]);
    }
}

impl Kind {
    /// The program an agent tool is started as.
    pub fn agent_program(tool: &str) -> &str {
        if tool == "cursor" { "agent" } else { tool }
    }

    /// What an agent session runs, word by word: the program, the tool's own spelling of the mode,
    /// effort and model (left out where it has none), then the user's arguments, last so they win.
    /// "custom" runs `args` as it is.
    pub fn agent_argv(tool: &str, args: &str, mode: &str, effort: &str, model: &str) -> Vec<String> {
        let mut v: Vec<String> = vec![];
        if tool != "custom" {
            v.push(Self::agent_program(tool).into());
            match tool {
                "claude" => {
                    flag(&mut v, "--permission-mode", match mode { "ask" => "default", "edit" => "acceptEdits", "auto" => "auto", _ => "" });
                    flag(&mut v, "--effort", effort);
                    flag(&mut v, "--model", model);
                }
                "codex" => {
                    // A read-only sandbox asks before every write; workspace-write asks only at its edges.
                    flag(&mut v, "--ask-for-approval", match mode { "ask" | "edit" => "on-request", "auto" => "never", _ => "" });
                    flag(&mut v, "--sandbox", match mode { "ask" => "read-only", "edit" | "auto" => "workspace-write", _ => "" });
                    if !effort.is_empty() {
                        flag(&mut v, "-c", &format!("model_reasoning_effort=\"{effort}\""));
                    }
                    flag(&mut v, "--model", model);
                }
                "copilot" => {
                    if mode == "edit" {
                        flag(&mut v, "--allow-tool", "write");
                    } else if mode == "auto" {
                        v.push("--allow-all-tools".into());
                    }
                    flag(&mut v, "--model", model);
                }
                "cursor" => {
                    if mode == "ask" {
                        flag(&mut v, "--mode", "ask");
                    } else if mode == "auto" {
                        v.push("--force".into());
                    }
                    flag(&mut v, "--model", model);
                }
                "opencode" => {
                    if mode == "auto" {
                        v.push("--auto".into());
                    }
                    flag(&mut v, "--model", model);
                }
                _ => {}
            }
        }
        v.extend(args.split_whitespace().map(String::from));
        v
    }

    /// The same on one line, for a tab's label.
    pub fn agent_command(tool: &str, args: &str, mode: &str, effort: &str, model: &str) -> String {
        Self::agent_argv(tool, args, mode, effort, model).join(" ")
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Ssh {
    #[serde(flatten)]
    pub target: Endpoint,
    #[serde(default)]
    pub jump: Option<Endpoint>,
    /// Typed into the shell right after login.
    #[serde(default)]
    pub command: String,
    /// Ciphers to offer, best first; empty: Settings > Defaults.
    #[serde(default)]
    pub ciphers: Vec<String>,
    /// Seconds between keepalives, 0 for none; None: Settings > Defaults.
    #[serde(default)]
    pub keepalive: Option<u32>,
    /// zlib compression; None: Settings > Defaults.
    #[serde(default)]
    pub compression: Option<bool>,
    /// Let the server use this computer's SSH agent (ssh -A): for git or hopping on from it.
    #[serde(default)]
    pub agent_forward: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Endpoint {
    pub host: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    /// Private key path. Empty = agent / default keys / password.
    #[serde(default)]
    pub key: String,
    /// A shared login's id (Store::credentials): its user, key and password are used instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub credential: String,
    /// The shared login's password, filled in to connect (Store::resolve); never stored.
    #[serde(skip)]
    pub password: Option<String>,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            user: String::new(),
            key: String::new(),
            credential: String::new(),
            password: None,
        }
    }
}

impl Endpoint {
    pub fn label(&self) -> String {
        let user = if self.user.is_empty() { String::new() } else { format!("{}@", self.user) };
        format!("{user}{}:{}", self.host, self.port)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Style {
    /// System font family; empty = built-in monospace.
    pub font: String,
    pub font_size: f32,
    pub term: String,
    pub fg: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
    pub cursor: Option<[u8; 3]>,
    /// ANSI 0-15.
    pub palette: Option<Vec<[u8; 3]>>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font: String::new(),
            font_size: 14.0,
            term: "xterm-256color".into(),
            fg: None,
            bg: None,
            cursor: None,
            palette: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum TunnelKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Tunnel {
    pub name: String,
    pub kind: TunnelKind,
    pub ssh: Endpoint,
    #[serde(default = "localhost")]
    pub bind: String,
    pub listen_port: u16,
    /// Unused for Dynamic.
    #[serde(default)]
    pub dest_host: String,
    #[serde(default)]
    pub dest_port: u16,
    /// Started when xtermium opens.
    #[serde(default)]
    pub autostart: bool,
}

fn ssh_port() -> u16 {
    22
}
fn rdp_port() -> u16 {
    3389
}
fn telnet_port() -> u16 {
    23
}
fn winrm_port() -> u16 {
    5985
}
fn vnc_port() -> u16 {
    5900
}
fn localhost() -> String {
    "127.0.0.1".into()
}

/// The data folder, which holds everything xtermium keeps: `XTERMIUM_HOME` (portable installs),
/// or `~/.xtermium`. What older versions kept in the OS config folder moves there on first use.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XTERMIUM_HOME") {
        return d.into();
    }
    if cfg!(test) {
        // Tests never touch the real data folder (ssh.rs `test_home` uses this one too).
        return std::env::temp_dir().join(format!("xtermium-test-{}", std::process::id()));
    }
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = directories::BaseDirs::new().map_or_else(|| PathBuf::from(".xtermium"), |d| d.home_dir().join(".xtermium"));
        match directories::ProjectDirs::from("", "", "xtermium") {
            Some(old) => moved(old.config_dir(), home),
            None => home,
        }
    })
    .clone()
}

/// `new`, with the data in `old` moved there (and its key's keychain entry, named after the
/// folder) when only `old` has any. `old` while it can't move, to try again at the next start:
/// while an older xtermium runs on it (it holds instance.rs's lock) or a browser holds its files.
/// ponytail: a move to another drive (a roaming AppData on a server) fails every time and the
/// data stays where it was; copy and delete if that comes up.
fn moved(old: &Path, new: PathBuf) -> PathBuf {
    if new.exists() || !old.exists() {
        return new;
    }
    let running = File::options().write(true).open(old.join("instance.lock")).is_ok_and(|f| f.try_lock().is_err());
    let key = keychain_of(old).and_then(|e| e.get_password().ok());
    if running || std::fs::rename(old, &new).is_err() {
        return old.to_path_buf();
    }
    if let (Some(k), Some(e)) = (key, keychain_of(&new))
        && e.set_password(&k).is_ok()
    {
        let _ = keychain_of(old).map(|e| e.delete_credential());
    }
    // `%APPDATA%\xtermium`, left empty (elsewhere the parent is a shared folder like ~/.config).
    #[cfg(windows)]
    let _ = old.parent().map(std::fs::remove_dir);
    new
}

/// The windows' browser data (WebView2, WebKitGTK; macOS keeps it in its own place), shared by
/// website views that have no profile of their own (web.rs).
pub fn webview_dir() -> PathBuf {
    dir().join("webview")
}

// ---- this run's passing data: <data folder>/temp/<process id> ----
// Browser profiles of gateway and console tabs (web.rs), terminal images spilled to disk (main.rs)
// and agents' MCP settings (automation.rs). Each xtermium holds `.lock` in its own folder while
// it runs; a folder whose lock is free belongs to one that ended (or crashed), and goes when the
// next one starts.

static TEMP: OnceLock<Option<(PathBuf, File)>> = OnceLock::new();

/// This run's folder for passing data, made on first use.
pub fn temp() -> Option<&'static Path> {
    let made = TEMP.get_or_init(|| {
        let dir = dir().join("temp").join(std::process::id().to_string());
        std::fs::create_dir_all(&dir).ok()?;
        let lock = File::create(dir.join(".lock")).ok()?;
        lock.try_lock().ok()?;
        Some((dir, lock))
    });
    made.as_ref().map(|(dir, _)| dir.as_path())
}

/// Removes the passing data of runs that have ended. At start, off the main thread (main.rs):
/// a crash leaves whole browser profiles behind.
pub fn sweep_temp() {
    if let Some(base) = temp().and_then(Path::parent) {
        sweep(base);
    }
}

fn sweep(base: &Path) {
    for dir in std::fs::read_dir(base).into_iter().flatten().flatten().map(|e| e.path()) {
        // No lock file: a run that never got going.
        let ended = File::open(dir.join(".lock")).map_or(true, |f| f.try_lock().is_ok());
        if ended {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

// ---- the data key and encrypted files ----

/// Everything stored is encrypted with this key (see vault.rs). It lives in the OS keychain;
/// a recovery passphrase can bring it back on another device or after a reinstall.
static KEY: Mutex<Option<Key>> = Mutex::new(None);

/// Encrypted files: `<name>.xtm` in the config dir.
const FILES: [&str; 11] =
    ["sessions", "settings", "known", "tabs", "sync", "logins", "background", "background-sidebar", "background-bar", "automation-cert", "lock"];
const RECOVERY: &str = "recovery.key";

pub enum KeyState {
    Ready,
    /// Files are encrypted but the key isn't in the keychain: unlock with the recovery passphrase.
    Locked {
        recoverable: bool,
    },
}

/// One keychain entry per data folder, so portable installs (`XTERMIUM_HOME`) keep their own key.
fn keychain() -> Option<keyring::Entry> {
    keychain_of(&dir())
}

fn keychain_of(dir: &Path) -> Option<keyring::Entry> {
    use sha2::{Digest, Sha256};
    let id = vault::hex(&Sha256::digest(dir.to_string_lossy().as_bytes())[..8]);
    keyring::Entry::new("xtermium", &format!("data-key-{id}")).ok()
}

fn path(name: &str) -> PathBuf {
    dir().join(format!("{name}.xtm"))
}

pub fn set_key(k: Key) {
    *KEY.lock().unwrap() = Some(k);
}

pub fn key() -> Result<Key> {
    KEY.lock().unwrap().ok_or_else(|| anyhow::anyhow!("sessions are locked"))
}

/// Loads the key from the keychain, or makes one on first run. Returns a note for the user when
/// the keychain can't hold it.
pub fn init_key() -> (KeyState, String) {
    if let Some(k) = keychain().and_then(|e| e.get_password().ok()).and_then(|h| vault::unhex(&h).ok())
        && let Ok(k) = Key::try_from(k)
    {
        set_key(k);
        return (KeyState::Ready, String::new());
    }
    if FILES.iter().any(|f| path(f).exists()) {
        return (KeyState::Locked { recoverable: has_recovery() }, String::new());
    }
    let k = vault::random::<32>();
    set_key(k);
    match keychain().map(|e| e.set_password(&vault::hex(&k))) {
        Some(Ok(())) => (KeyState::Ready, String::new()),
        _ => (
            KeyState::Ready,
            "The OS keychain isn't available. Set a recovery passphrase in Settings > Security: you'll need it to open your sessions next time.".into(),
        ),
    }
}

pub fn has_recovery() -> bool {
    dir().join(RECOVERY).exists()
}

/// Brings the key back from the recovery passphrase and puts it in the keychain.
pub fn unlock(passphrase: &str) -> Result<()> {
    let r: vault::Recovery = toml::from_str(&std::fs::read_to_string(dir().join(RECOVERY))?)?;
    let k = vault::unwrap(passphrase, &r)?;
    set_key(k);
    if let Some(e) = keychain() {
        let _ = e.set_password(&vault::hex(&k));
    }
    Ok(())
}

/// When the synced files (sessions, trusted keys) last changed, in Unix seconds.
pub fn modified() -> Option<u64> {
    ["sessions", "known"]
        .iter()
        .filter_map(|f| std::fs::metadata(path(f)).and_then(|m| m.modified()).ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .max()
}

/// `recovery.key` as it is on disk (sync uploads it).
pub fn recovery_text() -> Option<String> {
    std::fs::read_to_string(dir().join(RECOVERY)).ok()
}

pub fn set_recovery_text(text: &str) -> Result<()> {
    write_plain(&dir().join(RECOVERY), text.as_bytes())
}

/// Makes `new` the data key, encrypting every stored file again under it (joining a synced
/// account, whose key the other devices use).
///
/// ponytail: files are rewritten one by one; a crash halfway leaves some under each key, and
/// the recovery passphrase only opens the new ones. Write them all aside first if that matters.
pub fn rekey(new: Key) -> Result<()> {
    let old = key()?;
    let mut plain = vec![];
    for f in FILES.iter().filter(|f| path(f).exists()) {
        plain.push((*f, vault::open(&old, f, &std::fs::read(path(f))?)?));
    }
    set_key(new);
    for (f, p) in plain {
        write_plain(&path(f), &vault::seal(&new, f, &p))?;
    }
    crate::snapshot::rekey(&old, &new)?;
    if let Some(e) = keychain() {
        let _ = e.set_password(&vault::hex(&new));
    }
    Ok(())
}

/// Sets the recovery passphrase. Changing one there is takes `current`, so whoever sits at an
/// unlocked computer can't swap in a passphrase of their own.
pub fn set_recovery(passphrase: &str, current: &str) -> Result<()> {
    if let Some(text) = recovery_text() {
        let r: vault::Recovery = toml::from_str(&text)?;
        if vault::unwrap(current, &r).is_err() {
            anyhow::bail!("The current passphrase is wrong.");
        }
    }
    let r = vault::wrap(passphrase, &key()?)?;
    write_plain(&dir().join(RECOVERY), toml::to_string_pretty(&r)?.as_bytes())
}

/// Locked with no way back: set the unreadable files aside and start over with a new key.
pub fn start_over() -> Result<()> {
    for f in FILES {
        if path(f).exists() {
            std::fs::rename(path(f), dir().join(format!("{f}.xtm.unreadable")))?;
        }
    }
    let _ = std::fs::remove_file(dir().join(RECOVERY));
    let k = vault::random::<32>();
    set_key(k);
    if let Some(e) = keychain() {
        let _ = e.set_password(&vault::hex(&k));
    }
    Ok(())
}

pub fn write_plain(path: &Path, data: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    // Write-then-rename so a crash never leaves a truncated file. On disk before the rename: after
    // a power cut the file system may otherwise keep the rename but not the data, an empty file.
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    std::io::Write::write_all(&mut f, data)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn write(name: &str, text: &str) -> Result<()> {
    write_plain(&path(name), &vault::seal(&key()?, name, text.as_bytes()))
}

/// The decrypted contents of `<name>.xtm`. On first run after an upgrade, the old plain-text
/// files are read instead, encrypted, checked, and removed.
pub fn read(name: &str, plain: &[&str], convert: impl FnOnce(Vec<String>) -> Result<String>) -> Result<String> {
    if path(name).exists() {
        let data = std::fs::read(path(name))?;
        return Ok(String::from_utf8(vault::open(&key()?, name, &data)?)?);
    }
    let old: Vec<PathBuf> = plain.iter().map(|p| dir().join(p)).collect();
    if !old.iter().any(|p| p.exists()) {
        return Ok(String::new());
    }
    let text = convert(old.iter().map(|p| std::fs::read_to_string(p).unwrap_or_default()).collect())?;
    write(name, &text)?;
    let back = vault::open(&key()?, name, &std::fs::read(path(name))?)?;
    if back != text.as_bytes() {
        anyhow::bail!("could not verify the encrypted {name} file; plain-text copy kept");
    }
    for p in old.iter().filter(|p| p.exists()) {
        std::fs::remove_file(p)?;
    }
    Ok(text)
}

impl Store {
    pub fn load() -> Result<Self> {
        Ok(toml::from_str(&read("sessions", &["sessions.toml"], |t| Ok(t.concat()))?)?)
    }

    pub fn save(&self) -> Result<()> {
        write("sessions", &toml::to_string_pretty(self)?)
    }

    /// Adds sessions, tunnels and folder tags that aren't there yet. Returns (sessions, tunnels) added.
    pub fn merge(&mut self, other: Store) -> (usize, usize) {
        let before = (self.sessions.len(), self.tunnels.len());
        for s in other.sessions {
            if !self.sessions.iter().any(|e| e.name == s.name && e.folder == s.folder) {
                self.sessions.push(s);
            }
        }
        for t in other.tunnels {
            if !self.tunnels.iter().any(|e| e.name == t.name) {
                self.tunnels.push(t);
            }
        }
        for c in other.credentials {
            if !self.credentials.iter().any(|e| e.id == c.id) {
                self.credentials.push(c);
            }
        }
        for s in other.snippets {
            if !self.snippets.iter().any(|e| e.name == s.name) {
                self.snippets.push(s);
            }
        }
        for l in other.layouts {
            if !self.layouts.iter().any(|e| e.name == l.name) {
                self.layouts.push(l);
            }
        }
        for f in other.folders {
            match self.folders.iter_mut().find(|e| e.path == f.path) {
                Some(e) => {
                    e.tags.extend(f.tags.into_iter().filter(|t| !e.tags.contains(t)).collect::<Vec<_>>());
                    if e.color.is_empty() {
                        e.color = f.color;
                    }
                    if e.icon.is_empty() {
                        (e.icon, e.still) = (f.icon, f.still);
                    }
                    if e.order == 0 {
                        e.order = f.order;
                    }
                }
                None => self.folders.push(f),
            }
        }
        (self.sessions.len() - before.0, self.tunnels.len() - before.1)
    }

    /// The sessions and folders in `folder` and below; everything, tunnels included, for "".
    pub fn credential(&self, id: &str) -> Option<&Credential> {
        self.credentials.iter().find(|c| !id.is_empty() && c.id == id)
    }

    /// The SSH gateway of an endpoint's shared login: the way in to it (tunnels).
    pub fn endpoint_gateway(&self, e: &Endpoint) -> Option<Endpoint> {
        self.credential(&e.credential)?.gateway.as_ref().map(Gateway::endpoint)
    }

    /// An endpoint as it connects: a shared login's user, key and password filled in.
    pub fn resolve_endpoint(&self, e: &mut Endpoint) {
        if let Some(c) = self.credential(&e.credential) {
            e.user = c.user.clone();
            if !c.key.is_empty() {
                e.key = c.key.clone();
            }
            e.password = Some(c.password.clone()).filter(|p| !p.is_empty());
        }
    }

    /// A session as it connects: shared logins filled in. Returns the password an RDP or VNC
    /// session gets from one (SSH endpoints and WinRM sessions carry theirs).
    pub fn resolve(&self, kind: &mut Kind) -> Option<String> {
        match kind {
            Kind::Ssh(s) => {
                // The login's gateway is the jump host, unless the session names its own.
                if s.jump.is_none() {
                    s.jump = self.credential(&s.target.credential).and_then(|c| c.gateway.as_ref()).map(Gateway::endpoint);
                }
                self.resolve_endpoint(&mut s.target);
                if let Some(j) = &mut s.jump {
                    self.resolve_endpoint(j);
                }
                None
            }
            Kind::Rdp { user, credential, .. } => {
                let c = self.credential(credential)?;
                *user = c.user.clone();
                Some(c.password.clone()).filter(|p| !p.is_empty())
            }
            Kind::Vnc { credential, .. } => self.credential(credential).map(|c| c.password.clone()).filter(|p| !p.is_empty()),
            Kind::Winrm { user, credential, password, .. } => {
                let c = self.credential(credential)?;
                *user = c.user.clone();
                *password = Some(c.password.clone()).filter(|p| !p.is_empty());
                None
            }
            _ => None,
        }
    }

    /// The SSH gateway an RDP, VNC or website session reaches its host through (its login's).
    pub fn gateway(&self, kind: &Kind) -> Option<Endpoint> {
        let (Kind::Rdp { credential, .. }
        | Kind::Vnc { credential, .. }
        | Kind::Browser { credential, .. }
        | Kind::Kvm { credential, .. }) = kind
        else {
            return None;
        };
        self.credential(credential)?.gateway.as_ref().map(Gateway::endpoint)
    }

    /// Workspace `name` ("" the default one) taken out of the whole store: its sessions and
    /// folders with what every workspace has (shared logins, tunnels, snippets, layouts), and the
    /// rest with an empty place where it was. `join` puts them back together.
    pub fn split(mut self, name: &str) -> (Store, Store) {
        self.hoist();
        let workspaces = std::mem::take(&mut self.workspaces);
        if name.is_empty() {
            return (self, Store { workspaces, ..Default::default() });
        }
        let mut rest = Store { workspaces, ..self };
        let mut data = rest.workspaces.iter_mut().find(|w| w.name == name).map(|w| std::mem::take(&mut w.data)).unwrap_or_default();
        data.credentials = rest.credentials.clone();
        data.tunnels = rest.tunnels.clone();
        data.snippets = rest.snippets.clone();
        data.layouts = rest.layouts.clone();
        (data, rest)
    }

    /// The rest of a `split` with `data` back in workspace `name`'s place (made if missing): its
    /// sessions and folders there, what every workspace has at the top.
    pub fn join(&self, name: &str, data: &Store) -> Store {
        let mut data = Store { workspaces: vec![], ..data.clone() };
        if name.is_empty() {
            data.workspaces = self.workspaces.clone();
            return data;
        }
        let mut all = self.clone();
        all.credentials = std::mem::take(&mut data.credentials);
        all.tunnels = std::mem::take(&mut data.tunnels);
        all.snippets = std::mem::take(&mut data.snippets);
        all.layouts = std::mem::take(&mut data.layouts);
        match all.workspaces.iter_mut().find(|w| w.name == name) {
            Some(w) => w.data = data,
            None => all.workspaces.push(Workspace { name: name.into(), data }),
        }
        all
    }

    /// Workspaces keep only their sessions and folders apart. Shared logins, tunnels, snippets and
    /// layouts a workspace kept of its own (as each one did before) move to the top, for all: the
    /// same ones merge, and one differing from another of its name (a login: of its id) is kept
    /// beside it as "<name> (<workspace>)".
    fn hoist(&mut self) {
        for w in &mut self.workspaces {
            let d = &mut w.data;
            for mut c in std::mem::take(&mut d.credentials) {
                match self.credentials.iter().find(|e| e.id == c.id) {
                    Some(e) if *e == c => continue,
                    Some(_) => {
                        let id = (2..).map(|n| format!("{}{n}", c.id)).find(|id| !self.credentials.iter().any(|e| e.id == *id)).unwrap();
                        d.relogin(&c.id, &id);
                        (c.id, c.name) = (id, format!("{} ({})", c.name, w.name));
                    }
                    None => {}
                }
                self.credentials.push(c);
            }
            hoist_named(&mut self.tunnels, std::mem::take(&mut d.tunnels), &w.name, |t| &mut t.name);
            hoist_named(&mut self.snippets, std::mem::take(&mut d.snippets), &w.name, |s| &mut s.name);
            hoist_named(&mut self.layouts, std::mem::take(&mut d.layouts), &w.name, |l| &mut l.name);
        }
    }

    /// Sessions, tunnels and layouts using shared login `from` use `to` instead.
    fn relogin(&mut self, from: &str, to: &str) {
        let swap = |id: &mut String| {
            if id == from {
                *id = to.into();
            }
        };
        for s in self.sessions.iter_mut().chain(self.layouts.iter_mut().flat_map(|l| &mut l.panes)) {
            match &mut s.kind {
                Kind::Ssh(ssh) => {
                    swap(&mut ssh.target.credential);
                    if let Some(j) = &mut ssh.jump {
                        swap(&mut j.credential);
                    }
                }
                Kind::Rdp { credential, .. }
                | Kind::Vnc { credential, .. }
                | Kind::Browser { credential, .. }
                | Kind::Kvm { credential, .. }
                | Kind::Winrm { credential, .. } => swap(credential),
                _ => {}
            }
        }
        for t in &mut self.tunnels {
            swap(&mut t.ssh.credential);
        }
    }

    /// Takes sessions (by index) and folders (with everything in them) out, for another workspace.
    pub fn take_out(&mut self, sessions: &[usize], folders: &[String]) -> Store {
        let inside = |p: &str| folders.iter().any(|f| p == f || p.starts_with(&format!("{f}/")));
        let mut out = Store::default();
        let mut i = 0;
        self.sessions.retain(|s| {
            let go = sessions.contains(&i) || inside(&s.folder);
            i += 1;
            if go {
                out.sessions.push(s.clone());
            }
            !go
        });
        out.folders = self.folders.iter().filter(|f| inside(&f.path)).cloned().collect();
        self.folders.retain(|f| !inside(&f.path));
        out
    }

    /// Workspace `name`'s data in the whole store: "" is the default one, the fields at the top.
    pub fn data_mut(&mut self, name: &str) -> Option<&mut Store> {
        if name.is_empty() {
            return Some(self);
        }
        self.workspaces.iter_mut().find(|w| w.name == name).map(|w| &mut w.data)
    }

    /// Workspace `name`'s sessions and folders, on their own.
    pub fn data(&self, name: &str) -> Option<Store> {
        let d = if name.is_empty() { self } else { &self.workspaces.iter().find(|w| w.name == name)?.data };
        Some(Store { sessions: d.sessions.clone(), folders: d.folders.clone(), ..Default::default() })
    }

    /// Sync (Settings > Workspaces): the sessions and folders of `from` come into this workspace.
    /// What both have is updated (sessions by folder and name), what's new is added, nothing is
    /// removed.
    pub fn sync_sessions(&mut self, from: &Store) {
        upsert(&mut self.sessions, &from.sessions, |a, b| a.folder == b.folder && a.name == b.name);
        upsert(&mut self.folders, &from.folders, |a, b| a.path == b.path);
    }


    /// Every website session, in every workspace, gets a browser profile of its own: copies (a
    /// cloned or synced workspace's) get new ones. Returns whether one changed, and the profiles.
    pub fn own_profiles(&mut self) -> (bool, std::collections::HashSet<String>) {
        let mut seen = Default::default();
        let mut changed = self.profiles(&mut seen);
        for w in &mut self.workspaces {
            changed |= w.data.profiles(&mut seen);
        }
        (changed, seen)
    }

    /// Puts what `take_out` took in: sessions all (names may repeat, as they may anywhere),
    /// shared logins and folder settings that aren't here yet.
    pub fn put_in(&mut self, mut moved: Store) {
        self.sessions.append(&mut moved.sessions);
        self.merge(moved);
    }

    pub fn subtree(&self, folder: &str) -> Store {
        if folder.is_empty() {
            return self.clone();
        }
        let inside = |p: &str| p == folder || p.starts_with(&format!("{folder}/"));
        Store {
            sessions: self.sessions.iter().filter(|s| inside(&s.folder)).cloned().collect(),
            folders: self.folders.iter().filter(|f| inside(&f.path)).cloned().collect(),
            ..Default::default()
        }
    }
}

/// The tabs open when xtermium last closed (`tabs.xtm`), for Settings > General > Reopen tabs.
#[derive(Serialize, Deserialize, Default)]
pub struct Tabs {
    #[serde(default, rename = "tab", deserialize_with = "readable")]
    pub tabs: Vec<Session>,
    /// The names tabs were given (renamed in the tab strip), by position; "" for none.
    #[serde(default, rename = "label")]
    pub labels: Vec<String>,
}

impl Tabs {
    pub fn load() -> Tabs {
        read("tabs", &[], |_| Ok(String::new())).ok().and_then(|t| toml::from_str::<Tabs>(&t).ok()).unwrap_or_default()
    }

    pub fn save(tabs: Vec<Session>, labels: Vec<String>) -> Result<()> {
        write("tabs", &toml::to_string(&Tabs { tabs, labels })?)
    }
}

// ---- trusted server keys (SSH host keys, RDP server keys) ----

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Known {
    /// `host:port` → `algorithm base64`
    #[serde(default)]
    pub ssh: BTreeMap<String, String>,
    /// `host:port` → SHA-256 of the server's public key
    #[serde(default)]
    pub rdp: BTreeMap<String, String>,
    /// `host:port` → SHA-256 of a BMC's (self-signed) HTTPS certificate (kvm.rs)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub https: BTreeMap<String, String>,
}

static KNOWN: Mutex<Option<Known>> = Mutex::new(None);

impl Known {
    fn load() -> Result<Self> {
        let text = read("known", &["known_hosts", "known_rdp"], |old| {
            Ok(toml::to_string(&Known::from_plain(&old[0], &old[1]))?)
        })?;
        Ok(toml::from_str(&text)?)
    }

    /// Old plain files: OpenSSH `known_hosts` lines (unhashed ones; hashed hosts are asked again)
    /// and `host:port sha256` lines.
    fn from_plain(known_hosts: &str, known_rdp: &str) -> Self {
        let mut k = Known::default();
        for line in known_hosts.lines() {
            let mut f = line.split_whitespace();
            let (Some(hosts), Some(algo), Some(b64)) = (f.next(), f.next(), f.next()) else { continue };
            for h in hosts.split(',').filter(|h| !h.starts_with('|')) {
                let id = match h.strip_prefix('[').and_then(|h| h.split_once("]:")) {
                    Some((host, port)) => format!("{host}:{port}"),
                    None => format!("{h}:22"),
                };
                k.ssh.insert(id, format!("{algo} {b64}"));
            }
        }
        for (id, fp) in known_rdp.lines().filter_map(|l| l.split_once(' ')) {
            k.rdp.insert(id.into(), fp.trim().into());
        }
        k
    }

    /// Runs `f` on the trusted keys (loaded on first use); saves if `f` changed them.
    pub fn with<R>(f: impl FnOnce(&mut Known) -> R) -> Result<R> {
        let mut guard = KNOWN.lock().unwrap();
        let known = match guard.as_mut() {
            Some(k) => k,
            None => guard.insert(Known::load()?),
        };
        let before = toml::to_string(known)?;
        let r = f(known);
        let after = toml::to_string(known)?;
        if after != before {
            write("known", &after)?;
        }
        Ok(r)
    }
}

// ---- the workspace in use, and passwords remembered when asked for (Settings > Saved logins) ----

static WORKSPACE: Mutex<String> = Mutex::new(String::new());

/// The workspace in use ("" the default one); kept in the settings.
pub fn workspace() -> String {
    WORKSPACE.lock().unwrap().clone()
}

pub fn set_workspace(name: &str) {
    *WORKSPACE.lock().unwrap() = name.into();
}

/// A password saved with "Remember the password" (`logins.xtm`, this device only), for every
/// workspace.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Login {
    /// The workspace it was saved in, when each kept its own; read only to merge them (`one_each`).
    #[serde(default, skip_serializing)]
    pub workspace: String,
    /// What it opens: `user@host:port` (SSH), `rdp:user@host:port`, `vnc:host:port`.
    pub id: String,
    pub password: String,
}

/// Saved logins as one list for every workspace: of the ones each workspace kept (before), the
/// default workspace's wins, then the first.
fn one_each(mut list: Vec<Login>) -> Vec<Login> {
    list.sort_by_key(|x| !x.workspace.is_empty());
    let mut seen = std::collections::HashSet::new();
    list.retain(|x| seen.insert(x.id.clone()));
    list.iter_mut().for_each(|x| x.workspace.clear());
    list
}

#[derive(Serialize, Deserialize, Default)]
struct Logins {
    #[serde(default, rename = "login")]
    list: Vec<Login>,
}

static LOGINS: Mutex<Option<Vec<Login>>> = Mutex::new(None);

/// The saved logins as a restore point (snapshot.rs) carries them: `[[login]]` tables.
pub fn logins_text() -> Result<String> {
    Ok(toml::to_string(&Logins { list: logins(|l| l.clone())? })?)
}

/// Puts back the saved logins of a restore point (`logins_text`); text without them (a backup
/// file) leaves them as they are.
pub fn restore_logins(text: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct Carried {
        login: Option<Vec<Login>>,
    }
    if let Some(list) = toml::from_str::<Carried>(text)?.login {
        logins(|l| *l = one_each(list))?;
    }
    Ok(())
}

/// Runs `f` on the saved logins (loaded on first use); saves if `f` changed them.
pub fn logins<R>(f: impl FnOnce(&mut Vec<Login>) -> R) -> Result<R> {
    let mut guard = LOGINS.lock().unwrap();
    let list = match guard.as_mut() {
        Some(l) => l,
        None => guard.insert(one_each(toml::from_str::<Logins>(&read("logins", &[], |_| Ok(String::new()))?)?.list)),
    };
    let before = list.clone();
    let r = f(list);
    if *list != before {
        write("logins", &toml::to_string(&Logins { list: list.clone() })?)?;
    }
    Ok(r)
}

/// The OS keychain entry a password was remembered in before `logins.xtm`.
fn keychain_login(id: &str) -> Option<keyring::Entry> {
    keyring::Entry::new("xtermium", id).ok()
}

/// The password saved for `id`. Ones still in the OS keychain (where they were kept before) move
/// over the first time they're used.
pub fn login(id: &str) -> Option<String> {
    let saved = logins(|l| l.iter().find(|x| x.id == id).map(|x| x.password.clone()));
    if let Ok(Some(p)) = saved {
        return Some(p);
    }
    let old = keychain_login(id)?;
    let p = old.get_password().ok()?;
    if remember(id, &p).is_ok() {
        let _ = old.delete_credential();
    }
    Some(p)
}

/// Saves a password, for every workspace.
pub fn remember(id: &str, password: &str) -> Result<()> {
    logins(|l| match l.iter_mut().find(|x| x.id == id) {
        Some(x) => x.password = password.into(),
        None => l.push(Login { id: id.into(), password: password.into(), ..Default::default() }),
    })
}

pub fn forget(id: &str) {
    let _ = logins(|l| l.retain(|x| x.id != id));
    if let Some(e) = keychain_login(id) {
        let _ = e.delete_credential();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The data moves from the old folder once, and never over data already in the new one, nor
    /// while an older xtermium runs on it.
    #[test]
    fn data_moves_to_the_new_folder() {
        let base = std::env::temp_dir().join(format!("xtermium-move-test-{}", std::process::id()));
        let (old, new) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("sessions.xtm"), "x").unwrap();
        let held = File::create(old.join("instance.lock")).unwrap();
        held.try_lock().unwrap();
        assert_eq!(moved(&old, new.clone()), old, "an older xtermium runs on it: it stays");
        drop(held);
        assert_eq!(moved(&old, new.clone()), new);
        assert!(new.join("sessions.xtm").exists() && !old.exists());
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(moved(&old, new.clone()), new, "the new folder has data: nothing moves");
        assert!(old.exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn sweep_keeps_running_runs_only() {
        let base = std::env::temp_dir().join(format!("xtermium-temp-test-{}", std::process::id()));
        let (live, ended, old) = (base.join("1"), base.join("2"), base.join("3"));
        for d in [&live, &ended, &old] {
            std::fs::create_dir_all(d.join("web")).unwrap();
        }
        let held = File::create(live.join(".lock")).unwrap();
        held.try_lock().unwrap();
        drop(File::create(ended.join(".lock")).unwrap());
        sweep(&base);
        assert!(live.join("web").exists(), "a running xtermium's data stays");
        assert!(!ended.exists() && !old.exists(), "ended runs' data goes");
        drop(held);
        let _ = std::fs::remove_dir_all(base);
    }

    /// A session of a kind this version doesn't have (an asciinema replay, from before) is left
    /// out; the rest of the file still loads, from TOML on disk as from JSON off the UI.
    #[test]
    fn unreadable_sessions_are_skipped() {
        let toml = "[[session]]\nname = \"old\"\nfolder = \"\"\n[session.kind]\ntype = \"replay\"\npath = \"a.cast\"\n\n\
                    [[session]]\nname = \"web\"\nfolder = \"Prod\"\n[session.kind]\ntype = \"shell\"\nprogram = \"\"\n";
        let s: Store = toml::from_str(toml).unwrap();
        assert_eq!(s.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["web"]);
        let json = r#"{"session":[{"name":"old","folder":"","kind":{"type":"replay","path":"a"}},{"name":"web","folder":"","kind":{"type":"shell"}}]}"#;
        let s: Store = serde_json::from_str(json).unwrap();
        assert_eq!(s.sessions.len(), 1);
    }

    #[test]
    fn plain_known_hosts_convert() {
        let k = Known::from_plain(
            "[127.0.0.1]:2222 ssh-ed25519 AAAA1\nexample.com,192.0.2.11 ssh-rsa BBBB comment\n|1|hashed|= ssh-ed25519 CCCC\n",
            "win:3389 abcd\n",
        );
        assert_eq!(k.ssh["127.0.0.1:2222"], "ssh-ed25519 AAAA1");
        assert_eq!(k.ssh["example.com:22"], "ssh-rsa BBBB");
        assert_eq!(k.ssh["192.0.2.11:22"], "ssh-rsa BBBB");
        assert_eq!(k.ssh.len(), 3);
        assert_eq!(k.rdp["win:3389"], "abcd");
    }

    #[test]
    fn shared_logins() {
        let mut store = Store::default();
        store.credentials.push(Credential {
            id: "c1".into(),
            name: "prod admin".into(),
            user: "admin".into(),
            password: "s3cret".into(),
            key: String::new(),
            gateway: None,
        });
        let mut ssh = Ssh::default();
        ssh.target.host = "db1".into();
        ssh.target.user = "me".into();
        ssh.target.credential = "c1".into();
        let mut kind = Kind::Ssh(ssh);
        assert_eq!(store.resolve(&mut kind), None, "SSH endpoints carry their password");
        let Kind::Ssh(s) = &kind else { panic!() };
        assert_eq!((s.target.user.as_str(), s.target.password.as_deref()), ("admin", Some("s3cret")));
        // What's stored names the login, never its password.
        let text = toml::to_string(&Session::new("db1", "", kind.clone())).unwrap();
        assert!(text.contains("credential = \"c1\"") && !text.contains("s3cret"), "{text}");

        let mut rdp =
            Kind::Rdp { host: "w".into(), port: 3389, user: String::new(), quality: None, credential: "c1".into(), smb_login: None, no_shares: false };
        assert_eq!(store.resolve(&mut rdp).as_deref(), Some("s3cret"));
        assert!(matches!(&rdp, Kind::Rdp { user, .. } if user == "admin"));
        let mut own = Kind::Vnc { host: "v".into(), port: 5900, credential: "gone".into() };
        assert_eq!(store.resolve(&mut own), None, "an unknown login changes nothing");
        assert!(store.gateway(&rdp).is_none());

        // A login's gateway: the jump host of SSH sessions without one, the way in for the rest.
        let gw = Gateway { host: "bastion".into(), port: 2222, user: "gw".into(), password: "pw".into(), key: String::new() };
        store.credentials[0].gateway = Some(gw);
        let web = Kind::Browser { url: "https://pve:8006".into(), credential: "c1".into(), profile: String::new() };
        let e = store.gateway(&web).unwrap();
        assert_eq!((e.host.as_str(), e.port, e.user.as_str(), e.password.as_deref()), ("bastion", 2222, "gw", Some("pw")));
        assert!(store.gateway(&rdp).is_some() && store.gateway(&own).is_none());
        let mut ssh = Ssh::default();
        ssh.target.credential = "c1".into();
        let mut kind = Kind::Ssh(ssh.clone());
        store.resolve(&mut kind);
        assert!(matches!(&kind, Kind::Ssh(s) if s.jump.as_ref().unwrap().host == "bastion"));
        ssh.jump = Some(Endpoint { host: "own".into(), ..Default::default() });
        let mut kind = Kind::Ssh(ssh);
        store.resolve(&mut kind);
        assert!(matches!(&kind, Kind::Ssh(s) if s.jump.as_ref().unwrap().host == "own"), "the session's own jump host wins");
    }

    #[test]
    fn subtree_and_merge() {
        let s = |name: &str, folder: &str| Session::new(name, folder, Kind::Shell { program: String::new(), dir: String::new() });
        let mut store = Store {
            sessions: vec![s("a", "Prod"), s("b", "Prod/DB"), s("c", "Production")],
            tunnels: vec![],
            folders: vec![Folder { path: "Prod".into(), tags: vec!["eu".into()], ..Default::default() }],
            ..Default::default()
        };
        let sub = store.subtree("Prod");
        assert_eq!(sub.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        let mut incoming = sub.clone();
        incoming.sessions.push(s("d", "Prod"));
        incoming.folders[0].tags.push("critical".into());
        assert_eq!(store.merge(incoming), (1, 0));
        assert_eq!(store.folders[0].tags, ["eu", "critical"]);
    }

    /// Each workspace comes out on its own and goes back where it was; the file keeps them all.
    #[test]
    fn workspaces_split_and_join() {
        let s = |name: &str| Session::new(name, "", Kind::Shell { program: String::new(), dir: String::new() });
        let names = |st: &Store| st.sessions.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let mut all = Store { sessions: vec![s("home")], ..Default::default() };
        all.workspaces.push(Workspace { name: "Work".into(), data: Store { sessions: vec![s("job")], ..Default::default() } });

        let (work, rest) = all.clone().split("Work");
        assert_eq!((names(&work), names(&rest)), (vec!["job".to_string()], vec!["home".to_string()]));
        assert!(rest.workspaces[0].data.sessions.is_empty(), "the workspace's place is empty while it's out");
        let mut work = work;
        work.sessions.push(s("job2"));
        let back = rest.join("Work", &work);
        assert_eq!(names(&back.workspaces[0].data), ["job", "job2"]);
        assert_eq!(names(&back), ["home"]);

        let (home, rest) = back.clone().split("");
        assert_eq!((names(&home), names(&rest), rest.workspaces.len()), (vec!["home".to_string()], vec![], 1));
        let text = toml::to_string_pretty(&rest.join("", &home)).unwrap();
        let again: Store = toml::from_str(&text).unwrap();
        assert_eq!((names(&again), names(&again.workspaces[0].data)), (vec!["home".to_string()], vec!["job".to_string(), "job2".to_string()]));
        // A name not there yet makes the workspace.
        assert_eq!(rest.join("New", &Store::default()).workspaces.len(), 2);
    }

    /// Shared logins, tunnels, snippets and layouts are every workspace's: each workspace comes
    /// out with them and its changes to them go to the top. A workspace's own copies (from when
    /// each kept its own) move up: the same merge, a differing one is kept beside, renamed, and
    /// the workspace's sessions follow its new id.
    #[test]
    fn workspaces_keep_only_sessions_apart() {
        let s = |name: &str, credential: &str| Session::new(name, "", Kind::Vnc { host: "h".into(), port: 5900, credential: credential.into() });
        let login = |id: &str, user: &str| Credential { id: id.into(), name: id.into(), user: user.into(), ..Default::default() };
        let snippet = |name: &str, text: &str| Snippet { name: name.into(), text: text.into(), run: true };
        let mut all = Store { sessions: vec![s("home", "c1")], credentials: vec![login("c1", "root")], snippets: vec![snippet("ls", "ls")], ..Default::default() };
        let old = Store {
            sessions: vec![s("job", "c1"), s("db", "c2")],
            credentials: vec![login("c1", "admin"), login("c2", "dba")],
            snippets: vec![snippet("ls", "ls"), snippet("df", "df -h"), snippet("ls", "ls -la")],
            ..Default::default()
        };
        all.workspaces.push(Workspace { name: "Work".into(), data: old });

        let (mut work, rest) = all.split("Work");
        let ids: Vec<_> = work.credentials.iter().map(|c| (c.id.as_str(), c.name.as_str())).collect();
        assert_eq!(ids, [("c1", "c1"), ("c12", "c1 (Work)"), ("c2", "c2")]);
        let cred = |s: &Session| match &s.kind {
            Kind::Vnc { credential, .. } => credential.clone(),
            _ => unreachable!(),
        };
        assert_eq!((cred(&work.sessions[0]), cred(&work.sessions[1])), ("c12".into(), "c2".into()), "its sessions keep their own login");
        let snippets: Vec<_> = work.snippets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(snippets, ["ls", "df", "ls (Work)"]);
        assert!(rest.workspaces[0].data.credentials.is_empty() && rest.workspaces[0].data.snippets.is_empty());

        work.snippets.clear();
        let back = rest.join("Work", &work);
        assert!(back.snippets.is_empty() && back.workspaces[0].data.snippets.is_empty(), "a change in any workspace is everyone's");
        let (home, _) = back.split("");
        assert_eq!(home.credentials.len(), 3, "the default workspace has them too");
        assert_eq!(one_each(vec![
            Login { workspace: "Work".into(), id: "a".into(), password: "w".into() },
            Login { workspace: String::new(), id: "a".into(), password: "d".into() },
            Login { workspace: "Work".into(), id: "b".into(), password: "b".into() },
        ]), [Login { id: "a".into(), password: "d".into(), ..Default::default() }, Login { id: "b".into(), password: "b".into(), ..Default::default() }]);
    }


    /// Moving to another workspace takes the picked sessions and whole folders; what's there
    /// already stays, and shared logins (every workspace's) stay where they are.
    #[test]
    fn moving_between_workspaces() {
        let s = |name: &str, folder: &str, credential: &str| {
            Session::new(name, folder, Kind::Vnc { host: "h".into(), port: 5900, credential: credential.into() })
        };
        let names = |st: &Store| st.sessions.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let login = |id: &str| Credential { id: id.into(), name: id.into(), ..Default::default() };
        let mut here = Store {
            sessions: vec![s("a", "", "c1"), s("b", "Prod", ""), s("c", "Prod/Web", "c2"), s("d", "Production", ""), s("e", "", "c1")],
            folders: vec![Folder { path: "Prod/Web".into(), color: "#ff0000".into(), ..Default::default() }],
            credentials: vec![login("c1"), login("c2"), login("c3")],
            ..Default::default()
        };
        let moved = here.take_out(&[0], &["Prod".into()]);
        assert_eq!((names(&moved), names(&here)), (["a", "b", "c"].map(String::from).to_vec(), ["d", "e"].map(String::from).to_vec()));
        assert_eq!(moved.folders.len(), 1);
        assert!(here.folders.is_empty() && here.credentials.len() == 3 && moved.credentials.is_empty());
        let mut there = Store { sessions: vec![s("a", "", "")], ..Default::default() };
        there.put_in(moved);
        assert_eq!((names(&there).len(), there.folders[0].color.as_str()), (4, "#ff0000"));
    }

    /// What a folder export file carries survives the trip: encrypt, decrypt, parse, merge.
    #[test]
    fn folder_export_round_trip() {
        let mut src = Store::default();
        src.sessions.push(Session::new(
            "db",
            "Prod/DB",
            Kind::Ssh(Ssh { target: Endpoint { host: "192.0.2.15".into(), ..Default::default() }, ..Default::default() }),
        ));
        src.folders.push(Folder { path: "Prod".into(), tags: vec!["eu".into()], ..Default::default() });
        let file =
            crate::vault::export_seal("pw", toml::to_string_pretty(&src.subtree("Prod")).unwrap().as_bytes()).unwrap();
        let back: Store =
            toml::from_str(std::str::from_utf8(&crate::vault::export_open("pw", &file).unwrap()).unwrap()).unwrap();
        let mut dst = Store::default();
        assert_eq!(dst.merge(back), (1, 0));
        assert_eq!(dst.sessions[0].folder, "Prod/DB");
        assert_eq!(dst.folders, src.folders);
    }
}
