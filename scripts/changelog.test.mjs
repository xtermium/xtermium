import { test } from "node:test";
import assert from "node:assert/strict";
import { items } from "./changelog.mjs";

test("commit bodies become release-note items", () => {
  const body = [
    "- Desktop notifications (off by default): new release, dropped connection, and optionally",
    "  an agent done or waiting",
    "  - nested detail",
    "",
    "A plain paragraph",
    "wrapped over two lines.",
    "",
    "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>",
  ].join("\n");
  assert.deepEqual(items(body), [
    "- Desktop notifications (off by default): new release, dropped connection, and optionally an agent done or waiting",
    "  - nested detail",
    "- A plain paragraph wrapped over two lines.",
  ]);
  assert.deepEqual(items(""), []);
});
