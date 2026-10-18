import { TestBed } from '@angular/core/testing';

import type { Download as BridgeDownload } from '../tauri/bridge/Download';
import type { EngineSettings } from '../tauri/bridge/EngineSettings';
import type { Event as EngineEvent } from '../tauri/bridge/Event';
import type { ProbeInfo } from '../tauri/bridge/ProbeInfo';
import type { Segment as BridgeSegment } from '../tauri/bridge/Segment';
import { InvokeFn, ListenFn, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { bridgeDownload, bridgeSegment } from './bridge-mapping.spec';
import { DownloadEngine } from './download-engine';
import { ENGINE_EVENT, TauriEngineService } from './tauri-engine.service';

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

/**
 * A fake Rust side: an in-memory table of records and segments that `invoke` reads, and a
 * `listen` that hands the test an `emit` to push events through the service's subscription.
 */
class FakeBridge {
  readonly records = new Map<number, BridgeDownload>();
  readonly segments = new Map<number, BridgeSegment[]>();
  readonly calls: Call[] = [];
  settings: EngineSettings = { default_dir: 'C:\\dl', connections: 8, per_host: 4, max_active: 3, user_agent: null, category_dirs: {}, global_speed_limit: null, proxy: { mode: 'system', url: '' } };
  /** What `find_duplicate` answers; `null` = nothing like it. */
  duplicate: BridgeDownload | null = null;
  probeAnswer: ProbeInfo = { final_url: 'https://x/final.iso', filename: 'final.iso', size: 10, supports_ranges: true, content_type: null, resumable: true };
  nextId = 100;
  listenCalls: string[] = [];
  private handler: ((e: EngineEvent) => void) | null = null;

  /** The stable function the service holds; tests may swap `handle` underneath it. */
  readonly invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => this.handle<T>(cmd, args);

  handle: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    this.calls.push({ cmd, args });
    const rawId = args?.['id'];
    const id = typeof rawId === 'number' ? rawId : -1;
    switch (cmd) {
      case 'list_downloads':
        return Promise.resolve([...this.records.values()] as unknown as T);
      case 'get_download':
        return Promise.resolve((this.records.get(id) ?? null) as unknown as T);
      case 'get_segments':
        return Promise.resolve((this.segments.get(id) ?? []) as unknown as T);
      case 'get_engine_settings':
        return Promise.resolve(this.settings as unknown as T);
      case 'probe_url':
        return Promise.resolve(this.probeAnswer as unknown as T);
      case 'find_duplicate':
        return Promise.resolve(this.duplicate as unknown as T);
      case 'add_download': {
        const newId = this.nextId++;
        this.records.set(newId, bridgeDownload({ id: newId, state: { state: 'queued' }, url: String((args?.['request'] as { url: string }).url) }));
        return Promise.resolve(newId as unknown as T);
      }
      default:
        return Promise.resolve(undefined as unknown as T);
    }
  };

  readonly listen: ListenFn = <T>(event: string, handler: (payload: T) => void): Promise<() => void> => {
    this.listenCalls.push(event);
    this.handler = handler as unknown as (e: EngineEvent) => void;
    return Promise.resolve(() => {
      this.handler = null;
    });
  };

  emit(e: EngineEvent): void {
    if (this.handler === null) {
      throw new Error('nobody is listening');
    }
    this.handler(e);
  }

  seed(d: BridgeDownload, segments: BridgeSegment[] = []): void {
    this.records.set(d.id, d);
    this.segments.set(d.id, segments);
  }

  commands(cmd: string): Call[] {
    return this.calls.filter((c) => c.cmd === cmd);
  }
}

/** Lets every promise chain started by a fake `invoke` settle. */
async function settle(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    await Promise.resolve();
  }
}

