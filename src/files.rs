//! File browser panel: SFTP for SSH tabs (on the tab's own SSH connection) and VNC tabs (on one
//! of its own to the desktop's host), and SMB for RDP tabs (the Windows host's shares). The UI's panel sends commands; every change is published back
//! as a `files` event carrying the whole state. Paths are `/`-separated; on SMB the first part
//! is the share and `/` lists the shares.

use crate::config::Endpoint;
use crate::prompt::Prompter;
use crate::ssh::Ctl;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use smb2::{ClientConfig, SmbClient, Tree};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Serialize, Clone)]
struct Entry {
    name: String,
    dir: bool,
    size: u64,
    date: String,
}

struct Transfer {
    upload: bool,
    label: String,
    done: Arc<AtomicU64>,
    total: u64,
    /// None while running; Some(Ok(message)) or Some(Err(message)) when finished.
    result: Option<Result<String, String>>,
}

#[derive(Default)]
struct State {
    fs: Option<Arc<Fs>>,
    cwd: String,
    entries: Vec<Entry>,
    error: String,
    transfers: Vec<Arc<Mutex<Transfer>>>,
}

/// What the UI's panel shows.
#[derive(Serialize, Clone)]
struct Snapshot<'a> {
    tab: u32,
    connected: bool,
    cwd: &'a str,
    entries: &'a [Entry],
    error: &'a str,
    transfers: Vec<TransferView>,
}

#[derive(Serialize, Clone)]
struct TransferView {
    upload: bool,
    label: String,
    done: u64,
    total: u64,
    ok: Option<bool>,
    message: String,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Cmd {
    Cd { path: String },
    Mkdir { name: String },
    Rename { from: String, to: String },
    Delete { name: String, dir: bool },
    Download { name: String, size: u64 },
    Upload { paths: Vec<String> },
    Clear,
    Dismiss,
}

/// Where the files are.
pub enum Fs {
    /// With the SSH connection it's on when it has one of its own (a VNC tab's), held only to
    /// keep it open.
    Sftp(Arc<SftpSession>, #[allow(dead_code)] Option<crate::ssh::Conn>),
    Smb(Box<Smb>),
}

/// An SMB session to one server, with the shares it has opened.
pub struct Smb {
    inner: tokio::sync::Mutex<(SmbClient, HashMap<String, Tree>)>,
    /// The SSH gateway's forwarded port, while the shares are reached through it.
    _gateway: Option<crate::ssh::Forward>,
}

const CHUNK: u64 = 1 << 20;

impl Smb {
    /// `user` may be `DOMAIN\user`.
    /// `via`: the RDP session's SSH gateway (its login's), which port 445 is reached through.
    pub async fn connect(host: &str, user: &str, password: &str, via: Option<(Endpoint, Prompter)>) -> Result<Self, String> {
        let gateway = match &via {
            Some((gw, prompt)) => Some(crate::ssh::forward(gw, Some((host.into(), 445)), prompt).await.map_err(|e| format!("{e:#}"))?),
            None => None,
        };
        let (domain, username) = crate::rdp::split_user(user);
        let config = ClientConfig {
            addr: gateway.as_ref().map_or(format!("{host}:445"), |f| format!("127.0.0.1:{}", f.port)),
            timeout: Duration::from_secs(10),
            username,
            password: password.into(),
            domain,
            auto_reconnect: true,
            ..Default::default()
        };
        let client = SmbClient::connect(config).await.map_err(|e| format!("SMB to {host}: {e}"))?;
        Ok(Self { inner: tokio::sync::Mutex::new((client, HashMap::new())), _gateway: gateway })
    }

