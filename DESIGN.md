---
name: xtermium
description: A terminal and session manager for sysadmins, in the brand's near-black chrome with one phosphor amber accent and a cable colour per kind of connection.
colors:
  gold: "#ffb000"
  gold-ink: "#050506"
  gold-soft: "rgba(255, 176, 0, 0.14)"
  mark: "#f2f2f2"
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
  k-coop: "#c9b48a"
  ok: "#4fc08d"
  warn: "#e0b04a"
  error: "#ec6a6a"
  error-ink: "#200a0a"
  window: "#050506"
  deep: "#08080a"
  bg: "#0b0b0d"
  panel: "#111114"
  raised: "#1d1d22"
  hover: "#26262c"
  line: "#2c2c33"
  field-line: "#2c2c33"
  fg: "#e6e6e1"
  weak: "#9d9da6"
typography:
  prompt:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.4
  headline:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "16.5px"
    fontWeight: 600
    lineHeight: 1.618
    letterSpacing: "-0.025em"
  title:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "14.5px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "-0.01em"
  short:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.4
  short-strong:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "13px"
    fontWeight: 500
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
  label-short:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "11.5px"
    fontWeight: 400
    lineHeight: 1.4
  mono:
    fontFamily: "JetBrains Mono Variable, Cascadia Mono, Consolas, monospace"
    fontSize: "0.92em"
    fontWeight: 400
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
    typography: "{typography.short}"
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
    backgroundColor: "color-mix(in srgb, #ffb000 86%, white)"
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
    textColor: "{colors.fg}"
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
    typography: "{typography.short-strong}"
  title-bar:
    backgroundColor: "transparent"
    textColor: "{colors.weak}"
    height: "{spacing.bar}"
  title-tool:
    backgroundColor: "transparent"
    textColor: "{colors.weak}"
    rounded: "{rounded.control}"
    size: "{spacing.control}"
  title-tool-new:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.fg}"
    rounded: "{rounded.control}"
    size: "{spacing.control}"
  guide:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.menu}"
    padding: "3px"
    width: "min(610px, 100%)"
  guide-step:
    backgroundColor: "transparent"
    textColor: "{colors.fg}"
    typography: "{typography.short-strong}"
    rounded: "2px"
    padding: "3px 8px"
  guide-step-open:
    backgroundColor: "{colors.hover}"
    padding: "5px 8px 8px"
  guide-step-done:
    textColor: "{colors.weak}"
    typography: "{typography.short}"
  title-field:
    backgroundColor: "{colors.deep}"
    textColor: "{colors.weak}"
    typography: "{typography.short}"
    rounded: "{rounded.control}"
    padding: "0 8px"
    height: "{spacing.control}"
    width: "clamp(233px, 38vw, 610px)"
  rack-row:
    textColor: "{colors.fg}"
    padding: "0 5px"
    height: "{spacing.row}"
  rack-row-chosen:
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
    typography: "{typography.label-short}"
    padding: "0 8px"
    height: "{spacing.row}"
  tag:
    backgroundColor: "{colors.gold-soft}"
    textColor: "{colors.gold}"
    typography: "{typography.label-short}"
    rounded: "{rounded.control}"
    padding: "0 5px"
  toast:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.fg}"
    rounded: "{rounded.menu}"
    padding: "5px 13px"
  kbd:
    backgroundColor: "{colors.deep}"
    typography: "{typography.label-short}"
    rounded: "{rounded.control}"
    padding: "0 5px"
---

# Design System: xtermium

## Overview

**Creative North Star: "The Operator's Rack"**

xtermium is a rack of live connections, in the brand's world: the operator's terminal of the
website (xtermium.com) and the brand kit, carried into the app. The chrome is the website's
near-black ground and surfaces, neutral and a hair apart, getting lighter the nearer they sit to
the hand. Each kind of connection has its own hue, the way cables on a patch panel are
colour-coded: SSH is cobalt, RDP violet, serial chartreuse. One phosphor amber accent marks where
you are and what you picked. Nothing else is coloured, so a glance at the rack tells you which
kind of session each row and tab is, and a glance at the amber tells you where the keyboard is.
The start screen waits at the website's prompt, `~/xtermium $` and a blinking amber block cursor,
over the brand kit's logo.

The feel is quiet and exact, as dense as a modern code editor: 13px text, 26px controls, 21px
rows, small precise corners. JetBrains Mono, the brand's face, says everything short (titles,
buttons, tags, the status bar, code); the system face says everything long (text, menus and
lists), so the app still reads as a native tool. The sidebar (the session rack), the tab area
with its panes, and the file panel are rounded panels floating a few pixels apart on a darker
window. They are flat at rest and are told apart by tone and hairline, not by shadow. Only things
that float above the work cast a shadow: menus, dialogs, the command palette, toasts.

Every proportion comes from the golden ratio (φ ≈ 1.618). Spacing steps along the Fibonacci
scale, padding pairs keep both values on it with the horizontal at least the vertical, type steps
by a quarter of φ (rounded to the half pixel), and dialogs are 377, 610 or 987px wide. Settings is
a golden rectangle, 987 × 610. `ui/lib/scale.test.ts` enforces, in `npm test`, that padding,
margin, gap, inset, offsets and border-radius are on the scale and that every `font-size` is a
token; widths, heights and line-heights follow the scale by convention (the token audit at the
end names the exceptions).

**Key Characteristics:**
- Near-black neutrals in six tonal steps (the website's ground, surfaces and hairlines), a dark
  theme by default and a full light theme.
- One accent, Phosphor Amber, which the user can change. It is never decoration.
- Eleven cable colours, one per kind of connection, used only to say "this is an SSH/RDP/… thing".
- JetBrains Mono for short text, the system face for long text and lists.
- The brand kit's horizontal logo on the start screen and in Settings › About; its own amber and
  white stay inside it.
- Fibonacci spacing (1 2 3 5 8 13 21 34px) and five type sizes, both enforced by a test.
- Rounded panels on a darker window. Flat at rest; shadows only on floating layers.
- Icon-only tool buttons with tooltips. Boxicons SVGs. Session and folder icons move on hover; interface icons only spin or pulse while something waits.
- Right-to-left ready: logical properties throughout, with flips for chevrons, menus and carets.

## Colors

Near-black neutral greys, one phosphor amber, and a set of connection hues that act as cable
colours.

### Primary
- **Phosphor Amber** (`gold`): the accent and the only one, the website's phosphor (`#ffb000`; on the
  light theme ink as text or a line, amber only as a fill, a wash or a mark, below). The token keeps its earlier name, `--gold`. It marks focus
  (`:focus-visible` outline, field borders), toggles that are on and options that are picked
  (with `gold-soft` behind amber text), links that go to another place, drop targets, the focused
  pane's outline, the active Settings page's edge, the start screen's prompt sigil and block
  cursor, and fill on the single primary button of a dialog. Users can change it (Settings ›
  Appearance shows the default as "Amber (default)"): the presets are Brushed gold `#d8a84e` (the
  app's accent before the brand's), then Platinum `#c3ccd8`, Bronze `#a99a78` and Rose gold
  `#e2a395`, metals apart from every kind hue and the error red (`ACCENTS` in `api.ts`). On the
  light theme any accent does what the amber does: text and lines stay ink, and the colour itself
  is the fill, the wash and the marks, so no darkened (mustard or brown) accent shows as text.
  With the default amber the tokens are `ui/app.css`'s own static values; with a custom accent the
  app sets `--gold` as `light-dark(#141414, colour)`, `gold-fill` as the colour, and derives
  `gold-ink` and `gold-fill-ink` (`#111` or white, by luminance) and `gold-soft` (14%, 22% on
  light) at runtime (`App.svelte`). So never hard-code its hex in a component; use `var(--gold)`.
  The one exception is the Amber (default) swatch in Settings › Appearance, which hard-codes it
  so it shows the default whatever accent is picked.
