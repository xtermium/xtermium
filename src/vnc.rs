//! Embedded VNC tab (RFB, via vnc-rs). It speaks to the UI like an RDP tab (rdp.rs): the desktop
//! size, changed areas as RGBA, and a status line; and takes the same input, turned into X11
//! keysyms and pointer events.

use crate::config::{self, Session};
use crate::prompt::Prompter;
use crate::rdp::Input;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use vnc::{PixelFormat, Rect, VncConnector, VncEncoding, VncEvent, X11Event};

// The same messages as an RDP tab (rdp.rs).
const MSG_SIZE: u8 = 2;
const MSG_FRAME: u8 = 3;
const MSG_STATUS: u8 = 4;

/// What the UI asked for, handed to the session task.
enum Ctl {
    Input(Input),
    /// Automation wants a screenshot: the screen as it is (None before it's connected).
    Shot(tokio::sync::oneshot::Sender<Option<crate::rdp::Picture>>),
    Close,
}

pub struct VncTab {
    pub session: Session,
    tx: UnboundedSender<Ctl>,
    /// Where the file panel's SFTP goes: the desktop's host on port 22, as its shared login's
    /// user, and the login's gateway (main.rs `open_rdp`).
    pub sftp: Option<(config::Endpoint, Option<config::Endpoint>)>,
    pub files: Option<crate::files::Browser>,
}

/// The largest remote desktop taken, in pixels: 8192 × 8192 (three 4K screens fit), 256 MB as RGBA.
const MAX_PIXELS: usize = 1 << 26;

/// The remote screen as RGBA, kept to serve CopyRect and to send changed areas from.
struct Screen {
    w: usize,
    h: usize,
    px: Vec<u8>,
    dirty: Vec<Rect>,
}

impl Screen {
    fn new(w: u16, h: u16) -> Self {
        Self { w: w as usize, h: h as usize, px: vec![0; w as usize * h as usize * 4], dirty: vec![] }
    }

    /// `data`: the rectangle's pixels as R, G, B, unused (the pixel format asked for).
    fn put(&mut self, r: Rect, data: &[u8]) {
        let rw = r.width as usize;
        for row in 0..r.height as usize {
            let (y, x) = (r.y as usize + row, r.x as usize);
            if y >= self.h || x >= self.w {
                continue;
            }
            let n = rw.min(self.w - x);
            let src = data.get(row * rw * 4..(row * rw + n) * 4).unwrap_or(&[]);
            let dst = &mut self.px[(y * self.w + x) * 4..][..src.len()];
            dst.copy_from_slice(src);
            dst.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
        }
        self.dirty.push(r);
    }

    fn copy(&mut self, dst: Rect, src: Rect) {
        let rows: Vec<Vec<u8>> = (0..src.height as usize)
            .map(|row| {
                let (y, x) = (src.y as usize + row, src.x as usize);
                let n = (src.width as usize).min(self.w.saturating_sub(x));
                // A source outside the screen (a confused server) copies nothing.
                if y < self.h && x < self.w { self.px[(y * self.w + x) * 4..][..n * 4].to_vec() } else { vec![] }
            })
            .collect();
        for (row, line) in rows.into_iter().enumerate() {
            let (y, x) = (dst.y as usize + row, dst.x as usize);
            if y >= self.h || x >= self.w {
                continue;
            }
            let n = line.len().min((self.w - x) * 4);
            self.px[(y * self.w + x) * 4..][..n].copy_from_slice(&line[..n]);
        }
        self.dirty.push(dst);
    }

