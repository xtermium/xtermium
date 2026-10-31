//! Website tabs: the system's own browser view (WebView2 on Windows, WKWebView on macOS,
//! WebKitGTK on Linux) laid over the tab's area of the main window. The UI keeps it placed
//! (`web_place`) and hidden while dialogs are open, since a native view draws above the page.
//!
//! Websites can't call xtermium's commands: Tauri refuses IPC from remote origins unless a
//! capability names them, and none does (capabilities/default.json).

use crate::config::Kind;
use crate::rdp::{Input, Picture};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Url, WebviewBuilder, WebviewUrl, Window};

type Res<T> = Result<T, String>;

/// Where the view goes, in the main window's physical pixels.
#[derive(Deserialize)]
pub struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// What a website tab shows, for its title, address bar and status.
#[derive(Serialize, Clone)]
struct Page {
    id: u32,
    url: String,
    title: Option<String>,
    loading: bool,
}

/// Something the tab's user needs to know (a console the view can't show).
#[derive(Serialize, Clone)]
struct Notice {
    id: u32,
    message: String,
}

/// A page asked for a new window: the tab it came from, and the address.
#[derive(Serialize, Clone)]
struct Popup {
    id: u32,
    url: String,
}

fn label(id: u32) -> String {
    format!("web{id}")
}

fn web(window: &Window, id: u32) -> Res<tauri::Webview> {
    window.app_handle().get_webview(&label(id)).ok_or_else(|| "no such website tab".into())
}

/// http(s) only: no file:, javascript: or custom schemes in a tab.
fn address(url: &str) -> Res<Url> {
    let u: Url = url.trim().parse().map_err(|_| format!("not a web address: {url}"))?;
    if matches!(u.scheme(), "http" | "https") { Ok(u) } else { Err("Use an http(s) address.".into()) }
}

/// Website tabs' SSH gateways (a shared login's), by tab id: closing the tab closes its gateway.
static GATEWAYS: Mutex<BTreeMap<u32, crate::ssh::Forward>> = Mutex::new(BTreeMap::new());

/// Open website tabs by id, for automation and broadcast: the session's name, whether it's a BMC
/// console, and the page's title; and the browser profile it uses (`site` / `temp`).
struct Opened {
    name: String,
    kvm: bool,
    title: String,
    /// The website session's own profile (`site`), "" for none.
    profile: String,
    /// Its pages go through an SSH gateway (a proxy, which the profile's browser is started with).
    proxied: bool,
    /// It has a profile of its own for this tab only (`temp`), deleted with it.
    temp: bool,
    /// A page is loading: automation waits for it before reading or acting.
    loading: bool,
    /// The view is on screen (`web_place`); a hidden one doesn't draw.
    shown: bool,
}
static OPEN: Mutex<BTreeMap<u32, Opened>> = Mutex::new(BTreeMap::new());

