import { TestBed } from '@angular/core/testing';

import { fileFraction, formatRatio, isMagnetLink, magnetHash, selectedBytes } from '../model/torrent';
import type { Event as EngineEvent } from '../tauri/bridge/Event';
import type { TorrentInfo } from '../tauri/bridge/TorrentInfo';
import type { TorrentLive } from '../tauri/bridge/TorrentLive';
import type { TorrentPreviewInfo } from '../tauri/bridge/TorrentPreviewInfo';
import { InvokeFn, ListenFn, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { bridgeDownload } from './bridge-mapping.spec';
import { toBridgeSource, toBridgeTorrentAdd, toUiPreview, toUiTorrent, toUiTorrentLive } from './torrent-mapping';
import { TauriEngineService } from './tauri-engine.service';

const HASH = '0123456789abcdef0123456789abcdef01234567';

function info(over: Partial<TorrentInfo> = {}): TorrentInfo {
  return {
    download_id: 9,
    info_hash: HASH,
    name: 'Big Buck Bunny',
    files: [
      { index: 0, path: 'Big Buck Bunny/movie.mp4', len: 1000 },
      { index: 1, path: 'Big Buck Bunny/sub.srt', len: 140 },
      { index: 2, path: 'Big Buck Bunny/poster.jpg', len: 360 },
    ],
    selected: [1, 2],
    sequential: false,
    seeding: false,
    uploaded: 250,
    downloaded: 300,
    peers: 4,
    ...over,
  };
}

describe('torrent model helpers', () => {
  it('recognises magnet links in any case and with surrounding space', () => {
    expect(isMagnetLink('magnet:?xt=urn:btih:abc')).toBeTrue();
    expect(isMagnetLink('  MAGNET:?xt=urn:btih:abc ')).toBeTrue();
    expect(isMagnetLink('https://example.com/a.torrent')).toBeFalse();
    expect(isMagnetLink('magnet')).toBeFalse();
  });

  it('reads the info-hash of a magnet link (hex lower-cased, base32 kept)', () => {
    expect(magnetHash(`magnet:?dn=x&xt=urn:btih:${HASH.toUpperCase()}`)).toBe(HASH);
    expect(magnetHash('magnet:?xt=urn:btih:ABCDEFGHIJKLMNOPQRSTUVWXYZ234567')).toBe('ABCDEFGHIJKLMNOPQRSTUVWXYZ234567');
    expect(magnetHash('magnet:?xt=urn:btih:short')).toBeNull();
    expect(magnetHash('magnet:?dn=nothing')).toBeNull();
  });

  it('sums the chosen files, all of them when nothing is chosen', () => {
    const files = info().files;
    expect(selectedBytes(files, null)).toBe(1500);
    expect(selectedBytes(files, [1, 2])).toBe(500);
    expect(selectedBytes(files, [])).toBe(0);
  });

  it('keeps file progress and the ratio in range', () => {
    expect(fileFraction(100, 50)).toBe(0.5);
    expect(fileFraction(100, 500)).toBe(1);
    expect(fileFraction(100, undefined)).toBe(0);
    expect(fileFraction(0, 0)).toBe(1);
    expect(formatRatio(0)).toBe('0.00');
    expect(formatRatio(1.236)).toBe('1.24');
    expect(formatRatio(Number.NaN)).toBe('0.00');
  });
});

describe('torrent mapping', () => {
  it('maps the stored facts, carrying live numbers over from the previous view', () => {
    const first = toUiTorrent(info());
    expect(first.files.map((f) => f.index)).toEqual([0, 1, 2]);
    expect(first.selected).toEqual([1, 2]);
    expect(first.downloaded).toBe(300);
    expect(first.peers).toBe(4);
    expect(first.ratio).toBeCloseTo(0.5);
    const live = { ...first, peers: 11, peersSeen: 30, upBps: 5000, uploaded: 1000 };
    const next = toUiTorrent(info({ uploaded: 250 }), live);
    expect([next.peers, next.peersSeen, next.upBps]).toEqual([11, 30, 5000]);
    expect(next.uploaded).toBe(1000);
    expect(next.ratio).toBeCloseTo(2);
  });

  it('sends a magnet link or a file path as a tagged source', () => {
    expect(toBridgeSource({ kind: 'magnet', uri: 'magnet:?x' })).toEqual({ kind: 'magnet', uri: 'magnet:?x' });
    expect(toBridgeSource({ kind: 'file', path: 'C:\\t.torrent' })).toEqual({ kind: 'file', path: 'C:\\t.torrent' });
  });

  it('maps a preview and the add request (null selection means every file)', () => {
    const wire: TorrentPreviewInfo = {
      token: `${HASH}-1`,
      info_hash: HASH,
      name: 'x',
      files: [{ index: 0, path: 'x/a', len: 5, exists: true }],
      total_bytes: 5,
      private: true,
      existing: 3,
    };
    const ui = toUiPreview(wire);
    expect([ui.isPrivate, ui.existing, ui.files[0].exists, ui.totalBytes]).toEqual([true, 3, true, 5]);
    const add = toBridgeTorrentAdd({ token: 't', onlyFiles: null, sequential: true, startPaused: false });
    expect([add.only_files, add.sequential, add.dir, add.peers]).toEqual([null, true, null, []]);
    expect(toBridgeTorrentAdd({ token: 't', dir: 'D:\\x', onlyFiles: [2, 0], sequential: false, startPaused: true }).only_files).toEqual([2, 0]);
  });

  it('maps the live numbers of the details drawer', () => {
    const wire: TorrentLive = { info: info(), file_progress: [0, 140, 100], peers: 3, peers_seen: 9, down_bps: 1000, up_bps: 200, ratio: 0.5 };
    const v = toUiTorrentLive(wire);
    expect(v.fileProgress).toEqual([0, 140, 100]);
    expect([v.peers, v.peersSeen, v.downBps, v.upBps]).toEqual([3, 9, 1000, 200]);
    expect(v.selected).toEqual([1, 2]);
  });
});

/** A fake Rust side that knows one torrent row. */
class TorrentBridge {
  readonly calls: { cmd: string; args: Record<string, unknown> | undefined }[] = [];
  handler: ((e: EngineEvent) => void) | null = null;
  readonly invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    this.calls.push({ cmd, args });
    switch (cmd) {
      case 'list_downloads':
        return Promise.resolve([bridgeDownload({ id: 9, kind: 'torrent', url: `magnet:?xt=urn:btih:${HASH}`, filename: 'Big Buck Bunny', size: 500, state: { state: 'downloading' }, supports_ranges: false })] as unknown as T);
      case 'list_torrents':
        return Promise.resolve([info()] as unknown as T);
      case 'torrent_live':
        return Promise.resolve({ info: info({ downloaded: 400 }), file_progress: [0, 140, 260], peers: 2, peers_seen: 5, down_bps: 10, up_bps: 1, ratio: 0.5 } as unknown as T);
      case 'get_download':
        return Promise.resolve(bridgeDownload({ id: 9, kind: 'torrent', filename: 'Big Buck Bunny', size: 500, state: { state: 'completed' } }) as unknown as T);
      case 'get_segments':
        return Promise.resolve([] as unknown as T);
      case 'get_engine_settings':
        return Promise.resolve(undefined as unknown as T);
      case 'torrent_preview':
        return Promise.resolve({ token: `${HASH}-1`, info_hash: HASH, name: 'Big Buck Bunny', files: [], total_bytes: 0, private: false, existing: null } as unknown as T);
      case 'add_torrent':
        return Promise.resolve(9 as unknown as T);
      default:
        return Promise.resolve(undefined as unknown as T);
    }
  };
  readonly listen: ListenFn = <T>(_event: string, handler: (payload: T) => void): Promise<() => void> => {
    this.handler = handler as unknown as (e: EngineEvent) => void;
    return Promise.resolve(() => undefined);
  };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 30; i++) {
    await Promise.resolve();
  }
  await new Promise((r) => setTimeout(r, 25));
}

