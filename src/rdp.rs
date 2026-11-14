//! Embedded RDP tab. Decoding happens in the `xtermium-rdp` helper process; this side relays its
//! framebuffer updates (changed areas, batched per frame) to the UI and the UI's keyboard, mouse
//! and pane size to the helper.

use crate::config::{self, Session};
use crate::prompt::Prompter;
use crate::rdp_proto::{FromHelper, ToHelper};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufReader, BufWriter};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc::UnboundedSender;

/// Message types on an RDP tab's channel (first byte). Integers are little-endian.
/// `[2][width: u16][height: u16]` the desktop size; `[3]` then changed areas back to back, each
/// `[x][y][w][h: u16][w*h RGBA pixels]`; `[4][closed: u8][utf-8 status]`.
const MSG_SIZE: u8 = 2;
const MSG_FRAME: u8 = 3;
const MSG_STATUS: u8 = 4;
/// The session ended from inside (signed out): the UI closes the tab.
const MSG_ENDED: u8 = 5;
/// `[8]` then a `MSG_FRAME`'s changed areas, deflated (zlib).
const MSG_FRAME_PACKED: u8 = 8;
/// Frames smaller than this go as they are: packing them saves too little to be worth the time.
const PACK_FROM: usize = 16 << 10;
/// The desktop's pointer, which the view makes its own (6 is coop.rs' `MSG_JPEG`): `[7][0]` the
/// system arrow, `[7][1]` none, `[7][2][w][h][hot x][hot y: u16][w*h RGBA]` a picture.
const MSG_POINTER: u8 = 7;

/// Input from the UI. Coordinates are remote desktop pixels.
#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Input {
    /// A key by DOM `KeyboardEvent.code`.
    Key {
        code: String,
        down: bool,
    },
    /// Characters typed without a Ctrl/Alt chord.
    Text {
        text: String,
    },
    Move {
        x: u16,
        y: u16,
    },
    Button {
        button: u8,
        down: bool,
        x: u16,
        y: u16,
    },
    /// Wheel steps: `dy` positive = up, `dx` positive = right.
    Wheel {
        dx: f32,
        dy: f32,
    },
    /// Type the local clipboard into the session.
    Paste,
    /// The pane's size in device pixels: the remote desktop follows it if the server can.
    Resize {
        width: u16,
        height: u16,
    },
    /// Let go of every key and button held on the desktop: the view lost the keyboard, so their
    /// releases would go elsewhere (and the desktop would keep, say, Ctrl down).
    Release,
    /// The lock keys' states here, for the desktop to take on.
    Sync {
        caps: bool,
        num: bool,
        scroll: bool,
    },
    /// The view took the keyboard, or a paste chord is on its way: the desktop gets to paste
    /// what the clipboard here holds now (Settings › Terminal › RDP allowing).
    Clip,
}

/// What may cross the shared clipboard (Settings › Terminal › RDP): this computer's to the
/// desktop, the desktop's to here, both (the default) or neither.
pub const TO_DESKTOP: u8 = 1;
pub const FROM_DESKTOP: u8 = 2;
static CLIPBOARD: AtomicU8 = AtomicU8::new(TO_DESKTOP | FROM_DESKTOP);

/// Applies the clipboard setting to every RDP tab, open or to come.
pub fn set_clipboard(mode: u8) {
    CLIPBOARD.store(mode, Relaxed);
}

fn clipboard_allows(way: u8) -> bool {
    CLIPBOARD.load(Relaxed) & way != 0
}

/// Text bigger than this doesn't cross the clipboard (the helper's limit, `clip::MAX_TEXT`).
const MAX_TEXT: usize = 16 << 20;
/// Nor a picture bigger than this (`clip::MAX_IMAGE`): 4K screenshots pass, 8K ones don't.
const MAX_IMAGE: usize = 60 << 20;

/// A clipboard's content as it crossed: the last one each way, so neither side gets back what
/// it already has (the desktop's text put here isn't offered to it again).
#[derive(PartialEq)]
enum Clipped {
    Text(String),
    Files(Vec<String>),
    /// A picture, by a hash of its pixels.
    Image(u64),
}

/// What this computer's clipboard holds now: files (as Explorer copies them), else text, else a
/// picture (a screenshot from Win+Shift+S, say); with the message that offers it to the desktop.
fn clipboard_now() -> Option<(Clipped, ToHelper)> {
    let files = crate::term::clipboard(|c| c.get().file_list()).filter(|f| !f.is_empty());
    if let Some(f) = files {
        let f: Vec<String> = f.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        return Some((Clipped::Files(f.clone()), ToHelper::ClipFiles(f)));
    }
    if let Some(t) = crate::term::clipboard(|c| c.get_text()).filter(|t| !t.is_empty() && t.len() <= MAX_TEXT) {
        return Some((Clipped::Text(t.clone()), ToHelper::ClipText(t)));
    }
    let img = crate::term::clipboard(|c| c.get_image()).filter(|i| i.bytes.len() <= MAX_IMAGE && i.bytes.len() == i.width * i.height * 4)?;
    Some((Clipped::Image(hash(&img.bytes)), ToHelper::ClipImage(to_dib(img.width, img.height, &img.bytes))))
}

