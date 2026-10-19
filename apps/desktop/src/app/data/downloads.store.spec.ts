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
});
