// Types mirroring the Rust side (src/config.rs, src/settings.rs) and thin command wrappers.

import { invoke } from "@tauri-apps/api/core";
import { N_ } from "./i18n.svelte";

/** A program or path stored with a session or login (src/config.rs `Portable`): the same on every
 *  system, or one for each, since sessions sync between Windows, macOS and Linux computers. */
export type Portable = string | { windows: string; macos: string; linux: string };
export const SYSTEMS: [keyof Exclude<Portable, string>, string][] = [["windows", "Windows"], ["macos", "macOS"], ["linux", "Linux"]];
/** This computer's system as Rust names it (`std::env::consts::OS`); boot sets it (state.svelte.ts),
 *  here rather than read from `app` because state.svelte.ts imports this module. */
export const system = { os: "" };
/** This computer's value of a Portable; systems other than Windows and macOS read Linux's, as in Rust. */
export const here = (p: Portable): string => (typeof p === "string" ? p : (p[system.os as keyof typeof p] ?? p.linux ?? ""));

export type Endpoint = { host: string; port: number; user: string; key: Portable; credential?: string };
export type Rgb = [number, number, number];
export type Style = {
  font: string;
  font_size: number;
  term: string;
  fg: Rgb | null;
  bg: Rgb | null;
  cursor: Rgb | null;
  palette: Rgb[] | null;
};
export type Kind =
  /** `ciphers` empty, `keepalive` (seconds, 0 off) and `compression` null: Settings > Defaults. */
  | ({
      type: "ssh";
      jump: Endpoint | null;
      command: string;
      ciphers: string[];
      keepalive: number | null;
      compression: boolean | null;
      agent_forward: boolean;
    } & Endpoint)
  /** `dir`: the folder it starts in ("" or none: home), as "Open in xtermium" in File Explorer opens it. */
  | { type: "shell"; program: Portable; dir?: Portable }
  | { type: "serial"; port: string; baud: number }
  /** `quality`: 0 quality, 1 balanced, 2 speed (RDP_QUALITY); null = the global setting. */
  /** `smb_login`: the file panel logs in to the shares as the desktop's login; null = the global setting.
   *  `no_shares`: the file panel doesn't open the host's shared folders at all. */
  | { type: "rdp"; host: string; port: number; user: string; quality: number | null; credential?: string; smb_login?: boolean | null; no_shares?: boolean }
  /** `credential`: a shared login whose SSH gateway carries the pages. */
  | { type: "browser"; url: string; credential?: string; /** Its own browser profile (src/web.rs), given when stored. */ profile?: string }
  | { type: "telnet"; host: string; port: number }
  /** PowerShell remoting: `https` 5986, `insecure` skips the certificate check; no `user`: the Windows login;
   *  `credential`: a shared login, whose user and password are used. */
  | { type: "winrm"; host: string; port: number; user: string; https: boolean; insecure: boolean; credential?: string }
  | { type: "vnc"; host: string; port: number; credential?: string }
  /** A BMC's console (`vendor` from KVM_VENDORS), signed in with the shared login. */
  | { type: "kvm"; vendor: string; host: string; port: number; credential?: string }
  /** An AI agent's command line in folder `dir` ("" = home): `tool` from AGENTS, run with the flags for `mode`
   *  (AGENT_MODES), `effort` (AGENT_EFFORTS) and `model` ("" = the tool's default) and then `args`; "custom": `args` is the command.
   *  `mcp_off` and `skills_off` name what stays off in the session; `env` is its variables, one NAME=value a line;
   *  `resume` (a tab's session only) a past conversation to go on with, by the tool's id for it. */
  | { type: "agent"; tool: string; args: string; dir: Portable; mode: string; effort: string; model: string; mcp_off: string[]; skills_off: string[]; env: string; resume?: string }
  /** A tab someone in your Terminal Co-op ring shares with you (App.svelte `openRemote`): `tab` is its
   *  number at the owner, `picture` a remote desktop rather than a terminal. Never stored or saved. */
  | { type: "coop"; tab: number; picture: boolean; owner: string };
