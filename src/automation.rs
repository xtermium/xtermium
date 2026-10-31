//! Opt-in local automation (Settings > MCP): an MCP server on 127.0.0.1 for AI assistants
//! and scripts; the `xtermium` command line (cli.rs) calls the same tools through it.
//!
//! Every request must carry the token from the settings (`Authorization: Bearer …`). Requests
//! from web pages (an `Origin` that isn't local) and requests under another host name (DNS
//! rebinding) are refused, so a browser can't drive it even with the port known.
//!
//! On the network (Settings > MCP > Listen on) it speaks HTTPS, with a self-signed
//! certificate made once; plain HTTP is still taken from this computer (the command line, agent
//! tabs), and refused from anywhere else.

use crate::rdp::{Input, Picture};
use crate::{App, Tab};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use wezterm_term::{KeyCode, KeyModifiers};

/// The tools, as MCP's `tools/list` describes them.
pub fn tools() -> Value {
    let none = json!({ "type": "object", "properties": {} });
    let tab = json!({ "type": "integer", "description": "Tab id, from `tabs` or `open`" });
    let lines = json!({ "type": "integer", "description": "How many lines at most (default 50, at most 10000)" });
    let name = json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] });
    let int = |d: &str| json!({ "type": "integer", "description": d });
    let button = json!({ "type": "string", "enum": ["left", "right", "middle"], "description": "Default left" });
    json!([
        { "name": "sessions", "description": "List the saved sessions: name, folder and connection details, each in the shape session_save takes.", "inputSchema": none },
        {
            "name": "session_save",
            "description": SESSION_SAVE,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session": { "type": "object", "description": "The session (a new one), or the fields to change (an existing one, found by folder and name; `rename` to rename it, `move` to move it)" },
                    "rename": { "type": "string", "description": "A new name for an existing session" },
                    "move": { "type": "string", "description": "The folder to move an existing session to (\"Prod/Web\", \"\" for the top level)" }
                },
                "required": ["session"]
            }
        },
        {
            "name": "session_delete",
            "description": "Delete a saved session.",
            "inputSchema": { "type": "object", "properties": { "session": { "type": "string", "description": "Session name, or folder/name" } }, "required": ["session"] }
        },
        { "name": "logins", "description": "List the shared logins (id, name, user, whether they hold a password or key, their SSH gateway): a session's `credential` is a login's id. `saved`: the hosts a password was remembered for when asked (\"user@host:port\" for SSH, \"rdp:…\", \"vnc:…\"). Passwords are never shown. Both are the workspace in use's.", "inputSchema": none },
        {
            "name": "workspaces",
            "description": "List the workspaces and the one in use. Each has its own sessions, folders, logins, tunnels and snippets: `sessions`, `logins`, `tunnels`, `open` and `session_save` work in the one in use. \"\" is the default workspace.",
            "inputSchema": none
        },
        {
            "name": "workspace_switch",
            "description": "Switch to another workspace (\"\" for the default one). Open tabs stay open.",
            "inputSchema": { "type": "object", "properties": { "name": { "type": "string", "description": "Workspace name, \"\" for the default" } }, "required": ["name"] }
        },
        {
            "name": "workspace_clone",
            "description": "Copy a workspace's sessions and folders to a new one named \"<name> (clone)\" (shared and saved logins, tunnels, snippets and layouts are every workspace's already). Returns the new name.",
            "inputSchema": { "type": "object", "properties": { "name": { "type": "string", "description": "Workspace name, \"\" for the default" } }, "required": ["name"] }
        },
        {
            "name": "workspace_sync",
            "description": "Bring the sessions and folders of one workspace into another: what both have is updated, what's new is added, nothing is removed. A restore point is kept first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Workspace name, \"\" for the default" },
                    "to": { "type": "string", "description": "Workspace name, \"\" for the default" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "move_to_workspace",
            "description": "Move saved sessions, and folders with everything in them, from the workspace in use to another. Open tabs stay open.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Workspace name, \"\" for the default" },
                    "sessions": { "type": "array", "items": { "type": "string" }, "description": "Session names, or folder/name" },
                    "folders": { "type": "array", "items": { "type": "string" }, "description": "Folder paths (\"Prod/Web\")" }
                },
                "required": ["to"]
            }
        },
        { "name": "restore_points", "description": "List the restore points: full copies of xtermium's data kept on this computer (one a day, one before each restore, and ones taken on request; the newest 10). Restoring one is done in Settings > Import and export.", "inputSchema": none },
        { "name": "restore_point_take", "description": "Keep a restore point of everything now.", "inputSchema": none },
        { "name": "tabs", "description": "List the open tabs: id, session name, type, whether connected, and the title the program or page set (websites and BMC consoles: their address too). Tabs moved to a window of their own have \"window\": \"detached\".", "inputSchema": none },
        { "name": "layouts", "description": "List the saved layouts of the workspace in use: name, pane arrangement and the sessions in its panes.", "inputSchema": none },
        { "name": "layout_open", "description": "Open a saved layout: each of its sessions back in the pane it was in, in its arrangement, with its tab name and the panes' sizes. Tabs already open stay open.", "inputSchema": name },
        { "name": "detach", "description": "Move a terminal or RDP tab to a window of its own; its session goes on.", "inputSchema": { "type": "object", "properties": { "tab": tab }, "required": ["tab"] } },
        { "name": "attach", "description": "Put a detached tab back in the main window.", "inputSchema": { "type": "object", "properties": { "tab": tab }, "required": ["tab"] } },
        {
            "name": "open",
            "description": "Open a tab and return its id once it's open. Give one of: `session` (a saved session), `shell` (a local shell: a program with arguments, or \"\" for the default one; a scratch terminal of your own), `url` (a website tab at an http(s) address, browsing privately; drive it with the web_ tools), or `address` (connect without a saved session: \"user@host\", \"ssh user@host:2222\", \"rdp://host\", \"vnc host:5901\", \"telnet switch1\").",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session": { "type": "string", "description": "Session name, or folder/name" },
                    "shell": { "type": "string", "description": "Program and arguments; \"\" for the default shell" },
                    "dir": { "type": "string", "description": "With shell: the folder it starts in (default: home)" },
                    "url": { "type": "string", "description": "An http(s) address for a website tab" },
                    "address": { "type": "string" }
                }
            }
        },
        { "name": "close", "description": "Close a tab, ending its session.", "inputSchema": { "type": "object", "properties": { "tab": tab }, "required": ["tab"] } },
        {
            "name": "run",
            "description": "Type a command into a terminal tab and press Enter. Returns at once, unless: `until` (a regular expression) waits until the command's output matches it, e.g. a prompt like \"\\\\$ $\" or \"DONE\"; `wait_ms` alone waits until the output has been quiet for 300 ms. Either way at most `wait_ms` (default 30000 with `until`). The output returned is the command's own: from its line on.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab": tab, "command": { "type": "string" },
                    "until": { "type": "string", "description": "Regular expression to wait for in the command's output" },
                    "wait_ms": int("How long to wait at most (ms); 0 without `until`: return at once"),
                    "lines": lines
                },
                "required": ["tab", "command"]
            }
        },
        {
            "name": "read",
            "description": "The last lines of a terminal tab (screen and scrollback) as plain text, with the terminal's size and cursor position.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "lines": lines }, "required": ["tab"] }
        },
        {
            "name": "wait_for",
            "description": "Wait until a terminal tab's last lines match a regular expression (output printed later, a prompt coming back). Returns the match and the lines, or matched: false after timeout_ms.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab": tab, "pattern": { "type": "string" },
                    "timeout_ms": int("Default 30000, at most 600000"),
                    "lines": lines
                },
                "required": ["tab", "pattern"]
            }
        },
        {
            "name": "type",
            "description": "Type text into a terminal or remote desktop tab, without pressing Enter (a line break in the text does press it).",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "text": { "type": "string" } }, "required": ["tab", "text"] }
        },
        {
            "name": "key",
            "description": "Press keys in a terminal or remote desktop tab: chords separated by spaces, like \"ctrl+c\", \"Enter\", \"up up Enter\", \"alt+F4\", \"ctrl+alt+Delete\", \"ctrl+shift+Escape\". Names: Enter, Tab, Escape, Backspace, Delete, Insert, Home, End, PageUp, PageDown, Up, Down, Left, Right, Space, F1–F12, a letter or digit; modifiers ctrl, alt, shift, meta (win/cmd). A modifier alone taps it: \"meta\" opens the Start menu.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "keys": { "type": "string" } }, "required": ["tab", "keys"] }
        },
        {
            "name": "screenshot",
            "description": "A picture of a remote desktop (RDP, VNC or BMC console) tab as it is now, or of a website tab's page (Windows; act on a page with web_click and web_fill, not x and y). Scaled down to max_width (default 1568); the result says the desktop's size and the scale, and every tool's x and y are desktop pixels: divide what you measure on the picture by `scale`. `region` takes part of the desktop (desktop pixels) at full detail. With `save`, the PNG is written to that path instead of returned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab": tab,
                    "max_width": int("Widest the picture gets, in pixels (default 1568)"),
                    "region": { "type": "object", "properties": { "x": { "type": "integer" }, "y": { "type": "integer" }, "width": { "type": "integer" }, "height": { "type": "integer" } }, "required": ["x", "y", "width", "height"] },
                    "save": { "type": "string", "description": "Absolute path for a .png file" }
                },
                "required": ["tab"]
            }
        },
        {
            "name": "click",
            "description": "Click in a tab. Remote desktops: x and y are desktop pixels. Terminals: x is the column and y the row on screen (from 0), for programs that take the mouse (htop, mc, vim).",
            "inputSchema": {
                "type": "object",
                "properties": { "tab": tab, "x": int("Desktop pixel, or terminal column"), "y": int("Desktop pixel, or terminal row"), "button": button, "double": { "type": "boolean" } },
                "required": ["tab", "x", "y"]
            }
        },
        {
            "name": "move",
            "description": "Move the pointer over a remote desktop (hover), to x, y in desktop pixels.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "x": int("Desktop pixel"), "y": int("Desktop pixel") }, "required": ["tab", "x", "y"] }
        },
        {
            "name": "drag",
            "description": "Drag on a remote desktop: press at x, y, move to to_x, to_y, let go (desktop pixels).",
            "inputSchema": {
                "type": "object",
                "properties": { "tab": tab, "x": int(""), "y": int(""), "to_x": int(""), "to_y": int(""), "button": button },
                "required": ["tab", "x", "y", "to_x", "to_y"]
            }
        },
        {
            "name": "scroll",
            "description": "Scroll a tab up (positive `lines`) or down (negative). Terminals: by lines. Remote desktops: by mouse-wheel notches (about 3 lines each), where x, y point (desktop pixels) if given.",
            "inputSchema": {
                "type": "object",
                "properties": { "tab": tab, "lines": int("Positive up, negative down"), "x": int(""), "y": int("") },
                "required": ["tab", "lines"]
            }
        },
        {
            "name": "web_read",
            "description": "What a website tab shows: its address, title, the page's text (at most `chars`, default 8000) and the elements that take a click or text (links, buttons, fields, lists, boxes), each with a `ref` number for web_click and web_fill, its role, name, and a field's value (a password field's is hidden). Waits for a page that is loading. Read again after the page changes: elements keep their numbers while they stay on the page.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "chars": int("Most characters of text to return (default 8000, at most 200000)") }, "required": ["tab"] }
        },
        {
            "name": "web_click",
            "description": "Click an element of a website tab's page, given by one of: `ref` (from web_read), `selector` (CSS), or `text` (the name it shows, e.g. \"Sign in\"; an exact match first, else the first that contains it). Waits for a page the click opens to load.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "ref": int("Element number from web_read"), "selector": { "type": "string" }, "text": { "type": "string" } }, "required": ["tab"] }
        },
        {
            "name": "web_fill",
            "description": "Put `value` in a field of a website tab's page, as typing would (the page sees it change); for a list, the option with that value or text; for a box, \"true\" or \"false\". The field is given like web_click's: `ref`, `selector` or `text`. `submit` then sends the field's form (or presses Enter in it) and waits for the next page.",
            "inputSchema": {
                "type": "object",
                "properties": { "tab": tab, "ref": int("Element number from web_read"), "selector": { "type": "string" }, "text": { "type": "string" }, "value": { "type": "string" }, "submit": { "type": "boolean" } },
                "required": ["tab", "value"]
            }
        },
        {
            "name": "web_go",
            "description": "Take a website tab to an http(s) address, or \"back\", \"forward\" or \"reload\"; returns once the page has loaded (at most 30 s) with its address and title.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "to": { "type": "string", "description": "An http(s) address, back, forward or reload" } }, "required": ["tab", "to"] }
        },
        {
            "name": "web_eval",
            "description": "Run JavaScript in a website tab's page and return its result as JSON: `script` is a function, called with no arguments, like \"() => document.title\" or \"async () => { const r = await fetch('/api/health'); return r.status }\". An async function is awaited (at most `timeout_ms`, default 10000). The page's own sign-ins apply, as if typed in its console.",
            "inputSchema": { "type": "object", "properties": { "tab": tab, "script": { "type": "string" }, "timeout_ms": int("Default 10000, at most 120000") }, "required": ["tab", "script"] }
        },
        { "name": "tunnels", "description": "List the SSH tunnels (port forwards) and whether each is running.", "inputSchema": none },
        { "name": "tunnel_start", "description": "Start a tunnel by name.", "inputSchema": name },
        { "name": "tunnel_stop", "description": "Stop a tunnel by name.", "inputSchema": name },
    ])
}

