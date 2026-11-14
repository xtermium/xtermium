<p align="center"><img src="assets/readme/hero.svg" alt="xtermium. Every connection you have, in one window." width="100%"></p>

<p align="center"><b>xtermium is a terminal with a session manager.</b> Your SSH, RDP, VNC, telnet, serial, WinRM, website and BMC sessions live in one sidebar and open side by side, in one window.</p>

<p align="center">
  <a href="https://github.com/xtermium/xtermium/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/xtermium/xtermium?style=flat-square&labelColor=141414&color=FFC20E&label=release"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/github/license/xtermium/xtermium?style=flat-square&labelColor=141414&color=F2F2F2"></a>
  <img alt="Platforms: Windows, Linux and macOS" src="https://img.shields.io/badge/platforms-Windows%20%C2%B7%20Linux%20%C2%B7%20macOS-F2F2F2?style=flat-square&labelColor=141414">
  <a href="https://github.com/xtermium/xtermium/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/xtermium/xtermium/total?style=flat-square&labelColor=141414&color=F2F2F2"></a>
  <a href="https://xtermium.com"><img alt="Website: xtermium.com" src="https://img.shields.io/badge/website-xtermium.com-FFC20E?style=flat-square&labelColor=141414"></a>
</p>

<p align="center"><a href="https://xtermium.com">Homepage</a> · <a href="https://github.com/xtermium/xtermium/wiki">Docs</a> · <a href="https://xtermium.com/screens">Screens</a> · <a href="https://github.com/xtermium/xtermium/releases/latest">Download</a> · <a href="https://github.com/xtermium/xtermium">Source</a></p>

- **One window, not ten programs.** SSH here, RDP there, a VNC viewer, telnet for the switch, a browser for the console page: xtermium opens them all as tabs and split panes, and types into many at once.
- **Your logins stay locked.** Everything on disk is encrypted with a key in your OS keychain. A recovery passphrase brings it back on a new computer.
- **An agent can use it too.** AI assistants and scripts open sessions, run commands and read the output through the same tools you use, without ever seeing a password.