export type KindName = Kind["type"];
/** Shown as a picture (Rdp.svelte), not a terminal. */
export const isDesktop = (k: Kind) => k.type === "rdp" || k.type === "vnc";
/** Terminal sessions: they get the terminal look and settings. */
export const broadcasts = (k: Kind) => ["ssh", "shell", "serial", "telnet", "winrm", "agent"].includes(k.type);
/** Sessions broadcast reaches: terminals, remote desktops and BMC consoles (src/broadcast.rs `joins`). */
export const joinsBroadcast = (k: Kind) => k.type !== "browser" && k.type !== "coop";
/** BMC console vendors (src/config.rs `KVM_VENDORS`) and their names. */
export const KVM_VENDORS: [string, string][] = [
  ["supermicro", "Supermicro"],
  ["idrac", "Dell iDRAC"],
  ["ilo", "HPE iLO"],
  ["megarac", "AMI MegaRAC (ASUS, Gigabyte, ASRock Rack)"],
];
/** Agent tools (src/config.rs `AGENTS`) by the program that runs them, then "custom" for any other agent. */
export const AGENTS: [string, string][] = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
  ["omp", "omp (Oh My Pi)"],
  ["custom", N_("Custom command")],
];
/** How much an agent asks (src/config.rs `AGENT_MODES`); "" leaves the tool's default. */
export const AGENT_MODES: [string, string][] = [
  ["ask", N_("Ask before edits and commands")],
  ["edit", N_("Edit freely, ask before commands")],
  ["auto", N_("Run everything without asking")],
];
/** Reasoning effort (`AGENT_EFFORTS`), for the tools that take it. */
export const AGENT_EFFORTS = ["low", "medium", "high", "xhigh", "max"];
export const EFFORT_TOOLS = ["claude", "codex", "omp"];
/** The agent tools whose sessions can turn MCP servers off (src/agent_mcp.rs `TOOLS`); omp has no way to for one run. */
export const MCP_TOOLS = ["claude", "codex"];
/** The agent tools whose sessions can turn skills off (src/agent_mcp.rs `SKILL_TOOLS`). */
export const SKILL_TOOLS = ["claude", "codex", "omp"];
/** `color`: `#rrggbb` or "" (folder's, else the kind's). `icon`: a built-in icon name, a `data:` image, or "". */
export type Session = { name: string; folder: string; kind: Kind; style: Style; color: string; icon: string; /** A built-in icon doesn't move on hover. */ still?: boolean; notes: string };
export type TunnelKind = "Local" | "Remote" | "Dynamic";
export type Tunnel = {
  name: string;
  kind: TunnelKind;
  ssh: Endpoint;
  bind: string;
  listen_port: number;
  dest_host: string;
  dest_port: number;
  /** Started when xtermium opens. */
  autostart?: boolean;
};
/** A folder with something to remember; folders also exist through their sessions' paths. */
export type Folder = { path: string; tags: string[]; color: string; icon: string; still?: boolean; /** Place among its siblings from 1; 0 = by name, after. */ order?: number };
/** `agent_prompt`: this workspace's system prompt for AI agent sessions (Settings › General), added to each tool's own. */
export type Store = { session: Session[]; tunnel: Tunnel[]; folder: Folder[]; snippet: Snippet[]; credential: Credential[]; layout: SavedLayout[]; agent_prompt: string };
/** Sessions opened together in a pane arrangement (App.svelte `Layout`): `pane` in tab-strip
 *  order, `at` the pane each was in, `label` the name its tab was given ("" none), `sizes` the
 *  panes' sizes as dragged (none: even). Older layouts have only `pane`, one per pane in order. */
export type SavedLayout = { name: string; arrangement: string; pane: Session[]; at?: number[]; label?: string[]; sizes?: { rows: number[]; cols: number[][] } };
/** A login sessions can share (`credential` on an endpoint or RDP/VNC/website session is its id).
 *  `gateway`: an SSH server those sessions connect through. */