const SESSION_SAVE: &str = "Create a saved session, or change one. A new session needs `name` and `kind`; \
`folder` (\"Prod/Web\"), `color` (#rrggbb), `icon`, `notes` and `style` are optional. An existing one \
(same folder and name) gets only the fields given, nested ones included. `kind` is one of: \
{type: \"ssh\", host, port, user, key (private key path), credential (a login id), jump: {host, port, user, key, credential} or null, \
command (typed after login), ciphers: [], keepalive (seconds) or null, compression or null, agent_forward}; \
{type: \"rdp\", host, port, user (DOMAIN\\user), quality (0 quality, 1 balanced, 2 speed) or null, credential, smb_login (true/false) or null, no_shares (true: the file panel never opens its shared folders)}; \
{type: \"vnc\", host, port, credential}; {type: \"kvm\", vendor: \"supermicro\" or \"idrac\", host, port, credential} (a BMC console); {type: \"telnet\", host, port}; {type: \"serial\", port, baud}; \
{type: \"shell\", program}; {type: \"browser\", url, credential}; {type: \"agent\", tool, args, dir}. \
null means the global setting. `style`: {font, font_size, term, fg, bg, cursor: [r, g, b] or null, palette: 16 [r, g, b] or null}. \
Passwords aren't stored in sessions: give a shared login's id as `credential` (see `logins`), or xtermium asks when it connects.";

/// `patch` laid over `base`, objects merged key by key (JSON merge patch: null removes a key,
/// which for session fields means their default).
/// The element a `web_click` or `web_fill` names: `ref`, `selector` or `text`, in that order.
fn web_target(args: &Value) -> Result<crate::web::Target, String> {
    let s = |k: &str| args.get(k).and_then(Value::as_str).filter(|v| !v.trim().is_empty()).map(String::from);
    match (args.get("ref").and_then(Value::as_u64), s("selector"), s("text")) {
        (Some(n), _, _) => Ok(crate::web::Target::Ref(n)),
        (_, Some(sel), _) => Ok(crate::web::Target::Selector(sel)),
        (_, _, Some(t)) => Ok(crate::web::Target::Named(t)),
        _ => Err("give the element: \"ref\" (from web_read), \"selector\" or \"text\"".into()),
    }
}

fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(k);
                } else {
                    merge(b.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Stores the sessions and has the window show them.
fn store_changed(app: &App, handle: &AppHandle, mut store: crate::config::Store) -> Result<(), String> {
    store.sanitize();
    *app.store.lock().unwrap() = store;
    app.save_store()?;
    let _ = handle.emit_to("main", "store", &*app.store.lock().unwrap());
    Ok(())
}

/// A key chord ("ctrl+shift+t", "Enter", "alt+F4", "a") as the terminal (`key`, a DOM key name)
/// and a remote desktop (`code`, a DOM key code; None for keys a desktop gets as typed text)
/// want it.
#[derive(Debug, PartialEq)]
struct Chord {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    key: String,
    code: Option<String>,
}

fn chord(s: &str) -> Result<Chord, String> {
    let mut c = Chord { ctrl: false, alt: false, shift: false, meta: false, key: String::new(), code: None };
    let parts: Vec<&str> = s.split('+').collect();
    // "ctrl++" is Ctrl and the plus key.
    let (mods, key) = match parts.as_slice() {
        [.., "", ""] => (&parts[..parts.len() - 2], "+"),
        [mods @ .., key] => (mods, *key),
        [] => unreachable!(),
    };
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => c.ctrl = true,
            "alt" | "option" => c.alt = true,
            "shift" => c.shift = true,
            "meta" | "win" | "super" | "cmd" => c.meta = true,
            other => return Err(format!("unknown modifier \"{other}\" in \"{s}\"")),
        }
    }
    let named = |k: &str, code: &str| (k.to_string(), Some(code.to_string()));
    (c.key, c.code) = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => named("Enter", "Enter"),
        "tab" => named("Tab", "Tab"),
        "esc" | "escape" => named("Escape", "Escape"),
        "backspace" => named("Backspace", "Backspace"),
        "delete" | "del" => named("Delete", "Delete"),
        "insert" | "ins" => named("Insert", "Insert"),
        "home" => named("Home", "Home"),
        "end" => named("End", "End"),
        "pageup" | "pgup" => named("PageUp", "PageUp"),
        "pagedown" | "pgdn" => named("PageDown", "PageDown"),
        "up" | "arrowup" => named("ArrowUp", "ArrowUp"),
        "down" | "arrowdown" => named("ArrowDown", "ArrowDown"),
        "left" | "arrowleft" => named("ArrowLeft", "ArrowLeft"),
        "right" | "arrowright" => named("ArrowRight", "ArrowRight"),
        "space" => (" ".into(), Some("Space".into())),
        // A modifier on its own: "meta" taps the Windows key (the Start menu on a desktop).
        "ctrl" | "control" => named("Control", "ControlLeft"),
        "alt" | "option" => named("Alt", "AltLeft"),
        "shift" => named("Shift", "ShiftLeft"),
        "meta" | "win" | "super" | "cmd" => named("Meta", "MetaLeft"),
        k if k.len() >= 2 && k.starts_with('f') && k[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
            named(&k.to_ascii_uppercase(), &k.to_ascii_uppercase())
        }
        _ => {
            let mut chars = key.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                return Err(format!("unknown key \"{key}\" in \"{s}\""));
            };
            let code = match ch {
                'a'..='z' | 'A'..='Z' => Some(format!("Key{}", ch.to_ascii_uppercase())),
                '0'..='9' => Some(format!("Digit{ch}")),
                _ => None,
            };
            // The terminal gets the character shifted, as a keyboard would send it.
            let k = if c.shift && ch.is_ascii_alphabetic() { ch.to_ascii_uppercase() } else { ch };
            (k.to_string(), code)
        }
    };
    Ok(c)
}

/// Remote desktop input for a chord: modifiers down, the key down and up, modifiers up.
fn chord_input(c: &Chord) -> Result<Vec<Input>, String> {
    let Some(code) = &c.code else {
        if c.ctrl || c.alt || c.meta {
            return Err(format!("\"{}\" with a modifier: use a letter, digit or named key", c.key));
        }
        return Ok(vec![Input::Text { text: c.key.clone() }]);
    };
    let mods: Vec<&str> = [(c.ctrl, "ControlLeft"), (c.alt, "AltLeft"), (c.shift, "ShiftLeft"), (c.meta, "MetaLeft")]
        .into_iter()
        .filter_map(|(on, m)| on.then_some(m))
        .collect();
    let key = |code: &str, down| Input::Key { code: code.into(), down };
    let mut v: Vec<Input> = mods.iter().map(|m| key(m, true)).collect();
    v.extend([key(code, true), key(code, false)]);
    v.extend(mods.iter().rev().map(|m| key(m, false)));
    Ok(v)
}

/// Text as terminal keystrokes: line breaks press Enter.
fn typed(text: &str) -> Vec<crate::term::Typed> {
    text.replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\n' | '\r' => crate::term::Typed::Key(KeyCode::Enter, KeyModifiers::NONE),
            c => crate::term::Typed::Key(KeyCode::Char(c), KeyModifiers::NONE),
        })
        .collect()
}

/// A picture as PNG: cropped to `region`, then scaled down to at most `max_width`. Returns the
/// PNG and what an agent needs to map the picture back to the desktop.
fn png(p: Picture, region: Option<[u32; 4]>, max_width: u32) -> Result<(Vec<u8>, Value), String> {
    use image::{DynamicImage, ImageFormat, RgbaImage, imageops};
    let (w, h) = (p.w as u32, p.h as u32);
    let mut img = RgbaImage::from_raw(w, h, p.px).ok_or("damaged picture")?;
    let [x, y, rw, rh] = region.unwrap_or([0, 0, w, h]);
    let (x, y) = (x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1)));
    let (rw, rh) = (rw.clamp(1, w - x), rh.clamp(1, h - y));
    if (x, y, rw, rh) != (0, 0, w, h) {
        img = imageops::crop_imm(&img, x, y, rw, rh).to_image();
    }
    let scale = (max_width.max(16) as f64 / rw as f64).min(1.0);
    if scale < 1.0 {
        let (sw, sh) = (((rw as f64 * scale).round() as u32).max(1), ((rh as f64 * scale).round() as u32).max(1));
        img = imageops::resize(&img, sw, sh, imageops::FilterType::Triangle);
    }
    let (iw, ih) = img.dimensions();
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(img).to_rgb8().write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    let meta = json!({
        "desktop": { "width": w, "height": h },
        "region": { "x": x, "y": y, "width": rw, "height": rh },
        "image": { "width": iw, "height": ih },
        "scale": (scale * 10_000.0).round() / 10_000.0,
        "to_desktop": "desktop x = region.x + picture x / scale; desktop y = region.y + picture y / scale",
    });
    Ok((out.into_inner(), meta))
}

