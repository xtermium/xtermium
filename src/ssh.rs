use crate::config::{self, Endpoint, Tunnel, TunnelKind};
use crate::prompt::Prompter;
use anyhow::{Context, Result, anyhow, bail};
use russh::client::{self, Handle};
use russh::keys::{
    self, HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate, agent::client::AgentClient,
};
use russh::{ChannelMsg, Preferred, cipher, compression};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedReceiver;

pub struct Client {
    host: String,
    port: u16,
    prompt: Prompter,
    /// Where to send connections the server opens for a remote (-R) forward.
    forward_to: Option<(String, u16, Arc<Traffic>)>,
    /// Agent forwarding was asked for: only then are the server's agent channels accepted.
    agent: bool,
    /// Where to note what was negotiated (the status bar), for an interactive session.
    link: Option<Arc<Mutex<Link>>>,
}

/// An SSH tab's connection, for the status bar.
#[derive(Clone, Default, serde::Serialize)]
pub struct Link {
    pub cipher: String,
    pub kex: String,
    pub host_key: String,
    pub compression: bool,
    /// The last ping's round trip in milliseconds; None until one came back.
    pub rtt_ms: Option<u32>,
}

/// Aborts a background task when dropped (the session ended, however it ended).
struct Task(tokio::task::JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn kex_done(&mut self, _: Option<&[u8]>, names: &russh::Names, _: &mut client::Session) -> Result<()> {
        if let Some(link) = &self.link {
            let mut l = link.lock().unwrap();
            l.cipher = names.cipher.as_ref().to_string();
            l.kex = names.kex.as_ref().to_string();
            l.host_key = names.key.to_string();
            l.compression = !matches!(names.client_compression, compression::Compression::None);
        }
        Ok(())
    }

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            bail!("SSH certificates are not supported yet");
        };
        let id = format!("{}:{}", self.host, self.port);
        // `algorithm base64`, as in an OpenSSH known_hosts line.
        let line = key.to_openssh()?;
        let presented = line.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        match config::Known::with(|k| k.ssh.get(&id).cloned())? {
            Some(known) if known == presented => Ok(true),
            Some(_) => bail!(
                "HOST KEY CHANGED for {id}. Possible man-in-the-middle. \
                 If the change is expected, remove the old key under Settings > Security."
            ),
            None => {
                let fp = key.fingerprint(HashAlg::Sha256);
                let msg = format!(
                    "Unknown host {id}\n{} key fingerprint:\n{fp}\n\nTrust this host and remember its key?",
                    key.algorithm()
                );
                if self.prompt.confirm("Unknown host key", msg).await {
                    config::Known::with(|k| k.ssh.insert(id, presented))?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// The server opened a channel to our agent (agent forwarding): pipe it to the local one.
    /// Refused unless this connection asked for forwarding.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _: &mut client::Session,
    ) -> Result<()> {
        if !self.agent {
            reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
            return Ok(());
        }
        reply.accept().await;
        tokio::spawn(async move {
            // No local agent: the channel closes, and the server's request fails.
            if let Some(mut agent) = local_agent().await {
                let _ = tokio::io::copy_bidirectional(&mut channel.into_stream(), &mut agent).await;
            }
        });
        Ok(())
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _addr: &str,
        _port: u32,
        _orig_addr: &str,
        _orig_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<()> {
        let Some((host, port, traffic)) = self.forward_to.clone() else { return Ok(()) };
        reply.accept().await;
        tokio::spawn(async move {
            if let Ok(tcp) = TcpStream::connect((host.as_str(), port)).await {
                pump(channel.into_stream(), tcp, &traffic).await;
            }
        });
        Ok(())
    }
}

/// An authenticated connection. Keeps the jump-host connection alive with it.
pub struct Conn {
    pub handle: Handle<Client>,
    _jump: Option<Handle<Client>>,
}

pub async fn connect(
    target: &Endpoint,
    jump: Option<&Endpoint>,
    opts: &Options,
    prompt: &Prompter,
    forward_to: Option<(String, u16, Arc<Traffic>)>,
) -> Result<Conn> {
    let cfg = Arc::new(client_config(opts));
    let client =
        Client { host: target.host.clone(), port: target.port, prompt: prompt.clone(), forward_to, agent: opts.agent, link: opts.link.clone() };
    let (mut handle, jump) = match jump {
        None => {
            let h = client::connect(cfg, (target.host.as_str(), target.port), client)
                .await
                .with_context(|| format!("connecting to {}", target.label()))?;
            (h, None)
        }
        Some(j) => {
            let jh = connect_auth(j, cfg.clone(), prompt).await.with_context(|| format!("jump host {}", j.label()))?;
            let ch = jh.channel_open_direct_tcpip(target.host.clone(), target.port as u32, "127.0.0.1", 0).await?;
            let h = client::connect_stream(cfg, ch.into_stream(), client).await?;
            (h, Some(jh))
        }
    };
    authenticate(&mut handle, target, prompt).await?;
    Ok(Conn { handle, _jump: jump })
}

async fn connect_auth(ep: &Endpoint, cfg: Arc<client::Config>, prompt: &Prompter) -> Result<Handle<Client>> {
    let client = Client { host: ep.host.clone(), port: ep.port, prompt: prompt.clone(), forward_to: None, agent: false, link: None };
    let mut h = client::connect(cfg, (ep.host.as_str(), ep.port), client).await?;
    authenticate(&mut h, ep, prompt).await?;
    Ok(h)
}

/// The ciphers xtermium offers, best first: authenticated (AEAD) ones with 256-bit keys, which
/// keep 128-bit security against quantum computers, then AES-128-GCM, then CTR modes (every
/// server since OpenSSH 5.2). CBC is only for old network gear and isn't offered unless chosen.
/// AES-256-GCM leads ChaCha20-Poly1305 for speed on CPUs with AES instructions (488 against
/// 312 MB/s here; AES-128-GCM 896, CTR about 390: `bench_ciphers`); ChaCha20 is the faster one
/// without them, and is OpenSSH's own first choice.
pub const CIPHERS: [&str; 9] = [
    "aes256-gcm@openssh.com",
    "chacha20-poly1305@openssh.com",
    "aes128-gcm@openssh.com",
    "aes256-ctr",
    "aes192-ctr",
    "aes128-ctr",
    "aes256-cbc",
    "aes192-cbc",
    "aes128-cbc",
];

/// What's offered unless changed: everything but CBC.
pub fn default_ciphers() -> Vec<String> {
    CIPHERS.iter().filter(|c| !c.ends_with("-cbc")).map(|c| c.to_string()).collect()
}

/// How a connection is set up: ciphers to offer (best first), seconds between keepalives (0 for
/// none) and zlib compression.
#[derive(Clone)]
pub struct Options {
    pub ciphers: Vec<String>,
    pub keepalive: u32,
    pub compression: bool,
    /// Agent forwarding: the server may open channels to this computer's SSH agent.
    pub agent: bool,
    /// Where an interactive session notes what was negotiated (shell() sets it).
    pub link: Option<Arc<Mutex<Link>>>,
    /// Carries other programs' traffic (tunnels, gateways): see `carrier`.
    pub carrier: bool,
}

impl Options {
    /// A session's own choices, else Settings > Defaults; tunnels take the defaults.
    pub fn of(ssh: Option<&config::Ssh>) -> Self {
        let d = DEFAULTS.lock().unwrap().clone().unwrap_or_default();
        Options {
            ciphers: ssh.map(|s| s.ciphers.clone()).filter(|c| !c.is_empty()).unwrap_or(d.ciphers),
            keepalive: ssh.and_then(|s| s.keepalive).unwrap_or(d.keepalive),
            compression: ssh.and_then(|s| s.compression).unwrap_or(d.compression),
            agent: ssh.is_some_and(|s| s.agent_forward),
            link: None,
            carrier: false,
        }
    }

    /// For tunnels and gateways: a wide window, so one connection fills a gigabit link even far
    /// away (2 MB, russh's default, tops out near 500 Mbit/s at 30 ms), and keepalives often
    /// enough that a mobile link that went quiet is noticed, and reconnected, within a minute.
    pub fn carrier() -> Self {
        Options { carrier: true, ..Self::of(None) }
    }
}

fn client_config(o: &Options) -> client::Config {
    let mut ciphers: Vec<cipher::Name> = o
        .ciphers
        .iter()
        .filter(|c| CIPHERS.contains(&c.as_str()))
        .filter_map(|c| cipher::Name::try_from(c.as_str()).ok())
        .collect();
    if ciphers.is_empty() {
        ciphers = default_ciphers().iter().filter_map(|c| cipher::Name::try_from(c.as_str()).ok()).collect();
    }
    // zlib@openssh.com starts after login, so passwords never pass through the compressor.
    let compression = if o.compression {
        vec![compression::ZLIB_LEGACY, compression::ZLIB, compression::NONE]
    } else {
        vec![compression::NONE]
    };
    // Carriers send keepalives at least every 15 s, even with them off in Settings.
    let keepalive = if o.carrier { if o.keepalive == 0 { 15 } else { o.keepalive.min(15) } } else { o.keepalive };
    client::Config {
        // Three missed keepalives drop the connection, so a dead link shows in seconds, not hours;
        // four for carriers, which ride out a mobile handover rather than reconnect through it.
        keepalive_interval: (keepalive > 0).then(|| Duration::from_secs(keepalive.into())),
        keepalive_max: if o.carrier { 4 } else { 3 },
        nodelay: true,
        // 16 MB in flight per channel: about 4 Gbit/s at 30 ms. Memory only fills if the local
        // side reads slower than the link brings data.
        window_size: if o.carrier { 16 << 20 } else { 2 << 20 },
        channel_buffer_size: if o.carrier { 512 } else { 100 },
        preferred: Preferred {
            cipher: Cow::Owned(ciphers),
            compression: Cow::Owned(compression),
            ..Preferred::DEFAULT
        },
        ..Default::default()
    }
}

/// Settings > Defaults: used when a session leaves these empty.
#[derive(Clone, Default)]
pub struct Defaults {
    pub user: String,
    pub key: String,
    pub password: String,
    pub ciphers: Vec<String>,
    pub keepalive: u32,
    pub compression: bool,
}

static DEFAULTS: Mutex<Option<Defaults>> = Mutex::new(None);

pub fn set_defaults(d: Defaults) {
    *DEFAULTS.lock().unwrap() = Some(d);
}

/// agent -> key file (the session's; else the default plus ~/.ssh keys) -> remembered password ->
/// default password -> prompted password / keyboard-interactive.
async fn authenticate(h: &mut Handle<Client>, ep: &Endpoint, prompt: &Prompter) -> Result<()> {
    let d = DEFAULTS.lock().unwrap().clone().unwrap_or_default();
    let user = match (ep.user.is_empty(), d.user.is_empty()) {
        (false, _) => ep.user.clone(),
        (true, false) => d.user.clone(),
        (true, true) => {
            prompt.ask("Login", format!("Login as ({}):", ep.host), false).await.ok_or_else(|| anyhow!("cancelled"))?.0
        }
    };
    let rsa_hash = h.best_supported_rsa_hash().await?.flatten();

    if try_agent(h, &user, rsa_hash).await {
        return Ok(());
    }

    let keys: Vec<std::path::PathBuf> = if ep.key.is_empty() {
        let ssh = directories::UserDirs::new().map(|d| d.home_dir().join(".ssh")).unwrap_or_default();
        let default = (!d.key.is_empty()).then(|| home_path(&d.key));
        let usual = ["id_ed25519", "id_ecdsa", "id_rsa"].iter().map(|k| ssh.join(k));
        default.into_iter().chain(usual).filter(|p| p.exists()).collect()
    } else {
        vec![home_path(&ep.key)]
    };
    for path in keys {
        let key = match keys::load_secret_key(&path, None) {
            Ok(k) => k,
            Err(keys::Error::KeyIsEncrypted) => {
                let Some((pass, _)) =
                    prompt.ask("Key passphrase", format!("Passphrase for {}:", path.display()), true).await
                else {
                    continue;
                };
                keys::load_secret_key(&path, Some(&pass))?
            }
            Err(e) => {
                if !ep.key.is_empty() {
                    return Err(e).with_context(|| format!("loading key {}", path.display()));
                }
                continue;
            }
        };
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
        if h.authenticate_publickey(&user, key).await?.success() {
            return Ok(());
        }
    }

    // A shared login's password (Credentials) comes before a saved one and asking.
    if let Some(p) = &ep.password
        && password_auth(h, &user, p).await?
    {
        return Ok(());
    }
    let id = format!("{user}@{}:{}", ep.host, ep.port);
    let mut stored = crate::config::login(&id);
    let mut default = Some(d.password).filter(|p| !p.is_empty());
    for _ in 0..4 {
        let (pass, remember, from_store) = match (stored.take(), default.take()) {
            (Some(p), d) => {
                default = d;
                (p, false, true)
            }
            (None, Some(p)) => (p, false, false),
            (None, None) => match prompt.ask("Password", format!("Password for {user}@{}:", ep.host), true).await {
                Some((p, r)) => (p, r, false),
                None => bail!("cancelled"),
            },
        };
        if password_auth(h, &user, &pass).await? {
            if remember {
                let _ = crate::config::remember(&id, &pass);
            }
            return Ok(());
        }
        if from_store {
            crate::config::forget(&id);
        }
    }
    bail!("authentication failed")
}

/// `~/x` (or `~\x`) → the home directory's `x`.
fn home_path(p: &str) -> std::path::PathBuf {
    match (p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")), directories::UserDirs::new()) {
        (Some(rest), Some(d)) => d.home_dir().join(rest),
        _ => p.into(),
    }
}

async fn password_auth(h: &mut Handle<Client>, user: &str, pass: &str) -> Result<bool> {
    if h.authenticate_password(user, pass).await?.success() {
        return Ok(true);
    }
    use client::KeyboardInteractiveAuthResponse as K;
    let mut resp = h.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..5 {
        match resp {
            K::Success => return Ok(true),
            K::Failure { .. } => return Ok(false),
            K::InfoRequest { prompts, .. } => {
                resp = h.authenticate_keyboard_interactive_respond(vec![pass.to_string(); prompts.len()]).await?;
            }
        }
    }
    Ok(false)
}

/// The local SSH agent that holds keys, as a raw stream: SSH_AUTH_SOCK on Unix; on Windows
/// the OpenSSH agent service, else Pageant.
async fn local_agent() -> Option<Box<dyn keys::agent::client::AgentStream + Send + Unpin>> {
    #[cfg(unix)]
    {
        AgentClient::connect_env().await.ok().map(|a| a.into_inner())
    }
    #[cfg(windows)]
    {
        if let Ok(mut a) = AgentClient::connect_named_pipe(r"\.\pipe\openssh-ssh-agent").await
            && a.request_identities().await.is_ok_and(|ids| !ids.is_empty())
        {
            return Some(a.into_inner());
        }
        AgentClient::connect_pageant().await.ok().map(|a| a.into_inner())
    }
}

async fn try_agent(h: &mut Handle<Client>, user: &str, rsa_hash: Option<HashAlg>) -> bool {
    #[cfg(unix)]
    if let Ok(a) = AgentClient::connect_env().await {
        return agent_auth(h, user, rsa_hash, a).await;
    }
    #[cfg(windows)]
    {
        if let Ok(a) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await
            && agent_auth(h, user, rsa_hash, a).await
        {
            return true;
        }
        if let Ok(a) = AgentClient::connect_pageant().await {
            return agent_auth(h, user, rsa_hash, a).await;
        }
    }
    false
}

async fn agent_auth<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    h: &mut Handle<Client>,
    user: &str,
    rsa_hash: Option<HashAlg>,
    mut agent: AgentClient<S>,
) -> bool {
    let Ok(ids) = agent.request_identities().await else { return false };
    for id in ids {
        let keys::agent::AgentIdentity::PublicKey { key, .. } = id else { continue };
        let key: PublicKey = key;
        let hash = if key.algorithm().is_rsa() { rsa_hash } else { None };
        if let Ok(r) = h.authenticate_publickey_with(user, key, hash, &mut agent).await
            && r.success()
        {
            return true;
        }
    }
    false
}

/// The connection dropped after it was up (not a failed connect or login): worth reconnecting.
#[derive(Debug)]
pub struct Lost;

impl std::fmt::Display for Lost {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("connection lost")
    }
}

