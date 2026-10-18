import { computed, Injectable, InjectionToken, inject, signal } from '@angular/core';

import {
  BatchOptions,
  BatchResult,
  BatchSkipped,
  categoryOf,
  Download,
  DownloadRequest,
  downloadedBytes,
  filenameFromUrl,
  isActive,
  ProbeResult,
  Segment,
  isMediaUrl,
  MediaChoice,
  MediaProbe,
} from '../model/download';
import type { BatchExpansion, UrlListResult } from './batch-pattern';
import { DownloadEngine } from './download-engine';
import { GB, KB, MB, mulberry32, planSegments, seedDownloads } from './mock-data';

/** Whether the simulated engine advances on its own (tests turn it off). */
export const MOCK_ENGINE_AUTOSTART = new InjectionToken<boolean>('riptide.mockEngine.autostart', {
  providedIn: 'root',
  factory: () => true,
});

/** How often the simulation advances. */
const TICK_MS = 250;
/** How often a speed sample is appended to the history (one per second, like the engine). */
const SAMPLE_MS = 1000;
/** Active downloads at once in the simulated Main queue (the real engine reports its own). */
export const MAX_ACTIVE = 4;
/** Segment with writable fields, used only while the simulation advances one tick. */
type MutableSegment = { -readonly [K in keyof Segment]: Segment[K] };

/** A running segment is split for a free connection when at least this much remains. */
const MIN_SPLIT = 2 * MB;

/**
 * Simulated engine for the plain browser (`ng serve`) and Karma, where there is no Tauri bridge.
 * It owns the list of downloads as a signal and advances active ones on a timer: segments fill
 * at a jittered per-download speed, finished connections steal work from the largest remaining
 * segment, completed files go through `verifying`, and the queue starts the next item when a
 * slot frees.
 *
 * It implements the same `DownloadEngine` surface as `TauriEngineService`; every command does
 * its work synchronously and resolves at once, so specs can assert right after calling.
 */
@Injectable({ providedIn: 'root' })
export class MockEngineService extends DownloadEngine {
  private readonly rng = mulberry32(42);
  private readonly list = signal<readonly Download[]>(seedDownloads(Date.now()));
  private nextId = this.list().length + 1;
  private timer: ReturnType<typeof setInterval> | null = null;
  private lastSample = Date.now();

  /** Every download, oldest first. */
  readonly downloads = this.list.asReadonly();

  /** Downloads holding a queue slot. */
  readonly active = computed(() => this.list().filter((d) => isActive(d.state)));

  /** Sum of current speeds. */
  readonly totalSpeedBps = computed(() => this.active().reduce((n, d) => n + d.speedBps, 0));

  /** Last 60 global speed samples. */
  readonly totalSpeedHistory = signal<readonly number[]>(new Array<number>(60).fill(0));

  /** Queue slots in the simulation. */
  readonly maxActive = signal(MAX_ACTIVE);

  constructor() {
    super();
    if (inject(MOCK_ENGINE_AUTOSTART)) {
      this.start();
    }
  }

  /** Starts the simulation timer (idempotent). */
  start(): void {
    if (this.timer === null) {
      this.timer = setInterval(() => this.tick(TICK_MS), TICK_MS);
    }
  }

