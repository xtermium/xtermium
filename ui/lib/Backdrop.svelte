<script lang="ts">
  // A place's background (Settings: terminal, sidebar, tab bar): its picture or shader, and over
  // it `tint`, the place's own colour at the strength set. It goes in a parent that isolates its
  // stacking (`isolation: isolate`), under everything else there. It stands in for the parent's
  // own background, which it clears, and is see-through on a Transparent window (`--picture-alpha`,
  // a little stronger than the parent's own fill so a picture doesn't fade): it mustn't make the
  // window solid where it's shown.
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
  <div class="place-backdrop">
    <!-- the place's colour under a picture that doesn't cover it all (fit, centre) -->
    <div class="layer" style:background={tint(1)}></div>
    {#if b.kind === "image"}
      <div class="layer" style:background="url('{data}') {FIT[b.fit] ?? FIT.fill}"></div>
    {:else}
      {#key data}<canvas class="layer" {@attach (el) => show(el, data)}></canvas>{/key}
    {/if}
    <div class="layer" style:background={tint(1 - b.show)}></div>
  </div>
{/if}

<style>
  /* one layer, see-through on a Transparent window at `--picture-alpha`: a little stronger than the
     parent's own fill, so with the window's ground a place with a picture is a little more solid
     than the Opacity setting, rather than the picture fading */
  .place-backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    /* as strong as before the ground took the larger share (api.ts `seeThrough`), on light too */
    opacity: var(--picture-alpha, 1);
    pointer-events: none;
  }
  /* its colour is in the backdrop now: painted twice it would be more solid than set */
  :global(:has(> .place-backdrop)) {
    background: none !important;
  }
  /* a flat button under the pointer (the sidebar's rows, the bars' tools) lets the picture through
     a little: its solid `raised` stood on it as a slab */
  :global(:has(> .place-backdrop) button.flat:hover:not(:disabled)) {
    background: color-mix(in srgb, var(--raised) 60%, transparent);
  }
  .layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
</style>
