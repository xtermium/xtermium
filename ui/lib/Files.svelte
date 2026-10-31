<script lang="ts">
  // File panel for a tab on screen: SFTP for SSH tabs (and VNC tabs, over an SSH connection of
  // their own to the desktop's host), the Windows host's shares (SMB) for RDP tabs. The backend publishes its whole state as `files` events; this sends commands. Files
  // dropped on the window upload to the current folder. An RDP tab's panel connects as the
  // desktop's login once that's in (Settings > Defaults, or the session's own choice), else
  // after the user picks the account: the RDP login or another one.
  import { _ } from "./i18n.svelte";
  import { onDestroy, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { host, human, type Session } from "./api";
  import Icon from "./Icon.svelte";
  import SessionIcon from "./SessionIcon.svelte";
  import { app, lookOf } from "./state.svelte";
  import Modal from "./Modal.svelte";
  import { menu as menuAt } from "./menu";

  type Entry = { name: string; dir: boolean; size: number; date: string };
  type Transfer = { upload: boolean; label: string; done: number; total: number; ok: boolean | null; message: string };
  type Snapshot = { tab: number; connected: boolean; cwd: string; entries: Entry[]; error: string; transfers: Transfer[] };

  /** `rdp.connected`: the desktop is up, so its login is known. `cwd`: the shell's folder as the
   *  terminal reports it (OSC 7; "" when it doesn't), followed as it changes. */
  let { tab, session, rdp, cwd = "" }: { tab: number; session: Session; rdp: { host: string; user: string; connected: boolean } | null; cwd?: string } = $props();
  let panel: HTMLElement;

  let snap = $state<Snapshot | null>(null);
  /** RDP: false while the account is being picked. */
  let opened = $state(true);
  let login = $state({ who: "rdp", user: "", password: "", error: "" });
  let path = $state("");
  let editingPath = false;
  let dropping = $state(false);
  let menu = $state<{ x: number; y: number; e: Entry } | null>(null);
  let rename = $state<{ from: string; to: string } | null>(null);
  let mkdir = $state<string | null>(null);

  const cmd = (c: object) => invoke("files", { id: tab, cmd: c });
  const failed = (e: unknown) => (snap = { tab, connected: false, cwd: "", entries: [], error: String(e), transfers: [] });

  $effect(() => {
    snap = null;
    invoke<boolean>("files_open", { id: tab }).then((ok) => (opened = ok), failed);
  });

  /** The session turned its shared folders off (its settings › Advanced): the panel says so. */
  const off = $derived(session.kind.type === "rdp" && !!session.kind.no_shares);
  /** Open the shares with the desktop's login without asking, once (a failure shows the picker). */
  const auto = $derived(session.kind.type === "rdp" && !off && (session.kind.smb_login ?? app.settings?.smb_auto_login ?? true));
  let tried = false;
  $effect(() => {
    if (rdp?.connected && !opened && auto && !tried) {
      tried = true;
      login.who = "rdp";
      connect();
    }
  });

  async function connect() {
    const other = login.who === "other";
    try {
      await invoke("smb_connect", { id: tab, user: other ? login.user : null, password: other ? login.password : "" });
      [opened, snap, login.password, login.error] = [true, null, "", ""];
    } catch (e) {
      login.error = String(e);
    }
  }

  /** The shell's folder last followed: the panel goes where the shell goes, and stays where you
   *  browse to in it until the shell moves again. */
  let followed = "";
  function follow() {
    if (cwd && cwd !== followed && snap?.connected) {
      followed = cwd;
      cmd({ op: "cd", path: cwd });
    }
  }
  $effect(() => {
    void cwd;
    untrack(follow);
  });

  const unlisten = [
    listen<Snapshot>("files", (e) => {
      if (e.payload.tab !== tab) return;
      const was = snap?.connected;
      snap = e.payload;
      if (!editingPath) path = snap.cwd;
      if (snap.connected && !was) follow();
    }),
    getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      // With several panels open, the drop goes to the one under the pointer.
      const r = panel.getBoundingClientRect();
      const [x, y] = "position" in p ? [p.position.x / devicePixelRatio, p.position.y / devicePixelRatio] : [-1, -1];
      const over = x >= r.left && x < r.right && y >= r.top && y < r.bottom;
      dropping = over && (p.type === "enter" || p.type === "over");
      if (p.type === "drop" && over && snap?.connected) cmd({ op: "upload", paths: p.paths });
    }),
  ];
  onDestroy(() => unlisten.forEach((u) => u.then((f) => f())));

  const parent = (p: string) => p.replace(/\/+$/, "").replace(/\/[^/]*$/, "") || "/";
  const join = (d: string, n: string) => (d.endsWith("/") ? d + n : `${d}/${n}`);

  function open(e: Entry) {
    if (e.dir) cmd({ op: "cd", path: join(snap!.cwd, e.name) });
    else cmd({ op: "download", name: e.name, size: e.size });
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<section class:dropping bind:this={panel}>
  <div class="row"><Icon name="folder-open" /><h3>{rdp ? _("Files on {host}", { host: rdp.host }) : session.kind.type === "vnc" ? _("SFTP on {host}", { host: session.kind.host }) : "SFTP"}</h3></div>
  {#if rdp && off}
    <p class="weak">{_("Shared folders are off for this session: its settings › Advanced turns them back on.")}</p>
  {:else if rdp && !opened}
    <p>{_("Browse the shared folders on {host} (SMB, port 445).", { host: rdp.host })}</p>
    <div class="col">
      <label class="check"><input type="radio" bind:group={login.who} value="rdp" />{_("As the remote desktop login")}{rdp.user ? ` (${rdp.user})` : ""}</label>
      <label class="check"><input type="radio" bind:group={login.who} value="other" />{_("As another account")}</label>
    </div>
    {#if login.who === "other"}
      <input bind:value={login.user} placeholder={"DOMAIN\\user"} {@attach (el) => el.focus()} />
      <input type="password" bind:value={login.password} placeholder={_("password")} onkeydown={(e) => e.key === "Enter" && login.user && connect()} />
    {/if}
    {#if login.error}<div class="error">{login.error}</div>{/if}
    <div class="row"><button class="primary" disabled={login.who === "other" && !login.user} onclick={connect}>{_("Connect")}</button></div>
  {:else if !snap || !snap.connected}
    {#if snap?.error}
      <div class="error">{snap.error}</div>
      {#if rdp}<div class="row"><button onclick={() => (opened = false)}>{_("Use another account")}</button></div>{/if}
    {:else}
      <div class="row"><Icon name="loader-alt" spin /> {rdp ? _("connecting…") : _("opening SFTP…")}</div>
    {/if}
  {:else}
    <div class="row">
      <button class="icon-only" title={_("Parent folder")} onclick={() => cmd({ op: "cd", path: parent(snap!.cwd) })}><Icon name="up-arrow-alt" /></button>
      <button class="icon-only" title={_("Refresh")} onclick={() => cmd({ op: "cd", path: snap!.cwd })}><Icon name="refresh" /></button>
      <button class="icon-only" title={_("New folder")} onclick={() => (mkdir = "")}><Icon name="folder-plus" /></button>
    </div>
    <input
      bind:value={path}
      onfocus={() => (editingPath = true)}
      onblur={() => ((editingPath = false), (path = snap!.cwd))}
      onkeydown={(e) => e.key === "Enter" && (cmd({ op: "cd", path }), (e.target as HTMLInputElement).blur())}
    />
    {#if snap.error}
      <div class="row error">
        <span class="grow">{snap.error}</span>
        <button class="flat icon-only" title={_("Dismiss")} onclick={() => cmd({ op: "dismiss" })}><Icon name="x" /></button>
      </div>
    {/if}
    {#if dropping}
      <div class="row drop"><Icon name="upload" /> {_("Drop to upload to {path}", { path: snap.cwd })}</div>
    {/if}
    <div class="list">
      {#each snap.entries as e (e.name)}
        <button
          class="flat entry"
          ondblclick={() => open(e)}
          oncontextmenu={(ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            menu = { x: ev.clientX, y: ev.clientY, e };
          }}
        >
          <Icon name={e.dir ? "folder" : "file"} />
          <span class="name">{e.name}</span>
          <span class="weak">{e.dir ? "" : human(e.size)}</span>
          <span class="weak date">{e.date}</span>
        </button>
      {/each}
    </div>
    {#if snap.transfers.length}
      <div class="row"><strong class="grow">{_("Transfers")}</strong><button onclick={() => cmd({ op: "clear" })}>{_("Clear finished")}</button></div>
      {#each snap.transfers as t, i (i)}
        <div class="row transfer">
          {#if t.ok === null}
            <Icon name="loader-alt" spin />
            <Icon name={t.upload ? "upload" : "download"} />
            <div class="grow">
              <div>{t.label}: {_("{done} of {total}", { done: human(t.done), total: human(t.total) })}</div>
              <progress max={t.total || 1} value={t.done}></progress>
            </div>
          {:else if t.ok}
            <Icon name="check-circle" color="var(--ok)" /><span class="grow">{t.label} {t.message}</span>
          {:else}
            <Icon name="x-circle" color="var(--error)" /><span class="grow error">{t.label}: {t.message}</span>
          {/if}
        </div>
      {/each}
    {/if}
  {/if}
  <footer class="who row" title={session.name}>
    <SessionIcon icon={lookOf(session).icon} color={lookOf(session).color} size={13} />{session.name}<span class="weak">{host(session.kind)}</span>
  </footer>
</section>

{#if menu}
  {@const e = menu.e}
  <div class="menu" {@attach menuAt(() => (menu = null), menu.x, menu.y)}>
    {#if !e.dir}<button onclick={() => cmd({ op: "download", name: e.name, size: e.size })}><Icon name="download" />{_("Download")}</button>{/if}
    <button onclick={() => (rename = { from: e.name, to: e.name })}><Icon name="edit" />{_("Rename")}</button>
    <button class="danger" onclick={() => cmd({ op: "delete", name: e.name, dir: e.dir })}><Icon name="trash" />{_("Delete")}</button>
  </div>
{/if}

{#if rename}
  {@const r = rename}
  <Modal title={_("Rename")} onclose={() => (rename = null)}>
    <input bind:value={r.to} {@attach (el) => el.focus()} onkeydown={(e) => e.key === "Enter" && (cmd({ op: "rename", from: r.from, to: r.to }), (rename = null))} />
    <div class="row">
      <button class="primary" onclick={() => (cmd({ op: "rename", from: r.from, to: r.to }), (rename = null))}>{_("Rename")}</button>
      <button onclick={() => (rename = null)}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

{#if mkdir !== null}
  <Modal title={_("New folder")} onclose={() => (mkdir = null)}>
    <input bind:value={mkdir} {@attach (el) => el.focus()} onkeydown={(e) => e.key === "Enter" && mkdir && (cmd({ op: "mkdir", name: mkdir }), (mkdir = null))} />
    <div class="row">
      <button class="primary" disabled={!mkdir} onclick={() => (cmd({ op: "mkdir", name: mkdir }), (mkdir = null))}>{_("Create")}</button>
      <button onclick={() => (mkdir = null)}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

<style>
  section {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 8px 8px 0;
    min-height: 0;
    overflow: hidden;
  }
  /* panels stacked for several sessions */
  section:not(:first-child) {
    border-top: 1px solid var(--line);
  }
  .who {
    flex: none;
    gap: 5px;
    height: var(--row);
    margin: auto -8px 0;
    padding: 0 8px;
    font-size: var(--t-s);
    border-top: 1px solid var(--line);
    white-space: nowrap;
    overflow: hidden;
  }
  section.dropping {
    outline: 1px solid var(--gold);
    outline-offset: -2px;
  }
  .drop {
    font-weight: 600;
  }
  .list {
    flex: 1;
    overflow: auto;
    display: flex;
    flex-direction: column;
    min-height: 120px;
  }
  .entry {
    display: grid;
    grid-template-columns: 16px 1fr auto auto;
    gap: 8px;
    text-align: start;
    min-height: var(--row);
    padding: 0 5px;
    /* entries out of view aren't laid out or drawn: big folders list at once */
    content-visibility: auto;
    contain-intrinsic-size: auto 22px;
  }
  .entry:nth-child(odd) {
    background: rgba(127, 127, 127, 0.06);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .date {
    font-size: var(--t-s);
  }
  .transfer progress {
    width: 100%;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  p {
    margin: 0;
  }
</style>