/// Opens a website tab's view (hidden until placed); returns its id. Async: adding a view waits
/// for the main thread, which a synchronous command would be blocking. `credential`: the
/// session's shared login; with an SSH gateway, every request goes through it (SOCKS5).
/// `vendor`: a BMC console's (kvm.rs), which signs in with the login and opens pop-ups in place.
/// `profile`: a website session's own browser profile (`Kind::Browser`), where its sign-ins and
/// cookies stay between runs. Consoles, and websites that aren't saved, browse privately: nothing
/// of theirs is kept once the tab closes.
#[tauri::command]
pub async fn web_open(
    app: tauri::State<'_, crate::App>,
    window: Window,
    mut url: String,
    name: Option<String>,
    credential: Option<String>,
    vendor: Option<String>,
    profile: Option<String>,
) -> Res<u32> {
    // A console starts at its vendor's page (kvm.rs `home`); `url` is the BMC's address.
    if let Some(v) = &vendor {
        url = format!("{}{}", url.trim_end_matches('/'), crate::kvm::home(v));
    }
    let url = address(&url)?;
    let id = app.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let credential = credential.unwrap_or_default();
    let kvm = vendor.is_some();
    let (gateway, (user, password)) = {
        let store = app.store.lock().unwrap();
        (store.gateway(&Kind::Browser { url: String::new(), credential: credential.clone(), profile: String::new() }), crate::kvm::login(&store, &credential))
    };
    // As the page's `location.host` reads it: the port only when it isn't the scheme's own.
    let origin = format!("{}{}", url.host_str().unwrap_or_default(), url.port().map(|p| format!(":{p}")).unwrap_or_default());
    // The console's certificate pin: `host:port` (kvm.rs `trust`).
    #[cfg(windows)]
    let (pin, home) = (format!("{}:{}", url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or(443)), url.clone());
    let fwd = match gateway {
        // Without the `macos-proxy` feature (macOS 14+) the proxy would be ignored, and the
        // pages would go straight out.
        Some(_) if cfg!(target_os = "macos") => return Err("Websites can't go through an SSH gateway on macOS yet.".into()),
        Some(gw) => Some(crate::ssh::forward(&gw, None, &app.prompter).await.map_err(|e| format!("{e:#}"))?),
        None => None,
    };
    let page = move |url: &Url, title: Option<String>, loading| Page { id, url: url.to_string(), title, loading };
    let (h1, h2, h3) = (window.app_handle().clone(), window.app_handle().clone(), window.app_handle().clone());
    // Until a page paints, the view shows this: the theme's dark, not a white flash.
    let blank = if app.dark.load(std::sync::atomic::Ordering::Relaxed) { tauri::window::Color(21, 24, 28, 255) } else { tauri::window::Color(255, 255, 255, 255) };
    // A console's view starts empty (Windows): started at the BMC's address, its first request
    // would meet the self-signed certificate before the handler below is in place, and the view
    // would show the browser's own "not private" page instead of xtermium's trust prompt. The
    // handler sends it to the BMC once it's attached.
    let start = if kvm && cfg!(windows) { Url::parse("about:blank").expect("a fixed address") } else { url };
    let mut builder = WebviewBuilder::new(label(id), WebviewUrl::External(start))
        .background_color(blank)
        .on_page_load(move |_, p| {
            let started = p.event() == PageLoadEvent::Started;
            if let Some(o) = OPEN.lock().unwrap().get_mut(&id) {
                o.loading = started;
            }
            let _ = h1.emit_to("main", "web-page", page(p.url(), None, started));
        })
        .on_document_title_changed(move |w, title| {
            if let Some(o) = OPEN.lock().unwrap().get_mut(&id) {
                o.title = title.clone();
            }
            if let Ok(url) = w.url() {
                let _ = h2.emit_to("main", "web-page", page(&url, Some(title), false));
            }
        })
        // Pop-ups and links that open a new window open as a new website tab (App.svelte); a
        // console's (the BMC opens the console itself in one) open in its own tab instead.
        .on_new_window(move |url, _| {
            match url.scheme() {
                "http" | "https" if kvm => {
                    if let Some(w) = h3.get_webview(&label(id)) {
                        let _ = w.navigate(url);
                    }
                }
                "http" | "https" => {
                    let _ = h3.emit_to("main", "web-popup", Popup { id, url: url.to_string() });
                }
                _ => {}
            }
            NewWindowResponse::Deny
        });
    if let Some(v) = vendor.clone() {
        // A Java console comes as a .jnlp file to download, which no browser view runs: it's
        // refused, and the tab says what to change on the BMC.
        let h = window.app_handle().clone();
        builder = builder.on_download(move |_, e| match e {
            tauri::webview::DownloadEvent::Requested { url, destination } => {
                let java = url.path().contains(".jnlp") || destination.extension().is_some_and(|x| x == "jnlp");
                if java {
                    let _ = h.emit_to("main", "web-notice", Notice { id, message: crate::kvm::java_hint(&v).into() });
                }
                !java
            }
            _ => true,
        });
    }
    if kvm {
        let relay = crate::kvm::relay::address().await.unwrap_or_default();
        builder = builder.initialization_script_for_all_frames(crate::kvm::login_script(
            vendor.as_deref().unwrap_or_default(),
            &origin,
            &user,
            &password,
            &relay,
        ));
    }
    builder = builder.initialization_script_for_all_frames(AWAKE);
    // Where the view keeps its data: the session's own profile; or, when that's open already in a
    // browser started differently (a proxy is a browser-wide setting, and WebView2 refuses a second
    // one on the same folder), a profile for this tab only; or the main window's browser.
    // ponytail: a data folder per website session, so each starts its own WebView2 browser
    // processes (often 100 MB or more each; ten sessions open can cost about 1 GB more than one
    // shared browser). The lighter way is one browser with a cookie profile per session
    // (ICoreWebView2ControllerOptions::SetProfileName, which wry already passes on as
    // `profile_name`), but Tauri's WebviewBuilder doesn't expose it yet: switch to it once it does,
    // naming each profile by the session's `profile` id. `sweep_sites` would then delete gone
    // sessions' profiles through WebView2 (ICoreWebView2Profile8::Delete), not as folders.
    let profile: String = profile.unwrap_or_default().chars().filter(char::is_ascii_alphanumeric).take(64).collect();
    let private = kvm || profile.is_empty();
    let clash = OPEN.lock().unwrap().values().any(|o| o.profile == profile && (o.proxied || fwd.is_some()));
    let shared = !private && !clash;
    let temp = !shared && (fwd.is_some() || !private);
    let dir = if shared {
        site(&profile)
    } else if temp {
        self::profile(id).ok_or("can't make a browser profile folder")?
    } else {
        crate::config::webview_dir()
    };
    builder = builder.data_directory(dir);
    // macOS has no data folders: a data store of its own instead (macOS 14+).
    if shared && let Ok(Ok(store)) = crate::vault::unhex(&profile).map(<[u8; 16]>::try_from) {
        builder = builder.data_store_identifier(store);
    }
    builder = builder.incognito(private);
    let proxied = fwd.is_some();
    if let Some(f) = fwd {
        let proxy = format!("socks5://127.0.0.1:{}", f.port).parse().map_err(|_| "bad proxy address")?;
        builder = builder.proxy_url(proxy);
        GATEWAYS.lock().unwrap().insert(id, f);
    }
    let view = window.add_child(builder, LogicalPosition::new(0.0, 0.0), LogicalSize::new(1.0, 1.0)).map_err(|e| {
        GATEWAYS.lock().unwrap().remove(&id);
        if temp {
            forget_profile(id);
        }
        e.to_string()
    })?;
    let _ = view.hide();
    #[cfg(target_os = "linux")]
    let _ = view.with_webview(|w| overlay(&w.inner()));
    let profile = if shared { profile } else { String::new() };
    OPEN.lock().unwrap().insert(id, Opened { name: name.unwrap_or_default(), kvm, title: String::new(), profile, proxied, temp, loading: true, shown: false });
    // Clicks in the page never reach xtermium's own page, so the view says when it gets the
    // keyboard: its pane becomes the focused one (Web.svelte). And xtermium's shortcuts, which
    // the page would otherwise take (Alt+arrows: back and forward), go to xtermium.
    #[cfg(windows)]
    {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
        };
        let (h, h2) = (window.app_handle().clone(), window.app_handle().clone());
        let _ = view.with_webview(move |w| {
            let got = webview2_com::FocusChangedEventHandler::create(Box::new(move |_, _| {
                let _ = h.emit_to("main", "web-focus", id);
                Ok(())
            }));
            let keys = webview2_com::AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let (mut kind, mut vk) = (COREWEBVIEW2_KEY_EVENT_KIND::default(), 0u32);
                // SAFETY: the event's arguments, valid during the call.
                unsafe {
                    args.KeyEventKind(&mut kind)?;
                    args.VirtualKey(&mut vk)?;
                }
                if (kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN)
                    && let Some(c) = chord(vk)
                    && SHORTCUTS.lock().unwrap().contains(&c)
                {
                    // SAFETY: as above.
                    unsafe { args.SetHandled(true)? };
                    // xtermium's page takes the keyboard back, then runs the shortcut (App.svelte).
                    if let Some(main) = h2.get_webview("main") {
                        let _ = main.set_focus();
                    }
                    let _ = h2.emit_to("main", "web-key", c);
                }
                Ok(())
            }));
            let mut token = 0;
            // SAFETY: the controller is alive (with_webview runs on its thread) and keeps the handlers.
            unsafe {
                let _ = w.controller().add_GotFocus(&got, &mut token);
                let _ = w.controller().add_AcceleratorKeyPressed(&keys, &mut token);
            }
        });
    }
    // A BMC signs its own certificate, which the browser view refuses: a console trusts it on
    // first use, like an SSH host key (kvm.rs), and loads again once it's trusted.
    #[cfg(windows)]
    if kvm {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use webview2_com::Microsoft::Web::WebView2::Win32::{COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW, ICoreWebView2_14};
        use windows::core::{HSTRING, Interface, PWSTR};
        let (h, prompter) = (window.app_handle().clone(), app.prompter.clone());
        // One question at a time: every request of the page meets the same certificate.
        let asking = Arc::new(AtomicBool::new(false));
        let first = home.to_string();
        let _ = view.with_webview(move |w| {
            let handler = webview2_com::ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut pem = PWSTR::null();
                // SAFETY: the event's arguments, valid during the call; the PEM text is ours to free.
                let fp = unsafe {
                    args.ServerCertificate()?.ToPemEncoding(&mut pem)?;
                    crate::kvm::pem_fingerprint(&webview2_com::take_pwstr(pem))
                };
                let Some(fp) = fp else { return Ok(()) };
                if crate::config::Known::with(|k| k.https.get(&pin) == Some(&fp)).unwrap_or(false) {
                    // SAFETY: as above.
                    unsafe { args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW)? };
                } else if !asking.swap(true, Ordering::Relaxed) {
                    let (h, prompter, pin, home, asking) = (h.clone(), prompter.clone(), pin.clone(), home.clone(), asking.clone());
                    tauri::async_runtime::spawn(async move {
                        if crate::kvm::trust(&prompter, &pin, &fp).await == Ok(true)
                            && let Some(v) = h.get_webview(&label(id))
                        {
                            let _ = v.navigate(home);
                        }
                        asking.store(false, Ordering::Relaxed);
                    });
                }
                Ok(())
            }));
            let mut token = 0;
            // SAFETY: as for the handlers above.
            unsafe {
                if let Ok(core) = w.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_14>()) {
                    let _ = core.add_ServerCertificateErrorDetected(&handler, &mut token);
                    // Now the BMC's page, with the handler there to meet its certificate.
                    let _ = core.Navigate(&HSTRING::from(first.as_str()));
                }
            }
        });
    }
    Ok(id)
}

