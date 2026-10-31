# xtermium: notes for agents

A terminal with a session manager (SSH, SFTP, RDP, VNC, telnet, serial, BMC consoles, websites,
tunnels) for sysadmins. Rust backend on tokio + Tauri 2, Svelte 5 UI.
README.md lists the features for users; this file is how it's built and how the owner works.

## How it fits together

- **Backend owns all state and connections** (`src/main.rs`: `App` in Tauri state, every
  `#[tauri::command]`, registered in `generate_handler!`). The UI calls commands and listens to
  events (`emit_to("main", …)`; `emit` reaches every window). Commands that create windows must be
  `async` (a sync one deadlocks on Windows).
- **Terminal tabs** (`src/term.rs`): wezterm-term VT engine; `Inner::frame()` diffs the visible rows
  and streams only changed rows (runs of same-styled cells), meta (cursor, title, scroll…) and the
  style over a per-tab `Channel`. `ui/lib/Terminal.svelte` keeps the screen model and paints it
  (WebGL glyph cache in `glyphs.ts`, Canvas2D fallback). This is the hot path: measure before and
  after changing `frame()`, `advance_bytes` or `draw()` (a throwaway `#[ignore]` timing test works).
- **RDP** runs in a helper process (`rdp/`, its own crate and Cargo.lock, IronRDP) spoken to over
  stdin/stdout (`src/rdp_proto.rs`); `src/rdp.rs` relays frames to the UI and keeps the picture.
  The helper reconnects by itself after a network loss (same trusted key, no prompt) and keeps
  input in one ordered queue, typed text paced (`PACE`); the UI sends `release` when the view
  loses the keyboard and lock-key states as `sync`.
  VNC is in-process (`src/vnc.rs`). Websites and BMC consoles are native child webviews laid over
  the tab (`src/web.rs`, `src/kvm/`), hidden while a dialog/menu overlaps them.
- **Tabs moving between windows**: a terminal or RDP tab's output channel can be swapped
  (`Tab::attach`, command `tab_attach`), so `detach_tab` opens a window `tab-<id>`
  (`ui/lib/Detached.svelte`, chosen in `ui/main.ts` by `window.__DETACHED__`) that attaches to the
  running backend tab. Views take an `attach` prop and have `release()` so unmounting doesn't end
  the session. Closing a detached window puts the tab back (Rust `on_window_event`).
- **Broadcast** (`src/broadcast.rs`), **file panel** SFTP/SMB (`src/files.rs`), **tunnels** with
  traffic (`src/ssh.rs`, `src/flow.rs`), **automation** = MCP over HTTP on 127.0.0.1 with a token
  (`src/automation.rs`; on the network, HTTPS with a self-signed certificate kept in
  `automation-cert.xtm`, plain HTTP still taken from loopback for the CLI and agent tabs) and
  the `xtermium <command>` CLI (`src/cli.rs`) calling the same tools; website tabs are driven
  by scripts run through their own view (`web.rs` `page_*`, `web_*` tools; screenshots through
  WebView2 DevTools, Windows only),
  **cloud sync** (`src/sync.rs`, end-to-end encrypted), **tray / keep running** (`src/tray.rs`),
  **one instance per data folder** (`src/instance.rs`; a second start passes on `--open-here
  <folder>` from File Explorer's "Open in xtermium", `src/explorer.rs`, HKCU registry, Windows),
  importers (`src/formats/`), and **around the app itself** (`src/support.rs`): desktop
  notifications and start-at-login (Tauri plugins), the process priority, the build number
  (`XTERMIUM_BUILD`, stamped by release.yml) with the GitHub release check, the troubleshooting
  bundle, `custom.css` in the data folder (the UI applies it; a changed one is kept only on a yes
  within 30 s), and the native picker for background files.
- **Sync commands run on the main thread.** One that may read the clipboard (`term_key`,
  `term_mouse`) is `async`: on X11 a read waits for the clipboard's owner, and when that's our own
  page the answer needs the main loop (a 3 s freeze, then an empty paste).
- **The shell's folder** (OSC 7) comes in each frame's `Meta` (`cwd`); App.svelte `metaOf` hands
  it to the file panel. Bells and OSC 9/777 notifications are ignored (agent notifications were
  dropped on purpose).

## Data and storage

- Everything on disk is encrypted with a data key kept in the OS keychain (`src/vault.rs`,
  `src/config.rs`: `<name>.xtm`, list in `FILES`; a recovery passphrase can restore the key).
- **One data folder holds everything** (`config::dir`): `~/.xtermium`, or `XTERMIUM_HOME`; older
  versions' OS config folder moves there once (`config::moved`, with the key's keychain entry).
  Never write to the OS temp or app-data folders: `logs/`, `custom.css`, `webview/` (the windows'
  browser data: the main window is built in `setup` with `create: false` in tauri.conf.json, and
  detached windows and profile-less website views pass `config::webview_dir()` too, else Tauri
  uses local app data) and `temp/<pid>/` (`config::temp`: this run's passing data, swept at the
  next start). Tests get a scratch folder from `dir()`, never the real one.
