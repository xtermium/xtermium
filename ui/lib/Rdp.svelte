<script lang="ts">
  // An RDP tab: the helper's changed areas are painted into a canvas the size of the remote
  // desktop (surface.ts), scaled to fit; keyboard and mouse go back in remote coordinates. The
  // remote desktop follows the pane's size when the server allows it.
  import { _ } from "./i18n.svelte";
  import { onDestroy, onMount } from "svelte";
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { bytes, type Meta, type Session } from "./api";
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
  }: {
    session: Session;
    active: boolean;
    /** An RDP tab that's already running (one moving between windows): shown, not opened. */
    attach?: number;
    onid: (id: number) => void;
    onmeta: (m: Meta) => void;
    /** The desktop's picture arrived (true), or the connection is gone or starting over (false). */
    onconnected: (on: boolean) => void;
    onfocus: () => void;
    /** Signed out from inside the session. */
    onexit: () => void;
  } = $props();

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let id = 0;
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

  function message(m: unknown) {
    const b = bytes(m);
    const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
    const u16 = (at: number) => dv.getUint16(at, true);
    if (b[0] === 2) {
      size = [u16(1), u16(3)];
      locks = ""; // a new connection: its lock keys are whatever the desktop had
      screen.size(...size);
      onconnected(true);
    } else if (b[0] === 3) {
      // Changed areas back to back: [x][y][w][h: u16][w*h RGBA pixels].
      for (let at = 1; at + 8 <= b.length; ) {
        const [x, y, w, h] = [u16(at), u16(at + 2), u16(at + 4), u16(at + 6)];
        const end = at + 8 + w * h * 4;
        if (end > b.length) break;
        screen.put(x, y, w, h, b.subarray(at + 8, end));
        at = end;
      }
      if (!queued) {
        queued = true;
        requestAnimationFrame(() => ((queued = false), screen.show()));
      }
    } else if (b[0] === 4) {
      closed = b[1] === 1;
      status = new TextDecoder().decode(b.subarray(2));
      // Ended, or not showing the desktop for now (reconnecting): its picture comes back with a size.
      if (closed || status) onconnected(false);
      onmeta({ cursor: null, title: "", alive: !closed, ended: false, lost: false, link: false, scrolled: false, images: [] });
    } else if (b[0] === 5) onexit();
  }

  /** Attached (or opened) once already. */
  let attached = false;
  async function connect() {
    const [width, height] = desktop(box.getBoundingClientRect());
    const out = new Channel<unknown>(message);
    [closed, status, size] = [false, _("starting…"), null];
    onconnected(false);
    onmeta({ cursor: null, title: "", alive: true, ended: false, lost: false, link: false, scrolled: false, images: [] });
    // One moving here is attached to (once: reconnecting opens anew); if it's gone, it opens anew.
    const to = attached ? 0 : attach;
    attached = true;
    if (to && (await invoke<boolean>("tab_attach", { id: to, out }))) id = to;
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
    if (gone) return void invoke("close_tab", { id });
    onid(id);
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
    if (!id || closed) return;
    const to = id;
    queue = queue.then(() => invoke("rdp_input", { id: to, ev })).catch(() => {});
  };

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
    if (paste) return down && send({ t: "paste" });
    // The lock keys go over as their states here, not as presses that would toggle whatever the
    // desktop had (Num Lock on there would turn the arrows of a numpad here into digits).
    sync(e);
    if (LOCKS.includes(e.code)) return;
    // A held modifier repeats here; the desktop already has it down.
    if (down && e.repeat && MODIFIERS.has(e.code)) return;
    // An accent waiting for its letter: the next key's character comes with it (as "é").
    if (e.key === "Dead") return;
    // Plain characters go as Unicode (any layout); everything else as scancodes.
    const chord = (e.ctrlKey || e.altKey || e.metaKey || win) && !e.getModifierState("AltGraph");
    if (down && [...e.key].length === 1 && !chord) send({ t: "text", text: e.key });
    else send({ t: "key", code: e.code, down });
  }

  // The Windows keys reach the desktop with the keyboard, not the Start menu here: the backend
  // takes them from the system while we have it (src/rdp.rs `desktop_keys`).
  /** The Windows key held on the desktop, if one is. */
  let win: "MetaLeft" | "MetaRight" | null = null;
  const keys = (on: boolean) => invoke("desktop_keys", { on });
  const unwin = listen<[boolean, boolean]>("win-key", ({ payload: [right, down] }) => {
    const code = right ? "MetaRight" : "MetaLeft";
    // A release still goes to the desktop that got the press.
    if (document.activeElement !== box && !(win === code && !down)) return;
    win = down ? code : null;
    send({ t: "key", code, down });
  });

  function wheel(e: WheelEvent) {
    e.preventDefault();
    // In notches of the desktop's wheel: one here is 100 pixels or 3 lines; trackpads send less.
    const scale = e.deltaMode === 1 ? 1 / 3 : e.deltaMode === 2 ? 3 : 1 / 100;
    send({ t: "wheel", dx: e.deltaX * scale, dy: -e.deltaY * scale });
  }

  function reconnect() {
    invoke("close_tab", { id });
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
      if (!size || closed || r.width < 50 || r.height < 50) return; // hidden or not connected yet
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
    if (document.activeElement === box) keys(false);
    unwin.then((f) => f());
    observer.disconnect();
    clearTimeout(resizeTimer);
    screen?.drop();
    if (id && !released) invoke("close_tab", { id });
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
  onfocus={() => !dialogTakesKeys() && (keys(true), onfocus())}
  onblur={() => (send({ t: "release" }), (win = null), keys(false))}
>
  <canvas
    bind:this={canvas}
    class:hidden={!size}
    onpointerdown={(e) => (box.focus(), canvas.setPointerCapture(e.pointerId), sync(e), send({ t: "button", button: e.button, down: true, ...remote(e) }))}
    onpointerup={(e) => send({ t: "button", button: e.button, down: false, ...remote(e) })}
    onpointermove={(e) => {
      if (!move) requestAnimationFrame(() => move && (send({ t: "move", ...remote(move) }), (move = null)));
      move = e;
    }}
    onwheel={wheel}
  ></canvas>
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
  /* Floats over the desktop's top edge, in the app's colours. */
  .insecure {
    position: absolute;
    top: 8px;
    inset-inline: 8px;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 5px 8px;
    color: var(--fg);
    background: var(--raised);
    border: 1px solid color-mix(in srgb, var(--warn) 55%, var(--line));
    border-radius: var(--r-menu);
    box-shadow: var(--shadow-float);
    animation: float-in 0.15s ease-out;
  }
  .insecure span {
    flex: 1;
    min-width: 0;
  }
  /* Over the last picture (reconnecting, or the connection ended): dimmed so the words read. */
  .over {
    background: rgb(12 12 12 / 0.75);
  }
</style>
