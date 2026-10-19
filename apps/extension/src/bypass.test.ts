import { test } from 'node:test';
import assert from 'node:assert/strict';

import { BYPASS_ANY_MS, BYPASS_URL_MS, BypassTracker } from './bypass';

test('nothing is bypassed before an Alt-click', () => {
  assert.equal(new BypassTracker().was('https://a/x.zip', 1_000_000), false);
});

test('an Alt-click bypasses its URL for a few seconds, any URL for a shorter time', () => {
  const b = new BypassTracker();
  b.mark('https://a/x.zip', 10_000);
  assert.equal(b.was('https://a/x.zip', 10_000 + BYPASS_URL_MS - 1), true);
  assert.equal(b.was('https://a/x.zip', 10_000 + BYPASS_URL_MS), false);
  assert.equal(b.was('https://other/y', 10_000 + BYPASS_ANY_MS - 1), true);
  assert.equal(b.was('https://other/y', 10_000 + BYPASS_ANY_MS), false);
});

test('a click that is not on a link only sets the short flag', () => {
  const b = new BypassTracker();
  b.mark('', 5_000);
  assert.equal(b.was('https://a/x', 5_100), true);
  assert.equal(b.was('https://a/x', 5_000 + BYPASS_ANY_MS), false);
});

test('old flags are forgotten and memory stays bounded', () => {
  const b = new BypassTracker();
  for (let i = 0; i < 500; i++) b.mark(`https://a/${i}`, 1_000 + i);
  assert.equal(b.was('https://a/0', 3_000), false);
  assert.equal(b.was('https://a/499', 3_000), true);
});
