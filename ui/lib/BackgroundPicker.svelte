<script lang="ts">
  // Picks what's behind a place (Settings: terminal, sidebar, tab bar), as rows of a `.form`
  // grid: a picture or a shader from a file or a link, how a picture fits, and how much of it
  // shows. Shaders are compiled first: one that doesn't compile isn't kept, and says why.
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { FITS, PLACES, type Place, type Settings } from "./api";
  import Icon from "./Icon.svelte";
  import { compile } from "./shader";
  import { app, toast } from "./state.svelte";
  import { filled } from "./slider";

  /** `label`: the first row's ("Behind the text"). */
  let { place, label }: { place: Place; label: string } = $props();

  const b = $derived(app.settings![PLACES[place]]);
  /** The strength a new background starts at (src/settings.rs `Background`), for its reset
   *  button: null while asking. */
  let start = $state<number | null>(null);
  invoke<Settings>("default_settings").then((d) => (start = d[PLACES[place]].show));
  const same = $derived(start === null || Math.abs(b.show - start) < 0.001);
  const title = $derived(start === null ? "" : _("Reset to {keys}", { keys: _("{n} %", { n: Math.round(start * 100) }) }));

  /** The strength back to its default, with an Undo, as Settings' other reset buttons do. The
   *  button hides now, so the keyboard goes to the slider beside it. */
  function resetShow(e: MouseEvent) {
    const [was, slider] = [b.show, (e.currentTarget as HTMLElement).parentElement?.querySelector("input")];
    b.show = start!;
    slider?.focus();
    toast(_("Back to the default."), { label: _("Undo"), run: () => (app.settings![PLACES[place]].show = was) });
  }
  let url = $state("");
  let busy = $state(false);
  let error = $state("");

  async function set(kind: "" | "image" | "shader", name: string, data: string) {
    error = "";
    if (kind === "shader") {
      const made = compile(data);
      if (typeof made === "string") return void (error = _("The shader doesn't compile: {why}", { why: made }));
    }
    try {
      app.settings = await invoke<Settings>("background_set", { place, kind, name, data });
      url = "";
    } catch (e) {
      error = String(e);
    }
  }

  /** The system's own file dialog (src/support.rs): the page's picker left shader files greyed
   *  out on some desktops. */
  async function pickFile() {
    [busy, error] = [true, ""];
    try {
      const got = await invoke<["image" | "shader", string, string] | null>("pick_background");
      if (got) await set(got[0], got[1], got[2]);
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }

  async function link() {
    [busy, error] = [true, ""];
    try {
      const [kind, data] = await invoke<["image" | "shader", string]>("fetch_background", { url });
      await set(kind, url.trim(), data);
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }
</script>

<span>{label}</span>
<div class="col">
  <div class="row">
    <span class="grow name" class:weak={!b.kind} title={b.name}>
      {b.kind === "shader" ? (b.name ? _("Shader: {name}", { name: b.name }) : _("Shader")) : b.kind ? (b.name ? _("Picture: {name}", { name: b.name }) : _("Picture")) : _("Nothing")}
    </span>
    <button disabled={busy} onclick={pickFile}><Icon name="upload" />{_("File…")}</button>
    {#if b.kind}<button onclick={() => set("", "", "")}><Icon name="trash" />{_("Remove")}</button>{/if}
  </div>
  <div class="row">
    <input class="grow" bind:value={url} placeholder={_("https://… link to a picture or a shader file")} onkeydown={(e) => e.key === "Enter" && url.trim() && link()} />
    <button disabled={!url.trim() || busy} onclick={link}>{#if busy}<Icon name="loader-alt" spin />{/if}{_("Use link")}</button>
  </div>
  {#if error}<pre class="error">{error}</pre>{/if}
</div>
{#if b.kind === "image"}
  <span>{_("Fit")}</span>
  <div class="row wrap" role="group" aria-label={_("Fit")}>
    {#each FITS as [id, name] (id)}<button class:selected={b.fit === id} aria-pressed={b.fit === id} onclick={() => (b.fit = id)}>{_(name)}</button>{/each}
  </div>
{/if}
<span>{_("Strength")}</span>
<div class="row">
  <input type="range" min="0.05" max="0.95" step="0.05" bind:value={b.show} disabled={!b.kind} {@attach filled(b.show)} />{_("{n} %", { n: Math.round(b.show * 100) })}
  <!-- its way back to the default, as the other sliders have; it keeps its place while hidden -->
  <button class="flat icon-only back" class:same disabled={same || !b.kind} {title} aria-label={title} onclick={resetShow}><Icon name="refresh" /></button>
</div>

<style>
  .col {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  /* as Settings' own reset buttons (`back`) */
  .back {
    margin-block: calc((var(--row) - var(--control)) / 2);
  }
  .back.same {
    visibility: hidden;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  pre {
    margin: 0;
    max-height: 144px;
    overflow: auto;
    white-space: pre-wrap;
    font-size: var(--t-s);
    user-select: text;
  }
</style>
