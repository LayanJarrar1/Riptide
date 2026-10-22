import { TestBed } from '@angular/core/testing';

import { DownloadsStore } from './downloads.store';
import { provideDownloadEngine } from './engine.provider';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from './mock-engine.service';

describe('DownloadsStore', () => {
  let store: DownloadsStore;
  let engine: MockEngineService;

  beforeEach(() => {
    TestBed.configureTestingModule({ providers: [provideDownloadEngine(), { provide: MOCK_ENGINE_AUTOSTART, useValue: false }] });
    store = TestBed.inject(DownloadsStore);
    engine = TestBed.inject(MockEngineService);
  });

  it('shows everything by default, newest first', () => {
    expect(store.rows().length).toBe(engine.downloads().length);
    const added = store.rows().map((r) => r.d.addedAt);
    expect(added).toEqual([...added].sort((a, b) => b - a));
  });

  it('views filter by state and counts match', () => {
    store.setView('active');
    expect(store.rows().every((r) => ['probing', 'downloading', 'verifying'].includes(r.d.state))).toBeTrue();
    expect(store.rows().length).toBe(store.viewCounts().active);
    store.setView('failed');
    expect(store.rows().every((r) => r.d.state === 'failed' || r.d.state === 'cancelled')).toBeTrue();
    store.setView('completed');
    expect(store.rows().every((r) => r.d.state === 'completed')).toBeTrue();
  });

  it('category filter and search narrow the list; changing the view clears the category', () => {
    store.setCategory('video');
    expect(store.rows().length).toBe(store.categoryCounts().get('video') ?? -1);
    expect(store.rows().every((r) => r.d.category === 'video')).toBeTrue();
    store.setView('all');
    expect(store.category()).toBeNull();
    store.query.set('UBUNTU');
    expect(store.rows().length).toBe(1);
    expect(store.rows()[0].d.filename).toContain('ubuntu');
    store.query.set('nothing-matches-this');
    expect(store.rows().length).toBe(0);
  });

  it('search also matches a status or a category by its name, and the extension and site of a file', () => {
    store.query.set('paused');
    const paused = engine.downloads().filter((d) => d.state === 'paused').length;
    expect(paused).toBeGreaterThan(0);
    expect(store.rows().length).toBeGreaterThanOrEqual(paused);
    expect(store.rows().filter((r) => r.d.state === 'paused').length).toBe(paused);
    store.query.set('video');
    const videos = engine.downloads().filter((d) => d.category === 'video').length;
    expect(store.rows().filter((r) => r.d.category === 'video').length).toBe(videos);
    const ext = engine.downloads()[0].filename.split('.').pop() ?? '';
    store.query.set(`.${ext}`.toUpperCase());
    expect(store.rows().length).toBeGreaterThan(0);
    expect(store.rows().every((r) => r.d.filename.toLowerCase().includes(`.${ext}`) || r.d.url.toLowerCase().includes(`.${ext}`))).toBeTrue();
    const host = new URL(engine.downloads()[0].url).hostname;
    store.query.set(host);
    expect(store.rows().length).toBeGreaterThan(0);
    // short queries never match every status by its first letters
    store.query.set('pa');
    expect(store.rows().every((r) => r.d.filename.toLowerCase().includes('pa') || r.d.url.toLowerCase().includes('pa'))).toBeTrue();
  });

  it('sorts by any column and flips direction on a second click', () => {
    store.sortBy('size');
    expect(store.sortKey()).toBe('size');
    expect(store.sortDir()).toBe('desc');
    const sizes = store.rows().map((r) => r.d.size ?? -1);
    expect(sizes).toEqual([...sizes].sort((a, b) => b - a));
    store.sortBy('size');
    expect(store.sortDir()).toBe('asc');
    store.sortBy('name');
    expect(store.sortDir()).toBe('asc');
    const names = store.rows().map((r) => r.d.filename);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
  });

  it('selection: single, toggle, range and keyboard moves', () => {
    const ids = store.rows().map((r) => r.d.id);
    store.select(ids[2]);
    expect([...store.selected()]).toEqual([ids[2]]);
    store.toggle(ids[4]);
    expect(store.selected().size).toBe(2);
    store.toggle(ids[4]);
    expect(store.selected().size).toBe(1);
    store.selectRange(ids[5]);
    expect([...store.selected()].sort()).toEqual(ids.slice(2, 6).sort());
    store.clearSelection();
    expect(store.moveSelection(1)).toBe(ids[0]);
    expect(store.moveSelection(1)).toBe(ids[1]);
    expect(store.moveSelection(-1)).toBe(ids[0]);
    expect(store.moveSelection(-1)).toBe(ids[0]);
  });

  it('details follow the engine as it ticks and close when asked', () => {
    const running = engine.downloads().find((d) => d.state === 'downloading' && d.size !== null);
    expect(running).toBeDefined();
    if (!running) {
      return;
    }
    store.openDetails(running.id);
    const before = store.details()?.downloaded ?? 0;
    engine.tick(250);
    engine.tick(250);
    expect(store.details()?.downloaded ?? 0).toBeGreaterThan(before);
    expect(store.selected().has(running.id)).toBeTrue();
    store.closeDetails();
    expect(store.details()).toBeNull();
  });

  it('totals report active count and speed', () => {
    const t = store.totals();
    expect(t.active).toBe(engine.active().length);
    expect(t.speedBps).toBe(engine.totalSpeedBps());
    expect(t.history.length).toBe(60);
    expect(t.maxActive).toBe(engine.maxActive());
  });

  describe('queues', () => {
    beforeEach(async () => {
      await engine.moveToQueue([3, 1, 2], 5);
    });

    it('filters by queue and shows it in its manual order', () => {
      store.setQueue(5);
      expect(store.queue()).toBe(5);
      expect(store.view()).toBe('all');
      expect(store.manualOrder()).toBeTrue();
      expect(store.rows().map((r) => r.d.id)).toEqual([3, 1, 2]);
      expect(store.queueOrder(5)).toEqual([3, 1, 2]);
    });

    it('follows a reorder and leaves other queues out', async () => {
      store.setQueue(5);
      await engine.reorderQueue(5, [2, 3, 1]);
      expect(store.rows().map((r) => r.d.id)).toEqual([2, 3, 1]);
      store.setQueue(1);
      expect(store.rows().some((r) => [1, 2, 3].includes(r.d.id))).toBeFalse();
    });

    it('leaving the queue restores the newest-first sort; changing the view leaves the queue', () => {
      store.setQueue(5);
      store.setQueue(null);
      expect(store.manualOrder()).toBeFalse();
      expect(store.sortKey()).toBe('added');
      expect(store.sortDir()).toBe('desc');
      store.setQueue(5);
      store.setView('active');
      expect(store.queue()).toBeNull();
      expect(store.sortKey()).toBe('added');
    });

    it('sorting by a column turns the manual order off until the queue is chosen again', () => {
      store.setQueue(5);
      store.sortBy('size');
      expect(store.queue()).toBe(5);
      expect(store.manualOrder()).toBeFalse();
      store.setQueue(5);
      expect(store.manualOrder()).toBeTrue();
    });

    it('counts per queue: running, waiting and all', () => {
      const counts = store.queueCounts();
      const inQueue = engine.downloads().filter((d) => d.queueId === 5);
      expect(counts.get(5)?.total).toBe(3);
      expect(counts.get(5)?.active).toBe(inQueue.filter((d) => ['probing', 'downloading', 'verifying'].includes(d.state)).length);
      expect(counts.get(5)?.waiting).toBe(inQueue.filter((d) => d.state === 'queued').length);
      const main = counts.get(1);
      expect((main?.total ?? 0) + 3).toBe(engine.downloads().length);
    });
  });

  describe('keyboard navigation and selection by id', () => {
    const visibleIds = (): number[] => store.rows().map((r) => r.d.id);

    it('Home / End / PageDown / PageUp move the cursor and the selection follows', () => {
      const ids = visibleIds();
      expect(store.navigate('last', 10)?.id).toBe(ids[ids.length - 1]);
      expect([...store.selected()]).toEqual([ids[ids.length - 1]]);
      expect(store.navigate('first', 10)?.index).toBe(0);
      expect(store.navigate('pageDown', 5)?.index).toBe(4);
      expect(store.navigate('pageUp', 5)?.index).toBe(0);
      expect(store.cursor()).toBe(ids[0]);
    });

    it('Shift extends the selection from the anchor, Ctrl+A takes every visible row', () => {
      const ids = visibleIds();
      store.select(ids[3]);
      store.navigate('next', 10, true);
      store.navigate('next', 10, true);
      expect([...store.selected()].sort((a, b) => a - b)).toEqual([ids[3], ids[4], ids[5]].sort((a, b) => a - b));
      store.navigate('prev', 10, true);
      expect(store.selected().size).toBe(2);
      store.selectAll();
      expect(store.selected().size).toBe(ids.length);
      store.setView('completed');
      store.selectAll();
      expect(store.selected().size).toBe(store.rows().length);
    });

    it('keeps the cursor and selection by id when the list re-sorts or the engine ticks', () => {
      const running = engine.downloads().find((d) => d.state === 'downloading');
      expect(running).toBeDefined();
      store.select(running?.id ?? -1);
      store.sortBy('progress');
      engine.tick(250);
      engine.tick(250);
      expect(store.cursor()).toBe(running?.id ?? -1);
      expect(store.selectedRows().map((r) => r.d.id)).toEqual([running?.id ?? -1]);
    });

    it('starts from the top when neither the cursor row nor a selected row is in the list any more', () => {
      const ids = visibleIds();
      store.select(ids[5]);
      store.setView('completed');
      const completed = visibleIds();
      expect(completed).not.toContain(ids[5]);
      expect(store.navigate('next', 10)?.index).toBe(0);
    });

    it('answers null for an empty list', () => {
      store.query.set('no download is called this');
      expect(store.navigate('next', 10)).toBeNull();
      expect(store.navigate('last', 10)).toBeNull();
    });

    it('unticking a Ctrl-clicked row falls back to the first selected row as the anchor', () => {
      const ids = visibleIds();
      store.select(ids[1]);
      store.toggle(ids[6]);
      store.toggle(ids[6]);
      store.selectRange(ids[3]);
      expect([...store.selected()].sort((a, b) => a - b)).toEqual([ids[1], ids[2], ids[3]].sort((a, b) => a - b));
    });
  });

  describe('order and rows are reused between ticks', () => {
    it('unchanged downloads keep their row object; only the moving ones get new ones', () => {
      const before = new Map(store.allRows().map((r) => [r.d.id, r]));
      engine.tick(250);
      const after = store.allRows();
      const changed = after.filter((r) => before.get(r.d.id) !== r).length;
      const moving = engine.downloads().filter((d) => ['probing', 'downloading', 'verifying', 'queued'].includes(d.state)).length;
      expect(changed).toBeGreaterThan(0);
      expect(changed).toBeLessThanOrEqual(moving);
      expect(changed).toBeLessThan(after.length);
    });

    it('re-sorts correctly after a tick changes the sort key of some rows', () => {
      store.sortBy('progress');
      for (let i = 0; i < 6; i++) {
        engine.tick(250);
      }
      const progress = store.rows().map((r) => r.progress ?? -1);
      expect(progress).toEqual([...progress].sort((a, b) => b - a));
    });

    it('follows an add and a removal in the middle of the list', async () => {
      const id = await engine.add({ url: 'https://example.com/zzz-new.zip' });
      // newest first (a seed added in the same millisecond may tie with it)
      expect(store.rows().slice(0, 2).map((r) => r.d.id)).toContain(id);
      await engine.remove(3);
      expect(store.rows().some((r) => r.d.id === 3)).toBeFalse();
      expect(store.rows().length).toBe(engine.downloads().length);
      const added = store.rows().map((r) => r.d.addedAt);
      expect(added).toEqual([...added].sort((a, b) => b - a));
    });

    it('sorts Arabic and Latin names with one shared collator', () => {
      store.sortBy('name');
      const names = store.rows().map((r) => r.d.filename);
      const collator = new Intl.Collator();
      expect(names).toEqual([...names].sort((a, b) => collator.compare(a, b)));
    });
  });
});
