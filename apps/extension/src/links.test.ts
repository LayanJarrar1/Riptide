import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  LINK_EXTENSIONS,
  LINK_GROUPS,
  MAX_BATCH,
  MAX_CANDIDATES,
  buildItems,
  countByGroup,
  filterItems,
  groupOf,
  largestFromSrcset,
  mapLimit,
  mayCarryCookies,
  parseSrcset,
  sanitiseBatch,
  visibleRange,
} from './links';
import type { LinkGroup } from './links';

const none = new Set<LinkGroup>();

test('groups mirror the core category table', () => {
  let src = '';
  try {
    src = readFileSync(join(process.cwd(), '..', '..', 'crates', 'riptide-core', 'src', 'category.rs'), 'utf8');
  } catch {
    return; // extension built outside the monorepo
  }
  const names: Record<string, Exclude<LinkGroup, 'other'>> = {
    Compressed: 'archives',
    Documents: 'documents',
    Music: 'audio',
    Video: 'video',
    Programs: 'programs',
    Images: 'images',
  };
  for (const [rust, group] of Object.entries(names)) {
    const m = new RegExp(`Category::${rust} => &\\[([^\\]]*)\\]`).exec(src);
    assert.ok(m, rust);
    const exts = [...(m[1] ?? '').matchAll(/"([a-z0-9]+)"/g)].map((x) => x[1]);
    assert.deepEqual([...LINK_EXTENSIONS[group]].sort(), exts.sort(), rust);
  }
});

test('groupOf', () => {
  assert.equal(groupOf('ZIP'), 'archives');
  assert.equal(groupOf('mp3'), 'audio');
  assert.equal(groupOf('exe'), 'programs');
  assert.equal(groupOf('html'), 'other');
  assert.equal(groupOf(''), 'other');
  assert.equal(LINK_GROUPS.length, 7);
});

test('buildItems keeps http(s) only, strips fragments, de-duplicates, guesses names', () => {
  const items = buildItems([
    { url: 'https://a.com/files/Report%20Q1.pdf#page=2', text: '  Q1\n report ', source: 'link' },
    { url: 'https://a.com/files/Report%20Q1.pdf', text: 'again', source: 'link' },
    { url: ['java', 'script:void(0)'].join(''), source: 'link' },
    { url: 'mailto:x@y.z', source: 'link' },
    { url: 'ftp://a.com/x.zip', source: 'link' },
    { url: 'https://u:p@a.com/x.zip', source: 'link' },
    { url: 'https://a.com/img/cat.JPG', text: '', source: 'image' },
    { url: 'https://a.com/page', text: '', source: 'link' },
  ]);
  assert.deepEqual(
    items.map((i) => [i.url, i.name, i.ext, i.group]),
    [
      ['https://a.com/files/Report%20Q1.pdf', 'Q1 report', 'pdf', 'documents'],
      ['https://a.com/img/cat.JPG', 'cat.JPG', 'jpg', 'images'],
      ['https://a.com/page', 'page', '', 'other'],
    ],
  );
});

test('a later label replaces the guessed name once', () => {
  const [i] = buildItems([
    { url: 'https://a.com/f.zip', text: '', source: 'link' },
    { url: 'https://a.com/f.zip', text: 'Home', source: 'link' },
    { url: 'https://a.com/f.zip', text: 'Other', source: 'link' },
  ]);
  assert.equal(i?.name, 'Home');
});

test('very long urls are dropped and the list is capped', () => {
  assert.equal(buildItems([{ url: 'https://a.com/' + 'x'.repeat(9000), source: 'link' }]).length, 0);
  const many = Array.from({ length: MAX_CANDIDATES + 50 }, (_, n) => ({
    url: `https://a.com/${n}.zip`,
    source: 'link' as const,
  }));
  assert.equal(buildItems(many).length, MAX_CANDIDATES);
});