/// Linux: Tauri packs a child view into the window's GtkBox under xtermium's own page, where it
/// takes a band of the window and can't be moved or sized. The first website view wraps the page
/// in a GtkOverlay with a GtkFixed over it; every view goes into the Fixed, where `web_place`
/// moves and sizes it over its tab.
#[cfg(target_os = "linux")]
fn overlay(view: &webkit2gtk::WebView) {
    use gtk::prelude::*;
    let Some(parent) = view.parent() else { return };
    if parent.is::<gtk::Fixed>() {
        return;
    }
    let Ok(vbox) = parent.downcast::<gtk::Box>() else { return };
    let fixed = match vbox.children().into_iter().find_map(|c| c.downcast::<gtk::Overlay>().ok()) {
        Some(overlay) => overlay.children().into_iter().find_map(|c| c.downcast::<gtk::Fixed>().ok()),
        None => {
            // xtermium's page: the window's first view, packed before any website's.
            let me = view.upcast_ref::<gtk::Widget>();
            let Some(page) = vbox.children().into_iter().find(|c| c.is::<webkit2gtk::WebView>() && c != me) else { return };
            let overlay = gtk::Overlay::new();
            let fixed = gtk::Fixed::new();
            vbox.remove(&page);
            overlay.add(&page);
            overlay.add_overlay(&fixed);
            overlay.set_overlay_pass_through(&fixed, true);
            vbox.pack_start(&overlay, true, true, 0);
            overlay.show_all();
            Some(fixed)
        }
    };
    let Some(fixed) = fixed else { return };
    vbox.remove(view);
    fixed.put(view, 0, 0);
}

