---
name: xtermium
description: A terminal and session manager for sysadmins, in titanium chrome with one gold accent and an anodized hue per kind of connection.
colors:
  gold: "#d8a84e"
  gold-ink: "#1d1608"
  gold-soft: "rgba(216, 168, 78, 0.16)"
  k-ssh: "#5c8df0"
  k-shell: "#33b8a6"
  k-serial: "#b5c14a"
  k-rdp: "#9c70e6"
  k-browser: "#dd6b9c"
  k-telnet: "#d98a4a"
  k-winrm: "#4aa8d9"
  k-vnc: "#c46fd1"
  k-kvm: "#8fa3b8"
  k-agent: "#ec6a6a"
  ok: "#4fc08d"
  warn: "#e0b04a"
  error: "#ec6a6a"
  error-ink: "#200a0a"
  window: "#101215"
  deep: "#15181c"
  bg: "#1c2025"
  panel: "#22262c"
  raised: "#2b3037"
  hover: "#353b44"
  line: "#353b43"
  fg: "#e3e6ea"
  weak: "#959da8"
typography:
  display:
    fontFamily: "Recursive Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "clamp(13px, 1.9vw, 21px)"
    fontWeight: 700
    lineHeight: 1.12
    fontVariation: "'MONO' 1, 'CASL' 0"
  headline:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, -apple-system, Ubuntu, sans-serif"
    fontSize: "16.5px"
    fontWeight: 400
    lineHeight: 1.618
  title:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, -apple-system, Ubuntu, sans-serif"
    fontSize: "14.5px"
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, -apple-system, Ubuntu, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.4
  prose:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, -apple-system, Ubuntu, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.618
  label:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, -apple-system, Ubuntu, sans-serif"
    fontSize: "11.5px"
    fontWeight: 400
    lineHeight: 1.4
  mono:
    fontFamily: "Recursive Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "0.92em"
    fontWeight: 400
    fontVariation: "'MONO' 1, 'CASL' 0"
rounded:
  control: "3px"
  menu: "5px"
  window: "8px"
  round: "50%"
spacing:
  "1": "1px"
  "2": "2px"
  "3": "3px"
  "5": "5px"
  "8": "8px"
  "13": "13px"
  "21": "21px"
  "34": "34px"
  gap: "5px"
  row: "21px"
  control: "26px"
  bar: "34px"
components:
  button:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.fg}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "3px 8px"
    height: "{spacing.control}"
  button-hover:
    backgroundColor: "{colors.hover}"
  button-primary:
    backgroundColor: "{colors.gold}"
    textColor: "{colors.gold-ink}"
    rounded: "{rounded.control}"
    padding: "3px 8px"
    height: "{spacing.control}"
  button-primary-hover:
    backgroundColor: "color-mix(in srgb, #d8a84e 86%, white)"
  button-danger:
    backgroundColor: "{colors.error}"
    textColor: "{colors.error-ink}"
    rounded: "{rounded.control}"
    padding: "3px 8px"
    height: "{spacing.control}"
  button-on:
    backgroundColor: "{colors.gold-soft}"
    textColor: "{colors.gold}"
  button-flat:
    backgroundColor: "transparent"
    textColor: "{colors.weak}"
  button-flat-hover:
    backgroundColor: "{colors.raised}"
  button-icon-only:
    padding: "3px 5px"
    height: "{spacing.control}"
  input:
    backgroundColor: "{colors.deep}"
    textColor: "{colors.fg}"
    rounded: "{rounded.control}"
    padding: "3px 8px"
    height: "{spacing.control}"
  menu:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.menu}"
    padding: "3px"
    width: "190px"
  menu-item:
    backgroundColor: "transparent"
    rounded: "2px"
    padding: "3px 8px"
    height: "{spacing.control}"
  menu-item-hover:
    backgroundColor: "{colors.raised}"
  tab:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.weak}"
    padding: "0 5px"
    height: "{spacing.bar}"
  tab-active:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.fg}"
  rack-row:
    textColor: "{colors.fg}"
    padding: "0 5px"
    height: "{spacing.row}"
  rack-row-hit:
    backgroundColor: "{colors.gold-soft}"
  panel:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.window}"
  dialog:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.window}"
    padding: "13px"
    width: "610px"
  palette:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.menu}"
    padding: "5px"
    width: "610px"
  status-bar:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.weak}"
    typography: "{typography.label}"
    padding: "0 8px"
    height: "{spacing.row}"
  tag:
    backgroundColor: "{colors.gold-soft}"
    textColor: "{colors.gold}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 5px"
  toast:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.fg}"
    rounded: "{rounded.menu}"
    padding: "5px 13px"
  kbd:
    backgroundColor: "{colors.deep}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 5px"
