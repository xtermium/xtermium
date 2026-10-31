//! Broadcast: what's typed in one tab goes to every other ticked tab, whatever its kind.
//! Terminals get keystrokes, which each encodes for its own modes; remote desktops and BMC consoles
//! get key presses (DOM codes, US layout) and text. Between kinds it's approximate (the UI says
//! so): a terminal's Ctrl+C reaches a desktop as Ctrl+C, a desktop's typing reaches terminals as
//! the characters and keys it pressed, and the mouse isn't shared.

use crate::rdp::Input;
use crate::term::Typed;
use crate::{App, Kind, Tab};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering::Relaxed;
use tauri::AppHandle;
use wezterm_term::{KeyCode, KeyModifiers};

/// What was typed in the source tab.
pub enum Typing<'a> {
    Term(&'a [Typed]),
    Desktop(&'a Input),
}

/// Sessions broadcast reaches: every kind but websites.
pub fn joins(kind: &Kind) -> bool {
    !matches!(kind, Kind::Browser { .. })
}

/// Broadcast is on and tab `id` is ticked.
fn ticked(app: &App, id: u32) -> bool {
    app.broadcast.load(Relaxed) && !app.broadcast_off.lock().unwrap().contains(&id)
}

fn kind(t: &Tab) -> &Kind {
    match t {
        Tab::Term(t) => &t.session.kind,
        Tab::Rdp(t) => &t.session.kind,
        Tab::Vnc(t) => &t.session.kind,
    }
}

/// The modifier keys a desktop holds down, for the keys it sends terminals.
static MODS: Mutex<KeyModifiers> = Mutex::new(KeyModifiers::NONE);

/// Passes what was typed in tab `src` (already typed there) on to every other ticked tab.
pub fn share(app: &App, handle: &AppHandle, tabs: &HashMap<u32, Tab>, src: u32, typed: Typing) {
    if !tabs.get(&src).is_some_and(|t| joins(kind(t))) || !ticked(app, src) {
        return;
    }
    let (keys, desk): (Vec<Typed>, Vec<Input>) = match typed {
        Typing::Term(evs) => (evs.to_vec(), evs.iter().flat_map(to_desktop).collect()),
        Typing::Desktop(ev @ (Input::Key { .. } | Input::Text { .. })) => {
            (to_term(ev, &mut MODS.lock().unwrap()), vec![ev.clone()])
        }
        // The desktop typed in let go of its keys: so do the ones that got the same presses.
        Typing::Desktop(Input::Release) => {
            *MODS.lock().unwrap() = KeyModifiers::NONE;
            (vec![], vec![Input::Release])
        }
        Typing::Desktop(_) => return,
    };
    for (&id, t) in tabs {
        if id == src || !joins(kind(t)) || !ticked(app, id) {
            continue;
        }
        match t {
            Tab::Term(t) if !keys.is_empty() => t.apply(&keys),
            Tab::Rdp(t) => desk.iter().for_each(|e| t.input(e.clone())),
            Tab::Vnc(t) => desk.iter().for_each(|e| t.input(e.clone())),
            Tab::Term(_) => {}
        }
    }
    if !desk.is_empty() {
        for id in crate::web::consoles().into_iter().filter(|&id| id != src && ticked(app, id)) {
            let _ = crate::web::console_input(handle, id, &desk);
        }
    }
}

/// Keys named the same as DOM `key` and `code`.
fn named(code: &str) -> Option<KeyCode> {
    Some(match code {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Escape" => KeyCode::Escape,
        "ArrowUp" => KeyCode::UpArrow,
        "ArrowDown" => KeyCode::DownArrow,
        "ArrowLeft" => KeyCode::LeftArrow,
        "ArrowRight" => KeyCode::RightArrow,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Insert" => KeyCode::Insert,
        "Delete" => KeyCode::Delete,
        f => KeyCode::Function(f.strip_prefix('F')?.parse().ok().filter(|n| (1..=24).contains(n))?),
    })
}

/// A terminal keystroke as desktop input: plain characters as text, the rest as key presses
/// with their modifiers around them.
fn to_desktop(ev: &Typed) -> Vec<Input> {
    let (k, m) = match ev {
        Typed::Paste(s) => return vec![Input::Text { text: s.clone() }],
        Typed::Key(KeyCode::Char(c), m) if !m.intersects(KeyModifiers::CTRL | KeyModifiers::ALT) => {
            return vec![Input::Text { text: c.to_string() }];
        }
        Typed::Key(k, m) => (k, *m),
    };
    let code = match k {
        KeyCode::Char(c) if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()),
        KeyCode::Char(c) if c.is_ascii_digit() => format!("Digit{c}"),
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Function(n) => format!("F{n}"),
        k => match [
            "Enter",
            "Tab",
            "Backspace",
            "Escape",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Insert",
            "Delete",
        ]
        .into_iter()
        .find(|n| named(n).as_ref() == Some(k))
        {
            Some(n) => n.into(),
            None => return vec![],
        },
    };
    let mods: Vec<&str> =
        [(KeyModifiers::CTRL, "ControlLeft"), (KeyModifiers::ALT, "AltLeft"), (KeyModifiers::SHIFT, "ShiftLeft")]
            .into_iter()
            .filter_map(|(f, c)| m.contains(f).then_some(c))
            .collect();
    let key = |code: &str, down| Input::Key { code: code.into(), down };
    let mut v: Vec<Input> = mods.iter().map(|c| key(c, true)).collect();
    v.extend([key(&code, true), key(&code, false)]);
    v.extend(mods.iter().rev().map(|c| key(c, false)));
    v
}

