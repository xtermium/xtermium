// Run with `npm test`: node runs TypeScript directly.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ACTIONS, bindings, follow, cell, chord, defaultKeys, even, fill, find, type Filter, keysOf, neighbour, nudge, type Os, ownWindow, placeholders, quickConnect, resize, score, shown, TERMINAL_KEYS, tile, wheelAction, wheelChord } from "./actions.ts";

const key = (code: string, key: string, mods: Partial<KeyboardEvent> = {}) =>
  chord({ code, key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods } as KeyboardEvent);

test("chords name keys by position, modifiers in a fixed order", () => {
  assert.equal(key("KeyP", "P", { ctrlKey: true, shiftKey: true }), "Ctrl+Shift+P");
  assert.equal(key("Digit1", "!", { altKey: true, shiftKey: true }), "Alt+Shift+1");
  assert.equal(key("KeyQ", "a", { ctrlKey: true }), "Ctrl+Q", "AZERTY: same physical key");
  assert.equal(key("Comma", ",", { ctrlKey: true }), "Ctrl+,");
  assert.equal(key("Tab", "Tab", { ctrlKey: true }), "Ctrl+Tab");
  assert.equal(key("ShiftLeft", "Shift", { shiftKey: true }), "");
});

const wheel = (mods: Partial<WheelEvent> = {}) => ({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods }) as WheelEvent;

test("a wheel turn needs a modifier to be a chord, and Ctrl+wheel zooms the tab by default", () => {
  assert.equal(wheelChord(wheel()), "", "a plain wheel scrolls and never binds");
  assert.equal(wheelChord(wheel({ ctrlKey: true })), "Ctrl+Wheel");
  assert.equal(wheelChord(wheel({ metaKey: true, shiftKey: true, ctrlKey: true, altKey: true })), "Ctrl+Alt+Shift+Meta+Wheel", "modifiers in chord()'s order");
  assert.equal(wheelAction(wheel({ ctrlKey: true }), {}), "tab");
  assert.equal(wheelAction(wheel({ ctrlKey: true, shiftKey: true }), {}), null, "every terminal's zoom is off until bound");
  assert.equal(wheelAction(wheel({ ctrlKey: true, shiftKey: true }), { "wheel-zoom-all": "Ctrl+Shift+Wheel" }), "all");
  assert.equal(wheelAction(wheel({ ctrlKey: true }), { "wheel-zoom-tab": "" }), null, "cleared");
  assert.equal(wheelAction(wheel(), { "wheel-zoom-tab": "Wheel" }), null, "a bare wheel stays the terminal's");
  assert.equal(bindings({}).get("Ctrl+Wheel"), undefined, "no key press runs a wheel action");
});

test("default shortcuts don't collide, and overrides apply", () => {
  const all = ACTIONS.map((a) => a.keys).filter(Boolean);
  assert.equal(new Set(all).size, all.length);
  const b = bindings({ palette: "Ctrl+K", find: "" });
  assert.equal(b.get("Ctrl+K"), "palette");
  assert.equal(b.get("Ctrl+Shift+P"), undefined);
  assert.equal([...b.values()].includes("find"), false);
});

test("each system's defaults are chords a key press makes, apart from each other and the terminal's", () => {
  for (const os of ["windows", "macos", "linux"] as Os[]) {
    const all = ACTIONS.map((a) => defaultKeys(a, os)).filter(Boolean);
    assert.equal(new Set(all).size, all.length, `${os}: no two actions share keys`);
    for (const k of all) {
      // chord()'s order, Ctrl Alt Shift Meta, or bindings() never finds it
      assert.match(k, /^(Ctrl\+)?(Alt\+)?(Shift\+)?(Meta\+)?[^+]+$/, `${os}: ${k}`);
      assert.equal(TERMINAL_KEYS[k], undefined, `${os}: ${k} is the terminal's`);
    }
  }
  assert.equal(keysOf("palette", {}, "windows"), "Ctrl+Shift+P", "Windows keeps its keys");
  assert.equal(bindings({}, "macos").get("Meta+T"), "local-shell");
  assert.equal(bindings({}, "macos").get("Meta+1"), "tab-1");
  assert.equal(keysOf("palette", { palette: "Ctrl+K" }, "macos"), "Ctrl+K", "your change wins on every system");
});

test("macOS leaves Ctrl and Option to the terminal; Linux leaves GTK its Unicode keys", () => {
  for (const a of ACTIONS) {
    const k = defaultKeys(a, "macos");
    assert.ok(!k || k.includes("Meta") || ["Ctrl+Tab", "Ctrl+Shift+Tab", "F6"].includes(k), `${a.id}: ${k}`);
  }
  assert.ok(!ACTIONS.some((a) => defaultKeys(a, "linux") === "Ctrl+Shift+U"));
});

