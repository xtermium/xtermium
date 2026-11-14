// App-wide state, loaded from the backend at start.

import { invoke } from "@tauri-apps/api/core";
import { keysOf, shown } from "./actions";
import { listen } from "@tauri-apps/api/event";
import { memberColor } from "./coop";
import { COLORS, KIND_COLOR, KIND_ICON, OWNER_COLOR, system, type Background, type CoopClick, type CoopControl, type CoopCursor, type CoopEvent, type CoopSelection, type CoopRemote, type CoopRing, type Place, type Session, type Scheme, type Settings, type Store, type WorkspaceInfo, type Workspaces } from "./api";
import { _, setLanguage } from "./i18n.svelte";
import { shrinkIcon } from "./images";
import { oneAtATime } from "./serial";

export const app = $state({
  store: { session: [], tunnel: [], folder: [], snippet: [], credential: [], layout: [], agent_prompt: "", id: "", color: "" } as Store,
  /** Workspaces besides the default one (""), and the one in use. */
  workspaces: [] as string[],
  /** Every workspace with its id and colour, the default one first. The one in use is `store`:
   *  its colour there is the fresher one. */
  workspaceList: [] as WorkspaceInfo[],
  workspace: "",
  /** The Settings window is open (Settings.svelte `openSettings` opens it on a page). */
  showSettings: false,
  /** The shared logins window is open (any editor can open it). */
  showCredentials: false,
  /** The sign-in dialog is open (SignIn.svelte). */
  signIn: false,
  /** The Terminal Co-op dialog is open (Coop.svelte). */
  showCoop: false,
  /** Terminal Co-op as the backend last said (the `coop` event, src/coop.rs): the ring you are in, your
   *  tabs it shares (their ids), the tabs shared with you, where the others' pointers are, what
   *  they have selected, who drives each shared desktop and who asks to. */
  coop: {
    ring: null as CoopRing | null,
    shared: [] as number[],
    remote: [] as CoopRemote[],
    cursors: [] as CoopCursor[],
    selections: [] as CoopSelection[],
    control: [] as CoopControl[],
    wants: [] as CoopControl[],
    /** The ring's clicks of the last second (CoopCursors.svelte draws them as they come). */
    clicks: [] as CoopClick[],
  },
  settings: null as Settings | null,
  schemes: [] as Scheme[],
  ciphers: [] as string[],
  version: "",
  os: "",
  /** One-line message for the user (errors, import results). */
  status: "",
  /** The Settings page a status message names, shown as a link while that message is up. */
  statusLink: null as { text: string; page: string } | null,
  /** Some(recoverable) while stored data waits for the recovery passphrase. */
  locked: null as boolean | null,
  recovery: false,
  /** Cloud sync: running now; when it last worked; why it didn't; waiting for the recovery passphrase. */
  sync: {
    busy: false,
    last: null as Date | null,
    error: "",
    join: false,
    /** Both sides changed: the person picks one (App.svelte asks). */
    conflict: null as { local: SyncSide; remote: SyncSide } | null,
    /** The recovery passphrase of a join waiting on that choice (memory only). */
    pass: "",
    /** Who is signed in and whether the account has Pro, as the server last said (`loadAccount`). */
    account: null as Account | null,
    /** The server refused the last upload for want of Pro: the account menu offers it. */
    needsPro: false,
    /** The account's storage, bytes used and allowed, as the server last said. */
    quota: null as { used: number; total: number } | null,
  },
});

// Terminal Co-op: the whole picture arrives whenever any of it changes, pointers up to 20 times a
// second, so only the parts that differ are assigned and the rest of the interface isn't redrawn.
function takeCoop(p: CoopEvent) {
  const next = {
    ring: p.ring ?? null,
    shared: p.shared ?? [],
    remote: p.remote ?? [],
    cursors: p.cursors ?? [],
    selections: p.selections ?? [],
    control: p.control ?? [],
    wants: p.wants ?? [],
  };
  const same = (k: keyof typeof next) => JSON.stringify(app.coop[k]) === JSON.stringify(next[k]);
  Object.assign(app.coop, Object.fromEntries(Object.entries(next).filter(([k]) => !same(k as keyof typeof next))));
}

/** Everyone in the ring hears who drives a shared desktop, as it changes: a toast on each window, so
 *  nobody wonders why their clicks do nothing, or whose pointer that is. The desktop's own banner
 *  (Rdp.svelte) says the standing state. */
