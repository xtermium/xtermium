//! xtermium RDP helper: runs one IronRDP session and streams changed screen areas to xtermium
//! over stdout; input and control arrive on stdin (see `rdp_proto.rs`).
//! It is a separate process because IronRDP's crypto stack can't share a lockfile with russh.

#[allow(dead_code)] // each binary uses one direction
#[path = "../../src/rdp_proto.rs"]
mod proto;

use anyhow::{Context, Result, bail};
use ironrdp::connector::connection_activation::ConnectionActivationState;
use ironrdp::connector::{self, BitmapConfig, ClientConnector, Credentials, DesktopSize, ServerName};
use ironrdp::core::WriteBuf;
use ironrdp::displaycontrol::client::DisplayControlClient;
use ironrdp::displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp::dvc::DrdynvcClient;
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::input::{Database, MouseButton, MousePosition, Operation, Scancode, WheelRotations};
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::geometry::InclusiveRectangle;
use ironrdp::pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};
use ironrdp::pdu::rdp::capability_sets::{MajorPlatformType, client_codecs_capabilities};
use ironrdp::pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput, GracefulDisconnectReason, fast_path};
use ironrdp_tokio::{FramedWrite, TokioFramed};
use proto::{FromHelper, ToHelper};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};

/// Changed areas are collected this long and sent as one frame: far fewer messages to relay
/// and draw, and an area that changes twice is copied once. The UI draws at most once per
/// display refresh anyway.
const BATCH: Duration = Duration::from_millis(5);

/// A frame bigger than this is sent in parts (the protocol caps a message at 64 MiB).
const MAX_FRAME: usize = 32 << 20;

/// Typed text goes one character per this on average, a few at a time at most. A desktop fed a
/// paste's worth of characters at once garbles them: Windows Terminal reorders and drops some.
/// ponytail: a fixed pace, so 4 KB of text takes about 16 s; a clipboard channel would be instant.
const PACE: Duration = Duration::from_millis(4);
/// How far typing may catch up at once: timers wake about every 15 ms on Windows, so one
/// character per wake-up would be four times slower than `PACE`.
const BURST: u32 = 4;

/// A connection lost on the network is tried again this many times, after these waits (seconds).
const RETRY: [u64; 6] = [1, 2, 4, 8, 15, 30];

/// Connecting gives up after this long: a server that takes the TCP connection, then says nothing.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Ctrl, Alt and the Windows keys (scancodes).
const MODIFIERS: [u16; 6] = [0x1D, 0xE01D, 0x38, 0xE038, 0xE05B, 0xE05C];

fn main() {
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        while let Ok(m) = ToHelper::read(&mut stdin) {
            if in_tx.send(m).is_err() {
                break;
            }
        }
        // stdin closed = xtermium went away; the dropped sender ends the session.
    });
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let reason = match rt.block_on(run(in_rx)) {
        Ok(r) => r,
        Err(e) => format!("{e:#}"),
    };
    let _ = send(&FromHelper::Closed(reason));
}

/// Signed out, or chose Disconnect, from inside the session. IronRDP passes the server's
/// reason on as text only, so compare with its own text for those codes.
fn signed_out(reason: &GracefulDisconnectReason) -> bool {
    use ironrdp::pdu::rdp::server_error_info::{ErrorInfo, ProtocolIndependentCode as Code};
    let GracefulDisconnectReason::Other(text) = reason else { return false };
    [Code::LogoffByUser, Code::RpcInitiatedDisconnectByuser]
        .into_iter()
        .any(|c| *text == ErrorInfo::ProtocolIndependentCode(c).description())
}

fn send(m: &FromHelper) -> std::io::Result<()> {
    m.write(&mut std::io::stdout().lock())
}

struct NoKerberos;
impl ironrdp_tokio::NetworkClient for NoKerberos {
    async fn send(
        &mut self,
        _: &ironrdp::connector::sspi::generator::NetworkRequest,
    ) -> connector::ConnectorResult<Vec<u8>> {
        Err(connector::general_err!("Kerberos KDC access is not supported; use NTLM"))
    }
}

/// What to sign in with, from `ToHelper::Connect`.
struct Login {
    host: String,
    port: u16,
    user: String,
    domain: String,
    password: String,
    quality: u8,
}

