import { test } from 'node:test';
import assert from 'node:assert/strict';

import fixtures from './protocol.fixtures.json';
import {
  BROWSER_REQUEST_TYPES,
  ERROR_CODES,
  PROTOCOL_VERSION,
  ProtocolError,
  REQUEST_TYPES,
  RESPONSE_TYPES,
  decodeResponse,
  encodeRequest,
  makeRequest,
  parseResponse,
} from './protocol';
import type { RequestBody, Response } from './protocol';

interface RequestFixture {
  name: string;
  id: number;
  body: RequestBody;
  json: string;
}
interface ResponseFixture {
  name: string;
  json: string;
  expect?: Response;
  expectStates?: unknown[];
}
interface RejectedFixture {
  name: string;
  json: string;
}

const requests = fixtures.requests as unknown as RequestFixture[];
const responses = fixtures.responses as unknown as ResponseFixture[];
const rejected = fixtures.rejected as unknown as RejectedFixture[];

test('protocol version matches message.rs', () => {
  assert.equal(PROTOCOL_VERSION, 1);
});

test('enum tags mirror message.rs', () => {
  assert.deepEqual(
    [...REQUEST_TYPES],
    ['ping', 'probe', 'add', 'refresh', 'list', 'get', 'pause', 'resume', 'cancel', 'remove', 'subscribe', 'shutdown'],
  );
  assert.deepEqual([...RESPONSE_TYPES], ['pong', 'probed', 'added', 'downloads', 'download', 'ok', 'event', 'error']);
  assert.deepEqual([...ERROR_CODES], ['bad_request', 'not_found', 'illegal_transition', 'unsupported', 'internal']);
  // must match BROWSER_REQUEST_TYPES in crates/riptide-host/src/lib.rs
  assert.deepEqual([...BROWSER_REQUEST_TYPES], ['ping', 'probe', 'add', 'refresh', 'list']);
});

for (const f of requests) {
  test(`request fixture: ${f.name} encodes byte-for-byte`, () => {
    assert.equal(encodeRequest(f.id, f.body), f.json);
    // The flattened object must put `id` first, then `type` (what serde produces).
    assert.deepEqual(Object.keys(makeRequest(f.id, f.body)).slice(0, 2), ['id', 'type']);
  });
}

test('request ids must be non-negative integers', () => {
  assert.throws(() => makeRequest(-1, { type: 'ping' }), ProtocolError);
  assert.throws(() => makeRequest(1.5, { type: 'ping' }), ProtocolError);
});

for (const f of responses) {
  test(`response fixture: ${f.name} decodes`, () => {
    const parsed = decodeResponse(f.json);
    if (f.expect) assert.deepEqual(parsed, f.expect);
    if (f.expectStates) {
      assert.equal(parsed.type, 'downloads');
      if (parsed.type === 'downloads') {
        assert.deepEqual(
          parsed.downloads.map((d) => d.state),
          f.expectStates,
        );
        // Windows paths survive the double escaping.
        assert.equal(parsed.downloads[0]?.dir, 'C:\\Users\\u\\Downloads');
        assert.equal(parsed.downloads[0]?.etag, '"abc"');
      }
    }
  });
}

for (const f of rejected) {
  test(`rejected fixture: ${f.name}`, () => {
    assert.throws(() => decodeResponse(f.json), ProtocolError);
  });
}

test('parseResponse accepts already-decoded objects and rejects non-objects', () => {
  assert.deepEqual(parseResponse({ id: 1, type: 'ok' }), { id: 1, type: 'ok' });
  assert.throws(() => parseResponse(null), ProtocolError);
  assert.throws(() => parseResponse('ok'), ProtocolError);
  assert.throws(() => parseResponse({ id: 1 }), ProtocolError);
});

test('optional request fields are omitted, not sent as null, unless given', () => {
  const json = encodeRequest(1, { type: 'add', request: { url: 'https://a/b', filename: null } });
  assert.equal(json, '{"id":1,"type":"add","request":{"url":"https://a/b","filename":null}}');
  const noSecrets = encodeRequest(2, { type: 'probe', url: 'https://a/b' });
  assert.ok(!noSecrets.includes('secrets'));
});