    /// Runs `f` on the share's tree, opening the share on first use.
    async fn on<R>(
        &self,
        share: &str,
        f: impl AsyncFnOnce(&mut SmbClient, &mut Tree) -> smb2::Result<R>,
    ) -> Result<R, String> {
        let mut g = self.inner.lock().await;
        let (client, trees) = &mut *g;
        if !trees.contains_key(share) {
            let t = client.connect_share(share).await.map_err(|e| format!("{share}: {e}"))?;
            trees.insert(share.into(), t);
        }
        f(client, trees.get_mut(share).unwrap()).await.map_err(|e| e.to_string())
    }
}

/// `/share/dir/file` → ("share", "dir/file"); None for `/` (the list of shares).
fn split_share(path: &str) -> Option<(&str, &str)> {
    let p = path.trim_matches('/');
    (!p.is_empty()).then(|| p.split_once('/').unwrap_or((p, "")))
}

fn in_share(path: &str) -> Result<(&str, &str), String> {
    split_share(path).ok_or_else(|| "Open a share first.".into())
}

impl Fs {
    async fn home(&self) -> String {
        match self {
            Fs::Sftp(s, _) => s.canonicalize(".").await.unwrap_or_else(|_| "/".into()),
            Fs::Smb(_) => "/".into(),
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>, String> {
        let mut entries: Vec<Entry> = match self {
            Fs::Sftp(s, _) => s
                .read_dir(path)
                .await
                .map_err(|e| e.to_string())?
                .filter(|e| e.file_name() != "." && e.file_name() != "..")
                .map(|e| {
                    let m = e.metadata();
                    Entry {
                        name: e.file_name(),
                        dir: m.is_dir(),
                        size: m.len(),
                        date: m.mtime.map(date).unwrap_or_default(),
                    }
                })
                .collect(),
            Fs::Smb(smb) => match split_share(path) {
                None => {
                    let mut g = smb.inner.lock().await;
                    let (client, trees) = &mut *g;
                    let mut names: Vec<String> =
                        client.list_shares().await.map_err(|e| e.to_string())?.into_iter().map(|s| s.name).collect();
                    // smb2 leaves out administrative shares (C$, D$), often all a Windows host has
                    // for an admin: find the drives by opening them (kept open for browsing).
                    // ponytail: C$ to H$ only; a drive past H is reachable by typing /I$ in the path.
                    for d in 'C'..='H' {
                        let share = format!("{d}$");
                        if !trees.contains_key(&share)
                            && let Ok(t) = client.connect_share(&share).await
                        {
                            trees.insert(share.clone(), t);
                        }
                        if trees.contains_key(&share) {
                            names.push(share);
                        }
                    }
                    names.into_iter().map(|name| Entry { name, dir: true, size: 0, date: String::new() }).collect()
                }
                Some((share, rest)) => smb
                    .on(share, async |c, t| c.list_directory(t, rest).await)
                    .await?
                    .into_iter()
                    .filter(|e| e.name != "." && e.name != "..")
                    .map(|e| {
                        let secs = e.modified.to_system_time().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                        let date = secs.map(|d| date(d.as_secs() as u32)).unwrap_or_default();
                        Entry { name: e.name, dir: e.is_directory, size: e.size, date }
                    })
                    .collect(),
            },
        };
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }

    async fn mkdir(&self, path: &str) -> Result<(), String> {
        match self {
            Fs::Sftp(s, _) => s.create_dir(path).await.map_err(|e| e.to_string()),
            Fs::Smb(smb) => {
                let (share, p) = in_share(path)?;
                smb.on(share, async |c, t| c.create_directory(t, p).await).await
            }
        }
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        match self {
            Fs::Sftp(s, _) => s.rename(from, to).await.map_err(|e| e.to_string()),
            Fs::Smb(smb) => {
                let ((share, a), (_, b)) = (in_share(from)?, in_share(to)?);
                smb.on(share, async |c, t| c.rename(t, a, b).await).await
            }
        }
    }

    // ponytail: non-recursive; deleting a non-empty folder reports the server's error.
    async fn delete(&self, path: &str, dir: bool) -> Result<(), String> {
        match self {
            Fs::Sftp(s, _) if dir => s.remove_dir(path).await.map_err(|e| e.to_string()),
            Fs::Sftp(s, _) => s.remove_file(path).await.map_err(|e| e.to_string()),
            Fs::Smb(smb) => {
                let (share, p) = in_share(path)?;
                if dir {
                    smb.on(share, async |c, t| c.delete_directory(t, p).await).await
                } else {
                    smb.on(share, async |c, t| c.delete_file(t, p).await).await
                }
            }
        }
    }

    async fn download(&self, remote: &str, local: &Path, done: &AtomicU64, progress: impl Fn()) -> Result<(), String> {
        let dst = tokio::fs::File::create(local).await.map_err(|e| e.to_string())?;
        match self {
            Fs::Sftp(s, _) => {
                let src = s.open(remote).await.map_err(|e| e.to_string())?;
                copy(src, dst, done, progress).await.map_err(|e| e.to_string())
            }
            Fs::Smb(smb) => {
                let (share, p) = in_share(remote)?;
                // The reader doesn't hold the session: browsing goes on during the download.
                let r = smb.on(share, async |c, t| c.open_file_reader(t, p).await).await?;
                let mut dst = tokio::io::BufWriter::with_capacity(4 << 20, dst);
                let (size, mut at, mut last) = (r.size(), 0u64, Instant::now());
                let err = |e: smb2::Error| e.to_string();
                while at < size {
                    // Four reads in flight hide most of the round trip on distant links.
                    let base = at;
                    let o = move |i: u64| base + i * CHUNK;
                    let (a, b, c, d) = tokio::join!(
                        r.read_at(o(0), CHUNK),
                        r.read_at(o(1), CHUNK),
                        r.read_at(o(2), CHUNK),
                        r.read_at(o(3), CHUNK)
                    );
                    for (i, part) in [a, b, c, d].into_iter().enumerate() {
                        if o(i as u64) >= size {
                            break;
                        }
                        let part = part.map_err(err)?;
                        if part.is_empty() {
                            at = size;
                            break;
                        }
                        dst.write_all(&part).await.map_err(|e| e.to_string())?;
                        done.fetch_add(part.len() as u64, Relaxed);
                        at = o(i as u64) + part.len() as u64;
                    }
                    if last.elapsed() > Duration::from_millis(100) {
                        last = Instant::now();
                        progress();
                    }
                }
                let _ = r.close().await;
                dst.shutdown().await.map_err(|e| e.to_string())
            }
        }
    }

    async fn upload(&self, local: &Path, remote: &str, done: &AtomicU64, progress: impl Fn()) -> Result<(), String> {
        let mut src = tokio::fs::File::open(local).await.map_err(|e| e.to_string())?;
        match self {
            Fs::Sftp(s, _) => {
                let dst = s.create(remote).await.map_err(|e| e.to_string())?;
                copy(src, dst, done, progress).await.map_err(|e| e.to_string())
            }
            Fs::Smb(smb) => {
                let (share, p) = in_share(remote)?;
                let mut w = smb.on(share, async |c, t| c.create_file_writer(t, p).await).await?;
                let mut buf = vec![0u8; CHUNK as usize];
                let mut last = Instant::now();
                loop {
                    let n = src.read(&mut buf).await.map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    w.write_chunk(&buf[..n]).await.map_err(|e| e.to_string())?;
                    done.fetch_add(n as u64, Relaxed);
                    if last.elapsed() > Duration::from_millis(100) {
                        last = Instant::now();
                        progress();
                    }
                }
                w.finish().await.map(|_| ()).map_err(|e| e.to_string())
            }
        }
    }
}

#[derive(Clone)]
pub struct Browser {
    state: Arc<Mutex<State>>,
    app: AppHandle,
    tab: u32,
}

impl Browser {
    /// Starts opening `fs` and publishes the state once it's there (or failed).
    pub fn new(fs: impl Future<Output = Result<Fs, String>> + Send + 'static, app: AppHandle, tab: u32) -> Self {
        let b = Self { state: Default::default(), app, tab };
        let b2 = b.clone();
        tauri::async_runtime::spawn(async move {
            match fs.await {
                Ok(fs) => {
                    let fs = Arc::new(fs);
                    let home = fs.home().await;
                    b2.state.lock().unwrap().fs = Some(fs.clone());
                    b2.list(&fs, home).await;
                }
                Err(e) => b2.state.lock().unwrap().error = e,
            }
            b2.publish();
        });
        b
    }

    /// SFTP over an SSH tab's connection.
    pub fn sftp(tx: &UnboundedSender<Ctl>, app: AppHandle, tab: u32) -> Self {
        let (reply, rx) = tokio::sync::oneshot::channel();
        let ok = tx.send(Ctl::Sftp(reply)).is_ok();
        let fs = async move {
            match rx.await {
                Ok(Ok(s)) => Ok(Fs::Sftp(Arc::new(s), None)),
                Ok(Err(e)) => Err(format!("SFTP: {e:#}")),
                Err(_) if !ok => Err("session is not connected".into()),
                Err(_) => Err("SSH session not ready yet; reopen the panel once logged in".into()),
            }
        };
        Self::new(fs, app, tab)
    }

    /// SFTP on an SSH connection of its own: a VNC tab's, to the desktop's host (`target`, as its
    /// shared login's user), through `jump` (the login's gateway).
    pub fn sftp_alone(target: Endpoint, jump: Option<Endpoint>, prompt: Prompter, app: AppHandle, tab: u32) -> Self {
        let fs = async move {
            let (s, conn) = crate::ssh::sftp_alone(&target, jump.as_ref(), &prompt).await.map_err(|e| format!("SFTP: {e:#}"))?;
            Ok(Fs::Sftp(Arc::new(s), Some(conn)))
        };
        Self::new(fs, app, tab)
    }

    /// It couldn't open: opening again may work (the server is up now, the login was mistyped).
    pub fn failed(&self) -> bool {
        let st = self.state.lock().unwrap();
        st.fs.is_none() && !st.error.is_empty()
    }

    /// Send the current state to the UI.
    pub fn publish(&self) {
        let st = self.state.lock().unwrap();
        let transfers = st
            .transfers
            .iter()
            .map(|t| {
                let t = t.lock().unwrap();
                let (ok, message) = match &t.result {
                    None => (None, String::new()),
                    Some(Ok(m)) => (Some(true), m.clone()),
                    Some(Err(m)) => (Some(false), m.clone()),
                };
                TransferView {
                    upload: t.upload,
                    label: t.label.clone(),
                    done: t.done.load(Relaxed),
                    total: t.total,
                    ok,
                    message,
                }
            })
            .collect();
        let snap = Snapshot {
            tab: self.tab,
            connected: st.fs.is_some(),
            cwd: &st.cwd,
            entries: &st.entries,
            error: &st.error,
            transfers,
        };
        let _ = self.app.emit("files", snap);
    }

    pub fn cmd(&self, c: Cmd) {
        match c {
            Cmd::Cd { path } => self.run(|_, _| async { Ok(()) }, Some(path)),
            Cmd::Mkdir { name } => self.run(move |fs, cwd| async move { fs.mkdir(&join(&cwd, &name)).await }, None),
            Cmd::Rename { from, to } => {
                self.run(move |fs, cwd| async move { fs.rename(&join(&cwd, &from), &join(&cwd, &to)).await }, None)
            }
            Cmd::Delete { name, dir } => {
                self.run(move |fs, cwd| async move { fs.delete(&join(&cwd, &name), dir).await }, None)
            }
            Cmd::Download { name, size } => self.download(name, size),
            Cmd::Upload { paths } => paths.into_iter().for_each(|p| self.upload(p.into())),
            Cmd::Clear => {
                self.state.lock().unwrap().transfers.retain(|t| t.lock().unwrap().result.is_none());
                self.publish();
            }
            Cmd::Dismiss => {
                self.state.lock().unwrap().error.clear();
                self.publish();
            }
        }
    }

    /// Run an operation in the background, then list `path` (default: the current folder).
    fn run<F, Fut>(&self, f: F, path: Option<String>)
    where
        F: FnOnce(Arc<Fs>, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), String>> + Send,
    {
        let Some(fs) = self.state.lock().unwrap().fs.clone() else { return };
        let cwd = self.state.lock().unwrap().cwd.clone();
        let b = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = f(fs.clone(), cwd.clone()).await {
                b.state.lock().unwrap().error = e;
            }
            b.list(&fs, path.unwrap_or(cwd)).await;
            b.publish();
        });
    }

