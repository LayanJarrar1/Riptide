// node --test scripts/gen-release-manifest.test.mjs
// Manifest generator, doc-sync check and release-link check, all against in-memory fixtures (no network).
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { collectVersions, findProblems } from './check-doc-sync.mjs';
import { linksIn, manifestLinks, run } from './check-release-links.mjs';
import {
  buildManifest, channelOf, compareVersions, featured, parseSums, parseVersion, validateManifest,
} from './gen-release-manifest.mjs';

const BASE = 'https://github.com/LayanJarrar1/Riptide/releases/download';
const HEX = (c) => c.repeat(64);

/** A valid GitHub release object plus its SHA256SUMS text. */
function fixture(version, { prerelease = true, published = '2026-10-20T09:34:29Z' } = {}) {
  const tag = `v${version}`;
  const names = { setup: `Riptide_${version}_x64-setup.exe`, zip: `Riptide-${version}-portable.zip`, sums: 'SHA256SUMS.txt' };
  const asset = (name, size, digest) => ({ name, size, browser_download_url: `${BASE}/${tag}/${name}`, ...(digest ? { digest: `sha256:${digest}` } : {}) });
  const release = {
    tag_name: tag, draft: false, prerelease, published_at: published,
    html_url: `https://github.com/LayanJarrar1/Riptide/releases/tag/${tag}`,
    assets: [asset(names.setup, 1000, HEX('a')), asset(names.zip, 2000, HEX('b')), asset(names.sums, 200)],
  };
  const sums = `${HEX('a')}  ${names.setup}\n${HEX('b')}  ${names.zip}\n`;
  return { release, sums, tag };
}

const build = (...fx) => buildManifest(fx.map((f) => f.release), Object.fromEntries(fx.map((f) => [f.tag, f.sums])));

test('channel comes from a strict version pattern and the prerelease flag', () => {
  assert.equal(channelOf('1.2.3', false), 'stable');
  assert.equal(channelOf('0.1.0-alpha.4', true), 'alpha');
  assert.equal(channelOf('0.2.0-beta.1', true), 'beta');
  assert.throws(() => channelOf('1.2.3', true), /stable but GitHub marks/);
  assert.throws(() => channelOf('0.1.0-alpha.4', false), /does not mark/);
  for (const bad of ['0.1.0-rc.1', '01.2.3', '1.2', 'v1.2.3', '1.2.3+build', '1.2.3-alpha', '1.2.3-alpha.01', '1.2.3-alpha.1.2', ' 1.2.3', '1.2.3\n', '../1.2.3']) {
    assert.throws(() => channelOf(bad, false), /not a Riptide version/, bad);
  }
});

test('version precedence: alpha < beta < stable, numeric not lexical', () => {
  const order = ['0.1.0-alpha.2', '0.1.0-alpha.10', '0.1.0-beta.1', '0.1.0', '0.1.1-alpha.1', '1.0.0'];
  for (let i = 1; i < order.length; i++) assert.ok(compareVersions(order[i], order[i - 1]) > 0, `${order[i]} > ${order[i - 1]}`);
  assert.equal(parseVersion('2.0.0').pre, null);
});

test('builds a manifest with latest stable and latest pre-release', () => {
  const { manifest } = build(
    fixture('0.1.0-alpha.4', { published: '2026-10-20T09:00:00Z' }),
    fixture('0.1.0-alpha.3', { published: '2026-10-18T09:00:00Z' }),
    fixture('0.2.0', { prerelease: false, published: '2026-11-01T09:00:00Z' }),
  );
  assert.deepEqual(manifest.latest, { stable: 'v0.2.0', prerelease: 'v0.1.0-alpha.4' });
  assert.deepEqual(manifest.releases.map((r) => r.tag), ['v0.2.0', 'v0.1.0-alpha.4', 'v0.1.0-alpha.3']);
  assert.equal(featured(manifest).tag, 'v0.2.0');
  const alpha = manifest.releases[1];
  assert.equal(alpha.channel, 'alpha');
  assert.equal(alpha.installer.sha256, HEX('a'));
  assert.equal(alpha.portable.size, 2000);
  assert.equal(alpha.installer.url, `${BASE}/v0.1.0-alpha.4/Riptide_0.1.0-alpha.4_x64-setup.exe`);
  validateManifest(manifest);
});

