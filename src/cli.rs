//! `xtermium <command>`: drive the running xtermium from scripts, with JSON output. Commands call
//! the automation tools (automation.rs) on the running app, so Settings > MCP must be on;
//! the port and token come from the (encrypted) settings, so nothing extra is stored.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const USAGE: &str = "\
Drive the running xtermium (turn on Settings > MCP first). Output is JSON.

Terminals
  xtermium sessions                      saved sessions
  xtermium tabs                          open tabs, with their ids
  xtermium open <session>                open a saved session (name or folder/name); prints the tab id
  xtermium open --shell [program] [--dir <folder>]
                                         open a local shell (the default one without a program),
                                         in a folder if given
  xtermium open --address <address>      connect without a saved session: user@host, rdp://host, …
  xtermium open --url <address>          open a website tab at an http(s) address
  xtermium close <tab>                   close a tab
  xtermium run <tab> <command> [--until <regex>] [--wait <ms>] [--lines <n>]
                                         type a command and press Enter; with --until, wait for its
                                         output to match; with --wait alone, for it to settle
  xtermium read <tab> [--lines <n>]      the last lines of a tab
  xtermium wait <tab> <regex> [--timeout <ms>] [--lines <n>]
                                         wait until the last lines match
  xtermium type <tab> <text>             type text (no Enter)
  xtermium key <tab> <keys>              press keys: ctrl+c, Enter, \"up up Enter\", alt+F4, meta
  xtermium detach <tab>                  move a terminal or RDP tab to a window of its own
  xtermium attach <tab>                  put it back in the main window

Remote desktops (RDP, VNC); x and y are desktop pixels
  xtermium screenshot <tab> [--out <file.png>] [--max-width <px>] [--region <x,y,w,h>]
                                         save a PNG (default screenshot-<tab>.png)
  xtermium click <tab> <x> <y> [--right|--middle] [--double]
                                         (terminals: x is the column, y the row)
  xtermium move <tab> <x> <y>            hover
  xtermium drag <tab> <x> <y> <to-x> <to-y>
  xtermium scroll <tab> <lines> [--at <x,y>]  positive up, negative down
  type and key work on remote desktops too.

Websites (their tabs; elements are given by a number from web-read, a CSS selector,
or --text and the name the element shows)
  xtermium web-read <tab> [--chars <n>]  the page: address, title, text and its numbered elements
  xtermium web-click <tab> <ref|selector> | --text <name>
                                         click an element; waits for a page it opens
  xtermium web-fill <tab> <ref|selector> <value> [--submit]
  xtermium web-fill <tab> --text <name> <value> [--submit]
                                         fill a field, pick an option or tick a box (true/false);
                                         --submit sends the form
  xtermium web-go <tab> <address|back|forward|reload>
  xtermium web-eval <tab> <script>       run a JavaScript function in the page, like
                                         \"() => document.title\"; prints what it returns
  screenshot works on website tabs too (Windows).

Tunnels
  xtermium tunnels                       tunnels and whether they run
  xtermium tunnel start|stop <name>      start or stop a tunnel

