//! Cleans what the UI, an import or a sync hands over before it's stored or used: no control
//! characters, no stray spaces, numbers inside their ranges, names that work as names and as
//! file names.

use crate::config::{Endpoint, Kind, Store};
use crate::settings::Settings;

/// One line of text: trimmed, without control characters.
pub fn text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

/// Several lines (notes): control characters other than line breaks and tabs go.
fn lines(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect::<String>().trim().to_string()
}

/// A host name or address: no spaces anywhere.
fn host(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && !c.is_whitespace()).collect()
}

/// `a / b//c ` → `a/b/c`: folder paths in the session tree.
pub fn folder(s: &str) -> String {
    s.split('/').map(text).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("/")
}

/// `#rrggbb` (lower case), or "" for none.
fn color(s: &str) -> String {
    let s = s.trim().to_ascii_lowercase();
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok { s } else { String::new() }
}

/// A built-in icon name or a `data:` image; anything else goes.
fn icon(s: &str) -> String {
    let named =
        !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if named || s.starts_with("data:image/") { s.to_string() } else { String::new() }
}

fn finite(v: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { default }
}

/// A file name that works on Windows, macOS and Linux: no path separators or reserved
/// characters, not a reserved device name, no trailing dot or space.
pub fn file_name(s: &str) -> String {
    let mut name: String = text(s).chars().map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c }).take(120).collect();
    name = name.trim_end_matches(['.', ' ']).to_string();
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        name.insert(0, '_');
    }
    if name.is_empty() { "untitled".into() } else { name }
}

fn endpoint(e: &mut Endpoint, default_port: u16) {
    e.host = host(&e.host);
    e.user = text(&e.user);
    e.key = text(&e.key);
    if e.port == 0 {
        e.port = default_port;
    }
}

fn ciphers(list: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    list.retain(|c| crate::ssh::CIPHERS.contains(&c.as_str()) && seen.insert(c.clone()));
}

