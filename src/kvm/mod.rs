//! BMC consoles (`Kind::Kvm`): the BMC's own HTML5 console page in a website view (web.rs), signed
//! in with the session's shared login. Each vendor (`config::KVM_VENDORS`) has a module with where
//! its tab starts and the script that signs in and shows its console; this one has what they share:
//! the script's helpers, the scripts that type and click (paste, automation, broadcast) and take
//! screenshots, and trust on first use for the BMC's self-signed certificate.

mod idrac;
pub mod relay;
mod supermicro;

use crate::config::{self, Known};
use crate::rdp::Input;
use sha2::{Digest, Sha256};

type Res<T> = Result<T, String>;

/// Where a vendor's console tab starts, after `https://host:port`.
pub fn home(vendor: &str) -> &'static str {
    match vendor {
        "idrac" => idrac::HOME,
        _ => supermicro::HOME,
    }
}

/// Why a console tab shows nothing when the BMC hands over a Java console (a `.jnlp` file), and what to do.
pub fn java_hint(vendor: &str) -> &'static str {
    match vendor {
        "idrac" => idrac::JAVA,
        _ => supermicro::JAVA,
    }
}

/// Runs at the start of every page and frame in a console tab, on the BMC's own pages only (`host`,
/// as `location.host` reads): the shared helpers, then the vendor's script. `relay`: where the
/// console's own connection can go instead (relay.rs), "" for none.
pub fn login_script(vendor: &str, host: &str, user: &str, password: &str, relay: &str) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let script = match vendor {
        "idrac" => idrac::SCRIPT,
        _ => supermicro::SCRIPT,
    };
    format!(
        "(() => {{\n  if (location.host !== {}) return;\n  const [user, pass, relay] = [{}, {}, {}];{COMMON}{script}}})();",
        q(host),
        q(user),
        q(password),
        q(relay)
    )
}

/// The helpers vendors' scripts use.
const COMMON: &str = r#"
  // What happened in this tab so far, kept in window.name: it outlives the pages (Supermicro's
  // clears sessionStorage once signed in). The login is filled in once: a refused password shows
  // the form again, and that's left to the person.
  const notes = () => (window.name.startsWith("xtm:") ? window.name.slice(4).split(",") : []);
  const seen = (k) => notes().includes(k);
  const mark = (k) => (window.name = "xtm:" + [...new Set([...notes(), k])].join(","));
  const set = (el, v) => {
    el.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, v);
    for (const t of ["input", "change"]) el.dispatchEvent(new Event(t, { bubbles: true }));
  };
  const shown = (e) => e.offsetParent !== null;
  const passwordField = () => [...document.querySelectorAll("input[type=password]")].find(shown);
  // The user goes in the last text field before the password.
  const fillIn = (pw) => {
    const texts = [...document.querySelectorAll("input:not([type]), input[type=text], input[type=email]")].filter(
      (e) => shown(e) && e.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING,
    );
    if (user && texts.length) set(texts[texts.length - 1], user);
    set(pw, pass);
  };
  // Fills the form in and presses its sign-in button, once the page has set itself up (its sign-in
  // script may need what it loads).
  const fillForm = (pw) => {
    fillIn(pw);
    const buttons = [...document.querySelectorAll("button, input[type=submit], input[type=button], a[href^='javascript:']")].filter(shown);
    const button =
      buttons.find((b) => /log.?in|sign.?in|submit/i.test([b.id, b.name, b.className, b.textContent, b.value].join(" "))) ??
      buttons.find((b) => b.type === "submit");
    const go = () => setTimeout(() => (button ? button.click() : pw.form?.requestSubmit()), 300);
    if (document.readyState === "complete") go();
    else addEventListener("load", go, { once: true });
  };
  // A frame over the whole page for the console, keeping the page that signed in; `ready` gets
  // its window. Consoles cancel clicks on their screen, so a click wouldn't move the keyboard into
  // the frame: a click there, or the tab getting the keyboard, hands it to the console.
  const overlay = (src, ready) => {
    document.getElementById("xtm-console")?.remove();
    const f = document.createElement("iframe");
    f.id = "xtm-console";
    f.src = src;
    f.style.cssText = "position:fixed;inset:0;width:100vw;height:100vh;border:0;z-index:2147483647;background:#000";
    document.documentElement.style.overflow = "hidden";
    document.body.append(f);
    f.addEventListener("load", () => {
      const w = f.contentWindow;
      w.document.addEventListener("pointerdown", () => w.focus(), true);
      addEventListener("focus", () => w.focus());
      w.focus();
      ready(w);
    });
    return f;
  };
