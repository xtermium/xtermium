//! User settings (settings.toml) and terminal colour schemes. The Settings window is in the UI.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Effect {
    /// Opaque window.
    None,
    /// See-through, no blur (works wherever the compositor supports transparency). Older versions'
    /// blurred backdrops load as this, so a saved one keeps the window see-through.
    #[serde(alias = "Acrylic", alias = "Mica", alias = "Vibrancy")]
    Transparent,
}

/// How Browser sessions show websites.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Web {
    /// The system's default browser.
    External,
    /// Inside a tab, with the system's browser engine (web.rs).
    Embedded,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Cursor {
    Block,
    Bar,
    Underline,
}

/// What `coop_idle` can be, in minutes: 1, 2 and 8 hours, a day, and never.
pub const COOP_IDLE: [u32; 5] = [60, 120, 480, 1440, 0];

/// How finely, and how often, a shared website or BMC console's picture goes to a Terminal Co-op
/// ring (coop.rs `shoot`): the owner's choice for every viewer.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Low,
    #[default]
    Average,
    High,
}

impl Quality {
    /// The time between two pictures, and the JPEG quality (0 to 100).
    pub fn stream(self) -> (std::time::Duration, u8) {
        let ms = |n| std::time::Duration::from_millis(n);
        match self {
            Quality::Low => (ms(500), 45),
            Quality::Average => (ms(250), 70),
            Quality::High => (ms(125), 85),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub ui_scale: f32,
    pub effect: Effect,
    /// How solid the window is when the effect is on, the same everywhere: sidebar, bars and
    /// terminals. Older versions' window opacity loads as this; their terminal opacity is dropped.
    #[serde(alias = "window_opacity")]
    pub opacity: f32,
    /// Colour scheme name; "Auto" follows the light/dark theme.
    pub scheme: String,
    pub font: String,
    pub font_size: f32,
    /// Use the scheme and font above for every session, ignoring per-session styles.
    pub override_sessions: bool,
    pub cursor: Cursor,
    pub cursor_blink: bool,
    /// Terminal line height as a multiple of the font's.
    pub line_height: f32,
    /// Room between a terminal's text and its pane's edges, in pixels.
    pub pane_padding: u32,
    /// A scroll bar on terminals, showing where in the scrollback the screen is.
    pub scrollbar: bool,
    /// Highlight rules: text in the terminal's default colour that matches one is coloured.
    pub highlight: bool,
    pub highlights: Vec<Highlight>,
    /// A picture or shader behind the text of terminal tabs (Settings > Terminal).
    pub background: Background,
    /// Behind the sidebar, and behind the tab bar with its buttons (Settings > Appearance).
    pub sidebar_background: Background,
    pub bar_background: Background,
    pub scrollback: usize,
    /// New output brings a terminal scrolled back to the bottom; off, the screen stays put.
    pub scroll_on_output: bool,
    pub copy_on_select: bool,
    pub right_click_paste: bool,
    /// Trim whitespace off the start and end of what's pasted into a terminal.
    pub strip_paste: bool,
    /// Save each terminal tab's output to a text file in the logs folder. Off: big outputs cost.
    pub log_to_file: bool,
    pub web: Web,
    /// Program for "Local shell"; empty = the system default.
    pub shell: String,
    /// Used when a session leaves them empty.
    pub ssh_user: String,
    pub ssh_key: String,
    /// Tried after keys and before asking. Stored encrypted, like everything here.
    pub ssh_password: String,
    /// SSH ciphers to offer, best first (ssh.rs `CIPHERS`).
    pub ssh_ciphers: Vec<String>,
    /// Seconds between SSH keepalives; 0 for none.
    pub ssh_keepalive: u32,
    pub ssh_compression: bool,
    /// RDP picture quality against speed: 0 quality, 1 balanced, 2 speed. Sessions can have their own.
    pub rdp_quality: u8,
    /// What crosses the shared clipboard of RDP tabs: 1 this computer's to the desktop, 2 the
    /// desktop's to here, 3 both, 0 nothing (rdp.rs `TO_DESKTOP`, `FROM_DESKTOP`).
    pub rdp_clipboard: u8,
    /// A remote desktop with the keyboard takes the Windows keys and the system's shortcuts
    /// (Alt+Tab, Win+E…) from this computer (Windows).
    pub rdp_keyboard: bool,
    /// An RDP tab's file panel logs in to the host's shares as the desktop's login (sessions
    /// can say otherwise).
    pub smb_auto_login: bool,
    /// Local automation (MCP server and the `xtermium` command line): off unless turned on.
    pub automation: bool,
    /// Its port on 127.0.0.1.
    pub automation_port: u16,
    /// Listen on every network interface (0.0.0.0), for assistants on other machines; off, only
    /// on this computer (127.0.0.1).
    pub automation_network: bool,
    /// Every request must carry this (`Authorization: Bearer …`). Made by the UI when turned on.
    pub automation_token: String,
    /// Cloud sync (docs/sync.md) of sessions and trusted keys, with xtermium's server: on while
    /// signed in; the signed-in account's token and email.
    pub sync: bool,
    pub sync_token: String,
    pub sync_email: String,
    /// Whether the account had Pro the last time the server answered, so what depends on Pro
    /// (the Co-op button staying in the sidebar) holds at start-up and offline.
    pub sync_pro: bool,
    /// What else syncs, each only when ticked (Settings › Account and sync): this file's own copy
    /// (look and defaults, `for_cloud`), the three background files, and the saved logins.
    pub sync_settings: bool,
    pub sync_backgrounds: bool,
    pub sync_logins: bool,
    /// The account button at the bottom of the sidebar; always shown while signed in.
    pub account_button: bool,
    /// The Terminal Co-op button at the bottom of the sidebar; always shown with Pro or while a ring is on.
    pub coop_button: bool,
    /// The pictures of websites and BMC consoles you share in a ring.
    pub coop_quality: Quality,
    /// A ring you started ends once you haven't touched the mouse or keyboard for this many
    /// minutes (`COOP_IDLE` has the choices); 0: never.
    pub coop_idle: u32,
    /// Ask before closing a tab whose connection is still up.
    pub confirm_close: bool,
    /// Reconnect SSH tabs whose connection dropped, a few times with growing waits.
    pub auto_reconnect: bool,
    /// Reopen the tabs that were open when xtermium last closed.
    pub restore_tabs: bool,
    /// The bar under the panes: the focused tab's connection, uptime, cipher, round trip.
    pub status_bar: bool,
    /// Open sessions in the sidebar with one click instead of two.
    pub single_click_open: bool,
    /// With no tabs open, what is typed on the start screen shows at its prompt, and Enter runs it
    /// in a shell.
    pub quick_shell: bool,
    /// The program that shell runs (arguments allowed); empty = `shell`, Local shell's.
    pub start_shell: String,
    /// Reopen the sidebar's folders that were open last time; `open_folders` are those.
    pub remember_folders: bool,
    pub open_folders: Vec<String>,
    /// Tips turned off with "Don't remind me again" (ui/lib/Hint.svelte), by id.
    pub quiet_hints: Vec<String>,
    /// Interface language (a `ui/locales` file name, like "de" or "zh-CN"); "" follows the system.
    pub language: String,
    pub sidebar_width: u32,
    /// The sidebar is a narrow column of buttons, leaving the room to the tabs.
    pub sidebar_collapsed: bool,
    /// The getting-started list on the start screen (Settings › General). A new install starts
    /// with it (`Default`); a settings file from before it existed has no such field and keeps it
    /// off, so people who already know their way around don't get it.
    #[serde(default)]
    pub guide: bool,
    /// The list's steps done so far (ui/lib/Guide.svelte ids), and `pill-seen` once the tab bar's
    /// progress pill has had its one pulse (App.svelte).
    pub guide_done: Vec<String>,
    /// Keyboard shortcuts changed from the defaults: action → key chord ("" = none).
    /// The actions and their defaults are in the UI (ui/lib/actions.ts).
    pub shortcuts: BTreeMap<String, String>,
    /// How far the pane-edge shortcuts (Ctrl+Shift+arrows) move an edge, in pixels.
    pub pane_step: u32,
    /// Closing the window hides it to the tray and the sessions run on (tray.rs).
    pub keep_running: bool,
    /// Start xtermium when you sign in to the computer (support.rs `autostart`).
    pub start_at_login: bool,
    /// Desktop notifications: new versions and connections dropped.
    pub notifications: bool,
    /// Say when a newer release is on GitHub (Settings › About).
    pub update_notify: bool,
    /// Run above the normal scheduling priority (support.rs `priority`; may need administrator rights).
    pub high_priority: bool,
    /// Hex SHA-256 of the custom.css the user kept; a different one asks again (support.rs `custom_css`).
    pub custom_css: String,
    /// The outline on terminals broadcast types into: a style from `OUTLINES` and its colour,
    /// `#rrggbb` or "" for the accent (once `multi_outline`, from when broadcast was called multi-exec).
    #[serde(alias = "multi_outline")]
    pub broadcast_outline: String,
    #[serde(alias = "multi_outline_color")]
    pub broadcast_outline_color: String,
    /// The focused pane's ring glides to the next pane when the focus moves; false: it jumps.
    pub focus_animated: bool,
    /// The interface's accent colour (`#rrggbb`); "" = the built-in gold.
    pub accent: String,
    /// The app's own icon (Settings › Appearance): a square `data:image/png` URL shown on the
    /// windows, the taskbar, the tray, the title bar and the start screen; "" = the mark.
    pub app_icon: String,
    /// An http(s) address the start screen's logo opens in the browser when clicked; "" = none.
    pub logo_link: String,
    /// Terminal colour schemes the user imported, next to the built-in `SCHEMES`.
    pub custom_schemes: Vec<Scheme>,
    /// Images used as session or folder icons, kept to pick again (ui/lib/Appearance.svelte).
    pub custom_icons: Vec<CustomIcon>,
    /// The workspace in use ("" the default one); changed only by switching (main.rs `workspace`).
    pub workspace: String,
}

/// What's behind terminal text, the sidebar or the tab bar. The picture or shader itself is in
/// its own file (main.rs `background_set`); only `show` changes with the other settings.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Background {
    /// "" for none, "image" (PNG, JPEG, GIF, SVG, WebP) or "shader" (a GLSL fragment shader).
    pub kind: String,
    /// Where it came from, to show: a file name or a link.
    pub name: String,
    /// How much it shows through the terminal's colour, 0.05 to 0.95.
    pub show: f32,
    /// How a picture fits the place: one of `FITS`.
    pub fit: String,
    /// When it was set (unix ms): windows fetch it again when this changes.
    pub stamp: u64,
}

impl Settings {
    /// Where a background goes ("terminal", "sidebar", "bar"): its setting and its file's name.
    pub fn background_of(&mut self, place: &str) -> Option<(&mut Background, &'static str)> {
        Some(match place {
            "terminal" => (&mut self.background, "background"),
            "sidebar" => (&mut self.sidebar_background, "background-sidebar"),
            "bar" => (&mut self.bar_background, "background-bar"),
            _ => return None,
        })
    }
}

impl Default for Background {
    fn default() -> Self {
        Background { kind: String::new(), name: String::new(), show: 0.25, fit: "fill".into(), stamp: 0 }
    }
}

/// A terminal highlight rule: a regular expression (Rust syntax, `(?i)` for any case) and the
/// colour (`#rrggbb`) its matches are drawn in.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Highlight {
    pub pattern: String,
    pub color: String,
}