fn hash(b: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

/// RGBA pixels (rows top to bottom) as CF_DIB, what Windows pastes pictures from: a 40-byte
/// BITMAPINFOHEADER, then 32-bit BGRA rows bottom to top.
fn to_dib(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let mut d = Vec::with_capacity(40 + rgba.len());
    for v in [40, w as u32, h as u32] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&[1, 0, 32, 0]); // one plane, 32 bits a pixel
    // Uncompressed (BI_RGB), the pixels' size, then resolution and palette left out.
    for v in [0, (w * h * 4) as u32, 0, 0, 0, 0] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    for row in rgba.chunks_exact(w * 4).rev() {
        for px in row.as_chunks::<4>().0 {
            d.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    d
}

/// CF_DIB (as `to_dib` makes, or 24-bit, top-down, or with BITFIELDS masks as Windows makes)
/// as (width, height, RGBA rows top to bottom). None for what isn't a 24- or 32-bit picture.
fn from_dib(d: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?));
    let header = u32_at(0)? as usize;
    let (w, h) = (u32_at(4)? as i32, u32_at(8)? as i32);
    let bits = usize::from(u16::from_le_bytes(d.get(14..16)?.try_into().ok()?));
    let (compression, colours) = (u32_at(16)?, u32_at(32)? as usize);
    if header < 40 || w <= 0 || h == 0 || !matches!(bits, 24 | 32) || !matches!(compression, 0 | 3) {
        return None;
    }
    let (w, top_down, h) = (w as usize, h < 0, h.unsigned_abs() as usize);
    if w.checked_mul(h)?.checked_mul(4)? > MAX_IMAGE {
        return None;
    }
    // BITFIELDS puts three colour masks after a plain header (taken as the usual BGR order).
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let start = header + masks + colours * 4;
    let (bpp, stride) = (bits / 8, (w * bits / 8 + 3) & !3);
    let px = d.get(start..start + stride * h)?;
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &px[if top_down { y } else { h - 1 - y } * stride..][..w * bpp];
        for p in row.chunks_exact(bpp) {
            out.extend_from_slice(&[p[2], p[1], p[0], if bpp == 4 { p[3] } else { 0xFF }]);
        }
    }
    // Most 32-bit pictures leave alpha at 0, meaning "no alpha": shown opaque.
    if bpp == 4 && out.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        out.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 0xFF);
    }
    Some((w, h, out))
}

/// A remote desktop's picture as RGBA, for automation's screenshots.
#[derive(Clone, Default)]
pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Picture {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![0; w * h * 4] }
    }

    /// Paints a frame's changed areas: `[x][y][w][h: u16 LE][w*h RGBA]`, back to back.
    pub fn paint(&mut self, mut body: &[u8]) {
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
        while body.len() >= 8 {
            let (x, y, w, h) = (u16_at(body, 0), u16_at(body, 2), u16_at(body, 4), u16_at(body, 6));
            let Some(data) = body.get(8..8 + w * h * 4) else { return };
            let n = w.min(self.w.saturating_sub(x));
            for row in 0..h.min(self.h.saturating_sub(y)) {
                let dst = ((y + row) * self.w + x) * 4;
                self.px[dst..dst + n * 4].copy_from_slice(&data[row * w * 4..][..n * 4]);
            }
            body = &body[8 + w * h * 4..];
        }
    }
}

pub struct RdpTab {
    pub session: Session,
    /// What the desktop shows now (automation's screenshots).
    picture: Arc<Mutex<Picture>>,
    stdin: Arc<Mutex<Option<BufWriter<ChildStdin>>>>,
    child: Arc<Mutex<Option<Child>>>,
    /// The user and password the session logged in with: the file panel can use them for SMB.
    login: Arc<Mutex<Option<(String, String)>>>,
    /// The file panel (SMB), once connected.
    pub files: Option<crate::files::Browser>,
    /// The login's SSH gateway: the file panel reaches the shares through it too.
    pub gateway: Option<config::Endpoint>,
    /// The Windows key held down on the desktop (its scancode), 0 for none.
    win: std::sync::atomic::AtomicU16,
    /// Where messages go: the window showing the tab (it can move to another, `attach`).
    out: Arc<Mutex<Channel>>,
    /// The last status message, sent again to a window the tab moves to.
    status: Arc<Mutex<Vec<u8>>>,
    /// Terminal Co-op (coop.rs): a second place the messages go, while the tab is shared.
    sink: Arc<Mutex<Option<UnboundedSender<Vec<u8>>>>>,
    /// The clipboard content that last crossed, either way.
    clipped: Arc<Mutex<Option<Clipped>>>,
    /// The last pointer message, sent again to a window the tab moves to.
    pointer: Arc<Mutex<Vec<u8>>>,
}