/// Desktop input as terminal keystrokes; `mods` follows the modifier keys from one to the next.
fn to_term(ev: &Input, mods: &mut KeyModifiers) -> Vec<Typed> {
    match ev {
        Input::Text { text } => text
            .chars()
            .map(|c| Typed::Key(if c == '\n' { KeyCode::Enter } else { KeyCode::Char(c) }, KeyModifiers::NONE))
            .collect(),
        Input::Key { code, down } => {
            let m = match code.as_str() {
                "ControlLeft" | "ControlRight" => KeyModifiers::CTRL,
                "AltLeft" | "AltRight" => KeyModifiers::ALT,
                "ShiftLeft" | "ShiftRight" => KeyModifiers::SHIFT,
                _ => KeyModifiers::NONE,
            };
            if !m.is_empty() {
                mods.set(m, *down);
                return vec![];
            }
            let ch = |s: Option<&str>| s.filter(|s| s.len() == 1).and_then(|s| s.chars().next());
            let k = if let Some(c) = ch(code.strip_prefix("Key")) {
                KeyCode::Char(c.to_ascii_lowercase())
            } else if let Some(d) = ch(code.strip_prefix("Digit")) {
                KeyCode::Char(d)
            } else if code == "Space" {
                KeyCode::Char(' ')
            } else if let Some(k) = named(code) {
                k
            } else {
                return vec![];
            };
            if *down { vec![Typed::Key(k, *mods)] } else { vec![] }
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(v: &[Input]) -> Vec<String> {
        v.iter()
            .map(|e| match e {
                Input::Key { code, down } => format!("{}{code}", if *down { "+" } else { "-" }),
                Input::Text { text } => format!("'{text}'"),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn terminal_keys_reach_desktops() {
        assert_eq!(codes(&to_desktop(&Typed::Key(KeyCode::Char('x'), KeyModifiers::NONE))), ["'x'"]);
        assert_eq!(
            codes(&to_desktop(&Typed::Key(KeyCode::Char('c'), KeyModifiers::CTRL))),
            ["+ControlLeft", "+KeyC", "-KeyC", "-ControlLeft"]
        );
        assert_eq!(codes(&to_desktop(&Typed::Key(KeyCode::Enter, KeyModifiers::NONE))), ["+Enter", "-Enter"]);
        assert_eq!(codes(&to_desktop(&Typed::Key(KeyCode::Function(5), KeyModifiers::NONE))), ["+F5", "-F5"]);
    }

    #[test]
    fn desktop_keys_reach_terminals() {
        let mut m = KeyModifiers::NONE;
        let mut keys = |code: &str, down| to_term(&Input::Key { code: code.into(), down }, &mut m);
        assert!(keys("ControlLeft", true).is_empty());
        assert!(matches!(keys("KeyC", true)[..], [Typed::Key(KeyCode::Char('c'), KeyModifiers::CTRL)]));
        assert!(keys("KeyC", false).is_empty());
        keys("ControlLeft", false);
        assert!(matches!(keys("ArrowUp", true)[..], [Typed::Key(KeyCode::UpArrow, KeyModifiers::NONE)]));
        let typed = to_term(&Input::Text { text: "a\n".into() }, &mut m);
        assert!(matches!(typed[..], [Typed::Key(KeyCode::Char('a'), _), Typed::Key(KeyCode::Enter, _)]));
    }
}