    async fn list(&self, fs: &Fs, path: String) {
        match fs.list(&path).await {
            Ok(entries) => {
                let mut st = self.state.lock().unwrap();
                st.entries = entries;
                st.cwd = path;
            }
            Err(e) => self.state.lock().unwrap().error = format!("{path}: {e}"),
        }
    }

    fn transfer(&self, upload: bool, label: String, total: u64) -> (Arc<Mutex<Transfer>>, Arc<AtomicU64>) {
        let done = Arc::new(AtomicU64::new(0));
        let t = Arc::new(Mutex::new(Transfer { upload, label, done: done.clone(), total, result: None }));
        self.state.lock().unwrap().transfers.push(t.clone());
        self.publish();
        (t, done)
    }

    fn download(&self, name: String, size: u64) {
        let Some(fs) = self.state.lock().unwrap().fs.clone() else { return };
        let remote = join(&self.state.lock().unwrap().cwd, &name);
        let dir = directories::UserDirs::new()
            .and_then(|d| d.download_dir().map(|p| p.to_path_buf()).or(Some(d.home_dir().to_path_buf())))
            .unwrap_or_else(|| PathBuf::from("."));
        let local = unique(dir.join(local_name(&name)));
        let (t, done) = self.transfer(false, name, size);
        let b = self.clone();
        tauri::async_runtime::spawn(async move {
            let r = fs.download(&remote, &local, &done, || b.publish()).await;
            t.lock().unwrap().result = Some(r.map(|()| format!("saved to {}", local.display())));
            b.publish();
        });
    }

