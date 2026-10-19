import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  DEFAULT_RULES,
  FILE_TYPES,
  FILE_TYPE_KEYS,
  MIB,
  decide,
  extensionOf,
  filenameFromDisposition,
  filenameFromUrl,
  hostOf,
  isAttachmentDisposition,
  isDirectFileLink,
  isDownloadMime,
  isRenderableMime,
  normaliseRules,
  normaliseSite,
  parseExtensionList,
  parseSiteList,
  siteMatches,
} from './rules';
import type { Rules } from './rules';

function rules(over: Partial<Rules> = {}): Rules {
  return { ...DEFAULT_RULES, types: { ...DEFAULT_RULES.types }, ...over };
}

test('file type groups do not overlap', () => {
  const seen = new Map<string, string>();
  for (const key of FILE_TYPE_KEYS) {
    for (const ext of FILE_TYPES[key]) {
      assert.equal(seen.get(ext), undefined, `${ext} is in both ${seen.get(ext)} and ${key}`);
      seen.set(ext, key);
      assert.match(ext, /^[a-z0-9]{1,12}$/);
    }
  }
});

test('hostOf accepts http(s) only', () => {
  assert.equal(hostOf('https://Example.COM/a/b.zip'), 'example.com');
  assert.equal(hostOf('http://localhost:8080/x'), 'localhost');
  assert.equal(hostOf('ftp://example.com/x'), null);
  assert.equal(hostOf('blob:https://example.com/uuid'), null);
  // eslint-disable-next-line no-script-url -- the point is that such URLs are rejected
  assert.equal(hostOf('javascript:alert(1)'), null);
  assert.equal(hostOf('not a url'), null);
});

test('filenameFromUrl and extensionOf', () => {
  assert.equal(filenameFromUrl('https://a/b/c%20d.tar.gz?x=1#f'), 'c d.tar.gz');
  assert.equal(filenameFromUrl('https://a/'), '');
  assert.equal(extensionOf('https://a/b/c.tar.gz?x=1'), 'gz');
  assert.equal(extensionOf('report.PDF'), 'pdf');
  assert.equal(extensionOf('C:\\x\\y.exe'), 'exe');
  assert.equal(extensionOf('.hidden'), '');
  assert.equal(extensionOf('noext'), '');
  assert.equal(extensionOf('https://a/download?id=5'), '');
  assert.equal(extensionOf('weird.verylongextension'), '');
});

test('site patterns match host and subdomains, never partial labels', () => {
  assert.equal(normaliseSite(' HTTPS://*.Example.com:443/path?x '), 'example.com');
  assert.equal(normaliseSite('bad host!'), '');
  assert.ok(siteMatches('example.com', 'example.com'));
  assert.ok(siteMatches('example.com', 'cdn.files.example.com'));
  assert.ok(siteMatches('*.example.com', 'www.example.com'));
  assert.ok(!siteMatches('example.com', 'notexample.com'));
  assert.ok(!siteMatches('', 'example.com'));
  assert.deepEqual(parseSiteList('a.com, b.org\nhttps://c.net/x ; a.com'), ['a.com', 'b.org', 'c.net']);
  assert.deepEqual(parseExtensionList('.ZIP, rar *.7z  bad.ext,'), ['zip', 'rar', '7z']);
});

test('MIME classification', () => {
  assert.ok(isRenderableMime('text/html; charset=utf-8'));
  assert.ok(isRenderableMime('image/png'));
  assert.ok(isRenderableMime('application/pdf'));
  assert.ok(isRenderableMime('application/xhtml+xml'));
  assert.ok(isRenderableMime(''));
  assert.ok(isDownloadMime('application/octet-stream'));
  assert.ok(isDownloadMime('application/zip'));
  assert.ok(isDownloadMime('application/x-msdownload'));
  assert.ok(!isDownloadMime('text/plain'));
  assert.ok(!isDownloadMime(undefined));
});

test('Content-Disposition parsing', () => {
  assert.ok(isAttachmentDisposition('attachment; filename="a.zip"'));
  assert.ok(isAttachmentDisposition('Attachment'));
  assert.ok(!isAttachmentDisposition('inline; filename="a.pdf"'));
  assert.equal(filenameFromDisposition('attachment; filename="a b.zip"'), 'a b.zip');
  assert.equal(filenameFromDisposition('attachment; filename=plain.zip'), 'plain.zip');
  assert.equal(
    filenameFromDisposition("attachment; filename=\"fallback.txt\"; filename*=UTF-8''%D9%85%D9%84%D9%81.zip"),
    'ملف.zip',
  );
  assert.equal(filenameFromDisposition('inline'), '');
  assert.equal(filenameFromDisposition(undefined), '');
});

test('decide: defaults take a large archive and skip a small one', () => {
  const r = rules();
  assert.deepEqual(decide(r, { url: 'https://a/b.zip', size: 5 * MIB }), { take: true, reason: 'extension' });
  assert.deepEqual(decide(r, { url: 'https://a/b.zip', size: 100 }), { take: false, reason: 'too_small' });
  assert.deepEqual(decide(r, { url: 'https://a/b.zip' }), { take: true, reason: 'extension' }, 'unknown size is taken');
  assert.deepEqual(decide(r, { url: 'https://a/b.zip', size: -1 }), { take: true, reason: 'extension' });
});

test('decide: the suggested filename wins over the URL', () => {
  const r = rules();
  assert.deepEqual(decide(r, { url: 'https://a/download?id=5', filename: 'movie.mkv', size: 2 * MIB }), {
    take: true,
    reason: 'extension',
  });
});