describe('TauriEngineService', () => {
  let bridge: FakeBridge;
  let engine: TauriEngineService;

  beforeEach(() => {
    jasmine.clock().install();
    jasmine.clock().mockDate(new Date(1_760_000_000_000));
    bridge = new FakeBridge();
    bridge.seed(bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'downloading' } }), [
      bridgeSegment({ download_id: 1, index: 0, start: 0, end: 500_000, written: 100_000, state: 'active' }),
      bridgeSegment({ download_id: 1, index: 1, start: 500_000, end: 1_000_000, written: 0, state: 'pending' }),
    ]);
    bridge.seed(bridgeDownload({ id: 2, filename: 'two.mp4', state: { state: 'paused' } }), [bridgeSegment({ download_id: 2 })]);
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_INVOKE, useValue: bridge.invoke },
        { provide: TAURI_LISTEN, useValue: bridge.listen },
      ],
    });
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  async function start(): Promise<void> {
    engine = TestBed.inject(TauriEngineService);
    await engine.ready;
  }

  it('loads the list with segments and the queue limit, then subscribes to riptide://event', async () => {
    await start();
    expect(bridge.listenCalls).toEqual([ENGINE_EVENT]);
    expect(bridge.commands('list_downloads').length).toBe(1);
    expect(bridge.commands('get_segments').map((c) => c.args?.['id'])).toEqual([1, 2]);
    expect(engine.downloads().map((d) => d.id)).toEqual([1, 2]);
    const one = engine.get(1);
    expect(one?.state).toBe('downloading');
    expect(one?.category).toBe('compressed');
    expect(one?.segments.length).toBe(2);
    expect(one?.segments[0].written).toBe(100_000);
    expect(engine.get(2)?.state).toBe('paused');
    expect(engine.active().map((d) => d.id)).toEqual([1]);
    expect(engine.maxActive()).toBe(3);
  });

  it('reports loaded only once the initial list is in, even if list_downloads fails', async () => {
    engine = TestBed.inject(TauriEngineService);
    expect(engine.loaded()).toBeFalse();
    await engine.ready;
    expect(engine.loaded()).toBeTrue();

    TestBed.resetTestingModule();
    const broken = new FakeBridge();
    broken.handle = <T>(cmd: string): Promise<T> =>
      cmd === 'list_downloads' ? Promise.reject(new Error('db locked')) : Promise.resolve(undefined as unknown as T);
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_INVOKE, useValue: broken.invoke },
        { provide: TAURI_LISTEN, useValue: broken.listen },
      ],
    });
    const warn = spyOn(console, 'warn');
    const second = TestBed.inject(TauriEngineService);
    await second.ready;
    expect(second.loaded()).toBeTrue();
    expect(second.downloads()).toEqual([]);
    expect(warn).toHaveBeenCalled();
  });

  it('queues events that arrive before the list is in and replays them after', async () => {
    engine = TestBed.inject(TauriEngineService);
    // the subscription is attached first; the list is still loading
    await Promise.resolve();
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'paused' } }));
    bridge.emit({ type: 'state_changed', id: 1, from: { state: 'downloading' }, to: { state: 'paused' } });
    await engine.ready;
    await settle();
    expect(engine.get(1)?.state).toBe('paused');
  });

  it('added: fetches the new record and its segments and inserts it', async () => {
    await start();
    bridge.seed(bridgeDownload({ id: 3, filename: 'three.pdf', state: { state: 'queued' } }), [bridgeSegment({ download_id: 3, written: 0, state: 'pending' })]);
    bridge.emit({ type: 'added', id: 3 });
    await settle();
    expect(engine.downloads().map((d) => d.id)).toEqual([1, 2, 3]);
    expect(engine.get(3)?.filename).toBe('three.pdf');
    expect(engine.get(3)?.category).toBe('documents');
    expect(engine.get(3)?.segments[0].state).toBe('pending');
  });

  it('probed: refreshes the record so size, name and ranges appear', async () => {
    await start();
    bridge.records.set(2, bridgeDownload({ id: 2, filename: 'renamed.mkv', size: 42_000, supports_ranges: false, state: { state: 'probing' } }));
    bridge.emit({ type: 'probed', id: 2, size: 42_000, supports_ranges: false, filename: 'renamed.mkv', content_type: null });
    await settle();
    const two = engine.get(2);
    expect(two?.filename).toBe('renamed.mkv');
    expect(two?.size).toBe(42_000);
    expect(two?.supportsRanges).toBeFalse();
    expect(two?.state).toBe('probing');
  });

  it('state_changed: refreshes the record and segments; leaving downloading zeroes the speed', async () => {
    await start();
    bridge.emit({ type: 'progress', id: 1, downloaded: 100_000, total: 1_000_000, speed_bps: 2_000_000, eta_secs: 1, connections: 2 });
    expect(engine.get(1)?.speedBps).toBe(2_000_000);
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'paused' } }));
    bridge.segments.set(1, [bridgeSegment({ download_id: 1, index: 0, written: 123_456, state: 'pending' })]);
    bridge.emit({ type: 'state_changed', id: 1, from: { state: 'downloading' }, to: { state: 'paused' } });
    await settle();
    const one = engine.get(1);
    expect(one?.state).toBe('paused');
    expect(one?.speedBps).toBe(0);
    expect(one?.segments.length).toBe(1);
    expect(one?.segments[0].written).toBe(123_456);
    expect(engine.active().length).toBe(0);
  });

  it('progress: updates the speed at once but samples the history once per second', async () => {
    await start();
    const progress = (speed: number): EngineEvent => ({ type: 'progress', id: 1, downloaded: 1, total: 1_000_000, speed_bps: speed, eta_secs: 10, connections: 2 });
    bridge.emit(progress(100));
    expect(engine.get(1)?.speedBps).toBe(100);
    expect(engine.get(1)?.speedHistory.slice(-1)).toEqual([100]);
    jasmine.clock().tick(250);
    bridge.emit(progress(200));
    expect(engine.get(1)?.speedBps).toBe(200);
    expect(engine.get(1)?.speedHistory.slice(-2)).toEqual([0, 100]);
    jasmine.clock().tick(750);
    bridge.emit(progress(300));
    expect(engine.get(1)?.speedHistory.slice(-2)).toEqual([100, 300]);
    expect(engine.get(1)?.speedHistory.length).toBe(60);
    expect(engine.totalSpeedBps()).toBe(300);
  });

  it('progress: fills in the size and a placeholder segment when the engine has not planned yet', async () => {
    await start();
    bridge.seed(bridgeDownload({ id: 4, filename: 'stream.bin', size: null, state: { state: 'downloading' } }), []);
    bridge.emit({ type: 'added', id: 4 });
    await settle();
    expect(engine.get(4)?.size).toBeNull();
    bridge.emit({ type: 'progress', id: 4, downloaded: 5_000, total: 20_000, speed_bps: 1, eta_secs: 1, connections: 1 });
    const four = engine.get(4);
    expect(four?.size).toBe(20_000);
    expect(four?.segments).toEqual([{ index: 0, start: 0, end: 20_000, written: 5_000, state: 'active' }]);
  });

  it('progress for an unknown id fetches the record instead of guessing', async () => {
    await start();
    bridge.seed(bridgeDownload({ id: 9, filename: 'late.zip' }));
    bridge.emit({ type: 'progress', id: 9, downloaded: 1, total: 2, speed_bps: 3, eta_secs: 4, connections: 1 });
    await settle();
    expect(engine.get(9)?.filename).toBe('late.zip');
  });

  it('segment_progress: updates written and grows end; an unknown index refreshes', async () => {
    await start();
    bridge.emit({ type: 'segment_progress', id: 1, index: 1, written: 250_000, len: 500_000 });
    let one = engine.get(1);
    expect(one?.segments[1]).toEqual({ index: 1, start: 500_000, end: 1_000_000, written: 250_000, state: 'active' });
    bridge.emit({ type: 'segment_progress', id: 1, index: 1, written: 600_000, len: 600_000 });
    one = engine.get(1);
    expect(one?.segments[1].end).toBe(1_100_000);
    expect(one?.segments[1].state).toBe('done');
    expect(bridge.commands('get_segments').length).toBe(2);
    // work stealing split a segment the UI has not seen: refresh from the engine
    bridge.segments.set(1, [
      bridgeSegment({ download_id: 1, index: 0, end: 300_000, written: 300_000, state: 'done' }),
      bridgeSegment({ download_id: 1, index: 1, start: 500_000, end: 1_000_000, written: 600_000 }),
      bridgeSegment({ download_id: 1, index: 2, start: 300_000, end: 500_000, written: 1 }),
    ]);
    bridge.emit({ type: 'segment_progress', id: 1, index: 2, written: 1, len: 200_000 });
    await settle();
    expect(bridge.commands('get_segments').length).toBe(3);
    expect(engine.get(1)?.segments.map((s) => s.index)).toEqual([0, 1, 2]);
  });

  it('completed: refreshes the record with its completion time', async () => {
    await start();
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'completed' }, completed_at: 1_760_000_050_000 }));
    bridge.segments.set(1, []);
    bridge.emit({ type: 'completed', id: 1, path: 'C:\\dl\\one.zip' });
    await settle();
    const one = engine.get(1);
    expect(one?.state).toBe('completed');
    expect(one?.completedAt).toBe(1_760_000_050_000);
    expect(one?.segments).toEqual([{ index: 0, start: 0, end: 1_000_000, written: 1_000_000, state: 'done' }]);
    expect(one?.speedBps).toBe(0);
  });

  it('failed: refreshes the record and derives the failure kind', async () => {
    await start();
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'failed', retryable: false }, error: 'server answered 403 Forbidden' }));
    bridge.emit({ type: 'failed', id: 1, error: 'server answered 403 Forbidden', retryable: false });
    await settle();
    expect(engine.get(1)?.state).toBe('failed');
    expect(engine.get(1)?.failure).toBe('expired');
  });

  it('removed: drops the row; a refresh of a record the engine lost drops it too', async () => {
    await start();
    bridge.emit({ type: 'removed', id: 2 });
    expect(engine.downloads().map((d) => d.id)).toEqual([1]);
    bridge.records.delete(1);
    bridge.emit({ type: 'state_changed', id: 1, from: { state: 'downloading' }, to: { state: 'cancelled' } });
    await settle();
    expect(engine.downloads().length).toBe(0);
  });

  it('a slow refresh never overwrites a newer one', async () => {
    await start();
    let release: () => void = () => undefined;
    const original = bridge.handle;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    let delayed = false;
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'paused' } }));
    bridge.handle = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      if (cmd === 'get_download' && !delayed) {
        delayed = true;
        return gate.then(() => original<T>(cmd, args));
      }
      return original<T>(cmd, args);
    };
    bridge.emit({ type: 'state_changed', id: 1, from: { state: 'downloading' }, to: { state: 'paused' } });
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', state: { state: 'queued' } }));
    bridge.emit({ type: 'state_changed', id: 1, from: { state: 'paused' }, to: { state: 'queued' } });
    await settle();
    expect(engine.get(1)?.state).toBe('queued');
    release();
    await settle();
    expect(engine.get(1)?.state).toBe('queued');
  });

  it('totalSpeedHistory takes one sample of the summed speed per second', async () => {
    await start();
    bridge.emit({ type: 'progress', id: 1, downloaded: 1, total: 2, speed_bps: 1_000, eta_secs: 1, connections: 1 });
    expect(engine.totalSpeedHistory().slice(-1)).toEqual([0]);
    jasmine.clock().tick(1000);
    expect(engine.totalSpeedHistory().slice(-1)).toEqual([1_000]);
    expect(engine.totalSpeedHistory().length).toBe(60);
    jasmine.clock().tick(2000);
    expect(engine.totalSpeedHistory().slice(-3)).toEqual([1_000, 1_000, 1_000]);
  });

  it('commands: each UI action maps to its Tauri command with camelCase arguments', async () => {
    await start();
    await engine.pause(1);
    await engine.resume(2);
    await engine.retry(2);
    await engine.cancel(1);
    await engine.pauseAll();
    await engine.resumeAll();
    // "refresh link" is not an engine command yet: through the contract it resolves and sends nothing
    const contract: DownloadEngine = engine;
    await contract.refresh(1, 'https://x/new');
    const after = bridge.calls.slice(bridge.calls.findIndex((c) => c.cmd === 'pause_download'));
    expect(after.map((c) => c.cmd)).toEqual(['pause_download', 'resume_download', 'resume_download', 'cancel_download', 'pause_all', 'resume_all']);
    expect(after[0].args).toEqual({ id: 1 });
    expect(after[1].args).toEqual({ id: 2 });
    expect(after[3].args).toEqual({ id: 1 });
  });

  it('restart: maps to restart_download and leaves the row to the engine events', async () => {
    await start();
    await engine.restart(1);
    expect(bridge.commands('restart_download')[0].args).toEqual({ id: 1 });
    expect(engine.get(1)?.state).toBe('downloading');
    bridge.records.set(1, bridgeDownload({ id: 1, filename: 'one.zip', size: null, state: { state: 'queued' } }));
    bridge.segments.set(1, []);
    bridge.emit({ type: 'added', id: 1 });
    await settle();
    expect(engine.get(1)?.state).toBe('queued');
    expect(engine.get(1)?.size).toBeNull();
    expect(engine.downloads().length).toBe(2);
  });

  it('findDuplicate: asks find_duplicate with nulls for the optionals and maps the answer', async () => {
    await start();
    expect(await engine.findDuplicate('https://x/a')).toBeUndefined();
    expect(bridge.commands('find_duplicate')[0].args).toEqual({ url: 'https://x/a', filename: null, dir: null });
    // a known row comes back as the live object; an unknown one is mapped with its segments
    bridge.duplicate = bridgeDownload({ id: 2, filename: 'two.mp4', state: { state: 'paused' } });
    expect((await engine.findDuplicate('https://x/a', 'two.mp4', 'C:\\dl'))?.id).toBe(2);
    expect(bridge.commands('find_duplicate')[1].args).toEqual({ url: 'https://x/a', filename: 'two.mp4', dir: 'C:\\dl' });
    bridge.duplicate = bridgeDownload({ id: 7, filename: 'old.iso', state: { state: 'completed' } });
    bridge.segments.set(7, [bridgeSegment({ download_id: 7, written: 1_000_000, state: 'done' })]);
    const seven = await engine.findDuplicate('https://x/old.iso');
    expect(seven?.filename).toBe('old.iso');
    expect(seven?.category).toBe('compressed');
    expect(seven?.segments[0].written).toBe(1_000_000);
  });

  it('remove: sends deleteFile and drops the row without waiting for the event', async () => {
    await start();
    await engine.remove(2, true);
    expect(bridge.commands('remove_download')[0].args).toEqual({ id: 2, deleteFile: true });
    expect(engine.get(2)).toBeUndefined();
    await engine.remove(1);
    expect(bridge.commands('remove_download')[1].args).toEqual({ id: 1, deleteFile: false });
  });

  it('add: sends a snake_case request, fetches the new record and resolves with its id', async () => {
    await start();
    const id = await engine.add({ url: 'https://x/new.zip', connections: 4, startPaused: true, checksum: { algo: 'sha1', hex: 'ab'.repeat(20) } });
    expect(id).toBe(100);
    const call = bridge.commands('add_download')[0];
    expect(call.args).toEqual({
      request: jasmine.objectContaining({ url: 'https://x/new.zip', connections: 4, start_paused: true, checksum: { algo: 'sha1', hex: 'ab'.repeat(20) }, dir: null, filename: null }),
    });
    expect(engine.get(100)?.url).toBe('https://x/new.zip');
    expect(engine.get(100)?.state).toBe('queued');
  });

  it('probe: asks probe_url and maps the answer', async () => {
    await start();
    const result = await engine.probe('https://x/a');
    expect(bridge.commands('probe_url')[0].args).toEqual({ url: 'https://x/a' });
    expect(result).toEqual({ finalUrl: 'https://x/final.iso', filename: 'final.iso', size: 10, supportsRanges: true, contentType: null, category: 'compressed' });
  });

  it('a refused command rejects so the page can report it', async () => {
    await start();
    bridge.handle = (): Promise<never> => Promise.reject(new Error('illegal transition'));
    await expectAsync(engine.pause(1)).toBeRejectedWithError('illegal transition');
  });
});
