import { computed, inject, Injectable, signal } from '@angular/core';

import { CategoryKey, DownloadState } from '../model/download';
import { DownloadEngine } from './download-engine';
import { ListNav, navigateIndex } from './list-nav';
import { DownloadRow, QueueCount, RowIndex, SortDir, SortKey } from './row-index';

export type { DownloadRow, QueueCount, SortDir, SortKey } from './row-index';

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

const VIEW_STATE_SETS: Readonly<Record<ListView, ReadonlySet<DownloadState>>> = {
  all: new Set(VIEW_STATES.all),
  active: new Set(VIEW_STATES.active),
  queued: new Set(VIEW_STATES.queued),
  completed: new Set(VIEW_STATES.completed),
  failed: new Set(VIEW_STATES.failed),
};

const NO_ROWS: readonly DownloadRow[] = [];

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
  /** The row the keyboard is on (a download id, not a DOM node, so it survives virtualisation). */
  readonly cursor = signal<number | null>(null);
  /** Where a Shift-extended selection starts. */
  private anchor: number | null = null;
  /** Download shown in the details drawer, if any. */
  readonly detailsId = signal<number | null>(null);

  /** Rows, sort order and counters, kept up to date between engine ticks (see `RowIndex`). */
  private readonly index = new RowIndex();

  private readonly derived = computed(() =>
    this.index.update(
      this.engine.downloads(),
      { states: VIEW_STATE_SETS[this.view()], category: this.category(), queue: this.queue(), query: this.query().trim().toLowerCase() },
      this.sortKey(),
      this.sortDir(),
    ),
  );

  /** All rows with derived numbers, before filtering, in the engine's order. */
  readonly allRows = computed<readonly DownloadRow[]>(() => this.derived().all);

  /** Rows after view, category and search filters, in the chosen order. */
  readonly rows = computed<readonly DownloadRow[]>(() => this.derived().rows);

  private readonly stats = computed(() => this.derived().stats);

  /** Count per view for the sidebar badges. */
  readonly viewCounts = computed<Readonly<Record<ListView, number>>>(() => {
    const { byState } = this.stats();
    const count = (view: ListView): number => VIEW_STATES[view].reduce((n, state) => n + byState[state], 0);
    return { all: count('all'), active: count('active'), queued: count('queued'), completed: count('completed'), failed: count('failed') };
  });

  /** Count per category for the sidebar. */
  readonly categoryCounts = computed<ReadonlyMap<CategoryKey, number>>(() => this.stats().categories);

  /** Per queue: downloads holding a slot, waiting ones, and all of them. */
  readonly queueCounts = computed<ReadonlyMap<number, QueueCount>>(() => this.stats().queues);

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
    return ids.size === 0 ? NO_ROWS : this.allRows().filter((r) => ids.has(r.d.id));
  });

  /** Live totals for the status bar. */
  readonly totals = computed(() => {
    const { byState } = this.stats();
    return {
      active: byState.probing + byState.downloading + byState.verifying,
      speedBps: this.engine.totalSpeedBps(),
      history: this.engine.totalSpeedHistory(),
      queued: byState.queued,
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
    this.cursor.set(id);
    this.anchor = id;
  }

  /** Ctrl/⌘ click: add or remove. */
  toggle(id: number): void {
    const next = new Set(this.selected());
    const added = !next.delete(id);
    if (added) {
      next.add(id);
    }
    this.selected.set(next);
    this.cursor.set(id);
    // a new Ctrl-click starts the next range; unticking one falls back to the first still selected
    this.anchor = added ? id : (next.values().next().value ?? null);
  }

  /** Shift click: select every visible row between the anchor row and `id`. */
  selectRange(id: number): void {
    const visible = this.rows();
    const b = visible.findIndex((r) => r.d.id === id);
    const selected = this.selected();
    const anchorId = this.anchor !== null && visible.some((r) => r.d.id === this.anchor) ? this.anchor : (visible.find((r) => selected.has(r.d.id))?.d.id ?? id);
    const a = visible.findIndex((r) => r.d.id === anchorId);
    if (a < 0 || b < 0) {
      this.select(id);
      return;
    }
    const [lo, hi] = a < b ? [a, b] : [b, a];
    const next = new Set<number>();
    for (let i = lo; i <= hi; i++) {
      next.add(visible[i].d.id);
    }
    this.selected.set(next);
    this.cursor.set(id);
    this.anchor = anchorId;
  }

  /** Every visible row (Ctrl+A). The cursor stays where it is. */
  selectAll(): void {
    const rows = this.rows();
    this.selected.set(new Set(rows.map((r) => r.d.id)));
    if (this.anchor === null && rows.length > 0) {
      this.anchor = rows[0].d.id;
    }
  }

  clearSelection(): void {
    this.selected.set(new Set());
    this.cursor.set(null);
    this.anchor = null;
  }

  /**
   * Keyboard move of the cursor through the visible rows. Without `extend` the selection
   * follows the cursor; with it (Shift) the selection spans anchor to cursor. Everything is
   * kept by download id, so it does not matter which rows the virtual list has in the DOM.
   * Returns the id and visible index the cursor landed on, `null` for an empty list.
   */
  navigate(nav: ListNav, pageSize: number, extend = false): { readonly id: number; readonly index: number } | null {
    const visible = this.rows();
    const at = this.cursorIndex(visible);
    const index = navigateIndex(nav, at, visible.length, pageSize);
    if (index === null) {
      return null;
    }
    const id = visible[index].d.id;
    if (extend) {
      this.selectRange(id);
    } else {
      this.select(id);
      if (this.detailsId() !== null) {
        this.detailsId.set(id);
      }
    }
    return { id, index };
  }

  /** Moves the selection one visible row up or down; selects the first row when nothing is. */
  moveSelection(delta: 1 | -1): number | null {
    return this.navigate(delta > 0 ? 'next' : 'prev', 1)?.id ?? null;
  }

  /** Position of the cursor among `visible`: the cursor row, else the first selected one, else `null`. */
  private cursorIndex(visible: readonly DownloadRow[]): number | null {
    const cursor = this.cursor();
    if (cursor !== null) {
      const i = visible.findIndex((r) => r.d.id === cursor);
      if (i >= 0) {
        return i;
      }
    }
    const selected = this.selected();
    if (selected.size === 0) {
      return null;
    }
    const first = visible.findIndex((r) => selected.has(r.d.id));
    return first >= 0 ? first : null;
  }

  openDetails(id: number): void {
    this.detailsId.set(id);
    this.select(id);
  }

  closeDetails(): void {
    this.detailsId.set(null);
  }
}
