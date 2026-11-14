<script lang="ts">
  // An RDP tab: the helper's changed areas are painted into a canvas the size of the remote
  // desktop (surface.ts), scaled to fit; keyboard and mouse go back in remote coordinates. The
  // remote desktop follows the pane's size when the server allows it.
  import { _ } from "./i18n.svelte";
  import { onDestroy, onMount } from "svelte";
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { bytes, COOP_MSG_JPEG, type Meta, type Session } from "./api";
  import { throttle } from "./coop";
  import CoopCursors from "./CoopCursors.svelte";
  import Icon from "./Icon.svelte";
  import { dialogTakesKeys } from "./Modal.svelte";
  import { app } from "./state.svelte";
  import { surface, type Surface } from "./surface";

  let {
    session,
    active,
    onid,
    onmeta,
    onconnected,
    onfocus,
    onexit,
    attach = 0,
    remote: peer = 0,
    shared = 0,
  }: {
    session: Session;
    active: boolean;
    /** An RDP tab that's already running (one moving between windows): shown, not opened. */
    attach?: number;
    /** Another person's desktop in a Terminal Co-op ring, by its number at the owner: shown on the frames the
     *  ring sends, never opened or resized here (the picture is letterboxed). Keys and the mouse go to the owner. */
    remote?: number;
    /** Your own tab's number in the ring while you share it: the others' pointers show over it. */
    shared?: number;
    onid: (id: number) => void;
    onmeta: (m: Meta) => void;
    /** The desktop's picture arrived (true), or the connection is gone or starting over (false). */
    onconnected: (on: boolean) => void;
    onfocus: () => void;
    /** Signed out from inside the session. */
    onexit: () => void;
  } = $props();

  let box: HTMLDivElement;
  /** Bound once drawn: the cursors over the picture measure it. */
  let canvas: HTMLCanvasElement = $state()!;
  let id = 0;
  /** The channel of the latest connect(): a viewer's backend stops sending to it when this view goes. */
  let channel = 0;
  /** The tab's number in the ring, if it is in one: pointers are shown over it and sent from it. */
  const ringTab = $derived(peer || shared);
  // One person at a time drives a shared desktop (src/coop.rs `may_drive`): the owner, or the one
  // guest they gave control to. A guest asks; the owner gives, declines or takes it back any time.
  /** The guest driving this desktop, "" for the owner. */
  const controller = $derived(ringTab ? (app.coop.control.find((c) => c.tab === ringTab)?.user_id ?? "") : "");
  const me = $derived(app.coop.ring?.members.find((m) => m.you)?.id ?? "");
  const ownerName = $derived(app.coop.ring?.members.find((m) => m.owner)?.name ?? _("The owner"));
  const nameOf = (id: string) => app.coop.ring?.members.find((m) => m.id === id)?.name ?? _("Someone");
  /** The asks for this desktop: the owner sees every guest's, a guest their own. */
  const asks = $derived(ringTab ? app.coop.wants.filter((w) => w.tab === ringTab) : []);
  /** A guest drives only with control; the owner's own keys and pointer always go. */
  const mayDrive = $derived(!peer || (!!me && controller === me));
  const ask = (want: boolean) => invoke("coop_ask_control", { ringTab: peer, want }).catch((e) => (app.status = String(e))).finally(() => box.focus());
  const give = (user: string) => invoke("coop_control", { tab: shared, user }).catch((e) => (app.status = String(e))).finally(() => box.focus());
  const decline = (user: string) => invoke("coop_decline_control", { tab: shared, user }).catch((e) => (app.status = String(e))).finally(() => box.focus());
  let status = $state(_("starting…"));
  let closed = $state(false);
  let size = $state<[number, number] | null>(null);
  /** VNC is plain text on the wire: every new connection says so, until dismissed or ended. */
  let insecure = $state(false);
  /** A VNC session reached through an SSH gateway (its shared login's) or a tunnel's port here. */
  function tunneled() {
    const k = session.kind;
    if (k.type !== "vnc") return true;
    return /^(127\.|localhost$|::1$|\[::1\]$)/i.test(k.host.trim()) || !!app.store.credential.find((c) => c.id === k.credential)?.gateway;
  }
  let move: PointerEvent | null = null;
  let screen: Surface;
  let queued = false;

  /** Desktop sizes RDP accepts: even widths, 200 to 8192 pixels. */
  const desktop = (r: DOMRect): [number, number] => {
    const fit = (v: number) => Math.max(200, Math.min(8192, Math.round((v * devicePixelRatio) / 2) * 2));
    return [fit(r.width), fit(r.height)];
  };

  /** Messages are handled one after another: a packed frame inflates asynchronously, and a size
   *  or a pointer that comes after it must not overtake it. */
  let inbox: Promise<void> = Promise.resolve();
  function message(m: unknown) {
    const b = bytes(m);
    // A frame drawn: the helper may send the next (it holds them back while we're behind), so
    // frames never pile up here waiting to be inflated and drawn.
    const frame = (b[0] === 3 || b[0] === 8) && id && !peer && session.kind.type === "rdp";
    inbox = inbox
      .then(() => handle(b))
      .catch(() => {})
      .then(() => void (frame && !gone && invoke("rdp_drawn", { id }).catch(() => {})));
  }

  async function handle(b: Uint8Array) {
    if (gone) return;
    const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
    const u16 = (at: number) => dv.getUint16(at, true);
    if (b[0] === 2) {
      size = [u16(1), u16(3)];
      locks = ""; // a new connection: its lock keys are whatever the desktop had
      screen.size(...size);
      showPointer();
      // A viewer who joins mid-stream may never get the status that says the desktop is up.
      if (peer) status = "";
      onconnected(true);
    } else if (b[0] === 3) paint(b.subarray(1));
    else if (b[0] === 8) {
      // Deflated on the way (src/rdp.rs `frame_msg`): inflated natively.
      const inflated = new Blob([b.subarray(1) as BlobPart]).stream().pipeThrough(new DecompressionStream("deflate"));
      const areas = new Uint8Array(await new Response(inflated).arrayBuffer());
      if (!gone) paint(areas);
    } else if (b[0] === 4) {
      closed = b[1] === 1;
      status = new TextDecoder().decode(b.subarray(2));
      // Ended, or not showing the desktop for now (reconnecting): its picture comes back with a size.
      if (closed || status) onconnected(false);
      onmeta({ cursor: null, title: "", alive: !closed, ended: false, lost: false, link: false, scrolled: false, images: [] });
    } else if (b[0] === 5) onexit();
    else if (b[0] === 7) {
      pointer = b.slice();
      showPointer();
    } else if (b[0] === COOP_MSG_JPEG && peer) jpeg(b.subarray(1));
  }

  /** Changed areas back to back, `[x][y][w][h: u16][w*h RGBA pixels]`, into the picture; it's
   *  shown at the next display refresh. */
  function paint(areas: Uint8Array) {
    const dv = new DataView(areas.buffer, areas.byteOffset, areas.byteLength);
    const u16 = (at: number) => dv.getUint16(at, true);
    for (let at = 0; at + 8 <= areas.length; ) {
      const [x, y, w, h] = [u16(at), u16(at + 2), u16(at + 4), u16(at + 6)];
      const end = at + 8 + w * h * 4;
      if (end > areas.length) break;
      screen.put(x, y, w, h, areas.subarray(at + 8, end));
      at = end;
    }
    if (!queued) {
      queued = true;
      requestAnimationFrame(() => ((queued = false), screen.show()));
    }
  }

  /** The desktop's pointer as it last came (`[7]…`, src/rdp.rs `MSG_POINTER`). It becomes this
   *  computer's pointer over the picture, as in mstsc: one pointer, moving without delay. */
  let pointer: Uint8Array | null = null;
  function showPointer() {
    if (!pointer || !canvas) return;
    if (pointer[1] !== 2) return void (canvas.style.cursor = pointer[1] === 1 ? "none" : "");
    const dv = new DataView(pointer.buffer, pointer.byteOffset, pointer.byteLength);
    const [w, h, hx, hy] = [2, 4, 6, 8].map((at) => dv.getUint16(at, true));
    if (!w || !h || pointer.length < 10 + w * h * 4) return void (canvas.style.cursor = "none");
    const c = document.createElement("canvas");
    [c.width, c.height] = [w, h];
    c.getContext("2d")!.putImageData(new ImageData(new Uint8ClampedArray(pointer.slice(10, 10 + w * h * 4).buffer), w, h), 0, 0);
    const url = c.toDataURL();
    // The picture is scaled to fit: the pointer is drawn at the same scale (image-set's resolution),
    // its hot spot with it. Where image-set isn't taken, the plain picture stays.
    const r = canvas.getBoundingClientRect();
    const [dw, dh] = size ?? [w, h];
    const scale = Math.min(r.width / dw, r.height / dh) || 1;
    const hot = (v: number, max: number) => Math.min(Math.round(v * scale), Math.max(0, Math.round(max * scale) - 1));
    canvas.style.cursor = `url(${url}) ${Math.min(hx, w - 1)} ${Math.min(hy, h - 1)}, default`;
    canvas.style.cursor = `image-set(url(${url}) ${1 / scale}x) ${hot(hx, w)} ${hot(hy, h)}, default`;
  }

  /** A website shared with you arrives as the whole page in a JPEG, with no size message first (src/coop.rs
   *  MSG_JPEG). It is decoded into the same surface, which CSS scales to fit; one decodes at a time and only
   *  the newest waits, so a slow decode never queues stale pages.
   *  ponytail: a read-back of the whole page per frame (4 a second at most); upload the bitmap as a texture
   *  if that shows up in a profile. */
  let latest: Uint8Array | null = null;
  let decoding = false;
  async function jpeg(data: Uint8Array) {
    latest = data;
    if (decoding) return;
    decoding = true;
    while (latest) {
      const next: Uint8Array = latest;
      latest = null;
      try {
        const page = await createImageBitmap(new Blob([next as BlobPart], { type: "image/jpeg" }));
        const [w, h] = [page.width, page.height];
        const scratch = new OffscreenCanvas(w, h).getContext("2d", { willReadFrequently: true })!;
        scratch.drawImage(page, 0, 0);
        page.close();
        if (gone) break;
        if (!size || size[0] !== w || size[1] !== h) {
          size = [w, h];
          screen.size(w, h);
        }
        // The first page ends "starting…".
        if (status) [status, closed] = ["", false];
        onconnected(true);
        screen.put(0, 0, w, h, new Uint8Array(scratch.getImageData(0, 0, w, h).data.buffer));
        screen.show();
      } catch {
        // A frame that doesn't decode is skipped; the next one replaces it.
      }
    }
    decoding = false;
  }

  /** Attached (or opened) once already. */
  let attached = false;
  async function connect() {
    const [width, height] = desktop(box.getBoundingClientRect());
    const out = new Channel<unknown>(message);
    channel = out.id;
    [closed, status, size] = [false, _("starting…"), null];
    onconnected(false);
    onmeta({ cursor: null, title: "", alive: true, ended: false, lost: false, link: false, scrolled: false, images: [] });
    // One moving here is attached to (once: reconnecting opens anew); if it's gone, it opens anew.
    const to = attached ? 0 : attach;
    attached = true;
    if (peer) {
      // Someone else's desktop: its frames come on this channel, and nothing here owns the tab.
      await invoke("coop_open_remote", { ringTab: peer, out }).catch((e) => (app.status = String(e)));
      id = peer;
    } else if (to && (await invoke<boolean>("tab_attach", { id: to, out }))) id = to;
    else {
      insecure = !tunneled();
      id = await invoke<number>("open_rdp", {
        session: $state.snapshot(session),
        out,
        width,
        height,
      });
    }
    // Closed while starting: end it now (onDestroy found no id yet).
    if (gone) return void (!peer && invoke("close_tab", { id }));
    if (!peer) onid(id);
    // The keyboard was taken before the tab had its number: now the hook knows where keys go.
    if (focused) keys();
  }

  /** Pointer position in remote desktop pixels. */
  function remote(e: MouseEvent): { x: number; y: number } {
    const r = canvas.getBoundingClientRect();
    const [w, h] = size ?? [1, 1];
    const scale = Math.min(r.width / w, r.height / h);
    const [ox, oy] = [(r.width - w * scale) / 2, (r.height - h * scale) / 2];
    const clamp = (v: number, max: number) => Math.max(0, Math.min(max - 1, Math.round(v)));
    return { x: clamp((e.clientX - r.left - ox) / scale, w), y: clamp((e.clientY - r.top - oy) / scale, h) };
  }

  /** Input goes one message after another: a quick tap's press and release must not swap places
   *  on the way (a Windows key released before it was pressed stays held on the desktop). */
  let queue: Promise<unknown> = Promise.resolve();
  const send = (ev: object) => {
    if (!id || closed || !mayDrive) return;
    const to = id;
    queue = queue.then(() => deliver(to, ev)).catch(() => {});
  };
  /** A viewer's keys, text and pointer go to the owner as the events rdp_input takes; their desktop (or VNC
   *  server, website or BMC console) handles them as it does its own, while the owner gave them control. */
  const deliver = (to: number, ev: object) => (peer ? invoke("coop_remote_input", { ringTab: peer, ev }) : invoke("rdp_input", { id: to, ev }));

  /** Where the pointer is over a tab in the ring, in desktop pixels, for the others to see (20 times a
   *  second at most; -1 when it leaves). */
  const pointing = throttle((x: number, y: number) => void (ringTab && invoke("coop_cursor", { tab: ringTab, col: x, row: y }).catch(() => {})), 50);
  /** A press over a tab in the ring: the others see a mark there. */
  const clicking = (e: PointerEvent) => {
    if (!ringTab || !size) return;
    const { x, y } = remote(e);
    invoke("coop_click", { tab: ringTab, col: x, row: y, button: e.button }).catch(() => {});
  };

  /** Where a desktop pixel is in the box, in CSS pixels: the picture is centred and scaled to fit. */
  function pixelAt(x: number, y: number): [number, number] {
    const r = canvas.getBoundingClientRect();
    const [w, h] = size ?? [1, 1];
    const scale = Math.min(r.width / w, r.height / h);
    return [(r.width - w * scale) / 2 + x * scale, (r.height - h * scale) / 2 + y * scale];
  }

  const MODIFIERS = new Set(["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight"]);
  const LOCKS = ["CapsLock", "NumLock", "ScrollLock"];
  /** The lock keys' states the desktop has from us, "" until it has them. */
  let locks = "";
  /** Gives the desktop the lock keys' states here, when they differ from what it has. */
  function sync(e: KeyboardEvent | PointerEvent) {
    const [caps, num, scroll] = LOCKS.map((k) => e.getModifierState(k));
    if (!id || closed || `${caps}${num}${scroll}` === locks) return;
    locks = `${caps}${num}${scroll}`;
    send({ t: "sync", caps, num, scroll });
  }

  function key(e: KeyboardEvent, down: boolean) {
    e.preventDefault();
    // A dialog is open over the window: the key is its (a release still goes, nothing stays held).
    if (down && dialogTakesKeys()) return;
    // Gone (or it couldn't connect): R tries again; Return, Escape, Backspace or Ctrl+C close the tab.
    if (closed) {
      const quit = ["Enter", "Escape", "Backspace"].includes(e.key) || (e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && e.code === "KeyC");
      if (down && e.key.toLowerCase() === "r") reconnect();
      else if (down && quit) onexit();
      return;
    }
    // Shift+Insert / Ctrl+Shift+V: type the local clipboard into the session.
    const paste = (e.shiftKey && e.code === "Insert") || (e.ctrlKey && e.shiftKey && e.code === "KeyV");
    // Shared desktops take nothing to or from a clipboard (the owner's app refuses it too, coop.rs
    // `desktop_ok`): a viewer's paste chord only says so.
    if (paste && peer) return void (down && (app.status = _("Copy and paste aren't available in shared desktops.")));
    if (paste) return down && send({ t: "paste" });
    // Ctrl+V pastes on the desktop: it gets what the clipboard here holds first, if that changed.
    if (down && !e.repeat && e.ctrlKey && e.code === "KeyV") clip();
    // The Windows keys stay this computer's unless the desktop may take them (Settings › Terminal › RDP).
    if (!grabs && (e.code === "MetaLeft" || e.code === "MetaRight")) return;
    // The lock keys go over as their states here, not as presses that would toggle whatever the
    // desktop had (Num Lock on there would turn the arrows of a numpad here into digits).
    sync(e);
    if (LOCKS.includes(e.code)) return;
    // A held modifier repeats here; the desktop already has it down.
    if (down && e.repeat && MODIFIERS.has(e.code)) return;
    // An accent waiting for its letter: the next key's character comes with it (as "é").
    if (e.key === "Dead") return;
    // Plain characters go as Unicode (any layout); everything else as scancodes.
    const chord = (e.ctrlKey || e.altKey || e.metaKey) && !e.getModifierState("AltGraph");
    if (down && [...e.key].length === 1 && !chord) send({ t: "text", text: e.key });
    else send({ t: "key", code: e.code, down });
  }

  // The Windows keys and the system's shortcuts (Win+E, Alt+Tab, Ctrl+Esc…) reach the desktop,
  // not this computer, while this view has the keyboard, if Settings › Terminal › RDP says so:
  // the backend takes them from the system and sends them to the tab itself (src/rdp.rs `grab`).
  // Anywhere else in xtermium (settings, the sidebar, another pane) they're this computer's.
  // Not for a viewer's: its keys go to the owner (as they come here).
  /** The desktop may take them (on until the settings say otherwise). */
  const grabs = $derived(app.settings?.rdp_keyboard ?? true);
  /** The view has the keyboard. */
  let focused = false;
  const keys = () => invoke("desktop_keys", { id, on: focused && grabs && !!id && !peer });
  // The setting changed while this desktop has the keyboard.
  $effect(() => {
    void grabs;
    if (focused) keys();
  });

  /** The desktop gets to paste what the clipboard here holds (only what changed crosses, and
   *  only if Settings › Terminal › RDP lets it). RDP only; a viewer's would be the owner's. */
  const clip = () => void (session.kind.type === "rdp" && !peer && send({ t: "clip" }));

  function wheel(e: WheelEvent) {
    e.preventDefault();
    // In notches of the desktop's wheel: one here is 100 pixels or 3 lines; trackpads send less.
    const scale = e.deltaMode === 1 ? 1 / 3 : e.deltaMode === 2 ? 3 : 1 / 100;
    send({ t: "wheel", dx: e.deltaX * scale, dy: -e.deltaY * scale });
  }

  function reconnect() {
    if (!peer) invoke("close_tab", { id });
    id = 0;
    connect();
  }

  $effect(() => {
    if (active) box.focus();
  });

  // Ask for the pane's size once it has stopped changing (dragging a splitter resizes it often).
  let resizeTimer = 0;
  const observer = new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      const r = box.getBoundingClientRect();
      if (!size || closed || peer || r.width < 50 || r.height < 50) return; // hidden or not connected yet; a viewer follows the owner
      const [width, height] = desktop(r);
      if (width !== size[0] || height !== size[1]) send({ t: "resize", width, height });
    }, 400);
  });

  onMount(() => {
    screen = surface(canvas);
    observer.observe(box);
    connect();
  });
  /** The tab moves to another window: leaving this one doesn't end it. */
  let released = false;
  export function release() {
    released = true;
  }

  let gone = false;
  onDestroy(() => {
    gone = true;
    // Leaving for another window: nothing stays held (a focused view taken away gets no blur).
    send({ t: "release" });
    focused = false;
    keys();
    observer.disconnect();
    clearTimeout(resizeTimer);
    screen?.drop();
    if (id && !released && !peer) invoke("close_tab", { id });
    if (peer && channel) invoke("coop_close_remote", { ringTab: peer, channel }).catch(() => {});
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="rdp"
  bind:this={box}
  tabindex="0"
  role="application"
  aria-label={_("remote desktop")}
  onkeydown={(e) => key(e, true)}
  onkeyup={(e) => key(e, false)}
  onfocus={() => !dialogTakesKeys() && ((focused = true), keys(), clip(), onfocus())}
  onblur={() => (send({ t: "release" }), (focused = false), keys())}
>
  <canvas
    bind:this={canvas}
    class:hidden={!size}
    onpointerdown={(e) => (box.focus(), canvas.setPointerCapture(e.pointerId), sync(e), clicking(e), send({ t: "button", button: e.button, down: true, ...remote(e) }))}
    onpointerup={(e) => send({ t: "button", button: e.button, down: false, ...remote(e) })}
    onpointermove={(e) => {
      if (!move) requestAnimationFrame(() => move && (send({ t: "move", ...remote(move) }), (move = null)));
      move = e;
      if (ringTab && size) pointing(remote(e).x, remote(e).y);
    }}
    onpointerleave={() => ringTab && pointing(-1, -1)}
    onwheel={wheel}
  ></canvas>
  {#if ringTab && canvas}<CoopCursors tab={ringTab} at={pixelAt} />{/if}
  {#if status}
    <div class="status" class:over={size}>
      {#if !closed}<Icon name="loader-alt" size={32} spin />{/if}
      <div>{status}</div>
      {#if closed}<div class="weak">{_("R reconnects; Return, Esc, Backspace or Ctrl+C close the tab")}</div>{/if}
    </div>
  {/if}
  {#if insecure && !closed && session.kind.type === "vnc"}
    <div class="insecure" role="alert">
      <Icon name="error" color="var(--warn)" />
      <span>
        {_("VNC isn't encrypted: anyone on the way to {host} can watch this screen and read what you type. Connect through an SSH gateway or a tunnel.", { host: session.kind.host })}
      </span>
      <button class="flat icon-only" title={_("Dismiss")} aria-label={_("Dismiss")} onclick={() => ((insecure = false), box.focus())}><Icon name="x" /></button>
    </div>
  {/if}
  <!-- Who drives a shared desktop. A guest always sees it (with the way to ask); the owner only while a guest drives or asks. -->
  {#if ringTab && !closed && (peer || controller || asks.length)}
    <div class="control" role="status">
      <Icon name="group" color={controller === me && me ? "var(--gold)" : undefined} />
      <span>
        {#if peer}
          {controller === me ? _("You have control.") : _("{name} has control.", { name: controller ? nameOf(controller) : ownerName })}
        {:else if controller}
          {_("{name} has control.", { name: nameOf(controller) })}
        {/if}
      </span>
      {#if peer}
        {#if controller === me}
          <button class="flat" onclick={() => ask(false)}>{_("Give control back")}</button>
        {:else if asks.some((a) => a.user_id === me)}
          <span class="weak">{_("Asked…")}</span>
          <button class="flat" onclick={() => ask(false)}>{_("Cancel")}</button>
        {:else}
          <button class="flat" onclick={() => ask(true)}>{_("Ask for control")}</button>
        {/if}
      {:else}
        {#if controller}<button class="flat" onclick={() => give("")}>{_("Take control")}</button>{/if}
        {#each asks as a (a.user_id)}
          <span class="asker">{_("{name} asks for control.", { name: nameOf(a.user_id) })}</span>
          <button class="flat" onclick={() => give(a.user_id)}>{_("Give control")}</button>
          <button class="flat" onclick={() => decline(a.user_id)}>{_("Not now")}</button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .rdp {
    position: absolute;
    inset: 0;
    background: #0c0c0c;
    outline: none;
  }
  canvas {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .hidden {
    visibility: hidden;
  }
  .status {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 13px;
    font-size: var(--t-xxl);
    color: #ccc; /* over the picture's black, which doesn't follow the theme */
    pointer-events: none;
  }
  /* Float over the desktop's top edge, in the app's colours. */
  .insecure,
  .control {
    position: absolute;
    top: 8px;
    inset-inline: 8px;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 5px 8px;
    color: var(--fg);
    background: var(--raised);
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
    box-shadow: var(--shadow-float);
    animation: float-in var(--move) var(--ease-out);
  }
  .insecure {
    border-color: color-mix(in srgb, var(--warn) 55%, var(--line));
  }
  .insecure span,
  .control > span:first-of-type {
    flex: 1;
    min-width: 0;
  }
  /* one line, its buttons small, under the VNC warning when both show */
  .control {
    align-items: center;
    flex-wrap: wrap;
    width: fit-content;
    max-width: calc(100% - 2 * 8px);
    inset-inline-start: 8px;
    inset-inline-end: auto;
    font-size: var(--t-s);
  }
  .control button {
    min-height: var(--row);
    padding: 0 5px;
  }
  .insecure ~ .control {
    top: calc(8px + var(--bar) + 8px);
  }
  /* Over the last picture (reconnecting, or the connection ended): dimmed so the words read. */
  .over {
    background: rgb(12 12 12 / 0.75);
  }
</style>
