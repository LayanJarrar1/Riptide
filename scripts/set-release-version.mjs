#!/usr/bin/env node
// Points the website and the README download links at a release version.
//
//   node scripts/set-release-version.mjs 0.1.0-alpha.5          rewrite (idempotent)
//   node scripts/set-release-version.mjs 0.1.0-alpha.5 --check  exit 1 if anything differs
//
// Only these fields are touched:
//   apps/website/build.mjs  RELEASE_VERSION, RELEASE_TAG, INSTALLER_FILE, PORTABLE_FILE
//   README.md               installer and portable file names and the /download/v<version>/ links
// Pass --root <dir> to operate on another checkout (the tests use fixtures).
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const V = '\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?';

/** Each rule: [name, regex with three groups (before, old version, after)]. */
const WEBSITE_RULES = [
  ['RELEASE_VERSION', new RegExp(`(const RELEASE_VERSION = ')(${V})(')`, 'g')],
  ['RELEASE_TAG', new RegExp(`(const RELEASE_TAG = 'v)(${V})(')`, 'g')],
  ['INSTALLER_FILE', new RegExp(`(const INSTALLER_FILE = 'Riptide_)(${V})(_x64-setup\\.exe')`, 'g')],
  ['PORTABLE_FILE', new RegExp(`(const PORTABLE_FILE = 'Riptide-)(${V})(-portable\\.zip')`, 'g')],
];

const README_RULES = [
  ['installer file', new RegExp(`(Riptide_)(${V})(_x64-setup\\.exe)`, 'g')],
  ['portable file', new RegExp(`(Riptide-)(${V})(-portable\\.zip)`, 'g')],
  ['release tag link', new RegExp(`(/releases/download/v)(${V})(/)`, 'g')],
];

/**
 * Applies `rules` to `text`.
 * @returns {{text: string, matched: string[], changed: boolean}}
 */
export function applyRules(text, rules, version) {
  const matched = [];
  let out = text;
  for (const [name, re] of rules) {
    re.lastIndex = 0;
    if (!re.test(out)) continue;
    matched.push(name);
    re.lastIndex = 0;
    out = out.replace(re, (_m, a, _old, c) => `${a}${version}${c}`);
  }
  return { text: out, matched, changed: out !== text };
}

/**
 * Rewrites (or with `check`, only inspects) both files under `root`.
 * @returns {{file: string, changed: boolean, matched: string[]}[]}
 */
export function setReleaseVersion(root, version, { check = false } = {}) {
  if (!SEMVER.test(version)) throw new Error(`"${version}" is not a semver version (no leading v)`);
  const targets = [
    ['apps/website/build.mjs', WEBSITE_RULES],
    ['README.md', README_RULES],
  ];
  const results = [];
  for (const [rel, rules] of targets) {
    const path = join(root, rel);
    if (!existsSync(path)) throw new Error(`${rel} not found under ${root}`);
    const before = readFileSync(path, 'utf8');
    const { text, matched, changed } = applyRules(before, rules, version);
    if (matched.length === 0) throw new Error(`${rel}: none of the version fields were found`);
    if (changed && !check) writeFileSync(path, text);
    results.push({ file: rel, changed, matched });
  }
  return results;
}

function main(argv) {
  const args = argv.slice(2);
  const check = args.includes('--check');
  const rootAt = args.indexOf('--root');
  const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const root = rootAt >= 0 ? resolve(args[rootAt + 1]) : defaultRoot;
  const version = args.find((a, i) => !a.startsWith('--') && args[i - 1] !== '--root');
  if (!version) {
    console.error('usage: node scripts/set-release-version.mjs <version> [--check] [--root <dir>]');
    return 2;
  }
  let results;
  try {
    results = setReleaseVersion(root, version, { check });
  } catch (e) {
    console.error(e.message);
    return 2;
  }
  for (const r of results) {
    const state = r.changed ? (check ? 'DIFFERS' : 'updated') : 'ok';
    console.log(`${state.padEnd(8)} ${r.file} (${r.matched.join(', ')})`);
  }
  return check && results.some((r) => r.changed) ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv));
}