---

# Design System: xtermium

## Overview

**Creative North Star: "The Anodized Rack"**

xtermium is a rack of live connections. The chrome is cool titanium, a stack of blue-grey
metals that get lighter the nearer they sit to the hand. Each kind of connection gets the colour
titanium turns when anodized, the way cables on a patch panel are colour-coded: SSH is cobalt,
RDP violet, serial chartreuse. One brushed-gold accent marks where you are and what you picked.
Nothing else is coloured, so a glance at the rack tells you which kind of session each row and
tab is, and a glance at the gold tells you where the keyboard is.

The feel is machined and quiet, as dense as a modern code editor: 13px system text, 26px
controls, 21px rows, small precise corners. The sidebar (the session rack), the tab area with its
panes, and the file panel are rounded panels floating a few pixels apart on a darker window. They
are flat at rest and are told apart by tone, not by shadow. Only things that float above the work
cast a shadow: menus, dialogs, the command palette, toasts.

Every proportion comes from the golden ratio (φ ≈ 1.618). Spacing steps along the Fibonacci
scale, padding pairs go 1 : φ from vertical to horizontal, type steps by a quarter of φ, and
dialogs are 377, 610 or 987px wide. Settings is a golden rectangle, 987 × 610. `ui/lib/scale.test.ts`
enforces the spacing and type scales in `npm test`, so they are rules, not guidelines.

**Key Characteristics:**
- Titanium neutrals in six tonal steps, a dark theme by default and a full light theme.
- One accent, Brushed Gold, which the user can change. It is never decoration.
- Ten anodized hues, one per kind of connection, used only to say "this is an SSH/RDP/… thing".
- Fibonacci spacing (1 2 3 5 8 13 21 34px) and five type sizes, both enforced by a test.
- Rounded panels on a darker window. Flat at rest; shadows only on floating layers.
- Icon-only tool buttons with tooltips. Boxicons SVGs, animated only on session rows and tabs.
- Right-to-left ready: logical properties throughout, with flips for chevrons, menus and carets.

## Colors

Cool blue-grey titanium, one warm gold, and a set of anodized hues that act as cable colours.

### Primary
- **Brushed Gold** (`gold`): the accent and the only one. It marks focus (`:focus-visible`
  outline, field borders), toggles that are on and options that are picked (with `gold-soft`
  behind gold text), links that go to another place, drop targets, the focused pane's outline,
  the active Settings page's edge, and fill on the single primary button of a dialog. Users can
  change it: the presets are metals, apart from every kind hue and the error red (Platinum
  `#c3ccd8`, Bronze `#a99a78`, Rose gold `#e2a395`), each with a darker light-theme value
  (`ACCENTS` in `api.ts`); any other colour is darkened toward the text on the light theme, as
  the gold is. The app sets `--gold` as `light-dark(light, dark)` and derives `gold-ink` and
  `gold-soft` from it at runtime, so never hard-code its hex in a component; use `var(--gold)`.
- **Gold Ink** (`gold-ink`): text on a solid gold fill (the primary button).
- **Gold Wash** (`gold-soft`): the translucent tint behind picked rows, search hits, a setting a
  search points at, tags and drop zones.

The broadcast marker in a status bar reads the accent through `--broadcast`, an alias of `--gold`
(as `--fwd` and `--back` alias kind hues for tunnels).