    /// `[MSG_FRAME]` then each changed area `[x][y][w][h: u16 LE][RGBA]`; None if nothing changed.
    fn frame(&mut self) -> Option<Vec<u8>> {
        if self.dirty.is_empty() {
            return None;
        }
        // Many small areas: one covering them all is cheaper to send and to draw.
        if self.dirty.len() > 64 {
            let (x0, y0) = self.dirty.iter().fold((u16::MAX, u16::MAX), |(x, y), r| (x.min(r.x), y.min(r.y)));
            // Wide enough not to overflow on areas reaching past 65535 (the server says where).
            let end = |at: u16, len: u16| u32::from(at) + u32::from(len);
            let (x1, y1) = self.dirty.iter().fold((0, 0), |(x, y), r| (x.max(end(r.x, r.width)), y.max(end(r.y, r.height))));
            let span = |from: u16, to: u32| (to.saturating_sub(from.into())).min(u16::MAX.into()) as u16;
            self.dirty = vec![Rect { x: x0, y: y0, width: span(x0, x1), height: span(y0, y1) }];
        }
        let mut m = vec![MSG_FRAME];
        for r in self.dirty.drain(..) {
            let x = (r.x as usize).min(self.w);
            let y = (r.y as usize).min(self.h);
            let w = (r.width as usize).min(self.w - x);
            let h = (r.height as usize).min(self.h - y);
            if w == 0 || h == 0 {
                continue;
            }
            for v in [x, y, w, h] {
                m.extend_from_slice(&(v as u16).to_le_bytes());
            }
            for row in y..y + h {
                m.extend_from_slice(&self.px[(row * self.w + x) * 4..][..w * 4]);
            }
        }
        (m.len() > 1).then_some(m)
    }
}

impl VncTab {
    /// Connects right away; the UI sees "connecting…", then the desktop.
    /// `password`: a shared login's (Credentials), used instead of the keychain or asking;
    /// `gateway`: its SSH gateway, which the server is reached through.
    pub fn open(session: Session, prompt: Prompter, out: Channel, password: Option<String>, gateway: Option<config::Endpoint>) -> Self {
        let (tx, mut rx) = unbounded_channel();
        let config::Kind::Vnc { host, port, .. } = session.kind.clone() else { unreachable!("not VNC") };
        tauri::async_runtime::spawn(async move {
            let status = |s: &str, closed: bool| {
                let mut m = vec![MSG_STATUS, closed as u8];
                m.extend_from_slice(s.as_bytes());
                let _ = out.send(InvokeResponseBody::Raw(m));
            };
            status("connecting…", false);
            let r = run(&host, port, gateway.as_ref(), password, &prompt, &out, &mut rx).await;
            status(&r.err().map_or("disconnected".to_string(), |e| format!("{e:#}")), true);
        });
        Self { session, tx, sftp: None, files: None }
    }

    pub fn input(&self, ev: Input) {
        let _ = self.tx.send(Ctl::Input(ev));
    }

    /// What the desktop shows now; None before it's connected (or after it closed). The future
    /// doesn't borrow the tab, so the tab list's lock needn't be held while it waits.
    pub fn screenshot(&self) -> impl Future<Output = Option<crate::rdp::Picture>> + use<> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let sent = self.tx.send(Ctl::Shot(tx)).is_ok();
        async move { if sent { rx.await.ok().flatten() } else { None } }
    }
}

impl Drop for VncTab {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl::Close);
    }
}