impl Store {
    pub fn sanitize(&mut self) {
        for s in &mut self.sessions {
            s.folder = folder(&s.folder);
            s.notes = lines(&s.notes);
            s.color = color(&s.color);
            s.icon = icon(&s.icon);
            s.style.font = text(&s.style.font);
            s.style.font_size = finite(s.style.font_size, 6.0, 48.0, 14.0);
            s.style.term = host(&s.style.term);
            if s.style.term.is_empty() {
                s.style.term = "xterm-256color".into();
            }
            let fallback = match &mut s.kind {
                Kind::Ssh(ssh) => {
                    endpoint(&mut ssh.target, 22);
                    if let Some(j) = &mut ssh.jump {
                        endpoint(j, 22);
                    }
                    ssh.command = text(&ssh.command);
                    ciphers(&mut ssh.ciphers);
                    ssh.keepalive = ssh.keepalive.map(|k| k.min(3600));
                    ssh.target.host.clone()
                }
                Kind::Shell { program, dir } => {
                    (*program, *dir) = (text(program), text(dir));
                    "shell".into()
                }
                Kind::Serial { port, baud } => {
                    *port = host(port);
                    if !(50..=4_000_000).contains(baud) {
                        *baud = 115_200;
                    }
                    port.clone()
                }
                Kind::Rdp { host: h, port, user, quality, .. } => {
                    *h = host(h);
                    *user = text(user);
                    if *port == 0 {
                        *port = 3389;
                    }
                    *quality = quality.map(|q| q.min(2));
                    h.clone()
                }
                Kind::Telnet { host: h, port } => {
                    *h = host(h);
                    if *port == 0 {
                        *port = 23;
                    }
                    h.clone()
                }
                Kind::Winrm { host: h, port, user, https, .. } => {
                    *h = host(h);
                    *user = text(user);
                    if *port == 0 {
                        *port = if *https { 5986 } else { 5985 };
                    }
                    h.clone()
                }
                Kind::Agent { tool, args, dir, mode, effort, model } => {
                    if !crate::config::AGENTS.contains(&tool.as_str()) {
                        *tool = "custom".into();
                    }
                    if !crate::config::AGENT_MODES.contains(&mode.as_str()) {
                        mode.clear();
                    }
                    if !crate::config::AGENT_EFFORTS.contains(&effort.as_str()) {
                        effort.clear();
                    }
                    (*args, *dir, *model) = (text(args), text(dir), text(model));
                    tool.clone()
                }
                Kind::Vnc { host: h, port, .. } => {
                    *h = host(h);
                    if *port == 0 {
                        *port = 5900;
                    }
                    h.clone()
                }
                Kind::Kvm { vendor, host: h, port, .. } => {
                    if !crate::config::KVM_VENDORS.contains(&vendor.as_str()) {
                        *vendor = crate::config::KVM_VENDORS[0].into();
                    }
                    *h = host(h);
                    if *port == 0 {
                        *port = 443;
                    }
                    h.clone()
                }
                Kind::Browser { url, .. } => {
                    *url = host(url);
                    if !url.is_empty() && !url.contains("://") {
                        url.insert_str(0, "https://");
                    }
                    url.clone()
                }
            };
            s.name = text(&s.name).chars().take(200).collect();
            if s.name.is_empty() {
                s.name = if fallback.is_empty() { "session".into() } else { fallback };
            }
        }
        for t in &mut self.tunnels {
            t.name = text(&t.name);
            if t.name.is_empty() {
                t.name = "tunnel".into();
            }
            endpoint(&mut t.ssh, 22);
            t.bind = host(&t.bind);
            if t.bind.is_empty() {
                t.bind = "127.0.0.1".into();
            }
            t.dest_host = host(&t.dest_host);
        }
        for f in &mut self.folders {
            f.path = folder(&f.path);
            let mut seen = std::collections::HashSet::new();
            f.tags = f.tags.iter().map(|t| text(t)).filter(|t| !t.is_empty() && seen.insert(t.clone())).collect();
            f.color = color(&f.color);
            f.icon = icon(&f.icon);
        }
        self.folders.retain(|f| !f.path.is_empty());
        for s in &mut self.snippets {
            s.name = text(&s.name);
            s.text = lines(&s.text);
            if s.name.is_empty() {
                s.name = s.text.lines().next().unwrap_or("").chars().take(40).collect();
            }
        }
        self.snippets.retain(|s| !s.text.is_empty());
        for c in &mut self.credentials {
            c.name = text(&c.name);
            c.user = text(&c.user);
            c.key = text(&c.key);
            // Spaces can be part of a password: only control characters go.
            c.password.retain(|ch| !ch.is_control());
            c.id.retain(|ch| ch.is_ascii_alphanumeric());
            if let Some(g) = &mut c.gateway {
                (g.host, g.user, g.key) = (host(&g.host), text(&g.user), text(&g.key));
                g.password.retain(|ch| !ch.is_control());
                if g.port == 0 {
                    g.port = 22;
                }
            }
            // No host, no gateway.
            c.gateway = c.gateway.take().filter(|g| !g.host.is_empty());
        }
        let mut seen = std::collections::HashSet::new();
        self.credentials.retain(|c| !c.name.is_empty() && !c.id.is_empty() && seen.insert(c.id.clone()));
        // A session pointing at a login that's gone uses its own again.
        let ids: std::collections::HashSet<String> = self.credentials.iter().map(|c| c.id.clone()).collect();
        let keep = |id: &mut String| {
            if !ids.contains(id) {
                id.clear();
            }
        };
        for s in &mut self.sessions {
            match &mut s.kind {
                Kind::Ssh(ssh) => {
                    keep(&mut ssh.target.credential);
                    if let Some(j) = &mut ssh.jump {
                        keep(&mut j.credential);
                    }
                }
                Kind::Rdp { credential, .. }
                | Kind::Vnc { credential, .. }
                | Kind::Browser { credential, .. }
                | Kind::Kvm { credential, .. }
                | Kind::Winrm { credential, .. } => keep(credential),
                _ => {}
            }
        }
        for t in &mut self.tunnels {
            keep(&mut t.ssh.credential);
        }
        // Layouts: their panes' sessions are cleaned like saved ones (logins that are gone too).
        let mut seen = std::collections::HashSet::new();
        for l in &mut self.layouts {
            l.name = text(&l.name).chars().take(60).collect();
            if !LAYOUTS.contains(&l.arrangement.as_str()) {
                l.arrangement = "tiles".into();
            }
            l.panes.truncate(16);
            let mut panes = Store { sessions: std::mem::take(&mut l.panes), credentials: self.credentials.clone(), ..Default::default() };
            panes.sanitize();
            l.panes = panes.sessions;
            // Pane places and tab names go with the sessions, one each; out of step (an older
            // layout, or a session this version can't read), the panes fill one after another.
            if l.at.len() != l.panes.len() {
                l.at.clear();
            }
            l.at.iter_mut().for_each(|a| *a = (*a).min(15));
            l.labels = l.labels.iter().take(l.panes.len()).map(|s| text(s).chars().take(60).collect()).collect();
            let fractions = |v: &[f64]| !v.is_empty() && v.len() <= 16 && v.iter().all(|x| x.is_finite() && *x > 0.0 && *x <= 1.0);
            if l.sizes.as_ref().is_some_and(|s| !fractions(&s.rows) || s.cols.len() != s.rows.len() || !s.cols.iter().all(|c| fractions(c))) {
                l.sizes = None;
            }
        }
        self.layouts.retain(|l| !l.name.is_empty() && !l.panes.is_empty() && seen.insert(l.name.clone()));
        self.profiles(&mut Default::default());
    }

