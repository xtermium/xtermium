//! MobaXterm: `MobaXterm.ini` and `.mxtsessions` exports (same format).
//!
//! Format:
//!   [Bookmarks_N]  SubRep=Folder\Sub   ImgNum=42
//!   name=#<icon>#<type>%f1%f2...#<terminal fields>#...
//! type: 0 SSH, 4 RDP, 8 Serial, 11 Browser (others skipped).
//!   [PortForwarding] 0000.Name=Local;user@host:22;dest:port;listen;?;key;bind;proxy;-1
//! Passwords are encrypted with the master password and are not imported.

use super::{Export, Import};
use crate::config::{Endpoint, Kind, Session, Ssh, Store, Style, Tunnel, TunnelKind};
use std::collections::BTreeMap;
use std::fmt::Write;

pub fn parse(ini: &str) -> Import {
    let mut out = Import { sessions: vec![], tunnels: vec![], skipped: 0 };
    let mut section = "";
    let mut folder = String::new();
    for line in ini.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s;
            folder.clear();
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        if section == "Bookmarks" || section.starts_with("Bookmarks_") {
            match key {
                "SubRep" => folder = val.replace('\\', "/"),
                "ImgNum" => {}
                _ if val.starts_with('#') => match session(key, &folder, val) {
                    Some(s) => out.sessions.push(s),
                    None => out.skipped += 1,
                },
                _ => {}
            }
        } else if section == "PortForwarding" {
            match tunnel(key, val) {
                Some(t) => out.tunnels.push(t),
                None => out.skipped += 1,
            }
        }
    }
    out
}

fn session(name: &str, folder: &str, val: &str) -> Option<Session> {
    let blocks: Vec<&str> = val.split('#').collect();
    let f: Vec<&str> = blocks.get(2)?.split('%').collect();
    let get = |i: usize| f.get(i).copied().unwrap_or("").to_string();
    let port = |i: usize, def: u16| f.get(i).and_then(|p| p.parse().ok()).unwrap_or(def);
    let kind = match f[0] {
        "0" => Kind::Ssh(Ssh {
            target: Endpoint { host: get(1), port: port(2, 22), user: get(3), key: path(&get(14)), ..Default::default() },
            jump: (!get(8).is_empty()).then(|| Endpoint {
                host: get(8),
                port: port(9, 22),
                user: get(10),
                key: path(&get(15)),
                ..Default::default()
            }),
            command: get(7),
            ..Default::default()
        }),
        "1" => Kind::Telnet { host: get(1), port: port(2, 23) },
        "5" => Kind::Vnc { host: get(1), port: port(2, 5900), credential: String::new() },
        "4" => Kind::Rdp { host: get(1), port: port(2, 3389), user: get(3), quality: None, credential: String::new(), smb_login: None, no_shares: false },
        "8" => Kind::Serial { port: get(8).split_whitespace().next().unwrap_or("").to_string(), baud: baud(&get(2)) },
        "11" => Kind::Browser { url: get(1), credential: String::new(), profile: String::new() },
        _ => return None,
    };
    let style = blocks.get(3).map(|b| style(b)).unwrap_or_default();
    let mut s = Session::new(name, folder, kind);
    s.style = style;
    Some(s)
}

