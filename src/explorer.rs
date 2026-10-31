//! "Open in xtermium" on folders in File Explorer (Settings > General, Windows): an entry on a
//! folder's menu, a folder background's and a drive's, which starts xtermium (or asks the running
//! one, instance.rs) with a local shell in that folder. It lives in the user's own part of the
//! registry, so no administrator rights; Windows 11 lists it under "Show more options" unless
//! the classic menu is on. The entry runs the copy of xtermium that added it.
//!
//! ponytail: Windows only. macOS's counterpart is a Finder Quick Action (a Services workflow in
//! ~/Library/Services), which can't be built or tried here; add it with a Mac to test on.

/// The argument the entry passes, before the folder.
pub const ARG: &str = "--open-here";

/// The folder `xtermium --open-here <folder>` asks for, if it is one.
pub fn here(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == ARG)?;
    let dir = clean(args.get(at + 1)?);
    std::path::Path::new(&dir).is_dir().then_some(dir)
}

/// Explorer passes the folder in quotes, so a drive's `"C:\"` arrives as `C:"` (a backslash
/// before a quote escapes it): put the backslash back.
fn clean(arg: &str) -> String {
    let dir = arg.trim().trim_end_matches('"');
    if dir.len() == 2 && dir.ends_with(':') { format!("{dir}\\") } else { dir.to_string() }
}

/// Whether the entry is on; None where there's no such menu (not Windows).
pub fn state() -> Option<bool> {
    #[cfg(windows)]
    return Some(reg::exists(KEYS[0]));
    #[cfg(not(windows))]
    None
}

/// Adds the entry, named `label` (in the interface's language), or takes it away.
pub fn set(on: bool, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        if !on {
            KEYS.iter().for_each(|k| reg::delete(k));
            return Ok(());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy();
        let label: String = label.chars().filter(|c| !c.is_control()).take(80).collect();
        let label = if label.trim().is_empty() { "Open in xtermium".to_string() } else { label };
        for key in KEYS {
            reg::set(key, None, &label)?;
            reg::set(key, Some("Icon"), &format!("\"{exe}\",0"))?;
            // %V: the folder, or the one whose background was clicked.
            reg::set(&format!(r"{key}\command"), None, &format!("\"{exe}\" {ARG} \"%V\""))?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (on, label);
        Err("File Explorer's menu is Windows only".into())
    }
}

/// Under HKEY_CURRENT_USER: a folder's background, a folder, a drive.
#[cfg(windows)]
const KEYS: [&str; 3] = [
    r"Software\Classes\Directory\Background\shell\xtermium",
    r"Software\Classes\Directory\shell\xtermium",
    r"Software\Classes\Drive\shell\xtermium",
];

#[cfg(windows)]
mod reg {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW,
        RegOpenKeyExW, RegSetValueExW,
    };
    use windows::core::{HSTRING, PCWSTR};

    pub fn exists(key: &str) -> bool {
        let mut h = HKEY::default();
        // SAFETY: plain registry calls on a key handle we own and close.
        unsafe {
            let ok = RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, KEY_READ, &mut h).is_ok();
            if ok {
                let _ = RegCloseKey(h);
            }
            ok
        }
    }

    /// Sets a text value (`None`: the key's default one), making the key if needed.
    pub fn set(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
        let mut h = HKEY::default();
        let data: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        let name = name.map(HSTRING::from);
        // SAFETY: as above; `data` is NUL-terminated UTF-16, as REG_SZ wants.
        unsafe {
            RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut h, None)
                .ok()
                .map_err(|e| format!("{key}: {e}"))?;
            let r = RegSetValueExW(h, name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())), None, REG_SZ, Some(&data));
            let _ = RegCloseKey(h);
            r.ok().map_err(|e| format!("{key}: {e}"))
        }
    }

    pub fn delete(key: &str) {
        // SAFETY: as above. Gone already is fine.
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_from_explorer() {
        assert_eq!(clean("C:\""), "C:\\", "a drive's trailing backslash, eaten by the quote");
        assert_eq!(clean(r"C:\Users\me\proj"), r"C:\Users\me\proj");
        assert_eq!(clean("/home/me "), "/home/me");
        let tmp = std::env::temp_dir().to_string_lossy().to_string();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(here(&args(&[ARG, &tmp])), Some(tmp.clone()));
        assert_eq!(here(&args(&[ARG, "Z:\\surely\\not\\here"])), None, "not a folder: the app just starts");
        assert_eq!(here(&args(&[ARG])), None);
        assert_eq!(here(&args(&["tabs"])), None);
    }
}
