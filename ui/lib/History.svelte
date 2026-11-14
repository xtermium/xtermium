<script lang="ts">
  // Session history: the past conversations of an AI agent tab's tool in its working folder, from
  // the tool's own session files (src/agent_history.rs). A click goes on with one in a new tab; a
  // row can be renamed (kept by xtermium, the tool's file untouched) or deleted (the tool's file).
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { here, human, type Session } from "./api";
  import { toast } from "./state.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  /** A past conversation as the backend lists it. */
  type Conversation = { id: string; title: string; created: number; used: number; bytes: number; path: string };

  let { session, title, onresume }: { session: Session; title: string; onresume: (s: Session) => void } = $props();
  const kind = $derived(session.kind.type === "agent" ? session.kind : null);

  let list = $state<Conversation[] | null>(null);
  let error = $state("");
  let filter = $state("");
  /** The row being renamed and the name typed so far. */
  let renaming = $state<Conversation | null>(null);
  let newTitle = $state("");
  let deleting = $state<Conversation | null>(null);

  async function load() {
    if (!kind) return;
    const [tool, dir] = [kind.tool, here(kind.dir)];
    try {
      list = await invoke<Conversation[]>("agent_history", { tool, dir });
      error = "";
    } catch (e) {
      list = [];
      error = String(e);
    }
  }
  // Again whenever the tab's tool or folder changes (another agent tab focused).
  $effect(() => {
    if (!kind) return;
    void [kind.tool, kind.dir];
    list = null;
    load();
  });

  const shown = $derived((list ?? []).filter((c) => !filter.trim() || (c.title || c.id).toLowerCase().includes(filter.trim().toLowerCase())));

  /** A moment as "2 h ago", "3 d ago", or the date once it is far. */
  function ago(ms: number): string {
    if (!ms) return "";
    const s = Math.max(0, (Date.now() - ms) / 1000);
    if (s < 60) return _("just now");
    if (s < 3600) return _("{n} min ago", { n: Math.floor(s / 60) });
    if (s < 86400) return _("{n} h ago", { n: Math.floor(s / 3600) });
    if (s < 7 * 86400) return _("{n} d ago", { n: Math.floor(s / 86400) });
    return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }
  const when = (ms: number) => (ms ? new Date(ms).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "");

  function resume(c: Conversation) {
    if (!kind) return;
    onresume({ ...session, name: c.title || title, kind: { ...kind, resume: c.id } });
  }

  async function rename() {
    if (!renaming) return;
    const c = renaming;
    renaming = null;
    try {
      await invoke("agent_history_rename", { path: c.path, title: newTitle });
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function remove() {
    if (!deleting || !kind) return;
    const c = deleting;
    deleting = null;
    try {
      await invoke("agent_history_delete", { tool: kind.tool, path: c.path });
      toast(_("Conversation deleted."));
      await load();
    } catch (e) {
      error = String(e);
    }
  }
</script>

<section>
  <div class="row head">
    <Icon name="history" />
    <h3>{_("Session history")}</h3>
    <button class="icon-only" title={_("Refresh")} onclick={load}><Icon name={list === null ? "loader-alt" : "refresh"} spin={list === null} /></button>
  </div>
  <p class="weak">{_("Past conversations of {title} in {dir}. Click one to go on with it in a new tab.", { title, dir: (kind && here(kind.dir)) || _("your home folder") })}</p>
  {#if (list?.length ?? 0) > 8}
    <input bind:value={filter} placeholder={_("Find a conversation")} />
  {/if}
  {#if error}<div class="error">{error}</div>{/if}
  {#if list === null}
    <div class="row weak"><Icon name="loader-alt" spin /> {_("Looking…")}</div>
  {:else if !list.length}
    <p class="weak">{_("Nothing yet: the first conversation shows up here once the tool has saved it.")}</p>
  {:else}
    <ul>
      {#each shown as c (c.path)}
        <li>
          {#if renaming === c}
            <input
              bind:value={newTitle}
              placeholder={_("A name for it (empty: the tool's own)")}
              onkeydown={(e) => (e.key === "Enter" ? rename() : e.key === "Escape" ? (renaming = null) : null)}
              {@attach (el) => (el as HTMLInputElement).select()}
            />
          {:else}
            <button class="flat item title" title={c.title || c.id} onclick={() => resume(c)}>{c.title || c.id}</button>
          {/if}
          <span class="meta" title={_("Started {created}, last used {used}", { created: when(c.created), used: when(c.used) })}>
            {when(c.created)} · {ago(c.used)} · {human(c.bytes)}
          </span>
          <span class="tools">
            <button class="icon-only" title={_("Go on with it in a new tab")} onclick={() => resume(c)}><Icon name="play" /></button>
            <button class="icon-only" title={_("Rename")} onclick={() => ((newTitle = c.title), (renaming = c))}><Icon name="edit" /></button>
            <button class="icon-only danger" title={_("Delete")} onclick={() => (deleting = c)}><Icon name="trash" /></button>
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if deleting}
  <Modal title={_("Delete this conversation?")} onclose={() => (deleting = null)}>
    <p>{_("{title} goes from the tool's own files; this can't be undone.", { title: deleting.title || deleting.id })}</p>
    <div class="row">
      <button class="danger" onclick={remove}>{_("Delete")}</button>
      <button onclick={() => (deleting = null)} {@attach (el) => el.focus()}>{_("Cancel")}</button>
    </div>
  </Modal>
{/if}

<style>
  section {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 8px 8px 0;
    min-height: 0;
    overflow: hidden;
  }
  .head h3 {
    flex: 1;
    margin: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    min-height: 0;
  }
  li {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: "title tools" "meta tools";
    align-items: center;
    column-gap: var(--gap);
    padding: 3px 5px;
    border-radius: var(--r-control);
  }
  li:hover {
    background: var(--raised);
  }
  li .title {
    grid-area: title;
    justify-self: start;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0;
    height: auto;
    min-height: var(--row);
  }
  li input {
    grid-area: title;
  }
  .meta {
    grid-area: meta;
    color: var(--weak);
    font-size: var(--t-s);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tools {
    grid-area: tools;
    display: flex;
    gap: 1px;
    opacity: 0;
  }
  li:hover .tools,
  li:focus-within .tools {
    opacity: 1;
  }
</style>
