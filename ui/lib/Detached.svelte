<script lang="ts">
  // A terminal or RDP tab moved to a window of its own (App.svelte `detach`): just the tab, its
  // connection carried over. Right-click for its menu (Ctrl+right-click on a remote desktop, whose
  // right button is the desktop's); closing the window puts it back.
  import { invoke } from "@tauri-apps/api/core";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { bindings, chord } from "./actions";
  import type { Boot, Session, Settings } from "./api";
  import { _, setLanguage } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { menu } from "./menu";
  import { app, lock } from "./state.svelte";
  import LockScreen from "./LockScreen.svelte";
  import Rdp from "./Rdp.svelte";
  import Terminal from "./Terminal.svelte";

  /** Set by the backend before the page loads (src/main.rs `detach_tab`). */
  const tab = (window as unknown as { __DETACHED__: { id: number; title: string; zoom: number; session: Session } }).__DETACHED__;

  let ready = $state(false);
  let zoom = $state(tab.zoom);
  let view = $state<Terminal | Rdp>();
  const desktop = tab.session.kind.type === "rdp";
  /** A remote desktop's window says once how to reach the menu. */
  let hint = $state(desktop);
  if (desktop) setTimeout(() => (hint = false), 5000);
  let ctx = $state<{ x: number; y: number } | null>(null);
  let systemDark = $state(matchMedia("(prefers-color-scheme: dark)").matches);
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => (systemDark = e.matches));

  invoke<Boot>("boot").then(async (b) => {
    await setLanguage(b.settings.language);
    app.settings = b.settings;
    lock.locked = b.lock;
    ready = true;
  });
  // Changed in the main window's Settings.
  listen<Settings>("settings", (e) => {
    if (e.payload.language !== app.settings?.language) setLanguage(e.payload.language);
    app.settings = e.payload;
  });
  $effect(() => {
    const s = app.settings;
    if (!s) return;
    document.documentElement.dataset.theme = s.theme === "Dark" || (s.theme === "System" && systemDark) ? "dark" : "light";
    getCurrentWebview().setZoom(s.ui_scale).catch(() => {});
  });

  /** Back into the main window, which then closes this one. */
  function reattach() {
    view?.release();
    emitTo("main", "reattach", { id: tab.id, zoom });
  }
  /** Ends the tab (the main window drops it and closes this window). */
  const close = () => emitTo("main", "detached-close", tab.id);
  const paste = () =>
    desktop
      ? invoke("rdp_input", { id: tab.id, ev: { t: "paste" } })
      : invoke("term_key", { id: tab.id, key: { key: "v", ctrl: true, alt: false, shift: true, meta: false } });

  // Only the font zoom shortcuts act here; everything else is the terminal's.
  const keymap = $derived(bindings(app.settings?.shortcuts ?? {}));
  function keydown(e: KeyboardEvent) {
    if (desktop || lock.locked) return; // a remote desktop takes every key
    const id = keymap.get(chord(e));
    const by = { "font-bigger": 1, "font-smaller": -1, "font-reset": 0 }[id ?? ""];
    if (by === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    zoom = by ? Math.max(-8, Math.min(24, zoom + by)) : 0;
  }

  /** The right button opens the menu instead of reaching the terminal (no right-click paste here);
   *  on a remote desktop, Ctrl+right-click does, and the right button alone is the desktop's. */
  function pointer(e: PointerEvent) {
    if (e.button !== 2 || (desktop && !e.ctrlKey)) return;
    hint = false;
    e.preventDefault();
    e.stopPropagation();
    if (e.type === "pointerdown") ctx = { x: e.clientX, y: e.clientY };
  }
</script>

<svelte:window onkeydowncapture={keydown} onclick={() => (ctx = null)} />

{#if ready}
  <div class="detached" onpointerdowncapture={pointer} onpointerupcapture={pointer} role="presentation">
    {#if desktop}
      <Rdp bind:this={view} session={tab.session} attach={tab.id} active onid={() => {}} onmeta={() => {}} onconnected={() => {}} onfocus={() => {}} onexit={close} />
    {:else}
      <Terminal
        bind:this={view}
        session={tab.session}
        attach={tab.id}
        {zoom}
        active
        visible
        onid={() => {}}
        onmeta={(m) => getCurrentWindow().setTitle(m.title || tab.title).catch(() => {})}
        onfocus={() => {}}
        onexit={close}
      />
    {/if}
    {#if hint}<div class="hint" role="status">{_("Ctrl+right-click for xtermium's menu: put back, paste, close")}</div>{/if}
  </div>
  {#if ctx}
    <div class="menu" {@attach menu(() => (ctx = null), ctx.x, ctx.y)}>
      <button onclick={reattach}><Icon name="log-in" />{_("Put back in the main window")}</button>
      <button onclick={paste}><Icon name="paste" />{_("Paste")}</button>
      <button onclick={close}><Icon name="x" />{_("Close tab")}</button>
    </div>
  {/if}
{/if}
<LockScreen />

<style>
  .detached {
    position: fixed;
    inset: 0;
    background: rgb(var(--panel-rgb));
  }
  .hint {
    position: absolute;
    top: 8px;
    left: 50%;
    translate: -50% 0;
    padding: 5px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
    background: rgba(var(--panel-rgb), 0.95);
    box-shadow: var(--shadow-float);
    font-size: var(--t-s);
    pointer-events: none;
  }
</style>
