//! Terminal Co-op: up to three people share tabs. The sync server only brings them together
//! (docs/api.md "Co-op rings": a passcode, the owner's yes, each person's address); the tabs
//! themselves travel between the apps over iroh, QUIC with hole punching and our own relay, end to
//! end encrypted between the two NodeIds (the owner's side accepts only NodeIds the server lists as
//! approved). Nothing here uses n0's relays or discovery: peers learn each other's address only from
//! the server, and the relay is `XTERMIUM_COOP_RELAY` or `https://relay.xtermium.com`.
//!
//! The ring is a star. The owner shares tabs and every viewer connects to the owner, on one QUIC
//! bi-stream for control (Hello, shares, resizes, cursors, the viewers' input) and one uni-stream
//! per shared tab for its frames, the bytes the tab's own window gets (`Tab::share`; a terminal's
//! leave out the owner's selection, drawn like everyone's from `Msg::Selected`). A viewer's backend (`Remote`)
//! hands them unchanged to a Tauri channel, so `Terminal.svelte` and
//! `Rdp.svelte` draw them with the decoders they have. The UI hears about everything through one
//! `coop` event (`Payload`), at most 20 times a second.
//!
//! ponytail: one ring per run at a time, and a viewer sees the owner's tabs at the owner's size
//! (letterboxed); frames are not compressed in transit (an RDP picture is raw RGBA; compress it if
//! a slow link makes the ring lag, a lagging viewer is resynced meanwhile, see `Queue`).

use crate::{App, Tab, rdp, sync, term, web};
use iroh::endpoint::{Connection, Incoming, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey, TransportAddr};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tauri::async_runtime::{JoinHandle, spawn};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, WeakUnboundedSender, unbounded_channel};

type Res<T> = Result<T, String>;

const ALPN: &[u8] = b"xtermium/coop/1";
const DEFAULT_RELAY: &str = "https://relay.xtermium.com";
/// A ring holds the owner and two more.
const MAX_PEERS: usize = 2;

const NEEDS_PRO: &str = "Terminal Co-op needs XTermium Pro. Subscribe at xtermium.com/#pricing.";
const SIGN_IN: &str = "Sign in to use Terminal Co-op.";
const IN_RING: &str = "You are in a ring already. Leave or end it first.";
const NO_RING: &str = "You aren't in a ring.";
const NOT_OWNER: &str = "Only the person who started the ring can do that.";
const SITES: &str = "Website tabs can be shared on Windows only.";
/// The sentence the server answers with for a ring that is over (a 404).
const GONE: &str = "That ring has ended.";

/// Largest control message (Hello, a viewer's input, cursors) and largest frame: a whole 8192 x 8192
/// RDP picture is the biggest thing a tab sends.
const MAX_CTL: usize = 1 << 20;
const MAX_FRAME: usize = (1 << 28) + (1 << 16);
/// How long a frame stream may hold unsent bytes before a slow viewer is resynced instead.
const BACKLOG: usize = 32 << 20;
/// The most a viewer may paste into a shared terminal tab, in bytes.
const MAX_PASTE: usize = 128 << 10;
/// The most of a selection a viewer's copy takes, in bytes (the rest is cut off, and they're told).
const MAX_COPY: usize = 512 << 10;
const TOO_LONG: &str = "That's too long to paste into a shared tab: 128 KB at most.";
/// The most text one of a viewer's key presses may type on a shared desktop (a character, or an
/// accent with its letter): longer would be a paste, which shared desktops don't take.
const KEY_TEXT: usize = 4;
const GONE_TAB: &str = "That tab isn't shared with you any more.";
/// What this app speaks of the protocol, sent after the Hello's own bytes (`encode`): 1 the first
/// version, 2 copy and paste in terminal tabs (`Msg::Copy`, `Msg::Selection`, `Hands::Paste`), 3
/// everyone's selection drawn for the ring (`Msg::Selected`), 4 one person at a time driving a
/// shared desktop (`Msg::Control`, `Msg::Ask`), 5 everyone's clicks drawn for the ring
/// (`Msg::Click`). An older app reads
/// the Hello and ignores the byte, but drops the connection over a message it doesn't know, so
/// those go only to an app that said 2 (`needs`).
const WIRE: u8 = 5;
const HELLO_WAIT: Duration = Duration::from_secs(10);
const PRO_EVERY: Duration = Duration::from_secs(5 * 60);
/// How often the owner's ring looks at how long the owner has been away (`idle_watch`).
const IDLE_EVERY: Duration = Duration::from_secs(60);
/// The UI hears of changes at most this often (20 Hz): cursors would flood it otherwise.
const EMIT_EVERY: Duration = Duration::from_millis(50);
/// How often the owner's pointer over a shared website or console is read from its page (15 Hz).
const POINT_EVERY: Duration = Duration::from_millis(66);

/// A picture message on a tab's channel that Co-op adds for websites: `[6][JPEG]`, the whole page.
/// (The others are rdp.rs': 2 size, 3 changed areas, 4 status, 5 ended, 7 pointer.)
pub const MSG_JPEG: u8 = 6;

// Why a connection was closed, in the QUIC close code.
const KICKED: u32 = 1;
const ENDED: u32 = 2;
const REFUSED: u32 = 3;
const LEFT: u32 = 4;

// ---- the wire: postcard messages behind a u32 length ----

/// What a shared tab is: a terminal's cells, or a picture.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Text,
    Picture,
}

/// A tab the owner shares, as viewers hear of it. `cols` × `rows` for text, `w` × `h` pixels for a
/// picture (0 while unknown: the picture's own size message says).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShareInfo {
    pub tab: u32,
    pub title: String,
    pub kind: Shape,
    pub cols: u32,
    pub rows: u32,
    pub w: u32,
    pub h: u32,
}

/// The first message of every connection, in both directions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hello {
    pub user_id: String,
    pub name: String,
    pub image_url: Option<String>,
    /// The protocol version the sender speaks (`WIRE`); travels after the Hello, see `encode`.
    #[serde(skip)]
    pub wire: u8,
}

/// What a viewer does to a shared tab. A desktop's input stays JSON (`rdp::Input` is an internally
/// tagged enum, which postcard can't read back).
#[derive(Serialize, Deserialize)]
pub enum Hands {
    Key(term::Key),
    Text(String),
    /// A pointer event with the size in pixels of the viewer's cells, so it lands on the same cell here.
    Mouse(term::Mouse, f32, f32),
    Desktop(Vec<u8>),
    /// The viewer's own clipboard text, pasted (`WIRE` 2).
    Paste(String),
}

#[derive(Serialize, Deserialize)]
pub enum Msg {
    Hello(Hello),
    Share(ShareInfo),
    Unshare {
        tab: u32,
    },
    /// The exact bytes the tab's channel carries (term.rs `MSG_FRAME`/`MSG_IMAGE`, rdp.rs, `MSG_JPEG`).
    Frame {
        tab: u32,
        bytes: Vec<u8>,
    },
    Input {
        tab: u32,
        input: Hands,
    },
    /// The owner's terminal changed size; viewers follow (no resizing from their side).
    Resize {
        tab: u32,
        cols: u32,
        rows: u32,
    },
    /// Where someone's pointer is over a tab: a cell, or a pixel of a picture. Negative: gone.
    Cursor {
        tab: u32,
        user_id: String,
        col: i32,
        row: i32,
    },
    /// A viewer's window opened: send the whole tab again.
    Refresh {
        tab: u32,
    },
    Kicked,
    Ended,
    /// A viewer's own selection in a shared text tab, from `a` to `b` (stable row, column, as the
    /// tab's frames number them; `block` and `unit` as `term::Tab::pick` takes them): the owner
    /// answers with `Selection` number `ask` (`WIRE` 2).
    Copy {
        ask: u32,
        tab: u32,
        a: At,
        b: At,
        block: bool,
        unit: u8,
    },
    /// The answer to `Copy` number `ask`: the selection's ends, widened to words or lines, and its
    /// text ("" when there is none or the owner's xtermium is locked); `cut` when it was longer
    /// than `MAX_COPY` (`WIRE` 2).
    Selection {
        ask: u32,
        a: At,
        b: At,
        text: String,
        cut: bool,
    },
    /// Someone's own selection on a shared text tab changed (None: they have none now), for
    /// everyone else to draw in their colour. A viewer sends theirs (the owner fills in who it is
    /// from) and the owner tells the others, with their own (`WIRE` 3).
    Selected {
        tab: u32,
        user_id: String,
        sel: Option<Sel>,
    },
    /// Who drives shared desktop `tab` (a picture tab): the viewer `user_id`, or the owner again
    /// when it is empty. The owner tells everyone at each change, and a viewer who joins (`WIRE` 4).
    Control {
        tab: u32,
        user_id: String,
    },
    /// From a viewer: they ask for control of desktop `tab` (`want`), or give it back or withdraw
    /// the ask. From the owner to a viewer: their ask was turned down (`WIRE` 4).
    Ask {
        tab: u32,
        want: bool,
    },
    /// Someone pressed a mouse button over a tab, at a cell or a pixel: drawn for the ring (`WIRE` 5;
    /// last, so older apps keep reading the messages before it).
    /// `button`: 0 left, 1 middle, 2 right.
    Click {
        tab: u32,
        user_id: String,
        col: i32,
        row: i32,
        button: u8,
    },
}

/// A cell of a shared text tab: stable row, column.
pub type At = (i64, u32);
/// A selection in a shared text tab: its ends, and whether it is a rectangle.
pub type Sel = (At, At, bool);

/// The owner's terminal's selection as the ring numbers it.
fn sel_of(((ar, ac), (br, bc), block): term::Picked) -> Sel {
    ((ar as i64, ac as u32), (br as i64, bc as u32), block)
}

/// The protocol version a peer needs to read `m` (see `WIRE`).
fn needs(m: &Msg) -> u8 {
    match m {
        Msg::Copy { .. } | Msg::Selection { .. } | Msg::Input { input: Hands::Paste(_), .. } => 2,
        Msg::Selected { .. } => 3,
        Msg::Control { .. } | Msg::Ask { .. } => 4,
        Msg::Click { .. } => 5,
        _ => 1,
    }
}

fn encode(m: &Msg) -> Res<Vec<u8>> {
    let mut body = postcard::to_allocvec(m).map_err(|e| e.to_string())?;
    if let Msg::Hello(h) = m {
        body.push(h.wire);
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// A message; a Hello's version is the byte after it, 1 when there is none (an app before `WIRE`).
fn decode(body: &[u8]) -> Res<Msg> {
    let (mut m, rest) = postcard::take_from_bytes::<Msg>(body).map_err(|e| e.to_string())?;
    if let Msg::Hello(h) = &mut m {
        h.wire = rest.first().copied().unwrap_or(1);
    }
    Ok(m)
}

async fn write_msg<W: AsyncWrite + Unpin>(w: &mut W, m: &Msg) -> Res<()> {
    w.write_all(&encode(m)?).await.map_err(|e| e.to_string())
}

/// The next message; None when the stream ended between two. Longer than `max` is refused before
/// anything of it is read into memory.
async fn read_msg<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> Res<Option<Msg>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > max {
        return Err("A message was bigger than allowed.".into());
    }
    let mut body = vec![0; n];
    r.read_exact(&mut body).await.map_err(|e| e.to_string())?;
    decode(&body).map(Some)
}

// ---- what the server says (docs/api.md "Co-op rings") and how we are addressed ----

/// An iroh address as the server stores it: the NodeId, the relay and the direct addresses.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub relay: Option<String>,
    pub addrs: Vec<String>,
}

impl Node {
    fn of(a: &EndpointAddr) -> Self {
        Self {
            id: a.id.to_string(),
            relay: a.relay_urls().next().map(|u| u.to_string()),
            addrs: a.ip_addrs().take(16).map(|s| s.to_string()).collect(),
        }
    }

    fn addr(&self) -> Res<EndpointAddr> {
        let id: EndpointId = self.id.parse().map_err(|_| "The ring's address isn't valid.".to_string())?;
        let mut a = EndpointAddr::new(id);
        if let Some(u) = self.relay.as_ref().and_then(|r| r.parse::<RelayUrl>().ok()) {
            a = a.with_relay_url(u);
        }
        Ok(a.with_addrs(self.addrs.iter().filter_map(|s| s.parse::<SocketAddr>().ok()).map(TransportAddr::Ip)))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
struct ApiMember {
    id: String,
    name: String,
    #[serde(default)]
    image: Option<String>,
    state: String,
    #[serde(default)]
    node: Option<Node>,
}

#[derive(Deserialize)]
struct ApiRing {
    owner: Person,
    members: Vec<ApiMember>,
    #[serde(default)]
    passcode: Option<String>,
    #[serde(default)]
    passcode_until: u64,
}

#[derive(Deserialize)]
struct ApiOwner {
    #[serde(flatten)]
    who: Person,
    #[serde(default)]
    node: Option<Node>,
}

#[derive(Deserialize)]
struct ApiJoin {
    state: String,
    owner: ApiOwner,
    #[serde(default)]
    members: Vec<ApiMember>,
}

// ---- rules that don't need a network (and so have tests) ----

/// The member an incoming connection is: its NodeId is listed as approved, and the Hello names that
/// row's user. Anything else is somebody who was not let in.
fn admit<'a>(members: &'a [ApiMember], node: &str, hello: &Hello) -> Result<&'a ApiMember, &'static str> {
    let m = members
        .iter()
        .find(|m| m.state == "approved" && m.node.as_ref().is_some_and(|n| n.id == node))
        .ok_or("That address isn't on the ring's list.")?;
    if m.id != hello.user_id {
        return Err("That person isn't the one the ring let in.");
    }
    Ok(m)
}

/// A typed passcode: spaces, dashes and case don't matter; 8 characters of the server's alphabet.
fn clean_passcode(typed: &str) -> Res<String> {
    let code: String = typed.chars().filter(|c| !c.is_whitespace() && *c != '-').flat_map(char::to_uppercase).collect();
    if code.is_empty() {
        return Err("Type the ring's passcode.".into());
    }
    if code.chars().count() != 8 || !code.chars().all(|c| "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(c)) {
        return Err("A passcode is 8 letters and digits, like A7K2M9QX.".into());
    }
    Ok(code)
}

/// Starting a ring, and every acceptance of someone into it, needs Pro.
fn needs_pro(p: &sync::Pro) -> Res<()> {
    if p.active { Ok(()) } else { Err(NEEDS_PRO.into()) }
}

/// The account was read and says Pro is off. A failed read (offline) says nothing, so a ring
/// doesn't end because the network dropped.
fn pro_lapsed(answer: &anyhow::Result<sync::Account>) -> bool {
    answer.as_ref().is_ok_and(|a| !a.pro.active)
}

/// A viewer's key may type, but not run the owner's own shortcuts: paste and copy (the owner's
/// clipboard), the scrollback keys (they move the owner's screen), or anything with the Meta key.
fn key_ok(k: &term::Key) -> bool {
    let c = k.key.to_lowercase();
    !(k.meta
        || (k.ctrl && k.shift && matches!(c.as_str(), "c" | "v"))
        || (k.shift && matches!(k.key.as_str(), "Insert" | "PageUp" | "PageDown")))
}

/// A paste a viewer may send into a terminal: not more than `MAX_PASTE` bytes.
fn paste_ok(text: &str) -> Res<()> {
    if text.len() > MAX_PASTE { Err(TOO_LONG.into()) } else { Ok(()) }
}

/// A selection cut to `MAX_COPY` bytes, at a character's edge, and whether it was cut.
fn clip(mut s: String) -> (String, bool) {
    let cut = s.len() > MAX_COPY;
    s.truncate(s.floor_char_boundary(MAX_COPY));
    (s, cut)
}

/// What a viewer may do to a desktop: keys, a key's text, the pointer and the wheel. Nothing to
/// or from a clipboard: not "paste" (the owner's clipboard), nor the shared clipboard, nor text
/// longer than a key types (a paste typed out). Nor the owner's window size or lock keys. (Files
/// have no way in at all: a desktop's input carries none.)
fn desktop_ok(ev: &rdp::Input) -> bool {
    match ev {
        rdp::Input::Paste | rdp::Input::Clip | rdp::Input::Resize { .. } | rdp::Input::Sync { .. } => false,
        rdp::Input::Text { text } => text.chars().count() <= KEY_TEXT,
        _ => true,
    }
}

/// Whether viewer `from` may drive a shared tab with `hands`. A terminal takes everyone's typing;
/// a desktop's keys and pointer only the one viewer the owner gave control to (`Hosted::controller`),
/// so two people never fight over one pointer. The owner's own input never comes this way.
fn may_drive(controller: Option<&str>, from: &str, hands: &Hands) -> bool {
    match hands {
        Hands::Desktop(_) => controller == Some(from),
        _ => true,
    }
}

/// A user id is what the server made: letters, digits, `_` and `-`. Checked before it goes in a path.
fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A title or name from the other side: one clean line, not too long.
fn line(s: &str) -> String {
    crate::clean::text(s).chars().take(120).collect()
}

// ---- the connection: Hello, control and frame streams ----

/// Exactly one relay: ours, or the one `XTERMIUM_COOP_RELAY` names for development.
fn relay() -> Res<RelayMode> {
    relay_of(std::env::var("XTERMIUM_COOP_RELAY").ok())
}

fn relay_of(named: Option<String>) -> Res<RelayMode> {
    let url = named.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_RELAY.into());
    let url: RelayUrl = url.trim().parse().map_err(|_| "XTERMIUM_COOP_RELAY isn't a web address.".to_string())?;
    Ok(RelayMode::custom([url]))
}

