<script lang="ts">
  // Export a folder (or everything, for folder "") in one of the supported formats.
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { FORMATS, type Format } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { app, inside } from "./state.svelte";

  let { folder, onclose }: { folder: string; onclose: () => void } = $props();

  let format = $state<Format>("xtermium");
  let pass = $state("");
  let again = $state("");
  let busy = $state(false);
  let error = $state("");

  const count = $derived(folder ? app.store.session.filter((s) => inside(s.folder, folder)).length : app.store.session.length);
  const ok = $derived(format !== "xtermium" || (pass.length >= 8 && pass === again));
  const what = $derived(folder ? folder.split("/").pop()! : _("all sessions"));

  async function save() {
    [busy, error] = [true, ""];
    try {
      const r = await invoke<[string, number] | null>("export_sessions", { format, folder, passphrase: pass });
      if (r) {
        const [where, skipped] = r;
        const done = _("Exported to {path}.", { path: where });
        app.status = skipped ? `${done} ${_("{n} sessions were left out: this format can't hold their type.", { n: skipped })}` : done;
        onclose();
      }
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }
</script>

<Modal title={_("Export {what}", { what })} width={610} {onclose}>
  <p>
    {folder ? _("{n} sessions in this folder and its subfolders.", { n: count }) : _("{n} sessions, with folders, tags and tunnels.", { n: count })}
    {_("Passwords are never included.")}
  </p>
  <div class="formats">
    {#each FORMATS as f (f.id)}
      <label class="check format" class:on={format === f.id}>
        <input type="radio" bind:group={format} value={f.id} />
        <span><strong>{_(f.name)}</strong><span class="weak">{_(f.note)}</span></span>
      </label>
    {/each}
  </div>
  {#if format === "xtermium"}
    <div class="form">
      <span>{_("Passphrase")}</span>
      <input type="password" bind:value={pass} autocomplete="new-password" />
      <span>{_("Repeat it")}</span>
      <input type="password" bind:value={again} autocomplete="new-password" onkeydown={(e) => e.key === "Enter" && ok && save()} />
    </div>
    {#if pass && pass.length < 8}<p class="weak">{_("Use at least 8 characters.")}</p>{:else if again && pass !== again}<p class="weak">{_("The passphrases differ.")}</p>{/if}
  {:else}
    <p class="weak">{_("This file isn't encrypted: host names, users and key paths are readable by anyone who gets it.")}</p>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
  <div class="row">
    <button class="primary" disabled={!ok || busy || !count} onclick={save}>{#if busy}<Icon name="loader-alt" spin />{/if}<Icon name="download" />{_("Export…")}</button>
    <button onclick={onclose}>{_("Cancel")}</button>
  </div>
</Modal>

<style>
  p {
    margin: 0;
  }
  .formats {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .format {
    align-items: flex-start;
    padding: 5px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-control);
  }
  .format input {
    margin-top: 3px;
  }
  .format.on {
    border-color: color-mix(in srgb, var(--gold) 55%, transparent);
    background: var(--gold-soft);
  }
  .format > span {
    display: flex;
    flex-direction: column;
  }
</style>