/// xtermium's shortcuts (Settings > Shortcuts), named as App.svelte names chords: a website with
/// the keyboard hands these back instead of acting on them.
static SHORTCUTS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

#[tauri::command]
pub fn web_shortcuts(chords: Vec<String>) {
    *SHORTCUTS.lock().unwrap() = chords.into_iter().collect();
}

/// A key press in a website, named like ui/lib/actions.ts `chord`: "Alt+ArrowLeft", "Ctrl+Shift+P",
/// "F6". None for keys no shortcut uses.
#[cfg(windows)]
fn chord(vk: u32) -> Option<String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
    let key = match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x25 => "ArrowLeft".into(),
        0x26 => "ArrowUp".into(),
        0x27 => "ArrowRight".into(),
        0x28 => "ArrowDown".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        _ => return None,
    };
    // SAFETY: reads the calling thread's keyboard state.
    let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0.into()) } < 0;
    let mods = [(down(VK_CONTROL), "Ctrl"), (down(VK_MENU), "Alt"), (down(VK_SHIFT), "Shift"), (down(VK_LWIN) || down(VK_RWIN), "Meta")];
    Some(mods.iter().filter(|(on, _)| *on).map(|(_, m)| *m).chain([key.as_str()]).collect::<Vec<_>>().join("+"))
}

/// Moves the view over its tab's area and shows it; `None` hides it (tab hidden, dialog open).
#[tauri::command]
pub fn web_place(window: Window, id: u32, rect: Option<Rect>) -> Res<()> {
    let w = web(&window, id)?;
    if let Some(o) = OPEN.lock().unwrap().get_mut(&id) {
        o.shown = rect.is_some();
    }
    // Linux: the view sits in a GtkFixed (`overlay`), which Tauri's own placement doesn't reach.
    #[cfg(target_os = "linux")]
    if let Some(r) = &rect {
        let (x, y, width, height) = (r.x, r.y, r.w.max(1.0), r.h.max(1.0));
        let _ = w.with_webview(move |pw| {
            use gtk::prelude::*;
            let v = pw.inner();
            let s = f64::from(v.scale_factor());
            if let Some(f) = v.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) {
                f.move_(&v, (x / s).round() as i32, (y / s).round() as i32);
                v.set_size_request((width / s).round() as i32, (height / s).round() as i32);
            }
        });
    }
    let r = match rect {
        Some(r) => w
            .set_position(PhysicalPosition::new(r.x, r.y))
            .and_then(|_| w.set_size(PhysicalSize::new(r.w.max(1.0), r.h.max(1.0))))
            .and_then(|_| w.show()),
        None => w.hide(),
    };
    r.map_err(|e| e.to_string())
}

/// Back, forward, reload, focus, or go to an address.
#[tauri::command]
pub fn web_go(window: Window, id: u32, action: String, url: Option<String>) -> Res<()> {
    let w = web(&window, id)?;
    let r = match action.as_str() {
        "back" => w.eval("history.back()"),
        "forward" => w.eval("history.forward()"),
        "reload" => w.reload(),
        "focus" => w.set_focus(),
        "go" => w.navigate(address(url.as_deref().unwrap_or_default())?),
        _ => return Err(format!("unknown action {action}")),
    };
    r.map_err(|e| e.to_string())
}

/// Types text into a console tab's page as key presses: the local clipboard's when `text` is None.
#[tauri::command]
pub fn web_type(window: Window, id: u32, text: Option<String>) -> Res<()> {
    let text = match text {
        Some(t) => t,
        None => crate::term::clipboard(|c| c.get_text()).ok_or("The clipboard holds no text.")?,
    };
    web(&window, id)?.eval(crate::kvm::input_script(&[Input::Text { text }])).map_err(|e| e.to_string())
}

/// Some(true) for a BMC console tab, Some(false) for a website's, None for neither.
pub fn console(id: u32) -> Option<bool> {
    OPEN.lock().unwrap().get(&id).map(|o| o.kvm)
}