function sayControl(before: CoopControl[], wanted: CoopControl[]) {
  const ring = app.coop.ring;
  if (!ring) return;
  const me = ring.members.find((m) => m.you)?.id ?? "";
  const owner = ring.members.find((m) => m.owner);
  const nameOf = (id: string) => ring.members.find((m) => m.id === id)?.name ?? _("Someone");
  const held = (list: CoopControl[], tab: number) => list.find((c) => c.tab === tab)?.user_id ?? "";
  const tabs = new Set([...before, ...app.coop.control].map((c) => c.tab));
  for (const tab of tabs) {
    const [was, now] = [held(before, tab), held(app.coop.control, tab)];
    if (was === now) continue;
    if (now === me) toast(_("You have control of the shared desktop."));
    else if (now) toast(_("{name} has control of the shared desktop.", { name: nameOf(now) }));
    else if (ring.owner) toast(_("You have control of the shared desktop again."));
    else toast(_("{name} has control of the shared desktop again.", { name: owner?.name ?? _("The owner") }));
  }
  // The owner hears each new ask; a guest hears when theirs was turned down (it went without control following).
  for (const w of app.coop.wants) {
    if (ring.owner && !wanted.some((b) => b.tab === w.tab && b.user_id === w.user_id)) toast(_("{name} asks for control of the shared desktop.", { name: nameOf(w.user_id) }));
  }
  for (const w of wanted) {
    const kept = app.coop.wants.some((n) => n.tab === w.tab && n.user_id === w.user_id);
    if (!ring.owner && !kept && held(app.coop.control, w.tab) !== me && app.coop.remote.some((r) => r.id === w.tab)) toast(_("The owner kept control of the desktop."));
  }
}
let heard = false;
listen<CoopEvent>("coop", ({ payload: p }) => {
  heard = true;
  const was = app.coop.ring;
  const [control, wants] = [app.coop.control, app.coop.wants];
  takeCoop(p);
  sayControl(control, wants);
  // Said once, as the ring goes; later events that still carry the reason don't repeat it. The backend's
  // own sentence says why best; these are for when it sent none.
  if (!p.ended || !was) return;
  toast(
    p.message ||
      (p.ended === "pro"
        ? _("Your ring ended because your account no longer has Pro.")
        : p.ended === "kicked"
          ? _("The owner removed you from the ring.")
          : _("The ring ended.")),
  );
});
/** How long a click stays drawn, in milliseconds. */
export const CLICK_SHOWN = 1000;
// A click in the ring: kept for the moment the overlays draw it, then dropped.
listen<Omit<CoopClick, "at">>("coop-click", ({ payload: c }) => {
  const fresh = () => app.coop.clicks.filter((k) => performance.now() - k.at < CLICK_SHOWN);
  app.coop.clicks = [...fresh(), { ...c, at: performance.now() }];
  setTimeout(() => (app.coop.clicks = fresh()), CLICK_SHOWN + 100);
});
// A window that starts or reloads while a ring is up learns of it here; an event that got in first is newer.
invoke<CoopEvent>("coop_state")
  .then((p) => heard || takeCoop(p))
  .catch(() => {});

/** Tabs to reopen, with the names they were given, handed over once by boot(). */
let restore: [Session, string, string | undefined][] = [];
export const takeRestore = () => restore.splice(0);

/** Fields an older or empty store leaves out. */
function filled(s: Store): Store {
  s.folder ??= [];
  s.snippet ??= [];
  s.credential ??= [];
  s.layout ??= [];
  s.agent_prompt ??= "";
  s.id ??= "";
  s.color ??= "";
  return s;
}

// Automation (Settings > MCP) saved or deleted a session: show the sessions as stored.
listen<Store>("store", ({ payload: s }) => (app.store = filled(s)));

/** The colour a ring member wears everywhere, the same on every computer (coop.ts `memberColor`). */
export const coopColor = (id: string) => {
  const at = memberColor(id, app.coop.ring?.members ?? [], COLORS.length);
  return at < 0 ? OWNER_COLOR : COLORS[at];
};

/** Takes a workspace switch's (or sync's) answer: the workspaces and the data of the one in use. */
export function useWorkspaces(w: Workspaces) {
  Object.assign(app, { workspaces: w.workspaces, workspaceList: w.workspace_list, workspace: w.workspace, store: filled(w.store) });
}
// Automation switched workspaces.
listen<Workspaces>("workspaces", ({ payload }) => useWorkspaces(payload));

/** Switches, adds, renames, deletes, clones or syncs workspaces (src/main.rs `workspace`). */
export async function workspaceOp(
  op: { op: "switch" | "add" | "delete" | "clone"; name: string } | { op: "rename"; name: string; to: string } | { op: "sync"; from: string; to: string },
) {
  useWorkspaces(await invoke<Workspaces>("workspace", { op }));
}

