//! Dell PowerEdge iDRAC 7 and 8: the HTML5 virtual console (Virtual Console > Plug-in Type: HTML5).

/// iDRAC launches the virtual console right after signing in here.
pub const HOME: &str = "/login.html?console";

/// What to do when it hands over a Java console (Plug-in Type: Native) instead.
pub const JAVA: &str = "This iDRAC's virtual console is set to Java (Native), which xtermium can't run. \
     Set Virtual Console > Plug-in Type to HTML5 on the iDRAC (and Apply), then open the session again.";

/// After the shared helpers (mod.rs `COMMON`); runs in every frame, since iDRAC's pages are a
/// frameset. The console page (`virtualconsolehtml5.html`) opens in a pop-up and reads from the page
/// that opened it (`opener.top`), which must stay open: here it opens as one more frame of that
/// frameset, filling the tab, and the frameset is its opener. Its connection to its own port goes
/// through xtermium's relay (relay.rs), which trusts the BMC's certificate there.
pub const SCRIPT: &str = r#"
  const isConsole = (u) => /virtualconsolehtml5\.html/.test(String(u));
  // A page on the same BMC but another port: the console's own (kvmPort), where the console opens a
  // page for its certificate to be accepted before it can connect.
  const isOtherPort = (u) => {
    try {
      const x = new URL(String(u), location.href);
      return x.hostname === location.hostname && x.port !== location.port;
    } catch {
      return false;
    }
  };
  // Whichever frame opens the console, or that certificate page, it's handled in this tab (below).
  const open = window.open;
  window.open = function (u, ...rest) {
    if (isConsole(u) && top.__xtmConsole) return top.__xtmConsole(String(u));
    if (isOtherPort(u) && top.__xtmCert) return top.__xtmCert(String(u));
    return open.call(this, u, ...rest);
  };
  if (window.top !== window) {
    if (!isConsole(location.pathname)) return;
    if (!window.opener) Object.defineProperty(window, "opener", { value: window.parent, configurable: true });
    // The console's connection to its port goes through xtermium's relay (kvm/relay.rs), which
    // trusts the BMC's certificate there as it's trusted for the web interface.
    const WS = window.WebSocket;
    if (relay)
      window.WebSocket = Object.assign(
        function (url, protocols) {
          const u = new URL(url, location.href);
          if (u.protocol === "wss:" && isOtherPort(u)) url = `${relay}/${u.hostname}/${u.port || 443}${u.pathname}${u.search}`;
          return protocols === undefined ? new WS(url) : new WS(url, protocols);
        },
        { prototype: WS.prototype, CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 },
      );
    return;
  }
  // The page is a frameset (an iframe in one isn't drawn): the console is one more frame of it,
  // filling the tab; the others, and the certificate page's, shrink to nothing.
  const frameset = () => (document.body?.tagName === "FRAMESET" ? document.body : null);
  const frame = (id) => {
    const root = frameset();
    let f = document.getElementById(id);
    if (!f) {
      f = document.createElement("frame");
      Object.assign(f, { id, frameBorder: "0", scrolling: "no" });
      root.append(f);
    }
    const sizes = [...root.children].filter((c) => /^FRAME(SET)?$/.test(c.tagName)).map((c) => (c.id === "xtm-console" ? "*" : "0"));
    if (root.cols.includes(",")) root.cols = sizes.join(",");
    else root.rows = sizes.join(",");
    return f;
  };
  // The console asks for its port's certificate to be accepted in a pop-up when it can't connect;
  // the relay accepts it, so the pop-up is a stand-in that closes when told to.
  window.__xtmCert = () => ({ closed: false, onclose: null, focus() {}, close() { this.closed = true; this.onclose?.(); } });
  window.__xtmConsole = (u) => {
    if (!frameset()) return overlay(u, () => {}).contentWindow;
    document.getElementById("xtm-console")?.remove();
    const f = frame("xtm-console");
    // A new frame's first, blank page lends its window to the next same-site page: its opener,
    // set now, is there before the console's scripts read it.
    Object.defineProperty(f.contentWindow, "opener", { value: window, configurable: true });
    f.src = u;
    f.addEventListener("load", () => {
      const w = f.contentWindow;
      addEventListener("focus", () => w.focus());
      w.focus();
    });
    return f.contentWindow;
  };
  // Signing in: the page signs in by script through one request object it shares with its own
  // start-up requests, so pressing its Submit can lose the answer and wait forever. The same request,
  // made here, goes where the answer says: from `?console`, on to the console. Refused, the form is
  // left filled in for the person to try again.
  addEventListener("DOMContentLoaded", () => {
    const pw = passwordField();
    if (!pw || !pass || seen("tried") || !/\/login\.html$/.test(location.pathname)) return;
    mark("tried");
    fetch("/data/login" + location.search, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: "user=" + encodeURIComponent(user) + "&password=" + encodeURIComponent(pass),
    })
      .then((r) => r.text())
      .then((t) => {
        const x = new DOMParser().parseFromString(t, "text/xml");
        const to = x.querySelector("forwardUrl")?.textContent;
        if (x.querySelector("authResult")?.textContent === "0" && to) location.href = new URL(to, location.href).href;
        else fillIn(pw);
      })
      .catch(() => fillIn(pw));
  });
"#;