/// The open BMC console tabs.
pub fn consoles() -> Vec<u32> {
    OPEN.lock().unwrap().iter().filter(|(_, o)| o.kvm).map(|(&id, _)| id).collect()
}

/// The open website tabs, as automation lists tabs.
pub fn listed(handle: &AppHandle) -> Vec<(u32, serde_json::Value)> {
    OPEN.lock()
        .unwrap()
        .iter()
        .map(|(&id, o)| {
            let url = handle.get_webview(&label(id)).and_then(|w| w.url().ok()).map(|u| u.to_string());
            let kind = if o.kvm { "kvm" } else { "browser" };
            (id, serde_json::json!({ "id": id, "name": o.name, "type": kind, "url": url, "title": o.title }))
        })
        .collect()
}

/// A BMC console tab's view; websites take no input or screenshots (only consoles have a screen).
fn console_view(handle: &AppHandle, id: u32) -> Res<tauri::Webview> {
    match console(id) {
        Some(true) => handle.get_webview(&label(id)).ok_or_else(|| format!("no tab {id}")),
        Some(false) => Err("that's a website tab: use web_read, web_click, web_fill and web_go".into()),
        None => Err(format!("no tab {id}")),
    }
}

/// Remote desktop input for a BMC console tab (kvm.rs `input_script`).
pub fn console_input(handle: &AppHandle, id: u32, evs: &[Input]) -> Res<()> {
    console_view(handle, id)?.eval(crate::kvm::input_script(evs)).map_err(|e| e.to_string())
}

/// What a BMC console tab's screen shows now, in the console's own pixels.
pub async fn console_screenshot(handle: &AppHandle, id: u32) -> Res<Picture> {
    use base64::Engine;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Mutex::new(Some(tx));
    console_view(handle, id)?
        .eval_with_callback(crate::kvm::screenshot_script(), move |r| {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(r);
            }
        })
        .map_err(|e| e.to_string())?;
    let answer = tokio::time::timeout(Duration::from_secs(10), rx).await.map_err(|_| "the console didn't answer")?;
    let url: String = serde_json::from_str(&answer.unwrap_or_default()).unwrap_or_default();
    let b64 = url
        .strip_prefix("data:image/png;base64,")
        .filter(|b| !b.is_empty())
        .ok_or("the console isn't showing yet (still signing in or connecting?)")?;
    let png = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| e.to_string())?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    Ok(Picture { w: img.width() as usize, h: img.height() as usize, px: img.into_raw() })
}

// ---- Automating website tabs (automation.rs `web_*` tools) ----
//
// Scripts run in the page through the view itself, so an assistant drives a website tab without
// a browser of its own or its DevTools. Running script in a page is the point, and only the
// automation server's token holder can ask for it (Settings > MCP, off by default). Only
// website tabs: a BMC console's page holds the shared login it signed in with, and its tools stay
// screenshot, click and key. A script is spliced into the code the view runs rather than passed
// to the page's own `eval`, which a page's Content-Security-Policy may forbid.

/// A website tab's view, for the automation tools.
fn site_view(handle: &AppHandle, id: u32) -> Res<tauri::Webview> {
    match console(id) {
        Some(false) => handle.get_webview(&label(id)).ok_or_else(|| format!("no tab {id}")),
        Some(true) => Err("that's a BMC console tab: use screenshot, click and key".into()),
        None => Err(format!("tab {id} isn't a website tab")),
    }
}

/// Runs `script` in the view and gives back its value as the view reports it (JSON text).
async fn eval(view: &tauri::Webview, script: String) -> Res<String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Mutex::new(Some(tx));
    view.eval_with_callback(script, move |r| {
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(r);
        }
    })
    .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(5), rx).await.map_err(|_| "the page didn't answer".to_string()).map(Result::unwrap_or_default)
}

