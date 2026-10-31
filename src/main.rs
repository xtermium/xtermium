// A GUI app on Windows: no console window, in debug builds too.
#![windows_subsystem = "windows"]

//! The backend: sessions, terminals, SSH/SFTP/RDP and tunnels, exposed to the UI (`ui/`) as
//! Tauri commands. Terminal and RDP output streams to the UI over per-tab channels.

mod automation;
mod clean;
mod cli;
mod config;
mod explorer;
mod files;
mod flow;
mod formats;
mod instance;
mod kvm;
mod lock;
mod broadcast;
mod paths;
mod prompt;
mod rdp;
#[allow(dead_code)] // each binary uses one direction of the protocol
mod rdp_proto;
mod settings;
mod snapshot;
mod ssh;
mod sshkeys;
mod support;
mod sync;
mod telnet;
mod term;
mod tray;
mod vault;
mod vnc;
mod web;

use config::{Kind, Session, Store};
use prompt::Prompter;
use serde::Serialize;
use settings::{Effect, Scheme, Settings};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

enum Tab {
    Term(Box<term::Tab>),
    Rdp(Box<rdp::RdpTab>),
    Vnc(Box<vnc::VncTab>),
}

pub struct App {
    /// The workspace in use: its sessions, logins, tunnels… (config::workspace names it).
    store: Mutex<Store>,
    /// The other workspaces, with an empty place for the one in use (`Store::split`).
    rest: Mutex<Store>,
    /// One-line message for the user (load errors, keychain trouble).
    notice: Mutex<String>,
    /// Some(recoverable) while the data key is missing and the stored files can't be read yet.
    locked: Mutex<Option<bool>>,
    settings: Mutex<Settings>,
    dark: AtomicBool,
    /// Window effect currently applied, with the light/dark mode it was tinted for.
    applied: Mutex<Option<(Effect, bool)>>,
    tabs: Mutex<HashMap<u32, Tab>>,
    pub next_id: AtomicU32,
    broadcast: AtomicBool,
    /// Tabs unticked for broadcast (every tab takes part until unticked).
    broadcast_off: Mutex<HashSet<u32>>,
    tunnels: Mutex<HashMap<String, flow::Running>>,
    prompter: Prompter,
    replies: Arc<Mutex<HashMap<u32, prompt::Reply>>>,
    /// Automation `open` requests waiting for the UI's tab id (0: opened outside xtermium).
    opens: Mutex<HashMap<u32, tokio::sync::oneshot::Sender<u32>>>,
    /// The running automation server: its port and token, and the task.
    automation: Mutex<Option<Serving>>,
    automation_error: Arc<Mutex<String>>,
    /// The folder this start was asked to open a local shell in ("Open in xtermium"), until the
    /// window has booted and taken it.
    here: Mutex<Option<String>>,
}

/// Starts, restarts or stops the automation server to match the settings.
fn apply_automation(app: &App, handle: &AppHandle) {
    let s = app.settings.lock().unwrap().clone();
    let want = (s.automation && !s.automation_token.is_empty()).then_some((s.automation_port, s.automation_token, s.automation_network));
    let mut running = app.automation.lock().unwrap();
    if running.as_ref().map(|(p, t, n, _)| (*p, t.clone(), *n)) == want {
        return;
    }
    if let Some((.., task)) = running.take() {
        task.abort();
    }
    app.automation_error.lock().unwrap().clear();
    if let Some((port, token, network)) = want {
        let (h, error) = (handle.clone(), app.automation_error.clone());
        let t = token.clone();
        let task = tauri::async_runtime::spawn(async move {
            if let Err(e) = automation::serve(h, port, t, network).await {
                *error.lock().unwrap() = format!("port {port}: {e}");
            }
        });
        *running = Some((port, token, network, task));
    }
    *automation::ENDPOINT.lock().unwrap() = running.as_ref().map(|(p, t, ..)| (*p, t.clone()));
}

/// The automation server running: its port, token, whether it's on the network, and its task.
type Serving = (u16, String, bool, tauri::async_runtime::JoinHandle<()>);

type Res<T> = Result<T, String>;

fn fail(e: anyhow::Error) -> String {
    format!("{e:#}")
}

impl App {
    /// Terminal look for a session under the current settings.
    fn look(&self, s: &Session) -> term::Look {
        let st = self.settings.lock().unwrap().clone();
        let base = term::scheme_palette(st.scheme(self.dark.load(Relaxed)));
        let (font, font_size, palette) = if st.override_sessions {
            (st.font.clone(), st.font_size, base)
        } else {
            // The global size (and font zoom) scales each session's own size, 14 being 1×.
            let size = s.style.font_size * st.font_size / Settings::default().font_size;
            (s.style.font.clone(), size, term::palette(&s.style, base))
        };
        term::Look {
            font,
            font_size,
            palette,
            scrollback: st.scrollback,
            scroll_on_output: st.scroll_on_output,
            cursor: st.cursor,
            cursor_blink: st.cursor_blink,
            line_height: st.line_height.clamp(0.8, 2.0),
            bg_alpha: if st.transparent() { st.terminal_opacity } else { 1.0 },
            copy_on_select: st.copy_on_select,
            right_click_paste: st.right_click_paste,
            strip_paste: st.strip_paste,
            log: st.log_to_file,
            highlights: Arc::new(settings::Highlight::compile(st.highlight, &st.highlights)),
        }
    }

    /// Re-style every open terminal (settings or light/dark mode changed).
    fn restyle(&self) {
        for t in self.tabs.lock().unwrap().values() {
            if let Tab::Term(t) = t {
                t.set_look(self.look(&t.session));
            }
        }
    }

    /// Native backdrop effect behind the transparent window, when it changed.
    fn apply_effect(&self, w: &tauri::Window) -> Res<()> {
        let want = (self.settings.lock().unwrap().effect, self.dark.load(Relaxed));
        let mut applied = self.applied.lock().unwrap();
        if *applied == Some(want) {
            return Ok(());
        }
        *applied = Some(want);
        let (effect, dark) = want;
        #[cfg(windows)]
        {
            let _ = window_vibrancy::clear_acrylic(w);
            let _ = window_vibrancy::clear_mica(w);
        }
        #[cfg(target_os = "macos")]
        {
            let _ = window_vibrancy::clear_vibrancy(w);
        }
        let r = match effect {
            Effect::Acrylic => {
                window_vibrancy::apply_acrylic(w, Some(if dark { (18, 18, 22, 110) } else { (245, 245, 248, 110) }))
            }
            Effect::Mica => window_vibrancy::apply_mica(w, Some(dark)),
            #[cfg(target_os = "macos")]
            Effect::Vibrancy => {
                window_vibrancy::apply_vibrancy(w, window_vibrancy::NSVisualEffectMaterial::HudWindow, None, None)
            }
            _ => Ok(()),
        };
        r.map_err(|e| format!("window effect: {e}"))
    }

    /// Stores every workspace.
    fn save_store(&self) -> Res<()> {
        self.all().save().map_err(fail)
    }

    /// Every workspace's data, as stored and synced.
    fn all(&self) -> Store {
        let store = self.store.lock().unwrap();
        self.rest.lock().unwrap().join(&config::workspace(), &store)
    }

    /// Takes every workspace's data; the workspace in use stays, unless `all` lacks it.
    fn set_all(&self, all: Store) {
        let mut ws = config::workspace();
        if !all.workspaces.iter().any(|w| w.name == ws) {
            ws.clear();
        }
        self.use_workspace(all, &ws);
    }

    fn use_workspace(&self, all: Store, ws: &str) {
        let (data, rest) = all.split(ws);
        config::set_workspace(ws);
        self.settings.lock().unwrap().workspace = ws.into();
        *self.store.lock().unwrap() = data;
        *self.rest.lock().unwrap() = rest;
    }

    fn workspaces(&self) -> Vec<String> {
        self.rest.lock().unwrap().workspaces.iter().map(|w| w.name.clone()).collect()
    }

    fn in_use(&self) -> Workspaces {
        Workspaces {
            workspaces: self.workspaces(),
            workspace: config::workspace(),
            store: self.store.lock().unwrap().clone(),
        }
    }

    /// Keeps a restore point of everything as it is now (snapshot.rs): a full backup's contents
    /// and the saved logins.
    fn snapshot(&self, reason: &str) -> Res<()> {
        let known = config::Known::with(|k| k.clone()).map_err(fail)?;
        let text = backup(&self.all(), &self.settings.lock().unwrap(), &known).map_err(fail)?;
        let text = format!("{text}\n{}", config::logins_text().map_err(fail)?);
        snapshot::take(&text, reason).map_err(fail)
    }

    /// Reads everything stored, once the data key is available.
    fn load(&self) {
        let mut settings = Settings::load();
        if settings.ensure_token() {
            let _ = settings.save();
        }
        if settings.high_priority {
            let _ = support::priority();
        }
        config::set_workspace(&settings.workspace);
        let notice = match Store::load() {
            Ok(mut s) => {
                // Website sessions saved before they had profiles get theirs; profiles of sessions
                // that are gone go.
                let (changed, profiles) = s.own_profiles();
                if changed {
                    let _ = s.save();
                }
                web::sweep_sites(&profiles);
                self.set_all(s);
                String::new()
            }
            Err(e) => format!("could not load sessions: {e:#}"),
        };
        settings.workspace = config::workspace();
        apply_defaults(&settings);
        // Encrypts the old plain-text known_hosts / known_rdp now rather than on first connect.
        let notice = match config::Known::with(|_| ()) {
            Err(e) if notice.is_empty() => format!("could not load trusted server keys: {e:#}"),
            _ => notice,
        };
        *self.settings.lock().unwrap() = settings;
        *self.locked.lock().unwrap() = None;
        lock::start();
        // The daily restore point, of data that loaded.
        let notice = if notice.is_empty() && snapshot::due() {
            self.snapshot("daily").err().map(|e| format!("could not keep a restore point: {e}")).unwrap_or_default()
        } else {
            notice
        };
        let mut n = self.notice.lock().unwrap();
        if !notice.is_empty() {
            *n = notice;
        }
    }
}