### Secondary
The anodized connection hues, one per kind of session (`--k-*`), keyed like cable colours:
- **Cobalt** (`k-ssh`): SSH. Also forward tunnels (`--fwd`).
- **Teal** (`k-shell`): local shells. Also reverse tunnels (`--back`).
- **Chartreuse** (`k-serial`): serial lines.
- **Violet** (`k-rdp`): RDP desktops.
- **Rose** (`k-browser`): websites.
- **Copper** (`k-telnet`): telnet.
- **Cyan** (`k-winrm`): WinRM.
- **Orchid** (`k-vnc`): VNC.
- **Bare Titanium** (`k-kvm`): BMC consoles, deliberately the least coloured.
- **Ember** (`k-agent`): AI-agent tabs. It shares its value with `error`.

They colour a session's icon (always), the 2px connector on top of its tab (unless the tab
carries a mark), and the tile behind a kind glyph (15% of the hue). Components read them through
a local `--kind`.

**Marks.** A person can mark a session, a folder or a tab with a colour (`COLORS` in `api.ts`:
nine swatches, no gold, plus any colour). A mark never takes a kind hue's place: it tints the
session's or folder's name (`.tinted`, its OKLCH lightness held to at least 0.72 on dark and at
most 0.48 on light, so every swatch reads at 4.8:1 or better while keeping its hue) and the tab's
connector line. A folder has no kind, so its icon wears its mark too. A folder's mark carries
down to every name inside it, under each session's own.

