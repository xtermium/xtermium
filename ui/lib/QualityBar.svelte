<script lang="ts">
  // The RDP picture bar: from Quality (left, the default) to Speed. Changes apply on the next connect.
  import { _ } from "./i18n.svelte";
  import { RDP_QUALITY } from "./api";

  let { value = $bindable(), disabled = false }: { value: number; disabled?: boolean } = $props();
</script>

<div class="bar" class:disabled>
  <input type="range" min="0" max={RDP_QUALITY.length - 1} step="1" bind:value {disabled} aria-label={_("Picture quality against speed")} />
  <div class="names">
    {#each RDP_QUALITY as q, i (q.name)}<span class:on={i === value}>{_(q.name)}</span>{/each}
  </div>
  <span class="weak">{_(RDP_QUALITY[value]?.note ?? "")}</span>
</div>

<style>
  .bar {
    display: flex;
    flex-direction: column;
    gap: 5px;
    max-width: 420px;
  }
  .bar input {
    width: 100%;
  }
  .bar.disabled {
    opacity: 0.55;
  }
  .names {
    display: flex;
    justify-content: space-between;
    font-size: var(--t);
    color: var(--weak);
  }
  .names .on {
    color: var(--gold);
    font-weight: 600;
  }
</style>
