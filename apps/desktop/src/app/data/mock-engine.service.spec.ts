import { TestBed } from '@angular/core/testing';

import { downloadedBytes } from '../model/download';
import { MAX_ACTIVE, MOCK_ENGINE_AUTOSTART, MockEngineService } from './mock-engine.service';

describe('MockEngineService', () => {
  let engine: MockEngineService;

  beforeEach(() => {
    TestBed.configureTestingModule({ providers: [{ provide: MOCK_ENGINE_AUTOSTART, useValue: false }] });
    engine = TestBed.inject(MockEngineService);
  });

  it('seeds a mixed list that never exceeds the queue limit of active items', () => {
    const states = new Set(engine.downloads().map((d) => d.state));
    expect(states.has('downloading')).toBeTrue();
    expect(states.has('queued')).toBeTrue();
    expect(states.has('completed')).toBeTrue();
    expect(states.has('failed')).toBeTrue();
    expect(engine.active().length).toBeLessThanOrEqual(MAX_ACTIVE);
    expect(engine.maxActive()).toBe(MAX_ACTIVE);
  });

  it('advances active downloads on tick and never writes past a segment end', () => {
    const before = engine.downloads().find((d) => d.state === 'downloading' && d.size !== null);
    expect(before).toBeDefined();
    if (!before) {
      return;
    }
    const written = downloadedBytes(before);
    for (let i = 0; i < 8; i++) {
      engine.tick(250);
    }
    const after = engine.get(before.id);
    expect(after).toBeDefined();
    expect(downloadedBytes(after ?? before)).toBeGreaterThan(written);
    for (const s of after?.segments ?? []) {
      expect(s.written).toBeLessThanOrEqual(s.end - s.start);
    }
    expect(after?.speedBps).toBeGreaterThan(0);
  });

  it('starts a queued download when a slot is free and walks it to completed', async () => {
    const id = await engine.add({ url: 'https://example.com/tiny.bin', connections: 2 }, {
      finalUrl: 'https://example.com/tiny.bin',
      filename: 'tiny.bin',
      size: 3 * 1024 * 1024,
      supportsRanges: true,
      contentType: null,
      category: 'other',
    });
    // make room: pause everything else
    for (const d of engine.downloads()) {
      if (d.id !== id) {
        engine.pause(d.id);
      }
    }
    expect(engine.get(id)?.state).toBe('queued');
    engine.tick(250);
    expect(engine.get(id)?.state).toBe('probing');
    engine.tick(250);
    expect(engine.get(id)?.state).toBe('downloading');
    for (let i = 0; i < 400 && engine.get(id)?.state !== 'completed'; i++) {
      engine.tick(250);
    }
    expect(engine.get(id)?.state).toBe('completed');
    expect(engine.get(id)?.completedAt).not.toBeNull();
    const done = engine.get(id);
    expect(done?.segments.every((s) => s.state === 'done' && s.written === s.end - s.start)).toBeTrue();
  });

  it('pause, resume, cancel and remove follow the state machine', () => {
    const running = engine.downloads().find((d) => d.state === 'downloading');
    expect(running).toBeDefined();
    if (!running) {
      return;
    }
    engine.pause(running.id);
    expect(engine.get(running.id)?.state).toBe('paused');
    expect(engine.get(running.id)?.speedBps).toBe(0);
    engine.resume(running.id);
    expect(engine.get(running.id)?.state).toBe('queued');
    engine.cancel(running.id);
    expect(engine.get(running.id)?.state).toBe('cancelled');
    engine.resume(running.id); // no-op on a terminal state
    expect(engine.get(running.id)?.state).toBe('cancelled');
    engine.remove(running.id);
    expect(engine.get(running.id)).toBeUndefined();

    const completed = engine.downloads().find((d) => d.state === 'completed');
    if (completed) {
      engine.pause(completed.id);
      engine.cancel(completed.id);
      expect(engine.get(completed.id)?.state).toBe('completed');
    }
  });

  it('a checksum failure restarts from zero on retry, other failures keep their bytes', () => {
    const checksum = engine.downloads().find((d) => d.failure === 'checksum');
    const expired = engine.downloads().find((d) => d.failure === 'expired');
    expect(checksum).toBeDefined();
    expect(expired).toBeDefined();
    if (!checksum || !expired) {
      return;
    }
    expect(downloadedBytes(expired)).toBeGreaterThan(0);
    engine.retry(checksum.id);
    engine.retry(expired.id);
    expect(downloadedBytes(engine.get(checksum.id) ?? checksum)).toBe(0);
    expect(downloadedBytes(engine.get(expired.id) ?? expired)).toBe(downloadedBytes(expired));
    expect(engine.get(expired.id)?.failure).toBeNull();
  });

  it('probe derives a stable name and size from the URL', async () => {
    const a = await engine.probe('https://example.com/dir/report-2025.pdf');
    const b = await engine.probe('https://example.com/dir/report-2025.pdf');
    expect(a.filename).toBe('report-2025.pdf');
    expect(a.category).toBe('documents');
    expect(a.size).toBe(b.size);
    expect(a.size).toBeGreaterThan(0);
  });

  it('work stealing keeps every connection busy on a large file', () => {
    const big = engine.downloads().find((d) => d.state === 'downloading' && (d.size ?? 0) > 1024 * 1024 * 1024);
    expect(big).toBeDefined();
    if (!big) {
      return;
    }
    for (let i = 0; i < 20; i++) {
      engine.tick(250);
    }
    const after = engine.get(big.id);
    const active = after?.segments.filter((s) => s.state === 'active').length ?? 0;
    expect(active).toBe(big.connections);
    // segments still tile the file exactly
    const sorted = [...(after?.segments ?? [])].sort((x, y) => x.start - y.start);
    let cursor = 0;
    for (const s of sorted) {
      expect(s.start).toBe(cursor);
      cursor = s.end;
    }
    expect(cursor).toBe(big.size ?? 0);
  });
});