### Tertiary
Status, kept apart from the kind hues:
- **Live Green** (`ok`): connected (the status dot, the dot on a session's icon in the rack).
- **Amber** (`warn`): waiting, slow, or a dead tab's title; the border of the reconnect bar.
- **Signal Red** (`error`): disconnected, failures, the hover of a menu's
  destructive item (14% wash behind red text), and the fill of a danger button.
- **Red Ink** (`error-ink`): text on a solid red fill (a danger button).

### Neutral
From the back of the window to the hand:
- **Gunmetal** (`window`): the window itself, showing in the gaps between panels and split panes.
- **Inset Well** (`deep`): wells set into a panel: text fields, `kbd`, segmented controls, the
  sidebar's status pill, code snippets. In the light theme it is white, the lightest step.
- **Slate** (`bg`): the content area under the tabs, and the active tab that joins it.
- **Titanium Panel** (`panel`): the sidebar, the tab strip, the file panel, the status bar, and
  every floating layer (menus, dialogs, palette, toasts at 98% opacity).
- **Raised** (`raised`): button faces, menu-item hover, the current Settings page, scrollbar
  thumbs.
- **Polished** (`hover`): a button under the pointer.
- **Hairline** (`line`): every 1px border and divider.
- **Cold White** (`fg`): primary text.
- **Brushed Steel** (`weak`): secondary text, form labels, idle tabs and tool buttons, counts.

The light theme (`:root[data-theme="light"]` in `ui/app.css`) redefines every token above with
the same roles. Gold deepens to bronze, and the kind hues darken to hold contrast on light metal.
The light values are also kept in `.impeccable/design.json` (`colorMeta.*.light`).

### Named Rules
**The One Accent Rule.** Gold says "you are here" or "you picked this", and nothing else. Two
exceptions: the fill of a dialog's single primary button, and a folder's tag (`.tag`, `prod`,
`lab`…), which is a fact about where you are and shows on the folder's row and under a pane. No
gold headings, icons or decorative strokes.

**The Cable Colour Rule.** A kind hue identifies a kind of connection and nothing else, and
nothing else takes its place: a session's icon is always in its kind's hue, whatever it is marked
with. State (connected, waiting, failed) is always `ok` / `warn` / `error`, never a kind hue.

**The See-Through Chrome Rule.** Chrome backgrounds are written
`rgba(var(--panel-rgb), var(--chrome-alpha))` (or the `window`/`bg`/`deep` triplet), never as a
hex. `--chrome-alpha` drops below 1 while a window effect is on, and the desktop shows through.

## Typography

**Interface Font:** Segoe UI Variable Text (with Segoe UI, system-ui, -apple-system, Ubuntu)
**Mono Font:** Recursive Variable, mono axis on and casual off (with Cascadia Mono, Consolas)

**Character:** the operating system's own text for the interface, so it reads as a native tool.
Recursive, in its strict monospace cut, is reserved for code, keys and the ASCII banner. The
terminal's own font is the user's setting and is not part of this system.

### Hierarchy
- **Display** (700, `clamp(13px, 1.9vw, 21px)`, 1.12): the ASCII banner on the start screen,
  in Recursive, filled with the spectrum gradient. Nowhere else.
- **Headline** (400, 16.5px `--t-xl`, φ): the lead sentence under the banner.
- **Title** (600, 14.5px `--t-l`, 1.4): dialog and page titles (`h2`). Section heads inside a
  page (`h3`) are 600 at body size.
- **Body** (400, 13px `--t`, 1.4): everything by default: buttons, fields, rows, menus.
- **Prose** (400, 13px, line-height φ = 21px): paragraphs and text areas, so a line of text is
  as tall as a list row. Measure is capped at 62–70ch.
- **Label** (400, 11.5px `--t-s`): status bar, counts, tags, `kbd`, hints and secondary lines.
  Counts use `tabular-nums`.
- **Mono** (0.92em, Recursive `MONO 1, CASL 0`): `code`, `kbd`, `pre`. It always runs left to
  right, even in Arabic.

`--t-xxl` (18.5px) is used twice: the status centred over an RDP desktop while it connects or
after it closes, and the sign-in code a person approves in the browser (in Recursive, letters
0.1em apart). 21px (13 × φ) is the top of the scale, reached only by the banner.

### Named Rules
**The Five Sizes Rule.** Every `font-size` is a type token (`var(--t-s)` … `var(--t-xxl)`),
`inherit`, an `em`, or a `clamp()`. A raw `px` size fails `npm test`.

**The Golden Line Rule.** Prose and text areas set `line-height: var(--phi)`. Dense UI text
stays at 1.4.

## Layout

The window is padded by the gap (`--gap`, 5px) and holds up to three floating panels in a row:
the session rack (resizable; it collapses to a 38px rail), the center (a 34px tab strip over
the panes), and the file panel (340px). The splitter between rack and center is the gap itself.
Split panes are each their own panel (5px corners, 1px hairline) with the window's dark showing
between them. Pane sizers are 6px hit areas centred on that gap, gold-tinted on hover.

Spacing is the Fibonacci scale only: 1 2 3 5 8 13 21 34px (55 and 89 are allowed but unused).
Padding pairs go 1 : φ, vertical to horizontal: `3px 8px` for controls, `5px 8px` for compact
blocks, `8px 13px` for cards and asides, `13px` for a dialog body. Heights come from three
tokens: `--row` 21px (list rows, status bar, small buttons), `--control` 26px (buttons, fields,
menu items), `--bar` 34px (the tab strip). Dialogs are 377, 610 or 987px wide and keep 21px from
the window's edge.

The start screen sits on the golden section: the room above the welcome and below it go 1 : φ,
in a column at most 610px wide. Forms are a two-column grid (labels in `weak`, minimum 120px, gaps
`8px 13px`), and a label beside a multi-line field aligns to its first baseline.

This is a desktop app with no breakpoints: things shrink, ellipsize and scroll rather than
reflow. Long names truncate with an ellipsis; Settings page names wrap to two lines instead.
Layout uses logical properties (`inset-inline`, `margin-inline-start`, `padding-inline-end`) so
right-to-left languages mirror for free.

### Named Rules
**The Fibonacci Rule.** Padding, margin, gap, inset and border-radius values are Fibonacci pixels.
A stray 4px or 12px fails `npm test`.

**The Derived Value Rule.** A value worked out from others is written as `calc(…)` so it reads as
derived: `calc(8px + 16px + 5px)` is a field's text start past its icon, and
`calc(var(--r-menu) - 3px)` is a menu item's corner.

## Elevation & Depth

Depth is tonal first. The window is the darkest metal and each layer nearer the hand is a step
lighter (window → bg → panel → raised → hover). Text fields are set *into* the surface as darker
wells (`deep`). Resting panels have a 1px hairline and no shadow. Shadows are reserved for
layers that float above the work and close again, and they are soft and dark, never coloured.
Hairline edges inside chrome (the tab strip's bottom, the current Settings page's gold edge,
drag-insertion bars) are drawn as inset `box-shadow` lines so they take no room.

### Shadow Vocabulary
Three tokens on `:root` in `ui/app.css`, from lowest to highest:
- **Float** (`--shadow-float`: `0 4px 14px rgba(0, 0, 0, 0.3)`): bars floating over a view
  (find, reconnect) and the hint over a detached window.
- **Menu** (`--shadow-menu`: `0 6px 20px rgba(0, 0, 0, 0.36)`): context menus, dropdown lists
  and toasts.
- **Dialog** (`--shadow-dialog`: `0 8px 28px rgba(0, 0, 0, 0.4)`): modal dialogs and the command
  palette.
- **Scrim**: `rgba(8, 10, 12, 0.4)` behind a dialog, `0.32` behind the palette.

### Named Rules
**The Float-Only Shadow Rule.** A surface casts a shadow only if it floats over the work and goes
away again, and takes it from one of the three tokens. Panels, tabs, rows and buttons never do.

## Shapes

Three corners, all small: 3px (`--r-control`) for controls, fields, tags and panes; 5px
(`--r-menu`) for menus, toasts, the palette, split panes and cards; 8px (`--r-window`) for the
floating panels and dialogs. Status dots (7px), colour swatches (16–18px) and the dot on a live
session's icon are circles. Borders are a 1px hairline everywhere. Dashed hairlines mean "empty
or droppable" (an empty pane, a drop zone). A 2px gold border means "picked" (a colour scheme
card).