test('srcset parsing', () => {
  const base = 'https://a.com/dir/';
  assert.deepEqual(parseSrcset('a.jpg 1x, /b.jpg 2x', base), ['https://a.com/dir/a.jpg', 'https://a.com/b.jpg']);
  assert.deepEqual(parseSrcset('https://c.com/i.jpg?a=1,2 480w', base), ['https://c.com/i.jpg?a=1,2']);
  assert.equal(largestFromSrcset('s.jpg 480w, m.jpg 800w, l.jpg 1600w', base), 'https://a.com/dir/l.jpg');
  assert.equal(largestFromSrcset('', base), null);
});

test('filters by group and text, and counts per group', () => {
  const items = buildItems([
    { url: 'https://a.com/a.zip', source: 'link' },
    { url: 'https://a.com/b.mp4', text: 'Holiday', source: 'link' },
    { url: 'https://a.com/c.png', source: 'image' },
    { url: 'https://a.com/d', source: 'link' },
  ]);
  assert.equal(filterItems(items, { groups: none, text: '' }).length, 4);
  assert.deepEqual(
    filterItems(items, { groups: new Set<LinkGroup>(['video', 'archives']), text: '' }).map((i) => i.ext),
    ['zip', 'mp4'],
  );
  assert.deepEqual(filterItems(items, { groups: none, text: ' HOLI ' }).map((i) => i.ext), ['mp4']);
  assert.deepEqual(
    filterItems(items, { groups: new Set<LinkGroup>(['other']), text: 'a.com' }).map((i) => i.url),
    ['https://a.com/d'],
  );
  assert.deepEqual(countByGroup(items), {
    archives: 1,
    video: 1,
    audio: 0,
    documents: 0,
    images: 1,
    programs: 0,
    other: 1,
  });
});

test('visibleRange windows a long list', () => {
  assert.deepEqual(visibleRange(0, 0, 300, 30), { start: 0, end: 0 });
  assert.deepEqual(visibleRange(1000, 0, 300, 30), { start: 0, end: 16 });
  assert.deepEqual(visibleRange(1000, 3000, 300, 30), { start: 94, end: 116 });
  assert.deepEqual(visibleRange(1000, 29_900, 300, 30), { start: 990, end: 1000 });
  assert.deepEqual(visibleRange(10, -50, 300, 30), { start: 0, end: 10 });
});

test('cookies only for the page origin', () => {
  const page = 'https://a.com/list';
  assert.equal(mayCarryCookies('https://a.com/x.zip', page), true);
  assert.equal(mayCarryCookies('https://cdn.a.com/x.zip', page), false);
  assert.equal(mayCarryCookies('http://a.com/x.zip', page), false);
  assert.equal(mayCarryCookies('https://a.com:8443/x.zip', page), false);
  assert.equal(mayCarryCookies('https://a.com/x.zip', undefined), false);
  assert.equal(mayCarryCookies(['java', 'script:1'].join(''), page), false);
});

test('sanitiseBatch dedupes, drops junk, caps', () => {
  assert.deepEqual(sanitiseBatch(['https://a/x#1', 'https://a/x#2', 'file:///c', 'nope']), ['https://a/x']);
  const many = Array.from({ length: MAX_BATCH + 100 }, (_, n) => `https://a.com/${n}`);
  assert.equal(sanitiseBatch(many).length, MAX_BATCH);
});

test('mapLimit never exceeds the limit, keeps order, and can stop early', async () => {
  let running = 0;
  let peak = 0;
  const out = await mapLimit([1, 2, 3, 4, 5, 6, 7, 8], 3, async (n) => {
    running++;
    peak = Math.max(peak, running);
    await new Promise((r) => setTimeout(r, 5));
    running--;
    return n * 2;
  });
  assert.equal(peak, 3);
  assert.deepEqual(out, [2, 4, 6, 8, 10, 12, 14, 16]);
  let stop = false;
  const partial = await mapLimit(
    [1, 2, 3, 4],
    1,
    async (n) => {
      if (n === 2) stop = true;
      return n;
    },
    () => stop,
  );
  assert.deepEqual(partial, [1, 2, undefined, undefined]);
  assert.deepEqual(await mapLimit([], 4, async (n: number) => n), []);
});