- **Amber Ink** (`gold-ink`): text on a solid amber fill (the primary button): the ground,
  `#050506`, on dark, and white on light, where `gold` is ink; `#111` or white, whichever reads,
  on a custom accent.
- **Amber Fill** (`gold-fill`, `gold-fill-ink`): the accent where it is a fill or a mark rather
  than text or a line: the primary button, the focused pane's ring, checkboxes, sliders, selected
  text, the start screen's block cursor. The gold itself on dark; `#ffb000` under `#141414` on
  light (9.4:1), and the picked colour itself, with `#111` or white, for a custom accent.
- **Amber Wash** (`gold-soft`): the translucent tint (14%, 22% amber on light) behind picked rows, search hits, a
  setting a search points at, tags and drop zones. The sidebar's drop-into-folder highlight is a
  stronger mix, 18% amber, to tell "into" from "before" (the amber line).

The broadcast marker in a status bar reads the accent through `--broadcast`, an alias of `--gold`
(as `--fwd` and `--back` alias kind hues for tunnels). On light it is `warn` (`#a34800`): it is the
one piece of accent text that must stand out, amber text doesn't read there, and ink wouldn't
stand out.

### Secondary
The connection hues, one per kind of session (`--k-*`), keyed like cable colours:
- **Cobalt** (`k-ssh`): SSH. Also forward tunnels (`--fwd`).
- **Teal** (`k-shell`): local shells. Also reverse tunnels (`--back`).
- **Chartreuse** (`k-serial`): serial lines.
- **Violet** (`k-rdp`): RDP desktops.
- **Rose** (`k-browser`): websites.
- **Copper** (`k-telnet`): telnet.
- **Cyan** (`k-winrm`): WinRM.
- **Orchid** (`k-vnc`): VNC.
- **Bare Titanium** (`k-kvm`): BMC consoles, deliberately the least coloured.
- **Ember** (`k-agent`): AI-agent tabs. It shares its value with `error`, an accepted collision:
  an agent tab is told by its icon and connector, a failed one by the `error` dot and title, never
  by the hue alone.
- **Champagne** (`k-coop`): tabs shared with you in a Terminal Co-op ring.

They colour a session's icon (always), the 2px connector on top of its tab (unless the tab
carries a mark), and the tile behind a kind glyph (15% of the hue). Components read them through
a local `--kind`.

**Marks.** A person can mark a session, a folder or a tab with a colour (`COLORS` in `api.ts`:
nine swatches, no amber, plus any colour). A mark never takes a kind hue's place: it tints the
session's or folder's name (`.tinted`, its OKLCH lightness held to at least `--tint-lo`, 0.72, on
dark and at most `--tint-hi`, 0.48, on light, so every swatch reads at 4.8:1 or better while
keeping its hue) and the tab's
connector line. A folder has no kind, so its icon wears its mark too. A folder's mark carries
down to every name inside it, under each session's own.

