import { computed, inject, Injectable, signal } from '@angular/core';

import {
  CategoryKey,
  Download,
  DownloadState,
  downloadedBytes,
  etaSeconds,
  isActive,
  progressOf,
} from '../model/download';
import { DownloadEngine } from './download-engine';

/** Sidebar views: which states a list shows. */
export type ListView = 'all' | 'active' | 'queued' | 'completed' | 'failed';

/** Views in sidebar order. */
export const LIST_VIEWS: readonly ListView[] = ['all', 'active', 'queued', 'completed', 'failed'];

const VIEW_STATES: Readonly<Record<ListView, readonly DownloadState[]>> = {
  all: ['queued', 'probing', 'downloading', 'paused', 'verifying', 'completed', 'failed', 'cancelled'],
  active: ['probing', 'downloading', 'verifying'],
  queued: ['queued', 'paused'],
  completed: ['completed'],
  failed: ['failed', 'cancelled'],
};

/** Downloads of one queue, by what they are doing. */
export interface QueueCount {
  readonly active: number;
  readonly waiting: number;
  readonly total: number;
}

/** Sortable columns. */
export type SortKey = 'name' | 'size' | 'progress' | 'speed' | 'eta' | 'status' | 'added' | 'queue';

/** Sort direction. */
export type SortDir = 'asc' | 'desc';

/** A row as the table renders it: the download plus derived numbers computed once per tick. */
export interface DownloadRow {
  readonly d: Download;
  readonly downloaded: number;
  /** 0..1 or `null` when unknown. */
  readonly progress: number | null;
  readonly eta: number | null;
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

/**
 * Everything the list screen needs that is not the engine's business: the active view and
 * filters, sort order, selection and which item the details drawer shows. All signals.
 */
@Injectable({ providedIn: 'root' })
export class DownloadsStore {
  private readonly engine = inject(DownloadEngine);

  readonly view = signal<ListView>('all');
  readonly category = signal<CategoryKey | null>(null);
  /** Queue the list is limited to (`null` = every queue). */
  readonly queue = signal<number | null>(null);
  readonly query = signal('');
  readonly sortKey = signal<SortKey>('added');
  readonly sortDir = signal<SortDir>('desc');
  readonly selected = signal<ReadonlySet<number>>(new Set());
  /** Download shown in the details drawer, if any. */
  readonly detailsId = signal<number | null>(null);

  /** All rows with derived numbers, before filtering. */
  readonly allRows = computed<readonly DownloadRow[]>(() =>
    this.engine.downloads().map((d) => ({
      d,
      downloaded: downloadedBytes(d),
      progress: progressOf(d),
      eta: etaSeconds(d),
    })),
  );

  /** Rows after view, category and search filters, in the chosen order. */
  readonly rows = computed<readonly DownloadRow[]>(() => {
    const states = VIEW_STATES[this.view()];
    const category = this.category();
    const queue = this.queue();
    const q = this.query().trim().toLowerCase();
    const filtered = this.allRows().filter(
      ({ d }) =>
        states.includes(d.state) &&
        (category === null || d.category === category) &&
        (queue === null || d.queueId === queue) &&
        (q.length === 0 || d.filename.toLowerCase().includes(q) || d.url.toLowerCase().includes(q)),
    );
    const key = this.sortKey();
    const sign = this.sortDir() === 'asc' ? 1 : -1;
    return [...filtered].sort((a, b) => sign * compare(a, b, key));
  });

  /** Count per view for the sidebar badges. */
  readonly viewCounts = computed<Readonly<Record<ListView, number>>>(() => {
    const all = this.engine.downloads();
    const count = (view: ListView): number => all.filter((d) => VIEW_STATES[view].includes(d.state)).length;
    return { all: all.length, active: count('active'), queued: count('queued'), completed: count('completed'), failed: count('failed') };
  });

  /** Count per category for the sidebar. */
  readonly categoryCounts = computed<ReadonlyMap<CategoryKey, number>>(() => {
    const m = new Map<CategoryKey, number>();
    for (const d of this.engine.downloads()) {
      m.set(d.category, (m.get(d.category) ?? 0) + 1);
    }
    return m;
  });

  /** Per queue: downloads holding a slot, waiting ones, and all of them. */
  readonly queueCounts = computed<ReadonlyMap<number, QueueCount>>(() => {
    const m = new Map<number, { active: number; waiting: number; total: number }>();
    for (const d of this.engine.downloads()) {
      const c = m.get(d.queueId) ?? { active: 0, waiting: 0, total: 0 };
      c.total++;
      if (isActive(d.state)) {
        c.active++;
      } else if (d.state === 'queued') {
        c.waiting++;
      }
      m.set(d.queueId, c);
    }
    return m;
  });

