#!/usr/bin/env node
// Generates the 1024x1024 source icon for the desktop app as a PNG, using only
// Node built-ins (zlib + a hand-rolled chunk/CRC writer). No native modules.
//
//   node scripts/make-icon.mjs [out.png]
//
// The artwork is a rounded teal square (#19C6B5) with a near-black downward
// arrow (the "pull everything down to you" idea). Anti-aliased by 4x4
// supersampling. Run `npx tauri icon <out.png>` afterwards to regenerate
// src-tauri/icons/*.

import { deflateSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const SIZE = 1024;
const SS = 4; // supersampling factor per axis
const OUT = resolve(process.argv[2] ?? 'src-tauri/app-icon.png');

const TEAL = [0x19, 0xc6, 0xb5];
const INK = [0x0b, 0x0f, 0x12];

/** Signed distance to a rounded rectangle centred at (cx, cy). */
function sdRoundRect(x, y, cx, cy, hw, hh, r) {
  const dx = Math.abs(x - cx) - (hw - r);
  const dy = Math.abs(y - cy) - (hh - r);
  const ox = Math.max(dx, 0);
  const oy = Math.max(dy, 0);
  return Math.hypot(ox, oy) + Math.min(Math.max(dx, dy), 0) - r;
}

/** Signed distance to an axis-aligned rectangle given by its bounds. */
function sdRect(x, y, x0, y0, x1, y1) {
  const cx = (x0 + x1) / 2;
  const cy = (y0 + y1) / 2;
  const dx = Math.abs(x - cx) - (x1 - x0) / 2;
  const dy = Math.abs(y - cy) - (y1 - y0) / 2;
  return Math.hypot(Math.max(dx, 0), Math.max(dy, 0)) + Math.min(Math.max(dx, dy), 0);
}

/** True when (x, y) is inside the downward-pointing triangle (apex at the bottom). */
function inTriangle(x, y, apexX, apexY, halfBase, height) {
  const top = apexY - height;
  if (y < top || y > apexY) return false;
  const t = (apexY - y) / height; // 1 at the top edge, 0 at the apex
  return Math.abs(x - apexX) <= halfBase * t;
}

/** Returns [r, g, b, a] for a sample point in [0, SIZE) space. */
function sample(x, y) {
  const c = SIZE / 2;
  const half = SIZE * 0.5 * 0.92; // leave transparent padding around the plate
  const radius = SIZE * 0.22;
  if (sdRoundRect(x, y, c, c, half, half, radius) > 0) return [0, 0, 0, 0];

  // Arrow: shaft + head + a baseline bar underneath.
  const shaftW = SIZE * 0.11;
  const shaftTop = SIZE * 0.26;
  const headHalf = SIZE * 0.23;
  const headH = SIZE * 0.2;
  const apexY = SIZE * 0.66;
  const barTop = SIZE * 0.72;
  const barBottom = SIZE * 0.79;
  const barHalf = SIZE * 0.27;

  const inShaft = sdRect(x, y, c - shaftW, shaftTop, c + shaftW, apexY - headH + 2) <= 0;
  const inHead = inTriangle(x, y, c, apexY, headHalf, headH);
  const inBar = sdRoundRect(x, y, c, (barTop + barBottom) / 2, barHalf, (barBottom - barTop) / 2, SIZE * 0.03) <= 0;

  if (inShaft || inHead || inBar) return [...INK, 255];
  return [...TEAL, 255];
}

function render() {
  const stride = SIZE * 4 + 1; // + filter byte
  const raw = Buffer.alloc(stride * SIZE);
  const inv = 1 / (SS * SS);
  for (let py = 0; py < SIZE; py++) {
    raw[py * stride] = 0; // filter: None
    for (let px = 0; px < SIZE; px++) {
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const [sr, sg, sb, sa] = sample(px + (sx + 0.5) / SS, py + (sy + 0.5) / SS);
          // premultiplied accumulate, then un-premultiply
          r += sr * sa;
          g += sg * sa;
          b += sb * sa;
          a += sa;
        }
      }
      const o = py * stride + 1 + px * 4;
      if (a === 0) {
        raw[o] = raw[o + 1] = raw[o + 2] = raw[o + 3] = 0;
      } else {
        raw[o] = Math.round(r / a);
        raw[o + 1] = Math.round(g / a);
        raw[o + 2] = Math.round(b / a);
        raw[o + 3] = Math.round(a * inv);
      }
    }
  }
  return raw;
}

// ---- PNG encoding -----------------------------------------------------------

const CRC_TABLE = new Uint32Array(256);
for (let n = 0; n < 256; n++) {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  CRC_TABLE[n] = c >>> 0;
}

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeAndData = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(typeAndData), 0);
  return Buffer.concat([len, typeAndData, crc]);
}

function encodePng(raw) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(SIZE, 0);
  ihdr.writeUInt32BE(SIZE, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: RGBA
  ihdr[10] = 0; // compression
  ihdr[11] = 0; // filter
  ihdr[12] = 0; // interlace
  return Buffer.concat([
    sig,
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const png = encodePng(render());
writeFileSync(OUT, png);
console.log(`wrote ${OUT} (${png.length} bytes, ${SIZE}x${SIZE} RGBA)`);