### Tertiary
Status, kept apart from the kind hues:
- **Live Green** (`ok`): connected (the status dot, the dot on a session's icon in the rack).
- **Amber** (`warn`): waiting, slow, or a dead tab's title; the border of the reconnect bar.
  On light, a warning paragraph (`p.warn`, `.row.warn`, or a row holding a `span.warn`, as the
  MCP token warning and the missing recovery passphrase) is a box instead: ink text on
  `warn-wash` (amber `#ffb000` at 22%) with a 3px `warn-fill` edge at the start, `5px 8px`
  inside, its icon in `warn`. Dark text in the warning's own colour reads as rust on white.
  Short warnings inside a line (a shortcut's clash, a dead tab's name) keep `warn` text.
- **Signal Red** (`error`): disconnected, failures, the hover of a menu's
  destructive item (14% wash behind red text), and the fill of a danger button.
- **Red Ink** (`error-ink`): text on a solid red fill (a danger button).

### Neutral
From the back of the window to the hand, the website's near-black, with no blue in it:
- **Ground** (`window`): the window itself, showing in the gaps between panels and split panes.
- **Inset Well** (`deep`): wells set into a panel: text fields, `kbd`, segmented controls, the
  sidebar's status pill, code snippets. In the light theme it is white, the lightest step.
- **Base** (`bg`): the content area under the tabs, and the active tab that joins it.
- **Surface** (`panel`): the sidebar, the tab strip, the file panel, the status bar, and every
  floating layer (menus, dialogs, palette, toasts at 98% opacity, or the Opacity setting with the
  Transparent effect on, up to 98%).
- **Raised** (`raised`): button faces, menu-item hover, the current Settings page, scrollbar
  thumbs.
- **Lit** (`hover`): a button under the pointer.
- **Hairline** (`line`): every 1px border and divider.
- **Ink** (`fg`): primary text.
- **Muted** (`weak`): secondary text, form labels, idle tabs and tool buttons, counts, the
  start screen's prompt.
- **Mark** (`mark`): the logo's three large tiles and its wordmark, `#f2f2f2` on dark and the
  brand's charcoal `#141414` on light, as the brand kit draws them. Nothing else.

**Picked, current, lit.** Three surfaces say three things, and components keep them apart:
*picked* (an option that is on, a chosen row of a multi-selection, a search hit) is the amber
wash, `gold-soft`, with amber text on a toggle (`button.on`) and `fg` on a row (`.chosen`);
*current* (the keyboard's row in the session rack, the current Settings page) is `raised` with a
2px amber inset edge (`.current`); *lit* is under the pointer: a flat thing (a menu item, a list
row, `button.flat`) takes `raised`, a filled button takes `hover`. The palette's keyboard row
(`.item.on`) is the one place still written as picked; it means current. A picked button's edge is
`gold-fill` (`--picked-line`), at 55% on dark and solid on light: amber on both themes, where the
light theme's ink `gold` would draw a black box.

The light theme (`:root[data-theme="light"]` in `ui/app.css`) redefines every token above with
the same roles, in neutral greys with no blue tint and text in the brand's charcoal `#141414`. It
draws the same elements as dark, in other colours (one exception: links, under Buttons). It
reads like a light desktop app: a light grey ground (`window` `#e5e5e8`) with near-white panels
(`#f7f7f8`), white where the work and the fields are (`bg`, `deep`), `raised` `#ececef`,
`hover` `#e2e2e6`, hairlines `#dcdce1`, soft two-layer shadows and a light scrim. A field's edge is
its own token, `field-line` (`line` on dark): `#8e8e94` on light, 3.0:1 on `panel`, since a white
field on a near-white panel has nothing else to show where it is. It is neutral first: what the
accent says as text or a line (links, focus, a field's border, a tag's or a picked option's text) is
said in ink, `gold` `#141414`, with links underlined. The dark theme's amber `#ffb000` stays only
where it reads: as a fill under ink (`gold-fill`, 9.4:1), as a 22% wash behind ink (tags, picked
rows and toggles, search hits), and as marks (the focused
pane's ring, the terminal's and the start screen's cursor, slider knobs, checkboxes). As text it
would be 1.7:1 on these surfaces, and a darker amber that passes turns brown. The pane ring is a
mark at about 1.5:1 on the ground, under the 3:1 a graphic asks, kept amber because it says where
the keyboard is in the same colour on both themes. `weak`, `ok`, `warn` and `error` hold 4.5:1 on `panel`,
`raised` and `hover` (a button under the pointer; checked 2026-11-09: `weak` 5.04 on `hover`,
`error` 5.82, `warn` 4.67 (`#a34800`, a burnt orange: the old `#745000` read as mustard), `ok`
6.35), and the kind hues darken to hold contrast on the light greys (serial `#5a7d14` and Co-op
`#7a5f52`, away from olive and khaki). Nothing on light is a dark yellow: one dark enough to read
turns mustard, so yellows become orange. The terminal's highlight rules keep one colour each,
made for dark terminals; when a terminal's background is light, each is darkened in OKLCH just
enough to hold 4.5:1 on it, keeping its hue, with yellows taking `warn`'s orange
(`settings::readable`): on white, errors `#d14352`, warnings `#b95c22`, ok `#2b8743`, addresses
`#2f79c3`. The xtermium Light scheme's own colours hold 4.5:1 on its background the same way
(white, slot 7, excepted; a test checks it). The light values are also kept in `.impeccable/design.json`
(`colorMeta.*.light`).

### Named Rules
**The One Accent Rule.** Amber says "you are here" or "you picked this", and nothing else. Two
exceptions: the fill of a dialog's single primary button, and a folder's tag (`.tag`, `prod`,
`lab`…), which is a fact about where you are and shows on the folder's row and under a pane. No
amber headings, icons or decorative strokes. The start screen's prompt sigil and block cursor are
the website's: they say where you type.

**The Logo Colours Rule.** The logo's own amber, `#FFC20E`, and its white (`--mark`) live only in
the logo. The brand amber is not the accent and is never UI state, as the website's One Phosphor
Rule says; the accent is the phosphor `--gold`. `ui/lib/Logo.svelte` swaps the artwork's
`#F2F2F2` fills for `currentColor`, which is `--mark`, so one file serves both themes; the small
amber tile is never recoloured.

**The Cable Colour Rule.** A kind hue identifies a kind of connection and nothing else, and
nothing else takes its place: a session's icon is always in its kind's hue, whatever it is marked
with. State (connected, waiting, failed) is always `ok` / `warn` / `error`, never a kind hue.

**The See-Through Chrome Rule.** Chrome backgrounds are written
`rgba(var(--panel-rgb), var(--chrome-alpha))` (or the `window`/`bg`/`deep` triplet), never as a
hex. `--chrome-alpha` drops below 1 while the Transparent effect is on, and the desktop shows through.
Any place is two layers, the window's ground and a panel or terminal on it, and `--chrome-alpha` is
worked out so the two together make the Opacity setting, in every window: don't paint a third layer
between them. A floating layer is one layer at `--float-alpha`, over what's already there, as a window
is over another. The lock screen stays solid: it hides what's behind it. `--solid` (1, or 0 with
the Transparent effect) switches off the active tab's and a detached window's own fill so the pane
shows through. The title bar is the one place with only the ground under it, so a dark desktop
turns it grey on light: there its icons (`--bar-ink`) go from `weak` to ink while `--solid` is 0,
and the palette field and the New session key are a second layer at `--chrome-alpha`, as a panel
is, instead of solid slabs on it. On light the New session key is `deep` (white, as the field),
since `raised` is barely off the bar's grey.
Two exceptions, named: the scrim behind a dialog (`--scrim`) and the palette
(`--scrim-soft`) is the dark ground at 0.4 and 0.32 (black at 0.18 and 0.12 on light), and the RDP view's bars over
the picture use fixed dark greys (`#0c0c0c`, `rgb(12 12 12 / 0.75)`), because they sit on the
remote desktop's black, not on chrome.

## Typography

**Short Font:** JetBrains Mono Variable, the brand's face (with Cascadia Mono, Consolas), bundled
from `@fontsource-variable/jetbrains-mono`
**Long Font:** Segoe UI Variable Text (with Segoe UI, system-ui, -apple-system, Ubuntu)

**Character:** the website's split. JetBrains Mono, the face the logo's wordmark is drawn in
(Bold, as outlines in the artwork), says everything short: titles, buttons, tags, the pane's
status bar, the start screen's lead, and code and keys. The operating system's own face says
everything long: body text, prose, field labels, menus and lists, so the app still reads as a
native tool. `--short` names the short face (`var(--mono)`), `--ui` the long one. The terminal's
own font is the user's setting and is not part of this system.

### Hierarchy
- **Prompt** (mono 400, 13px `--t`, 1.4, in `weak`): the start screen's `~/xtermium $`, with the
  `$` at 500 in amber and a blinking amber block cursor after it; decorative, hidden from
  assistive tech. Nowhere else.
- **Headline** (mono 600, 16.5px `--t-xl`, 1.618 (φ), -0.025em, the website's title tracking): the
  lead sentence under the logo.
- **Title** (mono 600, 14.5px `--t-l`, 1.4, -0.01em): dialog and page titles (`h2`). Section
  heads inside a page (`h3`) are mono 600 at body size.
- **Short** (mono 400, 13px `--t`, 1.4): buttons.
- **Body** (400, 13px `--t`, 1.4, system face): everything else by default: fields, field
  labels, rows, menus.
- **Prose** (400, 13px, line-height φ = 21px): paragraphs and text areas, so a line of text is
  as tall as a list row. Measure is 55–75ch (62ch on the start screen, 70ch in Settings).
- **Label** (400, 11.5px `--t-s`): counts, hints and secondary lines in the system face; the
  status bar, tags and `kbd` at the same size in the mono. Counts use `tabular-nums`.
- **Mono** (0.92em, JetBrains Mono): `code` (`pre` keeps body size); `kbd` in the mono at label
  size (`--t-s`). It always runs left to right, even in Arabic.

`--t-xxl` (18.5px) is used three times: the status centred over an RDP desktop while it connects
or after it closes, the sign-in code a person approves in the browser (in the mono, letters
0.1em apart) and the Co-op passcode (in the mono, 0.2em apart). The logo is drawn, not set: it takes its size from its width (`min(377px, 100%)` on
the start screen, 233px in Settings › About).

### Named Rules
**The Five Sizes Rule.** Every `font-size` is a type token (`var(--t-s)` … `var(--t-xxl)`),
`inherit`, an `em`, or a `clamp()`. A raw `px` size fails `npm test`.

**The Golden Line Rule.** Prose and text areas set `line-height: var(--phi)`. Dense UI text
stays at 1.4.

**The Short-in-Mono Rule.** As on the website: if it is a title, a button, a tag, a key, a status
line or code, it is JetBrains Mono (`var(--short)`, or `var(--mono)` for code). If it is read as
text or down a list, it is the system face (`var(--ui)`). Lists are long even when each entry is
short, so these stay in the system face: menu items (`.menu button`), the rows of the session
rack and the palette (`button.item`), the Settings page list (`.pages button`) and the session
editor's kind picker (`.kinds button`, five to a row, where the narrower face keeps the names
whole). In Arabic, Hindi, Japanese and Chinese (`lang` ar, hi, ja, zh) `--short` falls back to
`--ui`, since JetBrains Mono has no letters for those scripts.

## Layout

Every window starts with xtermium's own title bar (`TitleBar.svelte`, `--titlebar`, as tall as
the tab strip: 34px) instead of the system's, so it looks the same on Windows, Linux and macOS. It
sits on the window's ground with no panel of its own, in three parts. At the start, 13px in, the
mark alone (`Logo.svelte` with `mark`, 26px; or the picture chosen under Settings › Appearance ›
App icon, which also takes the mark's place on the start screen, the windows, the taskbar and the
tray), then the main window's tools, icon-only, 21px glyphs in 26px squares 3px apart, `weak`
until pointed at: the burger that collapses and expands the sidebar, a local shell,
Tunnels and the saved layouts' menu, then, 13px apart, New session as a filled key (`raised`).
In the centre: a field (an inset
well, 26px tall, `clamp(233px, 38vw, 610px)` wide) naming where the keyboard goes: the focused
tab's icon in its kind hue, its name (`short-strong`: mono 500, `fg`) and, in `weak`, what the shell or page says
of itself; with no tab open, the palette's own placeholder. The field opens the command palette.
At its end, behind a hairline, a chevron opens the workspace picker (`ui/lib/WorkspacePicker.svelte`),
hung under the field at its width in a menu's dress: a search field, a row that creates the name
typed when no workspace has it, the workspace in use ticked, the others by name ten to a page
(Previous, Next), then Manage workspaces…. A right click on the bar's free space or the mark
opens the window's menu: Windows' own system menu, on Linux a `.menu` of Minimize, Maximize, Close. Locked, the centre is empty. A detached tab's window shows its name
there. At the end, the five layout buttons (21px glyphs in 26px squares, 3px apart, the one in
use amber), then the three
window buttons, 46px wide, square, their glyphs 10px hairlines in `weak`; under the pointer they
take `hover`, and Close takes `error`. The room between is what the window is dragged by (a
double click maximizes). While another window has the keyboard the bar's contents drop to half
strength. macOS keeps its own three buttons at the left instead of the mark, and the bar leaves
them room. Dialogs, the palette and the lock screen start under the bar, so the window can always
be moved, minimized and closed. There is no full-screen mode.

Every control in the bar is as tall as the field (`--control`), so the row shares one top and
bottom edge, centred in the bar. The sidebar has no header: its first row's middle sits on the tab
strip's middle beside it (`calc((var(--bar) - var(--row)) / 2)` above it), and the collapsed rail
holds the sidebar's foot and, while there is a message, a button for it that opens the sidebar. Finding a session is the palette's job (it searches names, addresses, folders and
folder tags).

Under the bar the window is padded by the gap (`--gap`, 5px) and holds up to three floating panels in a row:
the session rack (resizable; it collapses to a 38px rail), the center (a 34px tab strip over
the panes), and the file panel (340px). The splitter between rack and center is the gap itself.
Split panes are each their own panel (5px corners, 1px hairline) with the window's dark showing
between them. Pane sizers are 6px hit areas centred on that gap, amber-tinted on hover.

Spacing is the Fibonacci scale only: 1 2 3 5 8 13 21 34px (55 and 89 are allowed but unused).
Padding pairs keep both values on the scale, the horizontal at least the vertical, usually
adjacent steps: `3px 8px` for controls, `5px 8px` for compact blocks, `8px 13px` for cards and
asides, `13px` for a dialog body. Panel and column widths are Fibonacci (144, 233, 377, 610, 987)
or derived with `calc()`; a content-driven maximum (a tab title, a tooltip) may be any value; the
ones that are neither yet are in the token audit. Heights come from three
tokens: `--row` 21px (list rows, status bar, small buttons), `--control` 26px (buttons, fields,
menu items), `--bar` 34px (the tab strip). Dialogs are 377, 610 or 987px wide and keep 21px from
the window's edge.

The start screen sits on the golden section: the room above the welcome and below it go 1 : φ,
in a column at most 610px wide, 13px between its parts. The welcome starts at the website's
prompt (`~/xtermium $` and a blinking amber block cursor; what you type shows after the sigil in
`fg`, as the shell will get it), then the brand kit's horizontal logo
(the mark beside the "XTermium" wordmark, `assets/logo-horizontal.svg` through
`ui/lib/Logo.svelte`) at `min(377px, 100%)` wide, arriving with `float-in` (a button, looking the
same, when Settings › Appearance gives it a link), then the lead
sentence and the ways to start, and a `weak` line that says "Type a command and press Enter to
run it in a shell." Settings › About shows the same logo at 233px.

For someone new, the ways to start give way to **Getting started** (`ui/lib/Guide.svelte`): one
panel (hairline, `--r-menu`) headed by its title in the short face (`--t-l`, 600), "n of 8 done"
in `weak` ("All done" once every step is) and a flat Hide, over eight rows, most important first: a local shell, a saved server
(with an import link under it while that step is next), the palette, panes, a tunnel, broadcast,
the lock, sync. A row is a flat button in three columns, one line each: its number, plain and
`weak` (a tick on an `ok` tint once done; the next step's in a 21px `fg` ring), the step in the
short face at 500, and its shortcut as quiet `weak` text, not a key cap. One row is open at a
time, the next step's to start with: it sits on a `--hover` band with its shortcut in `fg`, and
under it its hint in the long face at `--t-s` (`weak`) and a Try it button (the list's one
primary; Do it again, an ordinary button, on a done step). Clicking another row opens that one
instead, so a step can be read before it's done; only Try it does the step, for real (panes and
broadcast open two local shells to show on), and the next step opens after it. The words wrap,
so a narrow window never puts them under the shortcut. A step is done when the state it leads
to is there (a saved session, a tunnel, a lock, an account) or once it was done; done rows step
back to `weak`. While the list shows with nothing saved, the
"No saved sessions yet" line goes (the list says it). A new install starts with it on; installs
from before it don't. Hide puts it away with an Undo toast; Settings › General › Getting started
turns it on again and Start over clears its ticks.

Most steps open a tab, which hides the start screen, so the list is reachable from there too.
While it's on, unfinished and behind tabs, the tab strip's tools start with its progress: an
amber pill (a `gold-soft` wash with a `picked-line` edge, `picked` on hover and while open)
holding a 16px ring filling in `gold-fill` over a `weak` track at 55%, "Getting started" in `fg`
at `--t-s` and 500, and "2/8" in the short face at 600. It is the one tinted thing in the strip,
where the eye goes, and over nothing (a card floating on the panes would cover output and sit
under website views). The first time it appears on a device it pulses twice, an amber halo
spreading 8px and fading; after that the tick's toast says where the list went. Reduced motion
stops the pulse. It opens the same list in a popover (a `.menu`, 377 wide, the list inset without
its own edge) hanging under it, its end lined up with the pill's, right to left too; the open
step's Try it takes the keyboard, opening another row keeps it in the popover, Escape closes it.
A step ticked away from the list brings a toast, "Done: … Next: …", with Show opening the
popover, and the palette's "Getting started" opens it as well (turning the list back on if it
was hidden). A workspace switch that ticks a step does so without a toast.

The prompt is real. A character typed there (outside any field, with no dialog, palette or menu
open) opens a local shell, and every key typed is held until the shell can take it, then handed
over in order, the first included (Settings › General › Sessions turns this off, and the line with
it). The tab shows in the strip at once, as the key's acknowledgement. The welcome leaves in
0.12s, accelerating away (ease-in), starting at the key with no delay. The terminal stays unseen
until the shell can take keys and the welcome has gone, then fades in over 0.2s on `--ease-out`:
one after the other, never a crossfade of the logo over the shell's text. Under reduced motion
it is a cut.

Forms are a two-column grid (labels in `weak`, minimum 120px, gaps `8px 13px`), and a label
beside a multi-line field aligns to its first baseline.
A customisable setting with more than two values ends its row with a flat reset button, shown
only once the value differs from the default and naming that default in its tooltip; it keeps
its place while hidden, so rows never move, and a reset offers Undo.

This is a desktop app with no breakpoints: things shrink, ellipsize and scroll rather than
reflow. Long names truncate with an ellipsis; Settings page names wrap to two lines instead.
Layout uses logical properties (`inset-inline`, `margin-inline-start`, `padding-inline-end`) so
right-to-left languages mirror for free.

### Named Rules
**The Fibonacci Rule.** Padding, margin, gap, inset, offsets and border-radius values are Fibonacci
pixels; a stray 4px or 12px there fails `npm test`. Widths, heights and line-heights are not
tested: keep them on the scale or derived, or name the exception (pane sizers 6px, scrollbars
10px, the title bar's 46px Windows buttons, `progress` 4px).

**The Derived Value Rule.** A value worked out from others is written as `calc(…)` from the
tokens it comes from, so it reads as derived and follows them: `calc(8px + var(--icon-s) +
var(--gap))` is a search field's text start past its 16px icon (`--icon` is the 18px of controls
and rows), `calc(var(--r-menu) - 3px)` a menu item's corner, and
`calc(var(--control) + 2 * var(--gap) + 2px)` the collapsed rail.

## Elevation & Depth

Depth is tonal first. The window is the darkest tone and each layer nearer the hand is a step
lighter (window → bg → panel → raised → hover). Text fields are set *into* the surface as darker
wells (`deep`). Resting panels have a 1px hairline and no shadow. Shadows are reserved for
layers that float above the work and close again, and they are soft and dark, never coloured.
Hairline edges inside chrome (the tab strip's bottom, the current Settings page's amber edge,
drag-insertion bars) are drawn as inset `box-shadow` lines so they take no room.

### Shadow Vocabulary
Three tokens on `:root` in `ui/app.css`, from lowest to highest:
- **Float** (`--shadow-float`: `0 4px 14px rgba(0, 0, 0, 0.3)`): bars floating over a view
  (find, reconnect), the insecure-connection bar over an unencrypted VNC desktop, and the hint
  over a detached window.
- **Menu** (`--shadow-menu`: `0 6px 20px rgba(0, 0, 0, 0.36)`): context menus, dropdown lists
  and toasts.
- **Dialog** (`--shadow-dialog`: `0 8px 28px rgba(0, 0, 0, 0.4)`): modal dialogs and the command
  palette.
- **Scrim** (`--scrim`: `rgba(5, 5, 6, 0.4)`, the dark ground) behind a dialog; `--scrim-soft`
  (`0.32`) behind the palette.

The light theme redefines all four lighter, each with a 1px ring so the edge holds on white:
`0 0 0 1px rgba(0, 0, 0, 0.05), 0 4px 12px rgba(0, 0, 0, 0.1)` (float),
`0 0 0 1px rgba(0, 0, 0, 0.08), 0 8px 24px rgba(0, 0, 0, 0.14)` (menu),
`0 0 0 1px rgba(0, 0, 0, 0.06), 0 18px 48px rgba(0, 0, 0, 0.2)` (dialog), and a black scrim at
0.18 and 0.12.

### Named Rules
**The Float-Only Shadow Rule.** A surface casts a shadow only if it floats over the work and goes
away again, and takes it from one of the three tokens. Panels, tabs, rows and buttons never do.

## Shapes

Three corners, all small: 3px (`--r-control`) for controls, fields, tags and panes; 5px
(`--r-menu`) for menus, toasts, the palette, split panes and cards; 8px (`--r-window`) for the
floating panels and dialogs. Status dots (7px), colour swatches (18px; 21px for the mark swatches in Settings › Appearance) and the dot on a live
session's icon are circles. Borders are a 1px hairline everywhere, bar the pane drop zones, which
draw a 2px dashed line so they read over a live terminal. Dashed lines mean "empty
or droppable" (an empty pane, a drop zone). A 2px amber border means "picked" (a colour scheme
card).

