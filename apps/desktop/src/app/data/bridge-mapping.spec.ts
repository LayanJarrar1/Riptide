import { Download as UiDownload, DownloadState as UiDownloadState } from '../model/download';
import type { Download } from '../tauri/bridge/Download';
import type { DownloadState } from '../tauri/bridge/DownloadState';
import type { ProbeInfo } from '../tauri/bridge/ProbeInfo';
import type { Segment } from '../tauri/bridge/Segment';
import {
  emptyHistory,
  failureKindOf,
  finalPath,
  HISTORY_LENGTH,
  toBridgeRequest,
  toMillis,
  toUiDownload,
  toUiProbe,
  toUiSegment,
  toUiSize,
  toUiState,
  toUiMedia,
} from './bridge-mapping';
import { isMediaUrl } from '../model/download';

const ADDED_MS = 1_760_000_000_000;

/** A complete record with sensible defaults; tests override what they care about. */
export function bridgeDownload(overrides: Partial<Download> = {}): Download {
  return {
    id: 7,
    url: 'https://example.com/files/report.pdf',
    final_url: null,
    filename: 'report.pdf',
    dir: 'C:\\Users\\Dana\\Downloads',
    size: 1_000_000,
    supports_ranges: true,
    etag: null,
    last_modified: null,
    content_type: 'application/pdf',
    state: { state: 'downloading' },
    category_id: null,
    queue_id: 1,
    priority: 'normal',
    speed_limit: 0,
    connections: 8,
    checksum_expected: null,
    checksum_actual: null,
    error: null,
    added_at: ADDED_MS,
    started_at: ADDED_MS + 1000,
    completed_at: 0,
    kind: 'http',
    media_format: null,
    ...overrides,
  };
}

export function bridgeSegment(overrides: Partial<Segment> = {}): Segment {
  return { download_id: 7, index: 0, start: 0, end: 500_000, written: 250_000, state: 'active', ...overrides };
}

