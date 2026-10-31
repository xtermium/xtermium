<script lang="ts">
  // A place's background (Settings: terminal, sidebar, tab bar): its picture or shader, and over
  // it `tint`, the place's own colour at the strength set. It goes in a parent that isolates its
  // stacking (`isolation: isolate`), under everything else there.
  import { PLACES, type Place } from "./api";
  import { show } from "./shader";
  import { app, backdrops, loadBackdrop } from "./state.svelte";

  /** `tint(alpha)`: the place's colour, at that alpha. */
  let { place, tint }: { place: Place; tint: (alpha: number) => string } = $props();

  const b = $derived(app.settings?.[PLACES[place]]);
  $effect(() => loadBackdrop(place, b));
  const data = $derived(b?.kind ? backdrops[place] : "");
  /** A picture's position, size and repeat for each way it fits (src/settings.rs `FITS`). */
  const FIT: Record<string, string> = {
    fill: "center / cover no-repeat",
    stretch: "0 0 / 100% 100% no-repeat",
    fit: "center / contain no-repeat",
    tile: "0 0 / auto repeat",
    center: "center / auto no-repeat",
  };
</script>

{#if b && data}
  {#if b.kind === "image"}
    <div class="layer" style:background="url('{data}') {FIT[b.fit] ?? FIT.fill}"></div>
  {:else}
    {#key data}<canvas class="layer" {@attach (el) => show(el, data)}></canvas>{/key}
  {/if}
  <div class="layer" style:background={tint(1 - b.show)}></div>
{/if}

<style>
  .layer {
    position: absolute;
    inset: 0;
    z-index: -1;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
</style>