test('with only pre-releases the newest pre-release is featured', () => {
  const { manifest } = build(fixture('0.1.0-alpha.4'));
  assert.equal(manifest.latest.stable, null);
  assert.equal(featured(manifest).version, '0.1.0-alpha.4');
});

test('drafts, feed tags and legacy releases are skipped, not emitted', () => {
  const ok = fixture('0.1.0-alpha.4');
  const draft = fixture('0.1.0-alpha.5');
  draft.release.draft = true;
  const feed = { tag_name: 'channel-beta', prerelease: true, draft: false, assets: [] };
  const legacy = { tag_name: 'v0.1.0-alpha.1', prerelease: true, draft: false, assets: [] };
  const { manifest, notices } = buildManifest([ok.release, draft.release, feed, legacy], { [ok.tag]: ok.sums });
  assert.deepEqual(manifest.releases.map((r) => r.tag), ['v0.1.0-alpha.4']);
  assert.equal(notices.length, 3);
});

test('refuses a download URL from another repository', () => {
  const f = fixture('0.1.0-alpha.4');
  f.release.assets[0].browser_download_url = f.release.assets[0].browser_download_url.replace('LayanJarrar1/Riptide', 'evil/Riptide');
  assert.throws(() => build(f), /expected "https:\/\/github\.com\/LayanJarrar1\/Riptide\/releases\/download\/v0\.1\.0-alpha\.4\//);
});

test('refuses a download URL under another tag', () => {
  const f = fixture('0.1.0-alpha.4');
  f.release.assets[1].browser_download_url = f.release.assets[1].browser_download_url.replace('/v0.1.0-alpha.4/', '/v0.1.0-alpha.3/');
  assert.throws(() => build(f), /URL is/);
});

test('refuses path traversal and odd characters in an asset name', () => {
  for (const name of ['../Riptide_0.1.0-alpha.4_x64-setup.exe', 'a/b.exe', 'x%2e%2e', 'Riptide_0.1.0-alpha.4_x64-setup.exe ', 'Riptide_0.1.0-alpha.4_x64-setup.exe?x=1']) {
    const f = fixture('0.1.0-alpha.4');
    f.release.assets[0].name = name;
    f.release.assets[0].browser_download_url = `${BASE}/v0.1.0-alpha.4/${name}`;
    assert.throws(() => build(f), /asset|unsafe|exactly one/, name);
  }
});

test('refuses an installer that does not follow Riptide_<version>_x64-setup.exe', () => {
  const f = fixture('0.1.0-alpha.4');
  f.release.assets[0].name = 'Riptide_0.1.0-alpha.3_x64-setup.exe';
  f.release.assets[0].browser_download_url = `${BASE}/v0.1.0-alpha.4/Riptide_0.1.0-alpha.3_x64-setup.exe`;
  assert.throws(() => build(f), /exactly one asset named Riptide_0\.1\.0-alpha\.4_x64-setup\.exe/);
});

test('refuses a tag that does not match the pattern, or a release page for another tag', () => {
  const f = fixture('0.1.0-alpha.4');
  f.release.html_url = 'https://github.com/LayanJarrar1/Riptide/releases/tag/v9.9.9';
  assert.throws(() => build(f), /release page/);
  const g = fixture('0.1.0-alpha.4');
  g.release.tag_name = 'v0.1.0-alpha.4/../x';
  assert.deepEqual(buildManifest([fixture('0.1.0-alpha.4').release, g.release], { 'v0.1.0-alpha.4': fixture('0.1.0-alpha.4').sums }).manifest.releases.length, 1);
});

test('refuses a checksum that disagrees with the digest GitHub reports', () => {
  const f = fixture('0.1.0-alpha.4');
  f.sums = f.sums.replace(HEX('a'), HEX('c'));
  assert.throws(() => build(f), /says c{64} for Riptide_0\.1\.0-alpha\.4_x64-setup\.exe but GitHub reports sha256:a{64}/);
});

test('refuses missing, malformed or duplicated SHA256SUMS content', () => {
  const missing = fixture('0.1.0-alpha.4');
  missing.sums = `${HEX('a')}  Riptide_0.1.0-alpha.4_x64-setup.exe\n`;
  assert.throws(() => build(missing), /portable\.zip is not listed/);
  assert.throws(() => parseSums('not a checksum line'), /malformed/);
  assert.throws(() => parseSums(`${HEX('a')}  x.zip\n${HEX('b')}  x.zip`), /twice/);
  assert.throws(() => parseSums(`${HEX('a')}  ../x.zip`), /malformed/);
  const noSums = fixture('0.1.0-alpha.4');
  noSums.release.assets.pop();
  assert.throws(() => build(noSums), /SHA256SUMS\.txt is missing/);
  assert.equal(parseSums(`${HEX('A')} *x.zip\r\n`).get('x.zip'), HEX('a'));
});

test('refuses a pre-release flag that contradicts the version, and bad dates and sizes', () => {
  assert.throws(() => build(fixture('0.2.0', { prerelease: true })), /stable but GitHub marks/);
  assert.throws(() => build(fixture('0.2.0-beta.1', { prerelease: false })), /does not mark/);
  const date = fixture('0.1.0-alpha.4', { published: 'yesterday' });
  assert.throws(() => build(date), /published_at/);
  const size = fixture('0.1.0-alpha.4');
  size.release.assets[0].size = 0;
  assert.throws(() => build(size), /valid size/);
});

test('one bad release means nothing is emitted', () => {
  const good = fixture('0.1.0-alpha.4');
  const bad = fixture('0.1.0-alpha.5');
  bad.sums = '';
  assert.throws(() => build(good, bad), /refusing to write the manifest/);
  assert.throws(() => buildManifest([], {}), /no valid release/);
});

test('validateManifest rejects a tampered file', () => {
  const { manifest } = build(fixture('0.1.0-alpha.4'));
  const clone = () => JSON.parse(JSON.stringify(manifest));
  const a = clone(); a.releases[0].installer.url = 'https://evil.example/x.exe';
  assert.throws(() => validateManifest(a), /installer entry/);
  const b = clone(); b.repo = 'evil/Riptide';
  assert.throws(() => validateManifest(b), /repo/);
  const c = clone(); c.releases[0].channel = 'stable';
  assert.throws(() => validateManifest(c), /inconsistent channel/);
  const d = clone(); d.releases[0].installer.sha256 = 'nope';
  assert.throws(() => validateManifest(d), /installer entry/);
  const e = clone(); e.latest.stable = 'v9.9.9';
  assert.throws(() => validateManifest(e), /unknown tag/);
});

// ---- doc sync ------------------------------------------------------------------------------

function docsFixture(over = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'riptide-docsync-'));
  const v = (k, d) => over[k] ?? d;
  const ver = '0.1.0-alpha.4';
  const { manifest } = build(fixture(v('manifest', ver)));
  mkdirSync(join(dir, 'apps/website'), { recursive: true });
  mkdirSync(join(dir, 'apps/desktop/src-tauri'), { recursive: true });
  writeFileSync(join(dir, 'apps/website/release.json'), JSON.stringify(manifest));
  const rv = v('readme', ver);
  writeFileSync(join(dir, 'README.md'), `[i](${BASE}/v${rv}/Riptide_${rv}_x64-setup.exe) [p](${BASE}/v${rv}/Riptide-${rv}-portable.zip)\n`);
  writeFileSync(join(dir, 'CHANGELOG.md'), `# Changelog\n\n## [Unreleased]\n\n## [${v('changelog', ver)}] - 2026-10-19\n\n## [0.0.9] - 2026-01-01\n`);
  writeFileSync(join(dir, 'Cargo.toml'), `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${v('cargo', ver)}"\nedition = "2021"\n`);
  writeFileSync(join(dir, 'apps/desktop/src-tauri/tauri.conf.json'), JSON.stringify(over.tauri === undefined ? { productName: 'Riptide' } : { version: over.tauri }));
  writeFileSync(join(dir, 'apps/desktop/package.json'), JSON.stringify({ version: v('pkg', ver) }));
  writeFileSync(join(dir, 'apps/website/build.mjs'), `const RELEASE_VERSION = '${v('build', ver)}';\n`);
  return dir;
}

