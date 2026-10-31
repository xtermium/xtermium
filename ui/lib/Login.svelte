<script lang="ts">
  // Which login a session uses: its own, or a shared one (Credentials), as a `.form` grid row.
  import { _ } from "./i18n.svelte";
  import { app } from "./state.svelte";

  let { value = $bindable() }: { value: string } = $props();
</script>

<span>{_("Login")}</span>
<div class="row">
  <select class="grow" bind:value>
    <option value="">{_("Its own username and password")}</option>
    {#each app.store.credential as c (c.id)}<option value={c.id}>{c.name}{c.user ? ` (${c.user})` : ""}</option>{/each}
  </select>
  <button class="flat" onclick={() => (app.showCredentials = true)}>{_("Manage…")}</button>
</div>
{#if !app.store.credential.length}
  <span></span>
  <p class="weak hint">{_("No shared logins yet: the session asks for its own username and password, or remembers them. Manage… keeps a login once for any number of sessions.")}</p>
{/if}

<style>
  .hint {
    margin: 0;
  }
</style>
