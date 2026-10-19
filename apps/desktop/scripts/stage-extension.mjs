#!/usr/bin/env node
// Builds the browser extension (apps/extension) so the bundler can ship it:
//   apps/extension/dist/chrome  -> <install>/extension/chrome
//   apps/extension/dist/edge    -> <install>/extension/edge
// (see `bundle.resources` in src-tauri/tauri.bundle.conf.json and scripts/portable.mjs). The
// user loads that folder once through chrome://extensions -> Developer mode -> Load unpacked;
// Settings -> Integration opens it. `npm run build` of the extension ends with verify-dist, so
// a broken manifest stops the release here. Plain Node, no dependencies.

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const extension = resolve(desktop, '..', 'extension');
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const run = (args) =>
  execFileSync(npm, args, { cwd: extension, stdio: 'inherit', shell: process.platform === 'win32' });

if (!existsSync(join(extension, 'node_modules'))) {
  run(['ci', '--no-audit', '--no-fund']);
}
run(['run', 'build']);

for (const browser of ['chrome', 'edge']) {
  const manifest = join(extension, 'dist', browser, 'manifest.json');
  if (!existsSync(manifest)) {
    console.error(`stage-extension: ${manifest} is missing after the build`);
    process.exit(1);
  }
}
console.log('stage-extension: apps/extension/dist/{chrome,edge} are ready');