async fn run(mut rx: mpsc::UnboundedReceiver<ToHelper>) -> Result<String> {
    let Some(ToHelper::Connect { host, port, user, domain, password, width, height, quality }) = rx.recv().await else {
        bail!("expected Connect");
    };
    let login = Login { host, port, user, domain, password, quality };
    // The desktop size the pane last asked for: a reconnection asks for it again.
    let mut size = (width, height);
    // The server's key once the user trusted it: a reconnection must find the same one.
    let mut trusted = None;
    let mut tries = 0;
    loop {
        let mut up = false;
        let err = match session(&login, &mut size, &mut rx, &mut trusted, &mut up).await {
            Ok(reason) => return Ok(reason),
            Err(e) => e,
        };
        if up {
            tries = 0;
        }
        // Only a connection that worked and then broke on the network is tried again: a wrong
        // password or a changed certificate stays an error.
        if trusted.is_none() || !lost(&err) || tries == RETRY.len() {
            return Err(err);
        }
        let until = Instant::now() + Duration::from_secs(RETRY[tries]);
        tries += 1;
        send(&FromHelper::Status(format!("connection lost, reconnecting ({tries} of {})…", RETRY.len())))?;
        loop {
            tokio::select! {
                _ = sleep_until(until) => break,
                msg = rx.recv() => match msg {
                    None | Some(ToHelper::Close) => return Ok("disconnected".into()),
                    Some(ToHelper::Resize { width, height }) => size = (width, height),
                    _ => {} // input for a desktop that isn't there
                },
            }
        }
    }
}

/// The network failed (as opposed to the server or the login saying no).
fn lost(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.is::<std::io::Error>())
}

/// `fut`, or an error once it has taken `CONNECT_TIMEOUT`.
async fn within<T>(fut: impl Future<Output = T>) -> Result<T> {
    timeout(CONNECT_TIMEOUT, fut)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "the server stopped answering").into())
}

