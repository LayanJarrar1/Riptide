/**
 * UI-side model of the site grabber. The wire shapes (`CrawlEvent`, `FoundFile`, `CrawlEnd`)
 * mirror `riptide-crawl` and `src-tauri/src/crawl.rs`; `toUiEvent` turns them into the small
 * shapes the dialog works with, and the pure helpers here (grouping, filtering, pattern lists)
 * are what its specs exercise.
 */

import type { CategoryKey } from './download';
import { CATEGORY_KEYS } from './download';

/** Which pages the crawl may walk into. */
export type CrawlScope = 'same-host' | 'same-domain' | 'any';

/** Scopes in menu order. */
export const CRAWL_SCOPES: readonly CrawlScope[] = ['same-host', 'same-domain', 'any'];

/** Deepest link depth the engine accepts (`MAX_DEPTH` in `crawl.rs`). */
export const MAX_CRAWL_DEPTH = 5;

/** What `start_crawl` takes (camelCase on the wire). */
export interface CrawlSpecInput {
  readonly url: string;
  readonly depth: number;
  readonly scope: CrawlScope;
  readonly offsiteFiles: boolean;
  readonly categories: readonly CategoryKey[];
  readonly include: readonly string[];
  readonly exclude: readonly string[];
  readonly minSize: number | null;
  readonly maxSize: number | null;
}

/** One file the crawl found, as the list shows it. */
export interface CrawlFile {
  readonly url: string;
  readonly name: string;
  readonly host: string;
  readonly category: CategoryKey;
  /** Bytes, when the crawl probed the file. */
  readonly size: number | null;
}

/** Counters the dialog shows. */
export interface CrawlCounters {
  readonly pages: number;
  readonly files: number;
  readonly skipped: number;
}

/** A progress event in the shape the dialog consumes. */
export type CrawlUiEvent =
  | { readonly kind: 'page'; readonly url: string; readonly depth: number }
  | { readonly kind: 'file'; readonly file: CrawlFile }
  | { readonly kind: 'skipped'; readonly url: string }
  | { readonly kind: 'done'; readonly counters: CrawlCounters };

/** How the crawl ended. */
export interface CrawlEnd {
  /** `null` when the crawl could not run at all. */
  readonly stop: 'finished' | 'cancelled' | 'time-budget' | null;
  readonly pageCapHit: boolean;
  readonly fileCapHit: boolean;
  readonly error: string | null;
}

/** One delivery from the backend: events in order, and `end` on the last. */
export interface CrawlUpdate {
  readonly events: readonly CrawlUiEvent[];
  readonly end?: CrawlEnd;
}

/** Where the chosen files go (the `add_crawl_files` options). */
export interface CrawlAddOptions {
  readonly dir: string;
  readonly queueId: number;
  readonly startPaused: boolean;
}

/** `riptide_crawl::FoundFile` on the wire. */
export interface WireFoundFile {
  readonly url: string;
  readonly guessed_name: string;
  readonly category: CategoryKey;
  readonly size: number | null;
}

/** `riptide_crawl::CrawlEvent` on the wire. */
export type WireCrawlEvent =
  | { readonly type: 'page-visited'; readonly url: string; readonly depth: number; readonly links: number }
  | ({ readonly type: 'file-found' } & WireFoundFile)
  | { readonly type: 'skipped'; readonly url: string }
  | { readonly type: 'done'; readonly stats: { readonly pages_visited: number; readonly files_found: number; readonly skipped: number } };

/** The `riptide://crawl` payload. */
export interface WireCrawlBatch {
  readonly id: number;
  readonly events: readonly WireCrawlEvent[];
  readonly end: CrawlEnd | null;
}

/** The host part of a link (`''` when it does not parse). */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
}

/** Wire event → dialog event. */
export function toUiEvent(e: WireCrawlEvent): CrawlUiEvent {
  switch (e.type) {
    case 'page-visited':
      return { kind: 'page', url: e.url, depth: e.depth };
    case 'file-found':
      return { kind: 'file', file: { url: e.url, name: e.guessed_name, host: hostOf(e.url), category: e.category, size: e.size } };
    case 'skipped':
      return { kind: 'skipped', url: e.url };
    case 'done':
      return { kind: 'done', counters: { pages: e.stats.pages_visited, files: e.stats.files_found, skipped: e.stats.skipped } };
  }
}

/** One row of the grouped, virtualised result list. */
export type CrawlRow =
  | { readonly kind: 'group'; readonly category: CategoryKey; readonly count: number; readonly urls: readonly string[] }
  | { readonly kind: 'file'; readonly file: CrawlFile };

/** Whether `file` passes the text filter (name, host or link, case-insensitive; blank = all). */
export function matchesFilter(file: CrawlFile, filter: string): boolean {
  const needle = filter.trim().toLowerCase();
  return needle.length === 0 || file.name.toLowerCase().includes(needle) || file.host.toLowerCase().includes(needle) || file.url.toLowerCase().includes(needle);
}

/**
 * The files that pass `filter`, grouped by type (in the sidebar's order, empty groups left
 * out), each group a header row followed by its files in discovery order.
 */
export function groupRows(files: readonly CrawlFile[], filter: string): readonly CrawlRow[] {
  const buckets = new Map<CategoryKey, CrawlFile[]>();
  for (const file of files) {
    if (!matchesFilter(file, filter)) {
      continue;
    }
    const bucket = buckets.get(file.category);
    if (bucket) {
      bucket.push(file);
    } else {
      buckets.set(file.category, [file]);
    }
  }
  const rows: CrawlRow[] = [];
  for (const category of CATEGORY_KEYS) {
    const bucket = buckets.get(category);
    if (bucket) {
      rows.push({ kind: 'group', category, count: bucket.length, urls: bucket.map((f) => f.url) });
      for (const file of bucket) {
        rows.push({ kind: 'file', file });
      }
    }
  }
  return rows;
}

/** A pattern box → patterns: one per line (commas also split), trimmed, blanks dropped. */
export function parsePatterns(text: string): readonly string[] {
  return text
    .split(/[\r\n,]+/)
    .map((p) => p.trim())
    .filter((p) => p.length > 0);
}

/** Whether `text` is an `http(s)` link with a host. */
export function isHttpLink(text: string): boolean {
  return /^https?:\/\/[^\s/?#]+[^\s]*$/i.test(text.trim());
}

/** Megabytes typed in a box → bytes; `null` for blank, negative or unreadable input. */
export function megabytesToBytes(text: string): number | null {
  const t = text.trim().replace(',', '.');
  if (t.length === 0) {
    return null;
  }
  const n = Number(t);
  return Number.isFinite(n) && n >= 0 ? Math.round(n * 1024 * 1024) : null;
}
