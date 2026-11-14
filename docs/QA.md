# Not yet tested

What has shipped (on `main`, in the nightly or a release) but hasn't been seen working on
real hardware or a real setup yet. Automated tests pass for all of it; what's missing is a
person, a second account or a system we don't have at hand. If you can check an item, say how
it went in an issue or a pull request, and remove the item once it has held up.

## Terminal Co-op

Needs two (or three) Pro accounts on separate machines, joined in one ring through the relay.

- Shared selections: everyone selects on their own, and each selection is drawn in its
  person's ring colour in every window.
- Copy and paste between members (sizes up to the limits on the wiki's Usage page), and a
  paste from an owner on an older version.
- The owner renames a ring member (needs the sync server with the rename, live since
  2026-11-07).
- A removed member rejoining.
- Remote desktop tabs shared in a ring: guests can't copy, paste or drop files.
- Ring colours: the owner amber and each guest their own colour, the same on every member's
  computer, round the pictures in the Co-op dialog and on pointers and selections (checked
  here only with a faked ring event in one window).
- Sharing an AI agent tab, a BMC console (its screen as pictures) and a website, and a guest
  with control driving the website (DevTools input, Windows) or the console: the owner's side
  was exercised with faked ring state in one window; a real ring hasn't driven either yet.
- Click rings and the moving pointer's name in every member's view, and the owner's pointer
  and clicks read from inside a shared page.
- A shared terminal drawn at each viewer's own font and zoom while the owner resizes or zooms.
- The picture quality setting's three levels as a viewer sees them.
- A pop-up or new-tab link of a shared website shared with the ring by itself, clicked by the
  owner and by a guest with control (checked here with a faked ring and a faked pop-up event).
- The ring ending after the owner's idle time (Settings › Account and sync › End the ring when
  away): only the idle clock and the setting were tested, not a ring left alone for an hour.

## BMC consoles

Checked on 2026-11-09 against real BMCs (read only): iLO 4 (autologin, console over the page,
video), a Gigabyte MegaRAC SP-X and ASUS ASMB10 and ASMB11 (sign-in, H5Viewer launched in the
tab, scaled to fill it), a Supermicro X11 (autologin, console), and pasting into a console.
Still to see:

- Autologin on the ASUS boards with their own passwords (they were signed in by hand; the
  form filled and submitted, and the BMC refused the other board's password, as expected).
- The mouse through the MegaRAC cursor layer (automation and Co-op desktop control).
- iDRAC 7 and 8 after the shared changes (notes kept on the top window, key target).
- The Java hints: an iLO with firmware older than 2.70, a MegaRAC with the H5Viewer disabled.

## macOS

The macOS build ships unsigned, and nobody has run these on a Mac yet.

- The fix for the SSH key picker hanging ([issue #40](https://github.com/xtermium/xtermium/issues/40)): native dialogs now open with the calling window as their parent (PR #39). Not confirmed on a Mac.
- The app's own title bar: the overlay style with its three buttons, dragging.
- The per-system shortcuts (⌘ in place of Ctrl), as the wiki's shortcut table lists them.
- Opening the unsigned `.dmg` past Gatekeeper, as the Installation page describes it.
- Developer ID signing and notarization (scripts/apple-env.sh): no build has run with the
  Apple secrets yet.
- The Co-op idle clock (`CGEventSourceSecondsSinceLastEventType`): compiled only by the
  release and nightly builds.
- Getting started away from the start screen: the pill at the tab bar's end, its popover
  under it (lined up with the pill's end, the keyboard on Try it, Escape back to the
  terminal), the palette's Getting started and the Done toast's Show.
- More tabs than fit: the trackpad's sideways scroll and the mouse wheel over the tab strip, the
  arrows over its faded ends and the All tabs list.

## Linux

- The app's own title bar (no system decorations): dragging, resizing from the edges,
  maximising, and the polish pass's sizes; the window menu (Minimize, Maximize, Close) a
  right-click on the bar's free space opens.
- The clipboard on X11 and Wayland, in terminals and in Co-op.
- Getting started away from the start screen, as under macOS: the pill, its popover and where
  the keyboard goes, on X11 and Wayland.
- More tabs than fit, as under macOS: the strip's fades (CSS masks on WebKitGTK), its arrows and
  the All tabs list.

## Right to left

- The Arabic locale with the new title bar, the sidebar row on the tab line and the
  Getting started guide.
- More tabs than fit in Arabic: the strip scrolls from the right, each arrow points and goes the
  way its end leads, and the All tabs list opens on screen.

## Windows

- Unlocking with Windows Hello (needs a camera or a fingerprint reader).
- The All tabs list over a website or BMC console tab: the view steps aside while it's open.

## Remote desktops

- RDP to xrdp: resizing the tab (the resize that keeps the session), on a real xrdp host. The
  picture and the saved-password login were seen right against the
  `scottyhardy/docker-remote-desktop` container on 2026-11-14 (helper on the individual IronRDP
  crates, no vendored patches); that container never answered a resize.
- The shared clipboard on macOS and Linux (text and pictures both ways, files to the desktop),
  and files from the desktop pasted in Explorer on Windows with the current helper.
- The Windows shortcuts on the desktop (Win+E, Alt+Tab, Alt+F4) with a key remapper running,
  and in a tab moved to a window of its own.
- A shared RDP desktop in a Terminal Co-op ring: the guest's picture and pointer, and what a
  guest with control can paste (the owner's clipboard once it was offered to the desktop).
- The RDP helper's macOS and Linux builds (compiled first by the nightly).
- Broadcast's ticks with a remote desktop or a BMC console among the tabs: an unticked one gets
  no keys and leaves the tiles (seen with local shells only, 2026-11-14).

## Tabs and workspaces

- "Only this workspace's tabs" with a website or BMC console tab open in the workspace left
  behind: its native view must go out of sight with the tab and come back with it (seen with
  local shells only, 2026-11-14).
- Ids after a sync between two computers that each gave their sessions ids before syncing: the
  pulled store's ids should replace this computer's, once, and stay (unit-tested within one
  store only: clone, workspace sync, split and join).
- An AI agent session started from an npm folder that is on the PATH in quotes, on a computer
  where that failed with "not a valid Win32 application" (the lookup is unit-tested; the
  report came from someone else's computer).
- The trust prompt for an RDP server's key and a BMC's certificate: Enter trusts, and the
  fingerprint is set in the mono (seen with an SSH host key only).

## Automation

- `run … --until` and `--wait` in an SSH tab right after `clear` (checked in a local cmd.exe
  tab on Windows).