test('doc sync passes when every place agrees (tauri.conf.json without a version inherits Cargo)', () => {
  assert.deepEqual(findProblems(collectVersions(docsFixture())), []);
  assert.deepEqual(findProblems(collectVersions(docsFixture({ tauri: '0.1.0-alpha.4' }))), []);
});

test('doc sync names every place that disagrees', () => {
  for (const [key, label] of [['readme', 'README.md'], ['changelog', 'CHANGELOG.md'], ['cargo', 'Cargo.toml'], ['tauri', 'tauri.conf.json'], ['pkg', 'package.json'], ['build', 'build.mjs']]) {
    const problems = findProblems(collectVersions(docsFixture({ [key]: '0.1.0-alpha.5' })));
    assert.equal(problems.length, 1, key);
    assert.match(problems[0], new RegExp(label.replace('.', '\\.')), key);
  }
  assert.match(findProblems(collectVersions(docsFixture({ manifest: '0.1.0-alpha.5' })))[0], /says 0\.1\.0-alpha\.5/);
});

// ---- release links -------------------------------------------------------------------------

test('link check collects manifest and text links, drops queries, ignores other repos', () => {
  const { manifest } = build(fixture('0.1.0-alpha.4'));
  assert.equal(manifestLinks(manifest).length, 4);
  const text = `<a href="${BASE}/v0.1.0-alpha.4/x.exe?download=1">x</a> see (${BASE}/v1/y.zip). https://github.com/other/Riptide/releases/download/v1/z.zip`;
  assert.deepEqual(linksIn(text), [`${BASE}/v0.1.0-alpha.4/x.exe`, `${BASE}/v1/y.zip`]);
});

