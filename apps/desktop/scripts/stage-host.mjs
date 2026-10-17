#!/usr/bin/env node
// Builds the Native Messaging host (crates/riptide-host) in release mode and stages it where
// the Tauri bundler expects an external binary: src-tauri/binaries/riptide-host-<target>.exe.
// `tauri build --config src-tauri/tauri.bundle.conf.json` runs this before the app build, and
// the bundler then ships it as riptide-host.exe next to riptide-desktop.exe (installer and
// --no-bundle output alike). Plain Node, no dependencies.

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(desktop, '..', '..');

const rustc = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
const target = /^host:\s*(\S+)$/m.exec(rustc)?.[1];
if (!target) {
  console.error('stage-host: cannot read the host target triple from `rustc -vV`');
  process.exit(1);
}
const exe = process.platform === 'win32' ? '.exe' : '';

execFileSync('cargo', ['build', '--release', '-p', 'riptide-host'], { cwd: repo, stdio: 'inherit' });

const from = join(repo, 'target', 'release', `riptide-host${exe}`);
const to = join(desktop, 'src-tauri', 'binaries', `riptide-host-${target}${exe}`);
mkdirSync(dirname(to), { recursive: true });
copyFileSync(from, to);
console.log(`stage-host: ${from} -> ${to}`);
