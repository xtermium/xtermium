//! Around the app itself: desktop notifications, start at login, the process priority, the build
//! and the look for a newer release (Settings › About), the troubleshooting bundle (Settings ›
//! Troubleshoot) and custom.css.

use crate::{App, Res, config};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The build number the release workflow stamps in (XTERMIUM_BUILD); 0 in a local build.
pub fn build() -> u32 {
    option_env!("XTERMIUM_BUILD").and_then(|b| b.parse().ok()).unwrap_or(0)
}

/// "debug" (built locally), "beta" (a pre-release version like 1.2.0-beta.1) or "release".
pub fn kind() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else if env!("CARGO_PKG_VERSION").contains('-') {
        "beta"
    } else {
        "release"
    }
}

#[derive(Serialize)]
pub struct About {
    version: &'static str,
    build: u32,
    kind: &'static str,
}

#[tauri::command]
pub fn about() -> About {
    About { version: env!("CARGO_PKG_VERSION"), build: build(), kind: kind() }
}

/// A desktop notification (the UI decides when one is due: Settings › General).
#[tauri::command]
pub fn notify(handle: AppHandle, title: String, body: String) -> Res<()> {
    use tauri_plugin_notification::NotificationExt;
    handle.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
}

/// Starts xtermium at sign-in, or stops that (a registry Run entry, a .desktop file, a launch agent).
pub fn autostart(handle: &AppHandle, on: bool) -> Res<()> {
    use tauri_plugin_autostart::ManagerExt;
    let auto = handle.autolaunch();
    if on {
        auto.enable()
    } else if auto.is_enabled().unwrap_or(false) {
        auto.disable()
    } else {
        Ok(())
    }
    .map_err(|e| e.to_string())
}

/// Runs the process above the normal priority: ABOVE_NORMAL on Windows, nice -5 elsewhere,
/// which only root may set (the error says so).
pub fn priority() -> Res<()> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{ABOVE_NORMAL_PRIORITY_CLASS, GetCurrentProcess, SetPriorityClass};
        // SAFETY: our own process handle; a plain system call.
        unsafe { SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS) }.map_err(|e| format!("Process priority: {e}"))
    }
    #[cfg(unix)]
    {
        // SAFETY: setpriority on this process; a plain system call.
        if unsafe { libc::setpriority(libc::PRIO_PROCESS as _, 0, -5) } == 0 {
            Ok(())
        } else {
            Err(format!("Process priority: {} (start xtermium as root, or allow it with ulimit -e).", std::io::Error::last_os_error()))
        }
    }
}

/// At start: start-at-login as set, and a quiet look for a newer release (not in local builds),
/// told to the UI as an `update` event.
pub fn setup(handle: &AppHandle) {
    let s = handle.state::<App>().settings.lock().unwrap().clone();
    if s.start_at_login {
        let _ = autostart(handle, true);
    }
    if s.update_notify && kind() != "debug" {
        let h = handle.clone();
        tauri::async_runtime::spawn(async move {
            // The UI is up and listening by then.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if let Ok(u) = update_check().await
                && u.newer
            {
                let _ = h.emit_to("main", "update", &u);
            }
        });
    }
}

/// The latest release on GitHub, with the download for this computer's system.
#[derive(Serialize, Clone)]
pub struct Update {
    version: String,
    /// A later version than this one, by semver's order.
    newer: bool,
    /// The release's page.
    page: String,
    /// The installer or package for this system, when the release has one.
    download: Option<String>,
}

const RELEASES: &str = "https://github.com/xtermium/xtermium/releases/latest";

#[tauri::command]
pub async fn update_check() -> Res<Update> {
    let (_, body) = crate::fetch("https://api.github.com/repos/xtermium/xtermium/releases/latest", 1 << 20, false).await?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    // Tags read v<version> (.github/workflows/release.yml).
    let version = v["tag_name"].as_str().unwrap_or_default().trim_start_matches('v');
    let newer = semver(version) > semver(env!("CARGO_PKG_VERSION"));
    let want: &[&str] = match std::env::consts::OS {
        "windows" => &["-setup.exe", ".msi"],
        "macos" => &[".dmg"],
        _ => linux_packages(),
    };
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let download = want.iter().find_map(|ext| {
        assets
            .iter()
            .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(ext)))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(String::from)
    });
    Ok(Update { version: version.into(), newer, page: v["html_url"].as_str().unwrap_or(RELEASES).into(), download })
}

