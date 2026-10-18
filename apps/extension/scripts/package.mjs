// Packages the store uploads from a fresh build:
//
//   dist/riptide-chrome-<ver>.zip    Chrome Web Store
//   dist/riptide-edge-<ver>.zip      Microsoft Edge Add-ons
//   dist/riptide-firefox-<ver>.zip   Firefox AMO
//   dist/riptide-source-<ver>.zip    AMO source-code review (git-tracked sources + build steps)
//
//   node scripts/package.mjs          (run through `npm run package`, which builds first)
//
// Store zips have manifest.json at the root, no source maps, and no `key` field: the Chrome Web
// Store refuses a new item whose manifest carries `key` ("key field is not allowed in manifest")
// and assigns its own id; Edge Add-ons asks for the same on the first upload. The unpacked
// builds in dist/<target>/ keep the dev key so their id stays stable for local testing.
// Entries use a fixed timestamp and sorted order so the same sources give byte-identical zips.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateRawSync } from 'node:zlib';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const dist = join(root, 'dist');
const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
const STORE_TARGETS = ['chrome', 'edge', 'firefox'];

// ---- minimal deterministic zip writer (stored names are UTF-8, data is deflated) ------------

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// 2024-01-01 00:00 in DOS format.
const DOS_TIME = 0;
const DOS_DATE = ((2024 - 1980) << 9) | (1 << 5) | 1;

/** @param {{name: string, data: Buffer}[]} entries */
function zip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const { name, data } of [...entries].sort((a, b) => a.name.localeCompare(b.name))) {
    const nameBuf = Buffer.from(name, 'utf8');
    const packed = deflateRawSync(data, { level: 9 });
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(8, 8); // deflate
    local.writeUInt16LE(DOS_TIME, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4); // made by
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(8, 10);
    central.writeUInt16LE(DOS_TIME, 12);
    central.writeUInt16LE(DOS_DATE, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(packed.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, packed);
    centrals.push(central, nameBuf);
    offset += local.length + nameBuf.length + packed.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

const problems = [];

// ---- store zips ----------------------------------------------------------------------------

for (const target of STORE_TARGETS) {
  const dir = join(dist, target);
  const entries = [];
  for (const file of walk(dir)) {
    const name = relative(dir, file).split(sep).join('/');
    if (name.endsWith('.map')) continue;
    let data = readFileSync(file);
    if (name === 'manifest.json') {
      const manifest = JSON.parse(data.toString('utf8'));
      if (manifest.version !== version) problems.push(`${target}: manifest ${manifest.version} != package ${version}`);
      delete manifest.key;
      data = Buffer.from(JSON.stringify(manifest, null, 2) + '\n', 'utf8');
    }
    if (/sourceMappingURL=/.test(name.endsWith('.js') ? data.toString('utf8') : '')) {
      problems.push(`${target}: ${name} references a source map`);
    }
    entries.push({ name, data });
  }
  if (!entries.some((e) => e.name === 'manifest.json')) problems.push(`${target}: no manifest.json at the zip root`);
  const out = join(dist, `riptide-${target}-${version}.zip`);
  writeFileSync(out, zip(entries));
  console.log(`zip  ${relative(root, out)} (${entries.length} files, ${statSync(out).size} bytes)`);
}

// ---- AMO source zip ------------------------------------------------------------------------

const BUILD_STEPS = `# Riptide extension: build from source

These are the exact sources of riptide-firefox-${version}.zip (and the Chrome/Edge zips).
Nothing is minified or obfuscated; esbuild only bundles the TypeScript modules and
webextension-polyfill into one IIFE per entry point.

Requirements: Node.js 20 or newer, npm 10. Any OS.

    npm ci
    npm run package

Output:

    dist/firefox/                      the unpacked Firefox build
    dist/riptide-firefox-${version}.zip   identical to the uploaded file

Steps the build runs (package.json "build" + scripts/package.mjs):
1. scripts/make-icons.mjs renders icons/riptide.svg to PNG (no dependencies).
2. scripts/build.mjs merges manifest/base.json + manifest/firefox.json, copies _locales and the
   popup/options HTML/CSS, and bundles src/*.ts with esbuild (versions pinned in
   package-lock.json).
3. scripts/verify-dist.mjs checks the manifest and the referenced files.
4. scripts/package.mjs writes the zips with fixed timestamps, so the output is reproducible.

Checks: npm run check (tsc --noEmit + eslint), npm test.
`;

// The source list comes from git (tracked + untracked-but-not-ignored). Outside a checkout, e.g.
// when a reviewer rebuilds from the source zip itself, the store zips are still produced.
let tracked = null;
try {
  tracked = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '.'], {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  })
    .split('\0')
    .filter((f) => f && f !== 'BUILD.md' && !f.startsWith('store/') && !f.startsWith('dist/'));
} catch {
  console.log('skip riptide-source zip: not a git checkout');
}
if (tracked) {
  if (tracked.length === 0) problems.push('source: git ls-files returned nothing');
  const sourceEntries = tracked.map((name) => ({ name, data: readFileSync(join(root, name)) }));
  sourceEntries.push({ name: 'BUILD.md', data: Buffer.from(BUILD_STEPS, 'utf8') });
  const sourceOut = join(dist, `riptide-source-${version}.zip`);
  writeFileSync(sourceOut, zip(sourceEntries));
  console.log(`zip  ${relative(root, sourceOut)} (${sourceEntries.length} files, ${statSync(sourceOut).size} bytes)`);
}

if (problems.length) {
  for (const p of problems) console.error(`FAIL ${p}`);
  process.exit(1);
}