### Named Rules
**The Nested Curve Rule.** A rounded thing inside another keeps the same curve: its radius is
the outer one less the room between them, written as `calc()` (a menu item is
`calc(var(--r-menu) - 3px)`, the kind glyph in the session editor `calc(var(--r-control) - 2px)`),
and never below `--r-control`: a row inside an 8px-radius, 8px-padded panel keeps 3px.

## Components

### Buttons
Quiet and exact: small, flat faces that brighten a step under the pointer.
- **Shape:** gently squared (3px), 26px tall, `3px 8px` padding, 5px between icon and text; the
  label is in the mono (`--short`); menu items (`.menu button`) and list rows (`button.item`)
  keep the system face.
- **Default:** a `raised` face with no visible border (a transparent 1px keeps sizes equal when a
  state adds one). Hover moves to `hover`. Disabled is 45% opacity; something merely stepped back
  (a lifted tab, the bar of a window without the keyboard, a done step's key, a tab's close glyph,
  the quality bar when off) is `--dim`, 0.5; 0.75 on light, where half of a grey falls under 3:1
  (the inactive window's title bar icons are 3.1:1 there).
- **Primary:** amber fill (`gold-fill`) with `gold-fill-ink` text; hover mixes in 14% white (on light, 8%
  black: a paler amber on white reads as fading). Disabled on light is a grey `raised` face at 45%, not
  a washed-out yellow. One per dialog, on its main action.
- **Danger:** the primary of a question whose yes ends or deletes something (close a running
  tab, delete a folder, replace everything from a backup, replace the MCP token): red
  fill with `error-ink` (`button.primary.danger`). Something that can be put back is not asked
  about at all: it happens, and a toast offers Undo.
- **On / selected:** amber wash, amber text, and a 55% amber border, for toggles and picked options
  (`--picked`, `--picked-line`). On light the text is ink, so the wash deepens to 42% amber and the
  border is solid amber; a flat tool button that's on (a layout, Broadcast) keeps that wash there
  (`--picked-flat`), where on dark its amber icon says it and it stays transparent.
- **Flat:** transparent at rest, `raised` on hover, text in `fg`. Tool buttons in bars are flat,
  icon-only (`3px 5px`), in `weak` (the title bar and the tab strip set that themselves), turning
  amber when on, and always carry a tooltip.
- **Link:** text that names another place is a borderless amber `button.link` (underline on
  hover; on light, ink and always underlined: the one place the light theme draws an element
  differently, since an ink link without it would read as text). A whole sentence that links is `.quiet`: body colour, dotted underline, amber on hover.
- **Focus:** a 1px amber outline drawn inside the edge (`outline-offset: -1px`); ink on light.

### Inputs / Fields
- **Style:** an inset well (`deep`) with a 1px `field-line` edge, 3px corners, 26px tall, `3px 8px`
  padding. The title bar's field draws the same edge. Selects use the app's own chevron, set in 8px from the edge.
- **Focus:** the border turns amber (ink on light); no glow.
- **Placeholder:** an example, in `weak` (never the engine's default grey, which fails contrast
  on the wells). Labels stay outside the field.
- **With an icon:** search fields put a 16px icon at 8px in and start text at
  `calc(8px + 16px + 5px)`.
- **Checks and progress:** native controls tinted with `accent-color: var(--gold-fill)`.
- **Radios:** drawn by `app.css`, the same on both themes (the engine's own put a thick black ring
  between a light accent's edge and its dot): a 16px well in `deep` with a 1px edge between `weak`
  and `field-line`; picked, a 5px `gold-fill` ring around a dot in the well's colour.
- **Sliders:** drawn by `app.css`, the same on both themes (the engine darkens a native slider's
  empty track to black under a light accent on a light page): a 5px `hover` groove filled in
  `gold-fill` up to the knob (`--f`, from `ui/lib/slider.ts`), and a 13px `gold-fill` knob with a
  2px white rim (with a 13% black hairline on light); disabled, the fill and the knob turn grey (`weak`), as a native slider's do.
- **Too long:** a value, placeholder or choice that doesn't fit ends in `…`; fields never widen
  their dialog. In a `.form` the labels take what the longest needs, from 120 to 233px, and end
  in `…` past that; the fields take the rest. Boxes of text don't resize.

### Menus
- **Style:** panel at `--float-alpha`, hairline, 5px corners, 3px padding, at least 190px wide, Menu shadow.
- **Items:** 26px, `3px 8px`, transparent; `raised` on hover. In a menu with icons, items
  without one are indented to line up with the others' text.
- **Groups:** `.sep` starts a group with a hairline 8px in from each side.
- **Destructive:** comes last, as `.danger`, and turns red on a 14% red wash when pointed at.
- **Saved-layouts menu:** one row per layout (icon, name, pane count) with a trash button at its
  end, then "Save the panes as a layout…" (dim with nothing on screen). Deleting is a row's own
  button, so the menu ends with its way to add: an accepted exception to "destructive item last".
- Keyboard-driven, kept on screen, and opening leftwards in right-to-left languages.
- Menus open instantly, with no arrival motion: they follow the pointer and the keyboard, and
  any delay there reads as lag.

### Navigation
- **Tab strip:** a 34px panel bar. Tabs are flat, split by hairlines, `weak` when idle. The
  active tab takes the content's `bg` and drops its bottom line so it joins its pane. A 2px
  connector sits on top of each tab, in its mark (the tab's own, else its session's or folder's)
  or else its kind hue: drawn dim (30%) on every tab, whether it is connected or idle, at 50% when
  the tab is visible in a split, and full on the active one. A tab whose connection is gone draws
  it in `warn`. The tab's icon is always in its kind hue, and the active tab's name is in
  `short-strong`, as the title bar's field names it (the mono's widths don't change with weight,
  so the strip doesn't shift). A drag shows a 2px amber insertion bar. Tabs keep their width;
  more than fit scroll sideways (the wheel scrolls it), with no scrollbar. At an end with tabs past
  it, a flat chevron (`fg`, amber when pointed at) lies over the strip: the tabs are hidden under
  its 26px and fade in over the next 21px, and the chevron's button covers both, since what shows
  of a tab there is mostly its X and a click meant to go back would close it. It glides the strip
  a page that way. A page that would carry the tab in use out of view stops short, with that tab
  at the far side clear of the chevron and fade there; the next click carries on past it. A tab
  scrolled into view (a click, Ctrl+Tab, All tabs) glides too, so tabs of one name still show
  which way the strip went, and stops clear of the chevron and the fade
  (`scroll-padding-inline`). Reduced motion makes both jump.
- **All tabs:** while tabs don't fit, the tab-strip tools start with a flat chevron ("All tabs
  (16)"), amber while open. It drops a menu (`.tasks-list.all-tabs`) hung from its end (right to
  left too), one row per
  tab in strip order: its icon in its kind hue and its name, the tab in use on the `picked` wash and
  holding the keyboard. It is as tall as the window allows, then scrolls; a click shows that tab.
- **Session rack:** 21px rows, folders with a rotating caret (0.12s) and a hairline guide under
  each level. Leaves indent past the caret so icons in one level line up. A session with an open
  tab gets a 7px green dot on its icon's corner, hollow once the connection is gone. Chosen rows
  take the amber wash; the keyboard's row (`.current`) is `raised` with a 2px amber inset edge on
  its leading side. A marked session's or folder's name is in its mark.
- **Sidebar foot:** the status pill (an inset well, label size in `weak`, fading in over 0.16s
  when a message arrives) on a row of its own, then, at the trailing end of the foot's row,
  Settings, Lock (when a lock is set), Terminal Co-op (in a ring, or signed in with Pro or its
  button turned on) and the account, in that order. The account is a flat icon button; signed
  in, it shows an 18px round avatar: the account's picture when it has one, else the first letter
  of the name on a solid amber disc. It is ringed 2px in `error` when
  sync fails or the quota is full and in `warn` when it is nearly full. Its menu opens upwards,
  led by a `.head` block (name, address, last sync, the quota bar) under a hairline.
- **Focus ring:** one 2px amber ring around the focused pane, outside it in the gap (5px corners
  in a split), drawn only when the layout has more than one pane. When the focus moves it glides to the next pane (0.2s, `--ease-out`; Static in
  Settings › Appearance makes it jump), so you see which way it went. While a pane edge is
  dragged it follows the edge with no delay.
- **Settings pages:** a 183px column. The current page is `raised` with a 2px amber edge on its
  leading side. Its search replaces the pages with hits; while it's open the page follows the
  best hit and lights that setting (`.lit`: its label and field in the amber wash, ringed 3px in
  it), and the hit is lit in the list too. Enter closes the search and puts the keyboard on the
  setting's control, whose light then fades over 2.6s (`.found`). Pages, in this order: General,
  Appearance, Terminal, Shortcuts, Defaults, Workspaces, Saved logins, SSH keys, Import and
  export, Account and sync, Security (encryption, recovery passphrase, the lock), MCP
  (sub-sections "AI assistants (MCP)" and "Command line", whose commands and configuration sit in
  `.snippet` wells: `deep`, `8px 13px`, 3px corners, in the mono), Troubleshoot and About.
- **Status bar:** a 21px panel strip under a tab's view, label size in the mono, in `weak`, items
  13px apart.
  It leads with where the keyboard goes: the session's icon (14px) and its address (`user@host:port`
  for SSH), then the session's folder with the folder's
  tags as amber chips (`Production/Web · prod`), so a production box says so under its pane. Then a
  state dot with the uptime ("up 3m"; "connecting…" in amber, "stopped" in red) and, on an SSH
  session, the cipher (and "+ zlib" when compressed) and the round-trip time, which turns `warn`
  past 200 ms. At the end come the amber broadcast marker and the terminal's columns × rows. With
  no tab it says "No tab".
- **Tab-strip tools** come as the tabs do, at the strip's end: All tabs while tabs don't fit,
  Getting started's progress while it's unfinished, Tasks (a layers glyph and a count)
  while a tab runs in the background or in a window of its own; File Browser and Snippets once a
  terminal-type tab is open (File Browser stays dim until a connected SSH, RDP or VNC tab is on
  screen); Broadcast with two or more tabs. Saved layouts and the layout segment live in the title
  bar, not here.
- **Tasks menu:** a menu (`.tasks-list`) hung from the Tasks button's end, one row per background
  or detached tab: its icon in its kind hue, its name, and "running", "done" or "in its own window"
  in `weak`. A click brings the tab back into the strip; a flat X closes it.
- **File-panel chips:** when more tabs have files than there are file panels (three at most), a row
  of chips under the panels, one per tab: its icon (13px) and name, `weak` and label size, with the
  tabs shown lit (`raised`, `fg`). A click shows that tab's files.

### Dialogs and the Command Palette
- **Dialog:** a panel at `--float-alpha` with 8px corners, a hairline header (`8px 8px 8px 13px`), and a 13px
  body with 8px between blocks; Dialog shadow over a 40% scrim. It holds the keyboard: Tab stays
  inside and Escape closes the top one. Widths: 377 / 610 / 987. A dialog whose body can run
  long puts its buttons in `foot`, a fixed footer under a hairline (`8px 13px`), so Save never
  scrolls out of reach. A dialog whose fields come and go while it's used (the session editor)
  is `top`: it hangs at 12vh, where the palette does, and grows downwards instead of
  re-centring, so what's under the pointer stays put. Its keyboard lands on the first field a
  person can see, never one in a closed `<details>` or a shortcut that fills the others in
  (`data-not-first`, as "Copy the server from"), else on the dialog itself. Dialogs and menus
  draw over everything wherever they're opened from (they move to the end of the page while open).
  A dialog opened over another is solid (98%, whatever the Opacity) and the ones under it step
  back behind a 3px blur: the last opened has the keyboard.
- **Sign in:** a 377 dialog. Sync needs an account and nothing else does; the browser opens on
  the sign-in page (Google, GitHub or Discord) and the dialog shows the code to approve there,
  centred, in the mono at `--t-xxl`, with a flat copy button beside it and an amber spinner while
  it waits. Cancel is flat; after a failure, Try again is the primary.
- **Lock screen:** not a dialog but a whole opaque window (`rgb(var(--window-rgb))`, over
  everything but the title bar, dialogs included) that fades in over 0.16s. A centred 377px column, 13px between
  parts: a 34px lock icon in amber, a 600 title at `--t-xl`, a password or PIN well with the
  primary Unlock beside it (Password / PIN switch as flat buttons, the picked one amber), then
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
  A search well over 26px rows; the picked row takes the amber wash, with its shortcut in a `kbd`
  at the end of the row. Empty, it lists the open tabs, then the saved sessions, then the actions,
  the saved layouts, the snippets and the tunnels to start or stop: a machine before a verb. Under
  the search well, Sessions, Snippets and Hosts filter buttons, each with its count; Tab steps
  through them without leaving the field, and picking one again gives back everything. A typed word has to sit in one stretch of a row's text (an abbreviation
  like `prd` for `prod`), never letters scattered across it.

