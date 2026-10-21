import { CategoryKey, Download, DownloadState, isActive } from '../model/download';
import { mulberry32, planSegments } from './mock-data';
import { compare, DownloadRow, makeRow, matches, RowFilter, RowIndex, SortDir, SortKey } from './row-index';

const STATES: readonly DownloadState[] = ['queued', 'probing', 'downloading', 'paused', 'verifying', 'completed', 'failed', 'cancelled'];
const CATEGORIES: readonly CategoryKey[] = ['video', 'music', 'documents', 'compressed', 'programs', 'images', 'other'];
const KEYS: readonly SortKey[] = ['name', 'size', 'progress', 'speed', 'eta', 'status', 'added', 'queue'];
const ALL_STATES: ReadonlySet<DownloadState> = new Set(STATES);
const ACTIVE_STATES: ReadonlySet<DownloadState> = new Set<DownloadState>(['probing', 'downloading', 'verifying']);

function download(id: number, rng: () => number): Download {
  const size = 1_000_000 + Math.floor(rng() * 9_000_000);
  const state = STATES[Math.floor(rng() * STATES.length)];
  const segments = planSegments(size, 4).map((s) => ({ ...s, written: Math.floor((s.end - s.start) * rng() * 0.9) }));
  return {
    id,
    url: `https://example.com/f${id}`,
    filename: `file-${Math.floor(rng() * 400)}-${id}.zip`,
    dir: 'D:\\Downloads',
    size: rng() < 0.1 ? null : size,
    supportsRanges: true,
    state,
    category: CATEGORIES[Math.floor(rng() * CATEGORIES.length)],
    queueId: 1 + Math.floor(rng() * 3),
    queuePosition: Math.floor(rng() * 50),
    priority: 'normal',
    connections: 4,
    segments,
    speedBps: state === 'downloading' ? Math.floor(rng() * 5_000_000) : 0,
    speedHistory: [],
    addedAt: 1_000_000 + id * 10 + Math.floor(rng() * 5),
    startedAt: null,
    completedAt: null,
    failure: null,
    checksum: null,
  };
}

/** What a from-scratch filter and sort give, with the same tie-break the index promises. */
function reference(ds: readonly Download[], filter: RowFilter, key: SortKey, dir: SortDir): number[] {
  const sign = dir === 'asc' ? 1 : -1;
  return ds
    .map((d, i) => ({ row: makeRow(d), i }))
    .filter(({ row }) => matches(row, filter))
    .sort((a, b) => sign * compare(a.row, b.row, key) || a.i - b.i)
    .map(({ row }) => row.d.id);
}

function referenceStats(ds: readonly Download[]): { states: Record<string, number>; categories: [string, number][]; queues: [number, number, number, number][] } {
  const states: Record<string, number> = Object.fromEntries(STATES.map((s) => [s, 0]));
  const categories = new Map<string, number>();
  const queues = new Map<number, [number, number, number]>();
  for (const d of ds) {
    states[d.state]++;
    categories.set(d.category, (categories.get(d.category) ?? 0) + 1);
    const q = queues.get(d.queueId) ?? [0, 0, 0];
    q[0] += isActive(d.state) ? 1 : 0;
    q[1] += d.state === 'queued' ? 1 : 0;
    q[2]++;
    queues.set(d.queueId, q);
  }
  return {
    states,
    categories: [...categories].sort(),
    queues: [...queues].map(([id, q]) => [id, q[0], q[1], q[2]] as [number, number, number, number]).sort((a, b) => a[0] - b[0]),
  };
}

function statsOf(index: ReturnType<RowIndex['update']>): ReturnType<typeof referenceStats> {
  const { byState, categories, queues } = index.stats;
  return {
    states: { ...byState },
    categories: [...categories].sort(),
    queues: [...queues].map(([id, q]) => [id, q.active, q.waiting, q.total] as [number, number, number, number]).sort((a, b) => a[0] - b[0]),
  };
}

const FILTER_ALL: RowFilter = { states: ALL_STATES, category: null, queue: null, query: '' };

