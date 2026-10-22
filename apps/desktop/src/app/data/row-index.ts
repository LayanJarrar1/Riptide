import { CategoryKey, Download, DownloadState, downloadedBytes, etaSeconds, isActive, progressOf } from '../model/download';

/** Sortable columns. */
export type SortKey = 'name' | 'size' | 'progress' | 'speed' | 'eta' | 'status' | 'added' | 'queue';

/** Sort direction. */
export type SortDir = 'asc' | 'desc';

/** A row as the table renders it: the download plus derived numbers computed once per change. */
export interface DownloadRow {
  readonly d: Download;
  readonly downloaded: number;
  /** 0..1 or `null` when unknown. */
  readonly progress: number | null;
  readonly eta: number | null;
  /** Lower-cased name and link, filled the first time a search needs it (see `haystack`). */
  hay?: string;
}

/** Downloads of one queue, by what they are doing. */
export interface QueueCount {
  readonly active: number;
  readonly waiting: number;
  readonly total: number;
}

/** Everything the sidebar and status bar count. */
export interface Stats {
  readonly byState: Readonly<Record<DownloadState, number>>;
  readonly categories: ReadonlyMap<CategoryKey, number>;
  readonly queues: ReadonlyMap<number, QueueCount>;
}

/** Which rows a list shows. `states` is compared by reference (the store passes constants). */
export interface RowFilter {
  readonly states: ReadonlySet<DownloadState>;
  readonly category: CategoryKey | null;
  readonly queue: number | null;
  /** Folded with `foldSearch` and trimmed; empty = no search. */
  readonly query: string;
  /** States whose name the search text matches ("paused", "مكتمل"): their rows match too. */
  readonly queryStates?: ReadonlySet<DownloadState>;
  /** Categories whose name the search text matches ("documents", "فيديو"): their rows match too. */
  readonly queryCategories?: ReadonlySet<CategoryKey>;
}

/** What one update produced. The arrays are never mutated afterwards (callers may keep them). */
export interface IndexResult {
  /** Every download as a row, in the engine's order. */
  readonly all: readonly DownloadRow[];
  /** The rows that pass the filter, in the chosen order. */
  readonly rows: readonly DownloadRow[];
  readonly stats: Stats;
}

const STATE_ORDER: Readonly<Record<DownloadState, number>> = {
  downloading: 0,
  probing: 1,
  verifying: 2,
  queued: 3,
  paused: 4,
  failed: 5,
  cancelled: 6,
  completed: 7,
};

/** Shared collator: `localeCompare` builds one per call, which dominates a 50 000-row sort. */
const COLLATOR = new Intl.Collator();

/** Lower-cased name and link of a row, built once per row object. */
export function haystack(row: DownloadRow): string {
  return (row.hay ??= foldSearch(`${row.d.filename}\n${row.d.url}`));
}

/**
 * Text as search compares it: compatibility-normalised and lower-cased, with Arabic diacritics
 * and tatweel dropped and the alef / yeh variants unified, so "إحصاء" finds "احصاء" and a
 * full-width "ＰＤＦ" finds "pdf". Applied to both the query and the names searched.
 */
export function foldSearch(text: string): string {
  return text
    .normalize('NFKC')
    .toLowerCase()
    .replace(/[\u064B-\u065F\u0670\u0640]/g, '')
    .replace(/[\u0622\u0623\u0625\u0671]/g, '\u0627')
    .replace(/\u0649/g, '\u064A');
}

/** A row for a download. */
export function makeRow(d: Download): DownloadRow {
  return { d, downloaded: downloadedBytes(d), progress: progressOf(d), eta: etaSeconds(d) };
}

/** Negative when `a` sorts before `b` for `key` (ascending). */
export function compare(a: DownloadRow, b: DownloadRow, key: SortKey): number {
  switch (key) {
    case 'name':
      return COLLATOR.compare(a.d.filename, b.d.filename);
    case 'size':
      return (a.d.size ?? -1) - (b.d.size ?? -1);
    case 'progress':
      return (a.progress ?? -1) - (b.progress ?? -1);
    case 'speed':
      return a.d.speedBps - b.d.speedBps;
    case 'eta':
      return (a.eta ?? Number.MAX_SAFE_INTEGER) - (b.eta ?? Number.MAX_SAFE_INTEGER);
    case 'status':
      return STATE_ORDER[a.d.state] - STATE_ORDER[b.d.state];
    case 'added':
      return a.d.addedAt - b.d.addedAt;
    case 'queue':
      return (a.d.queuePosition ?? 0) - (b.d.queuePosition ?? 0) || a.d.id - b.d.id;
  }
}