export type Credential = { id: string; name: string; user: string; password: string; key: Portable; gateway?: Gateway | null };
export type Gateway = { host: string; port: number; user: string; password: string; key: Portable };
/** A saved command. `text` may hold `{{name}}` placeholders; `run` presses Enter after it. */
export type Snippet = { name: string; text: string; run: boolean };

export type Effect = "None" | "Transparent";
export type CursorShape = "Block" | "Bar" | "Underline";
export type Settings = {
  theme: "System" | "Dark" | "Light";
  ui_scale: number;
  effect: Effect;
  opacity: number;
  scheme: string;
  font: string;
  font_size: number;
  override_sessions: boolean;
  cursor: CursorShape;
  cursor_blink: boolean;
  line_height: number;
  /** Room between a terminal's text and its pane's edges, in pixels. */
  pane_padding: number;
  /** A scroll bar on terminals. */
  scrollbar: boolean;
  /** Highlight rules: text in the default colour matching a pattern (Rust regex) gets its colour. */
  highlight: boolean;
  highlights: { pattern: string; color: string }[];
  /** Behind terminal text, the sidebar and the tab bar; the pictures or shaders come from `background_get`. */
  background: Background;
  sidebar_background: Background;
  bar_background: Background;
  scrollback: number;
  /** Output brings a terminal scrolled back to the bottom; off, it stays where it is. */
  scroll_on_output: boolean;
  copy_on_select: boolean;
  right_click_paste: boolean;
  /** Trim whitespace off the start and end of terminal pastes. */
  strip_paste: boolean;
  /** Save each terminal tab's output to a text file in the logs folder. */
  log_to_file: boolean;
  web: "External" | "Embedded";
  shell: string;
  ssh_user: string;
  ssh_key: string;
  ssh_password: string;
  /** RDP picture quality against speed, for sessions without their own (RDP_QUALITY). */
  /** SSH ciphers to offer, best first; keepalive seconds (0 off); zlib compression. */
  ssh_ciphers: string[];
  ssh_keepalive: number;
  ssh_compression: boolean;
  rdp_quality: number;
  /** What crosses RDP's shared clipboard: 1 this computer's to the desktop, 2 the desktop's here, 3 both, 0 nothing. */
  rdp_clipboard: number;
  /** A remote desktop with the keyboard takes the Windows keys and system shortcuts from this computer. */
  rdp_keyboard: boolean;
  /** RDP file panels log in to the host's shares as the desktop's login (sessions can override). */
  smb_auto_login: boolean;
  /** Local automation: MCP server and the `xtermium` command line (Settings > MCP). */
  automation: boolean;
  automation_port: number;
  /** Listen on every network interface (0.0.0.0) rather than only on this computer. */
  automation_network: boolean;
  automation_token: string;
  /** Cloud sync (docs/sync.md): on while signed in; the server ("" = xtermium's), the account's token and email. */
  sync: boolean;
  sync_token: string;
  sync_email: string;
  /** The account had Pro when the server last answered (`loadAccount`); kept so it holds offline. */
  sync_pro: boolean;
  /** What else syncs, each only when ticked (Settings › Account and sync): the settings, the three background files, the saved logins. */
  sync_settings: boolean;
  sync_backgrounds: boolean;
  sync_logins: boolean;
  /** The account button in the sidebar; always shown while signed in. */
  account_button: boolean;
  /** The Terminal Co-op button in the sidebar; always shown with Pro or while a ring is on. */
  coop_button: boolean;
  /** The pictures of websites and BMC consoles shared in a ring: how often and how finely (src/settings.rs `Quality`). */
  coop_quality: "low" | "average" | "high";
  /** Minutes away from the mouse and keyboard after which a ring you started ends; 0: never. */
  coop_idle: number;
  confirm_close: boolean;
  auto_reconnect: boolean;
  restore_tabs: boolean;
  status_bar: boolean;
  single_click_open: boolean;
  /** With no tabs open, typing on the start screen opens a local shell with what was typed. */
  quick_shell: boolean;
  /** The program the start screen's prompt runs commands in; "" = `shell`. */
  start_shell: string;
  /** The sidebar's open folders come back next time (`open_folders`). */
  remember_folders: boolean;
  open_folders: string[];
  /** Tips turned off with "Don't remind me again" (Hint.svelte), by id. */
  quiet_hints: string[];
  /** A ui/locales file name; "" follows the system. */
  language: string;
  sidebar_width: number;
  /** The sidebar is a narrow column of buttons (App.svelte). */
  sidebar_collapsed: boolean;
  /** Shortcuts changed from the defaults (actions.ts): action → chord, "" = none. */
  shortcuts: Record<string, string>;
  /** Pixels the pane-edge shortcuts move an edge by. */
  pane_step: number;
  /** Closing the window hides it to the tray; the sessions run on (src/tray.rs). */
  keep_running: boolean;
  /** Start xtermium at sign-in. */
  start_at_login: boolean;
  /** Desktop notifications: new versions, dropped connections. */
  notifications: boolean;
  /** Say when a newer release is out (About). */
  update_notify: boolean;
  /** Run above the normal process priority (may need administrator rights; takes a restart to undo). */
  high_priority: boolean;
  /** Hex SHA-256 of the custom.css kept; a different one asks again at start (App.svelte). */
  custom_css: string;
  /** The outline on terminals broadcast types into (app.css `.outline`) and its colour. */
  broadcast_outline: string;
  /** `#rrggbb`; "" = the accent. */
  broadcast_outline_color: string;
  /** The focused pane's ring glides to the next pane; false: it jumps. */
  focus_animated: boolean;
  /** The interface's accent colour, `#rrggbb`; "" = the built-in amber. */
  accent: string;
  /** The getting-started list on the start screen (Guide.svelte); `guide_done`: the steps done,
   *  and `pill-seen` once the tab bar's progress pill has pulsed. */
  guide: boolean;
  guide_done: string[];
  /** The app's own icon, a 256px square `data:image/png` URL (images.ts `appIcon`); "" = the mark. */
  app_icon: string;
  /** An http(s) address the start screen's logo opens; "" = none. */
  logo_link: string;
  /** Imported terminal colour schemes (Settings > Appearance). */
  custom_schemes: Scheme[];
  /** Images used as icons, kept to pick again (Appearance.svelte); `data` is a `data:image/…` URL. */
  custom_icons: { name: string; data: string }[];
  /** The workspace in use ("" the default); only the `workspace` command changes it. */
  workspace: string;
};
/** A password saved with "Remember the password" (Settings > Saved logins): `id` is
 *  `user@host:port` (SSH), `rdp:user@host:port` or `vnc:host:port`. */
