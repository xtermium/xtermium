---
name: xtermium-testing
description: Use when running or writing tests for the xtermium app, deciding which checks a change needs, a test fails, hangs or flakes, an SSH, SFTP, RDP, passkey or lock path needs exercising without the real thing, or a change must be seen working in the running app through a visual check over the webview's DevTools protocol.
---

# Testing xtermium

## Overview

Four commands cover the app: `npm run check` (types), `npm test` (UI and script tests, including the locale and spacing-scale rules), `cargo clippy --all-targets` and `cargo test` (the backend, including an end-to-end SSH run against an in-process server). A change people can see is then proven in the running app: a standalone debug exe with its own scratch data folder, driven and screenshotted over the webview's DevTools protocol (CDP) only. This skill lists what each command covers, the ignored tests, the visual-check recipe and the gotchas that make tests hang or flake.

## When to use

- Before opening a pull request (which checks to run).
- A test fails on locale keys or spacing, hangs on Windows, or fails now and then.
- A change needs an SSH or SFTP server, a host-key prompt, a passkey or the lock.
- A UI change must be seen in the real app, or a screenshot is needed.

## Quick reference

| Command | Covers |
|---|---|
| `npm run check` | svelte-check over the UI; must print 0 errors and 0 warnings (`--fail-on-warnings`) |
| `npm test` | `node --test ui/lib/ scripts/`: the UI and script tests below |
| `cargo clippy --all-targets && cargo test` | the backend; run `npm run rdp` once first |
| `(cd rdp && cargo clippy --all-targets)` | the RDP helper, when `rdp/` changed |
| `cargo audit && (cd rdp && cargo audit) && npm audit` | advisories, after dependency changes |

| `npm test` file | Fails when |
|---|---|
| `ui/lib/locales.test.ts` | a locale drops a `{placeholder}` of the English, keeps a key whose English the interface no longer has, or leaves one empty |
| `ui/lib/scale.test.ts` | spacing (padding, margin, gap, inset, `top`/`left`/`right`/`bottom`, border-radius) in `ui/app.css`, `ui/App.svelte` or a `ui/lib/*.svelte` style block is off the Fibonacci scale, or a `font-size` is raw px |
| `ui/lib/actions.test.ts` | shortcut chords, default-shortcut collisions, fuzzy matching, quick connect, snippet placeholders, tiling and pane focus or resize break |
| `ui/lib/schemes.test.ts` | terminal.sexy or Windows Terminal scheme import breaks |
| `ui/lib/dashicons.test.ts` | Dashboard Icons lookup breaks |
| `ui/lib/serial.test.ts` | the one-run-at-a-time helper breaks |
| `scripts/changelog.test.mjs` | commit bodies stop turning into release-note items |

## What `cargo test` covers

