// Terminal background shaders (Settings › Terminal). One WebGL 2 context per window compiles a
// shader once (programs kept by source; the GPU driver also caches compiled programs between
// runs) and draws one frame for every terminal showing it, at the size of the biggest of them;
// each terminal copies its middle part into a canvas of its own. WebGL 2 is in WebView2, WebKit
// and WebKitGTK alike, so this runs wherever xtermium does.
// ponytail: fixed at half resolution and 30 frames a second; settings for both if a shader needs more.
import { _ } from "./i18n.svelte";

const SCALE = 0.5;
const FPS = 30;

// Shadertoy's inputs. Its textures (iChannel0-3) aren't fed: they read black.
const PRELUDE = `#version 300 es
precision highp float;
uniform vec3 iResolution;
uniform float iTime;
uniform float iTimeDelta;
uniform int iFrame;
uniform vec4 iMouse;
uniform vec4 iDate;
uniform sampler2D iChannel0, iChannel1, iChannel2, iChannel3;
uniform vec3 iChannelResolution[4];
out vec4 xtermiumColor;
`;
const MAIN = "\nvoid main() { mainImage(xtermiumColor, gl_FragCoord.xy); xtermiumColor.a = 1.0; }\n";
const VERTEX_300 = "#version 300 es\nin vec2 p;\nvoid main() { gl_Position = vec4(p, 0.0, 1.0); }";
const VERTEX_100 = "attribute vec2 p;\nvoid main() { gl_Position = vec4(p, 0.0, 1.0); }";
/** Uniforms set when a shader has them: Shadertoy's, GLSL Sandbox's and The Book of Shaders'. */
const UNIFORMS = ["iResolution", "iTime", "iTimeDelta", "iFrame", "iMouse", "iDate", "time", "resolution", "mouse", "u_time", "u_resolution", "u_mouse"];

type Program = { prog: WebGLProgram; at: Record<string, WebGLUniformLocation | null> };

const canvas = document.createElement("canvas");
let gl: WebGL2RenderingContext | null = null;
/** Compiled programs, or why one didn't compile, by source. */
const programs = new Map<string, Program | string>();

/** The shader as compiled: a Shadertoy one (`mainImage`) inside its inputs, others as they
 *  are. And how many lines came before the user's, for error messages. */
function fragment(src: string): [string, number] {
  if (/\bmainImage\s*\(/.test(src) && !/\bvoid\s+main\s*\(/.test(src)) return [PRELUDE + src + MAIN, PRELUDE.split("\n").length - 1];
  return [src, 0];
}

function context(): WebGL2RenderingContext | null {
  if (gl && !gl.isContextLost()) return gl;
  programs.clear();
  gl = canvas.getContext("webgl2", { alpha: false, antialias: false, depth: false, stencil: false });
  if (!gl) return null;
  // One triangle over the whole frame.
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  return gl;
}

/** Compiles `src` (once: later calls get the same program), or says why it doesn't compile. */
export function compile(src: string): Program | string {
  const g = context();
  if (!g) return _("Shaders need WebGL 2, which this system doesn't have.");
  const known = programs.get(src);
  if (known) return known;
  const [fs, before] = fragment(src);
  const shader = (type: number, text: string) => {
    const s = g.createShader(type)!;
    g.shaderSource(s, text);
    g.compileShader(s);
    return s;
  };
  const v = shader(g.VERTEX_SHADER, /^\s*#version\s+300\s+es/.test(fs) ? VERTEX_300 : VERTEX_100);
  const f = shader(g.FRAGMENT_SHADER, fs);
  const prog = g.createProgram()!;
  g.attachShader(prog, v);
  g.attachShader(prog, f);
  g.bindAttribLocation(prog, 0, "p");
  g.linkProgram(prog);
  let made: Program | string;
  if (g.getProgramParameter(prog, g.LINK_STATUS)) {
    made = { prog, at: Object.fromEntries(UNIFORMS.map((n) => [n, g.getUniformLocation(prog, n)])) };
  } else {
    // "ERROR: 0:12: …" counts the lines added before the user's.
    const log = g.getShaderInfoLog(f) || g.getProgramInfoLog(prog) || "";
    made = log.replace(/\b0:(\d+)/g, (_m, n) => _("line {n}", { n: Math.max(1, +n - before) })).trim() || _("It doesn't compile.");
    g.deleteProgram(prog);
  }
  g.deleteShader(v);
  g.deleteShader(f);
  programs.set(src, made);
  return made;
}

type View = { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D; src: string };
const views = new Set<View>();
let raf = 0;
let last = 0;
let frames = 0;
const start = performance.now();

/** Shows shader `src` in `target` (a terminal's background) until the returned function runs. */
export function show(target: HTMLCanvasElement, src: string): () => void {
  const view = { canvas: target, ctx: target.getContext("2d")!, src };
  views.add(view);
  raf ||= requestAnimationFrame(tick);
  return () => {
    views.delete(view);
    if (views.size) return;
    cancelAnimationFrame(raf);
    raf = 0;
  };
}

function tick(now: number) {
  raf = requestAnimationFrame(tick);
  if (now - last < 1000 / FPS - 2) return;
  const dt = (now - last) / 1000;
  last = now;
  // Terminals on screen (hidden ones have no size), by shader.
  const by = new Map<string, View[]>();
  for (const v of views) if (v.canvas.clientWidth && v.canvas.clientHeight) by.set(v.src, [...(by.get(v.src) ?? []), v]);
  const g = by.size ? context() : null;
  if (!g) return;
  const k = devicePixelRatio * SCALE;
  const t = (now - start) / 1000;
  const d = new Date();
  frames++;
  for (const [src, list] of by) {
    const p = compile(src);
    if (typeof p === "string") continue;
    const w = Math.ceil(Math.max(...list.map((v) => v.canvas.clientWidth)) * k);
    const h = Math.ceil(Math.max(...list.map((v) => v.canvas.clientHeight)) * k);
    if (canvas.width !== w || canvas.height !== h) [canvas.width, canvas.height] = [w, h];
    g.viewport(0, 0, w, h);
    g.useProgram(p.prog);
    const u = p.at;
    g.uniform3f(u.iResolution, w, h, 1);
    for (const n of ["iTime", "time", "u_time"]) g.uniform1f(u[n], t);
    g.uniform1f(u.iTimeDelta, dt);
    g.uniform1i(u.iFrame, frames);
    g.uniform4f(u.iMouse, 0, 0, 0, 0);
    g.uniform4f(u.iDate, d.getFullYear(), d.getMonth(), d.getDate(), d.getHours() * 3600 + d.getMinutes() * 60 + d.getSeconds());
    for (const n of ["resolution", "u_resolution"]) g.uniform2f(u[n], w, h);
    for (const n of ["mouse", "u_mouse"]) g.uniform2f(u[n], 0.5, 0.5);
    g.drawArrays(g.TRIANGLES, 0, 3);
    // Copied in the same frame: the drawing buffer is cleared once it's shown.
    for (const v of list) {
      const [vw, vh] = [Math.ceil(v.canvas.clientWidth * k), Math.ceil(v.canvas.clientHeight * k)];
      if (v.canvas.width !== vw || v.canvas.height !== vh) [v.canvas.width, v.canvas.height] = [vw, vh];
      v.ctx.drawImage(canvas, (w - vw) >> 1, (h - vh) >> 1, vw, vh, 0, 0, vw, vh);
    }
  }
}