/// Waits until the tab's page has loaded (at most `limit`); false if it was still loading then.
async fn settle(id: u32, limit: Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        let loading = OPEN.lock().unwrap().get(&id).is_some_and(|o| o.loading);
        if !loading {
            return true;
        }
        if start.elapsed() > limit {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Runs a JavaScript function in a website tab's page and returns what it returns (an async one
/// is awaited), as JSON. A value JSON can't hold comes back as its text.
pub async fn page_run(handle: &AppHandle, id: u32, function: &str, limit: Duration) -> Res<serde_json::Value> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let view = site_view(handle, id)?;
    settle(id, Duration::from_secs(15)).await;
    // The answer waits in the page under a key of its own, fetched below: the views' own script
    // calls don't wait for a promise, and an async function is the usual kind.
    let key = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    eval(&view, format!(
        r#"(() => {{
  const put = (r) => {{ (window.__xtermium ||= {{}})[{key}] = r; }};
  const err = (e) => put({{ ok: false, e: String((e && e.message) || e) }});
  const safe = (v) => {{ if (v instanceof Element) return v.outerHTML.slice(0, 5000); try {{ return JSON.parse(JSON.stringify(v === undefined ? null : v)); }} catch {{ return String(v); }} }};
  try {{ Promise.resolve(({function})()).then((v) => put({{ ok: true, v: safe(v) }}), err); }} catch (e) {{ err(e); }}
}})()"#
    ))
    .await?;
    let fetch = format!(
        "(() => {{ const s = window.__xtermium, r = s && s[{key}]; if (!r) return ''; delete s[{key}]; return JSON.stringify(r); }})()"
    );
    let start = std::time::Instant::now();
    loop {
        // The view reports a string as JSON text: the answer is JSON inside JSON.
        let got: String = serde_json::from_str(&eval(&view, fetch.clone()).await?).unwrap_or_default();
        if !got.is_empty() {
            let r: serde_json::Value = serde_json::from_str(&got).map_err(|e| e.to_string())?;
            return if r["ok"] == true { Ok(r["v"].clone()) } else { Err(r["e"].as_str().unwrap_or("the script failed").to_string()) };
        }
        if start.elapsed() > limit {
            return Err("no answer from the page in time (did it go to another page?)".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The page's part of finding things: visible elements that take a click or text, each numbered
/// once (`data-xtermium-ref`, kept between reads), and their names as a person reads them.
const PAGE_HELPERS: &str = r#"
  const CLICKABLE = 'a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=switch],[role=tab],[role=menuitem],[role=option],[role=combobox],[contenteditable=""],[contenteditable=true],[onclick]';
  const shown = (el) => { const r = el.getBoundingClientRect(), s = getComputedStyle(el); return (r.width > 0 || r.height > 0) && s.visibility !== 'hidden' && s.display !== 'none'; };
  const clean = (s) => String(s || '').replace(/\s+/g, ' ').trim().slice(0, 100);
  const nameOf = (el) => {
    const by = el.getAttribute('aria-labelledby'), lab = by && document.getElementById(by.split(' ')[0]);
    // A label around its field also holds the field's own text (a list's options): not part of the name.
    const wrap = el.labels && el.labels[0] && clean(el.labels[0].innerText).replace(clean(el.innerText), '').trim();
    return clean(el.getAttribute('aria-label') || (lab && lab.innerText) || wrap
      || (['submit', 'button', 'reset'].includes(el.type) && el.value) || el.innerText || el.placeholder || el.title || el.alt || el.name);
  };
  const refOf = (el) => { let r = el.getAttribute('data-xtermium-ref'); if (!r) { r = String((window.__xtermiumRef = (window.__xtermiumRef || 0) + 1)); el.setAttribute('data-xtermium-ref', r); } return +r; };
  const find = (target, text) => {
    if (typeof target === 'number') { const el = document.querySelector('[data-xtermium-ref="' + target + '"]'); if (!el) throw new Error('no element ' + target + ' on the page now: read it again'); return el; }
    if (target) { const el = document.querySelector(target); if (!el) throw new Error('nothing on the page matches ' + target); return el; }
    const want = String(text).toLowerCase(), all = [...document.querySelectorAll(CLICKABLE)].filter(shown);
    const el = all.find((e) => nameOf(e).toLowerCase() === want) || all.find((e) => nameOf(e).toLowerCase().includes(want));
    if (!el) throw new Error('nothing to click or fill is named "' + text + '"');
    return el;
  };
"#;

/// What a website tab shows: its address, title, text (at most `chars`) and the elements that take
/// a click or text, numbered for `page_click` and `page_fill`. Password fields' values are hidden.
pub async fn page_read(handle: &AppHandle, id: u32, chars: usize) -> Res<serde_json::Value> {
    let script = format!(
        r#"() => {{ {PAGE_HELPERS}
  const elements = [];
  for (const el of document.querySelectorAll(CLICKABLE)) {{
    if (elements.length >= 300) break;
    if (!shown(el)) continue;
    const tag = el.tagName.toLowerCase();
    const e = {{ ref: refOf(el), role: el.getAttribute('role') || (tag === 'input' ? el.type || 'text' : tag === 'a' ? 'link' : tag), name: nameOf(el) }};
    if (tag === 'a') e.href = el.href;
    if (['input', 'textarea', 'select'].includes(tag) && !['checkbox', 'radio', 'submit', 'button', 'reset'].includes(el.type))
      e.value = el.type === 'password' ? (el.value ? '(hidden)' : '') : tag === 'select' ? clean(el.selectedOptions[0] && el.selectedOptions[0].text) : String(el.value).slice(0, 200);
    if (el.type === 'checkbox' || el.type === 'radio') e.checked = el.checked;
    if (el.disabled) e.disabled = true;
    elements.push(e);
  }}
  const text = document.body ? document.body.innerText.replace(/\n{{3,}}/g, '\n\n') : '';
  return {{ url: location.href, title: document.title, text: text.slice(0, {chars}), truncated: text.length > {chars}, elements }};
}}"#
    );
    let mut page = page_run(handle, id, &script, Duration::from_secs(10)).await?;
    page["loading"] = serde_json::json!(OPEN.lock().unwrap().get(&id).is_some_and(|o| o.loading));
    Ok(page)
}

/// What `page_click` and `page_fill` act on: an element's number from `page_read`, a CSS selector,
/// or the text it's named by.
pub enum Target {
    Ref(u64),
    Selector(String),
    Named(String),
}

impl Target {
    /// The arguments of the page's `find`.
    fn js(&self) -> String {
        let s = |v: &str| serde_json::to_string(v).unwrap_or_default();
        match self {
            Target::Ref(n) => format!("{n}, null"),
            Target::Selector(sel) => format!("{}, null", s(sel)),
            Target::Named(text) => format!("null, {}", s(text)),
        }
    }
}

/// Clicks an element of a website tab's page (scrolled into view first), then waits for a page
/// the click opened to load.
pub async fn page_click(handle: &AppHandle, id: u32, target: &Target) -> Res<serde_json::Value> {
    let script = format!(
        r#"() => {{ {PAGE_HELPERS}
  const el = find({});
  el.scrollIntoView({{ block: 'center', inline: 'center' }});
  const r = el.getBoundingClientRect(), at = {{ bubbles: true, cancelable: true, view: window, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, button: 0 }};
  // What a person's click sends, for pages that act on the press rather than the click.
  el.dispatchEvent(new PointerEvent('pointerdown', at)); el.dispatchEvent(new MouseEvent('mousedown', at));
  if (el.focus) el.focus();
  el.dispatchEvent(new PointerEvent('pointerup', at)); el.dispatchEvent(new MouseEvent('mouseup', at));
  el.click();
  return {{ clicked: nameOf(el) || el.tagName.toLowerCase() }};
}}"#,
        target.js()
    );
    let r = page_run(handle, id, &script, Duration::from_secs(10)).await?;
    // A click that goes to another page starts loading a moment later.
    tokio::time::sleep(Duration::from_millis(300)).await;
    settle(id, Duration::from_secs(15)).await;
    Ok(r)
}