    fn upload(&self, local: PathBuf) {
        let Some(name) = local.file_name().map(|n| n.to_string_lossy().to_string()) else { return };
        let size = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0);
        let (t, done) = self.transfer(true, name.clone(), size);
        let b = self.clone();
        self.run(
            move |fs, cwd| async move {
                let remote = join(&cwd, &name);
                let r = fs.upload(&local, &remote, &done, || b.publish()).await;
                t.lock().unwrap().result = Some(r.map(|()| format!("uploaded to {remote}")));
                Ok(())
            },
            None,
        );
    }
}

/// Copy with a live byte counter; `progress` runs at most every 100 ms. Transfers are binary,
/// byte for byte, both ways: SFTP 3 has no text mode, and nothing here touches line endings
/// (ssh.rs tests send every byte value through and back).
async fn copy(
    mut r: impl AsyncRead + Unpin,
    mut w: impl AsyncWrite + Unpin,
    done: &AtomicU64,
    progress: impl Fn(),
) -> std::io::Result<()> {
    let mut buf = vec![0u8; 256 * 1024];
    let mut last = Instant::now();
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).await?;
        done.fetch_add(n as u64, Relaxed);
        if last.elapsed() > Duration::from_millis(100) {
            last = Instant::now();
            progress();
        }
    }
    w.shutdown().await
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