describe('bridge-mapping', () => {
  describe('toUiState', () => {
    it('maps every state object to its string, dropping the failed flag', () => {
      const states: DownloadState[] = [
        { state: 'queued' },
        { state: 'probing' },
        { state: 'downloading' },
        { state: 'paused' },
        { state: 'verifying' },
        { state: 'completed' },
        { state: 'failed', retryable: true },
        { state: 'failed', retryable: false },
        { state: 'cancelled' },
      ];
      const expected: UiDownloadState[] = ['queued', 'probing', 'downloading', 'paused', 'verifying', 'completed', 'failed', 'failed', 'cancelled'];
      expect(states.map(toUiState)).toEqual(expected);
    });
  });

  describe('failureKindOf', () => {
    it('derives the kind from the error text, case-insensitively', () => {
      expect(failureKindOf('SHA-256 checksum mismatch')).toBe('checksum');
      expect(failureKindOf('Checksum: expected ab, got cd')).toBe('checksum');
      expect(failureKindOf('server answered 403')).toBe('expired');
      expect(failureKindOf('signed link expired')).toBe('expired');
      expect(failureKindOf('Content-Length lied: 10 declared, 9 received')).toBe('server');
      expect(failureKindOf('body shorter than declared')).toBe('server');
      expect(failureKindOf('disk full')).toBe('disk');
      expect(failureKindOf('not enough space on D:')).toBe('disk');
      expect(failureKindOf('connection reset by peer')).toBe('network');
    });

    it('checksum wins over the other words and no text means network', () => {
      expect(failureKindOf('checksum mismatch after 403')).toBe('checksum');
      expect(failureKindOf(null)).toBe('network');
      expect(failureKindOf(undefined)).toBe('network');
      expect(failureKindOf('')).toBe('network');
    });
  });

  describe('toUiSize and toMillis', () => {
    it('keeps numbers and turns null / undefined / non-finite into null', () => {
      expect(toUiSize(42)).toBe(42);
      expect(toUiSize(0)).toBe(0);
      expect(toUiSize(null)).toBeNull();
      expect(toUiSize(undefined)).toBeNull();
      expect(toUiSize(Number.NaN)).toBeNull();
    });

    it('promotes seconds to milliseconds and treats missing or zero as "not yet"', () => {
      expect(toMillis(ADDED_MS)).toBe(ADDED_MS);
      expect(toMillis(1_760_000_000)).toBe(ADDED_MS);
      expect(toMillis(1_760_000_000.5)).toBe(ADDED_MS + 500);
      expect(toMillis(0)).toBeNull();
      expect(toMillis(-5)).toBeNull();
      expect(toMillis(null)).toBeNull();
      expect(toMillis(undefined)).toBeNull();
    });
  });

  describe('toUiSegment', () => {
    it('copies the range and drops the owner id', () => {
      const s = toUiSegment(bridgeSegment({ index: 3, start: 10, end: 20, written: 5, state: 'done' }));
      expect(s).toEqual({ index: 3, start: 10, end: 20, written: 5, state: 'done' });
      expect('download_id' in s).toBeFalse();
    });
  });

  describe('toUiDownload', () => {
    it('maps every field: snake_case to camelCase, state object to string, category by extension', () => {
      const d = toUiDownload(
        bridgeDownload({ checksum_expected: { algo: 'sha256', hex: 'ab'.repeat(32) }, completed_at: null }),
        [bridgeSegment({ index: 1, start: 500_000, end: 1_000_000, written: 0, state: 'pending' }), bridgeSegment()],
      );
      expect(d.id).toBe(7);
      expect(d.url).toBe('https://example.com/files/report.pdf');
      expect(d.filename).toBe('report.pdf');
      expect(d.dir).toBe('C:\\Users\\Dana\\Downloads');
      expect(d.size).toBe(1_000_000);
      expect(d.supportsRanges).toBeTrue();
      expect(d.state).toBe('downloading');
      expect(d.category).toBe('documents');
      expect(d.queueId).toBe(1);
      expect(d.priority).toBe('normal');
      expect(d.connections).toBe(8);
      expect(d.segments.map((s) => s.index)).toEqual([0, 1]);
      expect(d.segments[0]).toEqual({ index: 0, start: 0, end: 500_000, written: 250_000, state: 'active' });
      expect(d.addedAt).toBe(ADDED_MS);
      expect(d.startedAt).toBe(ADDED_MS + 1000);
      expect(d.completedAt).toBeNull();
      expect(d.failure).toBeNull();
      expect(d.checksum).toEqual({ algo: 'sha256', hex: 'ab'.repeat(32) });
      expect(d.speedBps).toBe(0);
      expect(d.speedHistory.length).toBe(HISTORY_LENGTH);
      expect(d.speedHistory.every((v) => v === 0)).toBeTrue();
    });

    it('derives the failure kind only for a failed record', () => {
      const failed = toUiDownload(bridgeDownload({ state: { state: 'failed', retryable: false }, error: 'checksum mismatch' }), []);
      expect(failed.state).toBe('failed');
      expect(failed.failure).toBe('checksum');
      const paused = toUiDownload(bridgeDownload({ state: { state: 'paused' }, error: 'checksum mismatch' }), []);
      expect(paused.failure).toBeNull();
      const silent = toUiDownload(bridgeDownload({ state: { state: 'failed', retryable: true }, error: null }), []);
      expect(silent.failure).toBe('network');
    });

    it('keeps speed and history from the previous value while downloading, zeroes speed otherwise', () => {
      const history = [...emptyHistory().slice(1), 123];
      const previous: UiDownload = { ...toUiDownload(bridgeDownload(), []), speedBps: 999, speedHistory: history };
      const still = toUiDownload(bridgeDownload(), [], previous);
      expect(still.speedBps).toBe(999);
      expect(still.speedHistory).toBe(history);
      const paused = toUiDownload(bridgeDownload({ state: { state: 'paused' } }), [], previous);
      expect(paused.speedBps).toBe(0);
      expect(paused.speedHistory).toBe(history);
      const completed = toUiDownload(bridgeDownload({ state: { state: 'completed' } }), [], previous);
      expect(completed.speedBps).toBe(0);
    });

    it('treats a null size as unknown and a zero added_at as the previous value', () => {
      const d = toUiDownload(bridgeDownload({ size: null, added_at: 0 }), []);
      expect(d.size).toBeNull();
      expect(d.addedAt).toBe(0);
      const previous = toUiDownload(bridgeDownload(), []);
      expect(toUiDownload(bridgeDownload({ added_at: 0 }), [], previous).addedAt).toBe(ADDED_MS);
    });

    it('synthesises one done segment for a completed file without segments, else an empty pending one', () => {
      const completed = toUiDownload(bridgeDownload({ state: { state: 'completed' }, size: 4096, completed_at: ADDED_MS + 5000 }), []);
      expect(completed.segments).toEqual([{ index: 0, start: 0, end: 4096, written: 4096, state: 'done' }]);
      expect(completed.completedAt).toBe(ADDED_MS + 5000);
      const queued = toUiDownload(bridgeDownload({ state: { state: 'queued' } }), []);
      expect(queued.segments).toEqual([{ index: 0, start: 0, end: 0, written: 0, state: 'pending' }]);
      const unknown = toUiDownload(bridgeDownload({ state: { state: 'completed' }, size: null }), []);
      expect(unknown.segments).toEqual([{ index: 0, start: 0, end: 0, written: 0, state: 'pending' }]);
    });
  });

  describe('toBridgeRequest', () => {
    it('renames to snake_case and leaves what the dialog does not set as null', () => {
      const r = toBridgeRequest({
        url: 'https://example.com/a.zip',
        filename: 'a.zip',
        dir: 'D:\\dl',
        category: 'compressed',
        connections: 16,
        startPaused: true,
        checksum: { algo: 'md5', hex: 'cd'.repeat(16) },
      });
      expect(r).toEqual({
        url: 'https://example.com/a.zip',
        dir: 'D:\\dl',
        filename: 'a.zip',
        queue_id: null,
        category_id: null,
        priority: null,
        connections: 16,
        speed_limit: null,
        checksum: { algo: 'md5', hex: 'cd'.repeat(16) },
        start_paused: true,
        referrer: null,
        user_agent: null,
        secrets: null,
        kind: 'http',
        media_format: null,
      });
    });

    it('a video page goes out as kind media with its quality key', () => {
      const r = toBridgeRequest({ url: 'https://www.youtube.com/watch?v=x', kind: 'media', mediaFormat: 'h720', filename: 'Clip' });
      expect(r.kind).toBe('media');
      expect(r.media_format).toBe('h720');
      expect(toBridgeRequest({ url: 'https://youtu.be/x', kind: 'media' }).media_format).toBe('best');
    });

    it('maps the media probe and keeps media progress across record refreshes', () => {
      const m = toUiMedia({
        url: 'https://www.youtube.com/watch?v=x',
        id: 'x',
        title: 'Clip',
        uploader: null,
        extractor: 'youtube',
        thumbnail: null,
        duration: 10,
        is_live: false,
        drm: false,
        formats: [],
        choices: [{ key: 'h720', kind: 'video', height: 720, ext: 'mp4', approx_size: 1000 }],
      });
      expect(m.choices[0]).toEqual({ key: 'h720', kind: 'video', height: 720, ext: 'mp4', approxSize: 1000 });
      const media = bridgeDownload({ kind: 'media', media_format: 'h720', state: { state: 'paused' } });
      const before = toUiDownload(media, []);
      const progressed = { ...before, segments: [{ index: 0, start: 0, end: 1000, written: 400, state: 'active' as const }] };
      const after = toUiDownload(media, [], progressed);
      expect(after.kind).toBe('media');
      expect(after.segments[0].written).toBe(400);
    });

    it('recognises video sites by host', () => {
      expect(isMediaUrl('https://www.youtube.com/watch?v=x')).toBeTrue();
      expect(isMediaUrl('https://youtu.be/x')).toBeTrue();
      expect(isMediaUrl('https://player.vimeo.com/video/1')).toBeTrue();
      expect(isMediaUrl('https://notyoutube.com/x')).toBeFalse();
      expect(isMediaUrl('https://example.com/a.mp4')).toBeFalse();
      expect(isMediaUrl('not a url')).toBeFalse();
    });

    it('defaults: no name, dir, connections or checksum, and starts unpaused', () => {
      const r = toBridgeRequest({ url: 'https://example.com/b' });
      expect(r.filename).toBeNull();
      expect(r.dir).toBeNull();
      expect(r.connections).toBeNull();
      expect(r.checksum).toBeNull();
      expect(r.start_paused).toBeFalse();
      expect(r.speed_limit).toBeNull();
    });
  });

  describe('toUiProbe', () => {
    it('maps the probe answer and classifies the file by name', () => {
      const p: ProbeInfo = {
        final_url: 'https://cdn.example.com/v/clip.mkv',
        filename: 'clip.mkv',
        size: 123_456,
        supports_ranges: false,
        content_type: 'video/x-matroska',
        resumable: false,
      };
      expect(toUiProbe(p)).toEqual({
        finalUrl: 'https://cdn.example.com/v/clip.mkv',
        filename: 'clip.mkv',
        size: 123_456,
        supportsRanges: false,
        contentType: 'video/x-matroska',
        category: 'video',
      });
      expect(toUiProbe({ ...p, size: null, content_type: null }).size).toBeNull();
    });
  });

  describe('finalPath', () => {
    it('joins with the separator the directory already uses', () => {
      expect(finalPath('C:\\Users\\Dana\\Downloads', 'a.zip')).toBe('C:\\Users\\Dana\\Downloads\\a.zip');
      expect(finalPath('C:\\Users\\Dana\\Downloads\\', 'a.zip')).toBe('C:\\Users\\Dana\\Downloads\\a.zip');
      expect(finalPath('D:', 'a.zip')).toBe('D:\\a.zip');
      expect(finalPath('/home/dana/Downloads', 'a.zip')).toBe('/home/dana/Downloads/a.zip');
      expect(finalPath('/home/dana/Downloads/', 'a.zip')).toBe('/home/dana/Downloads/a.zip');
    });
  });
});