fn style(block: &str) -> Style {
    let t: Vec<&str> = block.split('%').collect();
    let rgb = |i: usize| -> Option<[u8; 3]> {
        let v: Vec<u8> = t.get(i)?.split(',').filter_map(|c| c.parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    };
    // The 16 colours are stored interleaved: black, bright black, red, bright red, ...
    let pal: Option<Vec<[u8; 3]>> = (0..16).map(|i| rgb(16 + i)).collect();
    let palette = pal.map(|p| (0..16).map(|i| p[(i % 8) * 2 + i / 8]).collect());
    let d = Style::default();
    Style {
        // MobaFont is MobaXterm's own built-in font.
        font: t.first().filter(|f| **f != "MobaFont").unwrap_or(&"").to_string(),
        font_size: t.get(1).and_then(|s| s.parse::<f32>().ok()).map(|s| s * 4.0 / 3.0).unwrap_or(d.font_size),
        term: t.get(13).filter(|s| !s.is_empty()).map(|s| s.to_string()).unwrap_or(d.term),
        fg: rgb(6),
        bg: rgb(7),
        cursor: rgb(8),
        palette,
    }
}

fn tunnel(key: &str, val: &str) -> Option<Tunnel> {
    let f: Vec<&str> = val.split(';').collect();
    let kind = match *f.first()? {
        "Local" => TunnelKind::Local,
        "Remote" => TunnelKind::Remote,
        "Dynamic" => TunnelKind::Dynamic,
        _ => return None,
    };
    let (user, hostport) = f.get(1)?.split_once('@')?;
    let (host, sport) = hostport.split_once(':').unwrap_or((hostport, "22"));
    let (dest_host, dest_port) = f.get(2)?.split_once(':').unwrap_or(("", "0"));
    Some(Tunnel {
        name: key.split_once('.').map_or(key, |(_, n)| n).to_string(),
        kind,
        ssh: Endpoint {
            host: host.into(),
            port: sport.parse().ok()?,
            user: user.into(),
            key: path(f.get(5).unwrap_or(&"")),
            ..Default::default()
        },
        bind: f.get(6).filter(|s| !s.is_empty()).unwrap_or(&"127.0.0.1").to_string(),
        listen_port: f.get(3)?.parse().ok()?,
        dest_host: if dest_host == "-" { String::new() } else { dest_host.into() },
        dest_port: dest_port.parse().unwrap_or(0),
        autostart: false,
    })
}

/// Expand the file's path macros.
fn path(p: &str) -> String {
    let dirs = directories::UserDirs::new();
    let base = directories::BaseDirs::new();
    let mut s = p.to_string();
    if let Some(d) = &dirs {
        s = s.replace("_ProfileDir_", &d.home_dir().to_string_lossy());
        if let Some(docs) = d.document_dir() {
            s = s.replace("_MyDocuments_", &docs.to_string_lossy());
        }
    }
    if let Some(b) = &base {
        s = s.replace("_LocalAppDataDir_", &b.data_local_dir().to_string_lossy());
    }
    s
}

/// The serial speed field looks like "1009600" for 9600; pick the standard rate it ends with.
// ponytail: suffix heuristic from one observed sample, decode the field properly if it misfires.
fn baud(s: &str) -> u32 {
    const RATES: [u32; 12] = [921600, 460800, 230400, 115200, 57600, 38400, 19200, 14400, 9600, 4800, 2400, 1200];
    RATES.into_iter().find(|r| s.ends_with(&r.to_string())).unwrap_or(9600)
}

/// `%` and `#` separate fields; the format has no escape for them.
fn clean(s: &str) -> String {
    s.replace(['%', '#', '\r', '\n'], "_")
}

/// Sessions as a `.mxtsessions` file: one `[Bookmarks_N]` section per folder.
// ponytail: serial sessions are left out; the speed field's encoding is only partly understood.
pub fn export(store: &Store) -> Export {
    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    folders.insert(String::new(), vec![]);
    for f in &store.folders {
        folders.entry(f.path.clone()).or_default();
    }
    let mut skipped = 0;
    for s in &store.sessions {
        match line(s) {
            Some(l) => folders.entry(s.folder.clone()).or_default().push(l),
            None => skipped += 1,
        }
    }
    let mut out = String::new();
    for (i, (folder, lines)) in folders.iter().enumerate() {
        let section = if i == 0 { "Bookmarks".to_string() } else { format!("Bookmarks_{i}") };
        let _ = write!(
            out,
            "[{section}]\r\nSubRep={}\r\nImgNum={}\r\n",
            folder.replace('/', "\\"),
            if i == 0 { 42 } else { 41 }
        );
        lines.iter().for_each(|l| out.push_str(l));
        out.push_str("\r\n");
    }
    if !store.tunnels.is_empty() {
        out.push_str("[PortForwarding]\r\n");
        for (i, t) in store.tunnels.iter().enumerate() {
            let kind = match t.kind {
                TunnelKind::Local => "Local",
                TunnelKind::Remote => "Remote",
                TunnelKind::Dynamic => "Dynamic",
            };
            let dest =
                if t.kind == TunnelKind::Dynamic { "-:0".into() } else { format!("{}:{}", t.dest_host, t.dest_port) };
            let _ = write!(
                out,
                "{i:04}.{}={kind};{}@{}:{};{dest};{};0;{};{};No proxy selected;-1\r\n",
                clean(&t.name).replace(['=', ';'], "_"),
                t.ssh.user,
                t.ssh.host,
                t.ssh.port,
                t.listen_port,
                t.ssh.key,
                t.bind
            );
        }
    }
    Export { files: vec![("sessions.mxtsessions".into(), out.into_bytes())], skipped }
}

/// One `name=#icon#fields#terminal#0# #-1` bookmark line.
fn line(s: &Session) -> Option<String> {
    let (icon, fields) = match &s.kind {
        Kind::Ssh(ssh) => {
            let t = &ssh.target;
            let j = ssh.jump.clone().unwrap_or_default();
            let jport = if ssh.jump.is_some() { j.port.to_string() } else { String::new() };
            let f = [&t.host, &t.port.to_string(), &t.user, "", "-1", "-1", &ssh.command, &j.host, &jport, &j.user]
                .iter()
                .map(|v| clean(v))
                .collect::<Vec<_>>()
                .join("%");
            (109, format!("0%{f}%0%0%0%{}%{}%-1%0%0%0%%1080%%0%0%1", clean(&t.key), clean(&j.key)))
        }
        Kind::Rdp { host, port, user, .. } => (91, format!("4%{}%{port}%{}", clean(host), clean(user))),
        Kind::Browser { url, .. } => (103, format!("11%{}", clean(url))),
        Kind::Serial { .. }
        | Kind::Shell { .. }
        | Kind::Telnet { .. }
        | Kind::Winrm { .. }
        | Kind::Vnc { .. }
        | Kind::Kvm { .. }
        | Kind::Agent { .. } => return None,
    };
    let name = clean(&s.name).replace(['=', '[', ']'], "_");
    Some(format!("{name}=#{icon}#{fields}#{}#0# #-1\r\n", terminal(&s.style)))
}

/// The terminal block: font, size in points, colours and TERM.
fn terminal(st: &Style) -> String {
    let rgb = |c: Option<[u8; 3]>, d: &str| c.map_or(d.to_string(), |[r, g, b]| format!("{r},{g},{b}"));
    let font = if st.font.is_empty() { "MobaFont".to_string() } else { clean(&st.font) };
    let pt = (st.font_size * 3.0 / 4.0).round() as u32;
    // Custom colours go in file order: black, bright black, red, bright red, ...
    let colors = match &st.palette {
        Some(p) if p.len() == 16 => {
            (0..16).map(|j| rgb(Some(p[j / 2 + 8 * (j % 2)]), "")).collect::<Vec<_>>().join("%")
        }
        _ => "_Std_Colors_0_".into(),
    };
    format!(
        "{font}%{pt}%0%0%-1%15%{}%{}%{}%0%-1%0%%{}%-1%0%{colors}%80%24%0%1%-1%<none>%%0%0%-1",
        rgb(st.fg, "236,236,236"),
        rgb(st.bg, "30,30,30"),
        rgb(st.cursor, "180,180,192"),
        clean(&st.term)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "[Bookmarks]\r\nSubRep=\r\nImgNum=42\r\n\
Serial (COM)=#131#8%1%1009600%3%0%0%1%0%COM3  (USB Serial Port (COM3))%0#0# #-1\r\n\
[Bookmarks_1]\r\nSubRep=Lab\\DB\r\nImgNum=39\r\n\
db1=#94#0%192.0.2.15%2222%root%%0%-1%htop%gw.example%22%jumper%0%0%0%_ProfileDir_\\.ssh\\id_ed25519%%-1%-1%0%0%%1080#FiraCode%10%0%0%0%15%236,236,236%30,30,30%64,255,0%5%-1%0%%xterm-256color%-1%0%0,0,0%1,1,1%2,0,0%3,0,0%4,0,0%5,0,0%6,0,0%7,0,0%8,0,0%9,0,0%10,0,0%11,0,0%12,0,0%13,0,0%14,0,0%15,0,0%80%24#0# #-1\r\n\
win=#91#4%192.0.2.19%3389%.\\Administrator%0#Consolas%10#0# #-1\r\n\
weird=#1#7%x#y#0\r\n\
[PortForwarding]\r\n0000.Socks=Dynamic;me@bastion:2200;-:0;1080;0;;0.0.0.0;No proxy selected;-1\r\n\
0001.Pg=Local;me@bastion;192.0.2.15:5432;15432;0;;127.0.0.1;No proxy selected;-1\r\n";

    #[test]
    fn imports_bookmarks_ini() {
        let r = parse(INI);
        assert_eq!(r.sessions.len(), 3);
        assert_eq!(r.skipped, 1);

        let serial = &r.sessions[0];
        assert!(matches!(&serial.kind, Kind::Serial { port, baud: 9600 } if port == "COM3"));

        let db = &r.sessions[1];
        assert_eq!(db.folder, "Lab/DB");
        let Kind::Ssh(ssh) = &db.kind else { panic!() };
        assert_eq!((ssh.target.host.as_str(), ssh.target.port, ssh.target.user.as_str()), ("192.0.2.15", 2222, "root"));
        assert!(ssh.target.key.ends_with(".ssh\\id_ed25519") && !ssh.target.key.contains("_ProfileDir_"));
        assert_eq!(ssh.command, "htop");
        let j = ssh.jump.as_ref().unwrap();
        assert_eq!((j.host.as_str(), j.user.as_str()), ("gw.example", "jumper"));
        assert_eq!(db.style.font, "FiraCode");
        assert_eq!(db.style.bg, Some([30, 30, 30]));
        let pal = db.style.palette.as_ref().unwrap();
        // interleaved -> ANSI order: normal red is slot 2, bright black is slot 1
        assert_eq!((pal[1], pal[8]), ([2, 0, 0], [1, 1, 1]));

        assert!(matches!(&r.sessions[2].kind, Kind::Rdp { port: 3389, .. }));

        let t = &r.tunnels;
        assert_eq!(
            (t[0].kind, t[0].ssh.port, t[0].listen_port, t[0].bind.as_str()),
            (TunnelKind::Dynamic, 2200, 1080, "0.0.0.0")
        );
        assert_eq!(
            (t[1].kind, t[1].ssh.port, t[1].dest_host.as_str(), t[1].dest_port),
            (TunnelKind::Local, 22, "192.0.2.15", 5432)
        );

        // round-trips through our TOML format
        let store = Store { sessions: r.sessions, tunnels: r.tunnels, ..Default::default() };
        let back: Store = toml::from_str(&toml::to_string_pretty(&store).unwrap()).unwrap();
        assert!(back.sessions[1].kind == store.sessions[1].kind);
    }

    #[test]
    fn export_round_trips() {
        let mut src = super::super::tests::sample();
        src.sessions[0].style.palette = Some((0..16u8).map(|i| [i, 0, 0]).collect());
        src.sessions[0].style.font_size = 16.0;
        let out = export(&src);
        assert_eq!(out.skipped, 2, "serial and shell have no equivalent");
        let back = parse(std::str::from_utf8(&out.files[0].1).unwrap());
        assert_eq!(back.skipped, 0);
        super::super::tests::same(&src, &back.sessions, &["db1", "web", "win", "wiki"]);
        let db = back.sessions.iter().find(|s| s.name == "db1").unwrap();
        assert_eq!(db.style.palette, src.sessions[0].style.palette, "colours keep their slots");
        assert_eq!(db.style.font_size, 16.0);
        let t = &back.tunnels[0];
        assert_eq!((t.name.as_str(), t.listen_port, t.dest_port, t.ssh.host.as_str()), ("pg", 15432, 5432, "bastion"));
    }
}
