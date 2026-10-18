import { computed, DestroyRef, inject, Injectable, signal } from '@angular/core';

import { BatchOptions, BatchResult, Download, DownloadRequest, isActive, MediaProbe, ProbeResult, Segment } from '../model/download';
import type { Download as BridgeDownload } from '../tauri/bridge/Download';
import type { EngineSettings } from '../tauri/bridge/EngineSettings';
import type { Event as EngineEvent } from '../tauri/bridge/Event';
import type { ProbeInfo } from '../tauri/bridge/ProbeInfo';
import type { MediaInfo } from '../tauri/bridge/MediaInfo';
import type { Segment as BridgeSegment } from '../tauri/bridge/Segment';
import { TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import type { BatchError } from '../tauri/bridge/BatchError';
import type { BatchOutcome } from '../tauri/bridge/BatchOutcome';
import type { UrlList } from '../tauri/bridge/UrlList';
import type { BatchExpansion, UrlListResult } from './batch-pattern';
import { emptyHistory, toBridgeRequest, toUiDownload, toUiMedia, toUiProbe, toUiSize } from './bridge-mapping';
import { withMovedToQueue, withQueueOrder } from '../model/queue-order';
import { DownloadEngine } from './download-engine';

/** The Tauri event every engine notification arrives on; its payload is the generated `Event`. */
export const ENGINE_EVENT = 'riptide://event';

/** Speed histories get one sample per second, per download and globally. */
const SAMPLE_MS = 1000;

/** Used until `get_engine_settings` answers. */
const DEFAULT_MAX_ACTIVE = 4;

type ProgressEvent = Extract<EngineEvent, { type: 'progress' }>;
type SegmentProgressEvent = Extract<EngineEvent, { type: 'segment_progress' }>;

/**
 * The real engine, driven over Tauri IPC. The list is loaded once (`list_downloads` plus
 * `get_segments` per item) and then kept current from the `riptide://event` stream:
 *
 * - `added`: fetch the record and its segments, insert it.
 * - `probed`: refresh the record and segments (size, name and ranges are now known).
 * - `state_changed`: refresh the record and segments; speed drops to 0 off `downloading`.
 * - `progress`: `speedBps`, plus one `speedHistory` sample per second.
 * - `segment_progress`: that segment's `written` (and `end` when `len` grew); an unknown
 *   index triggers a refresh.
 * - `completed`: refresh the record (`completedAt`, final segments).
 * - `failed`: refresh the record (`failure` kind derived from the error text).
 * - `removed`: drop the row.
 * - `download_moved`: refresh the record (its queue and place changed).
 * - `queue_order_changed`: re-read that queue's members (`queue_items`) for their new places.
 *
 * Events that arrive before the initial list is in are queued and replayed after it, so nothing
 * is lost or overwritten at startup. Refreshes of the same id are sequenced: a slow answer never
 * overwrites a newer one. `@tauri-apps/api` is only reached through the injected `TAURI_INVOKE`
 * and `TAURI_LISTEN` functions, so this file is safe to evaluate in Karma, where specs provide
 * fakes.
 */
@Injectable({ providedIn: 'root' })
export class TauriEngineService extends DownloadEngine {
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly listen = inject(TAURI_LISTEN);

  private readonly list = signal<readonly Download[]>([]);
  /** Events received before the initial list arrived; `null` once live. */
  private pending: EngineEvent[] | null = [];
  /** Last time a speed sample was appended, per download. */
  private readonly lastSample = new Map<number, number>();
  /** Sequence number of the latest refresh per download, to drop stale answers. */
  private readonly refreshSeq = new Map<number, number>();
  private unlisten: (() => void) | null = null;
  private readonly sampler: ReturnType<typeof setInterval>;

  readonly downloads = this.list.asReadonly();
  readonly active = computed(() => this.list().filter((d) => isActive(d.state)));
  readonly totalSpeedBps = computed(() => this.active().reduce((n, d) => n + d.speedBps, 0));
  readonly totalSpeedHistory = signal<readonly number[]>(emptyHistory());
  readonly maxActive = signal(DEFAULT_MAX_ACTIVE);
  /** Flips to `true` when `ready` resolves, whether or not `list_downloads` succeeded. */
  override readonly loaded = signal(false);

  /** Resolves once the event stream is attached and the initial list is in (never rejects). */
  readonly ready: Promise<void>;

  constructor() {
    super();
    this.sampler = setInterval(() => {
      const total = this.totalSpeedBps();
      this.totalSpeedHistory.update((h) => [...h.slice(1), total]);
    }, SAMPLE_MS);
    this.ready = this.start();
    inject(DestroyRef).onDestroy(() => {
      clearInterval(this.sampler);
      this.unlisten?.();
      this.unlisten = null;
    });
  }

  get(id: number): Download | undefined {
    return this.list().find((d) => d.id === id);
  }

  async probe(url: string): Promise<ProbeResult> {
    return toUiProbe(await this.invoke<ProbeInfo>('probe_url', { url }));
  }

  async probeMedia(url: string): Promise<MediaProbe> {
    return toUiMedia(await this.invoke<MediaInfo>('probe_media', { url }));
  }

  /**
   * Adds through `add_download` and fetches the new record right away, so the row exists even
   * if the `added` event raced ahead of the subscription. The engine probes on its own; the
   * dialog's preview is not forwarded.
   */
  async add(request: DownloadRequest): Promise<number> {
    const id = await this.invoke<number>('add_download', { request: toBridgeRequest(request) });
    await this.refreshRecord(id);
    return id;
  }

  /** `batch_expand`: a pattern the engine cannot expand answers a typed error, not a rejection. */
  async expandBatch(pattern: string): Promise<BatchExpansion> {
    try {
      return { urls: await this.invoke<string[]>('batch_expand', { pattern }), error: null };
    } catch (e: unknown) {
      if (isBatchError(e)) {
        return { urls: [], error: e };
      }
      throw e;
    }
  }

  async importUrlList(text: string): Promise<UrlListResult> {
    return this.invoke<UrlList>('batch_import', { text });
  }

  /**
   * `add_batch`: one transaction on the engine side. The rows arrive through the `added` events
   * (one per new download), so nothing is fetched here.
   */
  async addBatch(urls: readonly string[], options: BatchOptions): Promise<BatchResult> {
    const outcome = await this.invoke<BatchOutcome>('add_batch', {
      urls,
      options: {
        dir: options.dir ?? null,
        category_id: null,
        queue_id: null,
        priority: null,
        connections: options.connections ?? null,
        start_paused: options.startPaused ?? false,
        referrer: null,
      },
    });
    return { added: outcome.added, skipped: outcome.skipped.map((s) => ({ url: s.url, reason: s.reason })) };
  }

  /** Asks `find_duplicate`; the answer is mapped with the segments the list already holds, if any. */
  async findDuplicate(url: string, filename?: string, dir?: string): Promise<Download | undefined> {
    const record = await this.invoke<BridgeDownload | null>('find_duplicate', { url, filename: filename ?? null, dir: dir ?? null });
    if (record === null || record === undefined) {
      return undefined;
    }
    return this.get(record.id) ?? toUiDownload(record, await this.segmentsOf(record.id));
  }

  pause(id: number): Promise<void> {
    return this.invoke<void>('pause_download', { id });
  }

  /** `set_speed_limit`; the row shows the new cap at once (the engine emits no event for it). */
  async setSpeedLimit(id: number, bps: number | null): Promise<void> {
    await this.invoke<void>('set_speed_limit', { id, limit: bps });
    this.update(id, (d) => ({ ...d, speedLimitBps: bps }));
  }

  /** `restart_download`; the row follows the engine's events (paused → queued, or added again). */
  restart(id: number): Promise<void> {
    return this.invoke<void>('restart_download', { id });
  }

  resume(id: number): Promise<void> {
    return this.invoke<void>('resume_download', { id });
  }

  /** The engine has one transition for both: `resume_download`. */
  retry(id: number): Promise<void> {
    return this.resume(id);
  }

  cancel(id: number): Promise<void> {
    return this.invoke<void>('cancel_download', { id });
  }

  async remove(id: number, deleteFile = false): Promise<void> {
    await this.invoke<void>('remove_download', { id, deleteFile });
    this.drop(id);
  }

  pauseAll(): Promise<void> {
    return this.invoke<void>('pause_all');
  }

  resumeAll(): Promise<void> {
    return this.invoke<void>('resume_all');
  }

  /**
   * `move_to_queue` per download. The rows show their new queue at once; if the engine refuses
   * (or the call fails), the records are re-read so the list shows the engine's truth.
   */
  async moveToQueue(ids: readonly number[], queueId: number): Promise<void> {
    this.list.update((all) => withMovedToQueue(all, ids, queueId));
    try {
      for (const id of ids) {
        await this.invoke<void>('move_to_queue', { id, queue: queueId });
      }
    } catch (error: unknown) {
      await Promise.all(ids.map((id) => this.refreshRecord(id)));
      throw error;
    }
  }

  /** `reorder_queue_items`; the new order shows at once and the queue is re-read on failure. */
  async reorderQueue(queueId: number, ids: readonly number[]): Promise<void> {
    this.list.update((all) => withQueueOrder(all, queueId, ids));
    try {
      await this.invoke<void>('reorder_queue_items', { queue: queueId, order: ids });
    } catch (error: unknown) {
      await this.reloadQueue(queueId);
      throw error;
    }
  }

  /**
   * Not an engine command yet. The downloads page does not call this: for an expired link it
   * opens the add dialog pre-filled with the URL instead.
   */
  refresh(): Promise<void> {
    return Promise.resolve();
  }

  private async start(): Promise<void> {
    try {
      this.unlisten = await this.listen<EngineEvent>(ENGINE_EVENT, (e) => this.onEvent(e));
    } catch (error: unknown) {
      console.warn('[engine] could not subscribe to events', error);
    }
    try {
      const [records] = await Promise.all([this.invoke<BridgeDownload[]>('list_downloads'), this.loadSettings()]);
      const downloads = await Promise.all(records.map(async (r) => toUiDownload(r, await this.segmentsOf(r.id))));
      this.list.set(downloads);
    } catch (error: unknown) {
      console.warn('[engine] list_downloads failed', error);
    }
    const queued = this.pending ?? [];
    this.pending = null;
    for (const e of queued) {
      this.onEvent(e);
    }
    this.loaded.set(true);
  }

  private async loadSettings(): Promise<void> {
    try {
      const settings = await this.invoke<EngineSettings>('get_engine_settings');
      if (settings.max_active > 0) {
        this.maxActive.set(settings.max_active);
      }
    } catch (error: unknown) {
      console.warn('[engine] get_engine_settings failed', error);
    }
  }

  private onEvent(e: EngineEvent): void {
    if (this.pending !== null) {
      this.pending.push(e);
      return;
    }
    switch (e.type) {
      case 'added':
      case 'probed':
      case 'state_changed':
      case 'completed':
      case 'failed':
        void this.refreshRecord(e.id);
        break;
      case 'progress':
        this.onProgress(e);
        break;
      case 'segment_progress':
        this.onSegmentProgress(e);
        break;
      case 'removed':
        this.drop(e.id);
        break;
      case 'download_moved':
        void this.refreshRecord(e.id);
        break;
      case 'queue_order_changed':
        void this.reloadQueue(e.queue_id);
        break;
    }
  }

  /** Re-reads the members of one queue (their order changed) and applies queue and position. */
  private async reloadQueue(queueId: number): Promise<void> {
    try {
      const members = await this.invoke<BridgeDownload[]>('queue_items', { id: queueId });
      const placed = new Map(members.map((m) => [m.id, m]));
      this.list.update((all) =>
        all.map((d) => {
          const m = placed.get(d.id);
          return m === undefined ? d : { ...d, queueId: m.queue_id, queuePosition: m.queue_position };
        }),
      );
    } catch (error: unknown) {
      console.warn('[engine] queue_items failed', error);
    }
  }

  private onProgress(e: ProgressEvent): void {
    if (this.get(e.id) === undefined) {
      void this.refreshRecord(e.id);
      return;
    }
    const now = Date.now();
    const last = this.lastSample.get(e.id) ?? Number.NEGATIVE_INFINITY;
    const sample = now - last >= SAMPLE_MS;
    if (sample) {
      this.lastSample.set(e.id, now);
    }
    const speed = toUiSize(e.speed_bps) ?? 0;
    const total = toUiSize(e.total);
    this.update(e.id, (d) => {
      // before the engine has planned segments the only progress we have is the byte count
      const placeholder = d.segments.length === 1 && d.segments[0].end === 0 && d.segments[0].start === 0;
      // a media download (yt-dlp) never has segments: its byte count is the whole story
      const synthetic = placeholder || d.kind === 'media';
      const segments: readonly Segment[] =
        synthetic && e.downloaded > 0
          ? [{ index: 0, start: 0, end: Math.max(total ?? 0, e.downloaded), written: e.downloaded, state: 'active' }]
          : d.segments;
      return {
        ...d,
        size: d.size ?? (total !== null && total > 0 ? total : null),
        segments,
        speedBps: speed,
        speedHistory: sample ? [...d.speedHistory.slice(1), speed] : d.speedHistory,
      };
    });
  }

  private onSegmentProgress(e: SegmentProgressEvent): void {
    const d = this.get(e.id);
    if (d === undefined || !d.segments.some((s) => s.index === e.index)) {
      void this.refreshRecord(e.id);
      return;
    }
    this.update(e.id, (current) => ({
      ...current,
      segments: current.segments.map((s) =>
        s.index === e.index
          ? {
              ...s,
              written: e.written,
              end: Math.max(s.end, s.start + e.len),
              state: e.len > 0 && e.written >= e.len ? 'done' : 'active',
            }
          : s,
      ),
    }));
  }

  /** Re-reads one record and its segments; drops the row when the engine no longer has it. */
  private async refreshRecord(id: number): Promise<void> {
    const seq = (this.refreshSeq.get(id) ?? 0) + 1;
    this.refreshSeq.set(id, seq);
    try {
      const [record, segments] = await Promise.all([
        this.invoke<BridgeDownload | null>('get_download', { id }),
        this.segmentsOf(id),
      ]);
      if (this.refreshSeq.get(id) !== seq) {
        return;
      }
      if (record === null || record === undefined) {
        this.drop(id);
        return;
      }
      this.upsert(toUiDownload(record, segments, this.get(id)));
    } catch (error: unknown) {
      console.warn('[engine] get_download failed', error);
    }
  }

  private async segmentsOf(id: number): Promise<BridgeSegment[]> {
    try {
      return await this.invoke<BridgeSegment[]>('get_segments', { id });
    } catch (error: unknown) {
      console.warn('[engine] get_segments failed', error);
      return [];
    }
  }

  private upsert(d: Download): void {
    this.list.update((all) => (all.some((x) => x.id === d.id) ? all.map((x) => (x.id === d.id ? d : x)) : [...all, d]));
  }

  private update(id: number, fn: (d: Download) => Download): void {
    this.list.update((all) => all.map((d) => (d.id === id ? fn(d) : d)));
  }

  private drop(id: number): void {
    this.lastSample.delete(id);
    this.refreshSeq.delete(id);
    this.list.update((all) => (all.some((d) => d.id === id) ? all.filter((d) => d.id !== id) : all));
  }
}

/** Whether a rejected `invoke` carries the typed `BatchError` the backend serializes. */
function isBatchError(e: unknown): e is BatchError {
  return typeof e === 'object' && e !== null && 'code' in e && typeof (e as { code: unknown }).code === 'string';
}