test("chords are written the macOS way there, as stored elsewhere", () => {
  assert.equal(shown("Shift+Meta+P", "macos"), "⇧⌘P");
  assert.equal(shown("Ctrl+Meta+ArrowLeft", "macos"), "⌃⌘←");
  assert.equal(shown("Ctrl+Shift++", "macos"), "⌃⇧+", "the plus key");
  assert.equal(shown("F6", "macos"), "F6");
  assert.equal(shown("", "macos"), "");
  assert.equal(shown("Ctrl+Shift+P", "windows"), "Ctrl+Shift+P");
});

test("fuzzy matching", () => {
  assert.ok(score("prd db", "Prod/DB/db1") > 0);
  assert.equal(score("xyz", "Prod/DB/db1"), -1);
  assert.ok(score("db", "db1") > score("db", "dashboard"), "consecutive beats scattered");
  assert.ok(score("web", "Production/Web/web-01") > 0);
  assert.equal(score("web", "Show or hide the status bar"), -1, "letters scattered across the text aren't a hit");
  assert.ok(score("dbprim", "db-primary") > 0, "an abbreviation across a separator still is");
  assert.ok(score("SMDatabase", "Customers/Acme/SMDatabase") > 0, "the first s of the text isn't where the word starts");
  assert.ok(score("pc.home", "Personal/Computers/pc.home.arpa (via bastion)") > 0);
});

test("quick connect", () => {
  assert.deepEqual(quickConnect("me@192.0.2.15:2222"), { type: "ssh", host: "192.0.2.15", port: 2222, user: "me" });
  assert.deepEqual(quickConnect("rdp CORP\\admin@dc1.corp"), { type: "rdp", host: "dc1.corp", port: 3389, user: "CORP\\admin" });
  assert.deepEqual(quickConnect("192.0.2.19:3389"), { type: "rdp", host: "192.0.2.19", port: 3389, user: "" });
  assert.deepEqual(quickConnect("ssh://[::1]:22"), { type: "ssh", host: "::1", port: 22, user: "" });
  assert.deepEqual(quickConnect("telnet switch1"), { type: "telnet", host: "switch1", port: 23, user: "" });
  assert.deepEqual(quickConnect("192.0.2.11:23"), { type: "telnet", host: "192.0.2.11", port: 23, user: "" });
  assert.deepEqual(quickConnect("vnc://kiosk:5901"), { type: "vnc", host: "kiosk", port: 5901, user: "" });
  assert.deepEqual(quickConnect("192.0.2.17:5900"), { type: "vnc", host: "192.0.2.17", port: 5900, user: "" });
  assert.equal(quickConnect("prod"), null, "a word is a search");
  assert.equal(quickConnect("open settings"), null);
  assert.equal(quickConnect("host:99999"), null);
});

test("snippet placeholders", () => {
  const text = "ssh {{user}}@{{ host }} -p {{port}} # again {{user}}";
  assert.deepEqual(placeholders(text), ["user", "host", "port"]);
  assert.equal(fill(text, { user: "me", host: "db1", port: "22" }), "ssh me@db1 -p 22 # again me");
  assert.equal(fill("echo {{missing}}!", {}), "echo !");
  assert.deepEqual(placeholders("awk '{print $1}' {{}} {{a}"), [], "braces of other kinds are text");
});

test("tiles fill the area", () => {
  const rows = (n: number) => {
    const tops = new Map<number, number>();
    for (let i = 0; i < n; i++) tops.set(tile(n, i)[1], (tops.get(tile(n, i)[1]) ?? 0) + 1);
    return [...tops.values()];
  };
  assert.deepEqual(tile(1, 0), [0, 0, 100, 100]);
  assert.deepEqual(rows(2), [2]);
  assert.deepEqual(rows(3), [2, 1]);
  assert.deepEqual(rows(6), [3, 3]);
  assert.deepEqual(rows(7), [3, 2, 2]);
  assert.deepEqual(rows(17), [5, 4, 4, 4]);
  for (const n of [1, 2, 3, 5, 7, 10, 17, 30]) {
    const area = Array.from({ length: n }, (_, i) => tile(n, i)).reduce((a, [, , w, h]) => a + w * h, 0);
    assert.ok(Math.abs(area - 10000) < 1e-6, `${n} tiles cover it all`);
  }
  assert.deepEqual(tile(3, 2), [0, 50, 100, 50], "a lone last tile spans the row");
});

test("focus moves to the pane next to it", () => {
  const grid: [number, number, number, number][] = [[0, 0, 50, 50], [50, 0, 50, 50], [0, 50, 50, 50], [50, 50, 50, 50]];
  assert.deepEqual([neighbour(grid, 0, [1, 0]), neighbour(grid, 0, [0, 1]), neighbour(grid, 3, [-1, 0]), neighbour(grid, 3, [0, -1])], [1, 2, 2, 1]);
  assert.equal(neighbour(grid, 0, [-1, 0]), -1, "edge");
  assert.equal(neighbour([[0, 0, 100, 100]], 0, [1, 0]), -1, "one pane");
  // 7 tiles: rows of 3, 2, 2. Down from the middle of the top row lands on a pane under it.
  const seven = Array.from({ length: 7 }, (_, i) => tile(7, i));
  assert.ok([3, 4].includes(neighbour(seven, 1, [0, 1])));
  assert.equal(neighbour(seven, 2, [0, 1]), 4);
  assert.equal(neighbour(seven, 3, [1, 0]), 4);
});