fn apply_defaults(s: &Settings) {
    ssh::set_defaults(ssh::Defaults {
        user: s.ssh_user.clone(),
        key: s.ssh_key.clone(),
        password: s.ssh_password.clone(),
        ciphers: s.ssh_ciphers.clone(),
        keepalive: s.ssh_keepalive,
        compression: s.ssh_compression,
    });
}

#[derive(Serialize)]
struct Boot {
    store: Store,
    notice: String,
    /// Some(recoverable) when the UI must ask for the recovery passphrase first.
    locked: Option<bool>,
    /// The lock (lock.rs) is on: the lock screen comes first.
    lock: bool,
    recovery: bool,
    settings: Settings,
    /// Tabs to reopen (Settings > General), empty unless that's on, and the names they were given.
    tabs: Vec<Session>,
    labels: Vec<String>,
    /// Workspaces besides the default one, and the one in use ("" the default).
    workspaces: Vec<String>,
    workspace: String,
    schemes: &'static [Scheme],
    effects: &'static [Effect],
    /// SSH ciphers xtermium can use, best first.
    ciphers: &'static [&'static str],
    version: &'static str,
    os: &'static str,
}

#[tauri::command]
fn boot(app: State<App>) -> Boot {
    // One lock: guards taken inside the literal below live until it's built.
    let settings = app.settings.lock().unwrap().clone();
    let mut tabs = if settings.restore_tabs { config::Tabs::load() } else { config::Tabs::default() };
    // Started from a folder in File Explorer: its shell opens after the tabs put back, and shows.
    if let Some(dir) = app.here.lock().unwrap().take() {
        tabs.labels.resize(tabs.tabs.len(), String::new());
        tabs.tabs.push(instance::shell_in(&dir));
        tabs.labels.push(String::new());
    }
    Boot {
        store: app.store.lock().unwrap().clone(),
        notice: app.notice.lock().unwrap().clone(),
        locked: *app.locked.lock().unwrap(),
        lock: lock::is_locked(),
        recovery: config::has_recovery(),
        tabs: tabs.tabs,
        labels: tabs.labels,
        workspaces: app.workspaces(),
        workspace: config::workspace(),
        settings,
        schemes: settings::SCHEMES,
        effects: Effect::available(),
        ciphers: &ssh::CIPHERS,
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
    }
}

/// Stores workspace `workspace`'s sessions, cleaned (clean.rs); returns them as stored. One sent
/// just before a switch lands in the workspace it came from.
#[tauri::command]
fn save_store(app: State<App>, mut store: Store, workspace: String) -> Res<Store> {
    store.sanitize();
    store.workspaces.clear();
    if workspace == config::workspace() {
        *app.store.lock().unwrap() = store.clone();
    } else {
        let mut rest = app.rest.lock().unwrap();
        *rest = rest.join(&workspace, &store);
    }
    app.save_store()?;
    Ok(store)
}

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum WorkspaceOp {
    Switch { name: String },
    Add { name: String },
    Rename { name: String, to: String },
    Delete { name: String },
    /// A copy of workspace `name`'s sessions and folders, named "<name> (clone)".
    Clone { name: String },
    /// The sessions and folders of workspace `from` come into `to` (config.rs `sync_sessions`).
    Sync { from: String, to: String },
}

#[derive(Serialize)]
struct Workspaces {
    workspaces: Vec<String>,
    workspace: String,
    /// The data of the workspace in use.
    store: Store,
}

/// Settings > Workspaces, and the sidebar's switcher. The default workspace ("") always stays.
#[tauri::command]
fn workspace(app: State<App>, op: WorkspaceOp) -> Res<Workspaces> {
    workspace_op(&app, op)
}

/// What `workspace` does; automation (automation.rs) switches workspaces with it too.
fn workspace_op(app: &App, op: WorkspaceOp) -> Res<Workspaces> {
    let ws = config::workspace();
    let taken = |n: &str| app.workspaces().iter().any(|w| w == n);
    let valid = |n: &str| -> Res<String> {
        let n = n.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
        if n.is_empty() {
            return Err("Give the workspace a name.".into());
        }
        if n.chars().count() > 24 {
            return Err("Keep the name short: 24 characters at most.".into());
        }
        if taken(&n) {
            return Err(format!("There is a workspace named {n} already."));
        }
        Ok(n)
    };
    match op {
        WorkspaceOp::Switch { name } if name == ws => {}
        WorkspaceOp::Switch { name } => {
            if !name.is_empty() && !taken(&name) {
                return Err(format!("No workspace named {name}."));
            }
            app.use_workspace(app.all(), &name);
        }
        WorkspaceOp::Add { name } => {
            let name = valid(&name)?;
            app.rest.lock().unwrap().workspaces.push(config::Workspace { name, data: Store::default() });
        }
        WorkspaceOp::Rename { name, to } => {
            if !taken(&name) {
                return Err(format!("No workspace named {name}."));
            }
            let to = valid(&to)?;
            let mut rest = app.rest.lock().unwrap();
            if let Some(w) = rest.workspaces.iter_mut().find(|w| w.name == name) {
                w.name = to.clone();
            }
            drop(rest);
            if ws == name {
                config::set_workspace(&to);
                app.settings.lock().unwrap().workspace = to;
            }
        }
        WorkspaceOp::Delete { name } => {
            if name.is_empty() {
                return Err("The default workspace stays.".into());
            }
            if ws == name {
                app.use_workspace(app.all(), "");
            }
            app.rest.lock().unwrap().workspaces.retain(|w| w.name != name);
        }
        WorkspaceOp::Clone { name } => {
            let mut all = app.all();
            let data = all.data(&name).ok_or_else(|| format!("No workspace named {name}."))?;
            let to = clone_name(if name.is_empty() { "Default" } else { &name }, |n| n.is_empty() || taken(n));
            all.workspaces.push(config::Workspace { name: to, data });
            all.own_profiles(); // the copies' websites sign in on their own
            app.use_workspace(all, &ws);
        }
        WorkspaceOp::Sync { from, to } => {
            let known = |n: &str| n.is_empty() || taken(n);
            if !known(&from) || !known(&to) {
                return Err("Pick two workspaces there are.".into());
            }
            if from == to {
                return Err("Pick two different workspaces.".into());
            }
            app.snapshot("before-sync")?;
            let mut all = app.all();
            let src = all.data(&from).unwrap_or_default();
            if let Some(d) = all.data_mut(&to) {
                d.sync_sessions(&src);
            }
            all.own_profiles();
            app.use_workspace(all, &ws);
        }
    }
    app.save_store()?;
    let settings = app.settings.lock().unwrap().clone();
    settings.save().map_err(fail)?;
    Ok(app.in_use())
}

/// "<name> (clone)", or "<name> (clone 2)" and so on if that's `taken`, cut to fit 24 characters.
fn clone_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| {
            let tag = if n == 1 { " (clone)".to_string() } else { format!(" (clone {n})") };
            let base: String = name.chars().take(24 - tag.chars().count()).collect();
            format!("{}{tag}", base.trim_end())
        })
        .find(|n| !taken(n))
        .unwrap()
}

/// Moves sessions (by index in the workspace in use) and folders (with everything in them) to
/// workspace `to`; returns the workspace in use.
#[tauri::command]
fn move_to_workspace(app: State<App>, to: String, sessions: Vec<usize>, folders: Vec<String>) -> Res<Workspaces> {
    move_op(&app, &to, &sessions, &folders)
}

/// What `move_to_workspace` does; automation (automation.rs) moves sessions with it too.
fn move_op(app: &App, to: &str, sessions: &[usize], folders: &[String]) -> Res<Workspaces> {
    if to == config::workspace() {
        return Err("They're in that workspace already.".into());
    }
    if !to.is_empty() && !app.workspaces().iter().any(|w| w == to) {
        return Err(format!("No workspace named {to}."));
    }
    let moved = app.store.lock().unwrap().take_out(sessions, folders);
    let mut rest = app.rest.lock().unwrap();
    let (mut there, others) = rest.clone().split(to);
    there.put_in(moved);
    *rest = others.join(to, &there);
    drop(rest);
    app.save_store()?;
    Ok(app.in_use())
}

/// The passwords saved when asked for (Settings > Saved logins), every workspace's.
#[tauri::command]
fn saved_logins() -> Res<Vec<config::Login>> {
    config::logins(|l| l.clone()).map_err(fail)
}


/// Changes a saved password; None forgets it.
#[tauri::command]
fn set_login(id: String, password: Option<String>) -> Res<()> {
    match password {
        Some(p) => config::remember(&id, &p).map_err(fail),
        None => {
            config::forget(&id);
            Ok(())
        }
    }
}