/// One connection: signs in (asking to trust the server's key the first time), then shows the
/// desktop until it ends. `up` turns true once the desktop shows.
async fn session(
    login: &Login,
    size: &mut (u16, u16),
    rx: &mut mpsc::UnboundedReceiver<ToHelper>,
    trusted: &mut Option<Vec<u8>>,
    up: &mut bool,
) -> Result<String> {
    let Login { host, port, user, domain, password, quality } = login;
    let tcp = within(TcpStream::connect((host.as_str(), *port)))
        .await
        .and_then(|r| Ok(r?))
        .with_context(|| format!("connecting to {host}:{port}"))?;
    // A connection that died without a word (the laptop slept, a router restarted) is noticed
    // in about a minute instead of hanging forever.
    let keepalive = socket2::TcpKeepalive::new().with_time(Duration::from_secs(15)).with_interval(Duration::from_secs(5));
    let _ = socket2::SockRef::from(&tcp).set_tcp_keepalive(&keepalive);
    let client_addr = tcp.local_addr()?;
    let mut framed = TokioFramed::new(tcp);
    // Display Control (a dynamic channel) lets us resize the remote desktop with the pane.
    let cfg = config(user.clone(), password.clone(), domain.clone(), size.0, size.1, *quality);
    let mut connector = ClientConnector::new(cfg, client_addr)
        .with_static_channel(DrdynvcClient::new().with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new()))));
    let should_upgrade = within(ironrdp_tokio::connect_begin(&mut framed, &mut connector)).await??;

    let (tls, cert) = within(ironrdp_tls::upgrade(framed.into_inner_no_leftover(), host)).await?.context("TLS")?;
    let server_key = ironrdp_tls::extract_tls_server_public_key(&cert).context("server public key")?.to_vec();
    match trusted {
        Some(k) if *k == server_key => {}
        Some(_) => bail!("the server's certificate changed while reconnecting"),
        None => {
            send(&FromHelper::Certificate(server_key.clone()))?;
            loop {
                match rx.recv().await {
                    Some(ToHelper::Trust(true)) => break,
                    Some(ToHelper::Trust(false)) | None | Some(ToHelper::Close) => return Ok("certificate rejected".into()),
                    _ => {}
                }
            }
            *trusted = Some(server_key.clone());
        }
    }

    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = TokioFramed::new(tls);
    let res = within(ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut NoKerberos,
        ServerName::new(host),
        server_key,
        None,
    ))
    .await?
    .map_err(|e| {
        // CredSSP rejections are the server saying no to the credentials.
        if format!("{e:?}").contains("CredSSP") {
            anyhow::anyhow!("login failed: wrong username or password")
        } else {
            anyhow::anyhow!("RDP login: {e}")
        }
    })?;

    let (w, h) = (res.desktop_size.width, res.desktop_size.height);
    send(&FromHelper::Size { width: w, height: h })?;
    *up = true;
    let mut image = DecodedImage::new(PixelFormat::RgbA32, w, h);
    let (io_channel_id, user_channel_id) = (res.io_channel_id, res.user_channel_id);
    let activation = res.activation_factory;
    let mut stage: ActiveStage = ActiveStageBuilder {
        static_channels: res.static_channels,
        user_channel_id,
        io_channel_id,
        message_channel_id: res.message_channel_id,
        share_id: res.share_id,
        compression_type: res.compression_type,
        enable_server_pointer: res.enable_server_pointer,
        pointer_software_rendering: res.pointer_software_rendering,
    }
    .build();
    let mut input = Database::new();
    // Input waiting its turn behind typed text, which goes at `PACE`; and when the next
    // character may go.
    let mut queue: VecDeque<ToHelper> = VecDeque::new();
    let mut next_char = Instant::now();
    // Changed areas not sent yet, and when they go.
    let mut dirty: Vec<InclusiveRectangle> = vec![];
    let mut due = Instant::now();

    loop {
        let outputs = tokio::select! {
            pdu = framed.read_pdu() => {
                let (action, payload) = pdu.context("connection lost")?;
                stage.process(&mut image, action, &payload)?
            }
            _ = sleep_until(due), if !dirty.is_empty() => {
                send_frame(&image, &mut dirty)?;
                continue;
            }
            _ = sleep_until(next_char), if !queue.is_empty() => {
                let ev = drain(&mut queue, &mut input, &mut next_char, Instant::now());
                if ev.is_empty() {
                    continue;
                }
                stage.process_fastpath_input(&mut image, &ev)?
            }
            msg = rx.recv() => match msg {
                None | Some(ToHelper::Close) => {
                    // Tell the server we're leaving instead of just dropping TCP.
                    for o in stage.graceful_shutdown()? {
                        if let ActiveStageOutput::ResponseFrame(f) = o {
                            framed.write_all(&f).await?;
                        }
                    }
                    return Ok("disconnected".into());
                }
                Some(ToHelper::Resize { width, height }) => {
                    *size = (width, height);
                    let (w, h) = MonitorLayoutEntry::adjust_display_size(width.into(), height.into());
                    // None: the server has no Display Control; the UI keeps scaling the desktop.
                    if (w, h) != (image.width().into(), image.height().into())
                        && let Some(frame) = stage.encode_resize(w, h, None, None)
                    {
                        framed.write_all(&frame?).await?;
                    }
                    continue;
                }
                Some(ToHelper::Connect { .. } | ToHelper::Trust(_)) => continue,
                Some(m) => {
                    queue.push_back(m);
                    let ev = drain(&mut queue, &mut input, &mut next_char, Instant::now());
                    // Nothing changed (a release of a key that isn't down): nothing to send.
                    if ev.is_empty() {
                        continue;
                    }
                    stage.process_fastpath_input(&mut image, &ev)?
                }
            },
        };
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(f) => framed.write_all(&f).await?,
                ActiveStageOutput::GraphicsUpdate(r) => {
                    if dirty.is_empty() {
                        due = Instant::now() + BATCH;
                    }
                    dirty.push(r);
                }
                // The server changes the desktop (after a resize, say): run the reactivation
                // sequence, then carry on at the new size.
                ActiveStageOutput::DeactivateAll => {
                    let mut seq = activation.create();
                    let mut buf = WriteBuf::new();
                    let size = loop {
                        ironrdp_tokio::single_sequence_step(&mut framed, &mut seq, &mut buf).await?;
                        if let ConnectionActivationState::Finalized {
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                        } = seq.connection_activation_state()
                        {
                            stage.set_fastpath_processor(
                                fast_path::ProcessorBuilder {
                                    io_channel_id,
                                    user_channel_id,
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                    // config() asks for no bulk compression.
                                    bulk_decompressor: None,
                                }
                                .build(),
                            );
                            stage.set_share_id(share_id);
                            stage.set_enable_server_pointer(enable_server_pointer);
                            break desktop_size;
                        }
                    };
                    image = DecodedImage::new(PixelFormat::RgbA32, size.width, size.height);
                    dirty.clear();
                    send(&FromHelper::Size { width: size.width, height: size.height })?;
                }
                ActiveStageOutput::Terminate(reason) => {
                    if signed_out(&reason) {
                        send(&FromHelper::Ended)?;
                    }
                    return Ok(format!("{reason:?}"));
                }
                _ => {}
            }
        }
    }
}