/// Runs a tool. Results are JSON objects; a screenshot puts its PNG (base64) under `IMAGE`, which
/// the MCP answer turns into image content.
pub async fn call(handle: &AppHandle, tool: &str, args: &Value) -> Result<Value, String> {
    let app = handle.state::<App>();
    let text = |k: &str| args.get(k).and_then(Value::as_str).ok_or_else(|| format!("missing \"{k}\""));
    let num = |k: &str| args.get(k).and_then(Value::as_i64);
    let need = |k: &str| num(k).ok_or_else(|| format!("missing \"{k}\""));
    // AI agents' sessions and tabs don't exist for automation: an assistant can't drive another
    // (or itself) through xtermium.
    let agent = |s: &crate::Session| matches!(s.kind, crate::Kind::Agent { .. });
    // A saved session by "folder/name", else by name alone.
    let find = |store: &crate::config::Store, want: &str| -> Result<usize, String> {
        let full = |s: &crate::Session| if s.folder.is_empty() { s.name.clone() } else { format!("{}/{}", s.folder, s.name) };
        store
            .sessions
            .iter()
            .position(|s| full(s) == want && !agent(s))
            .or_else(|| store.sessions.iter().position(|s| s.name == want && !agent(s)))
            .ok_or_else(|| format!("no session named {want}"))
    };
    let agent_tab = |id: u32| matches!(app.tabs.lock().unwrap().get(&id), Some(Tab::Term(t)) if agent(&t.session));
    let tab = || {
        let id = args.get("tab").and_then(Value::as_u64).map(|t| t as u32).ok_or_else(|| "missing \"tab\"".to_string())?;
        if agent_tab(id) { Err(format!("no tab {id}")) } else { Ok(id) }
    };
    let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(50).clamp(1, 10_000) as usize;
    let button = || match args.get("button").and_then(Value::as_str).unwrap_or("left") {
        "left" => Ok(0u8),
        "middle" => Ok(1),
        "right" => Ok(2),
        b => Err(format!("unknown button {b}")),
    };
    let px = |k: &str| need(k).map(|v| v.clamp(0, u16::MAX as i64) as u16);
    // A terminal tab, for `f`.
    let term = |id: u32, f: &mut dyn FnMut(&mut crate::term::Tab) -> Result<Value, String>| match app
        .tabs
        .lock()
        .unwrap()
        .get_mut(&id)
    {
        Some(Tab::Term(t)) => f(t),
        Some(Tab::Rdp(_) | Tab::Vnc(_)) => Err("that's a remote desktop tab: use screenshot, click and key".into()),
        None => match crate::web::console(id) {
            Some(true) => Err("that's a BMC console tab: use screenshot, click and key".into()),
            Some(false) => Err("that's a website tab: use web_read, web_click, web_fill and web_go".into()),
            None => Err(format!("no tab {id}")),
        },
    };
    // Input for a remote desktop tab.
    let desktop = |id: u32, evs: Vec<Input>| -> Result<(), String> {
        match app.tabs.lock().unwrap().get(&id) {
            Some(Tab::Rdp(t)) => evs.into_iter().for_each(|e| t.input(e)),
            Some(Tab::Vnc(t)) => evs.into_iter().for_each(|e| t.input(e)),
            Some(Tab::Term(_)) => return Err("that's a terminal tab".into()),
            // A BMC console (or a website, which says it takes none).
            None => return crate::web::console_input(handle, id, &evs),
        }
        Ok(())
    };
    let is_desktop = |id: u32| match app.tabs.lock().unwrap().get(&id) {
        Some(Tab::Rdp(_) | Tab::Vnc(_)) => Ok(true),
        Some(Tab::Term(_)) => Ok(false),
        None if crate::web::console(id).is_some() => Ok(true),
        None => Err(format!("no tab {id}")),
    };
    match tool {
        "sessions" => {
            let store = app.store.lock().unwrap();
            let list: Vec<Value> =
                store.sessions.iter().filter(|s| !agent(s)).map(|s| json!({ "name": s.name, "folder": s.folder, "kind": s.kind })).collect();
            Ok(json!({ "sessions": list }))
        }
        "session_save" => {
            let patch = args.get("session").filter(|v| v.is_object()).ok_or("missing \"session\" (an object)")?;
            let name = patch.get("name").and_then(Value::as_str).ok_or("the session needs a \"name\"")?;
            let folder = crate::clean::folder(patch.get("folder").and_then(Value::as_str).unwrap_or(""));
            let mut store = app.store.lock().unwrap().clone();
            let at = store.sessions.iter().position(|s| s.name == name && s.folder == folder && !agent(s));
            let mut v = match at {
                Some(i) => serde_json::to_value(&store.sessions[i]).map_err(|e| e.to_string())?,
                None => json!({}),
            };
            merge(&mut v, patch);
            if let Some(n) = args.get("rename").and_then(Value::as_str) {
                v["name"] = json!(n);
            }
            if let Some(f) = args.get("move").and_then(Value::as_str) {
                v["folder"] = json!(f);
            }
            let session: crate::Session = serde_json::from_value(v).map_err(|e| format!("not a session: {e}"))?;
            if agent(&session) {
                return Err("AI agent sessions can't be made through automation".into());
            }
            let (name, folder) = (session.name.clone(), crate::clean::folder(&session.folder));
            if at.is_none_or(|i| store.sessions[i].name != name || store.sessions[i].folder != folder)
                && store.sessions.iter().any(|s| s.name == name && s.folder == folder)
            {
                return Err(format!("a session named {name} is already in that folder"));
            }
            match at {
                Some(i) => store.sessions[i] = session,
                None => store.sessions.push(session),
            }
            store_changed(&app, handle, store)?;
            // As stored: cleaned, defaults filled in.
            let store = app.store.lock().unwrap();
            let saved = store.sessions.iter().find(|s| s.name == name && s.folder == folder);
            Ok(json!({ "saved": saved, "created": at.is_none() }))
        }
        "session_delete" => {
            let want = text("session")?;
            let mut store = app.store.lock().unwrap().clone();
            let i = find(&store, want)?;
            store.sessions.remove(i);
            store_changed(&app, handle, store)?;
            Ok(json!({ "deleted": want }))
        }
        "logins" => {
            let store = app.store.lock().unwrap();
            let list: Vec<Value> = store
                .credentials
                .iter()
                .map(|c| {
                    let gateway = c.gateway.as_ref().map(|g| json!({ "host": g.host, "port": g.port, "user": g.user }));
                    json!({
                        "id": c.id, "name": c.name, "user": c.user,
                        "password": !c.password.is_empty(), "key": !c.key.is_empty(), "gateway": gateway
                    })
                })
                .collect();
            let ws = crate::config::workspace();
            let saved = crate::config::logins(|l| l.iter().filter(|x| x.workspace == ws).map(|x| x.id.clone()).collect::<Vec<_>>())
                .map_err(|e| format!("{e:#}"))?;
            Ok(json!({ "logins": list, "saved": saved }))
        }
        "move_to_workspace" => {
            let list = |k: &str| -> Vec<&str> { args.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect() };
            let store = app.store.lock().unwrap().clone();
            let sessions = list("sessions").into_iter().map(|want| find(&store, want)).collect::<Result<Vec<_>, _>>()?;
            let folders: Vec<String> = list("folders").into_iter().map(crate::clean::folder).collect();
            if let Some(f) = folders.iter().find(|f| f.is_empty() || !store.sessions.iter().any(|s| s.folder == **f || s.folder.starts_with(&format!("{f}/"))) && !store.folders.iter().any(|d| d.path == **f)) {
                return Err(format!("no folder named {f}"));
            }
            let moved = sessions.len() + folders.len();
            let w = crate::move_op(&app, text("to")?, &sessions, &folders)?;
            let _ = handle.emit_to("main", "workspaces", &w);
            Ok(json!({ "moved": moved, "to": text("to")? }))
        }
        "restore_points" => Ok(json!({ "restore_points": crate::snapshot::list() })),
        "restore_point_take" => {
            app.snapshot("manual")?;
            Ok(json!({ "restore_points": crate::snapshot::list() }))
        }
        "workspaces" => {
            let names: Vec<String> = std::iter::once(String::new()).chain(app.workspaces()).collect();
            Ok(json!({ "workspaces": names, "in_use": crate::config::workspace() }))
        }
        "workspace_clone" => {
            let before = app.workspaces();
            let w = crate::workspace_op(&app, crate::WorkspaceOp::Clone { name: text("name")?.into() })?;
            let _ = handle.emit_to("main", "workspaces", &w);
            Ok(json!({ "workspace": w.workspaces.iter().find(|n| !before.contains(n)) }))
        }
        "workspace_sync" => {
            let op = crate::WorkspaceOp::Sync { from: text("from")?.into(), to: text("to")?.into() };
            let w = crate::workspace_op(&app, op)?;
            let _ = handle.emit_to("main", "workspaces", &w);
            Ok(json!({ "synced": true }))
        }
        "workspace_switch" => {
            let w = crate::workspace_op(&app, crate::WorkspaceOp::Switch { name: text("name")?.into() })?;
            let _ = handle.emit_to("main", "workspaces", &w);
            Ok(json!({ "in_use": w.workspace, "sessions": w.store.sessions.iter().filter(|s| !agent(s)).count() }))
        }
        "layouts" => {
            let store = app.store.lock().unwrap();
            let list: Vec<Value> = store
                .layouts
                .iter()
                .map(|l| json!({ "name": l.name, "arrangement": l.arrangement, "sessions": l.panes.iter().map(|s| &s.name).collect::<Vec<_>>() }))
                .collect();
            Ok(json!({ "layouts": list }))
        }
        "layout_open" => {
            let name = text("name")?;
            if !app.store.lock().unwrap().layouts.iter().any(|l| l.name == name) {
                return Err(format!("no layout named {name}"));
            }
            handle.emit_to("main", "automation-layout", name).map_err(|e| e.to_string())?;
            Ok(json!({ "opened": name }))
        }
        "detach" | "attach" => {
            let id = tab()?;
            if !matches!(app.tabs.lock().unwrap().get(&id), Some(Tab::Term(_) | Tab::Rdp(_))) {
                return Err(format!("tab {id} can't move to a window of its own: terminal and RDP tabs can"));
            }
            let on = tool == "detach";
            handle.emit_to("main", "automation-detach", json!({ "tab": id, "on": on })).map_err(|e| e.to_string())?;
            Ok(json!({ "detached": on }))
        }
        "tabs" => {
            let mut list: Vec<(u32, Value)> = app
                .tabs
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, t)| !matches!(t, Tab::Term(t) if agent(&t.session)))
                .map(|(id, t)| {
                    let v = match t {
                        Tab::Term(t) => {
                            let mut v = json!({
                                "id": id, "name": t.session.name, "type": kind_name(&t.session.kind),
                                "connected": t.alive(), "title": t.title()
                            });
                            if handle.get_webview_window(&crate::detached_label(*id)).is_some() {
                                v["window"] = json!("detached");
                            }
                            v
                        }
                        Tab::Rdp(t) => {
                            let mut v = json!({ "id": id, "name": t.session.name, "type": "rdp" });
                            if handle.get_webview_window(&crate::detached_label(*id)).is_some() {
                                v["window"] = json!("detached");
                            }
                            v
                        }
                        Tab::Vnc(t) => json!({ "id": id, "name": t.session.name, "type": "vnc" }),
                    };
                    (*id, v)
                })
                .collect();
            list.extend(crate::web::listed(handle));
            list.sort_by_key(|(id, _)| *id);
            Ok(json!({ "tabs": list.into_iter().map(|(_, v)| v).collect::<Vec<_>>() }))
        }
        "open" => {
            let (session, address) = if let Ok(want) = text("session") {
                let store = app.store.lock().unwrap();
                let full = |s: &crate::Session| {
                    if s.folder.is_empty() { s.name.clone() } else { format!("{}/{}", s.folder, s.name) }
                };
                let s = store.sessions.iter().find(|s| (full(s) == want || s.name == want) && !agent(s)).cloned();
                (Some(s.ok_or_else(|| format!("no session named {want}"))?), None)
            } else if let Ok(program) = text("shell") {
                let name = if program.trim().is_empty() { "Shell" } else { program };
                let dir = args.get("dir").and_then(Value::as_str).unwrap_or_default().trim().to_string();
                if !dir.is_empty() && !std::path::Path::new(&dir).is_dir() {
                    return Err(format!("no folder {dir}"));
                }
                (Some(crate::Session::new(name, "", crate::Kind::Shell { program: program.trim().into(), dir })), None)
            } else if let Ok(url) = text("url") {
                // A website tab without a saved session: it browses privately, nothing kept.
                let u: tauri::Url = url.trim().parse().map_err(|_| format!("not a web address: {url}"))?;
                if !matches!(u.scheme(), "http" | "https") {
                    return Err("give an http(s) address".into());
                }
                let kind = crate::Kind::Browser { url: u.to_string(), credential: String::new(), profile: String::new() };
                (Some(crate::Session::new(u.host_str().unwrap_or("website"), "", kind)), None)
            } else {
                (None, Some(text("address").map_err(|_| "give a session, a shell, a url or an address".to_string())?))
            };
            let (tx, rx) = tokio::sync::oneshot::channel();
            let req = app.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            app.opens.lock().unwrap().insert(req, tx);
            handle
                .emit("automation-open", json!({ "req": req, "session": session, "address": address }))
                .map_err(|e| e.to_string())?;
            match tokio::time::timeout(Duration::from_secs(30), rx).await {
                Ok(Ok(0)) if address.is_some() => Err("not an address xtermium can connect to".into()),
                Ok(Ok(0)) => Err("that session opens outside xtermium (Settings > Websites)".into()),
                Ok(Ok(id)) => Ok(json!({ "tab": id })),
                _ => {
                    app.opens.lock().unwrap().remove(&req);
                    Err("xtermium didn't open the tab (is its window open?)".into())
                }
            }
        }
        "close" => {
            let id = tab()?;
            is_desktop(id)?;
            handle.emit("automation-close", json!({ "tab": id })).map_err(|e| e.to_string())?;
            Ok(json!({ "closed": true }))
        }
        "run" => {
            let id = tab()?;
            let mut keys = typed(text("command")?);
            keys.push(crate::term::Typed::Key(KeyCode::Enter, KeyModifiers::NONE));
            let mut from = 0;
            term(id, &mut |t| {
                if !t.alive() {
                    return Err(format!("tab {id} is disconnected"));
                }
                from = t.cursor_row();
                t.apply(&keys);
                Ok(Value::Null)
            })?;
            let until = args.get("until").and_then(Value::as_str).map(regex::Regex::new).transpose().map_err(|e| e.to_string())?;
            let default = if until.is_some() { 30_000 } else { 0 };
            let wait = Duration::from_millis(args.get("wait_ms").and_then(Value::as_u64).unwrap_or(default).min(600_000));
            if wait.is_zero() {
                return Ok(json!({ "sent": true }));
            }
            let start = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let (output, after, quiet) = match app.tabs.lock().unwrap().get(&id) {
                    // `after`: the command's output without its own line, which the pattern
                    // mustn't match.
                    Some(Tab::Term(t)) => (t.text_from(from, lines), t.text_from(from + 1, lines), t.quiet_for()),
                    _ => return Err(format!("tab {id} closed")),
                };
                let done = match &until {
                    Some(re) => re.is_match(&after),
                    // Quiet for a moment (after the command had a chance to start).
                    None => start.elapsed() > Duration::from_millis(300) && quiet > Duration::from_millis(300),
                };
                if done || start.elapsed() > wait {
                    return Ok(json!({ "output": output, "settled": done }));
                }
            }
        }
        "read" => term(tab()?, &mut |t| {
            let ((cols, rows), (cx, cy)) = t.grid();
            Ok(json!({ "text": t.text(lines), "cols": cols, "rows": rows, "cursor": { "col": cx, "row": cy } }))
        }),
        "wait_for" => {
            let id = tab()?;
            let re = regex::Regex::new(text("pattern")?).map_err(|e| e.to_string())?;
            let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(200).clamp(1, 10_000) as usize;
            let wait = Duration::from_millis(args.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30_000).min(600_000));
            let start = Instant::now();
            loop {
                let output = term(id, &mut |t| Ok(json!(t.text(lines))))?;
                let output = output.as_str().unwrap_or_default();
                if let Some(m) = re.find(output) {
                    return Ok(json!({ "matched": true, "match": m.as_str(), "output": output }));
                }
                if start.elapsed() > wait {
                    return Ok(json!({ "matched": false, "output": output }));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        "type" => {
            let (id, t) = (tab()?, text("text")?);
            if is_desktop(id)? {
                // Line breaks as Enter presses, between the lines as text.
                let mut evs = vec![];
                for (i, line) in t.replace("\r\n", "\n").split('\n').enumerate() {
                    if i > 0 {
                        evs.extend([Input::Key { code: "Enter".into(), down: true }, Input::Key { code: "Enter".into(), down: false }]);
                    }
                    if !line.is_empty() {
                        evs.push(Input::Text { text: line.into() });
                    }
                }
                desktop(id, evs)?;
            } else {
                let keys = typed(t);
                term(id, &mut |tab| {
                    tab.apply(&keys);
                    Ok(Value::Null)
                })?;
            }
            Ok(json!({ "typed": true }))
        }
        "key" => {
            let id = tab()?;
            let chords: Vec<Chord> = text("keys")?.split_whitespace().map(chord).collect::<Result<_, _>>()?;
            if chords.is_empty() {
                return Err("give keys, like \"ctrl+c\" or \"Enter\"".into());
            }
            if is_desktop(id)? {
                let evs = chords.iter().map(chord_input).collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect();
                desktop(id, evs)?;
            } else {
                term(id, &mut |t| {
                    for c in &chords {
                        let k = crate::term::Key { key: c.key.clone(), ctrl: c.ctrl, alt: c.alt, shift: c.shift, meta: c.meta };
                        if let Some(ev) = t.key(&k) {
                            t.apply(&[ev]);
                        }
                    }
                    Ok(Value::Null)
                })?;
            }
            Ok(json!({ "pressed": chords.len() }))
        }
        "screenshot" => {
            let id = tab()?;
            let pic = if crate::web::console(id) == Some(true) {
                crate::web::console_screenshot(handle, id).await?
            } else if crate::web::console(id) == Some(false) {
                crate::web::page_screenshot(handle, id).await?
            } else {
                let shot = match app.tabs.lock().unwrap().get(&id) {
                    Some(Tab::Rdp(t)) => Ok(t.screenshot()),
                    Some(Tab::Vnc(t)) => Err(t.screenshot()),
                    Some(Tab::Term(_)) => return Err("that's a terminal tab: use read for its text".into()),
                    None => return Err(format!("no tab {id}")),
                };
                match shot {
                    Ok(p) => p,
                    Err(waiting) => waiting.await,
                }
                .ok_or("the desktop isn't showing yet (still connecting?)")?
            };
            let region = args.get("region").map(|r| {
                ["x", "y", "width", "height"].map(|k| r.get(k).and_then(Value::as_u64).unwrap_or(0) as u32)
            });
            let max_width = args.get("max_width").and_then(Value::as_u64).unwrap_or(1568) as u32;
            let (png, mut meta) = tokio::task::spawn_blocking(move || png(pic, region, max_width))
                .await
                .map_err(|e| e.to_string())??;
            if let Ok(path) = text("save") {
                let path = std::path::Path::new(path);
                if !path.is_absolute() {
                    return Err("save needs an absolute path".into());
                }
                std::fs::write(path, &png).map_err(|e| format!("{}: {e}", path.display()))?;
                meta["saved"] = json!(path);
            } else {
                use base64::Engine;
                meta[IMAGE] = json!(base64::engine::general_purpose::STANDARD.encode(&png));
            }
            Ok(meta)
        }
        "click" => {
            let (id, button, double) = (tab()?, button()?, args.get("double").and_then(Value::as_bool).unwrap_or(false));
            let times = if double { 2 } else { 1 };
            if is_desktop(id)? {
                let (x, y) = (px("x")?, px("y")?);
                let mut evs = vec![Input::Move { x, y }];
                for _ in 0..times {
                    evs.extend([Input::Button { button, down: true, x, y }, Input::Button { button, down: false, x, y }]);
                }
                desktop(id, evs)?;
            } else {
                let (col, row) = (need("x")?.max(0) as usize, need("y")?.max(0) as usize);
                term(id, &mut |t| {
                    (0..times).for_each(|_| t.click_cell(col, row, button));
                    Ok(Value::Null)
                })?;
            }
            Ok(json!({ "clicked": true }))
        }
        "move" => {
            let (id, x, y) = (tab()?, px("x")?, px("y")?);
            desktop(id, vec![Input::Move { x, y }])?;
            Ok(json!({ "moved": true }))
        }
        "drag" => {
            let (id, button) = (tab()?, button()?);
            let (x0, y0, x1, y1) = (px("x")?, px("y")?, px("to_x")?, px("to_y")?);
            desktop(id, vec![Input::Move { x: x0, y: y0 }, Input::Button { button, down: true, x: x0, y: y0 }])?;
            // In steps, with time between them: apps watch the pointer travel, not just its ends.
            for i in 1..=12i32 {
                tokio::time::sleep(Duration::from_millis(16)).await;
                let at = |a: u16, b: u16| (a as i32 + (b as i32 - a as i32) * i / 12) as u16;
                desktop(id, vec![Input::Move { x: at(x0, x1), y: at(y0, y1) }])?;
            }
            desktop(id, vec![Input::Button { button, down: false, x: x1, y: y1 }])?;
            Ok(json!({ "dragged": true }))
        }
        "scroll" => {
            let (id, n) = (tab()?, need("lines")?.clamp(-1000, 1000));
            if is_desktop(id)? {
                let mut evs = vec![];
                if let (Ok(x), Ok(y)) = (px("x"), px("y")) {
                    evs.push(Input::Move { x, y });
                }
                evs.push(Input::Wheel { dx: 0.0, dy: n as f32 });
                desktop(id, evs)?;
            } else {
                term(id, &mut |t| {
                    use crate::term::{Mouse, MouseKind};
                    let m = Mouse { kind: MouseKind::Wheel, button: 0, x: 0.0, y: 0.0, lines: n as i32, ctrl: false, alt: false, shift: false, clicks: 0 };
                    t.mouse(&m);
                    Ok(Value::Null)
                })?;
            }
            Ok(json!({ "scrolled": n }))
        }
        "web_read" => {
            let chars = args.get("chars").and_then(Value::as_u64).unwrap_or(8000).clamp(100, 200_000) as usize;
            crate::web::page_read(handle, tab()?, chars).await
        }
        "web_click" => crate::web::page_click(handle, tab()?, &web_target(args)?).await,
        "web_fill" => {
            let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
            crate::web::page_fill(handle, tab()?, &web_target(args)?, text("value")?, submit).await
        }
        "web_go" => crate::web::page_go(handle, tab()?, text("to")?.trim()).await,
        "web_eval" => {
            let wait = Duration::from_millis(args.get("timeout_ms").and_then(Value::as_u64).unwrap_or(10_000).clamp(100, 120_000));
            let result = crate::web::page_run(handle, tab()?, text("script")?, wait).await?;
            Ok(json!({ "result": result }))
        }
        "tunnels" => {
            let running: HashMap<String, crate::flow::Stats> = crate::tunnel_stats(handle.state());
            let store = app.store.lock().unwrap();
            let list: Vec<Value> = store
                .tunnels
                .iter()
                .map(|t| {
                    let stats = running.get(&t.name);
                    json!({ "tunnel": t, "stats": stats })
                })
                .collect();
            Ok(json!({ "tunnels": list }))
        }
        "tunnel_start" => {
            crate::tunnel_start(handle.state(), text("name")?.into()).map(|()| json!({ "started": true }))
        }
        "tunnel_stop" => {
            crate::tunnel_stop(handle.state(), text("name")?.into());
            Ok(json!({ "stopped": true }))
        }
        _ => Err(format!("no tool named {tool}")),
    }
}

