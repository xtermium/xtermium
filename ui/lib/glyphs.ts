// The terminal on the GPU, the way fast terminals draw text: each glyph is drawn once by the
// browser's own text engine into an atlas texture (a glyph cache), and a frame is one instanced
// draw of cell-sized quads that copy from it. Backgrounds, lines and the cursor are quads too, and
// images are textured quads between backgrounds and text.
//
// The atlas holds coverage, not colour, so truecolor output can't fill it: the shader colours each
// glyph. A glyph is drawn twice at most, white for light text and black for dark text, because the
// text engine tunes antialiasing contrast to the text's brightness. Colour glyphs (emoji) are found
// when drawn and copied as they are.

import { contexts } from "./surface";

/** [column, width in cells, text, fg, bg or null, flags]: see Terminal.svelte. */
export type Run = [number, number, string, number, number | null, number];

export type Scene = {
  rows: Run[][];
  /** Cell width (fractional), height and baseline, in device pixels. */
  cw: number;
  ch: number;
  ascent: number;
  dpr: number;
  font: (flags: number) => string;
  /** Images: the bitmap, its normalized source rect and its destination in cells. */
  images: { bmp: ImageBitmap; uv: number[]; dst: number[] }[];
  cursor: { x: number; y: number; shape: "Bar" | "Underline" | "Block" | "Hollow"; color: number } | null;
};

export type Renderer = {
  draw(s: Scene): void;
  /** Forget every cached glyph (font, size or pixel ratio changed). */
  reset(): void;
  /**
   * A page gets about 16 live WebGL contexts and loses the oldest beyond that, so a hidden tab
   * gives its context back and takes it again (glyphs redrawn on demand) when shown. `wake` is
   * false when the budget (surface.ts `contexts`) has none to spare: it stays asleep.
   */
  sleep(): void;
  wake(): boolean;
  /** The context is there to draw with: not asleep, and not lost or still being restored. */
  ready(): boolean;
};

const BOLD = 1, ITALIC = 2, UNDERLINE = 4, STRIKE = 8, DIM = 16;

const VERTEX = `#version 300 es
uniform vec2 screen;
uniform vec2 tex;
in vec4 rect;
in vec4 src;
in vec4 color;
out vec2 t;
out vec4 c;
flat out int solid;
void main() {
  vec2 corner = vec2(gl_VertexID & 1, gl_VertexID >> 1);
  vec2 p = (rect.xy + corner * rect.zw) / screen * 2.0 - 1.0;
  gl_Position = vec4(p.x, -p.y, 0.0, 1.0);
  t = (src.xy + corner * src.zw) / tex;
  c = vec4(color.rgb * color.a, color.a);
  solid = src.x < 0.0 ? 1 : 0;
}`;

// Solid quads take their colour; textured ones with colour alpha 0 (images, emoji) copy the
// texture; the rest are glyph coverage in the quad's colour. All premultiplied.
const FRAGMENT = `#version 300 es
precision mediump float;
uniform sampler2D img;
in vec2 t;
in vec4 c;
flat in int solid;
out vec4 color;
void main() {
  if (solid == 1) {
    color = c;
    return;
  }
  vec4 s = texture(img, t);
  color = c.a == 0.0 ? s : c * s.a;
}`;

/** Per instance: rect (4 floats), source rect in texture pixels (4 floats, x < 0: solid), RGBA. */
const STRIDE = 36;
const AW = 2048;
const AH = 1024;

/** Where glyphs are drawn before they go up to a tab's atlas; shared, it's only scratch space. */
let scratch: OffscreenCanvasRenderingContext2D | null = null;
function pad(w: number, h: number) {
  if (!scratch || scratch.canvas.width < w || scratch.canvas.height < h) {
    const c = new OffscreenCanvas(Math.max(w, scratch?.canvas.width ?? 0), Math.max(h, scratch?.canvas.height ?? 0));
    scratch = c.getContext("2d", { willReadFrequently: true })!;
  }
  scratch.clearRect(0, 0, w, h);
  return scratch;
}

/** `restored`: the context is back after a loss; draw again. Null without WebGL2, or with no
 *  context to spare (surface.ts `contexts`). */