/// Tells the server the key this ring will use, before it is bound: the relay lets an endpoint in
/// only once the server knows it as a signed-in account's (`POST /v1/coop/node`, docs/sync.md).
async fn register(handle: &AppHandle, key: &SecretKey) -> Res<()> {
    let body = serde_json::json!({ "id": key.public().to_string() });
    call(handle, reqwest::Method::POST, "/v1/coop/node", Some(body)).await.map(|_| ()).map_err(|e| said(&e))
}

/// An endpoint with `key`, a key of its own for this ring, no discovery of any kind, and `mode`
/// for relays. Only the owner listens; a viewer refuses everything that dials it.
async fn bind(key: SecretKey, mode: RelayMode, listen: bool) -> Res<Endpoint> {
    let mut b = Endpoint::builder(presets::Minimal).secret_key(key).relay_mode(mode);
    if listen {
        b = b.alpns(vec![ALPN.to_vec()]);
    }
    b.bind().await.map_err(|e| format!("Co-op couldn't open a connection: {e}"))
}

/// Our address for the server: once the relay answered (a few seconds at most), else what we have.
async fn node_of(ep: &Endpoint) -> Node {
    let _ = tokio::time::timeout(Duration::from_secs(4), ep.online()).await;
    Node::of(&ep.addr())
}

/// The owner's end of the opening: the peer's Hello comes first, `admit` says whether that person
/// may be here, and ours answers.
async fn open_host(
    conn: &Connection,
    ours: &Hello,
    admit: impl FnOnce(&Hello) -> Result<(), &'static str>,
) -> Res<(SendStream, RecvStream, Hello)> {
    let late = || "Nobody said hello.".to_string();
    let (mut send, mut recv) =
        tokio::time::timeout(HELLO_WAIT, conn.accept_bi()).await.map_err(|_| late())?.map_err(|e| e.to_string())?;
    let first = tokio::time::timeout(HELLO_WAIT, read_msg(&mut recv, MAX_CTL)).await.map_err(|_| late())??;
    let Some(Msg::Hello(hello)) = first else { return Err("The first message wasn't a Hello.".into()) };
    admit(&hello)?;
    write_msg(&mut send, &Msg::Hello(ours.clone())).await?;
    Ok((send, recv, hello))
}

/// The viewer's end: we say hello first, the owner answers.
async fn open_guest(conn: &Connection, ours: &Hello) -> Res<(SendStream, RecvStream, Hello)> {
    let late = || "The owner didn't answer.".to_string();
    let (mut send, mut recv) =
        tokio::time::timeout(HELLO_WAIT, conn.open_bi()).await.map_err(|_| late())?.map_err(|e| e.to_string())?;
    write_msg(&mut send, &Msg::Hello(ours.clone())).await?;
    let first = tokio::time::timeout(HELLO_WAIT, read_msg(&mut recv, MAX_CTL)).await.map_err(|_| late())??;
    let Some(Msg::Hello(hello)) = first else { return Err("The owner's first message wasn't a Hello.".into()) };
    Ok((send, recv, hello))
}

/// Writes what is queued on a control stream, in order, until the stream or the queue ends.
fn control_writer(mut send: SendStream) -> UnboundedSender<Msg> {
    let (tx, mut rx) = unbounded_channel::<Msg>();
    spawn(async move {
        while let Some(m) = rx.recv().await {
            if write_msg(&mut send, &m).await.is_err() {
                break;
            }
        }
        let _ = send.finish();
    });
    tx
}

/// A viewer's queue of frame bytes for one tab. Back-pressure is the stream's flow control; but
/// frames are diffs, so when a viewer falls too far behind everything unsent is dropped and the
/// tab sends itself whole again (`frame_writer`): never a gap in the middle.
#[derive(Clone)]
struct Queue {
    tx: UnboundedSender<Vec<u8>>,
    pending: Arc<AtomicUsize>,
    lagged: Arc<AtomicBool>,
}

impl Queue {
    fn new() -> (Self, UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = unbounded_channel();
        (Self { tx, pending: Default::default(), lagged: Default::default() }, rx)
    }

    fn push(&self, bytes: Vec<u8>) {
        // Behind already: what follows is dropped too, until the writer has caught up with the
        // flag and asked for the tab again.
        if self.pending.load(Relaxed) > BACKLOG {
            self.lagged.store(true, Relaxed);
            return;
        }
        self.pending.fetch_add(bytes.len(), Relaxed);
        let _ = self.tx.send(bytes);
    }
}

/// One shared tab's frames to one viewer, on a uni-stream of its own. `again` asks the tab for a
/// whole frame; it runs when this viewer was too slow and its backlog was dropped.
async fn frame_writer(conn: Connection, tab: u32, mut rx: UnboundedReceiver<Vec<u8>>, q: Queue, again: impl Fn()) {
    let Ok(mut s) = conn.open_uni().await else { return };
    while let Some(bytes) = rx.recv().await {
        if q.lagged.swap(false, Relaxed) {
            while rx.try_recv().is_ok() {}
            q.pending.store(0, Relaxed);
            again();
            continue;
        }
        let len = bytes.len();
        let sent = write_msg(&mut s, &Msg::Frame { tab, bytes }).await;
        q.pending.fetch_sub(len.min(q.pending.load(Relaxed)), Relaxed);
        if sent.is_err() {
            return;
        }
    }
    let _ = s.finish();
}

/// Reads the owner's frame streams (one per shared tab) and hands each frame to `deliver`.
async fn read_frames(conn: Connection, deliver: impl Fn(u32, Vec<u8>) + Send + Sync + 'static) {
    let deliver = Arc::new(deliver);
    while let Ok(mut rx) = conn.accept_uni().await {
        let deliver = deliver.clone();
        spawn(async move {
            while let Ok(Some(Msg::Frame { tab, bytes })) = read_msg(&mut rx, MAX_FRAME).await {
                deliver(tab, bytes);
            }
        });
    }
}

/// The application close code a connection ended with, if the other side closed it on purpose.
async fn close_code(conn: &Connection) -> Option<u32> {
    let e = tokio::time::timeout(Duration::from_secs(1), conn.closed()).await.ok()?;
    match e {
        iroh::endpoint::ConnectionError::ApplicationClosed(a) => u32::try_from(u64::from(a.error_code)).ok(),
        _ => None,
    }
}

// ---- the ring ----

/// What the UI hears (`coop` event) and can ask for (`coop_state`). `ended` says why a ring just
/// went away ("pro", "kicked", "ended", "lost", ...); `message` is the sentence for the person.
#[derive(Serialize, Clone, Default)]
pub struct Payload {
    ring: Option<RingOut>,
    /// Your tabs you share, by id.
    shared: Vec<u32>,
    /// Tabs shared with you; `id` is the owner's number for the tab.
    remote: Vec<RemoteOut>,
    cursors: Vec<CursorOut>,
    /// The others' selections on shared text tabs.
    selections: Vec<SelOut>,
    /// Shared desktops a viewer drives, and who; any other shared desktop is the owner's to drive.
    control: Vec<ControlOut>,
    /// Asks for control of a desktop: the viewers asking (the owner's side), or your own (a viewer's).
    wants: Vec<ControlOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize, Clone)]
struct RingOut {
    id: String,
    /// The owner's passcode and when it stops working (unix seconds); null and 0 for a viewer.
    passcode: Option<String>,
    passcode_until: u64,
    owner: bool,
    /// "open" (yours), or as a viewer "pending" (waiting for the owner's yes), "connecting", "joined".
    state: &'static str,
    /// The owner first, then the others, those waiting and those removed too (an owner sees them).
    members: Vec<MemberOut>,
}

#[derive(Serialize, Clone)]
struct MemberOut {
    id: String,
    name: String,
    image: Option<String>,
    /// "pending", "approved", "kicked" (as the server says), "approved" for the owner.
    state: String,
    owner: bool,
    /// This is you.
    you: bool,
    /// Their app is connected to the owner's now.
    online: bool,
}

#[derive(Serialize, Clone)]
struct RemoteOut {
    id: u32,
    title: String,
    kind: Shape,
    cols: u32,
    rows: u32,
    w: u32,
    h: u32,
    owner_name: String,
}

#[derive(Serialize, Clone)]
struct SelOut {
    tab: u32,
    user_id: String,
    a: At,
    b: At,
    block: bool,
}

/// A viewer and a shared desktop: who drives it, or who asks to.
#[derive(Serialize, Clone)]
struct ControlOut {
    tab: u32,
    user_id: String,
}

#[derive(Serialize, Clone)]
struct CursorOut {
    tab: u32,
    user_id: String,
    name: String,
    image: Option<String>,
    col: i32,
    row: i32,
}

/// A click in the ring, as the UI hears it (the `coop-click` event, at once: it is drawn as it happens).
#[derive(Serialize, Clone)]
struct ClickOut {
    tab: u32,
    user_id: String,
    name: String,
    col: i32,
    row: i32,
    button: u8,
}

/// Where a shared tab's pictures come from: the tab's own frames (terminals, desktops), a website
/// taken by its view's DevTools, or a BMC console's screen read from its canvas.
#[derive(Clone, Copy, PartialEq)]
enum Feed {
    Tab,
    Site,
    Console,
}

/// A viewer's connection, seen from the owner.
struct Peer {
    person: Person,
    conn: Connection,
    ctl: UnboundedSender<Msg>,
    queues: HashMap<u32, Queue>,
    /// The protocol version their app speaks (`WIRE`).
    wire: u8,
}

/// The owner's connection, seen from a viewer.
struct Link {
    conn: Connection,
    ctl: UnboundedSender<Msg>,
    /// The protocol version the owner's app speaks (`WIRE`).
    wire: u8,
}

/// A tab the owner shares. `sink` is what the tab's frames go into (weak: when the tab goes, so
/// does the sink, and the fan-out ends); `force` tells a website's picture loop to send again.
struct Hosted {
    info: ShareInfo,
    sink: WeakUnboundedSender<Vec<u8>>,
    force: Arc<AtomicBool>,
    tasks: Vec<JoinHandle<()>>,
    /// The one viewer driving this desktop (`may_drive`); None: the owner. Always None for a
    /// text tab, where everyone types.
    controller: Option<String>,
}

/// A tab shared with us. Frames reach `channel` once the UI has opened it (`coop_open_remote`).
struct Remote {
    info: ShareInfo,
    channel: Arc<Mutex<Option<Channel>>>,
    /// Who drives this desktop, as the owner last said (`Msg::Control`); "" the owner.
    controller: String,
}

struct State {
    stage: &'static str,
    passcode: Option<(String, u64)>,
    me: Person,
    host: Person,
    /// The server's last word on who is in the ring (the owner sees every state, a viewer the approved).
    members: Vec<ApiMember>,
    peers: HashMap<String, Peer>,
    link: Option<Link>,
    hosted: BTreeMap<u32, Hosted>,
    remote: BTreeMap<u32, Remote>,
    cursors: HashMap<(u32, String), (i32, i32)>,
    /// Everyone's selections on shared text tabs, by tab and person (the owner's own among them,
    /// on the owner's side, to tell viewers who join).
    selections: HashMap<(u32, String), Sel>,
    /// A viewer's selections waiting for the owner's answer (`coop_remote_select`), by number.
    asks: HashMap<u32, tokio::sync::oneshot::Sender<Picked>>,
    next_ask: u32,
    /// Asks for control of a shared desktop, (tab, who): viewers asking, on the owner's side; the
    /// desktops we asked for (with our own id), on a viewer's.
    wants: BTreeSet<(u32, String)>,
}