/// The running server's port and token, for agent tabs (`agent_launch`).
pub static ENDPOINT: std::sync::Mutex<Option<(u16, String)>> = std::sync::Mutex::new(None);

/// An agent tab's command line and environment: the agent's command line (`Kind::agent_argv`)
/// and, while automation runs, xtermium's MCP server, so the agent can drive xtermium whether or
/// not it was set up with it. The token goes in the environment only, never on the command line
/// or on disk. Cursor's CLI takes servers only from its mcp.json files, so it gets none here.
pub fn agent_launch(tool: &str, mut argv: Vec<String>) -> (Vec<String>, Vec<(String, String)>) {
    let Some((port, token)) = ENDPOINT.lock().unwrap().clone() else { return (argv, vec![]) };
    let url = format!("http://127.0.0.1:{port}/mcp");
    let mut env = vec![("XTERMIUM_TOKEN".to_string(), token)];
    match tool {
        // Both read `${VAR}` in a configuration file.
        "claude" | "copilot" => {
            let mut server = json!({ "type": "http", "url": url, "headers": { "Authorization": "Bearer ${XTERMIUM_TOKEN}" } });
            if tool == "copilot" {
                server["tools"] = json!(["*"]);
            }
            let config = json!({ "mcpServers": { "xtermium": server } });
            if let Some(path) = crate::config::temp().map(|t| t.join(format!("mcp-{port}.json")))
                && std::fs::write(&path, config.to_string()).is_ok()
            {
                let option = if tool == "claude" { "--mcp-config" } else { "--additional-mcp-config" };
                let at = if tool == "claude" { "" } else { "@" };
                argv.extend([option.into(), format!("{at}{}", path.display())]);
            }
        }
        "codex" => {
            let server = ["-c".to_string(), format!("mcp_servers.xtermium.url={}", json!(url)), "-c".into(), r#"mcp_servers.xtermium.bearer_token_env_var="XTERMIUM_TOKEN""#.into()];
            codex_daemon(&server, &env);
            argv.splice(1..1, server).for_each(drop);
        }
        "opencode" => env.push((
            "OPENCODE_CONFIG_CONTENT".into(),
            json!({ "mcp": { "xtermium": { "type": "remote", "url": url, "headers": { "Authorization": "Bearer {env:XTERMIUM_TOKEN}" } } } }).to_string(),
        )),
        _ => {}
    }
    (argv, env)
}

/// Codex sessions share one background server, its app-server daemon, which Codex starts by itself
/// when it's missing. The first Codex session of a run starts it from here instead, with xtermium's
/// server in its configuration and the token in its environment, so every thread on it, from any
/// terminal, has xtermium's tools; `codex app-server daemon start` does nothing when it's already up.
/// ponytail: a daemon started before a new token keeps the old one; `codex app-server daemon restart` takes the new.
fn codex_daemon(server: &[String], env: &[(String, String)]) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if cfg!(test) {
        return; // tests build command lines; they don't start anyone's daemon
    }
    STARTED.call_once(|| {
        // Through cmd on Windows: an npm-installed codex is a .cmd shim, which only cmd resolves.
        let mut cmd = if cfg!(windows) { std::process::Command::new("cmd") } else { std::process::Command::new("codex") };
        if cfg!(windows) {
            cmd.args(["/c", "codex"]);
        }
        cmd.args(["app-server", "daemon", "start"]).args(server).envs(env.iter().cloned());
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        crate::term::no_window(&mut cmd);
        // The command returns once the daemon is up; waiting for it off this thread keeps the tab's start quick.
        std::thread::spawn(move || {
            let _ = cmd.spawn().and_then(|mut c| c.wait());
        });
    });
}