- An end-to-end SSH run against an in-process server (host-key prompt, password auth, shell, SFTP and a local tunnel), the importer, the RDP wire protocol and key scancodes, the SOCKS5 handshake, broadcast key encoding, highlight rules (the wiki's Development page, "Checks").
- Automation guards in `src/automation.rs`: `only_local_callers` (Host and Origin), the oversized request line, `locked_xtermium_refuses_tools`; the CLI's `commands` test in `src/cli.rs`.
- The data folder's 0700 mode on Linux and macOS (`src/config.rs`).

Ignored tests (run by name with `-- --ignored --nocapture`):

| Test | What | Run |
|---|---|---|
| `term::tests::bench_flood` | where a coloured log flood spends its time; measure before and after touching `frame()` or logging | `cargo test --release bench_flood -- --ignored --nocapture` |
| `ssh::tests::bench_ciphers` | throughput per SSH cipher, to choose the default order | `cargo test --release bench_ciphers -- --ignored --nocapture` |
| `files::tests::smb_live` | SMB against a real Windows host with its saved RDP session's login; writes, reads back, renames and deletes one file in `C:\Windows\Temp` | `XT_SMB_HOST=<host> cargo test smb_live -- --ignored --nocapture` |

## Visual check (summary)

1. Build the standalone debug exe: `CARGO_TARGET_DIR=target/work npm run tauri build -- --debug --no-bundle` → `target/work/debug/xtermium.exe`.
2. Launch it with a scratch `XTERMIUM_HOME`, a scratch `WEBVIEW2_USER_DATA_FOLDER` and `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`.
3. Drive and screenshot it over CDP only: `Runtime.evaluate`, `Page.captureScreenshot` with a clip, `Input.insertText` for typing, `Input.dispatchKeyEvent` for key presses. Seed data with `window.__TAURI_INTERNALS__.invoke("boot")` and `invoke("save_store", …)`, then `location.reload()`.
4. Never capture the desktop or send OS clicks or keys. Stop only the process you started.

Full commands, seeding and traps: `references/visual-check.md`.

## Gotchas

| Situation | What works |
|---|---|
| A test exe opens your real data, or just raises another xtermium's window | one xtermium runs per data folder (`src/instance.rs`); a second start on the same folder asks the running one to show its window and quits. Always give a test exe its own scratch `XTERMIUM_HOME`. The Scoop manifest sets `XTERMIUM_HOME` as a user environment variable, so on a machine with the Scoop install every xtermium started without an override shares that folder |
| SSH or SFTP needs a server and none runs locally | a throwaway Python `asyncssh` server: password auth, a `process_factory` shell, `line_editor=False`, and catch `asyncssh.TerminalSizeChanged` from `stdin.read()` (or the fake shell dies on xtermium's first resize); for the file panel add `sftp_factory` with `chroot` and an overridden `realpath`. Start it as a background job you can stop. Docker containers on 127.0.0.1 also work as demo hosts |
| Git Bash turns `/home/sotiris/...` into a Windows path | `MSYS_NO_PATHCONV=1` |
| A Rust test that runs a program through `run_argv` / `run_pty` hangs on Windows, then exits `0xC0000142` | its feed must answer ConPTY's startup `ESC[6n` (cursor position request) with `ESC[1;1R` through `Ctl::Input`; the model is `term::tests::exit_code_is_kept` |
| A real unknown-host-key prompt | connect to `github.com:22`; the `known_keys` / `forget_key` commands reset trust |
| `web::tests::website_keys_named_like_shortcuts` fails | it reads the live keyboard; a key was held while it ran: rerun |
| `automation::tests::agents_get_the_server` fails now and then | known flaky (races on the `ENDPOINT` global): rerun |
| Windows Hello option missing | the machine has no camera or fingerprint reader (`DeviceNotPresent`); report Hello as untested, not as a bug |
| Passkeys | CDP `WebAuthn.enable` plus `WebAuthn.addVirtualAuthenticator`, in the same CDP session as the page actions (the authenticator dies with the socket); WebView2's origin `http://tauri.localhost` is a secure context |
| Idle-lock test | `GetLastInputInfo` is system-wide: it fires within seconds only if nobody touches the computer |
| Checking a `code` (VS Code) launch | put a fake `code.cmd` first on the test exe's PATH that logs its arguments (for example `node "%~dp0args.js" %*`) |
| `cargo fmt --check` reports about 100 diffs | they are pre-existing on `main`: don't reformat files; format only your change |
| RDP needs a server and no Windows host is at hand | an xrdp stand-in: `docker run -d --rm -p 127.0.0.1:13389:3389 scottyhardy/docker-remote-desktop` (login `ubuntu` / `ubuntu`); the picture is right there, but it never answers a resize, so check resizing against a real host |

## Common mistakes

- Launching a test exe without a scratch `XTERMIUM_HOME`.
- Running plain `cargo test` before `npm run rdp` (Tauri checks the bundled helper; the build fails).
- Building the test exe into `target/debug` while another build runs from there.
- Sending synthetic `keyDown` text to a terminal over CDP: it never reaches it; use `Input.insertText`.
- Clicking a link in the test app (it opens a browser), or a stray `.backdrop button.primary` (it can be Trust on a host-key prompt).
- Capturing the desktop or killing every `xtermium` process instead of the one you started.
- Reporting a flaky or environment-bound failure as a regression without a rerun.
- Saying a change works because tests pass, when it was never seen in the running app.

## Read next

- `references/visual-check.md`: the full CDP recipe.
- `CLAUDE.md` "Build, test, run": the source of the recipe.
- `CONTRIBUTING.md`: the checks a pull request needs.
- `DESIGN.md`: what a visual check should find.
- `xtermium-developing`: conventions the tests enforce.
- `xtermium-designing`: screenshot rules (what may be shown, demo data).
- `xtermium-mcp`: driving a test instance through the CLI instead.
