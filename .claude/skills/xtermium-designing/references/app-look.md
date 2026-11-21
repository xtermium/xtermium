# App look, condensed

Source: `DESIGN.md` (the full record), `CLAUDE.md` "UI conventions", and the live tokens in `ui/app.css`. Read the full `DESIGN.md` before adding a component.

## Colour tokens (dark theme)

The website's near-black ground, surfaces, hairlines, ink and muted, neutral with no blue in them. The accent token keeps its old name, `--gold`.

| Role | Token | Hex |
|---|---|---|
| The window, showing between panels | `window` | `#0b0b0d` (the work area's own colour) |
| Wells: fields, `kbd`, segmented controls, snippets | `deep` | `#08080a` |
| Content under the tabs, the active tab, the sidebar's panel | `bg` | `#0b0b0d` |
| Tab strip, file panel, status bar, every floating layer | `panel` | `#111114` |
| Button faces, menu-item hover, current Settings page | `raised` | `#1d1d22` |
| Button under the pointer | `hover` | `#26262c` |
| Every 1px border | `line` | `#2c2c33` |
| Primary text | `fg` | `#e6e6e1` |
| Secondary text, labels, idle tabs and tools | `weak` | `#9d9da6` |
| The one accent, phosphor amber (user-changeable; never hard-code) | `gold` | `#ffb000` |
| Text on an amber fill | `gold-ink` | `#050506` |
| Picked rows, search hits, tags, drop zones | `gold-soft` | `rgba(255, 176, 0, 0.14)` |
| The logo's three large tiles and wordmark, nothing else | `mark` | `#f2f2f2` |
| Connected | `ok` | `#4fc08d` |
| Waiting, slow, nearly full | `warn` | `#e0b04a` |
| Disconnected, failures, danger | `error` (text on it: `error-ink` `#200a0a`) | `#ec6a6a` |

Kind hues (`--k-*`), read through a local `--kind`:

| Kind | Name | Hex |
|---|---|---|
| SSH (also forward tunnels `--fwd`) | Cobalt | `#5c8df0` |
| Local shell (also reverse tunnels `--back`) | Teal | `#33b8a6` |
| Serial | Chartreuse | `#b5c14a` |
| RDP | Violet | `#9c70e6` |
| Website | Rose | `#dd6b9c` |
| Telnet | Copper | `#d98a4a` |
| WinRM | Cyan | `#4aa8d9` |
| VNC | Orchid | `#c46fd1` |
| BMC console | Bare Titanium | `#8fa3b8` |
| AI agent | Ember (same value as `error`) | `#ec6a6a` |

- Accent presets: Brushed gold `#d8a84e` (the accent before the brand's), then the metals Platinum `#c3ccd8`, Bronze `#a99a78`, Rose gold `#e2a395`, each with a light-theme value (`ACCENTS` in `ui/lib/api.ts`); presets stay off the kind hues and the error red.
- User marks (`COLORS` in `api.ts`: nine swatches, no amber) tint names (`.tinted`) and tab connector lines, never a session's icon.
- The logo's own amber `#FFC20E` and `--mark` live only in the logo (`ui/lib/Logo.svelte`); the brand amber is never UI state.
- The light theme (`:root[data-theme="light"]`) redefines every token with the same roles: neutral greys (window `#d8d8db`, panel `#e8e8ea`, line `#c9c9ce`), text in charcoal `#141414`, the accent `#754b00`, `--mark` `#141414`; values also in `.impeccable/design.json` (`colorMeta.*.light`).

## Type

| Role | Size | Use |
|---|---|---|
| Prompt | mono 400, `--t` 13px, `weak` | the start screen's `~/xtermium $` with an amber `$` and a blinking amber block cursor (decorative, hidden from assistive tech) |
| Headline | mono 600, `--t-xl` 16.5px, -0.025em | the lead sentence under the logo |
| Title | mono 600, `--t-l` 14.5px, -0.01em | dialog and page titles (`h2`); section heads (`h3`) are mono 600 at body size |
| Short | mono 400, `--t` 13px, 1.4 | buttons |
| Body | 400, `--t` 13px, 1.4, system face | fields, field labels, rows, menus |
| Prose | 400, 13px, line height φ (21px), 62–70ch | paragraphs and text areas |
| Label | 400, `--t-s` 11.5px | counts (`tabular-nums`), hints in the system face; the status bar, tags and `kbd` in the mono |
| Mono | 0.92em, JetBrains Mono | `code`, `kbd`, `pre`, tunnel addresses; always left to right |
| `--t-xxl` 18.5px | | only the RDP connect/close status and the sign-in code |

Short face (`--short`, `--mono`): JetBrains Mono Variable, the brand's face (the logo's wordmark is JetBrains Mono Bold), from `@fontsource-variable/jetbrains-mono`. Long face (`--ui`): Segoe UI Variable Text, Segoe UI, system-ui, -apple-system, Ubuntu. Short-in-Mono: titles, buttons, tags, keys, status lines and code in the mono; body text, prose, field labels, menus (`.menu button`), list rows (`button.item`), the Settings page list (`.pages button`) and the kind picker (`.kinds button`, five to a row) in the system face. For `lang` ar, hi, ja, zh `--short` falls back to `--ui` (JetBrains Mono has no glyphs for those scripts). The logo is drawn, not set: `min(377px, 100%)` wide on the start screen, 233px in About. The terminal's own font is the user's setting.

## Layout and sizes

- The window is padded by `--panel-gap` (8px, also between its main sections and under the title bar's field) and holds the activity bar (26px, as wide as its buttons and the mark above, on the window, under the mark: New session as an amber primary square, Sessions (layers glyph), local shell, tunnels, layouts; account, Co-op, Settings at its foot; a new session saved while the sidebar is hidden puts an amber count on Sessions, pulsing as it grows, and its row pulses amber twice once the sidebar shows; Lock sits in the title bar 8px before the palette field, flat, hung there so the field stays centred), then the floating panels: the session rack (resizable, shown and hidden by Sessions), the centre (34px tab strip over the panes) and the file panel (340px).
- Split panes are each their own panel (5px corners, hairline) with the window's dark between them; pane sizers are 6px hit areas, amber-tinted on hover.
- Forms: a two-column grid, labels in `weak` (120 to 233px, ending in `…` past that), gaps `8px 13px` (`5px 13px` in Settings).
- No breakpoints: things shrink, ellipsize and scroll. Logical properties for right to left.
- The start screen sits on the golden section, a column at most 610px wide: the prompt, the horizontal logo (arriving with `float-in`), the lead, the ways to start.

## Named rules

| Rule | Says |
|---|---|
| One Accent | amber says "you are here" or "you picked this"; exceptions: the primary button's fill and folder tags |
| Logo Colours | the logo's amber `#FFC20E` and `--mark` live only in the logo, never as UI state |
| Short-in-Mono | short text in `var(--short)`; long text and lists (menus, rows, the Settings pages, the kind picker) in `var(--ui)` |
| Cable Colour | a kind hue identifies a kind of connection and nothing else; state is `ok` / `warn` / `error` |
| See-Through Chrome | chrome is `rgba(var(--…-rgb), var(--chrome-alpha))`, never an opaque hex |
| Five Sizes | every `font-size` is a type token, `inherit`, an `em` or a `clamp()` (the test fails on raw px) |
| Golden Line | prose and text areas use `line-height: var(--phi)`; dense UI text stays 1.4 |
| Fibonacci | padding, margin, gap, inset and radius are Fibonacci pixels (the test fails on 4, 6, 10, 12, 16) |
| Derived Value | a value worked out from others is a `calc(…)` (`calc(8px + 16px + 5px)`, `calc(var(--r-menu) - 3px)`) |
| Float-Only Shadow | only layers that float over the work and go away cast a shadow, from the three tokens |
| Nested Curve | an inner radius is the outer one less the room between them, as `calc()` |

## Shadows and motion

| Token | Value | For |
|---|---|---|
| `--shadow-float` | `0 4px 14px rgba(0, 0, 0, 0.3)` | find and reconnect bars, the detached-window hint |
| `--shadow-menu` | `0 6px 20px rgba(0, 0, 0, 0.36)` | context menus, dropdowns, toasts, tooltips |
| `--shadow-dialog` | `0 8px 28px rgba(0, 0, 0, 0.4)` | dialogs and the command palette |
| scrim | `rgba(5, 5, 6, 0.4)` behind a dialog, `0.32` behind the palette | |

- Arrival: `float-in` (a fade from 5px short, 0.16s, `--ease-out`); menus open instantly.
- Toasts: 2.4s, or 8s with an action (Undo, Reopen); leave in 0.1s.
- The focus ring glides between panes over 0.2s (Static in Settings › Appearance makes it jump).
- Settings search lights the setting (`.lit`), then fades it over 2.6s (`.found`).

## Components to know by name

- **Buttons**: 26px, `3px 8px`, 3px corners; default `raised`; primary amber with `gold-ink`; danger red with `error-ink` (`button.primary.danger`); on/selected amber wash with amber text and a 55% amber border; flat icon-only tool buttons (`3px 5px`) in `weak` with a tooltip; `button.link` amber, `.quiet` a dotted-underline sentence; focus a 1px amber outline inside the edge.
- **Fields**: `deep` wells, hairline, 26px, `3px 8px`; focus turns the border amber, no glow; placeholders are examples in `weak`; native checks, radios, ranges tinted with `accent-color: var(--gold)`.
- **Menus**: panel at `--float-alpha` (98%, solid with Transparent on), hairline, 5px corners, 3px padding, at least 190px; 26px items; `.sep` groups; `.danger` last, red on a 14% red wash when pointed at.
- **Tab strip**: 34px; flat tabs split by hairlines; the active tab takes `bg` and drops its bottom line; a 2px connector on top in the tab's mark or kind hue (hidden idle, 50% when visible in a split, full when active); a 2px amber insertion bar while dragging.
- **Session rack**: 21px rows, carets rotating 0.12s, a hairline guide per level, a 7px green dot on an open session's icon (hollow once the connection is gone).
- **Status bar**: 21px, label size in the mono, `user@host` first, the folder with its tags as amber chips (`Production/Web · prod`), a state dot, the amber broadcast marker last.
- **Dialogs**: panel at `--float-alpha` (98%, solid with Transparent on), 8px corners, hairline header `8px 8px 8px 13px`, 13px body; `foot` for long bodies; `top` dialogs hang at 12vh (the session editor).
- **Palette**: 610px wide, 12vh from the top, at most 1/φ of the window tall; empty, it lists open tabs, then saved sessions, then actions.
- **Lock screen**: an opaque window over everything, fading in 0.16s; a 377px column with a 34px amber lock icon, password or PIN with Unlock, then passkey and Windows Hello, then "Your sessions keep running." in `weak`.
- **Kind glyph**: a 21px tile, 3px corners, the hue on 15% of itself; the session editor's type picker is a row of them.
- **Broadcast outline**: a 2px inset outline in the user's colour and style (steady, breathe, fade, ring, march, rainbow, off); the focus ring turns dashed in `fg` and steps 2px out.
- **Quota bar**: a 3px track in `line`, amber fill, `warn` when nearly full, `error` when full (not a native `progress`).
- **Session icons**: Boxicons 16–18px; only session and folder icons animate (blink, flow, spin, bob), on hover of their row or tab, never under reduced motion.
