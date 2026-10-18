import { test } from 'node:test';
import assert from 'node:assert/strict';

import { DEFAULT_SETTINGS, normaliseSettings } from './settings';

test('video button defaults on, with no excluded sites', () => {
  const s = normaliseSettings(undefined);
  assert.equal(s.videoButton, DEFAULT_SETTINGS.videoButton);
  assert.equal(s.videoButton, true);
  assert.deepEqual(s.videoNeverSites, []);
});

test('video settings from storage are repaired', () => {
  const s = normaliseSettings({
    videoButton: false,
    videoNeverSites: ['https://Example.COM/path', 'example.com', 42, '', 'bad host!'],
  });
  assert.equal(s.videoButton, false);
  assert.deepEqual(s.videoNeverSites, ['example.com']);
  assert.deepEqual(normaliseSettings({ videoNeverSites: 'x', videoButton: 'no' }).videoNeverSites, []);
  assert.equal(normaliseSettings({ videoButton: 'no' }).videoButton, true);
});