impl std::error::Error for Lost {}

pub enum Ctl {
    Input(Vec<u8>),
    Resize(u16, u16),
    /// Open an SFTP session on the existing SSH connection (SSH tabs only).
    Sftp(tokio::sync::oneshot::Sender<Result<russh_sftp::client::SftpSession>>),
    Close,
}

async fn open_sftp(h: &Handle<Client>) -> Result<russh_sftp::client::SftpSession> {
    let ch = h.channel_open_session().await?;
    ch.request_subsystem(true, "sftp").await?;
    Ok(russh_sftp::client::SftpSession::new(ch.into_stream()).await?)
}

/// SFTP on a connection of its own to `target`, through `jump`: a VNC tab's file panel, on the
/// desktop's host. The connection has to live as long as the SFTP session (files.rs `Fs::Sftp`).
pub async fn sftp_alone(target: &Endpoint, jump: Option<&Endpoint>, prompt: &Prompter) -> Result<(russh_sftp::client::SftpSession, Conn)> {
    let conn = connect(target, jump, &Options::carrier(), prompt, None).await.with_context(|| format!("SSH to {}", target.label()))?;
    let s = open_sftp(&conn.handle).await?;
    Ok((s, conn))
}

/// Interactive shell: pumps output into `sink` until the channel closes. `link` gets what was
/// negotiated and the round trip, pinged every few seconds.
pub async fn shell(
    ssh: &config::Ssh,
    term: &str,
    size: (u16, u16),
    prompt: &Prompter,
    link: Arc<Mutex<Link>>,
    mut rx: UnboundedReceiver<Ctl>,
    mut sink: impl FnMut(&[u8]),
) -> Result<()> {
    let opts = Options { link: Some(link.clone()), ..Options::of(Some(ssh)) };
    // The tab can close while this still connects (or waits on a host key or password question):
    // then it stops, and the question goes away with it (main.rs `prompt-gone`). What comes
    // meanwhile (typing, a resize, the file panel) is kept for when the shell is up.
    let mut early = vec![];
    let connecting = connect(&ssh.target, ssh.jump.as_ref(), &opts, prompt, None);
    tokio::pin!(connecting);
    let conn = loop {
        tokio::select! {
            c = &mut connecting => break Arc::new(c?),
            ctl = rx.recv() => match ctl {
                Some(Ctl::Close) | None => return Ok(()),
                Some(c) => early.push(c),
            },
        }
    };
    // Pings on the side: waiting for the answer here would hold up typing.
    let _pinger = {
        let conn = conn.clone();
        Task(tokio::spawn(async move {
            loop {
                let t = std::time::Instant::now();
                let ok = tokio::time::timeout(Duration::from_secs(10), conn.handle.send_ping()).await;
                link.lock().unwrap().rtt_ms = matches!(ok, Ok(Ok(()))).then(|| t.elapsed().as_millis() as u32);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }))
    };
    let mut ch = conn.handle.channel_open_session().await?;
    ch.request_pty(false, term, size.0 as u32, size.1 as u32, 0, 0, &[]).await?;
    if ssh.agent_forward {
        ch.agent_forward(false).await?;
    }
    ch.request_shell(false).await?;
    if !ssh.command.is_empty() {
        ch.data_bytes(format!("{}\n", ssh.command)).await?;
    }
    // The shell ended (logout, Ctrl+D) rather than the connection dropping.
    let mut exited = false;
    let mut early = early.into_iter();
    loop {
        // What came while connecting goes first, in order.
        if let Some(c) = early.next() {
            control(&mut ch, &conn.handle, c).await?;
            continue;
        }
        tokio::select! {
            msg = ch.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => sink(&data),
                Some(ChannelMsg::ExitStatus { .. } | ChannelMsg::ExitSignal { .. } | ChannelMsg::Close) => {
                    exited = true;
                    if matches!(msg, Some(ChannelMsg::Close)) {
                        break;
                    }
                }
                None if exited => break,
                None => return Err(Lost.into()),
                _ => {}
            },
            ctl = rx.recv() => match ctl {
                Some(Ctl::Close) | None => break, // tab closed
                Some(c) => control(&mut ch, &conn.handle, c).await?,
            },
        }
    }
    // An open SFTP panel holds channels on this connection too: end it outright.
    // Politely, but a server that doesn't answer gets dropped anyway.
    let bye = conn.handle.disconnect(russh::Disconnect::ByApplication, "", "en");
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), bye).await;
    Ok(())
}