/// A name from the server's listing, made safe to save under here: only its last part, without
/// characters Windows refuses, and not a device name (`CON`, `NUL`…). A hostile server listing
/// `..\..\Startup\x.bat` or `C:\Windows\…` still only writes into the downloads folder.
fn local_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last.chars().map(|c| if c.is_control() || r#"<>:"|?*"#.contains(c) { '_' } else { c }).collect();
    // Windows drops trailing dots and spaces, which would make ".." the folder above.
    let clean = clean.trim_end_matches(['.', ' ']);
    let stem = clean.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    match clean {
        "" => "download".into(),
        _ if device => format!("_{clean}"),
        _ => clean.into(),
    }
}

/// Don't overwrite an existing download: `a.txt` → `a (1).txt`.
fn unique(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..).map(|i| p.with_file_name(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).unwrap()
}

/// Unix seconds → `YYYY-MM-DD HH:MM` (UTC).
/// `YYYY-MM-DD HH:MM`, UTC.
pub fn date(secs: u32) -> String {
    let (days, rem) = (secs as i64 / 86400, secs as i64 % 86400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_dates() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/home/me", "a"), "/home/me/a");
        assert_eq!(date(0), "1970-01-01 00:00");
        assert_eq!(date(951_782_400), "2000-02-29 00:00");
        assert_eq!(date(1_791_000_000), "2026-10-03 04:00");
    }

    #[test]
    fn downloads_stay_in_their_folder() {
        assert_eq!(local_name("report.pdf"), "report.pdf");
        assert_eq!(local_name(r"..\..\AppData\Roaming\Startup\x.bat"), "x.bat");
        assert_eq!(local_name("../../.bashrc"), ".bashrc");
        assert_eq!(local_name(r"C:\Windows\System32\evil.dll"), "evil.dll");
        assert_eq!(local_name(".."), "download");
        assert_eq!(local_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(local_name("con.txt"), "_con.txt");
        assert_eq!(local_name("COM1"), "_COM1");
        assert_eq!(local_name("console.log"), "console.log");
        assert_eq!(local_name("name. . "), "name");
        let dir = PathBuf::from("downloads");
        for hostile in [r"..\x", "/etc/passwd", r"\\server\share\x", "C:x"] {
            assert_eq!(dir.join(local_name(hostile)).parent(), Some(dir.as_path()), "{hostile}");
        }
    }

    #[test]
    fn share_paths() {
        assert_eq!(split_share("/"), None);
        assert_eq!(split_share("/C$"), Some(("C$", "")));
        assert_eq!(split_share("/Users/me/Desktop/a.txt"), Some(("Users", "me/Desktop/a.txt")));
        assert_eq!(in_share("/").unwrap_err(), "Open a share first.");
    }

    /// Against a real Windows host, with the login of its saved RDP session (user from the
    /// session, its saved password, never printed). Writes, reads back, renames and
    /// deletes one file in C:\Windows\Temp:
    /// `XT_SMB_HOST=<host> cargo test smb_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn smb_live() {
        let host = std::env::var("XT_SMB_HOST").expect("set XT_SMB_HOST");
        assert!(matches!(crate::config::init_key().0, crate::config::KeyState::Ready), "data is locked");
        let store = crate::config::Store::load().unwrap();
        // The session for this host, by name or address; saved passwords are keyed by address.
        let (user, addr, port) = store
            .sessions
            .iter()
            .find_map(|s| match &s.kind {
                crate::config::Kind::Rdp { host: h, port, user, .. }
                    if h.eq_ignore_ascii_case(&host) || s.name.eq_ignore_ascii_case(&host) =>
                {
                    Some((user.clone(), h.clone(), *port))
                }
                _ => None,
            })
            .expect("no saved RDP session named or addressed like XT_SMB_HOST");
        let user = if user.is_empty() { "Administrator".to_string() } else { user };
        let password = crate::config::login(&format!("rdp:{user}@{addr}:{port}")).expect("no saved password for that login");
        println!("login: {user}@{host} (saved password)");
        let fs = Fs::Smb(Box::new(Smb::connect(&host, &user, &password, None).await.unwrap()));
        let shares = fs.list("/").await.unwrap();
        println!("shares: {:?}", shares.iter().map(|e| &e.name).collect::<Vec<_>>());
        let root = fs.list("/C$").await.unwrap();
        println!("C$: {} entries, e.g. {:?}", root.len(), root.iter().take(8).map(|e| &e.name).collect::<Vec<_>>());

        let data: Vec<u8> = (0..8u32 << 20).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let dir = std::env::temp_dir();
        let (src, back) = (dir.join("xtermium-smb-up.bin"), dir.join("xtermium-smb-down.bin"));
        std::fs::write(&src, &data).unwrap();
        let name = format!("xtermium-smb-test-{}.bin", std::process::id());
        let remote = format!("/C$/Windows/Temp/{name}");
        let t = Instant::now();
        fs.upload(&src, &remote, &AtomicU64::new(0), || {}).await.unwrap();
        let up = t.elapsed();
        let listed = fs.list("/C$/Windows/Temp").await.unwrap();
        assert!(listed.iter().any(|e| e.name == name && e.size == data.len() as u64), "uploaded file not listed");
        let t = Instant::now();
        fs.download(&remote, &back, &AtomicU64::new(0), || {}).await.unwrap();
        let down = t.elapsed();
        assert!(std::fs::read(&back).unwrap() == data, "downloaded bytes differ");
        let renamed = format!("{remote}.renamed");
        fs.rename(&remote, &renamed).await.unwrap();
        fs.delete(&renamed, false).await.unwrap();
        assert!(!fs.list("/C$/Windows/Temp").await.unwrap().iter().any(|e| e.name.starts_with(&name)), "not deleted");
        let _ = (std::fs::remove_file(src), std::fs::remove_file(back));
        let mbs = |d: Duration| 8.0 / d.as_secs_f64();
        println!("8 MiB up {up:?} ({:.1} MB/s), down {down:?} ({:.1} MB/s); renamed and deleted", mbs(up), mbs(down));
    }
}
