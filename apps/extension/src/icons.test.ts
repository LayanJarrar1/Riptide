import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Tests run with the extension root as the working directory (see scripts/test.mjs).

// SHA-256 of the files exactly as delivered by the brand kit (02-icon/png/riptide-app-icon-<size>.png
// and 02-icon/svg/riptide-app-icon.svg). Changing the logo means replacing the files and these values.
const BRAND_SHA256: Record<string, string> = {
  'icon-16.png': '6fbbf4fcb97b867e250236822c12f4ab7cd9212e0090ebfeaf0530d95c275dbe',
  'icon-32.png': 'c7cae46aeeff20d8003852d8ee392b96d9695aaa0b91400798e8e83e81ba038a',
  'icon-48.png': '31bd873b9c7988df7bdca0a94ea385dd09142ef855f0526850f0bd6e4ea8b445',
  'icon-128.png': '3ed0fe87105e3ea9da3f09e4c8136c8b4e7607bd6917c329dd01e5527eccbb11',
};
const BRAND_SVG_SHA256 = '402c0f7c3322c0555d142f4a6d3b8ec7f5f68a410120da08630f01588a0b935c';

const ICON_SIZES = [16, 32, 48, 128];

const sha = (buf: Buffer): string => createHash('sha256').update(buf).digest('hex');

test('manifest references exactly the brand icon sizes', () => {
  const base = JSON.parse(readFileSync(join('manifest', 'base.json'), 'utf8')) as { icons: Record<string, string> };
  assert.deepEqual(Object.keys(base.icons).map(Number).sort((a, b) => a - b), ICON_SIZES);
});

test('built icons are byte-identical to the brand kit and have the right dimensions', () => {
  const dir = mkdtempSync(join(tmpdir(), 'rt-icons-'));
  try {
    // the real build step, run as a process so the test exercises exactly what the build does
    const run = spawnSync(process.execPath, [join('scripts', 'make-icons.mjs'), dir], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    for (const size of ICON_SIZES) {
      const name = `icon-${size}.png`;
      const buf = readFileSync(join(dir, name));
      assert.equal(sha(buf), BRAND_SHA256[name], `${name} differs from the brand kit`);
      assert.equal(buf.readUInt32BE(16), size, `${name} width`);
      assert.equal(buf.readUInt32BE(20), size, `${name} height`);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('icons/riptide.svg and brand/riptide-app-icon.svg are the brand-kit app icon', () => {
  assert.equal(sha(readFileSync(join('icons', 'riptide.svg'))), BRAND_SVG_SHA256);
  assert.equal(sha(readFileSync(join('brand', 'riptide-app-icon.svg'))), BRAND_SVG_SHA256);
});