/// Where a tool's result carries a PNG (base64) for the MCP answer's image content.
pub const IMAGE: &str = "image_png_base64";

fn kind_name(k: &crate::Kind) -> &'static str {
    match k {
        crate::Kind::Ssh(_) => "ssh",
        crate::Kind::Shell { .. } => "shell",
        crate::Kind::Serial { .. } => "serial",
        crate::Kind::Rdp { .. } => "rdp",
        crate::Kind::Browser { .. } => "browser",
        crate::Kind::Telnet { .. } => "telnet",
        crate::Kind::Winrm { .. } => "winrm",
        crate::Kind::Vnc { .. } => "vnc",
        crate::Kind::Kvm { .. } => "kvm",
        crate::Kind::Agent { .. } => "agent",
    }
}

/// Serves until the task is aborted (which also drops every open connection). `network`: on
/// every interface, for assistants on other machines, over HTTPS; they name this computer as
/// they please (no Host check; the token and the refusal of web pages still hold).
pub async fn serve(handle: AppHandle, port: u16, token: String, network: bool) -> std::io::Result<()> {
    let listener = TcpListener::bind((if network { "0.0.0.0" } else { "127.0.0.1" }, port)).await?;
    let tls = if network { Some(acceptor()?) } else { None };
    let mut conns = tokio::task::JoinSet::new();
    loop {
        let (sock, peer) = listener.accept().await?;
        let (handle, token, tls) = (handle.clone(), token.clone(), tls.clone());
        conns.spawn(async move {
            let Some(tls) = tls else { return converse(sock, &handle, &token, port, false).await };
            // A TLS handshake starts with 0x16; anything else is plain HTTP. A caller that says
            // nothing, or never finishes the handshake, doesn't get to hold the connection.
            let mut first = [0u8; 1];
            match tokio::time::timeout(HANDSHAKE, sock.peek(&mut first)).await.unwrap_or(Ok(0)) {
                Ok(1) if first[0] == 0x16 => {
                    if let Ok(Ok(s)) = tokio::time::timeout(HANDSHAKE, tls.accept(sock)).await {
                        converse(s, &handle, &token, port, true).await;
                    }
                }
                Ok(1) if peer.ip().is_loopback() => converse(sock, &handle, &token, port, false).await,
                Ok(1) => {
                    let body = json!({ "error": "use https://" }).to_string();
                    let mut sock = sock;
                    let reply = format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = sock.write_all(reply.as_bytes()).await;
                }
                _ => {}
            }
        });
        while conns.try_join_next().is_some() {} // reap finished connections
    }
}

