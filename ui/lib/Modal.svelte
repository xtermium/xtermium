<script lang="ts" module>
  const FIELDS = "input:not([type=hidden]):not(:disabled), select:not(:disabled), textarea:not(:disabled)";
  /** The first field in `root` that can take the keyboard: one in a closed `<details>` can't, and
   *  focusing it would leave the keyboard where it was, behind the dialog. A field marked
   *  `data-not-first` (a shortcut that fills the others in) is passed over: typing into it would
   *  pick from its list. */
  const firstField = (root: Element) =>
    [...root.querySelectorAll<HTMLElement>(FIELDS)].find((el) => el.checkVisibility() && !el.hasAttribute("data-not-first"));

  /** The dialogs open in this window, in the order they opened. */
  const opened: object[] = [];

  /** Whether a dialog (a Modal, or the command palette) is open in this window. If one is, it
   *  gets the keyboard: a terminal or remote desktop behind it calls this before taking a key,
   *  so nothing typed into a dialog ever reaches a server. */
  export function dialogTakesKeys(): boolean {
    const top = [...document.querySelectorAll<HTMLElement>(".backdrop")].at(-1);
    if (!top) return false;
    if (!top.contains(document.activeElement)) (firstField(top) ?? top.querySelector<HTMLElement>("[tabindex]") ?? top).focus();
    return true;
  }
</script>

<script lang="ts">
  import { _ } from "./i18n.svelte";
  import { onDestroy, onMount, tick, type Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import { portal } from "./menu";

  // A dialog window. `onclose` adds a close button and closes on Escape. `bar` (page
  // navigation) sits left of the body and stays put while the body scrolls; so does `foot`
  // (the dialog's buttons) under it, in reach however long the body is. `top` hangs it from a
  // fixed height instead of centring it, for a dialog whose fields come and go: it grows
  // downwards, and what's under the pointer stays put.
  let {
    title,
    width = 377,
    height,
    top = false,
    onclose,
    bar,
    foot,
    children,
  }: { title: string; width?: number; height?: number; top?: boolean; onclose?: () => void; bar?: Snippet; foot?: Snippet; children: Snippet } = $props();

  let win: HTMLDivElement;
  const TABBABLE = `${FIELDS}, button:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])`;
  /** Only the dialog on top (the last one opened) takes keys and focus: a question over Settings
   *  closes alone. The order they opened in decides, as the page's order can't yet while a new
   *  one is being drawn (it moves to the end of the page once drawn, and its button that takes the
   *  keyboard may have it already: the dialog under it mustn't take it back). The command
   *  palette, over everything, is last in the page. */
  const me = {};
  opened.push(me);
  onDestroy(() => void opened.splice(opened.indexOf(me), 1));
  const onTop = () => opened.at(-1) === me && win?.parentElement === [...document.querySelectorAll(".backdrop")].at(-1);
  const home = () => (firstField(win) ?? win).focus();

  // The dialog takes the keyboard, or typing would go on into the terminal behind it: its first
  // field (unless one of its own took it already), else the dialog itself, so Enter confirms nothing.
  // Closing gives the keyboard back to what had it before (a terminal, after a passphrase prompt).
  const before = document.activeElement;
  onMount(() => void tick().then(() => onTop() && !win.contains(document.activeElement) && home()));
  // After it's gone: a terminal hands the keyboard to any dialog still up (Terminal `dialogTakesKeys`).
  onDestroy(() => void tick().then(() => before instanceof HTMLElement && before.isConnected && before.focus()));

  function keydown(e: KeyboardEvent) {
    if (!onTop()) return;
    if (e.key === "Escape") onclose?.();
    if (e.key !== "Tab") return;
    // Tab and Shift+Tab go round inside the dialog.
    const all = [...win.querySelectorAll<HTMLElement>(TABBABLE)].filter((el) => el.checkVisibility());
    const at = document.activeElement;
    const edge = e.shiftKey ? at === all[0] || at === win : at === all.at(-1);
    if (!all.length || edge || !win.contains(at)) {
      e.preventDefault();
      (e.shiftKey ? all.at(-1) : all[0])?.focus();
    }
  }
  /** Something behind took the keyboard (a terminal coming into view): it comes back here. */
  const focusin = (e: FocusEvent) => onTop() && !win.contains(e.target as Node) && home();
</script>

<svelte:window onkeydown={keydown} />
<svelte:document onfocusin={focusin} />

<div class="backdrop" class:top {@attach portal}>
  <div class="window" bind:this={win} tabindex="-1" style:width="{width}px" style:height={height ? `${height}px` : null} role="dialog" aria-modal="true" aria-label={title}>
    <header class="row">
      <h2 class="grow">{title}</h2>
      {#if onclose}
        <button class="flat icon-only" title={_("Close")} aria-label={_("Close")} onclick={onclose}><Icon name="x" /></button>
      {/if}
    </header>
    <div class="main">
      {@render bar?.()}
      <div class="body">{@render children()}</div>
    </div>
    {#if foot}<footer class="row">{@render foot()}</footer>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    background: rgba(8, 10, 12, 0.4);
    animation: fade-in 0.16s ease-out;
  }
  .window {
    max-width: calc(100vw - 2 * 21px);
    max-height: calc(100vh - 2 * 21px);
    display: flex;
    flex-direction: column;
    background: rgba(var(--panel-rgb), 0.98);
    border: 1px solid var(--line);
    border-radius: var(--r-window);
    box-shadow: var(--shadow-dialog);
    animation: float-in 0.16s var(--ease-out);
  }
  /* focused only to hold the keyboard: no ring */
  .window:focus {
    outline: none;
  }
  /* hung where the command palette hangs */
  .backdrop.top {
    place-items: start center;
    padding-top: 12vh;
  }
  .backdrop.top .window {
    max-height: calc(88vh - 21px);
  }
  header {
    padding: 8px 8px 8px 13px;
    border-bottom: 1px solid var(--line);
  }
  /* a long name in "Edit …" ends in … rather than widening the dialog */
  h2 {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  footer {
    padding: 8px 13px;
    border-top: 1px solid var(--line);
  }
  .main {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .body {
    flex: 1;
    min-width: 0;
    padding: 13px 13px 13px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
</style>
