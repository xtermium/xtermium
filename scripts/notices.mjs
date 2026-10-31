// Writes THIRD-PARTY-NOTICES.txt, which the installers carry next to LICENSE and NOTICE
// (tauri.conf.json `bundle.resources`): the licences of everything in xtermium that others wrote.
// The Rust crates of both programs (the app, and the RDP helper in rdp/, on every OS in
// about.toml) come from cargo-about; the JavaScript and fonts in the UI, from a build of it; the
// rest is listed in OTHERS. release.yml runs it before every build. After changing dependencies,
// run it and commit the result:
//
//   cargo install --locked cargo-about --features cli
//   npm run notices
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { build } from "vite";

const OURS = new Set(["xtermium", "xtermium-rdp"]);
const RULE = "-".repeat(80);
/** Licence texts read on Windows and elsewhere compare equal. */
const clean = (text) => text.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();
/** Text in lines of at most 100 characters, broken at spaces. */
const wrap = (text) =>
  text
    .split(" ")
    .reduce((lines, word) => (lines.length && lines.at(-1).length + word.length < 100 ? lines.with(-1, `${lines.at(-1)} ${word}`) : [...lines, word]), [])
    .join("\n");

// Rust: each licence text once, with every crate shipped under it.
const groups = new Map(); // text -> { name, id, crates }
for (const manifest of ["Cargo.toml", "rdp/Cargo.toml"]) {
  const dir = mkdtempSync(join(tmpdir(), "notices-"));
  const out = join(dir, "about.json");
  // -o, not stdout: cargo-about refuses to be redirected under PowerShell.
  execFileSync("cargo", ["about", "generate", "--format", "json", "--fail", "--locked", "-c", "about.toml", "-m", manifest, "-o", out], { stdio: "inherit" });
  const { licenses } = JSON.parse(readFileSync(out, "utf8"));
  rmSync(dir, { recursive: true });
  for (const l of licenses) {
    const text = clean(l.text);
    const g = groups.get(text) ?? { name: l.name, id: l.id, crates: new Set() };
    for (const u of l.used_by) if (!OURS.has(u.crate.name)) g.crates.add(`${u.crate.name}@${u.crate.version}`);
    if (g.crates.size) groups.set(text, g);
  }
}
const sorted = (set) => [...set].sort((a, b) => a.localeCompare(b, "en"));
const rust = [...groups]
  .map(([text, g]) => ({ text, ...g, crates: sorted(g.crates) }))
  .sort((a, b) => a.id.localeCompare(b.id, "en") || a.crates[0].localeCompare(b.crates[0], "en"))
  .map((g) => `${wrap(`${g.name} (${g.id}), for ${g.crates.join(", ")}`)}\n\n${g.text}`);

// JavaScript and fonts: the packages a build of the UI takes code or files from.
const packages = new Set();
const add = (path) => {
  const dir = /^(.*node_modules[\/](?:@[^\/]+[\/])?[^\/]+)/.exec(path)?.[1];
  if (dir) packages.add(resolve("ui", dir)); // asset paths are relative to the UI's root
};
await build({
  logLevel: "silent",
  build: { write: false },
  plugins: [
    {
      name: "notices",
      generateBundle(_, bundle) {
        for (const out of Object.values(bundle)) for (const path of out.type === "chunk" ? out.moduleIds : out.originalFileNames) add(path);
      },
    },
  ],
});
const js = sorted(packages).map((dir) => {
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  // Sorted: a folder's order differs between systems. A .spdx file names the licence, not its text.
  const file = readdirSync(dir)
    .sort()
    .find((f) => /^(licen[cs]e|copying)\b/i.test(f) && !f.endsWith(".spdx"));
  if (!file) throw new Error(`${pkg.name} has no licence file to quote`);
  return `${pkg.name} ${pkg.version} (${pkg.license})\n\n${clean(readFileSync(join(dir, file), "utf8"))}`;
});

// What neither finds: [what, licence file or null].
const OTHERS = [
  ["Boxicons (https://boxicons.com): the interface's icons, in assets/icons.", "assets/icons/LICENSE-boxicons"],
  [
    "Dashboard Icons (https://github.com/homarr-labs/dashboard-icons), Apache License 2.0, whose text is in LICENSE: the icon picker's catalogue of names and aliases, ui/lib/dashicons.json. The pictures load from its CDN when shown.",
    null,
  ],
  ["The built-in colour schemes take their colours from the Dracula, Solarized, Gruvbox, Nord, One Dark, Tokyo Night and Catppuccin themes, by their authors.", null],
];
const others = OTHERS.map(([what, file]) => (file ? `${wrap(what)}\n\n${clean(readFileSync(file, "utf8"))}` : wrap(what)));

const part = (title, entries) => [`${"=".repeat(80)}\n${title}\n${"=".repeat(80)}`, entries.join(`\n\n${RULE}\n\n`)].join("\n\n");
writeFileSync(
  "THIRD-PARTY-NOTICES.txt",
  [
    "xtermium: third-party notices",
    wrap("xtermium is under the Apache License 2.0 (LICENSE), with its author's credit in NOTICE. It includes the work of others, listed here, each under the licence quoted with it. Written by scripts/notices.mjs: don't edit by hand."),
    part("Rust crates, in the app and in its RDP helper", rust),
    part("JavaScript and fonts in the interface", js),
    part("Icons, data and colours", others),
  ].join("\n\n") + "\n",
);
console.log(`THIRD-PARTY-NOTICES.txt: ${rust.length} Rust licence texts, ${js.length} JavaScript packages, ${others.length} others.`);
