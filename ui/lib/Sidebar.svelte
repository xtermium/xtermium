<script lang="ts">
  // Sessions: a folder tree (or a flat list while searching). Double-click opens (one click, if
  // set), right-click for more. Folders can be renamed, tagged (tags are searchable), coloured,
  // exported and deleted. Search is a button that opens into a field: arrow keys pick a result,
  // Enter closes the search and puts the keyboard on it in the tree, Escape just closes it. The
  // list is one stop for Tab; arrow keys walk it.
  // Sessions with a tab open have a dot on their icon. Ctrl+click (Cmd on macOS) and Shift+click
  // pick several sessions and folders, to drag, delete or move to another workspace together.
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { _, N_ } from "./i18n.svelte";
  import type { Snippet } from "svelte";
  import { host, KVM_VENDORS, newSession, openUrl, type Folder, type Session, type Workspaces } from "./api";
  import Appearance from "./Appearance.svelte";
  import Export from "./Export.svelte";
  import Icon from "./Icon.svelte";
  import { menu as menuAt } from "./menu";
  import Modal from "./Modal.svelte";
  import SessionIcon from "./SessionIcon.svelte";
  import { app, folderColor, inside, lookOf, saveStore, sessionKey, tip, toast, useWorkspaces } from "./state.svelte";

  const KIND_NAMES: Record<string, string> = {
    ssh: "SSH",
    shell: N_("Local shell"),
    serial: N_("Serial port"),
    rdp: N_("Remote desktop (RDP)"),
    vnc: "VNC",
    kvm: N_("BMC console"),
    telnet: "Telnet",
    winrm: "WinRM",
    browser: N_("Website"),
    agent: N_("AI agent"),
  };

  /** The hover hint on a session: its type, who and where it connects, and the rest worth knowing. */
  function details(s: Session, on?: boolean): string {
    const k = s.kind;
    const login = (id?: string) => app.store.credential.find((c) => c.id === id);
    const lines = [`${_(KIND_NAMES[k.type])}: ${s.name}`];
    const add = (label: string, v: unknown) => v && lines.push(`${label}: ${v}`);
    if (k.type === "ssh") {
      const c = login(k.credential);
      add(_("User"), c ? _("{user} (shared login “{name}”)", { user: c.user, name: c.name }) : k.user || _("asked when connecting"));
      add(_("Host"), `${k.host}:${k.port}`);
      add(_("Key"), !c && k.key);
      add(_("Jump host"), k.jump && `${k.jump.user ? k.jump.user + "@" : ""}${k.jump.host}:${k.jump.port}`);
      add(_("Runs"), k.command);
      add(_("Agent forwarding"), k.agent_forward && _("on"));
    } else if (k.type === "kvm") {
      const c = login(k.credential);
      add(_("User"), c ? _("{user} (shared login “{name}”)", { user: c.user, name: c.name }) : _("signs in on the page"));
      add(_("Host"), `${k.host}:${k.port}`);
      add(_("Vendor"), KVM_VENDORS.find(([id]) => id === k.vendor)?.[1]);
    } else if (k.type === "rdp" || k.type === "vnc") {
      const c = login(k.credential);
      add(_("User"), c ? _("{user} (shared login “{name}”)", { user: c.user, name: c.name }) : k.type === "rdp" && (k.user || _("asked when connecting")));
      add(_("Host"), `${k.host}:${k.port}`);
    } else if (k.type === "telnet") add(_("Host"), `${k.host}:${k.port}`);
    else if (k.type === "winrm") {
      add(_("User"), k.user || _("your Windows login"));
      add(_("Host"), `${k.host}:${k.port}${k.https ? " (HTTPS)" : ""}`);
    }
    else if (k.type === "serial") add(_("Port"), _("{port} at {baud} baud", { port: k.port, baud: k.baud }));
    else if (k.type === "shell") add(_("Program"), k.program || _("the default shell"));
    else if (k.type === "browser") add(_("Address"), k.url);
    else if (k.type === "agent") {
      add(_("Runs"), host(k));
      add(_("In"), k.dir || _("the home folder"));
    }
    else add(_("File"), host(k));
    add(_("Folder"), s.folder);
    if (on !== undefined) lines.push(on ? _("Open in a tab") : _("Open in a tab, disconnected"));
    if (s.notes) lines.push("", s.notes);
    return lines.join("\n");
  }

  let {
    onopen,
    onedit,
    actions,
    searching = $bindable(false),
    live = new Map(),
  }: {
    onopen: (s: Session) => void;
    onedit: (s: Session, index: number | null) => void;
    actions?: Snippet;
    searching?: boolean;
    /** Sessions with a tab, by `sessionKey`: true while one is connected. */
    live?: Map<string, boolean>;
  } = $props();

  /** `path`: `/`-separated like a session's folder; `count`: sessions in it and below. */
  type Node = { path: string; children: Map<string, Node>; items: number[]; count: number };
  let filter = $state("");
  /** Open folders; they come back next time (Settings > General). */
  let open = $state<Record<string, boolean>>(Object.fromEntries((app.settings?.remember_folders ? app.settings.open_folders : []).map((p) => [p, true])));
  /** `root`: the empty space around the sessions; `many`: on one of several picked rows. */
  let menu = $state<{ x: number; y: number; i?: number; folder?: string; root?: boolean; many?: boolean } | null>(null);
  /** The search result Enter opens. */
  let sel = $state(0);
  let search = $state<HTMLInputElement>();
  let list: HTMLDivElement;

  export async function focusSearch() {
    searching = true;
    await tick();
    search?.focus();
    search?.select();
  }

  function closeSearch() {
    [filter, searching] = ["", false];
  }

  const folderOf = (path: string) => app.store.folder.find((f) => f.path === path);
  /** Every folder there is, stored or there through a session (Prod/Web counts Prod too). */
  const folders = $derived(
    new Set([...app.store.folder.map((f) => f.path), ...app.store.session.flatMap((s) => s.folder.split("/").filter(Boolean).map((_, i, p) => p.slice(0, i + 1).join("/")))]),
  );
  $effect(() => {
    const s = app.settings;
    if (!s) return;
    const list = s.remember_folders ? Object.keys(open).filter((p) => open[p] && folders.has(p)).sort() : [];
    if (JSON.stringify(list) !== JSON.stringify(s.open_folders)) s.open_folders = list;
  });
  const tagsOf = (path: string) => folderOf(path)?.tags ?? [];
  /** Sibling folders: the ones dragged into place first, in that order, then the rest by name. */
  const rank = (p: string) => folderOf(p)?.order || Infinity;
  const byOrder = (a: string, b: string) => rank(a) - rank(b) || nameOf(a).localeCompare(nameOf(b));

  const tree = $derived.by(() => {
    const root: Node = { path: "", children: new Map(), items: [], count: 0 };
    const walk = (folder: string, count: boolean) => {
      let n = root;
      if (count) n.count++;
      for (const part of folder.split("/").filter(Boolean)) {
        const path = n.path ? `${n.path}/${part}` : part;
        if (!n.children.has(part)) n.children.set(part, { path, children: new Map(), items: [], count: 0 });
        n = n.children.get(part)!;
        if (count) n.count++;
      }
      return n;
    };
    app.store.folder.forEach((f) => walk(f.path, false));
    app.store.session.forEach((s, i) => walk(s.folder, true).items.push(i));
    const sort = (n: Node): Node => ({
      ...n,
      children: new Map([...n.children].sort(([, a], [, b]) => byOrder(a.path, b.path)).map(([k, v]) => [k, sort(v)])),
    });
    return sort(root);
  });

  /** Sessions whose name, host or folder matches, or whose folder (or a parent) has a matching tag. */
  const matches = $derived.by(() => {
    const f = filter.trim().toLowerCase().replace(/^#/, "");
    const tagged = app.store.folder.filter((d) => d.tags.some((t) => t.toLowerCase().includes(f))).map((d) => d.path);
    return app.store.session
      .map((s, i) => [s, i] as const)
      .filter(
        ([s]) =>
          s.name.toLowerCase().includes(f) ||
          host(s.kind).toLowerCase().includes(f) ||
          s.folder.toLowerCase().includes(f) ||
          tagged.some((p) => inside(s.folder, p)),
      )
      .map(([, i]) => i);
  });

  /** Search shows this many matches at most: more is a wall to scroll through, and slow to draw. */
  const SHOWN = 200;

  function searchKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, Math.min(Math.min(matches.length, SHOWN) - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
      requestAnimationFrame(() => list.querySelector(".hit")?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter" && filter && matches[sel] !== undefined) {
      // The search closes and the hit is shown in the tree with the keyboard on it: Enter again
      // opens it. Not this Enter, which stops here.
      e.preventDefault();
      const i = matches[sel];
      closeSearch();
      reveal(i);
    } else if (e.key === "Escape") {
      e.stopPropagation();
      closeSearch();
      // The field goes away: the keyboard goes on to the tree it was searching, not nowhere.
      tick().then(() => list.focus());
    }
  }

  /** Shows session `i` in the tree, its folders opened, and puts the keyboard on its row. */
  async function reveal(i: number) {
    const parts = app.store.session[i].folder.split("/").filter(Boolean);
    for (let n = 1; n <= parts.length; n++) open[parts.slice(0, n).join("/")] = true;
    await tick();
    const row = list.querySelector<HTMLElement>(`[data-session="${i}"]`);
    row?.scrollIntoView({ block: "nearest" });
    row?.focus();
  }

  /** Deletes one session at once (it was picked by name from its menu), with Undo for a while. */
  function remove(i: number) {
    const [gone] = app.store.session.splice(i, 1);
    saveStore();
    toast(_("Deleted {name}.", { name: gone.name }), {
      label: _("Undo"),
      run: () => {
        app.store.session.splice(Math.min(i, app.store.session.length), 0, gone);
        saveStore();
      },
    });
  }

  // ---- picking several: rows by key, "s<index>" for a session, "f<path>" for a folder ----
  let chosen = $state(new Set<string>());
  /** The row clicked last without Ctrl: Shift+click picks from there, Ctrl+click adds to it. */
  let anchor: string | null = null;
  // Sessions come and go, or another workspace's show: indexes no longer mean the same rows.
  $effect(() => {
    void [app.store, app.store.session.length];
    chosen = new Set();
  });
  const keyOf = (el: HTMLElement) => (el.dataset.folder !== undefined ? "f" + el.dataset.folder : "s" + el.dataset.session);

  /** Ctrl+click (Cmd on macOS) picks a row or lets it go; Shift+click picks the rows from the
   *  anchor. True if it did: the click does nothing else then. */
  function choose(e: MouseEvent, key: string): boolean {
    if (!(e.ctrlKey || e.metaKey || e.shiftKey)) {
      if (chosen.size) chosen = new Set();
      anchor = key;
      return false;
    }
    const next = new Set(chosen);
    const keys = rows().map(keyOf);
    if (e.shiftKey && anchor && keys.includes(anchor)) {
      const [a, b] = [keys.indexOf(anchor), keys.indexOf(key)].sort((x, y) => x - y);
      keys.slice(a, b + 1).forEach((k) => next.add(k));
    } else {
      // Only the row clicked: the anchor isn't shown picked, so picking it too would hand a
      // later "Delete" a row (maybe a whole folder) nobody saw chosen.
      if (!next.delete(key)) next.add(key);
      anchor = key;
    }
    chosen = next;
    return true;
  }

  /** The picked sessions (indexes) and folders; whatever's inside a picked folder goes with it. */
  function picked() {
    const all = [...chosen].filter((k) => k[0] === "f").map((k) => k.slice(1));
    const folders = all.filter((p) => !all.some((q) => q !== p && inside(p, q)));
    const sessions = [...chosen]
      .filter((k) => k[0] === "s")
      .map((k) => Number(k.slice(1)))
      .filter((i) => app.store.session[i] && !folders.some((p) => inside(app.store.session[i].folder, p)));
    return { sessions, folders };
  }
  /** How many sessions a pick holds, the ones in its folders too. */
  const countPicked = (p: ReturnType<typeof picked>) =>
    app.store.session.filter((s, i) => p.sessions.includes(i) || p.folders.some((f) => inside(s.folder, f))).length;

  /** Moves the picked rows into `folder` ("": the top level); sessions next to session `row` if given. */
  function moveChosen(folder: string, row?: number, after?: boolean) {
    const { sessions, folders } = picked();
    const list = app.store.session;
    const items = sessions.map((i) => list[i]);
    const next = row !== undefined && !sessions.includes(row) ? list[row] : undefined;
    for (const p of folders) {
      // Not into itself or below itself, nor where it is already.
      if (inside(folder, p) || parentOf(p) === folder) continue;
      const f = folderOf(p);
      if (f) f.order = 0;
      moveFolder(p, [folder, nameOf(p)].filter(Boolean).join("/"));
    }
    if (next) {
      for (const i of [...sessions].sort((a, b) => b - a)) list.splice(i, 1);
      list.splice(list.indexOf(next) + (after ? 1 : 0), 0, ...items);
    }
    for (const s of items) s.folder = folder;
    if (folder) open[folder] = true;
    chosen = new Set();
    saveStore();
  }

  /** The pick waiting for "Delete them?", and the one waiting for a workspace to move to. */
  let deletingMany = $state<ReturnType<typeof picked> | null>(null);
  let moving = $state<ReturnType<typeof picked> | null>(null);

  function deleteChosen() {
    const { sessions, folders } = deletingMany!;
    app.store.session = app.store.session.filter((s, i) => !sessions.includes(i) && !folders.some((p) => inside(s.folder, p)));
    app.store.folder = app.store.folder.filter((f) => !folders.some((p) => inside(f.path, p)));
    [deletingMany, chosen] = [null, new Set()];
    saveStore();
  }

  /** The other workspaces, to move sessions to ("" the default one). */
  const elsewhere = $derived(["", ...app.workspaces].filter((w) => w !== app.workspace));
  const wsName = (w: string) => w || _("Default workspace");

  /** Moves the pick to workspace `to` (src/main.rs `move_to_workspace`). */
  async function toWorkspace(to: string) {
    const { sessions, folders } = moving!;
    const n = countPicked(moving!);
    moving = null;
    try {
      await saveStore(); // the backend counts sessions as stored
      useWorkspaces(await invoke<Workspaces>("move_to_workspace", { to, sessions, folders }));
      app.status = _("Moved {n} sessions to {workspace}.", { n, workspace: wsName(to) });
    } catch (e) {
      app.status = String(e);
    }
  }

  // ---- folder actions ----
  /** `fresh`: a new folder, made in `parent`. */
  let editing = $state<{ path: string; parent: string; fresh: boolean; name: string; tags: string; color: string; icon: string; still: boolean } | null>(null);
  let exporting = $state<string | null>(null);
  let deleting = $state<string | null>(null);
  /** A folder whose many sessions wait for "open them all anyway?". */
  let openingAll = $state<string | null>(null);

  const parentOf = (p: string) => p.split("/").slice(0, -1).join("/");
  const nameOf = (p: string) => p.split("/").pop() ?? p;
  const countIn = (p: string) => app.store.session.filter((s) => inside(s.folder, p)).length;
  /** More than this many at once asks first. */
  const MANY = 16;
  function openAll(p: string, sure = false) {
    if (!sure && countIn(p) > MANY) return void (openingAll = p);
    openingAll = null;
    app.store.session.filter((s) => inside(s.folder, p)).forEach((s) => onopen(s));
  }

  /** Rename (moving everything inside along) and set the tags, colour and icon; or make a new folder. */
  function saveFolder() {
    const { path, parent, fresh, name, tags, color, icon, still } = editing!;
    const to = [parent, name.trim()].filter(Boolean).join("/");
    if (!fresh) moveFolder(path, to);
    const list = [...new Set(tags.split(",").map((t) => t.trim()).filter(Boolean))];
    const f = folderOf(to);
    if (f) Object.assign(f, { tags: list, color, icon, still });
    else app.store.folder.push({ path: to, tags: list, color, icon, still });
    // Only folders with something to remember, or nothing in them, are stored; the rest exist
    // through their sessions.
    app.store.folder = app.store.folder.filter((f) => f.tags.length || f.color || f.icon || f.order || !countIn(f.path));
    if (fresh) open[parent] = true;
    editing = null;
    saveStore();
  }

  /** Moves folder `from` to path `to`, its subfolders and sessions along; folders that end up on
   *  the same path become one. */
  function moveFolder(from: string, to: string) {
    const move = (p: string) => (inside(p, from) ? to + p.slice(from.length) : p);
    for (const s of app.store.session) s.folder = move(s.folder);
    for (const f of app.store.folder) f.path = move(f.path);
    for (const p of Object.keys(open)) if (inside(p, from) && open[p]) open[move(p)] = true;
    const merged = new Map<string, Folder>();
    for (const f of app.store.folder) {
      const m = merged.get(f.path);
      merged.set(f.path, m ? { ...m, tags: [...new Set([...m.tags, ...f.tags])], color: m.color || f.color, icon: m.icon || f.icon } : f);
    }
    app.store.folder = [...merged.values()];
  }

  function deleteFolder(path: string) {
    app.store.session = app.store.session.filter((s) => !inside(s.folder, path));
    app.store.folder = app.store.folder.filter((f) => !inside(f.path, path));
    deleting = null;
    saveStore();
  }

  const editFolder = (p: string) =>
    (editing = { path: p, parent: parentOf(p), fresh: false, name: nameOf(p), tags: tagsOf(p).join(", "), color: folderOf(p)?.color ?? "", icon: folderOf(p)?.icon ?? "", still: !!folderOf(p)?.still });
  const newFolder = (parent: string) => (editing = { path: "", parent, fresh: true, name: "", tags: "", color: "", icon: "", still: false });

  /** Copies a folder, its subfolders and their sessions next to it as "name (copy)". */
  function duplicateFolder(path: string) {
    let to = _("{name} (copy)", { name: path });
    for (let n = 2; folders.has(to); n++) to = _("{name} (copy {n})", { name: path, n });
    const move = (p: string) => to + p.slice(path.length);
    const sessions = app.store.session.filter((s) => inside(s.folder, path)).map((s) => ({ ...$state.snapshot(s), folder: move(s.folder) }));
    const copies = app.store.folder.filter((f) => inside(f.path, path)).map((f) => ({ ...$state.snapshot(f), path: move(f.path) }));
    if (!copies.some((f) => f.path === to)) copies.push({ path: to, tags: [], color: "", icon: "" });
    app.store.session.push(...sessions);
    app.store.folder.push(...copies);
    saveStore();
  }
  // ---- dragging: a session onto a folder moves it in, between sessions puts it there; a folder
  // onto another nests it there, onto its top or bottom edge puts it before or after, onto the
  // empty space moves it to the top level ----
  /** `folder`: where it would go (null: nowhere); `row`, `after`: next to that session. */
  let drag = $state<{ i: number; folder: string | null; row?: number; after?: boolean } | null>(null);
  /** The folder being dragged, and the folder it would go into or next to ("": the top level). */
  let fdrag = $state<{ p: string; to?: string; where?: "before" | "after" | "into" } | null>(null);
  /** A drag just ended: the click that follows doesn't open the session or folder. */
  let dragged = false;

  /** Follows a drag that starts at `e` once it has moved a little: `over` on each move, then
   *  `done(true)` on release, `done(false)` if cancelled. */
  function follow(e: PointerEvent, over: (m: PointerEvent) => void, done: (dropped: boolean) => void) {
    if (e.button !== 0 || filter) return;
    const el = e.currentTarget as HTMLElement;
    const [x0, y0] = [e.clientX, e.clientY];
    let moving = false;
    const move = (m: PointerEvent) => {
      if (!moving) {
        if (Math.hypot(m.clientX - x0, m.clientY - y0) < 6) return;
        el.setPointerCapture(m.pointerId);
        moving = dragged = true;
      }
      over(m);
    };
    const end = (u: PointerEvent) => {
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", end);
      removeEventListener("pointercancel", end);
      if (moving) done(u.type === "pointerup");
      setTimeout(() => (dragged = false));
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", end);
    addEventListener("pointercancel", end);
  }

  const grab = (e: PointerEvent, i: number) =>
    follow(
      e,
      (m) => {
        const under = document.elementFromPoint(m.clientX, m.clientY);
        const row = under?.closest<HTMLElement>("[data-session]");
        const dir = under?.closest<HTMLElement>("[data-folder]");
        if (row) {
          const [j, r] = [Number(row.dataset.session), row.getBoundingClientRect()];
          drag = { i, folder: app.store.session[j].folder, row: j, after: m.clientY > r.top + r.height / 2 };
        } else drag = { i, folder: dir ? dir.dataset.folder! : under?.closest(".list") ? "" : null };
      },
      (dropped) => {
        // A picked row takes the other picked ones along.
        if (dropped && drag && drag.folder !== null) {
          if (chosen.has("s" + i) && chosen.size > 1) moveChosen(drag.folder, drag.row, drag.after);
          else place(drag.i, drag.folder, drag.row, drag.after);
        }
        drag = null;
      },
    );

  const grabFolder = (e: PointerEvent, p: string) =>
    follow(
      e,
      (m) => {
        const under = document.elementFromPoint(m.clientX, m.clientY);
        const dir = under?.closest<HTMLElement>("[data-folder]");
        const to = dir ? dir.dataset.folder! : under?.closest(".list") && !under.closest("[data-session]") ? "" : null;
        // Not into itself or below itself, nor into where it already is.
        if (to === null || inside(to, p)) return void (fdrag = { p });
        const r = dir?.getBoundingClientRect();
        const y = r ? (m.clientY - r.top) / r.height : 0.5;
        const where = y < 0.3 ? "before" : y > 0.7 ? "after" : "into";
        fdrag = where === "into" && to === parentOf(p) ? { p } : { p, to, where };
      },
      (dropped) => {
        if (dropped && fdrag?.where) {
          const { to, where } = fdrag as { to: string; where: "before" | "after" | "into" };
          if (chosen.has("f" + p) && chosen.size > 1) moveChosen(where === "into" ? to : parentOf(to));
          else dropFolder(p, to, where);
        }
        fdrag = null;
      },
    );

  /** Moves folder `p` into folder `to`, or next to it (into its parent if that's elsewhere). */
  function dropFolder(p: string, to: string, where: "before" | "after" | "into") {
    const parent = where === "into" ? to : parentOf(to);
    let at = p;
    if (parent !== parentOf(p)) {
      at = [parent, nameOf(p)].filter(Boolean).join("/");
      const f = folderOf(p);
      if (f) f.order = 0; // its place was among its old siblings
      moveFolder(p, at);
    }
    if (where === "into") open[to] = true;
    else reorder(at, to, where === "after");
    saveStore();
  }

  /** Puts folder `p` before or after its sibling `to`, numbering all the siblings to keep the order. */
  function reorder(p: string, to: string, after: boolean) {
    const siblings = [...folders].filter((f) => f !== p && parentOf(f) === parentOf(p)).sort(byOrder);
    siblings.splice(siblings.indexOf(to) + (after ? 1 : 0), 0, p);
    siblings.forEach((path, i) => {
      const f = folderOf(path);
      if (f) f.order = i + 1;
      else app.store.folder.push({ path, tags: [], color: "", icon: "", order: i + 1 });
    });
  }

  /** Moves session `i` into `folder`: next to session `row` if given, else where it was. */
  function place(i: number, folder: string, row?: number, after?: boolean) {
    if (row === i) return;
    const list = app.store.session;
    const [s] = list.splice(i, 1);
    s.folder = folder;
    let to = row === undefined ? i : row + (after ? 1 : 0);
    if (row !== undefined && row > i) to--;
    list.splice(to, 0, s);
    if (folder) open[folder] = true;
    saveStore();
  }

  // ---- the keyboard: Tab stops once at the list, on the row used last; Up and Down walk the
  // rows, Right opens a folder (then steps into it), Left closes it (else steps out to the
  // folder above), Enter opens a session ----
  let lastRow: HTMLElement | null = null;
  const rows = () => [...list.querySelectorAll<HTMLElement>("[data-session], [data-folder]")];
  function enterList(e: FocusEvent) {
    if (!list.contains(e.relatedTarget as Element | null)) (lastRow?.isConnected ? lastRow : rows()[0])?.focus();
  }
  function walk(e: KeyboardEvent) {
    const row = e.target as HTMLElement;
    const all = rows();
    const at = all.indexOf(row);
    if (at < 0) return;
    // Shift+Tab leaves from the list itself, not from the rows above this one.
    if (e.key === "Tab" && e.shiftKey) return list.focus();
    const p = row.dataset.folder;
    const s = p === undefined ? app.store.session[Number(row.dataset.session)] : undefined;
    const rtl = getComputedStyle(list).direction === "rtl";
    const key = rtl ? ({ ArrowLeft: "ArrowRight", ArrowRight: "ArrowLeft" }[e.key] ?? e.key) : e.key;
    const go = (i: number) => all[Math.max(0, Math.min(all.length - 1, i))].focus();
    if (key === "ArrowDown") go(at + 1);
    else if (key === "ArrowUp") go(at - 1);
    else if (key === "Home") go(0);
    else if (key === "End") go(all.length - 1);
    else if (key === "ArrowRight" && p !== undefined) open[p] ? go(at + 1) : (open[p] = true);
    else if (key === "ArrowLeft" && !filter) {
      const up = p === undefined ? s!.folder : open[p] ? null : parentOf(p);
      if (up === null) open[p!] = false;
      else if (up) list.querySelector<HTMLElement>(`[data-folder="${CSS.escape(up)}"]`)?.focus();
    } else if (e.key === "Enter" && s) onopen(s);
    else if (e.key === "Escape" && chosen.size) (e.stopPropagation(), (chosen = new Set()));
    else if (e.key === "Delete" && chosen.size) deletingMany = picked();
    else return;
    e.preventDefault();
  }

  const newHere = (folder: string) => onedit(newSession("ssh", folder), null);
  const contextMenu = (e: MouseEvent, at: { i?: number; folder?: string; root?: boolean }) => {
    e.preventDefault();
    e.stopPropagation();
    // On a picked row, the menu is for all of them; on another, the pick is let go.
    const key = at.i !== undefined ? "s" + at.i : at.folder !== undefined ? "f" + at.folder : null;
    const many = !!key && chosen.has(key) && chosen.size > 1;
    if (!many && key) chosen = new Set();
    menu = { x: e.clientX, y: e.clientY, ...at, many };
  };
</script>

<svelte:window onclick={() => (menu = null)} />

{#snippet row(i: number, searching = false)}
  {@const s = app.store.session[i]}
  {@const look = lookOf(s)}
  {@const one = app.settings?.single_click_open}
  {@const on = live.get(sessionKey(s))}
  <button
    class="flat item"
    class:leaf={!searching}
    class:hit={searching && matches[sel] === i}
    class:before={drag?.row === i && !drag.after}
    class:after={drag?.row === i && drag.after}
    class:live={on === true}
    class:idle={on === false}
    class:chosen={chosen.has("s" + i)}
    data-session={i}
    tabindex="-1"
    title={details(s, on)}
    onpointerdown={(e) => grab(e, i)}
    onclick={(e) => !dragged && !choose(e, "s" + i) && one && onopen(s)}
    ondblclick={(e) => !one && !(e.ctrlKey || e.metaKey || e.shiftKey) && onopen(s)}
    oncontextmenu={(e) => contextMenu(e, { i })}
  >
    <SessionIcon icon={look.icon} color={look.color} still={look.still} /><span class="name" class:tinted={!!look.tint} style:--tint={look.tint || null}>{s.name}</span>
    {#if searching && s.folder}<span class="where">{s.folder}</span>{/if}
  </button>
{/snippet}

{#snippet folder(n: Node)}
  {#each n.children as [name, child] (name)}
    {@const p = child.path}
    {@const tags = tagsOf(p)}
    {@const icon = folderOf(p)?.icon || (open[p] ? "folder-open" : "folder")}
    {@const tint = folderColor(p)}
    <!-- a folder has no kind: its icon wears its mark, its lightness held like a marked name's (app.css `.tinted`) -->
    {@const iconTint = tint && `oklch(from ${tint} clamp(var(--tint-lo), l, var(--tint-hi)) c h)`}
    <button
      class="flat item"
      class:into={(drag?.folder === p && drag.row === undefined) || (fdrag?.to === p && fdrag.where === "into")}
      class:before={fdrag?.to === p && fdrag.where === "before"}
      class:after={fdrag?.to === p && fdrag.where === "after"}
      class:chosen={chosen.has("f" + p)}
      data-folder={p}
      tabindex="-1"
      aria-expanded={!!open[p]}
      aria-label={tags.length
        ? _("{folder}: {n} sessions, tagged {tags}", { folder: p.split("/").pop()!, n: child.count, tags: tags.join(", ") })
        : _("{folder}: {n} sessions", { folder: p.split("/").pop()!, n: child.count })}
      title={tags.length ? `Tags: ${tags.join(", ")}` : p}
      onpointerdown={(e) => grabFolder(e, p)}
      onclick={(e) => !dragged && !choose(e, "f" + p) && (open[p] = !open[p])}
      oncontextmenu={(e) => contextMenu(e, { folder: p })}
    >
      <span class="caret" class:down={open[p]}><Icon name="chevron-right" size={14} /></span><SessionIcon {icon} color={iconTint} still={folderOf(p)?.still} /><span class="name grow" class:tinted={!!tint} style:--tint={tint || null}>{name}</span>
      {#each tags.slice(0, 2) as t (t)}<span class="tag">{t}</span>{/each}
      {#if tags.length > 2}<span class="tag">+{tags.length - 2}</span>{/if}
      <span class="count">{child.count}</span>
    </button>
    {#if open[p]}
      <div class="indent">{@render folder(child)}</div>
    {/if}
  {/each}
  {#each n.items as i (i)}{@render row(i)}{/each}
{/snippet}

<nav>
  <!-- Searching, the field takes the buttons' place; it closes when left empty. -->
  <div class="top">
    {#if searching}
      <label class="search">
        <Icon name="search" />
        <input
          bind:this={search}
          bind:value={filter}
          oninput={() => (sel = 0)}
          onkeydown={searchKey}
          onblur={() => !filter && closeSearch()}
          aria-label={_("Search sessions")}
          placeholder={_("Name, address, folder or tag")}
          title={_("Find a session by name, address, folder or tag")}
        />
      </label>
    {:else}
      <div class="actions">
        <button class="flat icon-only find" onclick={focusSearch} title={tip(_("Search sessions"), "find")} aria-label={_("Search sessions")}><Icon name="search" size={16} /></button>
        {@render actions?.()}
      </div>
    {/if}
  </div>
  {#if filter && !matches.length}<p class="weak none">{_("Nothing matches “{text}”.", { text: filter })}</p>{/if}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_no_noninteractive_tabindex -->
  <div
    class="list"
    class:into={(drag?.folder === "" && drag.row === undefined) || fdrag?.to === ""}
    bind:this={list}
    tabindex="0"
    aria-label={_("Sessions")}
    onfocus={enterList}
    onfocusin={(e) => e.target !== list && (lastRow = e.target as HTMLElement)}
    onkeydown={walk}
    oncontextmenu={(e) => contextMenu(e, { root: true })}
  >
    {#if filter}
      {#each matches.slice(0, SHOWN) as i (i)}{@render row(i, true)}{/each}
      {#if matches.length > SHOWN}<p class="weak none">{_("{n} more: type more to narrow it down.", { n: matches.length - SHOWN })}</p>{/if}
    {:else}
      {@render folder(tree)}
    {/if}
  </div>
</nav>

{#if menu?.many}
  {@const p = picked()}
  <div class="menu" {@attach menuAt(() => (menu = null), menu.x, menu.y)}>
    {#if elsewhere.length}<button onclick={() => (moving = p)}><Icon name="cluster" />{_("Move to workspace…")}</button>{/if}
    <button onclick={() => (chosen = new Set())}>{_("Clear selection")}</button>
    <button class="danger" onclick={() => (deletingMany = p)}><Icon name="trash" />{_("Delete…")}</button>
  </div>
{:else if menu?.i !== undefined}
  {@const s = app.store.session[menu.i]}
  {@const i = menu.i}
  <div class="menu" {@attach menuAt(() => (menu = null), menu.x, menu.y)}>
    <button onclick={() => onopen(s)}><Icon name="play" />{_("Open")}</button>
    {#if s.kind.type === "browser"}
      {@const url = s.kind.url}
      <button onclick={() => openUrl(url)}><Icon name="globe" />{_("Open in system browser")}</button>
    {/if}
    <button onclick={() => onedit(s, i)}><Icon name="edit" />{_("Edit")}</button>
    <button onclick={() => onedit({ ...$state.snapshot(s), name: _("{name} (copy)", { name: s.name }) }, null)}><Icon name="copy" />{_("Duplicate")}</button>
    {#if elsewhere.length}<button onclick={() => (moving = { sessions: [i], folders: [] })}><Icon name="cluster" />{_("Move to workspace…")}</button>{/if}
    <button class="danger" onclick={() => remove(i)}><Icon name="trash" />{_("Delete")}</button>
  </div>
{:else if menu?.folder !== undefined}
  {@const p = menu.folder}
  <div class="menu" {@attach menuAt(() => (menu = null), menu.x, menu.y)}>
    <button onclick={() => openAll(p)} disabled={!countIn(p)}><Icon name="play" />{_("Open all ({n})", { n: countIn(p) })}</button>
    <button onclick={() => editFolder(p)}><Icon name="edit" />{_("Edit folder")}</button>
    <button onclick={() => newHere(p)}><Icon name="plus" />{_("New session here")}</button>
    <button onclick={() => newFolder(p)}><Icon name="folder-plus" />{_("New folder here")}</button>
    <button onclick={() => duplicateFolder(p)}><Icon name="copy" />{_("Duplicate folder")}</button>
    {#if elsewhere.length}<button onclick={() => (moving = { sessions: [], folders: [p] })}><Icon name="cluster" />{_("Move to workspace…")}</button>{/if}
    <button onclick={() => (exporting = p)}><Icon name="download" />{_("Export")}</button>
    <button class="danger" onclick={() => (deleting = p)}><Icon name="trash" />{_("Delete folder")}</button>
  </div>
{:else if menu?.root}
  <div class="menu" {@attach menuAt(() => (menu = null), menu.x, menu.y)}>
    <button onclick={() => newHere("")}><Icon name="plus" />{_("New session")}</button>
    <button onclick={() => newFolder("")}><Icon name="folder-plus" />{_("New folder")}</button>
  </div>
{/if}

{#if editing}
  {@const ed = editing}
  {@const clash = !ed.name.trim() || ed.name.includes("/")}
  <Modal title={ed.fresh ? _("New folder") : _("Folder")} width={610} onclose={() => (editing = null)}>
    <div class="form">
      <span>{_("Name")}</span>
      <input bind:value={ed.name} {@attach (el) => el.focus()} />
      <span>{_("Tags")}</span>
      <input bind:value={ed.tags} placeholder={_("production, eu-west, customer-a")} />
      <Appearance bind:color={ed.color} bind:icon={ed.icon} bind:still={ed.still} fallbackIcon="folder" fallbackColor={folderColor(ed.parent) || "var(--fg)"} name={ed.name} />
    </div>
    <p class="weak">
      {_("Separate tags with commas. Searching for a tag finds every session in the folder, including subfolders. Sessions without a colour of their own take the folder's.")}
    </p>
    <div class="row">
      <button class="primary" disabled={clash} onclick={saveFolder}>{ed.fresh ? _("Create") : _("Save")}</button>
      <button onclick={() => (editing = null)}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

{#if exporting !== null}<Export folder={exporting} onclose={() => (exporting = null)} />{/if}

{#if openingAll !== null}
  {@const p = openingAll}
  {@const n = countIn(p)}
  <Modal title={_("Open all {n} sessions?", { n })} width={377} onclose={() => (openingAll = null)}>
    <p>
      {_("That's a lot at once: each session is a connection and uses memory. xtermium may slow down, and servers may block that many logins arriving together.")}
    </p>
    <div class="row">
      <button class="primary" onclick={() => openAll(p, true)}>{_("Open all {n}", { n })}</button>
      <button {@attach (el) => el.focus()} onclick={() => (openingAll = null)}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

{#if deleting !== null}
  {@const p = deleting}
  <Modal title={_("Delete {name}?", { name: nameOf(p) })} width={377} onclose={() => (deleting = null)}>
    <p>{_("This deletes the folder, its subfolders and the {n} sessions in them. Export it first if you might want them back.", { n: countIn(p) })}</p>
    <div class="row">
      <button class="primary danger" onclick={() => deleteFolder(p)}>{_("Delete folder")}</button>
      <!-- it can't be undone: Enter cancels -->
      <button {@attach (el) => el.focus()} onclick={() => (deleting = null)}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

{#if deletingMany}
  {@const p = deletingMany}
  <Modal title={_("Delete {n} sessions?", { n: countPicked(p) })} width={377} onclose={() => (deletingMany = null)}>
    <p>
      {p.folders.length
        ? _("This deletes the picked sessions, and the {n} picked folders with everything in them. Export them first if you might want them back.", { n: p.folders.length })
        : _("This deletes the picked sessions.")}
    </p>
    <div class="row">
      <button class="primary danger" onclick={deleteChosen}>{_("Delete")}</button>
      <button {@attach (el) => el.focus()} onclick={() => (deletingMany = null)}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

{#if moving}
  {@const n = countPicked(moving)}
  <Modal title={_("Move {n} sessions to", { n })} width={377} onclose={() => (moving = null)}>
    <div class="choices">
      {#each elsewhere as w (w)}
        <button class="flat ws" onclick={() => toWorkspace(w)}><Icon name="cluster" />{wsName(w)}</button>
      {/each}
    </div>
    <p class="weak">{_("Folders keep their place, and the shared logins the sessions use are copied along. Open tabs stay open.")}</p>
  </Modal>
{/if}

<style>
  nav {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 8px 5px 5px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 0 5px;
  }
  .search {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    color: var(--weak);
  }
  /* search on its own at the start, the other buttons at the end */
  .actions {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .find {
    margin-inline-end: auto;
  }
  /* the field opens out from the search button */
  .search {
    animation: open-search 0.14s ease-out;
  }
  @keyframes open-search {
    from {
      max-width: 28px;
    }
    to {
      max-width: 100%;
    }
  }
  .search :global(.icon) {
    position: absolute;
    inset-inline-start: 8px;
    pointer-events: none;
  }
  .search input {
    width: 100%;
    padding-inline-start: calc(8px + 16px + 5px);
  }
  .none {
    margin: 5px 8px;
  }
  .list {
    flex: 1;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  /* where a dragged session or folder would land */
  .into {
    background: color-mix(in srgb, var(--gold) 18%, transparent);
  }
  .item.before {
    box-shadow: inset 0 2px 0 var(--gold);
  }
  .item.after {
    box-shadow: inset 0 -2px 0 var(--gold);
  }
  .item {
    flex: none;
    justify-content: flex-start;
    min-height: var(--row);
    padding: 0 5px;
    padding-inline-start: var(--lead);
    gap: 5px;
    text-align: start;
    /* rows out of view aren't laid out or drawn: a folder of thousands opens at once */
    content-visibility: auto;
    contain-intrinsic-size: auto var(--row);
    /* room before the row's first thing: a folder's caret, or a session's icon */
    --lead: 5px;
  }
  /* In the tree a session leaves room for the caret folders have, so the icons of one level
     line up, and each level sits one caret further in (VS Code's explorer does the same). */
  .item.leaf {
    --lead: calc(5px + 14px + 5px);
  }
  /* a session with a tab open: a dot on its icon's corner, hollow once the connection is gone */
  .item:is(.live, .idle) {
    position: relative;
  }
  .item:is(.live, .idle)::after {
    content: "";
    position: absolute;
    inset-inline-start: calc(var(--lead) + 18px - 6px); /* over the icon's corner */
    top: 50%;
    translate: 0 2px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 0 1.5px rgb(var(--panel-rgb));
  }
  .item.idle::after {
    background: rgb(var(--panel-rgb));
    border: 1.5px solid var(--weak);
  }
  .list:focus {
    outline: none;
  }
  .item.hit,
  .item.chosen {
    background: var(--gold-soft);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ws {
    justify-content: flex-start;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where {
    margin-inline-start: auto;
    padding-inline-start: 8px;
    font-size: var(--t-s);
    color: var(--weak);
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 1;
  }
  .count {
    font-size: var(--t-s);
    color: var(--weak);
    font-variant-numeric: tabular-nums;
  }
  /* a guide line shows which folder the rows belong to */
  .indent {
    display: flex;
    flex-direction: column;
    /* under the middle of the folder's caret */
    margin-inline-start: calc(5px + 7px - 1px);
    border-inline-start: 1px solid var(--line);
  }
  .caret {
    display: inline-flex;
    transition: transform 0.12s;
    color: var(--weak);
  }
  .caret.down {
    transform: rotate(90deg);
  }
  /* closed folders point the way the text runs */
  :global([dir="rtl"]) .caret:not(.down) {
    transform: scaleX(-1);
  }
</style>