/** Stores the sessions. One save at a time, so they reach the backend (which stores off its main
 *  thread) in order; edits made during one are stored together right after it. */
export const saveStore = oneAtATime(async () => {
  try {
    // The backend cleans what it stores (no control characters, valid ports, tidy folders).
    const workspace = app.workspace;
    const sent = JSON.stringify($state.snapshot(app.store));
    const stored = filled(await invoke<Store>("save_store", { store: JSON.parse(sent), workspace }));
    // The cleaned copy comes back, unless something changed meanwhile, which taking this older
    // copy would undo (the next save cleans it). Switched meanwhile: it was stored in the
    // workspace it came from.
    const now = JSON.stringify($state.snapshot(app.store));
    if (workspace === app.workspace && now === sent && JSON.stringify(stored) !== sent) app.store = stored;
  } catch (e) {
    app.status = _("save failed: {error}", { error: String(e) });
  }
});

/** One side of a sync conflict (src/sync.rs `Side`). */
export type SyncSide = { at: number | null; device: string; os: string; ip: string; location: string; sessions: number; tunnels: number; keys: number };
/** The signed-in account (src/sync.rs `Account`). */
export type Account = { email: string; name: string | null; image: string | null; pro: { active: boolean; plan: string | null; until: number | null } };
/** Why nothing synced, when the app acts on it rather than only showing it (src/sync.rs `Trouble`). */
type Trouble = { kind: "signed_out" } | { kind: "needs_pro"; message: string } | { kind: "later"; after: number };
type SyncReply = { join: boolean; conflict: { local: SyncSide; remote: SyncSide } | null; pulled: boolean; pushed: boolean; quota: { used: number; total: number } | null; trouble: Trouble | null; settings: Settings | null } & Workspaces;

/** Where Pro is bought: the page signs the person in first, so the purchase lands on the account. */
export const CHECKOUT = "https://account.xtermium.com/checkout/monthly";
/** The account page: name, picture, linked sign-ins, devices. */
export const ACCOUNT = "https://account.xtermium.com/account";

/** A sync the server put off (`later`): tried again on its own, waiting twice as long each time
 *  from 30 s up to 30 min, for a day. "Sync now" or a success starts over. */
const RETRY = { first: 30, max: 1800, giveUp: 86_400_000 };
const retry = { wait: 0, since: 0, timer: 0 as ReturnType<typeof setTimeout> | 0 };

/**
 * Syncs with the cloud: at start and when asked, never on its own otherwise except to retry a
 * server that was away. `passphrase` joins this device to the account's data; `choice` settles
 * the conflict the last sync reported.
 */
export function syncNow(passphrase?: string, choice?: "local" | "remote") {
  clearTimeout(retry.timer);
  Object.assign(retry, { wait: 0, since: 0 });
  return attempt(passphrase, choice);
}

async function attempt(passphrase?: string, choice?: "local" | "remote") {
  const sync = app.sync;
  if (!app.settings?.sync || app.locked !== null || sync.busy) return;
  sync.busy = true;
  try {
    const r = await invoke<SyncReply>("sync_now", { passphrase: passphrase ?? null, choice: choice ?? null });
    [sync.join, sync.error, sync.conflict] = [r.join, "", r.conflict];
    sync.quota = r.quota ?? sync.quota;
    // A join that conflicts needs the passphrase again with the choice.
    sync.pass = r.conflict ? (passphrase ?? "") : "";
    if (r.trouble) return troubled(r.trouble);
    sync.needsPro = false;
    if (r.join || r.conflict) return;
    if (r.pulled) useWorkspaces(r);
    // Settings or backgrounds that came from the cloud (the other windows got the `settings` event).
    if (r.settings) {
      if (r.settings.language !== app.settings?.language) setLanguage(r.settings.language);
      app.settings = r.settings;
    }
    sync.last = new Date();
  } catch (e) {
    sync.error = String(e);
    // Wrong passphrase: keep asking for it.
    if (passphrase) throw e;
  } finally {
    sync.busy = false;
  }
}

function troubled(t: Trouble) {
  const sync = app.sync;
  if (t.kind === "signed_out") {
    signOut();
    sync.error = _("Signed out: sign in again to sync.");
  } else if (t.kind === "needs_pro") {
    [sync.needsPro, sync.error] = [true, t.message];
  } else {
    const now = Date.now();
    retry.since ||= now;
    if (now - retry.since > RETRY.giveUp) return void (sync.error = _("Syncing issue, try later…"));
    retry.wait = Math.min(Math.max(retry.wait * 2, RETRY.first, t.after), RETRY.max);
    // Jitter, so every device of an outage doesn't come back in the same second.
    const wait = retry.wait * (0.8 + Math.random() * 0.4);
    retry.timer = setTimeout(() => attempt(), wait * 1000);
    sync.error = _("The sync server can't be reached. Trying again in a while; everything here keeps working.");
  }
}