"#;

/// A certificate's fingerprint as the pins keep it (hex SHA-256 of its DER), from PEM text.
pub fn pem_fingerprint(pem: &str) -> Option<String> {
    use base64::Engine;
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    Some(crate::vault::hex(&Sha256::digest(der)))
}

/// Trust on first use for a BMC's certificate (`id`: `host:port`), like SSH host keys: true when
/// it's the one trusted before, or the person trusts it now. A changed one is refused.
pub async fn trust(prompter: &crate::prompt::Prompter, id: &str, fp: &str) -> Res<bool> {
    match Known::with(|k| k.https.get(id).cloned()).map_err(|e| e.to_string())? {
        Some(k) if k == fp => Ok(true),
        Some(_) => {
            let text = format!(
                "The certificate of {id} differs from the one you trusted before.\nThis may be an attack. Connection refused.\n\
                 If the change is expected, remove the old one under Settings > Security."
            );
            prompter.confirm("BMC certificate CHANGED", text).await;
            Ok(false)
        }
        None => {
            let text = format!("{id}\ncertificate SHA256:\n{fp}\n\nTrust this BMC and remember its certificate?");
            if !prompter.confirm("Unknown BMC certificate", text).await {
                return Ok(false);
            }
            Known::with(|k| k.https.insert(id.into(), fp.into())).map_err(|e| e.to_string())?;
            Ok(true)
        }
    }
}

/// Remote desktop input (pasted text, automation, broadcast) for the console, the way a keyboard
/// on a US layout and a mouse would send it: keys through the page's noVNC viewer when it has one
/// (Supermicro's does), else as page key events; the mouse as events on the console's picture, at
/// desktop pixels. The console sends key codes to the server, not characters: characters without
/// a key are skipped. Keys are paced, since consoles drop keys that come too fast; one queue per
/// page keeps calls that overlap in order.
pub fn input_script(evs: &[Input]) -> String {
    let evs: Vec<Input> = evs
        .iter()
        .map(|e| match e {
            Input::Text { text } => Input::Text { text: text.replace("\r\n", "\n").replace('\r', "\n") },
            e => e.clone(),
        })
        .collect();
    format!("((evs) => {{{FIND}{INPUT}}})({})", serde_json::to_string(&evs).unwrap_or_default())
}