/// Stores and applies the settings; returns them as stored (an empty object resets to defaults).
#[tauri::command]
fn save_settings(app: State<App>, window: tauri::Window, mut settings: Settings) -> Res<Settings> {
    settings.sanitize();
    // The automation token only changes when "New token" sends one: a save without it (a reset,
    // an older copy of the settings) keeps the one there is.
    if settings.automation_token.is_empty() {
        settings.automation_token = app.settings.lock().unwrap().automation_token.clone();
    }
    settings.ensure_token();
    // Only switching changes the workspace (`workspace`), never a copy of the settings; only
    // `background_set` changes the terminal background (a copy can still change how much shows,
    // and how a picture fits).
    settings.workspace = config::workspace();
    let mut have = app.settings.lock().unwrap().clone();
    if settings.start_at_login != have.start_at_login {
        support::autostart(window.app_handle(), settings.start_at_login).map_err(|e| format!("Start at login: {e}"))?;
    }
    if settings.high_priority && !have.high_priority {
        support::priority()?;
    }
    for place in ["terminal", "sidebar", "bar"] {
        let (b, _) = settings.background_of(place).unwrap();
        let (kept, _) = have.background_of(place).unwrap();
        let fit = std::mem::take(&mut b.fit);
        *b = settings::Background { show: b.show, fit, ..kept.clone() };
    }
    let old = std::mem::replace(&mut *app.settings.lock().unwrap(), settings.clone());
    apply_defaults(&settings);
    apply_automation(&app, window.app_handle());
    tray::apply(window.app_handle(), settings.keep_running);
    if settings.restyles(&old) {
        app.restyle();
    }
    let effect = if window.label() == "main" { app.apply_effect(&window) } else { Ok(()) };
    settings.save().map_err(|e| format!("settings: {e:#}"))?;
    // Detached tabs' windows follow (pane padding, theme).
    let _ = window.app_handle().emit("settings", &settings);
    effect.map(|_| settings)
}

/// A background's picture (a `data:` URL) or shader source, "" for none. `place`: "terminal",
/// "sidebar" or "bar" (the tab bar).
#[tauri::command]
fn background_get(place: String) -> Res<String> {
    let (_, file) = Settings::default().background_of(&place).ok_or("No such place.")?;
    config::read(file, &[], |_| Ok(String::new())).map_err(fail)
}

/// Sets what's behind `place` (as in `background_get`): `kind` "image" with a `data:` URL,
/// "shader" with GLSL source (the UI compiled it already), or "" for nothing. Returns the
/// settings as stored.
#[tauri::command]
fn background_set(app: State<App>, window: tauri::Window, place: String, kind: String, name: String, data: String) -> Res<Settings> {
    let file = Settings::default().background_of(&place).ok_or("No such place.")?.1;
    let data = match kind.as_str() {
        "" => String::new(),
        "image" => {
            use base64::Engine;
            let body = data.split_once(";base64,").map(|(_, b)| b).ok_or("That isn't a picture.")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(body).map_err(|_| "That isn't a picture.")?;
            if bytes.len() > BACKGROUND_MAX {
                return Err(format!("Pick a picture under {} MB.", BACKGROUND_MAX as f64 / (1 << 20) as f64));
            }
            let mime = icon_type(&bytes).ok_or("That file isn't a PNG, GIF, JPEG, WebP or SVG picture.")?;
            if mime == "image/x-icon" {
                return Err("Icons are too small for a background: pick a PNG, GIF, JPEG, WebP or SVG.".into());
            }
            format!("data:{mime};base64,{body}")
        }
        "shader" if data.len() > SHADER_MAX => return Err(format!("A shader of {} KB at most, please.", SHADER_MAX >> 10)),
        "shader" if data.contains('\0') || !(data.contains("void main") || data.contains("mainImage")) => {
            return Err("That isn't a GLSL fragment shader: it needs a main() or Shadertoy's mainImage().".into());
        }
        "shader" => data,
        _ => return Err("Pick a picture or a shader.".into()),
    };
    config::write(file, &data).map_err(fail)?;
    let mut settings = app.settings.lock().unwrap().clone();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let (b, _) = settings.background_of(&place).unwrap();
    *b = settings::Background { kind, name, stamp, ..b.clone() };
    settings.sanitize();
    // save_settings keeps the backgrounds it has: this one goes in first.
    *app.settings.lock().unwrap().background_of(&place).unwrap().0 = settings.background_of(&place).unwrap().0.clone();
    save_settings(app, window, settings)
}

/// The biggest background picture (2.5 MB: it's read into memory and drawn on every frame) and shader taken.
const BACKGROUND_MAX: usize = 5 << 19;
const SHADER_MAX: usize = 256 << 10;

/// Downloads a terminal background from a link: `["image", data URL]` for a picture, else
/// `["shader", source]` for text (a raw GLSL file).
#[tauri::command]
async fn fetch_background(url: String) -> Res<(String, String)> {
    let body = download(url.trim(), BACKGROUND_MAX).await.map_err(|e| e.replace("file", "picture"))?;
    background_data(body)
}

/// A background's kind ("image" or "shader") and data (a `data:` picture, or the shader's source)
/// from a file's bytes.
fn background_data(body: Vec<u8>) -> Res<(String, String)> {
    use base64::Engine;
    if let Some(mime) = icon_type(&body).filter(|m| *m != "image/x-icon") {
        return Ok(("image".into(), format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&body))));
    }
    match String::from_utf8(body) {
        Ok(text) if text.len() <= SHADER_MAX => Ok(("shader".into(), text)),
        _ => Err("That link isn't a picture or a shader file.".into()),
    }
}

/// For assistants on other machines (Settings > MCP, on the network): what they may call
/// this computer, and the fingerprint of the certificate it answers with.
#[derive(Serialize)]
struct NetworkInfo {
    addresses: Vec<String>,
    fingerprint: String,
}

#[tauri::command]
fn automation_network_info() -> Res<NetworkInfo> {
    let (cert, _) = automation::certificate().map_err(fail)?;
    Ok(NetworkInfo { addresses: automation::addresses(), fingerprint: automation::fingerprint(&cert) })
}

/// Why a highlight rule's pattern doesn't work; "" if it does (Settings > Terminal).
#[tauri::command]
fn regex_error(pattern: String) -> String {
    regex::RegexBuilder::new(&pattern).size_limit(1 << 20).build().err().map(|e| e.to_string()).unwrap_or_default()
}

/// Remembers the open tabs, to reopen them next time.
#[tauri::command]
fn save_tabs(tabs: Vec<Session>, labels: Vec<String>) -> Res<()> {
    config::Tabs::save(tabs, labels).map_err(fail)
}

/// The UI tells us whether it is showing light or dark (for "Auto" colours and effect tints).
#[tauri::command]
fn set_dark(app: State<App>, window: tauri::Window, dark: bool) -> Res<()> {
    if app.dark.swap(dark, Relaxed) != dark {
        app.restyle();
    }
    app.apply_effect(&window)
}

/// Installed monospace font families, sorted.
#[tauri::command]
async fn fonts() -> Vec<String> {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(f) = FONTS.get() {
        return f.clone();
    }
    let list = tauri::async_runtime::spawn_blocking(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut v: Vec<String> =
            db.faces().filter(|f| f.monospaced).filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        v.sort_unstable_by_key(|n| n.to_lowercase());
        v.dedup();
        v
    })
    .await
    .unwrap_or_default();
    FONTS.get_or_init(|| list).clone()
}


#[tauri::command]
fn serial_ports() -> Vec<String> {
    serialport::available_ports().unwrap_or_default().into_iter().map(|p| p.port_name).collect()
}

/// Adds what an import brought that isn't there yet.
fn merge(app: &App, mut incoming: Store, skipped: usize) -> Res<(Store, String)> {
    incoming.sanitize();
    incoming.workspaces.clear();
    let (s, t) = app.store.lock().unwrap().merge(incoming);
    app.save_store()?;
    let store = app.store.lock().unwrap().clone();
    let mut msg = format!("Imported {s} sessions and {t} tunnels.");
    if skipped > 0 {
        msg.push_str(&format!(" {skipped} entries of unsupported kinds were skipped."));
    }
    Ok((store, msg))
}

/// Other apps' session files found on this computer.
#[tauri::command]
async fn import_sources() -> Vec<formats::Source> {
    tauri::async_runtime::spawn_blocking(formats::sources).await.unwrap_or_default()
}

/// Imports a file the user picked. Encrypted `.xtermium` exports need their passphrase.
#[tauri::command]
async fn import_file(handle: AppHandle, name: String, data: Vec<u8>, passphrase: String) -> Res<(Store, String)> {
    let format = formats::Format::detect(&name, &data).ok_or(
        "Not a file xtermium can import: use a .xtermium export, a MobaXterm .mxtsessions/.ini, or an OpenSSH config.",
    )?;
    let (incoming, skipped) = if format == formats::Format::Xtermium {
        let plain = tauri::async_runtime::spawn_blocking(move || vault::export_open(&passphrase, &data))
            .await
            .map_err(|e| e.to_string())?
            .map_err(fail)?;
        (toml::from_str(&String::from_utf8_lossy(&plain)).map_err(|e| e.to_string())?, 0)
    } else {
        let r = format.import(&data).map_err(fail)?;
        (Store { sessions: r.sessions, tunnels: r.tunnels, ..Default::default() }, r.skipped)
    };
    merge(&handle.state::<App>(), incoming, skipped)
}

/// Imports one of [`import_sources`].
#[tauri::command]
fn import_path(app: State<App>, format: formats::Format, path: String) -> Res<(Store, String)> {
    let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let r = format.import(&data).map_err(fail)?;
    merge(&app, Store { sessions: r.sessions, tunnels: r.tunnels, ..Default::default() }, r.skipped)
}

