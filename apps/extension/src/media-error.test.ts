import { test } from 'node:test';
import assert from 'node:assert/strict';

import { parseMediaError } from './media-error';

test('tagged refusals keep the raw yt-dlp line as details', () => {
  assert.deepEqual(parseMediaError('[rt:refused] [youtube] x: The page needs to be reloaded.'), {
    kind: 'refused',
    details: '[youtube] x: The page needs to be reloaded.',
  });
  assert.equal(parseMediaError('[rt:login] [youtube] x: Sign in to confirm your age.').kind, 'login');
});

test('untagged and missing messages have no kind', () => {
  assert.deepEqual(parseMediaError('live streams are not supported'), { kind: null, details: 'live streams are not supported' });
  assert.deepEqual(parseMediaError(undefined), { kind: null, details: '' });
});

test('details are plain text, the raw line is cut at 300 characters and the hint survives', () => {
  const parsed = parseMediaError(`[rt:refused] ${'x'.repeat(500)}\u0007\n | needs a JavaScript runtime`);
  const [raw, hint] = parsed.details.split(' | ');
  assert.equal(raw?.length, 300);
  assert.equal(hint, 'needs a JavaScript runtime');
  assert.ok(Array.from(parsed.details).every((ch) => ch.charCodeAt(0) >= 32));
});
