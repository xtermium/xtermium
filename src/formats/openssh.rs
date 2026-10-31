//! OpenSSH client config (`~/.ssh/config`). Each concrete `Host` becomes an SSH session, and its
//! `LocalForward` / `RemoteForward` / `DynamicForward` lines become tunnels. Wildcard hosts,
//! `Match` blocks and options outside a `Host` are left alone. Exports mark folders and tunnels
//! with `# xtermium` comments, so they come back the same.

use super::{Export, Import, endpoint, host_port, ssh, tunnel_name};
use crate::config::{Endpoint, Kind, Store, Tunnel, TunnelKind};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;

const FOLDER: &str = "# xtermium folder:";
const TUNNEL: &str = "# xtermium tunnel";

#[derive(Default)]
struct Block {
    alias: String,
    folder: String,
    tunnel: bool,
    /// Lower-case keyword → values; the first value given wins, as in ssh.
    opts: BTreeMap<String, Vec<String>>,
}

impl Block {
    fn get(&self, k: &str) -> Option<&str> {
        self.opts.get(k).and_then(|v| v.first()).map(String::as_str)
    }
}

fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    let (mut folder, mut tunnel) = (String::new(), false);
    // None outside a usable Host block (before the first, or in a wildcard / Match block).
    let mut cur: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(f) = line.strip_prefix(FOLDER) {
            folder = f.trim().into();
            continue;
        }
        if line == TUNNEL {
            tunnel = true;
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, rest) = line.split_once(|c: char| c.is_whitespace() || c == '=').unwrap_or((line, ""));
        let key = key.to_lowercase();
        let value =
            rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=').trim().trim_matches('"').to_string();
        match key.as_str() {
            "host" => {
                let alias = value.split_whitespace().find(|p| !p.contains(['*', '?', '!']));
                cur = alias.map(|a| {
                    out.push(Block {
                        alias: a.into(),
                        folder: std::mem::take(&mut folder),
                        tunnel,
                        ..Default::default()
                    });
                    out.len() - 1
                });
                tunnel = false;
            }
            "match" => cur = None,
            _ => {
                if let Some(i) = cur {
                    out[i].opts.entry(key).or_default().push(value);
                }
            }
        }
    }
    out
}

pub fn parse(text: &str) -> Import {
    let blocks = blocks(text);
    let target = |b: &Block| Endpoint {
        host: b.get("hostname").unwrap_or(&b.alias).into(),
        port: b.get("port").and_then(|p| p.parse().ok()).unwrap_or(22),
        user: b.get("user").unwrap_or("").into(),
        key: b.get("identityfile").unwrap_or("").into(),
        ..Default::default()
    };
    let mut out = Import::default();
    for b in &blocks {
        let ep = target(b);
        // ponytail: one jump host; for `ProxyJump a,b` only `a` is used.
        let jump = b.get("proxyjump").filter(|j| *j != "none").map(|j| {
            let hop = j.split(',').next().unwrap_or(j);
            let mut e = endpoint(hop, 22);
            // A jump named after another Host entry takes that entry's settings.
            if let Some(other) = blocks.iter().find(|o| o.alias == e.host) {
                let o = target(other);
                e = Endpoint { user: if e.user.is_empty() { o.user } else { e.user }, ..o };
            }
            e
        });
        let mut forwards = vec![];
        for (k, kind) in [
            ("localforward", TunnelKind::Local),
            ("remoteforward", TunnelKind::Remote),
            ("dynamicforward", TunnelKind::Dynamic),
        ] {
            for v in b.opts.get(k).into_iter().flatten() {
                if let Some(t) = forward(kind, v, &ep) {
                    forwards.push(t);
                }
            }
        }
        if b.tunnel && forwards.len() == 1 {
            forwards[0].name = b.alias.clone();
        } else {
            for t in &mut forwards {
                t.name = tunnel_name(&b.alias, t.kind, t.listen_port);
            }
        }
        out.tunnels.extend(forwards);
        if !b.tunnel {
            let mut s = ssh(&b.alias, &b.folder, ep, jump, b.get("remotecommand").unwrap_or("").into());
            if let Kind::Ssh(o) = &mut s.kind {
                o.ciphers = b.get("ciphers").map(ciphers).unwrap_or_default();
                o.compression = b.get("compression").map(|v| v.eq_ignore_ascii_case("yes"));
                o.keepalive = b.get("serveraliveinterval").and_then(|v| v.parse().ok());
                o.agent_forward = b.get("forwardagent").is_some_and(|v| v.eq_ignore_ascii_case("yes"));
            }
            out.sessions.push(s);
        }
    }
    out
}