export type Login = { id: string; password: string };
/** Workspaces besides the default one (""), the one in use, and its data. */
export type Workspaces = { workspaces: string[]; workspace: string; store: Store };
/** What's behind a place (src/settings.rs `Background`): a picture or a shader, how much shows. */
export type Background = { kind: "" | "image" | "shader"; name: string; show: number; fit: Fit; stamp: number };
/** How a background picture fits its place (src/settings.rs `FITS`). */
export type Fit = "fill" | "stretch" | "fit" | "tile" | "center";
export const FITS: [Fit, string][] = [["fill", N_("Fill")], ["stretch", N_("Stretch")], ["fit", N_("Fit")], ["tile", N_("Tile")], ["center", N_("Centre")]];
/** Where a background goes, and its setting. */
export const PLACES = { terminal: "background", sidebar: "sidebar_background", bar: "bar_background" } as const;
export type Place = keyof typeof PLACES;

/** Broadcast outline styles (src/settings.rs `OUTLINES`). */
export const OUTLINES: [string, string][] = [
  ["ring", N_("Ring")],
  ["breathe", N_("Breathing")],
  ["fade", N_("Fade")],
  ["static", N_("Static")],
  ["march", N_("Marching")],
  ["rainbow", N_("Rainbow")],
  ["off", N_("Off")],
];
export type Scheme = { name: string; bg: number; fg: number; cursor: number; selection: number; ansi: number[] };
export type Boot = {
  store: Store;
  notice: string;
  /** Set when stored data can't be read until the recovery passphrase is entered; true if it can be. */
  locked: boolean | null;
  /** The lock (src/lock.rs) is on: the lock screen comes first. */
  lock: boolean;
  recovery: boolean;
  settings: Settings;
  /** Tabs to reopen (Settings > General). */
  tabs: Session[];
  /** The names the reopened tabs were given; "" for none. */
  labels: string[];
  workspaces: string[];
  workspace: string;
  schemes: Scheme[];
  /** SSH ciphers xtermium can use, best first. */
  ciphers: string[];
  version: string;
  os: string;
};