### Tags, Keys and Toasts
- **Tag:** amber text on the amber wash, 3px corners, `0 5px`, label size, at most 80px wide.
- **Key cap (`kbd`):** an inset well with a hairline and a 2px bottom border so it reads as a
  key, in the mono at label size.
- **Toast:** panel, hairline, 5px corners, `5px 13px`, Menu shadow, 34px above the bottom edge.
  It arrives with `float-in` and leaves faster than it came, fading out in 0.1s. One with an
  action (Undo after a delete, Reopen after Close other tabs) carries an amber link button,
  takes the pointer, and stays 8s instead of 2.4s.
- **Tooltip (`.tooltip`, `ui/lib/tooltip.ts`):** a tool button's title in the app's own panel at
  `--float-alpha`, hairline, 5px corners, `3px 8px`, label size on golden lines, Menu shadow, at most 377px
  wide; its shortcut follows in a `kbd` in `weak`. It hangs under what it explains (above when
  there is no room) and takes no pointer.
- **Quota bar (`Quota.svelte`):** sync storage used, in the account menu and Settings › Account and sync. A
  3px track in `line` with 2px corners, filled in amber; the fill turns `warn` when nearly full
  and `error` when full, and a label-size line under it says how much. Not a native `progress`,
  so its 3px track and 2px corners are the system's; a native `progress` (file transfers) takes
  the accent through `accent-color` but keeps the engine's shape.