/// OpenSSH `Ciphers`: a list, or the defaults with some added (`+`), removed (`-`) or put first
/// (`^`). Only ciphers xtermium has are kept; none left means the defaults.
fn ciphers(v: &str) -> Vec<String> {
    let names = |s: &str| -> Vec<String> {
        s.split(',').map(str::trim).filter(|c| crate::ssh::CIPHERS.contains(c)).map(String::from).collect()
    };
    let mut d = crate::ssh::default_ciphers();
    match v.as_bytes().first() {
        Some(b'+') => {
            let more: Vec<String> = names(&v[1..]).into_iter().filter(|c| !d.contains(c)).collect();
            d.extend(more);
        }
        Some(b'-') => {
            let gone = names(&v[1..]);
            d.retain(|c| !gone.contains(c));
        }
        Some(b'^') => {
            let first = names(&v[1..]);
            d.retain(|c| !first.contains(c));
            d.splice(0..0, first);
        }
        _ => return names(v),
    }
    d
}

/// `[bind:]port host:port` (or `[bind:]port` for dynamic) → a tunnel over `ssh`.
fn forward(kind: TunnelKind, v: &str, ssh: &Endpoint) -> Option<Tunnel> {
    let mut parts = v.split_whitespace();
    let listen = endpoint(parts.next()?, 0);
    let (bind, listen_port) = match listen.port {
        0 => ("127.0.0.1".to_string(), listen.host.parse().ok()?),
        p => (if listen.host == "*" { "0.0.0.0".into() } else { listen.host }, p),
    };
    let dest = parts.next().map(|d| endpoint(d, 0)).unwrap_or_default();
    if kind != TunnelKind::Dynamic && dest.host.is_empty() {
        return None;
    }
    Some(Tunnel {
        name: String::new(),
        kind,
        ssh: ssh.clone(),
        bind,
        listen_port,
        dest_host: dest.host,
        dest_port: dest.port,
        autostart: false,
    })
}

