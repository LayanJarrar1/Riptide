import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findProblems } from './check-doc-sync.mjs';

const rows = (manifest, docs) => [
  { name: 'release.json (featured)', versions: [manifest] },
  ...['README.md', 'CHANGELOG.md', 'Cargo.toml'].map((name) => ({ name, versions: [docs] })),
];

test('everything on the same version passes', () => {
  assert.deepEqual(findProblems(rows('0.1.0-alpha.5', '0.1.0-alpha.5')), []);
});

test('documents ahead of release.json fail by default (the run after the merge)', () => {
  assert.equal(findProblems(rows('0.1.0-alpha.4', '0.1.0-alpha.5')).length, 3);
});

test('documents ahead of release.json pass while preparing a release', () => {
  assert.deepEqual(findProblems(rows('0.1.0-alpha.4', '0.1.0-alpha.5'), { allowUnpublished: true }), []);
});

test('preparing a release still catches documents that disagree with each other', () => {
  const r = rows('0.1.0-alpha.4', '0.1.0-alpha.5');
  r[2].versions = ['0.1.0-alpha.4'];
  assert.ok(findProblems(r, { allowUnpublished: true }).length > 0);
});

test('release.json newer than the documents is never accepted', () => {
  assert.ok(findProblems(rows('0.1.0-alpha.5', '0.1.0-alpha.4'), { allowUnpublished: true }).length > 0);
});

test('a document with no version is a problem even while preparing a release', () => {
  const r = rows('0.1.0-alpha.4', '0.1.0-alpha.5');
  r[1].versions = [];
  assert.ok(findProblems(r, { allowUnpublished: true }).length > 0);
});