/// Saves `folder` and everything in it (everything, for "") to a file the user picks. The
/// xtermium format is encrypted with `passphrase`. Returns where it went, and how many
/// sessions the format had no room for; None if the user cancelled.
#[tauri::command]
async fn export_sessions(
    handle: AppHandle,
    format: formats::Format,
    folder: String,
    passphrase: String,
) -> Res<Option<(String, usize)>> {
    use tauri_plugin_dialog::DialogExt;
    let part = handle.state::<App>().store.lock().unwrap().subtree(&folder);
    let base = if folder.is_empty() { "sessions" } else { folder.rsplit('/').next().unwrap_or("sessions") };
    let (data, skipped, ext) = if format == formats::Format::Xtermium {
        if passphrase.chars().count() < 8 {
            return Err("Use a passphrase of at least 8 characters.".into());
        }
        let text = toml::to_string_pretty(&part).map_err(|e| e.to_string())?;
        let data = tauri::async_runtime::spawn_blocking(move || vault::export_seal(&passphrase, text.as_bytes()))
            .await
            .map_err(|e| e.to_string())?
            .map_err(fail)?;
        (data, 0, "xtermium".to_string())
    } else {
        let mut out = format.export(&part).map_err(fail)?;
        let (name, data) = out.files.remove(0);
        let ext = name.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_string());
        (data, out.skipped, ext)
    };
    let base = clean::file_name(base);
    let file = if ext.is_empty() { base } else { format!("{base}.{ext}") };
    let mut dialog = handle.dialog().file().set_file_name(file);
    if !ext.is_empty() {
        dialog = dialog.add_filter(&ext, &[ext.as_str()]);
    }
    let Some(path) = dialog.blocking_save_file().and_then(|p| p.into_path().ok()) else { return Ok(None) };
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(Some((path.display().to_string(), skipped)))
}

/// This device's own settings, which a backup neither carries nor replaces: the sync sign-in and
/// the automation token.
fn keep_device(to: &mut Settings, from: &Settings) {
    to.sync = from.sync;
    to.sync_token = from.sync_token.clone();
    to.sync_email = from.sync_email.clone();
    to.automation_token = from.automation_token.clone();
}

/// A full backup (Settings > Import and export): the store as an export has it, with the
/// settings and trusted server keys beside it, so an import of sessions reads it too.
fn backup(store: &Store, settings: &Settings, known: &config::Known) -> anyhow::Result<String> {
    let toml::Value::Table(mut t) = toml::Value::try_from(store)? else { anyhow::bail!("the sessions aren't a table") };
    let mut settings = settings.clone();
    keep_device(&mut settings, &Settings::default());
    t.insert("settings".into(), toml::Value::try_from(settings)?);
    t.insert("known".into(), toml::Value::try_from(known)?);
    Ok(toml::to_string_pretty(&t)?)
}

/// What a backup holds; `None` settings for an export of sessions (not a full backup).
fn unbackup(text: &str) -> anyhow::Result<(Store, Option<Settings>, Option<config::Known>)> {
    let mut t: toml::Table = toml::from_str(text)?;
    let settings = t.remove("settings").map(|s| s.try_into()).transpose()?;
    let known = t.remove("known").map(|k| k.try_into()).transpose()?;
    Ok((toml::Value::Table(t).try_into()?, settings, known))
}

/// Saves everything (sessions, folders, shared logins with their passwords, tunnels, snippets,
/// settings, trusted server keys) to one file encrypted with `passphrase`. Where it went; None if
/// the user cancelled.
#[tauri::command]
async fn backup_save(handle: AppHandle, passphrase: String) -> Res<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    if passphrase.chars().count() < 8 {
        return Err("Use a passphrase of at least 8 characters.".into());
    }
    let app = handle.state::<App>();
    let known = config::Known::with(|k| k.clone()).map_err(fail)?;
    let text = backup(&app.all(), &app.settings.lock().unwrap(), &known).map_err(fail)?;
    let data = tauri::async_runtime::spawn_blocking(move || vault::export_seal(&passphrase, text.as_bytes()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(fail)?;
    let file = format!("xtermium backup {}.xtermium", chrono::Local::now().format("%Y-%m-%d"));
    let dialog = handle.dialog().file().set_file_name(file).add_filter("xtermium", &["xtermium"]);
    let Some(path) = dialog.blocking_save_file().and_then(|p| p.into_path().ok()) else { return Ok(None) };
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(Some(path.display().to_string()))
}

/// Replaces everything with a backup from [`backup_save`]; this device's sync sign-in and
/// automation token stay. The UI loads it all again afterwards.
#[tauri::command]
async fn backup_restore(handle: AppHandle, window: tauri::Window, data: Vec<u8>, passphrase: String) -> Res<()> {
    let plain = tauri::async_runtime::spawn_blocking(move || vault::export_open(&passphrase, &data))
        .await
        .map_err(|e| e.to_string())?
        .map_err(fail)?;
    replace_all(handle.state::<App>(), window, &String::from_utf8_lossy(&plain))
}

/// Replaces everything with `text` (a full backup's, or a restore point's, which also has the saved
/// logins); this device's sync sign-in and automation token stay. A restore point of what was
/// there is kept first, so it can be undone.
fn replace_all(app: State<App>, window: tauri::Window, text: &str) -> Res<()> {
    let (mut store, settings, known) = unbackup(text).map_err(fail)?;
    let mut settings = settings.ok_or("This file holds sessions only, not a full backup: import it with Import sessions.")?;
    app.snapshot("before-restore")?;
    keep_device(&mut settings, &app.settings.lock().unwrap());
    store.sanitize();
    app.set_all(store);
    app.save_store()?;
    if let Some(k) = known {
        config::Known::with(|cur| *cur = k).map_err(fail)?;
    }
    config::restore_logins(text).map_err(fail)?;
    save_settings(app, window, settings).map(|_| ())
}

/// The restore points (Settings > Import and export), newest first.
#[tauri::command]
fn restore_points() -> Vec<snapshot::Point> {
    snapshot::list()
}

/// Keeps a restore point of everything now.
#[tauri::command]
fn restore_point_take(app: State<App>) -> Res<Vec<snapshot::Point>> {
    app.snapshot("manual")?;
    Ok(snapshot::list())
}

#[tauri::command]
fn restore_point_delete(id: String) -> Res<Vec<snapshot::Point>> {
    snapshot::delete(&id).map_err(fail)?;
    Ok(snapshot::list())
}

/// Puts everything back as restore point `id` has it (`replace_all`). The UI loads it all again
/// afterwards, and may sync it up (state.svelte.ts `syncNow` with "local").
#[tauri::command]
fn restore_point_restore(app: State<App>, window: tauri::Window, id: String) -> Res<()> {
    replace_all(app, window, &snapshot::read(&id).map_err(fail)?)
}

/// Unlocks stored data with the recovery passphrase (new device, lost keychain entry).
#[tauri::command]
async fn unlock(handle: AppHandle, passphrase: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || config::unlock(&passphrase))
        .await
        .map_err(|e| e.to_string())?
        .map_err(fail)?;
    let app = handle.state::<App>();
    app.load();
    apply_automation(&app, &handle);
    Ok(())
}

/// No way to unlock: set the unreadable files aside and start empty.
#[tauri::command]
fn start_over(app: State<App>) -> Res<()> {
    config::start_over().map_err(fail)?;
    app.load();
    Ok(())
}

#[tauri::command]
async fn set_recovery(passphrase: String, current: String) -> Res<()> {
    if passphrase.chars().count() < 12 {
        return Err("Use a recovery passphrase of at least 12 characters.".into());
    }
    tauri::async_runtime::spawn_blocking(move || config::set_recovery(&passphrase, &current))
        .await
        .map_err(|e| e.to_string())?
        .map_err(fail)
}

#[derive(Serialize)]
struct SyncReply {
    /// The server holds data this device's key can't open: ask for the recovery passphrase.
    join: bool,
    /// Both sides changed: ask which copy to keep, then call again with the choice.
    conflict: Option<sync::Conflict>,
    pulled: bool,
    pushed: bool,
    /// The account's storage, used and allowed, when the server says.
    quota: Option<sync::Quota>,
    /// Nothing synced because of this, and the UI acts on it: signed out, needs Pro, or try later.
    trouble: Option<sync::Trouble>,
    /// The workspace in use's data, and the workspaces (a pull may bring new ones).
    #[serde(flatten)]
    data: Workspaces,
}

/// Syncs with the cloud (docs/sync.md); with `passphrase`, joins the account's data first.
/// `choice` settles a conflict the previous call reported.
#[tauri::command]
async fn sync_now(handle: AppHandle, passphrase: Option<String>, choice: Option<sync::Choice>) -> Res<SyncReply> {
    static SYNCING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one_at_a_time = SYNCING.lock().await;
    let app = handle.state::<App>();
    let s = app.settings.lock().unwrap().clone();
    let api = sync::Api::new("", &s.sync_token).map_err(fail)?;
    // Every workspace syncs; the UI gets back the one in use.
    let before = app.all();
    let key = config::key().map_err(fail)?;
    let mut dev = sync::Device {
        key,
        store: before.clone(),
        known: config::Known::with(|k| k.clone()).map_err(fail)?,
        recovery: config::recovery_text(),
        state: sync::State::load(),
        changed: config::modified(),
    };
    let r = match passphrase {
        Some(p) => sync::join(&mut dev, &api, &p, choice).await,
        None => sync::sync(&mut dev, &api, choice).await,
    };
    let reply = |join, conflict, quota, trouble| SyncReply { join, conflict, pulled: false, pushed: false, quota, trouble, data: app.in_use() };
    let out = match r {
        Ok(out) => out,
        Err(e) if e.is::<sync::NeedsPassphrase>() => return Ok(reply(true, None, None, None)),
        Err(e) => {
            return match e.downcast::<sync::Trouble>() {
                Ok(t) => Ok(reply(false, None, None, Some(t))),
                Err(e) => Err(fail(e)),
            };
        }
    };
    if out.conflict.is_some() {
        return Ok(reply(false, out.conflict, out.quota, None));
    }

    if dev.key != key {
        config::rekey(dev.key).map_err(fail)?;
    }
    if dev.recovery.is_some() && dev.recovery != config::recovery_text() {
        config::set_recovery_text(dev.recovery.as_deref().unwrap_or_default()).map_err(fail)?;
    }
    // Sessions edited during the sync stay as edited; the next sync takes them up.
    let now = app.all();
    if toml::to_string(&now).ok() != toml::to_string(&before).ok() {
        dev.store = now;
        dev.state.synced.clear();
    }
    app.set_all(std::mem::take(&mut dev.store));
    app.save_store()?;
    config::Known::with(|k| *k = std::mem::take(&mut dev.known)).map_err(fail)?;
    dev.state.save().map_err(fail)?;
    Ok(SyncReply { join: false, conflict: None, pulled: out.pulled, pushed: out.pushed, quota: out.quota, trouble: None, data: app.in_use() })
}

