#!/usr/bin/env node
// Generates apps/website/release.json, the one authoritative description of the published releases.
//
//   node scripts/gen-release-manifest.mjs                       read the GitHub Releases API (gh, or GITHUB_TOKEN)
//   node scripts/gen-release-manifest.mjs --fixture f.json      read a local fixture instead (tests, offline)
//   node scripts/gen-release-manifest.mjs --out path.json       default: apps/website/release.json
//   node scripts/gen-release-manifest.mjs --check               exit 1 if the file on disk differs from a fresh run
//
// The manifest is built from API data, never from user input, and every field is validated before it
// is written: tags and versions match strict patterns, every asset URL must be exactly
// https://github.com/LayanJarrar1/Riptide/releases/download/<tag>/<name>, installer and portable names
// must follow Riptide_<version>_x64-setup.exe / Riptide-<version>-portable.zip, and the SHA-256 in
// SHA256SUMS.txt must agree with the digest GitHub reports for the asset. Anything else is refused:
// the script exits 1 and writes nothing.
//
// Fixture format: { "releases": [<GitHub release objects>], "sums": { "<tag>": "<SHA256SUMS.txt text>" } }
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO_SLUG = 'LayanJarrar1/Riptide';
export const SCHEMA_VERSION = 1;
/** Releases older than this predate the SHA256SUMS / asset naming contract and are not listed. */
export const MIN_VERSION = '0.1.0-alpha.3';

// Strict patterns. No leading zeros, no build metadata, only these three channels.
const NUM = '(?:0|[1-9]\\d{0,5})';
export const VERSION_RE = new RegExp(`^(${NUM})\\.(${NUM})\\.(${NUM})(?:-(alpha|beta)\\.(${NUM}))?$`);
export const TAG_RE = new RegExp(`^v(${NUM}\\.${NUM}\\.${NUM}(?:-(?:alpha|beta)\\.${NUM})?)$`);
const SHA_RE = /^[0-9a-f]{64}$/;

/** @returns {{major:number,minor:number,patch:number,pre:string|null,n:number}|null} */
export function parseVersion(version) {
  const m = VERSION_RE.exec(String(version));
  if (!m) return null;
  return { major: +m[1], minor: +m[2], patch: +m[3], pre: m[4] ?? null, n: m[5] === undefined ? 0 : +m[5] };
}

/** semver precedence for the versions we accept: alpha < beta < stable of the same x.y.z. */
export function compareVersions(a, b) {
  const x = parseVersion(a);
  const y = parseVersion(b);
  const rank = (v) => (v.pre === 'alpha' ? 0 : v.pre === 'beta' ? 1 : 2);
  for (const k of ['major', 'minor', 'patch']) if (x[k] !== y[k]) return x[k] - y[k];
  if (rank(x) !== rank(y)) return rank(x) - rank(y);
  return x.n - y.n;
}

/** The channel of a version string, cross-checked against GitHub's prerelease flag. Throws on a mismatch. */
export function channelOf(version, prerelease) {
  const v = parseVersion(version);
  if (!v) throw new Error(`"${version}" is not a Riptide version (x.y.z, x.y.z-alpha.N or x.y.z-beta.N)`);
  const channel = v.pre ?? 'stable';
  if (channel === 'stable' && prerelease !== false) throw new Error(`${version} looks stable but GitHub marks it as a pre-release`);
  if (channel !== 'stable' && prerelease !== true) throw new Error(`${version} is an ${channel} but GitHub does not mark it as a pre-release`);
  return channel;
}

export const installerName = (version) => `Riptide_${version}_x64-setup.exe`;
export const portableName = (version) => `Riptide-${version}-portable.zip`;
export const SUMS_NAME = 'SHA256SUMS.txt';
const SAFE_NAME_RE = /^[A-Za-z0-9._-]+$/;

export const downloadUrl = (tag, name) => `https://github.com/${REPO_SLUG}/releases/download/${tag}/${name}`;
export const notesUrl = (tag) => `https://github.com/${REPO_SLUG}/releases/tag/${tag}`;

/** Parses SHA256SUMS.txt ("<64 hex>  <name>" or "<64 hex> *<name>"). Throws on a malformed line or a repeated name. */
export function parseSums(text) {
  const map = new Map();
  for (const line of String(text).split(/\r?\n/)) {
    if (!line.trim()) continue;
    const m = /^([0-9a-fA-F]{64}) [ *]([^\s/\\]+)$/.exec(line.trim());
    if (!m) throw new Error(`SHA256SUMS.txt: malformed line "${line.slice(0, 80)}"`);
    if (map.has(m[2])) throw new Error(`SHA256SUMS.txt: "${m[2]}" listed twice`);
    map.set(m[2], m[1].toLowerCase());
  }
  return map;
}