/// The input at the front of the queue that may go `now`: everything up to the next typed
/// character, and that character too if its turn has come. Order is kept: an Enter after a line
/// of text goes after the line.
fn drain(queue: &mut VecDeque<ToHelper>, db: &mut Database, next_char: &mut Instant, now: Instant) -> Vec<FastPathInputEvent> {
    let mut ev = vec![];
    while let Some(m) = queue.front() {
        if matches!(m, ToHelper::Unicode(_)) {
            if now < *next_char {
                break;
            }
            *next_char = (*next_char).max(now - PACE * BURST) + PACE;
        }
        let m = queue.pop_front().expect("front was there");
        ev.extend(events(db, m));
    }
    ev
}

/// The RDP events for one input message; `db` keeps what is held down.
fn events(db: &mut Database, m: ToHelper) -> Vec<FastPathInputEvent> {
    let op = match m {
        ToHelper::Key { scancode, pressed } => {
            let s = Scancode::from_u16(scancode);
            // A held key repeats as presses without a release between, as a keyboard's do: a
            // release would, say, open the menu bar each time a held Alt repeats.
            if pressed && db.is_key_pressed(s) {
                let (extended, code) = s.as_u8();
                let flags = if extended { KeyboardFlags::EXTENDED } else { KeyboardFlags::empty() };
                return vec![FastPathInputEvent::KeyboardEvent(flags, code)];
            }
            if pressed { Operation::KeyPressed(s) } else { Operation::KeyReleased(s) }
        }
        ToHelper::Unicode(c) => {
            // Ctrl, Alt or a Windows key down on the desktop would make the character a shortcut
            // there: AltGr arrives as Ctrl+Alt, and a key's release may have gone elsewhere.
            let mut ops: Vec<Operation> = MODIFIERS
                .iter()
                .map(|&s| Scancode::from_u16(s))
                .filter(|&s| db.is_key_pressed(s))
                .map(Operation::KeyReleased)
                .collect();
            ops.extend([Operation::UnicodeKeyPressed(c), Operation::UnicodeKeyReleased(c)]);
            return db.apply(ops).into_vec();
        }
        ToHelper::MouseMove { x, y } => Operation::MouseMove(MousePosition { x, y }),
        ToHelper::MouseButton { button, pressed } => {
            // DOM numbering: 0 left, 1 middle, 2 right, 3 back, 4 forward.
            let b = MouseButton::from_web_button(button).unwrap_or(MouseButton::Left);
            if pressed { Operation::MouseButtonPressed(b) } else { Operation::MouseButtonReleased(b) }
        }
        ToHelper::Wheel { vertical, units } => {
            // An event carries at most 255 units: a fast flick goes as several, a notch (120) each.
            let mut left = units.clamp(-1200, 1200);
            let mut ops = vec![];
            while left != 0 {
                let step = left.clamp(-120, 120);
                ops.push(Operation::WheelRotations(WheelRotations { is_vertical: vertical, rotation_units: step }));
                left -= step;
            }
            return db.apply(ops).into_vec();
        }
        ToHelper::ReleaseAll => return db.release_all().into_vec(),
        ToHelper::Sync { caps, num, scroll } => return vec![ironrdp::input::synchronize_event(scroll, num, caps, false)],
        ToHelper::Connect { .. } | ToHelper::Trust(_) | ToHelper::Close | ToHelper::Resize { .. } => return vec![],
    };
    db.apply([op]).into_vec()
}

/// Sends the changed areas' current pixels as one frame (or a few, if huge).
fn send_frame(image: &DecodedImage, dirty: &mut Vec<InclusiveRectangle>) -> Result<()> {
    let (iw, ih) = (image.width(), image.height());
    let rects = merge(dirty.drain(..).map(|r| (r.left, r.top, r.right.min(iw - 1), r.bottom.min(ih - 1))).collect());
    let stride = iw as usize * 4;
    let mut body = vec![];
    for (left, top, right, bottom) in rects {
        let (w, h) = (right - left + 1, bottom - top + 1);
        for n in [left, top, w, h] {
            body.extend_from_slice(&n.to_le_bytes());
        }
        for row in top..=bottom {
            let start = row as usize * stride + left as usize * 4;
            body.extend_from_slice(&image.data()[start..start + w as usize * 4]);
        }
        if body.len() > MAX_FRAME {
            send(&FromHelper::Frame(std::mem::take(&mut body)))?;
        }
    }
    if !body.is_empty() {
        send(&FromHelper::Frame(body))?;
    }
    Ok(())
}

