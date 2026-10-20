import assert from 'node:assert/strict';
import { test } from 'node:test';

import { HELP_URL } from './help';

test('the help page is a fixed https URL on the project site', () => {
  const url = new URL(HELP_URL);
  assert.equal(url.protocol, 'https:');
  assert.equal(url.hostname, 'layanjarrar1.github.io');
  assert.equal(url.username + url.password + url.search + url.hash, '');
});