/// Typing, a resize or the file panel's request, for an interactive shell's channel.
async fn control(ch: &mut russh::Channel<client::Msg>, h: &Handle<Client>, c: Ctl) -> Result<()> {
    match c {
        Ctl::Input(b) => ch.data_bytes(b).await.map_err(|_| Lost)?,
        Ctl::Resize(c, r) => ch.window_change(c as u32, r as u32, 0, 0).await.map_err(|_| Lost)?,
        Ctl::Sftp(reply) => {
            let _ = reply.send(open_sftp(h).await);
        }
        Ctl::Close => {} // the caller ends the session
    }
    Ok(())
}

/// Live counters for one tunnel, read by the UI's flow diagram.
#[derive(Default)]
pub struct Traffic {
    /// Bytes from the listening side towards the destination.
    pub fwd: AtomicU64,
    /// Bytes coming back.
    pub back: AtomicU64,
    pub active: AtomicUsize,
    pub total: AtomicU64,
    /// Most recent SOCKS destinations, newest last.
    pub targets: Mutex<Vec<String>>,
}

/// Copies both ways like `copy_bidirectional`, counting bytes. `a` is the listening side.
async fn pump(a: impl AsyncRead + AsyncWrite, b: impl AsyncRead + AsyncWrite, t: &Traffic) {
    async fn copy(mut r: impl AsyncRead + Unpin, mut w: impl AsyncWrite + Unpin, n: &AtomicU64) {
        // 64 KB: two SSH packets per read, fewer wakeups on a fast link.
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(len @ 1..) = r.read(&mut buf).await {
            if w.write_all(&buf[..len]).await.is_err() {
                return;
            }
            n.fetch_add(len as u64, Relaxed);
        }
        let _ = w.shutdown().await;
    }
    t.active.fetch_add(1, Relaxed);
    t.total.fetch_add(1, Relaxed);
    let (ar, aw) = tokio::io::split(a);
    let (br, bw) = tokio::io::split(b);
    tokio::join!(copy(ar, bw, &t.fwd), copy(br, aw, &t.back));
    t.active.fetch_sub(1, Relaxed);
}