/// Puts `value` in a field of a website tab's page as typing would (the page sees input and change),
/// picks an option of a list, or ticks a box ("true"/"false"); `submit` sends the field's form, or
/// presses Enter in it.
pub async fn page_fill(handle: &AppHandle, id: u32, target: &Target, value: &str, submit: bool) -> Res<serde_json::Value> {
    let v = serde_json::to_string(value).unwrap_or_default();
    let script = format!(
        r#"() => {{ {PAGE_HELPERS}
  const el = find({target}), v = {v};
  el.scrollIntoView({{ block: 'center' }});
  if (el.focus) el.focus();
  if (el.tagName === 'SELECT') {{
    const o = [...el.options].find((o) => o.value === v || o.text.trim() === v);
    if (!o) throw new Error('the list has no option ' + v + ': ' + [...el.options].map((o) => o.text.trim()).join(', '));
    el.value = o.value;
  }} else if (el.type === 'checkbox' || el.type === 'radio') {{
    if (el.checked !== !/^(false|no|off|0|)$/i.test(v)) el.click();
  }} else if (el.isContentEditable) {{
    document.execCommand('selectAll'); document.execCommand('insertText', false, v);
  }} else if ('value' in el) {{
    // The element's own setter, so pages that watch the value (React and the like) see the change.
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, v);
  }} else throw new Error('that element takes no text: ' + el.tagName.toLowerCase());
  el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  if ({submit}) {{
    if (el.form) el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit();
    else for (const type of ['keydown', 'keypress', 'keyup']) el.dispatchEvent(new KeyboardEvent(type, {{ key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true }}));
  }}
  return {{ filled: nameOf(el) || el.tagName.toLowerCase() }};
}}"#,
        target = target.js()
    );
    let r = page_run(handle, id, &script, Duration::from_secs(10)).await?;
    if submit {
        tokio::time::sleep(Duration::from_millis(300)).await;
        settle(id, Duration::from_secs(15)).await;
    }
    Ok(r)
}

/// Goes to an address, back, forward or reloads, and waits for the page to load (at most 30 s).
pub async fn page_go(handle: &AppHandle, id: u32, to: &str) -> Res<serde_json::Value> {
    let view = site_view(handle, id)?;
    if let Some(o) = OPEN.lock().unwrap().get_mut(&id) {
        o.loading = true; // until the view says it has loaded
    }
    let r = match to {
        "back" => view.eval("history.back()"),
        "forward" => view.eval("history.forward()"),
        "reload" => view.reload(),
        url => view.navigate(address(url)?),
    };
    r.map_err(|e| e.to_string())?;
    let loaded = settle(id, Duration::from_secs(30)).await;
    let url = view.url().map(|u| u.to_string()).unwrap_or_default();
    let title = OPEN.lock().unwrap().get(&id).map(|o| o.title.clone()).unwrap_or_default();
    Ok(serde_json::json!({ "url": url, "title": title, "loaded": loaded }))
}

