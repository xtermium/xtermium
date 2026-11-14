# xtermium: notes for agents

A terminal with a session manager (SSH, SFTP, RDP, VNC, telnet, serial, BMC consoles, websites,
tunnels) for sysadmins. Rust backend on tokio + Tauri 2, Svelte 5 UI.
README.md is a light overview for users; the details (menus, limits, formats, flows) live in the
wiki, https://github.com/xtermium/xtermium/wiki. This file is how it's built and how the owner works.

## How it fits together

- **Backend owns all state and connections** (`src/main.rs`: `App` in Tauri state, every
  `#[tauri::command]`, registered in `generate_handler!`). The UI calls commands and listens to
  events (`emit_to("main", …)`; `emit` reaches every window). Commands that create windows must be
  `async` (a sync one deadlocks on Windows). Background connection tasks ask the UI for
  passwords and confirmations through `src/prompt.rs`.
- **Terminal tabs** (`src/term.rs`): wezterm-term VT engine; `Inner::frame()` diffs the visible rows
  and streams only changed rows (runs of same-styled cells), meta (cursor, title, scroll…) and the
  style over a per-tab `Channel`. `ui/lib/Terminal.svelte` keeps the screen model and paints it
  (WebGL glyph cache in `glyphs.ts`, Canvas2D fallback). This is the hot path: measure before and
  after changing `frame()`, `advance_bytes` or `draw()` (a throwaway `#[ignore]` timing test works).