/// Runs a tunnel until the task is aborted. `status` is updated for the UI. If the connection
/// drops after it was up (a mobile link going quiet, the server restarting), it connects again
/// by itself; the local port stays open meanwhile, so programs using it just wait. `jump`: its
/// login's SSH gateway.
pub async fn tunnel(
    t: Tunnel,
    jump: Option<Endpoint>,
    prompt: Prompter,
    traffic: Arc<Traffic>,
    status: impl Fn(String) + Send + Sync + 'static,
) -> Result<()> {
    let fwd = (t.kind == TunnelKind::Remote).then(|| (t.dest_host.clone(), t.dest_port, traffic.clone()));
    status("connecting…".into());
    let jump = jump.as_ref();
    let mut conn = connect(&t.ssh, jump, &Options::carrier(), &prompt, fwd.clone()).await?;
    if t.kind == TunnelKind::Remote {
        loop {
            conn.handle.tcpip_forward(t.bind.clone(), t.listen_port as u32).await?;
            status(format!("server {}:{} forwards to {}:{}", t.bind, t.listen_port, t.dest_host, t.dest_port));
            // Forwarded connections arrive via Client::server_channel_open_forwarded_tcpip.
            while !conn.handle.is_closed() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            conn = reconnect(&t.ssh, jump, &prompt, fwd.clone(), &status).await?;
        }
    }
    let listener = TcpListener::bind((t.bind.as_str(), t.listen_port)).await?;
    let dest = (t.kind != TunnelKind::Dynamic).then_some((t.dest_host, t.dest_port));
    let up = format!("listening on {}:{}", t.bind, t.listen_port);
    keep_serving(listener, (&t.ssh, jump), conn, &prompt, dest, traffic, |s| status(s.unwrap_or_else(|| up.clone()))).await
}

/// A local port that reaches a session's host through an SSH gateway (a shared login's).
/// Dropping it closes the port and the gateway connection.
pub struct Forward {
    pub port: u16,
    _task: Task,
}

/// Logs in to `gateway`, then forwards a port on 127.0.0.1 to `dest`, or with None, serves
/// SOCKS5 there (websites, whose pages reach many hosts). Reconnects like a tunnel.
pub async fn forward(gateway: &Endpoint, dest: Option<(String, u16)>, prompt: &Prompter) -> Result<Forward> {
    let conn = connect(gateway, None, &Options::carrier(), prompt, None).await.with_context(|| format!("SSH gateway {}", gateway.label()))?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let (gateway, prompt) = (gateway.clone(), prompt.clone());
    let task = tokio::spawn(async move {
        let _ = keep_serving(listener, (&gateway, None), conn, &prompt, dest, Default::default(), |_| {}).await;
    });
    Ok(Forward { port, _task: Task(task) })
}

/// Serves `listener` over `conn`, and over a new connection to `ssh` (through `jump`) whenever
/// that one drops.
/// `status`: None while serving, else what's happening.
async fn keep_serving(
    listener: TcpListener,
    (ssh, jump): (&Endpoint, Option<&Endpoint>),
    mut conn: Conn,
    prompt: &Prompter,
    dest: Option<(String, u16)>,
    traffic: Arc<Traffic>,
    status: impl Fn(Option<String>) + Sync,
) -> Result<()> {
    loop {
        status(None);
        serve(&listener, Arc::new(conn), dest.clone(), traffic.clone()).await?;
        conn = reconnect(ssh, jump, prompt, None, &|s| status(Some(s))).await?;
    }
}

/// Connects again after a drop, waiting 1, 2, 4… up to 30 s between tries, for as long as it
/// takes. A refused login ends it: trying again won't help, and would ask again.
async fn reconnect(
    ssh: &Endpoint,
    jump: Option<&Endpoint>,
    prompt: &Prompter,
    fwd: Option<(String, u16, Arc<Traffic>)>,
    status: &(dyn Fn(String) + Sync),
) -> Result<Conn> {
    let mut wait = 1;
    loop {
        status(format!("connection lost; connecting again in {wait} s…"));
        tokio::time::sleep(Duration::from_secs(wait)).await;
        status("connecting again…".into());
        match connect(ssh, jump, &Options::carrier(), prompt, fwd.clone()).await {
            Ok(c) => return Ok(c),
            Err(e) if refused(&e) => return Err(e),
            Err(_) => wait = (wait * 2).min(30),
        }
    }
}

/// The server turned the login down, or the user cancelled it.
fn refused(e: &anyhow::Error) -> bool {
    let s = format!("{e:#}");
    s.contains("authentication failed") || s.contains("cancelled")
}

