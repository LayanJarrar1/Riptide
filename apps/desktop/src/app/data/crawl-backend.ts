import { inject, InjectionToken } from '@angular/core';

import { CrawlAddOptions, CrawlFile, CrawlSpecInput, CrawlUiEvent, CrawlUpdate, toUiEvent, WireCrawlBatch } from '../model/crawl';
import { BatchResult } from '../model/download';
import type { BatchOutcome } from '../tauri/bridge/BatchOutcome';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';

/** The webview event the Rust side streams crawl progress on (`CRAWL_EVENT` in `crawl.rs`). */
export const CRAWL_EVENT = 'riptide://crawl';

/**
 * What the site grabber needs from the engine. `TauriCrawlBackend` talks to the commands in
 * `src-tauri/src/crawl.rs`; `MemoryCrawlBackend` is the plain-browser simulation (`ng serve`).
 * Specs provide a fake of their own.
 */
export interface CrawlBackend {
  /**
   * Starts a crawl. `onUpdate` receives its progress, in order, until an update carries `end`.
   * Resolves with the crawl's id once the engine accepted the spec; rejects with the reason
   * when it did not (a bad address, a bad pattern).
   */
  start(spec: CrawlSpecInput, onUpdate: (update: CrawlUpdate) => void): Promise<number>;
  /** Stops the crawl; what it found stays addable. */
  cancel(id: number): Promise<void>;
  /** Stops the crawl and forgets its result (the dialog closed). */
  discard(id: number): Promise<void>;
  /** Adds the files with these links, in one step. */
  add(id: number, urls: readonly string[], options: CrawlAddOptions): Promise<BatchResult>;
}

/** The backend for this page: the real engine inside Tauri, the simulation elsewhere. */
export const CRAWL_BACKEND = new InjectionToken<CrawlBackend>('riptide.crawl.backend', {
  providedIn: 'root',
  factory: () => (inject(TAURI_AVAILABLE) ? new TauriCrawlBackend(inject(TAURI_INVOKE), inject(TAURI_LISTEN)) : new MemoryCrawlBackend()),
});

/** The real thing. */
export class TauriCrawlBackend implements CrawlBackend {
  constructor(
    private readonly invoke: InvokeFn,
    private readonly listen: ListenFn,
  ) {}

  async start(spec: CrawlSpecInput, onUpdate: (update: CrawlUpdate) => void): Promise<number> {
    // Events can arrive before `start_crawl` has answered with the id: hold them until it has.
    let id: number | null = null;
    const early: WireCrawlBatch[] = [];
    let finished = false;
    let unlisten: (() => void) | null = null;
    const deliver = (batch: WireCrawlBatch): void => {
      if (finished) {
        return;
      }
      onUpdate({ events: batch.events.map(toUiEvent), end: batch.end ?? undefined });
      if (batch.end) {
        finished = true;
        unlisten?.();
        unlisten = null;
      }
    };
    unlisten = await this.listen<WireCrawlBatch>(CRAWL_EVENT, (batch) => {
      if (id === null) {
        early.push(batch);
      } else if (batch.id === id) {
        deliver(batch);
      }
    });
    try {
      id = await this.invoke<number>('start_crawl', { spec });
    } catch (error: unknown) {
      unlisten();
      throw error;
    }
    for (const batch of early) {
      if (batch.id === id) {
        deliver(batch);
      }
    }
    return id;
  }

  cancel(id: number): Promise<void> {
    return this.invoke<void>('cancel_crawl', { id });
  }

  discard(id: number): Promise<void> {
    return this.invoke<void>('discard_crawl', { id });
  }

  async add(id: number, urls: readonly string[], options: CrawlAddOptions): Promise<BatchResult> {
    const outcome = await this.invoke<BatchOutcome>('add_crawl_files', {
      id,
      urls,
      options: { dir: options.dir, queueId: options.queueId, startPaused: options.startPaused },
    });
    return { added: outcome.added, skipped: outcome.skipped.map((s) => ({ url: s.url, reason: s.reason })) };
  }
}

const DEMO_FILES: readonly { readonly name: string; readonly category: CrawlFile['category']; readonly size: number | null; readonly host: string }[] = [
  { name: 'annual-report-2025.pdf', category: 'documents', size: 4_200_000, host: 'example.com' },
  { name: 'handbook.pdf', category: 'documents', size: 1_300_000, host: 'example.com' },
  { name: 'dataset.zip', category: 'compressed', size: 88_000_000, host: 'cdn.example.com' },
  { name: 'backup.7z', category: 'compressed', size: null, host: 'example.com' },
  { name: 'intro.mp4', category: 'video', size: 120_000_000, host: 'cdn.example.com' },
  { name: 'theme.mp3', category: 'music', size: 5_800_000, host: 'example.com' },
  { name: 'cover.jpg', category: 'images', size: 900_000, host: 'example.com' },
  { name: 'setup.exe', category: 'programs', size: 41_000_000, host: 'example.com' },
];

/** A small canned crawl so the dialog can be tried in a plain browser. */
export class MemoryCrawlBackend implements CrawlBackend {
  private nextId = 1;
  private readonly timers = new Map<number, ReturnType<typeof setTimeout>[]>();
  private readonly sinks = new Map<number, (update: CrawlUpdate) => void>();

  start(spec: CrawlSpecInput, onUpdate: (update: CrawlUpdate) => void): Promise<number> {
    const id = this.nextId++;
    const timers: ReturnType<typeof setTimeout>[] = [];
    this.timers.set(id, timers);
    this.sinks.set(id, onUpdate);
    const base = spec.url.replace(/\/+$/, '');
    DEMO_FILES.forEach((f, i) => {
      timers.push(
        setTimeout(() => {
          const events: CrawlUiEvent[] = [{ kind: 'page', url: `${base}/page-${i + 1}`, depth: Math.min(spec.depth, 1) }];
          if (spec.categories.length === 0 || spec.categories.includes(f.category)) {
            events.push({ kind: 'file', file: { url: `https://${f.host}/files/${f.name}`, name: f.name, host: f.host, category: f.category, size: f.size } });
          }
          onUpdate({ events });
        }, 250 * (i + 1)),
      );
    });
    timers.push(
      setTimeout(
        () => {
          this.timers.delete(id);
          onUpdate({ events: [], end: { stop: 'finished', pageCapHit: false, fileCapHit: false, error: null } });
        },
        250 * (DEMO_FILES.length + 1),
      ),
    );
    return Promise.resolve(id);
  }

  cancel(id: number): Promise<void> {
    const running = this.timers.has(id);
    this.stop(id);
    if (running) {
      this.sinks.get(id)?.({ events: [], end: { stop: 'cancelled', pageCapHit: false, fileCapHit: false, error: null } });
    }
    return Promise.resolve();
  }

  discard(id: number): Promise<void> {
    this.stop(id);
    this.sinks.delete(id);
    return Promise.resolve();
  }

  private stop(id: number): void {
    for (const t of this.timers.get(id) ?? []) {
      clearTimeout(t);
    }
    this.timers.delete(id);
  }

  add(_id: number, urls: readonly string[]): Promise<BatchResult> {
    return Promise.resolve({ added: urls.map((_, i) => 1000 + i), skipped: [] });
  }
}