test("resized panes keep filling the area", () => {
  // Two panes side by side, the line between them dragged a quarter to the right.
  const sizes = even([2]);
  sizes.cols[0] = nudge(sizes.cols[0], 1, 0.25);
  assert.deepEqual([cell([2], sizes, 0), cell([2], sizes, 1)], [[0, 0, 75, 100], [75, 0, 25, 100]]);
  // Dragged past the other pane: it keeps a tenth.
  assert.deepEqual(nudge([0.5, 0.5], 1, 0.9).map((v) => Math.round(v * 100)), [90, 10]);
  // 2 × 2 with the rows moved: the bottom row starts where the top one ends.
  const grid = even([2, 2]);
  grid.rows = nudge(grid.rows, 1, -0.2);
  assert.deepEqual(cell([2, 2], grid, 3).map((v) => Math.round(v)), [50, 30, 50, 70]);
});

test("a pane's edge moves the way of the arrow", () => {
  const close = (a: number[], b: number[]) => a.every((x, i) => Math.abs(x - b[i]) < 1e-9);
  // Side by side: the line between them goes right from either pane, and back left.
  assert.ok(close(resize([2], even([2]), 0, 0.1, 0)!.cols[0], [0.6, 0.4]));
  assert.ok(close(resize([2], even([2]), 1, 0.1, 0)!.cols[0], [0.6, 0.4]));
  assert.ok(close(resize([2], even([2]), 1, -0.1, 0)!.cols[0], [0.4, 0.6]));
  // Three across: the middle one moves its right edge; never below a tenth.
  assert.ok(close(resize([3], even([3]), 1, 0.1, 0)!.cols[0], [1 / 3, 1 / 3 + 0.1, 1 / 3 - 0.1]));
  assert.ok(close(resize([2], even([2]), 0, 0.9, 0)!.cols[0], [0.9, 0.1]));
  // A 2 x 2 grid: down moves the line between the rows, across only the pane's own row.
  const g = resize([2, 2], even([2, 2]), 3, 0, 0.2)!;
  assert.ok(close(g.rows, [0.7, 0.3]) && close(g.cols[1], [0.5, 0.5]));
  assert.ok(close(resize([2, 2], even([2, 2]), 3, -0.1, 0)!.cols[1], [0.4, 0.6]));
  // No neighbour that way: side by side panes have no edge up or down, one pane none at all.
  assert.equal(resize([2], even([2]), 0, 0, 0.1), null);
  assert.equal(resize([1], even([1]), 0, 0.1, 0), null);
});

test("the interface runs only in xtermium's own windows, never framed", () => {
  assert.ok(ownWindow("main", false) && ownWindow("tab-7", false));
  assert.ok(!ownWindow("web3", false), "a website tab's view");
  assert.ok(!ownWindow("main", true), "framed by a page");
});

test("a palette filter keeps its own items, and Hosts searches the address alone", () => {
  const items: { label: string; hint: string; in?: Filter[] }[] = [
    { label: "Prod/db1", hint: "me@10.0.0.5:22", in: ["sessions", "hosts"] },
    { label: "Run snippet: uptime", hint: "uptime", in: ["snippets"] },
    { label: "Settings", hint: "" },
  ];
  assert.equal(find(items, "", null).length, 3);
  assert.deepEqual(find(items, "", "snippets").map((it) => it.label), ["Run snippet: uptime"]);
  assert.deepEqual(find(items, "prod", "sessions").map((it) => it.label), ["Prod/db1"]);
  assert.equal(find(items, "prod", "hosts").length, 0, "a name isn't an address");
  assert.equal(find(items, "10.0", "hosts").length, 1);
  assert.equal(find(items, "set", "sessions").length, 0, "actions are in no filter");
});

test("the palette finds a session by its folder's tag", () => {
  const items = [
    { label: "Lab/web1", hint: "10.0.0.7", tags: "staging", in: ["sessions"] as Filter[] },
    { label: "Lab/web2", hint: "10.0.0.8", in: ["sessions"] as Filter[] },
  ];
  assert.deepEqual(find(items, "staging", "sessions").map((it) => it.label), ["Lab/web1"]);
  assert.equal(find(items, "staging", "hosts").length, 0, "Hosts searches the address alone");
});

test("broadcast's panes are the ticked tabs, and no other", () => {
  assert.deepEqual(follow([], [1, 2, 3]), [1, 2, 3], "it starts in the strip's order");
  assert.deepEqual(follow([3, 1, 2], [1, 3]), [3, 1], "unticking hides that pane, the rest stay put");
  assert.deepEqual(follow([3, 1], [1, 2, 3]), [3, 1, 2], "ticking shows it again, last");
  assert.deepEqual(follow([1, 2], []), [null], "none ticked leaves one empty pane");
  assert.deepEqual(follow([null, 7], [7]), [7], "an untiled layout's empty pane goes");
});