### Named Rules
**The Nested Curve Rule.** A rounded thing inside another keeps the same curve: its radius is
the outer one less the room between them, written as `calc()` (a menu item is
`calc(var(--r-menu) - 3px)`, a segmented button `calc(var(--r-control) - 1px)`).

## Components

### Buttons
Machined and quiet: small, flat faces that brighten a step under the pointer.
- **Shape:** gently squared (3px), 26px tall, `3px 8px` padding, 5px between icon and text.
- **Default:** a `raised` face with no visible border (a transparent 1px keeps sizes equal when a
  state adds one). Hover moves to `hover`. Disabled is 45% opacity.
- **Primary:** gold fill with `gold-ink` text; hover mixes in 14% white. One per dialog, on its
  main action.
- **Danger:** the primary of a question whose yes ends or deletes something (close a running
  tab, delete a folder, replace everything from a backup, replace the MCP token): red
  fill with `error-ink` (`button.primary.danger`). Something that can be put back is not asked
  about at all: it happens, and a toast offers Undo.
- **On / selected:** gold wash, gold text, and a 55% gold border, for toggles and picked options.
- **Flat:** transparent at rest, `raised` on hover. Tool buttons in bars are flat, icon-only
  (`3px 5px`), in `weak`, turning gold when on, and always carry a tooltip.
- **Link:** text that names another place is a borderless gold `button.link` (underline on
  hover). A whole sentence that links is `.quiet`: body colour, dotted underline, gold on hover.
- **Focus:** a 1px gold outline drawn inside the edge (`outline-offset: -1px`).

### Inputs / Fields
- **Style:** an inset well (`deep`) with a 1px hairline, 3px corners, 26px tall, `3px 8px`
  padding. Selects use the app's own chevron, set in 8px from the edge.
- **Focus:** the border turns gold; no glow.
- **Placeholder:** an example, in `weak` (never the engine's default grey, which fails contrast
  on the wells). Labels stay outside the field.
- **With an icon:** search fields put a 16px icon at 8px in and start text at
  `calc(8px + 16px + 5px)`.
- **Checks, radios, ranges, progress:** native controls tinted with `accent-color: var(--gold)`.
- **Too long:** a value, placeholder or choice that doesn't fit ends in `…`; fields never widen
  their dialog. In a `.form` the labels take what the longest needs, from 120 to 233px, and end
  in `…` past that; the fields take the rest. Boxes of text don't resize.