/// The numbers, whether it's a release (not a pre-release), and the pre-release's parts.
type Version = ((u64, u64, u64), bool, Vec<(u64, String)>);

/// A version in semver's order: the numbers, then a release above its pre-releases
/// ("1.2.0-beta.2" < "1.2.0"), pre-releases by their dot-separated parts (numbers as numbers,
/// and below words). Build metadata ("+abc") doesn't count.
fn semver(v: &str) -> Version {
    let v = v.split('+').next().unwrap_or_default();
    let (core, pre) = v.split_once('-').unwrap_or((v, ""));
    let mut n = core.split('.').map(|x| x.parse().unwrap_or(0));
    let parts = pre.split('.').filter(|p| !p.is_empty()).map(|p| p.parse().map_or((u64::MAX, p.to_string()), |n| (n, String::new())));
    ((n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0)), pre.is_empty(), parts.collect())
}

/// The package formats this Linux takes, best first (/etc/os-release says which family).
fn linux_packages() -> &'static [&'static str] {
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default().to_lowercase();
    if os.contains("debian") || os.contains("ubuntu") {
        &[".deb", ".AppImage"]
    } else if os.contains("rhel") || os.contains("fedora") || os.contains("suse") {
        &[".rpm", ".AppImage"]
    } else {
        &[".AppImage", ".deb", ".rpm"]
    }
}

