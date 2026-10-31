<script lang="ts">
  // "Copy the server from" a saved SSH session, as a `.form` grid row: hands its SSH settings to `onpick`
  // (host, port, user, key and shared login). Nothing shows without saved SSH sessions.
  import { _ } from "./i18n.svelte";
  import type { Kind } from "./api";
  import { app } from "./state.svelte";

  let { onpick }: { onpick: (k: Extract<Kind, { type: "ssh" }>) => void } = $props();

  const saved = $derived(app.store.session.filter((s) => s.kind.type === "ssh"));
</script>

{#if saved.length}
  <span>{_("Copy the server from")}</span>
  <!-- a shortcut for the fields below: a dialog opening doesn't give it the keyboard (Modal `firstField`) -->
  <select
    data-not-first
    value=""
    onchange={(ev) => {
      const s = saved[+ev.currentTarget.value];
      if (s?.kind.type === "ssh") onpick($state.snapshot(s.kind));
      ev.currentTarget.value = "";
    }}
  >
    <option value="">{_("a saved SSH session…")}</option>
    {#each saved as s, i (i)}
      {@const k = s.kind.type === "ssh" ? s.kind : null}
      <option value={i}>{s.folder ? `${s.folder}/` : ""}{s.name}{k ? ` (${k.user ? `${k.user}@` : ""}${k.host}${k.port === 22 ? "" : `:${k.port}`})` : ""}</option>
    {/each}
  </select>
{/if}
