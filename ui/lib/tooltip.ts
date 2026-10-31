// Tooltips: every element's `title`, drawn in the app's own panel (app.css `.tooltip`) a moment
// after the pointer rests on it, and at once while moving along a row of tool buttons. A title
// ending in a shortcut, as `tip()` writes them ("Snippets (Ctrl+Shift+S)"), shows it in key caps.
// The title is taken off the element while the pointer is on it, so the system's own tooltip
// doesn't show as well. Over a website or BMC console the system's shows instead: those pages are
// native views laid over the window, and would cover ours.

/** A pause before the first tooltip; after one, the next comes at once for a while. */
const DELAY = 500;
const WARM = 300;
/** The shortcut at the end of a title: "(Ctrl+Shift+S)", "(F6)", "(Esc)". */
const KEYS = / \(((?:(?:Ctrl|Alt|Shift|Meta)\+)+\S+|F\d{1,2}|Esc)\)$/;

let on: HTMLElement | null = null;
let text = "";
let box: HTMLDivElement | null = null;
let timer = 0;
let warmUntil = 0;
let pointer = { x: 0, y: 0 };

/** Puts the title back and takes the tooltip away. */
function leave() {
  clearTimeout(timer);
  // Svelte may have set a new title meanwhile: that one stays.
  if (on && !on.hasAttribute("title")) on.setAttribute("title", text);
  if (box) {
    box.remove();
    box = null;
    warmUntil = Date.now() + WARM;
  }
  on = null;
}

function show() {
  if (!on?.isConnected) return leave();
  const keys = text.match(KEYS);
  box = document.createElement("div");
  box.className = "tooltip";
  box.setAttribute("aria-hidden", "true");
  box.append(keys ? text.slice(0, keys.index) : text);
  if (keys) box.append(Object.assign(document.createElement("kbd"), { textContent: keys[1] }));
  document.body.append(box);

  // Under the element, centred; above it when there's no room. A tall one (a splitter, a row of
  // the sidebar's tree) has it by the pointer instead.
  const r = on.getBoundingClientRect();
  const b = box.getBoundingClientRect();
  const tall = r.height > 34;
  const x = (tall ? pointer.x : r.left + r.width / 2) - b.width / 2;
  let y = tall ? pointer.y + 21 : r.bottom + 5;
  if (y + b.height > innerHeight - 5) y = (tall ? pointer.y - 13 : r.top - 5) - b.height;
  const left = Math.max(5, Math.min(x, innerWidth - b.width - 5));
  const top = Math.max(5, y);
  const covers = (p: DOMRect) => p.width > 1 && left < p.right && left + b.width > p.left && top < p.bottom && top + b.height > p.top;
  if ([...document.querySelectorAll(".web > .area")].some((el) => covers(el.getBoundingClientRect()))) {
    // The page would hide it: the system's tooltip, from the title put back.
    box.remove();
    box = null;
    on.setAttribute("title", text);
    return;
  }
  Object.assign(box.style, { left: `${left}px`, top: `${top}px` });
}

/** Starts the tooltips of this window. */
export function tooltips() {
  addEventListener(
    "pointerover",
    (e) => {
      const target = e.target as Element;
      if (on?.contains(target)) return;
      const el = target.closest?.<HTMLElement>("[title]");
      leave();
      const title = el?.getAttribute("title");
      if (!el || !title || e.buttons) return;
      [on, text] = [el, title];
      el.removeAttribute("title");
      timer = setTimeout(show, Date.now() < warmUntil ? 0 : DELAY);
    },
    true,
  );
  addEventListener("pointermove", (e) => (pointer = { x: e.clientX, y: e.clientY }), { capture: true, passive: true });
  addEventListener(
    "pointerout",
    (e) => {
      if (on && !on.contains(e.relatedTarget as Node | null)) leave();
    },
    true,
  );
  for (const type of ["pointerdown", "keydown", "wheel", "blur"]) addEventListener(type, leave, true);
}
