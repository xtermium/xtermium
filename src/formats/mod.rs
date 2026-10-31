//! Sessions and tunnels to and from other apps' files: MobaXterm and OpenSSH's
//! `~/.ssh/config`. Passwords never cross: the other apps keep them encrypted, and exports
//! leave them out.

mod mobaxterm;
mod openssh;

use crate::config::{Endpoint, Kind, Session, Ssh, Store, TunnelKind};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Passphrase-protected `.xtermium` export (see vault.rs); the caller handles it.
    Xtermium,
    Mobaxterm,
    Openssh,
}

#[derive(Default)]
pub struct Import {
    pub sessions: Vec<Session>,
    pub tunnels: Vec<crate::config::Tunnel>,
    /// Entries of a kind xtermium has no equivalent for.
    pub skipped: usize,
}

pub struct Export {
    /// (suggested file name, contents).
    pub files: Vec<(String, Vec<u8>)>,
    /// Sessions the target app has no equivalent for, left out.
    pub skipped: usize,
}

impl Format {
    /// Guesses the format of a file from its name and contents.
    pub fn detect(name: &str, data: &[u8]) -> Option<Format> {
        if data.starts_with(b"XTMX") {
            return Some(Format::Xtermium);
        }
        let text = decode(data);
        let name = name.to_lowercase();
        Some(if name.ends_with(".mxtsessions") || text.contains("[Bookmarks") {
            Format::Mobaxterm
        } else if text.lines().any(|l| l.trim_start().to_lowercase().starts_with("host ")) {
            Format::Openssh
        } else {
            return None;
        })
    }

    pub fn import(self, data: &[u8]) -> Result<Import> {
        let text = decode(data);
        Ok(match self {
            Format::Xtermium => bail!("encrypted exports need their passphrase"),
            Format::Mobaxterm => mobaxterm::parse(&text),
            Format::Openssh => openssh::parse(&text),
        })
    }

    pub fn export(self, store: &Store) -> Result<Export> {
        Ok(match self {
            Format::Xtermium => bail!("encrypted exports need a passphrase"),
            Format::Mobaxterm => mobaxterm::export(store),
            Format::Openssh => openssh::export(store),
        })
    }
}

/// Text of a file: UTF-16 when it starts with a byte-order mark, else UTF-8.
pub fn decode(data: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match data {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

/// A file or folder another app on this computer keeps its sessions in.
#[derive(Serialize, Clone)]
pub struct Source {
    pub format: Format,
    pub path: String,
}

/// Session files of other apps, in their usual places.
pub fn sources() -> Vec<Source> {
    let home = directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()).unwrap_or_default();
    let config = directories::BaseDirs::new().map(|d| d.config_dir().to_path_buf()).unwrap_or_default();
    let docs = directories::UserDirs::new().and_then(|d| d.document_dir().map(Path::to_path_buf)).unwrap_or_default();
    let candidates: [(Format, PathBuf); 3] = [
        (Format::Openssh, home.join(".ssh").join("config")),
        (Format::Mobaxterm, config.join("MobaXterm").join("MobaXterm.ini")),
        (Format::Mobaxterm, docs.join("MobaXterm").join("MobaXterm.ini")),
    ];
    candidates
        .into_iter()
        .filter(|(_, p)| p.is_file())
        .map(|(format, p)| Source { format, path: p.display().to_string() })
        .collect()
}

// ---- helpers shared by the formats ----

fn ssh(name: &str, folder: &str, target: Endpoint, jump: Option<Endpoint>, command: String) -> Session {
    Session::new(name, folder, Kind::Ssh(Ssh { target, jump, command, ..Default::default() }))
}

/// `user@host:port` (all but the host optional) → an endpoint, with `port` as the default.
fn endpoint(s: &str, port: u16) -> Endpoint {
    let (user, rest) = s.rsplit_once('@').unwrap_or(("", s));
    let (host, p) = match rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((h, tail)) => (h, tail.strip_prefix(':').and_then(|p| p.parse().ok())),
        None => match rest.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h, p.parse().ok()),
            _ => (rest, None),
        },
    };
    Endpoint { host: host.into(), port: p.unwrap_or(port), user: user.into(), key: String::new(), ..Default::default() }
}

