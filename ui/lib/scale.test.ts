// Run with `npm test`. Spacing and type follow the golden ratio (the note atop ui/app.css):
// room is a Fibonacci number of pixels, and text sizes are the type tokens. A value worked out
// from others (`calc(…)`) or a token is fine; a stray 4px or 12px is not.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

const FIBONACCI = new Set([0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89]);
const SPACING = /^\s*((?:padding|margin|gap|row-gap|column-gap|inset)(?:-[a-z-]+)?|top|left|right|bottom|border-radius)\s*:\s*([^;{}]+);/;

/** Every stylesheet line of the interface, with where it is. */
function lines(): [string, string][] {
  const dir = "ui/lib/";
  const files = ["ui/app.css", "ui/App.svelte", ...readdirSync(dir).filter((f) => f.endsWith(".svelte")).map((f) => dir + f)];
  return files.flatMap((f) => {
    const src = readFileSync(f, "utf8");
    const at = f.endsWith(".css") ? 0 : src.indexOf("<style>");
    if (at < 0) return [];
    const first = src.slice(0, at).split("\n").length;
    return src
      .slice(at)
      .split("\n")
      .map((l, i) => [`${f}:${first + i}`, l] as [string, string]);
  });
}

test("spacing is on the Fibonacci scale", () => {
  for (const [where, line] of lines()) {
    const m = line.match(SPACING);
    if (!m || m[2].includes("calc(")) continue;
    for (const [, px] of m[2].matchAll(/(?<![\w.-])(\d+(?:\.\d+)?)px/g)) assert.ok(FIBONACCI.has(+px), `${where}: ${line.trim()}`);
  }
});

test("text sizes are the type scale's", () => {
  for (const [where, line] of lines()) {
    const m = line.match(/^\s*font-size\s*:\s*([^;]+);/);
    if (m) assert.match(m[1], /^(var\(--t[-\w]*\)|inherit|[\d.]+em|clamp\(.*\))$/, `${where}: ${line.trim()}`);
  }
});