export function glRenderer(canvas: HTMLCanvasElement, restored: () => void): Renderer | null {
  if (!contexts.room()) return null;
  const gl = canvas.getContext("webgl2", { alpha: true, premultipliedAlpha: true, antialias: false, depth: false });
  if (!gl) return null;
  contexts.take();

  /** Glyph slots by face (style | dark): ASCII by code, the rest by text. */
  let faces: { ascii: Int32Array; other: Map<string, number> }[] = [];
  let su: number[] = [], sv: number[] = [], sw: number[] = [];
  /** Slots holding a colour glyph. */
  let colour: boolean[] = [];
  let penX = 0, penY = 0;

  let atlas: WebGLTexture;
  let uScreen: WebGLUniformLocation | null, uTex: WebGLUniformLocation | null;
  let aRect = 0, aSrc = 0, aColor = 0;
  const images = new Map<ImageBitmap, WebGLTexture>();
  let lost = false;

  function texture(filter: number) {
    const t = gl!.createTexture()!;
    gl!.bindTexture(gl!.TEXTURE_2D, t);
    gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_MIN_FILTER, filter);
    gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_MAG_FILTER, filter);
    gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_WRAP_S, gl!.CLAMP_TO_EDGE);
    gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_WRAP_T, gl!.CLAMP_TO_EDGE);
    return t;
  }

  function init() {
    const g = gl!;
    const shader = (type: number, src: string) => {
      const s = g.createShader(type)!;
      g.shaderSource(s, src);
      g.compileShader(s);
      return s;
    };
    const prog = g.createProgram()!;
    g.attachShader(prog, shader(g.VERTEX_SHADER, VERTEX));
    g.attachShader(prog, shader(g.FRAGMENT_SHADER, FRAGMENT));
    g.linkProgram(prog);
    g.useProgram(prog);
    uScreen = g.getUniformLocation(prog, "screen");
    uTex = g.getUniformLocation(prog, "tex");
    [aRect, aSrc, aColor] = ["rect", "src", "color"].map((name) => g.getAttribLocation(prog, name));
    g.bindVertexArray(g.createVertexArray());
    g.bindBuffer(g.ARRAY_BUFFER, g.createBuffer());
    for (const at of [aRect, aSrc, aColor]) {
      g.enableVertexAttribArray(at);
      g.vertexAttribDivisor(at, 1);
    }
    g.enable(g.BLEND);
    g.blendFunc(g.ONE, g.ONE_MINUS_SRC_ALPHA);
    g.pixelStorei(g.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
    atlas = texture(g.NEAREST);
    g.texStorage2D(g.TEXTURE_2D, 1, g.RGBA8, AW, AH);
    images.clear();
    reset();
  }

  function reset() {
    faces = Array.from({ length: 8 }, () => ({ ascii: new Int32Array(128).fill(-1), other: new Map() }));
    su = [];
    sv = [];
    sw = [];
    colour = [];
    penX = penY = 0;
  }

  canvas.addEventListener("webglcontextlost", (e) => {
    e.preventDefault();
    lost = true;
  });
  canvas.addEventListener("webglcontextrestored", () => {
    lost = false;
    init();
    restored();
  });
  init();
  const lose = gl.getExtension("WEBGL_lose_context");
  let asleep = false;

  // Instance data, grown as needed.
  let data = new ArrayBuffer(STRIDE * 8192);
  let f32 = new Float32Array(data);
  let u32 = new Uint32Array(data);
  let n = 0;
  function push(x: number, y: number, w: number, h: number, sx: number, sy: number, sW: number, sH: number, rgb: number, a: number) {
    if ((n + 1) * STRIDE > data.byteLength) {
      const bigger = new ArrayBuffer(data.byteLength * 2);
      new Uint8Array(bigger).set(new Uint8Array(data));
      data = bigger;
      f32 = new Float32Array(data);
      u32 = new Uint32Array(data);
    }
    const i = (n++ * STRIDE) / 4;
    f32[i] = x;
    f32[i + 1] = y;
    f32[i + 2] = w;
    f32[i + 3] = h;
    f32[i + 4] = sx;
    f32[i + 5] = sy;
    f32[i + 6] = sW;
    f32[i + 7] = sH;
    // Bytes R, G, B, A.
    u32[i + 8] = (((rgb >> 16) & 255) | (rgb & 0xff00) | ((rgb & 255) << 16) | (Math.round(a * 255) << 24)) >>> 0;
  }
  const solid = (x: number, y: number, w: number, h: number, rgb: number, a = 1) => push(x, y, w, h, -1, 0, 0, 0, rgb, a);

  /** The glyph's slot, drawn into the atlas on first use; -1 when the atlas is full. */
  function glyph(s: Scene, text: string, cells: number, face: number, room: number): number {
    const f = faces[face];
    const code = text.length === 1 ? text.charCodeAt(0) : 128;
    const hit = code < 128 ? f.ascii[code] : f.other.get(text);
    if (hit !== undefined && hit >= 0) return hit;

    const width = Math.ceil(cells * s.cw) + 2 * room;
    if (penX + width > AW) {
      penX = 0;
      penY += s.ch;
    }
    if (penY + s.ch > AH) return -1;
    const g = pad(width, s.ch);
    g.font = s.font(face & 3);
    g.fillStyle = face & 4 ? "#000" : "#fff";
    // A wide or unusual glyph is squeezed into its cells, as the 2D renderer does.
    if (code < 128) g.fillText(text, room, s.ascent);
    else g.fillText(text, room, s.ascent, Math.round(cells * s.cw));
    const img = g.getImageData(0, 0, width, s.ch);
    let isColour = false;
    if (code >= 128) {
      const d = img.data;
      for (let i = 0; i < d.length && !isColour; i += 4) isColour = d[i + 3] > 0 && (d[i] !== d[i + 1] || d[i + 1] !== d[i + 2]);
    }
    gl!.texSubImage2D(gl!.TEXTURE_2D, 0, penX, penY, gl!.RGBA, gl!.UNSIGNED_BYTE, img);
    const slot = su.length;
    su.push(penX);
    sv.push(penY);
    sw.push(width);
    colour.push(isColour);
    if (code < 128) f.ascii[code] = slot;
    else f.other.set(text, slot);
    penX += width;
    return slot;
  }

  /** The frame's quads: [end of backgrounds, end of text]; null if the atlas filled up. */
  function build(s: Scene): [number, number] | null {
    const { cw, ch, rows } = s;
    const x = (col: number) => Math.round(col * cw);
    // Room either side of a glyph for italics and overhangs.
    const room = Math.ceil(cw / 2);
    const line = Math.max(1, Math.round(s.dpr));
    n = 0;
    for (let y = 0; y < rows.length; y++) {
      for (const r of rows[y]) if (r[4] !== null) solid(x(r[0]), y * ch, x(r[0] + r[1]) - x(r[0]), ch, r[4]);
    }
    const backgrounds = n;
    let lastFg = -1;
    let dark = 0;
    for (let y = 0; y < rows.length; y++) {
      const top = y * ch;
      for (const [col, w, text, fg, , flags] of rows[y]) {
        if (fg !== lastFg) {
          lastFg = fg;
          dark = 0.299 * (fg >> 16) + 0.587 * ((fg >> 8) & 255) + 0.114 * (fg & 255) < 128 ? 4 : 0;
        }
        const face = (flags & (BOLD | ITALIC)) | dark;
        const a = flags & DIM ? 0.6 : 1;
        const quad = (g: number, at: number) =>
          colour[g] ? push(at - room, top, sw[g], ch, su[g], sv[g], sw[g], ch, 0, 0) : push(at - room, top, sw[g], ch, su[g], sv[g], sw[g], ch, fg, a);
        if (w > 1 && text.length === w && text.charCodeAt(1) < 128) {
          // Plain ASCII run: one quad per character.
          for (let i = 0; i < w; i++) {
            if (text.charCodeAt(i) === 32) continue;
            const g = glyph(s, text[i], 1, face, room);
            if (g < 0) return null;
            quad(g, x(col + i));
          }
        } else if (text !== " ") {
          const g = glyph(s, text, w, face, room);
          if (g < 0) return null;
          quad(g, x(col));
        }
        if (flags & UNDERLINE) solid(x(col), top + ch - line, x(col + w) - x(col), line, fg, a);
        if (flags & STRIKE) solid(x(col), top + Math.round(ch / 2), x(col + w) - x(col), line, fg, a);
      }
    }
    const c = s.cursor;
    if (c) {
      const [px, py, cwr, bar] = [x(c.x), c.y * ch, Math.round(cw), Math.round(2 * s.dpr)];
      if (c.shape === "Bar") solid(px, py, bar, ch, c.color);
      else if (c.shape === "Underline") solid(px, py + ch - bar, cwr, bar, c.color);
      else if (c.shape === "Block") solid(px, py, cwr, ch, c.color, 0.7);
      else {
        solid(px, py, cwr, line, c.color);
        solid(px, py + ch - line, cwr, line, c.color);
        solid(px, py, line, ch, c.color);
        solid(px + cwr - line, py, line, ch, c.color);
      }
    }
    return [backgrounds, n];
  }

  function range(from: number, to: number) {
    if (to <= from) return;
    const g = gl!;
    const base = from * STRIDE;
    g.vertexAttribPointer(aRect, 4, g.FLOAT, false, STRIDE, base);
    g.vertexAttribPointer(aSrc, 4, g.FLOAT, false, STRIDE, base + 16);
    g.vertexAttribPointer(aColor, 4, g.UNSIGNED_BYTE, true, STRIDE, base + 32);
    g.drawArraysInstanced(g.TRIANGLE_STRIP, 0, 4, to - from);
  }

  return {
    reset,
    sleep() {
      if (asleep) return;
      if (!lost) lose?.loseContext();
      contexts.give();
      asleep = true;
    },
    wake() {
      if (!asleep) return true;
      if (!contexts.room()) return false;
      contexts.take();
      // Also brings back a context the browser took to make room for another.
      if (lost) lose?.restoreContext();
      asleep = false;
      return true;
    },
    ready: () => !asleep && !lost,
    draw(s) {
      if (lost) return;
      gl.bindTexture(gl.TEXTURE_2D, atlas);
      let built = build(s);
      if (!built) {
        // ponytail: a full atlas starts over (slots are rewritten as they're used again); a
        // second atlas page would avoid the redraw burst if a screen ever needs more than one.
        reset();
        built = build(s);
        if (!built) return;
      }
      const [backgrounds, text] = built;
      // Images go in after the text, in one upload, and are drawn between the two passes.
      for (const p of s.images) {
        const [u0, v0, u1, v1] = p.uv;
        const [x0, y0, x1, y1] = p.dst;
        const { width: W, height: H } = p.bmp;
        push(x0 * s.cw, y0 * s.ch, (x1 - x0) * s.cw, (y1 - y0) * s.ch, u0 * W, v0 * H, (u1 - u0) * W, (v1 - v0) * H, 0, 0);
      }
      gl.bufferData(gl.ARRAY_BUFFER, new Uint8Array(data, 0, n * STRIDE), gl.STREAM_DRAW);

      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform2f(uScreen, canvas.width, canvas.height);
      gl.uniform2f(uTex, AW, AH);
      range(0, backgrounds);
      const used = new Set<ImageBitmap>();
      s.images.forEach((p, i) => {
        let t = images.get(p.bmp);
        if (!t) {
          t = texture(gl.LINEAR);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, p.bmp);
          images.set(p.bmp, t);
        }
        used.add(p.bmp);
        gl.bindTexture(gl.TEXTURE_2D, t);
        gl.uniform2f(uTex, p.bmp.width, p.bmp.height);
        range(text + i, text + i + 1);
      });
      for (const [bmp, t] of images) {
        if (!used.has(bmp)) {
          gl.deleteTexture(t);
          images.delete(bmp);
        }
      }
      gl.bindTexture(gl.TEXTURE_2D, atlas);
      gl.uniform2f(uTex, AW, AH);
      range(backgrounds, text);
    },
  };
}
