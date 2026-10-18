#!/usr/bin/env node
// Assembles the portable layout from a `tauri build --no-bundle` output (run through
// `npm run bundle:portable`):
//
//   target/portable/Riptide/
//     riptide-desktop.exe
//     riptide-host.exe      Native Messaging host, found by the app next to itself
//     yt-dlp.exe, ffmpeg.exe, ffprobe.exe   video support (scripts/fetch-media-tools.mjs)
//     licenses\             third-party licenses of the video tools
//     portable.txt          data goes to .\data, nothing under %LOCALAPPDATA%
//
// and zips it as target/portable/Riptide-<version>-portable.zip (bsdtar, shipped with Windows
// 10+). Plain Node, no dependencies.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(desktop, '..', '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const release = join(repo, 'target', 'release');
const version = JSON.parse(readFileSync(join(desktop, 'package.json'), 'utf8')).version;

const out = join(repo, 'target', 'portable');
const folder = join(out, 'Riptide');
rmSync(folder, { recursive: true, force: true });
mkdirSync(folder, { recursive: true });

for (const name of [`riptide-desktop${exe}`, `riptide-host${exe}`]) {
  const from = join(release, name);
  if (!existsSync(from)) {
    console.error(`portable: ${from} is missing; run \`npm run bundle:portable\``);
    process.exit(1);
  }
  copyFileSync(from, join(folder, name));
}
// video tools: `tauri build` copies externalBin next to the app in target/release
for (const name of ['yt-dlp', 'ffmpeg', 'ffprobe']) {
  const from = join(release, `${name}${exe}`);
  if (!existsSync(from)) {
    console.error(`portable: ${from} is missing; run \`node scripts/fetch-media-tools.mjs\` first`);
    process.exit(1);
  }
  copyFileSync(from, join(folder, `${name}${exe}`));
}
const licenses = join(desktop, 'src-tauri', 'binaries', 'licenses');
if (existsSync(licenses)) {
  mkdirSync(join(folder, 'licenses'), { recursive: true });
  for (const name of readdirSync(licenses)) {
    copyFileSync(join(licenses, name), join(folder, 'licenses', name));
  }
}
writeFileSync(
  join(folder, 'portable.txt'),
  'Riptide portable: settings, database and logs live in the data folder next to this file.\r\n',
);

const zip = join(out, `Riptide-${version}-portable.zip`);
rmSync(zip, { force: true });
execFileSync('tar', ['-a', '-c', '-f', zip, '-C', out, 'Riptide'], { stdio: 'inherit' });
console.log(`portable: ${zip}`);
