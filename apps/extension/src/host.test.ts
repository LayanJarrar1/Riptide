import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';

import { HostClient } from './host';
import { behaviour, ports } from './test-support/browser-stub';

beforeEach(() => {
  ports.length = 0;
  behaviour.throwOnConnect = null;
});

function lastPort() {
  const port = ports[ports.length - 1];
  assert.ok(port, 'a port was opened');
  return port;
}

describe('HostClient', () => {
  it('a pong means connected and keeps the app version', async () => {
    const client = new HostClient(1_000, 1_000);
    const pending = client.ping(1_000);
    const sent = lastPort().sent[0] as { id: number; type: string };
    assert.equal(sent.type, 'ping');
    lastPort().receive({ id: sent.id, type: 'pong', version: '1.2.3', protocol: 1 });
    assert.equal(await pending, 'connected');
    assert.equal(client.version, '1.2.3');
    client.disconnect();
  });

  it('an error in answer to a ping means the app is not running', async () => {
    const client = new HostClient(1_000, 1_000);
    const pending = client.ping(1_000);
    const sent = lastPort().sent[0] as { id: number };
    lastPort().receive({ id: sent.id, type: 'error', code: 'internal', message: 'the Riptide app is not running' });
    assert.equal(await pending, 'app_not_running');
    client.disconnect();
  });

  it('a ping nobody answers ends as app_not_running, not as unknown', async () => {
    const client = new HostClient(1_000, 1_000);
    assert.equal(client.status, 'unknown');
    assert.equal(await client.ping(30), 'app_not_running');
    assert.equal(client.status, 'app_not_running');
    client.disconnect();
  });

  it('a host that is not registered is reported as host_missing and not retried at once', async () => {
    const client = new HostClient(1_000, 60_000);
    const pending = client.ping(1_000);
    lastPort().drop('Specified native messaging host not found.');
    assert.equal(await pending, 'host_missing');
    const before = ports.length;
    assert.equal(await client.ping(1_000), 'host_missing');
    assert.equal(ports.length, before, 'no new connection while the host is known to be missing');
  });

  it('connectNative throwing is classified the same way', async () => {
    behaviour.throwOnConnect = 'Specified native messaging host not found.';
    const client = new HostClient(1_000, 1_000);
    assert.equal(await client.ping(1_000), 'host_missing');
  });

  it('frames with an unknown id or the unsolicited id 0 are ignored', async () => {
    const client = new HostClient(1_000, 1_000);
    const pending = client.ping(1_000);
    const sent = lastPort().sent[0] as { id: number };
    lastPort().receive({ id: 0, type: 'ok' });
    lastPort().receive({ id: sent.id + 99, type: 'ok' });
    lastPort().receive({ id: sent.id, type: 'pong', version: '1', protocol: 1 });
    assert.equal(await pending, 'connected');
    client.disconnect();
  });
});
