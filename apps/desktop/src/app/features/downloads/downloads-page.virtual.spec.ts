import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FRAME_SCHEDULER, FrameScheduler, REDUCED_MOTION_QUERY } from '../../core/motion.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadsStore } from '../../data/downloads.store';
import { provideDownloadEngine } from '../../data/engine.provider';
import { MOCK_ENGINE_AUTOSTART, MOCK_LIST_SIZE, MockEngineService } from '../../data/mock-engine.service';
import { UiState } from '../shell/ui-state.service';
import { DownloadsPageComponent } from './downloads-page.component';

const NO_FRAMES: FrameScheduler = { request: () => 0, cancel: () => undefined };
const COUNT = 3000;
const VIEWPORT_PX = 460;
const ROW_PX = 46;

/** The list with 3 000 downloads in a window 460 px tall: the real layout, the real scroller. */
describe('DownloadsPageComponent virtual list', () => {
  let fixture: ComponentFixture<DownloadsPageComponent>;
  let store: DownloadsStore;
  let engine: MockEngineService;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const viewport = (): HTMLElement => el().querySelector<HTMLElement>('.viewport') as HTMLElement;
  const grid = (): HTMLElement => el().querySelector<HTMLElement>('.grid') as HTMLElement;
  const rows = (): HTMLElement[] => Array.from(el().querySelectorAll<HTMLElement>('app-download-row'));

  async function settle(): Promise<void> {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  }

  /** Scrolls like the user does: set the offset, let the scroll event reach the page. */
  async function scrollTo(top: number): Promise<void> {
    viewport().scrollTop = top;
    viewport().dispatchEvent(new Event('scroll'));
    await new Promise((r) => setTimeout(r, 0));
    await settle();
  }

  function press(key: string, init: KeyboardEventInit = {}): void {
    grid().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [DownloadsPageComponent],
      providers: [
        provideDownloadEngine(),
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: MOCK_LIST_SIZE, useValue: { count: COUNT, active: 30, tickMs: 100 } },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: FRAME_SCHEDULER, useValue: NO_FRAMES },
        { provide: REDUCED_MOTION_QUERY, useValue: { matches: true, addEventListener: (): void => undefined } },
      ],
    }).compileComponents();
    store = TestBed.inject(DownloadsStore);
    engine = TestBed.inject(MockEngineService);
    TestBed.inject(UiState).markBooted();
    fixture = TestBed.createComponent(DownloadsPageComponent);
    fixture.nativeElement.style.height = '560px';
    fixture.nativeElement.style.width = '1100px';
    document.body.appendChild(fixture.nativeElement);
    await settle();
    // the toolbar and header take their share; the viewport is what is left
    await new Promise((r) => setTimeout(r, 0));
    await settle();
  });

  afterEach(() => {
    fixture.nativeElement.remove();
  });

  it('renders only the rows in view plus a small overscan, whatever the length of the list', () => {
    expect(store.rows().length).toBe(COUNT);
    const inView = Math.ceil(viewport().clientHeight / ROW_PX);
    expect(viewport().clientHeight).toBeGreaterThan(VIEWPORT_PX - 100);
    expect(rows().length).toBeGreaterThanOrEqual(inView);
    expect(rows().length).toBeLessThanOrEqual(inView + 6 + 6 + 1);
    // the spacers stand in for the rest: the scroll height is the whole list
    expect(viewport().scrollHeight).toBeGreaterThan(COUNT * ROW_PX - 1);
  });

  it('keeps the DOM small while scrolling to the middle and the end, and shows the right rows there', async () => {
    await scrollTo(1500 * ROW_PX);
    expect(rows().length).toBeLessThanOrEqual(30);
    const first = Number(rows()[0].getAttribute('aria-rowindex'));
    expect(first).toBeGreaterThan(1490);
    expect(first).toBeLessThan(1510);
    // the row at that index is the one the store has there
    const index = first - 2;
    expect(rows()[0].querySelector('.name')?.textContent?.trim()).toBe(store.rows()[index].d.filename);

    await scrollTo(viewport().scrollHeight);
    expect(rows().length).toBeLessThanOrEqual(30);
    expect(rows()[rows().length - 1].querySelector('.name')?.textContent?.trim()).toBe(store.rows()[COUNT - 1].d.filename);
  });

  it('numbers rows for assistive technology: the header is row 1 and the grid knows the real length', async () => {
    expect(grid().getAttribute('aria-rowcount')).toBe(String(COUNT + 1));
    expect(rows()[0].getAttribute('aria-rowindex')).toBe('2');
    await scrollTo(100 * ROW_PX);
    expect(Number(rows()[0].getAttribute('aria-rowindex'))).toBeGreaterThan(90);
  });

  it('an engine tick updates the rows in place: the same elements, new numbers', async () => {
    const before = rows();
    const names = before.map((r) => r.querySelector('.name')?.textContent);
    engine.tick(100);
    await settle();
    const after = rows();
    expect(after.length).toBe(before.length);
    expect(after.every((r, i) => r === before[i])).toBeTrue();
    expect(after.map((r) => r.querySelector('.name')?.textContent)).toEqual(names);
  });

  it('only the rows whose download changed get a new row object', async () => {
    const seen = new Map(store.allRows().map((r) => [r.d.id, r]));
    engine.tick(100);
    const changed = store.allRows().filter((r) => seen.get(r.d.id) !== r).length;
    expect(changed).toBe(30);
    expect(store.allRows().length).toBe(COUNT);
  });

  describe('keyboard', () => {
    it('End goes to the last row, scrolls it into view and Home comes back', async () => {
      press('End');
      await settle();
      const last = store.rows()[COUNT - 1].d.id;
      expect(store.cursor()).toBe(last);
      expect(viewport().scrollTop).toBeGreaterThan((COUNT - 15) * ROW_PX);
      const active = el().querySelector<HTMLElement>('app-download-row.is-active');
      expect(active?.id).toBe(`dl-row-${last}`);
      expect(grid().getAttribute('aria-activedescendant')).toBe(`dl-row-${last}`);

      press('Home');
      await settle();
      expect(store.cursor()).toBe(store.rows()[0].d.id);
      expect(viewport().scrollTop).toBe(0);
    });

    it('PageDown / PageUp move a page less a row and keep the cursor row on screen', async () => {
      press('ArrowDown');
      press('PageDown');
      await settle();
      const page = Math.floor(viewport().clientHeight / ROW_PX);
      const index = store.rows().findIndex((r) => r.d.id === store.cursor());
      expect(index).toBe(page - 1);
      press('PageDown');
      press('PageDown');
      await settle();
      const cursorRow = el().querySelector<HTMLElement>('app-download-row.is-active');
      expect(cursorRow).not.toBeNull();
      const v = viewport().getBoundingClientRect();
      const r = (cursorRow as HTMLElement).getBoundingClientRect();
      expect(r.top).toBeGreaterThanOrEqual(v.top - 1);
      expect(r.bottom).toBeLessThanOrEqual(v.bottom + 1);
      press('PageUp');
      await settle();
      expect(store.rows().findIndex((x) => x.d.id === store.cursor())).toBe((page - 1) * 3 - (page - 1));
    });

    it('arrow keys move one row at a time and keep scrolling as the cursor leaves the viewport', async () => {
      for (let i = 0; i < 30; i++) {
        press('ArrowDown');
      }
      await settle();
      expect(store.rows().findIndex((x) => x.d.id === store.cursor())).toBe(29);
      expect(viewport().scrollTop).toBeGreaterThan(0);
      expect(el().querySelector('app-download-row.is-active')).not.toBeNull();
      for (let i = 0; i < 30; i++) {
        press('ArrowUp');
      }
      await settle();
      expect(store.rows().findIndex((x) => x.d.id === store.cursor())).toBe(0);
      expect(viewport().scrollTop).toBe(0);
    });

    it('Ctrl+A selects every download, not only the rendered ones; Escape clears', async () => {
      press('a', { ctrlKey: true });
      await settle();
      expect(store.selected().size).toBe(COUNT);
      expect(rows().every((r) => r.getAttribute('aria-selected') === 'true')).toBeTrue();
      expect(el().querySelector('.bulk-count')?.textContent).toContain('3,000');
      press('Escape');
      await settle();
      expect(store.selected().size).toBe(0);
    });

    it('a selection is kept by id: it survives the row leaving the DOM and coming back', async () => {
      const target = store.rows()[3].d.id;
      store.select(target);
      await settle();
      expect(el().querySelector(`#dl-row-${target}`)?.getAttribute('aria-selected')).toBe('true');
      await scrollTo(2000 * ROW_PX);
      expect(el().querySelector(`#dl-row-${target}`)).toBeNull();
      expect(store.selected().has(target)).toBeTrue();
      await scrollTo(0);
      expect(el().querySelector(`#dl-row-${target}`)?.getAttribute('aria-selected')).toBe('true');
    });

    it('Shift + End selects from the cursor to the last row', async () => {
      store.select(store.rows()[COUNT - 5].d.id);
      press('End', { shiftKey: true });
      await settle();
      expect(store.selected().size).toBe(5);
    });

    it('does not take the keys of the header buttons or the row actions', async () => {
      const header = el().querySelector<HTMLButtonElement>('.th-name') as HTMLButtonElement;
      header.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }));
      await settle();
      expect(store.cursor()).toBeNull();
    });
  });

  describe('sorting and filtering a long list', () => {
    it('sorts 3 000 rows by name, status and size and filters by text without re-creating the page', async () => {
      const t0 = performance.now();
      store.sortBy('name');
      expect(store.rows()[0].d.filename.localeCompare(store.rows()[1].d.filename)).toBeLessThanOrEqual(0);
      store.sortBy('size');
      store.sortBy('status');
      store.query.set('00042');
      expect(store.rows().length).toBeGreaterThan(0);
      expect(store.rows().length).toBeLessThan(5);
      await settle();
      expect(rows().length).toBe(store.rows().length);
      store.query.set('');
      await settle();
      expect(performance.now() - t0).toBeLessThan(2000);
      expect(rows().length).toBeLessThanOrEqual(30);
    });
  });
});