/// Writes a folder of plain-text facts about this installation for support@xtermium.com (no
/// passwords, tokens, names or addresses: settings, counts and the environment) and opens it.
/// Returns the folder.
#[tauri::command]
pub fn troubleshoot_bundle(handle: AppHandle) -> Res<String> {
    use tauri_plugin_opener::OpenerExt;
    let app = handle.state::<App>();
    let dir = config::dir().join("troubleshoot").join(chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str, text: String| std::fs::write(dir.join(name), text).map_err(|e| format!("{name}: {e}"));

    let mut about = format!(
        "xtermium {} build {} ({})\nsystem: {} {}\ntauri: {}\nwebview: {}\ndata folder: {}\nprogram: {}\n",
        env!("CARGO_PKG_VERSION"),
        build(),
        kind(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        tauri::VERSION,
        tauri::webview_version().unwrap_or_default(),
        config::dir().display(),
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
    );
    if let Some(w) = handle.get_webview_window("main") {
        if let Ok(s) = w.scale_factor() {
            about += &format!("display scale: {s}\n");
        }
        if let Ok(s) = w.inner_size() {
            about += &format!("window: {} x {}\n", s.width, s.height);
        }
    }
    for k in [
        "XTERMIUM_HOME",
        "WEBVIEW2_USER_DATA_FOLDER",
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "TERM",
        "SHELL",
        "LANG",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "GDK_BACKEND",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
    ] {
        if let Ok(v) = std::env::var(k) {
            about += &format!("{k}={v}\n");
        }
    }
    let (on, error) = (app.automation.lock().unwrap().is_some(), app.automation_error.lock().unwrap().clone());
    about += &format!("automation: {}{}\n", if on { "listening" } else { "off" }, if error.is_empty() { String::new() } else { format!(" ({error})") });
    about += &format!("notice: {}\n", app.notice.lock().unwrap());
    write("about.txt", about)?;

    // The settings without anything secret or personal.
    let mut s = app.settings.lock().unwrap().clone();
    for secret in [&mut s.automation_token, &mut s.sync_token, &mut s.sync_email, &mut s.ssh_password, &mut s.ssh_user, &mut s.ssh_key, &mut s.shell] {
        if !secret.is_empty() {
            *secret = "(set)".into();
        }
    }
    s.custom_icons.clear();
    s.open_folders.clear();
    for b in [&mut s.background, &mut s.sidebar_background, &mut s.bar_background] {
        b.name = if b.name.is_empty() { String::new() } else { "(set)".into() };
    }
    write("settings.json", serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?)?;

    // What's stored, as counts by kind: no names or addresses.
    let mut kinds = std::collections::BTreeMap::new();
    let (store, rest) = (app.store.lock().unwrap(), app.rest.lock().unwrap());
    for session in store.sessions.iter().chain(rest.workspaces.iter().flat_map(|w| w.data.sessions.iter())) {
        let kind = serde_json::to_value(&session.kind).ok().and_then(|v| v["type"].as_str().map(String::from)).unwrap_or_default();
        *kinds.entry(kind).or_insert(0u32) += 1;
    }
    let mut data = format!(
        "workspaces: {}\nfolders: {}\nlogins: {}\ntunnels: {}\nsnippets: {}\nlayouts: {}\n",
        1 + rest.workspaces.len(),
        store.folders.len(),
        store.credentials.len(),
        store.tunnels.len(),
        store.snippets.len(),
        store.layouts.len()
    );
    for (k, n) in &kinds {
        data += &format!("sessions ({k}): {n}\n");
    }
    drop((store, rest));
    for (id, tab) in app.tabs.lock().unwrap().iter() {
        data += &match tab {
            crate::Tab::Term(t) => format!(
                "tab {id}: {} {}\n",
                serde_json::to_value(&t.session.kind).ok().and_then(|v| v["type"].as_str().map(String::from)).unwrap_or_default(),
                if t.alive() { "running" } else { "stopped" }
            ),
            crate::Tab::Rdp(_) => format!("tab {id}: rdp\n"),
            crate::Tab::Vnc(_) => format!("tab {id}: vnc\n"),
        };
    }
    write("data.txt", data)?;

    let _ = handle.opener().open_path(dir.display().to_string(), None::<&str>);
    Ok(dir.display().to_string())
}

/// `custom.css` in the data folder, and its hex SHA-256: the UI applies it at start, asking
/// first when it isn't the one kept in Settings (`custom_css`). None when there's no such file.
#[tauri::command]
pub fn custom_css() -> Option<(String, String)> {
    use sha2::{Digest, Sha256};
    let path = config::dir().join("custom.css");
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() || text.len() > 1 << 20 {
        return None;
    }
    let hash = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    Some((text, hash))
}

/// A picture or shader file from the system's own file dialog (the page's own picker hides
/// shader files on some desktops): its kind, name and data as `fetch_background` gives them, or
/// None when the dialog was cancelled.
#[tauri::command]
pub async fn pick_background(handle: AppHandle) -> Res<Option<(String, String, String)>> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = handle
        .dialog()
        .file()
        .add_filter("Pictures and shaders", &["png", "jpg", "jpeg", "gif", "webp", "svg", "glsl", "frag", "fs", "fsh", "shader", "txt"]);
    let Some(path) = dialog.blocking_pick_file().and_then(|p| p.into_path().ok()) else { return Ok(None) };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let body = std::fs::read(&path).map_err(|e| e.to_string())?;
    if body.len() > crate::BACKGROUND_MAX {
        return Err(format!("Pick a file under {} MB.", crate::BACKGROUND_MAX as f64 / (1 << 20) as f64));
    }
    let (kind, data) = crate::background_data(body)?;
    Ok(Some((kind, name, data)))
}

#[cfg(test)]
mod tests {
    use super::semver;

    /// Versions compare as semver orders them, numbers as numbers.
    #[test]
    fn releases_compare_by_semver() {
        let order = ["0.9.9", "0.10.0", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1"];
        for w in order.windows(2) {
            assert!(semver(w[0]) < semver(w[1]), "{} < {}", w[0], w[1]);
        }
        assert_eq!(semver("1.0.0+abc"), semver("1.0.0"), "build metadata doesn't count");
        assert!(semver("0.1.0") <= semver("0.1.0"), "the same version isn't newer");
    }
}