/** Asks the server who is signed in and whether the account has Pro; keeps what it last said on a
 *  failure. Pro is also remembered in the settings, so it holds at start-up and offline. */
export async function loadAccount() {
  const token = app.settings?.sync_token;
  if (!token) return;
  const account = await invoke<Account>("account_info", { token }).catch(() => null);
  if (!account) return;
  app.sync.account = account;
  app.settings!.sync_pro = !!account.pro.active;
}

/** Opens the subscription's portal (cancel, change the card, invoices) in the browser. */
export async function openPortal() {
  const url = await invoke<string>("account_portal", { token: app.settings!.sync_token });
  await invoke("open_url", { url });
}

/** Signs out of the sync account, which turns sync off; this device keeps its data. `everywhere`
 *  ends every session of the account, on every device. */
export function signOut(everywhere = false) {
  const s = app.settings!;
  const token = s.sync_token;
  Object.assign(s, { sync: false, sync_token: "", sync_email: "", sync_pro: false });
  Object.assign(app.sync, { last: null, error: "", join: false, conflict: null, pass: "", quota: null, account: null, needsPro: false });
  clearTimeout(retry.timer);
  // The server forgets the token too, if it can be reached; here it's gone either way.
  if (token) invoke("account_sign_out", { token, everywhere }).catch(() => {});
}

/** Folder paths related to `path`: itself and everything below it. */
export const inside = (p: string, path: string) => p === path || p.startsWith(path + "/");

/** The colour a folder shows: its own, else the nearest parent's; "" for none. */
export function folderColor(path: string): string {
  for (let p = path; p; p = p.split("/").slice(0, -1).join("/")) {
    const c = app.store.folder.find((f) => f.path === p)?.color;
    if (c) return c;
  }
  return "";
}

/** The tags a folder carries, its parents' first: what a session inside it is (prod, lab…). */
export function folderTags(path: string): string[] {
  const tags: string[] = [];
  for (let p = path; p; p = p.split("/").slice(0, -1).join("/")) {
    for (const t of app.store.folder.find((f) => f.path === p)?.tags ?? []) if (!tags.includes(t)) tags.unshift(t);
  }
  return tags;
}

/** How a session looks in the sidebar and tab strip: its icon, always in its kind's colour (the
 *  cable colour says what kind of connection it is), and `tint`, the colour it or its folder is
 *  marked with ("" for none), which goes on its name and tab line instead. */
export const lookOf = (s: Session) => ({
  icon: s.icon || KIND_ICON[s.kind.type],
  color: KIND_COLOR[s.kind.type],
  tint: s.color || folderColor(s.folder),
  still: !!s.still,
});

/** Which saved session a tab is of: its folder and name (a reload or sync makes new objects). */
export const sessionKey = (s: Session) => `${s.folder}\n${s.name}`;

/** Which saved session a tab is of, for the sidebar's marks: the session's own id, the same
 *  through a rename or a move ("" for one never saved). */
export const sessionId = (s: Session) => s.id ?? "";

/** Each place's background picture (a `data:` URL) or shader source, as Settings set them. */
export const backdrops = $state<Record<Place, string>>({ terminal: "", sidebar: "", bar: "" });
const stamps: Record<Place, number> = { terminal: -1, sidebar: -1, bar: -1 };
/** Fetches a place's background when it changed (Backdrop.svelte calls it; once per change). */
export function loadBackdrop(place: Place, b: Background | undefined) {
  if (!b || b.stamp === stamps[place]) return;
  stamps[place] = b.stamp;
  if (!b.kind) return void (backdrops[place] = "");
  invoke<string>("background_get", { place }).then((d) => b.stamp === stamps[place] && (backdrops[place] = d));
}

/** Addresses whose icon was asked for since the app started: one try each. */
const askedIcons = new Set<string>();

/**
 * A saved website session without an icon of its own gets the site's (its favicon), in the
 * background. Called when one is saved or opened; sites behind an SSH gateway are skipped.
 */
