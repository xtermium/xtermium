// Terminal images (kitty, OSC 1337, sixel), still or animated, and session icons.

/** Frames and when each one ends, in ms from the start of a loop. */
export type Pic = { frames: ImageBitmap[]; ends: number[]; total: number; start: number };

// Browsers treat GIF delays of 10 ms or less as 100 ms; so do we.
const delay = (ms: number) => (ms <= 10 ? 100 : ms);

function pic(frames: ImageBitmap[], durations: number[]): Pic {
  let t = 0;
  const ends = durations.map((d) => (t += delay(d)));
  return { frames, ends, total: t, start: performance.now() };
}

const rgba = (data: Uint8Array<ArrayBuffer>, w: number, h: number) =>
  createImageBitmap(new ImageData(new Uint8ClampedArray(data.buffer, data.byteOffset, w * h * 4), w, h));

/** Decodes an encoded file, every frame of it when it is animated (GIF, APNG, WebP). */
async function encoded(data: Uint8Array<ArrayBuffer>): Promise<Pic> {
  const type = sniff(data);
  if (type && "ImageDecoder" in window && (await ImageDecoder.isTypeSupported(type))) {
    const dec = new ImageDecoder({ data, type });
    try {
      await dec.tracks.ready;
      const n = dec.tracks.selectedTrack?.animated ? dec.tracks.selectedTrack.frameCount : 1;
      const frames: ImageBitmap[] = [];
      const durations: number[] = [];
      for (let i = 0; i < n; i++) {
        const { image } = await dec.decode({ frameIndex: i });
        frames.push(await createImageBitmap(image));
        durations.push((image.duration ?? 0) / 1000);
        image.close();
      }
      return pic(frames, durations);
    } finally {
      dec.close();
    }
  }
  return pic([await createImageBitmap(new Blob([data]))], [0]);
}

function sniff(b: Uint8Array): string | null {
  const ascii = (at: number, s: string) => [...s].every((c, i) => b[at + i] === c.charCodeAt(0));
  if (ascii(0, "GIF8")) return "image/gif";
  if (ascii(1, "PNG")) return "image/png";
  if (ascii(0, "RIFF") && ascii(8, "WEBP")) return "image/webp";
  return null;
}

/**
 * Decodes an image message body: format 0 = RGBA, 1 = encoded file,
 * 2 = `[frames u32][duration ms u32 each][RGBA each]`.
 */
export async function decode(format: number, w: number, h: number, data: Uint8Array<ArrayBuffer>): Promise<Pic> {
  if (format === 0) return pic([await rgba(data, w, h)], [0]);
  if (format === 1) return encoded(data);
  const dv = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const n = dv.getUint32(0, true);
  const durations = Array.from({ length: n }, (_, i) => dv.getUint32(4 + 4 * i, true));
  const size = w * h * 4;
  const at = 4 + 4 * n;
  const frames = await Promise.all(durations.map((_, i) => rgba(data.subarray(at + i * size, at + (i + 1) * size), w, h)));
  return pic(frames, durations);
}

/** The frame to show at time `now`, and ms until the next one (Infinity if still). */
export function frameAt(p: Pic, now: number): [ImageBitmap, number] {
  if (p.frames.length === 1) return [p.frames[0], Infinity];
  const t = (now - p.start) % p.total;
  const i = p.ends.findIndex((e) => t < e);
  return [p.frames[i], p.ends[i] - t];
}

export const close = (p: Pic) => p.frames.forEach((f) => f.close());

/** A session or folder icon (Appearance.svelte): PNG, JPEG, WebP and ICO become a 64 px PNG;
 *  animated GIFs and SVGs are kept. */
export async function shrinkIcon(dataUrl: string): Promise<string> {
  if (/^data:image\/(gif|svg)/.test(dataUrl)) return dataUrl;
  const img = new Image();
  img.src = dataUrl;
  await img.decode();
  const scale = Math.min(1, 64 / Math.max(img.naturalWidth, img.naturalHeight));
  const c = document.createElement("canvas");
  [c.width, c.height] = [Math.max(1, Math.round(img.naturalWidth * scale)), Math.max(1, Math.round(img.naturalHeight * scale))];
  c.getContext("2d")!.drawImage(img, 0, 0, c.width, c.height);
  return c.toDataURL("image/png");
}
