// Builds the RDP helper (rdp/, its own crate) and puts it where Tauri takes sidecars from:
// binaries/xtermium-rdp-<target triple>. Tauri then copies it next to xtermium in every build
// and ships it in the installers.
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

const host = /host: (\S+)/.exec(execSync("rustc -vV").toString())[1];
// Tauri sets this for its before-commands when building with --target.
const triple = process.env.TAURI_ENV_TARGET_TRIPLE ?? host;
const ext = process.platform === "win32" ? ".exe" : "";
// The shared target dir, or CARGO_TARGET_DIR if set. Resolved here: cargo would resolve a
// relative CARGO_TARGET_DIR from rdp/, and we'd copy an old helper from target/.
const target = resolve(process.env.CARGO_TARGET_DIR ?? "target");
mkdirSync("binaries", { recursive: true });

// Builds for one triple and returns the path of the helper.
function build(t) {
  const flag = t === host ? "" : ` --target ${t}`;
  execSync(`cargo build --release${flag}`, { cwd: "rdp", stdio: "inherit", env: { ...process.env, CARGO_TARGET_DIR: target } });
  const out = `${target}/${t === host ? "" : `${t}/`}release/xtermium-rdp${ext}`;
  copyFileSync(out, `binaries/xtermium-rdp-${t}${ext}`);
  return out;
}

if (triple === "universal-apple-darwin") {
  const arm = build("aarch64-apple-darwin");
  const x86 = build("x86_64-apple-darwin");
  execSync(`lipo -create -output binaries/xtermium-rdp-${triple} "${arm}" "${x86}"`, { stdio: "inherit" });
} else {
  build(triple);
}