- **Quality bar (`QualityBar.svelte`):** the RDP picture against speed, in the session editor and
  Settings. A native range with the step names under it, Quality at the start to Speed at the
  end, the picked name in amber at 600, and a `weak` line saying what it means. At most 420px
  wide, 55% opacity when off. It applies on the next connect.
- **Hint (`Hint.svelte`):** a dismissible tip in a `.form` grid, under the field it explains: a
  14px info icon and `weak` text, ending in a quiet underlined "Don't remind me again"
  (`settings.quiet_hints`). Settings › General brings the dismissed tips back.

### Kind Glyph (signature)
A 21px tile with 3px corners, the kind's hue as the icon colour on 15% of the same hue (`.glyph`
in `ui/app.css`). It sits beside each way to start on the start screen and makes a connection
type recognisable before you read its name. The session editor's Type picker is a row of them:
one button per kind, glyph and name, the picked one in the amber wash. The type comes first in
the editor because it decides every field below it; changing it keeps the host typed so far.

### Broadcast Outline (signature)
Every terminal that receives broadcast keystrokes gets a 2px inset outline in `--broadcast`
(the accent on dark, the warning orange on light, as the status bar's marker) or a chosen colour
(`--oc`), drawn inside the pane at 3px corners. Its style is the user's (steady, breathe, fade,
ring, march, rainbow, off). While broadcast is on, the focus ring around the pane you type in
turns to a 2px dashed ring in the text colour (`fg` on dark, `weak` on light, where black dashes
looked dated), so it never reads as one band with an outline in the same colour. It stays where
the ring is: stepped further out, the window's edge cut it off at the sides and the bottom. On
light a pale outline colour is darkened to at most 0.7 Oklab lightness, keeping its hue, so it
shows on a light terminal without becoming a heavy frame. Lightness clamps on a user's colour
(`.tinted`, the outline) go through `oklab(from …)`, not `oklch(from …)`: Chromium drops a greyish
colour's hue in the latter and draws it pink. All animated styles stop under reduced motion, and
the preview in Settings › Appearance holds still while it is scrolled out of view.