- `sessions.xtm` = `Store`: sessions, tunnels, folders, snippets, shared logins (`credential`),
  saved layouts (`layout`), and `workspace` = the other workspaces. The default workspace's data is
  the top-level fields. In memory `App.store` holds **only the workspace in use** (so most code
  needs no workspace logic) and `App.rest` the others; `Store::split/join`, `App::all/set_all`.
  The workspace in use is `config::workspace()` (persisted in `settings.workspace`, which only the
  `workspace` command changes; `save_settings` ignores the UI's copy). `save_store` carries the
  workspace a save belongs to, so a save racing a switch lands in the right one.
- Workspaces keep only sessions and folders apart. Shared logins, tunnels, snippets and layouts
  live at the top of the store for all of them: `split` hands the workspace in use those along
  with its sessions, `join` puts them back at the top, and `Store::hoist` (in `split`) moves up
  the copies older versions kept per workspace. Clone and sync are `WorkspaceOp::Clone/Sync`
  (sessions and folders only; `Store::sync_sessions` updates and adds, never removes); copies'
  website sessions get fresh profiles (`Store::own_profiles`).
- `logins.xtm`: passwords remembered at a prompt, for every workspace, device-only (`config::login /
  remember / forget`; old OS-keychain entries migrate on first use). Shared logins (Credentials) are
  in the Store and sync.
- `settings.xtm` = `Settings` (`src/settings.rs`, `#[serde(default)]`, device-only, not synced).
  `known.xtm` trusted host keys, `tabs.xtm` tabs to reopen, `sync.xtm` sync state, `lock.xtm`
  the lock's hashes and passkey (`src/lock.rs`; never sent to the UI, changed only after an unlock),
  `background.xtm`, `background-sidebar.xtm`, `background-bar.xtm` the backgrounds of terminals,
  the sidebar and the tab bar (a `data:` picture or shader source; `background_set` is the only
  writer, `save_settings` keeps each `Settings::background_of` but its `show`). `Backdrop.svelte`
  draws one under a place (its parent isolates stacking), `BackgroundPicker.svelte` picks one.
  Shaders run in `ui/lib/shader.ts`: one WebGL 2 context per window compiles each once and draws
  one frame for every view showing it.
- `restore-points/<ms>-<reason>.xtm` (`src/snapshot.rs`): a full backup's text plus saved logins,
  sealed with the data key (re-sealed by `rekey`), newest 10; `replace_all` in main.rs takes one
  before any restore. `websites/<profile>`: each website session's WebView2 data folder
  (`Kind::Browser.profile`, given by clean.rs `profiles`; unused ones swept at load). BMC consoles
  and unsaved websites are incognito.
- Adding a Store or Settings field: Rust struct + `Default`, cleaning in `src/clean.rs`
  (`sanitize`; Store fields also in `Store::merge`), the TS type in `ui/lib/api.ts`, and for Store
  arrays the `filled()` default in `ui/lib/state.svelte.ts`. Restyling terminals on a setting
  change goes through `Settings::restyles`.

## UI conventions

- Svelte 5 runes. App state in `ui/lib/state.svelte.ts` (`app`), the window in `ui/App.svelte`
  (tabs, panes, layouts, detach, dialogs), components in `ui/lib/`.
- **Look: modern VS Code, dense, in golden-ratio proportions** (the note atop `ui/app.css`;
  `DESIGN.md` spells out the whole visual system: tokens, components, named rules).
  Spacing only on the Fibonacci scale (1 2 3 5 8 13 21 34 px; padding pairs 1 : φ like
  `5px 8px`); type tokens `--t-s --t --t-l --t-xl --t-xxl` (11.5 13 14.5 16.5 18.5); heights
  `--row` 21 (list rows), `--control` 26, `--bar` 34; radii `--r-control` 3 / `--r-menu` 5 /
  `--r-window` 8, nested ones `calc(outer - padding)`; `--gap` 5; dialogs 377 / 610 / 987 wide.
  A value derived from others is written as `calc(…)` so it reads as derived.
  `ui/lib/scale.test.ts` (in `npm test`) fails on off-scale spacing or a raw font size. Colours
  `--fg --weak --line --raised --gold` (the accent; user-changeable), per-kind hues `--k-*`.
  Rounded floating panels on a darker window, icon-only tool buttons with tooltips. Only layers
  floating over the work cast a shadow, from `--shadow-float/-menu/-dialog`, and they arrive with
  `float-in` (menus excepted; app.css). Svelte transitions don't obey app.css's reduced-motion
  rule: give them `duration: 0` when it's on. Menus end
  with their destructive item (`.danger`); `.sep` starts a group. Reuse tokens; no big padding.
- Icons are Boxicons SVGs in `assets/icons` via `Icon.svelte`; session/folder icons go through
  `SessionIcon.svelte` and only those animate on hover (`.session-icon .a-*` in app.css).
- Menus: `.menu` + `{@attach menu(close, x, y)}` (keyboard-driven, kept on screen); items without
  an icon are indented to line up. Dialogs: `Modal.svelte` (takes the keyboard, Tab stays inside,
  Escape closes the top one, the last opened). Both move to `<body>` while open (`portal` in
  `menu.ts`), so a stacking context around where they're declared (the sidebar isolates its
  background) can't put them under the panes. Terminals and RDP refuse keys while any dialog/palette is open
  (`dialogTakesKeys`), and give the keyboard back only after it's gone (`refocus` waits a tick).