/// The session: until the server hangs up or the tab closes (both Ok).
async fn run(
    host: &str,
    port: u16,
    gateway: Option<&config::Endpoint>,
    shared: Option<String>,
    prompt: &Prompter,
    out: &Channel,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Ctl>,
) -> anyhow::Result<()> {
    use anyhow::Context;
    // Through a gateway: a local port forwarded to the server (kept open while this runs).
    let fwd = match gateway {
        Some(gw) => Some(crate::ssh::forward(gw, Some((host.to_string(), port)), prompt).await?),
        None => None,
    };
    let to = fwd.as_ref().map_or((host, port), |f| ("127.0.0.1", f.port));
    let tcp = tokio::time::timeout(Duration::from_secs(15), tokio::net::TcpStream::connect(to))
        .await
        .map_err(|_| anyhow::anyhow!("{host}:{port} didn't answer within 15 s"))?
        .with_context(|| format!("connecting to {host}:{port}"))?;
    tcp.set_nodelay(true)?;
    // The VNC password: a saved one, else asked for (only if the server wants one).
    let login_id = format!("vnc:{host}:{port}");
    let from_saved = shared.is_none();
    let stored = shared.or_else(|| crate::config::login(&login_id));
    let asked: Arc<Mutex<Option<(String, bool)>>> = Default::default();
    let auth = {
        let (prompt, asked, stored, who) = (prompt.clone(), asked.clone(), stored.clone(), format!("{host}:{port}"));
        async move {
            if let Some(p) = stored {
                return Ok(p);
            }
            let answer = prompt.ask("Password", format!("VNC password for {who}:"), true).await;
            let (p, remember) = answer.ok_or(vnc::VncError::General("cancelled".into()))?;
            *asked.lock().unwrap() = Some((p.clone(), remember));
            Ok(p)
        }
    };
    let started = VncConnector::new(tcp)
        .set_auth_method(auth)
        .add_encoding(VncEncoding::Zrle)
        .add_encoding(VncEncoding::Trle)
        .add_encoding(VncEncoding::CopyRect)
        .add_encoding(VncEncoding::Raw)
        .add_encoding(VncEncoding::DesktopSizePseudo)
        .add_encoding(VncEncoding::ExtendedDesktopSizePseudo)
        .allow_shared(true)
        .set_pixel_format(PixelFormat::rgba())
        .build()?
        .try_start()
        .await;
    let vnc = match started.and_then(|s| s.finish()) {
        Ok(v) => v,
        Err(e) => {
            // A remembered password that no longer works is forgotten.
            if stored.is_some() && from_saved {
                crate::config::forget(&login_id);
            }
            anyhow::bail!("{e}");
        }
    };
    if let Some((p, true)) = asked.lock().unwrap().take() {
        let _ = crate::config::remember(&login_id, &p);
    }

    let mut screen: Option<Screen> = None;
    let mut held = Held::default();
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                // Events are polled: waiting for one would hold the client's lock against input.
                while let Some(ev) = vnc.poll_event().await? {
                    let size = match &ev {
                        VncEvent::SetResolution(s) => Some((s.width, s.height)),
                        VncEvent::DesktopUpdate(u) => u.layout.as_ref().map(|l| (l.width, l.height)),
                        _ => None,
                    };
                    // A new desktop size (the first, or the server followed a resize): a new screen,
                    // painted whole.
                    if let Some((w, h)) = size
                        && screen.as_ref().is_none_or(|s| (s.w, s.h) != (w as usize, h as usize))
                    {
                        // The server names the size: a picture this big would take gigabytes,
                        // and failing to get them ends all of xtermium, not just this tab.
                        if usize::from(w) * usize::from(h) > MAX_PIXELS {
                            anyhow::bail!("the remote desktop is too large ({w} × {h})");
                        }
                        let first = screen.is_none();
                        screen = Some(Screen::new(w, h));
                        let mut m = vec![MSG_SIZE];
                        m.extend_from_slice(&w.to_le_bytes());
                        m.extend_from_slice(&h.to_le_bytes());
                        let _ = out.send(InvokeResponseBody::Raw(m));
                        let _ = out.send(InvokeResponseBody::Raw(vec![MSG_STATUS, 0]));
                        if !first {
                            vnc.input(X11Event::FullRefresh).await?;
                        }
                    }
                    match ev {
                        VncEvent::RawImage(r, data) => {
                            if let Some(s) = &mut screen {
                                s.put(r, &data);
                            }
                        }
                        VncEvent::Copy(dst, src) => {
                            if let Some(s) = &mut screen {
                                s.copy(dst, src);
                            }
                        }
                        VncEvent::Error(e) => anyhow::bail!("{e}"),
                        _ => {}
                    }
                }
                if let Some(m) = screen.as_mut().and_then(Screen::frame)
                    && out.send(InvokeResponseBody::Raw(m)).is_err()
                {
                    break;
                }
                vnc.input(X11Event::Refresh).await?;
            }
            ctl = rx.recv() => match ctl {
                Some(Ctl::Input(Input::Resize { width, height })) => {
                    // The desktop follows the pane if the server can (ExtendedDesktopSize); the
                    // answer can take seconds, and events must keep flowing meanwhile.
                    let v = vnc.clone();
                    tokio::spawn(async move { v.resize_desktop(width, height).await });
                }
                Some(Ctl::Input(ev)) => {
                    for e in events(ev, &mut held) {
                        vnc.input(e).await?;
                    }
                }
                Some(Ctl::Shot(reply)) => {
                    let _ = reply.send(screen.as_ref().map(|s| crate::rdp::Picture { w: s.w, h: s.h, px: s.px.clone() }));
                }
                Some(Ctl::Close) | None => break,
            },
        }
    }
    let _ = vnc.close().await;
    Ok(())
}