/// Sends each connection to `listener` over `conn`: to `dest`, or with None, where its SOCKS5
/// request asks. Returns once `conn` has closed.
async fn serve(listener: &TcpListener, conn: Arc<Conn>, dest: Option<(String, u16)>, traffic: Arc<Traffic>) -> Result<()> {
    loop {
        // Wakes every second to notice a closed connection without waiting for a client.
        let (mut tcp, peer) = tokio::select! {
            a = listener.accept() => a?,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if conn.handle.is_closed() {
                    return Ok(());
                }
                continue;
            }
        };
        if conn.handle.is_closed() {
            return Ok(());
        }
        // Small writes (keystrokes, RDP input) go out at once.
        let _ = tcp.set_nodelay(true);
        let conn = conn.clone();
        let dest = dest.clone();
        let socks = dest.is_none();
        let traffic = traffic.clone();
        tokio::spawn(async move {
            let (host, port) = if let Some(d) = dest {
                d
            } else {
                match socks5_request(&mut tcp).await {
                    Ok((h, p)) => {
                        let mut recent = traffic.targets.lock().unwrap();
                        let target = format!("{h}:{p}");
                        recent.retain(|x| *x != target);
                        recent.push(target);
                        let excess = recent.len().saturating_sub(5);
                        recent.drain(..excess);
                        (h, p)
                    }
                    Err(_) => return,
                }
            };
            let ch = conn
                .handle
                .channel_open_direct_tcpip(host, port as u32, peer.ip().to_string(), peer.port() as u32)
                .await;
            if socks {
                let code = if ch.is_ok() { 0 } else { 5 };
                if tcp.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await.is_err() {
                    return;
                }
            }
            if let Ok(ch) = ch {
                pump(tcp, ch.into_stream(), &traffic).await;
            }
        });
    }
}

