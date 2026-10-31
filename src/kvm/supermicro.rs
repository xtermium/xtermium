//! Supermicro BMCs: the HTML5 iKVM console of X11 and newer boards, which is noVNC.

/// The web interface, where the tab signs in.
pub const HOME: &str = "/";

/// What to do when it hands over a Java console instead (older boards, X9 and X10).
pub const JAVA: &str = "This BMC's console is Java only (older Supermicro boards, X9 and X10), which xtermium can't run. \
     xtermium shows the HTML5 console of X11 and newer boards.";

/// After the shared helpers (mod.rs `COMMON`). Fills the login form; once past it, shows the
/// console over the page it landed on, in a frame: Supermicro's refuses its own token once the page
/// that signed in is gone (the web interface opens the console in a pop-up and keeps that page
/// open). Newer BMCs draw the form late (single-page apps), so it looks for a few seconds.
pub const SCRIPT: &str = r#"
  if (window.top !== window) return;
  const CONSOLE = "/cgi/url_redirect.cgi?url_name=man_ikvm_html5_bootstrap";
  // The console's picture fills the tab beside its menu column: the empty bar and margins go, and
  // noVNC scales the screen (and the mouse with it), again whenever the tab or resolution changes.
  const fit = (w) => {
    const d = w.document;
    const css = d.createElement("style");
    css.textContent = `html, body { margin: 0 !important; overflow: hidden !important; background: #000 !important; }
      .navbar { display: none !important; }
      .grid-container { display: flex !important; height: 100vh; margin: 0 !important; padding: 0 !important; }
      .jumbotron { flex: 1; min-width: 0; margin: 0 !important; padding: 0 !important; display: flex !important;
        align-items: center; justify-content: center; background: #000 !important; border-radius: 0 !important; }
      #noVNC_container { margin: 0 !important; padding: 0 !important; line-height: 0; }
      #noVNC_container > br, #noVNC_container > img { display: none !important; }
      #menu_card { flex: none; width: auto !important; }`;
    d.head.append(css);
    setInterval(() => {
      const rfb = w.UI?.rfb, box = d.querySelector(".jumbotron");
      if (!rfb || rfb._rfb_state !== "normal" || !box) return;
      const screen = rfb.get_display(), [fw, fh] = [screen.get_width(), screen.get_height()];
      if (!fw || !fh) return;
      const scale = Math.min(box.clientWidth / fw, box.clientHeight / fh);
      if (Math.abs(scale - (screen._scale ?? 1)) < 0.005) return;
      screen.set_scale(scale);
      rfb.get_mouse().set_scale(scale);
      w.UI.scale_level = scale * 100;
    }, 500);
  };
  let tries = 0;
  const look = () => {
    const pw = passwordField();
    if (pw) mark("login");
    if (pw && pass && !seen("tried")) {
      mark("tried");
      return fillForm(pw);
    }
    if (!pw && seen("login") && tries > 4 && !document.getElementById("xtm-console")) return void overlay(CONSOLE, fit);
    if (++tries < 40) setTimeout(look, 300);
  };
  addEventListener("DOMContentLoaded", look);
"#;
