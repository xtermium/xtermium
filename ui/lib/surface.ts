// A remote desktop's picture: a desktop-sized image that changed areas are written into, shown
// on a canvas sized to the desktop (CSS scales it to the pane). WebGL2 when the system has it:
// each area is one texture upload and the screen is one draw per display refresh. Else Canvas2D.

export type Surface = {
  /** New desktop size; the picture starts black. */
  size(w: number, h: number): void;
  /** RGBA pixels of a changed area. */
  put(x: number, y: number, w: number, h: number, px: Uint8Array): void;
  /** Show what was put since the last call; cheap to call once per animation frame. */
  show(): void;
  readonly kind: "webgl2" | "canvas2d";
  /** Give the GPU context back (the tab closed). */
  drop(): void;
};

/** A page gets about 16 live WebGL contexts and loses the oldest beyond that (a terminal or
 *  desktop goes blank). Terminals (glyphs.ts) and desktops share a budget below that; past it
 *  they draw with Canvas2D, slower but fine. */
const BUDGET = 14;
let live = 0;
export const contexts = {
  room: () => live < BUDGET,
  take: () => void live++,
  give: () => void (live = Math.max(0, live - 1)),
};

const GL: WebGLContextAttributes = { alpha: false, antialias: false, depth: false, desynchronized: true };

export function surface(canvas: HTMLCanvasElement): Surface {
  const gl = contexts.room() ? canvas.getContext("webgl2", GL) : null;
  if (!gl) return canvas2d(canvas);
  contexts.take();
  return webgl(canvas, gl);
}

const VERTEX = `#version 300 es
out vec2 uv;
void main() {
  // One triangle covering the canvas; texture row 0 (the desktop's top) at the top.
  vec2 p = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? -3.0 : 1.0);
  uv = vec2((p.x + 1.0) / 2.0, (1.0 - p.y) / 2.0);
  gl_Position = vec4(p, 0.0, 1.0);
}`;

const FRAGMENT = `#version 300 es
precision mediump float;
uniform sampler2D desktop;
in vec2 uv;
out vec4 color;
void main() {
  color = vec4(texture(desktop, uv).rgb, 1.0);
}`;

function webgl(canvas: HTMLCanvasElement, gl: WebGL2RenderingContext): Surface {
  let w = 0;
  let h = 0;
  let dirty = false;

  const shader = (type: number, src: string) => {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    return s;
  };
  const program = gl.createProgram()!;
  gl.attachShader(program, shader(gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(program, shader(gl.FRAGMENT_SHADER, FRAGMENT));
  gl.linkProgram(program);
  gl.useProgram(program);
  gl.bindVertexArray(gl.createVertexArray());
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);

  let texture: WebGLTexture | null = null;
  function allocate() {
    if (texture) gl.deleteTexture(texture);
    texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, texture);
    // The canvas is desktop-sized, so texels map 1:1 to canvas pixels.
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, Math.max(1, w), Math.max(1, h));
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    dirty = false;
  }

  // ponytail: after a GPU reset the picture comes back area by area as the server repaints;
  // keep a CPU copy (or ask the server for a full refresh) if that proves too visible.
  canvas.addEventListener("webglcontextlost", (e) => e.preventDefault());
  canvas.addEventListener("webglcontextrestored", () => {
    Object.assign(api, webgl(canvas, canvas.getContext("webgl2", GL)!));
    api.size(w, h);
  });

  const api: Surface = {
    kind: "webgl2",
    size(nw, nh) {
      [w, h] = [nw, nh];
      [canvas.width, canvas.height] = [w, h];
      allocate();
    },
    put(x, y, pw, ph, px) {
      if (!texture || x + pw > w || y + ph > h) return;
      gl.texSubImage2D(gl.TEXTURE_2D, 0, x, y, pw, ph, gl.RGBA, gl.UNSIGNED_BYTE, px);
      dirty = true;
    },
    show() {
      if (!dirty) return;
      dirty = false;
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    },
    drop() {
      gl.getExtension("WEBGL_lose_context")?.loseContext();
      contexts.give();
    },
  };
  return api;
}

function canvas2d(canvas: HTMLCanvasElement): Surface {
  const c = canvas.getContext("2d", { alpha: false })!;
  return {
    kind: "canvas2d",
    size(w, h) {
      [canvas.width, canvas.height] = [w, h];
      c.fillStyle = "#000";
      c.fillRect(0, 0, w, h);
    },
    put(x, y, w, h, px) {
      if (w && h && px.length === w * h * 4) c.putImageData(new ImageData(new Uint8ClampedArray(px.buffer as ArrayBuffer, px.byteOffset, px.length), w, h), x, y);
    },
    show() {},
    drop() {},
  };
}
