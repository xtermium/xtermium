<script lang="ts">
  // Host / port / user / key fields, as rows of a `.form` grid. They can be filled in from a
  // saved SSH session (its login too), e.g. to use it as a jump host: that pick comes first, above
  // what it fills, and a dialog's keyboard still lands on the host (SshPick is `data-not-first`).
  import { _ } from "./i18n.svelte";
  import type { Endpoint } from "./api";
  import Login from "./Login.svelte";
  import PathInput from "./PathInput.svelte";
  import SshPick from "./SshPick.svelte";

  let { e, label = _("Host / port") }: { e: Endpoint; label?: string } = $props();
</script>

<SshPick onpick={(k) => Object.assign(e, { host: k.host, port: k.port, user: k.user, key: k.key, credential: k.credential ?? "" })} />
<span>{label}</span>
<div class="row"><input class="grow" bind:value={e.host} /><input type="number" min="1" max="65535" bind:value={e.port} /></div>
<Login bind:value={() => e.credential ?? "", (v) => (e.credential = v)} />
{#if !e.credential}
  <span>{_("User")}</span>
  <input bind:value={e.user} />
  <span>{_("Private key")}</span>
  <PathInput bind:value={e.key} start="~/.ssh/" placeholder={_("empty: your SSH agent, keys in ~/.ssh or a password")} />
{/if}