/// What the desktop holds from us: the pointer (position and buttons, which every pointer event
/// repeats), the keys down, and the part of a wheel turn too small for a step yet (trackpads
/// scroll in slivers).
#[derive(Default)]
struct Held {
    pointer: (u16, u16, u8),
    keys: std::collections::BTreeSet<u32>,
    wheel: [f32; 2],
}

/// The X11 events for one UI input.
fn events(ev: Input, h: &mut Held) -> Vec<X11Event> {
    let key = |sym: u32, down| X11Event::KeyEvent((sym, down).into());
    let point = |p: &(u16, u16, u8)| X11Event::PointerEvent((p.0, p.1, p.2).into());
    let typed = |text: &str| text.chars().flat_map(|c| [key(char_sym(c), true), key(char_sym(c), false)]).collect::<Vec<_>>();
    let pointer = &mut h.pointer;
    match ev {
        Input::Key { code, down } => {
            let Some(k) = keysym(&code) else { return vec![] };
            if down { h.keys.insert(k) } else { h.keys.remove(&k) };
            vec![key(k, down)]
        }
        Input::Text { text } => typed(&text),
        Input::Move { x, y } => {
            (pointer.0, pointer.1) = (x, y);
            vec![point(pointer)]
        }
        Input::Button { button, down, x, y } => {
            // DOM 0 left, 1 middle, 2 right, 3 back; RFB buttons 1, 2, 3 and 8 are mask bits 0, 1,
            // 2 and 7. Forward (button 9) doesn't fit the mask.
            let bit = match button {
                0..=2 => 1 << button,
                3 => 1 << 7,
                _ => return vec![],
            };
            (pointer.0, pointer.1) = (x, y);
            pointer.2 = if down { pointer.2 | bit } else { pointer.2 & !bit };
            vec![point(pointer)]
        }
        Input::Wheel { dx, dy } => {
            // Wheel steps are buttons 4 (up), 5 (down), 6 (left), 7 (right), pressed and let go.
            let mut v = vec![];
            for (i, (amount, plus, minus)) in [(dy, 3, 4), (dx, 6, 5)].into_iter().enumerate() {
                // A turn the other way starts afresh.
                if h.wheel[i] * amount < 0.0 {
                    h.wheel[i] = 0.0;
                }
                h.wheel[i] += amount;
                let steps = h.wheel[i].trunc();
                h.wheel[i] -= steps;
                let bit = 1u8 << if steps > 0.0 { plus } else { minus };
                for _ in 0..(steps.abs() as usize).min(10) {
                    v.push(point(&(pointer.0, pointer.1, pointer.2 | bit)));
                    v.push(point(pointer));
                }
            }
            v
        }
        Input::Paste => {
            // Let go of the paste chord's Shift and Ctrl first, or the text arrives shifted.
            let mods = [0xffe1, 0xffe2, 0xffe3, 0xffe4];
            mods.iter().for_each(|k| _ = h.keys.remove(k));
            let mut v: Vec<X11Event> = mods.into_iter().map(|k| key(k, false)).collect();
            if let Some(s) = crate::term::clipboard(|c| c.get_text()) {
                v.extend(typed(&s.replace("\r\n", "\n")));
            }
            v
        }
        Input::Release => {
            let mut v: Vec<X11Event> = std::mem::take(&mut h.keys).into_iter().map(|k| key(k, false)).collect();
            if pointer.2 != 0 {
                pointer.2 = 0;
                v.push(point(pointer));
            }
            v
        }
        // RFB has no way to set the lock keys.
        Input::Sync { .. } => vec![],
        Input::Resize { .. } => vec![], // handled by the session (`run`)
    }
}

