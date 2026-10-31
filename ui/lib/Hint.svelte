<script lang="ts">
  // A tip, as a `.form` grid row, that can be turned off for good (settings.quiet_hints;
  // Settings › General brings them back).
  import type { Snippet } from "svelte";
  import { _ } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./state.svelte";

  let { id, children }: { id: string; children: Snippet } = $props();
</script>

{#if app.settings && !app.settings.quiet_hints.includes(id)}
  <span></span>
  <p class="weak hint">
    <Icon name="info-circle" size={14} />
    <span>{@render children()} <button class="flat quiet" onclick={() => app.settings!.quiet_hints.push(id)}>{_("Don't remind me again")}</button></span>
  </p>
{/if}

<style>
  .hint {
    display: flex;
    gap: 5px;
    align-items: baseline;
  }
  .quiet {
    display: inline;
    min-height: 0;
    padding: 0 5px;
    font-size: inherit;
    text-decoration: underline;
    color: inherit;
  }
</style>
