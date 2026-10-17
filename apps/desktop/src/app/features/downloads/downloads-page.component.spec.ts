import { Injectable, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FRAME_SCHEDULER, FrameScheduler, REDUCED_MOTION_QUERY } from '../../core/motion.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
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
    expect(header('added')?.classList).toContain('is-sorted');
    expect(header('added')?.getAttribute('aria-sort')).toBe('descending');
    expect(header('added')?.title).toBe('Sorted descending — click to flip');
    expect(header('added')?.querySelector('.sort-icon')?.classList).not.toContain('is-asc');
    expect(header('size')?.classList).not.toContain('is-sorted');
    expect(header('size')?.title).toBe('Sort by Size');

    header('added')?.click();
    fixture.detectChanges();
    expect(header('added')?.getAttribute('aria-sort')).toBe('ascending');
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
});