impl RdpTab {
    /// Connects right away; `size` is the desktop size to ask for, `quality` as in `ToHelper::Connect`.
    /// `password`: a shared login's (Credentials), used instead of the keychain or asking;
    /// `gateway`: its SSH gateway, which the desktop is reached through.
    pub fn open(
        session: Session,
        prompt: Prompter,
        out: Channel,
        size: [u16; 2],
        quality: u8,
        password: Option<String>,
        gateway: Option<config::Endpoint>,
    ) -> Self {
        let tab = Self {
            session,
            stdin: Default::default(),
            child: Default::default(),
            login: Default::default(),
            files: None,
            picture: Default::default(),
            gateway: gateway.clone(),
            win: Default::default(),
            out: Arc::new(Mutex::new(out)),
            status: Default::default(),
            sink: Default::default(),
            clipped: Default::default(),
            pointer: Default::default(),
        };
        let config::Kind::Rdp { host, port, user, .. } = tab.session.kind.clone() else { unreachable!("not RDP") };
        let (stdin, child, login, picture) = (tab.stdin.clone(), tab.child.clone(), tab.login.clone(), tab.picture.clone());
        let (out, last_status, sink, clipped) = (tab.out.clone(), tab.status.clone(), tab.sink.clone(), tab.clipped.clone());
        let last_pointer = tab.pointer.clone();
        std::thread::spawn(move || {
            // A window gone (the tab moving to another) misses messages until `attach` catches it up.
            // A Co-op sink gets a copy of each message; without one nothing is copied.
            let ui = |m: Vec<u8>| {
                if let Some(s) = sink.lock().unwrap().as_ref() {
                    let _ = s.send(m.clone());
                }
                let _ = out.lock().unwrap().send(InvokeResponseBody::Raw(m));
            };
            let status = |s: &str, closed: bool| {
                let mut m = vec![MSG_STATUS, closed as u8];
                m.extend_from_slice(s.as_bytes());
                *last_status.lock().unwrap() = m.clone();
                ui(m);
            };
            let ask = |title: &str, text: String, secret: bool| {
                tauri::async_runtime::block_on(prompt.ask(title, text, secret))
            };
            let raw_user = if user.is_empty() {
                match ask("User", format!("User name for {host} (user, DOMAIN\\user or user@domain):"), false) {
                    Some((u, _)) => u,
                    None => return status("cancelled", true),
                }
            } else {
                user
            };
            let (domain, user) = split_user(&raw_user);
            let login_id = format!("rdp:{raw_user}@{host}:{port}");
            let shared = password.is_some();
            let stored = password.or_else(|| crate::config::login(&login_id));
            let from_store = stored.is_some() && !shared;
            let (password, remember) = match stored {
                Some(p) => (p, false),
                None => match ask("Password", format!("Password for {raw_user}@{host}:"), true) {
                    Some(p) => p,
                    None => return status("cancelled", true),
                },
            };

            // Through a gateway, the helper connects to a local port forwarded to the host; the
            // certificate is still trusted (or not) as the host's.
            let fwd = match &gateway {
                Some(gw) => {
                    status(&format!("connecting to SSH gateway {}…", gw.label()), false);
                    match tauri::async_runtime::block_on(crate::ssh::forward(gw, Some((host.clone(), port)), &prompt)) {
                        Ok(f) => Some(f),
                        Err(e) => return status(&format!("{e:#}"), true),
                    }
                }
                None => None,
            };
            let (to_host, to_port) = fwd.as_ref().map_or((host.clone(), port), |f| ("127.0.0.1".into(), f.port));

            let mut cmd = Command::new(helper_path());
            #[cfg(windows)]
            crate::term::no_window(&mut cmd);
            let mut c = match cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                Ok(c) => c,
                Err(e) => return status(&format!("cannot start xtermium-rdp helper: {e}"), true),
            };
            let helper_out = c.stdout.take().unwrap();
            *stdin.lock().unwrap() = c.stdin.take().map(BufWriter::new);
            *child.lock().unwrap() = Some(c);
            let send = |m: ToHelper| {
                if let Some(w) = stdin.lock().unwrap().as_mut() {
                    let _ = m.write(w);
                }
            };
            send(ToHelper::Connect {
                host: to_host,
                port: to_port,
                user,
                domain,
                password: password.clone(),
                width: size[0],
                height: size[1],
                quality,
            });
            status("connecting…", false);

            let mut last = String::new();
            let mut helper_out = BufReader::new(helper_out);
            while let Ok(msg) = FromHelper::read(&mut helper_out) {
                match msg {
                    FromHelper::Certificate(key) => {
                        let ok = trust_certificate(&host, port, &key, &prompt);
                        if !ok {
                            last = "certificate not trusted".into();
                            status(&last, true);
                        }
                        send(ToHelper::Trust(ok));
                    }
                    FromHelper::Size { width, height } => {
                        if remember {
                            let _ = crate::config::remember(&login_id, &password);
                        }
                        *login.lock().unwrap() = Some((raw_user.clone(), password.clone()));
                        let mut p = picture.lock().unwrap();
                        *p = Picture::new(width as usize, height as usize);
                        ui(size_msg(width, height));
                        drop(p);
                        last.clear();
                        status("", false);
                    }
                    FromHelper::Frame(body) => {
                        let m = frame_msg(&body);
                        // Painted and sent under one lock: `attach` sends the whole picture
                        // between frames, never half of one.
                        let mut p = picture.lock().unwrap();
                        p.paint(&body);
                        ui(m);
                        drop(p);
                    }
                    FromHelper::Closed(reason) => {
                        if from_store && reason.starts_with("login failed") {
                            crate::config::forget(&login_id);
                        }
                        last = reason;
                        break;
                    }
                    FromHelper::Ended => {
                        status("signed out", false);
                        ui(vec![MSG_ENDED]);
                        return;
                    }
                    FromHelper::Status(s) => status(&s, false),
                    // A copy on the desktop: brought here if the setting allows (text and pictures
                    // come back as ClipText / ClipImage; files go on the clipboard from the
                    // helper, on Windows).
                    FromHelper::ClipOffer { text, files, image } => {
                        if clipboard_allows(FROM_DESKTOP) && (text || image || (files && cfg!(windows))) {
                            send(ToHelper::ClipFetch);
                        }
                    }
                    FromHelper::ClipText(t) => {
                        if clipboard_allows(FROM_DESKTOP) && crate::term::clipboard(|c| c.set_text(t.as_str())).is_some() {
                            *clipped.lock().unwrap() = Some(Clipped::Text(t));
                        }
                    }
                    FromHelper::ClipImage(dib) => {
                        if clipboard_allows(FROM_DESKTOP)
                            && let Some((width, height, px)) = from_dib(&dib)
                        {
                            let h = hash(&px);
                            if crate::term::clipboard(|c| c.set_image(arboard::ImageData { width, height, bytes: px.into() })).is_some() {
                                *clipped.lock().unwrap() = Some(Clipped::Image(h));
                            }
                        }
                    }
                    FromHelper::PointerDefault | FromHelper::PointerHidden | FromHelper::PointerBitmap { .. } => {
                        let m = pointer_msg(msg);
                        *last_pointer.lock().unwrap() = m.clone();
                        ui(m);
                    }
                }
            }
            status(if last.is_empty() { "disconnected" } else { &last }, true);
        });
        tab
    }

    /// Another window shows the tab from now on, on `out`: it gets the status, the desktop's size
    /// and the whole picture, then the changes as they come.
    pub fn attach(&self, out: Channel) {
        let p = self.picture.lock().unwrap();
        for m in catch_up(&self.status.lock().unwrap(), &p).into_iter().chain(self.last_pointer()) {
            let _ = out.send(InvokeResponseBody::Raw(m));
        }
        *self.out.lock().unwrap() = out;
    }

    /// The last pointer message, if the desktop has sent one.
    fn last_pointer(&self) -> Option<Vec<u8>> {
        Some(self.pointer.lock().unwrap().clone()).filter(|m| !m.is_empty())
    }

    /// The window took a frame: the helper may send the next (`ToHelper::Drawn`).
    pub fn drawn(&self) {
        self.send(ToHelper::Drawn);
    }

    /// Terminal Co-op: from now on the messages the window gets go to `sink` as well (None stops
    /// it). The sink starts with the catch-up `attach` sends, and a viewer who joins later gets
    /// another by calling this again. Done under the picture's lock, like `attach`, so a frame
    /// is either in the picture sent or comes after it.
    pub fn share(&self, sink: Option<UnboundedSender<Vec<u8>>>) {
        let p = self.picture.lock().unwrap();
        if let Some(s) = &sink {
            catch_up(&self.status.lock().unwrap(), &p).into_iter().chain(self.last_pointer()).for_each(|m| _ = s.send(m));
        }
        *self.sink.lock().unwrap() = sink;
    }

    /// The desktop's size in pixels; (0, 0) before it's connected.
    pub fn size(&self) -> (usize, usize) {
        let p = self.picture.lock().unwrap();
        (p.w, p.h)
    }

    /// What the desktop shows now; None before it's connected.
    pub fn screenshot(&self) -> Option<Picture> {
        let p = self.picture.lock().unwrap();
        (p.w > 0).then(|| p.clone())
    }

    pub fn login(&self) -> Option<(String, String)> {
        self.login.lock().unwrap().clone()
    }

    pub fn host(&self) -> String {
        match &self.session.kind {
            config::Kind::Rdp { host, .. } => host.clone(),
            _ => String::new(),
        }
    }

    /// Whether the helper got `m` (false before it has started, or once it's gone).
    fn send(&self, m: ToHelper) -> bool {
        match self.stdin.lock().unwrap().as_mut() {
            Some(w) => m.write(w).is_ok(),
            None => false,
        }
    }

    pub fn input(&self, ev: Input) {
        match ev {
            Input::Key { code, down } => {
                let Some(scancode) = scancode(&code) else { return };
                if matches!(code.as_str(), "MetaLeft" | "MetaRight") {
                    self.win.store(if down { scancode } else { 0 }, Relaxed);
                } else if down && self.win.load(Relaxed) == 0 {
                    // No Windows key is held here: one the desktop still thinks is down (its
                    // release went missing) would turn Enter or Backspace into Win+Enter. The
                    // helper only sends a release for a key that is down, so this costs nothing.
                    self.send(ToHelper::Key { scancode: 0xE05B, pressed: false });
                    self.send(ToHelper::Key { scancode: 0xE05C, pressed: false });
                }
                self.send(ToHelper::Key { scancode, pressed: down });
            }
            Input::Text { text } => typing(&text).into_iter().for_each(|m| _ = self.send(m)),
            Input::Move { x, y } => _ = self.send(ToHelper::MouseMove { x, y }),
            Input::Button { button, down, x, y } => {
                self.send(ToHelper::MouseMove { x, y });
                self.send(ToHelper::MouseButton { button, pressed: down });
            }
            Input::Wheel { dx, dy } => {
                if dy != 0.0 {
                    self.send(ToHelper::Wheel { vertical: true, units: (dy * 120.0) as i16 });
                }
                if dx != 0.0 {
                    self.send(ToHelper::Wheel { vertical: false, units: (dx * 120.0) as i16 });
                }
            }
            Input::Paste => {
                // Release the Shift / Ctrl of the paste chord first, or the text arrives shifted.
                for scancode in [0x2A, 0x36, 0x1D, 0xE01D] {
                    self.send(ToHelper::Key { scancode, pressed: false });
                }
                if let Some(s) = crate::term::clipboard(|c| c.get_text()) {
                    typing(&s).into_iter().for_each(|m| _ = self.send(m));
                }
            }
            Input::Resize { width, height } => _ = self.send(ToHelper::Resize { width, height }),
            Input::Release => {
                self.win.store(0, Relaxed);
                self.send(ToHelper::ReleaseAll);
            }
            Input::Sync { caps, num, scroll } => _ = self.send(ToHelper::Sync { caps, num, scroll }),
            Input::Clip => {
                if !clipboard_allows(TO_DESKTOP) {
                    return;
                }
                let Some((now, m)) = clipboard_now() else { return };
                let mut last = self.clipped.lock().unwrap();
                if last.as_ref() == Some(&now) {
                    return;
                }
                if self.send(m) {
                    *last = Some(now);
                }
            }
        }
    }
}