  /** The list shows one queue in its manual order: rows can be dragged and moved up and down. */
  readonly manualOrder = computed(() => this.queue() !== null && this.sortKey() === 'queue' && this.sortDir() === 'asc');

  /** The download in the drawer, kept fresh as the engine ticks. */
  readonly details = computed<DownloadRow | null>(() => {
    const id = this.detailsId();
    return id === null ? null : (this.allRows().find((r) => r.d.id === id) ?? null);
  });

  /** Selected rows that are still in the list. */
  readonly selectedRows = computed<readonly DownloadRow[]>(() => {
    const ids = this.selected();
    return this.allRows().filter((r) => ids.has(r.d.id));
  });

  /** Live totals for the status bar. */
  readonly totals = computed(() => {
    const all = this.engine.downloads();
    const active = all.filter((d) => isActive(d.state));
    return {
      active: active.length,
      speedBps: this.engine.totalSpeedBps(),
      history: this.engine.totalSpeedHistory(),
      queued: all.filter((d) => d.state === "queued").length,
      maxActive: this.engine.maxActive(),
    };
  });

  setView(view: ListView): void {
    this.view.set(view);
    this.category.set(null);
    this.setQueue(null);
  }

  /**
   * Limits the list to one queue (`null` = all). A queue is shown in its manual order; leaving
   * it puts the sort back to "newest first" if the manual order was in use.
   */
  setQueue(queue: number | null): void {
    this.queue.set(queue);
    if (queue !== null) {
      this.view.set('all');
      this.category.set(null);
      this.sortKey.set('queue');
      this.sortDir.set('asc');
    } else if (this.sortKey() === 'queue') {
      this.sortKey.set('added');
      this.sortDir.set('desc');
    }
  }

  /** Ids of a queue's downloads in manual order, whatever the view and search show. */
  queueOrder(queue: number): readonly number[] {
    return this.engine
      .downloads()
      .filter((d) => d.queueId === queue)
      .sort((a, b) => (a.queuePosition ?? 0) - (b.queuePosition ?? 0) || a.id - b.id)
      .map((d) => d.id);
  }

  setCategory(category: CategoryKey | null): void {
    this.category.set(category);
  }

  /** Toggles direction when the same column is chosen again. */
  sortBy(key: SortKey): void {
    if (this.sortKey() === key) {
      this.sortDir.update((d) => (d === 'asc' ? 'desc' : 'asc'));
    } else {
      this.sortKey.set(key);
      this.sortDir.set(key === 'name' || key === 'status' ? 'asc' : 'desc');
    }
  }

  /** Plain click: select only this row. */
  select(id: number): void {
    this.selected.set(new Set([id]));
  }

  /** Ctrl/⌘ click: add or remove. */
  toggle(id: number): void {
    this.selected.update((s) => {
      const next = new Set(s);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  /** Shift click: select every visible row between the first selected row and `id`. */
  selectRange(id: number): void {
    const visible = this.rows().map((r) => r.d.id);
    const anchor = [...this.selected()].find((s) => visible.includes(s)) ?? id;
    const a = visible.indexOf(anchor);
    const b = visible.indexOf(id);
    if (a < 0 || b < 0) {
      this.select(id);
      return;
    }
    const [lo, hi] = a < b ? [a, b] : [b, a];
    this.selected.set(new Set(visible.slice(lo, hi + 1)));
  }

  clearSelection(): void {
    this.selected.set(new Set());
  }

  /** Moves the selection one visible row up or down; selects the first row when nothing is. */
  moveSelection(delta: 1 | -1): number | null {
    const visible = this.rows().map((r) => r.d.id);
    if (visible.length === 0) {
      return null;
    }
    const current = [...this.selected()].find((s) => visible.includes(s));
    const index = current === undefined ? (delta > 0 ? 0 : visible.length - 1) : Math.min(visible.length - 1, Math.max(0, visible.indexOf(current) + delta));
    const id = visible[index];
    this.select(id);
    if (this.detailsId() !== null) {
      this.detailsId.set(id);
    }
    return id;
  }

  openDetails(id: number): void {
    this.detailsId.set(id);
    this.select(id);
  }

  closeDetails(): void {
    this.detailsId.set(null);
  }
}

function compare(a: DownloadRow, b: DownloadRow, key: SortKey): number {
  switch (key) {
    case 'name':
      return a.d.filename.localeCompare(b.d.filename);
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
