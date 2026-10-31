<script lang="ts" module>
  // Boxicons (MIT, assets/icons/LICENSE-boxicons), recoloured to follow the text colour.
  const files = import.meta.glob("../../assets/icons/*.svg", { query: "?raw", import: "default", eager: true });
  const svgs: Record<string, string> = {};
  for (const [path, svg] of Object.entries(files)) {
    const name = path.split("/").pop()!.replace(".svg", "");
    svgs[name] = (svg as string).replace('fill="#ffffff"', 'fill="currentColor"').replace(/(width|height)="24"/g, '$1="100%"');
  }
  /** An icon as a standalone SVG file in `color`, `size` pixels square (the tray menu draws it). */
  export const iconFile = (name: string, color: string, size: number) =>
    (svgs[name] ?? "").replaceAll("currentColor", color).replace(/(width|height)="100%"/g, `$1="${size}"`);
</script>

<script lang="ts">
  // Animation only for "something is in progress": `spin` (loader) and `pulse` (live indicator).
  let {
    name,
    size = 18,
    spin = false,
    pulse = false,
    color = "",
  }: { name: string; size?: number; spin?: boolean; pulse?: boolean; color?: string } = $props();
</script>

<span class="icon" class:spin class:pulse style:width="{size}px" style:height="{size}px" style:color={color || null}>
  {@html svgs[name] ?? ""}
</span>

<style>
  .icon {
    display: inline-flex;
    flex: none;
  }
  .icon :global(svg) {
    display: block;
  }
  .spin {
    color: var(--gold);
    animation: spin 1.25s linear infinite;
  }
  .pulse {
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes pulse {
    50% {
      opacity: 0.55;
    }
  }
</style>
