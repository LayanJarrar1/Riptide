// Run with: node --test scripts/set-release-version.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setReleaseVersion } from './set-release-version.mjs';

const OLD = '0.1.0-alpha.3';

const BUILD = `const RELEASE_VERSION = '${OLD}';
const RELEASE_TAG = 'v${OLD}';
const INSTALLER_FILE = 'Riptide_${OLD}_x64-setup.exe';
const PORTABLE_FILE = 'Riptide-${OLD}-portable.zip';
const UNRELATED = 'Riptide ${OLD} stays here';
`;
const README = `## What is new in alpha 3
| Installer | [Riptide_${OLD}_x64-setup.exe](https://github.com/o/r/releases/download/v${OLD}/Riptide_${OLD}_x64-setup.exe) |
| Portable | [Riptide-${OLD}-portable.zip](https://github.com/o/r/releases/download/v${OLD}/Riptide-${OLD}-portable.zip) |
Arabic: [installer](https://github.com/o/r/releases/download/v${OLD}/Riptide_${OLD}_x64-setup.exe)
Prose about ${OLD} stays.
`;

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'rt-ver-'));
  mkdirSync(join(root, 'apps', 'website'), { recursive: true });
  writeFileSync(join(root, 'apps', 'website', 'build.mjs'), BUILD);
  writeFileSync(join(root, 'README.md'), README);
  return root;
}
const read = (root, rel) => readFileSync(join(root, rel), 'utf8');

test('rewrites every field and nothing else', () => {
  const root = fixture();
  setReleaseVersion(root, '0.1.0-alpha.5');
  const keep = (s, marker) => s.split('\n').map((l) => (l.includes(marker) ? l : l.replaceAll(OLD, '0.1.0-alpha.5'))).join('\n');
  assert.equal(read(root, 'apps/website/build.mjs'), keep(BUILD, 'UNRELATED'));
  assert.equal(read(root, 'README.md'), keep(README, 'Prose about'));
});

test('is idempotent', () => {
  const root = fixture();
  setReleaseVersion(root, '0.2.0');
  const once = read(root, 'README.md');
  const again = setReleaseVersion(root, '0.2.0');
  assert.equal(read(root, 'README.md'), once);
  assert.ok(again.every((r) => !r.changed));
});

test('--check reports a difference without writing', () => {
  const root = fixture();
  const r = setReleaseVersion(root, '0.1.0-alpha.5', { check: true });
  assert.ok(r.some((x) => x.changed));
  assert.equal(read(root, 'README.md'), README);
  setReleaseVersion(root, '0.1.0-alpha.5');
  assert.ok(setReleaseVersion(root, '0.1.0-alpha.5', { check: true }).every((x) => !x.changed));
});

test('a stable version without a suffix works', () => {
  const root = fixture();
  setReleaseVersion(root, '1.0.0');
  assert.match(read(root, 'README.md'), /Riptide-1\.0\.0-portable\.zip\]\(.*\/download\/v1\.0\.0\/Riptide-1\.0\.0-portable\.zip\)/);
});

test('rejects a version with a leading v', () => {
  assert.throws(() => setReleaseVersion(fixture(), 'v0.1.0'), /not a semver/);
});

test('fails loudly when the fields are missing', () => {
  const root = fixture();
  writeFileSync(join(root, 'README.md'), 'nothing here');
  assert.throws(() => setReleaseVersion(root, '0.1.0'), /none of the version fields/);
});