/** Tab state from the backend's frames. */
export type Meta = {
  cursor: [number, number] | null;
  /** Rows scrolled back from the bottom, and rows of scrollback above the screen (terminals). */
  scrollback?: [number, number];
  title: string;
  alive: boolean;
  /** The session finished on its own (logout, exit): the tab closes. */
  ended: boolean;
  /** The connection dropped after it was up: the tab may reconnect by itself. */
  lost: boolean;
  /** The shell's folder as it reports it (OSC 7), "" until it does (terminals). */
  cwd?: string;
  /** Ctrl is over a link. */
  link: boolean;
  scrolled: boolean;
  images: { hash: string; dst: number[]; uv: number[] }[];
  /** While Terminal Co-op shares the tab: the stable row number of the top row on screen, and
   *  whether the program takes the mouse (terminals). */
  shared?: [number, boolean];
};

export type TunnelStats = {
  live: boolean;
  status: string;
  history: [number, number][];
  active: number;
  total: number;
  sent: number;
  received: number;
  target: string;
};

export const HOMEPAGE = "https://xtermium.com";
export const GITHUB = "https://github.com/xtermium/xtermium";

export const DEFAULT_MONO = `ui-monospace, "Cascadia Mono", Consolas, "DejaVu Sans Mono", Menlo, monospace`;

/** CSS font stack for a family name; empty = the default monospace. */
export function fontStack(family: string): string {
  return family ? `"${family.replace(/"/g, "")}", ${DEFAULT_MONO}` : DEFAULT_MONO;
}

/** Makes this window as see-through as the settings say. Two layers make up any place: the window's
 *  ground and the panel or terminal on it. Each lets through the square root of what the opacity
 *  setting does, so together they're exactly it. Anything between them paints only `--solid`. */
export function seeThrough(s: Settings) {
  const root = document.documentElement.style;
  root.setProperty("--chrome-alpha", String(s.effect === "None" ? 1 : 1 - Math.sqrt(1 - s.opacity)));
  root.setProperty("--solid", s.effect === "None" ? "1" : "0");
  // A floating layer lies over what's already there, like a window over another: one layer.
  root.setProperty("--float-alpha", String(s.effect === "None" ? 0.98 : Math.min(0.98, s.opacity)));
}

export const css = (rgb: number, alpha = 1) =>
  alpha >= 1
    ? `#${rgb.toString(16).padStart(6, "0")}`
    : `rgba(${rgb >> 16}, ${(rgb >> 8) & 255}, ${rgb & 255}, ${alpha})`;

export function human(b: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = b <= 0 ? 0 : Math.min(Math.floor(Math.log2(b) / 10), units.length - 1);
  const v = b / 2 ** (10 * i);
  return i === 0 ? `${v.toFixed(0)} B` : `${v.toFixed(1)} ${units[i]}`;
}

export function host(k: Kind): string {
  switch (k.type) {
    case "ssh":
      return `${k.user ? k.user + "@" : ""}${k.host}:${k.port}`;
    case "shell":
      return here(k.program);
    case "serial":
      return `${k.port} @ ${k.baud}`;
    case "rdp":
      return `${k.host}:${k.port}`;
    case "browser":
      return k.url;
    case "telnet":
      return `telnet ${k.host}:${k.port}`;
    case "winrm":
      return `winrm ${k.user ? k.user + "@" : ""}${k.host}:${k.port}`;
    case "vnc":
      return `vnc ${k.host}:${k.port}`;
    case "kvm":
      return `kvm ${k.host}:${k.port}`;
    case "agent":
      return (k.tool === "custom" ? k.args : `${k.tool} ${k.args}`).trim();
    case "coop":
      return `co-op ${k.owner}`;
  }
}

