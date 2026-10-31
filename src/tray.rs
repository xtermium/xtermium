//! The tray icon (Settings > General > Keep running): with it on, closing the window hides it and
//! the sessions go on running. Its menu opens the window, lists the open tabs (background ones
//! marked) to bring one up, and exits, which closes every session first (main.rs `RunEvent::Exit`).

use crate::App;
use serde::Deserialize;
use tauri::image::Image;
use tauri::menu::{IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent, Wry};

const ID: &str = "main";

/// An open tab as the UI sends it: its key there, name, whether it runs in the background, and
/// its session's icon as a PNG `data:` address.
#[derive(Deserialize)]
pub struct Task {
    key: u32,
    name: String,
    background: bool,
    icon: String,
}

/// The menu's words, in the interface's language.
#[derive(Deserialize)]
pub struct Labels {
    open: String,
    tasks: String,
    none: String,
    background: String,
    exit: String,
}

impl Default for Labels {
    fn default() -> Self {
        let s = |t: &str| t.to_string();
        Self { open: s("Open"), tasks: s("Tasks"), none: s("No tasks"), background: s("background"), exit: s("Exit") }
    }
}

/// Menus take `&` as a mnemonic mark: a name's own stays a character.
fn plain(s: &str) -> String {
    s.replace('&', "&&")
}

fn png(data_url: &str) -> Option<Image<'static>> {
    use base64::Engine;
    let b64 = data_url.strip_prefix("data:image/png;base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(Image::new_owned(img.into_raw(), w, h))
}

fn menu(h: &AppHandle, tasks: &[Task], l: &Labels) -> tauri::Result<Menu<Wry>> {
    let list = Submenu::with_id(h, "tasks", plain(&l.tasks), true)?;
    if tasks.is_empty() {
        list.append(&MenuItem::with_id(h, "none", plain(&l.none), false, None::<&str>)?)?;
    }
    for t in tasks {
        let name = if t.background { format!("{} ({})", t.name, l.background) } else { t.name.clone() };
        list.append(&IconMenuItem::with_id(h, format!("task:{}", t.key), plain(&name), true, png(&t.icon), None::<&str>)?)?;
    }
    let open = MenuItem::with_id(h, "open", plain(&l.open), true, None::<&str>)?;
    let exit = MenuItem::with_id(h, "exit", plain(&l.exit), true, None::<&str>)?;
    Menu::with_items(h, &[&open, &list, &PredefinedMenuItem::separator(h)?, &exit])
}

/// Brings the window back and gives it the keyboard.
fn open(h: &AppHandle) {
    if let Some(w) = h.get_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Makes the tray icon, and has closing the window hide it while Keep running is on.
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let h = app.handle();
    let mut tray = TrayIconBuilder::with_id(ID)
        .tooltip("xtermium")
        .menu(&menu(h, &[], &Labels::default())?)
        .show_menu_on_left_click(false)
        .on_menu_event(|h, e| match e.id().as_ref() {
            "open" => open(h),
            "exit" => h.exit(0),
            id => {
                if let Some(key) = id.strip_prefix("task:").and_then(|k| k.parse::<u32>().ok()) {
                    open(h);
                    let _ = h.emit_to("main", "tray-show", key);
                }
            }
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                open(t.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    if let Some(w) = app.get_window("main") {
        let h = h.clone();
        w.on_window_event(move |e| {
            if let WindowEvent::CloseRequested { api, .. } = e
                && h.state::<App>().settings.lock().unwrap().keep_running
            {
                api.prevent_close();
                if let Some(w) = h.get_window("main") {
                    let _ = w.hide();
                }
            }
            // Closed for good: detached tabs' windows don't keep xtermium running.
            if let WindowEvent::Destroyed = e {
                h.exit(0);
            }
        });
    }
    apply(h, h.state::<App>().settings.lock().unwrap().keep_running);
    Ok(())
}

/// The icon shows while Keep running is on.
pub fn apply(h: &AppHandle, keep_running: bool) {
    if let Some(t) = h.tray_by_id(ID) {
        let _ = t.set_visible(keep_running);
    }
}

/// The open tabs changed (or the language): the menu follows.
#[tauri::command]
pub fn tray_tasks(h: AppHandle, tasks: Vec<Task>, labels: Labels) -> Result<(), String> {
    let Some(t) = h.tray_by_id(ID) else { return Ok(()) };
    t.set_menu(Some(menu(&h, &tasks, &labels).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())
}