### Menus
- **Style:** panel at 98%, hairline, 5px corners, 3px padding, at least 190px wide, Menu shadow.
- **Items:** 26px, `3px 8px`, transparent; `raised` on hover. In a menu with icons, items
  without one are indented to line up with the others' text.
- **Groups:** `.sep` starts a group with a hairline 8px in from each side.
- **Destructive:** comes last, as `.danger`, and turns red on a 14% red wash when pointed at.
- Keyboard-driven, kept on screen, and opening leftwards in right-to-left languages.
- Menus open instantly, with no arrival motion: they follow the pointer and the keyboard, and
  any delay there reads as lag.

### Navigation
- **Tab strip:** a 34px panel bar. Tabs are flat, split by hairlines, `weak` when idle. The
  active tab takes the content's `bg` and drops its bottom line so it joins its pane. A 2px
  connector sits on top of each tab, in its mark (the tab's own, else its session's or folder's)
  or else its kind hue: hidden when idle, at 50% when the tab is visible in a split, and full on
  the active one. The tab's icon is always in its kind hue. A drag shows a 2px gold insertion bar.
- **Session rack:** 21px rows, folders with a rotating caret (0.12s) and a hairline guide under
  each level. Leaves indent past the caret so icons in one level line up. A session with an open
  tab gets a 7px green dot on its icon's corner, hollow once the connection is gone. Search hits
  and chosen rows take the gold wash. A marked session's or folder's name is in its mark. Its
  search opens out of the magnifier; Enter closes it and puts the keyboard on the hit's row in
  the tree, its folders opened (a second Enter opens the session); Escape gives the tree back the
  keyboard.
- **Sidebar foot:** the status pill (an inset well, label size in `weak`), then Settings and
  the account, in that order, at the foot's trailing end. The account is a flat icon button; signed
  in, it shows an 18px round avatar with the first letter of the name, ringed 2px in `error` when
  sync fails or the quota is full and in `warn` when it is nearly full. Its menu opens upwards,
  led by a `.head` block (name, address, last sync, the quota bar) under a hairline.
- **Focus ring:** one 2px gold ring around the focused pane, outside it in the gap (5px corners
  in a split). When the focus moves it glides to the next pane (0.2s, `--ease-out`; Static in
  Settings › Appearance makes it jump), so you see which way it went. While a pane edge is
  dragged it follows the edge with no delay.
- **Settings pages:** a 183px column. The current page is `raised` with a 2px gold edge on its
  leading side. Its search replaces the pages with hits; while it's open the page follows the
  best hit and lights that setting (`.lit`: its label and field in the gold wash, ringed 3px in
  it), and the hit is lit in the list too. Enter closes the search and puts the keyboard on the
  setting's control, whose light then fades over 2.6s (`.found`). Pages: General, Appearance,
  Shortcuts, Security (encryption, recovery passphrase, the lock), Sync, MCP (sub-sections "AI
  assistants (MCP)" and "Command line", whose commands and configuration sit in `.snippet`
  wells: `deep`, `8px 13px`, 3px corners, in Recursive) and About.
- **Status bar:** a 21px panel strip under a tab's view, label size in `weak`, items 13px apart.
  It leads with where the keyboard goes: `user@host`, then the session's folder with the folder's
  tags as gold chips (`Production/Web · prod`), so a production box says so under its pane. Then a
  state dot (green / amber / red / grey) and, at the end, the gold broadcast marker.
- **Tab-strip tools** come as the tabs do: Saved layouts always (it opens sessions), File
  Browser and Snippets once a tab is open, Broadcast and the layout segment with two or more.

### Dialogs and the Command Palette
- **Dialog:** a panel at 98% with 8px corners, a hairline header (`8px 8px 8px 13px`), and a 13px
  body with 8px between blocks; Dialog shadow over a 40% scrim. It holds the keyboard: Tab stays
  inside and Escape closes the top one. Widths: 377 / 610 / 987. A dialog whose body can run
  long puts its buttons in `foot`, a fixed footer under a hairline (`8px 13px`), so Save never
  scrolls out of reach. A dialog whose fields come and go while it's used (the session editor)
  is `top`: it hangs at 12vh, where the palette does, and grows downwards instead of
  re-centring, so what's under the pointer stays put. Its keyboard lands on the first field a
  person can see, never one in a closed `<details>` or a shortcut that fills the others in
  (`data-not-first`, as "Copy the server from"), else on the dialog itself. Dialogs and menus
  draw over everything wherever they're opened from (they move to the end of the page while open).
- **Sign in:** a 377 dialog. Sync needs an account and nothing else does; the browser opens on
  the sign-in page (Google, GitHub or Discord) and the dialog shows the code to approve there,
  centred, in Recursive at `--t-xxl`, with a flat copy button beside it and a gold spinner while
  it waits. Cancel is flat; after a failure, Try again is the primary.
- **Lock screen:** not a dialog but a whole opaque window (`rgb(var(--window-rgb))`, over
  everything, dialogs included) that fades in over 0.16s. A centred 377px column, 13px between
  parts: a 34px lock icon in gold, a 600 title at `--t-xl`, a password or PIN well with the
  primary Unlock beside it (Password / PIN switch as flat buttons, the picked one gold), then
  passkey and Windows Hello as plain buttons, and "Your sessions keep running." in `weak`.
- **Session editor:** 610 wide whatever it holds. The types in two even rows of five (a name
  that doesn't fit ends in `…`, the whole one in its tooltip), then where it connects (one
  "Host / port" row for every type that has a host), then the login, then Name and Folder; Advanced and "Icon, colour and notes"
  closed unless used; Save in the fixed footer.
- **Arrival:** dialogs, the palette, the bars over a terminal (find, reconnect) and toasts share
  one entrance, `float-in` in `ui/app.css`: a fade from 5px short of where the layer hangs,
  in 0.16s on `--ease-out`. The offset `--from` is 5px below by default and -5px for things
  hanging from the top (the palette, the find bar). The scrim behind a dialog fades in
  (`fade-in`) over the same 0.16s.
- **Palette:** 610px wide, 12vh from the top, at most 1/φ of the window tall, with 5px corners.
  A search well over 26px rows; the picked row takes the gold wash, with its shortcut in a `kbd`
  at the end of the row. Empty, it lists the open tabs, then the saved sessions, then the actions:
  a machine before a verb. A typed word has to sit in one stretch of a row's text (an abbreviation
  like `prd` for `prod`), never letters scattered across it.