impl Highlight {
    pub fn defaults() -> Vec<Highlight> {
        let h = |pattern: &str, color: &str| Highlight { pattern: pattern.into(), color: color.into() };
        vec![
            h(r"(?i)\b(error|err|fatal|fail(ed|ure)?|crit(ical)?|panic|emerg(ency)?|denied|refused)\b", "#ef5f6b"),
            h(r"(?i)\b(warn(ing)?|deprecated|timeout|timed out)\b", "#e6c472"),
            h(r"(?i)\b(ok|success(ful)?|pass(ed)?|done|ready|started|running)\b", "#7fd88f"),
            h(r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d{1,5}|/\d{1,2})?\b", "#5fa8f5"),
            // IPv6: in full, or shortened with "::" (2001:db8::1).
            h(r"\b(?:[0-9a-fA-F]{1,4}:){3,7}[0-9a-fA-F]{1,4}\b|\b(?:[0-9a-fA-F]{1,4}:)+:(?:[0-9a-fA-F]{1,4}(?::[0-9a-fA-F]{1,4})*)?", "#5fa8f5"),
        ]
    }

    /// The rules that work, ready to match; none while highlighting is off. Each colour is made
    /// to read on the terminal's background `bg` (`readable`), so one rule serves dark and light
    /// schemes.
    pub fn compile(on: bool, rules: &[Highlight], bg: u32) -> Vec<(regex::Regex, u32)> {
        if !on {
            return vec![];
        }
        rules
            .iter()
            .filter_map(|h| {
                let c = u32::from_str_radix(h.color.strip_prefix('#')?, 16).ok().filter(|_| h.color.len() == 7)?;
                Some((regex::RegexBuilder::new(&h.pattern).size_limit(1 << 20).build().ok()?, readable(c, bg)))
            })
            .collect()
    }
}