### Co-op Ring and Pointers
A tab shared in a Terminal Co-op ring wears a small ring (8px, 2px stroke in `ok`) after its name;
a tab shared with you is a `k-coop` tab. The ring's dialog is a 377px `Modal`: the passcode in
`--t-xxl` mono, people asking to join as 21px round pictures (the first letter when there is none),
and "End the ring" last, in `error`. Up to three people are drawn as a ring (`CoopRing.svelte`): a
flat ellipse with the owner's 34px picture in its middle, where everyone's connection meets, the
second person's at its left end and the third's at its right, and clear of it (over it for the
owner, under it for the others) their name (`--t`) and what they send and receive a second: two
figures in `--t-s` mono, an arrow, the number in `fg` and the unit in `weak` (b, Kb, Mb, Gb, Tb),
each in a room of fixed width so a changing figure moves nothing beside it; they are read over the
last 3 seconds, so they move calmly. Alone, it is a
dashed `line`; with two people connected an amber line comes up over it in 0.3s (as a running
tunnel's tube is amber) and amber packets go round: one while it is quiet, two from a kilobyte a
second, four from a megabyte, a lap in 20 to 4 seconds. A packet fades in and out where it is and
the speed eases to its new value, so nothing jumps; under reduced motion there are no packets and
the lit ring alone says people are connected. A seat fades in (0.2s), and the third person
glides to the left end when the second leaves (0.3s, `--ease-out`). Someone let in but not connected is dimmed.
For the owner the others' pictures are buttons: a click or a right-click opens a menu with Rename…
and, last and in `error`, Kick; the seat whose menu is open wears the focus outline. A ring of more than three has no
room for all that: it is a plain list of pictures and names, with Rename… and Kick as quiet
buttons on each row for the owner, and no traffic. Over a shared or remote pane, each other person's pointer is an 8px ring in
a mark colour (`COLORS`, held per person) with their 13px picture beside it, the name on hover. It
moves in 80ms and stands still under reduced motion. In a terminal tab of the ring, what each
other person has selected is drawn row by row in their ring colour at 30%, under the pointers
(`CoopSelections.svelte`); everyone selects on their own and sees all of it.

### Backgrounds
A picture or a GLSL shader behind the terminals, the sidebar or the tab bar (each its own),
drawn under the place's own colour at the strength set (a range of 5–95% in steps of 5,
`Backdrop.svelte`). A picture
fills its place by default, cut to fit; it can also stretch, fit whole, tile at its size, or sit
centred at its size (`FITS`). A shader always fills. The place isolates its stacking, so the
backdrop stays under everything in it.