/// Inclusive (left, top, right, bottom).
type Rect = (u16, u16, u16, u16);

/// Fewer, non-redundant rectangles for the same changes: empty ones and ones inside another are
/// dropped, and everything becomes one bounding box when that copies at most 25% more pixels.
fn merge(mut rects: Vec<Rect>) -> Vec<Rect> {
    let area = |&(l, t, r, b): &Rect| (r as u64 + 1 - l as u64) * (b as u64 + 1 - t as u64);
    rects.retain(|&(l, t, r, b)| l <= r && t <= b);
    rects.sort_by_key(|r| std::cmp::Reverse(area(r)));
    let mut kept: Vec<Rect> = vec![];
    for r in rects {
        if !kept.iter().any(|k| k.0 <= r.0 && k.1 <= r.1 && k.2 >= r.2 && k.3 >= r.3) {
            kept.push(r);
        }
    }
    if kept.len() > 1 {
        let bbox = kept.iter().fold(kept[0], |a, r| (a.0.min(r.0), a.1.min(r.1), a.2.max(r.2), a.3.max(r.3)));
        if area(&bbox) * 4 <= kept.iter().map(area).sum::<u64>() * 5 {
            return vec![bbox];
        }
    }
    kept
}

/// `quality`: how the session trades picture for speed.
/// - 0, quality (the default): lossless 32-bit bitmaps and the full desktop look.
/// - 1, balanced: RemoteFX (compressed, slightly lossy tiles), no wallpaper or animations.
/// - 2, speed: RemoteFX, 16-bit colour for plain bitmaps, and the plainest desktop.
fn config(user: String, password: String, domain: String, width: u16, height: u16, quality: u8) -> connector::Config {
    use PerformanceFlags as F;
    let looks = F::ENABLE_FONT_SMOOTHING | F::ENABLE_DESKTOP_COMPOSITION;
    let lean = F::DISABLE_WALLPAPER | F::DISABLE_FULLWINDOWDRAG | F::DISABLE_MENUANIMATIONS;
    let (remotefx, color_depth, performance_flags) = match quality {
        0 => (false, 32, looks),
        1 => (true, 32, looks | lean),
        _ => (true, 16, lean | F::DISABLE_THEMING | F::DISABLE_CURSOR_SHADOW | F::DISABLE_CURSORSETTINGS),
    };
    connector::Config {
        credentials: Credentials::UsernamePassword { username: user, password },
        domain: (!domain.is_empty()).then_some(domain),
        enable_tls: true,
        enable_credssp: true,
        desktop_size: DesktopSize { width, height },
        desktop_scale_factor: 0,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        // RemoteFX: compressed tiles instead of bitmaps, far less data for video, scrolling and
        // full-screen redraws, at a small cost in sharpness. Without it bitmaps are lossless.
        bitmap: Some(BitmapConfig {
            lossy_compression: remotefx,
            color_depth,
            codecs: client_codecs_capabilities(if remotefx { &[] } else { &["remotefx:off"] }).expect("codec list"),
        }),
        client_build: 0,
        client_name: "xtermium".into(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".into(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        platform: if cfg!(windows) {
            MajorPlatformType::WINDOWS
        } else if cfg!(target_os = "macos") {
            MajorPlatformType::MACINTOSH
        } else {
            MajorPlatformType::UNIX
        },
        hardware_id: None,
        request_data: None,
        autologon: false,
        enable_audio_playback: false,
        performance_flags,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: None,
        enable_server_pointer: true,
        pointer_software_rendering: true,
        multitransport_flags: None,
    }
}

#[cfg(test)]
mod tests {
    use super::merge;

    #[test]
    fn signing_out_ends_the_tab() {
        use super::{GracefulDisconnectReason as R, signed_out};
        use ironrdp::pdu::rdp::server_error_info::{ErrorInfo, ProtocolIndependentCode as Code};
        let other = |c| R::Other(ErrorInfo::ProtocolIndependentCode(c).description());
        assert!(signed_out(&other(Code::LogoffByUser)));
        assert!(signed_out(&other(Code::RpcInitiatedDisconnectByuser)));
        assert!(!signed_out(&other(Code::IdleTimeout)));
        assert!(!signed_out(&R::ServerInitiated));
    }