/** Whether a row passes the filter. */
export function matches(row: DownloadRow, f: RowFilter): boolean {
  const d = row.d;
  return (
    f.states.has(d.state) &&
    (f.category === null || d.category === f.category) &&
    (f.queue === null || d.queueId === f.queue) &&
    (f.query.length === 0 || haystack(row).includes(f.query) || f.queryStates?.has(d.state) === true || (d.category !== null && f.queryCategories?.has(d.category) === true))
  );
}

function sameSet<T>(a: ReadonlySet<T> | undefined, b: ReadonlySet<T> | undefined): boolean {
  if (a === b) {
    return true;
  }
  return (a?.size ?? 0) === (b?.size ?? 0) && [...(a ?? [])].every((v) => b?.has(v) === true);
}

function sameFilter(a: RowFilter, b: RowFilter): boolean {
  return (
    a.states === b.states &&
    a.category === b.category &&
    a.queue === b.queue &&
    a.query === b.query &&
    sameSet(a.queryStates, b.queryStates) &&
    sameSet(a.queryCategories, b.queryCategories)
  );
}

/** Counters being changed; turned into a read-only {@link Stats} when done. */
interface Counters {
  byState: Record<DownloadState, number>;
  categories: Map<CategoryKey, number>;
  queues: Map<number, { active: number; waiting: number; total: number }>;
}

function emptyCounters(): Counters {
  return {
    byState: { queued: 0, probing: 0, downloading: 0, paused: 0, verifying: 0, completed: 0, failed: 0, cancelled: 0 },
    categories: new Map(),
    queues: new Map(),
  };
}

/** Adds (`+1`) or removes (`-1`) one download from the counters. */
function count(c: Counters, d: Download, delta: 1 | -1): void {
  c.byState[d.state] += delta;
  const cat = (c.categories.get(d.category) ?? 0) + delta;
  if (cat > 0) {
    c.categories.set(d.category, cat);
  } else {
    c.categories.delete(d.category);
  }
  const prev = c.queues.get(d.queueId) ?? { active: 0, waiting: 0, total: 0 };
  // a new object each time: results already handed out must not change under their readers
  const next = {
    active: prev.active + (isActive(d.state) ? delta : 0),
    waiting: prev.waiting + (d.state === 'queued' ? delta : 0),
    total: prev.total + delta,
  };
  if (next.total > 0) {
    c.queues.set(d.queueId, next);
  } else {
    c.queues.delete(d.queueId);
  }
}

/**
 * The list screen's data, kept up to date incrementally. The engine replaces only the downloads
 * that changed, so between two updates almost every entry is the same object; `update` finds the
 * changed ones with one pointer compare per download and then does work proportional to them:
 *
 * - their rows are rebuilt, the rest are reused;
 * - the counters are adjusted (-1 for the old state, +1 for the new);
 * - the sort order is kept when each changed row still sits between its neighbours (progress and
 *   speed ticks almost never reorder an "added" or "name" sort), and otherwise re-sorted starting
 *   from the previous order, which TimSort finishes in about one pass;
 * - the filtered rows are patched in place when no changed row entered or left the filter.
 *
 * A new filter, a new sort, or a list that was added to, removed from or reordered falls back to
 * the matching amount of work (a filter pass, a re-sort, a full rebuild). The result is always what
 * a from-scratch filter and sort would give; the spec compares them on random edits.
 */
export class RowIndex {
  private all: DownloadRow[] = [];
  /** Indexes into `all`, sorted. */
  private order: number[] = [];
  /** `order` inverted: the position of each index of `all` in `order`. */
  private ordPos = new Int32Array(0);
  private rows: DownloadRow[] = [];
  /** Position of each index of `all` in `rows`, or -1 when filtered out. */
  private pos = new Int32Array(0);
  private filter: RowFilter | null = null;
  private key: SortKey | null = null;
  private dir: SortDir | null = null;
  private counters: Counters = emptyCounters();
  private result: IndexResult | null = null;

