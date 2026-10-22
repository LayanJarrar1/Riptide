import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FRAME_SCHEDULER, FrameScheduler, REDUCED_MOTION_QUERY } from '../../core/motion.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { COLUMN_WIDTHS_KEY, HIDDEN_COLUMNS_KEY } from '../../data/column-widths';
import { DownloadsStore } from '../../data/downloads.store';
import { provideDownloadEngine } from '../../data/engine.provider';
import { MOCK_ENGINE_AUTOSTART, MOCK_LIST_SIZE } from '../../data/mock-engine.service';
import { UiState } from '../shell/ui-state.service';
import { DownloadsPageComponent } from './downloads-page.component';

const NO_FRAMES: FrameScheduler = { request: () => 0, cancel: () => undefined };
const ROW_PX = 46;
/** Rows rendered beyond the viewport on each side (OVERSCAN in the page) plus one partial row. */
const SLACK = 6 + 6 + 1;

/**
 * The list at real scale and the layout around it: the DOM stays bounded by the viewport at
 * every list length, the details drawer docks or overlays by the page's width, and columns can
 * be resized, persisted and reset.
 */
describe('DownloadsPageComponent layout and scale', () => {
  let fixture: ComponentFixture<DownloadsPageComponent>;
  let store: DownloadsStore;
  let prefs: MemoryPreferenceStore;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const viewport = (): HTMLElement => el().querySelector<HTMLElement>('.viewport') as HTMLElement;
  const header = (): HTMLElement => el().querySelector<HTMLElement>('.header') as HTMLElement;
  const rows = (): HTMLElement[] => Array.from(el().querySelectorAll<HTMLElement>('app-download-row'));
  const cell = (key: string): HTMLElement => el().querySelector<HTMLElement>(`.hc-${key}`) as HTMLElement;
  const grip = (key: string): HTMLElement => cell(key).querySelector<HTMLElement>('.col-resize') as HTMLElement;
  const frame = (): Promise<void> => new Promise((r) => setTimeout(r, 30));

  async function settle(): Promise<void> {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  }

  async function scrollTo(top: number): Promise<void> {
    viewport().scrollTop = top;
    viewport().dispatchEvent(new Event('scroll'));
    await frame();
    await settle();
  }

  async function setup(count: number, width: number, saved: Record<string, string> = {}): Promise<void> {
    prefs = new MemoryPreferenceStore(saved);
    await TestBed.configureTestingModule({
      imports: [DownloadsPageComponent],
      providers: [
        provideDownloadEngine(),
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: MOCK_LIST_SIZE, useValue: { count, active: Math.min(count, 30), tickMs: 100 } },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: prefs },
        { provide: FRAME_SCHEDULER, useValue: NO_FRAMES },
        { provide: REDUCED_MOTION_QUERY, useValue: { matches: true, addEventListener: (): void => undefined } },
      ],
    }).compileComponents();
    store = TestBed.inject(DownloadsStore);
    TestBed.inject(UiState).markBooted();
    fixture = TestBed.createComponent(DownloadsPageComponent);
    fixture.nativeElement.style.height = '560px';
    fixture.nativeElement.style.width = `${width}px`;
    document.body.appendChild(fixture.nativeElement);
    await settle();
    await frame();
    await settle();
  }

  afterEach(() => {
    fixture?.nativeElement.remove();
  });

  for (const count of [100, 1_000, 10_000, 50_000]) {
    it(`keeps the DOM bounded by the viewport with ${count.toLocaleString('en')} downloads, at the top, middle and end`, async () => {
      await setup(count, 1100);
      expect(store.rows().length).toBe(count);
      const bound = Math.ceil(viewport().clientHeight / ROW_PX) + SLACK;
      expect(viewport().scrollHeight).toBeGreaterThan(count * ROW_PX - 1);
      expect(rows().length).toBeLessThanOrEqual(bound);

      for (const top of [Math.floor(count / 2) * ROW_PX, viewport().scrollHeight, 0, Math.floor(count / 3) * ROW_PX]) {
        await scrollTo(top);
        expect(rows().length).toBeLessThanOrEqual(bound);
        expect(rows().length).toBeGreaterThan(0);
      }
      // the row rendered at an index is the one the store has there
      const first = rows()[0];
      const index = Number(first.getAttribute('aria-rowindex')) - 2;
      expect(first.querySelector('.name')?.textContent?.trim()).toBe(store.rows()[index].d.filename);

      // searching and sorting keep the bound
      store.sortBy('size');
      store.query.set('0');
      await settle();
      expect(rows().length).toBeLessThanOrEqual(bound);
      store.query.set('');
      await settle();
      expect(rows().length).toBeLessThanOrEqual(bound);
    });
  }

  describe('details drawer', () => {
    it('docks beside the list on a wide page and keeps the list usable', async () => {
      await setup(1_000, 1400);
      await scrollTo(400 * ROW_PX);
      const before = viewport().scrollTop;
      const id = store.rows()[405].d.id;
      store.select(id);
      store.openDetails(id);
      await settle();
      await frame();
      await settle();
      expect(el().classList).toContain('has-details');
      expect(el().querySelector('.scrim')).toBeNull();
      expect(el().querySelector('app-download-details.is-overlay')).toBeNull();
      expect(viewport().scrollTop).toBe(before);
      expect(store.selected().has(id)).toBeTrue();
    });

    it('overlays the list on a narrow page: the table keeps its width, and the scrim closes it', async () => {
      await setup(1_000, 860);
      const widthBefore = viewport().clientWidth;
      await scrollTo(200 * ROW_PX);
      const before = viewport().scrollTop;
      const id = store.rows()[203].d.id;
      store.select(id);
      store.openDetails(id);
      await settle();
      await frame();
      await settle();
      expect(el().classList).not.toContain('has-details');
      expect(el().classList).toContain('details-overlay');
      expect(el().querySelector('app-download-details.is-overlay')).not.toBeNull();
      expect(viewport().clientWidth).toBe(widthBefore);
      expect(viewport().scrollTop).toBe(before);

      el().querySelector<HTMLElement>('.scrim')?.click();
      await settle();
      expect(store.detailsId()).toBeNull();
      expect(el().querySelector('.scrim')).toBeNull();
      expect(store.selected().has(id)).toBeTrue();
      expect(viewport().scrollTop).toBe(before);
    });

    it('survives being opened and closed repeatedly with rapid selection changes', async () => {
      await setup(5_000, 1400);
      const bound = Math.ceil(viewport().clientHeight / ROW_PX) + SLACK;
      for (let i = 0; i < 12; i++) {
        const id = store.rows()[i * 7].d.id;
        store.select(id);
        store.openDetails(id);
        await settle();
        if (i % 3 === 2) {
          store.closeDetails();
          await settle();
        }
        expect(rows().length).toBeLessThanOrEqual(bound);
      }
      store.closeDetails();
      await settle();
      expect(el().querySelector('app-download-details')).toBeNull();
    });
  });

  describe('column widths', () => {
    it('widens a column from the keyboard, saves it and restores it on double-click', async () => {
      await setup(200, 1400);
      const start = cell('name').getBoundingClientRect().width;
      grip('name').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true }));
      await settle();
      const widened = cell('name').getBoundingClientRect().width;
      expect(Math.round(widened)).toBe(Math.round(start + 64));
      expect(JSON.parse(prefs.get(COLUMN_WIDTHS_KEY) ?? '{}').name).toBe(Math.round(start + 64));
      // the rows follow the header
      const nameCell = rows()[0].querySelector<HTMLElement>('.c-name') as HTMLElement;
      expect(Math.round(nameCell.getBoundingClientRect().width)).toBe(Math.round(widened));
      // the list keyboard does not react to the grip's keys
      expect(store.cursor()).toBeNull();

      grip('name').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
      await settle();
      expect(Math.round(cell('name').getBoundingClientRect().width)).toBe(Math.round(start));
      expect(prefs.get(COLUMN_WIDTHS_KEY)).toBe('{}');
    });

    it('resizes by dragging the grip', async () => {
      await setup(200, 1400);
      const g = grip('status');
      const start = cell('status').getBoundingClientRect().width;
      const x = g.getBoundingClientRect().left + 5;
      g.dispatchEvent(new PointerEvent('pointerdown', { button: 0, clientX: x, pointerId: 1, bubbles: true }));
      g.dispatchEvent(new PointerEvent('pointermove', { clientX: x + 80, pointerId: 1, bubbles: true }));
      g.dispatchEvent(new PointerEvent('pointerup', { clientX: x + 80, pointerId: 1, bubbles: true }));
      await settle();
      expect(Math.round(cell('status').getBoundingClientRect().width)).toBe(Math.round(start + 80));
      expect(JSON.parse(prefs.get(COLUMN_WIDTHS_KEY) ?? '{}').status).toBe(Math.round(start + 80));
      // the drag did not sort by status
      expect(store.sortKey()).not.toBe('status');
    });

    it('applies saved widths on start, scrolls sideways when they overflow, and the header follows', async () => {
      await setup(200, 1100, { [COLUMN_WIDTHS_KEY]: JSON.stringify({ name: 900 }) });
      expect(Math.round(cell('name').getBoundingClientRect().width)).toBe(900);
      expect(viewport().scrollWidth).toBeGreaterThan(viewport().clientWidth);
      viewport().scrollLeft = 150;
      viewport().dispatchEvent(new Event('scroll'));
      await settle();
      expect(header().scrollLeft).toBe(viewport().scrollLeft);
    });

    it('keeps the widths while the details drawer is open, and the header menu resets them all', async () => {
      await setup(200, 1400, { [COLUMN_WIDTHS_KEY]: JSON.stringify({ name: 320, size: 90 }) });
      store.openDetails(store.rows()[0].d.id);
      await settle();
      await frame();
      await settle();
      expect(Math.round(cell('name').getBoundingClientRect().width)).toBe(320);
      header().dispatchEvent(new MouseEvent('contextmenu', { clientX: 100, clientY: 100, bubbles: true, cancelable: true }));
      await settle();
      const reset = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((b) => /Reset column widths|column\.resetWidths/.test(b.textContent ?? ''));
      expect(reset).toBeDefined();
      reset?.click();
      await settle();
      expect(prefs.get(COLUMN_WIDTHS_KEY)).toBe('{}');
      expect(Math.round(cell('name').getBoundingClientRect().width)).not.toBe(320);
    });

    it('turns the optional columns on from the header menu, shows them in every row and remembers it', async () => {
      await setup(200, 1600);
      expect(el().querySelector('.hc-duration')).toBeNull();
      expect(rows()[0].querySelector('.c-duration')).toBeNull();
      const menuItem = (pattern: RegExp): HTMLElement | undefined =>
        Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((b) => pattern.test(b.textContent ?? ''));
      for (const pattern of [/Duration|column\.duration/, /Avg\. speed|column\.avgSpeed/]) {
        header().dispatchEvent(new MouseEvent('contextmenu', { clientX: 100, clientY: 100, bubbles: true, cancelable: true }));
        await settle();
        menuItem(pattern)?.click();
        await settle();
      }
      expect(el().querySelector('.hc-duration')).not.toBeNull();
      expect(el().querySelector('.hc-avgSpeed')).not.toBeNull();
      expect(rows().every((r) => r.querySelector('.c-duration') !== null && r.querySelector('.c-avgSpeed') !== null)).toBeTrue();
      expect(JSON.parse(prefs.get(HIDDEN_COLUMNS_KEY) ?? '[]')).toEqual(['category']);
      // the cells line up with their headers
      const h = cell('duration').getBoundingClientRect();
      const c = (rows()[0].querySelector('.c-duration') as HTMLElement).getBoundingClientRect();
      expect(Math.round(c.left)).toBe(Math.round(h.left));
      // a sort on it works, and every row with a recorded time shows one
      store.sortBy('duration');
      await settle();
      const recorded = rows().filter((r) => r.querySelector('.c-duration')?.textContent?.trim() !== '—');
      expect(recorded.length).toBeGreaterThan(0);
    });

    it('hides a column the user turned off, in the header and in the rows', async () => {
      await setup(100, 1400, { [HIDDEN_COLUMNS_KEY]: JSON.stringify(['avgSpeed', 'duration', 'category', 'size']) });
      expect(el().querySelector('.hc-size')).toBeNull();
      expect(rows()[0].querySelector('.c-size')).toBeNull();
      const h = cell('progress').getBoundingClientRect();
      const c = (rows()[0].querySelector('.c-progress') as HTMLElement).getBoundingClientRect();
      expect(Math.round(c.left)).toBe(Math.round(h.left));
    });

    it('ignores corrupt saved widths', async () => {
      await setup(50, 1400, { [COLUMN_WIDTHS_KEY]: '{oops' });
      expect(rows().length).toBeGreaterThan(0);
      expect(cell('name').getBoundingClientRect().width).toBeGreaterThan(150);
    });
  });
});
