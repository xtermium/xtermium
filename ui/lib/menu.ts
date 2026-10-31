// Context menus (`.menu`): `{@attach menu(close, x, y)}` opens one at the pointer, moved back
// inside the window if it would spill over an edge. The keyboard goes to its first item; arrow
// keys, Home and End move; Escape or Tab closes it and gives the keyboard back.
import type { Attachment } from "svelte/attachments";

/** Moves a layer floating over everything (a dialog, a menu) to the end of the page while it's
 *  open. Left inside the sidebar, whose stacking it would share (the sidebar isolates its
 *  background), it would paint under the panes. Moving drops the keyboard: it goes back where
 *  it was inside. */
export function portal(el: HTMLElement) {
  const had = el.contains(document.activeElement) ? (document.activeElement as HTMLElement) : null;
  document.body.append(el);
  had?.focus({ preventScroll: true });
  // Svelte takes away only the nodes where it put them.
  return () => el.remove();
}

/** Without `x`, `y` the menu places itself (above its button, say) and only takes the keys. */
export function menu(close: () => void, x?: number, y?: number): Attachment<HTMLElement> {
  return (el) => {
    const back = document.activeElement;
    const unportal = portal(el);
    if (x !== undefined && y !== undefined) {
      Object.assign(el.style, { left: `${x}px`, top: `${y}px` });
      const r = el.getBoundingClientRect();
      const m = 4; // clear of the window's edge
      const dx = Math.max(0, r.right - innerWidth + m) - Math.max(0, m - r.left);
      const dy = Math.max(0, r.bottom - innerHeight + m) - Math.max(0, m - r.top);
      Object.assign(el.style, { left: `${x - dx}px`, top: `${y - dy}px` });
    }
    const items = () => [...el.querySelectorAll<HTMLElement>("button:not(:disabled)")];
    el.tabIndex = -1;
    (items()[0] ?? el).focus({ preventScroll: true });
    const key = (e: KeyboardEvent) => {
      const all = items();
      const at = all.indexOf(document.activeElement as HTMLElement);
      const to = { ArrowDown: at + 1, ArrowUp: (at < 0 ? 0 : at) - 1 + all.length, Home: 0, End: all.length - 1 }[e.key];
      if (to !== undefined) {
        e.preventDefault();
        all[to % all.length]?.focus();
      } else if (e.key === "Escape" || e.key === "Tab") {
        e.stopPropagation();
        if (e.key === "Escape") e.preventDefault();
        close();
      }
    };
    // The item picked acts from where the keyboard was, so a dialog it opens gives it back there.
    const pick = () => back instanceof HTMLElement && back.isConnected && back.focus({ preventScroll: true });
    el.addEventListener("keydown", key);
    el.addEventListener("click", pick, true);
    return () => {
      el.removeEventListener("keydown", key);
      el.removeEventListener("click", pick, true);
      // The keyboard goes back unless the item picked moved it on (to a dialog, say).
      const at = document.activeElement;
      if ((!at || at === document.body || el.contains(at)) && back instanceof HTMLElement && back.isConnected) back.focus({ preventScroll: true });
      unportal();
    };
  };
}
