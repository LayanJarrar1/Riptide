// Sanity checks on dist/<target> (a web-ext lint-style pass without the dependency):
//   manifest is valid JSON and MV3, every referenced file exists, every __MSG_x__ key exists in
//   every locale, locales share one key set, the Firefox id and the Chromium key/id are present,
//   and the README documents both ids.
//
//   node scripts/verify-dist.mjs [chrome|edge|firefox ...]
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { extensionIdFromKey } from './gen-key.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const distRoot = join(root, 'dist');
const requested = process.argv.slice(2);
const targets = requested.length
  ? requested
  : readdirSync(distRoot).filter((d) => d !== 'test' && statSync(join(distRoot, d)).isDirectory());

const FIREFOX_ID = 'riptide@riptide.app';
const problems = [];
const readme = readFileSync(join(root, 'README.md'), 'utf8');

function collectFiles(manifest) {
  const files = new Set();
  const addIcons = (icons) => {
    if (!icons) return;
    for (const p of Object.values(icons)) files.add(p);
  };
  addIcons(manifest.icons);
  addIcons(manifest.action?.default_icon);
  if (manifest.action?.default_popup) files.add(manifest.action.default_popup);
  if (manifest.options_ui?.page) files.add(manifest.options_ui.page);
  if (manifest.background?.service_worker) files.add(manifest.background.service_worker);
  for (const s of manifest.background?.scripts ?? []) files.add(s);
  for (const cs of manifest.content_scripts ?? []) {
    for (const f of [...(cs.js ?? []), ...(cs.css ?? [])]) files.add(f);
  }
  return files;
}

function collectMsgKeys(value, out = new Set()) {
  if (typeof value === 'string') {
    const m = /^__MSG_(\w+)__$/.exec(value);
    if (m) out.add(m[1]);
  } else if (value && typeof value === 'object') {
    for (const v of Object.values(value)) collectMsgKeys(v, out);
  }
  return out;
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

for (const target of targets) {
  const dir = join(distRoot, target);
  const fail = (msg) => problems.push(`${target}: ${msg}`);
  if (!existsSync(join(dir, 'manifest.json'))) {
    fail('manifest.json missing (run the build first)');
    continue;
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(join(dir, 'manifest.json'), 'utf8'));
  } catch (e) {
    fail(`manifest.json is not valid JSON: ${e.message}`);
    continue;
  }
  if (manifest.manifest_version !== 3) fail('manifest_version is not 3');
  if (!manifest.name || !manifest.version || !manifest.default_locale) fail('name/version/default_locale missing');
  if (!manifest.background) fail('no background entry');

  const files = collectFiles(manifest);
  for (const f of files) {
    if (!existsSync(join(dir, f))) fail(`referenced file missing: ${f}`);
  }

  // Locales: same keys everywhere, every __MSG_ used by the manifest defined.
  const localesDir = join(dir, '_locales');
  const locales = existsSync(localesDir) ? readdirSync(localesDir) : [];
  if (!locales.includes(manifest.default_locale)) fail(`default_locale ${manifest.default_locale} has no _locales entry`);
  const keySets = locales.map((l) => {
    const msgs = JSON.parse(readFileSync(join(localesDir, l, 'messages.json'), 'utf8'));
    return { locale: l, keys: Object.keys(msgs).sort() };
  });
  const reference = keySets[0]?.keys ?? [];
  for (const { locale, keys } of keySets) {
    if (keys.join(',') !== reference.join(',')) fail(`locale ${locale} keys differ from ${keySets[0].locale}`);
  }
  for (const key of collectMsgKeys(manifest)) {
    if (!reference.includes(key)) fail(`__MSG_${key}__ is not defined in _locales`);
  }

  // Per-browser identity.
  let id = '';
  if (target === 'firefox') {
    id = manifest.browser_specific_settings?.gecko?.id ?? '';
    if (id !== FIREFOX_ID) fail(`gecko id is ${JSON.stringify(id)}, expected ${FIREFOX_ID}`);
    if (!(manifest.permissions ?? []).includes('webRequestBlocking')) fail('webRequestBlocking missing');
    if (!manifest.background?.scripts) fail('Firefox needs background.scripts');
  } else {
    if (!manifest.key) fail('no key: the extension id would not be stable');
    else id = extensionIdFromKey(manifest.key);
    if (!manifest.background?.service_worker) fail('Chromium needs background.service_worker');
    if ((manifest.permissions ?? []).includes('webRequestBlocking')) fail('webRequestBlocking is Firefox-only');
  }
  if (id && !readme.includes(id)) fail(`README.md does not mention the extension id ${id}`);

  // No remote code: bundles must not load scripts from the network.
  for (const f of walk(dir).filter((p) => p.endsWith('.js') || p.endsWith('.html'))) {
    const text = readFileSync(f, 'utf8');
    if (/<script[^>]+src\s*=\s*["']https?:/i.test(text) || /importScripts\(\s*["']https?:/i.test(text)) {
      fail(`remote script reference in ${f}`);
    }
  }

  const total = walk(dir).length;
  console.log(`ok   dist/${target}: ${files.size} manifest references, ${total} files, ${reference.length} locale keys x ${locales.length} locales, id=${id}`);
}

if (problems.length) {
  for (const p of problems) console.error(`FAIL ${p}`);
  process.exit(1);
}