export const KIND_ICON: Record<KindName, string> = {
  ssh: "lock-alt",
  shell: "terminal",
  serial: "chip",
  rdp: "desktop",
  browser: "globe",
  telnet: "server",
  winrm: "network",
  vnc: "laptop",
  kvm: "tv",
  agent: "bot",
  coop: "group",
};

/** Cable colour per connection type (app.css). */
export const KIND_COLOR: Record<KindName, string> = {
  ssh: "var(--k-ssh)",
  shell: "var(--k-shell)",
  serial: "var(--k-serial)",
  rdp: "var(--k-rdp)",
  browser: "var(--k-browser)",
  telnet: "var(--k-telnet)",
  winrm: "var(--k-winrm)",
  vnc: "var(--k-vnc)",
  kvm: "var(--k-kvm)",
  agent: "var(--k-agent)",
  coop: "var(--k-coop)",
};

export const newEndpoint = (): Endpoint => ({ host: "", port: 22, user: "", key: "" });
export const newStyle = (): Style => ({
  font: "",
  font_size: 14,
  term: "xterm-256color",
  fg: null,
  bg: null,
  cursor: null,
  palette: null,
});

export function newKind(t: KindName): Kind {
  switch (t) {
    case "ssh":
      return { type: "ssh", ...newEndpoint(), jump: null, command: "", ciphers: [], keepalive: null, compression: null, agent_forward: false };
    case "shell":
      return { type: "shell", program: "" };
    case "serial":
      return { type: "serial", port: "", baud: 115200 };
    case "rdp":
      return { type: "rdp", host: "", port: 3389, user: "", quality: null };
    case "browser":
      return { type: "browser", url: "https://" };
    case "telnet":
      return { type: "telnet", host: "", port: 23 };
    case "winrm":
      return { type: "winrm", host: "", port: 5985, user: "", https: false, insecure: false };
    case "vnc":
      return { type: "vnc", host: "", port: 5900 };
    case "kvm":
      return { type: "kvm", vendor: "supermicro", host: "", port: 443, credential: "" };
    case "agent":
      return { type: "agent", tool: "claude", args: "", dir: "", mode: "", effort: "", model: "", mcp_off: [], skills_off: [], env: "" };
    case "coop":
      return { type: "coop", tab: 0, picture: false, owner: "" };
  }
}

/** Channel payloads: raw messages arrive as an ArrayBuffer (or a byte array when small). */
export function bytes(m: unknown): Uint8Array {
  if (m instanceof ArrayBuffer) return new Uint8Array(m);
  if (m instanceof Uint8Array) return m;
  return Uint8Array.from(m as number[]);
}

export const openUrl = (url: string) => invoke("open_url", { url });

export const newSession = (kind: KindName = "ssh", folder = "", name = ""): Session => ({
  name,
  folder,
  kind: newKind(kind),
  style: newStyle(),
  color: "",
  icon: "",
  notes: "",
});

/** File formats for import and export (src/formats). */
export type Format = "xtermium" | "mobaxterm" | "openssh";
export const FORMATS: { id: Format; name: string; note: string }[] = [
  { id: "xtermium", name: N_("xtermium (encrypted)"), note: N_("Everything, protected by a passphrase. Import it on any xtermium.") },
  { id: "mobaxterm", name: "MobaXterm (.mxtsessions)", note: N_("SSH, RDP and website sessions, folders, terminal colours and tunnels.") },
  { id: "openssh", name: N_("OpenSSH config"), note: N_("SSH sessions as Host entries, with jump hosts and port forwards. Paste into ~/.ssh/config.") },
];

