<script lang="ts">
  import { _, N_ } from "./i18n.svelte";
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { newEndpoint, type Tunnel, type TunnelKind, type TunnelStats } from "./api";
  import Endpoint from "./Endpoint.svelte";
  import Flow from "./Flow.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { app, saveStore } from "./state.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let stats = $state<Record<string, TunnelStats>>({});
  let error = $state("");
  const blank = (): Tunnel => ({ name: "", kind: "Local", ssh: newEndpoint(), bind: "127.0.0.1", listen_port: 0, dest_host: "", dest_port: 0 });
  let draft = $state(blank());
  /** The tunnel being edited (its index), or null for a new one. */
  let editing = $state<number | null>(null);
  let formOpen = $state(false);

  async function poll() {
    stats = await invoke<Record<string, TunnelStats>>("tunnel_stats");
  }
  poll();
  const timer = setInterval(poll, 1000);
  onDestroy(() => clearInterval(timer));

  async function start(name: string) {
    error = "";
    await invoke("tunnel_start", { name }).catch((e) => (error = String(e)));
    poll();
  }

  async function stop(name: string) {
    await invoke("tunnel_stop", { name });
    poll();
  }

  /** Why the draft can't be added yet; "" when it can. */
  const problem = $derived.by(() => {
    const name = draft.name.trim();
    if (editing !== null && stats[app.store.tunnel[editing]?.name]?.live) return _("The tunnel is running: stop it to save the changes.");
    if (!name) return _("Give the tunnel a name.");
    if (app.store.tunnel.some((t, i) => t.name === name && i !== editing)) return _("Another tunnel has that name.");
    if (!(draft.listen_port >= 1 && draft.listen_port <= 65535)) return _("Choose a listen port (1–65535).");
    if (draft.kind !== "Dynamic" && (!draft.dest_host.trim() || !(draft.dest_port >= 1 && draft.dest_port <= 65535)))
      return _("Enter the destination host and port.");
    if (!draft.ssh.host.trim()) return _("Enter the SSH server to tunnel through.");
    return "";
  });

  function add() {
    const t = $state.snapshot(draft);
    if (editing === null) app.store.tunnel.push(t);
    else app.store.tunnel[editing] = t;
    saveStore();
    [draft, editing] = [blank(), null];
  }

  /** Only a stopped tunnel can be edited: a running one would keep its old settings. */
  function edit(i: number) {
    const t = app.store.tunnel[i];
    if (stats[t.name]?.live) return void (error = _("{name} is running: stop it first, then edit it.", { name: t.name }));
    error = "";
    [draft, editing, formOpen] = [$state.snapshot(t), i, true];
  }

  function cancelEdit() {
    [draft, editing] = [blank(), null];
  }

  function remove(i: number) {
    app.store.tunnel.splice(i, 1);
    saveStore();
  }
</script>

<Modal title={_("SSH tunnels")} width={987} {onclose}>
  {#if error}<div class="error">{error}</div>{/if}
  {#if !app.store.tunnel.length}<div class="weak">{_("No tunnels yet. Add one below.")}</div>{/if}
  {#each app.store.tunnel as t, i (t.name)}
    {@const s = stats[t.name]}
    <section class="card">
      <div class="row">
        {#if s?.live}<Icon name="radio-circle-marked" color="var(--back)" pulse />{:else}<Icon name="transfer-alt" />{/if}
        <h3>{t.name}</h3>
        <span class="tag">{_(t.kind)}</span>
        <span class="grow"></span>
        {#if s}
          <!-- "connecting…", and "connection lost; connecting again…" while it reconnects -->
          {#if s.live && s.status.startsWith("connect")}<Icon name="loader-alt" spin />{/if}
          <span class="weak status">{s.status}</span>
        {/if}
        {#if s?.live}
          <button onclick={() => stop(t.name)}><Icon name="stop" />{_("Stop")}</button>
        {:else}
          <button onclick={() => start(t.name)}><Icon name="play" />{_("Start")}</button>
        {/if}
        <button class="flat icon-only" title={s?.live ? _("Stop the tunnel to edit it") : _("Edit")} onclick={() => edit(i)}><Icon name="edit" /></button>
        {#if !s?.live}
          <button class="flat icon-only" title={_("Delete")} onclick={() => remove(i)}><Icon name="trash" /></button>
        {/if}
      </div>
      <Flow {t} stats={s} />
    </section>
  {/each}

  <details bind:open={formOpen}>
    <summary><strong>{editing === null ? _("New tunnel") : _("Edit {name}", { name: app.store.tunnel[editing]?.name ?? "" })}</strong></summary>
    <div class="preview"><Flow t={draft} demo /></div>
    <div class="form">
      <span>{_("Name")}</span><input bind:value={draft.name} />
      <span>{_("Type")}</span>
      <div class="row">
        {#each [N_("Local"), N_("Remote"), N_("Dynamic")] as TunnelKind[] as k (k)}
          <button class:selected={draft.kind === k} onclick={() => (draft.kind = k)}>{_(k)}</button>
        {/each}
      </div>
      <span>{_("Listen on (address / port)")}</span>
      <div class="row">
        <input bind:value={draft.bind} style:width="150px" />
        <input type="number" min="1" max="65535" placeholder={_("port")} bind:value={() => draft.listen_port || null, (v) => (draft.listen_port = v ?? 0)} />
      </div>
      {#if draft.kind !== "Dynamic"}
        <span>{_("Destination")}</span>
        <div class="row">
          <input bind:value={draft.dest_host} placeholder={_("host")} style:width="200px" />
          <input type="number" min="1" max="65535" placeholder={_("port")} bind:value={() => draft.dest_port || null, (v) => (draft.dest_port = v ?? 0)} />
        </div>
      {/if}
      <Endpoint e={draft.ssh} label={_("SSH server")} />
      <span></span>
      <label class="check"><input type="checkbox" bind:checked={() => !!draft.autostart, (v) => (draft.autostart = v)} />{_("Start when xtermium opens")}</label>
    </div>
    <div class="row">
      {#if editing === null}
        <button class="primary" disabled={!!problem} onclick={add}><Icon name="plus" />{_("Add tunnel")}</button>
      {:else}
        <button class="primary" disabled={!!problem} onclick={add}>{_("Save")}</button>
        <button onclick={cancelEdit}>{_("Cancel")}</button>
      {/if}
      {#if problem}<span class="weak">{problem}</span>{/if}
    </div>
  </details>
</Modal>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 13px;
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
    background: rgba(var(--bg-rgb), 0.6);
  }
  .tag {
    padding: 0 5px;
    border-radius: 3px;
    background: var(--gold-soft);
    font-size: var(--t-s);
  }
  .status {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  details {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  summary {
    cursor: pointer;
    margin-bottom: 8px;
  }
  details > .row {
    margin-top: 8px;
  }
  .preview {
    padding: 8px 13px;
    margin-bottom: 8px;
    border: 1px dashed var(--line);
    border-radius: var(--r-menu);
  }
</style>
