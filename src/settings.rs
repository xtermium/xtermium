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
    /// See-through, no blur (works wherever the compositor supports transparency).
    Transparent,
    /// Windows 10/11 blurred backdrop.
    Acrylic,
    /// Windows 11 wallpaper-tinted backdrop.
    Mica,
    /// macOS blurred backdrop.
    Vibrancy,
}

impl Effect {
    pub fn available() -> &'static [Effect] {
        if cfg!(windows) {
            &[Effect::None, Effect::Transparent, Effect::Acrylic, Effect::Mica]
        } else if cfg!(target_os = "macos") {
            &[Effect::None, Effect::Transparent, Effect::Vibrancy]
        } else {
            &[Effect::None, Effect::Transparent]
        }
    }
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub ui_scale: f32,
    pub effect: Effect,
    /// Opacity of the window chrome (sidebar, bars, dialogs) when an effect is on.
    pub window_opacity: f32,
    /// Opacity of the terminal background when an effect is on.
    pub terminal_opacity: f32,
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
    /// The account button at the bottom of the sidebar; always shown while signed in.
    pub account_button: bool,
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
    /// The outline on terminals broadcast types into: a style from `OUTLINES` and its colour
    /// (once `multi_outline`, from when broadcast was called multi-exec).
    #[serde(alias = "multi_outline")]
    pub broadcast_outline: String,
    #[serde(alias = "multi_outline_color")]
    pub broadcast_outline_color: String,
    /// The focused pane's ring glides to the next pane when the focus moves; false: it jumps.
    pub focus_animated: bool,
    /// The interface's accent colour (`#rrggbb`); "" = the built-in gold.
    pub accent: String,
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

    /// The rules that work, ready to match; none while highlighting is off.
    pub fn compile(on: bool, rules: &[Highlight]) -> Vec<(regex::Regex, u32)> {
        if !on {
            return vec![];
        }
        rules
            .iter()
            .filter_map(|h| {
                let c = u32::from_str_radix(h.color.strip_prefix('#')?, 16).ok().filter(|_| h.color.len() == 7)?;
                Some((regex::RegexBuilder::new(&h.pattern).size_limit(1 << 20).build().ok()?, c))
            })
            .collect()
    }
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
            window_opacity: 0.82,
            terminal_opacity: 0.88,
            scheme: "Auto".into(),
            font: String::new(),
            font_size: 14.0,
            override_sessions: true,
            cursor: Cursor::Block,
            cursor_blink: true,
            line_height: 1.0,
            pane_padding: 6,
            scrollbar: true,
            highlight: true,
            highlights: Highlight::defaults(),
            background: Background::default(),
            sidebar_background: Background::default(),
            bar_background: Background::default(),
            scrollback: 10_000,
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
            smb_auto_login: true,
            automation: false,
            automation_port: 8721,
            automation_network: false,
            automation_token: String::new(),
            sync: false,
            sync_token: String::new(),
            sync_email: String::new(),
            account_button: false,
            confirm_close: true,
            auto_reconnect: true,
            restore_tabs: false,
            status_bar: true,
            single_click_open: false,
            remember_folders: true,
            open_folders: vec![],
            quiet_hints: vec![],
            language: String::new(),
            sidebar_width: 280,
            sidebar_collapsed: false,
            shortcuts: BTreeMap::new(),
            pane_step: 21,
            keep_running: true,
            start_at_login: false,
            notifications: false,
            update_notify: true,
            high_priority: false,
            custom_css: String::new(),
            broadcast_outline: "ring".into(),
            broadcast_outline_color: "#9aa3ad".into(),
            focus_animated: true,
            accent: String::new(),
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
        text.ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
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
                (s.line_height, s.scrollback, s.scroll_on_output, s.effect, s.terminal_opacity, s.copy_on_select, s.right_click_paste, s.strip_paste),
            )
        };
        key(self) != key(old)
            || self.custom_schemes != old.custom_schemes
            || (self.highlight, &self.highlights) != (old.highlight, &old.highlights)
    }

    pub fn transparent(&self) -> bool {
        self.effect != Effect::None
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
        name: Cow::Borrowed("xtermium Dark"),
        bg: 0x101114,
        fg: 0xd4d7dd,
        cursor: 0x5fa8f5,
        selection: 0x2d4a6e,
        ansi: [
            0x1d1f24, 0xef5f6b, 0x7fd88f, 0xe6c472, 0x5fa8f5, 0xc792ea, 0x56c8d8, 0xc8ccd4, //
            0x4b505a, 0xff7a85, 0x98e6a6, 0xf2d48f, 0x82bdff, 0xd8a8f5, 0x79dbe8, 0xffffff,
        ],
    },
    Scheme {
        name: Cow::Borrowed("xtermium Light"),
        bg: 0xfafafa,
        fg: 0x2b2f36,
        cursor: 0x1f62c6,
        selection: 0xc7dbf7,
        ansi: [
            0x2b2f36, 0xd1343f, 0x2f8f3e, 0xa66a00, 0x1f62c6, 0x8e3fb0, 0x0f7f8c, 0x9aa0a8, //
            0x6b7280, 0xe5484d, 0x3aa655, 0xb88600, 0x3b7be0, 0xa35bc4, 0x1495a3, 0x4b5563,
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