/// Deletes everything the sync server holds for the account (this device keeps its data).
#[tauri::command]
async fn sync_delete(app: State<'_, App>) -> Res<()> {
    let s = app.settings.lock().unwrap().clone();
    sync::Api::new("", &s.sync_token).map_err(fail)?.delete().await.map_err(fail)?;
    sync::State::default().save().map_err(fail)
}

/// Starts a sign-in on xtermium's server (docs/sync.md): a code the person approves in the
/// browser. The UI opens the page and polls with `account_device_poll`.
#[tauri::command]
async fn account_device_start() -> Res<sync::DeviceCode> {
    sync::device_start("").await.map_err(fail)
}

/// One poll of the sign-in: pending, slow down, or the token, which the UI keeps in Settings.
#[tauri::command]
async fn account_device_poll(device_code: String) -> Res<sync::Poll> {
    sync::device_poll("", &device_code).await.map_err(fail)
}

/// Who is signed in, and whether the account has Pro.
#[tauri::command]
async fn account_info(token: String) -> Res<sync::Account> {
    sync::account("", &token).await.map_err(fail)
}

/// The address of the subscription's portal (cancel, change the card, invoices).
#[tauri::command]
async fn account_portal(token: String) -> Res<String> {
    sync::portal("", &token).await.map_err(fail)
}

/// Ends the session on the server (`everywhere`: every session of the account); the UI forgets
/// the token whatever the answer.
#[tauri::command]
async fn account_sign_out(token: String, everywhere: bool) -> Res<()> {
    sync::sign_out("", &token, everywhere).await.map_err(fail)
}

/// The lock (Settings › Security): the ways to unlock that are set, and whether it's locked.
#[tauri::command]
async fn lock_info() -> lock::Info {
    lock::info()
}

/// Locks every window, if there's a way to unlock; says whether it locked.
#[tauri::command]
fn lock_now(handle: AppHandle) -> bool {
    let locked = lock::lock();
    if locked {
        let _ = handle.emit("lock", true);
    }
    locked
}

/// The window Windows Hello's question goes over.
fn hwnd(w: &tauri::Window) -> isize {
    #[cfg(windows)]
    return w.hwnd().map_or(0, |h| h.0 as isize);
    #[cfg(not(windows))]
    {
        let _ = w;
        0
    }
}

/// Unlocks every window with `proof` (or, unlocked, confirms it's you before a change).
#[tauri::command]
async fn lock_unlock(handle: AppHandle, window: tauri::Window, proof: lock::Proof) -> Res<()> {
    let hwnd = hwnd(&window);
    tauri::async_runtime::spawn_blocking(move || lock::unlock(proof, hwnd)).await.map_err(|e| e.to_string())?.map_err(fail)?;
    let _ = handle.emit("lock", false);
    Ok(())
}

/// A challenge for a passkey to sign or be made with.
#[tauri::command]
fn lock_challenge() -> String {
    lock::challenge()
}

#[tauri::command]
async fn lock_set(handle: AppHandle, window: tauri::Window, change: lock::Change) -> Res<()> {
    let hwnd = hwnd(&window);
    tauri::async_runtime::spawn_blocking(move || lock::set(change, hwnd)).await.map_err(|e| e.to_string())?.map_err(fail)?;
    let _ = handle.emit("lock", lock::is_locked());
    Ok(())
}

/// The window saw input: the lock's idle time starts over (where the OS doesn't tell it).
#[tauri::command]
fn lock_activity() {
    lock::activity();
}

/// Settings › SSH keys: the key pairs in ~/.ssh.
#[tauri::command]
fn ssh_keys() -> Res<sshkeys::Keys> {
    sshkeys::list().map_err(fail)
}

/// Makes a key pair in ~/.ssh (an RSA one takes a moment, so off the main thread).
#[tauri::command]
async fn ssh_key_create(name: String, kind: String, comment: String, passphrase: String) -> Res<sshkeys::Key> {
    tauri::async_runtime::spawn_blocking(move || sshkeys::create(name.trim(), &kind, &comment, &passphrase))
        .await
        .map_err(|e| e.to_string())?
        .map_err(fail)
}

#[tauri::command]
fn ssh_key_delete(name: String) -> Res<()> {
    sshkeys::delete(&name).map_err(fail)
}

/// Trusted SSH host keys and RDP server keys.
#[tauri::command]
fn known_keys() -> Res<config::Known> {
    config::Known::with(|k| k.clone()).map_err(fail)
}

#[tauri::command]
fn forget_key(kind: String, id: String) -> Res<()> {
    config::Known::with(|k| {
        match kind.as_str() {
            "rdp" => k.rdp.remove(&id),
            "https" => k.https.remove(&id),
            _ => k.ssh.remove(&id),
        };
    })
    .map_err(fail)
}

/// Downloads an image for a sidebar icon. Returns it as a `data:` URL, stored with the session
/// so it shows offline. The UI shows icons in `<img>`, where an SVG's scripts don't run.
#[tauri::command]
async fn fetch_icon(url: String) -> Res<String> {
    use base64::Engine;
    let body = download(&url, 512 * 1024).await.map_err(|e| e.replace("file", "image"))?;
    let mime = icon_type(&body).ok_or("That link isn't a PNG, GIF, JPEG, WebP, ICO or SVG image.")?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&body)))
}

/// The icon of the website at `url`, as a `data:` URL: the page's biggest `<link rel=icon>`, else
/// `/favicon.ico`. Certificates aren't checked: admin pages are mostly self-signed, and all that
/// comes back is a picture shown in `<img>`.
#[tauri::command]
async fn fetch_favicon(url: String) -> Res<String> {
    use base64::Engine;
    let start = reqwest::Url::parse(url.trim()).map_err(|_| "Enter a web address starting with https://".to_string())?;
    // A page that can't be read (a sign-in wall) may still have /favicon.ico.
    let (base, links) = match fetch(start.as_str(), 1024 * 1024, true).await {
        Ok((at, page)) => {
            let links = favicon_links(&String::from_utf8_lossy(&page), &at);
            (at, links)
        }
        Err(_) => (start, vec![]),
    };
    for link in links.into_iter().chain(base.join("/favicon.ico").ok()) {
        if link.scheme() == "data" {
            if link.as_str().starts_with("data:image/") {
                return Ok(link.into());
            }
            continue;
        }
        if let Ok((_, body)) = fetch(link.as_str(), 512 * 1024, true).await
            && let Some(mime) = icon_type(&body)
        {
            return Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&body)));
        }
    }
    Err("That website has no icon xtermium can use.".into())
}

/// The icons a page links to, biggest first (an SVG counts as biggest). Mask icons are left
/// out: they're one-colour shapes meant to be tinted.
fn favicon_links(page: &str, base: &reqwest::Url) -> Vec<reqwest::Url> {
    static TAG: OnceLock<(regex::Regex, regex::Regex)> = OnceLock::new();
    let (tag, attr) = TAG.get_or_init(|| {
        (
            regex::Regex::new(r"(?i)<link\b[^>]*>").unwrap(),
            // After a space: `data-base-href=` isn't the link.
            regex::Regex::new(r#"(?i)\s(rel|href|sizes|type)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap(),
        )
    });
    let mut found: Vec<(u32, reqwest::Url)> = vec![];
    for t in tag.find_iter(page) {
        let mut a: HashMap<String, String> = HashMap::new();
        for c in attr.captures_iter(t.as_str()) {
            let v = c.get(2).or(c.get(3)).or(c.get(4)).map_or("", |m| m.as_str());
            a.insert(c[1].to_ascii_lowercase(), v.replace("&amp;", "&"));
        }
        let rel = a.get("rel").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
        let rels: Vec<&str> = rel.split_whitespace().collect();
        let touch = rels.iter().any(|r| r.starts_with("apple-touch-icon"));
        if !(rels.contains(&"icon") || touch) {
            continue;
        }
        let Some(link) = a.get("href").and_then(|h| base.join(h.trim()).ok()) else { continue };
        let svg = a.get("type").is_some_and(|t| t.contains("svg")) || link.path().ends_with(".svg");
        let sizes = a.get("sizes").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let size = if svg || sizes == "any" {
            u32::MAX
        } else {
            let listed = sizes.split_whitespace().filter_map(|s| s.split('x').next()?.parse().ok()).max();
            listed.unwrap_or(if touch { 180 } else { 16 })
        };
        found.push((size, link));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0)); // stable: the page's order among equals
    found.into_iter().map(|f| f.1).collect()
}

/// Downloads a text file (colour schemes and their catalogue) of at most 2 MB.
#[tauri::command]
async fn fetch_text(url: String) -> Res<String> {
    String::from_utf8(download(&url, 2 * 1024 * 1024).await?).map_err(|_| "That link isn't a text file.".into())
}

/// Downloads `url`, refusing more than `max` bytes.
async fn download(url: &str, max: usize) -> Res<Vec<u8>> {
    fetch(url, max, false).await.map(|r| r.1)
}

/// Downloads `url` (at most `max` bytes) and says where it ended up after redirects.
/// `insecure`: any certificate will do.
async fn fetch(url: &str, max: usize, insecure: bool) -> Res<(reqwest::Url, Vec<u8>)> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Use an http(s) link.".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("xtermium/", env!("CARGO_PKG_VERSION"))) // GitHub's API wants one
        .tls_danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp =
        client.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| format!("download failed: {e}"))?;
    let at = resp.url().clone();
    let mut body = vec![];
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("download failed: {e}"))? {
        body.extend_from_slice(&chunk);
        if body.len() > max {
            return Err(format!("The file is larger than {} KB.", max / 1024));
        }
    }
    Ok((at, body))
}

