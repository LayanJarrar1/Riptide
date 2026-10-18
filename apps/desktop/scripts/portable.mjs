#!/usr/bin/env node
// Assembles the portable layout from a `tauri build --no-bundle` output (run through
// `npm run bundle:portable`):
//
//   target/portable/Riptide/
//     riptide-desktop.exe
//     riptide-host.exe      Native Messaging host, found by the app next to itself
//     yt-dlp.exe, ffmpeg.exe video support (pinned, see fetch-media-tools.mjs) + licenses/
//     portable.txt          data goes to .\data, nothing under %LOCALAPPDATA%
//
// and zips it as target/portable/Riptide-<version>-portable.zip (bsdtar, shipped with Windows
// 10+). Plain Node, no dependencies.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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

// video support: the pinned yt-dlp and ffmpeg staged by scripts/fetch-media-tools.mjs
// (src-tauri/binaries/<tool>-<target>.exe), shipped as <tool>.exe next to the app like the
// installer does, with their license texts.
const binaries = join(desktop, 'src-tauri', 'binaries');
const target = /^host:\s*(\S+)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
for (const tool of ['yt-dlp', 'ffmpeg']) {
  const from = join(binaries, `${tool}-${target}${exe}`);
  if (!existsSync(from)) {
    console.error(`portable: ${from} is missing; run \`node scripts/fetch-media-tools.mjs\``);
    process.exit(1);
  }
  copyFileSync(from, join(folder, `${tool}${exe}`));
}
mkdirSync(join(folder, 'licenses'), { recursive: true });
copyFileSync(join(desktop, 'src-tauri', 'THIRD-PARTY-NOTICES.txt'), join(folder, 'THIRD-PARTY-NOTICES.txt'));
for (const tool of ['yt-dlp', 'ffmpeg']) {
  copyFileSync(join(binaries, `${tool}-LICENSE.txt`), join(folder, 'licenses', `${tool}-LICENSE.txt`));
}
writeFileSync(
  join(folder, 'portable.txt'),
  'Riptide portable: settings, database and logs live in the data folder next to this file.\r\n',
);

const zip = join(out, `Riptide-${version}-portable.zip`);
rmSync(zip, { force: true });
execFileSync('tar', ['-a', '-c', '-f', zip, '-C', out, 'Riptide'], { stdio: 'inherit' });
console.log(`portable: ${zip}`);