  update(ds: readonly Download[], filter: RowFilter, key: SortKey, dir: SortDir): IndexResult {
    const changed = this.diff(ds);
    if (changed === null) {
      return this.rebuild(ds, filter, key, dir);
    }
    const sameSort = this.key === key && this.dir === dir;
    const sameRows = this.filter !== null && sameFilter(this.filter, filter);
    if (changed.length === 0 && sameSort && sameRows && this.result !== null) {
      return this.result;
    }

    const all = changed.length === 0 ? this.all : this.all.slice();
    if (changed.length > 0) {
      this.counters = this.cloneCounters();
      for (const i of changed) {
        count(this.counters, all[i].d, -1);
        all[i] = makeRow(ds[i]);
        count(this.counters, ds[i], 1);
      }
    }
    this.all = all;

    const sign = dir === 'asc' ? 1 : -1;
    const cmp = (a: number, b: number): number => sign * compare(all[a], all[b], key) || a - b;

    let resort = !sameSort;
    if (!resort) {
      const last = this.order.length - 1;
      for (const i of changed) {
        const p = this.ordPos[i];
        if ((p > 0 && cmp(this.order[p - 1], i) > 0) || (p < last && cmp(i, this.order[p + 1]) > 0)) {
          resort = true;
          break;
        }
      }
    }
    if (resort) {
      this.order = this.order.slice();
      this.order.sort(cmp);
      this.invertOrder();
    }

    const patch = !resort && sameRows && changed.every((i) => (this.pos[i] >= 0) === matches(all[i], filter));
    if (patch) {
      if (changed.length > 0) {
        const rows = this.rows.slice();
        for (const i of changed) {
          if (this.pos[i] >= 0) {
            rows[this.pos[i]] = all[i];
          }
        }
        this.rows = rows;
      }
    } else {
      this.refilter(filter);
    }
    this.filter = filter;
    this.key = key;
    this.dir = dir;
    return this.publish();
  }

  /** Indexes of downloads that are not the same object as before; `null` when the list changed shape. */
  private diff(ds: readonly Download[]): number[] | null {
    if (this.result === null || ds.length !== this.all.length) {
      return null;
    }
    const changed: number[] = [];
    for (let i = 0; i < ds.length; i++) {
      const d = ds[i];
      const row = this.all[i];
      if (row.d !== d) {
        if (row.d.id !== d.id) {
          return null;
        }
        changed.push(i);
      }
    }
    return changed;
  }

  /** From scratch: rows, counters, a full sort, the filter pass. */
  private rebuild(ds: readonly Download[], filter: RowFilter, key: SortKey, dir: SortDir): IndexResult {
    const reuse = new Map<Download, DownloadRow>();
    for (const row of this.all) {
      reuse.set(row.d, row);
    }
    const counters = emptyCounters();
    const all = new Array<DownloadRow>(ds.length);
    for (let i = 0; i < ds.length; i++) {
      const d = ds[i];
      all[i] = reuse.get(d) ?? makeRow(d);
      count(counters, d, 1);
    }
    this.all = all;
    this.counters = counters;
    const sign = dir === 'asc' ? 1 : -1;
    const order = new Array<number>(ds.length);
    for (let i = 0; i < order.length; i++) {
      order[i] = i;
    }
    order.sort((a, b) => sign * compare(all[a], all[b], key) || a - b);
    this.order = order;
    this.invertOrder();
    this.refilter(filter);
    this.filter = filter;
    this.key = key;
    this.dir = dir;
    return this.publish();
  }

  private invertOrder(): void {
    const inverse = new Int32Array(this.order.length);
    for (let p = 0; p < this.order.length; p++) {
      inverse[this.order[p]] = p;
    }
    this.ordPos = inverse;
  }

  /** The filter pass over the sorted order. */
  private refilter(filter: RowFilter): void {
    const rows: DownloadRow[] = [];
    const pos = new Int32Array(this.all.length).fill(-1);
    for (const index of this.order) {
      const row = this.all[index];
      if (matches(row, filter)) {
        pos[index] = rows.length;
        rows.push(row);
      }
    }
    this.rows = rows;
    this.pos = pos;
  }

  private cloneCounters(): Counters {
    return { byState: { ...this.counters.byState }, categories: new Map(this.counters.categories), queues: new Map(this.counters.queues) };
  }

  private publish(): IndexResult {
    this.result = { all: this.all, rows: this.rows, stats: this.counters };
    return this.result;
  }
}