/// An image's type from its first bytes (servers often label files wrongly).
fn icon_type(b: &[u8]) -> Option<&'static str> {
    Some(match b {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [0, 0, 1, 0, ..] => "image/x-icon",
        _ if String::from_utf8_lossy(&b[..b.len().min(1024)]).contains("<svg") => "image/svg+xml",
        _ => return None,
    })
}

/// Suggestions for a path field: entries starting like `typed` (`~` is the home folder), folders
/// ending in a separator so the next level can be typed. `exec`: a program, also looked up on
/// PATH (with nothing typed, the shells found there).
#[tauri::command]
async fn complete_path(typed: String, dirs: bool, exec: bool) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || paths::complete(&typed, dirs, exec)).await.unwrap_or_default()
}

/// The system's file or folder picker; None if cancelled.
#[tauri::command]
async fn pick_path(handle: AppHandle, dirs: bool, start: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let mut d = handle.dialog().file();
    if let Some(dir) = paths::start_dir(&start) {
        d = d.set_directory(dir);
    }
    let picked = if dirs { d.blocking_pick_folder() } else { d.blocking_pick_file() };
    picked.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string())
}

/// Where terminal logs go (Settings > Terminal > Logging).
#[tauri::command]
fn logs_folder() -> String {
    term::logs_dir().display().to_string()
}

