<script lang="ts">
  // Colour and icon for a session or folder, as rows of a `.form` grid. The icon is a built-in
  // one, an image file, or an image from a link; images are kept inside the session (as a
  // `data:` URL) so they show offline. Photos are shrunk to icon size; GIFs and SVGs stay as they are.
  // Images are also kept in the settings, under Custom, to pick again and rename. Dashboard Icons
  // (dashicons.ts) are searched here and fetched when picked; a thing named after one gets it by itself.
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { score } from "./actions";
  import { COLORS, ICONS } from "./api";
  import type { DashIcon } from "./dashicons";
  import Icon from "./Icon.svelte";
  import { shrinkIcon } from "./images";
  import SessionIcon from "./SessionIcon.svelte";
  import { app } from "./state.svelte";

  /** `site`: a website session's address, offering its icon. `still`: a built-in icon stays still on hover.
   *  `iconColor`: the icons' own colour when the picked one goes elsewhere (a session's name and
   *  tab line: its icon keeps its kind's colour); `label` names the colour row then. `name`: what
   *  the thing is called; one called after a Dashboard Icon ("gitlab.example.com") gets it as it's typed. */
  let {
    color = $bindable(),
    icon = $bindable(),
    still = $bindable(false),
    fallbackIcon,
    fallbackColor,
    iconColor = "",
    label = _("Colour"),
    site = "",
    name = "",
  }: { color: string; icon: string; still?: boolean; fallbackIcon: string; fallbackColor: string; iconColor?: string; label?: string; site?: string; name?: string } = $props();

  // Dashboard Icons: the catalogue is its own chunk, loaded when the picker opens.
  let dash = $state<typeof import("./dashicons") | null>(null);
  import("./dashicons").then((m) => (dash = m));
  const dark = document.documentElement.dataset.theme !== "light";
  const DASH_PAGE = 48; // 16 to a row, 3 rows
  let dashQuery = $state("");
  let dashPage = $state(0);
  /** Searched: the whole catalogue; otherwise the icons for sysadmins, network and devops people. */
  const dashList = $derived.by((): DashIcon[] => {
    if (!dash) return [];
    const q = dashQuery.trim();
    if (!q) return dash.DASH.filter((i) => i.ops);
    return dash.DASH.map((i) => [i, score(q, `${i.name} ${i.aliases.join(" ")}`)] as const)
      .filter(([, s]) => s >= 0)
      .sort((a, b) => b[1] - a[1])
      .map(([i]) => i);
  });
  const dashPages = $derived(Math.max(1, Math.ceil(dashList.length / DASH_PAGE)));
  $effect(() => {
    if (dashPage >= dashPages) dashPage = dashPages - 1;
  });
  const useDash = (i: DashIcon) => set(() => invoke<string>("fetch_icon", { url: dash!.dashUrl(i, dark) }), null);

  /** The name changed and there's no icon: the Dashboard Icon it names, if any, half a second later. */
  let seenName: string | null = null;
  $effect(() => {
    const n = name.trim();
    if (seenName === n) return;
    const opening = seenName === null;
    seenName = n;
    if (opening || icon || !n) return;
    const t = setTimeout(async () => {
      const m = dash ?? (await import("./dashicons"));
      const hit = m.dashFind(n);
      if (!hit || icon || name.trim() !== n) return;
      try {
        icon = await shrinkIcon(await invoke<string>("fetch_icon", { url: m.dashUrl(hit, dark) }));
      } catch {
        // offline, or the picture is gone: the picker is still there
      }
    }, 500);
    return () => clearTimeout(t);
  });

  let url = $state("");
  let busy = $state(false);
  let error = $state("");
  let file: HTMLInputElement;

  const MAX = 512 * 1024;
  const saved = $derived(app.settings?.custom_icons ?? []);
  /** The picked icon, when it's a saved one. */
  const mine = $derived(saved.find((c) => c.data === icon));
  const keep = (name: string) => app.settings?.custom_icons.push({ name, data: icon });
  const forget = () => app.settings && (app.settings.custom_icons = saved.filter((c) => c.data !== icon));
  /** A link's file name without its extension, else its host. */
  function linkName(u: string) {
    try {
      const l = new URL(u);
      return decodeURIComponent(l.pathname.split("/").pop() ?? "").replace(/\.\w+$/, "") || l.hostname;
    } catch {
      return u;
    }
  }

  /** `name`: what to keep it under in Custom; null for a site's icon, which isn't kept. */
  async function set(get: () => Promise<string>, name: string | null) {
    [busy, error] = [true, ""];
    try {
      icon = await shrinkIcon(await get());
      if (!mine && name !== null) keep(name);
      url = "";
    } catch (e) {
      error = String(e).replace(/^Error: /, "");
    }
    busy = false;
  }

  function pick(f: File | undefined) {
    if (!f) return;
    if (f.size > MAX) return void (error = _("Pick an image under 512 KB."));
    set(
      () =>
        new Promise((ok, fail) => {
          const r = new FileReader();
          r.onload = () => (String(r.result).startsWith("data:image/") ? ok(String(r.result)) : fail(_("That file isn't an image.")));
          r.onerror = () => fail(_("Could not read the file."));
          r.readAsDataURL(f);
        }),
      f.name.replace(/\.\w+$/, ""),
    );
  }