/// A picture of a website tab's page as it shows now, through the view's own DevTools (WebView2).
/// ponytail: Windows only; WKWebView (takeSnapshot) and WebKitGTK (get_snapshot) have their own
/// calls, not reached through Tauri yet.
pub async fn page_screenshot(handle: &AppHandle, id: u32) -> Res<Picture> {
    let view = site_view(handle, id)?;
    settle(id, Duration::from_secs(15)).await;
    #[cfg(windows)]
    {
        use base64::Engine;
        use windows::core::HSTRING;
        // A hidden view draws nothing to capture: it's shown for the moment, outside the window's
        // client area so nothing on screen changes, and hidden again after.
        let hidden = !OPEN.lock().unwrap().get(&id).is_some_and(|o| o.shown);
        if hidden {
            view.set_position(PhysicalPosition::new(-30_000.0, -30_000.0)).and_then(|_| view.show()).map_err(|e| e.to_string())?;
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        let (tx, rx) = tokio::sync::oneshot::channel::<Res<String>>();
        let shot = view.with_webview(move |w| {
            let done = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, json| {
                let _ = tx.send(hr.map(|()| json).map_err(|e| e.to_string()));
                Ok(())
            }));
            // SAFETY: with_webview runs on the view's thread with its controller alive; the
            // handler is kept by WebView2 until it has been called.
            unsafe {
                if let Ok(core) = w.controller().CoreWebView2() {
                    let _ = core.CallDevToolsProtocolMethod(&HSTRING::from("Page.captureScreenshot"), &HSTRING::from(r#"{"format":"png"}"#), &done);
                }
            }
        });
        let answer = match shot {
            Ok(()) => tokio::time::timeout(Duration::from_secs(10), rx).await.ok().and_then(Result::ok),
            Err(_) => None,
        };
        // Unless the tab came on screen meanwhile (web_place showed it where it belongs).
        if hidden && !OPEN.lock().unwrap().get(&id).is_some_and(|o| o.shown) {
            let _ = view.hide();
        }
        let json = answer.ok_or("the page didn't answer")??;
        let data: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let png = base64::engine::general_purpose::STANDARD.decode(data["data"].as_str().unwrap_or_default()).map_err(|e| e.to_string())?;
        let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
        Ok(Picture { w: img.width() as usize, h: img.height() as usize, px: img.into_raw() })
    }
    #[cfg(not(windows))]
    {
        let _ = view;
        Err("screenshots of websites work on Windows only for now: use web_read for what the page says".into())
    }
}

#[tauri::command]
pub fn web_close(window: Window, id: u32) {
    let temp = OPEN.lock().unwrap().remove(&id).is_some_and(|o| o.temp);
    if let Ok(w) = web(&window, id) {
        let _ = w.close();
    }
    GATEWAYS.lock().unwrap().remove(&id);
    if temp {
        forget_profile(id);
    }
}

/// Keeps a page from timing out while it sits there: now and then a mouse move the page's own idle
/// timer sees (dispatched on the document, so no console's screen gets it), and a quiet request
/// for the page, which keeps the server's session going. Hidden pages' timers run once a minute at
/// most, which is plenty.
const AWAKE: &str = r#"(() => {
  if (window.__xtmAwake || !location.protocol.startsWith("http")) return;
  window.__xtmAwake = true;
  setInterval(() => {
    document.dispatchEvent(new MouseEvent("mousemove", { bubbles: true }));
    if (window === window.top) fetch(location.href, { credentials: "include", cache: "no-store" }).catch(() => {});
  }, 4 * 60 * 1000);
})();"#;

// ---- website sessions' own profiles: <data folder>/websites/<profile> ----

fn sites() -> PathBuf {
    crate::config::dir().join("websites")
}

/// A website session's profile folder, kept between runs.
fn site(profile: &str) -> PathBuf {
    sites().join(profile)
}

/// Removes the profiles of website sessions that are gone (`keep`: every saved one's), once the
/// sessions are loaded; at start, while no website tab has its profile open.
pub fn sweep_sites(keep: &std::collections::HashSet<String>) {
    for dir in std::fs::read_dir(sites()).into_iter().flatten().flatten() {
        if !keep.contains(dir.file_name().to_string_lossy().as_ref()) {
            let _ = std::fs::remove_dir_all(dir.path());
        }
    }
}

/// A profile for one tab only (gateway and BMC console tabs), in this run's passing data
/// (config.rs `temp`, swept at the next start when this run ended without deleting it).
fn profile(id: u32) -> Option<PathBuf> {
    Some(crate::config::temp()?.join("web").join(id.to_string()))
}

/// Deletes a closed tab's profile once its browser has let go of the files (it lingers a few
/// seconds after its last view closes); whatever's still held then goes at the next start.
fn forget_profile(id: u32) {
    let Some(dir) = profile(id) else { return };
    std::thread::spawn(move || {
        for _ in 0..30 {
            std::thread::sleep(Duration::from_secs(2));
            if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Website keys are named as the UI names shortcuts (no modifier is held during tests).
    #[cfg(windows)]
    #[test]
    fn website_keys_named_like_shortcuts() {
        assert_eq!(chord(0x25).as_deref(), Some("ArrowLeft"));
        assert_eq!(chord(0x75).as_deref(), Some("F6"));
        assert_eq!(chord(0x50).as_deref(), Some("P"));
        assert_eq!(chord(0xBC).as_deref(), Some(","));
        assert_eq!(chord(0x5B), None, "the Windows key alone");
    }

}
