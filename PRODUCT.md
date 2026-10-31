# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

A desktop app (Tauri 2) whose whole interface is a web UI in the system webview: WebView2 on
Windows, WKWebView on macOS, WebKitGTK on Linux. It has one design language of its own on all
three, not each OS's native one.

## Users

Sysadmins and infrastructure engineers who keep xtermium open all day, juggling dozens to
hundreds of servers over SSH, RDP, VNC, telnet, serial and BMC consoles, often on production
systems, and working from the keyboard.

## Product Purpose

A terminal with a session manager: every kind of remote connection a
sysadmin uses, saved, organised and opened from one window, so the day's work happens in one
place instead of a pile of separate tools. Success is that a sysadmin reaches any machine, in the
right way, in seconds, and trusts what they type to land where they meant.

## Positioning

- **Every connection, one window:** SSH, SFTP, RDP, VNC, telnet, serial, WinRM, BMC consoles,
  websites and SSH tunnels side by side in tabs and split panes, with broadcast typing into many
  at once.
- **Encrypted and synced:** everything on disk is encrypted with a data key kept in the OS
  keychain (a recovery passphrase can restore it); workspaces sync end-to-end encrypted.
- **Automatable:** an MCP server and the `xtermium` command line drive the same tools, so AI
  agents and scripts can open sessions, type commands and read output.
- **Fast and native-feeling:** a Rust backend and a GPU-drawn terminal; the terminal's speed and
  rendering quality come first.

## Operating Context

Long sessions on production systems, many tabs and panes at once, connections that drop and come
back, host keys to trust, shared logins, jump hosts and tunnels. Sessions live in folders and
workspaces; layouts, snippets and broadcast speed up repeated work. The same person may drive it
by hand and through an AI agent or script.

## Capabilities and Constraints

- Cross-platform with equal priority: Windows, macOS and Linux.
- Keyboard-first: every action is reachable from the keyboard and has a rebindable shortcut; the
  command palette reaches everything; dialogs hold the keyboard so nothing typed into them ever
  reaches a server.
- 12 interface languages besides English (ar, de, el, es, fi, fr, hi, ja, nb, pl, sv, zh-CN),
  Arabic right to left; all text goes through i18n, and new text may stay in English until
  translated.
- Automation never reveals passwords; AI-agent sessions are invisible to automation.
- Licensing: the app is Apache-2.0, crediting its author in `NOTICE`; the sync server is
  proprietary and stays out of the repository.

## Brand Commitments

- The name is xtermium, written in lower case.
- Copy is short plain sentences in the second person, with no jargon in labels.

## Evidence on Hand

The README's feature list and the app itself. There are no customers, testimonials, case studies
or benchmarks on hand: future work must not invent any.

## Product Principles

1. Never lose a sysadmin's work or connection by surprise: undo or ask before anything that can't
   be undone, and make the safe answer the default.
2. The keyboard is the primary interface; the mouse is a convenience.
3. Be explicit about where input goes: which tab, which pane, which servers.
4. The terminal's speed and fidelity outrank decoration.
5. Everything a person can do, an agent or script can do too, without ever seeing a secret.

## Accessibility & Inclusion

Full keyboard operation; right-to-left layout for Arabic; reduced motion respected; text
contrast at WCAG AA in both the dark and light themes.