- Text that names another place links to it: `openSettings(page)` from `Settings.svelte`;
  `button.link` (gold) or `button.link.quiet` (a whole sentence, dotted underline). Status messages
  naming a page use `tell(text, page)`. A moment's confirmation is a toast (`toast(text)`; `copyText` copies and says so).
- Every field passes `cleanInput` / `clampNumber` (App.svelte); the backend cleans again.
- Enter does the obvious thing: in a filter field it acts on the first hit (runs the snippet, adds
  the scheme; highlight that row); a search bar closes and puts the keyboard on its first hit (the
  session's row in the tree, the setting's control, lit while the search is open), and a yes/no question focuses its
  yes button (`{@attach (el) => el.focus()}`, as Close tab), unless yes can't be undone or is a
  security call: trusting a new host key, deleting a folder or sessions, a restore, a new automation
  token, discarding an edit. Those focus Cancel/Reject (Keep editing), so Enter never does the
  irreversible thing. A handler that moves the keyboard to a terminal calls `e.preventDefault()`,
  or the Enter's keypress lands in it.
- **i18n**: all UI text through `_("…")` (or `N_("…")` in tables), keyed by the English;
  `ui/locales/<code>.json`: ar (right to left), de, el, es, fi, fr, hi, ja, nb, pl, sv, zh-CN;
  English is built in and the default (`no`/`nn` systems get nb). New text may stay untranslated (shows English). `npm test` fails on
  locale keys whose English no longer exists: when changing or removing text, delete those keys
  from every locale (keep the files' 2-space JSON format).
- Copy style: short plain sentences, second person, "Settings › Page" for places, no jargon in
  labels. Comments read the same way: full sentences saying why, `///` / `/** */` on most items,
  `ponytail:` notes on deliberate shortcuts with their ceiling.

## Automation (MCP + CLI)

New user-facing capabilities usually get a tool: add it in `automation::tools()` (schema and
description) and `call()`, a CLI word in `cli.rs` (`USAGE`, `COMMANDS`, `parse`, the `commands`
test), the list sentence in Settings › MCP, and a README line. Tools never reveal
passwords; AI-agent sessions/tabs are invisible to automation. UI-side actions are done by
emitting an event to the main window (`automation-open`, `automation-detach`, `automation-layout`).

## Build, test, run

```
npm run check                      # svelte-check, must be 0 errors 0 warnings
npm test                           # UI tests incl. locale consistency
cargo clippy --all-targets && cargo test
npm run tauri build -- --debug --no-bundle   # standalone debug exe: target/debug/xtermium.exe
```

- The owner runs `target/debug/xtermium.exe` day to day. While it runs, `target/debug` is locked:
  build and test in `CARGO_TARGET_DIR=target/work`. Never kill their process.
- Visual checks: launch the target/work exe with its own `XTERMIUM_HOME` (a scratch folder),
  `WEBVIEW2_USER_DATA_FOLDER` (scratch) and
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`, then drive/screenshot the
  webview over CDP only (Runtime.evaluate, Page.captureScreenshot with a clip, Input.insertText for
  typing; synthetic keyDown text never reaches the terminal). Never capture the desktop or send OS
  clicks. Seed data with `window.__TAURI_INTERNALS__.invoke(...)`: `boot` returns the store and
  its workspace, `save_store` takes them back, then `location.reload()`. A key press for a field or
  button is `Input.dispatchKeyEvent` keyDown (with `text: "\r"` for Enter) then keyUp. Stop only
  the target/work process afterwards, and before rebuilding it (its exe is locked while it runs).
  A hidden test window doesn't run requestAnimationFrame. Don't click links that open a URL
  (they open the owner's browser), and close test prompts by their own button: a stray
  `.backdrop button.primary` click can be Trust. No sshd here: github.com:22 shows a real unknown
  host key prompt (`known_keys` / `forget_key` reset it).
  `WEBVIEW2_USER_DATA_FOLDER` overrides every view's data folder, website profiles included: to
  check those, run without the WebView2 variables and drive it with the CLI (automation on).
- `web::tests::website_keys_named_like_shortcuts` reads the live keyboard: it fails if a key is
  held while it runs; rerun.
- `cargo fmt --check` already reports ~100 diffs on main: don't reformat whole files.

## How the owner works

- Requests come as numbered lists; do all of them, verify in the running app, then report briefly
  per item with what was tested and what couldn't be.
- "commit, merge to main and rebuild": branch `feat/<topic>` from main, commit (message body as
  bullets, ending with the Co-Authored-By line), push, `gh pr create --fill-first` with a short
  body, `gh pr merge <n> --merge --delete-branch`, switch to main and pull, check xtermium isn't
  running, then `npm run tauri build -- --debug --no-bundle`. Don't commit unless asked.
- **Releases follow semver.** `Cargo.toml`'s version is the app's (tauri.conf.json has none; keep
  package.json's in step). A PR that raises it publishes `v<version>` when merged; merges without
  a bump publish nothing (release.yml skips a version already tagged). Pre-1.0: a new feature or
  a breaking change bumps the minor, a fix the patch. Bump only when asked to release.
- The PR title and the commit bodies' bullets become the release notes (`scripts/changelog.mjs`,
  run by release.yml, everything merged since the previous tag): write them for developers
  reading what changed.
- Fix root causes in the shared path (one guard where all callers pass), keep diffs small,
  question speculative work, and leave one runnable check for non-trivial logic.
- Licensing: Apache-2.0, with the author's credit in `NOTICE`; the sync server is proprietary
  and stays out of the repo.