/// `c` (0xRRGGBB) made to hold 4.5:1 (WCAG AA for text) on `bg`: darkened on a light background,
/// lightened on a dark one, in OKLCH so it keeps its hue and strength rather than going grey. A
/// colour that already reads is kept as it is, so a rule made for dark terminals is unchanged
/// there and still reads on a white one. A yellow can't be dark and still look yellow (it turns
/// olive), so on a light background it takes the light theme's warning orange (`--warn`) instead.
pub fn readable(c: u32, bg: u32) -> u32 {
    if contrast(c, bg) >= 4.5 {
        return c;
    }
    let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let gam = |v: f64| if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    let [r, g, b] = [16, 8, 0].map(|s| lin(((c >> s) & 0xff) as f64 / 255.0));
    let [l, m, s] = [
        0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b,
        0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b,
        0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b,
    ]
    .map(f64::cbrt);
    let lightness = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    let (mut chroma, mut hue) = (a.hypot(bb), bb.atan2(a).to_degrees().rem_euclid(360.0));
    let light = luminance(bg) > 0.179;
    // A grey's hue is rounding noise, often in the yellows: only a real yellow turns orange.
    if light && chroma >= 0.05 && (60.0..115.0).contains(&hue) {
        // `--warn`'s hue and strength on the light theme (#a34800)
        (hue, chroma) = (48.5, chroma.max(0.138));
    }
    let (ca, sa) = (hue.to_radians().cos(), hue.to_radians().sin());
    for i in 1..=200 {
        let lt = lightness + if light { -0.005 } else { 0.005 } * i as f64;
        if !(0.0..=1.0).contains(&lt) {
            break;
        }
        // Lose strength, not hue, where the colour falls outside what a screen shows.
        let mut k = chroma;
        let rgb = loop {
            let (a, b) = (k * ca, k * sa);
            let [l, m, s] = [
                lt + 0.3963377774 * a + 0.2158037573 * b,
                lt - 0.1055613458 * a - 0.0638541728 * b,
                lt - 0.0894841775 * a - 1.2914855480 * b,
            ]
            .map(|v| v * v * v);
            let rgb = [
                4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
            ];
            if rgb.iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v)) || k < 0.001 {
                break rgb;
            }
            k *= 0.97;
        };
        let out = rgb.iter().fold(0, |o, v| o << 8 | (gam(v.clamp(0.0, 1.0)) * 255.0).round() as u32);
        if contrast(out, bg) >= 4.5 {
            return out;
        }
    }
    if light { 0 } else { 0xffffff }
}