const reply = (status) => ({ status, body: { cancel: async () => {} } });

test('link check skips with a notice while the repository is private', async () => {
  const seen = [];
  const result = await run(['https://github.com/LayanJarrar1/Riptide/releases/tag/v1'], { fetchImpl: async (u) => { seen.push(u); return reply(404); } });
  assert.equal(result.status, 'skipped');
  assert.equal(seen.length, 1);
  assert.equal((await run([], { fetchImpl: async () => reply(404), requirePublic: true })).status, 'failed');
});

test('link check reports every non-200 link once the repository is public', async () => {
  const links = ['https://github.com/LayanJarrar1/Riptide/releases/tag/v1', 'https://github.com/LayanJarrar1/Riptide/releases/download/v1/gone.exe', 'https://github.com/LayanJarrar1/Riptide/releases/download/v1/ok.exe'];
  const fetchImpl = async (u) => reply(u.includes('api.github.com') ? 200 : u.endsWith('gone.exe') ? 404 : u.endsWith('ok.exe') ? 206 : 200);
  const result = await run(links, { fetchImpl });
  assert.equal(result.status, 'failed');
  assert.deepEqual(result.failures, [`404  ${links[1]}`]);
  assert.equal((await run([links[2]], { fetchImpl })).status, 'ok');
});