/// How long a network caller may take to start talking and finish the TLS handshake.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// How long a connection may sit between requests (or take over one) before it's closed.
/// Clients open a new one when they need it.
const IDLE: Duration = Duration::from_secs(120);

/// Requests and answers over one connection (plain or TLS), until it closes or goes quiet.
async fn converse(stream: impl AsyncRead + AsyncWrite + Unpin, handle: &AppHandle, token: &str, port: u16, network: bool) {
    let (r, mut w) = tokio::io::split(stream);
    let mut r = BufReader::new(r);
    while let Ok(Some(req)) = tokio::time::timeout(IDLE, read_request(&mut r)).await {
        let (status, body) = respond(handle, &req, token, port, network).await;
        let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len());
        if w.write_all(head.as_bytes()).await.is_err() || w.write_all(body.as_bytes()).await.is_err() || w.flush().await.is_err() || req.close {
            return;
        }
    }
}

/// HTTPS with the certificate below.
fn acceptor() -> std::io::Result<tokio_rustls::TlsAcceptor> {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    let (cert, key) = certificate().map_err(std::io::Error::other)?;
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.into()], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .map_err(std::io::Error::other)?;
    Ok(tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config)))
}

/// The self-signed certificate served on the network, and its key (both DER): made the first
/// time, for this computer's names, and kept (encrypted) in `automation-cert.xtm`, so it stays
/// the same and its fingerprint can be checked. Assistants are told not to verify it.
pub fn certificate() -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    use base64::Engine;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Kept {
        cert: String,
        key: String,
    }
    // One at a time: the server and Settings asking at once mustn't make two different ones.
    static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = MAKING.lock().unwrap();
    let b64 = base64::engine::general_purpose::STANDARD;
    let text = crate::config::read("automation-cert", &[], |_| Ok(String::new()))?;
    if let Ok(k) = toml::from_str::<Kept>(&text) {
        return Ok((b64.decode(k.cert)?, b64.decode(k.key)?));
    }
    let mut names = vec!["localhost".to_string(), "127.0.0.1".into()];
    names.extend(addresses());
    let made = rcgen::generate_simple_self_signed(names)?;
    let (cert, key) = (made.cert.der().to_vec(), made.signing_key.serialize_der());
    crate::config::write("automation-cert", &toml::to_string(&Kept { cert: b64.encode(&cert), key: b64.encode(&key) })?)?;
    Ok((cert, key))
}

/// The certificate's SHA-256, as browsers and `openssl x509 -fingerprint` show it.
pub fn fingerprint(der: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(der).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// What other machines may call this one: its address on the network it reaches others through
/// (no packet is sent to find it), and its name.
pub fn addresses() -> Vec<String> {
    let ip = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()))
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback());
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .ok()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty());
    ip.map(|ip| ip.to_string()).into_iter().chain(name).collect()
}

pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Vec<u8>,
    pub(crate) close: bool,
}