pub fn export(store: &Store) -> Export {
    let mut out = String::from("# Exported from xtermium\n");
    let mut used = HashSet::new();
    // Host aliases are single words and must be unique.
    let mut alias = |name: &str| {
        let base: String = name.split_whitespace().collect::<Vec<_>>().join("-");
        let a = (1..)
            .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
            .find(|a| !used.contains(a))
            .unwrap();
        used.insert(a.clone());
        a
    };
    let common = |out: &mut String, e: &Endpoint| {
        let _ = writeln!(out, "    HostName {}", e.host);
        if e.port != 22 {
            let _ = writeln!(out, "    Port {}", e.port);
        }
        if !e.user.is_empty() {
            let _ = writeln!(out, "    User {}", e.user);
        }
        if !e.key.is_empty() {
            let _ = writeln!(out, "    IdentityFile \"{}\"", e.key);
        }
    };
    let mut skipped = 0;
    for s in &store.sessions {
        let Kind::Ssh(ssh) = &s.kind else {
            skipped += 1;
            continue;
        };
        out.push('\n');
        if !s.folder.is_empty() {
            let _ = writeln!(out, "{FOLDER} {}", s.folder);
        }
        let _ = writeln!(out, "Host {}", alias(&s.name));
        common(&mut out, &ssh.target);
        if let Some(j) = &ssh.jump {
            let user = if j.user.is_empty() { String::new() } else { format!("{}@", j.user) };
            let _ = writeln!(out, "    ProxyJump {user}{}", host_port(&j.host, j.port));
        }
        if !ssh.command.is_empty() {
            let _ = writeln!(out, "    RemoteCommand {}\n    RequestTTY yes", ssh.command);
        }
        if !ssh.ciphers.is_empty() {
            let _ = writeln!(out, "    Ciphers {}", ssh.ciphers.join(","));
        }
        if let Some(on) = ssh.compression {
            let _ = writeln!(out, "    Compression {}", if on { "yes" } else { "no" });
        }
        if let Some(secs) = ssh.keepalive {
            let _ = writeln!(out, "    ServerAliveInterval {secs}");
        }
        if ssh.agent_forward {
            let _ = writeln!(out, "    ForwardAgent yes");
        }
    }
    for t in &store.tunnels {
        let _ = writeln!(out, "\n{TUNNEL}\nHost {}", alias(&t.name));
        common(&mut out, &t.ssh);
        let listen = host_port(&t.bind, t.listen_port);
        let _ = match t.kind {
            TunnelKind::Local => writeln!(out, "    LocalForward {listen} {}", host_port(&t.dest_host, t.dest_port)),
            TunnelKind::Remote => writeln!(out, "    RemoteForward {listen} {}", host_port(&t.dest_host, t.dest_port)),
            TunnelKind::Dynamic => writeln!(out, "    DynamicForward {listen}"),
        };
    }
    Export { files: vec![("config".into(), out.into_bytes())], skipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
Host *
    User nobody
    ServerAliveInterval 30

Host bastion
    HostName 203.0.113.10
    User me
    Port 2200

Host db db.internal
    HostName 192.0.2.15
    User=root
    IdentityFile ~/.ssh/id_ed25519
    ProxyJump bastion
    LocalForward 15432 localhost:5432
    DynamicForward 0.0.0.0:1080

Match host *.corp
    User corp

Host *.example
    User web
";

    #[test]
    fn imports_ssh_config() {
        let r = parse(CONFIG);
        assert_eq!(r.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["bastion", "db"]);
        let Kind::Ssh(db) = &r.sessions[1].kind else { panic!() };
        assert_eq!(
            (db.target.host.as_str(), db.target.user.as_str(), db.target.key.as_str()),
            ("192.0.2.15", "root", "~/.ssh/id_ed25519")
        );
        let j = db.jump.as_ref().unwrap();
        assert_eq!(
            (j.host.as_str(), j.port, j.user.as_str()),
            ("203.0.113.10", 2200, "me"),
            "jump resolved through its Host entry"
        );
        let t = &r.tunnels;
        assert_eq!(
            (t[0].name.as_str(), t[0].listen_port, t[0].dest_host.as_str(), t[0].dest_port),
            ("db L15432", 15432, "localhost", 5432)
        );
        assert_eq!((t[1].kind, t[1].bind.as_str(), t[1].listen_port), (TunnelKind::Dynamic, "0.0.0.0", 1080));
        assert_eq!(t[1].ssh.host, "192.0.2.15");
    }

    #[test]
    fn imports_connection_options() {
        let r = parse("Host old\n    Ciphers +aes128-cbc,3des-cbc\n    Compression yes\n    ServerAliveInterval 15\n    ForwardAgent yes\n");
        let Kind::Ssh(o) = &r.sessions[0].kind else { panic!() };
        assert!(o.agent_forward);
        let back = parse(std::str::from_utf8(&export(&Store { sessions: r.sessions.clone(), ..Default::default() }).files[0].1).unwrap());
        assert!(matches!(&back.sessions[0].kind, Kind::Ssh(s) if s.agent_forward), "exported too");
        let r = parse("Host old\n    Ciphers +aes128-cbc,3des-cbc\n    Compression yes\n    ServerAliveInterval 15\n");
        let Kind::Ssh(o) = &r.sessions[0].kind else { panic!() };
        assert_eq!(o.ciphers.last().map(String::as_str), Some("aes128-cbc"), "3des-cbc isn't available");
        assert_eq!((o.compression, o.keepalive), (Some(true), Some(15)));
        assert_eq!(ciphers("^aes128-gcm@openssh.com")[..2], ["aes128-gcm@openssh.com", "aes256-gcm@openssh.com"]);
        assert!(!ciphers("-chacha20-poly1305@openssh.com").iter().any(|c| c.starts_with("chacha")));
        assert_eq!(ciphers("aes256-ctr,blowfish-cbc"), ["aes256-ctr"]);
        let Kind::Ssh(db) = &parse(CONFIG).sessions[1].kind else { panic!() };
        assert_eq!((&db.ciphers, db.compression, db.keepalive), (&vec![], None, None), "wildcard hosts are left alone");
    }

    #[test]
    fn export_round_trips() {
        let src = super::super::tests::sample();
        let out = export(&src);
        assert_eq!(out.skipped, 4, "only SSH sessions");
        let back = parse(std::str::from_utf8(&out.files[0].1).unwrap());
        super::super::tests::same(&src, &back.sessions, &["db1", "web"]);
        assert_eq!(back.sessions.len(), 2, "the tunnel is not a session");
        let t = &back.tunnels[0];
        assert_eq!(
            (t.name.as_str(), t.listen_port, t.dest_host.as_str(), t.ssh.host.as_str()),
            ("pg", 15432, "192.0.2.15", "bastion")
        );
    }
}
