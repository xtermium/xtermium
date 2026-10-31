<script lang="ts">
  // A session's or folder's icon: a built-in one in its colour, or the user's own image. Images
  // go in <img>, where an SVG's scripts can't run.
  import Icon from "./Icon.svelte";

  /** `still`: a built-in icon that doesn't move on hover. */
  let { icon, color = "", size = 18, still = false }: { icon: string; color?: string; size?: number; still?: boolean } = $props();
</script>

<!-- Only these icons move on hover (app.css `.session-icon`); the interface's own stay still. -->
<span class="session-icon" class:still>
  {#if icon.startsWith("data:")}
    <img src={icon} alt="" width={size} height={size} />
  {:else}
    <Icon name={icon} {size} {color} />
  {/if}
</span>

<style>
  .session-icon {
    display: contents;
  }
  img {
    flex: none;
    object-fit: contain;
    border-radius: var(--r-control);
  }
</style>