  /** Stops the simulation timer. */
  stop(): void {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  /** One download by id. */
  get(id: number): Download | undefined {
    return this.list().find((d) => d.id === id);
  }

  /**
   * Pretends to probe `url`: resolves after a short delay with a name, size and category derived
   * deterministically from the URL so the add dialog preview is stable.
   */
  probe(url: string): Promise<ProbeResult> {
    const filename = filenameFromUrl(url);
    const hash = Array.from(url).reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
    // believable sizes per kind of file, jittered by the URL hash
    const category = categoryOf(filename);
    const [lo, hi]: [number, number] =
      category === 'video' || (category === 'compressed' && /\.iso$/i.test(filename))
        ? [700 * MB, 6 * GB]
        : category === 'compressed' || category === 'programs'
          ? [40 * MB, 900 * MB]
          : category === 'music'
            ? [4 * MB, 120 * MB]
            : category === 'images'
              ? [300 * KB, 24 * MB]
              : category === 'documents'
                ? [120 * KB, 30 * MB]
                : [500 * KB, 2 * GB];
    const size = Math.round(lo + ((hash % 1000) / 1000) * (hi - lo));
    const supportsRanges = hash % 7 !== 0;
    return new Promise((resolve) => {
      setTimeout(
        () =>
          resolve({
            finalUrl: url,
            filename,
            size,
            supportsRanges,
            contentType: 'application/octet-stream',
            category: categoryOf(filename),
          }),
        450 + (hash % 400),
      );
    });
  }

  /**
   * Pretends to ask yt-dlp about a video page: a fixed quality menu for the known video sites,
   * a rejection for anything else.
   */
  probeMedia(url: string): Promise<MediaProbe> {
    return new Promise((resolve, reject) => {
      setTimeout(() => {
        if (!isMediaUrl(url)) {
          reject(new Error('this page has no video Riptide can download'));
          return;
        }
        const choice = (key: string, kind: MediaChoice['kind'], height: number | null, ext: string, approxSize: number): MediaChoice => ({ key, kind, height, ext, approxSize });
        resolve({
          title: 'Big Buck Bunny',
          uploader: 'Blender',
          thumbnail: null,
          duration: 635,
          choices: [
            choice('best', 'best', 2160, 'mp4', 1_676_000_000),
            choice('h1080', 'video', 1080, 'mp4', 324_000_000),
            choice('h720', 'video', 720, 'mp4', 109_000_000),
            choice('h360', 'video', 360, 'mp4', 40_000_000),
            choice('audio-m4a', 'audio', null, 'm4a', 10_300_000),
            choice('audio-mp3', 'audio', null, 'mp3', 19_400_000),
          ],
        });
      }, 300);
    });
  }

  /** Adds a download and resolves with its id. It starts when the queue has a free slot. */
  add(request: DownloadRequest, probe?: ProbeResult): Promise<number> {
    const id = this.nextId++;
    const filename = request.filename ?? probe?.filename ?? filenameFromUrl(request.url);
    const size = probe?.size ?? null;
    const connections = request.connections ?? 8;
    const now = Date.now();
    const d: Download = {
      id,
      url: request.url,
      filename,
      dir: request.dir ?? 'D:\\Downloads',
      size,
      supportsRanges: probe?.supportsRanges ?? true,
      state: request.startPaused ? 'paused' : 'queued',
      category: request.category ?? categoryOf(filename),
      queueId: 1,
      priority: 'normal',
      connections,
      segments: size === null ? [{ index: 0, start: 0, end: 0, written: 0, state: 'pending' }] : planSegments(size, connections),
      speedBps: 0,
      speedHistory: new Array<number>(60).fill(0),
      addedAt: now,
      startedAt: null,
      completedAt: null,
      failure: null,
      checksum: request.checksum ?? null,
      kind: request.kind ?? 'http',
    };
    this.list.update((all) => [...all, d]);
    return Promise.resolve(id);
  }

  /** The TypeScript port of the Rust expander (see `batch-pattern.ts`). */
  async expandBatch(pattern: string): Promise<BatchExpansion> {
    // loaded on demand: the port only matters to the batch dialog, never to the first paint
    return (await import('./batch-pattern')).expandPattern(pattern);
  }

  async importUrlList(text: string): Promise<UrlListResult> {
    return (await import('./batch-pattern')).importUrlList(text);
  }

  /** Adds the new links in one go and reports the ones already in the list, like the engine. */
  async addBatch(urls: readonly string[], options: BatchOptions): Promise<BatchResult> {
    const known = new Set(this.list().map((d) => stripFragment(d.url)));
    const added: number[] = [];
    const skipped: BatchSkipped[] = [];
    for (const url of urls) {
      const key = stripFragment(url);
      if (known.has(key)) {
        skipped.push({ url, reason: 'duplicate' });
        continue;
      }
      known.add(key);
      added.push(await this.add({ url, dir: options.dir, connections: options.connections, startPaused: options.startPaused }));
    }
    return { added, skipped };
  }

  /** Scans the list like the engine would: same link without its fragment, or same name in the same folder. */
  findDuplicate(url: string, filename?: string, dir?: string): Promise<Download | undefined> {
    const wanted = stripFragment(url);
    const name = filename ?? filenameFromUrl(url);
    const folder = dir ?? 'D:\\Downloads';
    const match = [...this.list()].reverse().find((d) => stripFragment(d.url) === wanted || (d.filename === name && d.dir === folder));
    return Promise.resolve(match);
  }

  /** Resets the simulation of one download: progress gone, back to queued (or paused if it was). */
  restart(id: number): Promise<void> {
    this.update(id, (d) => ({
      ...d,
      state: d.state === 'paused' ? 'paused' : 'queued',
      segments: d.size === null ? [{ index: 0, start: 0, end: 0, written: 0, state: 'pending' }] : planSegments(d.size, d.connections),
      speedBps: 0,
      speedHistory: new Array<number>(60).fill(0),
      startedAt: null,
      completedAt: null,
      failure: null,
    }));
    return Promise.resolve();
  }

  /** Records the cap (the simulation does not throttle). */
  setSpeedLimit(id: number, bps: number | null): Promise<void> {
    this.update(id, (d) => ({ ...d, speedLimitBps: bps }));
    return Promise.resolve();
  }

  /** Pauses a queued or running download. */
  pause(id: number): Promise<void> {
    this.update(id, (d) =>
      d.state === 'queued' || d.state === 'probing' || d.state === 'downloading'
        ? {
            ...d,
            state: 'paused',
            speedBps: 0,
            segments: d.segments.map((s) => (s.state === 'active' ? { ...s, state: 'pending' } : s)),
          }
        : d,
    );
    return Promise.resolve();
  }

  /** Resumes a paused download or retries a failed one; it re-enters the queue. */
  resume(id: number): Promise<void> {
    this.update(id, (d) =>
      d.state === 'paused' || d.state === 'failed'
        ? {
            ...d,
            state: 'queued',
            failure: null,
            // a checksum failure restarts from zero (never splice two versions of a file)
            segments: d.failure === 'checksum' ? planSegments(d.size ?? 0, d.connections) : d.segments.map((s) => (s.state === 'failed' ? { ...s, state: 'pending' } : s)),
          }
        : d,
    );
    return Promise.resolve();
  }

  /** Alias of [`resume`] for failed items (same transition, clearer call sites). */
  retry(id: number): Promise<void> {
    return this.resume(id);
  }

  /** Cancels a non-terminal download; the row stays in the list. */
  cancel(id: number): Promise<void> {
    this.update(id, (d) =>
      d.state === 'completed' || d.state === 'cancelled' ? d : { ...d, state: 'cancelled', speedBps: 0 },
    );
    return Promise.resolve();
  }

  /** Removes a download from the list (the simulation has no file to delete). */
  remove(id: number): Promise<void> {
    this.list.update((all) => all.filter((d) => d.id !== id));
    return Promise.resolve();
  }

  /** Pauses every queued or running download. */
  pauseAll(): Promise<void> {
    for (const d of this.list()) {
      void this.pause(d.id);
    }
    return Promise.resolve();
  }

  /** Resumes every paused or failed download. */
  resumeAll(): Promise<void> {
    for (const d of this.list()) {
      void this.resume(d.id);
    }
    return Promise.resolve();
  }

  /** Gives an expired download a fresh address. */
  refresh(id: number, url: string): Promise<void> {
    this.update(id, (d) => ({ ...d, url, state: 'queued', failure: null }));
    return Promise.resolve();
  }

  /**
   * Advances every active download by `elapsedMs`. Exposed so tests can drive the simulation
   * without a timer.
   */
  tick(elapsedMs: number): void {
    const now = Date.now();
    const sample = now - this.lastSample >= SAMPLE_MS;
    if (sample) {
      this.lastSample = now;
    }
    this.list.update((all) => {
      let activeCount = all.filter((d) => isActive(d.state)).length;
      const next = all.map((d) => {
        switch (d.state) {
          case 'queued':
            if (activeCount < this.maxActive()) {
              activeCount++;
              return { ...d, state: 'probing' as const, startedAt: d.startedAt ?? now };
            }
            return d;
          case 'probing':
            return { ...d, state: 'downloading' as const, speedBps: this.targetSpeed(d) * 0.3 };
          case 'downloading':
            return this.advance(d, elapsedMs, sample);
          case 'verifying':
            return this.rng() < 0.15 ? { ...d, state: 'completed' as const, completedAt: now, speedBps: 0 } : d;
          default:
            return d;
        }
      });
      return next;
    });
    if (sample) {
      const total = this.totalSpeedBps();
      this.totalSpeedHistory.update((h) => [...h.slice(1), total]);
    }
  }

  private update(id: number, fn: (d: Download) => Download): void {
    this.list.update((all) => all.map((d) => (d.id === id ? fn(d) : d)));
  }

  /** The speed a download settles around, derived from its connection count. */
  private targetSpeed(d: Download): number {
    // ~2 MB/s per connection: fast enough to see movement, slow enough that a demo list
    // keeps changing for a few minutes rather than finishing in seconds
    const perConnection = 2 * MB;
    return d.supportsRanges ? perConnection * Math.min(d.connections, 8) : 2.4 * MB;
  }

  private advance(d: Download, elapsedMs: number, sample: boolean): Download {
    const target = this.targetSpeed(d);
    // smooth approach to the target with a little turbulence
    const speed = Math.max(0, d.speedBps + (target - d.speedBps) * 0.08 + (this.rng() - 0.5) * target * 0.12);
    const budget = (speed * elapsedMs) / 1000;

    if (d.size === null) {
      const only = d.segments[0];
      const grown: Segment = { ...only, end: only.end + budget, written: only.written + budget, state: 'active' };
      return { ...d, segments: [grown], speedBps: speed, speedHistory: sample ? [...d.speedHistory.slice(1), speed] : d.speedHistory };
    }

    let segments: MutableSegment[] = d.segments.map((s) => ({ ...s }));
    const running = segments.filter((s) => s.state === 'active' || s.state === 'pending');
    const share = running.length > 0 ? budget / running.length : 0;
    for (const s of segments) {
      if (s.state === 'active' || s.state === 'pending') {
        const len = s.end - s.start;
        const written = Math.min(len, s.written + share * (0.7 + this.rng() * 0.6));
        s.written = written;
        s.state = written >= len ? 'done' : 'active';
      }
    }
    segments = this.steal(segments, d.connections);

    const done = segments.every((s) => s.state === 'done');
    if (done) {
      const full = segments.map((s) => ({ ...s, written: s.end - s.start }));
      return { ...d, segments: full, state: 'verifying', speedBps: 0, speedHistory: sample ? [...d.speedHistory.slice(1), 0] : d.speedHistory };
    }
    return {
      ...d,
      segments,
      speedBps: speed,
      speedHistory: sample ? [...d.speedHistory.slice(1), speed] : d.speedHistory,
    };
  }

  /** Work stealing: while a connection is free, split the largest remaining segment. */
  private steal(segments: MutableSegment[], connections: number): MutableSegment[] {
    let active = segments.filter((s) => s.state === 'active').length;
    while (active < connections) {
      let best: MutableSegment | null = null;
      for (const s of segments) {
        if (s.state === 'active' && s.end - s.start - s.written >= MIN_SPLIT) {
          if (best === null || s.end - s.start - s.written > best.end - best.start - best.written) {
            best = s;
          }
        }
      }
      if (best === null) {
        break;
      }
      const remaining = best.end - best.start - best.written;
      const mid = best.start + best.written + Math.floor(remaining / 2);
      const upper: MutableSegment = { index: segments.length, start: mid, end: best.end, written: 0, state: "active" };
      best.end = mid;
      segments.push(upper);
      active++;
    }
    return segments;
  }
}

/** `url` without its `#fragment`; the raw string when it does not parse. */
function stripFragment(url: string): string {
  try {
    const u = new URL(url);
    u.hash = '';
    return u.toString();
  } catch {
    return url;
  }
}

/** Convenience for templates: bytes on disk. */
export { downloadedBytes };