</script>

<span>{label}</span>
<div class="row wrap swatches">
  <button class="swatch auto" class:selected={!color} title={_("Default")} aria-label={_("Default colour")} onclick={() => (color = "")} style:--c={fallbackColor}></button>
  {#each COLORS as c (c)}
    <button class="swatch" class:selected={color === c} title={c} aria-label={_("Colour {c}", { c })} style:--c={c} onclick={() => (color = c)}></button>
  {/each}
  <label class="swatch custom" class:selected={color && !COLORS.includes(color)} title={_("Any colour")} style:--c={color || fallbackColor}>
    <input type="color" value={color || "#d8a84e"} oninput={(e) => (color = (e.target as HTMLInputElement).value)} />
  </label>
</div>

<span>{_("Icon")}</span>
<div class="col">
  <div class="row wrap icons">
    <button class="flat icon-only" class:selected={!icon} title={_("Default")} onclick={() => (icon = "")}><SessionIcon icon={fallbackIcon} color={iconColor || color || fallbackColor} {still} /></button>
    {#each ICONS.filter((i) => i !== fallbackIcon) as i (i)}
      <button class="flat icon-only" class:selected={icon === i} title={i} onclick={() => (icon = i)}><SessionIcon icon={i} color={iconColor || color || fallbackColor} {still} /></button>
    {/each}
  </div>
  {#if !icon.startsWith("data:")}
    <div class="row" role="group" aria-label={_("Icon motion")}>
      <button class:selected={!still} onclick={() => (still = false)} title={_("The icon moves while the pointer is over it")}>{_("Animated")}</button>
      <button class:selected={still} onclick={() => (still = true)} title={_("The icon never moves")}>{_("Static")}</button>
    </div>
  {/if}
  {#if saved.length || icon.startsWith("data:")}
    <span class="weak">{_("Custom")}</span>
    <div class="row wrap icons">
      {#each saved as c (c.data)}
        <button class="flat icon-only" class:selected={icon === c.data} title={c.name} onclick={() => (icon = c.data)}><SessionIcon icon={c.data} /></button>
      {/each}
      {#if icon.startsWith("data:") && !mine}
        <button class="flat icon-only selected" title={_("Keep under Custom")} onclick={() => keep(_("Your image"))}><SessionIcon {icon} /></button>
      {/if}
    </div>
  {/if}
  {#if mine}
    <div class="row">
      <input class="grow" bind:value={mine.name} placeholder={_("Name")} aria-label={_("Icon name")} />
      <button onclick={forget} title={_("Remove it from Custom; sessions using it keep it")}><Icon name="trash" />{_("Remove")}</button>
    </div>
  {/if}
  <input type="search" bind:value={dashQuery} oninput={() => (dashPage = 0)} placeholder={_("Search Dashboard Icons: servers, network gear, tools…")} aria-label={_("Search Dashboard Icons")} />
  <div class="dash">
    {#each dashList.slice(dashPage * DASH_PAGE, (dashPage + 1) * DASH_PAGE) as i (i.name)}
      <button class="flat icon-only" title={i.name} disabled={busy} onclick={() => useDash(i)}><img src={dash!.dashUrl(i, dark)} alt="" width="18" height="18" loading="lazy" /></button>
    {:else}
      <p class="weak none">{dash ? _("No icon matches.") : _("Loading…")}</p>
    {/each}
  </div>
  <div class="row pager">
    <button class="flat" disabled={dashPage === 0} onclick={() => dashPage--}>{_("Previous")}</button>
    <span class="weak">{_("Page {n} of {total} · {icons} icons from dashboardicons.com", { n: dashPage + 1, total: dashPages, icons: dashList.length })}</span>
    <button class="flat" disabled={dashPage >= dashPages - 1} onclick={() => dashPage++}>{_("Next")}</button>
  </div>
  <div class="row wrap">
    {#if site}
      <button disabled={busy} title={_("Download the icon the website shows in browser tabs")} onclick={() => set(() => invoke<string>("fetch_favicon", { url: site }), null)}>
        <Icon name="globe" />{_("Website icon")}
      </button>
    {/if}
    <input
      class="grow link"
      bind:value={url}
      placeholder={_("https://… link to a PNG, GIF or SVG")}
      onkeydown={(e) => e.key === "Enter" && url && set(() => invoke<string>("fetch_icon", { url }), linkName(url))}
    />
    <button disabled={!url || busy} onclick={() => set(() => invoke<string>("fetch_icon", { url }), linkName(url))}>{#if busy}<Icon name="loader-alt" spin />{/if}{_("Use link")}</button>
    <button onclick={() => file.click()}><Icon name="upload" />{_("Image…")}</button>
    <input bind:this={file} type="file" accept="image/png,image/gif,image/svg+xml,image/jpeg,image/webp,image/x-icon" hidden onchange={(e) => pick((e.target as HTMLInputElement).files?.[0])} />
  </div>
  {#if error}<span class="error">{error}</span>{/if}
</div>

<style>
  .col {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .swatches {
    gap: 5px;
  }
  .swatch {
    width: var(--row);
    height: var(--row);
    cursor: pointer;
  }
  .swatch.selected {
    box-shadow: 0 0 0 2px var(--gold-soft);
  }
  /* "default": the colour it would have anyway (a folder's, the kind's), struck through */
  .swatch.auto {
    background: linear-gradient(135deg, transparent 45%, var(--weak) 45% 55%, transparent 55%), color-mix(in srgb, var(--c) 35%, transparent);
  }
  .swatch.custom {
    display: inline-block;
    background: conic-gradient(#ec6a6a, #d8a84e, #b5c14a, #33b8a6, #5c8df0, #9c70e6, #dd6b9c, #ec6a6a);
    position: relative;
    overflow: hidden;
  }
  .swatch.custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }
  .icons {
    gap: 2px;
  }
  .link {
    min-width: 144px;
  }
  .icons button,
  .dash button {
    min-height: var(--control);
    padding: 5px;
    border: 1px solid transparent;
  }
  /* three rows of sixteen, the height held while a page fills */
  .dash {
    display: grid;
    grid-template-columns: repeat(16, minmax(0, 1fr));
    gap: 2px;
    min-height: calc(3 * var(--control) + 2 * 2px);
  }
  .dash .none {
    grid-column: 1 / -1;
    margin: 0;
  }
  .pager {
    justify-content: space-between;
  }
</style>