function fileEntry(release, tag, name, sums) {
  if (!SAFE_NAME_RE.test(name) || name.includes('..')) throw new Error(`${tag}: unsafe asset name "${name}"`);
  const asset = (release.assets ?? []).filter((a) => a.name === name);
  if (asset.length !== 1) throw new Error(`${tag}: expected exactly one asset named ${name}, found ${asset.length}`);
  const url = downloadUrl(tag, name);
  if (asset[0].browser_download_url !== url) throw new Error(`${tag}: ${name} URL is "${asset[0].browser_download_url}", expected "${url}"`);
  if (!Number.isSafeInteger(asset[0].size) || asset[0].size <= 0) throw new Error(`${tag}: ${name} has no valid size`);
  const sha = sums.get(name);
  if (!sha) throw new Error(`${tag}: ${name} is not listed in ${SUMS_NAME}`);
  if (!SHA_RE.test(sha)) throw new Error(`${tag}: bad checksum for ${name}`);
  const digest = asset[0].digest;
  if (digest !== undefined && digest !== null && digest !== `sha256:${sha}`) {
    throw new Error(`${tag}: ${SUMS_NAME} says ${sha} for ${name} but GitHub reports ${digest}`);
  }
  return { name, url, size: asset[0].size, sha256: sha };
}

/**
 * Validates one GitHub release object and returns its manifest entry.
 * @throws Error describing the first rule that failed
 */
export function buildEntry(release, sumsText) {
  const tag = release.tag_name;
  const m = TAG_RE.exec(String(tag));
  if (!m) throw new Error(`tag "${tag}" is not v<version>`);
  const version = m[1];
  const channel = channelOf(version, release.prerelease);
  const publishedAt = String(release.published_at ?? '');
  if (!/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/.test(publishedAt)) throw new Error(`${tag}: bad published_at "${publishedAt}"`);
  if (release.html_url !== undefined && release.html_url !== notesUrl(tag)) throw new Error(`${tag}: release page is "${release.html_url}", expected "${notesUrl(tag)}"`);
  const sumsAsset = (release.assets ?? []).filter((a) => a.name === SUMS_NAME);
  if (sumsAsset.length !== 1) throw new Error(`${tag}: ${SUMS_NAME} is missing`);
  if (sumsText === undefined || sumsText === null) throw new Error(`${tag}: could not read ${SUMS_NAME}`);
  const sums = parseSums(sumsText);
  const sumsUrl = downloadUrl(tag, SUMS_NAME);
  if (sumsAsset[0].browser_download_url !== sumsUrl) throw new Error(`${tag}: ${SUMS_NAME} URL is "${sumsAsset[0].browser_download_url}"`);
  return {
    tag,
    version,
    channel,
    prerelease: channel !== 'stable',
    publishedAt,
    notesUrl: notesUrl(tag),
    installer: fileEntry(release, tag, installerName(version), sums),
    portable: fileEntry(release, tag, portableName(version), sums),
    sums: { name: SUMS_NAME, url: sumsUrl },
  };
}

/**
 * Builds the manifest from raw API data. Drafts and non-version tags (such as the `channel-beta` feed
 * release) are ignored, as are versions older than MIN_VERSION; every other release must validate.
 * @param {object[]} releases GitHub release objects
 * @param {Record<string,string>} sums SHA256SUMS.txt text by tag
 * @returns {{manifest: object, notices: string[]}}
 */
export function buildManifest(releases, sums) {
  if (!Array.isArray(releases)) throw new Error('releases must be an array');
  const notices = [];
  const entries = [];
  const errors = [];
  for (const release of releases) {
    if (release.draft) { notices.push(`skipped draft ${release.tag_name}`); continue; }
    const m = TAG_RE.exec(String(release.tag_name));
    if (!m) { notices.push(`skipped ${release.tag_name} (not a version tag)`); continue; }
    if (compareVersions(m[1], MIN_VERSION) < 0) { notices.push(`skipped ${release.tag_name} (older than ${MIN_VERSION})`); continue; }
    try {
      entries.push(buildEntry(release, sums?.[release.tag_name]));
    } catch (e) {
      errors.push(e.message);
    }
  }
  if (errors.length) throw new Error(`refusing to write the manifest:\n  - ${errors.join('\n  - ')}`);
  if (entries.length === 0) throw new Error('refusing to write the manifest: no valid release found');
  const tags = new Set(entries.map((e) => e.tag));
  if (tags.size !== entries.length) throw new Error('refusing to write the manifest: duplicate tag');
  entries.sort((a, b) => compareVersions(b.version, a.version));
  const newest = (pred) => entries.find(pred)?.tag ?? null;
  const manifest = {
    schemaVersion: SCHEMA_VERSION,
    repo: REPO_SLUG,
    latest: { stable: newest((e) => e.channel === 'stable'), prerelease: newest((e) => e.channel !== 'stable') },
    releases: entries,
  };
  return { manifest, notices };
}

/** The release the site and README feature: the newest stable build, else the newest pre-release. */
export function featured(manifest) {
  const tag = manifest.latest.stable ?? manifest.latest.prerelease;
  return manifest.releases.find((r) => r.tag === tag) ?? null;
}

/**
 * Re-validates a manifest read from disk (build.mjs, check scripts): the same rules, no network.
 * @throws Error when anything is off
 */
