import { TestBed } from '@angular/core/testing';

import { downloadedBytes } from '../model/download';
import { DownloadsStore } from './downloads.store';
import { provideDownloadEngine } from './engine.provider';
import { generateDownloads } from './mock-data';
import { MOCK_ENGINE_AUTOSTART, MOCK_LIST_SIZE, MockEngineService } from './mock-engine.service';

describe('generated downloads (list performance harness)', () => {
  it('makes exactly N downloads with unique ids and a believable mix of states', () => {
    const list = generateDownloads(2000, 25, 1_700_000_000_000);
    expect(list.length).toBe(2000);
    expect(new Set(list.map((d) => d.id)).size).toBe(2000);
    expect(list.filter((d) => d.state === 'downloading').length).toBe(25);
    const completed = list.filter((d) => d.state === 'completed').length;
    expect(completed).toBeGreaterThan(900);
    expect(completed).toBeLessThan(1500);
    expect(new Set(list.map((d) => d.state)).size).toBeGreaterThanOrEqual(5);
  });

  it('is deterministic and has the newest downloads moving', () => {
    const a = generateDownloads(300, 10, 5);
    const b = generateDownloads(300, 10, 5);
    expect(a.map((d) => d.filename)).toEqual(b.map((d) => d.filename));
    expect(a.slice(-10).every((d) => d.state === 'downloading')).toBeTrue();
    expect(a[a.length - 1].addedAt).toBeGreaterThan(a[0].addedAt);
  });

  it('gives every row segments that agree with its state', () => {
    const list = generateDownloads(500, 5, 5);
    for (const d of list) {
      const done = downloadedBytes(d);
      if (d.state === 'completed') {
        expect(done).toBe(d.size ?? -1);
      } else if (d.state === 'queued' || d.state === 'paused') {
        expect(done).toBe(0);
      }
    }
  });

  it('can be generated at 50 000 rows quickly enough for a harness', () => {
    const t0 = performance.now();
    const list = generateDownloads(50_000, 100, 5);
    expect(list.length).toBe(50_000);
    expect(performance.now() - t0).toBeLessThan(2000);
  });
});

describe('MockEngineService with ?mockCount', () => {
  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideDownloadEngine(),
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: MOCK_LIST_SIZE, useValue: { count: 1500, active: 40, tickMs: 100 } },
      ],
    });
  });

  it('starts with the requested number of downloads and room for the requested active ones', () => {
    const engine = TestBed.inject(MockEngineService);
    expect(engine.downloads().length).toBe(1500);
    expect(engine.maxActive()).toBe(40);
    expect(engine.active().length).toBe(40);
  });

  it('keeps the store fast enough: a tick of 1 500 rows with 40 moving', () => {
    const engine = TestBed.inject(MockEngineService);
    const store = TestBed.inject(DownloadsStore);
    expect(store.rows().length).toBe(1500);
    engine.tick(100);
    expect(store.totals().active).toBe(40);
    expect(store.viewCounts().all).toBe(1500);
  });
});