#[tauri::command]
fn open_logs(app: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = term::logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) links open in the browser".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Opens THIRD-PARTY-NOTICES.txt, which every build carries next to the app (tauri.conf.json
/// `bundle.resources`), in the system's viewer for text (Settings > About).
#[tauri::command]
fn open_notices(app: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let file = dunce::simplified(&dir).join("THIRD-PARTY-NOTICES.txt");
    if !file.is_file() {
        return Err("The third-party notices are missing: reinstall xtermium to get them back.".into());
    }
    app.opener().open_path(file.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

/// Opens terminal tab `id`'s folder in VS Code (`term::code_args`): `cwd` is the shell's, empty
/// when the shell doesn't say. Then an SSH session's is its home folder on the host, asked over SFTP.
#[tauri::command]
async fn open_in_code(app: State<'_, App>, id: u32, cwd: String) -> Res<()> {
    let (kind, home) = match app.tabs.lock().unwrap().get(&id) {
        Some(Tab::Term(t)) => {
            let home = (t.is_ssh() && !cwd.starts_with('/')).then(|| {
                let (reply, rx) = tokio::sync::oneshot::channel();
                let _ = t.tx().send(ssh::Ctl::Sftp(reply));
                rx
            });
            (t.session.kind.clone(), home)
        }
        _ => return Err("no such terminal".into()),
    };
    let cwd = match home {
        Some(rx) => {
            let sftp = rx.await.map_err(|_| "The session isn't connected.".to_string())?.map_err(|e| format!("SFTP: {e:#}"))?;
            sftp.canonicalize(".").await.map_err(|e| format!("SFTP: {e}"))?
        }
        None => cwd,
    };
    let args = term::code_args(&kind, &cwd, &app.settings.lock().unwrap().ssh_user)?;
    tauri::async_runtime::spawn_blocking(move || {
        use std::process::Stdio;
        // VS Code's command is a batch file on Windows, which Rust only finds by its full name.
        let mut cmd = std::process::Command::new(if cfg!(windows) { "code.cmd" } else { "code" });
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        term::no_window(&mut cmd);
        match cmd.status() {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("VS Code's code command failed ({s}).")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err("VS Code wasn't found: install it, and check that its code command is on your PATH.".into())
            }
            Err(e) => Err(format!("VS Code: {e}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether "Open in xtermium" is on folders' menus in File Explorer; None where there's no such
/// menu (not Windows).
#[tauri::command]
fn explorer_menu() -> Option<bool> {
    explorer::state()
}

/// Adds "Open in xtermium" to File Explorer's folder menus, named `label`, or takes it away.
#[tauri::command]
fn set_explorer_menu(on: bool, label: String) -> Res<()> {
    explorer::set(on, &label)
}

#[tauri::command]
fn open_term(app: State<App>, mut session: Session, out: Channel) -> u32 {
    app.store.lock().unwrap().resolve(&mut session.kind);
    if let Kind::Shell { program, .. } = &mut session.kind
        && program.is_empty()
    {
        *program = app.settings.lock().unwrap().shell.clone();
    }
    let look = app.look(&session);
    let tab = term::Tab::open(session, look, app.prompter.clone(), out);
    let id = app.next_id.fetch_add(1, Relaxed);
    app.tabs.lock().unwrap().insert(id, Tab::Term(Box::new(tab)));
    id
}

/// A remote desktop tab: RDP or VNC.
#[tauri::command]
fn open_rdp(app: State<App>, mut session: Session, out: Channel, width: u16, height: u16) -> Res<u32> {
    let (password, gateway, sftp) = {
        let store = app.store.lock().unwrap();
        // A VNC tab's file panel: SFTP to the same host, as its shared login's user (with its key
        // and password, then the usual ways).
        let sftp = match &session.kind {
            Kind::Vnc { host, credential, .. } => {
                let mut ep = config::Endpoint { host: host.clone(), port: 22, credential: credential.clone(), ..Default::default() };
                store.resolve_endpoint(&mut ep);
                Some(ep)
            }
            _ => None,
        };
        (store.resolve(&mut session.kind), store.gateway(&session.kind), sftp)
    };
    let quality = match session.kind {
        Kind::Rdp { quality: Some(q), .. } => q,
        Kind::Rdp { quality: None, .. } => app.settings.lock().unwrap().rdp_quality,
        Kind::Vnc { .. } => {
            let mut tab = vnc::VncTab::open(session, app.prompter.clone(), out, password, gateway.clone());
            tab.sftp = sftp.map(|ep| (ep, gateway));
            let id = app.next_id.fetch_add(1, Relaxed);
            app.tabs.lock().unwrap().insert(id, Tab::Vnc(Box::new(tab)));
            return Ok(id);
        }
        _ => return Err("not a remote desktop session".into()),
    };
    // RDP wants an even width; clamp to what servers accept.
    let size = [width.clamp(640, 8192) & !1, height.clamp(480, 8192)];
    let tab = rdp::RdpTab::open(session, app.prompter.clone(), out, size, quality, password, gateway);
    let id = app.next_id.fetch_add(1, Relaxed);
    app.tabs.lock().unwrap().insert(id, Tab::Rdp(Box::new(tab)));
    Ok(id)
}

/// Saves a terminal's scrollback and screen as text; the path, or None if cancelled.
#[tauri::command]
async fn save_output(handle: AppHandle, id: u32) -> Res<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (name, text) = match handle.state::<App>().tabs.lock().unwrap().get(&id) {
        Some(Tab::Term(t)) => (t.session.name.clone(), t.text(usize::MAX / 2)),
        _ => return Err("no such terminal".into()),
    };
    let file = clean::file_name(&format!("{} {name}.txt", chrono::Local::now().format("%Y-%m-%d %H-%M-%S")));
    let dialog = handle.dialog().file().set_file_name(file).add_filter("txt", &["txt"]);
    let Some(path) = dialog.blocking_save_file().and_then(|p| p.into_path().ok()) else { return Ok(None) };
    std::fs::write(&path, text + "\n").map_err(|e| e.to_string())?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
fn close_tab(app: State<App>, handle: AppHandle, id: u32) {
    let tab = app.tabs.lock().unwrap().remove(&id);
    // A detached tab's window goes with it.
    if let Some(w) = handle.get_webview_window(&detached_label(id)) {
        let _ = w.destroy();
    }
    // Shuts the backend down, off the UI thread: a stuck session must not freeze the window.
    std::thread::spawn(move || drop(tab));
}

/// The window a terminal tab was detached to.
fn detached_label(id: u32) -> String {
    format!("tab-{id}")
}

/// Moves tab `id` (a terminal or RDP) to a window of its own, `width` × `height`. `state` is what the
/// window's page starts from (ui/lib/Detached.svelte). Closing the window puts the tab back.
/// Async: making a window from a synchronous command deadlocks on Windows.
#[tauri::command]
async fn detach_tab(handle: AppHandle, id: u32, title: String, state: serde_json::Value, width: f64, height: f64) -> Res<()> {
    let w = tauri::WebviewWindowBuilder::new(&handle, detached_label(id), tauri::WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(width.max(320.0), height.max(200.0))
        .min_inner_size(320.0, 200.0)
        .initialization_script(format!("window.__DETACHED__ = {state};"))
        .data_directory(config::webview_dir())
        .build()
        .map_err(|e| e.to_string())?;
    let h = handle.clone();
    w.on_window_event(move |e| {
        // With the main window gone (the app is quitting), it just closes.
        if let tauri::WindowEvent::CloseRequested { api, .. } = e
            && h.get_webview_window("main").is_some()
        {
            api.prevent_close();
            let _ = h.emit_to("main", "reattach", serde_json::json!({ "id": id }));
        }
    });
    Ok(())
}

/// A window shows tab `id` (a terminal or RDP) from now on, on `out`; false if the tab is gone.
/// Back in the main window, the window it was detached to closes.
#[tauri::command]
fn tab_attach(app: State<App>, window: tauri::Window, id: u32, out: Channel) -> bool {
    match app.tabs.lock().unwrap().get(&id) {
        Some(Tab::Term(t)) => t.attach(out),
        Some(Tab::Rdp(t)) => t.attach(out),
        _ => return false,
    }
    if window.label() == "main"
        && let Some(w) = window.app_handle().get_webview_window(&detached_label(id))
    {
        let _ = w.destroy();
    }
    true
}

/// Types into tab `src` and, with broadcast on, into every other ticked tab (broadcast.rs).
fn type_into(app: &App, handle: &AppHandle, tabs: &HashMap<u32, Tab>, src: u32, evs: &[term::Typed]) {
    let Some(Tab::Term(t)) = tabs.get(&src) else { return };
    t.apply(evs);
    broadcast::share(app, handle, tabs, src, broadcast::Typing::Term(evs));
}

// Keys and clicks may read the clipboard (paste), and async commands run off the main thread: a
// sync one runs on it, and on X11 the read waits for the clipboard's owner to answer. When that
// owner is xtermium's own page (text copied from Settings), the answer comes from the main
// thread, which is busy waiting: a frozen window, then an empty paste once arboard gives up.
#[tauri::command]
async fn term_key(app: State<'_, App>, handle: AppHandle, id: u32, key: term::Key) -> Res<()> {
    let mut tabs = app.tabs.lock().unwrap();
    let Some(Tab::Term(t)) = tabs.get_mut(&id) else { return Ok(()) };
    if let Some(ev) = t.key(&key) {
        type_into(&app, &handle, &tabs, id, &[ev]);
    }
    Ok(())
}

#[tauri::command]
fn term_text(app: State<App>, handle: AppHandle, id: u32, text: String) {
    use wezterm_term::{KeyCode, KeyModifiers};
    // Line breaks (snippets) press Enter, encoded for the terminal's mode like a typed one.
    let evs: Vec<_> = text
        .replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\n' | '\r' => term::Typed::Key(KeyCode::Enter, KeyModifiers::NONE),
            c => term::Typed::Key(KeyCode::Char(c), KeyModifiers::NONE),
        })
        .collect();
    type_into(&app, &handle, &app.tabs.lock().unwrap(), id, &evs);
}

/// A pointer event; returns a link's address on Ctrl+click, for the UI to open.
#[tauri::command]
async fn term_mouse(app: State<'_, App>, id: u32, ev: term::Mouse) -> Res<Option<String>> {
    Ok(match app.tabs.lock().unwrap().get(&id) {
        Some(Tab::Term(t)) => t.mouse(&ev),
        _ => None,
    })
}

/// `cw`/`ch`: cell size in device pixels.
#[tauri::command]
fn term_resize(app: State<App>, id: u32, cols: usize, rows: usize, cw: f32, ch: f32) {
    if let Some(Tab::Term(t)) = app.tabs.lock().unwrap().get(&id) {
        t.resize(cols, rows, (cw, ch));
    }
}

/// The scroll bar: shows the screen `back` rows up from the bottom of a terminal's output.
#[tauri::command]
fn term_scroll(app: State<App>, id: u32, back: usize) {
    if let Some(Tab::Term(t)) = app.tabs.lock().unwrap().get(&id) {
        t.scroll_to(back);
    }
}


/// The status bar's facts about a tab (terminal tabs; None for others, the UI knows enough).
#[tauri::command]
fn tab_info(app: State<App>, id: u32) -> Option<term::Info> {
    match app.tabs.lock().unwrap().get(&id) {
        Some(Tab::Term(t)) => Some(t.info()),
        _ => None,
    }
}

/// The find bar: (matches, current one 1-based). See term::Tab::find.
#[tauri::command]
fn term_find(app: State<App>, id: u32, query: String, case: bool, step: i32) -> (usize, usize) {
    match app.tabs.lock().unwrap().get(&id) {
        Some(Tab::Term(t)) => t.find(&query, case, step),
        _ => (0, 0),
    }
}

#[tauri::command]
fn set_broadcast(app: State<App>, on: bool) {
    app.broadcast.store(on, Relaxed);
}

#[tauri::command]
fn set_tab_broadcast(app: State<App>, id: u32, on: bool) {
    let mut off = app.broadcast_off.lock().unwrap();
    if on { off.remove(&id) } else { off.insert(id) };
}

#[tauri::command]
fn rdp_input(app: State<App>, handle: AppHandle, id: u32, ev: rdp::Input) {
    let tabs = app.tabs.lock().unwrap();
    broadcast::share(&app, &handle, &tabs, id, broadcast::Typing::Desktop(&ev));
    match tabs.get(&id) {
        Some(Tab::Rdp(t)) => t.input(ev),
        Some(Tab::Vnc(t)) => t.input(ev),
        _ => {}
    }
}

/// Opens a tab's file panel and publishes its state. SSH tabs get SFTP on first use; an RDP
/// tab's panel needs `smb_connect` first. Returns whether the panel has something to show.
#[tauri::command]
fn files_open(app: State<App>, handle: AppHandle, id: u32) -> Res<bool> {
    let mut tabs = app.tabs.lock().unwrap();
    let b = match tabs.get_mut(&id) {
        Some(Tab::Term(t)) if t.is_ssh() => {
            if t.files.is_none() || !t.alive() {
                t.files = Some(files::Browser::sftp(t.tx(), handle, id));
            }
            t.files.as_ref()
        }
        Some(Tab::Rdp(t)) => t.files.as_ref(),
        Some(Tab::Vnc(t)) => {
            if t.files.as_ref().is_none_or(|b| b.failed())
                && let Some((ep, jump)) = t.sftp.clone()
            {
                t.files = Some(files::Browser::sftp_alone(ep, jump, app.prompter.clone(), handle, id));
            }
            t.files.as_ref()
        }
        Some(Tab::Term(_)) => return Err("Files need an SSH, RDP or VNC tab.".into()),
        None => return Err("no such tab".into()),
    };
    if let Some(b) = b {
        b.publish();
    }
    Ok(b.is_some())
}

/// Connects an RDP tab's file panel to the host's shares, as `user` or (None) as the RDP login.
#[tauri::command]
fn smb_connect(app: State<App>, handle: AppHandle, id: u32, user: Option<String>, password: String) -> Res<()> {
    let mut tabs = app.tabs.lock().unwrap();
    let Some(Tab::Rdp(t)) = tabs.get_mut(&id) else { return Err("no such RDP tab".into()) };
    if matches!(t.session.kind, Kind::Rdp { no_shares: true, .. }) {
        return Err("Shared folders are off for this session.".into());
    }
    let (user, password) = match user {
        Some(u) => (u, password),
        None => t.login().ok_or("Log in to the remote desktop first, or use another account.")?,
    };
    let host = t.host();
    let via = t.gateway.clone().map(|gw| (gw, app.prompter.clone()));
    let smb = async move { files::Smb::connect(&host, &user, &password, via).await.map(|s| files::Fs::Smb(Box::new(s))) };
    t.files = Some(files::Browser::new(smb, handle, id));
    Ok(())
}

#[tauri::command]
fn files(app: State<App>, id: u32, cmd: files::Cmd) {
    let tabs = app.tabs.lock().unwrap();
    let b = match tabs.get(&id) {
        Some(Tab::Term(t)) => t.files.as_ref(),
        Some(Tab::Rdp(t)) => t.files.as_ref(),
        Some(Tab::Vnc(t)) => t.files.as_ref(),
        None => None,
    };
    if let Some(b) = b {
        b.cmd(cmd);
    }
}

#[tauri::command]
fn tunnel_start(app: State<App>, name: String) -> Res<()> {
    let store = app.store.lock().unwrap();
    let Some(mut t) = store.tunnels.iter().find(|t| t.name == name).cloned() else {
        return Err(format!("no tunnel named {name}"));
    };
    let jump = store.endpoint_gateway(&t.ssh);
    store.resolve_endpoint(&mut t.ssh);
    drop(store);
    let status = Arc::new(Mutex::new(String::new()));
    let traffic = Arc::new(ssh::Traffic::default());
    let done = Arc::new(AtomicBool::new(false));
    let (st, tr, p, d) = (status.clone(), traffic.clone(), app.prompter.clone(), done.clone());
    let task = tauri::async_runtime::spawn(async move {
        let st2 = st.clone();
        let r = ssh::tunnel(t, jump, p, tr, move |s| *st2.lock().unwrap() = s).await;
        if let Err(e) = r {
            *st.lock().unwrap() = format!("error: {e:#}");
        }
        d.store(true, Relaxed);
    });
    if let Some(old) = app.tunnels.lock().unwrap().insert(name, flow::Running::new(task, status, traffic, done)) {
        old.task.abort();
    }
    Ok(())
}

#[tauri::command]
fn tunnel_stop(app: State<App>, name: String) {
    if let Some(r) = app.tunnels.lock().unwrap().remove(&name) {
        r.task.abort();
    }
}

#[tauri::command]
fn tunnel_stats(app: State<App>) -> HashMap<String, flow::Stats> {
    app.tunnels.lock().unwrap().iter_mut().map(|(n, r)| (n.clone(), r.stats())).collect()
}

/// The UI opened a tab for an automation `open` request (`id` 0: it opened elsewhere).
#[tauri::command]
fn automation_opened(app: State<App>, req: u32, id: u32) {
    if let Some(reply) = app.opens.lock().unwrap().remove(&req) {
        let _ = reply.send(id);
    }
}

/// Whether the automation server runs, and why not if it should.
#[tauri::command]
fn automation_status(app: State<App>) -> (bool, String) {
    (app.automation.lock().unwrap().is_some(), app.automation_error.lock().unwrap().clone())
}

#[tauri::command]
fn prompt_reply(app: State<App>, id: u32, answer: Option<(String, bool)>) {
    if let Some(reply) = app.replies.lock().unwrap().remove(&id) {
        let _ = reply.send(answer);
    }
}

#[derive(Serialize, Clone)]
struct PromptMsg {
    id: u32,
    title: String,
    text: String,
    input: Option<bool>,
}

fn main() {
    // `xtermium <command>` drives the running app from scripts instead of starting another.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    // "Open in xtermium" on a folder in File Explorer (explorer.rs).
    let here = explorer::here(&args);
    // One xtermium per data folder: a second start shows the running one's window instead (with
    // the folder's shell).
    let Some(_instance) = instance::claim(here.as_deref()) else { return };
    // Started from a console or script that ignores Ctrl+C (background jobs, some launchers),
    // xtermium would pass that on to every shell it starts, and Ctrl+C would stop nothing in
    // them. Windows lets a process clear it for itself and its children.
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
        }
        unsafe { SetConsoleCtrlHandler(None, 0) };
    }
    // kitty/OSC 1337 images may be spilled to disk by wezterm-term.
    if let Some(temp) = config::temp()
        && let Ok(dir) = wezterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(temp)
    {
        let _ = wezterm_blob_leases::register_storage(Arc::new(dir));
    }
    // HTTPS (icons by URL) uses rustls with ring, the crypto russh is built on.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // One tokio runtime for Tauri and for our connections.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    tauri::async_runtime::set(rt.handle().clone());

    let (prompter, prompts) = Prompter::new();
    let (key, notice) = config::init_key();
    let app = App {
        store: Mutex::new(Store::default()),
        rest: Mutex::new(Store::default()),
        notice: Mutex::new(notice),
        locked: Mutex::new(match key {
            config::KeyState::Ready => None,
            config::KeyState::Locked { recoverable } => Some(recoverable),
        }),
        settings: Mutex::new(Settings::default()),
        dark: AtomicBool::new(true),
        applied: Mutex::new(None),
        tabs: Mutex::new(HashMap::new()),
        next_id: AtomicU32::new(1),
        broadcast: AtomicBool::new(false),
        broadcast_off: Mutex::default(),
        tunnels: Mutex::new(HashMap::new()),
        prompter,
        replies: Default::default(),
        opens: Default::default(),
        automation: Default::default(),
        automation_error: Default::default(),
        here: Mutex::new(here),
    };
    if app.locked.lock().unwrap().is_none() {
        app.load();
    }
    let replies = app.replies.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(app)
        .setup(move |a| {
            // The main window (tauri.conf.json), made here so its browser data is in the data
            // folder too: Tauri's default is the OS's local app data.
            tauri::WebviewWindowBuilder::from_config(a.handle(), &a.config().app.windows[0])?.data_directory(config::webview_dir()).build()?;
            apply_automation(&a.state::<App>(), a.handle());
            support::setup(a.handle());
            std::thread::spawn(config::sweep_temp);
            instance::listen(a.handle().clone());
            tray::setup(a)?;
            // The lock comes on after the minutes set without input (Settings › Security).
            let handle = a.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if lock::idle_check() {
                        let _ = handle.emit("lock", true);
                    }
                }
            });
            // Background tasks' questions become `prompt` events; the UI answers with prompt_reply.
            let handle = a.handle().clone();
            std::thread::spawn(move || {
                for (id, req) in (1..).zip(prompts) {
                    replies.lock().unwrap().insert(id, req.reply);
                    let _ = handle.emit("prompt", PromptMsg { id, title: req.title, text: req.text, input: req.input });
                    // Nobody waits for the answer any more (its tab closed, its tunnel stopped):
                    // the question leaves the screen.
                    let (replies, handle) = (replies.clone(), handle.clone());
                    tauri::async_runtime::spawn(async move {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            let mut r = replies.lock().unwrap();
                            match r.get(&id) {
                                None => return, // answered
                                Some(reply) if reply.is_closed() => {
                                    r.remove(&id);
                                    drop(r);
                                    let _ = handle.emit("prompt-gone", id);
                                    return;
                                }
                                Some(_) => {}
                            }
                        }
                    });
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot,
            explorer_menu,
            set_explorer_menu,
            save_store,
            workspace,
            saved_logins,
            set_login,
            detach_tab,
            tab_attach,
            sync_now,
            sync_delete,
            account_device_start,
            account_device_poll,
            account_info,
            account_portal,
            account_sign_out,
            logs_folder,
            open_logs,
            complete_path,
            pick_path,
            save_settings,
            save_tabs,
            regex_error,
            set_dark,
            fonts,
            serial_ports,
            import_sources,
            import_file,
            import_path,
            export_sessions,
            backup_save,
            tray::tray_tasks,
            backup_restore,
            restore_points,
            restore_point_take,
            restore_point_delete,
            restore_point_restore,
            move_to_workspace,
            unlock,
            start_over,
            set_recovery,
            ssh_keys,
            lock_info,
            lock_now,
            lock_unlock,
            lock_challenge,
            lock_set,
            lock_activity,
            ssh_key_create,
            ssh_key_delete,
            known_keys,
            forget_key,
            fetch_icon,
            fetch_favicon,
            automation_network_info,
            fetch_background,
            background_get,
            background_set,
            support::about,
            support::notify,
            support::update_check,
            support::troubleshoot_bundle,
            support::custom_css,
            support::pick_background,
            fetch_text,
            web::web_open,
            web::web_shortcuts,
            web::web_place,
            web::web_go,
            web::web_close,
            web::web_type,
            open_url,
            open_notices,
            open_in_code,
            open_term,
            open_rdp,
            close_tab,
            save_output,
            term_find,
            tab_info,
            term_key,
            term_text,
            term_mouse,
            term_resize,
            term_scroll,
            set_broadcast,
            set_tab_broadcast,
            rdp_input,
            rdp::desktop_keys,
            automation_opened,
            automation_status,
            files_open,
            smb_connect,
            files,
            tunnel_start,
            tunnel_stop,
            tunnel_stats,
            prompt_reply,
        ])
        .build(tauri::generate_context!())
        .expect("starting xtermium")
        .run(|h, e| {
            if let tauri::RunEvent::Exit = e {
                // Closing tabs tells every backend to shut down (remote shells, RDP);
                // give those background threads a moment before the process goes away.
                let tabs = std::mem::take(&mut *h.state::<App>().tabs.lock().unwrap());
                if !tabs.is_empty() {
                    drop(tabs);
                    std::thread::sleep(std::time::Duration::from_millis(600));
                }
            }
        });
    drop(rt);
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_types_from_content() {
        let t = super::icon_type;
        assert_eq!(t(b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(t(b"GIF89a"), Some("image/gif"));
        assert_eq!(t(b"RIFF\0\0\0\0WEBPVP8"), Some("image/webp"));
        assert_eq!(t(b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"), Some("image/svg+xml"));
        assert_eq!(t(b"<html>not an image</html>"), None);
    }

    /// A backup brings everything back but the device's sign-in; an import of sessions reads it.
    #[test]
    fn backups_round_trip() {
        use super::*;
        let mut store = Store::default();
        store.sessions.push(Session::new("db1", "Prod", Kind::Shell { program: String::new(), dir: String::new() }));
        store.credentials.push(config::Credential { id: "c1".into(), password: "s3cret".into(), ..Default::default() });
        let mut settings = Settings { font_size: 17.0, sync_token: "tok".into(), automation_token: "auto".into(), ..Default::default() };
        settings.shortcuts.insert("palette".into(), "F1".into());
        let mut known = config::Known::default();
        known.ssh.insert("db1:22".into(), "ssh-ed25519 AAAA".into());
        let text = backup(&store, &settings, &known).unwrap();
        assert!(!text.contains("\"tok\"") && !text.contains("\"auto\""), "{text}");
        let (s, set, k) = unbackup(&text).unwrap();
        assert_eq!((s.sessions[0].name.as_str(), s.credentials[0].password.as_str()), ("db1", "s3cret"));
        let set = set.unwrap();
        assert_eq!((set.font_size, set.shortcuts["palette"].as_str()), (17.0, "F1"));
        assert_eq!(k.unwrap().ssh["db1:22"], "ssh-ed25519 AAAA");
        let sessions_only: Store = toml::from_str(&text).unwrap();
        assert_eq!(sessions_only.sessions.len(), 1);
        assert!(unbackup(&toml::to_string(&store).unwrap()).unwrap().1.is_none());
    }

    /// A clone's name says what it's of, counts up when taken, and fits 24 characters.
    #[test]
    fn clone_names() {
        let taken = |n: &str| ["Lab (clone)", "Lab (clone 2)"].contains(&n);
        assert_eq!(super::clone_name("Work", taken), "Work (clone)");
        assert_eq!(super::clone_name("Lab", taken), "Lab (clone 3)");
        let long = super::clone_name("A very long workspace name", taken);
        assert_eq!((long.as_str(), long.chars().count()), ("A very long work (clone)", 24));
    }

    /// A page's icon links: relative ones resolved, biggest first, mask icons skipped.
    #[test]
    fn finds_favicons() {
        let base = reqwest::Url::parse("https://pve.lan:8006/ui/").unwrap();
        let page = r#"<head><link rel="stylesheet" href="a.css">
            <link rel="shortcut icon" href="/pve2/images/favicon.ico">
            <LINK REL=icon sizes="32x32 64x64" href='icon64.png?v=1&amp;x=2' data-base-href="/no">
            <link rel="icon" type="image/svg+xml" href="/logo.svg" data-base-href="/logo">
            <link rel="mask-icon" href="/mask.svg">
            <link href="//cdn.example/touch.png" rel="apple-touch-icon"></head>"#;
        let got: Vec<String> = super::favicon_links(page, &base).into_iter().map(String::from).collect();
        assert_eq!(
            got,
            [
                "https://pve.lan:8006/logo.svg",
                "https://cdn.example/touch.png",
                "https://pve.lan:8006/ui/icon64.png?v=1&x=2",
                "https://pve.lan:8006/pve2/images/favicon.ico"
            ]
        );
    }
}