struct Ring {
    handle: AppHandle,
    id: String,
    owner: bool,
    ep: Endpoint,
    /// Pro as last seen (when the ring started, then every 5 minutes): new people are let in only while it holds.
    pro: AtomicBool,
    over: AtomicBool,
    changed: Notify,
    st: Mutex<State>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

static CURRENT: Mutex<Option<Arc<Ring>>> = Mutex::new(None);
/// Starting or joining a ring takes a while (the network); one at a time.
static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn current() -> Option<Arc<Ring>> {
    CURRENT.lock().unwrap().clone()
}

fn token(handle: &AppHandle) -> Res<String> {
    let t = handle.state::<App>().settings.lock().unwrap().sync_token.trim().to_string();
    if t.is_empty() { Err(SIGN_IN.into()) } else { Ok(t) }
}

/// A call to the ring routes with the signed-in token.
async fn call(
    handle: &AppHandle,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    let t = token(handle).map_err(anyhow::Error::msg)?;
    sync::call("", &t, method, path, body).await
}

/// A failed call as the sentence for the person; whether the server was just away is `later`.
fn said(e: &anyhow::Error) -> String {
    format!("{e:#}")
}

fn later(e: &anyhow::Error) -> bool {
    matches!(e.downcast_ref::<sync::Trouble>(), Some(sync::Trouble::Later { .. } | sync::Trouble::Busy { .. }))
}

fn parse<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Res<T> {
    serde_json::from_value(v).map_err(|_| "The sync server's answer wasn't understood.".to_string())
}

impl Ring {
    /// The owner's ring starts "open" with Pro vouched for (the caller checked it); a viewer's
    /// waits for the owner's yes.
    fn new(handle: AppHandle, id: String, owner: bool, ep: Endpoint, me: Person, host: Person) -> Arc<Self> {
        Arc::new(Self {
            handle,
            id,
            owner,
            ep,
            pro: AtomicBool::new(owner),
            over: AtomicBool::new(false),
            changed: Notify::new(),
            st: Mutex::new(State {
                stage: if owner { "open" } else { "pending" },
                passcode: None,
                me,
                host,
                members: vec![],
                peers: HashMap::new(),
                link: None,
                hosted: BTreeMap::new(),
                remote: BTreeMap::new(),
                cursors: HashMap::new(),
                selections: HashMap::new(),
                asks: HashMap::new(),
                next_ask: 0,
                wants: BTreeSet::new(),
            }),
            tasks: Mutex::new(vec![]),
        })
    }

    fn st(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap()
    }

    /// Something the UI shows changed.
    fn notify(&self) {
        self.changed.notify_one();
    }

    fn keep(&self, task: JoinHandle<()>) {
        self.tasks.lock().unwrap().push(task);
    }

    fn hello(&self) -> Hello {
        let me = self.st().me.clone();
        Hello { user_id: me.id, name: me.name, image_url: me.image, wire: WIRE }
    }

    /// Everything the UI shows, as of now.
    fn payload(&self) -> Payload {
        let st = self.st();
        let who = |p: &Person| (p.name.clone(), p.image.clone());
        let mut people: HashMap<&str, (String, Option<String>)> = HashMap::new();
        people.insert(&st.host.id, who(&st.host));
        for m in &st.members {
            people.insert(&m.id, (m.name.clone(), m.image.clone()));
        }
        let mut members = vec![MemberOut {
            id: st.host.id.clone(),
            name: st.host.name.clone(),
            image: st.host.image.clone(),
            state: "approved".into(),
            owner: true,
            you: self.owner,
            online: true,
        }];
        for m in st.members.iter().filter(|m| m.id != st.host.id) {
            members.push(MemberOut {
                id: m.id.clone(),
                name: m.name.clone(),
                image: m.image.clone(),
                state: m.state.clone(),
                owner: false,
                you: !self.owner && m.id == st.me.id,
                online: st.peers.contains_key(&m.id) || (!self.owner && st.link.is_some()),
            });
        }
        let (passcode, passcode_until) = st.passcode.clone().map_or((None, 0), |(c, u)| (Some(c), u));
        let mut cursors: Vec<CursorOut> = st
            .cursors
            .iter()
            .filter_map(|((tab, user), (col, row))| {
                let (name, image) = people.get(user.as_str())?.clone();
                Some(CursorOut { tab: *tab, user_id: user.clone(), name, image, col: *col, row: *row })
            })
            .collect();
        cursors.sort_by(|a, b| (a.tab, &a.user_id).cmp(&(b.tab, &b.user_id)));
        let mut selections: Vec<SelOut> = st
            .selections
            .iter()
            .filter(|((_, user), _)| *user != st.me.id)
            .map(|((tab, user), (a, b, block))| SelOut { tab: *tab, user_id: user.clone(), a: *a, b: *b, block: *block })
            .collect();
        selections.sort_by(|a, b| (a.tab, &a.user_id).cmp(&(b.tab, &b.user_id)));
        let control = if self.owner {
            st.hosted.iter().filter_map(|(t, h)| h.controller.clone().map(|u| ControlOut { tab: *t, user_id: u })).collect()
        } else {
            st.remote
                .values()
                .filter(|r| !r.controller.is_empty())
                .map(|r| ControlOut { tab: r.info.tab, user_id: r.controller.clone() })
                .collect()
        };
        Payload {
            ring: Some(RingOut {
                id: self.id.clone(),
                passcode,
                passcode_until,
                owner: self.owner,
                state: st.stage,
                members,
            }),
            shared: st.hosted.keys().copied().collect(),
            remote: st
                .remote
                .values()
                .map(|r| RemoteOut {
                    id: r.info.tab,
                    title: r.info.title.clone(),
                    kind: r.info.kind,
                    cols: r.info.cols,
                    rows: r.info.rows,
                    w: r.info.w,
                    h: r.info.h,
                    owner_name: st.host.name.clone(),
                })
                .collect(),
            cursors,
            selections,
            control,
            wants: st.wants.iter().map(|(t, u)| ControlOut { tab: *t, user_id: u.clone() }).collect(),
            ended: None,
            message: None,
        }
    }