/// Text for the desktop: characters as themselves, line breaks and tabs as the Enter and Tab keys
/// (a raw line feed is Ctrl+Enter to some programs, and a pasted line should run like a typed one).
fn typing(text: &str) -> Vec<ToHelper> {
    let key = |scancode| [ToHelper::Key { scancode, pressed: true }, ToHelper::Key { scancode, pressed: false }];
    let mut v = vec![];
    for c in text.replace("\r\n", "\n").chars() {
        match c {
            '\n' | '\r' => v.extend(key(0x1C)),
            '\t' => v.extend(key(0x0F)),
            c => v.push(ToHelper::Unicode(c)),
        }
    }
    v
}

impl Drop for RdpTab {
    fn drop(&mut self) {
        self.send(ToHelper::Close);
        if let Some(mut c) = self.child.lock().unwrap().take() {
            // Give the helper a moment to say goodbye to the server, then make sure it is gone.
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = c.kill();
                let _ = c.wait();
            });
        }
    }
}

/// The UI says which remote desktop has the keyboard (`on`), and Settings › Terminal › RDP lets it
/// take the keyboard from the system: the Windows keys and the system's shortcuts then go to
/// that desktop instead of acting here (Windows).
#[tauri::command]
pub fn desktop_keys(app: tauri::AppHandle, id: u32, on: bool) {
    #[cfg(windows)]
    grab::grab(&app, id, on);
    #[cfg(not(windows))]
    let _ = (app, id, on);
}