export function siteIcon(s: Session) {
  const k = s.kind;
  if (k.type !== "browser" || s.icon || askedIcons.has(k.url)) return;
  const [key, url] = [sessionKey(s), k.url];
  const saved = () => app.store.session.find((x) => sessionKey(x) === key && x.kind.type === "browser" && x.kind.url === url && !x.icon);
  if (!saved() || app.store.credential.find((c) => c.id === k.credential)?.gateway) return;
  askedIcons.add(url);
  invoke<string>("fetch_favicon", { url })
    .then(shrinkIcon)
    .then((icon) => {
      const s = saved();
      if (!s) return;
      s.icon = icon;
      saveStore();
    })
    .catch(() => {}); // no icon: the kind's stays
}

/** Loads (or reloads, after unlocking) everything from the backend. */
export async function boot() {
  const b = await invoke<import("./api").Boot>("boot");
  await setLanguage(b.settings.language);
  restore = b.tabs.map((s, i) => [s, b.labels[i] ?? "", b.tab_workspaces[i]]);
  useWorkspaces(b);
  system.os = b.os;
  Object.assign(app, {
    settings: b.settings,
    schemes: b.schemes,
    ciphers: b.ciphers,
    version: b.version,
    os: b.os,
    status: b.notice,
    locked: b.locked,
    recovery: b.recovery,
  });
  lock.locked = b.lock;
  loadLock().catch(() => {});
  if (!b.locked) syncNow();
}

// ---- the lock (src/lock.rs): a lock screen over every window until it's you ----

/** The ways to unlock that are set (never their secrets), and the lock's state. `passkey`: its
 *  credential id and relying party. */
export type LockInfo = { password: boolean; pin: boolean; passkey: [string, string] | null; biometrics: boolean; idle: number; locked: boolean; confirmed: boolean; hello: boolean };
export const lock = $state({
  /** The lock screen covers this window: from boot, then the `lock` event. */
  locked: false,
  info: null as LockInfo | null,
  /** Settings asked to confirm it's you before the lock changes: called with the answer. */
  confirm: null as ((ok: boolean) => void) | null,
});
export async function loadLock() {
  lock.info = await invoke<LockInfo>("lock_info");
  lock.locked = lock.info.locked;
}
listen<boolean>("lock", ({ payload }) => ((lock.locked = payload), loadLock()));
/** Unlocks while unlocked, before the lock's settings change: true once it's you. */
export const confirmIt = () => new Promise<boolean>((done) => (lock.confirm = (ok) => ((lock.confirm = null), done(ok))));
// Input here starts the lock's idle time over, said once a minute at most (Windows tells the
// backend itself; this is for the others).
let saidActive = 0;
for (const kind of ["keydown", "pointerdown", "wheel"])
  addEventListener(
    kind,
    () => {
      if (Date.now() - saidActive < 60_000) return;
      saidActive = Date.now();
      invoke("lock_activity").catch(() => {});
    },
    { capture: true, passive: true },
  );

/** Short notes that show at the bottom of the window for a moment ("Copied to clipboard"). One
 *  with an `action` (Undo, Reopen) carries its button and stays long enough to reach it. */
export type ToastAction = { label: string; run: () => void };
export const toasts = $state<{ id: number; text: string; action?: ToastAction }[]>([]);
let toastId = 0;
/** Ticks a getting-started step that has no state to show it by (Guide.svelte `STEPS`). */
export function guideDone(id: string) {
  const s = app.settings;
  if (s?.guide && !s.guide_done.includes(id)) s.guide_done.push(id);
}
export function toast(text: string, action?: ToastAction) {
  const id = ++toastId;
  toasts.push({ id, text, action });
  setTimeout(() => dismiss(id), action ? 8000 : 2400);
}
export function dismiss(id: number) {
  const at = toasts.findIndex((t) => t.id === id);
  if (at >= 0) toasts.splice(at, 1);
}

/** Copies `text` to the clipboard, and says so. */
export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(_("Copied to clipboard"));
  } catch (e) {
    app.status = String(e);
  }
}

/** A status message that names a Settings page, with a link to it. */
export function tell(text: string, page: string) {
  app.status = text;
  app.statusLink = { text, page };
}

/** The keys an action is bound to, with your changes (Settings › Shortcuts), as this system
 *  writes them ("⇧⌘S" on macOS); "" for none. */
export const shortcut = (action: string) => shown(keysOf(action, app.settings?.shortcuts ?? {}));

/** A tooltip with the action's shortcut after it: "Snippets (Ctrl+Shift+S)", "Snippets (⇧⌘S)". tooltip.ts draws
 *  that ending as key caps. */
export function tip(text: string, action: string): string {
  const keys = shortcut(action);
  return keys ? `${text} (${keys})` : text;
}

/** A desktop notification, when Settings › General allows them. */
export function notify(title: string, body: string) {
  if (app.settings?.notifications) invoke("notify", { title, body }).catch(() => {});
}