/// A colour's relative luminance (WCAG).
fn luminance(c: u32) -> f64 {
    let ch = |s: u32| {
        let v = ((c >> s) & 0xff) as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/// The WCAG contrast ratio of two colours.
fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct CustomIcon {
    pub name: String,
    /// A `data:image/…` URL.
    pub data: String,
}

/// Broadcast outline styles (ui/app.css `.outline`); "off" draws none.
pub const OUTLINES: &[&str] = &["ring", "breathe", "fade", "static", "march", "rainbow", "off"];

/// How a background picture fits its place (ui/lib/Backdrop.svelte): "fill" covers it, cut to
/// fit; "stretch" covers it, out of proportion; "fit" shows all of it; "tile" repeats it at its
/// size; "center" shows it once at its size.
pub const FITS: &[&str] = &["fill", "stretch", "fit", "tile", "center"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            ui_scale: 1.0,
            effect: Effect::None,
            opacity: 0.95,
            scheme: "Auto".into(),
            font: String::new(),
            font_size: 14.0,
            override_sessions: true,
            cursor: Cursor::Block,
            cursor_blink: true,
            line_height: 1.0,
            pane_padding: 5,
            scrollbar: true,
            highlight: true,
            highlights: Highlight::defaults(),
            background: Background::default(),
            sidebar_background: Background::default(),
            bar_background: Background::default(),
            scrollback: 20_000,
            scroll_on_output: false,
            copy_on_select: true,
            right_click_paste: true,
            strip_paste: true,
            log_to_file: false,
            web: Web::External,
            shell: String::new(),
            ssh_user: String::new(),
            ssh_key: String::new(),
            ssh_password: String::new(),
            ssh_ciphers: crate::ssh::default_ciphers(),
            ssh_keepalive: 30,
            ssh_compression: false,
            rdp_quality: 0,
            rdp_clipboard: 3,
            rdp_keyboard: true,
            smb_auto_login: true,
            automation: false,
            automation_port: 8721,
            automation_network: false,
            automation_token: String::new(),
            sync: false,
            sync_token: String::new(),
            sync_email: String::new(),
            sync_pro: false,
            sync_settings: false,
            sync_backgrounds: false,
            sync_logins: false,
            account_button: true,
            coop_button: true,
            coop_quality: Quality::Average,
            coop_idle: 120,
            confirm_close: true,
            auto_reconnect: true,
            restore_tabs: false,
            status_bar: true,
            single_click_open: false,
            quick_shell: true,
            start_shell: String::new(),
            remember_folders: true,
            open_folders: vec![],
            quiet_hints: vec![],
            language: String::new(),
            sidebar_width: 280,
            sidebar_collapsed: false,
            guide: true,
            guide_done: vec![],
            shortcuts: BTreeMap::new(),
            pane_step: 21,
            keep_running: true,
            start_at_login: false,
            notifications: false,
            update_notify: true,
            high_priority: false,
            custom_css: String::new(),
            broadcast_outline: "static".into(),
            broadcast_outline_color: String::new(),
            focus_animated: true,
            accent: String::new(),
            app_icon: String::new(),
            logo_link: String::new(),
            custom_schemes: vec![],
            custom_icons: vec![],
            workspace: String::new(),
        }
    }
}