/// Minimal SOCKS5 server handshake (no auth, CONNECT only). Returns the requested destination.
async fn socks5_request<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S) -> Result<(String, u16)> {
    let mut hdr = [0u8; 2];
    s.read_exact(&mut hdr).await?;
    if hdr[0] != 5 {
        bail!("not socks5");
    }
    let mut methods = vec![0u8; hdr[1] as usize];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        s.write_all(&[5, 0xff]).await?;
        bail!("no acceptable auth method");
    }
    s.write_all(&[5, 0]).await?;
    let mut req = [0u8; 4];
    s.read_exact(&mut req).await?;
    if req[1] != 1 {
        s.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        bail!("only CONNECT is supported");
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        3 => {
            let len = s.read_u8().await? as usize;
            let mut d = vec![0u8; len];
            s.read_exact(&mut d).await?;
            String::from_utf8(d)?
        }
        4 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        _ => bail!("bad address type"),
    };
    let port = s.read_u16().await?;
    Ok((host, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn socks5_handshake() {
        let (mut client, mut server) = tokio::io::duplex(64);
        let srv = tokio::spawn(async move { socks5_request(&mut server).await.unwrap() });
        client.write_all(&[5, 1, 0]).await.unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [5, 0]);
        let mut req = vec![5, 1, 0, 3, 11];
        req.extend_from_slice(b"example.com");
        req.extend_from_slice(&443u16.to_be_bytes());
        client.write_all(&req).await.unwrap();
        assert_eq!(srv.await.unwrap(), ("example.com".into(), 443));
    }

    /// The trusted-keys store for SSH tests: one place and one key for all of them, as they run
    /// in parallel in one process.
    fn test_home() -> std::path::PathBuf {
        let home = std::env::temp_dir().join(format!("xtermium-test-{}", std::process::id()));
        // SAFETY: every test sets the same value.
        unsafe { std::env::set_var("XTERMIUM_HOME", &home) };
        config::set_key([7; 32]);
        home
    }

    /// An [`Echo`] server on a free port, offering `preferred`.
    async fn echo_server(preferred: russh::Preferred) -> u16 {
        let cfg = Arc::new(russh::server::Config {
            keys: vec![keys::PrivateKey::random(&mut rand::rng(), keys::Algorithm::Ed25519).unwrap()],
            preferred,
            ..Default::default()
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let cfg = cfg.clone();
                tokio::spawn(async move { russh::server::run_stream(cfg, sock, Echo::default()).await.unwrap().await });
            }
        });
        port
    }

    /// Trusts the host and logs in with "pw".
    fn yes_prompter() -> Prompter {
        let (p, rx) = Prompter::new();
        std::thread::spawn(move || {
            for req in rx {
                let a = match req.title.as_str() {
                    "Unknown host key" => Some((String::new(), false)),
                    "Password" => Some(("pw".to_string(), false)),
                    _ => None,
                };
                let _ = req.reply.send(a);
            }
        });
        p
    }

    /// Sends `n` bytes through an echo channel with `opts`; returns how long the round trip took.
    async fn echo_through(port: u16, opts: &Options, n: usize) -> Result<Duration> {
        let target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), key: String::new(), ..Default::default() };
        let conn = connect(&target, None, opts, &yes_prompter(), None).await?;
        let ch = conn.handle.channel_open_session().await?;
        let (mut r, mut w) = tokio::io::split(ch.into_stream());
        let t = std::time::Instant::now();
        let send = async {
            let chunk = vec![0x5au8; 64 * 1024];
            for _ in 0..n / chunk.len() {
                w.write_all(&chunk).await?;
            }
            anyhow::Ok(())
        };
        let recv = async {
            let mut buf = vec![0u8; 64 * 1024];
            let mut got = 0;
            while got < n / (64 * 1024) * (64 * 1024) {
                got += r.read(&mut buf).await?;
            }
            anyhow::Ok(())
        };
        tokio::try_join!(send, recv)?;
        Ok(t.elapsed())
    }

    /// A session's own cipher and compression are what gets negotiated: a server that only
    /// speaks AES-128-GCM with zlib works with them and refuses the defaults.
    #[tokio::test(flavor = "multi_thread")]
    async fn options_negotiate() {
        test_home();
        let only = russh::Preferred {
            cipher: Cow::Borrowed(&[cipher::AES_128_GCM]),
            compression: Cow::Borrowed(&[compression::ZLIB_LEGACY]),
            ..russh::Preferred::DEFAULT
        };
        let port = echo_server(only).await;
        let opts = |ciphers: &[&str], compression| Options {
            ciphers: ciphers.iter().map(|c| c.to_string()).collect(),
            keepalive: 5,
            compression,
            agent: false,
            link: None,
            carrier: false,
        };
        echo_through(port, &opts(&["aes128-gcm@openssh.com"], true), 1 << 20).await.unwrap();
        assert!(echo_through(port, &opts(&["chacha20-poly1305@openssh.com"], true), 1 << 16).await.is_err());
        assert!(echo_through(port, &opts(&["aes128-gcm@openssh.com"], false), 1 << 16).await.is_err());
    }

    /// Throughput per cipher, to choose the default order:
    /// `cargo test --release bench_ciphers -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn bench_ciphers() {
        test_home();
        let all: Vec<cipher::Name> = CIPHERS.iter().filter_map(|c| cipher::Name::try_from(*c).ok()).collect();
        let port = echo_server(russh::Preferred { cipher: Cow::Owned(all), ..russh::Preferred::DEFAULT }).await;
        for c in CIPHERS {
            for compression in [false, true] {
                let opts = Options { ciphers: vec![c.into()], keepalive: 0, compression, agent: false, link: None, carrier: true };
                let target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), key: String::new(), ..Default::default() };
                let conn = connect(&target, None, &opts, &yes_prompter(), None).await.unwrap();
                let ch = conn.handle.channel_open_session().await.unwrap();
                ch.request_subsystem(true, "sink").await.unwrap();
                let mut w = ch.into_stream();
                const N: u64 = 256 << 20;
                // Compressible like a terminal's output, not random.
                let chunk: Vec<u8> = (0..64 * 1024u32)
                    .map(|i| {
                        b"ls -la /var/log\n"[(i % 16) as usize] ^ (i / 997) as u8
                    })
                    .collect();
                let start = SUNK.load(Relaxed);
                let t = std::time::Instant::now();
                for _ in 0..N / chunk.len() as u64 {
                    w.write_all(&chunk).await.unwrap();
                }
                while SUNK.load(Relaxed) - start < N {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                println!("{c:32} zlib {compression:5}: {:5.0} MB/s", 256.0 / t.elapsed().as_secs_f64());
            }
        }
    }

    /// The status bar learns the negotiated cipher and key exchange, and a measured round trip.
    #[tokio::test(flavor = "multi_thread")]
    async fn reports_the_link() {
        let _home = test_home();
        let port = echo_server(russh::Preferred::DEFAULT).await;
        let target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), key: String::new(), ..Default::default() };
        let ssh = config::Ssh { target, ..Default::default() };
        let link: Arc<Mutex<Link>> = Default::default();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let l2 = link.clone();
        let sh = tokio::spawn(async move { shell(&ssh, "xterm", (80, 24), &yes_prompter(), l2, rx, |_| {}).await });
        for _ in 0..200 {
            if link.lock().unwrap().rtt_ms.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let l = link.lock().unwrap().clone();
        assert!(l.rtt_ms.is_some_and(|ms| ms < 1000), "a ping came back");
        assert!(CIPHERS.contains(&l.cipher.as_str()), "cipher {:?}", l.cipher);
        assert!(!l.kex.is_empty() && l.host_key.contains("ed25519"), "{} {}", l.kex, l.host_key);
        tx.send(Ctl::Close).unwrap();
        sh.await.unwrap().unwrap();
    }

    /// A shared login's password (Credentials) logs in without asking.
    #[tokio::test(flavor = "multi_thread")]
    async fn logs_in_with_a_shared_password() {
        let _home = test_home();
        let port = echo_server(russh::Preferred::DEFAULT).await;
        let (prompter, rx) = Prompter::new();
        let asked = Arc::new(std::sync::Mutex::new(vec![]));
        let a2 = asked.clone();
        std::thread::spawn(move || {
            for req in rx {
                a2.lock().unwrap().push(req.title.clone());
                let _ = req.reply.send((req.title == "Unknown host key").then(|| (String::new(), false)));
            }
        });
        let mut target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), ..Default::default() };
        target.password = Some("pw".into());
        let opts = Options::of(None);
        connect(&target, None, &opts, &prompter, None).await.expect("logged in");
        assert!(!asked.lock().unwrap().iter().any(|t| t == "Password"), "{:?}", asked.lock().unwrap());
    }

    /// A client asked the echo server for agent forwarding.
    static AGENT_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// The server may reach this computer's SSH agent only on a connection that asked for it.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_forwarding_only_when_asked() {
        let _home = test_home();
        let port = echo_server(russh::Preferred::DEFAULT).await;
        let target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), key: String::new(), ..Default::default() };
        let open = |agent_forward: bool| {
            let ssh = config::Ssh { target: target.clone(), agent_forward, ..Default::default() };
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let out = Arc::new(std::sync::Mutex::new(Vec::new()));
            let out2 = out.clone();
            tokio::spawn(async move {
                shell(&ssh, "xterm", (80, 24), &yes_prompter(), Default::default(), rx, move |b| out2.lock().unwrap().extend_from_slice(b)).await
            });
            (tx, out)
        };
        let (tx, out) = open(false);
        tx.send(Ctl::Input(b"\x01".to_vec())).unwrap();
        for _ in 0..200 {
            if !out.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(&out.lock().unwrap()[..], b"agent:refused");
        assert!(!AGENT_ASKED.load(Relaxed));
        let _ = tx.send(Ctl::Close);

        let (tx, _) = open(true);
        for _ in 0..200 {
            if AGENT_ASKED.load(Relaxed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(AGENT_ASKED.load(Relaxed), "asked the server to forward the agent");
        let _ = tx.send(Ctl::Close);
    }

    /// Bytes the echo server has received, for the benchmark.
    static SUNK: AtomicU64 = AtomicU64::new(0);

    /// Echo server: accepts password "pw", echoes shell and direct-tcpip data, serves an
    /// in-memory SFTP subsystem.
    #[derive(Default)]
    struct Echo {
        channels: std::collections::HashMap<russh::ChannelId, russh::Channel<russh::server::Msg>>,
        sftp: std::collections::HashSet<russh::ChannelId>,
    }

    impl russh::server::Handler for Echo {
        type Error = anyhow::Error;
        async fn subsystem_request(
            &mut self,
            id: russh::ChannelId,
            name: &str,
            s: &mut russh::server::Session,
        ) -> Result<()> {
            match (name, self.channels.remove(&id)) {
                // Takes data without echoing it (throughput benchmarks). The channel's handle is
                // dropped: russh also queues data there, and an unread queue stalls the server.
                ("sink", Some(_)) => {
                    self.sftp.insert(id);
                    s.channel_success(id)?;
                }
                ("sftp", Some(ch)) => {
                    self.sftp.insert(id);
                    s.channel_success(id)?;
                    russh_sftp::server::run(ch.into_stream(), MemFs::default()).await;
                }
                _ => s.channel_failure(id)?,
            }
            Ok(())
        }
        async fn agent_request(&mut self, _: russh::ChannelId, _: &mut russh::server::Session) -> Result<bool> {
            AGENT_ASKED.store(true, Relaxed);
            Ok(true)
        }
        async fn auth_password(&mut self, _: &str, pw: &str) -> Result<russh::server::Auth> {
            Ok(if pw == "pw" { russh::server::Auth::Accept } else { russh::server::Auth::reject() })
        }
        async fn channel_open_session(
            &mut self,
            ch: russh::Channel<russh::server::Msg>,
            reply: russh::server::ChannelOpenHandle,
            _: &mut russh::server::Session,
        ) -> Result<()> {
            self.channels.insert(ch.id(), ch);
            reply.accept().await;
            Ok(())
        }
        async fn channel_open_direct_tcpip(
            &mut self,
            _: russh::Channel<russh::server::Msg>,
            _: &str,
            _: u32,
            _: &str,
            _: u32,
            reply: russh::server::ChannelOpenHandle,
            _: &mut russh::server::Session,
        ) -> Result<()> {
            reply.accept().await;
            Ok(())
        }
        async fn data(&mut self, ch: russh::ChannelId, data: &[u8], s: &mut russh::server::Session) -> Result<()> {
            SUNK.fetch_add(data.len() as u64, Relaxed);
            if self.sftp.contains(&ch) {
                return Ok(());
            }
            if data == b"\x04" {
                // Ctrl+D at an empty prompt: the shell logs out, like a real one.
                s.exit_status_request(ch, 0)?;
                s.eof(ch)?;
                s.close(ch)?;
                return Ok(());
            }
            if data == b"\x01" {
                // Ctrl+A: try to reach the client's agent, and say whether that was allowed.
                let h = s.handle();
                tokio::spawn(async move {
                    let said: &[u8] = if h.channel_open_agent().await.is_ok() { b"agent:open" } else { b"agent:refused" };
                    let _ = h.data(ch, said.to_vec()).await;
                });
                return Ok(());
            }
            if data == b"\x05" {
                // Ctrl+E: the server goes away mid-session, like a dropped network.
                s.disconnect(russh::Disconnect::ByApplication, "gone", "en")?;
                return Ok(());
            }
            s.data(ch, data.to_vec())?;
            Ok(())
        }
    }

    /// Flat in-memory filesystem: enough SFTP for list / upload / download / delete.
    #[derive(Default)]
    struct MemFs {
        files: std::collections::BTreeMap<String, Vec<u8>>,
        listed: bool,
    }

    impl russh_sftp::server::Handler for MemFs {
        type Error = russh_sftp::protocol::StatusCode;
        fn unimplemented(&self) -> Self::Error {
            russh_sftp::protocol::StatusCode::OpUnsupported
        }
        async fn realpath(&mut self, id: u32, _: String) -> Result<russh_sftp::protocol::Name, Self::Error> {
            Ok(russh_sftp::protocol::Name { id, files: vec![russh_sftp::protocol::File::dummy("/")] })
        }
        async fn open(
            &mut self,
            id: u32,
            path: String,
            flags: russh_sftp::protocol::OpenFlags,
            _: russh_sftp::protocol::FileAttributes,
        ) -> Result<russh_sftp::protocol::Handle, Self::Error> {
            if flags.contains(russh_sftp::protocol::OpenFlags::CREATE) {
                self.files.insert(path.clone(), vec![]);
            } else if !self.files.contains_key(&path) {
                return Err(russh_sftp::protocol::StatusCode::NoSuchFile);
            }
            Ok(russh_sftp::protocol::Handle { id, handle: path })
        }
        async fn write(
            &mut self,
            id: u32,
            h: String,
            offset: u64,
            data: Vec<u8>,
        ) -> Result<russh_sftp::protocol::Status, Self::Error> {
            let f = self.files.get_mut(&h).ok_or(russh_sftp::protocol::StatusCode::NoSuchFile)?;
            let end = offset as usize + data.len();
            if f.len() < end {
                f.resize(end, 0);
            }
            f[offset as usize..end].copy_from_slice(&data);
            Ok(ok(id))
        }
        async fn read(
            &mut self,
            id: u32,
            h: String,
            offset: u64,
            len: u32,
        ) -> Result<russh_sftp::protocol::Data, Self::Error> {
            let f = self.files.get(&h).ok_or(russh_sftp::protocol::StatusCode::NoSuchFile)?;
            let start = (offset as usize).min(f.len());
            if start == f.len() {
                return Err(russh_sftp::protocol::StatusCode::Eof);
            }
            let end = (start + len as usize).min(f.len());
            Ok(russh_sftp::protocol::Data { id, data: f[start..end].to_vec() })
        }
        async fn fstat(&mut self, id: u32, h: String) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
            let attrs = russh_sftp::protocol::FileAttributes {
                size: self.files.get(&h).map(|f| f.len() as u64),
                ..Default::default()
            };
            Ok(russh_sftp::protocol::Attrs { id, attrs })
        }
        async fn close(&mut self, id: u32, _: String) -> Result<russh_sftp::protocol::Status, Self::Error> {
            Ok(ok(id))
        }
        async fn opendir(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Handle, Self::Error> {
            self.listed = false;
            Ok(russh_sftp::protocol::Handle { id, handle: path })
        }
        async fn readdir(&mut self, id: u32, _: String) -> Result<russh_sftp::protocol::Name, Self::Error> {
            if std::mem::replace(&mut self.listed, true) {
                return Err(russh_sftp::protocol::StatusCode::Eof);
            }
            let files = self
                .files
                .iter()
                .map(|(name, data)| {
                    let a = russh_sftp::protocol::FileAttributes {
                        size: Some(data.len() as u64),
                        permissions: Some(0o100644),
                        ..Default::default()
                    };
                    russh_sftp::protocol::File::new(name.trim_start_matches('/'), a)
                })
                .collect();
            Ok(russh_sftp::protocol::Name { id, files })
        }
        async fn remove(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Status, Self::Error> {
            self.files.remove(&path).ok_or(russh_sftp::protocol::StatusCode::NoSuchFile)?;
            Ok(ok(id))
        }
    }

    fn ok(id: u32) -> russh_sftp::protocol::Status {
        russh_sftp::protocol::Status {
            id,
            status_code: russh_sftp::protocol::StatusCode::Ok,
            error_message: "Ok".into(),
            language_tag: "en-US".into(),
        }
    }

    /// Ctrl+D logs out: the shell ends cleanly (its tab closes), not with "connection lost".
    #[tokio::test(flavor = "multi_thread")]
    async fn ctrl_d_ends_the_shell() {
        let _home = test_home();
        let port = echo_server(russh::Preferred::DEFAULT).await;
        let (prompter, rx) = Prompter::new();
        std::thread::spawn(move || {
            for req in rx {
                let a = match req.title.as_str() {
                    "Unknown host key" => Some((String::new(), false)),
                    "Password" => Some(("pw".into(), false)),
                    _ => None,
                };
                let _ = req.reply.send(a);
            }
        });
        let target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), key: String::new(), ..Default::default() };
        let ssh = config::Ssh { target, ..Default::default() };
        let run = |key: &'static [u8]| {
            let (ssh, prompter) = (ssh.clone(), prompter.clone());
            async move {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let sh = tokio::spawn(async move { shell(&ssh, "xterm", (80, 24), &prompter, Default::default(), rx, |_| {}).await });
                tx.send(Ctl::Input(key.to_vec())).unwrap();
                tokio::time::timeout(Duration::from_secs(10), sh).await.expect("shell still running").unwrap()
            }
        };
        let r = run(b"\x04").await;
        assert!(r.is_ok(), "{r:?}");
        // The server dropping the connection is "lost": the tab may reconnect by itself.
        let r = run(b"\x05").await;
        assert!(r.as_ref().is_err_and(|e| e.is::<Lost>()), "{r:?}");
    }

    /// End to end against an in-process server: host-key prompt, password auth
    /// (wrong first, then right), shell echo, and a local tunnel.
    #[tokio::test(flavor = "multi_thread")]
    async fn shell_and_tunnel_against_local_server() {
        let home = test_home();
        let port = echo_server(russh::Preferred::DEFAULT).await;

        let (prompter, rx) = Prompter::new();
        let seen = Arc::new(std::sync::Mutex::new(vec![]));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            let mut wrong_once = true;
            for req in rx {
                seen2.lock().unwrap().push(req.title.clone());
                let answer = match (req.title.as_str(), req.input) {
                    ("Unknown host key", None) => Some((String::new(), false)),
                    ("Password", _) if std::mem::take(&mut wrong_once) => Some(("nope".into(), false)),
                    ("Password", _) => Some(("pw".into(), false)),
                    _ => None, // e.g. passphrase for the developer's own ~/.ssh keys
                };
                let _ = req.reply.send(answer);
            }
        });

        let target = Endpoint { host: "127.0.0.1".into(), port, user: "tester".into(), key: String::new(), ..Default::default() };
        let ssh = config::Ssh { target: target.clone(), command: "hello".into(), ..Default::default() };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let out = Arc::new(std::sync::Mutex::new(Vec::new()));
        let out2 = out.clone();
        let sh = tokio::spawn(async move {
            shell(&ssh, "xterm", (80, 24), &prompter.clone(), Default::default(), rx, move |b| out2.lock().unwrap().extend_from_slice(b))
                .await
        });
        tx.send(Ctl::Input(b"ping".to_vec())).unwrap();
        for _ in 0..100 {
            if out.lock().unwrap().ends_with(b"hello\nping") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(&out.lock().unwrap()[..], b"hello\nping");

        // SFTP on the same connection: upload, list, download, delete.
        let (reply, sftp) = tokio::sync::oneshot::channel();
        tx.send(Ctl::Sftp(reply)).unwrap();
        let sftp = sftp.await.unwrap().unwrap();
        assert_eq!(sftp.canonicalize(".").await.unwrap(), "/");
        let payload: Vec<u8> = (0..100_000u32).map(|i| i as u8).collect();
        let mut f = sftp.create("/big.bin").await.unwrap();
        f.write_all(&payload).await.unwrap();
        f.shutdown().await.unwrap();
        let names: Vec<_> = sftp.read_dir("/").await.unwrap().map(|e| (e.file_name(), e.metadata().len())).collect();
        assert_eq!(names, vec![("big.bin".to_string(), 100_000)]);
        let mut back = vec![];
        sftp.open("/big.bin").await.unwrap().read_to_end(&mut back).await.unwrap();
        assert!(back == payload, "downloaded {} bytes", back.len());
        sftp.remove_file("/big.bin").await.unwrap();
        assert_eq!(sftp.read_dir("/").await.unwrap().count(), 0);

        tx.send(Ctl::Close).unwrap();
        sh.await.unwrap().unwrap();
        // Closing the tab ends the connection, even with the SFTP panel still holding a channel.
        let gone = tokio::time::timeout(Duration::from_secs(5), sftp.read_dir("/")).await;
        assert!(matches!(gone, Ok(Err(_))), "the connection outlived the tab");
        let titles = seen.lock().unwrap().clone();
        assert_eq!(titles.iter().filter(|t| *t == "Unknown host key").count(), 1, "{titles:?}");
        assert_eq!(titles.iter().filter(|t| *t == "Password").count(), 2, "{titles:?}");
        let known = config::Known::with(|k| k.ssh.get(&format!("127.0.0.1:{port}")).cloned()).unwrap();
        assert!(known.as_deref().is_some_and(|k| k.starts_with("ssh-ed25519 ")), "{known:?}");

        // Local tunnel: listener -> direct-tcpip -> echo. Host key is now known: no new prompt.
        let (p2, rx2) = Prompter::new();
        std::thread::spawn(move || {
            for req in rx2 {
                let a = (req.title == "Password").then(|| ("pw".to_string(), false));
                let _ = req.reply.send(a);
            }
        });
        let lp = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let t = Tunnel {
            name: "t".into(),
            kind: TunnelKind::Local,
            ssh: target.clone(),
            bind: "127.0.0.1".into(),
            listen_port: lp,
            dest_host: "x".into(),
            dest_port: 1,
            autostart: false,
        };
        let traffic = Arc::new(Traffic::default());
        let task = tokio::spawn(tunnel(t, None, p2.clone(), traffic.clone(), |_| {}));
        let mut c = loop {
            if let Ok(c) = TcpStream::connect(("127.0.0.1", lp)).await {
                break c;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        c.write_all(b"through the tunnel").await.unwrap();
        let mut buf = [0u8; 18];
        tokio::time::timeout(Duration::from_secs(5), c.read_exact(&mut buf)).await.unwrap().unwrap();
        assert_eq!(&buf, b"through the tunnel");
        // The counter is bumped right after the write the client just read, so give it a moment.
        for _ in 0..50 {
            if traffic.back.load(Relaxed) == 18 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!((traffic.fwd.load(Relaxed), traffic.back.load(Relaxed), traffic.active.load(Relaxed)), (18, 18, 1));
        task.abort();

        // A shared login's gateway: a port forwarded to one host (RDP, VNC), and SOCKS5 (websites).
        let read = async |c: &mut TcpStream, n: usize| {
            let mut buf = vec![0u8; n];
            tokio::time::timeout(Duration::from_secs(5), c.read_exact(&mut buf)).await.unwrap().unwrap();
            buf
        };
        let f = forward(&target, Some(("x".into(), 1)), &p2).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", f.port)).await.unwrap();
        c.write_all(b"via gateway").await.unwrap();
        assert_eq!(read(&mut c, 11).await, b"via gateway");
        let s = forward(&target, None, &p2).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).await.unwrap();
        c.write_all(&[5, 1, 0]).await.unwrap();
        assert_eq!(read(&mut c, 2).await, [5, 0]);
        c.write_all(&[5, 1, 0, 3, 1, b'x', 0, 1]).await.unwrap();
        assert_eq!(read(&mut c, 10).await[1], 0, "SOCKS5 CONNECT succeeds");
        c.write_all(b"socks").await.unwrap();
        assert_eq!(read(&mut c, 5).await, b"socks");
        // Dropping the forward closes its port.
        let port = s.port;
        drop(s);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err(), "the gateway port outlived its tab");

        // The link drops (a relay in between is cut): the forward connects again by itself and
        // its port, which stayed open, works again.
        let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rport = relay.local_addr().unwrap().port();
        let links: Arc<Mutex<Vec<tokio::task::AbortHandle>>> = Default::default();
        let links2 = links.clone();
        tokio::spawn(async move {
            while let Ok((mut a, _)) = relay.accept().await {
                let mut b = TcpStream::connect(("127.0.0.1", target.port)).await.unwrap();
                let copy = tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
                });
                links2.lock().unwrap().push(copy.abort_handle());
            }
        });
        let (p3, rx3) = Prompter::new();
        std::thread::spawn(move || {
            for req in rx3 {
                let a = match req.title.as_str() {
                    "Unknown host key" => Some((String::new(), false)),
                    "Password" => Some(("pw".to_string(), false)),
                    _ => None,
                };
                let _ = req.reply.send(a);
            }
        });
        let via = Endpoint { host: "127.0.0.1".into(), port: rport, user: "tester".into(), ..Default::default() };
        let f = forward(&via, Some(("x".into(), 1)), &p3).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", f.port)).await.unwrap();
        c.write_all(b"before").await.unwrap();
        assert_eq!(read(&mut c, 6).await, b"before");
        links.lock().unwrap().drain(..).for_each(|l| l.abort());
        let mut back = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let Ok(mut c) = TcpStream::connect(("127.0.0.1", f.port)).await else { continue };
            if c.write_all(b"after").await.is_err() {
                continue;
            }
            let mut buf = [0u8; 5];
            if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(1), c.read_exact(&mut buf)).await {
                back = &buf == b"after";
                break;
            }
        }
        assert!(back, "the gateway didn't come back after its link dropped");
        assert_eq!(links.lock().unwrap().len(), 1, "one new connection");
        let _ = std::fs::remove_dir_all(home);
    }
}
