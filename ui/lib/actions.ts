// App actions: what keyboard shortcuts and the command palette can do. App.svelte runs them.
// Users rebind them in Settings > Shortcuts; only changes are stored (settings.shortcuts).

export type Action = { id: string; label: string; keys: string };

const tabs: Action[] = Array.from({ length: 9 }, (_, i) => ({ id: `tab-${i + 1}`, label: `Go to tab ${i + 1}`, keys: `Alt+${i + 1}` }));

export const ACTIONS: Action[] = [
  { id: "palette", label: "Command palette", keys: "Ctrl+Shift+P" },
  { id: "find", label: "Find a session", keys: "Ctrl+Shift+O" },
  { id: "find-text", label: "Find in terminal", keys: "Ctrl+Shift+F" },
  { id: "new-session", label: "New session", keys: "Ctrl+Shift+N" },
  { id: "local-shell", label: "Open a local shell", keys: "Ctrl+Shift+T" },
  { id: "close-tab", label: "Close tab", keys: "Ctrl+Shift+W" },
  { id: "reconnect", label: "Reconnect tab", keys: "Ctrl+Shift+R" },
  { id: "duplicate-tab", label: "Duplicate tab", keys: "Ctrl+Shift+D" },
  { id: "detach-tab", label: "Move tab to a new window", keys: "" },
  { id: "next-tab", label: "Next tab", keys: "Ctrl+Tab" },
  { id: "prev-tab", label: "Previous tab", keys: "Ctrl+Shift+Tab" },
  ...tabs,
  { id: "next-pane", label: "Focus the next pane", keys: "F6" },
  { id: "focus-left", label: "Focus the pane to the left", keys: "Alt+ArrowLeft" },
  { id: "focus-right", label: "Focus the pane to the right", keys: "Alt+ArrowRight" },
  { id: "focus-up", label: "Focus the pane above", keys: "Alt+ArrowUp" },
  { id: "focus-down", label: "Focus the pane below", keys: "Alt+ArrowDown" },
  { id: "move-left", label: "Swap the pane with the one to the left", keys: "Ctrl+Shift+ArrowLeft" },
  { id: "move-right", label: "Swap the pane with the one to the right", keys: "Ctrl+Shift+ArrowRight" },
  { id: "move-up", label: "Swap the pane with the one above", keys: "Ctrl+Shift+ArrowUp" },
  { id: "move-down", label: "Swap the pane with the one below", keys: "Ctrl+Shift+ArrowDown" },
  { id: "edge-left", label: "Move the pane's edge left", keys: "Alt+Shift+ArrowLeft" },
  { id: "edge-right", label: "Move the pane's edge right", keys: "Alt+Shift+ArrowRight" },
  { id: "edge-up", label: "Move the pane's edge up", keys: "Alt+Shift+ArrowUp" },
  { id: "edge-down", label: "Move the pane's edge down", keys: "Alt+Shift+ArrowDown" },
  { id: "layout-single", label: "Layout: single", keys: "" },
  { id: "layout-columns", label: "Layout: side by side", keys: "" },
  { id: "layout-rows", label: "Layout: stacked", keys: "" },
  { id: "layout-grid", label: "Layout: 2 × 2 grid", keys: "" },
  { id: "layout-tiles", label: "Layout: tiles", keys: "" },
  { id: "toggle-sidebar", label: "Show or hide the sidebar", keys: "Ctrl+Shift+B" },
  { id: "collapse-sidebar", label: "Collapse or expand the sidebar", keys: "Ctrl+Shift+L" },
  { id: "toggle-files", label: "Show or hide the file panel", keys: "Ctrl+Shift+E" },
  { id: "toggle-status", label: "Show or hide the status bar", keys: "" },
  { id: "toggle-broadcast", label: "Turn broadcast on or off", keys: "Ctrl+Shift+A" },
  { id: "tunnels", label: "Tunnels", keys: "Ctrl+Shift+U" },
  { id: "snippets", label: "Snippets", keys: "Ctrl+Shift+S" },
  { id: "credentials", label: "Shared logins", keys: "" },
  { id: "import", label: "Import sessions", keys: "" },
  { id: "export", label: "Export all sessions", keys: "" },
  { id: "settings", label: "Settings", keys: "Ctrl+," },
  { id: "font-bigger", label: "Larger terminal font", keys: "Ctrl+=" },
  { id: "font-smaller", label: "Smaller terminal font", keys: "Ctrl+-" },
  { id: "font-reset", label: "Default terminal font size", keys: "Ctrl+0" },
  { id: "theme", label: "Switch between light and dark", keys: "" },
  { id: "fullscreen", label: "Full screen", keys: "F11" },
  { id: "lock", label: "Lock xtermium", keys: "Ctrl+Shift+K" },
];

