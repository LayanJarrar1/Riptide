import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import en from './i18n/en.json';
import { dirOf, extraKeys, formatBytes, missingKeys, placeholdersOf, resolveLang, t } from './i18n';
import { classifyDisconnect } from './status';

test('Arabic table has every English key and nothing more', () => {
  assert.deepEqual(missingKeys('ar'), []);
  assert.deepEqual(extraKeys('ar'), []);
  assert.deepEqual(missingKeys('en'), []);
});

test('placeholders match between languages', () => {
  for (const key of Object.keys(en) as (keyof typeof en)[]) {
    assert.deepEqual(placeholdersOf(t('ar', key)), placeholdersOf(t('en', key)), key);
  }
});

test('manifest locales have the same keys', () => {
  const read = (lang: string) =>
    Object.keys(JSON.parse(readFileSync(join(process.cwd(), '_locales', lang, 'messages.json'), 'utf8'))).sort();
  assert.deepEqual(read('ar'), read('en'));
  assert.ok(read('en').includes('extName'));
});

test('t substitutes params and leaves unknown placeholders alone', () => {
  assert.equal(t('en', 'popup.captured', { n: 3 }), '3 this session');
  assert.equal(t('ar', 'panel.partial', { ok: 2, failed: 1 }), 'أُرسل 2، وفشل 1');
  assert.equal(t('en', 'notify.failed.body', { name: 'a' }), 'a: {error}');
});

test('language resolution and direction', () => {
  assert.equal(resolveLang('auto', 'ar-SA'), 'ar');
  assert.equal(resolveLang('auto', 'en-GB'), 'en');
  assert.equal(resolveLang('auto', 'fr'), 'en');
  assert.equal(resolveLang('ar', 'en-US'), 'ar');
  assert.equal(resolveLang('en', 'ar'), 'en');
  assert.equal(dirOf('ar'), 'rtl');
  assert.equal(dirOf('en'), 'ltr');
});

test('formatBytes picks a unit', () => {
  assert.equal(formatBytes('en', 512 * 1024), '512 KB');
  assert.equal(formatBytes('en', 1.5 * 1024 * 1024), '1.5 MB');
  assert.equal(formatBytes('en', 3 * 1024 ** 3), '3 GB');
  assert.ok(formatBytes('ar', 2 * 1024 * 1024).endsWith('م.ب'));
});

test('disconnect classification', () => {
  assert.equal(classifyDisconnect('Specified native messaging host not found.'), 'host_missing');
  assert.equal(classifyDisconnect('Access to the specified native messaging host is forbidden.'), 'host_missing');
  assert.equal(classifyDisconnect('No such native application app.riptide.host'), 'host_missing');
  assert.equal(classifyDisconnect('Native host has exited.'), 'app_not_running');
  assert.equal(classifyDisconnect(''), 'app_not_running');
});
