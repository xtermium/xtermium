// Interface languages. Text is keyed by its English, so English needs no file and anything not
// translated yet shows in English. ui/locales/<code>.json maps English → the language; only the
// chosen one is loaded. `npm test` checks every file against the text in the code.

const files = import.meta.glob<Record<string, string>>("../locales/*.json", { import: "default" });

/** Every language there is a file for, and English. */
export const LOCALES = ["en", ...Object.keys(files).map((p) => p.slice(p.lastIndexOf("/") + 1, -5))];

/** Written right to left. */
const RTL = new Set(["ar"]);
/** System languages that read as one there is a file for: Norwegian is written in Bokmål. */
const ALIASES: Record<string, string> = { no: "nb", nn: "nb" };

let dict = $state<Record<string, string>>({});

/** A language's name in itself ("Deutsch", "日本語"). */
export const nativeName = (code: string) => {
  const name = new Intl.DisplayNames([code], { type: "language" }).of(code) ?? code;
  return name[0].toLocaleUpperCase(code) + name.slice(1);
};

/** The language to show for a setting ("" = the system's): exact, else by base ("de-AT" → "de"). */
export function pick(want: string, system: readonly string[] = navigator.languages): string {
  for (const w of want ? [want] : system) {
    const lower = w.toLowerCase();
    const base = ALIASES[lower.split("-")[0]] ?? lower.split("-")[0];
    const hit =
      LOCALES.find((l) => l.toLowerCase() === lower) ??
      LOCALES.find((l) => l.toLowerCase() === base) ??
      LOCALES.find((l) => l.split("-")[0].toLowerCase() === base);
    if (hit) return hit;
  }
  return "en";
}

/** Switches the interface language; right-to-left ones mirror the layout. */
export async function setLanguage(want: string) {
  const code = pick(want);
  const next = code === "en" ? {} : await files[`../locales/${code}.json`]().catch(() => ({}));
  dict = next;
  document.documentElement.lang = code;
  document.documentElement.dir = RTL.has(code.split("-")[0]) ? "rtl" : "ltr";
}

/** `text` in the interface language; `{name}` in it takes `vars.name`. Named like gettext's. */
export function _(text: string, vars?: Record<string, string | number>): string {
  const s = dict[text] || text;
  return vars ? s.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : s;
}

/** Marks text for translation where `_` can't run yet (tables made at load); show it with `_`. */
export const N_ = <T extends string>(text: T): T => text;