![xtermium with four panes in one window: htop on web-01 over SSH, a Grafana dashboard in a web tab, core-switch over telnet, and desktop-01 over VNC with a runbook open; five tabs above them](https://xtermium.com/assets/shots/main.webp)

## Get it

**Windows** (x64):

```powershell
irm https://xtermium.com/install.ps1 | iex
```

Or with [Scoop](https://scoop.sh):

```powershell
scoop bucket add xtermium https://github.com/xtermium/scoop
scoop install xtermium/xtermium
```

**Linux** (x64) and **macOS** (Apple silicon and Intel):

```sh
curl -fsSL https://xtermium.com/install.sh | sh
```

Both scripts check each download against its sha256 before installing anything. The [latest release](https://github.com/xtermium/xtermium/releases/latest) has every file to download by hand (`.exe`, `.msi`, portable `.zip`, `.deb`, `.rpm`, `.AppImage`, `.dmg`). The macOS app isn't signed yet: [Installation › macOS](https://github.com/xtermium/xtermium/wiki/Installation#macos) says how to open it. [Nightly builds](https://github.com/xtermium/xtermium/wiki/Installation#nightly-builds) carry what the next release will.

## What you get

Every feature is free except two that need **Pro**, a hosted service: sync and Terminal Co-op.

- **[Connections](https://github.com/xtermium/xtermium/wiki/Usage#connections):** SSH with keys, your agent and jump hosts, telnet, serial ports, the local shell and WinRM, each coming back by itself where it can.
- **[Remote desktops](https://github.com/xtermium/xtermium/wiki/Usage#remote-desktops):** RDP and VNC in a tab, with the host's files beside them (shared folders over RDP, SFTP over VNC). RDP shares the clipboard both ways (text, pictures and files) and, while it has the keyboard, takes the Windows shortcuts (Win+E, Alt+Tab) for the desktop.
- **[Websites and BMC consoles](https://github.com/xtermium/xtermium/wiki/Usage#websites-and-bmc-consoles)** in your browser or in a tab, each website with its own cookies and sign-ins.
- **[Sessions and workspaces](https://github.com/xtermium/xtermium/wiki/Usage#sessions):** a folder tree, shared logins with an SSH gateway, workspaces, snippets, and import from MobaXterm or an OpenSSH config.
- **[Tabs and panes](https://github.com/xtermium/xtermium/wiki/Usage#tabs-and-panes):** split panes for any kind of tab, broadcast, saved layouts, a tab in its own window.
- **[In a terminal](https://github.com/xtermium/xtermium/wiki/Usage#in-a-terminal):** find, pictures, a status bar, highlight rules, logging, and Open in VS Code.
- **[Files](https://github.com/xtermium/xtermium/wiki/Usage#files)** over SFTP and SMB, and **[tunnels](https://github.com/xtermium/xtermium/wiki/Usage#tunnels)** (local, remote, SOCKS5) with their traffic live.
- **[Keeping it safe](https://github.com/xtermium/xtermium/wiki/Security):** encrypted on disk, a lock, host keys you trust once, restore points, your SSH keys.
- **[Agents and scripts](https://github.com/xtermium/xtermium/wiki/AI-agents):** Claude Code, Codex and omp in tabs with their session history and a prompt per workspace, an MCP server and the `xtermium` command line.
- **[Looks](https://github.com/xtermium/xtermium/wiki/Customization):** themes, colour schemes, fonts, backgrounds, your own shortcuts, 13 languages and `custom.css`.
- **[Sync](https://github.com/xtermium/xtermium/wiki/Usage#sync)** (Pro, not open yet): end-to-end encrypted, on every computer you sign in on.
- **[Terminal Co-op](https://github.com/xtermium/xtermium/wiki/Usage#terminal-co-op)** (Pro): share tabs with up to two other people, straight between your computers.

![desktop-01 over VNC in one pane, htop on web-01 beside it](https://xtermium.com/assets/shots/split.webp)

![Claude Code in an xtermium tab answering that a gzip process is using the CPU on web-01 and db-01 is idle, with a table of what it found; beside it, web-01's terminal shows the output of the ps command it ran](https://xtermium.com/assets/shots/agent.webp)

## Documentation

The [wiki](https://github.com/xtermium/xtermium/wiki) has the detail:

- [Installation](https://github.com/xtermium/xtermium/wiki/Installation): every system, the first start, updates, nightly builds
- [Usage](https://github.com/xtermium/xtermium/wiki/Usage): sessions, tabs and panes, the terminal, remote desktops, websites, files, tunnels, sync, Terminal Co-op
- [Security](https://github.com/xtermium/xtermium/wiki/Security): what's stored where and how, the lock, restore points
- [AI agents](https://github.com/xtermium/xtermium/wiki/AI-agents): agent sessions, the MCP server, the command line
- [Customization](https://github.com/xtermium/xtermium/wiki/Customization): looks, shortcuts, language, behaviour
- [Development](https://github.com/xtermium/xtermium/wiki/Development): building, the code's layout, the checks

Something wrong? Settings › Troubleshoot makes a bundle to send to [support@xtermium.com](mailto:support@xtermium.com), or [open an issue](https://github.com/xtermium/xtermium/issues).

## Build from source

You need Rust and Node.js; then `npm install` and `npm run tauri build`. [Development](https://github.com/xtermium/xtermium/wiki/Development) has the requirements, the layout of the code and the checks, and [CONTRIBUTING.md](CONTRIBUTING.md) how changes get in.

## License and credits

xtermium is licensed under the Apache License 2.0 (`LICENSE`). Copyright © 2026 Sotirios Roussis. You may use, change, rename and share it, commercially or not. Every copy and fork must keep `LICENSE` and the credit in `NOTICE`, and a fork that shows credits must show that one among them.

The cloud sync server is not part of this repository. It is proprietary, and so is the hosted sync service. The client in this repository talks to it through the open API in [docs/sync.md](docs/sync.md). "xtermium" and its logo are trademarks of the author, and forks must use another name.

Third-party parts keep their own licenses, quoted in full in `THIRD-PARTY-NOTICES.txt`; the installers carry it next to `LICENSE` and `NOTICE`, and Settings › About opens it. `npm run notices` rewrites it (it needs `cargo install --locked cargo-about --features cli`), and every release build does. Icons: [Boxicons](https://boxicons.com) (MIT, `assets/icons/LICENSE-boxicons`). Type: [JetBrains Mono](https://www.jetbrains.com/lp/mono/) (SIL Open Font License, bundled from `@fontsource-variable/jetbrains-mono`; the README's hero embeds it in `assets/readme/hero.svg`). Built on Tauri, Svelte, wezterm-term, russh, russh-sftp, IronRDP, portable-pty and serialport. Logo, logo animations, colours and the design system by [dehawk](https://github.com/dehawk202).

<p align="center"><b>Star it if it saves you a window.</b></p>
