#!/usr/bin/env node
// Fetches the video tools Riptide bundles, verifies them against pinned SHA-256 digests and
// stages them for the Tauri bundler and for development:
//
//   yt-dlp  (Unlicense)  the pinned release's yt-dlp.exe
//   ffmpeg  (LGPL 2.1+)  a BtbN *lgpl* static build: bin/ffmpeg.exe + bin/ffprobe.exe
//
//   apps/desktop/src-tauri/binaries/<tool>-<target>.exe   Tauri `externalBin` (bundled as
//                                                         <tool>.exe next to riptide-desktop.exe)
//   apps/desktop/src-tauri/binaries/dev/<tool>.exe        what a debug build finds
//   apps/desktop/src-tauri/binaries/licenses/             ffmpeg's LICENSE and README
//
// Downloads are cached in target/media-tools/. Nothing here is committed (binaries/ is in
// .gitignore). Bumping a tool = change its url and sha256 below (the GitHub release page and
// its API `digest` field give the SHA-256). BtbN prunes old autobuild tags after a while; when
// the pinned one disappears, pick a newer `autobuild-*` tag and its `-lgpl-<major>.zip`.
//
// Plain Node 20+, no dependencies; extraction uses bsdtar (`tar`, shipped with Windows 10+).

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TOOLS = {
  ytDlp: {
    version: '2026.08.19',
    url: 'https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe',
    sha256: '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a',
  },
  ffmpeg: {
    version: 'n9.0.2-27-g7e93014c5b (BtbN autobuild-2026-10-14-13-03, win64 lgpl)',
    url: 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-14-13-03/ffmpeg-n9.0.2-27-g7e93014c5b-win64-lgpl-9.0.zip',
    sha256: '395b6b0e621716ce5fc263af6388e2f35fdfe0d05a5d3f26e1e2aad09cc43d7f',
  },
};

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const cache = join(repo, 'target', 'media-tools');
const binaries = join(repo, 'apps', 'desktop', 'src-tauri', 'binaries');

function sha256Of(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

async function fetchVerified(tool) {
  mkdirSync(cache, { recursive: true });
  const file = join(cache, tool.url.split('/').pop());
  if (existsSync(file) && sha256Of(file) === tool.sha256) {
    console.log(`fetch-media-tools: cached ${file}`);
    return file;
  }
  console.log(`fetch-media-tools: downloading ${tool.url}`);
  const res = await fetch(tool.url, { redirect: 'follow' });
  if (!res.ok) {
    throw new Error(`${tool.url}: HTTP ${res.status}`);
  }
  const bytes = Buffer.from(await res.arrayBuffer());
  const digest = createHash('sha256').update(bytes).digest('hex');
  if (digest !== tool.sha256) {
    throw new Error(`${tool.url}: SHA-256 ${digest} does not match the pinned ${tool.sha256}`);
  }
  writeFileSync(file, bytes);
  return file;
}

function findFile(dir, name) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) {
      const hit = findFile(p, name);
      if (hit) return hit;
    } else if (entry.toLowerCase() === name.toLowerCase()) {
      return p;
    }
  }
  return null;
}

function hostTarget() {
  const rustc = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const target = /^host:\s*(\S+)$/m.exec(rustc)?.[1];
  if (!target) throw new Error('cannot read the host target triple from `rustc -vV`');
  return target;
}

async function main() {
  if (process.platform !== 'win32') {
    console.log('fetch-media-tools: the pinned builds are Windows binaries; skipping on this platform');
    return;
  }
  const target = hostTarget();
  const dev = join(binaries, 'dev');
  const licenses = join(binaries, 'licenses');
  mkdirSync(dev, { recursive: true });
  mkdirSync(licenses, { recursive: true });

  const ytDlp = await fetchVerified(TOOLS.ytDlp);
  copyFileSync(ytDlp, join(binaries, `yt-dlp-${target}.exe`));
  copyFileSync(ytDlp, join(dev, 'yt-dlp.exe'));

  const zip = await fetchVerified(TOOLS.ffmpeg);
  const unpacked = join(cache, 'ffmpeg');
  rmSync(unpacked, { recursive: true, force: true });
  mkdirSync(unpacked, { recursive: true });
  execFileSync('tar', ['-x', '-f', zip, '-C', unpacked], { stdio: 'inherit' });
  for (const name of ['ffmpeg', 'ffprobe']) {
    const from = findFile(unpacked, `${name}.exe`);
    if (!from) throw new Error(`${name}.exe is not in ${zip}`);
    copyFileSync(from, join(binaries, `${name}-${target}.exe`));
    copyFileSync(from, join(dev, `${name}.exe`));
  }
  const license = findFile(unpacked, 'LICENSE.txt');
  if (license) copyFileSync(license, join(licenses, 'ffmpeg-LICENSE.txt'));
  writeFileSync(
    join(licenses, 'THIRD-PARTY-MEDIA.txt'),
    [
      'Riptide bundles these programs unmodified and runs them as separate processes.',
      '',
      `yt-dlp ${TOOLS.ytDlp.version} - The Unlicense - https://github.com/yt-dlp/yt-dlp`,
      `FFmpeg ${TOOLS.ffmpeg.version} - GNU LGPL 2.1 or later - https://ffmpeg.org`,
      `  binary: ${TOOLS.ffmpeg.url}`,
      '  source: https://github.com/FFmpeg/FFmpeg (the build scripts: https://github.com/BtbN/FFmpeg-Builds)',
      '',
    ].join('\r\n'),
  );
  console.log(`fetch-media-tools: staged yt-dlp, ffmpeg and ffprobe for ${target} in ${binaries}`);
}

main().catch((e) => {
  console.error(`fetch-media-tools: ${e.message}`);
  process.exit(1);
});
