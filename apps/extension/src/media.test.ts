import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  buildMenu,
  classifyMediaResponse,
  cookieOriginOf,
  isMediaHost,
  isMediaKey,
  isSubstantialVideo,
  isVideoPageUrl,
  withSite,
} from './media';
import type { MediaChoice } from './protocol';

test('known hosts and their subdomains', () => {
  assert.ok(isMediaHost('www.youtube.com'));
  assert.ok(isMediaHost('m.youtube.com.'));
  assert.ok(isMediaHost('youtu.be'));
  assert.ok(isMediaHost('player.vimeo.com'));
  assert.ok(!isMediaHost('notyoutube.com'));
  assert.ok(!isMediaHost('youtube.com.evil.example'));
  assert.ok(!isMediaHost('example.com'));
});

test('quality keys mirror MediaSelection::parse', () => {
  for (const k of ['best', 'audio-m4a', 'audio-mp3', 'h1', 'h720', 'h1080', 'h8640']) assert.ok(isMediaKey(k), k);
  for (const k of ['', 'h0', 'h8641', 'h12345', 'H720', 'h-1', 'bestvideo', 'audio-flac', 'h720 ', '-f best']) {
    assert.ok(!isMediaKey(k), k);
  }
});

test('video page URLs: watch pages yes, home and channel lists no', () => {
  assert.ok(isVideoPageUrl('https://www.youtube.com/watch?v=abc'));
  assert.ok(isVideoPageUrl('https://www.youtube.com/shorts/abc'));
  assert.ok(isVideoPageUrl('https://youtu.be/abc'));
  assert.ok(isVideoPageUrl('https://vimeo.com/12345'));
  assert.ok(!isVideoPageUrl('https://www.youtube.com/'));
  assert.ok(!isVideoPageUrl('https://www.youtube.com/watch'));
  assert.ok(!isVideoPageUrl('https://www.youtube.com/feed/subscriptions'));
  assert.ok(!isVideoPageUrl('https://vimeo.com/'));
  assert.ok(!isVideoPageUrl('https://example.com/watch?v=abc'));
  assert.ok(!isVideoPageUrl('ftp://youtube.com/watch?v=abc'));
  assert.ok(!isVideoPageUrl('not a url'));
});

test('media responses by MIME and extension', () => {
  assert.equal(classifyMediaResponse('https://a/b', 'video/mp4'), 'progressive');
  assert.equal(classifyMediaResponse('https://a/b', 'audio/mpeg; charset=x'), 'progressive');
  assert.equal(classifyMediaResponse('https://a/x.m3u8?t=1', 'text/plain'), 'hls');
  assert.equal(classifyMediaResponse('https://a/x', 'application/vnd.apple.mpegurl'), 'hls');
  assert.equal(classifyMediaResponse('https://a/x.mpd', undefined), 'dash');
  assert.equal(classifyMediaResponse('https://a/x', 'application/dash+xml'), 'dash');
  assert.equal(classifyMediaResponse('https://a/x.mp4', 'application/octet-stream'), 'progressive');
  assert.equal(classifyMediaResponse('https://a/x.webm', ''), 'progressive');
  // pieces and non-media
  assert.equal(classifyMediaResponse('https://a/seg1.ts', 'video/mp2t'), null);
  assert.equal(classifyMediaResponse('https://a/seg1.ts', 'application/octet-stream'), null);
  assert.equal(classifyMediaResponse('https://a/x.mp4', 'text/html'), null);
  assert.equal(classifyMediaResponse('https://a/x.zip', 'application/zip'), null);
  assert.equal(classifyMediaResponse('not a url', null), null);
});

test('substantial video size', () => {
  assert.ok(isSubstantialVideo(640, 360));
  assert.ok(!isSubstantialVideo(120, 90));
  assert.ok(!isSubstantialVideo(0, 0));
});

test('cookie origin is scheme, host and port of the page', () => {
  assert.equal(cookieOriginOf('https://www.youtube.com/watch?v=x'), 'https://www.youtube.com');
  assert.equal(cookieOriginOf('http://localhost:8080/a'), 'http://localhost:8080');
  assert.equal(cookieOriginOf(['javascript', 'alert(1)'].join(':')), null);
  assert.equal(cookieOriginOf('blob:https://a/b'), null);
});

test('menu split, order kept, unknown and duplicate keys dropped', () => {
  const choices: MediaChoice[] = [
    { key: 'best', kind: 'best', height: 1080, ext: 'mp4', approx_size: 100 },
    { key: 'h720', kind: 'video', height: 720, ext: 'mp4', approx_size: null },
    { key: 'h720', kind: 'video', height: 720, ext: 'mp4', approx_size: null },
    { key: 'bestvideo+bestaudio', kind: 'video', height: null, ext: 'mp4', approx_size: null },
    { key: 'audio-m4a', kind: 'audio', height: null, ext: 'm4a', approx_size: 5 },
  ];
  const menu = buildMenu(choices);
  assert.deepEqual(
    menu.video.map((r) => r.key),
    ['best', 'h720'],
  );
  assert.deepEqual(
    menu.audio.map((r) => r.key),
    ['audio-m4a'],
  );
  assert.equal(menu.video[0]?.approxSize, 100);
});

test('withSite adds once', () => {
  assert.deepEqual(withSite(['a.com'], 'b.com'), ['a.com', 'b.com']);
  assert.deepEqual(withSite(['a.com'], 'a.com'), ['a.com']);
});
