<script lang="ts">
  // A terminal tab. The Rust side runs the VT engine and sends frames with the rows that
  // changed; this keeps the full screen model and paints it: on the GPU with a glyph cache
  // (glyphs.ts), or with Canvas2D where WebGL2 isn't available.
  import { _ } from "./i18n.svelte";
  import { onDestroy, onMount, tick as nextTick, untrack } from "svelte";
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { bytes, css, fontStack, openUrl, type Meta, type Session } from "./api";
  import { close, decode, frameAt, type Pic } from "./images";
  import { glRenderer, type Renderer, type Run } from "./glyphs";
  import { contexts } from "./surface";
  import Icon from "./Icon.svelte";
  import { dialogTakesKeys } from "./Modal.svelte";
  import Backdrop from "./Backdrop.svelte";
  import { app } from "./state.svelte";
  /** `blink`: the cursor blinks while focused; `line`: line height as a multiple of the font's. */
  type Look = { font: string; size: number; cursor: "Block" | "Bar" | "Underline"; blink: boolean; line: number; fg: number; bg: number; cc: number; alpha: number };
  type Frame = { rows: number; scroll?: number; lines: [number, Run[]][]; meta?: Meta; style?: Look };

  const BOLD = 1, ITALIC = 2, UNDERLINE = 4, STRIKE = 8, DIM = 16;

  let {
    session,
    active,
    visible,
    onid,
    onmeta,
    onfocus,
    onexit,
    zoom = 0,
    attach = 0,
  }: {
    session: Session;
    /** In the focused pane: takes the keyboard. */
    active: boolean;
    visible: boolean;
    /** Points added to this tab's font size (font zoom is per tab). */
    zoom?: number;
    /** A backend tab that's already running (one moving between windows): shown, not opened. */
    attach?: number;
    onid: (id: number) => void;
    onmeta: (m: Meta) => void;
    onfocus: () => void;
    /** The session ended by itself (`ended`: once, as it ends), or Return was pressed after it stopped. */
    onexit: (ended: boolean) => void;
  } = $props();

  let box: HTMLDivElement;
  /** The session had ended at the last update: `onexit(true)` comes once per ending. */
  let ended = false;
  let canvas: HTMLCanvasElement = $state()!;
  /** The GPU renderer; null: Canvas2D. */
  let gpu: Renderer | null = null;
  /** A canvas draws one way for good: a new one (a new key) switches between GPU and Canvas2D. */
  let canvasKey = $state(0);
  /** The canvas has drawn since it was last shown. Until then it stays invisible: a canvas whose
   *  GPU context is still coming back (after its tab was hidden) shows the browser's white
   *  "lost" placeholder for a frame, which flashes on every tab switch. */
  let painted = $state(true);
  /** This canvas has drawn with Canvas2D. */
  let flat = false;
  /** Measures the font (a WebGL canvas has no 2D context). */
  const ruler = new OffscreenCanvas(1, 1).getContext("2d")!;
  let input: HTMLTextAreaElement;
  let id = 0;
  let rows: Run[][] = [];
  /** Rows the canvas doesn't show as they are; `full` repaints everything. */
  let dirty: boolean[] = [];
  let full = true;
  /** Rows the canvas content moved up (down if negative) since the last paint. */
  let shift = 0;
  /** Where the cursor and images were painted, to erase them. */
  let drawnCursor = -1;
  let drawnImages = false;
  let meta: Meta = { cursor: null, title: "", alive: true, ended: false, lost: false, link: false, scrolled: false, images: [] };
  /** Ctrl is over a link: the pointer is a hand. */
  let overLink = $state(false);
  let look: Look | null = null;
  /** Decoded images by hash; null while decoding. */
  const images = new Map<string, Pic | null>();
  /** Cell metrics in device pixels; `mono`: every ASCII glyph advances exactly one cell. */
  let cell = { w: 8, h: 16, ascent: 12, dpr: 0, mono: true };
  let grid = { cols: 0, rows: 0, w: 0, h: 0 };
  let focused = false;
  /** The terminal background, also painted around the grid's margin. */
  let bg = $state("");
  /** The terminal's background colour (`bg` is it with the window effect's transparency). */
  let base = $state(0);

  let queued = false;
  let wheel = 0;
  let move: PointerEvent | null = null;
  /** Next repaint for an animated image. */
  let tick = 0;
  /** Blinking cursor: shown in this half of the cycle; restarts (shown) on typing. */
  let blinkOn = true;
  let blinker = 0;
  const decoder = new TextDecoder();

  const out = new Channel<unknown>((m) => {
    const b = bytes(m);
    if (b[0] === 0) frame(JSON.parse(decoder.decode(b.subarray(1))));
    else if (b[0] === 1) image(b);
  });

  function frame(f: Frame) {
    if (f.style) {
      look = f.style;
      bg = css(look.bg, look.alpha);
      base = look.bg;
      measure();
      blink();
    }
    if (rows.length !== f.rows) {
      rows = Array.from({ length: f.rows }, (_, i) => rows[i] ?? []);
      full = true;
    }
    if (dirty.length !== f.rows) dirty = new Array(f.rows).fill(true);
    const s = f.scroll ?? 0;
    if (s) {
      // Row y now shows what row y + s did; rows scrolled in are repainted.
      rows = rows.map((_, y) => rows[y + s] ?? []);
      dirty = dirty.map((_, y) => dirty[y + s] ?? true);
      shift += s;
    }
    for (const [y, runs] of f.lines) {
      rows[y] = runs;
      dirty[y] = true;
    }
    if (f.meta) {
      const was = meta.alive;
      meta = f.meta;
      const [back, above] = meta.scrollback ?? [0, 0];
      if (back !== bar.back || above !== bar.above || grid.rows !== bar.rows) bar = { back, above, rows: grid.rows };
      if (was !== meta.alive) aliveChanged();
      // Images that left the screen: the backend forgets them too.
      const live = new Set(meta.images.map((p) => p.hash));
      for (const [h, p] of images) {
        if (!live.has(h)) {
          if (p) close(p);
          images.delete(h);
        }
      }
      overLink = meta.link;
      onmeta(meta);
      if (meta.ended && !ended) onexit(true);
      ended = meta.ended;
    }
    schedule();
  }

  /** `[1][hash: 32][format][width u32][height u32][data]`; formats in images.ts. */
  function image(b: Uint8Array) {
    const hash = Array.from(b.subarray(1, 33), (x) => x.toString(16).padStart(2, "0")).join("");
    const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
    const [w, h] = [dv.getUint32(34, true), dv.getUint32(38, true)];
    images.set(hash, null);
    decode(b[33], w, h, b.slice(42))
      .then((p) => {
        if (images.has(hash)) {
          images.set(hash, p);
          schedule();
        } else close(p);
      })
      .catch(() => images.delete(hash));
  }

  function font(flags: number) {
    const size = Math.round(Math.max(6, look!.size + zoom) * devicePixelRatio * 100) / 100;
    return `${flags & ITALIC ? "italic " : ""}${flags & BOLD ? "bold " : ""}${size}px ${fontStack(look!.font)}`;
  }

  function measure() {
    if (!look || !canvas) return;
    const c = ruler;
    c.font = font(0);
    const m = c.measureText("M");
    const text = m.fontBoundingBoxAscent + m.fontBoundingBoxDescent;
    const h = Math.ceil(text * Math.max(1, look.line || 1));
    // Extra line height is shared above and below the text.
    const mono = ["i", "W", ".", "@"].every((ch) => c.measureText(ch).width === m.width);
    cell = { w: m.width, h, ascent: m.fontBoundingBoxAscent + Math.round((h - text) / 2), dpr: devicePixelRatio, mono };
    grid.cols = 0; // force a resize report
    full = true;
    gpu?.reset();
    fit();
  }

  /** Size the canvas to its box and tell the backend the grid size. */
  function fit() {
    if (!look || !box) return;
    if (cell.dpr !== devicePixelRatio) return measure();
    const r = canvas.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) return; // hidden
    const W = Math.round(r.width * devicePixelRatio);
    const H = Math.round(r.height * devicePixelRatio);
    if (canvas.width !== W || canvas.height !== H) {
      canvas.width = W;
      canvas.height = H;
      full = true;
      schedule();
    }
    const cols = Math.max(2, Math.floor(W / cell.w));
    const nrows = Math.max(1, Math.floor(H / cell.h));
    if (id && (cols !== grid.cols || nrows !== grid.rows || cell.w !== grid.w || cell.h !== grid.h)) {
      grid = { cols, rows: nrows, w: cell.w, h: cell.h };
      invoke("term_resize", { id, cols, rows: nrows, cw: cell.w, ch: cell.h });
    }
  }

  /** (Re)start the cursor blink, cursor shown; it only blinks while this terminal has focus. */
  function blink() {
    clearInterval(blinker);
    blinkOn = true;
    if (look?.blink && focused)
      blinker = setInterval(() => {
        blinkOn = !blinkOn;
        schedule();
      }, 530);
    schedule();
  }

  function schedule() {
    if (!queued) {
      queued = true;
      requestAnimationFrame(draw);
    }
  }

  function draw() {
    queued = false;
    clearTimeout(tick);
    if (!visible || !look) return;
    // Images: the frame due now, and a repaint when the next one is.
    const now = performance.now();
    let next = Infinity;
    const pics: { bmp: ImageBitmap; uv: number[]; dst: number[] }[] = [];
    for (const p of meta.images) {
      const pic = images.get(p.hash);
      if (!pic) continue;
      const [bmp, wait] = frameAt(pic, now);
      next = Math.min(next, wait);
      pics.push({ bmp, uv: p.uv, dst: p.dst });
    }
    if (next < Infinity) tick = setTimeout(schedule, Math.max(16, next));
    const cursor = meta.cursor && !meta.scrolled && (blinkOn || !focused) ? meta.cursor : null;

    if (gpu) {
      // The GPU redraws the whole screen each frame; it's one draw call.
      // Without the keyboard, an outline whatever the style: the one terminal that has it stands out.
      const shape = focused ? look.cursor : "Hollow";
      gpu.draw({
        rows,
        cw: cell.w,
        ch: cell.h,
        ascent: cell.ascent,
        dpr: devicePixelRatio,
        font,
        images: pics,
        cursor: cursor && { x: cursor[0], y: cursor[1], shape, color: look.cc },
      });
      full = false;
      shift = 0;
      // Shown again once there's a real picture; then drawn once more, now that it's on screen.
      if (!painted && gpu.ready()) {
        painted = true;
        full = true;
        schedule();
      }
      return;
    }
    painted = true;

    const c = canvas.getContext("2d");
    if (!c) return; // still the GPU's canvas, about to be replaced
    flat = true;
    const { w: cw, h: ch, ascent } = cell;
    const dpr = devicePixelRatio;
    const W = canvas.width;
    const n = rows.length;
    const x = (col: number) => Math.round(col * cw);

    // Images lie under the text across rows: repaint everything while there are any.
    if (full || meta.images.length || drawnImages || Math.abs(shift) >= n) {
      full = false;
      dirty.fill(true);
      c.clearRect(0, 0, W, canvas.height);
    } else if (shift) {
      // A scroll moves the picture on the GPU; only the rows scrolled in are drawn.
      c.globalCompositeOperation = "copy";
      c.drawImage(canvas, 0, -shift * ch);
      c.globalCompositeOperation = "source-over";
      c.clearRect(0, n * ch, W, canvas.height - n * ch); // a row moved down into the margin
      drawnCursor -= shift;
    }
    shift = 0;
    // The cursor is painted over its row: repaint where it was and where it is.
    if (drawnCursor >= 0 && drawnCursor < n) dirty[drawnCursor] = true;
    if (cursor && cursor[1] < n) dirty[cursor[1]] = true;

    for (let y = 0; y < n; y++) {
      if (!dirty[y]) continue;
      c.clearRect(0, y * ch, W, ch);
      for (const r of rows[y]) {
        if (r[4] !== null) {
          c.fillStyle = css(r[4]);
          c.fillRect(x(r[0]), y * ch, x(r[0] + r[1]) - x(r[0]), ch);
        }
      }
    }

    for (const { bmp, uv, dst } of pics) {
      const [u0, v0, u1, v1] = uv;
      const [x0, y0, x1, y1] = dst;
      c.drawImage(bmp, u0 * bmp.width, v0 * bmp.height, (u1 - u0) * bmp.width, (v1 - v0) * bmp.height, x0 * cw, y0 * ch, (x1 - x0) * cw, (y1 - y0) * ch);
    }
    drawnImages = meta.images.length > 0;

    // Text on the cell grid, each row clipped to itself so repainting a neighbour never leaves a
    // glyph's overhang behind.
    c.textBaseline = "alphabetic";
    c.font = font(0);
    for (let y = 0; y < n; y++) {
      if (!dirty[y]) continue;
      dirty[y] = false;
      const top = y * ch;
      const base = top + ascent;
      c.save();
      c.beginPath();
      c.rect(0, top, W, ch);
      c.clip();
      let fill = -1;
      let style = 0;
      for (const [col, w, text, fg, , flags] of rows[y]) {
        if (fg !== fill) {
          c.fillStyle = css(fg);
          fill = fg;
        }
        c.globalAlpha = flags & DIM ? 0.6 : 1;
        if ((flags & 3) !== style) {
          style = flags & 3;
          c.font = font(style);
        }
        if (w > 1 && text.length === w && text.charCodeAt(1) < 128) {
          // Plain ASCII, merged by the backend: one call for the run when the font keeps to the
          // cells (it does: the cell is its advance), else one per character. Per character
          // costs 3.5x the frame time in a log flood (1.9 vs 0.55 ms at 120x46).
          if (cell.mono) c.fillText(text, x(col), base);
          else for (let i = 0; i < w; i++) if (text[i] !== " ") c.fillText(text[i], x(col + i), base);
        } else if (text !== " ") {
          // One cluster: squeezed into its cells if the font's glyph is wider.
          c.fillText(text, x(col), base, x(col + w) - x(col));
        }
        const width = x(col + w) - x(col);
        if (flags & UNDERLINE) c.fillRect(x(col), top + ch - Math.max(1, dpr), width, Math.max(1, dpr));
        if (flags & STRIKE) c.fillRect(x(col), top + Math.round(ch / 2), width, Math.max(1, dpr));
      }
      c.restore();
    }

    drawnCursor = cursor ? cursor[1] : -1;
    if (cursor) {
      const [cx, cy] = cursor;
      const [px, py] = [x(cx), cy * ch];
      c.fillStyle = c.strokeStyle = css(look.cc);
      if (!focused) {
        c.lineWidth = Math.max(1, dpr);
        c.strokeRect(px + 0.5, py + 0.5, Math.round(cw) - 1, ch - 1);
      } else if (look.cursor === "Bar") c.fillRect(px, py, Math.round(2 * dpr), ch);
      else if (look.cursor === "Underline") c.fillRect(px, py + ch - Math.round(2 * dpr), Math.round(cw), Math.round(2 * dpr));
      else {
        c.globalAlpha = 0.7;
        c.fillRect(px, py, Math.round(cw), ch);
        c.globalAlpha = 1;
      }
    }
  }

  // ---- keyboard: specials and Ctrl/Alt chords go as keys, everything else as text ----

  const IGNORED = new Set(["Shift", "Control", "Alt", "Meta", "CapsLock", "AltGraph", "NumLock", "ScrollLock", "Dead", "Unidentified", "Process", "OS"]);

  function keydown(e: KeyboardEvent) {
    // A dialog is open over the window: what's typed is its, never the server's.
    if (dialogTakesKeys()) return void e.preventDefault();
    if (e.key === "Control" && !e.repeat) hoverLinks(true);
    if (e.isComposing || IGNORED.has(e.key) || !id) return;
    blink();
    // Ctrl+Alt is AltGr on Windows layouts: its characters arrive as text.
    const altGr = e.getModifierState("AltGraph") || (e.ctrlKey && e.altKey && e.key.length === 1);
    const chord = (e.ctrlKey || e.altKey || e.metaKey) && !altGr;
    // A stopped session (it ended, or couldn't connect): Return, Escape, Backspace or Ctrl+C
    // close the tab, S saves the output, R restarts (backend). While it reconnects by itself,
    // Escape only stops that, as the reconnect bar says.
    const plainCtrl = e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
    const quit = ["Enter", "Escape", "Backspace"].includes(e.key) || (plainCtrl && e.code === "KeyC");
    if (!meta.alive && (quit || !chord)) {
      e.preventDefault();
      const k = e.key.toLowerCase();
      if (k === "escape" && retryAt) giveUp();
      else if (quit) onexit(false);
      else if (k === "s")
        invoke<string | null>("save_output", { id })
          .then((path) => path && (app.status = _("Saved {path}", { path })))
          .catch((err) => (app.status = String(err)));
      else if (k === "r") {
        stopRetry();
        invoke("term_key", { id, key: { key: e.key, ctrl: false, alt: false, shift: false, meta: false } });
      }
      return;
    }
    if (e.key.length > 1 || chord) {
      e.preventDefault();
      // A chord on a non-Latin layout (Greek, Cyrillic…) goes by the physical key: Ctrl+C is ^C
      // for the shell whatever letter the layout puts there.
      const latin = chord && /^Key[A-Z]$/.test(e.code) && !/^[\x20-\x7e]$/.test(e.key);
      const key = { key: latin ? e.code.slice(3).toLowerCase() : e.key, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey };
      invoke("term_key", { id, key });
    }
  }

  function text() {
    const t = input.value;
    input.value = "";
    if (t && id && !dialogTakesKeys()) invoke("term_text", { id, text: t });
  }

  // ---- auto-reconnect (Settings > SSH connections): a dropped connection comes back by itself ----

  const RETRIES = 8;
  /** Retries made since the connection last held for a while. */
  let attempt = $state(0);
  /** When the session last came up. */
  let upSince = Date.now();
  /** When the next retry starts (ms); 0: none pending. */
  let retryAt = $state(0);
  let now = $state(Date.now());
  let retryTimer = 0;
  let clock = 0;

  function aliveChanged() {
    if (meta.alive) return void (upSince = Date.now());
    // Up for a while before it dropped: a new run of retries.
    if (Date.now() - upSince > 20_000) attempt = 0;
    // Once retrying, failed connects count too: the network may still be down.
    const drop = meta.lost || (attempt > 0 && !meta.ended);
    if (app.settings?.auto_reconnect && drop && attempt < RETRIES) {
      // 3, 6, 12, 24, then 30 s: the first wait leaves time for a brief blip to pass.
      const wait = Math.min(30, 3 * 2 ** attempt) * 1000;
      attempt++;
      retryAt = Date.now() + wait;
      retryTimer = setTimeout(reconnectNow, wait);
      clock = setInterval(() => (now = Date.now()), 250);
    }
  }

  function stopRetry() {
    clearTimeout(retryTimer);
    clearInterval(clock);
    retryAt = 0;
  }

  /** Stop reconnecting until the session is up again. */
  function giveUp() {
    stopRetry();
    attempt = RETRIES;
  }

  function reconnectNow() {
    stopRetry();
    if (id && !meta.alive) invoke("term_key", { id, key: { key: "r", ctrl: false, alt: false, shift: false, meta: false } });
  }

  // ---- find bar (Ctrl+Shift+F): the backend searches the scrollback and highlights matches ----

  let finding = $state(false);
  let query = $state("");
  let matchCase = $state(false);
  /** Matches, and the current one (1-based; 0: none). */
  let found = $state<[number, number]>([0, 0]);
  /** The query the matches are for: the first Enter after typing shows the newest match. */
  let searched = "";
  let findInput = $state<HTMLInputElement>();
  let findTimer = 0;

  /** Opens the find bar (App calls this for the focused tab), with its text selected. */
  export function find() {
    finding = true;
    nextTick().then(() => findInput?.select());
  }

  /** Searches again (output may have arrived) and moves `step` matches; -1 is older (up). */
  function search(step: number) {
    clearTimeout(findTimer);
    if (!id) return;
    if (query !== searched) step = 0;
    searched = query;
    invoke<[number, number]>("term_find", { id, query, case: matchCase, step }).then((r) => (found = r));
  }

  function closeFind() {
    finding = false;
    [query, searched, found] = ["", "", [0, 0]];
    if (id) invoke("term_find", { id, query: "", case: false, step: 0 });
    input.focus();
  }

  function findKey(e: KeyboardEvent) {
    if (e.key === "Enter") search(e.shiftKey ? 1 : -1);
    else if (e.key === "Escape") closeFind();
    else return;
    e.preventDefault();
  }

  // ---- mouse ----

  /** Where the pointer last was over the grid (CSS pixels), for showing links when Ctrl changes. */
  let pointer: { x: number; y: number } | null = null;

  function mouse(kind: string, e: MouseEvent, lines = 0, clicks = 1) {
    pointer = { x: e.offsetX, y: e.offsetY };
    // Moves report button -1; the backend takes 0 (left), 1 (middle) or 2 (right).
    report({ kind, button: Math.max(0, e.button), x: e.offsetX, y: e.offsetY, lines, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, clicks });
  }

  /** `clicks`: 2 selects the word under the pointer, 3 its line. */
  type Pointer = { kind: string; button: number; x: number; y: number; lines: number; ctrl: boolean; alt: boolean; shift: boolean; clicks?: number };
  function report(p: Pointer) {
    if (!id) return;
    const dpr = devicePixelRatio;
    // Ctrl+click on a link: the backend hands back its address.
    invoke<string | null>("term_mouse", { id, ev: { ...p, x: p.x * dpr, y: p.y * dpr } }).then(
      (url) => url && openUrl(url).catch((e) => (app.status = String(e))),
    );
  }

  /** Ctrl went down or up without the pointer moving: show or hide the link under it. */
  function hoverLinks(ctrl: boolean) {
    if (pointer && (ctrl || overLink)) report({ kind: "move", button: 0, ...pointer, lines: 0, ctrl, alt: false, shift: false });
  }

  /** The last press, to count double and triple clicks (pointer events don't). */
  let press = { at: 0, x: 0, y: 0, n: 0 };
  function pointerdown(e: PointerEvent) {
    e.preventDefault();
    canvas.setPointerCapture(e.pointerId);
    input.focus();
    onfocus();
    const again = e.button === 0 && e.timeStamp - press.at < 450 && Math.hypot(e.offsetX - press.x, e.offsetY - press.y) < 5;
    press = { at: e.timeStamp, x: e.offsetX, y: e.offsetY, n: again ? (press.n % 3) + 1 : 1 };
    mouse("down", e, 0, press.n);
  }

  function pointermove(e: PointerEvent) {
    // Send at most one move per frame.
    if (!move) requestAnimationFrame(() => move && (mouse("move", move), (move = null)));
    move = e;
  }

  function onwheel(e: WheelEvent) {
    e.preventDefault();
    // One notch ≈ 100 px in Chromium ≈ 3 lines.
    wheel += e.deltaMode === 1 ? -e.deltaY : e.deltaMode === 2 ? -e.deltaY * grid.rows : -e.deltaY / 33.3;
    const n = Math.trunc(wheel);
    wheel -= n;
    if (n) mouse("wheel", e, n);
  }

  $effect(() => {
    if (visible) untrack(onScreen);
    else if (gpu) {
      painted = false;
      gpu.sleep();
    }
  });

  /** Shown: the GPU if a context is free (many panes on screen can use them all), else Canvas2D. */
  async function onScreen() {
    if (gpu && !gpu.wake()) {
      gpu = null;
      await newCanvas();
    } else if (!gpu && contexts.room()) {
      if (flat) await newCanvas();
      gpu = glRenderer(canvas, schedule);
    }
    fit();
    schedule();
  }
  async function newCanvas() {
    canvasKey++;
    flat = false;
    full = true;
    await nextTick();
  }
  $effect(() => {
    if (active && visible) input.focus();
  });
  // A new font zoom measures again; new padding resizes the canvas, not the box it watches.
  $effect(() => {
    void zoom;
    untrack(measure);
  });
  $effect(() => {
    void app.settings?.pane_padding;
    nextTick().then(() => untrack(fit));
  });

  /** The tab moves to another window: leaving this one doesn't end it. */
  let released = false;
  export function release() {
    released = true;
  }

  // ---- scroll bar (Settings > Terminal): where the screen is in the scrollback; drag or click it ----
  /** Rows scrolled back from the bottom, rows of scrollback above the screen, rows on screen. */
  let bar = $state({ back: 0, above: 0, rows: 0 });
  let track = $state<HTMLDivElement>();
  const all = $derived(bar.above + bar.rows);
  let scrollTo = -1;
  /** Sends at most one scroll every 16 ms. */
  function scrollBack(back: number) {
    const was = scrollTo;
    scrollTo = Math.max(0, Math.min(bar.above, Math.round(back)));
    if (was < 0) setTimeout(() => (invoke("term_scroll", { id, back: scrollTo }), (scrollTo = -1)), 16);
  }
  /** Click on the track: the screen centres there. */
  function trackDown(e: PointerEvent) {
    if (e.button !== 0 || !track || !all) return;
    const r = track.getBoundingClientRect();
    scrollBack(bar.above - ((e.clientY - r.top) / r.height) * all + bar.rows / 2);
  }
  function thumbDown(e: PointerEvent) {
    if (e.button !== 0 || !track || !all) return;
    e.stopPropagation();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const [y0, back0, h] = [e.clientY, bar.back, track.getBoundingClientRect().height];
    const move = (m: PointerEvent) => scrollBack(back0 - ((m.clientY - y0) / h) * all);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", () => el.removeEventListener("pointermove", move), { once: true });
  }

  let observer: ResizeObserver;
  // Cached glyphs drawn before a font finished loading came from a fallback font.
  const fontsLoaded = () => measure();
  onMount(async () => {
    document.fonts.addEventListener("loadingdone", fontsLoaded);
    observer = new ResizeObserver(() => fit());
    observer.observe(box);
    // One moving here is attached to; if it ended meanwhile, the session starts again.
    if (attach && (await invoke<boolean>("tab_attach", { id: attach, out }))) id = attach;
    else id = await invoke<number>("open_term", { session: $state.snapshot(session), out });
    // Closed while starting: end it now (onDestroy found no id yet).
    if (gone) return void invoke("close_tab", { id });
    onid(id);
    fit();
  });
  let gone = false;
  onDestroy(() => {
    gone = true;
    observer?.disconnect();
    document.fonts.removeEventListener("loadingdone", fontsLoaded);
    if (id && !released) invoke("close_tab", { id });
    clearTimeout(tick);
    clearInterval(blinker);
    gpu?.sleep();
    stopRetry();
    for (const p of images.values()) if (p) close(p);
  });