/// A tab closed or moving to another window lets go of the keyboard: its view may be gone
/// without saying so (a detached window closed while the desktop had the keyboard), and the
/// keys mustn't keep going to it. The view in the window it moves to takes it again on focus.
pub fn let_go(id: u32) {
    #[cfg(windows)]
    grab::want(id, false);
    #[cfg(not(windows))]
    let _ = id;
}

/// A low-level keyboard hook takes the Windows keys and the system's shortcuts (Alt+Tab,
/// Alt+Esc, Alt+Space, Alt+F4, Ctrl+Esc, Ctrl+Shift+Esc) from the system while a desktop has the
/// keyboard and xtermium is in front, and every key pressed while a Windows key it took is down.
/// Windows never sees them, so Win+E opens Explorer on the desktop and not here.
///
/// The hook is set only while a desktop has the keyboard: the rest of the time no key on this
/// computer passes through xtermium. Windows calls it while our own window is in front only
/// because main.rs turns tao's raw input off (`device_event_filter`).
///
/// The keys it takes go straight to the desktop's tab from here (`crate::desktop_input`), in the
/// order they were pressed: E after the Windows key, never before it, however busy the window
/// is. A key taken on its way down is taken on its way up too, so nothing stays stuck.
/// Ctrl+Alt+Del and Win+L stay the system's (no program can take them).
#[cfg(windows)]
mod grab {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed, Ordering::SeqCst};
    use std::sync::mpsc::{Sender, channel};
    use tauri::AppHandle;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_F4, VK_LWIN, VK_RWIN, VK_SPACE, VK_TAB,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// A desktop has the keyboard.
    static GRAB: AtomicBool = AtomicBool::new(false);
    /// The tab whose desktop gets the keys (or last got them: releases still go there).
    static TAB: AtomicU32 = AtomicU32::new(0);
    /// The views that have the keyboard, the latest last: keys go to that one. Moving between
    /// two desktops sends one's "off" and the other's "on", which may arrive in either order.
    static WANT: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
    /// The keys taken on their way down, by virtual key: their releases are taken too.
    static HELD: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
    /// Where the hook hands the keys it takes: the key's code, and down or up.
    static TAKEN: OnceLock<Sender<(String, bool)>> = OnceLock::new();
    /// The thread that sets and removes the hook, 0 before it's there.
    static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);

    pub fn grab(app: &AppHandle, id: u32, on: bool) {
        want(id, on);
        let (tx, rx) = channel();
        if on && TAKEN.set(tx).is_ok() {
            // The hook must answer Windows at once: one that keeps it waiting is dropped without
            // a word, and the Windows key then opens the Start menu here. So it only hands the
            // key over, and this thread passes it on.
            let app = app.clone();
            std::thread::spawn(move || {
                for (code, down) in rx {
                    crate::desktop_input(&app, TAB.load(Relaxed), super::Input::Key { code, down });
                }
            });
            // The hook runs on the thread that set it, which must pump messages.
            std::thread::spawn(|| {
                let mut msg = MSG::default();
                // SAFETY: makes this thread's queue before others post to it.
                let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
                HOOK_THREAD.store(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }, SeqCst);
                let mut hook_ = None;
                follow(&mut hook_);
                // SAFETY: this thread's own message loop.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                    if msg.message == WM_APP && msg.hwnd.is_invalid() {
                        follow(&mut hook_);
                    }
                }
            });
        }
    }

    /// Tab `id`'s view has the keyboard (`on`) or not any more; the hook follows.
    pub fn want(id: u32, on: bool) {
        let last = {
            let mut want = WANT.lock().unwrap();
            want.retain(|&w| w != id);
            if on {
                want.push(id);
            }
            want.last().copied()
        };
        if let Some(t) = last {
            TAB.store(t, Relaxed);
        }
        // SeqCst with HOOK_THREAD: the hook thread starting up either sees this or gets the post.
        if GRAB.swap(last.is_some(), SeqCst) != last.is_some() {
            let thread = HOOK_THREAD.load(SeqCst);
            if thread != 0 {
                // SAFETY: posting to the hook thread, which has a message queue (it pumps one).
                let _ = unsafe { PostThreadMessageW(thread, WM_APP, WPARAM(0), LPARAM(0)) };
            }
        }
    }

    /// Sets the hook while a desktop has the keyboard and removes it once none has. Set anew
    /// each time, so it's the newest: Windows asks the newest hook first, and one set since (a
    /// key remapper's, another app's) would otherwise see the Windows key first and could keep
    /// it from us.
    fn follow(hook_: &mut Option<HHOOK>) {
        if let Some(h) = hook_.take() {
            // SAFETY: a hook this thread set.
            let _ = unsafe { UnhookWindowsHookEx(h) };
            // Releases it didn't see (the lock screen takes Win+L's) mustn't keep a key taken.
            HELD.iter().for_each(|h| h.store(false, Relaxed));
        }
        if GRAB.load(SeqCst) {
            // SAFETY: our own module, and a hook procedure that lives as long as the program.
            let module = unsafe { GetModuleHandleW(None) }.ok().map(|m| HINSTANCE(m.0));
            *hook_ = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0) }.ok();
        }
    }

    /// Whether xtermium has the foreground window.
    fn in_front() -> bool {
        let mut pid = 0;
        // SAFETY: plain queries about the foreground window.
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        pid == std::process::id()
    }

    fn held(vk: u16) -> bool {
        HELD[usize::from(vk)].load(Relaxed)
    }

    /// Whether the system would act on `vk` pressed now: a Windows key always, any key while one
    /// we took is down, Tab, Space and F4 with Alt, Esc with Alt or Ctrl.
    fn system(vk: u16, alt: bool) -> bool {
        // SAFETY: a plain query of a key's state.
        let ctrl = || unsafe { GetAsyncKeyState(VK_CONTROL.0.into()) } < 0;
        match vk {
            _ if vk == VK_LWIN.0 || vk == VK_RWIN.0 || held(VK_LWIN.0) || held(VK_RWIN.0) => true,
            _ if vk == VK_ESCAPE.0 => alt || ctrl(),
            _ => alt && [VK_TAB.0, VK_SPACE.0, VK_F4.0].contains(&vk),
        }
    }

    /// Windows waits on this for every key: it decides, hands a taken key over and returns.
    unsafe extern "system" fn hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for HC_ACTION, lparam points at the key's KBDLLHOOKSTRUCT.
            let k = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
            let vk = (k.vkCode & 0xFF) as u16;
            let down = matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let take = held(vk) || (down && GRAB.load(Relaxed) && system(vk, k.flags.contains(LLKHF_ALTDOWN)) && in_front());
            // Keys another program sent in our stead (a key remapper, a KVM switch) count too:
            // xtermium sends none itself. The Windows keys go by their virtual key: some
            // keyboards and remappers report them without the scancode (or extended flag) they have.
            let code_ = || match vk {
                _ if vk == VK_LWIN.0 => Some("MetaLeft".to_string()),
                _ if vk == VK_RWIN.0 => Some("MetaRight".to_string()),
                _ => super::code_of((k.scanCode & 0xFF) as u16 | if k.flags.contains(LLKHF_EXTENDED) { 0xE000 } else { 0 }),
            };
            if take && let Some(code) = code_() {
                HELD[usize::from(vk)].store(down, Relaxed);
                if let Some(keys) = TAKEN.get() {
                    let _ = keys.send((code, down));
                }
                return LRESULT(1);
            }
        }
        unsafe { CallNextHookEx(None, code, w, l) }
    }
}