### Session Icons
Interface icons are Boxicons SVGs through `Icon.svelte`: 16–18px in controls, 13–15 in tabs and
the status bar, 21 in the title bar, 34 on the lock. They stand still, except a loader that spins
(in amber) or a live dot that pulses while something is in progress. Session and folder icons
(`SessionIcon.svelte`) are a Boxicon, or the user's own picture, or a Dashboard Icon picked from
the catalogue (`dashicons.ts`). A Boxicon has small moving parts (blink, flow, spin, bob…) that
play only while the pointer is over that session's row or tab, never on the interface's own
buttons, and never under reduced motion.

## Do's and Don'ts

### Do:
- **Do** use only Fibonacci pixels for spacing and radii (1 2 3 5 8 13 21 34), with padding pairs
  on the scale, horizontal at least vertical (`3px 8px`, `5px 8px`, `8px 13px`).
- **Do** set every text size from the type tokens (`--t-s --t --t-l --t-xl --t-xxl`).
- **Do** size things with `--row` (21px), `--control` (26px) and `--bar` (34px), and write any
  derived value as `calc(…)`.
- **Do** keep amber for focus, selection, toggles that are on, links, drop targets and the one
  primary action. Read it as `var(--gold)`, since the user can change it.
- **Do** colour a session by its kind hue (icon, tab connector, glyph tile) through `--kind`.
- **Do** paint chrome as `rgba(var(--…-rgb), var(--chrome-alpha))` so the Transparent effect shows through.
- **Do** end a menu with its destructive item (`.danger`) and start groups with `.sep`.
- **Do** offer Undo instead of a question when the thing can be put back, and ask (with a
  danger button) only when it can't: a running connection, a folder of sessions, a restore.
- **Do** focus the safe answer when yes can't be undone or is a security call (Reject on a new
  host key, Cancel on a delete or restore, Keep editing on discard), so Enter never does the
  irreversible thing; an ordinary yes (Close tab, which Reopen can undo) keeps the focus.
- **Do** make tool buttons icon-only with a tooltip.
- **Do** keep motion to state and continuity, in three durations: `--quick` (0.1s) for an exit,
  `--move` (0.16s) for an arrival or a change of state, `--glide` (0.2s) for a thing moving to
  another place; arrivals on `--ease-out`, a new floating layer through `float-in`, exits faster
  than entrances. The longer ones are named: the welcome leaving in 0.12s, the Co-op ring's 0.3s
  line and seat glide and its 0.5s packets, the 2.6s fade of a found setting. When one view replaces another (the
  start screen giving way to the shell typed into it), the old one leaves first and the new one
  arrives after it. CSS motion stops under `prefers-reduced-motion` through `ui/app.css`; a
  Svelte transition must check it itself.
- **Do** use logical properties (`inset-inline-*`, `margin-inline-*`) so right-to-left mirrors.
- **Do** set short text (titles, buttons, tags, status lines) in `var(--short)` and anything read
  as text or down a list (menus, rows, the Settings pages, the kind picker) in `var(--ui)`.
- **Do** show the logo through `Logo.svelte`, so its tiles and wordmark follow the theme through
  `--mark`.

### Don't:
- **Don't** use off-scale spacing (4px, 6px, 10px, 12px, 16px) in padding, margin, gap, inset,
  offsets or radii, or a raw `font-size` in px; the scale test fails on those. Sizes (width,
  height, line-height) aren't tested: keep them on the scale or name the exception.
- **Don't** put a shadow on a resting panel, tab, row or button or write a new shadow value for a floating layer: use `--shadow-float`, `--shadow-menu` or `--shadow-dialog`.
- **Don't** use amber for headings, icons, illustration or large fills other than the primary
  button.
- **Don't** use a kind hue to show state, or a status colour to mean a kind of connection.
- **Don't** let a user's colour recolour a session's icon: marks go on names (`.tinted`) and tab
  lines, and accent presets stay off the kind hues and the error red.
- **Don't** hard-code the accent's hex (but for the Amber (default) swatch in Settings ›
  Appearance), or the chrome as an opaque hex.
- **Don't** animate the interface's own icons, bar a spinner or a pulse that says something is
  waiting. Only session and folder icons move for their own sake, and only on hover.
- **Don't** add big padding or wide gaps. The densest comfortable layout is the house style.
- **Don't** set a list, a menu or a five-to-a-row grid in the mono: the names stop fitting and it
  no longer reads down.
- **Don't** use the logo's amber `#FFC20E` or `--mark` outside the logo, or recolour, stretch,
  rotate or add effects to the logo.

## Token audit (2026-11-08)

Two reviews read this file against `ui/app.css`, every component's style block and
`scale.test.ts`. What they found is either fixed or listed here, so the next change knows the
state of the system rather than rediscovering it.

**Fixed on 2026-11-08.** The light theme's `weak` (`#55555c`), `ok` (`#155c3d`), `error`
(`#a4242d`) and `warn` (`#745000`) were darkened so each holds 4.5:1 on `hover` too (they held it
on `panel` only); `--scrim` and `--scrim-soft` replaced two literal ground colours; `fade-in` runs
on `--ease-out` everywhere; raw radii became `--r-control` / `--r-menu` (`kbd`, `.tag`, the
scrollbar thumb, the terminal's thumb, Flow's spark, a tunnel's tag, the appearance preview);
`input[type="color"]`, the title bar's mark and the guide's rings read `--control` / `--row`; the
toasts sit `--bar` up; the tab menu's tint swatches are the 18px every swatch is; list rows under
the pointer take `raised` everywhere (Credentials, Snippets); the hint's "Don't remind me again"
is the one `button.link.quiet`; the rules above now say what the test enforces, what padding pairs
are, where the nested curve stops, which motion runs longer, and name `--solid`, `--tint-lo`,
`--tint-hi`, the Ember/`error` collision and the RDP bars.

**Also fixed, later the same day.** `--icon` (18px) and `--icon-s` (16px) for the icon offsets,
which were `calc()`s over literals (the palette's search icon was drawn at 18 while its offset
assumed 16: it is 16 now); every `gap: 5px` reads `--gap`; `--quick`, `--move` and `--glide`
replaced nineteen duration literals (0.15s joined 0.16s); `--dim` replaced the 0.5, 0.55 and 0.6
step-backs; the collapsed rail is derived. The file panel's transfer progress now sends the
transfers alone (`files-progress`), not the whole listing ten times a second.

**Open, in the order worth doing.**
1. Widths off the scale: the file panel 340px, the Settings pages 183px, menus 190px, the tasks
   menu 260px, the rename field 160px, tab titles 180px, the chord field 150px, the catalogue
   220px, the account menu 280px, the tunnels status 320px, previews 144px.
2. The palette's keyboard row is `.item.on` (picked) where the rack and Settings use `.current`;
   its recipe should follow theirs.
3. A selected swatch is a `fg` border in `app.css` and a 2px `gold-soft` ring in
   `Appearance.svelte`; the custom-colour swatch's gradient hard-codes the dark theme's kind hues.
4. The colour input is `--control` × `--row` in `app.css` and 30px wide in a highlight rule.
5. Two recipes for a hairline under the pointer (`color-mix(var(--fg) 25%, var(--line))` on fields,
   `var(--weak)` on scheme cards); the file list's zebra stripe is a literal
   `rgba(127, 127, 127, 0.06)`; `CoopRing` sets `line-height: 1.3` where dense text is 1.4; the
   guide re-declares `h2`.
6. `scale.test.ts` accepts any `var(--t…)` as a font size (`--titlebar` would pass), only matches
    `font-size` at a line's start, and skips a whole declaration when `calc(` appears anywhere in
    it; the select chevrons bake `weak`'s hex into their data URIs for each theme.