describe('RowIndex', () => {
  it('builds rows, the filtered order and the counters', () => {
    const rng = mulberry32(3);
    const ds = Array.from({ length: 200 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    const out = index.update(ds, FILTER_ALL, 'added', 'desc');
    expect(out.all.length).toBe(200);
    expect(out.rows.map((r) => r.d.id)).toEqual(reference(ds, FILTER_ALL, 'added', 'desc'));
    expect(statsOf(out)).toEqual(referenceStats(ds));
  });

  it('returns the same result when nothing changed', () => {
    const rng = mulberry32(4);
    const ds = Array.from({ length: 50 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    const first = index.update(ds, FILTER_ALL, 'name', 'asc');
    expect(index.update([...ds], FILTER_ALL, 'name', 'asc')).toBe(first);
  });

  it('reuses the row of every download that did not change and replaces only the ones that did', () => {
    const rng = mulberry32(5);
    const ds = Array.from({ length: 100 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    const first = index.update(ds, FILTER_ALL, 'added', 'desc');
    const next = ds.slice();
    next[10] = { ...ds[10], speedBps: ds[10].speedBps + 1 };
    next[60] = { ...ds[60], state: 'paused' };
    const second = index.update(next, FILTER_ALL, 'added', 'desc');
    const replaced = second.all.filter((r, i) => r !== first.all[i]).map((r) => r.d.id);
    expect(replaced).toEqual([ds[10].id, ds[60].id]);
    // the previous result is untouched (callers keep references)
    expect(first.all[10].d).toBe(ds[10]);
    expect(first.rows.length).toBe(reference(ds, FILTER_ALL, 'added', 'desc').length);
  });

  it('keeps the order without sorting when changed rows stay between their neighbours, and patches the rows', () => {
    const rng = mulberry32(6);
    const ds = Array.from({ length: 300 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    const first = index.update(ds, FILTER_ALL, 'added', 'desc');
    const sortSpy = spyOn(Array.prototype, 'sort').and.callThrough();
    const next = ds.slice();
    next[100] = { ...ds[100], speedBps: 123 };
    const second = index.update(next, FILTER_ALL, 'added', 'desc');
    expect(sortSpy).not.toHaveBeenCalled();
    expect(second.rows.map((r) => r.d.id)).toEqual(first.rows.map((r) => r.d.id));
    expect(second.rows.find((r) => r.d.id === ds[100].id)?.d.speedBps).toBe(123);
  });

  it('re-sorts when a change breaks the order', () => {
    const rng = mulberry32(7);
    const ds = Array.from({ length: 100 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    index.update(ds, FILTER_ALL, 'speed', 'desc');
    const next = ds.slice();
    next[3] = { ...ds[3], speedBps: 99_000_000, state: 'downloading' };
    const out = index.update(next, FILTER_ALL, 'speed', 'desc');
    expect(out.rows[0].d.id).toBe(ds[3].id);
    expect(out.rows.map((r) => r.d.id)).toEqual(reference(next, FILTER_ALL, 'speed', 'desc'));
  });

  it('follows a download entering or leaving the filter', () => {
    const rng = mulberry32(8);
    const ds = Array.from({ length: 120 }, (_, i) => download(i + 1, rng));
    const filter: RowFilter = { states: ACTIVE_STATES, category: null, queue: null, query: '' };
    const index = new RowIndex();
    index.update(ds, filter, 'added', 'desc');
    const idle = ds.findIndex((d) => !ACTIVE_STATES.has(d.state));
    const running = ds.findIndex((d) => ACTIVE_STATES.has(d.state));
    const next = ds.slice();
    next[idle] = { ...ds[idle], state: 'downloading' };
    next[running] = { ...ds[running], state: 'paused' };
    const out = index.update(next, filter, 'added', 'desc');
    expect(out.rows.map((r) => r.d.id)).toEqual(reference(next, filter, 'added', 'desc'));
    expect(out.rows.some((r) => r.d.id === ds[idle].id)).toBeTrue();
    expect(out.rows.some((r) => r.d.id === ds[running].id)).toBeFalse();
  });

  it('rebuilds when downloads are added, removed or reordered', () => {
    const rng = mulberry32(9);
    const ds = Array.from({ length: 80 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    index.update(ds, FILTER_ALL, 'name', 'asc');
    const added = [...ds, download(500, rng)];
    expect(index.update(added, FILTER_ALL, 'name', 'asc').rows.map((r) => r.d.id)).toEqual(reference(added, FILTER_ALL, 'name', 'asc'));
    const removed = added.filter((d) => d.id !== 7);
    const out = index.update(removed, FILTER_ALL, 'name', 'asc');
    expect(out.rows.map((r) => r.d.id)).toEqual(reference(removed, FILTER_ALL, 'name', 'asc'));
    expect(statsOf(out)).toEqual(referenceStats(removed));
    const swapped = removed.slice();
    [swapped[3], swapped[9]] = [swapped[9], swapped[3]];
    expect(index.update(swapped, FILTER_ALL, 'name', 'asc').rows.map((r) => r.d.id)).toEqual(reference(swapped, FILTER_ALL, 'name', 'asc'));
  });

  it('matches a from-scratch filter, sort and count after random edits, filters and sorts', () => {
    for (const seed of [11, 12, 13, 14]) {
      const rng = mulberry32(seed);
      let ds = Array.from({ length: 150 }, (_, i) => download(i + 1, rng));
      let nextId = 1000;
      const index = new RowIndex();
      let filter: RowFilter = FILTER_ALL;
      let key: SortKey = 'added';
      let dir: SortDir = 'desc';
      for (let step = 0; step < 120; step++) {
        const roll = rng();
        if (roll < 0.55) {
          // a tick: a few downloads change (some state, some numbers, some names)
          ds = ds.slice();
          for (let n = 0; n < 1 + Math.floor(rng() * 6); n++) {
            const i = Math.floor(rng() * ds.length);
            const d = ds[i];
            ds[i] =
              rng() < 0.3
                ? { ...d, state: STATES[Math.floor(rng() * STATES.length)], speedBps: Math.floor(rng() * 1e6) }
                : rng() < 0.2
                  ? { ...d, filename: `renamed-${Math.floor(rng() * 400)}.bin`, size: 12345 }
                  : { ...d, speedBps: Math.floor(rng() * 1e6), segments: d.segments.map((s) => ({ ...s, written: Math.min(s.end - s.start, s.written + 1000) })) };
          }
        } else if (roll < 0.62) {
          ds = [...ds, download(nextId++, rng)];
        } else if (roll < 0.68 && ds.length > 20) {
          const i = Math.floor(rng() * ds.length);
          ds = ds.filter((_, k) => k !== i);
        } else if (roll < 0.78) {
          key = KEYS[Math.floor(rng() * KEYS.length)];
          dir = rng() < 0.5 ? 'asc' : 'desc';
        } else if (roll < 0.9) {
          filter = {
            states: rng() < 0.5 ? ALL_STATES : ACTIVE_STATES,
            category: rng() < 0.5 ? null : CATEGORIES[Math.floor(rng() * CATEGORIES.length)],
            queue: rng() < 0.7 ? null : 1 + Math.floor(rng() * 3),
            query: rng() < 0.7 ? '' : String(Math.floor(rng() * 9)),
          };
        }
        // else: no change at all
        const out = index.update(ds, filter, key, dir);
        const rows: readonly DownloadRow[] = out.rows;
        expect(rows.map((r) => r.d.id)).withContext(`seed ${seed} step ${step} ${key} ${dir}`).toEqual(reference(ds, filter, key, dir));
        expect(out.all.map((r) => r.d)).toEqual(ds);
        expect(statsOf(out)).withContext(`stats, seed ${seed} step ${step}`).toEqual(referenceStats(ds));
      }
    }
  });

  it('handles an empty list and a list that empties', () => {
    const rng = mulberry32(2);
    const index = new RowIndex();
    expect(index.update([], FILTER_ALL, 'added', 'desc').rows).toEqual([]);
    const ds = Array.from({ length: 5 }, (_, i) => download(i + 1, rng));
    expect(index.update(ds, FILTER_ALL, 'added', 'desc').rows.length).toBe(5);
    const out = index.update([], FILTER_ALL, 'added', 'desc');
    expect(out.rows).toEqual([]);
    expect(out.stats.queues.size).toBe(0);
    expect(out.stats.categories.size).toBe(0);
  });

  it('an update on 50 000 downloads where a hundred changed is cheap', () => {
    const rng = mulberry32(21);
    const ds = Array.from({ length: 50_000 }, (_, i) => download(i + 1, rng));
    const index = new RowIndex();
    index.update(ds, FILTER_ALL, 'added', 'desc');
    const next = ds.slice();
    for (let i = 0; i < 100; i++) {
      const at = 40_000 + i * 50;
      next[at] = { ...ds[at], speedBps: ds[at].speedBps + 7 };
    }
    index.update(ds, FILTER_ALL, 'added', 'desc');
    const t0 = performance.now();
    for (let n = 0; n < 5; n++) {
      next[40_000] = { ...next[40_000], speedBps: n };
      index.update(next, FILTER_ALL, 'added', 'desc');
    }
    const perTick = (performance.now() - t0) / 5;
    // a full rebuild of 50 000 rows is tens of milliseconds; this must be a small fraction of it
    expect(perTick).toBeLessThan(30);
  });
});