/// What a window that starts watching needs first: the last status, the desktop's size and the
/// whole picture as one frame (nothing before it's connected).
pub(crate) fn catch_up(status: &[u8], p: &Picture) -> Vec<Vec<u8>> {
    let mut v = vec![];
    if !status.is_empty() {
        v.push(status.to_vec());
    }
    if p.w > 0 {
        v.push(size_msg(p.w as u16, p.h as u16));
        let mut m = Vec::with_capacity(9 + p.px.len());
        m.push(MSG_FRAME);
        for n in [0, 0, p.w as u16, p.h as u16] {
            m.extend_from_slice(&n.to_le_bytes());
        }
        m.extend_from_slice(&p.px);
        v.push(m);
    }
    v
}

/// A frame's changed areas for the window: deflated when that's worth it. The IPC into the page
/// costs per byte (about 13 ms a megabyte on WebView2) and a dragged window sends a megabyte or
/// more each frame, while desktop pictures shrink ten- to thirty-fold at the fastest level in a
/// few milliseconds. The page inflates them natively (`DecompressionStream`).
fn frame_msg(body: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    if body.len() >= PACK_FROM {
        let mut z = flate2::write::ZlibEncoder::new(vec![MSG_FRAME_PACKED], flate2::Compression::fast());
        if z.write_all(body).is_ok()
            && let Ok(m) = z.finish()
        {
            return m;
        }
    }
    let mut m = Vec::with_capacity(1 + body.len());
    m.push(MSG_FRAME);
    m.extend_from_slice(body);
    m
}

fn size_msg(width: u16, height: u16) -> Vec<u8> {
    let mut m = vec![MSG_SIZE];
    m.extend_from_slice(&width.to_le_bytes());
    m.extend_from_slice(&height.to_le_bytes());
    m
}

/// `[MSG_POINTER]…` for the window, from the helper's pointer message.
fn pointer_msg(p: FromHelper) -> Vec<u8> {
    match p {
        FromHelper::PointerBitmap { width, height, hot_x, hot_y, rgba } => {
            let mut m = Vec::with_capacity(10 + rgba.len());
            m.extend_from_slice(&[MSG_POINTER, 2]);
            for n in [width, height, hot_x, hot_y] {
                m.extend_from_slice(&n.to_le_bytes());
            }
            m.extend_from_slice(&rgba);
            m
        }
        FromHelper::PointerHidden => vec![MSG_POINTER, 1],
        _ => vec![MSG_POINTER, 0],
    }
}

/// `DOMAIN\user` → ("DOMAIN", "user"); `.\user` (local account) → ("", "user").
pub(crate) fn split_user(u: &str) -> (String, String) {
    match u.split_once('\\') {
        Some((".", user)) => (String::new(), user.into()),
        Some((d, user)) => (d.into(), user.into()),
        None => (String::new(), u.into()),
    }
}

