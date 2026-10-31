<script lang="ts" module>
  import { app } from "./state.svelte";
  // The page you were on, kept while the app runs.
  let last = "general";
  /** Turns the open Settings window to a page. */
  let turn: ((p: string) => void) | null = null;
  /** Opens Settings on page `p`, from anywhere: a link on text that names a setting. Already
   *  open, it turns to that page. */
  export function openSettings(p: string) {
    last = p;
    turn?.(p);
    app.showSettings = true;
  }
</script>

<script lang="ts">
  // Settings window, one page per category. Changes apply (and save) as you make them;
  // App.svelte watches them.
  import { onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ACTIONS, chord, keysOf, score, TERMINAL_KEYS } from "./actions";
  import { ACCENTS, COLORS, css, openUrl, OUTLINES, type CursorShape, type Login, type Scheme, type Settings } from "./api";
  import About from "./About.svelte";
  import BackgroundPicker from "./BackgroundPicker.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import Ciphers from "./Ciphers.svelte";
  import Export from "./Export.svelte";
  import Import from "./Import.svelte";
  import PathInput from "./PathInput.svelte";
  import QualityBar from "./QualityBar.svelte";
  import Quota from "./Quota.svelte";
  import { CATALOGUE, CATALOGUE_RAW, catalogueName, parseScheme } from "./schemes";
  import { _, LOCALES, N_, nativeName, pick } from "./i18n.svelte";
  import { boot, confirmIt, copyText, loadLock, lock, signOut, syncNow, toast, workspaceOp } from "./state.svelte";
  import { passkeyMake, passkeysWork } from "./passkey";

  /** `onrun`: runs a command in a new local shell (the MCP page's Run buttons; App.svelte). */
  let { onclose, onrun }: { onclose: () => void; onrun: (command: string) => void } = $props();
  const s = $derived(app.settings!);
  const transparent = $derived(s.effect !== "None");

  // Everyday first, then how connections behave, then accounts and safety; automation last.
  const PAGES: [string, string, string][] = [
    ["general", "cog", N_("General")],
    ["appearance", "adjust", N_("Appearance")],
    ["terminal", "terminal", N_("Terminal")],
    ["shortcuts", "grid-alt", N_("Shortcuts")],
    ["defaults", "server", N_("Defaults")],
    ["workspaces", "cluster", N_("Workspaces")],
    ["logins", "log-in", N_("Saved logins")],
    ["keys", "key", N_("SSH keys")],
    ["browser", "globe", N_("Browser")],
    ["backup", "backup", N_("Import and export")],
    ["sync", "user-circle", N_("Account and sync")],
    ["security", "lock-alt", N_("Security")],
    ["automation", "target-lock", N_("MCP")],
    ["troubleshoot", "error", N_("Troubleshoot")],
    ["about", "info-circle", N_("About")],
  ];
  const pageName = (id: string) => _(PAGES.find((p) => p[0] === id)![2]);

  // ---- search: what each page shows, as it reads there ----
  const FINDS: Record<string, string[]> = {
    general: [
      N_("Language"),
      N_("Tabs"),
      N_("Ask before closing a tab that is still connected"),
      N_("Keep running in the tray when the window is closed, so tabs and background tasks go on"),
      N_("Reopen the tabs that were open when xtermium closed"),
      N_("Show a status bar under each pane: connection, uptime, encryption and response time"),
      N_("This computer"),
      N_("Start xtermium when you sign in"),
      N_("Desktop notifications: a new version, a connection that dropped"),
      N_("Also when an AI agent is done or waiting for you"),
      N_("Run above the normal process priority (may need administrator rights; turning it off takes a restart)"),
      N_("Sessions"),
      N_("Open a session with one click (instead of a double-click)"),
      N_("Remember which folders are open in the sidebar"),
    ],
    appearance: [
      N_("Theme"),
      N_("Interface scale"),
      N_("Window effect"),
      N_("Window opacity"),
      N_("Terminal opacity"),
      N_("Accent"),
      N_("Terminal colours"),
      N_("Backgrounds"),
      N_("Focused pane"),
      N_("Broadcast outline"),
      N_("Reset appearance and terminal settings"),
    ],
    terminal: [
      N_("Font and cursor"),
      N_("Family"),
      N_("Size"),
      N_("Cursor"),
      N_("Line height"),
      N_("Pane padding"),
      N_("Behaviour"),
      N_("Scrollback"),
      N_("Clipboard"),
      N_("Copy on select"),
      N_("Right-click pastes"),
      N_("Strip whitespace from the start and end of what you paste"),
      N_("Show a scroll bar"),
      N_("Making a pane bigger or smaller with the keyboard changes it by"),
      N_("Scroll to the bottom on output"),
      N_("Background"),
      N_("Behind the text"),
      N_("Highlight rules"),
      N_("Colour words and addresses in terminal output"),
      N_("Logging"),
      N_("Save what each terminal tab shows to a text file"),
    ],
    shortcuts: [N_("Reset all shortcuts"), ...ACTIONS.map((a) => a.label)],
    defaults: [
      N_("Local shell"),
      N_("Program"),
      N_("SSH"),
      N_("Private key"),
      N_("Password"),
      N_("SSH connections"),
      N_("Ciphers"),
      N_("Keepalive"),
      N_("Compression"),
      N_("Reconnect by itself when a connection drops (up to 8 tries, waiting longer each time)"),
      N_("Remote desktop"),
      N_("Picture"),
      N_("The file panel opens the host's shared folders (SMB) with the desktop's login, without asking"),
    ],
    workspaces: [N_("Default workspace"), N_("New workspace"), N_("Sync")],
    logins: [N_("Shared logins")],
    keys: [N_("New key"), N_("Create key"), N_("Public key")],
    browser: [N_("Open Browser sessions"), N_("External: in your default web browser"), N_("Embedded: inside an xtermium tab")],
    backup: [N_("Import sessions…"), N_("Export sessions…"), N_("MobaXterm"), N_("Restore points"), N_("Take a restore point"), N_("Back up everything"), N_("Restore everything")],
    sync: [
      N_("Account"),
      N_("Sign in"),
      N_("Create an account"),
      N_("Sign out"),
      N_("Show the account button in the sidebar"),
      N_("Sync now"),
      N_("Delete the cloud copy"),
    ],
    security: [N_("Encryption"), N_("Recovery passphrase"), N_("Lock"), N_("PIN"), N_("Passkey"), N_("Windows Hello"), N_("Lock after"), N_("Lock now"), N_("Trusted server keys")],
    automation: [N_("Let AI assistants and scripts control xtermium"), N_("Listen on"), N_("Port"), N_("Token"), N_("AI assistants (MCP)"), N_("Run"), N_("Command line")],
    troubleshoot: [N_("Create bundle")],
    about: [N_("Built with"), N_("Check for updates"), N_("Tell me when a new version is out")],
  };
  let query = $state("");
  /** Fuzzy over each setting as shown (and in English), best first: [page, text]. */
  const found = $derived.by(() => {
    if (!query.trim()) return [];
    const hits: [number, string, string][] = [];
    for (const [id, , label] of PAGES)
      for (const t of [label, ...FINDS[id]]) {
        const sc = Math.max(score(query, _(t)), score(query, t));
        if (sc >= 0) hits.push([sc, id, _(t)]);
      }
    return hits
      .sort((a, b) => b[0] - a[0])
      .slice(0, 40)
      .map(([, id, text]) => [id, text] as [string, string]);
  });

  /** Opens the setting's page and scrolls to it: its heading, check box, or label (with its
   *  field, the label's neighbour in the form). */
  async function locate(id: string, text: string): Promise<HTMLElement[]> {
    page = id;
    await tick();
    const body = document.querySelector(".window .body");
    const el = [...(body?.querySelectorAll<HTMLElement>("h3, label, .form > span, button, p") ?? [])].find((e) => e.textContent?.trim().startsWith(text));
    if (!el) return [];
    el.scrollIntoView({ block: "center" });
    const field = el.matches(".form > span") ? el.nextElementSibling : null;
    return field instanceof HTMLElement ? [el, field] : [el];
  }

  // While searching, the page follows the best hit and keeps it lit, so it's found at a glance.
  let lit: HTMLElement[] = [];
  $effect(() => {
    const best = found[0];
    lit.forEach((e) => e.classList.remove("lit"));
    lit = [];
    if (!best) return;
    locate(...best).then((els) => {
      if (found[0] !== best) return; // a newer search took over
      lit = els;
      lit.forEach((e) => e.classList.add("lit"));
    });
  });

  /** Goes to the setting: the search closes, the keyboard goes to its control, and it glows a
   *  moment longer. */
  async function reveal(id: string, text: string) {
    query = "";
    const els = await locate(id, text);
    const at = els.at(-1);
    const control = at?.matches("input, select, textarea, button") ? at : at?.querySelector<HTMLElement>("input, select, textarea, button");
    control?.focus({ preventScroll: true });
    for (const e of els) {
      e.classList.remove("found");
      void e.offsetWidth; // restarts the glow when found twice
      e.classList.add("found");
    }
  }
  /** Window effects, as the backend names them. */
  const EFFECTS: Record<string, string> = { None: N_("None"), Transparent: N_("Transparent"), Acrylic: N_("Acrylic"), Mica: N_("Mica"), Vibrancy: N_("Vibrancy") };
  /** Languages by their own names, in that order. */
  const languages = LOCALES.map((code) => [code, nativeName(code)]).sort((a, b) => a[1].localeCompare(b[1]));

  // ---- automation: MCP server and command line, off unless turned on ----
  let showToken = $state(false);
  /** "Open in xtermium" on File Explorer's folder menus: on or off, null where there's none (not Windows). */
  let explorerMenu = $state<boolean | null>(null);
  invoke<boolean | null>("explorer_menu").then((v) => (explorerMenu = v));
  async function setExplorerMenu(on: boolean) {
    try {
      await invoke("set_explorer_menu", { on, label: _("Open in xtermium") });
    } catch (e) {
      app.status = String(e);
    }
    explorerMenu = await invoke<boolean | null>("explorer_menu");
  }
  let automation = $state<[boolean, string]>([false, ""]); // running, error
  $effect(() => {
    if (page !== "automation") return;
    const poll = () => invoke<[boolean, string]>("automation_status").then((r) => (automation = r));
    poll();
    const t = setInterval(poll, 1000);
    return () => clearInterval(t);
  });
  /** "New token" asks first: every assistant set up with the old one has to be set up again. */
  let rotating = $state(false);
  function newToken() {
    rotating = false;
    s.automation_token = Array.from(crypto.getRandomValues(new Uint8Array(32)), (b) => b.toString(16).padStart(2, "0")).join("");
  }
  // The token is made once, when xtermium first starts, and kept; only "New token" replaces it.
  function setAutomation(on: boolean) {
    s.automation = on;
  }
  // On the network, assistants elsewhere reach this computer by an address of its own, over
  // HTTPS with a self-signed certificate (automation.rs `certificate`).
  let net = $state<{ addresses: string[]; fingerprint: string } | null>(null);
  let netHost = $state("");
  $effect(() => {
    if (!s.automation || !s.automation_network) return;
    invoke<{ addresses: string[]; fingerprint: string }>("automation_network_info").then((i) => {
      net = i;
      netHost ||= i.addresses[0] ?? "";
    });
  });
  const mcpUrl = $derived(s.automation_network ? `https://${netHost.trim() || "localhost"}:${s.automation_port}/mcp` : `http://127.0.0.1:${s.automation_port}/mcp`);
  const mcpConfig = (token: string) =>
    JSON.stringify({ mcpServers: { xtermium: { type: "http", url: mcpUrl, headers: { Authorization: `Bearer ${token}` } } } }, null, 2);
  /**
   * One command per assistant that adds this server; run it in a terminal. On the network they
   * run on another machine (a macOS or Linux shell, mostly), and the ones built on Node start the
   * assistant with certificate checks off for that run, as the certificate is self-signed.
   */
  const mcpCommands = (token: string): [string, string][] => {
    const url = mcpUrl;
    const header = `Authorization: Bearer ${token}`;
    const vscode = JSON.stringify({ name: "xtermium", type: "http", url, headers: { Authorization: `Bearer ${token}` } });
    const remote = s.automation_network;
    const start = (cmd: string) => (remote ? ` && NODE_TLS_REJECT_UNAUTHORIZED=0 ${cmd}` : "");
    const windows = app.os === "windows" && !remote;
    return [
      ["Claude Code", `claude mcp add --transport http xtermium ${url} --header "${header}"` + start("claude")],
      ["Gemini CLI", `gemini mcp add --transport http --header "${header}" xtermium ${url}` + start("gemini")],
      ["Copilot CLI", `copilot mcp add --transport http --header "${header}" xtermium ${url}` + start("copilot")],
      ["opencode", `opencode mcp add xtermium --global --url ${url} --header "Authorization=Bearer ${token}"` + start("opencode")],
      // Codex sends a bearer token it reads from an environment variable: set it for good first.
      [
        "Codex",
        (windows ? `setx XTERMIUM_TOKEN ${token} && ` : `echo 'export XTERMIUM_TOKEN=${token}' >> ~/.profile && export XTERMIUM_TOKEN=${token} && `) +
          `codex mcp add xtermium --url ${url} --bearer-token-env-var XTERMIUM_TOKEN`,
      ],
      // cmd and PowerShell 7 take \" inside double quotes; other shells take single quotes.
      ["VS Code", windows ? `code --add-mcp "${vscode.replaceAll('"', '\\"')}"` : `code --add-mcp '${vscode}'`],
    ];
  };
  /** The Codex (or, on the network, VS Code) command was copied: say what else to know. */
  let codexHint = $state("");
  const copy = copyText;

  // ---- import and export: sessions (Import/Export.svelte), and everything at once ----
  let showImport = $state(false);
  let showExport = $state(false);
  let backup = $state({ pass: "", again: "", file: null as File | null, restorePass: "", busy: false, error: "", done: "" });
  const backupOk = $derived(backup.pass.length >= 8 && backup.pass === backup.again);

  async function backupSave() {
    Object.assign(backup, { busy: true, error: "", done: "" });
    try {
      await saved();
      const where = await invoke<string | null>("backup_save", { passphrase: backup.pass });
      if (where) Object.assign(backup, { pass: "", again: "", done: _("Backed up to {path}.", { path: where }) });
    } catch (e) {
      backup.error = String(e);
    }
    backup.busy = false;
  }

  async function backupRestore() {
    Object.assign(backup, { busy: true, error: "", done: "" });
    try {
      const data = Array.from(new Uint8Array(await backup.file!.arrayBuffer()));
      await invoke("backup_restore", { data, passphrase: backup.restorePass });
      await boot();
      Object.assign(backup, { file: null, restorePass: "", done: _("Restored. Tabs that are open keep their old settings until they connect again.") });
    } catch (e) {
      backup.error = String(e);
    }
    backup.busy = false;
    points = null; // a restore point of what was there came first
  }

  // ---- restore points: full copies of everything, kept on this device (src/snapshot.rs) ----
  type Point = { id: string; at: number; reason: string; sessions: number; workspaces: number };
  let points = $state<Point[] | null>(null);
  /** The restore point waiting for a second click on Restore; `push`: then replace the cloud copy. */
  let restoring = $state<{ id: string; push: boolean } | null>(null);
  const REASONS: Record<string, string> = { manual: N_("Taken by you"), daily: N_("Daily"), "before-restore": N_("Before a restore"), "before-sync": N_("Before a workspace sync"), "before-share": N_("Before workspaces shared") };
  $effect(() => {
    if (page === "backup" && !points) invoke<Point[]>("restore_points").then((p) => (points = p));
  });
  const pointsDo = (cmd: string, args = {}) =>
    invoke<Point[]>(cmd, args)
      .then((p) => (points = p))
      .catch((e) => (backup.error = String(e)));
  async function restorePoint(id: string, push: boolean) {
    Object.assign(backup, { busy: true, error: "", done: "" });
    try {
      await saved();
      await invoke("restore_point_restore", { id });
      await boot();
      if (push) await syncNow(undefined, "local");
      backup.done = _("Restored. Tabs that are open keep their old settings until they connect again.");
    } catch (e) {
      backup.error = String(e);
    }
    restoring = null;
    points = null;
    backup.busy = false;
  }

  let logsFolder = $state("");
  invoke<string>("logs_folder").then((d) => (logsFolder = d));

  // ---- sync ----
  let join = $state({ pass: "", error: "", busy: false });
  let confirmDelete = $state(false);
  /** Settings save shortly after each change; make sure the backend has these first. */
  const saved = () => invoke("save_settings", { settings: $state.snapshot(s) });
  async function joinAccount() {
    [join.busy, join.error] = [true, ""];
    try {
      await saved();
      await syncNow(join.pass);
      join = { pass: "", error: "", busy: false };
      // A conflict waits for the choice (App.svelte asks); the join finishes with it.
      if (!app.sync.conflict) app.recovery = true;
    } catch (e) {
      [join.error, join.busy] = [String(e), false];
    }
  }
  async function deleteCloud() {
    if (!confirmDelete) return void (confirmDelete = true);
    confirmDelete = false;
    try {
      await saved();
      await invoke("sync_delete");
      [app.sync.last, app.sync.error] = [null, ""];
      app.status = _("The cloud copy is deleted. It's uploaded again on the next sync while sync is on.");
    } catch (e) {
      app.sync.error = String(e);
    }
  }

  // ---- shortcuts: only changes from the defaults are stored ----
  /** The action whose new chord is being typed. */
  let recording = $state<string | null>(null);
  let hint = $state("");

  function bind(id: string, keys: string) {
    if (keys === ACTIONS.find((a) => a.id === id)?.keys) delete s.shortcuts[id];
    else s.shortcuts[id] = keys;
  }

  /** While recording, every key is the chord's: nothing reaches the app or closes the window. */
  function record(e: KeyboardEvent, id: string) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return void (recording = null);
    if (e.key === "Backspace" && !e.ctrlKey && !e.altKey && !e.metaKey) return void ((recording = null), bind(id, ""));
    const c = chord(e);
    if (!c) return;
    // A plain key would be taken from everything you type, the terminal included.
    if (!/^(Ctrl|Alt|Meta)\+|^(Shift\+)?F\d+$/.test(c)) return void (hint = _("Use Ctrl, Alt or a function key (F1–F12)."));
    [recording, hint] = [null, ""];
    bind(id, c);
  }

  /** Why a chord may not do what you expect: another action has it, or the terminal uses it. */
  function clash(id: string, keys: string): string {
    if (!keys) return "";
    const other = ACTIONS.find((a) => a.id !== id && keysOf(a.id, s.shortcuts) === keys);
    if (other)
      return ACTIONS.indexOf(other) < ACTIONS.findIndex((a) => a.id === id)
        ? _("“{action}” uses the same keys and takes them first.", { action: _(other.label) })
        : _("“{action}” uses the same keys; this shortcut takes them first.", { action: _(other.label) });
    if (TERMINAL_KEYS[keys]) return _("The terminal uses these keys to {what}; this shortcut takes them over.", { what: _(TERMINAL_KEYS[keys]) });
    return "";
  }
  let page = $state(last);
  $effect(() => {
    last = page;
  });
  let nav = $state<HTMLElement>();
  /** Another page from the list: it opens at its top (the body is one scroller for every page,
   *  which would keep where the last one was scrolled to). */
  function goTo(id: string) {
    page = id;
    nav?.nextElementSibling?.scrollTo(0, 0);
  }

  // ---- troubleshooting: a bundle of plain-text facts for support@xtermium.com ----
  let bundle = $state({ busy: false, path: "", error: "" });
  async function makeBundle() {
    Object.assign(bundle, { busy: true, path: "", error: "" });
    try {
      bundle.path = await invoke<string>("troubleshoot_bundle");
    } catch (e) {
      bundle.error = String(e);
    }
    bundle.busy = false;
  }
  turn = (p) => (page = p);
  onDestroy(() => (turn = null));

  let fonts = $state<string[]>([]);
  invoke<string[]>("fonts").then((f) => (fonts = f));
  /** Windows only: null while checking, then the usable WSL distributions. */

  let showPassword = $state(false);

  async function reset() {
    // Saving only the defaults resets everything else (missing fields take their defaults).
    // Only appearance and terminal settings go back: defaults, automation, sync and the rest stay.
    const { theme, ui_scale, effect, window_opacity, terminal_opacity, scheme, font, font_size, override_sessions, cursor, cursor_blink, line_height, pane_padding, scrollback, scroll_on_output, copy_on_select, right_click_paste, strip_paste, ...keep } =
      $state.snapshot(s);
    app.settings = await invoke<Settings>("save_settings", { settings: keep });
  }

  // ---- colour schemes: imported from a link or pasted JSON, or picked from terminal.sexy ----
  let schemeSource = $state("");
  let schemeError = $state("");
  let schemeBusy = $state(false);
  /** terminal.sexy's catalogue: null until opened, then its file paths. */
  let catalogue = $state<string[] | null>(null);
  let catalogueQuery = $state("");
  const addFromCatalogue = (p: string) => addScheme(async () => [await invoke<string>("fetch_text", { url: CATALOGUE_RAW + p }), catalogueName(p)]);
  const catalogueHits = $derived(
    (catalogue ?? [])
      .map((p) => [p, catalogueQuery.trim() ? score(catalogueQuery, catalogueName(p)) : 0] as const)
      .filter(([, sc]) => sc >= 0)
      .sort((a, b) => b[1] - a[1])
      .map(([p]) => p),
  );

  /** Adds (or replaces, by name) an imported scheme and switches to it. */
  async function addScheme(get: () => Promise<[string, string]>) {
    [schemeBusy, schemeError] = [true, ""];
    try {
      const [text, name] = await get();
      const sc = parseScheme(text, name);
      if (sc.name === "Auto" || app.schemes.some((b) => b.name === sc.name)) sc.name += " (imported)";
      s.custom_schemes = [...s.custom_schemes.filter((c) => c.name !== sc.name), sc];
      s.scheme = sc.name;
      schemeSource = "";
    } catch (e) {
      schemeError = e instanceof Error ? e.message : String(e);
    }
    schemeBusy = false;
  }
  /** A link to a scheme's JSON, or the JSON itself. */
  const importScheme = () =>
    addScheme(async () => {
      const src = schemeSource.trim();
      if (src.startsWith("{")) return [src, "Imported"];
      return [await invoke<string>("fetch_text", { url: src }), decodeURIComponent(src.split("/").pop()!.replace(/\.json$/i, ""))];
    });
  async function openCatalogue() {
    [schemeBusy, schemeError] = [true, ""];
    try {
      const tree: { tree: { path: string }[] } = JSON.parse(await invoke<string>("fetch_text", { url: CATALOGUE }));
      catalogue = tree.tree.map((t) => t.path).filter((p) => p.startsWith("dist/schemes/") && p.endsWith(".json"));
    } catch (e) {
      schemeError = _("The catalogue didn't load: {error}", { error: String(e) });
    }
    schemeBusy = false;
  }
  const removeScheme = (name: string) => {
    s.custom_schemes = s.custom_schemes.filter((c) => c.name !== name);
    if (s.scheme === name) s.scheme = "Auto";
  };

  // ---- security ----
  let rec = $state({ current: "", pass: "", again: "", error: "", busy: false, done: false, show: false, copied: false });
  /** A change needs the passphrase there is (the backend checks it). */
  const recOk = $derived(rec.pass.length >= 12 && rec.pass === rec.again && (!app.recovery || !!rec.current));
  const PASS_CHARS = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789-_.!@#%+=";
  /** 24 random characters (about 145 bits), filled in both fields and copied. */
  function generatePass() {
    let pass = "";
    // Bytes past the last whole multiple of the alphabet would favour its first letters: skipped.
    const limit = 256 - (256 % PASS_CHARS.length);
    while (pass.length < 24)
      for (const b of crypto.getRandomValues(new Uint8Array(32))) if (b < limit && pass.length < 24) pass += PASS_CHARS[b % PASS_CHARS.length];
    [rec.pass, rec.again] = [pass, pass];
    copyPass();
  }
  function copyPass() {
    navigator.clipboard.writeText(rec.pass).then(
      () => ((rec.copied = true), toast(_("Copied to clipboard")), setTimeout(() => (rec.copied = false), 2500)),
      (e) => (rec.error = String(e)),
    );
  }
  async function setRecovery() {
    [rec.busy, rec.error] = [true, ""];
    try {
      await invoke("set_recovery", { passphrase: rec.pass, current: rec.current });
      app.recovery = true;
      rec = { current: "", pass: "", again: "", error: "", busy: false, done: true, show: false, copied: false };
      if (app.status.startsWith("Set a recovery passphrase")) app.status = "";
    } catch (e) {
      [rec.error, rec.busy] = [String(e), false];
    }
  }

  // ---- the lock (src/lock.rs): the ways to unlock, and when it locks ----
  let lockForm = $state({ password: "", pin: "", busy: false, error: "", done: "" });
  $effect(() => {
    if (page === "security") loadLock();
  });
  const lockOn = $derived(!!lock.info && (lock.info.password || lock.info.pin || !!lock.info.passkey || lock.info.biometrics));
  /** Changes the lock (lock.rs `Change`); with one already set, after an unlock to confirm it's
   *  you. `passkey: true` makes a new passkey first. */
  async function lockSet(change: Record<string, unknown>, done: string) {
    [lockForm.error, lockForm.done] = ["", ""];
    if (!lock.info?.confirmed && !(await confirmIt())) return;
    lockForm.busy = true;
    try {
      if (change.passkey === true) change = { set: "passkey", passkey: await passkeyMake() };
      await invoke("lock_set", { change });
      Object.assign(lockForm, { password: "", pin: "", done });
    } catch (e) {
      lockForm.error = e instanceof Error ? e.message : String(e);
    }
    lockForm.busy = false;
    loadLock();
  }
  const pinOk = $derived(/^\d{4,12}$/.test(lockForm.pin));

  // ---- highlight rules: patterns checked by the backend, which uses Rust's regex syntax ----
  let ruleErrors = $state<Record<number, string>>({});
  async function checkRule(i: number) {
    const error = await invoke<string>("regex_error", { pattern: s.highlights[i]?.pattern ?? "" });
    ruleErrors[i] = error.split("\n").filter(Boolean).pop() ?? "";
  }
  /** Saving without them brings the defaults back (missing fields take their defaults). */
  async function resetRules() {
    const { highlights, ...keep } = $state.snapshot(s);
    void highlights;
    app.settings = await invoke<Settings>("save_settings", { settings: keep });
    ruleErrors = {};
  }

  // ---- workspaces: each its own sessions and folders; the default one always stays ----
  let ws = $state({ name: "", error: "", done: "", renaming: null as string | null, to: "", deleting: null as string | null });
  /** `done`: what to say once it worked. */
  async function wsDo(op: Parameters<typeof workspaceOp>[0], done = "") {
    [ws.error, ws.done] = ["", ""];
    try {
      await workspaceOp(op);
      Object.assign(ws, { name: "", renaming: null, deleting: null, done });
      return true;
    } catch (e) {
      ws.error = String(e);
      return false;
    }
  }
  const wsName = (w: string) => w || _("Default workspace");
  // Sync: one workspace's sessions and folders into another.
  let wsSync = $state({ from: "", to: null as string | null });
  const syncTo = $derived(wsSync.to !== null && wsSync.to !== wsSync.from ? wsSync.to : null);
  async function runSync() {
    const { from } = $state.snapshot(wsSync);
    if (syncTo === null) return;
    wsDo({ op: "sync", from, to: syncTo }, _("Copied the sessions of {from} to {to}.", { from: wsName(from), to: wsName(syncTo) }));
  }

  // ---- SSH keys: the key pairs in ~/.ssh (src/sshkeys.rs) ----
  type SshKey = { name: string; kind: string; comment: string; fingerprint: string; public: string; encrypted: boolean };
  let sshKeys = $state<{ dir: string; keys: SshKey[]; comment: string } | null>(null);
  let newKey = $state({ kind: "ed25519", name: "", comment: "", pass: "", show: false, busy: false, error: "" });
  let keyDeleting = $state<string | null>(null);
  const loadKeys = () => invoke<typeof sshKeys>("ssh_keys").then((k) => (sshKeys = k), (e) => (newKey.error = String(e)));
  $effect(() => {
    if (page === "keys") loadKeys();
  });
  /** `id_<type>`, numbered when that's taken. */
  function freeName(kind: string) {
    const taken = new Set(sshKeys?.keys.map((k) => k.name));
    let [name, n] = [`id_${kind}`, 1];
    while (taken.has(name)) name = `id_${kind}_${++n}`;
    return name;
  }
  async function createKey() {
    [newKey.busy, newKey.error] = [true, ""];
    try {
      const name = newKey.name.trim() || freeName(newKey.kind);
      const comment = newKey.comment.trim() || sshKeys?.comment || "";
      await invoke("ssh_key_create", { name, kind: newKey.kind, comment, passphrase: newKey.pass });
      Object.assign(newKey, { name: "", pass: "", show: false, busy: false });
      await loadKeys();
      toast(_("Made the key {name}.", { name }));
    } catch (e) {
      [newKey.error, newKey.busy] = [String(e), false];
    }
  }
  /** The first click asks, the second deletes (both files). */
  async function deleteKey(name: string) {
    if (keyDeleting !== name) return void (keyDeleting = name);
    keyDeleting = null;
    await invoke("ssh_key_delete", { name }).catch((e) => (newKey.error = String(e)));
    loadKeys();
  }

  // ---- saved logins: passwords remembered when asked for, every workspace's ----
  type Row = Login & { edit: string; show: boolean };
  let logins = $state<Row[] | null>(null);
  const loadLogins = () => invoke<Login[]>("saved_logins").then((l) => (logins = l.map((x) => ({ ...x, edit: x.password, show: false }))));
  $effect(() => {
    if (page === "logins") loadLogins();
  });
  async function setLogin(r: Row, password: string | null) {
    await invoke("set_login", { id: r.id, password }).catch((e) => (app.status = String(e)));
    loadLogins();
  }
  /** What a login opens, by its id: `rdp:…`, `vnc:…`, `winrm:…`, else SSH. */
  const loginLook = (id: string): [string, string, string, string] => {
    const m = id.match(/^(rdp|vnc|winrm):(.*)$/);
    const [kind, rest] = m ? [m[1], m[2]] : ["ssh", id];
    return {
      ssh: ["lock-alt", "var(--k-ssh)", "SSH", rest],
      rdp: ["desktop", "var(--k-rdp)", "RDP", rest],
      vnc: ["monitor", "var(--k-vnc)", "VNC", rest],
      winrm: ["network", "var(--k-winrm)", "WinRM", rest],
    }[kind] as [string, string, string, string];
  };

  type Known = { ssh: Record<string, string>; rdp: Record<string, string>; https?: Record<string, string> };
  let known = $state<Known | null>(null);
  $effect(() => {
    if (page === "security" && !known) invoke<Known>("known_keys").then((k) => (known = k));
  });
  async function forget(kind: "ssh" | "rdp" | "https", id: string) {
    await invoke("forget_key", { kind, id });
    known = await invoke<Known>("known_keys");
  }
  /** Trusted keys: fuzzy search over host and key, five to a page. */
  const PER_PAGE = 5;
  let keyQuery = $state("");
  let keyPage = $state(0);
  const keyRows = $derived.by(() => {
    if (!known) return [];
    const all = [
      ...Object.entries(known.ssh).map(([id, k]) => ["ssh", id, k] as const),
      ...Object.entries(known.rdp).map(([id, k]) => ["rdp", id, k] as const),
      ...Object.entries(known.https ?? {}).map(([id, k]) => ["https", id, k] as const),
    ];
    if (!keyQuery.trim()) return all;
    return all
      .map((r) => [r, score(keyQuery, `${r[0]} ${r[1]} ${r[2]}`)] as const)
      .filter(([, sc]) => sc >= 0)
      .sort((a, b) => b[1] - a[1])
      .map(([r]) => r);
  });
  const keyPages = $derived(Math.max(1, Math.ceil(keyRows.length / PER_PAGE)));
  $effect(() => {
    if (keyPage >= keyPages) keyPage = keyPages - 1; // the last key of the last page was removed
  });
  /** Trusted keys: SSH hosts, RDP servers, BMC certificates. */
  const KEY_LOOK = { ssh: ["lock-alt", "var(--k-ssh)"], rdp: ["desktop", "var(--k-rdp)"], https: ["tv", "var(--k-kvm)"] };
  const short = (k: string) => {
    const [algo, b64] = k.includes(" ") ? k.split(" ") : ["SHA-256", k];
    return `${algo} …${b64.slice(-16)}`;
  };