/** Chords the terminal itself uses; binding one to an action takes it away from the terminal. */
export const TERMINAL_KEYS: Record<string, string> = {
  "Ctrl+Shift+C": "copy",
  "Ctrl+Shift+V": "paste",
  "Shift+Insert": "paste",
  "Shift+PageUp": "scroll back",
  "Shift+PageDown": "scroll forward",
};

const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta", "AltGraph", "CapsLock"]);

/** The chord a key press makes, like "Ctrl+Shift+P"; "" for a lone modifier. Letters and digits
 *  go by physical key, so Shift or the keyboard layout don't change them. */
export function chord(e: KeyboardEvent): string {
  if (MODIFIERS.has(e.key)) return "";
  const key = e.code.startsWith("Key")
    ? e.code.slice(3)
    : e.code.startsWith("Digit")
      ? e.code.slice(5)
      : e.key === " "
        ? "Space"
        : e.key.length === 1
          ? e.key.toUpperCase()
          : e.key;
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Meta", key].filter(Boolean).join("+");
}

/** The chord bound to an action, with the user's changes applied. */
export const keysOf = (id: string, overrides: Record<string, string>) => overrides[id] ?? ACTIONS.find((a) => a.id === id)?.keys ?? "";

/** Chord → action, for every bound action. */
export function bindings(overrides: Record<string, string>): Map<string, string> {
  const m = new Map<string, string>();
  for (const a of ACTIONS) {
    const k = keysOf(a.id, overrides);
    if (k && !m.has(k)) m.set(k, a.id);
  }
  return m;
}

/** How well `query` matches `text`: every word must appear in order (fuzzily); higher is better,
 *  -1 for no match. Consecutive letters and word starts score more. */
export function score(query: string, text: string): number {
  const t = text.toLowerCase();
  let total = 0;
  for (const word of query.toLowerCase().split(/\s+/).filter(Boolean)) {
    // The walk is greedy, so it starts wherever the first letter is, not only at its first place:
    // "SMDatabase" in "Acme/SMDatabase" has an s in the folder, from where a walk leads nowhere.
    const first = [...word][0];
    let best = -1;
    for (let at = t.indexOf(first); at >= 0; at = t.indexOf(first, at + 1)) {
      best = Math.max(best, walk(t, word, at));
    }
    if (best < 0) return -1;
    total += best;
  }
  return total - t.length / 100;
}

/** One greedy walk of `word` through `t` from `start`, where its first letter is; -1 when it doesn't fit. */
function walk(t: string, word: string, start: number): number {
  let [from, last, s] = [start, -2, 0];
  for (const c of word) {
    const j = t.indexOf(c, from);
    if (j < 0) return -1;
    s += j === last + 1 ? 3 : j === 0 || " /-_.@:".includes(t[j - 1]) ? 2 : 1;
    [last, from] = [j, j + 1];
  }
  // letters scattered across the text ("web" in "sho-w or hid-e the status b-ar") aren't a hit:
  // the word has to sit in a stretch about its own length, as an abbreviation does ("prd" in "prod")
  return last - start + 1 > word.length * 2 + 2 ? -1 : s;
}

export type Address = { type: "ssh" | "rdp" | "telnet" | "vnc"; host: string; port: number; user: string };

/** Each protocol's usual port; a bare `host:port` with one of these picks the protocol. */
const PORTS: Record<Address["type"], number> = { ssh: 22, rdp: 3389, telnet: 23, vnc: 5900 };

/** Quick connect: "ssh me@host:2222", "rdp://192.0.2.19", "telnet switch1", "me@host" or
 *  "vnc host:5901", "host:3389" → where to connect. Null when the query isn't an address: a bare word like
 *  "prod" is a search. */
export function quickConnect(q: string): Address | null {
  const m = /^(?:(ssh|rdp|telnet|vnc)(?:\s+|:\/\/))?((?:[^@\s]+@)?(?:\[[0-9a-fA-F:.]+\]|[A-Za-z0-9._-]+)(?::\d{1,5})?)$/i.exec(q.trim());
  if (!m) return null;
  const proto = m[1]?.toLowerCase() as Address["type"] | undefined;
  const at = m[2].lastIndexOf("@");
  const user = at >= 0 ? m[2].slice(0, at) : "";
  let rest = m[2].slice(at + 1);
  let port = 0;
  const p = /^(\[.*\]|[^:]+):(\d+)$/.exec(rest);
  if (p) [rest, port] = [p[1], +p[2]];
  const host = rest.replace(/^\[|\]$/g, "");
  if (!proto && !user && !port && !/[.:]/.test(host)) return null;
  if (port > 65535) return null;
  const type = proto ?? ((Object.keys(PORTS) as Address["type"][]).find((t) => PORTS[t] === port) || "ssh");
  return { type, host, port: port || PORTS[type], user };
}

const PLACEHOLDER = /\{\{\s*([^{}]+?)\s*\}\}/g;

