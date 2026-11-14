<script lang="ts">
  // The title bar's workspace switcher (App.svelte), dropped under its field as VS Code's quick
  // pick is: a search field, a row to make the workspace typed when none has that name, then every
  // workspace by name (the default one included, the one in use ticked), ten to a page. The field keeps the keyboard: arrows move,
  // Enter picks, Escape or a click outside closes it.
  import Icon from "./Icon.svelte";
  import { _ } from "./i18n.svelte";
  import { portal } from "./menu";
  import { openSettings } from "./Settings.svelte";
  import { COLORS } from "./api";
  import { app, saveStore, workspaceOp } from "./state.svelte";

  /** `x`, `y`: where it hangs (the field's start, in a right-to-left language its end); `width`: the field's. */
  let { x, y, width, onclose }: { x: number; y: number; width: number; onclose: () => void } = $props();

  const PAGE = 10;
  let query = $state("");
  let page = $state(0);
  let sel = $state(0);
  let field: HTMLInputElement;

  const nameOf = (w: string) => w || _("Default");
  /** A workspace's colour, "" for none; the one in use is read from the store, where it's set. */
  const colorOf = (w: string) => (w === app.workspace ? app.store.color : (app.workspaceList.find((x) => x.name === w)?.color ?? ""));
  function paint(c: string) {
    app.store.color = c;
    saveStore();
    field.focus();
  }
  const q = $derived(query.trim().toLocaleLowerCase());
  /** Every workspace: the backend lists the others only (api.ts `Workspaces`), the default one is "". */
  const every = $derived(["", ...app.workspaces.filter(Boolean)]);
  /** Those that match the search, by name, the one in use among them (the default sorts by its name too). */
  const matches = $derived(every.filter((w) => nameOf(w).toLocaleLowerCase().includes(q)).sort((a, b) => nameOf(a).localeCompare(nameOf(b))));
  const pages = $derived(Math.max(1, Math.ceil(matches.length / PAGE)));
  /** A name typed that no workspace has yet (24 characters at most, as Settings › Workspaces takes). */
  const fresh = $derived(q && !every.some((w) => nameOf(w).toLocaleLowerCase() === q) ? query.trim().slice(0, 24) : "");
  type Row = { kind: "new" | "current" | "other"; w: string };
  const rows = $derived.by(() => {
    const r: Row[] = [];
    if (fresh) r.push({ kind: "new", w: fresh });
    for (const w of matches.slice(page * PAGE, (page + 1) * PAGE)) r.push({ kind: w === app.workspace ? "current" : "other", w });
    return r;
  });

  async function pick(r: Row | undefined) {
    if (!r) return;
    try {
      if (r.kind === "new") await workspaceOp({ op: "add", name: r.w });
      if (r.kind !== "current") await workspaceOp({ op: "switch", name: r.w });
    } catch (e) {
      app.status = String(e);
    }
    onclose();
  }
  function turn(by: number) {
    page = Math.max(0, Math.min(pages - 1, page + by));
    sel = 0;
    field.focus();
  }
  /** The keys, from the field or any button in it (a Previous or Next click moves the keyboard back to the field). */
  function keydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, Math.min(rows.length - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
    } else if (e.key === "PageDown" || e.key === "PageUp") {
      e.preventDefault();
      turn(e.key === "PageDown" ? 1 : -1);
    } else if (e.key === "Enter" && e.target === field) {
      e.preventDefault();
      pick(rows[sel]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="menu picker" role="dialog" tabindex="-1" aria-label={_("Workspace")} style:left="{x}px" style:top="{y}px" style:width="{width}px" {@attach portal} onclick={(e) => e.stopPropagation()} onkeydown={keydown}>
  <input bind:this={field} bind:value={query} oninput={() => ((page = 0), (sel = 0))} {@attach (el) => el.focus()} placeholder={_("Search workspaces, or type a name to create one")} aria-label={_("Search workspaces")} />
  {#each rows as r, i (r.kind + r.w)}
    <button class:sel={i === sel} class:first={i === 0} aria-current={r.kind === "current" ? "true" : undefined} onclick={() => pick(r)} onpointermove={() => (sel = i)}>
      <Icon name={r.kind === "new" ? "plus" : r.kind === "current" ? "check" : "cluster"} color={r.kind === "new" ? undefined : colorOf(r.w) || undefined} />
      <span class="label">{r.kind === "new" ? _("Create workspace “{name}”", { name: r.w }) : nameOf(r.w)}</span>
      {#if r.kind === "current"}<span class="hint">{_("current")}</span>{/if}
    </button>
  {:else}
    <p class="weak none">{_("Nothing matches.")}</p>
  {/each}
  {#if pages > 1}
    <div class="pages">
      <button class="flat" disabled={page === 0} onclick={() => turn(-1)}>{_("Previous")}</button>
      <span class="weak">{_("{from}–{to} of {total}", { from: page * PAGE + 1, to: Math.min(matches.length, (page + 1) * PAGE), total: matches.length })}</span>
      <button class="flat" disabled={page >= pages - 1} onclick={() => turn(1)}>{_("Next")}</button>
    </div>
  {/if}
  <!-- The colour of the workspace in use: the title bar's field wears it. -->
  <div class="row tint sep" role="group" aria-label={_("Colour of this workspace")}>
    <button class="swatch auto" class:selected={!app.store.color} title={_("No colour")} aria-label={_("No colour")} style:--c="var(--weak)" onclick={() => paint("")}></button>
    {#each COLORS as c (c)}
      <button class="swatch" class:selected={app.store.color === c} title={c} aria-label={_("Colour {c}", { c })} style:--c={c} onclick={() => paint(c)}></button>
    {/each}
  </div>
  <button onclick={() => (onclose(), openSettings("workspaces"))}><Icon name="cog" />{_("Manage workspaces…")}</button>
</div>

<style>
  /* a menu's look (app.css `.menu`), the field's width, hung from the field with a gap */
  .picker {
    gap: 3px;
    max-height: calc(100vh / var(--phi));
    overflow: auto;
  }
  .picker > input {
    width: 100%;
    margin-bottom: 2px;
  }
  /* the row the keyboard is on (a pointer moves it there too, like the palette's) */
  .picker > button.sel {
    background: var(--raised);
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    margin-inline-start: auto;
    color: var(--weak);
    font-size: var(--t-s);
  }
  .none {
    margin: 0;
    padding: 3px 8px;
  }
  .pages {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--gap);
    padding: 0 5px;
  }
</style>
