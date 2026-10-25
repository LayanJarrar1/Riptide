#!/usr/bin/env node
// Fetches the pinned video-support tools and stages them where the Tauri bundler expects
// external binaries (src-tauri/binaries/<tool>-<target>.exe), next to their license texts:
//
//   yt-dlp-<target>.exe     yt-dlp release below (Unlicense)
//   ffmpeg-<target>.exe     BtbN FFmpeg-Builds, win64 LGPL variant (no GPL parts)
//   yt-dlp-LICENSE.txt      LICENSE + THIRD_PARTY_LICENSES.txt of that yt-dlp tag
//   ffmpeg-LICENSE.txt      LICENSE.txt from the ffmpeg archive
//
// Every download is checked against the SHA-256 pinned here; a mismatch fails the build. Files
// already staged with the right hash are kept, so this is cheap to run before every bundle
// (`tauri build --config src-tauri/tauri.bundle.conf.json` does) and for development
// (`npm run media-tools`; debug builds find the staged copies on their own).
//
// To move to a newer release: change the version/URL, run with RIPTIDE_MEDIA_PRINT_HASHES=1,
// review, and paste the printed hashes. Plain Node 20, no dependencies; Windows x64 only for now
// (bsdtar from Windows 10+ unpacks the ffmpeg zip).

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const YT_DLP_VERSION = '2026.08.19';
const FFMPEG_DIR = 'ffmpeg-n9.0.2-28-g669f5ab083-win64-lgpl-9.0';

/**
 * macOS: yt-dlp's own universal build (Apple Silicon and Intel), pinned like the Windows one. No
 * ffmpeg is bundled on macOS yet (there is no LGPL-only macOS build to pin), so video pages fall
 * back to the formats that need no merging; see docs/MACOS.md.
 */
const MAC_DOWNLOADS = {
  ytDlp: {
    url: `https://github.com/yt-dlp/yt-dlp/releases/download/${YT_DLP_VERSION}/yt-dlp_macos`,
    sha256: '0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202',
  },
};

const DOWNLOADS = {
  ytDlp: {
    url: `https://github.com/yt-dlp/yt-dlp/releases/download/${YT_DLP_VERSION}/yt-dlp.exe`,
    sha256: '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a',
  },
  ytDlpLicense: {
    url: `https://raw.githubusercontent.com/yt-dlp/yt-dlp/${YT_DLP_VERSION}/LICENSE`,
    sha256: '7e12e5df4bae12cb21581ba157ced20e1986a0508dd10d0e8a4ab9a4cf94e85c',
  },
  ytDlpThirdParty: {
    url: `https://raw.githubusercontent.com/yt-dlp/yt-dlp/${YT_DLP_VERSION}/THIRD_PARTY_LICENSES.txt`,
    sha256: '472aefe951c7db35e1657c1d13fd337140511ed6f2b329205105ad441c5a02b7',
  },
  ffmpegZip: {
    url: `https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-18-13-01/${FFMPEG_DIR}.zip`,
    sha256: '7f4cbda0ece7721488eeebf2fa69762d7388bca69450fd13045b408744457b14',
  },
};

const printHashes = process.env.RIPTIDE_MEDIA_PRINT_HASHES === '1';
const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binaries = join(desktop, 'src-tauri', 'binaries');

if (process.platform !== 'win32' && process.platform !== 'darwin') {
  console.log('fetch-media-tools: only Windows and macOS builds bundle video support; skipping');
  process.exit(0);
}

