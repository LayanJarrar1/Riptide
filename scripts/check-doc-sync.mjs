#!/usr/bin/env node
// Fails when the release version is not the same everywhere it is written down.
//
//   node scripts/check-doc-sync.mjs [--root <dir>]
//
// Compared with the featured release in apps/website/release.json (the authority):
//   README.md                  every /releases/download/v<version>/ link and Riptide_ / Riptide- file name
//   CHANGELOG.md               the newest "## [x.y.z]" section (Unreleased is skipped)
//   Cargo.toml                 [workspace.package] version
//   apps/desktop/src-tauri/tauri.conf.json   "version" when present (absent means: inherited from Cargo)
//   apps/desktop/package.json  "version"
//   apps/website/build.mjs     RELEASE_VERSION
// During a release candidate (Cargo bumped, release not published yet) this is expected to fail: it guards
// the moment after publishing, see guide/release-process.md.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const V = '\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?';

/**
 * Collects every version string found in the documents under `root`.
 * @returns {{name: string, versions: string[]}[]} one row per source; an empty list means "not found"
 */
export function collectVersions(root) {
  const read = (rel) => (existsSync(join(root, rel)) ? readFileSync(join(root, rel), 'utf8') : null);
  const rows = [];
  const add = (name, versions) => rows.push({ name, versions: [...new Set(versions)] });

  const manifest = read('apps/website/release.json');
  if (manifest) {
    const m = JSON.parse(manifest);
    const tag = m.latest?.stable ?? m.latest?.prerelease;
    add('release.json (featured)', m.releases.filter((r) => r.tag === tag).map((r) => r.version));
  } else add('release.json (featured)', []);

  const readme = read('README.md') ?? '';
  const fromReadme = [
    ...[...readme.matchAll(new RegExp(`/releases/download/v(${V})/`, 'g'))].map((x) => x[1]),
    ...[...readme.matchAll(new RegExp(`Riptide_(${V})_x64-setup\\.exe`, 'g'))].map((x) => x[1]),
    ...[...readme.matchAll(new RegExp(`Riptide-(${V})-portable\\.zip`, 'g'))].map((x) => x[1]),
  ];
  add('README.md download links', fromReadme);

  const changelog = read('CHANGELOG.md') ?? '';
  const top = [...changelog.matchAll(new RegExp(`^## \\[(${V})\\]`, 'gm'))][0];
  add('CHANGELOG.md top section', top ? [top[1]] : []);

  const cargo = read('Cargo.toml') ?? '';
  const pkg = /\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/.exec(cargo);
  add('Cargo.toml workspace version', pkg ? [pkg[1]] : []);

  const tauri = read('apps/desktop/src-tauri/tauri.conf.json');
  if (tauri) {
    const v = JSON.parse(tauri).version;
    if (v !== undefined) add('tauri.conf.json version', [v]);
  }
  const pj = read('apps/desktop/package.json');
  if (pj) add('apps/desktop/package.json', [JSON.parse(pj).version].filter(Boolean));

  const build = read('apps/website/build.mjs') ?? '';
  const rv = new RegExp(`const RELEASE_VERSION = '(${V})'`).exec(build);
  add('build.mjs RELEASE_VERSION', rv ? [rv[1]] : []);
  return rows;
}

/** @returns {string[]} problems; empty when everything agrees */
export function findProblems(rows) {
  const problems = [];
  const reference = rows[0]?.versions[0];
  if (!reference) return ['release.json has no featured release'];
  for (const row of rows) {
    if (row.versions.length === 0) problems.push(`${row.name}: no version found`);
    else if (row.versions.length > 1) problems.push(`${row.name}: mixed versions ${row.versions.join(', ')}`);
    else if (row.versions[0] !== reference) problems.push(`${row.name} is ${row.versions[0]}, release.json says ${reference}`);
  }
  return problems;
}

function main(argv) {
  const at = argv.indexOf('--root');
  const root = at >= 0 ? resolve(argv[at + 1]) : resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const rows = collectVersions(root);
  for (const r of rows) console.log(`${r.versions.join(', ') || '-'}\t${r.name}`);
  const problems = findProblems(rows);
  if (problems.length) {
    console.error(`doc sync: ${problems.length} problem(s)`);
    for (const p of problems) console.error(`  - ${p}`);
    process.exit(1);
  }
  console.log('doc sync: ok');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main(process.argv.slice(2));
