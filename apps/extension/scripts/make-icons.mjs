// Renders the Riptide mark (see icons/riptide.svg) to PNG at the sizes the manifest needs.
// No dependencies: shapes are rasterised with 4x4 supersampling and encoded with node:zlib.
//
//   node scripts/make-icons.mjs [outDir]      (default: icons/)
//   import { writeIcons } from './make-icons.mjs'   (used by build.mjs per target)
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ICON_SIZES = [16, 32, 48, 128];

const TEAL = [0x19, 0xc6, 0xb5];
const INK = [0x06, 0x20, 0x1d];
const SUPERSAMPLE = 4;

// ---- shapes in unit coordinates (0..1), matching icons/riptide.svg -------------------------

function inRoundedSquare(u, v, r = 0.22) {
  if (u < 0 || v < 0 || u > 1 || v > 1) return false;
  const cx = u < r ? r : u > 1 - r ? 1 - r : u;
  const cy = v < r ? r : v > 1 - r ? 1 - r : v;
  return (u - cx) ** 2 + (v - cy) ** 2 <= r * r;
}

function inRect(u, v, x0, y0, x1, y1) {
  return u >= x0 && u <= x1 && v >= y0 && v <= y1;
}

function inBowl(u, v) {
  const cx = 0.47;
  const cy = 0.375;
  if (u < cx) return false;
  const d = Math.hypot(u - cx, v - cy);
  return d >= 0.055 && d <= 0.155;
}

function inLeg(u, v) {
  // Capsule from (0.44, 0.52) to (0.66, 0.78), half-width 0.06, flat at the baseline.
  if (v > 0.78) return false;
  const ax = 0.44;
  const ay = 0.52;
  const bx = 0.66;
  const by = 0.78;
  const abx = bx - ax;
  const aby = by - ay;
  const t = Math.max(0, Math.min(1, ((u - ax) * abx + (v - ay) * aby) / (abx * abx + aby * aby)));
  const px = ax + t * abx;
  const py = ay + t * aby;
  return Math.hypot(u - px, v - py) <= 0.06;
}

function inGlyph(u, v) {
  return (
    inRect(u, v, 0.3, 0.22, 0.42, 0.78) ||
    inRect(u, v, 0.3, 0.22, 0.47, 0.32) ||
    inRect(u, v, 0.3, 0.43, 0.47, 0.53) ||
    inBowl(u, v) ||
    inLeg(u, v)
  );
}

// ---- rasteriser -----------------------------------------------------------------------------

export function renderRgba(size) {
  const out = Buffer.alloc(size * size * 4);
  const samples = SUPERSAMPLE * SUPERSAMPLE;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (let sy = 0; sy < SUPERSAMPLE; sy++) {
        for (let sx = 0; sx < SUPERSAMPLE; sx++) {
          const u = (x + (sx + 0.5) / SUPERSAMPLE) / size;
          const v = (y + (sy + 0.5) / SUPERSAMPLE) / size;
          if (!inRoundedSquare(u, v)) continue;
          const c = inGlyph(u, v) ? INK : TEAL;
          r += c[0];
          g += c[1];
          b += c[2];
          a += 1;
        }
      }
      const i = (y * size + x) * 4;
      if (a > 0) {
        out[i] = Math.round(r / a);
        out[i + 1] = Math.round(g / a);
        out[i + 2] = Math.round(b / a);
        out[i + 3] = Math.round((a / samples) * 255);
      }
    }
  }
  return out;
}

// ---- PNG encoder ----------------------------------------------------------------------------

const CRC_TABLE = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

export function encodePng(size, rgba) {
  const stride = size * 4;
  const raw = Buffer.alloc((stride + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (stride + 1)] = 0; // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** Write `icon-<size>.png` for every size into `dir`. Returns the file names. */
export function writeIcons(dir) {
  mkdirSync(dir, { recursive: true });
  const names = [];
  for (const size of ICON_SIZES) {
    const name = `icon-${size}.png`;
    writeFileSync(join(dir, name), encodePng(size, renderRgba(size)));
    names.push(name);
  }
  return names;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const root = dirname(dirname(fileURLToPath(import.meta.url)));
  const dir = process.argv[2] ? process.argv[2] : join(root, 'icons');
  const names = writeIcons(dir);
  console.log(`icons: wrote ${names.join(', ')} to ${dir}`);
}