/// `host:port`, bracketing IPv6 addresses.
fn host_port(host: &str, port: u16) -> String {
    if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

/// Name for a tunnel imported from a session's port forwarding.
fn tunnel_name(session: &str, kind: TunnelKind, port: u16) -> String {
    let k = match kind {
        TunnelKind::Local => "L",
        TunnelKind::Remote => "R",
        TunnelKind::Dynamic => "D",
    };
    format!("{session} {k}{port}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::Tunnel;

    /// A store with one of everything, for round trips.
    pub fn sample() -> Store {
        let ep = |h: &str, p: u16, u: &str| Endpoint { host: h.into(), port: p, user: u.into(), key: String::new(), ..Default::default() };
        Store {
            sessions: vec![
                ssh(
                    "db1",
                    "Lab/DB",
                    Endpoint { key: "/home/me/.ssh/id_ed25519".into(), ..ep("192.0.2.15", 2222, "root") },
                    Some(ep("gw.example", 22, "jumper")),
                    "htop".into(),
                ),
                ssh("web", "", ep("web.example", 22, "deploy"), None, String::new()),
                Session::new(
                    "win",
                    "Lab",
                    Kind::Rdp { host: "192.0.2.19".into(), port: 3389, user: r"CORP\admin".into(), quality: None, credential: String::new(), smb_login: None, no_shares: false },
                ),
                Session::new("console", "Lab", Kind::Serial { port: "COM3".into(), baud: 115200 }),
                Session::new("wiki", "", Kind::Browser { url: "https://wiki.example".into(), credential: String::new(), profile: String::new() }),
                Session::new("local", "", Kind::Shell { program: "pwsh.exe".into(), dir: String::new() }),
            ],
            tunnels: vec![Tunnel {
                name: "pg".into(),
                kind: TunnelKind::Local,
                ssh: ep("bastion", 22, "me"),
                bind: "127.0.0.1".into(),
                listen_port: 15432,
                dest_host: "192.0.2.15".into(),
                dest_port: 5432,
                autostart: false,
            }],
            ..Default::default()
        }
    }

    /// The named sessions come back with the same connection settings and folder.
    pub fn same(a: &Store, b: &[Session], names: &[&str]) {
        for n in names {
            let x = a.sessions.iter().find(|s| s.name == *n).unwrap_or_else(|| panic!("{n} missing from source"));
            let y = b.iter().find(|s| s.name == *n).unwrap_or_else(|| panic!("{n} did not come back"));
            assert!(x.kind == y.kind, "{n} changed");
            assert_eq!(x.folder, y.folder, "{n} moved");
        }
    }

    #[test]
    fn detects_formats() {
        let d = |n: &str, t: &str| Format::detect(n, t.as_bytes());
        assert_eq!(d("x.mxtsessions", ""), Some(Format::Mobaxterm));
        assert_eq!(d("MobaXterm.ini", "[Bookmarks]\nSubRep=\n"), Some(Format::Mobaxterm));
        assert_eq!(d("config", "Host db\n  HostName 192.0.2.15\n"), Some(Format::Openssh));
        assert_eq!(d("x.xtermium", "XTMX..."), Some(Format::Xtermium));
        assert_eq!(d("notes.txt", "hello"), None);
        let utf16: Vec<u8> =
            [0xFF, 0xFE].into_iter().chain("[Bookmarks]".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(Format::detect("m.ini", &utf16), Some(Format::Mobaxterm));
    }

    #[test]
    fn endpoints_parse() {
        let e = endpoint("me@host:2200", 22);
        assert_eq!((e.user.as_str(), e.host.as_str(), e.port), ("me", "host", 2200));
        assert_eq!(endpoint("host", 22).port, 22);
        assert_eq!(endpoint("[::1]:2222", 22).host, "::1");
        assert_eq!(endpoint("2001:db8::1", 22).host, "2001:db8::1");
        assert_eq!(host_port("::1", 22), "[::1]:22");
    }
}
