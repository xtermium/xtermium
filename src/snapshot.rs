//! Restore points (Settings > Import and export): full copies of xtermium's data on this device,
//! as a full backup has it plus the saved logins, encrypted with the data key. Taken when asked,
//! once a day at start, and before a restore replaces everything; the newest `KEEP` stay.

use crate::config::{self, Store};
use crate::vault::{self, Key};
use anyhow::{Result, bail};
use serde::Serialize;
use std::path::PathBuf;

/// Restore points kept; taking another removes the oldest.
const KEEP: usize = 10;

/// Why a restore point was taken (the end of its file name).
pub const REASONS: [&str; 5] = ["manual", "daily", "before-restore", "before-sync", "before-share"];

/// A restore point, as Settings lists it.
#[derive(Serialize, Debug, PartialEq)]
pub struct Point {
    /// Its file name without `.xtm`: `<unix milliseconds>-<reason>`.
    pub id: String,
    /// Unix milliseconds.
    pub at: u64,
    pub reason: String,
    /// Sessions in all workspaces, and the workspaces besides the default one.
    pub sessions: usize,
    pub workspaces: usize,
}

fn folder() -> PathBuf {
    config::dir().join("restore-points")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `id` as a restore point's: digits, a dash, a known reason (nothing that leaves the folder).
fn parse(id: &str) -> Option<(u64, &str)> {
    let (at, reason) = id.split_once('-')?;
    Some((at.parse().ok()?, REASONS.iter().find(|r| **r == reason)?))
}

/// The restore points' ids, newest first.
fn ids() -> Vec<String> {
    let mut ids: Vec<(u64, String)> = std::fs::read_dir(folder())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".xtm").map(str::to_string))
        .filter_map(|id| Some((parse(&id)?.0, id)))
        .collect();
    ids.sort_by(|a, b| b.cmp(a));
    ids.into_iter().map(|(_, id)| id).collect()
}

/// Keeps `text` (a full backup's, `main.rs` `backup`) as a restore point; the oldest beyond
/// `KEEP` go.
pub fn take(text: &str, reason: &str) -> Result<()> {
    if !REASONS.contains(&reason) {
        bail!("unknown reason {reason}");
    }
    let mut at = now();
    // Two in the same millisecond (a restore right after one was taken) keep both.
    while ids().iter().any(|id| parse(id).is_some_and(|(t, _)| t == at)) {
        at += 1;
    }
    let id = format!("{at}-{reason}");
    config::write_plain(&folder().join(format!("{id}.xtm")), &vault::seal(&config::key()?, "restore-point", text.as_bytes()))?;
    for old in ids().iter().skip(KEEP) {
        let _ = std::fs::remove_file(folder().join(format!("{old}.xtm")));
    }
    Ok(())
}

/// True when the newest restore point is a day old or there is none: time for the daily one.
pub fn due() -> bool {
    ids().first().and_then(|id| parse(id)).is_none_or(|(at, _)| now().saturating_sub(at) >= 24 * 3600 * 1000)
}

/// What restore point `id` holds (`take`'s text).
pub fn read(id: &str) -> Result<String> {
    if parse(id).is_none() {
        bail!("no such restore point");
    }
    let data = std::fs::read(folder().join(format!("{id}.xtm")))?;
    Ok(String::from_utf8(vault::open(&config::key()?, "restore-point", &data)?)?)
}

/// The restore points, newest first, with what's in them; ones that can't be read are left out.
pub fn list() -> Vec<Point> {
    ids()
        .into_iter()
        .filter_map(|id| {
            let store: Store = toml::from_str(&read(&id).ok()?).ok()?;
            let (at, reason) = parse(&id)?;
            Some(Point {
                at,
                reason: reason.into(),
                sessions: store.sessions.len() + store.workspaces.iter().map(|w| w.data.sessions.len()).sum::<usize>(),
                workspaces: store.workspaces.len(),
                id,
            })
        })
        .collect()
}

pub fn delete(id: &str) -> Result<()> {
    if parse(id).is_none() {
        bail!("no such restore point");
    }
    Ok(std::fs::remove_file(folder().join(format!("{id}.xtm")))?)
}

/// Encrypts the restore points again under a new data key (config.rs `rekey`), so they still
/// open after joining a synced account.
pub fn rekey(old: &Key, new: &Key) -> Result<()> {
    for id in ids() {
        let path = folder().join(format!("{id}.xtm"));
        let plain = vault::open(old, "restore-point", &std::fs::read(&path)?)?;
        config::write_plain(&path, &vault::seal(new, "restore-point", &plain))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restore points come back as taken, newest first, ten at most; ids can't name other files.
    #[test]
    fn restore_points() {
        // The data folder and key of ssh.rs `test_home`: tests run in parallel in one process.
        let home = std::env::temp_dir().join(format!("xtermium-test-{}", std::process::id()));
        // SAFETY: every test sets the same value.
        unsafe { std::env::set_var("XTERMIUM_HOME", &home) };
        config::set_key([7; 32]);
        assert!(due() && list().is_empty());
        let text = "[[session]]\nname = \"db1\"\nkind = { type = \"shell\" }\n\n[[workspace]]\nname = \"Lab\"\n";
        take(text, "manual").unwrap();
        assert!(!due());
        for _ in 0..11 {
            take("", "daily").unwrap();
        }
        let points = list();
        assert_eq!(points.len(), KEEP);
        assert!(points.windows(2).all(|p| p[0].at > p[1].at) && points.iter().all(|p| p.reason == "daily"), "the oldest went");
        take(text, "before-restore").unwrap();
        let newest = &list()[0];
        assert_eq!((newest.sessions, newest.workspaces, newest.reason.as_str()), (1, 1, "before-restore"));
        assert_eq!(read(&newest.id).unwrap(), text);
        let (key, new) = (config::key().unwrap(), vault::random::<32>());
        rekey(&key, &new).unwrap();
        let sealed = std::fs::read(folder().join(format!("{}.xtm", newest.id))).unwrap();
        assert_eq!(vault::open(&new, "restore-point", &sealed).unwrap(), text.as_bytes(), "opens under the new key");
        rekey(&new, &key).unwrap();
        assert!(read("../sessions").is_err() && delete("1-other").is_err() && take("", "x").is_err());
        delete(&newest.id).unwrap();
        assert_eq!(list().len(), KEEP - 1);
        let _ = std::fs::remove_dir_all(folder());
    }
}