### Tags, Keys and Toasts
- **Tag:** gold text on the gold wash, 3px corners, `0 5px`, label size, at most 80px wide.
- **Key cap (`kbd`):** an inset well with a hairline and a 2px bottom border so it reads as a
  key, in Recursive at label size.
- **Toast:** panel, hairline, 5px corners, `5px 13px`, Menu shadow, 34px above the bottom edge.
  It arrives with `float-in` and leaves faster than it came, fading out in 0.1s. One with an
  action (Undo after a delete, Reopen after Close other tabs) carries a gold link button,
  takes the pointer, and stays 8s instead of 2.4s.
- **Tooltip (`.tooltip`, `ui/lib/tooltip.ts`):** a tool button's title in the app's own panel at
  98%, hairline, 5px corners, `3px 8px`, label size on golden lines, Menu shadow, at most 377px
  wide; its shortcut follows in a `kbd` in `weak`. It hangs under what it explains (above when
  there is no room) and takes no pointer.
- **Quota bar (`Quota.svelte`):** sync storage used, in the account menu and Settings › Sync. A
  3px track in `line` with 2px corners, filled in gold; the fill turns `warn` when nearly full
  and `error` when full, and a label-size line under it says how much. Not a native `progress`:
  the webview paints that flat grey whatever the accent.

### Kind Glyph (signature)
A 21px tile with 3px corners, the kind's hue as the icon colour on 15% of the same hue (`.glyph`
in `ui/app.css`). It sits beside each way to start on the start screen and makes a connection
type recognisable before you read its name. The session editor's Type picker is a row of them:
one button per kind, glyph and name, the picked one in the gold wash. The type comes first in
the editor because it decides every field below it; changing it keeps the host typed so far.

