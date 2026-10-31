import assert from "node:assert/strict";
import { test } from "node:test";
import { catalogueName, parseScheme } from "./schemes.ts";

test("terminal.sexy and Windows Terminal schemes", () => {
  const colors = Array.from({ length: 16 }, (_, i) => `#${i.toString(16).repeat(6)}`);
  const sexy = parseScheme(JSON.stringify({ name: "Hybrid", color: colors, foreground: "#C5C8C6", background: "#1d1f21" }), "x");
  assert.equal(sexy.name, "Hybrid");
  assert.equal(sexy.ansi[15], 0xffffff);
  assert.equal(sexy.fg, 0xc5c8c6);
  assert.equal(sexy.cursor, sexy.fg);

  const wt = parseScheme(
    JSON.stringify({ name: "WT", background: "#000", foreground: "#fff", cursorColor: "#f00", black: "#111", red: "#222", green: "#333", yellow: "#444", blue: "#555", purple: "#666", cyan: "#777", white: "#888", brightRed: "#abc" }),
    "x",
  );
  assert.deepEqual([wt.bg, wt.fg, wt.cursor], [0, 0xffffff, 0xff0000]);
  assert.equal(wt.ansi[9], 0xaabbcc, "bright red");
  assert.equal(wt.ansi[8], 0x111111, "a missing bright colour falls back to the normal one");

  assert.equal(parseScheme(JSON.stringify({ color: colors, foreground: "#fff", background: "#000" }), "file").name, "file");
  assert.throws(() => parseScheme("{}", "x"), /Expected/);
  assert.throws(() => parseScheme(JSON.stringify({ color: colors, foreground: "nope", background: "#000" }), "x"), /colour/);
  assert.equal(catalogueName("dist/schemes/base16/ashes.dark.json"), "ashes dark (base16)");
});