impl Settings {
    /// Stored encrypted in `settings.xtm`; unreadable or missing settings fall back to defaults.
    pub fn load() -> Self {
        let text = crate::config::read("settings", &["settings.toml"], |t| Ok(t.concat()));
        text.ok().and_then(|s| Self::parse(&s)).unwrap_or_default()
    }

    /// A settings file's text, cleaned: an older version may have saved it under wider limits.
    /// No text at all is a new install: it gets the defaults, not an older file's missing fields.
    fn parse(text: &str) -> Option<Self> {
        if text.trim().is_empty() {
            return None;
        }
        let mut settings: Self = toml::from_str(text).ok()?;
        settings.sanitize();
        Some(settings)
    }

    /// Makes the automation token if there is none yet; true if it did. It's made once and kept
    /// from then on, through restarts and upgrades, until "New token" replaces it.
    pub fn ensure_token(&mut self) -> bool {
        if !self.automation_token.is_empty() {
            return false;
        }
        self.automation_token = crate::vault::random::<32>().iter().map(|b| format!("{b:02x}")).collect();
        true
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::config::write("settings", &toml::to_string_pretty(self)?)
    }

    /// Whether open terminals need restyling after a change from `old`.
    pub fn restyles(&self, old: &Settings) -> bool {
        let key = |s: &Settings| {
            (
                (s.scheme.clone(), s.font.clone(), s.font_size, s.override_sessions, s.cursor, s.cursor_blink),
                (s.line_height, s.scrollback, s.scroll_on_output, s.copy_on_select, s.right_click_paste, s.strip_paste),
            )
        };
        key(self) != key(old)
            || self.custom_schemes != old.custom_schemes
            || (self.highlight, &self.highlights) != (old.highlight, &old.highlights)
    }

    /// Copies the fields that belong to this device from `from`: the account and what it syncs,
    /// automation, start at login and priority (a synced copy must not open a listener or change
    /// the OS here), the default SSH password (passwords travel only with the saved logins), the
    /// window's layout, the kept custom.css, and what each background is (its own file).
    fn take_device_fields(&mut self, from: &Settings) {
        self.sync = from.sync;
        self.sync_token = from.sync_token.clone();
        self.sync_email = from.sync_email.clone();
        self.sync_pro = from.sync_pro;
        (self.sync_settings, self.sync_backgrounds, self.sync_logins) = (from.sync_settings, from.sync_backgrounds, from.sync_logins);
        self.automation = from.automation;
        self.automation_port = from.automation_port;
        self.automation_network = from.automation_network;
        self.automation_token = from.automation_token.clone();
        self.workspace = from.workspace.clone();
        self.start_at_login = from.start_at_login;
        self.high_priority = from.high_priority;
        self.ssh_password = from.ssh_password.clone();
        self.custom_css = from.custom_css.clone();
        self.open_folders = from.open_folders.clone();
        self.sidebar_width = from.sidebar_width;
        self.sidebar_collapsed = from.sidebar_collapsed;
        (self.guide, self.guide_done) = (from.guide, from.guide_done.clone());
        for (b, f) in [(&mut self.background, &from.background), (&mut self.sidebar_background, &from.sidebar_background), (&mut self.bar_background, &from.bar_background)] {
            (b.kind, b.name, b.stamp) = (f.kind.clone(), f.name.clone(), f.stamp);
        }
    }