Workspaces (each its own sessions and folders; the commands above use the one in use)
  xtermium workspaces                    the workspaces and the one in use (\"\" is the default)
  xtermium workspace <name>              switch to one; no name: the default
  xtermium clone-workspace <name>        copy its sessions and folders, as \"<name> (clone)\"
  xtermium sync-workspace <from> <to>    bring one's sessions and folders into another;
                                         nothing is removed
  xtermium move-to <workspace> <session|folder/>...
                                         move sessions, and folders (ending in /) with everything
                                         in them, to another workspace (\"\" is the default)
  xtermium layouts                       saved layouts: sessions opened together in panes
  xtermium layout <name>                 open one
  xtermium logins                        shared logins, and the hosts with a saved password (never
                                         the passwords themselves)

Restore points (full copies of xtermium's data on this computer; restore in Settings)
  xtermium restore-points                the ones kept, newest first
  xtermium restore-point                 keep one of everything now";

/// Runs a command-line invocation; None when the arguments aren't one (the app starts as usual).
pub fn run(args: &[String]) -> Option<i32> {
    let parsed = parse(args)?;
    attach_console();
    let (tool, arguments) = match parsed {
        Ok(call) => call,
        Err(msg) => {
            eprintln!("{msg}\n\n{USAGE}");
            return Some(2);
        }
    };
    match call((tool, arguments)) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            Some(0)
        }
        Err(e) => {
            println!("{}", json!({ "error": e }));
            Some(1)
        }
    }
}

type Call = (&'static str, Value);

const COMMANDS: &[&str] = &[
    "sessions", "tabs", "tunnels", "open", "close", "run", "read", "wait", "type", "key", "screenshot", "click", "move",
    "drag", "scroll", "tunnel", "workspaces", "workspace", "clone-workspace", "sync-workspace", "layouts", "layout", "logins", "detach", "attach", "move-to", "restore-points", "restore-point", "web-read", "web-click", "web-fill", "web-go", "web-eval", "help", "--help", "-h",
];
/// Flags that take a value; every other `--flag` is on or off.
const VALUED: &[&str] = &["--wait", "--lines", "--until", "--timeout", "--out", "--max-width", "--region", "--at", "--address", "--url", "--dir", "--chars", "--text"];

fn parse(args: &[String]) -> Option<Result<Call, String>> {
    let cmd = args.first()?.as_str();
    // Anything else (no arguments, a file the OS passed) starts the app.
    if !COMMANDS.contains(&cmd) {
        return None;
    }
    // Flags and their values, and the plain words left.
    let mut flags: Vec<(&str, Option<&str>)> = vec![];
    let mut plain: Vec<&str> = vec![];
    let mut rest = args[1..].iter().map(String::as_str);
    while let Some(a) = rest.next() {
        if a.starts_with("--") && a.len() > 2 {
            flags.push((a, if VALUED.contains(&a) { rest.next() } else { None }));
        } else {
            plain.push(a);
        }
    }
    let has = |name: &str| flags.iter().any(|(f, _)| *f == name);
    let value = |name: &str| -> Result<Option<&str>, String> {
        match flags.iter().find(|(f, _)| *f == name) {
            None => Ok(None),
            Some((_, Some(v))) => Ok(Some(v)),
            Some(_) => Err(format!("{name} needs a value")),
        }
    };
    let number = |name: &str| -> Result<Option<i64>, String> {
        value(name)?.map(|v| v.parse().map_err(|_| format!("{name} needs a number"))).transpose()
    };
    // "x,y" or "x,y,w,h".
    let numbers = |name: &str, n: usize| -> Result<Option<Vec<i64>>, String> {
        let Some(v) = value(name)? else { return Ok(None) };
        let parts: Result<Vec<i64>, _> = v.split(',').map(|p| p.trim().parse()).collect();
        match parts {
            Ok(p) if p.len() == n => Ok(Some(p)),
            _ => Err(format!("{name} takes {n} numbers separated by commas")),
        }
    };
    let tab =
        || plain.first().and_then(|t| t.parse::<u32>().ok()).ok_or("give a tab id (see `xtermium tabs`)".to_string());
    // The n-th word (after the tab) as a number.
    let int = |i: usize, what: &str| -> Result<i64, String> {
        plain.get(i).and_then(|v| v.parse().ok()).ok_or(format!("give {what}"))
    };
    let words = |from: usize, what: &str| plain.get(from..).map(|w| w.join(" ")).filter(|w| !w.is_empty()).ok_or(format!("give {what}"));
    Some((|| -> Result<Call, String> {
        Ok(match cmd {
            "sessions" => ("sessions", json!({})),
            "tabs" => ("tabs", json!({})),
            "tunnels" => ("tunnels", json!({})),
            "workspaces" => ("workspaces", json!({})),
            "logins" => ("logins", json!({})),
            "layouts" => ("layouts", json!({})),
            "layout" => ("layout_open", json!({ "name": words(0, "a layout name")? })),
            "workspace" => ("workspace_switch", json!({ "name": plain.join(" ") })),
            "clone-workspace" => ("workspace_clone", json!({ "name": plain.join(" ") })),
            "sync-workspace" => {
                let [from, to] = plain.as_slice() else { return Err("give two workspaces (\"\" for the default)".into()) };
                ("workspace_sync", json!({ "from": from, "to": to }))
            }
            "move-to" => {
                let to = plain.first().ok_or("give a workspace (\"\" for the default)")?;
                let (folders, sessions): (Vec<&str>, Vec<&str>) = plain[1..].iter().partition(|w| w.ends_with('/'));
                if folders.is_empty() && sessions.is_empty() {
                    return Err("give sessions, or folders ending in /".into());
                }
                ("move_to_workspace", json!({ "to": to, "sessions": sessions, "folders": folders }))
            }
            "restore-points" => ("restore_points", json!({})),
            "restore-point" => ("restore_point_take", json!({})),
            "detach" => ("detach", json!({ "tab": tab()? })),
            "attach" => ("attach", json!({ "tab": tab()? })),
            "open" if has("--shell") => match value("--dir")? {
                Some(dir) => ("open", json!({ "shell": plain.join(" "), "dir": std::path::absolute(dir).map_err(|e| format!("{dir}: {e}"))? })),
                None => ("open", json!({ "shell": plain.join(" ") })),
            },
            "open" if has("--address") => ("open", json!({ "address": value("--address")? })),
            "open" if has("--url") => ("open", json!({ "url": value("--url")? })),
            "open" => ("open", json!({ "session": words(0, "a session name")? })),
            "close" => ("close", json!({ "tab": tab()? })),
            "run" => {
                let mut a = json!({ "tab": tab()?, "command": words(1, "a command to run")? });
                if let Some(u) = value("--until")? {
                    a["until"] = json!(u);
                }
                if let Some(ms) = number("--wait")? {
                    a["wait_ms"] = json!(ms);
                }
                if let Some(n) = number("--lines")? {
                    a["lines"] = json!(n);
                }
                ("run", a)
            }
            "read" => ("read", json!({ "tab": tab()?, "lines": number("--lines")?.unwrap_or(50) })),
            "wait" => {
                let mut a = json!({ "tab": tab()?, "pattern": words(1, "a pattern to wait for")? });
                if let Some(ms) = number("--timeout")? {
                    a["timeout_ms"] = json!(ms);
                }
                if let Some(n) = number("--lines")? {
                    a["lines"] = json!(n);
                }
                ("wait_for", a)
            }
            "type" => ("type", json!({ "tab": tab()?, "text": words(1, "text to type")? })),
            "key" => ("key", json!({ "tab": tab()?, "keys": words(1, "keys, like ctrl+c or Enter")? })),
            "screenshot" => {
                let id = tab()?;
                let out = value("--out")?.map_or_else(|| format!("screenshot-{id}.png"), String::from);
                // The app writes the file: give it a full path.
                let path = std::path::absolute(&out).map_err(|e| format!("{out}: {e}"))?;
                let mut a = json!({ "tab": id, "save": path });
                if let Some(w) = number("--max-width")? {
                    a["max_width"] = json!(w);
                }
                if let Some(r) = numbers("--region", 4)? {
                    a["region"] = json!({ "x": r[0], "y": r[1], "width": r[2], "height": r[3] });
                }
                ("screenshot", a)
            }
            "click" => {
                let button = if has("--right") { "right" } else if has("--middle") { "middle" } else { "left" };
                let a = json!({ "tab": tab()?, "x": int(1, "x and y")?, "y": int(2, "x and y")?, "button": button, "double": has("--double") });
                ("click", a)
            }
            "move" => ("move", json!({ "tab": tab()?, "x": int(1, "x and y")?, "y": int(2, "x and y")? })),
            "drag" => {
                let n = |i| int(i, "x, y, to-x and to-y");
                ("drag", json!({ "tab": tab()?, "x": n(1)?, "y": n(2)?, "to_x": n(3)?, "to_y": n(4)? }))
            }
            "scroll" => {
                let mut a = json!({ "tab": tab()?, "lines": int(1, "how many lines (negative: down)")? });
                if let Some(p) = numbers("--at", 2)? {
                    (a["x"], a["y"]) = (json!(p[0]), json!(p[1]));
                }
                ("scroll", a)
            }
            "web-read" => ("web_read", json!({ "tab": tab()?, "chars": number("--chars")?.unwrap_or(8000) })),
            "web-click" | "web-fill" => {
                let mut a = json!({ "tab": tab()? });
                // The element: --text, else the word after the tab (a number is web-read's).
                let from = match value("--text")? {
                    Some(t) => {
                        a["text"] = json!(t);
                        1
                    }
                    None => {
                        let target = plain.get(1).ok_or("give the element: its number from web-read, a CSS selector, or --text <name>")?;
                        match target.parse::<u64>() {
                            Ok(n) => a["ref"] = json!(n),
                            Err(_) => a["selector"] = json!(target),
                        }
                        2
                    }
                };
                if cmd == "web-click" {
                    ("web_click", a)
                } else {
                    a["value"] = json!(plain.get(from..).map(|w| w.join(" ")).unwrap_or_default());
                    a["submit"] = json!(has("--submit"));
                    ("web_fill", a)
                }
            }
            "web-go" => ("web_go", json!({ "tab": tab()?, "to": words(1, "an address, back, forward or reload")? })),
            "web-eval" => ("web_eval", json!({ "tab": tab()?, "script": words(1, "a script, like \"() => document.title\"")? })),
            "tunnel" => match (plain.first().copied(), plain.get(1)) {
                (Some("start"), Some(name)) => ("tunnel_start", json!({ "name": name })),
                (Some("stop"), Some(name)) => ("tunnel_stop", json!({ "name": name })),
                _ => return Err("use: xtermium tunnel start|stop <name>".into()),
            },
            _ => return Err("xtermium command line".into()), // help
        })
    })())
}

/// Calls a tool over the automation server.
fn call((tool, arguments): Call) -> Result<Value, String> {
    let s = settings()?;
    if !s.automation || s.automation_token.is_empty() {
        return Err("Automation is off: turn it on in xtermium, Settings > MCP.".into());
    }
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": arguments } }).to_string();
    let port = s.automation_port;
    let mut sock = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("xtermium isn't running (or not on port {port}): {e}"))?;
    sock.set_read_timeout(Some(Duration::from_secs(660))).ok();
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        s.automation_token,
        body.len()
    );
    sock.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    sock.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (_, body) = text.split_once("\r\n\r\n").ok_or("bad answer from xtermium")?;
    let v: Value = serde_json::from_str(body).map_err(|_| format!("bad answer from xtermium: {body}"))?;
    if let Some(e) = v.get("error") {
        return Err(e.get("message").or(Some(e)).and_then(Value::as_str).unwrap_or("error").to_string());
    }
    let result = &v["result"];
    if result["isError"] == true {
        return Err(result["content"][0]["text"].as_str().unwrap_or("error").to_string());
    }
    Ok(result["structuredContent"].clone())
}