    /// The ring is over, for the reason given (None: the person ended or left it themselves). Closes
    /// every connection, stops sharing, tells the UI. Safe to call from any of the ring's own tasks:
    /// it does not wait for anything.
    fn finish(self: &Arc<Self>, reason: Option<&str>, message: &str) {
        if self.over.swap(true, Relaxed) {
            return;
        }
        {
            let mut cur = CURRENT.lock().unwrap();
            if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, self)) {
                *cur = None;
            }
        }
        let (peers, link, hosted) = {
            let mut st = self.st();
            st.remote.clear();
            st.cursors.clear();
            st.selections.clear();
            st.wants.clear();
            (std::mem::take(&mut st.peers), st.link.take(), std::mem::take(&mut st.hosted))
        };
        let code = if reason == Some("kicked") { KICKED } else { ENDED };
        for p in peers.values() {
            let _ = p.ctl.send(Msg::Ended);
            p.conn.close(code.into(), b"ended");
        }
        if let Some(l) = link {
            l.conn.close(LEFT.into(), b"left");
        }
        for (tab, h) in hosted {
            h.tasks.iter().for_each(JoinHandle::abort);
            with_tab(&self.handle, tab, |t| share_tab(t, None));
        }
        let _ = self.handle.emit_to(
            "main",
            "coop",
            &Payload {
                ended: reason.map(str::to_string),
                message: (!message.is_empty()).then(|| message.to_string()),
                ..Default::default()
            },
        );
        self.tasks.lock().unwrap().iter().for_each(JoinHandle::abort);
        let ep = self.ep.clone();
        spawn(async move { ep.close().await });
    }

    /// Tells the UI whenever something changed, at most 20 times a second.
    async fn emitter(self: Arc<Self>) {
        loop {
            self.changed.notified().await;
            let _ = self.handle.emit_to("main", "coop", &self.payload());
            tokio::time::sleep(EMIT_EVERY).await;
        }
    }

    // ---- the owner ----

    /// Asks the server who is in the ring now.
    async fn fetch_members(&self) -> anyhow::Result<()> {
        let v = call(&self.handle, reqwest::Method::GET, &format!("/v1/coop/rings/{}", self.id), None).await?;
        let r: ApiRing = serde_json::from_value(v)?;
        let mut st = self.st();
        st.host = r.owner;
        st.members = r.members;
        if let Some(code) = r.passcode {
            st.passcode = Some((code, r.passcode_until));
        }
        // Whoever the server no longer lists as approved (removed, or left) is cut off here too.
        let out: Vec<String> = st
            .peers
            .keys()
            .filter(|id| !st.members.iter().any(|m| &m.id == *id && m.state == "approved"))
            .cloned()
            .collect();
        let cut: Vec<(Peer, bool)> = out
            .into_iter()
            .filter_map(|id| {
                st.cursors.retain(|(_, u), _| *u != id);
                let kicked = st.members.iter().any(|m| m.id == id && m.state == "kicked");
                st.peers.remove(&id).map(|p| (p, kicked))
            })
            .collect();
        drop(st);
        for (p, kicked) in cut {
            let _ = p.ctl.send(if kicked { Msg::Kicked } else { Msg::Ended });
            p.conn.close(if kicked { KICKED } else { LEFT }.into(), b"removed");
            self.gone(&p.person.id);
        }
        self.notify();
        Ok(())
    }

    /// Accepts connections; each is greeted, and let in only if the server lists it as approved.
    async fn accept_loop(self: Arc<Self>) {
        while let Some(incoming) = self.ep.accept().await {
            let ring = self.clone();
            spawn(async move { ring.serve(incoming).await });
        }
    }

    async fn serve(self: Arc<Self>, incoming: Incoming) {
        let Ok(conn) = incoming.await else { return };
        // Whatever the reason, a refused peer is told nothing about the ring.
        if self.greet(&conn).await.is_err() {
            conn.close(REFUSED.into(), b"refused");
        }
    }

    /// One viewer's connection from its Hello to its end.
    async fn greet(self: &Arc<Self>, conn: &Connection) -> Res<()> {
        let node = conn.remote_id().to_string();
        if !self.pro.load(Relaxed) {
            return Err(NEEDS_PRO.into());
        }
        // A person approved a moment ago may not be in the last list yet: ask the server once.
        let known = |me: &Self| {
            me.st().members.iter().any(|m| m.state == "approved" && m.node.as_ref().is_some_and(|n| n.id == node))
        };
        if !known(self) {
            let _ = self.fetch_members().await;
        }
        let members = self.st().members.clone();
        let (send, mut recv, hello) = open_host(conn, &self.hello(), |h| admit(&members, &node, h).map(|_| ())).await?;
        let m = admit(&members, &node, &hello)?.clone();
        let person = Person { id: m.id, name: m.name, image: m.image };

        let ctl = control_writer(send);
        let shares = {
            let mut st = self.st();
            if self.over.load(Relaxed) || (!st.peers.contains_key(&person.id) && st.peers.len() >= MAX_PEERS) {
                return Err("The ring is full.".into());
            }
            let mut peer = Peer {
                person: person.clone(),
                conn: conn.clone(),
                ctl: ctl.clone(),
                queues: HashMap::new(),
                wire: hello.wire,
            };
            let tabs: Vec<ShareInfo> = st.hosted.values().map(|h| h.info.clone()).collect();
            for info in &tabs {
                self.add_queue(&mut peer, info.tab);
            }
            // The same person coming back (the network changed) replaces their old connection.
            if let Some(old) = st.peers.insert(person.id.clone(), peer) {
                old.conn.close(REFUSED.into(), b"replaced");
            }
            tabs
        };
        for info in &shares {
            let _ = ctl.send(Msg::Share(info.clone()));
        }
        // Every viewer starts from the same state as the owner's window: whole frames again,
        // everyone's selections, and who drives each desktop.
        for info in &shares {
            self.refresh(info.tab);
        }
        let sels: Vec<_> = self.st().selections.iter().map(|((t, u), s)| (*t, u.clone(), *s)).collect();
        for (tab, user_id, sel) in sels.into_iter().filter(|(_, u, _)| *u != person.id) {
            self.give(&person.id, Msg::Selected { tab, user_id, sel: Some(sel) });
        }
        let held: Vec<(u32, String)> =
            self.st().hosted.iter().filter_map(|(t, h)| h.controller.clone().map(|u| (*t, u))).collect();
        for (tab, user_id) in held {
            self.give(&person.id, Msg::Control { tab, user_id });
        }
        self.notify();

        let stable = conn.stable_id();
        while let Ok(Some(m)) = read_msg(&mut recv, MAX_CTL).await {
            self.on_peer(&person, m);
        }
        let mut st = self.st();
        if st.peers.get(&person.id).is_some_and(|p| p.conn.stable_id() == stable) {
            st.peers.remove(&person.id);
            st.cursors.retain(|(_, u), _| *u != person.id);
            let gone: Vec<_> = st.peers.values().map(|p| p.ctl.clone()).collect();
            let tabs: Vec<u32> = st.hosted.keys().copied().collect();
            drop(st);
            for c in gone {
                for tab in &tabs {
                    let _ = c.send(Msg::Cursor { tab: *tab, user_id: person.id.clone(), col: -1, row: -1 });
                }
            }
            self.gone(&person.id);
        }
        Ok(())
    }

    /// Who drives shared desktop `tab` from now: viewer `user` (connected), or the owner again when
    /// `user` is empty. Everyone in the ring hears, and the ask this answers is done.
    fn control(&self, tab: u32, user: &str) -> Res<()> {
        {
            let mut st = self.st();
            if !user.is_empty() && !st.peers.contains_key(user) {
                return Err("That person isn't connected to the ring.".into());
            }
            let h = st.hosted.get_mut(&tab).ok_or("That tab isn't shared.")?;
            if h.info.kind != Shape::Picture {
                return Err("Only a shared desktop has control to give.".into());
            }
            h.controller = (!user.is_empty()).then(|| user.to_string());
            st.wants.retain(|(t, u)| !(*t == tab && u == user));
        }
        self.tell(tab, user);
        self.notify();
        Ok(())
    }

    /// Tells every viewer who drives desktop `tab` now ("" the owner).
    fn tell(&self, tab: u32, user_id: &str) {
        let ids: Vec<String> = self.st().peers.keys().cloned().collect();
        for id in ids {
            self.give(&id, Msg::Control { tab, user_id: user_id.into() });
        }
    }

    /// Viewer `user` asks for control of desktop `tab` (`want`), or gives it back or withdraws the
    /// ask. The owner's window shows the ask; giving it back needs no yes.
    fn asked(&self, tab: u32, user: &str, want: bool) {
        let holds = {
            let mut st = self.st();
            let Some(h) = st.hosted.get(&tab).filter(|h| h.info.kind == Shape::Picture) else { return };
            let holds = h.controller.as_deref() == Some(user);
            if want && !holds {
                st.wants.insert((tab, user.to_string()));
            } else {
                st.wants.remove(&(tab, user.to_string()));
            }
            holds
        };
        if !want && holds {
            let _ = self.control(tab, "");
        }
        self.notify();
    }

    /// Turns down viewer `user`'s ask for control of desktop `tab`; they're told.
    fn decline(&self, tab: u32, user: &str) {
        self.st().wants.remove(&(tab, user.to_string()));
        self.give(user, Msg::Ask { tab, want: false });
        self.notify();
    }

    /// `user`'s selection on shared tab `tab` changed (None: gone): kept, and told to everyone else
    /// in the ring who can read it. Nothing while xtermium is locked.
    fn selected(&self, tab: u32, user: &str, sel: Option<Sel>) {
        if crate::lock::is_locked() {
            return;
        }
        let others: Vec<String> = {
            let mut st = self.st();
            if !st.hosted.contains_key(&tab) {
                return;
            }
            let key = (tab, user.to_string());
            match sel {
                Some(s) => st.selections.insert(key, s),
                None => st.selections.remove(&key),
            };
            st.peers.keys().filter(|id| *id != user).cloned().collect()
        };
        for id in others {
            self.give(&id, Msg::Selected { tab, user_id: user.into(), sel });
        }
        self.notify();
    }

    /// Someone left or was taken out: their selections go, here and for the others, the desktops
    /// they drove are the owner's again, and their asks are forgotten.
    fn gone(&self, user: &str) {
        let (tabs, ids, drove) = {
            let mut st = self.st();
            let tabs: Vec<u32> = st.selections.keys().filter(|(_, u)| u == user).map(|(t, _)| *t).collect();
            st.selections.retain(|(_, u), _| u != user);
            st.wants.retain(|(_, u)| u != user);
            let drove: Vec<u32> = st
                .hosted
                .iter_mut()
                .filter(|(_, h)| h.controller.as_deref() == Some(user))
                .map(|(t, h)| {
                    h.controller = None;
                    *t
                })
                .collect();
            (tabs, st.peers.keys().cloned().collect::<Vec<_>>(), drove)
        };
        for tab in tabs {
            for id in &ids {
                self.give(id, Msg::Selected { tab, user_id: user.into(), sel: None });
            }
        }
        for tab in drove {
            self.tell(tab, "");
        }
        self.notify();
    }

    /// A new frame stream for one viewer and one tab.
    fn add_queue(self: &Arc<Self>, peer: &mut Peer, tab: u32) {
        let (q, rx) = Queue::new();
        peer.queues.insert(tab, q.clone());
        let ring = self.clone();
        spawn(frame_writer(peer.conn.clone(), tab, rx, q, move || ring.refresh(tab)));
    }

    /// What a verified viewer sent. Their user id is the one the server gave their address, never
    /// one they wrote.
    fn on_peer(&self, from: &Person, m: Msg) {
        match m {
            Msg::Input { tab, input } => self.drive(tab, &from.id, input),
            Msg::Ask { tab, want } => self.asked(tab, &from.id, want),
            Msg::Copy { ask, tab, a, b, block, unit } => {
                let ((a, b), text) = self.pick(tab, (a, b), block, unit);
                let (text, cut) = clip(text.unwrap_or_default());
                self.give(&from.id, Msg::Selection { ask, a, b, text, cut });
            }
            Msg::Refresh { tab } => self.refresh(tab),
            Msg::Selected { tab, sel, .. } => self.selected(tab, &from.id, sel),
            Msg::Click { tab, col, row, button, .. } => {
                if !self.st().hosted.contains_key(&tab) {
                    return;
                }
                self.clicked(tab, &from.id, col, row, button);
            }
            Msg::Cursor { tab, col, row, .. } => {
                let mut st = self.st();
                if !st.hosted.contains_key(&tab) {
                    return;
                }
                if col < 0 || row < 0 {
                    st.cursors.remove(&(tab, from.id.clone()));
                } else {
                    st.cursors.insert((tab, from.id.clone()), (col, row));
                }
                let others: Vec<_> =
                    st.peers.values().filter(|p| p.person.id != from.id).map(|p| p.ctl.clone()).collect();
                drop(st);
                for c in others {
                    let _ = c.send(Msg::Cursor { tab, user_id: from.id.clone(), col, row });
                }
                self.notify();
            }
            _ => {}
        }
    }

    /// Sends a viewer `m`, if their app can read it.
    fn give(&self, user: &str, m: Msg) {
        if let Some(p) = self.st().peers.get(user).filter(|p| p.wire >= needs(&m)) {
            let _ = p.ctl.send(m);
        }
    }

    /// Who `id` is, as the ring knows them: the owner, a member, or a stranger with no name.
    fn person(&self, id: &str) -> (String, Option<String>) {
        let st = self.st();
        if st.host.id == id {
            return (st.host.name.clone(), st.host.image.clone());
        }
        st.members.iter().find(|m| m.id == id).map(|m| (m.name.clone(), m.image.clone())).unwrap_or_default()
    }

    /// Someone clicked over shared tab `tab` (the owner's side): everyone else hears, and the UI draws it.
    fn clicked(&self, tab: u32, user: &str, col: i32, row: i32, button: u8) {
        let others: Vec<String> = self.st().peers.keys().filter(|p| *p != user).cloned().collect();
        for id in others {
            self.give(&id, Msg::Click { tab, user_id: user.into(), col, row, button });
        }
        self.show_click(tab, user, col, row, button);
    }

    /// Tells the UI of a click, to draw at once (not in the payload: it is a moment, not a state).
    fn show_click(&self, tab: u32, user: &str, col: i32, row: i32, button: u8) {
        if user == self.st().me.id {
            return;
        }
        let (name, _) = self.person(user);
        let _ = self.handle.emit_to("main", "coop-click", &ClickOut { tab, user_id: user.into(), name, col, row, button });
    }

    /// Where the owner's own pointer is over a shared tab (negative: gone): every viewer hears.
    fn my_cursor(&self, tab: u32, col: i32, row: i32) {
        let st = self.st();
        if !st.hosted.contains_key(&tab) {
            return;
        }
        let me = st.me.id.clone();
        for p in st.peers.values() {
            let _ = p.ctl.send(Msg::Cursor { tab, user_id: me.clone(), col, row });
        }
    }

    /// The owner's own click over a shared tab: every viewer whose app draws clicks hears.
    fn my_click(&self, tab: u32, col: i32, row: i32, button: u8) {
        let me = self.st().me.id.clone();
        if !self.st().hosted.contains_key(&tab) {
            return;
        }
        let ids: Vec<String> = self.st().peers.keys().cloned().collect();
        for id in ids {
            self.give(&id, Msg::Click { tab, user_id: me.clone(), col, row, button });
        }
    }

    /// A viewer's own selection in a shared text tab, widened and read (`term::Tab::pick`); the
    /// tab's own selection stays as it is. Nothing while xtermium is locked.
    fn pick(&self, tab: u32, (a, b): (At, At), block: bool, unit: u8) -> ((At, At), Option<String>) {
        let none = ((a, b), None);
        if crate::lock::is_locked() || !self.st().hosted.contains_key(&tab) {
            return none;
        }
        let cell = |(r, c): At| (r.clamp(isize::MIN as i64, isize::MAX as i64) as isize, c as usize);
        let at = |(r, c): (isize, usize)| (r as i64, c as u32);
        with_tab(&self.handle, tab, |t| match t {
            Tab::Term(t) => {
                let ((a, b), text) = t.pick(cell(a), cell(b), block, unit);
                ((at(a), at(b)), text)
            }
            _ => none.clone(),
        })
        .unwrap_or(none)
    }

    /// Viewer `from` types, pastes or clicks in a shared tab: the owner's own paths for it, unless the
    /// owner has locked xtermium, or it is a desktop someone else drives (`may_drive`). Never through
    /// broadcast, never to or from the owner's clipboard, never the owner's selection.
    fn drive(&self, tab: u32, from: &str, hands: Hands) {
        if crate::lock::is_locked() {
            return;
        }
        let controller = match self.st().hosted.get(&tab) {
            Some(h) => h.controller.clone(),
            None => return,
        };
        if !may_drive(controller.as_deref(), from, &hands) {
            return;
        }
        // A website or BMC console tab: its view takes the input (web.rs), not a tab of `tabs`.
        if let (Hands::Desktop(json), Some(console)) = (&hands, web::console(tab)) {
            let Ok(ev) = serde_json::from_slice::<rdp::Input>(json).map_err(|_| ()).and_then(|e| desktop_ok(&e).then_some(e).ok_or(())) else { return };
            if console {
                let _ = web::console_input(&self.handle, tab, &[ev]);
            } else {
                let handle = self.handle.clone();
                spawn(async move {
                    let _ = web::site_input(&handle, tab, ev).await;
                });
            }
            return;
        }
        let app = self.handle.state::<App>();
        let mut tabs = app.tabs.lock().unwrap();
        match (tabs.get_mut(&tab), hands) {
            (Some(Tab::Term(t)), Hands::Key(k)) => {
                // A session that ended asks for R to restart: that stays with the owner.
                if t.alive()
                    && key_ok(&k)
                    && let Some(ev) = t.key(&k)
                {
                    t.apply(&[ev]);
                }
            }
            (Some(Tab::Term(t)), Hands::Text(s)) => t.apply(&term::typed(&s.chars().take(4096).collect::<String>())),
            (Some(Tab::Term(t)), Hands::Paste(s)) if paste_ok(&s).is_ok() => t.paste(&s),
            (Some(Tab::Term(t)), Hands::Mouse(m, cw, ch)) => t.mouse_remote(&m, (cw, ch)),
            (Some(Tab::Rdp(t)), Hands::Desktop(json)) => {
                if let Ok(ev) = serde_json::from_slice::<rdp::Input>(&json)
                    .map_err(|_| ())
                    .and_then(|e| desktop_ok(&e).then_some(e).ok_or(()))
                {
                    t.input(ev);
                }
            }
            (Some(Tab::Vnc(t)), Hands::Desktop(json)) => {
                if let Ok(ev) = serde_json::from_slice::<rdp::Input>(&json)
                    .map_err(|_| ())
                    .and_then(|e| desktop_ok(&e).then_some(e).ok_or(()))
                {
                    t.input(ev);
                }
            }
            _ => {}
        }
    }

    /// Sends a shared tab whole again to everyone watching it.
    fn refresh(&self, tab: u32) {
        let (sink, force) = {
            let st = self.st();
            match st.hosted.get(&tab) {
                Some(h) => (h.sink.upgrade(), h.force.clone()),
                None => return,
            }
        };
        force.store(true, Relaxed);
        if let Some(sink) = sink {
            with_tab(&self.handle, tab, |t| share_tab(t, Some(sink)));
        }
    }

    /// Starts sharing a tab: viewers are told, each gets a stream, the tab starts feeding the sink.
    async fn share(self: &Arc<Self>, tab: u32) -> Res<()> {
        if self.st().hosted.contains_key(&tab) {
            return Ok(());
        }
        let (info, feed) = describe(&self.handle, tab)?;
        let (tx, rx) = unbounded_channel::<Vec<u8>>();
        let force = Arc::new(AtomicBool::new(true));
        let mut tasks = vec![spawn(self.clone().fan_out(tab, rx))];
        if feed != Feed::Tab {
            tasks.push(spawn(self.clone().shoot(tab, tx.clone(), force.clone(), feed)));
            tasks.push(spawn(self.clone().point(tab)));
        }
        let ctls = {
            let mut st = self.st();
            // Two requests for the same tab at once: the second one gives up.
            if st.hosted.contains_key(&tab) {
                tasks.iter().for_each(JoinHandle::abort);
                return Ok(());
            }
            st.hosted.insert(tab, Hosted { info: info.clone(), sink: tx.downgrade(), force, tasks, controller: None });
            let mut ctls = vec![];
            let ring = self.clone();
            for p in st.peers.values_mut() {
                ring.add_queue(p, tab);
                ctls.push(p.ctl.clone());
            }
            ctls
        };
        for c in ctls {
            let _ = c.send(Msg::Share(info.clone()));
        }
        if feed == Feed::Tab {
            with_tab(&self.handle, tab, |t| share_tab(t, Some(tx)));
        }
        self.notify();
        Ok(())
    }

    /// Stops sharing a tab (or, when it closed, notices that it did).
    fn unshare(&self, tab: u32) {
        let (h, ctls) = {
            let mut st = self.st();
            let Some(h) = st.hosted.remove(&tab) else { return };
            for p in st.peers.values_mut() {
                p.queues.remove(&tab);
            }
            st.cursors.retain(|(t, _), _| *t != tab);
            st.selections.retain(|(t, _), _| *t != tab);
            st.wants.retain(|(t, _)| *t != tab);
            (h, st.peers.values().map(|p| p.ctl.clone()).collect::<Vec<_>>())
        };
        h.tasks.iter().for_each(JoinHandle::abort);
        with_tab(&self.handle, tab, |t| share_tab(t, None));
        for c in ctls {
            let _ = c.send(Msg::Unshare { tab });
        }
        self.notify();
    }

    /// Hands a tab's frames to every viewer's queue. Ends when the tab is gone.
    async fn fan_out(self: Arc<Self>, tab: u32, mut rx: UnboundedReceiver<Vec<u8>>) {
        while let Some(bytes) = rx.recv().await {
            // The owner's selection, told by the terminal (term.rs `MSG_SELECTION`): for the ring.
            if bytes.first() == Some(&term::MSG_SELECTION) {
                if let Ok(picked) = serde_json::from_slice::<Option<term::Picked>>(&bytes[1..]) {
                    let me = self.st().me.id.clone();
                    self.selected(tab, &me, picked.map(sel_of));
                }
                continue;
            }
            let queues: Vec<Queue> = self.st().peers.values().filter_map(|p| p.queues.get(&tab).cloned()).collect();
            for q in queues {
                q.push(bytes.clone());
            }
        }
        self.unshare(tab);
    }

    /// A website or console tab has no frames of its own: its picture is taken (a website through
    /// its view's DevTools, Windows only; a console from its screen's canvas) as often, and as
    /// finely, as Settings › Terminal Co-op says, and sent when it changed, as `MSG_JPEG`.
    async fn shoot(self: Arc<Self>, tab: u32, tx: UnboundedSender<Vec<u8>>, force: Arc<AtomicBool>, feed: Feed) {
        let (mut last, mut failed) = (vec![], 0);
        loop {
            let (every, quality) = self.handle.state::<App>().settings.lock().unwrap().coop_quality.stream();
            tokio::time::sleep(every).await;
            let shot = match feed {
                Feed::Console => web::console_jpeg(&self.handle, tab, quality).await,
                _ => site_jpeg(&self.handle, tab, quality).await,
            };
            match shot {
                Ok(Some(jpeg)) => {
                    failed = 0;
                    if force.swap(false, Relaxed) || jpeg != last {
                        let mut m = Vec::with_capacity(1 + jpeg.len());
                        m.push(MSG_JPEG);
                        m.extend_from_slice(&jpeg);
                        if tx.send(m).is_err() {
                            return;
                        }
                        last = jpeg;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    failed += 1;
                    if failed >= 8 {
                        return self.unshare(tab);
                    }
                }
            }
        }
    }

    /// The owner's own pointer over a shared website or console, and their clicks, read from the
    /// page (web.rs `pointer`) and sent on as everyone else's are: the view is native, so the window
    /// itself never sees the pointer over it. Ends with the share (`Hosted::tasks`).
    async fn point(self: Arc<Self>, tab: u32) {
        let mut last: Option<(i32, i32)> = None;
        loop {
            tokio::time::sleep(POINT_EVERY).await;
            let Ok(seen) = web::pointer(&self.handle, tab).await else { continue };
            let at = seen.as_ref().and_then(|p| p.at).unwrap_or((-1, -1));
            if last != Some(at) {
                last = Some(at);
                self.my_cursor(tab, at.0, at.1);
            }
            for (x, y, button) in seen.map(|p| p.clicks).unwrap_or_default() {
                self.my_click(tab, x, y, button);
            }
        }
    }

    /// The owner's terminal changed size: viewers follow.
    fn resized(&self, tab: u32, cols: usize, rows: usize) {
        let mut st = self.st();
        let Some(h) = st.hosted.get_mut(&tab) else { return };
        if h.info.kind != Shape::Text || (h.info.cols, h.info.rows) == (cols as u32, rows as u32) {
            return;
        }
        (h.info.cols, h.info.rows) = (cols as u32, rows as u32);
        let ctls: Vec<_> = st.peers.values().map(|p| p.ctl.clone()).collect();
        drop(st);
        for c in ctls {
            let _ = c.send(Msg::Resize { tab, cols: cols as u32, rows: rows as u32 });
        }
    }

    /// Checks Pro every five minutes and that the ring still exists; Pro gone ends the ring.
    async fn watch(self: Arc<Self>) {
        loop {
            tokio::time::sleep(PRO_EVERY).await;
            let Ok(t) = token(&self.handle) else {
                return self.finish(Some("signed-out"), "You were signed out, so the ring ended.");
            };
            let account = sync::account("", &t).await;
            if pro_lapsed(&account) {
                self.pro.store(false, Relaxed);
                let _ = call(&self.handle, reqwest::Method::DELETE, &format!("/v1/coop/rings/{}", self.id), None).await;
                return self.finish(Some("pro"), "The ring ended because your XTermium Pro isn't active any more.");
            }
            if account.is_ok() {
                self.pro.store(true, Relaxed);
            }
            if let Err(e) = self.fetch_members().await
                && said(&e) == GONE
            {
                return self.finish(Some("ended"), GONE);
            }
        }
    }

    /// Ends the ring once the owner has been away from the mouse and keyboard for as long as
    /// Settings › Account and sync › Terminal Co-op allows: a ring left open is a way in. The
    /// setting is read each minute, so a change applies to the ring that is on.
    async fn idle_watch(self: Arc<Self>) {
        loop {
            tokio::time::sleep(IDLE_EVERY).await;
            let minutes = self.handle.state::<App>().settings.lock().unwrap().coop_idle;
            if idle_out(minutes, idle()) {
                let _ = call(&self.handle, reqwest::Method::DELETE, &format!("/v1/coop/rings/{}", self.id), None).await;
                let long = match minutes {
                    60 => "an hour".to_string(),
                    1440 => "a day".to_string(),
                    m => format!("{} hours", m / 60),
                };
                return self.finish(Some("idle"), &format!("The ring ended because you were away for {long}."));
            }
        }
    }

    // ---- a viewer ----

    /// Waits for the owner's yes, then connects and stays up to date until the ring ends.
    async fn follow(self: Arc<Self>) {
        let mut tried = false;
        loop {
            let polled = call(&self.handle, reqwest::Method::GET, &format!("/v1/coop/join/{}", self.id), None).await;
            let join = match polled
                .map_err(|e| (later(&e), said(&e)))
                .and_then(|v| serde_json::from_value::<ApiJoin>(v).map_err(|_| (true, String::new())))
            {
                Ok(j) => j,
                Err((true, _)) => {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                Err((false, why)) => return self.finish(Some("ended"), &why),
            };
            match join.state.as_str() {
                "kicked" => return self.finish(Some("kicked"), "The owner removed you from the ring."),
                "ended" => return self.finish(Some("ended"), "The owner ended the ring."),
                "approved" => {
                    let ours = self.ep.id().to_string();
                    {
                        let mut st = self.st();
                        st.host = join.owner.who.clone();
                        if let Some(m) = join.members.iter().find(|m| m.node.as_ref().is_some_and(|n| n.id == ours)) {
                            st.me = Person { id: m.id.clone(), name: m.name.clone(), image: m.image.clone() };
                        }
                        st.members = join.members;
                    }
                    if !tried {
                        tried = true;
                        self.st().stage = "connecting";
                        self.notify();
                        if let Err(why) = self.connect(&join.owner).await {
                            return self.finish(Some("lost"), &why);
                        }
                    }
                    self.notify();
                }
                _ => {
                    self.st().host = join.owner.who;
                    self.notify();
                }
            }
            let joined = self.st().link.is_some();
            tokio::time::sleep(Duration::from_secs(if joined { 5 } else { 2 })).await;
        }
    }

    /// Dials the owner (three tries) and starts listening to them.
    async fn connect(self: &Arc<Self>, owner: &ApiOwner) -> Res<()> {
        let unreachable = || format!("Couldn't reach {}. Their xtermium may be closed or offline.", owner.who.name);
        if self.st().me.id.is_empty() {
            return Err("xtermium couldn't tell which of the ring's people you are.".into());
        }
        let addr = owner.node.as_ref().ok_or_else(unreachable)?.addr()?;
        for _ in 0..3 {
            if let Ok(Ok(conn)) =
                tokio::time::timeout(Duration::from_secs(15), self.ep.connect(addr.clone(), ALPN)).await
            {
                return self.link(conn, owner).await;
            }
            if self.over.load(Relaxed) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Err(unreachable())
    }

    async fn link(self: &Arc<Self>, conn: Connection, owner: &ApiOwner) -> Res<()> {
        let (send, recv, hello) = open_guest(&conn, &self.hello()).await?;
        if hello.user_id != owner.who.id {
            conn.close(REFUSED.into(), b"not the owner");
            return Err("That isn't the ring's owner, so xtermium didn't join.".into());
        }
        let ctl = control_writer(send);
        {
            let mut st = self.st();
            st.link = Some(Link { conn: conn.clone(), ctl, wire: hello.wire });
            st.stage = "joined";
        }
        let ring = self.clone();
        self.keep(spawn(read_frames(conn.clone(), move |tab, bytes| ring.deliver(tab, bytes))));
        self.keep(spawn(self.clone().listen(conn, recv)));
        self.notify();
        Ok(())
    }

    /// The owner's control messages.
    async fn listen(self: Arc<Self>, conn: Connection, mut recv: RecvStream) {
        while let Ok(Some(m)) = read_msg(&mut recv, MAX_CTL).await {
            match m {
                Msg::Share(i) => {
                    let info = ShareInfo {
                        title: line(&i.title),
                        cols: i.cols.clamp(1, 1000),
                        rows: i.rows.clamp(1, 1000),
                        w: i.w.min(16384),
                        h: i.h.min(16384),
                        ..i
                    };
                    // The window already showing a tab (a Share sent again) keeps its channel.
                    self.st()
                        .remote
                        .entry(info.tab)
                        .and_modify(|r| r.info = info.clone())
                        .or_insert_with(|| Remote { info, channel: Default::default(), controller: String::new() });
                    self.notify();
                }
                Msg::Unshare { tab } => {
                    let mut st = self.st();
                    st.remote.remove(&tab);
                    st.cursors.retain(|(t, _), _| *t != tab);
                    st.selections.retain(|(t, _), _| *t != tab);
                    st.wants.retain(|(t, _)| *t != tab);
                    drop(st);
                    self.notify();
                }
                // Who drives a desktop now. Given to us, our ask is answered; given to someone
                // else, it still waits with the owner.
                Msg::Control { tab, user_id } => {
                    let mut st = self.st();
                    let me = st.me.id.clone();
                    if let Some(r) = st.remote.get_mut(&tab) {
                        r.controller = line(&user_id);
                    }
                    if user_id == me {
                        st.wants.remove(&(tab, me));
                    }
                    drop(st);
                    self.notify();
                }
                // The owner turned our ask down.
                Msg::Ask { tab, .. } => {
                    let mut st = self.st();
                    let me = st.me.id.clone();
                    st.wants.remove(&(tab, me));
                    drop(st);
                    self.notify();
                }
                Msg::Resize { tab, cols, rows } => {
                    if let Some(r) = self.st().remote.get_mut(&tab) {
                        (r.info.cols, r.info.rows) = (cols.clamp(1, 1000), rows.clamp(1, 1000));
                    }
                    self.notify();
                }
                Msg::Click { tab, user_id, col, row, button } => {
                    if self.st().remote.contains_key(&tab) {
                        self.show_click(tab, &user_id, col, row, button);
                    }
                }
                Msg::Cursor { tab, user_id, col, row } => {
                    let mut st = self.st();
                    if user_id != st.me.id && st.remote.contains_key(&tab) {
                        if col < 0 || row < 0 {
                            st.cursors.remove(&(tab, user_id));
                        } else {
                            st.cursors.insert((tab, user_id), (col, row));
                        }
                    }
                    drop(st);
                    self.notify();
                }
                Msg::Selected { tab, user_id, sel } => {
                    let mut st = self.st();
                    if user_id != st.me.id && st.remote.contains_key(&tab) {
                        match sel {
                            Some(s) => st.selections.insert((tab, user_id), s),
                            None => st.selections.remove(&(tab, user_id)),
                        };
                    }
                    drop(st);
                    self.notify();
                }
                // The answer to one of our selections (`coop_remote_select` waits for it).
                Msg::Selection { ask, a, b, text, cut } => {
                    if let Some(tx) = self.st().asks.remove(&ask) {
                        let _ = tx.send(Picked { a, b, text, cut });
                    }
                }
                Msg::Kicked => return self.finish(Some("kicked"), "The owner removed you from the ring."),
                Msg::Ended => return self.finish(Some("ended"), "The owner ended the ring."),
                _ => {}
            }
        }
        if self.over.load(Relaxed) {
            return;
        }
        match close_code(&conn).await {
            Some(KICKED) => self.finish(Some("kicked"), "The owner removed you from the ring."),
            Some(ENDED) => self.finish(Some("ended"), "The owner ended the ring."),
            _ => self.finish(Some("lost"), "The connection to the ring was lost."),
        }
    }

    /// A frame from the owner for a remote tab, on to the window showing it (once the UI opened it).
    fn deliver(&self, tab: u32, bytes: Vec<u8>) {
        let channel = self.st().remote.get(&tab).map(|r| r.channel.clone());
        if let Some(c) = channel.as_ref().and_then(|c| c.lock().unwrap().clone()) {
            let _ = c.send(InvokeResponseBody::Raw(bytes));
        }
    }

    /// Something to the owner, if we are connected and their app can read it.
    fn to_owner(&self, m: Msg) -> Res<()> {
        let st = self.st();
        let l = st.link.as_ref().ok_or(GONE_TAB)?;
        if l.wire < needs(&m) {
            return Err(format!("{} needs a newer xtermium for that.", st.host.name));
        }
        l.ctl.send(m).map_err(|_| GONE_TAB.to_string())
    }
}

// ---- the tabs ----

/// Runs `f` on the tab with this id, if it is open.
fn with_tab<R>(handle: &AppHandle, id: u32, f: impl FnOnce(&Tab) -> R) -> Option<R> {
    handle.state::<App>().tabs.lock().unwrap().get(&id).map(f)
}

fn share_tab(t: &Tab, sink: Option<UnboundedSender<Vec<u8>>>) {
    match t {
        Tab::Term(t) => t.share(sink),
        Tab::Rdp(t) => t.share(sink),
        Tab::Vnc(t) => t.share(sink),
    }
}

/// What sharing tab `id` would show viewers, and where its pictures come from. Every kind of tab
/// shares, but websites off Windows (no way to take their picture there yet).
fn describe(handle: &AppHandle, id: u32) -> Res<(ShareInfo, Feed)> {
    let picture = |title: &str, (w, h): (usize, usize)| ShareInfo {
        tab: id,
        title: line(title),
        kind: Shape::Picture,
        cols: 0,
        rows: 0,
        w: w as u32,
        h: h as u32,
    };
    let app = handle.state::<App>();
    let tabs = app.tabs.lock().unwrap();
    match tabs.get(&id) {
        Some(Tab::Term(t)) => {
            let title = Some(t.title()).filter(|s| !s.is_empty()).unwrap_or_else(|| t.session.name.clone());
            let (cols, rows) = t.grid().0;
            Ok((
                ShareInfo {
                    tab: id,
                    title: line(&title),
                    kind: Shape::Text,
                    cols: cols as u32,
                    rows: rows as u32,
                    w: 0,
                    h: 0,
                },
                Feed::Tab,
            ))
        }
        Some(Tab::Rdp(t)) => Ok((picture(&t.session.name, t.size()), Feed::Tab)),
        Some(Tab::Vnc(t)) => Ok((picture(&t.session.name, (0, 0)), Feed::Tab)),
        None => match web::console(id) {
            // Not `web::listed`: it asks every view for its address, which waits for the main thread
            // while `tabs` is held here (a command on the main thread wanting `tabs` would deadlock it).
            Some(false) if cfg!(windows) => Ok((picture(&web::title_of(id).unwrap_or_default(), (0, 0)), Feed::Site)),
            Some(false) => Err(SITES.into()),
            Some(true) => Ok((picture(&web::title_of(id).unwrap_or_default(), (0, 0)), Feed::Console)),
            None => Err("That tab isn't open.".into()),
        },
    }
}

/// A website tab's page as a JPEG (None while the tab is out of sight). WebView2's DevTools, so
/// Windows only; `describe` refuses websites elsewhere before this is ever asked.
async fn site_jpeg(handle: &AppHandle, id: u32, quality: u8) -> Res<Option<Vec<u8>>> {
    #[cfg(windows)]
    {
        web::page_jpeg(handle, id, quality).await
    }
    #[cfg(not(windows))]
    {
        let _ = (handle, id, quality);
        Err(SITES.into())
    }
}

/// A tab closed: it stops being shared (main.rs `close_tab`, web.rs `web_close`).
pub fn forget(id: u32) {
    if let Some(r) = current().filter(|r| r.owner) {
        r.unshare(id);
    }
}

/// The owner's terminal changed size (main.rs `term_resize`).
pub fn resized(id: u32, cols: usize, rows: usize) {
    if let Some(r) = current().filter(|r| r.owner) {
        r.resized(id, cols, rows);
    }
}

// ---- commands ----

fn host() -> Res<Arc<Ring>> {
    let r = current().ok_or(NO_RING)?;
    if r.owner { Ok(r) } else { Err(NOT_OWNER.into()) }
}

fn viewer() -> Res<Arc<Ring>> {
    current().filter(|r| !r.owner).ok_or_else(|| NO_RING.to_string())
}

/// Starts a ring (needs Pro): the passcode to give out comes back in the payload.
#[tauri::command]
pub async fn coop_create(handle: AppHandle) -> Res<Payload> {
    let _one = BUSY.lock().await;
    if current().is_some() {
        return Err(IN_RING.into());
    }
    let t = token(&handle)?;
    let account = sync::account("", &t).await.map_err(crate::fail)?;
    needs_pro(&account.pro)?;
    let key = SecretKey::generate();
    register(&handle, &key).await?;
    let ep = bind(key, relay()?, true).await?;
    let made = async {
        let node = node_of(&ep).await;
        let v = call(&handle, reqwest::Method::POST, "/v1/coop/rings", Some(serde_json::json!({ "node": node })))
            .await
            .map_err(|e| said(&e))?;
        let id =
            v["id"].as_str().filter(|s| id_ok(s)).ok_or("The sync server's answer wasn't understood.")?.to_string();
        let code = v["passcode"].as_str().unwrap_or_default().to_string();
        let until = v["passcode_until"].as_u64().unwrap_or(0);
        let info: ApiRing = parse(
            call(&handle, reqwest::Method::GET, &format!("/v1/coop/rings/{id}"), None).await.map_err(|e| said(&e))?,
        )?;
        Ok::<_, String>((id, code, until, info))
    }
    .await;
    let (id, code, until, info) = match made {
        Ok(m) => m,
        Err(e) => {
            ep.close().await;
            return Err(e);
        }
    };
    let me = info.owner.clone();
    let ring = Ring::new(handle, id, true, ep, me, info.owner);
    {
        let mut st = ring.st();
        st.passcode = Some((code, until));
        st.members = info.members;
    }
    ring.keep(spawn(ring.clone().accept_loop()));
    ring.keep(spawn(ring.clone().watch()));
    ring.keep(spawn(ring.clone().idle_watch()));
    ring.keep(spawn(ring.clone().emitter()));
    *CURRENT.lock().unwrap() = Some(ring.clone());
    ring.notify();
    Ok(ring.payload())
}

/// When the owner last used xtermium, in seconds since 1970 (`coop_active`): what `idle` goes by
/// where the system doesn't say when it last had input.
static ACTIVE: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The owner moved the mouse or pressed a key in xtermium (the UI says so at most once a minute
/// while its ring is on, on the systems `idle` needs it).
#[tauri::command]
pub fn coop_active() {
    ACTIVE.store(now_secs(), Relaxed);
}

/// How long this computer has had no mouse or keyboard input, in any program.
#[cfg(windows)]
fn idle() -> Duration {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut last = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // No answer: not away, so a ring is never ended on a guess.
    if !unsafe { GetLastInputInfo(&mut last) }.as_bool() {
        return Duration::ZERO;
    }
    // Both count milliseconds since the system started and wrap together, after 49 days.
    Duration::from_millis(unsafe { GetTickCount() }.wrapping_sub(last.dwTime).into())
}

/// How long this Mac has had no mouse or keyboard input, in any program.
#[cfg(target_os = "macos")]
fn idle() -> Duration {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event: u32) -> f64;
    }
    // 0: the login session's combined state; !0: any kind of input event.
    let secs = unsafe { CGEventSourceSecondsSinceLastEventType(0, !0) };
    Duration::try_from_secs_f64(secs).unwrap_or(Duration::ZERO)
}

/// How long xtermium itself has had no mouse or keyboard input (`coop_active`).
/// ponytail: Linux has no one call for the whole desktop's idle time (X11 and each Wayland
/// compositor differ), so input in other programs doesn't count here; ask the desktop
/// (org.freedesktop.ScreenSaver, ext-idle-notify) if rings end too early for people.
#[cfg(not(any(windows, target_os = "macos")))]
fn idle() -> Duration {
    Duration::from_secs(now_secs().saturating_sub(ACTIVE.load(Relaxed)))
}

/// Away for as long as the setting allows (`minutes`; 0: never too long).
fn idle_out(minutes: u32, idle: Duration) -> bool {
    minutes > 0 && idle >= Duration::from_secs(u64::from(minutes) * 60)
}

/// The owner's dialog polls this every 2 s: who asks to join, and the passcode.
#[tauri::command]
pub async fn coop_refresh() -> Res<Payload> {
    let ring = host()?;
    if let Err(e) = ring.fetch_members().await {
        if said(&e) == GONE {
            ring.finish(Some("ended"), GONE);
            return Err(GONE.into());
        }
        if !later(&e) {
            return Err(said(&e));
        }
    }
    Ok(ring.payload())
}

/// A fresh passcode (the old one stops working).
#[tauri::command]
pub async fn coop_passcode() -> Res<Payload> {
    let ring = host()?;
    let v = call(
        &ring.handle,
        reqwest::Method::POST,
        &format!("/v1/coop/rings/{}/passcode", ring.id),
        Some(serde_json::json!({})),
    )
    .await
    .map_err(|e| said(&e))?;
    let code = v["passcode"].as_str().ok_or("The sync server's answer wasn't understood.")?.to_string();
    ring.st().passcode = Some((code, v["passcode_until"].as_u64().unwrap_or(0)));
    ring.notify();
    Ok(ring.payload())
}

async fn decide(user: &str, state: &str) -> Res<Payload> {
    let ring = host()?;
    if !id_ok(user) {
        return Err("That person isn't in the ring.".into());
    }
    if state == "approved" && !ring.pro.load(Relaxed) {
        return Err(NEEDS_PRO.into());
    }
    let path = format!("/v1/coop/rings/{}/members/{user}", ring.id);
    call(&ring.handle, reqwest::Method::POST, &path, Some(serde_json::json!({ "state": state })))
        .await
        .map_err(|e| said(&e))?;
    let _ = ring.fetch_members().await;
    Ok(ring.payload())
}

/// Lets in a person who asked (the server refuses a fourth person).
#[tauri::command]
pub async fn coop_approve(user: String) -> Res<Payload> {
    decide(&user, "approved").await
}

/// Turns down a person who asked; they can't ask again for this ring.
#[tauri::command]
pub async fn coop_decline(user: String) -> Res<Payload> {
    decide(&user, "kicked").await
}

/// Removes a person from the ring: their connection closes and they can't come back.
#[tauri::command]
pub async fn coop_kick(user: String) -> Res<Payload> {
    let payload = decide(&user, "kicked").await?;
    let ring = host()?;
    let peer = ring.st().peers.remove(&user);
    if let Some(p) = peer {
        let _ = p.ctl.send(Msg::Kicked);
        p.conn.close(KICKED.into(), b"kicked");
        ring.st().cursors.retain(|(_, u), _| *u != user);
        ring.gone(&user);
    }
    Ok(payload)
}

/// Gives viewer `user` control of your shared desktop `tab` (keys and pointer; nobody else's reach
/// it meanwhile), or takes it back with `user` empty. Everyone in the ring is told.
#[tauri::command]
pub async fn coop_control(tab: u32, user: String) -> Res<()> {
    host()?.control(tab, &user)
}

/// Turns down viewer `user`'s ask for control of your shared desktop `tab`.
#[tauri::command]
pub async fn coop_decline_control(tab: u32, user: String) -> Res<()> {
    let ring = host()?;
    if !id_ok(&user) {
        return Err("That person isn't in the ring.".into());
    }
    ring.decline(tab, &user);
    Ok(())
}

/// Gives a person in the ring another name (theirs was offensive, say). The server keeps it for this
/// ring only, so everyone in the ring reads the new one at their next look.
#[tauri::command]
pub async fn coop_rename(user: String, name: String) -> Res<Payload> {
    let ring = host()?;
    let name = line(&name);
    if !id_ok(&user) || user == ring.st().host.id {
        return Err("That person isn't in the ring.".into());
    }
    if name.is_empty() {
        return Err("Type a name.".into());
    }
    let path = format!("/v1/coop/rings/{}/members/{user}/name", ring.id);
    call(&ring.handle, reqwest::Method::POST, &path, Some(serde_json::json!({ "name": name })))
        .await
        .map_err(|e| said(&e))?;
    let _ = ring.fetch_members().await;
    Ok(ring.payload())
}

/// The bytes one person has sent and received in the ring since their connection opened
/// (`coop_traffic`): the dialog reads them every second and shows the difference.
#[derive(Serialize, Debug, PartialEq)]
pub struct Traffic {
    id: String,
    up: u64,
    down: u64,
}

/// Each person's bytes, from this side's connections (`links`: whose it is, what we sent them and
/// what they sent us). Ours is the sum; theirs is their link turned round, which is all of it only
/// when `whole`: the owner's ring is a star, so the owner knows everyone's, and a viewer knows the
/// owner's while nobody else is in.
fn tally(me: &str, links: &[(String, u64, u64)], whole: bool) -> Vec<Traffic> {
    if links.is_empty() {
        return vec![];
    }
    let (sent, got) = links.iter().fold((0, 0), |(s, g), l| (s + l.1, g + l.2));
    let mut out = vec![Traffic { id: me.into(), up: sent, down: got }];
    if whole {
        out.extend(links.iter().map(|(id, s, g)| Traffic { id: id.clone(), up: *g, down: *s }));
    }
    out
}

/// The ring's traffic so far, per person, as QUIC counted it (everything on the wire, the relay's
/// way included). Empty while nobody is connected.
#[tauri::command]
pub fn coop_traffic() -> Vec<Traffic> {
    let Some(ring) = current() else { return vec![] };
    let st = ring.st();
    let link = |id: &str, c: &Connection| {
        let s = c.stats();
        (id.to_string(), s.udp_tx.bytes, s.udp_rx.bytes)
    };
    let links: Vec<_> = if ring.owner {
        st.peers.values().map(|p| link(&p.person.id, &p.conn)).collect()
    } else {
        st.link.iter().map(|l| link(&st.host.id, &l.conn)).collect()
    };
    tally(&st.me.id, &links, ring.owner || st.members.len() <= 1)
}

/// Ends the ring for everyone (as a viewer: leaves it).
#[tauri::command]
pub async fn coop_end() -> Res<()> {
    let ring = current().ok_or(NO_RING)?;
    let path =
        if ring.owner { format!("/v1/coop/rings/{}", ring.id) } else { format!("/v1/coop/rings/{}/leave", ring.id) };
    let method = if ring.owner { reqwest::Method::DELETE } else { reqwest::Method::POST };
    // Whatever the server says, this side is done: the person asked to stop.
    let _ = call(&ring.handle, method, &path, None).await;
    ring.finish(None, "");
    Ok(())
}

/// Leaves a ring you joined.
#[tauri::command]
pub async fn coop_leave() -> Res<()> {
    coop_end().await
}

/// Shares your tab `tab` with the ring, or stops.
#[tauri::command]
pub async fn coop_share(tab: u32, on: bool) -> Res<()> {
    let ring = host()?;
    if on {
        ring.share(tab).await
    } else {
        ring.unshare(tab);
        Ok(())
    }
}

/// Where your pointer is over tab `tab` (a cell of a text tab, a pixel of a picture); negative
/// numbers: it left. The owner's go to the viewers, a viewer's to the owner.
#[tauri::command]
pub async fn coop_cursor(tab: u32, col: i32, row: i32) -> Res<()> {
    let Some(ring) = current() else { return Ok(()) };
    if ring.owner {
        ring.my_cursor(tab, col, row);
    } else if let Some(l) = ring.st().link.as_ref() {
        let _ = l.ctl.send(Msg::Cursor { tab, user_id: String::new(), col, row });
    }
    Ok(())
}

/// You pressed mouse `button` (0 left, 1 middle, 2 right) over tab `tab`, at a cell of a text tab or
/// a pixel of a picture: the others see a mark there. Nothing happens on their tabs from this.
#[tauri::command]
pub async fn coop_click(tab: u32, col: i32, row: i32, button: u8) -> Res<()> {
    let Some(ring) = current() else { return Ok(()) };
    if ring.owner {
        ring.my_click(tab, col, row, button.min(2));
    } else if ring.st().remote.contains_key(&tab) {
        // An owner on an older version can't read it: nothing to say to the person over a click.
        let _ = ring.to_owner(Msg::Click { tab, user_id: String::new(), col, row, button: button.min(2) });
    }
    Ok(())
}

/// Asks to join a ring by its passcode. The owner has to say yes; the `coop` event follows
/// ("pending", then "connecting", then "joined" and the tabs they share).
#[tauri::command]
pub async fn coop_join(handle: AppHandle, passcode: String) -> Res<Payload> {
    let code = clean_passcode(&passcode)?;
    let _one = BUSY.lock().await;
    if current().is_some() {
        return Err(IN_RING.into());
    }
    let key = SecretKey::generate();
    register(&handle, &key).await?;
    let ep = bind(key, relay()?, false).await?;
    let asked = async {
        let node = node_of(&ep).await;
        let body = serde_json::json!({ "passcode": code, "node": node });
        let v = call(&handle, reqwest::Method::POST, "/v1/coop/join", Some(body)).await.map_err(|e| said(&e))?;
        v["ring"]
            .as_str()
            .filter(|s| id_ok(s))
            .map(str::to_string)
            .ok_or_else(|| "The sync server's answer wasn't understood.".to_string())
    }
    .await;
    let id = match asked {
        Ok(id) => id,
        Err(e) => {
            ep.close().await;
            return Err(e);
        }
    };
    let nobody = Person { id: String::new(), name: String::new(), image: None };
    let ring = Ring::new(handle, id, false, ep, nobody.clone(), nobody);
    ring.keep(spawn(ring.clone().follow()));
    ring.keep(spawn(ring.clone().emitter()));
    *CURRENT.lock().unwrap() = Some(ring.clone());
    ring.notify();
    Ok(ring.payload())
}

/// Everything the UI shows, for when it starts or reloads.
#[tauri::command]
pub fn coop_state() -> Payload {
    current().map_or_else(Payload::default, |r| r.payload())
}

/// The window for a tab shared with you: its frames arrive on `out` as the owner's own window
/// gets them. The owner sends the tab whole again.
#[tauri::command]
pub fn coop_open_remote(ring_tab: u32, out: Channel) -> bool {
    let Ok(ring) = viewer() else { return false };
    let channel = ring.st().remote.get(&ring_tab).map(|r| r.channel.clone());
    let Some(channel) = channel else { return false };
    *channel.lock().unwrap() = Some(out);
    ring.to_owner(Msg::Refresh { tab: ring_tab }).is_ok()
}

/// The window that showed a tab shared with you is gone: its frames stop going to its channel, and
/// dropping the channel ends the callback on the page (a page keeps decoding a channel's frames until
/// then). `channel` is the closed window's channel id: a newer window for the same tab has another
/// one, and is left alone.
#[tauri::command]
pub fn coop_close_remote(ring_tab: u32, channel: u32) {
    let Ok(ring) = viewer() else { return };
    let slot = ring.st().remote.get(&ring_tab).map(|r| r.channel.clone());
    if let Some(slot) = slot {
        let mut open = slot.lock().unwrap();
        if open.as_ref().is_some_and(|c| c.id() == channel) {
            *open = None;
        }
    }
}

fn hands(ring_tab: u32, input: Hands) -> Res<()> {
    to_tab(ring_tab, Msg::Input { tab: ring_tab, input })
}

fn to_tab(ring_tab: u32, m: Msg) -> Res<()> {
    let ring = viewer()?;
    if !ring.st().remote.contains_key(&ring_tab) {
        return Err(GONE_TAB.into());
    }
    ring.to_owner(m)
}

/// Your own selection in a remote text tab changed (None: you have none now): the others in the
/// ring draw it in your colour. An owner's app from before draws nothing; nothing is said then.
#[tauri::command]
pub async fn coop_selection(ring_tab: u32, sel: Option<Sel>) -> Res<()> {
    let _ = to_tab(ring_tab, Msg::Selected { tab: ring_tab, user_id: String::new(), sel });
    Ok(())
}

/// The owner's answer to a viewer's selection: its ends, and (for the clipboard) its text.
#[derive(Serialize)]
pub struct Picked {
    a: At,
    b: At,
    #[serde(skip)]
    text: String,
    /// The text was longer than `MAX_COPY`: the clipboard got its start.
    cut: bool,
}

/// Your own selection in a remote text tab, from `a` to `b` (stable row and column, as its frames
/// number them; `block`: the rectangle between them), widened to words (`unit` 1) or lines (2) as
/// a double or triple click widens one. Its ends come back; with `copy` its text lands on your
/// clipboard, never the owner's. The owner's selection stays as it is.
#[tauri::command]
pub async fn coop_remote_select(ring_tab: u32, a: At, b: At, block: bool, unit: u8, copy: bool) -> Res<Picked> {
    let ring = viewer()?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let ask = {
        let mut st = ring.st();
        st.next_ask = st.next_ask.wrapping_add(1);
        let n = st.next_ask;
        st.asks.insert(n, tx);
        n
    };
    let sent = to_tab(ring_tab, Msg::Copy { ask, tab: ring_tab, a, b, block, unit });
    let got = match sent {
        Ok(()) => tokio::time::timeout(Duration::from_secs(10), rx).await,
        Err(e) => {
            ring.st().asks.remove(&ask);
            return Err(e);
        }
    };
    ring.st().asks.remove(&ask);
    let picked = got.map_err(|_| "The owner's xtermium didn't answer.".to_string())?.map_err(|_| GONE_TAB.to_string())?;
    if copy && !picked.text.is_empty() {
        term::clipboard(|c| c.set_text(picked.text.clone()));
    }
    Ok(picked)
}

/// Your clipboard's text pasted into a remote text tab: the owner's terminal takes it as a paste
/// (bracketed when the program asked for that), never through their clipboard or broadcast.
#[tauri::command]
pub async fn coop_remote_paste(ring_tab: u32, text: String) -> Res<()> {
    paste_ok(&text)?;
    hands(ring_tab, Hands::Paste(text))
}

/// A key pressed in a remote text tab.
#[tauri::command]
pub async fn coop_remote_key(ring_tab: u32, key: term::Key) -> Res<()> {
    hands(ring_tab, Hands::Key(key))
}

/// Text typed into a remote text tab (what `term_text` is for a local one).
#[tauri::command]
pub async fn coop_remote_text(ring_tab: u32, text: String) -> Res<()> {
    hands(ring_tab, Hands::Text(text.chars().take(4096).collect()))
}

/// A pointer event over a remote text tab. `cw` and `ch` are the size of one of your cells in device
/// pixels (what `term_resize` takes), so the click lands on the same cell at the owner's.
#[tauri::command]
pub async fn coop_remote_mouse(ring_tab: u32, ev: term::Mouse, cw: Option<f32>, ch: Option<f32>) -> Res<()> {
    hands(ring_tab, Hands::Mouse(ev, cw.unwrap_or(0.0), ch.unwrap_or(0.0)))
}

/// Keyboard and pointer for a remote picture (`rdp_input`'s events). Only while the owner gave you
/// control of it: the owner's app refuses the rest anyway (`may_drive`), so nothing is sent.
#[tauri::command]
pub async fn coop_remote_input(ring_tab: u32, ev: rdp::Input) -> Res<()> {
    if !desktop_ok(&ev) {
        return Ok(());
    }
    let ring = viewer()?;
    let mine = {
        let st = ring.st();
        st.remote.get(&ring_tab).is_some_and(|r| r.controller == st.me.id)
    };
    if !mine {
        return Ok(());
    }
    hands(ring_tab, Hands::Desktop(serde_json::to_vec(&ev).map_err(|e| e.to_string())?))
}

/// Asks the owner for control of a desktop shared with you (`want`), or gives it back or withdraws
/// the ask. The owner's yes arrives as the `coop` event's `control`; a no takes your ask off `wants`.
#[tauri::command]
pub async fn coop_ask_control(ring_tab: u32, want: bool) -> Res<()> {
    let ring = viewer()?;
    let me = {
        let mut st = ring.st();
        if !st.remote.contains_key(&ring_tab) {
            return Err(GONE_TAB.into());
        }
        let me = st.me.id.clone();
        if want {
            st.wants.insert((ring_tab, me.clone()));
        } else {
            st.wants.remove(&(ring_tab, me.clone()));
        }
        me
    };
    let sent = ring.to_owner(Msg::Ask { tab: ring_tab, want });
    if sent.is_err() {
        ring.st().wants.remove(&(ring_tab, me));
    }
    ring.notify();
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring ends once its owner has been away for as long as the setting says, never when it says never.
    #[test]
    fn idle_ends_a_ring() {
        let mins = |m: u64| Duration::from_secs(m * 60);
        assert!(!idle_out(120, mins(119)));
        assert!(idle_out(120, mins(120)));
        assert!(!idle_out(0, mins(100_000)), "never");
        // The system's own idle clock answers, and the test's computer was used within a day.
        assert!(idle() < mins(1440) || cfg!(not(any(windows, target_os = "macos"))));
        let mut s = crate::settings::Settings { coop_idle: 7, ..Default::default() };
        s.sanitize();
        assert_eq!(s.coop_idle, 120, "an unknown value falls back to the default, 2 hours");
    }

    fn hello(user: &str) -> Hello {
        Hello { user_id: user.into(), name: "Sam".into(), image_url: None, wire: WIRE }
    }

    fn member(id: &str, state: &str, node: &str) -> ApiMember {
        ApiMember {
            id: id.into(),
            name: id.into(),
            image: None,
            state: state.into(),
            node: Some(Node { id: node.into(), relay: None, addrs: vec![] }),
        }
    }

    /// Every message survives encoding with its length in front, and nothing else changes.
    #[test]
    fn messages_round_trip() {
        let key = term::Key { key: "a".into(), ctrl: true, alt: false, shift: false, meta: false };
        let mouse = term::Mouse {
            kind: term::MouseKind::Move,
            button: 0,
            x: 3.5,
            y: 9.0,
            lines: 0,
            ctrl: false,
            alt: false,
            shift: true,
            clicks: 1,
        };
        let share = ShareInfo { tab: 7, title: "build".into(), kind: Shape::Text, cols: 80, rows: 24, w: 0, h: 0 };
        let msgs = vec![
            Msg::Hello(hello("u1")),
            Msg::Share(share),
            Msg::Unshare { tab: 7 },
            Msg::Frame { tab: 7, bytes: (0..=255).cycle().take(100_000).collect() },
            Msg::Input { tab: 7, input: Hands::Key(key) },
            Msg::Input { tab: 7, input: Hands::Text("ls\n".into()) },
            Msg::Input { tab: 7, input: Hands::Mouse(mouse, 8.0, 16.0) },
            Msg::Input {
                tab: 8,
                input: Hands::Desktop(
                    serde_json::to_vec(&rdp::Input::Button { button: 0, down: true, x: 4, y: 5 }).unwrap(),
                ),
            },
            Msg::Resize { tab: 7, cols: 100, rows: 30 },
            Msg::Cursor { tab: 7, user_id: "u2".into(), col: -1, row: 4 },
            Msg::Refresh { tab: 7 },
            Msg::Kicked,
            Msg::Ended,
            Msg::Input { tab: 7, input: Hands::Paste("echo hi
".into()) },
            Msg::Copy { ask: 3, tab: 7, a: (-5, 2), b: (12, 0), block: true, unit: 1 },
            Msg::Selection { ask: 3, a: (-5, 0), b: (12, 79), text: "ls /var/log".into(), cut: false },
            Msg::Control { tab: 8, user_id: "u2".into() },
            Msg::Ask { tab: 8, want: true },
        ];
        for m in &msgs {
            let wire = encode(m).unwrap();
            assert_eq!(u32::from_be_bytes(wire[..4].try_into().unwrap()) as usize, wire.len() - 4);
            assert_eq!(encode(&decode(&wire[4..]).unwrap()).unwrap(), wire);
        }
        let Msg::Input { input: Hands::Key(k), .. } = decode(&encode(&msgs[4]).unwrap()[4..]).unwrap() else {
            panic!("not a key")
        };
        assert_eq!((k.key.as_str(), k.ctrl), ("a", true));
        let Msg::Frame { bytes, .. } = decode(&encode(&msgs[3]).unwrap()[4..]).unwrap() else { panic!("not a frame") };
        assert_eq!(bytes.len(), 100_000);
    }

    /// Messages read back in order from a stream, a cut-off one is an error, a huge length is
    /// refused before it is read, and the end between two messages is just the end.
    #[tokio::test]
    async fn messages_travel_over_a_stream() {
        let (mut a, mut b) = tokio::io::duplex(1 << 16);
        write_msg(&mut a, &Msg::Hello(hello("u1"))).await.unwrap();
        write_msg(&mut a, &Msg::Unshare { tab: 3 }).await.unwrap();
        assert!(matches!(read_msg(&mut b, MAX_CTL).await.unwrap(), Some(Msg::Hello(h)) if h.user_id == "u1"));
        assert!(matches!(read_msg(&mut b, MAX_CTL).await.unwrap(), Some(Msg::Unshare { tab: 3 })));
        a.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        assert!(read_msg(&mut b, MAX_CTL).await.is_err());
        drop(a);
        let (mut c, mut d) = tokio::io::duplex(64);
        c.write_all(&10u32.to_be_bytes()).await.unwrap();
        c.write_all(&[1, 2]).await.unwrap();
        drop(c);
        assert!(read_msg(&mut d, MAX_CTL).await.is_err());
        let (c, mut d) = tokio::io::duplex(64);
        drop(c);
        assert!(read_msg(&mut d, MAX_CTL).await.unwrap().is_none());
    }

    /// The owner lets in only an address the server lists as approved, with the user it lists.
    #[test]
    fn hello_from_an_unlisted_node_is_refused() {
        let list = vec![
            member("u1", "approved", "node-a"),
            member("u2", "pending", "node-b"),
            member("u3", "kicked", "node-c"),
        ];
        assert_eq!(admit(&list, "node-a", &hello("u1")).unwrap().id, "u1");
        // Not on the list at all.
        assert!(admit(&list, "node-z", &hello("u1")).is_err());
        // Listed, but waiting or removed.
        assert!(admit(&list, "node-b", &hello("u2")).is_err());
        assert!(admit(&list, "node-c", &hello("u3")).is_err());
        // An approved address claiming to be somebody else.
        assert!(admit(&list, "node-a", &hello("u2")).is_err());
        assert!(admit(&[], "node-a", &hello("u1")).is_err());
    }

    #[test]
    fn a_join_needs_a_well_formed_passcode() {
        assert_eq!(clean_passcode("").unwrap_err(), "Type the ring's passcode.");
        assert_eq!(clean_passcode("  - ").unwrap_err(), "Type the ring's passcode.");
        assert_eq!(clean_passcode("a7k2-m9qx").unwrap(), "A7K2M9QX");
        assert_eq!(clean_passcode(" A7K2 M9QX ").unwrap(), "A7K2M9QX");
        // The server's alphabet has no 0, O, 1 or I; and the length is 8.
        assert!(clean_passcode("A7K2M9Q0").is_err());
        assert!(clean_passcode("A7K2M9Q").is_err());
        assert!(clean_passcode("A7K2M9QXX").is_err());
    }

    #[test]
    fn pro_gates_a_ring() {
        let pro = |active| sync::Pro { active, plan: None, until: None };
        assert!(needs_pro(&pro(true)).is_ok());
        assert_eq!(needs_pro(&pro(false)).unwrap_err(), NEEDS_PRO);
        let account = |active| sync::Account { email: String::new(), name: None, image: None, pro: pro(active) };
        // Only an answer that says Pro is off ends a ring; being offline doesn't.
        assert!(pro_lapsed(&Ok(account(false))));
        assert!(!pro_lapsed(&Ok(account(true))));
        assert!(!pro_lapsed(&Err(anyhow::anyhow!("offline"))));
    }

    #[test]
    fn viewers_cannot_use_the_owners_shortcuts() {
        let key = |k: &str, ctrl, shift, meta| term::Key { key: k.into(), ctrl, alt: false, shift, meta };
        assert!(key_ok(&key("a", false, false, false)));
        assert!(key_ok(&key("c", true, false, false)), "Ctrl+C interrupts, it doesn't copy");
        assert!(key_ok(&key("V", false, true, false)), "a capital V is a letter");
        assert!(!key_ok(&key("V", true, true, false)));
        assert!(!key_ok(&key("c", true, true, false)));
        assert!(!key_ok(&key("Insert", false, true, false)));
        assert!(!key_ok(&key("PageUp", false, true, false)));
        assert!(!key_ok(&key("v", false, false, true)));
        assert!(desktop_ok(&rdp::Input::Wheel { dx: 0.0, dy: 1.0 }));
        assert!(!desktop_ok(&rdp::Input::Paste));
        assert!(!desktop_ok(&rdp::Input::Resize { width: 1, height: 1 }));
        assert!(desktop_ok(&rdp::Input::Release));
    }

    /// Shared desktops take nothing to or from a clipboard: no paste of the owner's clipboard, and
    /// no paste typed out either (more text than a key types); a key's character still goes.
    #[test]
    fn shared_desktops_take_no_paste() {
        assert!(!desktop_ok(&rdp::Input::Paste));
        assert!(desktop_ok(&rdp::Input::Text { text: "a".into() }));
        assert!(desktop_ok(&rdp::Input::Text { text: "e\u{301}".into() }), "an accent with its letter");
        assert!(!desktop_ok(&rdp::Input::Text { text: "rm -rf /".into() }));
        assert!(desktop_ok(&rdp::Input::Key { code: "KeyV".into(), down: true }));
        assert!(desktop_ok(&rdp::Input::Button { button: 0, down: true, x: 1, y: 1 }));
        // What a modified viewer would send instead, as the owner reads it: nothing gets through.
        for ev in [rdp::Input::Paste, rdp::Input::Text { text: "x".repeat(KEY_TEXT + 1) }] {
            let json = serde_json::to_vec(&ev).unwrap();
            let back = serde_json::from_slice::<rdp::Input>(&json).unwrap();
            assert!(!desktop_ok(&back));
        }
    }

    /// A Hello carries its sender's protocol version after its own bytes: an app from before reads
    /// the Hello as it always did, and its Hello (no byte) reads as version 1 here.
    #[test]
    fn hellos_say_which_version_they_speak() {
        let new = encode(&Msg::Hello(hello("u1"))).unwrap();
        let Msg::Hello(h) = decode(&new[4..]).unwrap() else { panic!("not a Hello") };
        assert_eq!((h.user_id.as_str(), h.wire), ("u1", WIRE));
        // What an older app does with it: postcard's plain read, which leaves the byte alone.
        let Msg::Hello(old) = postcard::from_bytes::<Msg>(&new[4..]).unwrap() else { panic!("not a Hello") };
        assert_eq!(old.user_id, "u1");
        // An older app's Hello.
        let before = postcard::to_allocvec(&Msg::Hello(hello("u2"))).unwrap();
        let Msg::Hello(h) = decode(&before).unwrap() else { panic!("not a Hello") };
        assert_eq!((h.user_id.as_str(), h.wire), ("u2", 1));
    }

    /// Copy and paste go only to an app that reads them: an older one would drop the connection
    /// over a message it doesn't know (postcard can't skip an unknown variant).
    #[test]
    fn copy_and_paste_wait_for_an_app_that_knows_them() {
        let paste = Msg::Input { tab: 1, input: Hands::Paste("x".into()) };
        let picked = Msg::Selection { ask: 1, a: (0, 0), b: (0, 1), text: "x".into(), cut: false };
        let ask = Msg::Copy { ask: 1, tab: 1, a: (0, 0), b: (0, 1), block: false, unit: 0 };
        for m in [&paste, &picked, &ask] {
            assert_eq!(needs(m), 2);
            assert!(decode(&encode(m).unwrap()[4..]).is_ok());
            // What an app from before (version 1) knows ends at `Ended`: these are past it.
            let b = postcard::to_allocvec(m).unwrap();
            assert!(if let Msg::Input { .. } = m { b[2] > 3 } else { b[0] > 9 });
        }
        assert_eq!(needs(&Msg::Input { tab: 1, input: Hands::Text("x".into()) }), 1);
        assert_eq!(needs(&Msg::Refresh { tab: 1 }), 1);
        // Everyone's selection is version 3: an app of version 2 never gets one.
        let sel = Msg::Selected { tab: 1, user_id: "u1".into(), sel: Some(((-3, 0), (2, 79), false)) };
        assert_eq!(needs(&sel), 3);
        assert!(needs(&sel) > 2 && WIRE >= 3);
        // A click is drawn only by an app that knows the message: the fifth version.
        let click = Msg::Click { tab: 3, user_id: "u1".into(), col: 10, row: 4, button: 2 };
        assert_eq!((needs(&click), WIRE >= 5), (5, true));
        assert!(matches!(decode(&encode(&click).unwrap()[4..]).unwrap(), Msg::Click { tab: 3, col: 10, row: 4, button: 2, .. }));
        assert_eq!(postcard::to_allocvec(&sel).unwrap()[0], 12, "past what a version 2 app knows");
        let Msg::Selected { sel: back, .. } = decode(&encode(&sel).unwrap()[4..]).unwrap() else { panic!("not Selected") };
        assert_eq!(back, Some(((-3, 0), (2, 79), false)));
        let gone = Msg::Selected { tab: 1, user_id: "u1".into(), sel: None };
        assert!(matches!(decode(&encode(&gone).unwrap()[4..]).unwrap(), Msg::Selected { sel: None, .. }));
        // Control of a shared desktop is version 4.
        let control = Msg::Control { tab: 1, user_id: "u1".into() };
        let ask = Msg::Ask { tab: 1, want: true };
        for m in [&control, &ask] {
            assert_eq!(needs(m), 4);
            assert!(needs(m) <= WIRE);
            assert!(postcard::to_allocvec(m).unwrap()[0] > 12, "past what a version 3 app knows");
            assert!(decode(&encode(m).unwrap()[4..]).is_ok());
        }
    }

    /// A shared terminal takes everyone's typing; a shared desktop only the one viewer the owner
    /// gave control to, and nobody's until the owner gives it.
    #[test]
    fn only_the_viewer_with_control_drives_a_desktop() {
        let click = || Hands::Desktop(serde_json::to_vec(&rdp::Input::Button { button: 0, down: true, x: 1, y: 1 }).unwrap());
        let typing = || Hands::Text("ls".into());
        assert!(!may_drive(None, "u1", &click()));
        assert!(may_drive(Some("u1"), "u1", &click()));
        assert!(!may_drive(Some("u1"), "u2", &click()));
        assert!(may_drive(None, "u1", &typing()));
        assert!(may_drive(Some("u2"), "u1", &typing()));
        assert!(may_drive(None, "u1", &Hands::Paste("x".into())));
    }

    /// A paste has a limit that keeps it inside one control message, and says it; a copy is cut
    /// at a character's edge, and says so.
    #[test]
    fn pastes_and_copies_have_limits() {
        assert!(paste_ok(&"a".repeat(MAX_PASTE)).is_ok());
        assert_eq!(paste_ok(&"a".repeat(MAX_PASTE + 1)).unwrap_err(), TOO_LONG);
        assert!(TOO_LONG.contains("128 KB"));
        let m = Msg::Input { tab: 1, input: Hands::Paste("a".repeat(MAX_PASTE)) };
        assert!(encode(&m).unwrap().len() - 4 <= MAX_CTL);
        // A copy fits as well.
        let (text, cut) = clip("é".repeat(MAX_COPY));
        let m = Msg::Selection { ask: 1, a: (0, 0), b: (0, 0), text, cut };
        assert!(encode(&m).unwrap().len() - 4 <= MAX_CTL);
        assert_eq!(clip("ab".into()), ("ab".to_string(), false));
        let (text, cut) = clip("é".repeat(MAX_COPY));
        assert_eq!((text.len(), cut), (MAX_COPY, true), "two bytes each: cut between two");
        let (text, _) = clip(format!("a{}", "é".repeat(MAX_COPY)));
        assert_eq!(text.len(), MAX_COPY - 1, "never inside a character");
        assert!(!clip("a".repeat(MAX_COPY)).1);
    }

    #[test]
    fn nodes_round_trip_through_the_servers_json() {
        let id = iroh::SecretKey::generate().public();
        let a = EndpointAddr::new(id)
            .with_ip_addr("203.0.113.5:4433".parse().unwrap())
            .with_ip_addr("[2001:db8::1]:4433".parse().unwrap());
        let n = Node::of(&a);
        assert_eq!(n.id, id.to_string());
        assert_eq!((n.relay.as_deref(), n.addrs.len()), (None, 2));
        let json = serde_json::to_value(&n).unwrap();
        assert_eq!(json["relay"], serde_json::Value::Null);
        assert_eq!(serde_json::from_value::<Node>(json).unwrap().addr().unwrap(), a);
        assert!(Node { id: "nope".into(), relay: None, addrs: vec![] }.addr().is_err());
        // A relay URL comes back as one.
        let b = a.clone().with_relay_url("https://relay.example/".parse().unwrap());
        assert_eq!(Node::of(&b).addr().unwrap(), b);
    }

    /// Against a running relay that asks the server (deploy/iroh-relay.toml `access.http`): a key
    /// the server doesn't know stays out; once registered for a Pro account it gets in. Run it twice
    /// with `XTERMIUM_COOP_RELAY` set, before and after `POST /v1/coop/node` with the printed id,
    /// with `XTERMIUM_COOP_ONLINE=0` and then `=1`.
    #[test]
    #[ignore = "needs a relay and the server at XTERMIUM_COOP_RELAY"]
    fn the_relay_lets_only_a_registered_key_in() {
        tauri::async_runtime::block_on(async {
            let key = SecretKey::from_bytes(&[7; 32]);
            println!("endpoint id {}", key.public());
            let ep = bind(key, relay().unwrap(), false).await.unwrap();
            let online = tokio::time::timeout(Duration::from_secs(8), ep.online()).await.is_ok();
            ep.close().await;
            assert_eq!(online, std::env::var("XTERMIUM_COOP_ONLINE").as_deref() == Ok("1"));
        });
    }

    /// Our relay and no other: nothing of n0's.
    #[test]
    fn exactly_one_relay_of_ours() {
        let urls = |mode: Res<RelayMode>| {
            mode.unwrap().relay_map().urls::<Vec<RelayUrl>>().into_iter().map(|u| u.to_string()).collect::<Vec<_>>()
        };
        assert_eq!(urls(relay_of(None)), ["https://relay.xtermium.com/"]);
        assert_eq!(urls(relay_of(Some("  ".into()))), ["https://relay.xtermium.com/"]);
        assert_eq!(urls(relay_of(Some("http://127.0.0.1:3340".into()))), ["http://127.0.0.1:3340/"]);
        assert!(relay_of(Some("not a url".into())).is_err());
        assert!(!urls(relay_of(None)).iter().any(|u| u.contains("iroh") || u.contains("n0")));
    }

    /// The owner knows everyone's traffic (theirs is their link turned round); a viewer knows its
    /// own, and the owner's only while nobody else is in the ring.
    #[test]
    fn traffic_is_told_per_person() {
        let t = |id: &str, up, down| Traffic { id: id.into(), up, down };
        let links = [("a".to_string(), 100, 7), ("b".to_string(), 50, 3)];
        assert_eq!(tally("me", &links, true), [t("me", 150, 10), t("a", 7, 100), t("b", 3, 50)]);
        assert_eq!(tally("me", &links[..1], true), [t("me", 100, 7), t("a", 7, 100)]);
        assert_eq!(tally("me", &links[..1], false), [t("me", 100, 7)]);
        assert_eq!(tally("me", &[], true), []);
    }

    #[test]
    fn user_ids_are_checked_before_a_path() {
        assert!(id_ok("a1B2_c-3"));
        assert!(!id_ok(""));
        assert!(!id_ok("../rings"));
        assert!(!id_ok("a/b"));
    }

    /// A viewer too far behind loses its backlog, once, instead of the ring growing without end.
    #[tokio::test]
    async fn a_slow_viewer_is_resynced() {
        let (q, mut rx) = Queue::new();
        q.push(vec![0; 10]);
        assert_eq!(q.pending.load(Relaxed), 10);
        q.pending.store(BACKLOG + 1, Relaxed);
        q.push(vec![1; 10]);
        assert!(q.lagged.load(Relaxed), "too much unsent: told to give up what it holds");
        assert_eq!(rx.try_recv().unwrap().len(), 10);
        assert!(rx.try_recv().is_err(), "the frame past the limit was dropped");
    }

    /// Two endpoints on this computer (no relay): the owner admits the viewer from a fake approved
    /// list, a tab's frames reach the viewer on their own stream byte for byte, the viewer's input
    /// reaches the owner, and an address not on the list is closed out. Run by hand:
    /// `cargo test -- --ignored coop`.
    #[test]
    #[ignore = "opens sockets on this computer"]
    fn loopback_streams_a_frame_end_to_end() {
        tauri::async_runtime::block_on(async {
            let owner = bind(SecretKey::generate(), RelayMode::Disabled, true).await.unwrap();
            let viewer = bind(SecretKey::generate(), RelayMode::Disabled, false).await.unwrap();
            let stranger = bind(SecretKey::generate(), RelayMode::Disabled, false).await.unwrap();
            let on_loopback = |ep: &Endpoint| {
                let port = ep.bound_sockets().into_iter().find(|s| s.is_ipv4()).expect("an IPv4 socket").port();
                EndpointAddr::new(ep.id()).with_ip_addr(SocketAddr::from(([127, 0, 0, 1], port)))
            };
            let approved = vec![member("u1", "approved", &viewer.id().to_string())];
            // No discovery of any kind: addresses come from the server alone.
            for ep in [&owner, &viewer] {
                assert!(ep.address_lookup().unwrap().is_empty());
            }

            // The owner's side: greet whoever dials, keep the first admitted one.
            let (up_tx, mut up_rx) = unbounded_channel::<Msg>();
            let served = {
                let (owner, approved) = (owner.clone(), approved.clone());
                spawn(async move {
                    let mut kept = vec![];
                    while let Some(incoming) = owner.accept().await {
                        let conn = incoming.await.unwrap();
                        let node = conn.remote_id().to_string();
                        match open_host(&conn, &hello("owner"), |h| admit(&approved, &node, h).map(|_| ())).await {
                            Ok((send, mut recv, who)) => {
                                assert_eq!(who.user_id, "u1");
                                let ctl = control_writer(send);
                                ctl.send(Msg::Share(ShareInfo {
                                    tab: 7,
                                    title: "build".into(),
                                    kind: Shape::Text,
                                    cols: 80,
                                    rows: 24,
                                    w: 0,
                                    h: 0,
                                }))
                                .unwrap();
                                let (q, rx) = Queue::new();
                                spawn(frame_writer(conn.clone(), 7, rx, q.clone(), || {}));
                                for n in 0..3u8 {
                                    q.push(vec![n; 300_000]);
                                }
                                let up = up_tx.clone();
                                spawn(async move {
                                    while let Ok(Some(m)) = read_msg(&mut recv, MAX_CTL).await {
                                        let _ = up.send(m);
                                    }
                                });
                                kept.push((conn, ctl, q));
                            }
                            Err(_) => conn.close(REFUSED.into(), b"refused"),
                        }
                    }
                })
            };

            // An address nobody approved is closed out.
            let c = stranger.connect(on_loopback(&owner), ALPN).await.unwrap();
            let refused = open_guest(&c, &hello("u1")).await;
            assert!(refused.is_err(), "the stranger got in");
            assert_eq!(close_code(&c).await, Some(REFUSED));

            // The viewer is let in, hears of the share and gets the frames.
            let conn = viewer.connect(on_loopback(&owner), ALPN).await.unwrap();
            let (send, mut recv, who) = open_guest(&conn, &hello("u1")).await.unwrap();
            assert_eq!(who.user_id, "owner");
            let ctl = control_writer(send);
            let Some(Msg::Share(info)) = read_msg(&mut recv, MAX_CTL).await.unwrap() else { panic!("no Share") };
            assert_eq!((info.tab, info.cols), (7, 80));
            let (frames_tx, mut frames) = unbounded_channel();
            spawn(read_frames(conn.clone(), move |tab, bytes| {
                let _ = frames_tx.send((tab, bytes));
            }));
            for n in 0..3u8 {
                let (tab, bytes) = tokio::time::timeout(Duration::from_secs(10), frames.recv())
                    .await
                    .expect("a frame in time")
                    .unwrap();
                assert_eq!(
                    (tab, bytes.len(), bytes[0], bytes[299_999]),
                    (7, 300_000, n, n),
                    "frames arrive whole and in order"
                );
            }

            // The viewer's input reaches the owner.
            ctl.send(Msg::Input { tab: 7, input: Hands::Text("ls\n".into()) }).unwrap();
            let Some(Msg::Input { tab: 7, input: Hands::Text(t) }) =
                tokio::time::timeout(Duration::from_secs(10), up_rx.recv()).await.expect("input in time")
            else {
                panic!("no input")
            };
            assert_eq!(t, "ls\n");

            // The connection counts what crossed it (`coop_traffic`): the three frames came in, the input went out.
            let seen = conn.stats();
            assert!(seen.udp_rx.bytes > 900_000 && seen.udp_tx.bytes > 0, "the traffic is counted");

            conn.close(LEFT.into(), b"left");
            served.abort();
            for ep in [owner, viewer, stranger] {
                ep.close().await;
            }

            // A development setup without our relay running (nothing answers at the address) still
            // connects over the direct addresses the server hands out.
            let dead = || relay_of(Some("http://127.0.0.1:9".into())).unwrap();
            let (a, b) = (bind(SecretKey::generate(), dead(), true).await.unwrap(), bind(SecretKey::generate(), dead(), false).await.unwrap());
            let to = on_loopback(&a);
            let accepted = {
                let a = a.clone();
                spawn(async move { a.accept().await.unwrap().await.map(|_| ()).is_ok() })
            };
            let c = tokio::time::timeout(Duration::from_secs(20), b.connect(to, ALPN))
                .await
                .expect("connected in time")
                .unwrap();
            assert!(accepted.await.unwrap());
            c.close(LEFT.into(), b"left");
            for ep in [a, b] {
                ep.close().await;
            }
        });
    }
}
