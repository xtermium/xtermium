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
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeResponseBody};

/// Message types on an RDP tab's channel (first byte). Integers are little-endian.
/// `[2][width: u16][height: u16]` the desktop size; `[3]` then changed areas back to back, each
/// `[x][y][w][h: u16][w*h RGBA pixels]`; `[4][closed: u8][utf-8 status]`.
const MSG_SIZE: u8 = 2;
const MSG_FRAME: u8 = 3;
const MSG_STATUS: u8 = 4;
/// The session ended from inside (signed out): the UI closes the tab.
const MSG_ENDED: u8 = 5;

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
        };
        let config::Kind::Rdp { host, port, user, .. } = tab.session.kind.clone() else { unreachable!("not RDP") };
        let (stdin, child, login, picture) = (tab.stdin.clone(), tab.child.clone(), tab.login.clone(), tab.picture.clone());
        let (out, last_status) = (tab.out.clone(), tab.status.clone());
        std::thread::spawn(move || {
            // A window gone (the tab moving to another) misses messages until `attach` catches it up.
            let ui = |m: Vec<u8>| {
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
                        // Painted and sent under one lock: `attach` sends the whole picture
                        // between frames, never half of one.
                        let mut p = picture.lock().unwrap();
                        p.paint(&body);
                        let mut m = Vec::with_capacity(1 + body.len());
                        m.push(MSG_FRAME);
                        m.extend_from_slice(&body);
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
        let send = |m: Vec<u8>| {
            let _ = out.send(InvokeResponseBody::Raw(m));
        };
        let status = self.status.lock().unwrap().clone();
        if !status.is_empty() {
            send(status);
        }
        if p.w > 0 {
            send(size_msg(p.w as u16, p.h as u16));
            let mut m = Vec::with_capacity(9 + p.px.len());
            m.push(MSG_FRAME);
            for v in [0, 0, p.w as u16, p.h as u16] {
                m.extend_from_slice(&v.to_le_bytes());
            }
            m.extend_from_slice(&p.px);
            send(m);
        }
        *self.out.lock().unwrap() = out;
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

    fn send(&self, m: ToHelper) {
        if let Some(w) = self.stdin.lock().unwrap().as_mut() {
            let _ = m.write(w);
        }
    }

    pub fn input(&self, ev: Input) {
        match ev {
            Input::Key { code, down } => {
                use std::sync::atomic::Ordering::Relaxed;
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
            Input::Text { text } => typing(&text).into_iter().for_each(|m| self.send(m)),
            Input::Move { x, y } => self.send(ToHelper::MouseMove { x, y }),
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
                    typing(&s).into_iter().for_each(|m| self.send(m));
                }
            }
            Input::Resize { width, height } => self.send(ToHelper::Resize { width, height }),
            Input::Release => {
                self.win.store(0, std::sync::atomic::Ordering::Relaxed);
                self.send(ToHelper::ReleaseAll);
            }
            Input::Sync { caps, num, scroll } => self.send(ToHelper::Sync { caps, num, scroll }),
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

/// The UI says whether a remote desktop has the keyboard: its Windows keys then go to the desktop
/// instead of opening the Start menu here (Windows), as `win-key` events `[right, down]`.
#[tauri::command]
pub fn desktop_keys(app: tauri::AppHandle, on: bool) {
    #[cfg(windows)]
    win_key::grab(&app, on);
    #[cfg(not(windows))]
    let _ = (app, on);
}

/// A low-level keyboard hook takes the Windows keys from the system while a desktop has the
/// keyboard and xtermium is in front. Windows never sees them down, so the keys pressed with one
/// held reach the desktop too, and nothing stays stuck when the key comes back up.
#[cfg(windows)]
mod win_key {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    use std::sync::mpsc::{Sender, channel};
    use tauri::{AppHandle, Emitter};
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LWIN, VK_RWIN};
    use windows::Win32::UI::WindowsAndMessaging::*;

    static GRAB: AtomicBool = AtomicBool::new(false);
    /// A Windows key went down to the desktop: its release goes there too.
    static HELD: AtomicBool = AtomicBool::new(false);
    /// Where the hook hands the keys it takes, `[right, down]`.
    static KEYS: OnceLock<Sender<(bool, bool)>> = OnceLock::new();

    pub fn grab(app: &AppHandle, on: bool) {
        GRAB.store(on, Relaxed);
        let (tx, rx) = channel();
        if on && KEYS.set(tx).is_ok() {
            // The hook must answer Windows at once: one that keeps it waiting is dropped without
            // a word, and the Windows key then opens the Start menu here. So it only hands the
            // key over, and this thread tells the UI.
            let app = app.clone();
            std::thread::spawn(move || rx.into_iter().for_each(|k| _ = app.emit("win-key", k)));
            // The hook runs on the thread that set it, which must pump messages.
            std::thread::spawn(|| unsafe {
                let module = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
                if SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0).is_ok() {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                }
            });
        }
    }

    fn in_front() -> bool {
        let mut pid = 0;
        // SAFETY: plain queries about the foreground window.
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        pid == std::process::id()
    }

    unsafe extern "system" fn hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: for HC_ACTION, lparam points at the key's KBDLLHOOKSTRUCT.
        let k = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        let right = k.vkCode == VK_RWIN.0 as u32;
        let win = right || k.vkCode == VK_LWIN.0 as u32;
        if code == HC_ACTION as i32 && win && !k.flags.contains(LLKHF_INJECTED) {
            let down = matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            if HELD.load(Relaxed) || (down && GRAB.load(Relaxed) && in_front()) {
                HELD.store(down, Relaxed);
                if let Some(keys) = KEYS.get() {
                    let _ = keys.send((right, down));
                }
                return LRESULT(1);
            }
        }
        unsafe { CallNextHookEx(None, code, w, l) }
    }
}

fn size_msg(width: u16, height: u16) -> Vec<u8> {
    let mut m = vec![MSG_SIZE];
    m.extend_from_slice(&width.to_le_bytes());
    m.extend_from_slice(&height.to_le_bytes());
    m
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

/// PC scancode set 1 (0xE0xx = extended) for a DOM `KeyboardEvent.code`.
fn scancode(code: &str) -> Option<u16> {
    const ROWS: [(&str, u16); 3] = [("QWERTYUIOP", 0x10), ("ASDFGHJKL", 0x1E), ("ZXCVBNM", 0x2C)];
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
    // The numpad as keys (Num Lock off they are the arrows, Home, End…), and with Alt held the
    // digits of a character code.
    if let Some(d) = code.strip_prefix("Numpad").and_then(|d| d.parse::<usize>().ok()).filter(|d| *d <= 9) {
        return Some([0x52, 0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48, 0x49][d]);
    }
    Some(match code {
        "Escape" => 0x01,
        "Minus" => 0x0C,
        "Equal" => 0x0D,
        "Backspace" => 0x0E,
        "Tab" => 0x0F,
        "BracketLeft" => 0x1A,
        "BracketRight" => 0x1B,
        "Enter" => 0x1C,
        "ControlLeft" => 0x1D,
        "Semicolon" => 0x27,
        "Quote" => 0x28,
        "Backquote" => 0x29,
        "ShiftLeft" => 0x2A,
        "Backslash" => 0x2B,
        "Comma" => 0x33,
        "Period" => 0x34,
        "Slash" => 0x35,
        "ShiftRight" => 0x36,
        "NumpadMultiply" => 0x37,
        "AltLeft" => 0x38,
        "Space" => 0x39,
        "CapsLock" => 0x3A,
        "IntlBackslash" => 0x56,
        "NumLock" => 0x45,
        "ScrollLock" => 0x46,
        "NumpadSubtract" => 0x4A,
        "NumpadAdd" => 0x4E,
        "NumpadDecimal" => 0x53,
        "NumpadEqual" => 0x59,
        "IntlRo" => 0x73,
        "IntlYen" => 0x7D,
        "NumpadComma" => 0x7E,
        "NumpadEnter" => 0xE01C,
        "NumpadDivide" => 0xE035,
        "PrintScreen" => 0xE037,
        "ControlRight" => 0xE01D,
        "AltRight" => 0xE038,
        "Home" => 0xE047,
        "ArrowUp" => 0xE048,
        "PageUp" => 0xE049,
        "ArrowLeft" => 0xE04B,
        "ArrowRight" => 0xE04D,
        "End" => 0xE04F,
        "ArrowDown" => 0xE050,
        "PageDown" => 0xE051,
        "Insert" => 0xE052,
        "Delete" => 0xE053,
        "MetaLeft" => 0xE05B,
        "MetaRight" => 0xE05C,
        "ContextMenu" => 0xE05D,
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

    #[test]
    fn split_user() {
        assert_eq!(super::split_user(r".\Administrator"), ("".into(), "Administrator".into()));
        assert_eq!(super::split_user(r"CORP\jdoe"), ("CORP".into(), "jdoe".into()));
        assert_eq!(super::split_user("jdoe@corp.example"), ("".into(), "jdoe@corp.example".into()));
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