fn settings() -> Result<crate::Settings, String> {
    match crate::config::init_key().0 {
        crate::config::KeyState::Ready => Ok(crate::Settings::load()),
        crate::config::KeyState::Locked { .. } => Err("xtermium's data is locked: unlock it in the app first.".into()),
    }
}

/// The app is a GUI program on Windows: without this, output started from a console goes nowhere.
#[cfg(windows)]
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn GetStdHandle(which: u32) -> isize;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    // Piped or redirected output already has a handle; only a console needs attaching.
    unsafe {
        if matches!(GetStdHandle(STD_OUTPUT_HANDLE), 0 | -1) {
            AttachConsole(u32::MAX); // the parent's console
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &str) -> Option<Result<Call, String>> {
        parse(&a.split(' ').filter(|s| !s.is_empty()).map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn commands() {
        assert!(p("").is_none() && p("--something-else").is_none(), "not CLI: the app starts");
        assert_eq!(p("tabs").unwrap().unwrap(), ("tabs", json!({})));
        assert_eq!(p("open Prod/db1").unwrap().unwrap(), ("open", json!({ "session": "Prod/db1" })));
        assert_eq!(
            p("run 3 ls -la --wait 2000").unwrap().unwrap(),
            ("run", json!({ "tab": 3, "command": "ls -la", "wait_ms": 2000 }))
        );
        assert_eq!(p("read 3 --lines 10").unwrap().unwrap(), ("read", json!({ "tab": 3, "lines": 10 })));
        assert_eq!(p("tunnel stop db").unwrap().unwrap(), ("tunnel_stop", json!({ "name": "db" })));
        assert!(p("run x ls").unwrap().is_err());
        assert!(p("run 3 --wait nope").unwrap().is_err());
        assert_eq!(
            p("run 3 make test --until PASS|FAIL").unwrap().unwrap(),
            ("run", json!({ "tab": 3, "command": "make test", "until": "PASS|FAIL" }))
        );
        assert_eq!(p("open --shell").unwrap().unwrap(), ("open", json!({ "shell": "" })));
        assert_eq!(p("open --shell pwsh -NoLogo").unwrap().unwrap(), ("open", json!({ "shell": "pwsh -NoLogo" })));
        assert_eq!(p("open --address root@192.0.2.15").unwrap().unwrap(), ("open", json!({ "address": "root@192.0.2.15" })));
        assert_eq!(p("key 3 ctrl+c").unwrap().unwrap(), ("key", json!({ "tab": 3, "keys": "ctrl+c" })));
        assert_eq!(p("open --url https://pve:8006").unwrap().unwrap(), ("open", json!({ "url": "https://pve:8006" })));
        assert_eq!(p("web-read 7").unwrap().unwrap(), ("web_read", json!({ "tab": 7, "chars": 8000 })));
        assert_eq!(p("web-click 7 12").unwrap().unwrap(), ("web_click", json!({ "tab": 7, "ref": 12 })));
        assert_eq!(p("web-click 7 #login").unwrap().unwrap(), ("web_click", json!({ "tab": 7, "selector": "#login" })));
        assert_eq!(p("web-click 7 --text Login").unwrap().unwrap(), ("web_click", json!({ "tab": 7, "text": "Login" })));
        assert_eq!(
            p("web-fill 7 --text Email me@example.com --submit").unwrap().unwrap(),
            ("web_fill", json!({ "tab": 7, "text": "Email", "value": "me@example.com", "submit": true }))
        );
        assert_eq!(
            p("web-fill 7 3 hello world").unwrap().unwrap(),
            ("web_fill", json!({ "tab": 7, "ref": 3, "value": "hello world", "submit": false }))
        );
        assert_eq!(p("web-go 7 back").unwrap().unwrap(), ("web_go", json!({ "tab": 7, "to": "back" })));
        assert!(p("web-click 7").unwrap().is_err());
        assert_eq!(p("workspace Lab 2").unwrap().unwrap(), ("workspace_switch", json!({ "name": "Lab 2" })));
        assert_eq!(p("workspace").unwrap().unwrap(), ("workspace_switch", json!({ "name": "" })));
        assert_eq!(p("clone-workspace Lab 2").unwrap().unwrap(), ("workspace_clone", json!({ "name": "Lab 2" })));
        assert_eq!(p("sync-workspace Lab Work").unwrap().unwrap(), ("workspace_sync", json!({ "from": "Lab", "to": "Work" })));
        assert!(p("sync-workspace Lab").unwrap().is_err());
        assert_eq!(
            p("move-to Lab db1 Prod/").unwrap().unwrap(),
            ("move_to_workspace", json!({ "to": "Lab", "sessions": ["db1"], "folders": ["Prod/"] }))
        );
        assert!(p("move-to Lab").unwrap().is_err());
        assert_eq!(p("restore-point").unwrap().unwrap(), ("restore_point_take", json!({})));
        assert_eq!(p("detach 3").unwrap().unwrap(), ("detach", json!({ "tab": 3 })));
        assert_eq!(
            p("click 4 100 200 --right --double").unwrap().unwrap(),
            ("click", json!({ "tab": 4, "x": 100, "y": 200, "button": "right", "double": true }))
        );
        assert_eq!(
            p("scroll 4 -3 --at 10,20").unwrap().unwrap(),
            ("scroll", json!({ "tab": 4, "lines": -3, "x": 10, "y": 20 }))
        );
        let (tool, a) = p("screenshot 4 --region 0,0,640,480").unwrap().unwrap();
        assert_eq!((tool, &a["region"]["width"]), ("screenshot", &json!(640)));
        assert!(a["save"].as_str().unwrap().ends_with("screenshot-4.png"));
        assert!(p("screenshot 4 --region 1,2").unwrap().is_err());
        assert_eq!(p("wait 3 ready --timeout 5000").unwrap().unwrap(), ("wait_for", json!({ "tab": 3, "pattern": "ready", "timeout_ms": 5000 })));
    }
}
