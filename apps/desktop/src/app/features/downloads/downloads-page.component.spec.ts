import { Injectable, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FRAME_SCHEDULER, FrameScheduler, REDUCED_MOTION_QUERY } from '../../core/motion.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { QueuesService } from '../../data/queues.service';
import { UiState } from '../shell/ui-state.service';
import { DownloadsPageComponent } from './downloads-page.component';

/** The simulation, but with a list that arrives when the test says so (like the Tauri engine). */
@Injectable()
class LateMockEngine extends MockEngineService {
  override readonly loaded = signal(false);
}

/** No frames ever run: tickers write their final value at once. */
const NO_FRAMES: FrameScheduler = { request: () => 0, cancel: () => undefined };

describe('DownloadsPageComponent', () => {
  let fixture: ComponentFixture<DownloadsPageComponent>;
  let engine: LateMockEngine;
  let store: DownloadsStore;
  let ui: UiState;

  function el(): HTMLElement {
    return fixture.nativeElement as HTMLElement;
  }

  function rows(): HTMLElement[] {
    return Array.from(el().querySelectorAll<HTMLElement>('app-download-row'));
  }

  beforeEach(async () => {
    jasmine.clock().install();
    await TestBed.configureTestingModule({
      imports: [DownloadsPageComponent],
      providers: [
        { provide: DownloadEngine, useClass: LateMockEngine },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: FRAME_SCHEDULER, useValue: NO_FRAMES },
        { provide: REDUCED_MOTION_QUERY, useValue: { matches: false, addEventListener: (): void => undefined } },
      ],
    }).compileComponents();
    engine = TestBed.inject(DownloadEngine) as LateMockEngine;
    store = TestBed.inject(DownloadsStore);
    ui = TestBed.inject(UiState);
    ui.booting.set(true);
    fixture = TestBed.createComponent(DownloadsPageComponent);
    fixture.detectChanges();
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  it('shows six skeleton rows, never the empty state, until the engine has its list', () => {
    expect(el().querySelectorAll('.sk-row').length).toBe(6);
    expect(el().querySelector('app-empty-state')).toBeNull();
    expect(rows().length).toBe(0);
    expect(el().querySelector('.skeleton')?.getAttribute('aria-label')).toBe('Loading your downloads…');

    engine.loaded.set(true);
    fixture.detectChanges();
    expect(el().querySelector('.skeleton')).toBeNull();
    expect(rows().length).toBeGreaterThan(5);
  });

  it('cascades the first rows on boot only, and never treats the first list as new', () => {
    engine.loaded.set(true);
    fixture.detectChanges();
    const first = rows()[0];
    expect(first.classList).toContain('is-staggered');
    expect(first.style.getPropertyValue('--i')).toBe('0');
    expect(rows()[1].style.getPropertyValue('--i')).toBe('1');
    expect(rows().some((r) => r.classList.contains('is-entering'))).toBeFalse();

    ui.markBooted();
    fixture.detectChanges();
    expect(rows().some((r) => r.classList.contains('is-staggered'))).toBeFalse();

    // a download added now is new: it slides in, and stops being "fresh" shortly after
    void engine.add({ url: 'https://example.com/new-file.zip' });
    fixture.detectChanges();
    const added = rows().find((r) => r.textContent?.includes('new-file.zip'));
    expect(added?.classList).toContain('is-entering');
    jasmine.clock().tick(700);
    fixture.detectChanges();
    expect(added?.classList).not.toContain('is-entering');
  });

  it('explains a failed row under its name and offers the fix inline', () => {
    engine.loaded.set(true);
    fixture.detectChanges();
    const failed = rows().filter((r) => r.dataset['state'] === 'failed');
    expect(failed.length).toBeGreaterThan(0);
    const expired = failed.find((r) => r.querySelector('.reason-text')?.textContent?.trim() === 'Link expired');
    const checksum = failed.find((r) => r.querySelector('.reason-text')?.textContent?.trim() === 'Checksum mismatch');
    expect(expired?.querySelector('.reason-action')?.textContent?.trim()).toBe('Get a new link');
    expect(checksum?.querySelector('.reason-action')?.textContent?.trim()).toBe('Retry');
    expect(failed.every((r) => r.querySelector('.host') === null)).toBeTrue();

    const id = engine.downloads().find((d) => d.failure === 'checksum')?.id ?? -1;
    checksum?.querySelector<HTMLButtonElement>('.reason-action')?.click();
    fixture.detectChanges();
    expect(engine.get(id)?.state).toBe('queued');
    expect(checksum?.classList).not.toContain('is-failed');
    expect(checksum?.querySelector('.host')).not.toBeNull();
  });

  it('shows the sort direction on the active column and flips it on a second click', () => {
    engine.loaded.set(true);
    fixture.detectChanges();
    const header = (key: string): HTMLButtonElement | null => el().querySelector<HTMLButtonElement>(`.th-${key}`);
    // aria-sort sits on the columnheader cell that holds the sort button and the resize grip
    const columnHeader = (key: string): HTMLElement | null => el().querySelector<HTMLElement>(`.hc-${key}[role="columnheader"]`);
    expect(header('added')?.classList).toContain('is-sorted');
    expect(columnHeader('added')?.getAttribute('aria-sort')).toBe('descending');
    expect(header('added')?.title).toBe('Sorted descending — click to flip');
    expect(header('added')?.querySelector('.sort-icon')?.classList).not.toContain('is-asc');
    expect(header('size')?.classList).not.toContain('is-sorted');
    expect(header('size')?.title).toBe('Sort by Size');

    header('added')?.click();
    fixture.detectChanges();
    expect(columnHeader('added')?.getAttribute('aria-sort')).toBe('ascending');
    expect(header('added')?.querySelector('.sort-icon')?.classList).toContain('is-asc');
  });

  it('removing rows fades them out first, then asks the engine to drop them', () => {
    engine.loaded.set(true);
    fixture.detectChanges();
    const before = engine.downloads().length;
    const victim = store.rows()[0];
    store.select(victim.d.id);
    fixture.detectChanges();
    el().dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }));
    fixture.detectChanges();
    el().querySelector<HTMLButtonElement>('app-dialog .v-danger')?.click();
    fixture.detectChanges();
    const leaving = rows().find((r) => r.classList.contains('is-leaving'));
    expect(leaving?.querySelector('.name')?.textContent?.trim()).toBe(victim.d.filename);
    expect(engine.downloads().length).toBe(before);
    jasmine.clock().tick(250);
    fixture.detectChanges();
    expect(engine.downloads().length).toBe(before - 1);
    expect(engine.get(victim.d.id)).toBeUndefined();
  });

  describe('queues', () => {
    let queues: QueuesService;
    const order = (): number[] => store.rows().map((r) => r.d.id);
    const key = (name: string, altKey = false): void => {
      el().dispatchEvent(new KeyboardEvent('keydown', { key: name, altKey, bubbles: true, cancelable: true }));
      fixture.detectChanges();
    };
    const menuLabels = (): string[] => Array.from(el().querySelectorAll('app-menu .item .label')).map((n) => n.textContent?.trim() ?? '');
    const chooseMenu = (label: string): void => {
      Array.from(el().querySelectorAll<HTMLButtonElement>('app-menu .item'))
        .find((b) => b.querySelector('.label')?.textContent?.trim() === label)
        ?.click();
      fixture.detectChanges();
    };
    function contextMenuOn(row: HTMLElement): void {
      row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 40 }));
      fixture.detectChanges();
    }

    beforeEach(async () => {
      queues = TestBed.inject(QueuesService);
      await queues.ready;
      await queues.create({ name: 'Night', maxConcurrent: 2, schedule: { window: null, days: 127, start_at: null }, onDone: null });
      engine.loaded.set(true);
      await engine.moveToQueue([3, 1, 2, 4], 2);
      store.setQueue(2);
      fixture.detectChanges();
    });

    it('shows one queue in its manual order, titled with its name, with start / stop in the toolbar', async () => {
      expect(el().querySelector('.title')?.textContent?.trim()).toBe('Night');
      expect(order()).toEqual([3, 1, 2, 4]);
      expect(rows().length).toBe(4);
      expect(el().querySelector('.queue-state')?.textContent?.trim()).toBe('Running');
      expect(el().querySelector('.queue-tools')?.textContent).not.toContain('Queue order');

      el().querySelector<HTMLButtonElement>('.queue-tools button')?.click();
      await fixture.whenStable();
      fixture.detectChanges();
      expect(queues.get(2)?.enabled).toBeFalse();
      expect(el().querySelector('.queue-state')?.textContent?.trim()).toBe('Stopped');
    });

    it('offers the way back to the queue order after sorting by a column', () => {
      store.sortBy('size');
      fixture.detectChanges();
      const chip = Array.from(el().querySelectorAll<HTMLButtonElement>('.queue-tools button')).find((b) => b.textContent?.includes('Queue order'));
      expect(chip).toBeDefined();
      chip?.click();
      fixture.detectChanges();
      expect(store.manualOrder()).toBeTrue();
      expect(order()).toEqual([3, 1, 2, 4]);
    });

    it('Alt + arrow moves the selected download one place in its queue', () => {
      store.select(3);
      key('ArrowDown', true);
      expect(order()).toEqual([1, 3, 2, 4]);
      key('ArrowDown', true);
      expect(order()).toEqual([1, 2, 3, 4]);
      key('ArrowUp', true);
      expect(order()).toEqual([1, 3, 2, 4]);
      store.select(1);
      key('ArrowUp', true);
      expect(order()).toEqual([1, 3, 2, 4]);
    });

    it('a plain arrow still just moves the selection', () => {
      store.select(3);
      key('ArrowDown');
      expect(order()).toEqual([3, 1, 2, 4]);
      expect([...store.selected()]).toEqual([1]);
    });

    it('Alt + arrow does nothing outside a queue\'s manual order', () => {
      store.sortBy('size');
      const before = order();
      store.select(before[0]);
      key('ArrowDown', true);
      expect(order()).toEqual(before);
    });

    it('the row menu has Move up / down / to queue, and Move down works', () => {
      contextMenuOn(rows()[0]);
      expect(menuLabels()).toEqual(jasmine.arrayContaining(['Move up', 'Move down', 'Move to queue…']));
      const disabled = Array.from(el().querySelectorAll<HTMLButtonElement>('app-menu .item')).find((b) => b.textContent?.includes('Move up'));
      expect(disabled?.disabled).toBeTrue();
      chooseMenu('Move down');
      expect(order()).toEqual([1, 3, 2, 4]);
    });

    it('"Move to queue…" moves the selection to the chosen queue', async () => {
      contextMenuOn(rows()[1]);
      chooseMenu('Move to queue…');
      await fixture.whenStable();
      fixture.detectChanges();
      const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
      expect(dialog?.querySelector('h2')?.textContent?.trim()).toBe('Move to queue');
      const radios = Array.from(dialog?.querySelectorAll<HTMLInputElement>('input[type="radio"]') ?? []);
      expect(radios.map((r) => r.disabled)).toEqual([false, true]);
      radios[0].click();
      fixture.detectChanges();
      dialog?.querySelector<HTMLButtonElement>('.foot .v-primary')?.click();
      await fixture.whenStable();
      fixture.detectChanges();
      expect(engine.get(1)?.queueId).toBe(1);
      expect(order()).toEqual([3, 2, 4]);
    });

    it('Move to queue is not offered when there is only one queue', async () => {
      await queues.remove(2);
      store.setQueue(null);
      fixture.detectChanges();
      contextMenuOn(rows()[0]);
      expect(menuLabels()).not.toContain('Move to queue…');
    });

    describe('dragging rows', () => {
      function drag(from: number, to: number, lowerHalf: boolean): DataTransfer {
        const dt = new DataTransfer();
        const target = rows()[to];
        spyOn(target, 'getBoundingClientRect').and.returnValue({ top: 0, bottom: 40, height: 40, left: 0, right: 100, width: 100, x: 0, y: 0, toJSON: () => ({}) });
        rows()[from].dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
        target.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt, clientY: lowerHalf ? 35 : 5 }));
        fixture.detectChanges();
        return dt;
      }

      it('marks where the row would land and reorders on drop', () => {
        const dt = drag(0, 2, true);
        expect(rows()[2].classList).toContain('drop-after');
        rows()[2].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
        fixture.detectChanges();
        expect(order()).toEqual([1, 2, 3, 4]);
        expect(rows().some((r) => r.classList.contains('drop-after') || r.classList.contains('drop-before'))).toBeFalse();
      });

      it('dropping in the upper half puts the row before the target', () => {
        const dt = drag(3, 1, false);
        expect(rows()[1].classList).toContain('drop-before');
        rows()[1].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
        fixture.detectChanges();
        expect(order()).toEqual([3, 4, 1, 2]);
      });

      it('moves the whole selection together when a selected row is dragged', () => {
        store.select(3);
        store.toggle(2);
        fixture.detectChanges();
        const dt = drag(0, 3, true);
        rows()[3].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
        fixture.detectChanges();
        expect(order()).toEqual([1, 4, 3, 2]);
      });

      it('shows no drop mark outside a queue\'s manual order', () => {
        store.sortBy('size');
        fixture.detectChanges();
        const dt = new DataTransfer();
        rows()[0].dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
        rows()[1].dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt, clientY: 5 }));
        fixture.detectChanges();
        expect(rows().some((r) => r.classList.contains('drop-before') || r.classList.contains('drop-after'))).toBeFalse();
      });
    });
  });
});