### Broadcast Outline (signature)
Every terminal that receives broadcast keystrokes gets a 2px inset outline in a chosen colour
(`--oc`), drawn inside the pane at 3px corners. Its style is the user's (steady, breathe, fade,
ring, march, rainbow, off). While broadcast is on, the focus ring around the pane you type in
turns to a 2px dashed ring in the text colour (`fg`) and steps 2px further out, so it never reads
as one band with an outline in the same colour. All animated styles stop under reduced motion.

### Backgrounds
A picture or a GLSL shader behind the terminals, the sidebar or the tab bar (each its own),
drawn under the place's own colour at the strength set (a range of 5–95% in steps of 5,
`Backdrop.svelte`). A picture
fills its place by default, cut to fit; it can also stretch, fit whole, tile at its size, or sit
centred at its size (`FITS`). A shader always fills. The place isolates its stacking, so the
backdrop stays under everything in it.

### Session Icons
Boxicons SVGs through `Icon.svelte`, 16–18px. Session and folder icons (`SessionIcon.svelte`)
have small moving parts (blink, flow, spin, bob…) that play only while the pointer is over that
session's row or tab, never on the interface's own buttons, and never under reduced motion.

## Do's and Don'ts

### Do:
- **Do** use only Fibonacci pixels for spacing and radii (1 2 3 5 8 13 21 34), with padding in
  1 : φ pairs (`3px 8px`, `5px 8px`, `8px 13px`).
- **Do** set every text size from the type tokens (`--t-s --t --t-l --t-xl --t-xxl`).
- **Do** size things with `--row` (21px), `--control` (26px) and `--bar` (34px), and write any
  derived value as `calc(…)`.
- **Do** keep gold for focus, selection, toggles that are on, links, drop targets and the one
  primary action. Read it as `var(--gold)`, since the user can change it.
- **Do** colour a session by its kind hue (icon, tab connector, glyph tile) through `--kind`.
- **Do** paint chrome as `rgba(var(--…-rgb), var(--chrome-alpha))` so window effects show through.
- **Do** end a menu with its destructive item (`.danger`) and start groups with `.sep`.
- **Do** offer Undo instead of a question when the thing can be put back, and ask (with a
  danger button) only when it can't: a running connection, a folder of sessions, a restore.
- **Do** focus the safe answer when yes can't be undone or is a security call (Reject on a new
  host key, Cancel on a delete or restore, Keep editing on discard), so Enter never does the
  irreversible thing; an ordinary yes (Close tab, which Reopen can undo) keeps the focus.
- **Do** make tool buttons icon-only with a tooltip.
- **Do** keep motion to state and continuity: 0.1–0.2s, arrivals on `--ease-out`, a new floating
  layer through `float-in`, exits faster than entrances. CSS motion stops under
  `prefers-reduced-motion` through `ui/app.css`; a Svelte transition must check it itself.
- **Do** use logical properties (`inset-inline-*`, `margin-inline-*`) so right-to-left mirrors.

### Don't:
- **Don't** use off-scale spacing (4px, 6px, 10px, 12px, 16px) or a raw `font-size` in px; the
  scale test fails on them.
- **Don't** put a shadow on a resting panel, tab, row or button, or write a new shadow value for
  a floating layer: use `--shadow-float`, `--shadow-menu` or `--shadow-dialog`.
- **Don't** use gold for headings, icons, illustration or large fills other than the primary
  button.
- **Don't** use a kind hue to show state, or a status colour to mean a kind of connection.
- **Don't** let a user's colour recolour a session's icon: marks go on names (`.tinted`) and tab
  lines, and accent presets stay off the kind hues and the error red.
- **Don't** hard-code the accent's hex, or the chrome as an opaque hex.
- **Don't** animate the interface's own icons. Only session and folder icons move, and only on
  hover.
- **Don't** add big padding or wide gaps. The densest comfortable layout is the house style.
- **Don't** use the spectrum gradient anywhere but the start screen's banner, where it plays once.