describe('TauriEngineService with torrents', () => {
  let bridge: TorrentBridge;
  let engine: TauriEngineService;

  beforeEach(async () => {
    bridge = new TorrentBridge();
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_INVOKE, useValue: bridge.invoke },
        { provide: TAURI_LISTEN, useValue: bridge.listen },
      ],
    });
    engine = TestBed.inject(TauriEngineService);
    await engine.ready;
    await settle();
  });

  it('shows a torrent row with its files, selection and the verified byte count as progress', () => {
    const d = engine.get(9);
    expect(d?.kind).toBe('torrent');
    expect(d?.torrent?.files.length).toBe(3);
    expect(d?.torrent?.selected).toEqual([1, 2]);
    expect(d?.segments[0].written).toBe(300);
    expect(d?.segments[0].end).toBe(500);
    expect(bridge.calls.filter((c) => c.cmd === 'list_torrents').length).toBe(1);
  });

  it('applies torrent_stats events to the peers, upload and ratio of the row', async () => {
    bridge.handler?.({ type: 'torrent_stats', id: 9, peers: 7, peers_seen: 20, up_bps: 4096, uploaded: 750, ratio_milli: 1500, seeding: true });
    await settle();
    const t = engine.get(9)?.torrent;
    expect([t?.peers, t?.peersSeen, t?.upBps, t?.uploaded, t?.ratio, t?.seeding]).toEqual([7, 20, 4096, 750, 1.5, true]);
  });

  it('turns progress events of a torrent into a byte count without segments', async () => {
    bridge.handler?.({ type: 'progress', id: 9, downloaded: 450, total: 500, speed_bps: 1234, eta_secs: 4, connections: 6 });
    await settle();
    const d = engine.get(9);
    expect(d?.segments[0].written).toBe(450);
    expect(d?.speedBps).toBe(1234);
  });

  it('refreshes the torrent facts when the row changes state', async () => {
    bridge.handler?.({ type: 'state_changed', id: 9, from: { state: 'verifying' }, to: { state: 'completed' } });
    await settle();
    expect(engine.get(9)?.state).toBe('completed');
    expect(bridge.calls.some((c) => c.cmd === 'torrent_live')).toBeTrue();
    expect(engine.get(9)?.torrent?.downloaded).toBe(400);
  });

  it('previews, adds and polls through the torrent commands', async () => {
    const preview = await engine.torrentPreview({ kind: 'magnet', uri: `magnet:?xt=urn:btih:${HASH}` }, 'D:\\dl');
    expect(preview.token).toBe(`${HASH}-1`);
    const call = bridge.calls.find((c) => c.cmd === 'torrent_preview');
    expect(call?.args).toEqual({ source: { kind: 'magnet', uri: `magnet:?xt=urn:btih:${HASH}` }, dir: 'D:\\dl' });

    const id = await engine.addTorrent({ token: preview.token, onlyFiles: [1], sequential: true, startPaused: false });
    expect(id).toBe(9);
    expect(bridge.calls.find((c) => c.cmd === 'add_torrent')?.args).toEqual({
      request: { token: `${HASH}-1`, dir: null, only_files: [1], sequential: true, start_paused: false, queue_id: null, peers: [] },
    });

    const live = await engine.torrentLive(9);
    expect(live.fileProgress).toEqual([0, 140, 260]);
    expect(live.peersSeen).toBe(5);
  });
});
