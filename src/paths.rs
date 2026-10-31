//! Suggestions for path fields (SSH keys, programs, folders): what's on disk that starts like
//! what was typed, so the UI can offer it in a dropdown.

use std::path::{Path, PathBuf};

const MAX: usize = 100;

fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

/// `~` and `~/…` as the home folder.
fn expand(typed: &str) -> PathBuf {
    match (typed.strip_prefix('~'), home()) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            h.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(typed),
    }
}

/// The folder a picker opens in for `typed`: itself if it's a folder, else its parent.
pub fn start_dir(typed: &str) -> Option<PathBuf> {
    let p = expand(typed.trim());
    if typed.trim().is_empty() {
        return home();
    }
    if p.is_dir() { Some(p) } else { p.parent().filter(|d| d.is_dir()).map(Path::to_path_buf) }
}

fn executable(p: &Path) -> bool {
    if cfg!(windows) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        ["exe", "cmd", "bat", "com"].contains(&ext.as_str())
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            return p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
        }
        #[allow(unreachable_code)]
        false
    }
}

/// Programs on PATH whose names start with `prefix`.
fn on_path(prefix: &str) -> Vec<String> {
    let mut out: Vec<String> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter(|e| executable(&e.path()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| starts(n, prefix))
        .collect();
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup_by_key(|n| n.to_lowercase());
    out.truncate(MAX);
    out
}

/// Case-insensitive on Windows and macOS, as their file systems are.
fn starts(name: &str, prefix: &str) -> bool {
    if cfg!(target_os = "linux") {
        name.starts_with(prefix)
    } else {
        name.to_lowercase().starts_with(&prefix.to_lowercase())
    }
}

pub fn complete(typed: &str, dirs: bool, exec: bool) -> Vec<String> {
    let typed = typed.trim_start();
    if exec && typed.is_empty() {
        let shells = ["cmd.exe", "powershell.exe", "pwsh.exe", "wsl.exe", "bash", "zsh", "fish", "sh", "nu"];
        return on_path("").into_iter().filter(|n| shells.contains(&n.to_lowercase().as_str())).collect();
    }
    let sep_at = typed.rfind(['/', '\\']);
    if exec && sep_at.is_none() && !typed.starts_with('~') {
        return on_path(typed);
    }
    // What's typed up to the last separator stays as typed (`~/.ssh/`); the rest is matched.
    let (head, prefix) = match sep_at {
        Some(i) => typed.split_at(i + 1),
        None if typed == "~" => ("~/", ""),
        None => ("", typed),
    };
    let dir = if head.is_empty() { std::env::current_dir().unwrap_or_default() } else { expand(head) };
    // Folders end in the separator typed last, else the system's.
    let sep = match head.chars().last() {
        Some(c @ ('/' | '\\')) => c,
        _ => std::path::MAIN_SEPARATOR,
    };
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let mut found: Vec<(bool, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let is_dir = e.file_type().ok()?.is_dir() || e.path().is_dir();
            (starts(&name, prefix) && (is_dir || !dirs)).then_some((is_dir, name))
        })
        .collect();
    // Folders first, then files; hidden ones only once a dot is typed.
    found.retain(|(_, n)| !n.starts_with('.') || prefix.starts_with('.'));
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    found
        .into_iter()
        .take(MAX)
        .map(|(d, n)| format!("{head}{n}{}", if d { sep.to_string() } else { String::new() }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_entries() {
        let dir = std::env::temp_dir().join(format!("xtermium-paths-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("keys")).unwrap();
        std::fs::write(dir.join("id_ed25519"), "").unwrap();
        std::fs::write(dir.join("id_rsa"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();
        let typed = format!("{}/", dir.display());
        let all = complete(&typed, false, false);
        assert_eq!(all, [format!("{typed}keys/"), format!("{typed}id_ed25519"), format!("{typed}id_rsa")]);
        assert_eq!(complete(&format!("{typed}id_e"), false, false), [format!("{typed}id_ed25519")]);
        assert_eq!(complete(&typed, true, false), [format!("{typed}keys/")], "folders only");
        assert_eq!(complete(&format!("{typed}."), false, false), [format!("{typed}.hidden")]);
        assert!(complete("/no/such/place/", false, false).is_empty());
        if home().is_some() {
            assert!(complete("~", true, false).iter().all(|p| p.starts_with("~/")));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
