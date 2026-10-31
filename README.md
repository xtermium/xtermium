# xtermium

A terminal with a built-in session manager for Windows, macOS and Linux. The backend is Rust on tokio: WezTerm's VT engine (kitty, sixel and OSC 1337 images), SSH, SFTP, RDP and tunnels. The interface is TypeScript and Svelte in a Tauri window.

Homepage: https://xtermium.com · Source: https://github.com/xtermium/xtermium

## Features

- **Sessions:** folder tree with search (Enter puts you on the session in the tree), edit, duplicate and delete (Ctrl+click and Shift+click pick several, to drag into a folder, delete, or move to another workspace together), plus a start dashboard. Sessions can be imported from a bookmarks `.ini` file: folders, per-session font, colours and TERM, jump hosts and tunnels. Stored passwords are not imported.
- **Folders:** right-click one to rename it, tag it (search finds every session under a matching tag), add a session to it, delete it, or export it to a passphrase-protected `.xtermium` file that Import reads back.
- **Shared logins:** a user, password and key kept once (command palette › Shared logins, or Manage… next to a session's Login) and picked by any number of SSH, RDP, VNC and WinRM sessions, jump hosts and tunnels. Change the password once; every session uses the new one. A login can also carry an **SSH gateway** (host, port, user, password or key): SSH sessions without a jump host of their own use it as one, RDP and VNC reach their host through a port forwarded over it, and website sessions (which can pick a login for this) send every request through it as a SOCKS proxy, internal host names included. Stored encrypted with the sessions and synced with them; never written into exports other than the passphrase-protected .xtermium file.
- **Workspaces:** each holds its own sessions and folders; shared and saved logins, tunnels, snippets, layouts, trusted keys and settings are the same in all of them. Switch from the dropdown next to the sidebar's search; Settings › Workspaces adds, renames and deletes them. Right-click sessions or folders › Move to workspace… takes them to another. **Clone** copies a workspace's sessions and folders ("Work (clone)"). **Sync** brings one workspace's sessions and folders into another: what both have is updated, what's new is added, nothing is removed, and a restore point is kept first. They all sync and go into backups.
- **Encrypted storage:** everything xtermium stores is encrypted on disk (see [Configuration](#configuration)).
- **Lock:** for stepping away from the computer, Settings › Security sets a password, a PIN, a passkey (a security key or the computer's own) and Windows Hello (face, fingerprint or Windows' PIN) to unlock with. Locked, every window shows only the lock screen while the sessions keep running. It locks when xtermium starts, with Ctrl+Shift+K or the lock beside Settings, and after the minutes you pick without input; changing it takes an unlock first, and wrong passwords and PINs make the next try wait.
- **Restore points:** Settings › Import and export keeps full copies of everything (sessions, logins, tunnels, snippets, settings, trusted keys) on this computer: one a day, one before each restore, and any you take; the newest 10 stay. Restoring one can also replace the cloud copy.
- **SSH:** agent (OpenSSH / Pageant on Windows), key files (with a passphrase prompt), password and keyboard-interactive, optional "remember the password" (Settings › Saved logins lists, changes and forgets them), jump hosts, agent forwarding (per session, off by default), host-key trust-on-first-use, and reconnecting by itself when a connection drops.
- **Telnet:** for switches, routers and serial servers, with window-size and terminal-type negotiation. Imported from MobaXterm too; `telnet host` in the command palette connects.
- **WinRM (PowerShell remoting):** the local PowerShell (7 if installed) enters a session on a Windows host, as your Windows login or as a user. The user's password comes from the session's shared login, else a remembered one (Settings › Saved logins), else xtermium asks and can remember it; one the host refuses is forgotten. It reaches PowerShell in an environment variable that PowerShell removes at once, never on the command line.
- **Local shell and serial ports:** ConPTY on Windows, a pty elsewhere. On Windows, Settings › General can add **Open in xtermium** to folders in File Explorer (a folder, its background, a drive): a local shell starts in that folder, in the running xtermium if there is one.
- **SFTP side panel** for SSH tabs, over the same connection, and for VNC tabs, over an SSH connection of its own to the desktop's host (as its shared login's user, through the login's gateway). Browse, download (to your Downloads folder), drag files in to upload, rename, delete, create folders, with progress bars. A panel is named and coloured as its tab is, and follows the shell's folder when the shell reports it (OSC 7: fish does by itself; for bash add `PROMPT_COMMAND='printf "\e]7;file://%s%s\e\\" "$HOSTNAME" "$PWD"'` to `~/.bashrc`, and for zsh the same in `precmd`). With several tabs on screen (split panes, broadcast), three panels show at most: the focused pane's and the first others; with more tabs than that, a chip under the panels for each tab picks whose to show.
- **Snippets:** saved commands typed into the focused terminal, or every ticked tab with broadcast on (Ctrl+Shift+S, or the command palette). `{{name}}` placeholders are asked for when one runs. They're stored and synced with the sessions.
- **Status bar:** the focused tab's connection, how long it's been up, and for SSH the negotiated cipher and a live round trip; the terminal's size. Settings › General hides it.
- **Tabs in their own window:** right-click a terminal or RDP tab › Move to a new window. The session goes on there, with nothing around it. Right-click in it (Ctrl+right-click on a remote desktop) or close the window to put it back. Ctrl+= / Ctrl+- zoom one tab's font.
- **Open in VS Code:** right-click an SSH, local shell or AI agent tab › Open the folder in VS Code (its `code` command has to be on your PATH). A local folder opens as it is; an SSH session's opens on its host through VS Code's Remote-SSH extension, as the session's user and port. VS Code signs in by itself (keys and agent, or it asks for the password); a jump host or key file has to be in `~/.ssh/config` for it. The folder is the shell's when the shell reports it (OSC 7, as for the SFTP panel), else your home folder on the host, or the tab's starting folder.
- **Saved layouts:** the Layouts button next to the layout picker saves the sessions on screen under a name ("Prod web tier"): their arrangement, the pane each is in, the names their tabs were given and the panes' sizes; opening it later puts each session back in its pane, named and sized as it was, in one click (or from the command palette). Every workspace has the same ones.
- **Highlight rules:** Settings › Terminal colours matches of regular expressions in output, like `ERROR`, `WARN` or IP addresses, which helps when tailing logs. Only text the program printed in the normal colour is coloured. Comes with rules for errors, warnings, successes and IPv4/IPv6 addresses.
- **Scroll bar** on terminals (Settings › Terminal), to see and drag where you are in the scrollback. Scrolled back, new output leaves what you're reading where it is, or brings you to the bottom if you turn on "Scroll to the bottom on output".
- **Split panes** (side by side, stacked, 2×2, or tiles: tick any tabs, SSH, RDP, VNC, website or other, and each gets a pane sized to fit) and **broadcast** (Ctrl+Shift+A, for "all"): keystrokes typed in one terminal are replayed into every ticked terminal tab, encoded for each tab's own terminal modes.
- **RDP embedded in a tab** via IronRDP: NLA/NTLM login, server-key trust-on-first-use, keyboard and mouse. A connection lost on the network (a laptop waking up, a router restarting) reconnects on its own. Keys held when the tab loses the keyboard are let go on the desktop, so none stays stuck; Caps, Num and Scroll Lock follow this computer's; pasted and typed text arrives intact, accents and AltGr characters included. The file panel opens the host's shared folders (SMB) with the desktop's login once it's connected (Settings > Defaults, or per session), or asks which account; a session can turn its shared folders off.
- **VNC embedded in a tab** via vnc-rs (MIT/Apache): ZRLE, TRLE, CopyRect and Raw, VNC password (remembered if you like, under Settings › Saved logins), the desktop following the pane where the server allows it. VNC isn't encrypted: every connection says so over the desktop, unless it goes through an SSH gateway or a tunnel's local port.
- **SSH tunnels** (local, remote, dynamic/SOCKS5) with a live traffic-flow diagram: throughput per direction, active and total connections, bytes, and a 60-second sparkline. A tunnel whose connection drops connects again by itself (waiting 1, 2, 4... up to 30 s between tries) while its local port stays open, keepalives notice a quiet mobile link within a minute, and a 16 MB window per channel keeps gigabit links full even far away. Tunnels can start when xtermium opens, and can go through their login's SSH gateway.
- **Automation:** Settings › MCP lets AI assistants (MCP over HTTP on 127.0.0.1, with a token) and scripts (`xtermium help`) list and open sessions, run commands and read terminals, drive remote desktops and website tabs (read the page and its elements, click, fill fields, go to addresses, run a script in the page, and on Windows take screenshots, all through the tab itself with no browser or DevTools of their own), start tunnels, switch workspaces, clone and sync them, move sessions between them, detach tabs and keep restore points. It listens on this computer only, or on the network (0.0.0.0) for assistants running on other machines, over HTTPS with a self-signed certificate (its SHA-256 fingerprint is shown); the commands to add it start those assistants with certificate checks off.
- **Websites and BMC consoles:** Browser sessions open in your default browser, or in a tab (Settings › Browser) with the system's own browser engine: WebView2 on Windows, WebKit on macOS and Linux. Each website session has its own cookies and sign-ins, kept when xtermium restarts; BMC consoles browse privately and keep nothing once closed. Pages are kept from timing out (a mouse move now and then, and a quiet request for the page), and a website can go through its login's SSH gateway. A website session without an icon of its own takes the site's (its favicon) when it's saved or opened; "Website icon" in the session editor fetches it again. Supermicro and iDRAC consoles open the same way.
- **Settings:**
  - light, dark or system theme, and interface scale
  - window effects (Acrylic or Mica on Windows, vibrancy on macOS, plain transparency elsewhere) with window and terminal opacity
  - ten terminal colour schemes, font family and size, cursor shape, scrollback
  - a background for terminal tabs, and others for the sidebar and the tab bar (Settings › Appearance): a PNG, JPEG, GIF, WebP or SVG picture, or a GLSL shader (Shadertoy's `mainImage`, or one with its own `main()`), from a file or a link, under the place's colour at the strength you set; a picture fills, stretches, fits, tiles or sits centred
  - clipboard behaviour, and the Websites mode
  - defaults: the program for Local shell, and the SSH user, key and password used when a session leaves them empty
  - security: recovery passphrase (changing it takes the current one), the lock, and the trusted server keys (remove one after a server's key changed on purpose)
  - SSH keys: the key pairs in `~/.ssh` with their public keys to copy, and new ones (Ed25519 by default, ECDSA P-256 or RSA 4096, with a passphrase if you like)
  - this computer (Settings › General): start xtermium when you sign in, desktop notifications (a new version, a connection that dropped; off by default), and running above the normal process priority (may need administrator rights)
  - Settings › About shows the version (semantic versioning; each release on GitHub lists what changed since the one before), build number and kind of build, checks GitHub for a newer release (a quiet check at start, which "Tell me when a new version is out" turns off) and opens the download for this computer; installing it is up to you, xtermium never updates itself
  - Settings › Troubleshoot makes a bundle: a folder of plain-text files about the installation (version, system, settings without secrets, counts of what's stored and open) to send to support@xtermium.com

## Requirements

- Rust 1.89 or newer, and Node.js 20 or newer with npm.
- On Linux, the WebKitGTK development packages that Tauri needs (see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)). Windows 10/11 ship WebView2.
- Windows: long git paths. The WezTerm git dependency contains paths longer than 260 characters. If `cargo fetch` fails with `path too long`, run `git config --global core.longpaths true`, or run that one fetch with a `HOME`/`USERPROFILE` whose `.gitconfig` sets it.

## Build and run

```
npm install
npm run tauri build
target/release/xtermium
```

`npm run tauri build` does three things:
- builds `xtermium-rdp`, the RDP helper, which is its own crate in `rdp/`
- builds the UI into `dist/`
- compiles the app with the UI embedded, and makes the installers (`target/release/bundle/msi` and `nsis` on Windows)

The helper ships next to `xtermium` in every build and installer, which is where the app looks for it. Add `-- --no-bundle` to skip the installers.

While working on it, use `npm run tauri dev`. That starts Vite and a debug build that reloads the UI as you edit `ui/`, and rebuilds the Rust side when `src/` changes. For a standalone debug exe, use `npm run tauri build -- --debug --no-bundle`.

A plain `cargo build`, `cargo test` or `cargo clippy` needs the helper to have been built once (`npm run rdp`), because Tauri checks the files it bundles. A plain `cargo build` also gives the dev-mode binary, which expects the Vite server on port 1420.

Dependencies are compiled with optimizations even in dev builds, because graphics-heavy sessions crawl without it. The first build therefore takes a few minutes.

## Using it

- Double-click a session in the sidebar to open it. Right-click it to edit, duplicate or delete; right-click a folder to rename, tag, export or delete it.
- Click a tab to show it in the focused pane. Middle-click a tab, or press its ×, to close it.
- Point at a button to see what it does and its shortcut; menus show their items' shortcuts too (Settings › Shortcuts changes them).

| In a terminal | |
|---|---|
| Select text | copies it (can be turned off in Settings) |
| Right-click, Shift+Insert, Ctrl+Shift+V | paste |
| Ctrl+Shift+C | copy the selection |
| Shift+PgUp / Shift+PgDn, mouse wheel | scroll back |
| Shift + drag | select even when the program captures the mouse |
| Ctrl+click a link | opens it in your browser (web addresses in the text, and links programs print with OSC 8) |
| Ctrl+Shift+F | find in the scrollback: Enter for older matches, Shift+Enter for newer, Esc closes |
| R (after a session ends) | reconnect, in the same terminal: the earlier output stays above |
| Return, Esc, Backspace or Ctrl+C (after a session ends or fails to connect) | close the tab (also on a remote desktop that couldn't connect) |
| Ctrl+Shift+←/→/↑/↓ | with several panes, swap the focused pane with the one that way |
| Shift+Alt+←/→/↑/↓ | with several panes, move the focused pane's edge that way (the step is in Settings › Terminal) |

| In an RDP tab | |
|---|---|
| Typing, Ctrl/Alt chords, mouse | forwarded to the remote desktop |
| Shift+Insert, Ctrl+Shift+V | types your local clipboard into the session (the clipboard itself isn't shared) |

### Websites in a tab

**Settings › Browser › Embedded** shows websites in a tab with the system's browser engine (WebView2 on Windows, WebKit elsewhere), as a native view that follows its pane. Each website session keeps its own cookies and sign-ins in a profile of its own (in the data folder; a deleted session's goes at the next start); BMC consoles and websites that aren't saved browse privately. Pages can't reach xtermium itself. While a dialog or menu is over the tab, the page steps aside so it doesn't cover it.

## Configuration

Everything xtermium keeps lives in one folder, `~/.xtermium` (`%USERPROFILE%\.xtermium` on Windows): settings and sessions, terminal logs, website profiles, the windows' browser data and what a run needs only while it runs (`temp`, cleared at the next start). Set `XTERMIUM_HOME` to use another folder (portable mode). The first start of this version moves the data from where older ones kept it (`%APPDATA%\xtermium\config`, `~/.config/xtermium` or `~/Library/Application Support/xtermium`). Only the data key stays outside, in the OS keychain (below); macOS keeps the windows' browser data in its own place.

| File | Contents |
|---|---|
| `sessions.xtm` | sessions, folders and their tags, logins, tunnels, snippets, saved layouts, workspaces |
| `settings.xtm` | everything in the Settings window, including the default SSH password |
| `known.xtm` | trusted SSH host keys and RDP server keys |
| `lock.xtm` | the lock: its password and PIN as Argon2id hashes, the passkey's public key, when it locks |
| `recovery.key` | the data key, sealed under your recovery passphrase |

All `.xtm` files are encrypted with XChaCha20-Poly1305 under a random 256-bit data key. The key lives in the OS keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux), so there's nothing to type day to day.

- **Recovery passphrase:** set one under Settings > Security. On a new computer, after a reinstall, or if the keychain entry is lost, xtermium asks for it and unlocks everything. Without it, a lost keychain entry means the files can't be read.
- **Post-quantum safety:** only symmetric cryptography is used (XChaCha20-Poly1305, Argon2id for passphrases). Quantum computers threaten public-key schemes, and 256-bit symmetric keys stay secure.
- **Upgrading:** the first start of this version encrypts the old plain-text `sessions.toml`, `settings.toml`, `known_hosts` and `known_rdp`, checks the result and deletes the plain-text files.
- **Remembered passwords:** "Remember" stores a session's password in `logins.xtm`, on this computer only (it isn't synced).
- **custom.css:** a `custom.css` in the data folder restyles the interface (the tokens and classes are in `ui/app.css` and `DESIGN.md`). It's applied at start; one that changed since you last kept it is tried for 30 seconds and taken off unless you keep it, so a stylesheet that hides the window can't lock you out.

The formats, and the API a sync server would need, are in [docs/sync.md](docs/sync.md).

## Security notes

- **Trust on first use:** SSH host keys and RDP server keys are confirmed the first time you connect. A changed key refuses the connection instead of asking.
- **Imported tunnels** keep the bind address from the imported file, which is often `0.0.0.0`, meaning reachable from your network. New tunnels default to `127.0.0.1`.
- **`cargo audit`:** two advisories are accepted, with the reasons in `.cargo/audit.toml` and `rdp/.cargo/audit.toml`:
  - `rsa` "Marvin" timing attack: no fix exists, and exposure is low (one signature per login).
  - `lru` unsoundness: not reachable here, because the only cache's keys can't panic on drop.
- Unmaintained or unsound deep dependencies (`adler`, `paste`, `proc-macro-error`, and `glib`, which Tauri uses on Linux) show as warnings only.
- The UI runs under a strict content security policy (`tauri.conf.json`), and its only permissions beyond xtermium's own commands are listed in `capabilities/default.json`.

## Architecture

The Rust backend owns all state and connections; the UI calls it through Tauri commands. Each terminal tab streams to the UI over its own channel: a frame holds only the rows that changed since the last one (as runs of identically styled cells), the cursor, the title and image placements. Images are sent once, by hash, and decoded by the webview. The UI keeps the screen model and paints it on a canvas. Keys and mouse go back as events, and the backend encodes them for the terminal's current modes. RDP tabs stream the helper's dirty rectangles the same way.

| Path | What |
|---|---|
| `src/main.rs` | app state and the Tauri commands, tabs, broadcast, window effects |
| `src/term.rs` | terminal tab: wezterm-term engine, frame diffing, keyboard and mouse, highlight rules, PTY, serial and telnet backends |
| `src/ssh.rs` | SSH connect and auth, host keys, jump hosts, shell, SFTP channel, tunnels and SOCKS5 |
| `src/sftp.rs` | SFTP browser state and transfers, published to the UI as `sftp` events |
| `src/rdp.rs` | RDP tab: runs `xtermium-rdp`, relays its frames and the UI's input |
| `src/rdp_proto.rs` | stdin/stdout wire format between the app and `xtermium-rdp` |
| `src/flow.rs` | running tunnels and their traffic samples |
| `src/settings.rs` | settings and colour schemes |
| `src/config.rs` | sessions, folders and tunnels; trusted server keys; the data key, encrypted reads and writes, migration |
| `src/vault.rs` | encryption: sealed files, recovery key, passphrase-protected exports |
| `src/import.rs` | bookmarks `.ini` importer |
| `src/prompt.rs` | password and confirmation prompts from background tasks (`prompt` events) |
| `ui/App.svelte` | window layout: toolbar, sidebar, tabs and split panes, dashboard, prompts, import |
| `ui/lib/Terminal.svelte` | canvas renderer and input for terminal tabs |
| `ui/lib/Rdp.svelte` | RDP view and input |
| `ui/lib/` (others) | sidebar, session editor, SFTP panel, tunnels and flow diagram, settings, about, Boxicons |
| `rdp/` | `xtermium-rdp`, the RDP helper (IronRDP), with its own `Cargo.lock` because IronRDP and russh pin incompatible pre-release crypto crates |

## Development

```
cargo fmt && cargo clippy --all-targets && cargo test
(cd rdp && cargo fmt && cargo clippy --all-targets)
npm run check
cargo audit && (cd rdp && cargo audit) && npm audit
```

To contribute, read [CONTRIBUTING.md](CONTRIBUTING.md): changes reach `main` only as pull requests from `feat/` or `fix/` branches.

The tests include an end-to-end SSH run against an in-process server: host-key prompt, password auth, shell, SFTP and a local tunnel. They also cover the importer, the RDP wire protocol and key scancodes, the SOCKS5 handshake, broadcast key encoding, and highlight rules. `npm run check` type-checks the UI.

## License and credits

xtermium is licensed under the Apache License 2.0 (`LICENSE`). Copyright © 2026 Sotirios Roussis. You may use, change, rename and share it, commercially or not. Every copy and fork must keep `LICENSE` and the credit in `NOTICE`, and a fork that shows credits must show that one among them.

The cloud sync server is not part of this repository. It is proprietary, and so is the hosted sync service. The client in this repository talks to it through the open API in [docs/sync.md](docs/sync.md). "xtermium" and its logo are trademarks of the author, and forks must use another name.

Third-party parts keep their own licenses, quoted in full in `THIRD-PARTY-NOTICES.txt`; the installers carry it next to `LICENSE` and `NOTICE`, and Settings › About opens it. `npm run notices` rewrites it (it needs `cargo install --locked cargo-about --features cli`), and every release build does. Icons: [Boxicons](https://boxicons.com) (MIT, `assets/icons/LICENSE-boxicons`). Type: [Recursive](https://www.recursive.design) (SIL Open Font License, bundled from `@fontsource-variable/recursive`). Built on Tauri, Svelte, wezterm-term, russh, russh-sftp, IronRDP, portable-pty and serialport.