/// The keysym for a typed character: Latin-1 as it is, line breaks as Return, the rest Unicode.
fn char_sym(c: char) -> u32 {
    match c as u32 {
        0x0a | 0x0d => 0xff0d,
        0x09 => 0xff09,
        n @ (0x20..=0x7e | 0xa0..=0xff) => n,
        n => 0x0100_0000 | n,
    }
}

/// The X11 keysym for a DOM `KeyboardEvent.code` (keys sent as keys: specials and chords).
fn keysym(code: &str) -> Option<u32> {
    if let Some(k) = code.strip_prefix("Key").filter(|k| k.len() == 1) {
        return Some(k.to_ascii_lowercase().as_bytes()[0] as u32);
    }
    if let Some(d) = code.strip_prefix("Digit").filter(|d| d.len() == 1) {
        return Some(d.as_bytes()[0] as u32);
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=24).contains(n)) {
        return Some(0xffbd + n);
    }
    if let Some(d) = code.strip_prefix("Numpad").and_then(|d| d.parse::<u32>().ok()).filter(|d| *d <= 9) {
        return Some(0xffb0 + d);
    }
    Some(match code {
        "Enter" => 0xff0d,
        "NumpadEnter" => 0xff8d,
        "Backspace" => 0xff08,
        "Tab" => 0xff09,
        "Escape" => 0xff1b,
        "Delete" => 0xffff,
        "Insert" => 0xff63,
        "Home" => 0xff50,
        "End" => 0xff57,
        "PageUp" => 0xff55,
        "PageDown" => 0xff56,
        "ArrowLeft" => 0xff51,
        "ArrowUp" => 0xff52,
        "ArrowRight" => 0xff53,
        "ArrowDown" => 0xff54,
        "ShiftLeft" => 0xffe1,
        "ShiftRight" => 0xffe2,
        "ControlLeft" => 0xffe3,
        "ControlRight" => 0xffe4,
        "CapsLock" => 0xffe5,
        "AltLeft" => 0xffe9,
        "AltRight" => 0xffea,
        "MetaLeft" => 0xffeb,
        "MetaRight" => 0xffec,
        "ContextMenu" => 0xff67,
        "Space" => 0x20,
        "Minus" => b'-' as u32,
        "Equal" => b'=' as u32,
        "BracketLeft" => b'[' as u32,
        "BracketRight" => b']' as u32,
        "Backslash" | "IntlBackslash" => b'\\' as u32,
        "Semicolon" => b';' as u32,
        "Quote" => b'\'' as u32,
        "Backquote" => b'`' as u32,
        "Comma" => b',' as u32,
        "Period" => b'.' as u32,
        "Slash" => b'/' as u32,
        "NumpadAdd" => 0xffab,
        "NumpadSubtract" => 0xffad,
        "NumpadMultiply" => 0xffaa,
        "NumpadDivide" => 0xffaf,
        "NumpadDecimal" => 0xffae,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms() {
        assert_eq!(
            [keysym("KeyA"), keysym("Digit7"), keysym("F1"), keysym("F12"), keysym("Enter"), keysym("Fn")],
            [Some(0x61), Some(0x37), Some(0xffbe), Some(0xffc9), Some(0xff0d), None]
        );
        assert_eq!([char_sym('a'), char_sym('é'), char_sym('€'), char_sym('\n')], [0x61, 0xe9, 0x0100_20ac, 0xff0d]);
    }

    #[test]
    fn pointer_buttons_and_wheel() {
        let mut p = Held::default();
        let mask = |evs: Vec<X11Event>| -> Vec<u8> {
            evs.into_iter()
                .filter_map(|e| match e {
                    X11Event::PointerEvent(m) => Some(m.bottons),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(mask(events(Input::Button { button: 0, down: true, x: 5, y: 6 }, &mut p)), [1]);
        assert_eq!(mask(events(Input::Button { button: 2, down: true, x: 5, y: 6 }, &mut p)), [5]);
        assert_eq!(mask(events(Input::Button { button: 0, down: false, x: 5, y: 6 }, &mut p)), [4]);
        // Two notches up while the right button is held: button 4 pressed and let go twice.
        assert_eq!(mask(events(Input::Wheel { dx: 0.0, dy: 2.0 }, &mut p)), [12, 4, 12, 4]);
        assert_eq!(p.pointer, (5, 6, 4));
        // Slivers add up to a step; turning back starts afresh.
        assert!(mask(events(Input::Wheel { dx: 0.0, dy: 0.6 }, &mut p)).is_empty());
        assert_eq!(mask(events(Input::Wheel { dx: 0.0, dy: 0.6 }, &mut p)), [12, 4]);
        assert!(mask(events(Input::Wheel { dx: 0.0, dy: -0.6 }, &mut p)).is_empty());
        // Back is button 8; letting go of everything lifts it and the right button, and the keys.
        assert_eq!(mask(events(Input::Button { button: 3, down: true, x: 5, y: 6 }, &mut p)), [132]);
        events(Input::Key { code: "ControlLeft".into(), down: true }, &mut p);
        let up = events(Input::Release, &mut p);
        assert!(matches!(up[..], [X11Event::KeyEvent(_), X11Event::PointerEvent(_)]), "{up:?}");
        assert_eq!(mask(up), [0]);
        assert!(events(Input::Release, &mut p).is_empty());
    }

    #[test]
    fn screen_updates() {
        let mut s = Screen::new(4, 3);
        let red = [255, 0, 0, 0].repeat(2 * 2);
        s.put(Rect { x: 1, y: 1, width: 2, height: 2 }, &red);
        s.copy(Rect { x: 0, y: 0, width: 1, height: 1 }, Rect { x: 1, y: 1, width: 1, height: 1 });
        let m = s.frame().unwrap();
        assert_eq!(m[0], MSG_FRAME);
        // First area: 2x2 at (1, 1), opaque red.
        assert_eq!(&m[1..9], [1, 0, 1, 0, 2, 0, 2, 0]);
        assert_eq!(&m[9..13], [255, 0, 0, 255]);
        // Then the copied pixel at (0, 0).
        assert_eq!(&m[25..37], [0, 0, 0, 0, 1, 0, 1, 0, 255, 0, 0, 255]);
        assert!(s.frame().is_none(), "nothing new");
    }

    /// What a confused or hostile server sends can't panic the session.
    #[test]
    fn odd_areas_are_harmless() {
        let mut s = Screen::new(4, 3);
        s.copy(Rect { x: 0, y: 0, width: 2, height: 2 }, Rect { x: 9, y: 2, width: 2, height: 2 });
        for _ in 0..70 {
            s.put(Rect { x: 65000, y: 65000, width: 1000, height: 1000 }, &[]);
        }
        assert!(s.frame().is_some(), "the copy's area still goes out");
    }
}