fn helper_path() -> std::path::PathBuf {
    let name = format!("xtermium-rdp{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| name.into())
}

/// Trust on first use, like SSH host keys. Keyed by the server's public key, which survives
/// the yearly self-signed certificate renewal Windows does.
fn trust_certificate(host: &str, port: u16, key: &[u8], prompt: &Prompter) -> bool {
    let fp: String = Sha256::digest(key).iter().map(|b| format!("{b:02x}")).collect();
    let id = format!("{host}:{port}");
    let Ok(known) = config::Known::with(|k| k.rdp.get(&id).cloned()) else { return false };
    match known {
        Some(k) if k == fp => true,
        Some(_) => {
            tauri::async_runtime::block_on(prompt.confirm(
                "RDP certificate CHANGED",
                format!(
                    "The key of {id} differs from the one you trusted before.\nThis may be an attack. Connection refused.\n\
                     If the change is expected, remove the old key under Settings > Security."
                ),
            ));
            false
        }
        None => {
            let ok = tauri::async_runtime::block_on(prompt.confirm(
                "Unknown RDP server",
                format!("{id}\nserver key SHA256:\n{fp}\n\nTrust this server and remember its key?"),
            ));
            ok && config::Known::with(|k| k.rdp.insert(id, fp)).is_ok()
        }
    }
}

/// The letter rows and the scancode of each row's first key.
const ROWS: [(&str, u16); 3] = [("QWERTYUIOP", 0x10), ("ASDFGHJKL", 0x1E), ("ZXCVBNM", 0x2C)];
/// The numpad's digits 0 to 9 as keys (Num Lock off they are the arrows, Home, End…), and with
/// Alt held the digits of a character code.
const NUMPAD: [u16; 10] = [0x52, 0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48, 0x49];
/// The other keys by name.
const NAMED: &[(&str, u16)] = &[
    ("Escape", 0x01),
    ("Minus", 0x0C),
    ("Equal", 0x0D),
    ("Backspace", 0x0E),
    ("Tab", 0x0F),
    ("BracketLeft", 0x1A),
    ("BracketRight", 0x1B),
    ("Enter", 0x1C),
    ("ControlLeft", 0x1D),
    ("Semicolon", 0x27),
    ("Quote", 0x28),
    ("Backquote", 0x29),
    ("ShiftLeft", 0x2A),
    ("Backslash", 0x2B),
    ("Comma", 0x33),
    ("Period", 0x34),
    ("Slash", 0x35),
    ("ShiftRight", 0x36),
    ("NumpadMultiply", 0x37),
    ("AltLeft", 0x38),
    ("Space", 0x39),
    ("CapsLock", 0x3A),
    ("IntlBackslash", 0x56),
    ("NumLock", 0x45),
    ("ScrollLock", 0x46),
    ("NumpadSubtract", 0x4A),
    ("NumpadAdd", 0x4E),
    ("NumpadDecimal", 0x53),
    ("NumpadEqual", 0x59),
    ("IntlRo", 0x73),
    ("IntlYen", 0x7D),
    ("NumpadComma", 0x7E),
    ("NumpadEnter", 0xE01C),
    ("NumpadDivide", 0xE035),
    ("PrintScreen", 0xE037),
    ("ControlRight", 0xE01D),
    ("AltRight", 0xE038),
    ("Home", 0xE047),
    ("ArrowUp", 0xE048),
    ("PageUp", 0xE049),
    ("ArrowLeft", 0xE04B),
    ("ArrowRight", 0xE04D),
    ("End", 0xE04F),
    ("ArrowDown", 0xE050),
    ("PageDown", 0xE051),
    ("Insert", 0xE052),
    ("Delete", 0xE053),
    ("MetaLeft", 0xE05B),
    ("MetaRight", 0xE05C),
    ("ContextMenu", 0xE05D),
];

/// PC scancode set 1 (0xE0xx = extended) for a DOM `KeyboardEvent.code`.
fn scancode(code: &str) -> Option<u16> {
    if let Some(k) = code.strip_prefix("Key") {
        return ROWS.iter().find_map(|(row, base)| row.find(k).filter(|_| k.len() == 1).map(|i| base + i as u16));
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return match d.parse::<u16>().ok()? {
            0 => Some(0x0B),
            n => Some(0x01 + n),
        };
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        return match n {
            1..=10 => Some(0x3A + n),
            11 | 12 => Some(0x57 + n - 11),
            13..=23 => Some(0x64 + n - 13),
            24 => Some(0x76),
            _ => None,
        };
    }
    if let Some(d) = code.strip_prefix("Numpad").and_then(|d| d.parse::<usize>().ok()).filter(|d| *d <= 9) {
        return Some(NUMPAD[d]);
    }
    NAMED.iter().find(|(c, _)| *c == code).map(|&(_, s)| s)
}

/// The DOM `KeyboardEvent.code` of a scancode (`scancode` the other way), for keys the keyboard
/// hook takes (`grab`).
#[cfg_attr(not(windows), allow(dead_code))]
fn code_of(sc: u16) -> Option<String> {
    if let Some((c, _)) = NAMED.iter().find(|&&(_, s)| s == sc) {
        return Some(c.to_string());
    }
    if let Some(d) = NUMPAD.iter().position(|&s| s == sc) {
        return Some(format!("Numpad{d}"));
    }
    for (row, base) in ROWS {
        if let Some(k) = sc.checked_sub(base).and_then(|i| row.chars().nth(i.into())) {
            return Some(format!("Key{k}"));
        }
    }
    Some(match sc {
        0x02..=0x0A => format!("Digit{}", sc - 0x01),
        0x0B => "Digit0".into(),
        0x3B..=0x44 => format!("F{}", sc - 0x3A),
        0x57 | 0x58 => format!("F{}", sc - 0x57 + 11),
        0x64..=0x6E => format!("F{}", sc - 0x64 + 13),
        0x76 => "F24".into(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// Frames paint their changed areas into the picture screenshots are taken from; areas
    /// reaching past the edge are cut.
    #[test]
    fn pictures_follow_frames() {
        let mut p = super::Picture::new(4, 3);
        let area = |x: u16, y: u16, w: u16, h: u16, v: u8| {
            let mut m = vec![];
            [x, y, w, h].iter().for_each(|n| m.extend_from_slice(&n.to_le_bytes()));
            m.extend(std::iter::repeat_n(v, w as usize * h as usize * 4));
            m
        };
        let mut body = area(1, 1, 2, 1, 9);
        body.extend(area(3, 2, 3, 3, 7)); // past the right and bottom edges
        p.paint(&body);
        let at = |x: usize, y: usize| p.px[(y * 4 + x) * 4];
        assert_eq!((at(0, 1), at(1, 1), at(2, 1), at(3, 1)), (0, 9, 9, 0));
        assert_eq!((at(3, 2), at(2, 2)), (7, 0));
        p.paint(&[1, 0, 0]); // a cut-off message changes nothing
    }

    /// Pictures cross the clipboard as CF_DIB and come back as they went; Windows' own forms
    /// (24-bit with padded rows, top-down, alpha left at 0) read right too.
    #[test]
    fn clipboard_pictures() {
        // 3x2: red, green, blue / white, black, half-transparent grey.
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 9, 9, 9, 128];
        let dib = super::to_dib(3, 2, &rgba);
        assert_eq!(dib.len(), 40 + 24);
        assert_eq!(dib[40..44], [255, 255, 255, 255]); // bottom row first, as BGRA
        assert_eq!(super::from_dib(&dib), Some((3, 2, rgba.to_vec())));

        // 24-bit, top-down (negative height): rows padded to 4 bytes.
        let mut d = super::to_dib(2, 1, &[0; 8])[..40].to_vec();
        d[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        d[14] = 24;
        d.extend_from_slice(&[1, 2, 3, 4, 5, 6, 0, 0]);
        assert_eq!(super::from_dib(&d), Some((2, 1, vec![3, 2, 1, 255, 6, 5, 4, 255])));

        // 32-bit with alpha all 0: opaque.
        let zero = super::to_dib(1, 1, &[7, 8, 9, 0]);
        assert_eq!(super::from_dib(&zero), Some((1, 1, vec![7, 8, 9, 255])));
        assert_eq!(super::from_dib(&dib[..50]), None); // cut short
    }

    /// Big frames go deflated and inflate to the same areas; small ones go as they are.
    #[test]
    fn big_frames_go_packed() {
        use std::io::Read as _;
        let body: Vec<u8> = [0u16, 0, 128, 64].iter().flat_map(|n| n.to_le_bytes()).chain((0..128 * 64 * 4).map(|i| (i / 512) as u8)).collect();
        let m = super::frame_msg(&body);
        assert_eq!(m[0], super::MSG_FRAME_PACKED);
        assert!(m.len() < body.len() / 4);
        let mut back = vec![];
        flate2::read::ZlibDecoder::new(&m[1..]).read_to_end(&mut back).unwrap();
        assert_eq!(back, body);
        assert_eq!(super::frame_msg(&body[..100])[..], [&[super::MSG_FRAME][..], &body[..100]].concat()[..]);
    }

    /// A window or Co-op viewer that starts watching gets the status, the size, then the whole
    /// picture as one frame; before the desktop is there it gets only the status.
    #[test]
    fn a_new_watcher_is_caught_up() {
        let mut p = super::Picture::new(2, 1);
        p.px[0..4].copy_from_slice(&[1, 2, 3, 4]);
        let status = [super::MSG_STATUS, 0];
        let m = super::catch_up(&status, &p);
        assert_eq!(m[0], status);
        assert_eq!(m[1], [super::MSG_SIZE, 2, 0, 1, 0]);
        assert_eq!(m[2][..9], [super::MSG_FRAME, 0, 0, 0, 0, 2, 0, 1, 0]);
        assert_eq!(m[2][9..13], [1, 2, 3, 4]);
        assert_eq!(super::catch_up(&status, &super::Picture::default()), vec![status.to_vec()]);
        assert!(super::catch_up(&[], &super::Picture::default()).is_empty());
    }

    #[test]
    fn split_user() {
        assert_eq!(super::split_user(r".\Administrator"), ("".into(), "Administrator".into()));
        assert_eq!(super::split_user(r"CORP\jdoe"), ("CORP".into(), "jdoe".into()));
        assert_eq!(super::split_user("jdoe@corp.example"), ("".into(), "jdoe@corp.example".into()));
    }

    /// The keyboard hook's scancodes become the codes the view would have sent, and back.
    #[test]
    fn scancodes_both_ways() {
        for sc in (0..0x100).chain(0xE000..0xE100) {
            if let Some(code) = super::code_of(sc) {
                assert_eq!(super::scancode(&code), Some(sc), "{code}");
            }
        }
        let code = |sc| super::code_of(sc).unwrap();
        assert_eq!([code(0xE05B), code(0x12), code(0x0F), code(0x3E), code(0x48), code(0xE048)], ["MetaLeft", "KeyE", "Tab", "F4", "Numpad8", "ArrowUp"]);
    }

    #[test]
    fn scancodes() {
        let s = super::scancode;
        assert_eq!(
            (s("KeyQ"), s("KeyP"), s("KeyA"), s("KeyL"), s("KeyZ"), s("KeyM")),
            (Some(0x10), Some(0x19), Some(0x1E), Some(0x26), Some(0x2C), Some(0x32))
        );
        assert_eq!((s("Digit1"), s("Digit9"), s("Digit0")), (Some(0x02), Some(0x0A), Some(0x0B)));
        assert_eq!(
            (s("F1"), s("F10"), s("F11"), s("F12"), s("F13"), s("F23"), s("F24"), s("F25")),
            (Some(0x3B), Some(0x44), Some(0x57), Some(0x58), Some(0x64), Some(0x6E), Some(0x76), None)
        );
        assert_eq!((s("ArrowUp"), s("KeyQW"), s("Fn")), (Some(0xE048), None, None));
        assert_eq!(
            (s("Numpad0"), s("Numpad1"), s("Numpad5"), s("Numpad9"), s("NumpadDivide"), s("Numpad10")),
            (Some(0x52), Some(0x4F), Some(0x4C), Some(0x49), Some(0xE035), None)
        );
    }

    /// Line breaks (any kind) and tabs are typed as keys, the rest as characters.
    #[test]
    fn typing() {
        use crate::rdp_proto::ToHelper::{Key, Unicode};
        let keys = |s| [Key { scancode: s, pressed: true }, Key { scancode: s, pressed: false }];
        let mut want = vec![Unicode('a')];
        want.extend(keys(0x1C));
        want.push(Unicode('b'));
        want.extend(keys(0x1C));
        want.extend(keys(0x0F));
        want.extend(keys(0x1C));
        assert_eq!(super::typing("a\r\nb\n\t\r"), want);
    }
}