test('decide: switches and lists', () => {
  const big = { url: 'https://files.example.com/x.zip', size: 10 * MIB };
  assert.equal(decide(rules({ enabled: false }), big).reason, 'disabled');
  assert.equal(decide(rules(), big, true).reason, 'paused');
  assert.equal(decide(rules({ neverAskSites: ['example.com'] }), big).reason, 'never_ask');
  assert.equal(decide(rules(), { ...big, altHeld: true }).reason, 'alt_bypass');
  assert.equal(decide(rules({ altBypass: false }), { ...big, altHeld: true }).take, true);
  assert.equal(decide(rules({ denySites: ['example.com'] }), big).reason, 'site_denied');
  assert.equal(decide(rules({ allowSites: ['other.org'] }), big).reason, 'site_not_allowed');
  assert.equal(decide(rules({ allowSites: ['example.com'] }), big).take, true);
  assert.equal(decide(rules(), { url: 'ftp://a/x.zip', size: 10 * MIB }).reason, 'not_http');
  // never-ask beats everything after the master switch, including alt / lists.
  assert.equal(decide(rules({ neverAskSites: ['example.com'], denySites: ['example.com'] }), big).reason, 'never_ask');
});

test('decide: file types', () => {
  const r = rules();
  assert.equal(decide(r, { url: 'https://a/photo.png', size: 10 * MIB }).reason, 'type', 'images off by default');
  assert.equal(decide(rules({ types: { ...r.types, images: true } }), { url: 'https://a/photo.png', size: 10 * MIB }).take, true);
  assert.equal(decide(r, { url: 'https://a/page.html', size: 10 * MIB }).reason, 'type');
  assert.deepEqual(decide(rules({ customExtensions: ['bin'] }), { url: 'https://a/fw.bin', size: 10 * MIB }), {
    take: true,
    reason: 'extension',
  });
  // Attachments without a known extension are downloads.
  assert.deepEqual(decide(r, { url: 'https://a/get?id=1', attachment: true, size: 10 * MIB }), {
    take: true,
    reason: 'attachment',
  });
  // ... unless the extension is a known, switched-off type.
  assert.equal(decide(r, { url: 'https://a/get?id=1', filename: 'x.png', attachment: true, size: 10 * MIB }).reason, 'type');
  assert.deepEqual(decide(r, { url: 'https://a/get?id=1', mime: 'application/octet-stream', size: 10 * MIB }), {
    take: true,
    reason: 'mime',
  });
  assert.equal(decide(r, { url: 'https://a/get?id=1', mime: 'text/html', size: 10 * MIB }).reason, 'type');
  assert.equal(decide(r, { url: 'https://a/get?id=1', attachment: true, size: 10 }).reason, 'too_small');
});

test('isDirectFileLink follows the rules', () => {
  assert.ok(isDirectFileLink('https://a/b.zip', rules()));
  assert.ok(!isDirectFileLink('https://a/b.png', rules()));
  assert.ok(!isDirectFileLink('https://a/', rules()));
  assert.ok(!isDirectFileLink('mailto:x@y.z', rules()));
});

test('normaliseRules repairs garbage and keeps valid values', () => {
  assert.deepEqual(normaliseRules(undefined), DEFAULT_RULES);
  assert.deepEqual(normaliseRules('nope'), DEFAULT_RULES);
  const n = normaliseRules({
    enabled: 'yes',
    minSize: -5,
    types: { images: true, archives: 'no', bogus: true },
    customExtensions: ['.BIN', 42, 'bad ext', 'bin'],
    allowSites: ['https://A.com/x', ''],
    denySites: 'b.com',
    altBypass: false,
    neverAskSites: ['*.c.org'],
  });
  assert.equal(n.enabled, true);
  assert.equal(n.minSize, MIB);
  assert.equal(n.types.images, true);
  assert.equal(n.types.archives, true);
  assert.deepEqual(n.customExtensions, ['bin']);
  assert.deepEqual(n.allowSites, ['a.com']);
  assert.deepEqual(n.denySites, []);
  assert.equal(n.altBypass, false);
  assert.deepEqual(n.neverAskSites, ['c.org']);
  assert.equal(normaliseRules({ minSize: 2.9 }).minSize, 2);
});

test('ignored extensions stay in the browser even when their group is on', () => {
  const r = rules({ ignoreExtensions: ['zip'] });
  assert.deepEqual(decide(r, { url: 'https://a/x.zip', size: 10 * MIB }), { take: false, reason: 'ignored_type' });
  assert.deepEqual(decide(r, { url: 'https://a/x.rar', size: 10 * MIB }), { take: true, reason: 'extension' });
  assert.deepEqual(decide(r, { url: 'https://a/dl?id=1', filename: 'x.zip', attachment: true }), {
    take: false,
    reason: 'ignored_type',
  });
});

test('ignored extensions are normalised from storage', () => {
  assert.deepEqual(normaliseRules({ ignoreExtensions: ['.ISO', 'bad ext', 5, 'iso'] }).ignoreExtensions, ['iso']);
  assert.deepEqual(normaliseRules({}).ignoreExtensions, []);
});

test('rule order: never-ask beats everything, Alt beats the lists', () => {
  const r = rules({ neverAskSites: ['a.com'], denySites: [], allowSites: ['b.com'] });
  assert.equal(decide(r, { url: 'https://a.com/x.zip', size: 9 * MIB }).take, false);
  assert.deepEqual(decide(rules({ altBypass: true }), { url: 'https://a.com/x.zip', altHeld: true }), {
    take: false,
    reason: 'alt_bypass',
  });
  assert.deepEqual(decide(rules({ altBypass: false }), { url: 'https://a.com/x.zip', altHeld: true, size: 9 * MIB }), {
    take: true,
    reason: 'extension',
  });
});
