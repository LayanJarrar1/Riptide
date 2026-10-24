#!/usr/bin/env node
// i18n lint for the desktop app. Plain Node, no dependencies.
//
// Fails (exit 1) when:
//   * a key exists in one of src/i18n/{ar,en}.json but not the other,
//   * a value is not a non-empty string (the tables are flat),
//   * a key used in src/**/*.ts|html via  t('key')  or  'key' | t  is missing from either file.
// A template-literal lookup such as t(`lang.${value}`) counts as a reference to every key under
// that prefix (and fails if the prefix matches nothing). Keys that exist in the tables but are
// never referenced are reported as a warning only.
//
// *.spec.ts files are skipped: tests deliberately look up keys that do not exist.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src');
const files = {
  ar: join(srcDir, 'i18n', 'ar.json'),
  en: join(srcDir, 'i18n', 'en.json'),
};

const problems = [];
const warnings = [];

function loadTable(lang, file) {
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(file, 'utf8'));
  } catch (error) {
    problems.push(`${rel(file)}: cannot parse JSON (${error.message})`);
    return new Map();
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    problems.push(`${rel(file)}: must be a flat JSON object`);
    return new Map();
  }
  const table = new Map();
  for (const [key, value] of Object.entries(parsed)) {
    if (typeof value !== 'string') {
      problems.push(`${rel(file)}: "${key}" must be a string (tables are flat)`);
    } else if (value.trim() === '') {
      problems.push(`${rel(file)}: "${key}" is empty`);
    }
    table.set(key, value);
  }
  return table;
}

function rel(file) {
  return relative(root, file).replaceAll('\\', '/');
}

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      yield* walk(full);
    } else if (/\.(ts|html)$/.test(entry) && !entry.endsWith('.spec.ts')) {
      yield full;
    }
  }
}

// t('key') / t("key") / i18n.t('key')  and  'key' | t  /  "key" | t
const CALL_RE = /\bt\(\s*(['"])([^'"\n]+)\1\s*\)/g;
const PIPE_RE = /(['"])([^'"\n]+)\1\s*\|\s*t\b/g;
// t(`prefix.${expr}`) — a dynamic lookup; every key under `prefix.` counts as referenced.
// plural('key', n) — a plural lookup; `key.other` must exist and every `key.<form>` counts as referenced.
const PLURAL_RE = /\bplural\(\s*(?:\w+\s*,\s*)?(['"])([^'"\n]+)\1\s*,/g;
const PREFIX_RE = /\bt\(\s*`([^`$\n]+\.)\$\{[^`\n]*\}`\s*\)/g;

function lineOf(source, index) {
  return source.slice(0, index).split('\n').length;
}

function collectUsedKeys() {
  const used = new Map(); // key -> first location
  const prefixes = new Map(); // prefix -> first location
  for (const file of walk(srcDir)) {
    const source = readFileSync(file, 'utf8');
    for (const re of [CALL_RE, PIPE_RE]) {
      re.lastIndex = 0;
      for (const match of source.matchAll(re)) {
        const key = match[2];
        if (!used.has(key)) {
          used.set(key, `${rel(file)}:${lineOf(source, match.index)}`);
        }
      }
    }
    PLURAL_RE.lastIndex = 0;
    for (const match of source.matchAll(PLURAL_RE)) {
      const key = `${match[2]}.other`;
      if (!used.has(key)) {
        used.set(key, `${rel(file)}:${lineOf(source, match.index)}`);
      }
      const prefix = `${match[2]}.`;
      if (!prefixes.has(prefix)) {
        prefixes.set(prefix, `${rel(file)}:${lineOf(source, match.index)}`);
      }
    }
    PREFIX_RE.lastIndex = 0;
    for (const match of source.matchAll(PREFIX_RE)) {
      const prefix = match[1];
      if (!prefixes.has(prefix)) {
        prefixes.set(prefix, `${rel(file)}:${lineOf(source, match.index)}`);
      }
    }
  }
  return { used, prefixes };
}

const ar = loadTable('ar', files.ar);
const en = loadTable('en', files.en);

// Arabic has more plural forms than English; those extras may exist in one file only, as long as
// the `.other` form of the same key is in both.
const isExtraPlural = (key) => /\.(zero|two|few|many)$/.test(key) && ar.has(key.replace(/\.\w+$/, '.other')) && en.has(key.replace(/\.\w+$/, '.other'));

for (const key of ar.keys()) {
  if (!en.has(key) && !isExtraPlural(key)) problems.push(`en.json is missing "${key}" (present in ar.json)`);
}
for (const key of en.keys()) {
  if (!ar.has(key) && !isExtraPlural(key)) problems.push(`ar.json is missing "${key}" (present in en.json)`);
}

const { used, prefixes } = collectUsedKeys();
for (const [key, where] of used) {
  const missing = [];
  if (!ar.has(key)) missing.push('ar.json');
  if (!en.has(key)) missing.push('en.json');
  if (missing.length > 0) {
    problems.push(`"${key}" used at ${where} is missing from ${missing.join(' and ')}`);
  }
}

for (const [prefix, where] of prefixes) {
  if (![...en.keys()].some((key) => key.startsWith(prefix))) {
    problems.push(`dynamic prefix "${prefix}" used at ${where} matches no key`);
  }
}

const isReferenced = (key) =>
  used.has(key) || [...prefixes.keys()].some((prefix) => key.startsWith(prefix));

for (const key of en.keys()) {
  if (ar.has(key) && !isReferenced(key)) {
    warnings.push(`"${key}" is defined but never referenced`);
  }
}

if (warnings.length > 0) {
  console.warn(`i18n:lint — ${warnings.length} warning(s):`);
  for (const w of warnings) console.warn(`  - ${w}`);
}

if (problems.length > 0) {
  console.error(`i18n:lint — ${problems.length} problem(s):`);
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}

console.log(
  `i18n:lint — ok (${en.size} keys, ${used.size} referenced in ${[...walk(srcDir)].length} files)`,
);
