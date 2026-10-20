#!/usr/bin/env node
// Checks that every release link the site and the README advertise really answers 200.
//
//   node scripts/check-release-links.mjs                  manifest links + release links found in apps/website/dist and README.md
//   node scripts/check-release-links.mjs --list           print the links and exit
//   node scripts/check-release-links.mjs --require-public fail instead of skipping when the repository is private
//
// Links checked: installer, portable, SHA256SUMS and the notes page of every release in release.json, plus any
// github.com/<repo>/releases/... URL in the built pages and README.md. While the repository is private an
// unauthenticated request answers 404 for all of them, so the check says so and skips (exit 0).
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { REPO_SLUG, validateManifest } from './gen-release-manifest.mjs';

const RELEASE_LINK = new RegExp(`https://github\\.com/${REPO_SLUG}/releases/(?:download|tag)/[^\\s"'<>)\\]]+`, 'g');

/** Every release link a manifest declares. */
export function manifestLinks(manifest) {
  const urls = [];
  for (const r of manifest.releases) urls.push(r.notesUrl, r.installer.url, r.portable.url, r.sums.url);
  return urls;
}

/** Release URLs mentioned in a text (HTML or Markdown), query and fragment removed. */
export function linksIn(text) {
  return [...String(text).matchAll(RELEASE_LINK)].map((m) => m[0].replace(/[.,;]+$/, '').split(/[?#]/)[0]);
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (full.endsWith('.html')) out.push(full);
  }
  return out;
}

/** Collects the unique links to check. */
export function collectLinks(root) {
  const set = new Set();
  const manifestPath = join(root, 'apps/website/release.json');
  if (existsSync(manifestPath)) manifestLinks(validateManifest(JSON.parse(readFileSync(manifestPath, 'utf8')))).forEach((u) => set.add(u));
  const dist = join(root, 'apps/website/dist');
  if (existsSync(dist)) for (const f of walk(dist)) linksIn(readFileSync(f, 'utf8')).forEach((u) => set.add(u));
  if (existsSync(join(root, 'README.md'))) linksIn(readFileSync(join(root, 'README.md'), 'utf8')).forEach((u) => set.add(u));
  return [...set].sort();
}

/**
 * Probes one URL: follows redirects and reads one byte (so a download is not pulled in full).
 * @returns {Promise<number>} the final HTTP status (206 counts as available)
 */
export async function probe(url, fetchImpl = fetch) {
  const res = await fetchImpl(url, { method: 'GET', headers: { Range: 'bytes=0-0', 'User-Agent': 'riptide-link-check' }, redirect: 'follow' });
  try { await res.body?.cancel(); } catch { /* already closed */ }
  return res.status;
}

/** True when the repository answers the unauthenticated API (public). 404 means private or missing. */
export async function repoIsPublic(fetchImpl = fetch) {
  const res = await fetchImpl(`https://api.github.com/repos/${REPO_SLUG}`, { headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'riptide-link-check' } });
  try { await res.body?.cancel(); } catch { /* already closed */ }
  return res.status === 200;
}

/** Runs the check. @returns {Promise<{status: 'ok'|'skipped'|'failed', failures: string[], checked: number}>} */
export async function run(links, { fetchImpl = fetch, requirePublic = false } = {}) {
  if (!(await repoIsPublic(fetchImpl))) {
    if (requirePublic) return { status: 'failed', failures: [`${REPO_SLUG} is not publicly reachable`], checked: 0 };
    return { status: 'skipped', failures: [], checked: 0 };
  }
  const failures = [];
  for (const url of links) {
    let status;
    try { status = await probe(url, fetchImpl); } catch (e) { status = `error ${e.message}`; }
    if (status !== 200 && status !== 206) failures.push(`${status}  ${url}`);
  }
  return { status: failures.length ? 'failed' : 'ok', failures, checked: links.length };
}

async function main(argv) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const links = collectLinks(root);
  if (argv.includes('--list')) { console.log(links.join('\n')); return; }
  const result = await run(links, { requirePublic: argv.includes('--require-public') });
  if (result.status === 'skipped') {
    console.log(`NOTICE: ${REPO_SLUG} is private (the unauthenticated API answers 404), so ${links.length} release links cannot be checked from here. Skipped, not passed. Re-run once the repository is public.`);
    return;
  }
  if (result.status === 'failed') {
    console.error(`release links: ${result.failures.length} problem(s)`);
    for (const f of result.failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log(`release links: ${result.checked} links answer 200`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main(process.argv.slice(2)).catch((e) => { console.error(e.message); process.exit(1); });