/// The console's picture as a PNG `data:` address; "" while there is none (still connecting).
pub fn screenshot_script() -> String {
    format!(r#"(() => {{{FIND}const c = screen(); return c && c.width && c.height ? c.toDataURL("image/png") : ""; }})()"#)
}

/// Finds the console in the page: noVNC's viewer and its picture, in whichever frame they are.
const FIND: &str = r#"
  // Every window of the page, frames inside frames too (the console sits in one).
  const wins = (w = window) => {
    const all = [w];
    try {
      for (const f of Array.from(w.frames)) all.push(...wins(f));
    } catch {}
    return all;
  };
  // Supermicro's console is noVNC: its own viewer sends keys the way the BMC wants them (X11
  // keysyms, mapped to USB codes for ATEN's iKVM). Page events are for consoles without one.
  let found;
  const viewer = () => {
    if (found) return found;
    for (const w of wins()) {
      try {
        for (const v of [w.UI?.rfb, w.rfb]) if (typeof v?.sendKey === "function") return (found = v);
        for (const v of Object.values(w)) if (v && typeof v.sendKey === "function" && "_rfb_state" in v) return (found = v);
      } catch {}
    }
  };
  // The console's picture: the largest canvas, sized in desktop pixels.
  const screen = () => {
    let best = null;
    for (const w of wins()) {
      try {
        for (const c of w.document.querySelectorAll("canvas")) if (!best || c.width * c.height > best.width * best.height) best = c;
      } catch {}
    }
    return best;
  };
"#;

const INPUT: &str = r#"
  const SHIFTED = '~!@#$%^&*()_+{}|:"<>?', BASE = "`1234567890-=[]\\;',./";
  const KEYS = { "`": [192, "Backquote"], "-": [189, "Minus"], "=": [187, "Equal"], "[": [219, "BracketLeft"], "]": [221, "BracketRight"],
    "\\": [220, "Backslash"], ";": [186, "Semicolon"], "'": [222, "Quote"], ",": [188, "Comma"], ".": [190, "Period"], "/": [191, "Slash"],
    " ": [32, "Space"], "\n": [13, "Enter"], "\t": [9, "Tab"] };
  const keyOf = (c) => {
    let [base, shift] = [c, false];
    if (SHIFTED.includes(c)) [base, shift] = [BASE[SHIFTED.indexOf(c)], true];
    else if (c >= "A" && c <= "Z") [base, shift] = [c.toLowerCase(), true];
    if (base >= "a" && base <= "z") return [base.toUpperCase().charCodeAt(0), "Key" + base.toUpperCase(), shift];
    if (base >= "0" && base <= "9") return [base.charCodeAt(0), "Digit" + base, shift];
    return KEYS[base] && [...KEYS[base], shift];
  };
  const target = () => {
    let d = document;
    while (/^I?FRAME$/.test(d.activeElement?.tagName) && d.activeElement.contentDocument) d = d.activeElement.contentDocument;
    return d.activeElement ?? d.body;
  };
  const fire = (type, key, code, keyCode, shiftKey) => {
    const e = new KeyboardEvent(type, { key, code, shiftKey, bubbles: true, cancelable: true });
    for (const p of ["keyCode", "which"]) Object.defineProperty(e, p, { get: () => keyCode });
    target().dispatchEvent(e);
  };
  const SYMS = { "\n": 0xff0d, "\t": 0xff09 };
  const typeChar = (c) => {
    const k = keyOf(c), rfb = viewer();
    if (k && rfb) {
      const sym = SYMS[c] ?? c.charCodeAt(0);
      if (k[2]) rfb.sendKey(0xffe1, true);
      rfb.sendKey(sym, true);
      rfb.sendKey(sym, false);
      if (k[2]) rfb.sendKey(0xffe1, false);
    } else if (k) {
      const [keyCode, code, shift] = k;
      const key = c === "\n" ? "Enter" : c === "\t" ? "Tab" : c;
      if (shift) fire("keydown", "Shift", "ShiftLeft", 16, true);
      fire("keydown", key, code, keyCode, shift);
      fire("keyup", key, code, keyCode, shift);
      if (shift) fire("keyup", "Shift", "ShiftLeft", 16, false);
    }
  };
  // X11 keysyms of DOM key codes, for keys pressed by name (automation, broadcast).
  const CODES = { Enter: 0xff0d, Tab: 0xff09, Escape: 0xff1b, Backspace: 0xff08, Delete: 0xffff, Insert: 0xff63, Home: 0xff50, End: 0xff57,
    PageUp: 0xff55, PageDown: 0xff56, ArrowLeft: 0xff51, ArrowUp: 0xff52, ArrowRight: 0xff53, ArrowDown: 0xff54, Space: 0x20,
    ShiftLeft: 0xffe1, ShiftRight: 0xffe2, ControlLeft: 0xffe3, ControlRight: 0xffe4, AltLeft: 0xffe9, AltRight: 0xffea,
    MetaLeft: 0xffeb, MetaRight: 0xffec, Minus: 0x2d, Equal: 0x3d, BracketLeft: 0x5b, BracketRight: 0x5d, Backslash: 0x5c,
    Semicolon: 0x3b, Quote: 0x27, Backquote: 0x60, Comma: 0x2c, Period: 0x2e, Slash: 0x2f };
  const symOf = (code) =>
    CODES[code] ??
    (/^Key[A-Z]$/.test(code) ? code.toLowerCase().charCodeAt(3) : /^Digit\d$/.test(code) ? code.charCodeAt(5) : /^F([1-9]|1[0-2])$/.test(code) ? 0xffbd + +code.slice(1) : 0);
  // The queue outlives this call: input sent while earlier text is still being typed waits its turn.
  const q = (window.__xtmInput ??= { ops: [], busy: false, held: 0, x: 0, y: 0 });
  // A mouse event on the picture at desktop pixel x, y: noVNC maps it back through its own scale.
  const mouse = (type, x, y, init) => {
    const c = screen();
    if (!c || !c.width || !c.height) return;
    const w = c.ownerDocument.defaultView, r = c.getBoundingClientRect();
    [q.x, q.y] = [x, y];
    const at = { clientX: r.left + ((x + 0.5) * r.width) / c.width, clientY: r.top + ((y + 0.5) * r.height) / c.height };
    const E = type.includes("wheel") ? w.WheelEvent : w.MouseEvent;
    c.dispatchEvent(new E(type, { bubbles: true, cancelable: true, view: w, buttons: q.held, ...at, ...init }));
  };
  const BITS = [1, 4, 2]; // `buttons` bits of the left, middle and right buttons
  const ops = [];
  const step = (f, pace = 25) => ops.push([f, pace]);
  for (const e of evs) {
    if (e.t === "text") for (const c of e.text) step(() => typeChar(c));
    else if (e.t === "key") step(() => (viewer() && symOf(e.code) ? viewer().sendKey(symOf(e.code), e.down) : undefined));
    else if (e.t === "move") step(() => mouse("mousemove", e.x, e.y, {}), 0);
    else if (e.t === "button")
      step(() => {
        q.held = e.down ? q.held | BITS[e.button] : q.held & ~BITS[e.button];
        mouse(e.down ? "mousedown" : "mouseup", e.x, e.y, { button: e.button });
      }, 0);
    // Newer noVNC reads `wheel`, older `mousewheel`: each takes the one it knows.
    else if (e.t === "wheel")
      step(() => {
        const init = { deltaX: -e.dx * 100, deltaY: -e.dy * 100, wheelDeltaX: e.dx * 120, wheelDeltaY: e.dy * 120 };
        for (const t of ["wheel", "mousewheel"]) mouse(t, q.x, q.y, init);
      }, 0);
  }
  q.ops.push(...ops);
  const run = () => {
    const op = q.ops.shift();
    if (!op) return void (q.busy = false);
    q.busy = true;
    try {
      op[0]();
    } catch {}
    setTimeout(run, op[1]);
  };
  if (!q.busy) run();
"#;

/// The login a console tab fills in: its shared login's user and password.
pub fn login(store: &config::Store, credential: &str) -> (String, String) {
    store.credential(credential).map(|c| (c.user.clone(), c.password.clone())).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_fingerprints_match_the_pins() {
        use base64::Engine;
        let der = b"not really a certificate";
        let pem = format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n", base64::engine::general_purpose::STANDARD.encode(der));
        assert_eq!(pem_fingerprint(&pem), Some(crate::vault::hex(&Sha256::digest(der))));
        assert_eq!(pem_fingerprint("-----BEGIN-----\n%%%\n"), None);
    }

    /// Console sessions saved before vendors existed are Supermicro's.
    #[test]
    fn older_sessions_are_supermicro() {
        let k: config::Kind = serde_json::from_str(r#"{"type":"kvm","host":"b"}"#).unwrap();
        assert!(matches!(k, config::Kind::Kvm { vendor, port: 443, .. } if vendor == "supermicro"));
    }

    /// Each vendor's console tab starts at its own page, with its own script after the shared one.
    #[test]
    fn vendors_have_their_own_start_and_script() {
        assert_eq!((home("supermicro"), home("idrac")), ("/", "/login.html?console"));
        let s = login_script("idrac", "bmc", "u", "p", "");
        assert!(s.contains("const fillIn") && s.contains("virtualconsolehtml5") && !s.contains("man_ikvm"));
        assert!(login_script("supermicro", "bmc", "u", "p", "").contains("man_ikvm_html5_bootstrap"));
    }

    #[test]
    fn scripts_quote_what_they_carry() {
        let s = login_script("supermicro", "bmc:8443", "ad\"min", "p'w</script>", "");
        assert!(s.contains(r#"location.host !== "bmc:8443""#) && s.contains(r#""ad\"min""#));
        let typed = input_script(&[Input::Text { text: "a\"b\r\nc".into() }]);
        assert!(typed.ends_with(r#"})([{"t":"text","text":"a\"b\nc"}])"#), "{typed}");
    }
}