/** A snippet's `{{name}}` placeholders, each once, in order of appearance. */
export const placeholders = (text: string) => [...new Set([...text.matchAll(PLACEHOLDER)].map((m) => m[1]))];

/** A snippet's text with its placeholders filled in. */
export const fill = (text: string, values: Record<string, string>) => text.replace(PLACEHOLDER, (_, k: string) => values[k] ?? "");

/** How `n` tiled panes sit, as panes per row: about as many columns as rows, and rows that come
 *  out short share their width (7 → 3, 2, 2). */
export function tileRows(n: number): number[] {
  const rows = Math.ceil(n / Math.ceil(Math.sqrt(n)));
  const base = Math.floor(n / rows);
  const extra = n % rows; // the first `extra` rows hold one more
  return Array.from({ length: rows }, (_, r) => base + (r < extra ? 1 : 0));
}

/** Panes' sizes as fractions: each row's height, and each pane's width within its row. */
export type Sizes = { rows: number[]; cols: number[][] };

/** Every row and every pane in a row the same size. */
export const even = (shape: number[]): Sizes => ({ rows: shape.map(() => 1 / shape.length), cols: shape.map((n) => Array(n).fill(1 / n)) });

/** Pane `i` (left to right, top to bottom) of rows holding `shape[r]` panes each, as
 *  [left, top, width, height] in percent. */
export function cell(shape: number[], sizes: Sizes, i: number): [number, number, number, number] {
  let row = 0;
  while (i >= shape[row]) i -= shape[row++];
  const sum = (a: number[], n: number) => a.slice(0, n).reduce((x, y) => x + y, 0) * 100;
  return [sum(sizes.cols[row], i), sum(sizes.rows, row), sizes.cols[row][i] * 100, sizes.rows[row] * 100];
}

/** Moves the line between `list[at - 1]` and `list[at]` by `by` (a fraction): one grows, the
 *  other shrinks, neither below a tenth. */
export function nudge(list: number[], at: number, by: number): number[] {
  const out = [...list];
  const both = out[at - 1] + out[at];
  out[at - 1] = Math.min(both - 0.1, Math.max(0.1, out[at - 1] + by));
  out[at] = both - out[at - 1];
  return out;
}

/** Pane `i` of `n` tiled to fill the area, as [left, top, width, height] in percent. */
export const tile = (n: number, i: number) => cell(tileRows(n), even(tileRows(n)), i);

/** Direction of each move-* action (swap the focused pane with its neighbour), as [x, y]. */
export const MOVES: Record<string, [number, number]> = { "move-left": [-1, 0], "move-right": [1, 0], "move-up": [0, -1], "move-down": [0, 1] };

/** Direction of each edge-* action (move the focused pane's edge), as [x, y]. */
export const EDGES: Record<string, [number, number]> = { "edge-left": [-1, 0], "edge-right": [1, 0], "edge-up": [0, -1], "edge-down": [0, 1] };

/**
 * Pane `pane` with an edge it shares with a neighbour moved by `dx`, a fraction of the area
 * across, or `dy` down: its right (lower) edge if it has one, else its left (upper) one, so the
 * line between panes goes the way of the arrow, as in Windows Terminal. Null when it has no
 * neighbour that way across or down.
 */
export function resize(shape: number[], sizes: Sizes, pane: number, dx: number, dy: number): Sizes | null {
  let row = 0;
  let i = pane;
  while (i >= shape[row]) i -= shape[row++];
  /** The line after item `at` of `n`, or before it for the last one, moved by `d`. */
  const shift = (list: number[], at: number, n: number, d: number) => nudge(list, at < n - 1 ? at + 1 : at, d);
  const out = structuredClone(sizes);
  if (dx && shape[row] > 1) out.cols[row] = shift(sizes.cols[row], i, shape[row], dx);
  else if (dy && shape.length > 1) out.rows = shift(sizes.rows, row, shape.length, dy);
  else return null;
  return out;
}

/** Direction of each focus-* action, as [x, y]. */
export const DIRECTIONS: Record<string, [number, number]> = { "focus-left": [-1, 0], "focus-right": [1, 0], "focus-up": [0, -1], "focus-down": [0, 1] };

/** The pane next to pane `from` in direction [dx, dy], from their rectangles (tile() order);
 *  -1 at the edge. The nearest one that way wins, and the one most in line with `from` on a tie. */
export function neighbour(rects: [number, number, number, number][], from: number, [dx, dy]: [number, number]): number {
  const mid = ([l, t, w, h]: number[]) => [l + w / 2, t + h / 2];
  const [x, y] = mid(rects[from]);
  let [best, least] = [-1, Infinity];
  rects.forEach((r, i) => {
    const [cx, cy] = mid(r);
    const along = (cx - x) * dx + (cy - y) * dy;
    if (i === from || along < 0.5) return;
    const d = along + 2 * Math.abs(dx ? cy - y : cx - x);
    if (d < least) [best, least] = [i, d];
  });
  return best;
}