</script>

<div class="term" bind:this={box} style:background={bg} style:--pad="{app.settings?.pane_padding ?? 6}px">
  <!-- What's behind the text (Settings › Terminal), under the terminal's own colour; cells with
       a colour of their own stay solid. -->
  <Backdrop place="terminal" tint={(a) => css(base, a)} />
  {#key canvasKey}<canvas
    bind:this={canvas}
    onpointerdown={pointerdown}
    onpointermove={pointermove}
    onpointerup={(e) => {
      // Send the last move first, so a selection ends where the button came up.
      if (move) mouse("move", move), (move = null);
      mouse("up", e);
    }}
    onwheel={onwheel}
    onpointerleave={() => (hoverLinks(false), (pointer = null))}
    style:cursor={overLink ? "pointer" : null}
    style:visibility={painted ? null : "hidden"}
  ></canvas>{/key}
  <textarea
    bind:this={input}
    class="sink"
    aria-label={_("terminal input")}
    autocomplete="off"
    spellcheck="false"
    onkeydown={keydown}
    onkeyup={(e) => e.key === "Control" && hoverLinks(false)}
    oninput={(e) => !(e as unknown as InputEvent).isComposing && text()}
    oncompositionend={text}
    onfocus={() => !dialogTakesKeys() && ((focused = true), blink(), onfocus())}
    onblur={() => ((focused = false), blink(), hoverLinks(false))}
  ></textarea>
  {#if app.settings?.scrollbar !== false && bar.above > 0}
    <!-- The keyboard scrolls with Shift+PageUp and Shift+PageDown; this is for the mouse. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="scrollbar" aria-hidden="true" bind:this={track} onpointerdown={trackDown}>
      <div
        class="thumb"
        style:top="{((bar.above - bar.back) / all) * 100}%"
        style:height="{(bar.rows / all) * 100}%"
        onpointerdown={thumbDown}
      ></div>
    </div>
  {/if}
  {#if retryAt}
    <div class="retry" role="status">
      <Icon name="refresh" size={16} />
      <span
        >{_("Connection lost. Reconnecting in {s} s", { s: Math.max(0, Math.ceil((retryAt - now) / 1000)) })}
        <span class="weak">{_("(try {n} of {total})", { n: attempt, total: RETRIES })}</span></span
      >
      <button class="flat" onclick={reconnectNow}>{_("Reconnect now")}</button>
      <button class="flat" title={_("Stop reconnecting (Esc)")} onclick={giveUp}>{_("Stop")}</button>
    </div>
  {/if}
  {#if finding}
    <div class="find" role="search">
      <input
        bind:this={findInput}
        bind:value={query}
        placeholder={_("Find")}
        aria-label={_("Find in terminal")}
        spellcheck="false"
        oninput={() => {
          clearTimeout(findTimer);
          findTimer = setTimeout(() => search(0), 120);
        }}
        onkeydown={findKey}
      />
      <span class="count weak">{query ? (found[0] ? _("{n} of {total}", { n: found[1], total: `${found[0]}${found[0] >= 10000 ? "+" : ""}` }) : _("No results")) : ""}</span>
      <button class="flat icon-only aa" class:on={matchCase} aria-pressed={matchCase} title={_("Match case")} onclick={() => ((matchCase = !matchCase), (searched = ""), search(0))}>Aa</button>
      <button class="flat icon-only" title={_("Previous match, older (Enter)")} disabled={!found[0]} onclick={() => search(-1)}><Icon name="up-arrow-alt" size={16} /></button>
      <button class="flat icon-only down" title={_("Next match, newer (Shift+Enter)")} disabled={!found[0]} onclick={() => search(1)}><Icon name="up-arrow-alt" size={16} /></button>
      <button class="flat icon-only" title={_("Close (Esc)")} onclick={closeFind}><Icon name="x" size={16} /></button>
    </div>
  {/if}
</div>

<style>
  .term {
    position: absolute;
    inset: 0;
    overflow: hidden;
    isolation: isolate; /* the Backdrop goes under the text, over the background colour */
  }
  canvas {
    position: absolute;
    inset: var(--pad);
    width: calc(100% - 2 * var(--pad));
    height: calc(100% - 2 * var(--pad));
    cursor: text;
  }
  /* thin, over the terminal's right edge; clearer while the pointer is on it */
  .scrollbar {
    position: absolute;
    top: 5px;
    right: 2px;
    bottom: 5px;
    width: 8px;
    z-index: 1;
  }
  .thumb {
    position: absolute;
    left: 1px;
    right: 1px;
    min-height: 24px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--fg) 22%, transparent);
    transition: background 0.15s, left 0.15s, right 0.15s;
  }
  .scrollbar:hover .thumb,
  .thumb:active {
    left: 0;
    right: 0;
    background: color-mix(in srgb, var(--fg) 45%, transparent);
  }
  .find {
    position: absolute;
    top: 5px;
    right: 13px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    background: rgba(var(--panel-rgb), 0.98);
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
    box-shadow: var(--shadow-float);
    --from: -5px;
    animation: float-in 0.16s var(--ease-out);
  }
  .retry {
    position: absolute;
    left: 50%;
    bottom: 13px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 5px 5px 8px;
    white-space: nowrap;
    background: rgba(var(--panel-rgb), 0.98);
    border: 1px solid var(--warn);
    border-radius: var(--r-menu);
    box-shadow: var(--shadow-float);
    animation: float-in 0.16s var(--ease-out);
  }
  .retry > :global(.icon) {
    color: var(--warn);
  }
  .find input {
    width: 200px;
  }
  .find .count {
    min-width: 76px;
    padding: 0 5px;
    font-size: var(--t-s);
    text-align: right;
    white-space: nowrap;
  }
  .find .aa {
    font-size: var(--t-s);
    font-weight: 600;
  }
  .find .down :global(.icon) {
    transform: rotate(180deg);
  }
  .sink {
    position: absolute;
    left: 0;
    top: 0;
    width: 1px;
    height: 1px;
    opacity: 0;
    resize: none;
    border: 0;
    padding: 0;
    pointer-events: none;
  }
</style>