export function validateManifest(manifest) {
  if (manifest?.schemaVersion !== SCHEMA_VERSION) throw new Error(`release.json: schemaVersion must be ${SCHEMA_VERSION}`);
  if (manifest.repo !== REPO_SLUG) throw new Error(`release.json: repo must be ${REPO_SLUG}`);
  if (!Array.isArray(manifest.releases) || manifest.releases.length === 0) throw new Error('release.json: no releases');
  for (const r of manifest.releases) {
    const m = TAG_RE.exec(String(r.tag));
    if (!m || m[1] !== r.version) throw new Error(`release.json: bad tag/version ${r.tag}/${r.version}`);
    const v = parseVersion(r.version);
    if (r.channel !== (v.pre ?? 'stable') || r.prerelease !== (r.channel !== 'stable')) throw new Error(`release.json: ${r.tag} has an inconsistent channel`);
    if (r.notesUrl !== notesUrl(r.tag)) throw new Error(`release.json: ${r.tag} notes URL`);
    for (const [key, name] of [['installer', installerName(r.version)], ['portable', portableName(r.version)]]) {
      const f = r[key];
      if (f?.name !== name || f.url !== downloadUrl(r.tag, name) || !SHA_RE.test(f.sha256) || !Number.isSafeInteger(f.size) || f.size <= 0) throw new Error(`release.json: ${r.tag} ${key} entry is not valid`);
    }
    if (r.sums?.name !== SUMS_NAME || r.sums.url !== downloadUrl(r.tag, SUMS_NAME)) throw new Error(`release.json: ${r.tag} sums entry`);
  }
  const tags = manifest.releases.map((r) => r.tag);
  for (const k of ['stable', 'prerelease']) {
    const t = manifest.latest?.[k];
    if (t !== null && !tags.includes(t)) throw new Error(`release.json: latest.${k} points at an unknown tag`);
  }
  return manifest;
}

// ---------------------------------------------------------------------------------------
// Sources: gh / https against the API, or a fixture file.
// ---------------------------------------------------------------------------------------

function ghAvailable() {
  try { execFileSync('gh', ['--version'], { stdio: 'ignore' }); return true; } catch { return false; }
}

async function httpGet(url, accept, token) {
  const headers = { Accept: accept, 'User-Agent': 'riptide-release-manifest', 'X-GitHub-Api-Version': '2022-11-28' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(url, { headers, redirect: 'follow' });
  if (!res.ok) throw new Error(`GET ${url} -> ${res.status}${res.status === 404 && !token ? ' (private repository? set GITHUB_TOKEN or install gh)' : ''}`);
  return res;
}

/** Reads releases and SHA256SUMS.txt texts from GitHub. Uses `gh` when present, else fetch with GITHUB_TOKEN/GH_TOKEN. */
export async function fetchFromGitHub() {
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN || '';
  const useGh = !token && ghAvailable();
  const api = `repos/${REPO_SLUG}/releases`;
  let releases;
  if (useGh) {
    releases = JSON.parse(execFileSync('gh', ['api', '--paginate', '--slurp', `${api}?per_page=100`], { encoding: 'utf8', maxBuffer: 64 << 20 })).flat();
  } else {
    releases = [];
    for (let page = 1; page < 20; page++) {
      const batch = await (await httpGet(`https://api.github.com/${api}?per_page=100&page=${page}`, 'application/vnd.github+json', token)).json();
      releases.push(...batch);
      if (batch.length < 100) break;
    }
  }
  const sums = {};
  for (const r of releases) {
    const a = (r.assets ?? []).find((x) => x.name === SUMS_NAME);
    if (!a || r.draft || !TAG_RE.test(String(r.tag_name))) continue;
    const path = `${api}/assets/${a.id}`;
    sums[r.tag_name] = useGh
      ? execFileSync('gh', ['api', '-H', 'Accept: application/octet-stream', path], { encoding: 'utf8' })
      : await (await httpGet(`https://api.github.com/${path}`, 'application/octet-stream', token)).text();
  }
  return { releases, sums };
}

export const serialize = (manifest) => `${JSON.stringify(manifest, null, 2)}\n`;

async function main(argv) {
  const args = argv.slice(2);
  const opt = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const out = resolve(opt('--out') ?? join(root, 'apps', 'website', 'release.json'));
  const fixture = opt('--fixture');
  const source = fixture ? JSON.parse(readFileSync(resolve(fixture), 'utf8')) : await fetchFromGitHub();
  const { manifest, notices } = buildManifest(source.releases, source.sums);
  validateManifest(manifest);
  for (const n of notices) console.log(`note: ${n}`);
  const text = serialize(manifest);
  if (args.includes('--check')) {
    const current = existsSync(out) ? readFileSync(out, 'utf8') : '';
    if (current !== text) { console.error(`${out} is out of date; run node scripts/gen-release-manifest.mjs`); process.exit(1); }
    console.log('release.json is up to date');
    return;
  }
  writeFileSync(out, text);
  const f = featured(manifest);
  console.log(`wrote ${out}: ${manifest.releases.length} releases, featured ${f.tag} (${f.channel})`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv).catch((e) => { console.error(e.message); process.exit(1); });
}