const rustc = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
const target = /^host:\s*(\S+)$/m.exec(rustc)?.[1];
if (!target) {
  console.error('fetch-media-tools: cannot read the host target triple from `rustc -vV`');
  process.exit(1);
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

if (process.platform === 'darwin') {
  await stageMac();
  process.exit(0);
}

/** Downloads `entry.url` and returns its bytes after checking the pinned hash. */
async function fetchPinned(name, entry) {
  const res = await fetch(entry.url, { redirect: 'follow', headers: { 'User-Agent': 'riptide-build' } });
  if (!res.ok) {
    throw new Error(`${name}: HTTP ${res.status} for ${entry.url}`);
  }
  const bytes = Buffer.from(await res.arrayBuffer());
  const actual = sha256(bytes);
  if (printHashes) {
    console.log(`${name}: ${actual}`);
  } else if (actual !== entry.sha256) {
    throw new Error(`${name}: SHA-256 mismatch (expected ${entry.sha256}, got ${actual}) for ${entry.url}`);
  }
  return bytes;
}

/** `true` when `path` exists and hashes to `expected`. */
function staged(path, expected) {
  return existsSync(path) && sha256(readFileSync(path)) === expected;
}

const stamp = join(binaries, 'media-tools.json');
const wanted = JSON.stringify({ ytDlp: DOWNLOADS.ytDlp.sha256, ffmpeg: DOWNLOADS.ffmpegZip.sha256, target });
const ytDlpExe = join(binaries, `yt-dlp-${target}.exe`);
const ffmpegExe = join(binaries, `ffmpeg-${target}.exe`);
const ytDlpLicense = join(binaries, 'yt-dlp-LICENSE.txt');
const ffmpegLicense = join(binaries, 'ffmpeg-LICENSE.txt');

const upToDate =
  !printHashes &&
  existsSync(stamp) &&
  readFileSync(stamp, 'utf8') === wanted &&
  staged(ytDlpExe, DOWNLOADS.ytDlp.sha256) &&
  [ffmpegExe, ytDlpLicense, ffmpegLicense].every((p) => existsSync(p));
if (upToDate) {
  console.log(`fetch-media-tools: yt-dlp ${YT_DLP_VERSION} and ${FFMPEG_DIR} already staged`);
  process.exit(0);
}

mkdirSync(binaries, { recursive: true });

try {
  const exe = await fetchPinned('yt-dlp.exe', DOWNLOADS.ytDlp);
  const license = await fetchPinned('yt-dlp LICENSE', DOWNLOADS.ytDlpLicense);
  const thirdParty = await fetchPinned('yt-dlp THIRD_PARTY_LICENSES', DOWNLOADS.ytDlpThirdParty);
  const zip = await fetchPinned('ffmpeg zip', DOWNLOADS.ffmpegZip);
  if (printHashes) {
    process.exit(0);
  }

  writeFileSync(ytDlpExe, exe);
  writeFileSync(
    ytDlpLicense,
    Buffer.concat([
      Buffer.from(`yt-dlp ${YT_DLP_VERSION} - https://github.com/yt-dlp/yt-dlp\r\n\r\n`),
      license,
      Buffer.from('\r\n\r\n---- THIRD_PARTY_LICENSES.txt (bundled in yt-dlp.exe) ----\r\n\r\n'),
      thirdParty,
    ]),
  );

  const work = mkdtempSync(join(tmpdir(), 'riptide-ffmpeg-'));
  try {
    const zipPath = join(work, 'ffmpeg.zip');
    writeFileSync(zipPath, zip);
    // Windows' own bsdtar reads zip; a GNU tar earlier on PATH (Git Bash) does not
    const tar = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe');
    execFileSync(
      tar,
      ['-x', '-f', zipPath, '-C', work, `${FFMPEG_DIR}/bin/ffmpeg.exe`, `${FFMPEG_DIR}/LICENSE.txt`],
      { stdio: 'inherit' },
    );
    writeFileSync(ffmpegExe, readFileSync(join(work, FFMPEG_DIR, 'bin', 'ffmpeg.exe')));
    writeFileSync(
      ffmpegLicense,
      Buffer.concat([
        Buffer.from(`${FFMPEG_DIR} - ${DOWNLOADS.ffmpegZip.url}\r\n\r\n`),
        readFileSync(join(work, FFMPEG_DIR, 'LICENSE.txt')),
      ]),
    );
  } finally {
    rmSync(work, { recursive: true, force: true });
  }

  writeFileSync(stamp, wanted);
  console.log(`fetch-media-tools: staged yt-dlp ${YT_DLP_VERSION} and ${FFMPEG_DIR} in ${binaries}`);
} catch (error) {
  console.error(`fetch-media-tools: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}

/** Stages `yt-dlp-<target>` (executable) and its licence text for a macOS bundle. */
async function stageMac() {
  const exe = join(binaries, `yt-dlp-${target}`);
  const license = join(binaries, 'yt-dlp-LICENSE.txt');
  const macStamp = join(binaries, 'media-tools.json');
  const want = JSON.stringify({ ytDlp: MAC_DOWNLOADS.ytDlp.sha256, target });
  if (!printHashes && existsSync(macStamp) && readFileSync(macStamp, 'utf8') === want && staged(exe, MAC_DOWNLOADS.ytDlp.sha256) && existsSync(license)) {
    console.log(`fetch-media-tools: yt-dlp ${YT_DLP_VERSION} (macOS) already staged`);
    return;
  }
  mkdirSync(binaries, { recursive: true });
  const bytes = await fetchPinned('yt-dlp_macos', MAC_DOWNLOADS.ytDlp);
  const text = await fetchPinned('yt-dlp LICENSE', DOWNLOADS.ytDlpLicense);
  const thirdParty = await fetchPinned('yt-dlp THIRD_PARTY_LICENSES', DOWNLOADS.ytDlpThirdParty);
  if (printHashes) {
    return;
  }
  writeFileSync(exe, bytes, { mode: 0o755 });
  writeFileSync(
    license,
    Buffer.concat([
      Buffer.from(`yt-dlp ${YT_DLP_VERSION} - https://github.com/yt-dlp/yt-dlp\n\n`),
      text,
      Buffer.from('\n\n---- THIRD_PARTY_LICENSES.txt (bundled in yt-dlp_macos) ----\n\n'),
      thirdParty,
    ]),
  );
  writeFileSync(macStamp, want);
  console.log(`fetch-media-tools: staged ${exe}`);
}