</script>

{#snippet card(sc: Scheme)}
  <button class="scheme" class:selected={s.scheme === sc.name} style:background={css(sc.bg)} style:color={css(sc.fg)} onclick={() => (s.scheme = sc.name)}>
    <span>{sc.name}</span>
    <span class="swatches">
      {#each sc.ansi as c, i (i)}<span style:background={css(c)}></span>{/each}
    </span>
  </button>
{/snippet}

{#snippet bar()}
  <div class="pages" role="tablist" aria-orientation="vertical" bind:this={nav}>
    <input
      type="search"
      bind:value={query}
      placeholder={_("Search settings")}
      aria-label={_("Search settings")}
      {@attach (el) => el.focus()}
      onkeydown={(e) => {
        if (e.key === "Enter" && found.length) reveal(...found[0]);
        // Escape clears the search first; the next one closes the window.
        if (e.key === "Escape" && query) {
          e.stopPropagation();
          query = "";
        }
      }}
    />
    {#if query.trim()}
      {#each found as [id, text], i (i)}
        <button class="flat hit" class:first={i === 0} onclick={() => reveal(id, text)}><span class="what">{text}</span><span class="where">{pageName(id)}</span></button>
      {:else}
        <p class="weak none">{_("Nothing matches.")}</p>
      {/each}
    {:else}
      {#each PAGES as [id, icon, label] (id)}
        <button class="flat" role="tab" aria-selected={page === id} class:current={page === id} onclick={() => goTo(id)}><Icon name={icon} />{_(label)}</button>
      {/each}
    {/if}
  </div>
{/snippet}

<!-- Fixed height: switching pages doesn't make the window jump. -->
<Modal title={_("Settings")} width={987} height={610} {onclose} {bar}>
  {#if page === "general"}
    <h3>{_("Language")}</h3>
    <select bind:value={s.language}>
      <option value="">{_("System ({language})", { language: nativeName(pick("")) })}</option>
      {#each languages as [code, name] (code)}<option value={code}>{name}</option>{/each}
    </select>
    <p class="weak">{_("Messages from servers and connections stay in English.")}</p>
    <h3>{_("Tabs")}</h3>
    <div class="col">
      <label class="check"><input type="checkbox" bind:checked={s.confirm_close} />{_("Ask before closing a tab that is still connected")}</label>
      <label class="check"><input type="checkbox" bind:checked={s.keep_running} />{_("Keep running in the tray when the window is closed, so tabs and background tasks go on")}</label>
      <label class="check"><input type="checkbox" bind:checked={s.restore_tabs} />{_("Reopen the tabs that were open when xtermium closed")}</label>
      <label class="check"><input type="checkbox" bind:checked={s.status_bar} />{_("Show a status bar under each pane: connection, uptime, encryption and response time")}</label>
    </div>
    <p class="weak">{_("Reopened tabs start new connections; what ran in them before is gone.")}</p>
    <h3>{_("This computer")}</h3>
    <div class="col">
      <label class="check"><input type="checkbox" bind:checked={s.start_at_login} />{_("Start xtermium when you sign in")}</label>
      <label class="check"><input type="checkbox" bind:checked={s.notifications} />{_("Desktop notifications: a new version, a connection that dropped")}</label>
      <label class="check"><input type="checkbox" bind:checked={s.high_priority} />{_("Run above the normal process priority (may need administrator rights; turning it off takes a restart)")}</label>
    </div>
    <h3>{_("Sessions")}</h3>
    <div class="col">
      <label class="check"><input type="checkbox" bind:checked={s.single_click_open} />{_("Open a session with one click (instead of a double-click)")}</label>
      <label class="check"><input type="checkbox" bind:checked={s.remember_folders} />{_("Remember which folders are open in the sidebar")}</label>
      {#if explorerMenu !== null}
        <label class="check"><input type="checkbox" checked={explorerMenu} onchange={(e) => setExplorerMenu((e.target as HTMLInputElement).checked)} />{_("Add \"Open in xtermium\" to folders in File Explorer: a local shell in that folder")}</label>
      {/if}
    </div>
    {#if explorerMenu}<p class="weak">{_("On Windows 11 it is under Show more options. It starts this copy of xtermium.")}</p>{/if}
    {#if s.quiet_hints.length}
      <div><button onclick={() => (s.quiet_hints = [])}>{_("Show the tips I turned off again ({n})", { n: s.quiet_hints.length })}</button></div>
    {/if}
  {:else if page === "shortcuts"}
    <p class="weak">{_("Click a shortcut, then press the new keys. Backspace removes it, Escape keeps it as it was.")}</p>
    {#if hint}<p class="warn">{hint}</p>{/if}
    <div class="shortcuts">
      {#each ACTIONS as a (a.id)}
        {@const keys = keysOf(a.id, s.shortcuts)}
        {@const why = clash(a.id, keys)}
        <div class="row shortcut">
          <span class="grow">{_(a.label)}{#if why}<span class="warn note">{why}</span>{/if}</span>
          <button
            class="flat chord"
            class:on={recording === a.id}
            data-record
            onclick={(e) => ((e.currentTarget as HTMLElement).focus(), ([recording, hint] = [a.id, ""]))}
            onkeydown={(e) => recording === a.id && record(e, a.id)}
            onblur={() => recording === a.id && (recording = null)}
          >
            {#if recording === a.id}{_("Press keys…")}{:else if keys}<kbd>{keys}</kbd>{:else}<span class="weak">{_("none")}</span>{/if}
          </button>
          <button class="flat icon-only" title={_("Reset to {keys}", { keys: a.keys || _("none") })} disabled={!(a.id in s.shortcuts)} onclick={() => delete s.shortcuts[a.id]}><Icon name="refresh" /></button>
        </div>
      {/each}
    </div>
    <div class="foot"><button disabled={!Object.keys(s.shortcuts).length} onclick={() => (s.shortcuts = {})}>{_("Reset all shortcuts")}</button></div>
  {:else if page === "automation"}
    <label class="check"><input type="checkbox" checked={s.automation} onchange={(e) => setAutomation((e.target as HTMLInputElement).checked)} />{_("Let AI assistants and scripts control xtermium")}</label>
    <p class="warn">
      {#if s.automation_network}
        {_("Anything that has the token can open your sessions, type into your terminals and start tunnels, as you. On the network, any machine that can reach this one can try, and the token travels unencrypted: use it only on networks you trust, or through an SSH tunnel. Web pages are still refused.")}
      {:else}
        {_("Anything that has the token can open your sessions, type into your terminals and start tunnels, as you. xtermium only listens on this computer (127.0.0.1), and refuses web pages.")}
      {/if}
    </p>
    {#if s.automation}
      <div class="form">
        <span>{_("Listen on")}</span>
        <select bind:value={s.automation_network}>
          <option value={false}>{_("This computer only (127.0.0.1)")}</option>
          <option value={true}>{_("The network (0.0.0.0)")}</option>
        </select>
        {#if s.automation_network}
          <span></span>
          <p class="weak">{_("Assistants on other machines use this computer's address instead of 127.0.0.1. Your firewall may ask whether xtermium may accept connections.")}</p>
        {/if}
        <span>{_("Port")}</span>
        <input type="number" min="1024" max="65535" bind:value={s.automation_port} />
        <span>{_("Token")}</span>
        <div class="row">
          <input class="grow" style:min-width="0" type={showToken ? "text" : "password"} readonly value={s.automation_token} />
          <button class="flat" onclick={() => (showToken = !showToken)}>{showToken ? _("Hide") : _("Show")}</button>
          <button onclick={() => copy(s.automation_token)}>{_("Copy")}</button>
          <button onclick={() => (rotating = true)} title={_("The old token stops working")}>{_("New token")}</button>
        </div>
        {#if rotating}
          <span></span>
          <div class="row">
            <span class="weak grow">{_("Assistants and scripts using the current token stop working until you give them the new one.")}</span>
            <button class="primary danger" onclick={newToken}>{_("Replace the token")}</button>
            <button {@attach (el) => el.focus()} onclick={() => (rotating = false)}>{_("Cancel")}</button>
          </div>
        {/if}
        <span>{_("Status")}</span>
        {#if automation[1]}
          <span class="row error"><Icon name="x-circle" />{automation[1]}</span>
        {:else if automation[0]}
          <span class="row"><Icon name="check-circle" color="var(--ok)" />{_("Listening on {address}", { address: s.automation_network ? `https://0.0.0.0:${s.automation_port}/mcp` : `http://127.0.0.1:${s.automation_port}/mcp` })}</span>
        {:else}
          <span class="row weak"><Icon name="loader-alt" spin />{_("Starting…")}</span>
        {/if}
      </div>
      <h3>{_("AI assistants (MCP)")}</h3>
      {#if s.automation_network}
        <p class="weak">
          {_("Run these on the machine the assistant runs on (in a macOS or Linux shell). They reach this computer over HTTPS; its certificate is self-signed, so they start the assistant with certificate checks off.")}
        </p>
        <div class="form">
          <span>{_("Address")}</span>
          <input list="mcp-addresses" bind:value={netHost} placeholder="192.0.2.10" aria-label={_("Address")} />
          <datalist id="mcp-addresses">{#each net?.addresses ?? [] as a (a)}<option value={a}></option>{/each}</datalist>
          <span>{_("Certificate (SHA-256)")}</span>
          <code class="fingerprint" title={net?.fingerprint}>{net?.fingerprint ?? "…"}</code>
        </div>
      {:else}
        <p class="weak">{_("Add xtermium to an assistant by running its command in a terminal:")}</p>
      {/if}
      <div class="commands">
        {#each mcpCommands(showToken ? s.automation_token : "•••") as [who, command], i (who)}
          <span>{who}</span>
          <code class="snippet one" title={command}>{command}</code>
          <button onclick={() => (copy(mcpCommands(s.automation_token)[i][1]), (codexHint = who))}>{_("Copy")}</button>
          <button title={_("Run it in a local shell now")} onclick={() => (onrun(mcpCommands(s.automation_token)[i][1]), onclose())}>{_("Run")}</button>
        {/each}
      </div>
      {#if s.automation_network && (codexHint === "Codex" || codexHint === "VS Code")}
        <p class="row warn">
          <Icon name="info-circle" />
          <span>
            {_("{who} checks certificates and can't be told to skip it, so it may refuse xtermium's self-signed one. If it does, forward the port over SSH from that machine (ssh -L {port}:127.0.0.1:{port} this computer) and use http://127.0.0.1:{port}/mcp there.", { who: codexHint, port: s.automation_port })}
          </span>
        </p>
      {/if}
      {#if codexHint === "Codex"}
        <p class="row warn">
          <Icon name="info-circle" />
          <span>
            {@html app.os === "windows"
              ? _("Codex reads the token from an environment variable, so this command also saves it as <code>XTERMIUM_TOKEN</code> in your user environment (setx). Open a new terminal before starting Codex, and run it again if you make a new token.")
              : _("Codex reads the token from an environment variable, so this command also saves it as <code>XTERMIUM_TOKEN</code> in ~/.profile. Open a new terminal before starting Codex, and run it again if you make a new token.")}
          </span>
        </p>
      {/if}
      <p class="weak">{_("Other assistants that take MCP servers over HTTP mostly read a configuration like this one:")}</p>
      <pre class="snippet">{mcpConfig(showToken ? s.automation_token : "•••")}</pre>
      <div><button onclick={() => copy(mcpConfig(s.automation_token))}>{_("Copy configuration")}</button></div>
      <p class="weak">
        {_("Terminals: sessions, tabs, open (a saved session, a local shell or an address), run (wait for the output with a pattern), read, wait_for, type, key, click, scroll, close. Remote desktops (RDP, VNC): screenshot, click, move, drag, scroll, type, key. Tunnels: tunnels, tunnel_start, tunnel_stop.")}
        {_("Websites: open (a url), web_read (the page and its elements), web_click, web_fill, web_go, web_eval (run a script in the page), screenshot (Windows), close.")}
        {_("Workspaces: workspaces, workspace_switch, workspace_clone, workspace_sync, move_to_workspace. Logins: logins (never the passwords). Saved layouts: layouts, layout_open. Tabs in their own window: detach, attach. Restore points: restore_points, restore_point_take.")}
      </p>
      <h3>{_("Command line")}</h3>
      <p class="weak">{@html _("The same tools from scripts, with JSON output (<code>xtermium help</code> lists them all):")}</p>
      <pre class="snippet">{`xtermium open --shell                    # a terminal of your own; prints the tab id
xtermium run 3 "make test" --until "PASS|FAIL"
xtermium key 3 ctrl+c
xtermium open "Prod/win-app"             # an RDP session
xtermium screenshot 4 --out app.png
xtermium click 4 640 360 --double
xtermium close 3`}</pre>
    {/if}
  {:else if page === "sync"}
    <h3>{_("Account")}</h3>
    {#if s.sync_token}
      <p class="row">
        <Icon name="check-circle" color="var(--ok)" /><span class="grow">{_("Signed in as {who}. Sync is on.", { who: s.sync_email || _("your account") })}</span>
        <button onclick={() => signOut()}><Icon name="log-out" />{_("Sign out")}</button>
      </p>
      <p class="weak">{_("Signing out turns sync off. This device keeps its sessions.")}</p>
    {:else}
      <p>{_("Sync is off. Sign in with Google, GitHub or Discord to sync your sessions, folders, logins, tunnels, snippets, layouts and trusted host keys across your devices; without an account, everything stays on this device and nothing asks for one.")}</p>
      <div class="row">
        <button class="primary" onclick={() => (app.signIn = true)}><Icon name="log-in" />{_("Sign in")}</button>
      </div>
    {/if}
    <label class="check" title={s.sync_token ? _("Always shown while signed in") : ""}>
      <input type="checkbox" checked={s.account_button || !!s.sync_token} disabled={!!s.sync_token} onchange={(e) => (s.account_button = (e.target as HTMLInputElement).checked)} />{_("Show the account button in the sidebar")}
    </label>
    <p class="weak">
      {_("Your data is encrypted on this computer before it leaves it, so the sync server stores data it can't read. Settings stay on each device. Syncing happens when xtermium starts and when you click Sync now; if both sides changed, you choose which copy to keep.")}
    </p>
    {#if s.sync_token}
      <div class="form">
        <span>{_("Status")}</span>
        {#if app.sync.busy}
          <span class="row weak"><Icon name="loader-alt" spin />{_("Syncing…")}</span>
        {:else if app.sync.error}
          <span class="row error"><Icon name="x-circle" />{app.sync.error}</span>
        {:else if app.sync.last}
          <span class="row"><Icon name="check-circle" color="var(--ok)" />{_("Synced at {time}", { time: app.sync.last.toLocaleTimeString() })}</span>
        {:else}
          <span class="weak">{_("Not synced yet.")}</span>
        {/if}
        {#if app.sync.quota}
          <span>{_("Storage")}</span>
          <Quota />
        {/if}
      </div>
      {#if !app.recovery && !app.sync.join}
        <p class="row warn"><Icon name="error" /><button class="link quiet" onclick={() => (page = "security")}>{_("Set a recovery passphrase first (Security): your other devices join with it.")}</button></p>
      {/if}
      {#if app.sync.join}
        <h3>{_("Join your account's data")}</h3>
        <p>
          {_("This account already holds data from another device. Enter the recovery passphrase set there; this device's sessions are added to the account's.")}
        </p>
        <div class="row">
          <input class="grow" type="password" bind:value={join.pass} autocomplete="off" onkeydown={(e) => e.key === "Enter" && join.pass && joinAccount()} />
          <button class="primary" disabled={!join.pass || join.busy} onclick={joinAccount}>{#if join.busy}<Icon name="loader-alt" spin />{/if}{_("Join")}</button>
        </div>
        {#if join.error}<p class="error">{join.error}</p>{/if}
      {/if}
      <div class="row">
        <button disabled={app.sync.busy} onclick={() => saved().then(() => syncNow())}>{_("Sync now")}</button>
        <button onclick={deleteCloud}>{confirmDelete ? _("Click again to delete it") : _("Delete the cloud copy")}</button>
      </div>
    {/if}
  {:else if page === "appearance"}
    <div class="form">
      <span>{_("Theme")}</span>
      <div class="row">
        {#each [N_("System"), N_("Dark"), N_("Light")] as const as theme (theme)}
          <button class:selected={s.theme === theme} aria-pressed={s.theme === theme} onclick={() => (s.theme = theme)}>{_(theme)}</button>
        {/each}
      </div>
      <span>{_("Interface scale")}</span>
      <div class="row"><input type="range" min="0.8" max="1.6" step="0.05" bind:value={s.ui_scale} />{s.ui_scale.toFixed(2)}×</div>
      <span>{_("Window effect")}</span>
      <div class="row wrap">
        {#each app.effects as e (e)}
          <button class:selected={s.effect === e} aria-pressed={s.effect === e} onclick={() => (s.effect = e)}>{_(EFFECTS[e] ?? e)}</button>
        {/each}
      </div>
      <span class:weak={!transparent}>{_("Window opacity")}</span>
      <div class="row"><input type="range" min="0.3" max="1" step="0.01" disabled={!transparent} bind:value={s.window_opacity} />{Math.round(s.window_opacity * 100)}%</div>
      <span class:weak={!transparent}>{_("Terminal opacity")}</span>
      <div class="row"><input type="range" min="0.3" max="1" step="0.01" disabled={!transparent} bind:value={s.terminal_opacity} />{Math.round(s.terminal_opacity * 100)}%</div>
      <span>{_("Accent")}</span>
      <div class="row wrap">
        <!-- each swatch shows the value the theme in use gets -->
        <button class="swatch" class:selected={!s.accent} title={_("Gold (default)")} aria-label={_("Default accent")} style:--c="light-dark(#85590a, #d8a84e)" onclick={() => (s.accent = "")}></button>
        {#each ACCENTS as [name, dark, light] (dark)}
          <button class="swatch" class:selected={s.accent === dark} title={_(name)} aria-label={_("Accent {c}", { c: _(name) })} style:--c="light-dark({light}, {dark})" onclick={() => (s.accent = dark)}></button>
        {/each}
        <input type="color" title={_("Any colour")} value={s.accent || "#d8a84e"} oninput={(e) => (s.accent = (e.target as HTMLInputElement).value)} />
      </div>
    </div>
    <h3>{_("Terminal colours")}</h3>
    <div><button class:selected={s.scheme === "Auto"} onclick={() => (s.scheme = "Auto")}>{_("Auto (follows theme)")}</button></div>
    <div class="schemes">
      {#each app.schemes as sc (sc.name)}{@render card(sc)}{/each}
      {#each s.custom_schemes as sc (sc.name)}
        <div class="custom-scheme">
          {@render card(sc)}
          <button class="flat icon-only remove" title={_("Remove {name}", { name: sc.name })} aria-label={_("Remove {name}", { name: sc.name })} onclick={() => removeScheme(sc.name)}><Icon name="trash" size={14} /></button>
        </div>
      {/each}
    </div>
    <p class="weak">
      {_("Import a scheme from a link to its JSON, or paste the JSON: a terminal.sexy export or a Windows Terminal scheme.")}
      <button class="link" onclick={() => openUrl("https://terminal.sexy/")}>terminal.sexy</button>
    </p>
    <div class="row">
      <input class="grow" bind:value={schemeSource} placeholder={_("https://… .json, or the JSON itself")} onkeydown={(e) => e.key === "Enter" && schemeSource.trim() && importScheme()} />
      <button disabled={!schemeSource.trim() || schemeBusy} onclick={importScheme}>{_("Import")}</button>
      <button disabled={schemeBusy} onclick={() => (catalogue ? (catalogue = null) : openCatalogue())}>
        {#if schemeBusy}<Icon name="loader-alt" spin />{/if}{catalogue ? _("Close catalogue") : _("Browse terminal.sexy")}
      </button>
    </div>
    {#if schemeError}<p class="error">{schemeError}</p>{/if}
    {#if catalogue}
      <input
        type="search"
        placeholder={_("Search {n} schemes", { n: catalogue.length })}
        bind:value={catalogueQuery}
        onkeydown={(e) => e.key === "Enter" && catalogueHits.length && !schemeBusy && addFromCatalogue(catalogueHits[0])}
      />
      <div class="catalogue">
        {#each catalogueHits as p (p)}
          <button class="flat" disabled={schemeBusy} onclick={() => addFromCatalogue(p)}>
            <Icon name="download" size={14} />{catalogueName(p)}
          </button>
        {/each}
      </div>
    {/if}
    <h3>{_("Backgrounds")}</h3>
    <div class="form">
      <BackgroundPicker place="sidebar" label={_("Sidebar")} />
      <BackgroundPicker place="bar" label={_("Tab bar")} />
    </div>
    <p class="weak">
      {_("A picture or an animated shader behind the sidebar or the tab bar, faded into their colour. For terminals, see")}
      <button class="link" onclick={() => (page = "terminal")}>{_("Settings › Terminal")}</button>
    </p>
    <h3>{_("Focused pane")}</h3>
    <p class="weak">{_("The ring around the pane you type in, when the focus moves to another pane.")}</p>
    <div class="row" role="group" aria-label={_("Focused pane")}>
      <button class:selected={s.focus_animated} aria-pressed={s.focus_animated} onclick={() => (s.focus_animated = true)} title={_("The ring glides to the next pane")}>{_("Animated")}</button>
      <button class:selected={!s.focus_animated} aria-pressed={!s.focus_animated} onclick={() => (s.focus_animated = false)} title={_("The ring jumps straight to the next pane")}>{_("Static")}</button>
    </div>
    <h3>{_("Broadcast outline")}</h3>
    <p class="weak">{_("Drawn around every terminal that gets what you type while broadcast is on.")}</p>
    <div class="form">
      <span>{_("Style")}</span>
      <div class="row wrap">
        {#each OUTLINES as [id, name] (id)}
          <button class:selected={s.broadcast_outline === id} onclick={() => (s.broadcast_outline = id)}>{_(name)}</button>
        {/each}
      </div>
      <span class:weak={s.broadcast_outline === "rainbow"}>{_("Colour")}</span>
      <div class="row wrap">
        {#each ["#9aa3ad", ...COLORS] as c (c)}
          <button class="swatch" class:selected={s.broadcast_outline_color === c} title={c} aria-label={_("Colour {c}", { c })} style:--c={c} onclick={() => (s.broadcast_outline_color = c)}></button>
        {/each}
        <input type="color" title={_("Any colour")} value={s.broadcast_outline_color} oninput={(e) => (s.broadcast_outline_color = (e.target as HTMLInputElement).value)} />
      </div>
      <span>{_("Preview")}</span>
      <div class="outline preview" data-outline={s.broadcast_outline} style:--oc={s.broadcast_outline_color}>$ uptime</div>
    </div>
    <div class="foot"><button onclick={reset}>{_("Reset appearance and terminal settings")}</button></div>
  {:else if page === "terminal"}
    <p class="weak"><button class="link quiet" onclick={() => (page = "appearance")}>{_("Terminal colours are under Appearance.")}</button></p>
    <h3>{_("Font and cursor")}</h3>
    <div class="form">
      <span>{_("Family")}</span>
      <input list="mono-fonts" bind:value={s.font} placeholder={_("Built-in monospace")} />
      <datalist id="mono-fonts">
        {#each fonts as f (f)}<option value={f}></option>{/each}
      </datalist>
      <span>{_("Size")}</span>
      <div class="row"><input type="range" min="8" max="32" step="0.5" bind:value={s.font_size} />{s.font_size}</div>
      <span></span>
      <span class="weak">
        {_("For every terminal. To make one tab's text bigger or smaller, use the font shortcuts in it:")}
        <button class="link" onclick={() => (page = "shortcuts")}>{_("Settings › Shortcuts")}</button>
      </span>
      <span>{_("Cursor")}</span>
      <div class="row">
        {#each [N_("Block"), N_("Bar"), N_("Underline")] as CursorShape[] as c (c)}
          <button class:selected={s.cursor === c} onclick={() => (s.cursor = c)}>{_(c)}</button>
        {/each}
        <label class="check"><input type="checkbox" bind:checked={s.cursor_blink} />{_("Blink")}</label>
      </div>
      <span>{_("Line height")}</span>
      <div class="row"><input type="range" min="1" max="2" step="0.05" bind:value={s.line_height} />{s.line_height.toFixed(2)}×</div>
      <span>{_("Pane padding")}</span>
      <div class="row"><input type="range" min="0" max="32" step="1" bind:value={s.pane_padding} />{_("{n} px", { n: s.pane_padding })}</div>
      <span>{_("Apply to")}</span>
      <label class="check"><input type="checkbox" bind:checked={s.override_sessions} />{_("all sessions (ignore per-session colours and fonts)")}</label>
    </div>
    <h3>{_("Behaviour")}</h3>
    <div class="form">
      <span>{_("Scrollback")}</span>
      <div class="row"><input type="number" min="500" max="200000" step="500" bind:value={s.scrollback} /> {_("lines")}</div>
      <span></span>
      <label class="row">
        {_("Making a pane bigger or smaller with the keyboard changes it by")}
        <input type="number" min="1" max="500" step="1" bind:value={s.pane_step} aria-label={_("Pixels a pane grows or shrinks by")} />{_("pixels")}
      </label>
      <span></span>
      <label class="check"><input type="checkbox" bind:checked={s.scrollbar} />{_("Show a scroll bar")}</label>
      <span></span>
      <label class="check" title={_("When off, new output doesn't move the text you've scrolled back to")}>
        <input type="checkbox" bind:checked={s.scroll_on_output} />{_("Scroll to the bottom on output")}
      </label>
      <span>{_("Clipboard")}</span>
      <div class="col">
        <label class="check"><input type="checkbox" bind:checked={s.copy_on_select} />{_("Copy on select")}</label>
        <label class="check"><input type="checkbox" bind:checked={s.right_click_paste} />{_("Right-click pastes")}</label>
        <label class="check"><input type="checkbox" bind:checked={s.strip_paste} />{_("Strip whitespace from the start and end of what you paste")}</label>
      </div>
    </div>
    <h3>{_("Background")}</h3>
    <div class="form"><BackgroundPicker place="terminal" label={_("Behind the text")} /></div>
    <p class="weak">
      {_("A picture (PNG, JPEG, GIF, WebP or SVG) or an animated GLSL shader, such as one from Shadertoy. It shows behind the text in terminal tabs, faded into the terminal's colour by the strength you set.")}
    </p>
    <h3>{_("Highlight rules")}</h3>
    <label class="check"><input type="checkbox" bind:checked={s.highlight} />{_("Colour words and addresses in terminal output")}</label>
    <p class="weak">
      {@html _("Words that match a rule get its colour, unless the program already coloured them. Rules are regular expressions; start one with <code>(?i)</code> to ignore case. When several match, the first one wins.")}
    </p>
    {#if s.highlight}
      <div class="rules">
        {#each s.highlights as h, i (i)}
          <div class="row rule">
            <input type="color" bind:value={h.color} title={_("Colour")} aria-label={_("Colour")} />
            <input class="grow mono" bind:value={h.pattern} spellcheck="false" aria-label={_("Pattern")} onchange={() => checkRule(i)} style:color={h.color} />
            <button class="flat icon-only" title={_("Remove")} aria-label={_("Remove")} onclick={() => (s.highlights.splice(i, 1), delete ruleErrors[i])}><Icon name="trash" /></button>
          </div>
          {#if ruleErrors[i]}<p class="error rule-error">{ruleErrors[i]}</p>{/if}
        {/each}
      </div>
      <div class="row">
        <button onclick={() => s.highlights.push({ pattern: "", color: "#e6c472" })}><Icon name="plus" />{_("Add a rule")}</button>
        <button class="flat" onclick={resetRules}>{_("Back to the default rules")}</button>
      </div>
    {/if}
    <h3>{_("Logging")}</h3>
    <label class="check"><input type="checkbox" bind:checked={s.log_to_file} />{_("Save what each terminal tab shows to a text file")}</label>
    <p class="weak">
      {_("One plain-text file per tab, named after the session and the time it opened. Colours are left out, and full-screen programs (editors, top) aren't saved. Applies to tabs you open from now on.")}
    </p>
    {#if s.log_to_file}
      <p class="row warn">
        <Icon name="error" />{_("Every line a terminal shows is also written to disk. With huge outputs (a flood of logs, a giant file) this can slow xtermium down.")}
      </p>
    {/if}
    <div class="row">
      <code class="weak grow" title={logsFolder}>{logsFolder}</code>
      <button onclick={() => invoke("open_logs").catch((e) => (app.status = String(e)))}><Icon name="folder-open" />{_("Open folder")}</button>
    </div>
  {:else if page === "workspaces"}
    <p class="weak">
      {_("Each workspace keeps its own sessions and folders. Switch between them at the top of the sidebar; open tabs stay open. Everything else is the same in all of them: shared and saved logins, tunnels, snippets, layouts, trusted keys and settings.")}
    </p>
    <div class="keys">
      {#each ["", ...app.workspaces] as w (w)}
        <div class="row key">
          <Icon name="cluster" color={w === app.workspace ? "var(--gold)" : ""} />
          {#if ws.renaming === w}
            <input
              class="grow"
              bind:value={ws.to}
              maxlength="24"
              aria-label={_("New name")}
              {@attach (el) => (el.focus(), el.select())}
              onkeydown={(e) => {
                if (e.key === "Enter") wsDo({ op: "rename", name: w, to: ws.to });
                if (e.key === "Escape") (e.stopPropagation(), (ws.renaming = null));
              }}
            />
            <button onclick={() => wsDo({ op: "rename", name: w, to: ws.to })}>{_("Rename")}</button>
            <button class="flat" onclick={() => (ws.renaming = null)}>{_("Cancel")}</button>
          {:else}
            <span class="grow">{w || _("Default workspace")}</span>
            {#if w === app.workspace}
              <span class="weak">{_("in use")}</span>
            {:else}
              <button class="flat" onclick={() => wsDo({ op: "switch", name: w })}>{_("Switch to it")}</button>
            {/if}
            <button
              class="flat icon-only"
              title={_("Clone: a new workspace with copies of this one's sessions")}
              aria-label={_("Clone {name}", { name: wsName(w) })}
              onclick={() => wsDo({ op: "clone", name: w }, _("Cloned {name}.", { name: wsName(w) }))}><Icon name="copy" /></button
            >
            {#if !w}<span class="slot"></span><span class="slot"></span>{/if}
            {#if w}
              <button class="flat icon-only" title={_("Rename")} aria-label={_("Rename {name}", { name: w })} onclick={() => Object.assign(ws, { renaming: w, to: w })}><Icon name="edit" /></button>
              <button
                class="flat"
                class:icon-only={ws.deleting !== w}
                title={_("Delete it with its sessions")}
                aria-label={_("Delete {name}", { name: w })}
                onclick={() => (ws.deleting === w ? wsDo({ op: "delete", name: w }) : (ws.deleting = w))}
              >
                {#if ws.deleting === w}{_("Click again to delete it")}{:else}<Icon name="trash" />{/if}
              </button>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
    <div class="row">
      <input class="grow" bind:value={ws.name} maxlength="24" placeholder={_("New workspace")} aria-label={_("New workspace")} onkeydown={(e) => e.key === "Enter" && ws.name.trim() && wsDo({ op: "add", name: ws.name })} />
      <button disabled={!ws.name.trim()} onclick={() => wsDo({ op: "add", name: ws.name })}><Icon name="plus" />{_("Add")}</button>
    </div>
    {#if ws.error}<p class="error">{ws.error}</p>{:else if ws.done}<p class="ok">{ws.done}</p>{/if}
    <p class="weak">{_("Deleting a workspace deletes its sessions and folders. The default workspace can't be deleted.")}</p>
    {#if app.workspaces.length}
      <h3>{_("Sync")}</h3>
      <p class="weak">{_("Copies the sessions and folders of one workspace into another. Matching sessions are updated, new ones are added, and nothing is deleted. A restore point is saved first.")}</p>
      <div class="form">
        <span>{_("From")}</span>
        <select bind:value={wsSync.from}>{#each ["", ...app.workspaces] as w (w)}<option value={w}>{wsName(w)}</option>{/each}</select>
        <span>{_("Into")}</span>
        <select bind:value={() => syncTo, (v) => (wsSync.to = v)}>
          <option value={null} disabled>{_("Pick a workspace")}</option>
          {#each ["", ...app.workspaces].filter((w) => w !== wsSync.from) as w (w)}<option value={w}>{wsName(w)}</option>{/each}
        </select>
      </div>
      <div class="row">
        <button class="primary" disabled={syncTo === null} onclick={runSync}><Icon name="refresh" />{_("Sync")}</button>
        {#if syncTo === null}<span class="weak">{_("Pick the workspace to sync into.")}</span>{/if}
      </div>
    {/if}
    <p class="row weak">
      {_("For every workspace:")}
      <button class="link" onclick={() => (app.showCredentials = true)}>{_("Shared logins")}</button> ·
      <button class="link" onclick={() => (page = "logins")}>{_("Saved logins")}</button>
    </p>
  {:else if page === "logins"}
    <p class="weak">
      {_("Passwords you asked xtermium to remember when it asked for them, for every workspace. They stay on this device, encrypted with the rest of your data. A password that stops working is forgotten by itself.")}
    </p>
    {#if !logins}
      <p class="row weak"><Icon name="loader-alt" spin />{_("Loading…")}</p>
    {:else if !logins.length}
      <p class="weak">{_("None yet.")}</p>
    {:else}
      <div class="keys">
        {#each logins as r (r.id)}
          {@const [icon, color, kind, target] = loginLook(r.id)}
          <div class="row key">
            <Icon name={icon} {color} />
            <span class="grow" title={r.id}><span class="weak">{kind}</span> {target}</span>
            <input type={r.show ? "text" : "password"} bind:value={r.edit} autocomplete="off" aria-label={_("Password for {who}", { who: target })} onkeydown={(e) => e.key === "Enter" && r.edit && setLogin(r, r.edit)} />
            <button class="flat icon-only" title={r.show ? _("Hide") : _("Show")} aria-label={r.show ? _("Hide") : _("Show")} onclick={() => (r.show = !r.show)}><Icon name={r.show ? "hide" : "show"} /></button>
            {#if r.edit !== r.password}<button disabled={!r.edit} onclick={() => setLogin(r, r.edit)}>{_("Save")}</button>{/if}
            <button class="flat icon-only" title={_("Forget")} aria-label={_("Forget the password for {who}", { who: target })} onclick={() => setLogin(r, null)}><Icon name="trash" /></button>
          </div>
        {/each}
      </div>
    {/if}
    <p class="weak">
      {_("One password for many sessions goes in a shared login:")}
      <button class="link" onclick={() => (app.showCredentials = true)}>{_("Shared logins")}</button>
    </p>
  {:else if page === "keys"}
    <p class="weak">
      {_("Key pairs in {dir}, where SSH sessions look for keys. Add a public key to a server's ~/.ssh/authorized_keys to log in there with its private key.", { dir: sshKeys?.dir ?? "~/.ssh" })}
    </p>
    {#if !sshKeys}
      <p class="row weak"><Icon name="loader-alt" spin />{_("Loading…")}</p>
    {:else if !sshKeys.keys.length}
      <p class="weak">{_("None yet.")}</p>
    {:else}
      <div class="keys">
        {#each sshKeys.keys as k (k.name)}
          <div class="key ssh-key">
            <div class="row">
              <Icon name="key" />
              <span class="grow">{k.name} <span class="weak">· {k.kind}{k.comment ? ` · ${k.comment}` : ""}{k.encrypted ? ` · ${_("passphrase")}` : ""}</span></span>
              <button class="flat icon-only" title={_("Copy the public key")} aria-label={_("Copy the public key of {name}", { name: k.name })} onclick={() => copyText(k.public)}><Icon name="copy" /></button>
              <button class="flat" class:icon-only={keyDeleting !== k.name} title={_("Delete the key pair")} aria-label={_("Delete {name}", { name: k.name })} onclick={() => deleteKey(k.name)}>
                {#if keyDeleting === k.name}{_("Click again to delete it")}{:else}<Icon name="trash" />{/if}
              </button>
            </div>
            <pre class="public" aria-label={_("Public key")}>{k.public}</pre>
            <span class="weak fingerprint">{k.fingerprint}</span>
          </div>
        {/each}
      </div>
    {/if}
    <h3>{_("New key")}</h3>
    <div class="form">
      <span>{_("Type")}</span>
      <select bind:value={newKey.kind}>
        <option value="ed25519">{_("Ed25519 (recommended)")}</option>
        <option value="ecdsa">ECDSA P-256</option>
        <option value="rsa">RSA 4096</option>
      </select>
      <span>{_("Name")}</span>
      <input bind:value={newKey.name} placeholder={freeName(newKey.kind)} maxlength="64" />
      <span>{_("Comment")}</span>
      <input bind:value={newKey.comment} placeholder={sshKeys?.comment ?? ""} />
      <span>{_("Passphrase")}</span>
      <div class="row">
        <input class="grow" type={newKey.show ? "text" : "password"} bind:value={newKey.pass} placeholder={_("none")} autocomplete="new-password" onkeydown={(e) => e.key === "Enter" && !newKey.busy && createKey()} />
        <button class="flat icon-only" title={newKey.show ? _("Hide") : _("Show")} aria-label={newKey.show ? _("Hide passphrase") : _("Show passphrase")} onclick={() => (newKey.show = !newKey.show)}
          ><Icon name={newKey.show ? "hide" : "show"} /></button
        >
      </div>
    </div>
    <p class="weak">{_("A passphrase keeps the key safe if the file is copied; xtermium asks for it when the key is used.")}</p>
    {#if newKey.error}<p class="error">{newKey.error}</p>{/if}
    <div class="row">
      <button class="primary" disabled={newKey.busy} onclick={createKey}>{#if newKey.busy}<Icon name="loader-alt" spin />{:else}<Icon name="plus" />{/if}{_("Create key")}</button>
    </div>
  {:else if page === "browser"}
    <div class="form">
      <span>{_("Open Browser sessions")}</span>
      <div class="col">
        <label class="check"><input type="radio" bind:group={s.web} value="External" />{_("External: in your default web browser")}</label>
        <label class="check"><input type="radio" bind:group={s.web} value="Embedded" />{_("Embedded: inside an xtermium tab")}</label>
        <span class="weak">
          {_("Tabs use your system's browser engine ({engine}). Each website session keeps its own cookies and sign-ins, even after a restart; BMC consoles keep nothing once closed. xtermium keeps open pages from timing out, and websites can't reach xtermium itself.", {
            engine: app.os === "windows" ? "Edge WebView2" : app.os === "macos" ? "WebKit" : "WebKitGTK",
          })}
        </span>
      </div>
    </div>
  {:else if page === "defaults"}
    <h3>{_("Local shell")}</h3>
    <div class="form">
      <span>{_("Program")}</span>
      <PathInput bind:value={s.shell} exec placeholder={_("System default ({shell})", { shell: app.os === "windows" ? "cmd.exe" : "$SHELL" })} />
    </div>
    <p class="weak">{@html _("Used by Local shell, and by shell sessions that don't name a program. Arguments are allowed, e.g. <code>wsl.exe -d Debian</code>.")}</p>
    <h3>{_("SSH")}</h3>
    <p class="weak">{_("Used when a session leaves these empty.")}</p>
    <div class="form">
      <span>{_("User")}</span>
      <input bind:value={s.ssh_user} placeholder={_("ask each time")} />
      <span>{_("Private key")}</span>
      <PathInput bind:value={s.ssh_key} start="~/.ssh/" placeholder={_("path, e.g. ~/.ssh/id_ed25519")} />
      <span>{_("Password")}</span>
      <div class="row">
        <input class="grow" type={showPassword ? "text" : "password"} bind:value={s.ssh_password} placeholder={_("ask each time")} autocomplete="off" />
        <button class="flat" onclick={() => (showPassword = !showPassword)}>{showPassword ? _("Hide") : _("Show")}</button>
      </div>
    </div>
    <p class="weak">
      {_("The password is tried after keys and before asking, and is stored encrypted with your other settings.")}
      {_("Passwords you chose to remember for one host are in")}
      <button class="link" onclick={() => (page = "logins")}>{_("Settings › Saved logins")}</button>
    </p>
    <h3>{_("SSH connections")}</h3>
    <p class="weak">{_("For SSH sessions that don't have their own, and for tunnels.")}</p>
    <div class="form">
      <span>{_("Ciphers")}</span>
      <Ciphers bind:value={s.ssh_ciphers} />
      <span>{_("Keepalive")}</span>
      <div class="row"><input type="number" min="0" max="3600" bind:value={s.ssh_keepalive} />{_("seconds; 0 turns it off")}</div>
      <span>{_("Compression")}</span>
      <label class="check"><input type="checkbox" bind:checked={s.ssh_compression} />{_("Squeezes the traffic: helps on slow links, costs CPU on fast ones")}</label>
    </div>
    <p class="weak">{_("How often xtermium checks that an SSH connection is still up; after three missed checks it counts as lost. Applies to new connections.")}</p>
    <label class="check"><input type="checkbox" bind:checked={s.auto_reconnect} />{_("Reconnect by itself when a connection drops (up to 8 tries, waiting longer each time)")}</label>
    <h3>{_("Remote desktop")}</h3>
    <div class="form">
      <span>{_("Picture")}</span>
      <QualityBar bind:value={s.rdp_quality} />
    </div>
    <p class="weak">{_("For RDP sessions that don't have their own setting. Applies the next time a session connects.")}</p>
    <label class="check"><input type="checkbox" bind:checked={s.smb_auto_login} />{_("The file panel opens the host's shared folders (SMB) with the desktop's login, without asking")}</label>
  {:else if page === "backup"}
    <h3>{_("Sessions")}</h3>
    <p class="weak">{_("Bring sessions in from MobaXterm, an OpenSSH config or another xtermium, or send them out in those formats. Sessions you already have are kept as they are.")}</p>
    <div class="row">
      <button onclick={() => (showImport = true)}><Icon name="import" />{_("Import sessions…")}</button>
      <button onclick={() => (showExport = true)}><Icon name="upload" />{_("Export sessions…")}</button>
    </div>
    <h3>{_("Restore points")}</h3>
    <p class="weak">
      {_("Backups of everything (sessions, shared and saved logins, tunnels, snippets, settings and trusted server keys) kept on this computer: one a day, one before each restore or sync, and any you make. The newest 10 are kept.")}
    </p>
    {#if points?.length}
      <div class="keys">
        {#each points as p (p.id)}
          <div class="row key">
            <Icon name="backup" />
            <span class="grow">
              {new Date(p.at).toLocaleString()}
              <span class="weak">
                · {_(REASONS[p.reason])} ·
                {p.workspaces ? _("{n} sessions in {w} workspaces", { n: p.sessions, w: p.workspaces + 1 }) : _("{n} sessions", { n: p.sessions })}
              </span>
            </span>
            {#if restoring?.id === p.id}
              {#if app.settings?.sync}<label class="check"><input type="checkbox" bind:checked={restoring.push} />{_("then replace the cloud copy with it")}</label>{/if}
              <button class="primary danger" disabled={backup.busy} onclick={() => restorePoint(p.id, restoring!.push)}>{#if backup.busy}<Icon name="loader-alt" spin />{/if}{_("Replace everything")}</button>
              <button class="flat" {@attach (el) => el.focus()} onclick={() => (restoring = null)}>{_("Cancel")}</button>
            {:else}
              <button class="flat" disabled={backup.busy} onclick={() => (restoring = { id: p.id, push: !!app.settings?.sync })}>{_("Restore")}</button>
              <button class="flat icon-only" title={_("Delete")} aria-label={_("Delete the restore point of {time}", { time: new Date(p.at).toLocaleString() })} onclick={() => pointsDo("restore_point_delete", { id: p.id })}>
                <Icon name="trash" />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {:else if points}
      <p class="weak">{_("None yet.")}</p>
    {/if}
    <div class="row"><button disabled={backup.busy} onclick={() => saved().then(() => pointsDo("restore_point_take"))}><Icon name="plus" />{_("Take a restore point")}</button></div>
    {#if restoring}<p class="warn">{_("Restoring replaces everything here with the restore point. A restore point of the current state is saved first. This device's account sign-in and automation token stay.")}</p>{/if}
    <h3>{_("Back up everything")}</h3>
    <p class="weak">
      {_("One file with all of xtermium's configuration: sessions, folders, shared logins with their passwords, tunnels, snippets, settings and trusted server keys, encrypted with this passphrase.")}
    </p>
    <div class="form">
      <span>{_("Passphrase")}</span>
      <input type="password" bind:value={backup.pass} autocomplete="new-password" />
      <span>{_("Repeat it")}</span>
      <input type="password" bind:value={backup.again} autocomplete="new-password" onkeydown={(e) => e.key === "Enter" && backupOk && backupSave()} />
    </div>
    {#if backup.pass && backup.pass.length < 8}<p class="weak">{_("Use at least 8 characters.")}</p>{:else if backup.again && backup.pass !== backup.again}<p class="weak">{_("The passphrases differ.")}</p>{/if}
    <div class="row"><button disabled={!backupOk || backup.busy} onclick={backupSave}><Icon name="download" />{_("Back up everything…")}</button></div>
    <h3>{_("Restore everything")}</h3>
    <p class="warn">{_("Replaces everything here with the backup. This device's account sign-in and automation token stay.")}</p>
    <input type="file" accept=".xtermium" disabled={backup.busy} onchange={(e) => (backup.file = (e.target as HTMLInputElement).files?.[0] ?? null)} />
    {#if backup.file}
      <div class="form">
        <span>{_("Passphrase for {name}", { name: backup.file.name })}</span>
        <!-- no Enter here: replacing everything is only ever a click on its button -->
        <input type="password" bind:value={backup.restorePass} />
      </div>
      <div class="row">
        <button class="primary danger" disabled={!backup.restorePass || backup.busy} onclick={backupRestore}>{#if backup.busy}<Icon name="loader-alt" spin />{/if}{_("Replace everything")}</button>
      </div>
    {/if}
    {#if backup.error}<p class="error">{backup.error}</p>{/if}
    {#if backup.done}<p class="row ok"><Icon name="check-circle" />{backup.done}</p>{/if}
  {:else if page === "security"}
    <h3>{_("Encryption")}</h3>
    <p>
      {_("Everything xtermium stores (sessions, folders, tunnels, settings and trusted server keys) is encrypted on this computer. The key is random and kept in your system's keychain. (XChaCha20-Poly1305 with a 256-bit key, which stays safe even against quantum computers.)")}
    </p>
    <h3>{_("Recovery passphrase")}</h3>
    <p class="row">
      {#if app.recovery}
        <Icon name="check-circle" color="var(--ok)" />{_("Set. It unlocks your sessions on a new computer or after a reinstall.")}
      {:else}
        <Icon name="error" color="var(--warn)" /><span class="warn">{_("Not set. If your system's keychain loses the key, your sessions can't be opened.")}</span>
      {/if}
    </p>
    <div class="form">
      {#if app.recovery}
        <span>{_("Current passphrase")}</span>
        <input type="password" bind:value={rec.current} autocomplete="current-password" aria-label={_("Current passphrase")} />
      {/if}
      <span>{app.recovery ? _("New passphrase") : _("Passphrase")}</span>
      <div class="row">
        <input class="grow" type={rec.show ? "text" : "password"} bind:value={rec.pass} autocomplete="new-password" />
        <button class="flat icon-only" title={rec.show ? _("Hide") : _("Show")} aria-label={rec.show ? _("Hide passphrase") : _("Show passphrase")} onclick={() => (rec.show = !rec.show)}
          ><Icon name={rec.show ? "hide" : "show"} /></button
        >
        <button class="flat icon-only" title={_("Copy")} aria-label={_("Copy passphrase")} disabled={!rec.pass} onclick={copyPass}><Icon name="copy" /></button>
      </div>
      <span>{_("Repeat it")}</span>
      <input type={rec.show ? "text" : "password"} bind:value={rec.again} autocomplete="new-password" onkeydown={(e) => e.key === "Enter" && recOk && setRecovery()} />
      <span></span>
      <div class="row">
        <button onclick={generatePass}><Icon name="refresh" />{_("Generate")}</button>
        {#if rec.copied}<span class="ok">{_("Copied. Keep it somewhere safe, like a password manager.")}</span>{/if}
      </div>
    </div>
    {#if rec.pass && rec.pass.length < 12}<p class="weak">{_("At least 12 characters.")}</p>{:else if rec.again && !recOk}<p class="weak">{_("The passphrases differ.")}</p>{/if}
    {#if rec.error}<p class="error">{rec.error}</p>{/if}
    <div class="row">
      <button class="primary" disabled={!recOk || rec.busy} onclick={setRecovery}>{#if rec.busy}<Icon name="loader-alt" spin />{/if}{app.recovery ? _("Change passphrase") : _("Set passphrase")}</button>
      {#if rec.done}<span class="ok">{_("Saved.")}</span>{/if}
    </div>
    <h3>{_("Lock")}</h3>
    <p class="weak">
      {_("For when you step away: the lock hides every xtermium window until you unlock it, and your sessions keep running. It locks when xtermium starts, when you press {keys} or click the lock beside Settings, and after the time below without input.", {
        keys: keysOf("lock", s.shortcuts) || _("its shortcut"),
      })}
    </p>
    {#if lock.info}
      {@const i = lock.info}
      <div class="form">
        <span>{_("Password")}</span>
        <div class="row">
          <input
            class="grow"
            type="password"
            bind:value={lockForm.password}
            placeholder={i.password ? _("Set. Type a new one to change it") : _("At least 8 characters")}
            autocomplete="new-password"
            aria-label={_("Lock password")}
            onkeydown={(e) => e.key === "Enter" && lockForm.password.length >= 8 && lockSet({ set: "password", secret: lockForm.password }, _("The password is saved."))}
          />
          <button disabled={lockForm.password.length < 8 || lockForm.busy} onclick={() => lockSet({ set: "password", secret: lockForm.password }, _("The password is saved."))}>{i.password ? _("Change") : _("Set")}</button>
          {#if i.password}<button class="flat icon-only" title={_("Remove")} aria-label={_("Remove the password")} onclick={() => lockSet({ set: "password", secret: "" }, _("The password is removed."))}><Icon name="trash" /></button>{/if}
        </div>
        <span>{_("PIN")}</span>
        <div class="row">
          <input
            class="grow"
            type="password"
            inputmode="numeric"
            maxlength="12"
            bind:value={lockForm.pin}
            placeholder={i.pin ? _("Set. Type a new one to change it") : _("4 to 12 digits")}
            autocomplete="off"
            aria-label={_("Lock PIN")}
            onkeydown={(e) => e.key === "Enter" && pinOk && lockSet({ set: "pin", secret: lockForm.pin }, _("The PIN is saved."))}
          />
          <button disabled={!pinOk || lockForm.busy} onclick={() => lockSet({ set: "pin", secret: lockForm.pin }, _("The PIN is saved."))}>{i.pin ? _("Change") : _("Set")}</button>
          {#if i.pin}<button class="flat icon-only" title={_("Remove")} aria-label={_("Remove the PIN")} onclick={() => lockSet({ set: "pin", secret: "" }, _("The PIN is removed."))}><Icon name="trash" /></button>{/if}
        </div>
        {#if passkeysWork()}
          <span>{_("Passkey")}</span>
          <div class="row">
            {#if i.passkey}
              <span class="grow row"><Icon name="check-circle" color="var(--ok)" />{_("Set: a security key or this computer's")}</span>
              <button class="flat icon-only" title={_("Remove")} aria-label={_("Remove the passkey")} onclick={() => lockSet({ set: "passkey", passkey: null }, _("The passkey is removed."))}><Icon name="trash" /></button>
            {:else}
              <button disabled={lockForm.busy} onclick={() => lockSet({ set: "passkey", passkey: true }, _("The passkey is saved."))}><Icon name="key" />{_("Add a passkey")}</button>
            {/if}
          </div>
        {/if}
        {#if i.hello}
          <span>{_("Windows Hello")}</span>
          <label class="check"
            ><input
              type="checkbox"
              checked={i.biometrics}
              disabled={lockForm.busy}
              onchange={(e) => {
                const on = e.currentTarget.checked;
                e.currentTarget.checked = !on; // the answer comes back with lock.info
                lockSet({ set: "biometrics", on }, on ? _("Windows Hello unlocks it now.") : _("Windows Hello is off."));
              }}
            />{_("Your face, fingerprint or Windows PIN")}</label
          >
        {/if}
        <span>{_("Lock after")}</span>
        <select
          value={i.idle}
          disabled={!lockOn || lockForm.busy}
          onchange={(e) => {
            const minutes = Number(e.currentTarget.value);
            e.currentTarget.value = String(i.idle);
            lockSet({ set: "idle", minutes }, _("Saved."));
          }}
        >
          {#each [0, 1, 5, 10, 15, 30, 60] as m (m)}
            <option value={m}>{m === 0 ? _("Never") : m === 1 ? _("1 minute without input") : _("{n} minutes without input", { n: m })}</option>
          {/each}
        </select>
      </div>
      {#if lockOn && !i.confirmed}<p class="weak">{_("Changing these asks you to unlock first.")}</p>{/if}
      {#if lockForm.error}<p class="error">{lockForm.error}</p>{:else if lockForm.done}<p class="ok">{lockForm.done}</p>{/if}
      <div class="row">
        <button disabled={!lockOn} onclick={() => invoke("lock_now")}><Icon name="lock-alt" />{_("Lock now")}</button>
      </div>
    {/if}
    <h3>{_("Trusted server keys")}</h3>
    <p class="weak">{_("Remembered the first time you connect. Remove a key when a server was reinstalled and its key changed on purpose.")}</p>
    {#if known}
      {@const total = Object.keys(known.ssh).length + Object.keys(known.rdp).length + Object.keys(known.https ?? {}).length}
      {#if !total}<p class="weak">{_("None yet.")}</p>{/if}
      {#if total > PER_PAGE}
        <input type="search" placeholder={_("Search hosts and keys")} bind:value={keyQuery} oninput={() => (keyPage = 0)} />
      {/if}
      {#if total && !keyRows.length}<p class="weak">{_("No key matches.")}</p>{/if}
      <div class="keys">
        {#each keyRows.slice(keyPage * PER_PAGE, (keyPage + 1) * PER_PAGE) as [kind, id, k] (kind + id)}
          <div class="row key">
            <Icon name={KEY_LOOK[kind][0]} color={KEY_LOOK[kind][1]} />
            <span class="grow">{id}</span>
            <code class="weak" title={k}>{short(k)}</code>
            <button class="flat icon-only" title={_("Remove")} onclick={() => forget(kind, id)}><Icon name="trash" /></button>
          </div>
        {/each}
      </div>
      {#if keyPages > 1}
        <div class="row pager">
          <button class="flat" disabled={keyPage === 0} onclick={() => keyPage--}>{_("Previous")}</button>
          <span class="weak">{_("Page {n} of {total} · {keys} keys", { n: keyPage + 1, total: keyPages, keys: keyRows.length })}</span>
          <button class="flat" disabled={keyPage >= keyPages - 1} onclick={() => keyPage++}>{_("Next")}</button>
        </div>
      {/if}
    {:else}
      <p class="row weak"><Icon name="loader-alt" spin />{_("Loading…")}</p>
    {/if}
  {:else if page === "troubleshoot"}
    <h3>{_("Troubleshoot")}</h3>
    <p>
      {_("Something isn't working? Make a bundle: a folder of plain-text files about this installation (version, system, settings, counts of what's stored and what's open), then send its files to support@xtermium.com with what you saw.")}
    </p>
    <p class="weak">{_("Nothing in it names a server, a login or a password, and it never leaves this computer by itself: have a look before you send it.")}</p>
    <div class="row">
      <button class="primary" disabled={bundle.busy} onclick={makeBundle}>{#if bundle.busy}<Icon name="loader-alt" spin />{/if}{_("Create bundle")}</button>
      {#if bundle.path}<span class="weak">{_("Saved in {path}", { path: bundle.path })}</span>{/if}
    </div>
    {#if bundle.error}<div class="error">{bundle.error}</div>{/if}
  {:else if page === "about"}
    <About />
  {/if}
</Modal>
{#if showImport}<Import onclose={() => (showImport = false)} />{/if}
{#if showExport}<Export folder="" onclose={() => (showExport = false)} />{/if}

<style>
  /* page list down the left: room for any number of pages at any interface scale */
  .pages {
    flex: none;
    width: 183px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    /* in from the window's edge as far as the title above it */
    padding: 8px 13px;
    border-inline-end: 1px solid var(--line);
    overflow-y: auto;
  }
  .pages button {
    justify-content: flex-start;
    color: var(--weak);
    border: 0;
    /* a long name (some languages') takes two lines rather than running out of the list */
    white-space: normal;
    text-align: start;
  }
  .pages input {
    margin-bottom: 5px;
  }
  .pages .hit {
    flex: none;
    flex-direction: column;
    align-items: flex-start;
    gap: 0;
    text-align: start;
    white-space: normal;
  }
  .hit .what {
    color: var(--fg);
  }
  .hit .where {
    font-size: var(--t-s);
  }
  .none {
    padding: 5px 8px;
  }
  /* The setting the search points at, lit while it's open (its first hit, also lit in the list);
     the one it went to keeps the light a moment, then lets it go. */
  :global(.lit),
  .pages .hit.first {
    border-radius: var(--r-control);
    background: var(--gold-soft);
    box-shadow: 0 0 0 3px var(--gold-soft);
  }
  :global(.found) {
    border-radius: var(--r-control);
    animation: found 2.6s ease-in;
  }
  @keyframes found {
    from,
    40% {
      background: var(--gold-soft);
      box-shadow: 0 0 0 3px var(--gold-soft);
    }
  }
  .pages button.current {
    color: var(--fg);
    background: var(--raised);
    box-shadow: inset 2px 0 var(--gold);
  }
  h3 {
    margin-top: 5px;
  }
  p {
    margin: 0;
    max-width: 70ch;
  }
  .foot {
    margin-top: 8px;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .schemes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .scheme {
    flex-direction: column;
    align-items: stretch;
    gap: 5px;
    padding: 5px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-control);
    text-align: start;
  }
  .scheme:hover:not(:disabled) {
    background: inherit;
    border-color: var(--weak);
  }
  .scheme.selected {
    border: 2px solid var(--gold);
  }
  /* a link that opens in the browser */
  .custom-scheme {
    position: relative;
    display: grid;
  }
  .custom-scheme .remove {
    position: absolute;
    top: 2px;
    inset-inline-end: 2px;
  }
  .catalogue {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    max-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--line);
    border-radius: var(--r-control);
  }
  .catalogue button {
    flex: none;
    justify-content: flex-start;
    gap: 8px;
    border-radius: 0;
  }
  .swatches {
    display: grid;
    grid-template-columns: repeat(16, 1fr);
    gap: 2px;
  }
  .swatches span {
    height: 10px;
    border-radius: 1px;
  }
  .snippet {
    /* The page is a fixed-height column: without this it squeezes the snippet to two lines. */
    flex-shrink: 0;
    margin: 0;
    padding: 8px 13px;
    border-radius: var(--r-control);
    background: rgba(var(--deep-rgb), 1);
    font-size: var(--t-s);
    overflow-x: auto;
    user-select: text;
  }
  .commands {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr) max-content max-content;
    gap: 5px 8px;
    align-items: center;
  }
  .commands > span {
    color: var(--weak);
  }
  .snippet.one {
    padding: 5px 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .shortcuts {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
  }
  .shortcut {
    padding: 2px 5px 2px 8px;
    border-bottom: 1px solid var(--line);
  }
  .shortcut .grow {
    display: flex;
    flex-direction: column;
  }
  .note {
    font-size: var(--t-s);
  }
  .chord {
    min-width: 150px;
    justify-content: flex-end;
  }
  .keys {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
  }
  /* rows the same height with or without buttons */
  .key {
    min-height: calc(var(--control) + 8px);
    padding: 5px 5px 5px 8px;
    border-bottom: 1px solid var(--line);
  }
  .key code {
    font-size: var(--t-s);
  }
  .ssh-key {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .public {
    margin: 0;
    padding: 5px 8px;
    font-size: var(--t-s);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    border-radius: var(--r-control);
    background: rgba(var(--deep-rgb), 1);
  }
  .fingerprint {
    min-width: 0;
    overflow-wrap: anywhere;
    font-size: var(--t-s);
    user-select: text;
  }
  /* the room Rename and Delete take in the other rows, so the buttons line up */
  .slot {
    flex: none;
    width: calc(18px + 2 * 5px + 2px); /* an icon button: icon, padding, border */
  }

  .rules {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .rule input[type="color"] {
    flex: none;
    width: 30px;
    height: var(--control);
    padding: 2px;
  }
  .rule-error {
    margin-inline-start: calc(30px + var(--gap)); /* under the pattern */
    font-size: var(--t-s);
  }
  .pager {
    justify-content: space-between;
    padding-top: 5px;
  }
  .preview {
    height: 64px;
    padding: 8px 13px;
    font: 13px var(--mono);
    border-radius: 3px;
    background: rgba(var(--deep-rgb), 1);
  }
</style>