- **RDP** runs in a helper process (`rdp/`, its own crate and Cargo.lock, IronRDP) spoken to over
  stdin/stdout (`src/rdp_proto.rs`); `src/rdp.rs` relays frames to the UI and keeps the picture.
  The helper reconnects by itself after a network loss (same trusted key, no prompt) and keeps
  input in one ordered queue, typed text paced (`PACE`); the UI sends `release` when the view
  loses the keyboard and lock-key states as `sync`. The picture comes over the graphics pipeline
  (egfx), held to two frames in flight until the view acks `rdp_drawn`; the clipboard is
  `rdp/src/clip.rs` (files from the desktop via `rdp/src/ole.rs`, Windows). While a view has the
  keyboard, a low-level hook (rdp.rs `grab`) takes Win+…, Alt+Tab and Ctrl+Esc for it; Windows
  calls that hook while our own window is in front only because main.rs sets
  `device_event_filter(Always)` (tao's raw input otherwise silences it): keep that line.
  VNC is in-process (`src/vnc.rs`), and so is telnet (`src/telnet.rs`, with RFC 854 option
  negotiation). Websites and BMC consoles are native child webviews laid over
  the tab (`src/web.rs`, `src/kvm/`), hidden while a dialog/menu overlaps them; for iDRAC
  consoles `src/kvm/relay.rs` is a local WebSocket-to-TLS relay that trusts only the BMC's
  pinned certificate.
- **Windows have no system title bar** (`own_bar` in main.rs: no decorations on Windows and Linux,
  the overlay style with its own three buttons on macOS). `ui/lib/TitleBar.svelte` draws the bar
  in the main window and detached ones: Tauri's `data-tauri-drag-region` moves the window, the
  buttons call the window API (`capabilities/default.json`), and its centre (App.svelte `.where`)
  names the focused tab and opens the palette, empty while locked. The page, dialogs and the lock start
  under it (`--titlebar` in app.css, 0 in full screen).
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
  **Terminal Co-op** (`src/coop.rs`: Pro people share tabs with up to two more over iroh, QUIC
  with our own relay; the sync server only introduces them, see `docs/sync.md`. A shared tab's frames
  are teed to a second sink, `Tab::share` in term.rs, rdp.rs and vnc.rs (a terminal's without the
  owner's selection: everyone selects on their own, a viewer's read by `Tab::pick`, and every
  selection travels as `Msg::Selected` for all windows to draw in the selecting person's ring
  colour), and a viewer's backend hands
  them unchanged to the channel `Terminal.svelte` / `Rdp.svelte` already decode; viewers' input goes
  through the owner's own key and mouse paths, never broadcast, and stops while xtermium is locked),
  **cloud sync** (`src/sync.rs`, end-to-end encrypted), **tray / keep running** (`src/tray.rs`),
  **one instance per data folder** (`src/instance.rs`; a second start passes on `--open-here
  <folder>` from File Explorer's "Open in xtermium", `src/explorer.rs`, HKCU registry, Windows),
  importers (`src/formats/`), **AI agent sessions' MCP servers and skills** (`src/agent_mcp.rs`:
  read from each tool's own config, turned off per session), **Settings › SSH keys**
  (`src/sshkeys.rs`: the key pairs in `~/.ssh`), path-field suggestions (`src/paths.rs`), and **around the app itself** (`src/support.rs`): desktop
  notifications and start-at-login (Tauri plugins), the process priority, the login shell's
  environment for programs the app starts on macOS and Linux (`shell_env`: PATH and exports an
  app started from the desktop lacks; Windows npm `.cmd` shims go through cmd, `term::through_cmd`), the build number
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
- **Ids.** Every session, every folder record and every workspace has an `id` of its own (16
  random bytes in hex; `Store::ids`, `own_ids`): given by `sanitize` and `use_workspace`, kept
  through renames and moves, new for a copy (a clone, a duplicate, what a workspace sync adds).
  A workspace's id and colour are `Store::id` and `Store::color` of its own record, so the UI has
  the one in use as `app.store.id`; `workspace_list` (Boot, `Workspaces`) lists all of them. Tell
  things apart by id, never by name: a tab is its session's `id` in its workspace's `id`
  (App.svelte `Tab.workspace`, `sessionId`). Sessions still name their folder by its path, and
  folders without a record have no id.
- Workspaces keep only sessions and folders apart. Shared logins, tunnels, snippets and layouts
  live at the top of the store for all of them: `split` hands the workspace in use those along
  with its sessions, `join` puts them back at the top, and `Store::hoist` (in `split`) moves up
  the copies older versions kept per workspace. Clone and sync are `WorkspaceOp::Clone/Sync`
  (sessions and folders only; `Store::sync_sessions` updates and adds, never removes); copies'
  website sessions get fresh profiles (`Store::own_profiles`).
- `logins.xtm`: passwords remembered at a prompt, for every workspace, device-only unless
  `sync_logins` is ticked (`config::login / remember / forget`; old OS-keychain entries migrate on
  first use). Shared logins (Credentials) are in the Store and sync.
- `settings.xtm` = `Settings` (`src/settings.rs`, `#[serde(default)]`, device-only unless
  `sync_settings` is ticked: then `Settings::for_cloud` goes to the sync server and `applied_over`
  applies a pulled copy over this device's own fields; the backgrounds' files travel with
  `sync_backgrounds`; docs/sync.md "Optional files").
  `known.xtm` trusted host keys, `tabs.xtm` tabs to reopen, `sync.xtm` sync state, `lock.xtm`
  the lock's hashes and passkey (`src/lock.rs`; never sent to the UI, changed only after an unlock),
  `background.xtm`, `background-sidebar.xtm`, `background-bar.xtm` the backgrounds of terminals,
  the sidebar and the tab bar (a `data:` picture or shader source; `background_set` and a sync pull
  are the only writers, `save_settings` keeps each `Settings::background_of` but its `show` and `fit`). `Backdrop.svelte`
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
  `--fg --weak --line --raised --gold` (the accent, phosphor amber; user-changeable), per-kind hues `--k-*`.
  The interface is in the system face (`--ui`, inherited from `body`); the mono (`--mono`) only
  for what is read character by character: code, commands, `kbd`, fingerprints, passcodes.
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
  `button.link` (amber) or `button.link.quiet` (a whole sentence, dotted underline). Status messages
  naming a page use `tell(text, page)`. A moment's confirmation is a toast (`toast(text)`; `copyText` copies and says so).
- Every field passes `cleanInput` / `clampNumber` (App.svelte); the backend cleans again.
- Enter does the obvious thing: in a filter field it acts on the first hit (runs the snippet, adds
  the scheme; highlight that row); a search bar closes and puts the keyboard on its first hit (the
  session's row in the tree, the setting's control, lit while the search is open), and a yes/no question focuses its
  yes button (`{@attach (el) => el.focus()}`, as Close tab), unless yes can't be undone: deleting
  a folder or sessions, a restore, a new automation token, discarding an edit. Those focus Cancel
  (Keep editing), so Enter never does the irreversible thing. Trusting a host, RDP server or BMC
  seen for the first time focuses Trust, as ssh's "yes" (the owner's call, 2026-11-14); a key
  that changed is refused whatever is answered. A handler that moves the keyboard to a terminal calls `e.preventDefault()`,
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
test), the list sentence in Settings › MCP, and its entry on the wiki's AI agents page. Tools never reveal
passwords; AI-agent sessions/tabs are invisible to automation. UI-side actions are done by
emitting an event to the main window (`automation-open`, `automation-detach`, `automation-layout`).

## Build, test, run

```
npm run check                      # svelte-check, must be 0 errors 0 warnings
npm test                           # UI tests incl. locale consistency
npm run rdp                        # the RDP helper into binaries/, once, before any plain cargo command
cargo clippy --all-targets && cargo test
CARGO_TARGET_DIR=target/work npm run tauri build -- --debug --no-bundle   # a test exe: target/work/debug/xtermium.exe
```

- Build and test in `CARGO_TARGET_DIR=target/work`. The owner uses the Scoop install, not a
  local build, so no rebuild follows a merge.
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

- **Every change that adds, changes, removes, breaks or deprecates a feature, or changes the
  project, is documented in the wiki** (https://github.com/xtermium/xtermium/wiki). It is this
  repository's GitHub wiki and the only one: no other xtermium repository keeps wiki pages. Edit
  its pages in a clone of `git@github.com:xtermium/xtermium.wiki.git` and push once the change
  has shipped. The README stays light: what xtermium is,
  install, a short feature list and links to the wiki; never add menus, limits or steps to it.
- Requests come as numbered lists; do all of them, verify in the running app, then report briefly
  per item with what was tested and what couldn't be.
- "commit and merge to main": branch `feat/<topic>` from main, commit (message body as
  bullets, ending with the Co-Authored-By line), push, `gh pr create --fill-first` with a short
  body, `gh pr merge <n> --merge --delete-branch`, switch to main and pull. The next nightly
  carries it. Don't commit unless asked.
- **Releases follow semver.** `Cargo.toml`'s version is the app's (tauri.conf.json has none; keep
  package.json's in step). A PR that raises it publishes `v<version>` when merged; merges without
  a bump publish nothing (release.yml skips a version already tagged). Pre-1.0: a new feature or
  a breaking change bumps the minor, a fix the patch. Bump only when asked to release.
- **Merged work ships as nightly builds first; releases are seldom.** `nightly.yml` builds main
  every night it has changed (all three systems, or nothing) and replaces the files of the one
  rolling `nightly` pre-release, under names without a version (`xtermium_nightly_…`, but the RPM is `xtermium-nightly-1.x86_64.rpm`); the Scoop
  bucket's `xtermium-nightly` and the site link to them. A nightly build carries the last
  release's version, `XTERMIUM_NIGHTLY` (its commit) makes it say "nightly" in Settings › About,
  and its update check reads the commit on the first line of that pre-release's notes
  (`support.rs` `latest`): keep that line's shape in nightly.yml. A release follows once the
  nightlies have held up.
- The PR title and the commit bodies' bullets become the release notes (`scripts/changelog.mjs`,
  run by release.yml, everything merged since the previous tag): write them for developers
  reading what changed.
- Fix root causes in the shared path (one guard where all callers pass), keep diffs small,
  question speculative work, and leave one runnable check for non-trivial logic.
- Licensing: Apache-2.0, with the author's credit in `NOTICE`; the sync server is proprietary
  and stays out of the repo.