/** Built-in icons a session or folder can use instead of its kind's (assets/icons). */
export const ICONS = [
  "server", "desktop", "laptop", "terminal", "lock-alt", "chip", "globe", "world", "folder", "target-lock",
  "transfer-alt", "broadcast", "cog", "file", "grid-alt", "palette", "radio-circle-marked", "github", "bot",
  // infrastructure and devops; their moving parts play on hover (app.css `.a-*`)
  "storage", "database", "router", "switch", "access-point", "firewall", "waf", "proxy", "load-balancer", "network", "vpn",
  "directory", "identity", "key", "container", "cluster", "layers", "cloud", "monitor", "pipeline", "git", "backup", "queue",
  "power", "tv",
];

/** Swatches for marking a session, folder or tab (its name and tab line; the icon keeps its kind's
 *  colour), and for the broadcast outline. No amber: amber text means picked, or a link. */
export const COLORS = ["#5c8df0", "#33b8a6", "#b5c14a", "#9c70e6", "#dd6b9c", "#4fb86a", "#ec6a6a", "#e08a3c", "#8a96a3"];
/** A Co-op ring's owner wears amber on every computer, whatever accent each has picked (coop.ts `memberColor`). */
export const OWNER_COLOR = "#ffb000";

/** Accents offered besides the default amber: metals, apart from the kinds' colours, each with
 *  the colour that is stored. On the light theme it is the fill, the wash and the marks, as the
 *  amber is there, and text stays in ink (App.svelte `accent`). */
export const ACCENTS: [name: string, colour: string][] = [
  [N_("Brushed gold"), "#d8a84e"],
  [N_("Platinum"), "#c3ccd8"],
  [N_("Bronze"), "#a99a78"],
  [N_("Rose gold"), "#e2a395"],
];

/** RDP picture quality against speed, by level (the helper's `config` has the details). */
export const RDP_QUALITY = [
  { name: N_("Quality"), note: N_("Lossless picture and the full desktop look. Best on a LAN.") },
  { name: N_("Balanced"), note: N_("Compressed (RemoteFX), slightly softer picture; no wallpaper or animations.") },
  { name: N_("Speed"), note: N_("Compressed, fewer colours and a plain desktop, for slow or distant links.") },
];

/** A session for an address typed in the palette or sent by automation (actions.ts `quickConnect`). */
export function addressSession(a: { type: "ssh" | "rdp" | "telnet" | "vnc"; host: string; port: number; user: string }): Session {
  const label = `${a.user ? a.user + "@" : ""}${a.host}:${a.port}`;
  const s = newSession(a.type, "", label);
  Object.assign(s.kind, { host: a.host, port: a.port }, "user" in s.kind && { user: a.user });
  return s;
}

/** The latest release on GitHub (src/support.rs `update_check`). */
export type Update = { version: string; newer: boolean; page: string; download: string | null };

// ---- Terminal Co-op (src/coop.rs) ----
//
// Commands (Tauri turns Rust's snake_case argument names into camelCase):
//   owner:  coop_create() · coop_refresh() every 2 s while the dialog is open · coop_passcode() ·
//           coop_approve({ user }) · coop_decline({ user }) · coop_kick({ user }) · coop_rename({ user, name }) · coop_end() ·
//           coop_share({ tab, on }) · coop_cursor({ tab, col, row }) · coop_control({ tab, user }) (user "": take it back) ·
//           coop_decline_control({ tab, user })
//   viewer: coop_join({ passcode }) · coop_leave() · coop_cursor({ tab, col, row }) · coop_ask_control({ ringTab, want }) ·
//           coop_open_remote({ ringTab, out }) (a Channel, like tab_attach) · coop_remote_key({ ringTab, key }) ·
//           coop_remote_text({ ringTab, text }) · coop_remote_mouse({ ringTab, ev, cw, ch }) (the term_key / term_text /
//           term_mouse arguments; cw, ch: one of your cells in device pixels, as term_resize takes) ·
//           coop_remote_input({ ringTab, ev }) (rdp_input's events, for pictures) · coop_close_remote({ ringTab, channel }) (the view
//           closed: channel is the closed Channel's id, so the backend lets go of it)
//   both:   coop_state() → CoopEvent, for when the window starts or reloads · coop_traffic() → CoopTraffic[], read
//           every second while the dialog is open.
// coop_create, coop_refresh, coop_passcode, coop_approve, coop_decline, coop_kick, coop_rename and coop_join answer with a
// CoopEvent; the others answer nothing (coop_open_remote: whether the tab is still shared). Any of them refuses
// with a sentence for the person. Every change also arrives as the `coop` event, at most 20 times a second.