/// One HTTP/1.1 request; None when the connection ends or sends something unreasonable.
pub(crate) async fn read_request(r: &mut (impl AsyncBufReadExt + Unpin)) -> Option<Request> {
    const HEAD_MAX: usize = 16 << 10;
    let mut line = String::new();
    let mut head = 0;
    let mut next = async |line: &mut String| {
        line.clear();
        // Read no further than the head may go: a line that never ends mustn't fill memory.
        let n = (&mut *r).take((HEAD_MAX - head) as u64).read_line(line).await.ok()?;
        head += n;
        (n > 0 && head < HEAD_MAX).then_some(())
    };
    next(&mut line).await?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = HashMap::new();
    loop {
        next(&mut line).await?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let len: usize = headers.get("content-length").map_or(Some(0), |v| v.parse().ok())?;
    if len > 1 << 20 {
        return None;
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).await.ok()?;
    let close = headers.get("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
    Some(Request { method, path, headers, body, close })
}

async fn respond(handle: &AppHandle, req: &Request, token: &str, port: u16, network: bool) -> (&'static str, String) {
    let error = |status, msg: &str| (status, json!({ "error": msg }).to_string());
    if !network && !local_host(req.headers.get("host").map(String::as_str), port) {
        return error("403 Forbidden", "wrong Host");
    }
    if let Some(origin) = req.headers.get("origin")
        && !local_origin(origin)
    {
        return error("403 Forbidden", "requests from web pages are not allowed");
    }
    if req.path.split('?').next() != Some("/mcp") {
        return error("404 Not Found", "the MCP endpoint is /mcp");
    }
    if req.method != "POST" {
        // No server-to-client stream: everything is request and response. Answered before the
        // token check (it says nothing), as some clients probe the endpoint without their headers.
        return error("405 Method Not Allowed", "POST JSON-RPC messages to /mcp");
    }
    let given = req.headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if token.is_empty() || !same(given.as_bytes(), token.as_bytes()) {
        return error("401 Unauthorized", "missing or wrong token (Settings > MCP)");
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
        return ("400 Bad Request", rpc_error(Value::Null, -32700, "parse error").to_string());
    };
    match rpc(handle, &msg).await {
        Some(reply) => ("200 OK", reply.to_string()),
        None => ("202 Accepted", String::new()),
    }
}

/// One JSON-RPC message; None for notifications (nothing to answer).
async fn rpc(handle: &AppHandle, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "xtermium", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Drive xtermium. Terminals: open a session, a local shell of your own or an address; run commands (with `until` to wait for their end) and read the output; send keys like ctrl+c; close tabs when done. Remote desktops (RDP, VNC, BMC consoles): take a screenshot, then click, drag, scroll, type and press keys at desktop pixel positions (divide picture positions by the screenshot's scale), and screenshot again to check. Also SSH tunnels.",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(handle, name, &args).await {
                Ok(mut v) => {
                    // A screenshot: the picture goes as image content, beside the text.
                    let image = v.as_object_mut().and_then(|o| o.remove(IMAGE));
                    let mut content = vec![json!({ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() })];
                    if let Some(data) = image {
                        content.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
                    }
                    json!({ "content": content, "structuredContent": v, "isError": false })
                }
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            }
        }
        m => return Some(rpc_error(id, -32601, &format!("unknown method {m}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn local_host(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else { return false };
    ["127.0.0.1", "localhost"].iter().any(|h| host == *h || host == format!("{h}:{port}"))
}

fn local_origin(origin: &str) -> bool {
    let rest = origin.strip_prefix("http://").unwrap_or("");
    let host = rest.split(':').next().unwrap_or("");
    host == "127.0.0.1" || host == "localhost"
}

/// Compares without stopping at the first difference (timing doesn't leak the token).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_callers() {
        assert!(local_host(Some("127.0.0.1:8721"), 8721));
        assert!(local_host(Some("localhost:8721"), 8721));
        assert!(!local_host(Some("evil.example:8721"), 8721), "DNS rebinding");
        assert!(!local_host(Some("127.0.0.1:9999"), 8721));
        assert!(!local_host(None, 8721));
        assert!(local_origin("http://localhost:3000"));
        assert!(!local_origin("https://evil.example"));
        assert!(!local_origin("http://localhost.evil.example"));
        assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab"));
    }

    #[test]
    fn key_chords() {
        let c = chord("ctrl+shift+t").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.meta);
        assert_eq!((c.key.as_str(), c.code.as_deref()), ("T", Some("KeyT")), "shifted for the terminal");
        assert_eq!(chord("Enter").unwrap().code.as_deref(), Some("Enter"));
        assert_eq!(chord("up").unwrap().key, "ArrowUp");
        assert_eq!(chord("F4").unwrap().code.as_deref(), Some("F4"));
        assert!(chord("win+r").unwrap().meta);
        let win = chord("meta").unwrap();
        assert_eq!((win.meta, win.code.as_deref()), (false, Some("MetaLeft")), "a modifier alone is a key");
        assert_eq!(chord("ctrl++").unwrap().key, "+");
        assert!(chord("hyper+a").is_err() && chord("F13").is_err() && chord("bogus").is_err());
        // Desktop: modifiers wrap the key, let go in reverse; a symbol is typed text.
        let codes: Vec<(String, bool)> = chord_input(&chord("ctrl+alt+Delete").unwrap())
            .unwrap()
            .into_iter()
            .map(|e| match e {
                Input::Key { code, down } => (code, down),
                _ => panic!("keys only"),
            })
            .collect();
        let want = [("ControlLeft", true), ("AltLeft", true), ("Delete", true), ("Delete", false), ("AltLeft", false), ("ControlLeft", false)];
        assert_eq!(codes, want.map(|(c, d)| (c.to_string(), d)));
        assert!(matches!(chord_input(&chord("/").unwrap()).unwrap()[..], [Input::Text { .. }]));
        assert!(chord_input(&chord("ctrl+/").unwrap()).is_err());
    }

    #[test]
    fn screenshots_scale_and_crop() {
        let pic = |w, h| Picture { w, h, px: vec![200; w * h * 4] };
        let (png_data, meta) = png(pic(3000, 1000), None, 1500).unwrap();
        assert_eq!(&png_data[1..4], b"PNG");
        assert_eq!((meta["image"]["width"].as_u64(), meta["image"]["height"].as_u64()), (Some(1500), Some(500)));
        assert_eq!(meta["scale"], json!(0.5));
        // A region is taken at full detail, kept inside the desktop.
        let (_, meta) = png(pic(800, 600), Some([700, 500, 400, 400]), 1568).unwrap();
        assert_eq!(meta["region"], json!({ "x": 700, "y": 500, "width": 100, "height": 100 }));
        assert_eq!(meta["scale"], json!(1.0));
    }

    #[test]
    fn session_patches_merge() {
        // A new SSH session from JSON, the way session_save gets it.
        let mut v = json!({});
        merge(&mut v, &json!({ "name": "db1", "folder": "Prod", "kind": { "type": "ssh", "host": "192.0.2.15", "user": "me", "keepalive": 30 } }));
        let s: crate::Session = serde_json::from_value(v).unwrap();
        let crate::Kind::Ssh(ssh) = &s.kind else { panic!("not ssh") };
        assert_eq!((ssh.target.host.as_str(), ssh.target.port, ssh.keepalive), ("192.0.2.15", 22, Some(30)));
        // Changing one nested field keeps the rest; null goes back to the default.
        let mut v = serde_json::to_value(&s).unwrap();
        merge(&mut v, &json!({ "kind": { "port": 2222, "keepalive": null }, "notes": "primary" }));
        let s: crate::Session = serde_json::from_value(v).unwrap();
        let crate::Kind::Ssh(ssh) = &s.kind else { panic!("not ssh") };
        assert_eq!((ssh.target.host.as_str(), ssh.target.port, ssh.keepalive), ("192.0.2.15", 2222, None));
        assert_eq!((s.folder.as_str(), s.notes.as_str()), ("Prod", "primary"));
    }

    /// Agents get xtermium's server their own way; the token stays in the environment.
    #[test]
    fn agents_get_the_server() {
        *ENDPOINT.lock().unwrap() = Some((7071, "tok".into()));
        let (argv, env) = agent_launch("claude", crate::Kind::agent_argv("claude", "--model x", "", "", ""));
        assert_eq!(argv[..4], ["claude", "--model", "x", "--mcp-config"]);
        assert!(std::path::Path::new(&argv[4]).starts_with(crate::config::dir()), "in the data folder: {}", argv[4]);
        let config = std::fs::read_to_string(&argv[4]).unwrap();
        assert!(config.contains("127.0.0.1:7071/mcp") && config.contains("${XTERMIUM_TOKEN}") && !config.contains("tok\""), "{config}");
        let _ = std::fs::remove_file(&argv[4]);
        assert_eq!(env, [("XTERMIUM_TOKEN".to_string(), "tok".to_string())]);
        let (argv, _) = agent_launch("codex", vec!["codex".into()]);
        assert_eq!(argv.first().map(String::as_str), Some("codex"));
        assert!(argv.contains(&r#"mcp_servers.xtermium.url="http://127.0.0.1:7071/mcp""#.to_string()));
        let (argv, env) = agent_launch("opencode", vec!["opencode".into()]);
        assert_eq!(argv, ["opencode"]);
        assert!(env[1].1.contains("{env:XTERMIUM_TOKEN}"));
        *ENDPOINT.lock().unwrap() = None;
        // Automation off: the agent's own command line, nothing added.
        assert_eq!(agent_launch("claude", vec!["claude".into()]).0, ["claude"]);
    }

    /// Each tool spells its mode, effort and model its own way; the user's arguments come last.
    #[test]
    fn agent_flags_follow_the_tool() {
        use crate::Kind;
        assert_eq!(
            Kind::agent_argv("claude", "--continue", "edit", "high", "opus"),
            ["claude", "--permission-mode", "acceptEdits", "--effort", "high", "--model", "opus", "--continue"]
        );
        assert_eq!(Kind::agent_argv("codex", "", "auto", "low", ""), ["codex", "--ask-for-approval", "never", "--sandbox", "workspace-write", "-c", "model_reasoning_effort=\"low\""]);
        assert_eq!(Kind::agent_argv("cursor", "", "auto", "max", "x"), ["agent", "--force", "--model", "x"], "no effort flag for a tool without one");
        assert_eq!(Kind::agent_argv("copilot", "", "", "", ""), ["copilot"], "defaults add nothing");
        assert_eq!(Kind::agent_argv("custom", "aider --model sonnet", "auto", "", ""), ["aider", "--model", "sonnet"]);
    }

    #[test]
    fn tools_have_schemas() {
        for t in tools().as_array().unwrap() {
            assert!(t["name"].is_string() && t["description"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[tokio::test]
    async fn reads_requests() {
        let raw = b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:8721\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
        let mut r = BufReader::new(&raw[..]);
        let req = read_request(&mut r).await.unwrap();
        assert_eq!(
            (req.method.as_str(), req.path.as_str(), req.body.as_slice(), req.close),
            ("POST", "/mcp", &b"{}"[..], true)
        );
        let huge = b"POST /mcp HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n";
        assert!(read_request(&mut BufReader::new(&huge[..])).await.is_none());
        // A line that never ends is cut off at the head's limit, not read to the end.
        let endless = vec![b'a'; 1 << 20];
        let mut r = BufReader::new(&endless[..]);
        assert!(read_request(&mut r).await.is_none());
        assert!(r.get_ref().len() > endless.len() - (32 << 10), "read on past the limit");
    }
}
