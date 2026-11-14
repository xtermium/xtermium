<script lang="ts">
  import { tick, untrack } from "svelte";
  import { fade } from "svelte/transition";
  import { cubicIn } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { bindings, cell, chord, DIRECTIONS, EDGES, even, fill as fillIn, MOVES, neighbour, nudge, resize as resized, placeholders, quickConnect, tile, tileRows, zoomed, type Filter, type Sizes } from "./lib/actions";
  import { addressSession, broadcasts, COLORS, isDesktop, joinsBroadcast, newSession, openUrl, seeThrough, type CoopRemote, type Meta, type SavedLayout, type Session, type Snippet, type Update } from "./lib/api";
  import Backdrop from "./lib/Backdrop.svelte";
  import Guide, { guideRows, type Step } from "./lib/Guide.svelte";
  import Logo from "./lib/Logo.svelte";
  import Account from "./lib/Account.svelte";
  import Coop from "./lib/Coop.svelte";
  import Export from "./lib/Export.svelte";
  import Icon, { iconFile } from "./lib/Icon.svelte";
  import Import from "./lib/Import.svelte";
  import Modal from "./lib/Modal.svelte";
  import { menu } from "./lib/menu";
  import Palette from "./lib/Palette.svelte";
  import Rdp from "./lib/Rdp.svelte";
  import SessionEditor from "./lib/SessionEditor.svelte";
  import SessionIcon from "./lib/SessionIcon.svelte";
  import Settings, { openSettings } from "./lib/Settings.svelte";
  import LockScreen from "./lib/LockScreen.svelte";
  import Files from "./lib/Files.svelte";
  import History from "./lib/History.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SignIn from "./lib/SignIn.svelte";
  import Snippets from "./lib/Snippets.svelte";
  import Credentials from "./lib/Credentials.svelte";
  import StatusBar from "./lib/StatusBar.svelte";
  import { _, N_, setLanguage } from "./lib/i18n.svelte";
  import { app, boot, dismiss, guideDone, lock, lookOf, notify, saveStore, shortcut, tell, tip, toast, toasts, sessionKey, siteIcon, syncNow, takeRestore, workspaceOp, type SyncSide } from "./lib/state.svelte";
  import Terminal, { IGNORED } from "./lib/Terminal.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import WorkspacePicker from "./lib/WorkspacePicker.svelte";
  import Tunnels from "./lib/Tunnels.svelte";
  import Web from "./lib/Web.svelte";

  /** `connected`: a desktop's picture is showing (status bar). `label`, `color`: what the tab was
   *  renamed to and coloured in the tab strip, over its title and its session's colour. */
  type Tab = {
    key: number;
    session: Session;
    id: number;
    title: string;
    alive: boolean;
    broadcast: boolean;
    connected?: boolean;
    label?: string;
    color?: string;
    /** Running in the background: off the strip and the panes, listed under Tasks. */
    background?: boolean;
    /** Points added to its font size (Ctrl+= and Ctrl+- zoom one tab). */
    zoom?: number;
    /** Shown in a window of its own (Detached.svelte), listed under Tasks. */
    detached?: boolean;
    /** Its backend tab, still running, to show again instead of opening a new one. */
    attach?: number;
    /** Terminals: the shell's folder (Meta). */
    cwd?: string;
    /** A website's pop-up or new-tab link, from a tab the ring shares: shared too, once it has started. */
    share?: boolean;
  };
  /** "tiles": a pane for every ticked tab, sized to fit however many there are (broadcast ticks them all). */
  type Layout = "single" | "columns" | "rows" | "grid" | "tiles";
  type Prompt = { id: number; title: string; text: string; input: boolean | null; answer: string; remember: boolean };

  /** The layouts to pick from, and their columns and rows. */
  const GRID: Record<Exclude<Layout, "tiles">, [number, number]> = { single: [1, 1], columns: [2, 1], rows: [1, 2], grid: [2, 2] };
  const LAYOUTS: [Layout, string][] = [
    ["single", N_("Single")],
    ["columns", N_("Side by side")],
    ["rows", N_("Stacked")],
    ["grid", N_("2 × 2 grid")],
    ["tiles", N_("Tiles: tick the tabs to show side by side")],
  ];

  let ready = $state(false);
  let tabs = $state<Tab[]>([]);
  let layout = $state<Layout>("single");
  /** Tab key shown in each pane. */
  let panes = $state<(number | null)[]>([null]);
  /** Pane that has the keyboard. */
  let focus = $state(0);
  /** A pane edge is being dragged: the focus ring follows it at once instead of gliding. */
  let sizing = $state(false);
  /** The system asks for less motion; CSS motion obeys app.css, Svelte transitions read this. */
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let broadcasting = $state(false);
  let showFiles = $state(false);
  let showTunnels = $state(false);
  let showImport = $state(false);
  let showExport = $state(false);
  let showPalette = $state(false);
  /** The filter the palette opens on: Sessions when it's opened to find one. */
  let paletteFilter = $state<Filter | null>(null);
  let showSnippets = $state(false);
  /** A snippet waiting for its placeholders' values. */
  let asking = $state<{ snippet: Snippet; values: Record<string, string> } | null>(null);
  /** Broadcast was asked for with fewer than two terminals open. */
  let broadcastAlone = $state(false);
  let showSidebar = $state(true);
  /** A tab waiting for "close it anyway?". */
  let closing = $state<Tab | null>(null);
  let editor = $state<{ session: Session; index: number | null } | null>(null);
  let prompts = $state<Prompt[]>([]);
  let systemDark = $state(matchMedia("(prefers-color-scheme: dark)").matches);
  /** Terminal views by tab key, for commands that act on the focused one. */
  const views: Record<number, Terminal | undefined> = $state({});
  /** Remote desktop views by tab key (detaching one releases it). */
  const desks: Record<number, Rdp | undefined> = $state({});
  let nextKey = 1;

  boot().then(() => {
    ready = true;
    applyCustomCss();
    for (const [s, label] of takeRestore()) {
      const key = open(s);
      if (key !== undefined && label) update(key, { label });
    }
    for (const t of app.store.tunnel.filter((t) => t.autostart))
      invoke("tunnel_start", { name: t.name }).catch((e) => (app.status = `${t.name}: ${e}`));
  });
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => (systemDark = e.matches));
  listen<Omit<Prompt, "answer" | "remember">>("prompt", (e) => prompts.push({ ...e.payload, answer: "", remember: false }));
  // Its tab closed (or its tunnel stopped) first: nobody waits for the answer.
  listen<number>("prompt-gone", ({ payload: id }) => {
    const at = prompts.findIndex((p) => p.id === id);
    if (at >= 0) prompts.splice(at, 1);
  });
  // boot() loads the language before the first draw; this follows changes in Settings.
  $effect(() => void (ready && setLanguage(app.settings?.language ?? "")));

  // ---- appearance: theme, scale, translucency; settings are saved as they change ----
  const dark = $derived(app.settings?.theme === "Dark" || (app.settings?.theme === "System" && systemDark));
  $effect(() => {
    const s = app.settings;
    if (!s) return;
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    seeThrough(s);
    accent(s.accent);
  });
  // Apart, so dragging the opacity slider or the accent picker doesn't call the backend each step.
  $effect(() => void (app.settings && getCurrentWebview().setZoom(app.settings.ui_scale).catch(() => {})));
  $effect(() => void (app.settings && invoke("set_dark", { dark }).catch((e) => (app.status = String(e)))));
  /** A custom accent replaces the gold everywhere on dark. On the light theme it does what the
   *  amber does there: text and lines stay in ink, and the colour itself is the fill, the wash and
   *  the marks (`light-dark()` follows the theme's color-scheme, app.css). Text on it is black or
   *  white, whichever reads. */
  function accent(c: string) {
    const root = document.documentElement.style;
    for (const v of ["--gold", "--gold-ink", "--gold-soft", "--gold-fill", "--gold-fill-ink"]) root.removeProperty(v);
    if (!c) return;
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16) / 255);
    const light = 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.5;
    root.setProperty("--gold", `light-dark(#141414, ${c})`);
    root.setProperty("--gold-ink", `light-dark(#fff, ${light ? "#111" : "#fff"})`);
    root.setProperty("--gold-soft", `light-dark(color-mix(in srgb, ${c} 22%, transparent), color-mix(in srgb, ${c} 14%, transparent))`);
    // fills and marks take the colour itself in both themes
    root.setProperty("--gold-fill", c);
    root.setProperty("--gold-fill-ink", light ? "#111" : "#fff");
  }
  let saveTimer = 0;
  let loaded = false;
  $effect(() => {
    const settings = $state.snapshot(app.settings);
    if (!settings) return;
    if (!loaded) return void (loaded = true); // just loaded: nothing to save
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => invoke("save_settings", { settings }).catch((e) => (app.status = String(e))), 150);
  });
  $effect(() => {
    invoke("set_broadcast", { on: broadcasting });
  });
  // Open tabs are remembered (Settings > General > Reopen tabs).
  let tabsTimer = 0;
  $effect(() => {
    if (!ready) return;
    // A tab's colour goes with its session; the name it was given beside it.
    const kept = app.settings?.restore_tabs ? tabs.filter((t) => !coopTab(t)) : [];
    const list = kept.map((t) => {
      return { ...$state.snapshot(t.session), color: t.color ?? t.session.color };
    });
    const labels = kept.map((t) => t.label ?? "");
    clearTimeout(tabsTimer);
    tabsTimer = setTimeout(() => invoke("save_tabs", { tabs: list, labels }).catch((e) => (app.status = String(e))), 300);
  });

  // Broadcast outlines animate as one: every outline animation starts at the same moment, so
  // panes lit at different times still pulse together.
  $effect(() => {
    void [broadcasting, app.settings?.broadcast_outline, tabs.map((t) => t.broadcast), panes.slice()];
    tick().then(() => {
      for (const a of document.getAnimations())
        if (a instanceof CSSAnimation && a.animationName.startsWith("outline-")) a.startTime = 0;
    });
  });

  // ---- tabs and panes ----
  /** The tabs in the strip: all but the background and detached ones. */
  const strip = $derived(tabs.filter((t) => !t.background && !t.detached));
  /** Background and detached tabs, as Tasks lists them. */
  const tasks = $derived(tabs.filter((t) => t.background || t.detached));
  const count = (l: Layout) => (l === "tiles" ? Math.max(1, strip.length) : GRID[l][0] * GRID[l][1]);
  const activeTab = $derived(tabs.find((t) => t.key === panes[focus]));
  /** Saved sessions with a tab, by `sessionKey`: true while one of those tabs is up (the sidebar marks them). */
  const live = $derived(tabs.filter((t) => !coopTab(t)).reduce((m, t) => m.set(sessionKey(t.session), !!m.get(sessionKey(t.session)) || t.alive), new Map<string, boolean>()));

  /** Opens a tab and returns its key; undefined if the session opened outside xtermium. A tab that
   *  a website tab the ring shares opened (`from`: a pop-up, a link for a new tab, whoever clicked
   *  it, the owner or a guest driving the page) is shared with the ring as well. */
  function open(session: Session, from?: Tab): number | undefined {
    siteIcon(session);
    if (session.kind.type === "browser" && app.settings?.web === "External") {
      openUrl(session.kind.url).catch((e) => (app.status = String(e)));
      return;
    }
    const key = nextKey++;
    const share = !!from?.id && !!app.coop.ring?.owner && app.coop.shared.includes(from.id);
    tabs.push({ key, session, id: 0, title: session.name, alive: true, broadcast: true, share });
    toPane(key);
    return key;
  }

  // ---- Terminal Co-op (Coop.svelte): a tab for every tab the ring shares with you, opened and closed
  // as the `coop` event's list changes ----
  /** The number at its owner of the shared tab a tab shows; 0 for any other tab. */
  const coopTab = (t: Tab) => (t.session.kind.type === "coop" ? t.session.kind.tab : 0);
  const remoteName = (r: CoopRemote) => (r.owner_name ? `${r.owner_name}: ${r.title}` : r.title);
  /** Ring tabs closed by hand: they stay closed until their owner stops sharing them. */
  const hidden = new Set<number>();
  $effect(() => {
    const list = app.coop.remote;
    untrack(() => syncRemote(list));
  });

  function syncRemote(list: CoopRemote[]) {
    const ids = new Set(list.map((r) => r.id));
    for (const id of hidden) if (!ids.has(id)) hidden.delete(id);
    for (const t of tabs.filter((t) => coopTab(t) && !ids.has(coopTab(t)))) {
      close(t.key, true);
      hidden.delete(coopTab(t));
    }
    for (const r of list) {
      const t = tabs.find((t) => coopTab(t) === r.id);
      if (t) {
        t.title = r.title;
        t.session.name = remoteName(r);
      } else if (!hidden.has(r.id)) openRemote(r);
    }
  }

  /** Opens a tab shared with you. It takes an empty pane if there is one, but never the keyboard: what
   *  you are typing must not land in someone else's terminal. */
  function openRemote(r: CoopRemote) {
    const key = nextKey++;
    const session = newSession("coop", "", remoteName(r));
    session.kind = { type: "coop", tab: r.id, picture: String(r.kind).toLowerCase() === "picture", owner: r.owner_name ?? "" };
    tabs.push({ key, session, id: 0, title: r.title, alive: true, broadcast: false });
    const free = panes.indexOf(null);
    if (layout !== "tiles" && free >= 0) panes[free] = key;
    toast(_("{name} shared {title}.", { name: r.owner_name || _("Someone"), title: r.title }), panes.includes(key) ? undefined : { label: _("Show"), run: () => show(key) });
  }

  // The owner's ring ends when they have been away (coop.rs `idle_watch`). Windows and macOS say
  // when the computer last had input; elsewhere xtermium tells the backend of its own, once a minute.
  let activeAt = 0;
  function active() {
    if (!app.coop.ring?.owner || app.os === "windows" || app.os === "macos" || Date.now() - activeAt < 60_000) return;
    activeAt = Date.now();
    invoke("coop_active");
  }

  /** The ring's owner puts a tab of theirs in front of everyone in it, or takes it back. */
  function share(t: Tab, on: boolean) {
    invoke("coop_share", { tab: t.id, on }).catch((e) => (app.status = String(e)));
  }
  /** Why a tab can't be shared, for the menu's tooltip; "" when it can. */
  function unshareable(t: Tab) {
    const k = t.session.kind.type;
    if (k === "agent") return _("AI agent tabs can't be shared.");
    if (k === "browser" && app.os !== "windows") return _("Website tabs can be shared on Windows only.");
    return "";
  }

  /** A tab coming on screen: tiled, it gets a pane of its own; else an empty pane, if there is
   *  one, or the focused one. */
  function toPane(key: number) {
    if (layout === "tiles") panes = [...panes.filter((p) => p !== null), key];
    else panes[panes.includes(null) ? panes.indexOf(null) : focus] = key;
    focus = panes.indexOf(key);
    tick().then(refocus);
  }

  // Automation (Settings > MCP) opens sessions here and gets the tab id once it exists.
  const automationOpens = new Map<number, number>(); // tab key → request
  // `session`: saved or a shell; `address`: "user@host" and the like, as the palette takes them.
  listen<{ req: number; session: Session | null; address: string | null }>("automation-open", (e) => {
    const addr = e.payload.address === null ? null : quickConnect(e.payload.address);
    const session = e.payload.session ?? (addr && addressSession(addr));
    const key = session ? open(session) : undefined;
    if (key === undefined) invoke("automation_opened", { req: e.payload.req, id: 0 });
    else automationOpens.set(key, e.payload.req);
  });
  // "Open in xtermium" on a folder in File Explorer, while xtermium was running: a shell there.
  listen<Session>("open-here", (e) => open(e.payload));
  // A newer release is out (src/support.rs, Settings › About): a note, and a desktop notification.
  listen<Update>("update", (e) => {
    const u = e.payload;
    const text = _("xtermium {version} is out.", { version: u.version });
    toast(text, { label: _("Get it"), run: () => openUrl(u.download ?? u.page) });
    notify(_("xtermium update"), text);
  });
  /** Commands to type into a shell once it's up (Settings › MCP "Run", the start screen's prompt), by tab key. */
  const typeWhenUp = new Map<number, string>();
  /** Opens a shell (`program`, or "" for Local shell's) and types `command` into it once it's up. */
  function runInShell(command: string, program = "") {
    const key = program ? open({ ...newSession("shell", "", "local"), kind: { type: "shell", program } }) : localShell();
    if (key !== undefined && command) typeWhenUp.set(key, command);
    return key;
  }

  // ---- the start screen's prompt (Settings › General): with no tabs open, what you type shows
  // after `~/xtermium $`; Enter opens a shell (the one Settings › General names, else Local shell's)
  // and runs it there ----
  /** What has been typed at the prompt; null until a first character, and after Escape. */
  let draft = $state<string | null>(null);
  /** The shell Enter opened: its view stays unseen until the shell can take keys (`up`) and the
   *  welcome has gone (`clear`); only then does it fade in, so the two never blur into each other. */
  let arriving = $state<{ key: number; up: boolean; clear: boolean } | null>(null);
  /** The welcome's exit, in ms: faster than the terminal's 200 ms entrance (app.css motion rules),
   *  and accelerating away, so it never lingers half-faded like the window's stepped-back look. */
  const LEAVE = 120;
  // A tab opened some other way takes the prompt's text with it.
  $effect(() => void (strip.length && draft !== null && (draft = null)));

  /** A key no shortcut took. On the start screen, a character typed outside any field goes to the
   *  prompt; Backspace takes one back, Escape clears it, Enter runs it. */
  function quickKey(e: KeyboardEvent) {
    if (e.isComposing || IGNORED.has(e.key)) return;
    if (strip.length || app.settings?.quick_shell === false || document.querySelector(".backdrop, .menu")) return;
    const el = e.target as HTMLElement;
    // A field takes its own typing.
    if (el.closest?.("input, textarea, select, [contenteditable]")) return;
    const altGr = e.getModifierState("AltGraph") || (e.ctrlKey && e.altKey && e.key.length === 1);
    const chord = (e.ctrlKey || e.altKey || e.metaKey) && !altGr;
    if (e.key.length === 1 && !chord) {
      // Space presses the button or row that has the keyboard.
      if (e.key === " " && draft === null && el !== document.body) return;
      draft = (draft ?? "") + e.key;
    } else if (draft === null || chord) return;
    else if (e.key === "Backspace") draft = draft.slice(0, -1);
    else if (e.key === "Escape") draft = null;
    else if (e.key === "Enter") {
      const command = draft.trim();
      draft = null;
      const key = runInShell(command, app.settings?.start_shell ?? "");
      if (key !== undefined) arriving = { key, up: false, clear: false };
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  /** A terminal's meta: the tab's state. A connection that dropped becomes a desktop notification
   *  (Settings › General). */
  function metaOf(t: Tab, m: Meta) {
    if (m.lost && t.alive && !coopTab(t)) notify(_("Connection lost"), titleOf(t));
    update(t.key, { alive: m.alive, title: m.title, cwd: m.cwd });
  }

  // custom.css in the data folder (README › Configuration) restyles the interface. It's applied
  // at start; one that changed since it was last kept is tried for 30 seconds and taken out
  // unless you keep it, so a stylesheet that hides the window can't lock you out.
  let customCss = $state<{ hash: string; left: number } | null>(null);
  let customTimer = 0;
  async function applyCustomCss() {
    const got = await invoke<[string, string] | null>("custom_css").catch(() => null);
    if (!got) return;
    const [css, hash] = got;
    const el = document.createElement("style");
    el.id = "custom-css";
    el.textContent = css;
    document.head.append(el);
    if (hash === app.settings?.custom_css) return;
    customCss = { hash, left: 30 };
    customTimer = setInterval(() => customCss && --customCss.left <= 0 && keepCustomCss(false), 1000);
  }
  function keepCustomCss(keep: boolean) {
    clearInterval(customTimer);
    if (keep && app.settings && customCss) app.settings.custom_css = customCss.hash;
    else document.getElementById("custom-css")?.remove();
    customCss = null;
    refocus();
  }
  listen<{ tab: number }>("automation-close", (e) => {
    const t = tabs.find((t) => t.id === e.payload.tab);
    if (t) close(t.key, true);
  });
  // A website's pop-up opens as a tab of its own, with the same login (and SSH gateway).
  listen<{ id: number; url: string }>("web-popup", (e) => {
    const tab = tabs.find((t) => t.id === e.payload.id);
    const from = tab?.session;
    if (from?.kind.type !== "browser") return;
    open({ ...$state.snapshot(from), name: new URL(e.payload.url).host, kind: { ...$state.snapshot(from.kind), url: e.payload.url } }, tab);
  });

  /** A tab's backend is up: remember its id, and answer automation if it asked for this tab. */
  function started(key: number, id: number) {
    update(key, { id });
    const t = tabs.find((t) => t.key === key);
    if (t?.share) {
      t.share = false;
      share(t, true);
    }
    if (arriving?.key === key) arriving.up = true;
    const command = typeWhenUp.get(key);
    if (command) {
      typeWhenUp.delete(key);
      // The shell reads it once it's up; the pty keeps it until then.
      setTimeout(() => invoke("term_text", { id, text: `${command}\n` }), 500);
    }
    const req = automationOpens.get(key);
    if (req !== undefined) {
      automationOpens.delete(key);
      invoke("automation_opened", { req, id });
    }
  }

  /** Show a tab in the focused pane, or focus the pane already showing it; a background tab
   *  comes back to the strip. */
  function show(key: number) {
    if (tabs.find((t) => t.key === key)?.detached) return reattach(key);
    update(key, { background: false });
    const p = panes.indexOf(key);
    if (p >= 0) focus = p;
    else panes[focus] = key;
    tick().then(refocus);
  }

  /** Put tabs that aren't visible into empty panes. */
  function fill() {
    const hidden = strip.map((t) => t.key).filter((k) => !panes.includes(k));
    panes = panes.map((p) => p ?? hidden.shift() ?? null);
  }

  /** The tab that had the keyboard stays on screen and keeps it. */
  function setLayout(l: Layout) {
    if (l !== "single") guideDone("split");
    const was = panes[focus];
    layout = l;
    const n = count(l);
    panes = Array.from({ length: n }, (_, i) => panes[i] ?? null);
    if (was != null && !panes.includes(was)) panes[n - 1] = was;
    focus = Math.max(0, panes.indexOf(was ?? null));
    fill();
    tick().then(refocus);
  }

  /** Tiled: a tab gets a pane of its own, or gives its pane back (the last one stays). */
  function toggleTile(key: number) {
    const was = panes[focus];
    const adding = !panes.includes(key);
    if (adding) panes = [...panes.filter((p) => p !== null), key];
    else if (panes.length > 1) panes = panes.filter((p) => p !== key);
    focus = Math.max(0, panes.indexOf(adding ? key : was));
    tick().then(refocus);
  }

  /** Close a tab; one still connected asks first (Settings > General), unless `force`. */
  function close(key: number, force = false) {
    const t = tabs.find((t) => t.key === key);
    if (!force && t?.alive && app.settings?.confirm_close && !coopTab(t)) return void (closing = t);
    // Its view isn't here to end it: this does, and closes its window.
    if (t?.detached) invoke("close_tab", { id: t.id });
    if (t && coopTab(t)) hidden.add(coopTab(t));
    tabs = tabs.filter((t) => t.key !== key);
    unpane(key);
  }

  /** A tab leaves the panes (closed, to the background, to its own window): tiled, its pane goes
   *  too (the last one stays, empty). With one tab left, or none, the split ends: there is nothing
   *  to put beside it, and the layout buttons only show with two tabs. */
  function unpane(key: number) {
    panes = layout === "tiles" && strip.length ? panes.filter((p) => p !== key) : panes.map((p) => (p === key ? null : p));
    focus = Math.min(focus, panes.length - 1);
    if (strip.length <= 1 && layout !== "single") return setLayout("single");
    fill();
    tick().then(refocus);
  }

  /** Close every tab in the strip but `key` (`right`: only those after it). */
  function closeOthers(key: number, right = false) {
    const from = strip.findIndex((t) => t.key === key);
    const gone = strip.filter((t, i) => t.key !== key && (!right || i > from));
    // Asked for by name from the menu: no "close it anyway?" for each one. What was still
    // connected is named afterwards, with a way to connect again.
    const live = gone.filter((t) => t.alive);
    for (const t of gone) close(t.key, true);
    if (live.length)
      toast(
        live.length === 1 ? _("Closed {name}, which was still connected.", { name: titleOf(live[0]) }) : _("Closed {n} tabs that were still connected.", { n: live.length }),
        { label: _("Reopen"), run: () => live.forEach((t) => open(t.session)) },
      );
  }

  // ---- more tabs than the strip shows: it scrolls (the wheel, Ctrl+Tab, Alt+1–9), fades at an end
  // with tabs past it, and All tabs lists every one. Its scrollbar stays hidden: a bar under 34px
  // of tabs would crowd them, so the fade says there's more instead.
  let tabsEl = $state<HTMLElement>();
  /** Whether tabs are hidden past the strip's start and end, in reading order. */
  let tabsOver = $state({ start: false, end: false });
  let allTabs = $state<{ x: number; y: number } | null>(null);
  function measureTabs() {
    const el = tabsEl;
    if (!el) return;
    const at = Math.abs(el.scrollLeft); // 0 down to negative, right to left
    const room = el.scrollWidth - el.clientWidth;
    const over = { start: at > 1, end: at < room - 1 };
    if (over.start !== tabsOver.start || over.end !== tabsOver.end) tabsOver = over;
  }
  // The strip's width and every tab's (titles change as shells run) can bring tabs in or out of view.
  $effect(() => {
    const el = tabsEl;
    void strip.length;
    if (!el) return;
    const ro = new ResizeObserver(measureTabs);
    ro.observe(el);
    for (const c of el.children) ro.observe(c);
    return () => ro.disconnect();
  });
  $effect(() => {
    if (allTabs && !strip.length) allTabs = null;
  });
  /** The arrow over a faded end: the strip glides a page that way (`way`: -1 toward the start, 1
   *  toward the end, in reading order). A page that would carry the tab in use out of view stops
   *  short, with it at the far side (clear of the arrow and fade there), so you see where you were
   *  before leaving it; the next click carries on past it. */
  function stepTabs(way: -1 | 1) {
    const el = tabsEl;
    if (!el) return;
    const rtl = document.documentElement.dir === "rtl";
    let by = Math.max(el.clientWidth - 2 * 55, 55);
    const a = el.querySelector<HTMLElement>(".tab.active");
    if (a) {
      const pad = parseFloat(getComputedStyle(el).scrollPaddingInlineStart) || 0;
      const v = el.getBoundingClientRect();
      const r = a.getBoundingClientRect();
      // Toward the end the tabs move toward the start, so the tab in use may go as far as the
      // start's padding; toward the start, as far as the end's.
      const toLeft = way > 0 !== rtl;
      const room = toLeft ? r.left - (v.left + pad) : v.right - pad - r.right;
      // Already at that side, or past it: a whole page, so the click always moves the strip.
      if (room >= 21) by = Math.min(by, room);
    }
    el.scrollBy({ left: way * (rtl ? -1 : 1) * by, behavior: still ? "auto" : "smooth" });
  }

  // ---- background tasks: a tab off the strip and the panes, still running, listed under Tasks
  // and brought back into a tab from there (or the palette). One that ends there stays, to be read.
  let showTasks = $state<{ x: number; y: number } | null>(null);
  function toBackground(key: number) {
    const t = tabs.find((t) => t.key === key);
    if (!t) return;
    t.background = true;
    // Out of sight, out of broadcast.
    if (t.broadcast) {
      t.broadcast = false;
      invoke("set_tab_broadcast", { id: t.id, on: false });
    }
    unpane(key);
  }

  /** Terminals and RDP desktops move to a window of their own; VNC, websites and consoles don't. */
  const detachable = (k: Session["kind"]) => k.type === "rdp" || !(isDesktop(k) || k.type === "browser" || k.type === "kvm" || k.type === "coop");

  // ---- detached tabs: a terminal in a window of its own (Detached.svelte), its connection kept.
  // Closing that window, its menu's "Reattach", or Tasks here bring it back.
  async function detach(key: number) {
    const t = tabs.find((t) => t.key === key);
    if (!t) return;
    const k = t.session.kind;
    if (!t.id || !detachable(k)) return void (app.status = _("Only terminal and RDP tabs can move to a window of their own."));
    const box = document.querySelector(`.view[data-pane="${panes.indexOf(key)}"]`)?.getBoundingClientRect();
    const scale = app.settings?.ui_scale ?? 1;
    // The new window has a title bar of its own over the view: the view keeps its size.
    const bar = document.querySelector<HTMLElement>(".titlebar")?.offsetHeight ?? 0;
    try {
      await invoke("detach_tab", {
        id: t.id,
        title: titleOf(t),
        state: { id: t.id, title: titleOf(t), zoom: t.zoom ?? 0, session: $state.snapshot(t.session) },
        width: (box?.width ?? 800) * scale,
        height: ((box?.height ?? 500) + bar) * scale,
      });
    } catch (e) {
      return void (app.status = String(e));
    }
    (views[key] ?? desks[key])?.release();
    Object.assign(t, { detached: true, attach: t.id });
    unpane(key);
  }

  /** Back from its window into a pane; it brings its font zoom along. */
  function reattach(key: number, zoom?: number) {
    const t = tabs.find((t) => t.key === key);
    if (!t?.detached) return;
    Object.assign(t, { detached: false, background: false }, zoom === undefined ? {} : { zoom });
    toPane(key);
  }
  listen<{ id: number; zoom?: number }>("reattach", (e) => {
    const t = tabs.find((t) => t.id === e.payload.id);
    if (t) reattach(t.key, e.payload.zoom);
  });
  // Automation opens a saved layout by name.
  listen<string>("automation-layout", (e) => {
    const l = app.store.layout.find((l) => l.name === e.payload);
    if (l) openLayout(l);
  });
  // Automation's detach and attach (src/automation.rs).
  listen<{ tab: number; on: boolean }>("automation-detach", (e) => {
    const t = tabs.find((t) => t.id === e.payload.tab);
    if (t) (e.payload.on ? detach : reattach)(t.key);
  });
  // Ended there, or closed from its menu.
  listen<number>("detached-close", (e) => {
    const t = tabs.find((t) => t.id === e.payload);
    if (t) close(t.key, true);
  });

  // ---- saved layouts: sessions opened together in a pane arrangement, one click (the tab bar's
  // Layouts menu, or the palette). Stored with the workspace's sessions.
  let layoutsMenu = $state<{ x: number; y: number } | null>(null);
  /** The name a layout is being saved under, while asking for it. */
  let naming = $state<string | null>(null);
  /** The tabs on screen, in tab-strip order: what a layout saves. */
  const onScreen = $derived(strip.filter((t) => panes.includes(t.key) && !coopTab(t)));

  /** Saves the tabs on screen: their sessions in strip order, the pane each is in, the names
   *  they were given, and the panes' sizes. */
  function saveLayout(name: string) {
    name = name.trim();
    if (!name || !onScreen.length) return;
    const l: SavedLayout = {
      name,
      arrangement: layout,
      pane: onScreen.map((t) => $state.snapshot(t.session)),
      at: onScreen.map((t) => panes.indexOf(t.key)),
      label: onScreen.map((t) => t.label ?? ""),
      sizes: $state.snapshot(current),
    };
    const at = app.store.layout.findIndex((x) => x.name === name);
    if (at >= 0) app.store.layout[at] = l;
    else app.store.layout.push(l);
    saveStore();
    naming = null;
    app.status = _("Layout {name} saved: {n} sessions.", { name, n: l.pane.length });
  }

  /** Opens each session in the pane it was in, in the layout's arrangement, its tab named as it
   *  was and the panes sized as they were; tabs already open stay open, out of the panes. A session
   *  saved since takes the place of the layout's copy. */
  function openLayout(l: SavedLayout) {
    layoutsMenu = null;
    setLayout(l.arrangement as Layout);
    panes = layout === "tiles" ? [] : panes.map(() => null);
    focus = 0;
    const keys = l.pane.map((s, i) => {
      const key = open(app.store.session.find((x) => sessionKey(x) === sessionKey(s)) ?? s);
      const t = tabs.find((t) => t.key === key);
      if (t && l.label?.[i]) t.label = l.label[i];
      return key;
    });
    // Each back in its own pane (open() filled them one after another); tiles have no places.
    const at = l.at;
    if (layout !== "tiles" && at?.length === keys.length) {
      panes = panes.map(() => null);
      keys.forEach((k, i) => k !== undefined && at[i] < panes.length && (panes[at[i]] = k));
      focus = Math.max(0, panes.findIndex((p) => p !== null));
    }
    // The saved sizes, if they fit this arrangement's shape.
    const s = l.sizes;
    if (s && s.rows.length === shape.length && s.cols.every((c, r) => c.length === shape[r])) sizes[shape.join()] = s;
  }

  /** Deletes a saved layout at once (one click on its bin), with Undo for a while, as a session's delete. */
  function deleteLayout(name: string) {
    const at = app.store.layout.findIndex((l) => l.name === name);
    if (at < 0) return;
    const [gone] = app.store.layout.splice(at, 1);
    saveStore();
    toast(_("Deleted {name}.", { name }), {
      label: _("Undo"),
      run: () => {
        app.store.layout.splice(Math.min(at, app.store.layout.length), 0, gone);
        saveStore();
      },
    });
  }

  // ---- workspaces: each with its own sessions, logins and tunnels (Settings > Workspaces) ----
  /** The title bar's workspace picker (WorkspacePicker.svelte), hung under the field, as wide as it. */
  let workspaceMenu = $state<{ x: number; y: number; w: number } | null>(null);
  /** Opens the picker under the field the chevron `e` came from; a second click closes it. */
  function toggleWorkspaces(e: MouseEvent) {
    e.stopPropagation();
    if (workspaceMenu) return void (workspaceMenu = null);
    const r = (e.currentTarget as HTMLElement).parentElement!.getBoundingClientRect();
    workspaceMenu = { x: document.documentElement.dir === "rtl" ? r.right : r.left, y: r.bottom + 3, w: r.width };
  }
  /** Opens a menu under the button `e` came from, at its start; a second click closes it. */
  const below = (e: MouseEvent, was: { x: number; y: number } | null) => {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return was ? null : { x: document.documentElement.dir === "rtl" ? r.right : r.left, y: r.bottom + 3 };
  };

  // ---- dragging: a tab along the strip (its place) or onto a pane, a pane's status bar onto
  // another pane. Pointer events, not the page's drag and drop: the window takes dropped files for
  // the file panel (Files.svelte), and with that on the webview never delivers a drop.
  /** The tab being dragged; the pane under it, or the place in the strip it would go. */
  let dragging = $state<number | null>(null);
  let dropOn = $state<number | null>(null);
  let dropAt = $state<number | null>(null);
  /** A drag just ended: the click it ends with isn't a click on the tab. */
  let dragged = false;

  /** Watches a press on a tab (`fromStrip`) or a status bar: once it moves, it's a drag. */
  function dragStart(e: PointerEvent, key: number, fromStrip: boolean) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("input, .close")) return;
    const el = e.currentTarget as HTMLElement;
    const [x0, y0] = [e.clientX, e.clientY];
    dragged = false;
    const move = (m: PointerEvent) => {
      if (dragging === null) {
        if (Math.hypot(m.clientX - x0, m.clientY - y0) < 6) return;
        // Held from here on, even over a website's view in another pane.
        el.setPointerCapture(m.pointerId);
        [dragging, dragged] = [key, true];
      }
      const under = document.elementFromPoint(m.clientX, m.clientY);
      const pane = under?.closest<HTMLElement>(".drop");
      dropOn = pane ? Number(pane.dataset.pane) : null;
      const row = fromStrip ? under?.closest(".tabs") : null;
      if (!row) return void (dropAt = null);
      // Before the first tab whose middle is past the pointer.
      const at = [...row.querySelectorAll(".tab")].findIndex((t) => {
        const r = t.getBoundingClientRect();
        return m.clientX < r.left + r.width / 2;
      });
      dropAt = at < 0 ? strip.length : at;
    };
    const end = (u: PointerEvent) => {
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", end);
      removeEventListener("pointercancel", end);
      if (dragging !== null && u.type === "pointerup") {
        if (dropAt !== null) reorder(dragging, dropAt);
        else if (dropOn !== null) drop(dragging, dropOn);
      }
      [dragging, dropOn, dropAt] = [null, null, null];
      // The click comes right after the release, if at all.
      setTimeout(() => (dragged = false));
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", end);
    addEventListener("pointercancel", end);
  }

  /** The tab takes the pane; shown in another one, it swaps places with what was there. */
  function drop(key: number, pane: number) {
    const from = panes.indexOf(key);
    if (from >= 0) panes[from] = panes[pane];
    panes[pane] = key;
    focus = pane;
    tick().then(refocus);
  }

  /** Moves a tab to place `at` in the strip (counted before it moves): before the tab there. */
  function reorder(key: number, at: number) {
    const t = tabs.find((t) => t.key === key)!;
    const before = strip[at];
    if (before === t) return;
    tabs.splice(tabs.indexOf(t), 1);
    tabs.splice(before ? tabs.indexOf(before) : tabs.length, 0, t);
    tick().then(refocus);
  }

  // ---- renaming and colouring tabs: the strip's right-click menu, or a double-click ----
  let renaming = $state<number | null>(null);
  /** Takes the new name; empty goes back to the title the tab had. */
  function rename(t: Tab, name: string) {
    if (renaming !== t.key) return;
    t.label = name.trim() || undefined;
    renaming = null;
    tick().then(refocus);
  }
  /** A tab is called what you called it (renamed, or its session's name), so two shells or two
   *  servers never look alike; the title the shell or page sets is only the fallback. */
  const titleOf = (t: Tab) => t.label || t.session.name || t.title;
  /** Tooltip: the name, its folder, then the shell's or page's own title when it says more. */
  const tipOf = (t: Tab) => [titleOf(t), t.session.folder, t.title && t.title !== titleOf(t) ? t.title : ""].filter(Boolean).join("\n");

  // ---- the tray menu (src/tray.rs): every open tab, background ones marked, each with its icon ----
  listen<number>("tray-show", (e) => show(e.payload));
  /** A session's icon as a small PNG, as the tray menu takes it; drawn once per icon and colour. */
  const trayIcons = new Map<string, Promise<string>>();
  function trayIcon(icon: string, color: string): Promise<string> {
    const c = color.startsWith("var(") ? getComputedStyle(document.documentElement).getPropertyValue(color.slice(4, -1)).trim() : color;
    const k = `${icon}|${c}`;
    if (!trayIcons.has(k))
      trayIcons.set(
        k,
        (async () => {
          const img = new Image();
          img.src = icon.startsWith("data:") ? icon : "data:image/svg+xml;charset=utf-8," + encodeURIComponent(iconFile(icon, c || "#8a96a3", 32));
          await img.decode();
          const canvas = document.createElement("canvas");
          canvas.width = canvas.height = 32;
          canvas.getContext("2d")!.drawImage(img, 0, 0, 32, 32);
          return canvas.toDataURL("image/png");
        })().catch(() => ""),
      );
    return trayIcons.get(k)!;
  }
  let trayTimer = 0;
  $effect(() => {
    const list = tabs.map((t) => ({ key: t.key, name: titleOf(t), background: !!t.background, look: lookOf(t.session) }));
    const labels = { open: _("Open"), tasks: _("Tasks"), none: _("No tasks"), background: _("background"), exit: _("Exit") };
    clearTimeout(trayTimer);
    trayTimer = setTimeout(async () => {
      // the icon in its kind's colour, as in the tab strip
      const tasks = await Promise.all(list.map(async ({ look, ...t }) => ({ ...t, icon: await trayIcon(look.icon, look.color) })));
      invoke("tray_tasks", { tasks, labels }).catch((e) => (app.status = String(e)));
    }, 400);
  });

  /** The tab strip's right-click menu. */
  let tabMenu = $state<{ x: number; y: number; key: number } | null>(null);

  /** Connect again: a new key remounts the tab's view, which drops the old connection. */
  function reconnect(key: number) {
    const t = tabs.find((t) => t.key === key);
    if (!t) return;
    const k = nextKey++;
    panes = panes.map((p) => (p === key ? k : p));
    Object.assign(t, { key: k, id: 0, alive: true, title: t.session.name, attach: undefined });
  }

  /** Shells, agents and SSH sessions open their folder in VS Code; an SSH session's on its host. */
  const inCode = (k: Session["kind"]) => k.type === "ssh" || k.type === "shell" || k.type === "agent";
  function openInCode(t: Tab) {
    invoke("open_in_code", { id: t.id, cwd: t.cwd ?? "" })
      .then(() => toast(_("Opening in VS Code…")))
      .catch((e) => (app.status = String(e)));
  }

  /** Show the next (or previous) tab after the focused one. */
  function cycle(by: number) {
    if (!strip.length) return;
    const i = strip.findIndex((t) => t.key === panes[focus]);
    show(strip[(i + by + strip.length) % strip.length].key);
  }

  const update = (key: number, patch: Partial<Tab>) => {
    const t = tabs.find((t) => t.key === key);
    if (t) Object.assign(t, patch);
  };

  /** Panes per row, top to bottom. */
  const shape = $derived(layout === "tiles" ? tileRows(panes.length) : Array<number>(GRID[layout][1]).fill(GRID[layout][0]));
  /** Pane sizes dragged by hand, per shape (2 side by side, 2 × 2…); even until then. */
  let sizes = $state<Record<string, Sizes>>({});
  const current = $derived(sizes[shape.join()] ?? even(shape));

  /** Pane rectangle in percent: [left, top, width, height]. */
  const rect = (pane: number) => cell(shape, current, pane);

  /** Drags the line before row `row` (`col` -1) or before pane `col` of row `row`. */
  function resize(e: PointerEvent, row: number, col: number) {
    const handle = e.currentTarget as HTMLElement;
    const area = handle.parentElement!.getBoundingClientRect();
    const [key, start] = [shape.join(), $state.snapshot(current)];
    handle.setPointerCapture(e.pointerId);
    sizing = true;
    const move = (m: PointerEvent) => {
      if (!m.buttons) return end();
      const s = structuredClone(start);
      if (col < 0) s.rows = nudge(s.rows, row, (m.clientY - e.clientY) / area.height);
      else s.cols[row] = nudge(s.cols[row], col, (m.clientX - e.clientX) / area.width);
      sizes[key] = s;
    };
    const end = () => {
      sizing = false;
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", end);
      removeEventListener("pointercancel", end);
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", end);
    addEventListener("pointercancel", end);
  }
  const gap = $derived(panes.length === 1 ? 0 : 3);
  // The focused pane's tab stays in view in the strip (it scrolls when the tabs don't fit).
  $effect(() => {
    void panes[focus];
    // Gliding, so with tabs of one name you still see which way the strip went.
    tick().then(() => document.querySelector(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: still ? "auto" : "smooth" }));
  });

  const place = (pane: number) => {
    const [l, t, w, h] = rect(pane);
    return `left: calc(${l}% + ${gap}px); top: calc(${t}% + ${gap}px); width: calc(${w}% - ${2 * gap}px); height: calc(${h}% - ${2 * gap}px)`;
  };

  // ---- dialogs ----
  const createSession = () => (editor = { session: newSession(), index: null });
  const localShell = () => (guideDone("shell"), open(newSession("shell", "", "local")));

  function answer(ok: boolean) {
    const p = prompts.shift()!;
    invoke("prompt_reply", { id: p.id, answer: ok ? [p.answer, p.remember] : null });
  }

  /** Give the keyboard back to the focused pane's terminal or remote desktop (an empty pane
   *  passes it to the first one showing a tab). */
  function refocus() {
    if (panes[focus] == null) focus = Math.max(0, panes.findIndex((p) => p !== null));
    // Once a dialog that just closed is gone: while one is up, the terminal hands keys to it.
    tick().then(() => document.querySelector<HTMLElement>(`.view[data-pane="${focus}"] :is(.sink, [tabindex])`)?.focus());
  }

  /** The focused tab, if it's a terminal that's up: snippets run there. */
  const snippetTarget = $derived(activeTab?.id && activeTab.alive && broadcasts(activeTab.session.kind) ? activeTab : undefined);

  /** Types a snippet into the focused terminal (broadcast passes it on); asks for its
   *  placeholders' values first. */
  function runSnippet(snippet: Snippet, values?: Record<string, string>) {
    const t = snippetTarget;
    if (!t)
      return void (app.status =
        activeTab && !activeTab.alive ? _("This tab's session has stopped: press R in it to start it again.") : _("Focus a terminal tab to run a snippet in it."));
    const names = placeholders(snippet.text);
    if (names.length && !values) return void (asking = { snippet, values: Object.fromEntries(names.map((n) => [n, ""])) });
    [showSnippets, asking] = [false, null];
    invoke("term_text", { id: t.id, text: fillIn(snippet.text, values ?? {}) + (snippet.run ? "\n" : "") });
    tick().then(refocus);
  }

  // ---- every field: control characters never get in, numbers stay in range ----
  // (The backend cleans what it stores too: src/clean.rs.)

  /** Runs before the field's own handlers, so bindings only ever see the cleaned text. */
  function cleanInput(e: Event) {
    const el = e.target;
    const multiline = el instanceof HTMLTextAreaElement;
    // The terminal's input sink carries keystrokes, not field text.
    if (!(el instanceof HTMLInputElement || multiline) || el.classList.contains("sink")) return;
    if (el instanceof HTMLInputElement && !["text", "password", "search", "url", "email"].includes(el.type)) return;
    const bad = multiline ? /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g : /[\u0000-\u001f\u007f-\u009f]/g;
    const clean = el.value.replace(bad, "");
    if (clean !== el.value) {
      const at = Math.max(0, (el.selectionStart ?? clean.length) - (el.value.length - clean.length));
      el.value = clean;
      el.setSelectionRange(at, at);
    }
  }

  /** Number fields snap into their min and max once left (empty stays empty: some mean "default"). */
  function clampNumber(e: Event) {
    const el = e.target;
    if (!(el instanceof HTMLInputElement) || el.type !== "number" || el.value === "") return;
    const min = el.min === "" ? -Infinity : +el.min;
    const max = el.max === "" ? Infinity : +el.max;
    const v = el.valueAsNumber;
    const fixed = Number.isNaN(v) ? (Number.isFinite(min) ? min : 0) : Math.min(max, Math.max(min, v));
    if (fixed !== v) {
      el.value = String(fixed);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }
  }

  // ---- keyboard shortcuts and the command palette (actions.ts) ----
  const keymap = $derived(bindings(app.settings?.shortcuts ?? {}));

  const DIALOGS = new Set(["palette", "settings", "snippets", "tunnels", "credentials", "coop"]);

  // A website with the keyboard hands xtermium's shortcuts back (src/web.rs, Windows).
  $effect(() => void (app.settings && invoke("web_shortcuts", { chords: [...keymap.keys()] })));
  listen<string>("web-key", ({ payload }) => {
    const id = keymap.get(payload);
    if (id && !document.querySelector(".backdrop")) run(id);
  });

  /** Runs before the terminal sees the key, so a bound chord never reaches it. */
  function keydown(e: KeyboardEvent) {
    if (!ready || app.locked !== null || lock.locked || (e.target as HTMLElement).closest?.("[data-record]")) return;
    const id = keymap.get(chord(e));
    if (!id) return quickKey(e);
    // In a text field (not the terminal's), arrows and the like edit the text: Ctrl+Shift+← there
    // selects a word rather than swapping panes.
    const field = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
    if (field && !(e.target as HTMLElement).classList.contains("sink") && /^(Arrow|Home$|End$|Page)/.test(e.key)) return;
    // A dialog is open: only the shortcuts that open or close dialogs act, not ones that would
    // close or change tabs behind it.
    if (document.querySelector(".backdrop") && !DIALOGS.has(id)) return;
    // No pane that way: the keys stay the terminal's (Alt+arrows move by word in some shells).
    if ((DIRECTIONS[id] ?? MOVES[id]) && toward(DIRECTIONS[id] ?? MOVES[id]) < 0) return;
    // No pane that way to share an edge with: the keys stay the terminal's.
    if (EDGES[id] && !paneResized(EDGES[id])) return;
    e.preventDefault();
    e.stopPropagation();
    run(id);
  }

  /** Broadcast needs two or more tabs it reaches: terminals, remote desktops, BMC consoles. */
  const canBroadcast = $derived(broadcasting || tabs.filter((t) => joinsBroadcast(t.session.kind)).length >= 2);
  /** Ticked tabs of more than one kind: terminals, remote desktops and consoles take keys differently. */
  const mixed = $derived(
    new Set(
      tabs.filter((t) => t.broadcast && joinsBroadcast(t.session.kind)).map((t) => (broadcasts(t.session.kind) ? "term" : t.session.kind.type === "kvm" ? "kvm" : "desktop")),
    ).size > 1,
  );
  $effect(() => {
    if (broadcasting && mixed)
      app.status = _(
        "Broadcast is typing into different kinds of tabs. Terminals, remote desktops and BMC consoles handle keys differently, so some keys may come out wrong. What you type in a BMC console isn't sent to the others.",
      );
  });
  /** Tabs in the panes that have files to show (SFTP for SSH and VNC, the host's shares for RDP),
   *  in pane order. */
  const withFiles = $derived(
    panes
      .map((k) => tabs.find((t) => t.key === k))
      .filter((t): t is Tab => !!t?.id && t.alive && ["ssh", "rdp", "vnc"].includes(t.session.kind.type)),
  );
  /** File panels open at once, whatever the sessions (split panes, broadcast's tiles). */
  const MAX_FILES = 3;
  /** One file panel each, at most `MAX_FILES`: the focused pane's always among them, then the
   *  first others, shown in pane order. */
  /** A tab whose panel was picked by hand (the chips under the panels): shown as long as it's on screen. */
  let pickedFiles = $state<number | null>(null);
  const fileTabs = $derived.by(() => {
    const first = [panes[focus], pickedFiles].map((k) => withFiles.find((t) => t.key === k)).filter((t): t is Tab => !!t);
    const keep = first.concat(withFiles.filter((t) => !first.includes(t))).slice(0, MAX_FILES);
    return withFiles.filter((t) => keep.includes(t));
  });
  /** The focused tab when it's an AI agent's: the panel shows its tool's past conversations instead of files. */
  const historyTab = $derived(activeTab?.session.kind.type === "agent" && activeTab.id && activeTab.alive ? activeTab : null);
  const canFiles = $derived(fileTabs.length > 0 || !!historyTab);
  /** A text tab is open (a terminal, not a desktop or a page): the file panel and snippets are for those. */
  const hasText = $derived(strip.some((t) => broadcasts(t.session.kind)));

  /** The layout to go back to when broadcast stops tiling every tab. */
  let untiled: Layout = "single";

  /** Broadcast shows every tab at once, each in a pane of its own. */
  function toggleBroadcast() {
    if (!canBroadcast) return void (broadcastAlone = true); // from the keyboard: say why nothing happened
    broadcasting = !broadcasting;
    if (broadcasting) {
      guideDone("broadcast");
      untiled = layout;
      setLayout("tiles"); // already tiled: the unticked tabs come back
    } else if (layout === "tiles" && untiled !== "tiles") setLayout(untiled);
    tick().then(refocus);
  }

  /** The focused tab's font only (or `t`'s, for the wheel); Settings › Terminal sets everyone's size. */
  function zoom(by: number, t = activeTab) {
    if (t) t.zoom = zoomed(t.zoom ?? 0, by);
  }

  /** Everyone's font size, the one in Settings › Terminal (same range as its slider), by Ctrl+Shift+wheel. */
  function fontStep(by: number) {
    const s = app.settings;
    if (s) s.font_size = Math.max(8, Math.min(32, s.font_size + by));
  }
  // A detached tab's window turns the wheel for everyone's size too; the settings live here.
  listen<number>("font-step", (e) => fontStep(e.payload));

  /** The panes' sizes with an edge of the focused pane moved [x, y] by the step in Settings ›
   *  Terminal (actions.ts `resize`); null if it has no neighbour that way. */
  function paneResized([dx, dy]: [number, number]): Sizes | null {
    const area = document.querySelector(".panes")?.getBoundingClientRect();
    if (!area?.width || panes.length < 2) return null;
    const step = app.settings?.pane_step ?? 21;
    return resized(shape, $state.snapshot(current), focus, (dx * step) / area.width, (dy * step) / area.height);
  }

  /** The pane next to the focused one in a direction, -1 if none. */
  const toward = (d: [number, number]) => neighbour(panes.map((_, i) => rect(i)), focus, d);

  function run(id: string) {
    const t = activeTab;
    if (DIRECTIONS[id]) {
      const p = toward(DIRECTIONS[id]);
      if (p < 0) return;
      focus = p;
      tick().then(refocus);
      return;
    }
    if (MOVES[id]) {
      // The focused tab and the one in the pane that way trade places; the focus goes along.
      const p = toward(MOVES[id]);
      if (p < 0) return;
      [panes[focus], panes[p]] = [panes[p], panes[focus]];
      focus = p;
      tick().then(refocus);
      return;
    }
    if (EDGES[id]) {
      const s = paneResized(EDGES[id]);
      if (s) sizes[shape.join()] = s;
      return;
    }
    const n = /^tab-(\d)$/.exec(id);
    if (n) return void (strip[+n[1] - 1] && show(strip[+n[1] - 1].key));
    if (id.startsWith("layout-")) return setLayout(id.slice(7) as Layout);
    switch (id) {
      case "palette":
        paletteFilter = null;
        showPalette = !showPalette;
        if (showPalette) guideDone("palette");
        break;
      case "getting-started":
        openGuide();
        break;
      case "find":
        paletteFilter = "sessions";
        showPalette = true;
        guideDone("palette");
        break;
      case "find-text":
        if (t) views[t.key]?.find();
        break;
      case "new-session":
        createSession();
        break;
      case "local-shell":
        localShell();
        break;
      case "close-tab":
        if (t) close(t.key);
        break;
      case "reconnect":
        if (t && !coopTab(t)) reconnect(t.key);
        break;
      case "duplicate-tab":
        if (t && !coopTab(t)) open(t.session);
        break;
      case "detach-tab":
        if (t) detach(t.key);
        break;
      case "next-tab":
        cycle(1);
        break;
      case "prev-tab":
        cycle(-1);
        break;
      case "next-pane":
        focus = (focus + 1) % panes.length;
        tick().then(refocus);
        break;
      case "toggle-sidebar":
        showSidebar = !showSidebar;
        break;
      case "collapse-sidebar":
        // A hidden sidebar comes back open.
        app.settings!.sidebar_collapsed = showSidebar && !app.settings!.sidebar_collapsed;
        showSidebar = true;
        break;
      case "toggle-files":
        showFiles = !showFiles;
        break;
      case "toggle-status":
        app.settings!.status_bar = !app.settings!.status_bar;
        break;
      case "toggle-broadcast":
        toggleBroadcast();
        break;
      case "tunnels":
        showTunnels = !showTunnels;
        break;
      case "snippets":
        showSnippets = !showSnippets;
        break;
      case "credentials":
        app.showCredentials = !app.showCredentials;
        break;
      case "coop":
        app.showCoop = !app.showCoop;
        break;
      case "sync":
        if (app.settings?.sync_token) syncNow().catch((e) => (app.status = _("Sync: {error}", { error: String(e) })));
        else tell(_("Sign in first: Settings › Account and sync."), "sync");
        break;
      case "import":
        showImport = true;
        break;
      case "export":
        showExport = true;
        break;
      case "settings":
        app.showSettings = !app.showSettings;
        break;
      case "lock":
        invoke<boolean>("lock_now").then((locked) => locked || tell(_("Set a way to unlock first: Settings › Security."), "security"));
        break;
      case "font-bigger":
        zoom(1);
        break;
      case "font-smaller":
        zoom(-1);
        break;
      case "font-reset":
        zoom(0);
        break;
      case "theme":
        app.settings!.theme = dark ? "Light" : "Dark";
        break;
    }
  }

  // ---- locked: stored data waits for the recovery passphrase ----
  let unlockPass = $state("");
  let unlockError = $state("");
  let unlocking = $state(false);
  let confirmStartOver = $state(false);

  async function unlock() {
    [unlocking, unlockError] = [true, ""];
    try {
      await invoke("unlock", { passphrase: unlockPass });
      unlockPass = "";
      await boot();
    } catch (e) {
      unlockError = String(e);
    }
    unlocking = false;
  }

  async function startOver() {
    await invoke("start_over");
    await boot();
  }

  function resizeSidebar(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const r = el.previousElementSibling!.getBoundingClientRect();
    // Right to left, the sidebar is on the right.
    const rtl = document.documentElement.dir === "rtl";
    // Dragging a collapsed sidebar's handle opens it out.
    const move = (m: PointerEvent) => {
      app.settings!.sidebar_collapsed = false;
      app.settings!.sidebar_width = Math.round(Math.max(180, Math.min(600, rtl ? r.right - m.clientX : m.clientX - r.left)));
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", () => el.removeEventListener("pointermove", move), { once: true });
  }

  /** Opens a collapsed sidebar out. */
  function expandSidebar() {
    app.settings!.sidebar_collapsed = false;
  }

  /** Double-click on the handle: the sidebar opens out just wide enough for the rows on show (a
   *  closed folder's sessions don't count), measured as they are drawn: the list's edge to the
   *  name, the name's whole text, what follows it (a folder's tags and count), and a little air. */
  async function fitSidebar() {
    app.settings!.sidebar_collapsed = false;
    await tick();
    const rack = document.querySelector<HTMLElement>(".rack");
    const list = rack?.querySelector<HTMLElement>(".list");
    if (!rack || !list) return;
    const rtl = document.documentElement.dir === "rtl";
    const edge = list.getBoundingClientRect();
    const c = document.createElement("canvas").getContext("2d")!;
    const rows = [...list.querySelectorAll<HTMLElement>(".item")];
    // Rows out of view aren't laid out (Sidebar.svelte `content-visibility`): for the measure they are.
    for (const row of rows) row.style.contentVisibility = "visible";
    let w = 0;
    for (const row of rows) {
      const name = row.querySelector<HTMLElement>(".name");
      if (!name) continue;
      c.font = getComputedStyle(name).font;
      const r = name.getBoundingClientRect();
      const before = rtl ? edge.right - r.right : r.left - edge.left;
      let after = 5; // the row's end padding
      for (let el = name.nextElementSibling; el; el = el.nextElementSibling) after += (el as HTMLElement).offsetWidth + 5;
      w = Math.max(w, before + c.measureText(name.textContent ?? "").width + after);
    }
    for (const row of rows) row.style.contentVisibility = "";
    // What the sidebar puts around the list (its padding, a scroll bar if there is one), and air.
    w += rack.getBoundingClientRect().width - list.clientWidth + 13;
    app.settings!.sidebar_width = Math.round(Math.max(180, Math.min(600, w)));
  }

  /** A getting-started step, done for real: panes and broadcast open two local shells to show on. */
  async function guideStep(step: Step) {
    const two = async () => {
      for (let n = tabs.filter((t) => joinsBroadcast(t.session.kind)).length; n < 2; n++) localShell();
      await tick();
    };
    switch (step) {
      case "shell":
        return void localShell();
      case "session":
        return createSession();
      case "palette":
        return run("palette");
      case "split":
        await two();
        return setLayout("columns");
      case "tunnel":
        return void (showTunnels = true);
      case "broadcast":
        await two();
        return toggleBroadcast();
      case "lock":
        return openSettings("security");
      case "sync":
        return openSettings("sync");
    }
  }

  // ---- Getting started while tabs hide the start screen ----
  /** The list's popover: from the tab bar's progress button, the palette or a tick's toast,
   *  hanging under the tab bar by its inline-end edge (both from the window's edges). */
  let guidePop = $state<{ end: number; top: number } | null>(null);
  /** The tab bar's progress button, which the popover hangs from. */
  let guideButton = $state<HTMLElement>();
  const guide = $derived(app.settings?.guide ? guideRows() : []);
  const guideDoneCount = $derived(guide.filter((r) => r.done).length);
  /** The progress button shows while the list is on, unfinished and out of sight behind tabs. */
  const guideAway = $derived(strip.length > 0 && guideDoneCount < guide.length);

  /** Shows the list: on the start screen it's there already; with tabs, in the popover. Turns
   *  the list back on if it was hidden (the palette's "Getting started" is how you ask). */
  async function openGuide() {
    if (!app.settings) return;
    app.settings.guide = true;
    if (!strip.length) return void (guidePop = null);
    await tick(); // the progress button, if the list was hidden until now
    const rtl = document.documentElement.dir === "rtl";
    // Under the progress button, its end lined up with the button's; with every step done there is
    // no button (the palette still opens the list), so under the tab bar's tools.
    const at = guideButton ?? document.querySelector<HTMLElement>(".strip .tools");
    const r = at?.getBoundingClientRect();
    if (!r || !r.height) guidePop = { end: 13, top: 89 };
    else guidePop = { end: Math.max(5, rtl ? r.left : innerWidth - r.right), top: r.bottom + 5 };
    // After the palette or a dialog that opened it has handed the keyboard back (`refocus`).
    const step = (sel: string) => document.querySelector<HTMLElement>(`.guide-pop ${sel}`);
    setTimeout(() => (step(".open .go") ?? step(".step"))?.focus(), 0);
  }
  function closeGuide() {
    guidePop = null;
    // The keyboard goes back where it was; if that's gone (the palette), to the focused pane.
    tick().then(() => document.activeElement === document.body && refocus());
  }

  // Hidden (its own Hide) or the last tab closed (the start screen has it): the popover goes, and
  // doesn't come back by itself when the list or a tab does.
  $effect(() => {
    if (guidePop && (!app.settings?.guide || !strip.length)) guidePop = null;
  });

  /** Where each step stood when last told, so a step that ticks says so, once. */
  const guideSeen = new Map<Step, boolean>();
  /** The workspace the steps were last read in. */
  let guideWorkspace: string | undefined;
  // A step done away from the list (most of them open a tab, which hides the start screen) brings
  // a toast naming it and the next one, with Show. Not on the list's own screen, where the tick is
  // seen, nor for steps only now known (the lock's state arrives after the window opens), nor on a
  // workspace switch: one with saved sessions ticks "Save your first server", but nobody did that.
  $effect(() => {
    const rows = app.settings ? guideRows() : [];
    const switched = guideWorkspace !== undefined && guideWorkspace !== app.workspace;
    guideWorkspace = app.workspace;
    let ticked: Step | undefined;
    for (const r of rows) {
      if (!r.known) continue;
      if (guideSeen.get(r.id) === false && r.done) ticked ??= r.id;
      guideSeen.set(r.id, r.done);
    }
    if (!ticked || switched || !app.settings?.guide || !strip.length || guidePop) return;
    const done = rows.find((r) => r.id === ticked)!.title;
    const next = rows.find((r) => !r.done)?.title;
    untrack(() => toast(next ? _("Done: {done}. Next: {next}.", { done, next }) : _("Getting started is all done."), { label: _("Show"), run: openGuide }));
  });

  /** The progress button pulses for a moment the first time it shows on this device (the start
   *  screen just went behind a tab), so the eye finds where the list went; after that the Done
   *  toast says so. `guide_done` remembers it as `pill-seen`, and Start over clears it with the
   *  steps. Not under reduced motion (app.css stops animations). */
  let guideNudge = $state(false);
  let nudgeTimer = 0;
  function nudgeGuide() {
    guideNudge = false;
    clearTimeout(nudgeTimer);
    // off, then on again a moment later: a pulse already running starts over
    nudgeTimer = setTimeout(() => {
      guideNudge = true;
      nudgeTimer = setTimeout(() => (guideNudge = false), 2600);
    }, 21);
  }
  let wasAway = false;
  $effect(() => {
    const away = guideAway;
    if (away && !wasAway)
      untrack(() => {
        if (app.settings?.guide_done.includes("pill-seen")) return;
        guideDone("pill-seen");
        nudgeGuide();
      });
    wasAway = away;
  });

  /** Every level of every session's folder: Prod/Web counts Prod as well. */
  const folders = $derived(new Set(app.store.session.flatMap((s) => s.folder.split("/").filter(Boolean).map((_, i, p) => p.slice(0, i + 1).join("/")))).size);
</script>

{#snippet syncSide(title: string, s: SyncSide, cloud: boolean)}
  <div class="side">
    <h3>{title}</h3>
    <dl>
      <dt>{_("Last changed")}</dt><dd>{s.at ? new Date(s.at * 1000).toLocaleString() : _("unknown")}</dd>
      <dt>{_("Device")}</dt><dd>{s.device || _("unknown")}</dd>
      <dt>{_("System")}</dt><dd>{s.os || _("unknown")}</dd>
      <dt>{cloud ? _("Uploaded from") : _("Address")}</dt>
      <dd>{cloud ? `${s.ip || _("unknown")}${s.location ? ` (${s.location})` : ""}` : _("this computer")}</dd>
      <dt>{_("Holds")}</dt><dd>{_("{sessions} sessions, {tunnels} tunnels, {keys} trusted keys", { sessions: s.sessions, tunnels: s.tunnels, keys: s.keys })}</dd>
    </dl>
  </div>
{/snippet}

{#snippet layoutGlyph(l: Layout)}
  <!-- Tiles: drawn with three panes. -->
  {@const [cols, rows] = l === "tiles" ? [1, 1] : GRID[l]}
  {@const cells = l === "tiles" ? [0, 1, 2].map((i) => tile(3, i)) : Array.from({ length: cols * rows }, (_, i): [number, number, number, number] => [(i % cols) * (100 / cols), Math.floor(i / cols) * (100 / rows), 100 / cols, 100 / rows])}
  <svg width="21" height="17" viewBox="0 0 20 16" aria-hidden="true">
    {#each cells as [x, y, w, h], i (i)}
      <rect
        x={1 + x * 0.18 + (w < 100 ? 0.75 : 0)}
        y={1 + y * 0.14 + (h < 100 ? 0.75 : 0)}
        width={w * 0.18 - (w < 100 ? 1.5 : 0)}
        height={h * 0.14 - (h < 100 ? 1.5 : 0)}
        rx="1.5"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
      />
    {/each}
  </svg>
{/snippet}

<!-- A menu item's shortcut, at its end. -->
{#snippet keys(action: string)}
  {@const k = shortcut(action)}
  {#if k}<span class="keys"><kbd>{k}</kbd></span>{/if}
{/snippet}

{#snippet corner()}
  <!-- The sidebar's foot (the rail's, collapsed), at its end: settings (About is a page there),
       then the account. -->
  <button class="flat icon-only settings" class:on={app.showSettings} onclick={() => (app.showSettings = !app.showSettings)} title={tip(_("Settings"), "settings")} aria-label={_("Settings")}><Icon name="cog" size={16} /></button>
  {#if lock.info && (lock.info.password || lock.info.pin || lock.info.passkey || lock.info.biometrics)}
    <button class="flat icon-only" onclick={() => run("lock")} title={tip(_("Lock xtermium"), "lock")} aria-label={_("Lock xtermium")}><Icon name="lock-alt" size={16} /></button>
  {/if}
  {#if app.coop.ring || (app.settings?.sync_token && (app.settings.coop_button || app.settings.sync_pro))}
    <button class="flat icon-only" class:on={!!app.coop.ring} onclick={() => run("coop")} title={tip(_("Terminal Co-op"), "coop")} aria-label={_("Terminal Co-op")}><Icon name="group" size={16} /></button>
  {/if}
  <Account onsettings={() => openSettings("sync")} />
{/snippet}

<svelte:window
  onpointermovecapture={active}
  onkeydown={active}
  onkeydowncapture={keydown}
  oninputcapture={cleanInput}
  onchangecapture={clampNumber}
  onclick={() => ([tabMenu, showTasks, layoutsMenu, workspaceMenu, allTabs] = [null, null, null, null, null])}
  onpointerdown={(e) => guidePop && !(e.target as Element).closest?.(".guide-pop, .guide-button") && closeGuide()}
/>

{#if ready}
  <div class="app">
    {#if showSidebar && app.settings?.sidebar_collapsed}
      <!-- Collapsed: the sidebar's foot in a column (the title bar's burger opens it out). -->
      <aside class="rack rail">
        <Backdrop place="sidebar" tint={(a) => `rgba(var(--panel-rgb), ${a})`} />
        <!-- The sidebar shows messages; collapsed, they wait behind this. -->
        {#if app.status}
          <button class="flat icon-only" onclick={() => expandSidebar()} title={app.status} aria-label={app.status}><Icon name="info-circle" size={16} color="var(--weak)" /></button>
        {/if}
        <span class="grow"></span>
        {@render corner()}
      </aside>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="splitter" onpointerdown={resizeSidebar} ondblclick={fitSidebar} title={_("Drag to resize; double-click to fit the names")}></div>
    {:else if showSidebar}
      <aside class="rack" style:width="{app.settings?.sidebar_width ?? 280}px">
        <Backdrop place="sidebar" tint={(a) => `rgba(var(--panel-rgb), ${a})`} />
        <Sidebar {live} current={activeTab && !coopTab(activeTab) ? sessionKey(activeTab.session) : ""} onopen={open} onedit={(session, index) => (editor = { session, index })} />
        {#if app.status}
          <div class="status row" role="status">
            {#if app.statusLink?.text === app.status}
              {@const page = app.statusLink.page}
              <button class="link quiet grow" title={app.status} onclick={() => openSettings(page)}>{app.status}</button>
            {:else}
              <span class="grow" title={app.status}>{app.status}</span>
            {/if}
            <button class="flat icon-only" title={_("Dismiss")} onclick={() => (app.status = "")}><Icon name="x" size={16} /></button>
          </div>
        {/if}
        <div class="corner row">{@render corner()}</div>
      </aside>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="splitter" onpointerdown={resizeSidebar} ondblclick={fitSidebar} title={_("Drag to resize; double-click to fit the names")}></div>
    {/if}

    <main class="center">
      <header class="strip">
        <Backdrop place="bar" tint={(a) => `rgba(var(--panel-rgb), ${a})`} />
        <!-- The arrows lie over the faded ends and take every click there: what shows of a tab
             past the edge is mostly its X, so a click meant to go back would close it. -->
        <div class="tabs-wrap">
        {#if tabsOver.start}
          <button class="flat icon-only tabs-step start" tabindex="-1" title={_("Earlier tabs")} aria-label={_("Earlier tabs")} onclick={() => stepTabs(-1)}
            ><Icon name="chevron-right" size={15} /></button
          >
        {/if}
        {#if tabsOver.end}
          <button class="flat icon-only tabs-step end" tabindex="-1" title={_("Later tabs")} aria-label={_("Later tabs")} onclick={() => stepTabs(1)}
            ><Icon name="chevron-right" size={15} /></button
          >
        {/if}
        <div
          class="tabs"
          class:over-start={tabsOver.start}
          class:over-end={tabsOver.end}
          bind:this={tabsEl}
          onscroll={measureTabs}
          onwheel={(e) => !e.deltaX && (e.currentTarget.scrollLeft += e.deltaY)}
        >
          {#each strip as t, i (t.key)}
            {@const look = lookOf(t.session)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="tab"
              class:active={t.key === panes[focus]}
              class:visible={panes.includes(t.key)}
              class:dead={!t.alive}
              class:lifted={dragging === t.key}
              class:before={dropAt === i}
              class:after={dropAt === strip.length && i === strip.length - 1}
              style:--kind={t.alive ? look.color : "var(--warn)"}
              style:--tint={(t.alive && (t.color || look.tint)) || null}
              onpointerdown={(e) => renaming !== t.key && dragStart(e, t.key, true)}
              onclickcapture={(e) => dragged && e.stopPropagation()}
            >
              {#if broadcasting && joinsBroadcast(t.session.kind)}
                <input
                  type="checkbox"
                  title={_("Include in broadcast")}
                  checked={t.broadcast}
                  onchange={(e) => {
                    t.broadcast = (e.target as HTMLInputElement).checked;
                    invoke("set_tab_broadcast", { id: t.id, on: t.broadcast });
                    tick().then(refocus);
                  }}
                />
              {:else if layout === "tiles" && !broadcasting}
                {@const shown = panes.includes(t.key)}
                <input type="checkbox" title={_("Show in the tiles")} checked={shown} disabled={shown && panes.length === 1} onchange={() => toggleTile(t.key)} />
              {/if}
              {#if renaming === t.key}
                <input
                  class="rename"
                  value={titleOf(t)}
                  aria-label={_("Tab name")}
                  {@attach (el) => (el.focus(), el.select())}
                  onkeydown={(e) => {
                    if (e.key === "Enter") (e.preventDefault(), rename(t, e.currentTarget.value));
                    if (e.key === "Escape") (e.stopPropagation(), (renaming = null), tick().then(refocus));
                  }}
                  onblur={(e) => rename(t, e.currentTarget.value)}
                />
              {:else}
                <button
                  class="flat label"
                  aria-current={t.key === panes[focus] ? "true" : undefined}
                  onclick={() => show(t.key)}
                  ondblclick={() => (renaming = t.key)}
                  onauxclick={(e) => e.button === 1 && close(t.key)}
                  oncontextmenu={(e) => {
                    e.preventDefault();
                    tabMenu = { x: e.clientX, y: e.clientY, key: t.key };
                  }}
                  title={tipOf(t)}
                >
                  {#if t.alive}<SessionIcon icon={look.icon} size={15} still={look.still} />{:else}<Icon name="error" size={15} />{/if}
                  <span class="title">{titleOf(t)}</span>
                  {#if t.id && app.coop.shared.includes(t.id)}<span class="shared" role="img" title={_("Shared with the ring")} aria-label={_("Shared with the ring")}></span>{/if}
                </button>
              {/if}
              <button class="flat icon-only close" title={tip(_("Close"), "close-tab")} onclick={() => close(t.key)}><Icon name="x" size={15} /></button>
            </div>
          {/each}
        </div>
        </div>
        <!-- the tools act on tabs, so they come as the tabs do: files and snippets with a terminal tab, broadcast with two tabs -->
        <div class="tools row">
          <!-- every tab in a list, while some don't fit: first, right by the tabs it stands for -->
          {#if tabsOver.start || tabsOver.end}
            {@const label = _("All tabs ({n})", { n: strip.length })}
            <button
              class="flat icon-only"
              class:on={!!allTabs}
              title={label}
              aria-label={label}
              aria-expanded={!!allTabs}
              onclick={(e) => {
                e.stopPropagation();
                const r = e.currentTarget.getBoundingClientRect();
                // hung from the button's end: its right edge, or its left one right to left
                const rtl = document.documentElement.dir === "rtl";
                allTabs = allTabs ? null : { x: rtl ? r.left : r.right, y: r.bottom + 5 };
              }}><Icon name="chevron-down" /></button
            >
          {/if}
          <!-- Getting started's progress while tabs hide the start screen: a ring, the name and
               "3/8", opening the list in a popover under it. First, so it reads before the tools. -->
          {#if guideAway}
            {@const label = _("Getting started: {n} of {total} done", { n: guideDoneCount, total: guide.length })}
            <button
              bind:this={guideButton}
              class="guide-button"
              class:open={!!guidePop}
              class:nudge={guideNudge}
              title={label}
              aria-label={label}
              aria-expanded={!!guidePop}
              onclick={() => (guidePop ? closeGuide() : openGuide())}
            >
              <svg class="steps-ring" viewBox="0 0 16 16" aria-hidden="true">
                <circle cx="8" cy="8" r="6.5" pathLength="100" />
                <circle class="fill" cx="8" cy="8" r="6.5" pathLength="100" stroke-dasharray="{(guideDoneCount / guide.length) * 100} 100" />
              </svg>
              <span>{_("Getting started")}</span><span class="count">{guideDoneCount}/{guide.length}</span>
            </button>
          {/if}
          {#if tasks.length}
            <button
              class="flat tasks"
              class:on={showTasks}
              title={_("Tasks: tabs running in the background")}
              onclick={(e) => {
                e.stopPropagation();
                const r = e.currentTarget.getBoundingClientRect();
                showTasks = showTasks ? null : { x: r.right, y: r.bottom + 4 };
              }}><Icon name="layers" />{tasks.length}</button
            >
          {/if}
          {#if hasText}
            <button
              class="flat icon-only"
              class:on={showFiles && canFiles}
              disabled={!canFiles}
              title={tip(
                historyTab
                  ? _("Session history: the past conversations of this agent in its folder, to go on with, rename or delete")
                  : canFiles
                    ? _("File Browser: files of the SSH and VNC (SFTP) and RDP (shared folders) sessions on screen")
                    : _("File Browser: show a connected SSH, RDP or VNC tab to browse its files, or an AI agent tab for its session history"),
                "toggle-files",
              )}
              aria-label={historyTab ? _("Session history") : _("File Browser")}
              onclick={() => (showFiles = !showFiles)}><Icon name="folder-open" /></button
            >
            <button class="flat icon-only" class:on={showSnippets} title={tip(_("Snippets: saved commands to type into the terminal"), "snippets")} aria-label={_("Snippets")} onclick={() => (showSnippets = !showSnippets)}>
              <Icon name="play" />
            </button>
          {/if}
          {#if strip.length > 1}
            <button
              class="flat icon-only"
              class:on={broadcasting}
              disabled={!canBroadcast}
              title={tip(
                canBroadcast
                  ? _("Broadcast: type into every ticked tab at once (terminals, remote desktops, BMC consoles)")
                  : _("Broadcast: open two or more terminal, remote desktop or BMC console tabs first"),
                "toggle-broadcast",
              )}
              aria-label={_("Broadcast")}
              onclick={toggleBroadcast}
            >
              <Icon name="broadcast" />
            </button>
          {/if}
        </div>
      </header>

      <div class="panes" class:split={strip.length && panes.length > 1} class:broadcasting class:sizing class:steady={app.settings?.focus_animated === false}>
        {#if !strip.length}
          {@const link = /^https?:\/\//.test(app.settings?.logo_link ?? "") ? app.settings!.logo_link : ""}
          <div class="dashboard" out:fade={{ duration: arriving && !still ? LEAVE : 0, easing: cubicIn }} onoutroend={() => arriving && (arriving.clear = true)}>
            <div class="intro">
              <p class="prompt" aria-hidden={draft === null}>~/xtermium <b>$</b> {#if draft}<span class="typed">{draft}</span>{/if}<i class="cursor"></i></p>
              {#if link}
                <button class="flat logo" title={link} aria-label={_("Open {url}", { url: link })} onclick={() => openUrl(link).catch((e) => (app.status = String(e)))}><Logo /></button>
              {:else}
                <div class="logo"><Logo /></div>
              {/if}
              <p class="lead">{_("Terminals, SSH, SFTP, RDP and tunnels in one window.")}</p>
              <!-- with nothing saved yet, the getting-started list says it, as its second step -->
              {#if app.store.session.length || !app.settings?.guide}
              <p class="weak">
                {#if app.store.session.length}
                  {@const counts = { sessions: app.store.session.length, folders, tunnels: app.store.tunnel.length }}
                  {counts.sessions === 1
                    ? _("1 saved session.")
                    : folders > 1
                      ? _("{sessions} saved sessions in {folders} folders.", counts)
                      : _("{sessions} saved sessions.", counts)}
                  {#if counts.tunnels === 1}{_("1 tunnel.")}{:else if counts.tunnels}{_("{tunnels} tunnels.", counts)}{/if}
                  {app.settings?.single_click_open ? _("Click a session in the sidebar to connect.") : _("Double-click a session in the sidebar to connect.")}
                {:else}
                  {_("No saved sessions yet. Create one, or import them from MobaXterm or your ~/.ssh/config.")}
                {/if}
              </p>
              {/if}
              {#if app.settings?.guide}
                <Guide onstep={guideStep} onimport={() => (showImport = true)} />
              {:else}
                <div class="starts">
                  <button class="flat" onclick={localShell} style:--kind="var(--k-shell)" title={_("A terminal on this computer")}>
                    <span class="glyph"><Icon name="terminal" size={16} /></span>{_("Open a local shell")}
                  </button>
                  <button class="flat" onclick={createSession} style:--kind="var(--weak)" title={_("SSH, RDP, serial port or website")}>
                    <span class="glyph"><Icon name="plus" size={16} /></span>{_("New session")}
                  </button>
                  <button class="flat" onclick={() => (showImport = true)} style:--kind="var(--weak)" title={_("From MobaXterm, ~/.ssh/config or an xtermium export")}>
                    <span class="glyph"><Icon name="import" size={16} /></span>{_("Import sessions")}
                  </button>
                  <button class="flat" onclick={() => (showTunnels = true)} style:--kind="var(--weak)" title={_("Forward ports over SSH")}>
                    <span class="glyph"><Icon name="transfer-alt" size={16} /></span>{_("Tunnels")}
                  </button>
                </div>
              {/if}
              <p class="weak">
                {#if app.settings?.quick_shell !== false}{_("Type a command and press Enter to run it in a shell.")}{/if}
                {_("Keyboard shortcuts are listed, and can be changed, in")}
                <button class="link" onclick={() => openSettings("shortcuts")}>{_("Settings › Shortcuts")}</button>
              </p>
            </div>
          </div>
        {:else}
          {#each panes as p, i (i)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="pane" class:empty={p === null} style={place(i)} onpointerdown={() => (focus = i)}>
              {#if p === null}<span class="weak">{_("Empty pane. Click it, then pick a tab.")}</span>{/if}
            </div>
          {/each}
          {#if layout !== "single" && focus < panes.length}<div class="ring" style={place(focus)}></div>{/if}
        {/if}
          <!-- Every tab stays mounted (its canvas and connection live on); panes only position them. -->
          {#each tabs.filter((t) => !t.detached) as t (t.key)}
            {@const pane = panes.indexOf(t.key)}
            {@const lit = broadcasting && t.broadcast && joinsBroadcast(t.session.kind)}
            {@const drawn = lit && app.settings?.broadcast_outline !== "off"}
            <!-- Terminals, desktops and pages run left to right in any interface language. -->
            <div
              class="view"
              dir="ltr"
              class:outline={drawn}
              data-outline={drawn ? app.settings?.broadcast_outline : null}
              style:--oc={app.settings?.broadcast_outline_color || null}
              data-pane={pane}
              class:unseen={arriving?.key === t.key}
              class:arrive={arriving?.key === t.key && arriving.up && arriving.clear}
              onanimationend={(e) => e.target === e.currentTarget && e.animationName === "fade-in" && (arriving = null)}
              style={pane >= 0 ? place(pane) : "display: none"}
            >
              <div class="body">
              <!-- A tab of the ring's: someone else's, on the frames the ring sends, with no session of ours behind it (`onid` never comes). -->
              {#if t.session.kind.type === "coop"}
                {@const k = t.session.kind}
                {@const r = app.coop.remote.find((r) => r.id === k.tab)}
                {#if k.picture}
                  <Rdp
                    bind:this={desks[t.key]}
                    session={t.session}
                    remote={k.tab}
                    active={pane === focus}
                    onid={() => {}}
                    onmeta={(m: Meta) => update(t.key, { alive: m.alive })}
                    onconnected={(connected) => update(t.key, { connected })}
                    onfocus={() => (focus = pane)}
                    onexit={() => close(t.key, true)}
                  />
                {:else}
                  <Terminal
                    bind:this={views[t.key]}
                    session={t.session}
                    remote={k.tab}
                    size={r ? [r.cols, r.rows] : undefined}
                    zoom={t.zoom}
                    active={pane === focus}
                    visible={pane >= 0}
                    onzoom={(scope, by) => (scope === "tab" ? zoom(by, t) : fontStep(by))}
                    onid={() => {}}
                    onmeta={(m) => metaOf(t, m)}
                    onfocus={() => (focus = pane)}
                    onexit={() => close(t.key, true)}
                  />
                {/if}
              {:else if isDesktop(t.session.kind)}
                <Rdp
                  bind:this={desks[t.key]}
                  session={t.session}
                  attach={t.attach}
                  shared={t.id && app.coop.shared.includes(t.id) ? t.id : 0}
                  active={pane === focus}
                  onid={(id) => started(t.key, id)}
                  onmeta={(m: Meta) => update(t.key, { alive: m.alive })}
                  onconnected={(connected) => update(t.key, { connected })}
                  onfocus={() => (focus = pane)}
                  onexit={() => close(t.key, true)}
                />
              {:else if t.session.kind.type === "browser" || t.session.kind.type === "kvm"}
                <Web
                  session={t.session}
                  active={pane === focus}
                  visible={pane >= 0}
                  shared={t.id && app.coop.shared.includes(t.id) ? t.id : 0}
                  onid={(id) => started(t.key, id)}
                  ontitle={(title) => update(t.key, { title })}
                  onfocus={() => (focus = pane)}
                />
              {:else}
                <Terminal
                  bind:this={views[t.key]}
                  session={t.session}
                  zoom={t.zoom}
                  attach={t.attach}
                  shared={t.id && app.coop.shared.includes(t.id) ? t.id : 0}
                  active={pane === focus}
                  visible={pane >= 0}
                  onid={(id) => started(t.key, id)}
                  onmeta={(m) => metaOf(t, m)}
                  onfocus={() => (focus = pane)}
                  onexit={(ended) => !(ended && t.background) && close(t.key, true)}
                  onzoom={(scope, by) => (scope === "tab" ? zoom(by, t) : fontStep(by))}
                />
              {/if}
              </div>
              <!-- More than six panes: they need the room more than their status. -->
              {#if app.settings?.status_bar && panes.length <= 6}
                <!-- The status bar is the pane's handle: drag it onto another pane. -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="handle"
                  class:movable={panes.length > 1}
                  title={panes.length > 1 ? _("Drag onto another pane to move this tab there") : null}
                  onpointerdown={(e) => panes.length > 1 && dragStart(e, t.key, false)}
                >
                  <StatusBar tab={t} broadcast={lit} visible={pane >= 0} />
                </div>
              {/if}
            </div>
          {/each}
        <!-- Lines between panes, in the gap: drag to resize, double-click to even them out. -->
        {#if strip.length && panes.length > 1}
          {#each shape as across, r (r)}
            {@const first = shape.slice(0, r).reduce((a, b) => a + b, 0)}
            {@const [, top, , height] = rect(first)}
            {#if r > 0}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="sizer rows" style="top: {top}%" onpointerdown={(e) => resize(e, r, -1)} ondblclick={() => delete sizes[shape.join()]}></div>
            {/if}
            {#each { length: across - 1 }, c}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="sizer cols"
                style="left: {rect(first + c + 1)[0]}%; top: {top}%; height: {height}%"
                onpointerdown={(e) => resize(e, r, c + 1)}
                ondblclick={() => delete sizes[shape.join()]}
              ></div>
            {/each}
          {/each}
        {/if}
        <!-- While a tab is dragged, every pane is a place to drop it (website views step aside). -->
        {#if dragging !== null && panes.length > 1}
          <div class="drops">
            {#each panes as p, i (i)}
              {@const there = tabs.find((t) => t.key === p)}
              <div class="drop" class:over={dropOn === i} style={place(i)} data-pane={i} role="region" aria-label={_("Pane {n}", { n: i + 1 })}>
                <span>{p === dragging ? _("Here now") : there ? titleOf(there) : _("Empty pane")}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </main>

    {#if showFiles && (fileTabs.length || historyTab)}
      <aside class="files">
        {#if historyTab}
          <History session={historyTab.session} title={titleOf(historyTab)} onresume={(s) => open(s)} />
        {/if}
        {#each fileTabs as t (t.key)}
          {@const k = t.session.kind}
          <!-- Named and coloured as its tab is (renamed, recoloured), following the shell's folder. -->
          <Files
            tab={t.id}
            session={{ ...t.session, name: titleOf(t), color: t.color ?? t.session.color }}
            cwd={t.cwd ?? ""}
            rdp={k.type === "rdp" ? { host: k.host, user: k.user, connected: !!t.connected } : null}
          />
        {/each}
        {#if withFiles.length > fileTabs.length}
          <!-- More tabs with files than panels: a chip each; the shown ones are lit, a click shows another's. -->
          <div class="row wrap chips" role="group" aria-label={_("File browsers")}>
            {#each withFiles as t (t.key)}
              <button class="flat chip" class:on={fileTabs.includes(t)} title={_("Show the files of {name}", { name: titleOf(t) })} onclick={() => (pickedFiles = t.key)}>
                <SessionIcon icon={lookOf(t.session).icon} color={t.color ?? lookOf(t.session).color} size={13} still />{titleOf(t)}
              </button>
            {/each}
          </div>
        {/if}
      </aside>
    {/if}
  </div>

  {#if tabMenu}
    {@const k = tabMenu.key}
    {@const t = tabs.find((t) => t.key === k)}
    <div class="menu" {@attach menu(() => (tabMenu = null), tabMenu.x, tabMenu.y)}>
      <button onclick={() => (renaming = k)}><Icon name="edit" />{_("Rename tab")}</button>
      <div class="row tint" role="group" aria-label={_("Tab colour")}>
        <button class="swatch" class:selected={!t?.color} title={_("Default")} aria-label={_("Default colour")} style:--c={t && (lookOf(t.session).tint || lookOf(t.session).color)} onclick={() => t && (t.color = undefined)}></button>
        {#each COLORS as c (c)}
          <button class="swatch" class:selected={t?.color === c} title={c} aria-label={_("Colour {c}", { c })} style:--c={c} onclick={() => t && (t.color = c)}></button>
        {/each}
      </div>
      <button disabled={!t || !!coopTab(t)} onclick={() => reconnect(k)}><Icon name="refresh" />{_("Reconnect tab")}{@render keys("reconnect")}</button>
      <button disabled={!t || !!coopTab(t)} onclick={() => t && open(t.session)}><Icon name="copy" />{_("Duplicate tab")}{@render keys("duplicate-tab")}</button>
      <button disabled={!t || !detachable(t.session.kind)} onclick={() => detach(k)}><Icon name="log-out" />{_("Move to a new window")}{@render keys("detach-tab")}</button>
      <!-- Only the ring's owner shares: your own tabs, and not the kinds that can't be (the tooltip says why). -->
      {#if t && app.coop.ring?.owner && !coopTab(t)}
        {@const on = app.coop.shared.includes(t.id)}
        {@const why = unshareable(t)}
        <button disabled={!t.id || (!on && !!why)} title={why || null} onclick={() => share(t, !on)}><Icon name="group" />{on ? _("Stop sharing") : _("Share with the ring")}</button>
      {/if}
      {#if t && inCode(t.session.kind)}
        <button
          disabled={!t.id}
          title={t.cwd ||
            (t.session.kind.type === "ssh"
              ? _("The shell doesn't report its folder, so your home folder on the host opens.")
              : _("The shell doesn't report its folder, so the tab's starting folder opens."))}
          onclick={() => openInCode(t)}><Icon name="folder-open" />{_("Open the folder in VS Code")}</button
        >
      {/if}
      <button onclick={() => toBackground(k)}><Icon name="layers" />{_("Run in the background")}</button>
      <button class="sep" onclick={() => close(k)}><Icon name="x" />{_("Close tab")}{@render keys("close-tab")}</button>
      <button disabled={strip.length < 2} onclick={() => closeOthers(k)}>{_("Close other tabs")}</button>
      <button disabled={strip.at(-1)?.key === k} onclick={() => closeOthers(k, true)}>{_("Close tabs to the right")}</button>
    </div>
  {/if}
  {#if workspaceMenu}
    <WorkspacePicker x={workspaceMenu.x} y={workspaceMenu.y} width={workspaceMenu.w} onclose={() => ((workspaceMenu = null), refocus())} />
  {/if}
  {#if layoutsMenu}
    <div class="menu tasks-list" {@attach menu(() => (layoutsMenu = null), layoutsMenu.x, layoutsMenu.y)}>
      {#each app.store.layout as l (l.name)}
        <div class="row task">
          <button class="grow" title={l.pane.map((s) => s.name).join(", ")} onclick={() => openLayout(l)}>
            <Icon name="grid-alt" /><span class="grow title">{l.name}</span><span class="weak">{l.pane.length}</span>
          </button>
          <button class="icon-only" title={_("Delete")} aria-label={_("Delete {name}", { name: l.name })} onclick={(e) => (e.stopPropagation(), deleteLayout(l.name))}><Icon name="trash" size={15} /></button>
        </div>
      {:else}
        <p class="weak none">{_("No saved layouts yet. Put sessions in panes, then save them as one.")}</p>
      {/each}
      <button disabled={!onScreen.length} onclick={() => ((naming = ""), (layoutsMenu = null))}><Icon name="plus" />{_("Save the panes as a layout…")}</button>
    </div>
  {/if}
  {#if naming !== null}
    <Modal title={_("Save layout")} width={377} onclose={() => ((naming = null), refocus())}>
      <p class="weak">{_("{n} sessions on screen, in this arrangement, with their panes, sizes and tab names. Opening the layout puts each back where it was.", { n: onScreen.length })}</p>
      <input
        bind:value={naming}
        list="layout-names"
        maxlength="60"
        placeholder={_("e.g. Prod web tier")}
        aria-label={_("Layout name")}
        {@attach (el) => el.focus()}
        onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), saveLayout(naming ?? ""))}
      />
      <datalist id="layout-names">{#each app.store.layout as l (l.name)}<option value={l.name}></option>{/each}</datalist>
      {#if app.store.layout.some((l) => l.name === naming?.trim())}<p class="weak">{_("Saving replaces the layout with this name.")}</p>{/if}
      <div class="row">
        <button class="primary" disabled={!naming?.trim()} onclick={() => saveLayout(naming ?? "")}>{_("Save")}</button>
        <button onclick={() => ((naming = null), refocus())}>{_("Cancel")}</button>
      </div>
    </Modal>
  {/if}
  {#if allTabs && strip.length}
    <!-- The keyboard starts on the tab in use, so the arrows step from where you are. -->
    <div
      class="menu tasks-list all-tabs"
      aria-label={_("All tabs")}
      {@attach menu(() => (allTabs = null), allTabs.x, allTabs.y)}
      {@attach (el) => el.querySelector<HTMLElement>(".current")?.focus()}
    >
      {#each strip as t (t.key)}
        {@const look = lookOf(t.session)}
        <button
          class:current={t.key === panes[focus]}
          aria-current={t.key === panes[focus] ? "true" : undefined}
          style:--kind={t.alive ? look.color : "var(--warn)"}
          title={tipOf(t)}
          onclick={() => ((allTabs = null), show(t.key))}
        >
          {#if t.alive}<SessionIcon icon={look.icon} size={15} still={look.still} />{:else}<Icon name="error" size={15} />{/if}
          <span class="grow title">{titleOf(t)}</span>
        </button>
      {/each}
    </div>
  {/if}
  {#if showTasks && tasks.length}
    <div class="menu tasks-list" {@attach menu(() => (showTasks = null), showTasks.x, showTasks.y)}>
      {#each tasks as t (t.key)}
        {@const look = lookOf(t.session)}
        <div class="row task" style:--kind={look.color}>
          <button class="grow" title={_("Bring back into a tab")} onclick={() => show(t.key)}>
            <SessionIcon icon={look.icon} size={15} /><span class="grow title">{titleOf(t)}</span>
            <span class="weak">{t.detached ? _("in its own window") : t.alive ? _("running") : _("done")}</span>
          </button>
          <button class="icon-only" title={_("Close")} aria-label={_("Close")} onclick={() => close(t.key)}><Icon name="x" size={15} /></button>
        </div>
      {/each}
    </div>
  {/if}
  {#if editor}<SessionEditor session={editor.session} index={editor.index} onclose={() => (editor = null)} onconnect={open} />{/if}
  {#if showTunnels}<Tunnels onclose={() => (showTunnels = false)} />{/if}
  {#if app.showSettings}<Settings onclose={() => (app.showSettings = false)} onrun={runInShell} />{/if}
  {#if customCss}
    <Modal title={_("custom.css changed")} width={377} onclose={() => keepCustomCss(false)}>
      <p>{_("Your custom.css isn't the one you kept. It's on now: keep this look?")}</p>
      <p class="weak">{_("Without an answer it comes off in {s} s.", { s: customCss.left })}</p>
      <div class="row">
        <button class="primary" onclick={() => keepCustomCss(true)}>{_("Keep it")}</button>
        <button {@attach (el) => el.focus()} onclick={() => keepCustomCss(false)}>{_("Take it off")}</button>
      </div>
    </Modal>
  {/if}
  {#if showImport}<Import onclose={() => (showImport = false)} />{/if}
  {#if showExport}<Export folder="" onclose={() => (showExport = false)} />{/if}
  {#if broadcastAlone}
    <Modal title={_("Broadcast")} width={377} onclose={() => ((broadcastAlone = false), refocus())}>
      <p class="note">
        {_("Broadcast sends what you type to every open terminal at once. It needs at least two terminals: open them first, then turn it on.")}
      </p>
      <div class="row"><button class="primary" {@attach (el) => el.focus()} onclick={() => ((broadcastAlone = false), refocus())}>{_("OK")}</button></div>
    </Modal>
  {/if}
  {#if app.sync.conflict}
    {@const c = app.sync.conflict}
    {@const resolve = (choice: "local" | "remote") => {
      const pass = app.sync.pass || undefined;
      app.sync.conflict = null;
      syncNow(pass, choice).catch((e) => (app.status = _("Sync: {error}", { error: String(e) })));
    }}
    <Modal title={_("Sync conflict")} width={610} onclose={() => (app.sync.conflict = null)}>
      <p class="note">
        {_("Your sessions changed both on this device and in the cloud since they were last in sync. Choose which copy to keep: the other one is replaced, nothing is merged.")}
      </p>
      <div class="sides">
        {@render syncSide(_("This device"), c.local, false)}
        {@render syncSide(_("Cloud"), c.remote, true)}
      </div>
      <div class="row">
        <button class="primary" {@attach (el) => el.focus()} onclick={() => resolve("local")}>{_("Keep this device's copy")}</button>
        <button onclick={() => resolve("remote")}>{_("Use the cloud copy")}</button>
        <button class="flat" onclick={() => (app.sync.conflict = null)}>{_("Decide later")}</button>
      </div>
      <p class="weak">{_("Deciding later leaves both as they are; the next sync asks again.")}</p>
    </Modal>
  {/if}
  {#if showPalette}
    <Palette
      start={paletteFilter}
      tabs={tabs.map((t) => ({ ...t, title: titleOf(t), session: t.color ? { ...t.session, color: t.color } : t.session }))}
      onaction={run}
      onsnippet={runSnippet}
      onlayout={openLayout}
      onopen={open}
      onshow={show}
      onclose={() => {
        showPalette = false;
        refocus();
      }}
    />
  {/if}
  {#if guidePop && app.settings?.guide && strip.length}
    <!-- Getting started away from the start screen. A `.menu`, so website views step aside for it
         and it takes the keys (Escape or Tab closes it); a step picked closes it, then is done. -->
    <div
      class="menu guide-pop"
      style:inset-inline-end="{guidePop.end}px"
      style:top="{guidePop.top}px"
      {@attach menu(closeGuide)}
    >
      <Guide
        inset
        onstep={(s) => {
          closeGuide();
          guideStep(s);
        }}
        onimport={() => {
          closeGuide();
          showImport = true;
        }}
      />
    </div>
  {/if}
  {#if showSnippets}
    <Snippets target={snippetTarget ? titleOf(snippetTarget) : ""} broadcast={broadcasting} onrun={(s) => runSnippet(s)} onclose={() => ((showSnippets = false), refocus())} />
  {/if}
  {#if app.signIn}<SignIn />{/if}
  {#if app.showCredentials}<Credentials onclose={() => (app.showCredentials = false)} />{/if}
  {#if app.showCoop}<Coop onclose={() => ((app.showCoop = false), refocus())} />{/if}
  {#if asking}
    {@const a = asking}
    {@const names = Object.keys(a.values)}
    <Modal title={_("Run {name}", { name: a.snippet.name })} width={377} onclose={() => ((asking = null), refocus())}>
      <!-- Each Enter here and in the dialogs below stops at its field: the keyboard is back in the
           terminal before the key's keypress, which would land there (and run a snippet that isn't meant to). -->
      <div class="form">
        {#each names as n, i (n)}
          <span>{n}</span>
          <input
            bind:value={a.values[n]}
            {@attach (el) => void (i === 0 && el.focus())}
            onkeydown={(e) => e.key === "Enter" && i === names.length - 1 && (e.preventDefault(), runSnippet(a.snippet, $state.snapshot(a.values)))}
          />
        {/each}
      </div>
      <code class="preview">{fillIn(a.snippet.text, a.values)}</code>
      <div class="row">
        <button class="primary" onclick={() => runSnippet(a.snippet, $state.snapshot(a.values))}><Icon name="play" />{_("Run")}</button>
        <button onclick={() => ((asking = null), refocus())}>{_("Cancel")}</button>
      </div>
    </Modal>
  {/if}
  {#if closing}
    {@const t = closing}
    <Modal title={_("Close {name}?", { name: titleOf(t) })} width={377} onclose={() => ((closing = null), refocus())}>
      <p>{_("The session is still running. Closing the tab stops it, and ends it forcefully if it hasn't stopped after a few seconds.")}</p>
      <div class="row">
        <button class="primary danger" {@attach (el) => el.focus()} onclick={() => (close(t.key, true), (closing = null))}>{_("Close tab")}</button>
        <button onclick={() => ((closing = null), refocus())}>{_("Cancel")}</button>
      </div>
      <p class="weak"><button class="link quiet" onclick={() => openSettings("general")}>{_("Settings › General turns this question off.")}</button></p>
    </Modal>
  {/if}
  {#if app.locked !== null}
    <Modal title={_("Unlock your sessions")} width={377}>
      {#if app.locked}
        <p>{_("Your sessions and settings are encrypted, and this computer's keychain doesn't hold the key. Enter your recovery passphrase to unlock them.")}</p>
        <input type="password" bind:value={unlockPass} {@attach (el) => el.focus()} onkeydown={(e) => e.key === "Enter" && unlockPass && unlock()} />
        {#if unlockError}<p class="error">{unlockError}</p>{/if}
        <div class="row">
          <button class="primary" disabled={!unlockPass || unlocking} onclick={unlock}>{#if unlocking}<Icon name="loader-alt" spin />{/if}{_("Unlock")}</button>
        </div>
      {:else}
        <p>{_("Your sessions and settings are encrypted, but the key is missing from this computer's keychain and no recovery passphrase was set. They can't be opened.")}</p>
      {/if}
      <details bind:open={confirmStartOver}>
        <summary class="weak">{_("Start over without them")}</summary>
        <p>{_("The unreadable files are kept next to the new ones (ending in .unreadable), so a backup of the key could still open them later.")}</p>
        <div class="row"><button onclick={startOver}><Icon name="trash" />{_("Start over")}</button></div>
      </details>
    </Modal>
  {/if}
  {#if prompts.length}
    {@const p = prompts[0]}
    <!-- The backend's questions are in English; their titles are known words. -->
    <!-- Escape (or ×) answers no: Cancel, or Reject for a host key. -->
    <Modal title={_(p.title)} width={377} onclose={() => answer(false)}>
      <div class="prompt">{p.text}</div>
      {#if p.input !== null}
        <input type={p.input ? "password" : "text"} bind:value={p.answer} {@attach (el) => el.focus()} onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), answer(true))} />
        {#if p.input && p.title === "Password"}
          <label class="check"><input type="checkbox" bind:checked={p.remember} />{_("Remember the password")}</label>
        {/if}
        <div class="row"><button class="primary" onclick={() => answer(true)}>{_("Continue")}</button><button onclick={() => answer(false)}>{_("Cancel")}</button></div>
      {:else}
        <!-- Trusting a key is a security call: Reject holds the keyboard, so Enter never trusts one;
             Shift+Tab reaches Trust. -->
        <div class="row"><button class="primary" onclick={() => answer(true)}>{_("Trust")}</button><button {@attach (el) => el.focus()} onclick={() => answer(false)}>{_("Reject")}</button></div>
      {/if}
    </Modal>
  {/if}
{/if}

<!-- The bar's centre says where the keyboard goes, the focused tab, and opens the palette, which goes anywhere else. Locked, it says nothing. -->
<TitleBar>
  <!-- What reaches past any one session stands by the mark: the sidebar's burger, a local shell,
       tunnels, saved layouts and a new session. -->
  {#snippet tools()}
    {#if ready && app.locked === null && !lock.locked}
      {@const open = showSidebar && !app.settings?.sidebar_collapsed}
      <button class="flat icon-only bar-tool" tabindex="-1" onclick={() => run("collapse-sidebar")} title={tip(open ? _("Collapse the sidebar") : _("Expand the sidebar"), "collapse-sidebar")} aria-label={open ? _("Collapse the sidebar") : _("Expand the sidebar")} aria-expanded={open}><Icon name="menu" size={21} /></button>
      <button class="flat icon-only bar-tool" tabindex="-1" onclick={localShell} title={tip(_("Open a local shell"), "local-shell")} aria-label={_("Open a local shell")}><Icon name="terminal" size={21} /></button>
      <button class="flat icon-only bar-tool" tabindex="-1" class:on={showTunnels} onclick={() => (showTunnels = !showTunnels)} title={tip(_("SSH tunnels"), "tunnels")} aria-label={_("SSH tunnels")}><Icon name="transfer-alt" size={21} /></button>
      <button class="flat icon-only bar-tool" tabindex="-1" class:on={!!layoutsMenu} onclick={(e) => (layoutsMenu = below(e, layoutsMenu))} title={_("Saved layouts: sessions opened together in panes")} aria-label={_("Saved layouts")}><Icon name="grid-alt" size={21} /></button>
      <button class="icon-only bar-tool new" tabindex="-1" onclick={createSession} title={tip(_("New session"), "new-session")} aria-label={_("New session")}><Icon name="plus" size={21} /></button>
    {/if}
  {/snippet}
  <!-- The centre: what has the keyboard (a way into the palette), and the workspace's picker. Locked, nothing. -->
  {#if ready && app.locked === null && !lock.locked}
    {@const ws = _("Workspace: {name}", { name: app.workspace || _("Default") })}
    <div class="finder">
      <div class="field">
        <button class="flat where" tabindex="-1" title={tip(_("Command palette"), "palette")} onclick={() => run("palette")}>
          {#if activeTab}
            {@const look = lookOf(activeTab.session)}
            {#if activeTab.alive}<SessionIcon icon={look.icon} color={look.color} size={16} still />{:else}<Icon name="error" size={16} color="var(--warn)" />{/if}
            <span class="name">{titleOf(activeTab)}</span>
            <!-- the shell's or page's own title, when it says more than the name -->
            {#if activeTab.title && activeTab.title !== titleOf(activeTab)}<span>{activeTab.title}</span>{/if}
          {:else}
            <span>{_("Type a command, a session, or user@host to connect")}</span>
          {/if}
        </button>
        <button class="flat icon-only workspace" class:on={!!workspaceMenu} tabindex="-1" aria-haspopup="dialog" aria-expanded={!!workspaceMenu} title={ws} aria-label={ws} onclick={toggleWorkspaces}>
          <Icon name="chevron-down" size={18} />
        </button>
      </div>
    </div>
  {/if}
  {#snippet end()}
    {#if ready && app.locked === null && !lock.locked}
      <div class="layouts" role="group" aria-label={_("Layout")}>
        {#each LAYOUTS as [l, name] (l)}
          <button class="flat icon-only bar-tool" tabindex="-1" class:on={layout === l} title={tip(_(name), `layout-${l}`)} aria-label={_(name)} onclick={() => setLayout(l)}>{@render layoutGlyph(l)}</button>
        {/each}
      </div>
    {/if}
  {/snippet}
</TitleBar>
<LockScreen />

<div class="toasts" aria-live="polite">
  <!-- gone a little faster than it came -->
  {#each toasts as t (t.id)}
    <div class="toast" out:fade={{ duration: still ? 0 : 100 }}>
      {t.text}
      {#if t.action}{@const act = t.action}<button class="link" onclick={() => (dismiss(t.id), act.run())}>{act.label}</button>{/if}
    </div>
  {/each}
</div>

<style>
  /* Rounded panels floating on the window, like a modern code editor: the sidebar, the tabs
     with their panes, the file panel. The splitter is the gap between the first two. */
  .app {
    height: 100%;
    display: flex;
    padding: 0 var(--gap) var(--gap); /* the title bar is the room above */
  }
  .rack,
  .center,
  .files {
    border: 1px solid var(--line);
    border-radius: var(--r-window);
    overflow: hidden;
  }

  /* ---- the session rack (sidebar) ---- */
  .rack {
    flex: none;
    display: flex;
    flex-direction: column;
    position: relative;
    isolation: isolate; /* its Backdrop goes under the rest */
    min-width: 0;
    background: rgba(var(--panel-rgb), var(--chrome-alpha));
  }
  .status {
    gap: var(--gap);
    margin: 0 5px 5px;
    padding: 2px;
    padding-inline-start: 8px;
    border-radius: var(--r-control);
    background: rgba(var(--deep-rgb), 1);
    font-size: var(--t-s);
    color: var(--weak);
    /* a message arriving at the foot of the sidebar: noticed, without a jolt */
    animation: fade-in var(--move) var(--ease-out);
  }
  .status :is(span, .link) {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* settings, then the account, at the end of the sidebar's foot */
  .corner {
    gap: 2px;
    padding: 0 5px 5px;
  }
  .corner .settings {
    margin-inline-start: auto;
  }
  /* Getting started's progress at the tab bar's end: a ring filling with the steps done, the
     name and the count, on an amber wash with an amber edge (the accent as a fill, as a picked
     button wears it) so it stands out from the quiet tools beside it: it's where the list went. */
  .tools .guide-button {
    flex: none;
    gap: var(--gap);
    padding: 2px 8px 2px 5px;
    border: 1px solid var(--picked-line);
    background: var(--gold-soft);
    color: var(--fg);
    font-size: var(--t-s);
    font-weight: 500;
  }
  .tools .guide-button:hover:not(:disabled),
  .tools .guide-button.open {
    background: var(--picked);
    color: var(--fg);
  }
  .guide-button .count {
    font-family: var(--short);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  /* a ring widening out of its edge and fading, twice */
  .guide-button.nudge {
    animation: guide-nudge 1.2s var(--ease-out) 2;
  }
  @keyframes guide-nudge {
    from {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--gold-fill) 70%, transparent);
    }
    to {
      box-shadow: 0 0 0 8px transparent;
    }
  }
  .steps-ring {
    flex: none;
    width: 16px;
    height: 16px;
    rotate: -90deg;
    fill: none;
    /* the track, what's left to do: a hairline's `--line` vanished on the amber wash */
    stroke: color-mix(in srgb, var(--weak) 55%, transparent);
    stroke-width: 2;
  }
  .steps-ring .fill {
    stroke: var(--gold-fill);
    stroke-linecap: round;
  }
  /* the list away from the start screen, hanging from the tab bar's end over the panes */
  .guide-pop {
    width: min(377px, calc(100vw - 2 * 13px));
    max-height: calc(100vh - var(--titlebar) - var(--bar) - 3 * 13px);
    overflow: auto;
    padding: 0;
  }
  /* placed by its end already: not moved back by its width, as a menu at the pointer is (app.css) */
  :global([dir="rtl"]) .guide-pop {
    translate: none;
  }
  .rack :global(.on) {
    color: var(--gold);
  }
  .splitter {
    width: var(--gap);
    flex: none;
    cursor: col-resize;
  }
  /* the collapsed sidebar: its foot in a column */
  .rail {
    width: calc(var(--control) + 2 * var(--gap) + 2px);
    align-items: center;
    gap: 2px;
    padding: 5px 0 8px;
  }

  /* ---- the title bar: tools by the mark, a field in the centre, the layouts at the end ---- */
  /* the bar's own tools step back like its text until pointed at */
  .bar-tool:not(.on, :hover) {
    color: inherit;
  }
  /* every control in the bar is a square as tall as the field, so the row shares one top and
     bottom edge */
  .bar-tool {
    width: var(--control);
    height: var(--control);
    min-height: 0;
    padding: 0;
    justify-content: center;
  }
  /* a new session is the bar's one standing action: a filled key, set a little apart */
  .new {
    margin-inline-start: 13px;
    border: 0;
    background: var(--raised);
    color: var(--fg);
  }
  .new:hover {
    background: var(--hover);
  }
  /* On light `raised` is barely off the bar's grey, so the key is white like the field beside it,
     and see-through it is a second layer over the bar, as the field is. */
  :global(:root[data-theme="light"]) .new:not(:hover) {
    background: rgba(var(--deep-rgb), var(--chrome-alpha));
  }
  /* the field: never so wide that no bar is left to drag the window by */
  .finder {
    display: flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
  }
  .field {
    display: flex;
    align-items: stretch;
    width: clamp(233px, 38vw, 610px);
    min-width: 0;
    height: var(--control);
    border: 1px solid var(--field-line);
    border-radius: var(--r-control);
    background: rgba(var(--deep-rgb), 1);
    overflow: hidden;
  }
  /* See-through on light, a solid white field stood out as a slab on the bar the desktop greys:
     it is the second layer over the bar's ground, as a panel is, and its text is ink (`--bar-ink`). */
  :global(:root[data-theme="light"]) .field {
    background: rgba(var(--deep-rgb), var(--chrome-alpha));
  }
  .field:hover {
    border-color: color-mix(in srgb, var(--fg) 25%, var(--field-line));
  }
  /* the focused tab, its type's icon and its name, centred; a way into the palette */
  .where {
    flex: 1;
    min-width: 0;
    min-height: 0;
    justify-content: center;
    padding: 0 8px;
    border: 0;
    border-radius: 0;
    color: inherit;
  }
  .where:hover {
    color: var(--fg);
  }
  .where span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where .name {
    flex: none;
    max-width: 233px;
    color: var(--fg);
    font-weight: 500;
  }
  /* the name and its icon are one thing; what the shell says of itself stands a little off */
  .where .name + span {
    margin-inline-start: 3px;
  }
  /* the workspace's menu, at the field's end behind a line */
  .workspace {
    flex: none;
    width: var(--control);
    min-height: 0;
    padding: 0;
    border: 0;
    border-inline-start: 1px solid var(--line);
    border-radius: 0;
    justify-content: center;
  }
  .layouts {
    display: flex;
    gap: 3px;
  }

  /* ---- tabs and view tools share one strip ---- */
  .center {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  /* editor-style tab bar: flat 34px tabs split by lines; the active one joins its pane */
  .strip {
    position: relative;
    isolation: isolate; /* its Backdrop goes under the tabs and tools */
    display: flex;
    align-items: stretch;
    gap: 8px;
    height: var(--bar);
    flex: none;
    background: rgba(var(--panel-rgb), var(--chrome-alpha));
    box-shadow: inset 0 -1px var(--line);
  }
  /* the tabs and the arrows over their ends. At an end with tabs past it the tabs are hidden under
     the arrow (`--edge`, its width) and fade in over `--fade` after it; a tab scrolled into view
     stops clear of both. */
  .tabs-wrap {
    --edge: var(--control);
    --fade: 21px;
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
  }
  .tabs {
    flex: 1;
    min-width: 0;
    display: flex;
    overflow-x: auto;
    scrollbar-width: none;
    --hide-start: 0px;
    --fade-start: 0px;
    --hide-end: 0px;
    --fade-end: 0px;
    --toward: right;
    scroll-padding-inline: calc(var(--edge) + var(--fade));
  }
  :global([dir="rtl"]) .tabs {
    --toward: left;
  }
  /* tabs past an end: hidden under its arrow, then fading in, so the strip reads as going on */
  .tabs.over-start {
    --hide-start: var(--edge);
    --fade-start: var(--fade);
  }
  .tabs.over-end {
    --hide-end: var(--edge);
    --fade-end: var(--fade);
  }
  .tabs-step {
    position: absolute;
    z-index: 1;
    top: 0;
    bottom: 1px; /* clear of the strip's bottom line */
    /* over the fade too, which shows a tab's X: a click there goes this way, it closes nothing */
    width: calc(var(--edge) + var(--fade));
    padding: 0;
    border-radius: 0;
    color: var(--fg);
  }
  /* the chevron in the outer part, over the hidden tabs; the faded ones show through the rest */
  .tabs-step.start {
    inset-inline-start: 0;
    padding-inline-end: var(--fade);
  }
  .tabs-step.end {
    inset-inline-end: 0;
    padding-inline-start: var(--fade);
  }
  .tabs-step:hover:not(:disabled) {
    background: none;
    color: var(--gold);
  }
  /* one chevron, pointing the way it goes: turned around for the start, and both in right to left */
  .tabs-step.start :global(.icon),
  :global([dir="rtl"]) .tabs-step.end :global(.icon) {
    transform: scaleX(-1);
  }
  :global([dir="rtl"]) .tabs-step.start :global(.icon) {
    transform: none;
  }
  .tabs:is(.over-start, .over-end) {
    mask-image: linear-gradient(
      to var(--toward),
      transparent var(--hide-start),
      #000 calc(var(--hide-start) + var(--fade-start)),
      #000 calc(100% - var(--hide-end) - var(--fade-end)),
      transparent calc(100% - var(--hide-end))
    );
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    padding: 0 5px 0 5px;
    color: var(--weak);
    border-inline-end: 1px solid var(--line);
    box-shadow: inset 0 -1px var(--line);
    flex: none;
  }
  /* the connector: a line on top in the tab's mark (its own, its session's or folder's), else
     the cable colour of its kind: dim on every tab, half on a tiled one, full on the focused one */
  .tab::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 2px;
    background: var(--tint, var(--kind));
    opacity: 0.3;
  }
  .tab.visible {
    color: var(--fg);
  }
  /* over the strip, so only while the window is solid: see-through, it would be a third layer */
  .tab.active {
    background: rgba(var(--bg-rgb), var(--solid));
    box-shadow: none;
    color: var(--fg);
  }
  /* where typing goes reads as the title bar's field says it: the mono's widths don't change
     with its weight, so the strip doesn't shift */
  .tab.active .title {
    font-weight: 500;
  }
  .tab.visible::before {
    opacity: var(--dim);
  }
  .tab.active::before {
    opacity: 1;
  }
  .tab.dead .label {
    color: var(--warn);
  }
  .tab .label {
    padding: 0 5px;
    min-height: var(--control);
    color: inherit;
  }
  .tab .label:hover:not(:disabled) {
    background: transparent;
  }
  .tab .label :global(.icon) {
    color: var(--kind);
  }
  .tab .title {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* a tab shared with the ring wears a small ring after its name */
  .tab .shared {
    flex: none;
    width: 8px;
    height: 8px;
    border: 2px solid var(--ok);
    border-radius: 50%;
  }
  .tab.lifted {
    opacity: var(--dim);
  }
  /* where a dragged tab would go: a bar at this tab's start (or the last one's end) */
  .tab.before::after,
  .tab.after::after {
    content: "";
    position: absolute;
    top: 5px;
    bottom: 5px;
    width: 2px;
    background: var(--gold);
  }
  .tab.before::after {
    inset-inline-start: 0;
  }
  .tab.after::after {
    inset-inline-end: 0;
  }
  .rename {
    width: 160px;
    height: var(--control);
    margin: 0 2px;
  }
  .menu .tint {
    gap: var(--gap);
    padding: 5px 8px;
  }
  .menu .tint .swatch {
    min-height: 0;
    padding: 0;
    border-radius: 50%;
    background: var(--c);
  }
  .tab .close {
    min-height: var(--row);
    padding: 2px;
    opacity: var(--dim);
  }
  .tab .close:hover {
    opacity: 1;
  }
  .tools {
    gap: 2px;
    padding: 0 5px;
  }
  .tools button {
    color: var(--weak);
  }
  .tools button.on {
    color: var(--gold);
  }

  /* ---- panes ---- */
  .panes {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  /* split: each pane a panel of its own, with the window's dark showing between them */
  .split .view {
    border: 1px solid var(--line);
  }
  .split :is(.view, .pane) {
    border-radius: var(--r-menu);
  }
  .pane {
    position: absolute;
    display: grid;
    place-items: center;
    border-radius: var(--r-control);
  }
  .pane.empty {
    border: 1px dashed var(--line);
  }
  /* The focused pane's ring, outside the tile, in the gap: clear of the broadcast outline drawn
     inside terminals, and of desktops and website views, which draw over the tile itself. One
     ring for every pane, so moving the focus glides it over and shows which way it went. */
  .ring {
    position: absolute;
    z-index: 2;
    pointer-events: none;
    border-radius: var(--r-control);
    outline: 2px solid var(--gold-fill);
    transition-property: left, top, width, height;
    transition-duration: var(--glide);
    transition-timing-function: var(--ease-out);
  }
  .split .ring {
    border-radius: var(--r-menu);
  }
  /* it follows a dragged edge at once, and never glides when set to Static (Settings › Appearance) */
  :is(.sizing, .steady) .ring {
    transition: none;
  }
  /* Broadcast outlines the terminals too, in a colour that may be the ring's, right inside the
     tile's edge: next to it the ring read as one thick band. The ring is dashed instead, in the
     window's own foreground colour (grey on light, where black dashes looked dated), so the pane
     with the keyboard stands apart from the outlined ones. It stays where the ring is: stepped
     out further, the window's edge cut it off at the sides and the bottom. */
  .broadcasting .ring {
    outline: 2px dashed var(--fg);
  }
  :global(:root[data-theme="light"]) .broadcasting .ring {
    outline-color: var(--weak);
  }
  /* the lines between panes: as wide as the gap, above the panes */
  .sizer {
    position: absolute;
    z-index: 2;
  }
  .sizer.rows {
    left: 0;
    right: 0;
    height: 6px;
    margin-top: -3px;
    cursor: row-resize;
  }
  .sizer.cols {
    width: 6px;
    margin-left: -3px;
    cursor: col-resize;
  }
  .sizer:hover {
    background: color-mix(in srgb, var(--gold-fill) 45%, transparent);
  }
  /* one file panel per session on screen, stacked */
  .files {
    width: 340px;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    margin-inline-start: var(--gap);
    background: rgba(var(--panel-rgb), var(--chrome-alpha));
  }
  /* one chip per tab with files when there are more than panels, under them */
  .files .chips {
    flex: none;
    gap: 3px;
    padding: 5px 8px;
    border-top: 1px solid var(--line);
  }
  .files .chip {
    height: var(--row);
    padding: 0 5px;
    font-size: var(--t-s);
    color: var(--weak);
  }
  .files .chip.on {
    color: var(--fg);
    background: var(--raised);
  }
  .view {
    position: absolute;
    z-index: 1;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-control);
    overflow: hidden;
  }
  .handle {
    flex: none;
  }
  .handle.movable {
    cursor: grab;
  }
  /* drop zones over the panes while a tab is dragged */
  .drops {
    position: absolute;
    inset: 0;
    z-index: 3;
  }
  .drop {
    position: absolute;
    display: grid;
    place-items: center;
    border: 2px dashed var(--line);
    border-radius: var(--r-control);
    background: rgba(var(--bg-rgb), 0.55);
    color: var(--weak);
    transition: background var(--quick), border-color var(--quick);
  }
  .drop span {
    pointer-events: none;
    max-width: 80%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drop.over {
    border-color: var(--gold);
    background: var(--gold-soft);
    color: var(--fg);
  }
  /* the tab's terminal or desktop, above its own status bar */
  .view > .body {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  /* ---- toasts: a moment's note at the bottom, over everything ---- */
  .toasts {
    position: fixed;
    inset-inline: 0;
    bottom: var(--bar);
    z-index: 60;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--gap);
    pointer-events: none;
  }
  .toast {
    padding: 5px 13px;
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
    background: rgba(var(--panel-rgb), var(--float-alpha));
    box-shadow: var(--shadow-menu);
    animation: float-in var(--move) var(--ease-out);
  }
  /* one with a button (Undo) takes the pointer */
  .toast:has(button) {
    display: flex;
    gap: 13px;
    pointer-events: auto;
  }

  /* ---- start screen ---- */
  /* The welcome sits on the golden section: the room above it and below it go 1 : φ. */
  .dashboard {
    position: absolute;
    inset: 0;
    background: rgba(var(--bg-rgb), var(--chrome-alpha));
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 34px;
  }
  .dashboard::before,
  .dashboard::after {
    content: "";
    flex: 1 1 0;
  }
  .dashboard::after {
    flex-grow: var(--phi);
  }
  .intro {
    flex: none;
    width: min(610px, 100%);
    display: flex;
    flex-direction: column;
    gap: 13px;
  }
  .intro p {
    margin: 0;
    max-width: 62ch;
    text-wrap: pretty; /* no last line of one word */
  }
  /* the website's prompt, waiting for you: the amber sigil and a block cursor (only the start
     screen's: a host-key question is a `.prompt` too, and must read as text, not as muted mono) */
  .intro .prompt {
    font-family: var(--mono);
    color: var(--weak);
  }
  .intro .prompt b {
    font-weight: 500;
    color: var(--gold);
  }
  /* what you have typed, as the shell will get it (spaces and all) */
  .intro .prompt .typed {
    color: var(--fg);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cursor {
    display: inline-block;
    width: 0.6em;
    height: 1.2em;
    vertical-align: text-bottom;
    background: var(--gold-fill);
    animation: blink 1.1s steps(1) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
  .logo {
    width: min(377px, 100%);
    margin-block-end: 8px;
    animation: float-in var(--glide) var(--ease-out) both;
  }
  /* with a link under it (Settings › Appearance) the logo is a button, looking the same */
  button.logo {
    display: block;
    min-height: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: none;
  }
  button.logo:hover {
    opacity: 0.85;
  }
  .lead {
    font-family: var(--short);
    font-size: var(--t-xl);
    font-weight: 600;
    letter-spacing: -0.025em;
  }
  /* A shell opened by typing on the start screen stays unseen until it can take keys and the
     welcome has gone, then fades in (`arriving`, the quick shell above). */
  .view.unseen {
    opacity: 0;
  }
  .view.arrive {
    opacity: 1;
    animation: fade-in var(--glide) var(--ease-out);
  }
  /* the ways to start, side by side */
  .starts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap);
  }
  .starts button {
    gap: 8px;
    padding: 5px 8px 5px 5px;
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
  }
  .tools .tasks {
    gap: var(--gap);
    padding: 2px 5px;
    font-variant-numeric: tabular-nums;
  }
  /* hangs from the Tasks button's end */
  .tasks-list {
    min-width: 260px;
    translate: -100% 0;
  }
  .tasks-list .task {
    gap: 2px;
  }
  /* every open tab: as long as the window allows, then it scrolls */
  .all-tabs {
    max-width: min(377px, calc(100vw - 2 * 13px));
    max-height: calc(100vh - var(--titlebar) - var(--bar) - 3 * 13px);
    overflow-y: auto;
  }
  /* right to left it starts at the button's left edge and opens over the tabs, not moved back by
     its width as the list above and a menu at the pointer are (app.css) */
  :global([dir="rtl"]) .all-tabs {
    translate: none;
  }
  .all-tabs button {
    flex: none;
    gap: var(--gap);
  }
  .all-tabs button :global(.icon) {
    color: var(--kind);
  }
  /* the tab in use, as a picked option */
  .all-tabs button.current {
    background: var(--picked);
    color: var(--fg);
  }
  .tasks-list .task :global(.icon) {
    color: var(--kind);
  }
  .tasks-list .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: start;
  }

  /* A dialog's paragraphs are blocks of its body, set apart by its 8px gap as in every other
     dialog; the browser's own 13px margins doubled the room around them. */
  p {
    margin: 0;
  }
  .tasks-list .none {
    margin: 5px 8px;
  }
  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 8px 13px;
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
  }
  .side dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 5px 8px;
    margin: 0;
  }
  .side dt {
    color: var(--weak);
  }
  .side dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  div.prompt {
    white-space: pre-line;
    user-select: text;
    word-break: break-word;
  }
  .preview {
    display: block;
    padding: 5px 8px;
    font-family: var(--mono);
    font-size: var(--t-s);
    white-space: pre-wrap;
    word-break: break-all;
    background: rgba(var(--deep-rgb), 0.6);
    border: 1px solid var(--line);
    border-radius: var(--r-control);
  }
</style>