    /// The copy of the settings that goes to the cloud when "Settings" is ticked: this device's
    /// own fields (`take_device_fields`) at their defaults, so it reads the same from any device.
    pub fn for_cloud(&self) -> Settings {
        let mut s = self.clone();
        s.take_device_fields(&Settings::default());
        s
    }

    /// A copy from the cloud (`for_cloud` of another device) as it applies here: this device's
    /// fields stay as `mine` has them. `for_cloud` of the result is the copy it came from, so
    /// pulling never looks like a change of this device's own.
    pub fn applied_over(mut self, mine: &Settings) -> Settings {
        self.take_device_fields(mine);
        self
    }

    /// The scheme in effect, resolving "Auto" against the current light/dark mode.
    pub fn scheme(&self, dark: bool) -> &Scheme {
        let name = if self.scheme == "Auto" {
            if dark { "xtermium Dark" } else { "xtermium Light" }
        } else {
            self.scheme.as_str()
        };
        SCHEMES.iter().chain(&self.custom_schemes).find(|s| s.name == name).unwrap_or(&SCHEMES[0])
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Scheme {
    pub name: Cow<'static, str>,
    pub bg: u32,
    pub fg: u32,
    pub cursor: u32,
    pub selection: u32,
    pub ansi: [u32; 16],
}

pub fn rgb(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

pub const SCHEMES: &[Scheme] = &[
    Scheme {
        // The brand's: the website's ink on its near-black, a phosphor-amber block cursor and
        // selection; the greys are neutral, the colours as before.
        name: Cow::Borrowed("xtermium Dark"),
        bg: 0x0b0b0d,
        fg: 0xe6e6e1,
        cursor: 0xffb000,
        selection: 0x4a3608,
        ansi: [
            0x1d1d22, 0xef5f6b, 0x7fd88f, 0xe6c472, 0x5fa8f5, 0xc792ea, 0x56c8d8, 0xc9c9c4, //
            0x50505a, 0xff7a85, 0x98e6a6, 0xf2d48f, 0x82bdff, 0xd8a8f5, 0x79dbe8, 0xffffff,
        ],
    },
    Scheme {
        name: Cow::Borrowed("xtermium Light"),
        bg: 0xfafafa,
        fg: 0x141414,
        cursor: 0xffb000,
        selection: 0xf6e3b4,
        // Every colour but white holds 4.5:1 on the background, darkened in OKLCH so it keeps its hue
        // (`readable`); the yellows are orange, since a yellow dark enough to read turns mustard.
        ansi: [
            0x2b2b2e, 0xd1343f, 0x218433, 0x9c5100, 0x1f62c6, 0x8e3fb0, 0x0f7f8c, 0x9d9da6, //
            0x6e6e76, 0xd53840, 0x028536, 0xb5591e, 0x3070d4, 0x9d55bd, 0x027e8b, 0x4f4f57,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Dracula"),
        bg: 0x282a36,
        fg: 0xf8f8f2,
        cursor: 0xf8f8f2,
        selection: 0x44475a,
        ansi: [
            0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, //
            0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Solarized Dark"),
        bg: 0x002b36,
        fg: 0x839496,
        cursor: 0x93a1a1,
        selection: 0x073642,
        ansi: [
            0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, //
            0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Solarized Light"),
        bg: 0xfdf6e3,
        fg: 0x657b83,
        cursor: 0x586e75,
        selection: 0xeee8d5,
        ansi: [
            0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, //
            0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Gruvbox Dark"),
        bg: 0x282828,
        fg: 0xebdbb2,
        cursor: 0xebdbb2,
        selection: 0x504945,
        ansi: [
            0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, //
            0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Nord"),
        bg: 0x2e3440,
        fg: 0xd8dee9,
        cursor: 0xd8dee9,
        selection: 0x434c5e,
        ansi: [
            0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, //
            0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
        ],
    },
    Scheme {
        name: Cow::Borrowed("One Dark"),
        bg: 0x282c34,
        fg: 0xabb2bf,
        cursor: 0x528bff,
        selection: 0x3e4451,
        ansi: [
            0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, //
            0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Tokyo Night"),
        bg: 0x1a1b26,
        fg: 0xc0caf5,
        cursor: 0xc0caf5,
        selection: 0x33467c,
        ansi: [
            0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, //
            0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
        ],
    },
    Scheme {
        name: Cow::Borrowed("Catppuccin Mocha"),
        bg: 0x1e1e2e,
        fg: 0xcdd6f4,
        cursor: 0xf5e0dc,
        selection: 0x45475a,
        ansi: [
            0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, //
            0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Highlight colours read on a white terminal and stay as chosen on a dark one.
    #[test]
    fn highlight_colours_read_on_any_terminal() {
        for h in Highlight::defaults() {
            let c = u32::from_str_radix(&h.color[1..], 16).unwrap();
            assert_eq!(readable(c, 0x0b0b0d), c, "{} changed on dark", h.color);
            let on_white = readable(c, 0xffffff);
            assert!(contrast(on_white, 0xffffff) >= 4.5, "{} is {on_white:06x} on white", h.color);
            assert!(contrast(on_white, 0xffffff) < 5.5, "{} darkened further than it needs", h.color);
        }
        // The warning yellow turns orange on white, not olive: much more red than green.
        let w = readable(0xe6c472, 0xffffff);
        assert!(w >> 16 > (w >> 8 & 0xff) + 0x40, "warning on white is {w:06x}");
        // Too dark for a dark terminal: lightened instead.
        assert!(contrast(readable(0x202060, 0x0b0b0d), 0x0b0b0d) >= 4.5);
    }

    /// Text in any of the light scheme's colours reads on its background (white, slot 7, is for
    /// backgrounds).
    #[test]
    fn light_scheme_colours_read() {
        let light = SCHEMES.iter().find(|s| s.name == "xtermium Light").unwrap();
        for (i, c) in light.ansi.iter().enumerate().filter(|(i, _)| *i != 7) {
            assert!(contrast(*c, light.bg) >= 4.5, "colour {i} ({c:06x}) is {:.2}:1", contrast(*c, light.bg));
        }
        assert!(contrast(light.fg, light.bg) >= 4.5);
    }

    /// A grey highlight rule (dim timestamps, say) darkens on a light terminal but stays grey:
    /// only a real yellow turns orange.
    #[test]
    fn grey_highlights_stay_grey_on_light() {
        for grey in [0x888888, 0x999999, 0xaaaaaa, 0xb0b0b0, 0x8a8a80] {
            let c = readable(grey, 0xfafafa);
            let [r, g, b] = [16, 8, 0].map(|s| ((c >> s) & 0xff) as i32);
            assert!((r - b).abs() <= 12 && (r - g).abs() <= 12, "{grey:06x} became {c:06x}");
            assert!(contrast(c, 0xfafafa) >= 4.5, "{grey:06x} became {c:06x}");
        }
        let yellow = readable(0xe0c040, 0xfafafa);
        let (r, g) = ((yellow >> 16) as i64, ((yellow >> 8) & 0xff) as i64);
        assert!(r - g > 40, "a yellow turns orange: {yellow:06x}");
    }

    /// A new install (no settings file yet) starts with the getting-started list; a file saved
    /// before it existed doesn't.
    #[test]
    fn a_new_install_gets_the_guide() {
        assert!(Settings::parse("").is_none(), "no file: `load` takes the defaults");
        assert!(Settings::default().guide);
        assert!(!Settings::parse("sidebar_collapsed = false").unwrap().guide);
    }

    /// Files saved by older versions keep their window see-through, at their window opacity, raised
    /// to the lowest one that's still readable.
    #[test]
    fn older_effects_and_opacities_load() {
        for effect in ["Acrylic", "Mica", "Vibrancy", "Transparent"] {
            for (saved, loaded) in [(0.9, 0.9), (0.6, 0.8)] {
                let text = format!("effect = \"{effect}\"\nwindow_opacity = {saved}\nterminal_opacity = 0.4\n");
                let s = Settings::parse(&text).unwrap();
                assert_eq!(s.effect, Effect::Transparent);
                assert_eq!(s.opacity, loaded);
            }
        }
    }
}