    #[test]
    fn every_quality_level_makes_a_config() {
        for q in 0..=3 {
            let c = super::config("u".into(), "p".into(), String::new(), 1920, 1080, q);
            let codecs = c.bitmap.unwrap().codecs.0.len();
            assert_eq!(codecs > 0, q > 0, "RemoteFX only when trading quality for speed");
        }
    }

    /// Typed text goes one character per `PACE`, and input after it waits its turn.
    #[test]
    fn typed_text_is_paced_in_order() {
        use super::{Database, PACE, ToHelper, drain};
        use tokio::time::Instant;
        let mut db = Database::new();
        let t0 = Instant::now();
        let mut next = t0;
        let enter = |pressed| ToHelper::Key { scancode: 0x1C, pressed };
        let mut q: std::collections::VecDeque<ToHelper> =
            [ToHelper::Unicode('a'), ToHelper::Unicode('b'), enter(true), enter(false)].into();
        assert_eq!(drain(&mut q, &mut db, &mut next, t0).len(), 2, "a goes (press, release), b waits");
        assert_eq!((q.len(), next), (3, t0 + PACE));
        assert!(drain(&mut q, &mut db, &mut next, t0).is_empty(), "not b's turn yet");
        assert_eq!(drain(&mut q, &mut db, &mut next, t0 + PACE).len(), 4, "b, then Enter down and up");
        assert!(q.is_empty());

        // A late wake-up catches up, a few characters at once and no more.
        q.extend("abcdefghij".chars().map(ToHelper::Unicode));
        let late = t0 + PACE * 100;
        assert_eq!(drain(&mut q, &mut db, &mut next, late).len(), 2 * 5, "five characters");
        assert_eq!(q.len(), 5);
        assert_eq!(drain(&mut q, &mut db, &mut next, late + PACE * 3).len(), 2 * 3, "three more, at the pace");
    }

    /// Characters let go of Ctrl/Alt first; held keys repeat without a release; a big wheel
    /// turn is split; back and forward are their own buttons; releasing lets go of everything.
    #[test]
    fn input_events() {
        use super::{Database, ToHelper, events};
        use ironrdp::pdu::input::fast_path::{FastPathInputEvent as E, KeyboardFlags as F};
        let mut db = Database::new();
        let ctrl = ToHelper::Key { scancode: 0x1D, pressed: true };
        assert!(matches!(events(&mut db, ctrl)[..], [E::KeyboardEvent(f, 0x1D)] if f.is_empty()));
        let ev = events(&mut db, ToHelper::Unicode('@'));
        assert!(matches!(ev[0], E::KeyboardEvent(f, 0x1D) if f == F::RELEASE), "Ctrl let go first: {ev:?}");
        assert_eq!(ev.len(), 3);

        let alt = || ToHelper::Key { scancode: 0x38, pressed: true };
        events(&mut db, alt());
        assert!(matches!(events(&mut db, alt())[..], [E::KeyboardEvent(f, 0x38)] if f.is_empty()), "a repeat is a press alone");

        let wheel = events(&mut db, ToHelper::Wheel { vertical: true, units: 300 });
        let units: Vec<i16> = wheel
            .iter()
            .map(|e| match e {
                E::MouseEvent(m) => m.number_of_wheel_rotation_units,
                e => panic!("{e:?}"),
            })
            .collect();
        assert_eq!(units, [120, 120, 60]);

        assert!(matches!(events(&mut db, ToHelper::MouseButton { button: 3, pressed: true })[..], [E::MouseEventEx(_)]));
        let released = events(&mut db, ToHelper::ReleaseAll);
        assert_eq!(released.len(), 2, "Alt and the back button: {released:?}");
        assert!(events(&mut db, ToHelper::ReleaseAll).is_empty());
    }

    #[test]
    fn merging() {
        // Contained and empty rectangles go.
        assert_eq!(merge(vec![(0, 0, 99, 99), (10, 10, 20, 20), (5, 5, 4, 4)]), vec![(0, 0, 99, 99)]);
        // Neighbours become their bounding box: no extra pixels.
        assert_eq!(merge(vec![(0, 0, 49, 9), (50, 0, 99, 9)]), vec![(0, 0, 99, 9)]);
        // Far apart (a clock and a cursor) stay separate: the box would copy the whole screen.
        assert_eq!(merge(vec![(0, 0, 9, 9), (1900, 1000, 1909, 1009)]).len(), 2);
    }
}