/** A person in a ring, the owner first (`owner`). `state` as the server has it: "pending" while they
 *  wait for the owner's yes, "approved" once in, "kicked" when removed or turned down (only the owner
 *  sees those). `you`: this is you. `online`: their app is connected to the owner's now. `image` is
 *  a picture URL or null. */
export type CoopMember = { id: string; name: string; image: string | null; state: string; owner?: boolean; you?: boolean; online?: boolean };
/** The ring you are in. `owner`: you started it; only then `passcode` is set, until `passcode_until`
 *  (unix seconds; 0 for a viewer). `state`: "open" for the owner; for a viewer "pending" (waiting for
 *  the owner's yes), "connecting", then "joined". */
export type CoopRing = { id: string; passcode: string | null; passcode_until: number; owner: boolean; state: "open" | "pending" | "connecting" | "joined"; members: CoopMember[] };
/** A tab shared with you; `id` is its number at the owner (what coop_open_remote and coop_remote_* take as
 *  `ringTab`). "text": a terminal of `cols` × `rows` cells, shown with Terminal.svelte's decoder; "picture":
 *  Rdp.svelte's messages (`w` × `h` pixels; 0 until the picture's own size message arrives), and for a
 *  website the message COOP_MSG_JPEG: `[6]` then a JPEG of the whole page, to draw scaled to fit. */
export type CoopRemote = { id: number; title: string; kind: "text" | "picture"; cols: number; rows: number; w: number; h: number; owner_name: string };
export const COOP_MSG_JPEG = 6;
/** The bytes a person has sent (`up`) and received (`down`) in the ring since their connection opened.
 *  The owner hears of everyone's; a viewer of their own, and of the owner's while nobody else is in. */
export type CoopTraffic = { id: string; up: number; down: number };
/** Where another person's pointer is over a shared tab (`tab` is the owner's number for it): a cell of
 *  a text tab, a pixel (`col` = x, `row` = y) of a picture. Never your own. */
export type CoopCursor = { tab: number; user_id: string; name: string; image: string | null; col: number; row: number };
/** A click in the ring (the `coop-click` event): a cell or a pixel, which button, and when it arrived (`at`, performance.now()). */
export type CoopClick = { tab: number; user_id: string; name: string; col: number; row: number; button: number; at: number };
/** Another person's selection on a shared terminal tab: its ends in stable rows (`Meta.shared`). */
export type CoopSelection = { tab: number; user_id: string; a: [number, number]; b: [number, number]; block: boolean };
/** A guest and a shared desktop (`tab` is the owner's number for it): the one driving it (`control`),
 *  or one asking to (`wants`). A shared desktop with nobody in `control` is the owner's to drive. */
export type CoopControl = { tab: number; user_id: string };
/** The `coop` event: all of it, whenever any of it changes. `shared`: your own tabs (by their id) that
 *  you share. `control`: who drives each shared desktop a guest drives; `wants`: the guests asking for
 *  control (the owner sees everyone's, a guest their own). When the ring is over `ring` is null, and
 *  `ended` says why when it wasn't you: "pro" (the owner's Pro lapsed), "kicked", "ended" (the owner
 *  ended it), "lost" (the connection), "signed-out"; `message` is the sentence to show. */
export type CoopEvent = { ring: CoopRing | null; shared: number[]; remote: CoopRemote[]; cursors: CoopCursor[]; selections?: CoopSelection[]; control?: CoopControl[]; wants?: CoopControl[]; ended?: string; message?: string };
