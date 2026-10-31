// Run with `npm test`. Each language keeps every {placeholder} of the English, translates only
// text the interface has, and leaves nothing empty. Missing ones show in English.
import assert from "node:assert/strict";
import { test } from "node:test";
import { locales, texts } from "../../scripts/i18n.mjs";

const holes = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

test("translations fit the English they replace", () => {
  const all = texts();
  for (const [code, dict] of Object.entries(locales()) as [string, Record<string, string>][]) {
    for (const [en, tr] of Object.entries(dict)) {
      assert.ok(all.has(en), `${code}: "${en}" isn't in the interface any more`);
      assert.ok(tr.trim(), `${code}: "${en}" is empty`);
      assert.deepEqual(holes(tr), holes(en), `${code}: "${en}" lost or gained a placeholder`);
      assert.equal(/<code>/.test(tr), /<code>/.test(en), `${code}: "${en}" lost its <code>`);
    }
  }
});
