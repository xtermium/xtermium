<script lang="ts" module>
  /** Where the account's storage stands: from 95% a warning (past 100% nothing more syncs), at 100% full. */
  export function level(q: { used: number; total: number } | null): "ok" | "nearly" | "full" {
    if (!q?.total) return "ok";
    const share = q.used / q.total;
    return share >= 1 ? "full" : share >= 0.95 ? "nearly" : "ok";
  }
</script>

<script lang="ts">
  // The sync account's storage, used against allowed, as the server last said (docs/sync.md
  // `quota`): a bar, and a word when it's nearly or quite full. What takes the room is pictures
  // used as session and folder icons, which travel inside the sessions file.
  import { _ } from "./i18n.svelte";
  import { human } from "./api";
  import Icon from "./Icon.svelte";
  import { app } from "./state.svelte";

  const q = $derived(app.sync.quota);
  const state = $derived(level(q));
  const advice = _("Pictures used as session and folder icons take most of the room: pick built-in or Dashboard icons, or smaller pictures.");
</script>

{#if q?.total}
  {@const pct = Math.min(100, Math.round((100 * q.used) / q.total))}
  <div class="quota">
    <!-- Not <progress>: the webview paints it as a flat grey line, whatever the accent. -->
    <div class="bar" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100"><i class={state} style:width="{pct}%"></i></div>
    <span class="weak">{_("{used} of {total} of sync storage used", { used: human(q.used), total: human(q.total) })}</span>
    {#if state === "full"}
      <span class="row error"><Icon name="x-circle" size={14} /><span>{_("Full: nothing more syncs until it shrinks.")} {advice}</span></span>
    {:else if state === "nearly"}
      <span class="row warn"><Icon name="error" size={14} /><span>{_("Nearly full: past 100% nothing more syncs.")} {advice}</span></span>
    {/if}
  </div>
{/if}

<style>
  .quota {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: var(--t-s);
  }
  .bar {
    height: 3px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--gold);
  }
  .bar i.nearly {
    background: var(--warn);
  }
  .bar i.full {
    background: var(--error);
  }
  .row {
    align-items: flex-start;
    gap: 5px;
  }
  .row :global(.icon) {
    margin-top: 2px;
  }
</style>
