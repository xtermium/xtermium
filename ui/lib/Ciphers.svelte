<script lang="ts">
  // SSH ciphers to offer. They're always offered in xtermium's order (best first, ssh.rs CIPHERS);
  // CBC is only for old network gear.
  import { _ } from "./i18n.svelte";
  import { app } from "./state.svelte";

  let { value = $bindable(), disabled = false }: { value: string[]; disabled?: boolean } = $props();

  function toggle(c: string, on: boolean) {
    const chosen = new Set(value);
    if (on) chosen.add(c);
    else chosen.delete(c);
    value = app.ciphers.filter((x) => chosen.has(x));
  }
</script>

<div class="ciphers">
  {#each app.ciphers as c (c)}
    <label class="check">
      <input type="checkbox" {disabled} checked={value.includes(c)} onchange={(e) => toggle(c, e.currentTarget.checked)} />
      <code>{c}</code>
      {#if c.endsWith("-cbc")}<span class="weak">{_("legacy, for old devices")}</span>{/if}
    </label>
  {/each}
  {#if !value.length && !disabled}<span class="weak">{_("None picked: xtermium offers its defaults.")}</span>{/if}
</div>

<style>
  .ciphers {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
</style>