    /// Gives every website session a browser profile of its own (web.rs): one without, or with one
    /// a session before it has (a copy), gets a new one. `seen`: the profiles taken already (other
    /// workspaces'). True if one changed.
    pub fn profiles(&mut self, seen: &mut std::collections::HashSet<String>) -> bool {
        let mut changed = false;
        for s in &mut self.sessions {
            if let Kind::Browser { profile, .. } = &mut s.kind {
                profile.retain(|c| c.is_ascii_alphanumeric());
                if profile.is_empty() || !seen.insert(profile.clone()) {
                    *profile = crate::vault::hex(&crate::vault::random::<16>());
                    seen.insert(profile.clone());
                    changed = true;
                }
            }
        }
        changed
    }
}

/// Pane arrangements a saved layout can have (ui/App.svelte `Layout`).
const LAYOUTS: [&str; 5] = ["single", "columns", "rows", "grid", "tiles"];

impl Settings {
    pub fn sanitize(&mut self) {
        let d = Settings::default();
        self.ui_scale = finite(self.ui_scale, 0.8, 1.6, d.ui_scale);
        self.window_opacity = finite(self.window_opacity, 0.3, 1.0, d.window_opacity);
        self.terminal_opacity = finite(self.terminal_opacity, 0.3, 1.0, d.terminal_opacity);
        self.scheme = text(&self.scheme);
        self.font = text(&self.font);
        self.font_size = finite(self.font_size, 6.0, 48.0, d.font_size);
        self.line_height = finite(self.line_height, 0.8, 2.0, d.line_height);
        self.scrollback = self.scrollback.clamp(100, 200_000);
        self.pane_padding = self.pane_padding.min(48);
        self.highlights.retain(|h| h.color.len() == 7 && h.color.starts_with('#') && h.color[1..].chars().all(|c| c.is_ascii_hexdigit()));
        for h in &mut self.highlights {
            h.pattern = text(&h.pattern).chars().take(300).collect();
        }
        self.highlights.truncate(50);
        for b in [&mut self.background, &mut self.sidebar_background, &mut self.bar_background] {
            if !["", "image", "shader"].contains(&b.kind.as_str()) {
                b.kind.clear();
            }
            b.name = text(&b.name).chars().take(200).collect();
            b.show = finite(b.show, 0.05, 0.95, 0.25);
            if !crate::settings::FITS.contains(&b.fit.as_str()) {
                b.fit = crate::settings::FITS[0].into();
            }
        }
        self.shell = text(&self.shell);
        self.ssh_user = text(&self.ssh_user);
        self.ssh_key = text(&self.ssh_key);
        ciphers(&mut self.ssh_ciphers);
        if self.ssh_ciphers.is_empty() {
            self.ssh_ciphers = d.ssh_ciphers;
        }
        self.ssh_keepalive = self.ssh_keepalive.min(3600);
        self.rdp_quality = self.rdp_quality.min(2);
        if self.automation_port < 1024 {
            self.automation_port = d.automation_port;
        }
        self.automation_token.retain(|c| c.is_ascii_hexdigit());
        self.sync_token = host(&self.sync_token);
        self.sync_email = host(&self.sync_email);
        self.sidebar_width = self.sidebar_width.clamp(180, 640);
        let mut seen = std::collections::HashSet::new();
        self.open_folders.retain_mut(|f| {
            *f = folder(f);
            !f.is_empty() && seen.insert(f.clone())
        });
        self.open_folders.truncate(1000);
        self.quiet_hints.retain(|h| !h.is_empty() && h.len() < 64 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        self.quiet_hints.truncate(100);
        self.language.retain(|c| c.is_ascii_alphanumeric() || c == '-');
        self.language.truncate(16);
        self.custom_css.retain(|c| c.is_ascii_hexdigit());
        self.custom_css.truncate(64);
        self.pane_step = self.pane_step.clamp(1, 500);
        // Broadcast was once multi-exec: a chord set for "toggle-multi" stays with it.
        self.shortcuts = std::mem::take(&mut self.shortcuts)
            .into_iter()
            .map(|(k, v)| (if k == "toggle-multi" { "toggle-broadcast".into() } else { text(&k) }, text(&v)))
            .collect();
        if !crate::settings::OUTLINES.contains(&self.broadcast_outline.as_str()) {
            self.broadcast_outline = d.broadcast_outline;
        }
        self.broadcast_outline_color = color(&self.broadcast_outline_color);
        self.accent = color(&self.accent);
        // Imported schemes: named, one per name, none hiding a built-in one; colours are 24-bit.
        let mut names: Vec<String> = crate::settings::SCHEMES.iter().map(|s| s.name.to_string()).collect();
        names.push("Auto".into());
        self.custom_schemes.retain_mut(|sc| {
            sc.name = text(&sc.name).chars().take(60).collect::<String>().into();
            for c in [&mut sc.bg, &mut sc.fg, &mut sc.cursor, &mut sc.selection].into_iter().chain(&mut sc.ansi) {
                *c &= 0xff_ffff;
            }
            let fresh = !sc.name.is_empty() && !names.contains(&sc.name.to_string());
            names.push(sc.name.to_string());
            fresh
        });
        self.custom_schemes.truncate(200);
        // Saved icons: images (ones fetch_icon or a file gave), each once, named.
        let mut seen = std::collections::HashSet::new();
        self.custom_icons.retain_mut(|i| {
            i.name = text(&i.name).chars().take(60).collect();
            i.data.starts_with("data:image/") && i.data.len() <= 1024 * 1024 && seen.insert(i.data.clone())
        });
        self.custom_icons.truncate(100);
        if self.broadcast_outline_color.is_empty() {
            self.broadcast_outline_color = d.broadcast_outline_color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Session, Snippet, Ssh, Tunnel, TunnelKind};

    #[test]
    fn cleans_names_paths_and_numbers() {
        assert_eq!(text("  db\u{7}1\n "), "db1");
        assert_eq!(folder(" Prod / /DB//"), "Prod/DB");
        assert_eq!((color("#ABCDEF"), color("red"), color("#12345")), ("#abcdef".into(), String::new(), String::new()));
        assert_eq!(file_name("a/b:c*?.log. "), "a_b_c__.log");
        assert_eq!(file_name("CON.txt"), "_CON.txt");
        assert_eq!(file_name("com1"), "_com1");
        assert_eq!(file_name("  "), "untitled");

        let mut ssh = Ssh::default();
        ssh.target.host = " 192.0.2.15 ".into();
        ssh.target.port = 0;
        ssh.ciphers = vec!["aes256-gcm@openssh.com".into(), "rot13".into(), "aes256-gcm@openssh.com".into()];
        ssh.keepalive = Some(99_999);
        let mut store = Store {
            sessions: vec![
                Session::new("\u{1b}[31m", "a//b ", Kind::Ssh(ssh)),
                Session::new("web", "", Kind::Browser { url: "example.com".into(), credential: String::new(), profile: String::new() }),
            ],
            tunnels: vec![Tunnel {
                name: " ".into(),
                kind: TunnelKind::Local,
                ssh: Endpoint::default(),
                bind: "".into(),
                listen_port: 8080,
                dest_host: " x ".into(),
                dest_port: 80,
                autostart: false,
            }],
            snippets: vec![
                Snippet { name: " df ".into(), text: "df -h\u{7}\n".into(), run: true },
                Snippet { name: "".into(), text: " ".into(), run: true },
                Snippet { name: "".into(), text: "uptime\nw".into(), run: false },
            ],
            ..Default::default()
        };
        store.sanitize();
        assert_eq!(
            store.snippets,
            [
                Snippet { name: "df".into(), text: "df -h".into(), run: true },
                Snippet { name: "uptime".into(), text: "uptime\nw".into(), run: false }
            ]
        );
        let s = &store.sessions[0];
        assert_eq!((s.name.as_str(), s.folder.as_str()), ("[31m", "a/b"));
        let Kind::Ssh(ssh) = &s.kind else { panic!() };
        assert_eq!((ssh.target.host.as_str(), ssh.target.port, ssh.keepalive), ("192.0.2.15", 22, Some(3600)));
        assert_eq!(ssh.ciphers, ["aes256-gcm@openssh.com"]);
        assert!(matches!(&store.sessions[1].kind, Kind::Browser { url, .. } if url == "https://example.com"));
        let t = &store.tunnels[0];
        assert_eq!((t.name.as_str(), t.bind.as_str(), t.dest_host.as_str()), ("tunnel", "127.0.0.1", "x"));

        let mut st = Settings { font_size: f32::NAN, scrollback: 5, automation_port: 80, ..Default::default() };
        st.ssh_ciphers.clear();
        st.sanitize();
        assert_eq!((st.font_size, st.scrollback, st.automation_port), (14.0, 100, 8721));
        assert!(!st.ssh_ciphers.is_empty());
    }
}
