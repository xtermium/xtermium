// Interface text for translators. `node scripts/i18n.mjs <code>` prints what ui/locales/<code>.json
// still lacks, as JSON to translate and merge in; without a code, how complete each language is.
// The text is every `_("…")` and `N_("…")` in ui/, and the action names (ui/lib/actions.ts).
import { readdirSync, readFileSync } from "node:fs";
import { ACTIONS, TERMINAL_KEYS } from "../ui/lib/actions.ts";

const ui = new URL("../ui/", import.meta.url);
const LITERAL = /\bN?_\(\s*("(?:[^"\\]|\\.)*")/g;

/** Every text the interface translates. */
export function texts() {
  const found = new Set([...ACTIONS.map((a) => a.label), ...Object.values(TERMINAL_KEYS)]);
  for (const f of readdirSync(ui, { recursive: true })) {
    if (!/\.(svelte|ts)$/.test(f) || f.endsWith(".test.ts")) continue;
    for (const m of readFileSync(new URL(f.replaceAll("\\", "/"), ui), "utf8").matchAll(LITERAL)) found.add(JSON.parse(m[1]));
  }
  return found;
}

/** Language code → its translations. */
export function locales() {
  const dir = new URL("locales/", ui);
  return Object.fromEntries(readdirSync(dir).filter((f) => f.endsWith(".json")).map((f) => [f.slice(0, -5), JSON.parse(readFileSync(new URL(f, dir), "utf8"))]));
}

if (import.meta.main) {
  const code = process.argv[2];
  const all = texts();
  const files = locales();
  if (code) {
    const have = files[code] ?? {};
    console.log(JSON.stringify(Object.fromEntries([...all].filter((t) => !have[t]).map((t) => [t, ""])), null, 2));
  } else {
    for (const [c, d] of Object.entries(files)) {
      const missing = [...all].filter((t) => !d[t]).length;
      const stale = Object.keys(d).filter((t) => !all.has(t)).length;
      console.log(`${c}: ${all.size - missing}/${all.size}${stale ? `, ${stale} no longer used` : ""}`);
    }
  }
}
