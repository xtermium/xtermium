// Terminal colour schemes from elsewhere: terminal.sexy's JSON export (also its catalogue's
// files), and Windows Terminal's scheme JSON (the big scheme collections publish every scheme so).

/** As src/settings.rs `Scheme`: colours are 0xRRGGBB. */
export type Scheme = { name: string; bg: number; fg: number; cursor: number; selection: number; ansi: number[] };

/** terminal.sexy's catalogue: its scheme files on GitHub. */
export const CATALOGUE = "https://api.github.com/repos/stayradiated/terminal.sexy/git/trees/master?recursive=1";
export const CATALOGUE_RAW = "https://raw.githubusercontent.com/stayradiated/terminal.sexy/master/";

const WT = ["black", "red", "green", "yellow", "blue", "purple", "cyan", "white"];

/** `#rgb` or `#rrggbb` → 0xRRGGBB. */
function hex(v: unknown): number {
  let s = String(v ?? "").trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(s)) s = [...s].map((c) => c + c).join("");
  if (!/^[0-9a-f]{6}$/i.test(s)) throw new Error(`"${v}" isn't a colour`);
  return parseInt(s, 16);
}

/** A scheme from JSON text; `fallback` names it when the file doesn't. */
export function parseScheme(text: string, fallback: string): Scheme {
  let j: Record<string, unknown>;
  try {
    j = JSON.parse(text);
  } catch {
    throw new Error("That isn't JSON.");
  }
  let ansi: number[];
  if (Array.isArray(j.color) && j.color.length >= 16) ansi = j.color.slice(0, 16).map(hex); // terminal.sexy
  else if (WT.every((k) => k in j)) {
    const bright = (k: string) => j["bright" + k[0].toUpperCase() + k.slice(1)] ?? j[k];
    ansi = [...WT.map((k) => hex(j[k])), ...WT.map((k) => hex(bright(k)))]; // Windows Terminal
  } else throw new Error("Expected terminal.sexy JSON (a 16-colour \"color\" list) or a Windows Terminal scheme.");
  const fg = hex(j.foreground);
  const bg = hex(j.background);
  const name = String(j.name || fallback).trim() || "Imported";
  return { name, bg, fg, cursor: j.cursorColor ? hex(j.cursorColor) : fg, selection: j.selectionBackground ? hex(j.selectionBackground) : ansi[8], ansi };
}

/** "dist/schemes/base16/ashes.dark.json" → "ashes dark (base16)". */
export const catalogueName = (path: string) => {
  const parts = path.split("/");
  return `${parts.at(-1)!.replace(/\.json$/, "").replaceAll(/[._-]+/g, " ")} (${parts.at(-2)})`;
};
