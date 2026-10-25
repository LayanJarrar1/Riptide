import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';

import { AppComponent } from './app.component';
import { routes } from './app.routes';
import {
  DARK_SCHEME_QUERY,
  MemoryPreferenceStore,
  NAVIGATOR_LANGUAGES,
  PREFERENCE_STORE,
} from './core/preferences';
import { NARROW_QUERY } from './core/nav-rail.service';
import { DownloadsStore } from './data/downloads.store';
import { provideDownloadEngine } from './data/engine.provider';
import { MOCK_ENGINE_AUTOSTART } from './data/mock-engine.service';
import { UiState } from './features/shell/ui-state.service';
import { I18nService } from './i18n/i18n.service';

describe('AppComponent (shell)', () => {
  let fixture: ComponentFixture<AppComponent>;
  let i18n: I18nService;
  let ui: UiState;

  function el(): HTMLElement {
    return fixture.nativeElement as HTMLElement;
  }

  function text(selector: string): string {
    return el().querySelector(selector)?.textContent?.trim() ?? '';
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [AppComponent],
      providers: [
        provideRouter(routes),
        provideDownloadEngine(),
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        {
          provide: DARK_SCHEME_QUERY,
          useValue: { matches: true, addEventListener: (): void => undefined },
        },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(AppComponent);
    i18n = TestBed.inject(I18nService);
    ui = TestBed.inject(UiState);
    fixture.detectChanges();
    // the router does not navigate on its own inside TestBed; activate the home route
    await TestBed.inject(Router).navigateByUrl('/');
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  });

  afterEach(() => {
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
    delete document.documentElement.dataset['theme'];
  });

  it('renders the frame in English: brand, search, views and the add action', () => {
    expect(text('.brand-name')).toBe('Riptide');
    expect(el().querySelector<HTMLInputElement>('.search-input')?.placeholder).toBe('Search downloads');
    const items = Array.from(el().querySelectorAll('.nav-group:first-of-type .nav-text')).map((n) => n.textContent?.trim());
    expect(items).toEqual(['All downloads', 'Active', 'Waiting', 'Completed', 'Needs attention']);
    expect(text('.topbar-actions .v-primary')).toBe('Add download');
  });

  it('shows the seeded downloads in the (virtual) list with live counts', () => {
    const rendered = el().querySelectorAll('app-download-row').length;
    expect(rendered).toBeGreaterThan(5);
    const allCount = Number(text('.nav-group:first-of-type .nav-item:first-of-type .nav-count'));
    const store = TestBed.inject(DownloadsStore);
    expect(allCount).toBe(store.rows().length);
    // the viewport renders only what fits plus overscan, never more than the data
    expect(rendered).toBeLessThanOrEqual(store.rows().length);
  });

  it('switches to Arabic live and mirrors the document', () => {
    i18n.setLang('ar');
    fixture.detectChanges();
    expect(document.documentElement.dir).toBe('rtl');
    expect(text('.brand-name')).toBe('ريبتايد');
    expect(text('.topbar-actions .v-primary')).toBe('إضافة تحميل');
  });

  it('filters the list through the search box', () => {
    const input = el().querySelector<HTMLInputElement>('.search-input');
    expect(input).toBeTruthy();
    if (!input) {
      return;
    }
    input.value = 'ubuntu';
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    const names = Array.from(el().querySelectorAll('app-download-row .name')).map((n) => n.textContent?.trim());
    expect(names.length).toBe(1);
    expect(names[0]).toContain('ubuntu');
  });

  it('highlights a downloads view only on the list page, never on Files, Research or Settings', async () => {
    const router = TestBed.inject(Router);
    const active = (): string[] => Array.from(el().querySelectorAll('.nav-item.is-active')).map((n) => n.textContent?.trim() ?? '');
    expect(active().some((t) => t.startsWith('All downloads'))).toBeTrue();
    for (const url of ['/files', '/research', '/settings']) {
      await router.navigateByUrl(url);
      fixture.detectChanges();
      await fixture.whenStable();
      fixture.detectChanges();
      expect(active().some((t) => t.startsWith('All downloads')))
        .withContext(url)
        .toBeFalse();
    }
  });

  it('opens the add dialog with Ctrl+N (its chunk is fetched on the first request)', async () => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }));
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
    const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
    expect(dialog?.querySelector('h2')?.textContent?.trim()).toBe('Add download');
  });

  it('opens the batch dialog with Ctrl+Shift+N or its toolbar button, and plain Ctrl+N still adds one download', () => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'N', ctrlKey: true, shiftKey: true, bubbles: true }));
    expect(ui.batchOpen()).toBeTrue();
    expect(ui.addOpen()).toBeFalse();
    ui.closeBatch();
    el().querySelector<HTMLButtonElement>('.batch-button')?.click();
    expect(ui.batchOpen()).toBeTrue();
    ui.closeBatch();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }));
    expect(ui.addOpen()).toBeTrue();
    expect(ui.batchOpen()).toBeFalse();
  });

  it('opens the keyboard shortcuts with ? and Ctrl+/, but not while typing in the search box', async () => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(ui.shortcutsOpen()).toBeTrue();
    const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
    expect(dialog?.querySelector('h2')?.textContent?.trim()).toBe('Keyboard shortcuts');
    expect(dialog?.querySelectorAll('.row').length).toBe(19);

    el().querySelector<HTMLInputElement>('.search-input')?.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
    expect(ui.shortcutsOpen()).toBeTrue();

    document.dispatchEvent(new KeyboardEvent('keydown', { key: '/', ctrlKey: true, bubbles: true }));
    fixture.detectChanges();
    expect(ui.shortcutsOpen()).toBeFalse();
    expect(dialog?.open).toBeFalse();
  });

  it('shows the frame as booting at first, then settles so later rows enter the ordinary way', () => {
    expect(el().querySelector('.frame')?.classList).toContain('is-booting');
    expect(el().querySelectorAll('app-download-row.is-staggered').length).toBeGreaterThan(0);
    ui.markBooted();
    fixture.detectChanges();
    expect(el().querySelector('.frame')?.classList).not.toContain('is-booting');
    expect(el().querySelectorAll('app-download-row.is-staggered').length).toBe(0);
    expect(el().querySelector('.status-help')?.getAttribute('title')).toBe('Keyboard shortcuts');
  });
});

describe('AppComponent sidebar toggle', () => {
  let fixture: ComponentFixture<AppComponent>;
  let store: MemoryPreferenceStore;
  let narrowListeners: ((e: { matches: boolean }) => void)[];

  function el(): HTMLElement {
    return fixture.nativeElement as HTMLElement;
  }

  function toggle(): HTMLButtonElement {
    const button = el().querySelector<HTMLButtonElement>('.nav-toggle');
    expect(button).withContext('toggle').toBeTruthy();
    return button as HTMLButtonElement;
  }

  async function settle(): Promise<void> {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  }

  async function open(url: string): Promise<void> {
    await TestBed.inject(Router).navigateByUrl(url);
    await settle();
  }

  async function setup(narrow = false, lang: 'en' | 'ar' = 'en'): Promise<void> {
    narrowListeners = [];
    store = new MemoryPreferenceStore();
    await TestBed.configureTestingModule({
      imports: [AppComponent],
      providers: [
        provideRouter(routes),
        provideDownloadEngine(),
        { provide: NAVIGATOR_LANGUAGES, useValue: [lang === 'ar' ? 'ar-SA' : 'en-US'] },
        { provide: PREFERENCE_STORE, useValue: store },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        {
          provide: NARROW_QUERY,
          useValue: { matches: narrow, addEventListener: (_: string, l: (e: { matches: boolean }) => void) => narrowListeners.push(l) },
        },
        { provide: DARK_SCHEME_QUERY, useValue: { matches: true, addEventListener: (): void => undefined } },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(AppComponent);
    await settle();
    await open('/');
  }

  afterEach(() => {
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
  });

  it('is present on Downloads, Files, Research and Settings, with the same label and state rules', async () => {
    await setup();
    for (const [url, expanded] of [['/', true], ['/files', false], ['/research', false], ['/settings', true]] as const) {
      await open(url);
      expect(el().querySelectorAll('.nav-toggle').length).withContext(url).toBe(1);
      expect(toggle().getAttribute('aria-expanded')).withContext(url).toBe(String(expanded));
      expect(toggle().getAttribute('aria-label')).withContext(url).toBe(expanded ? 'Hide sidebar' : 'Show sidebar');
      expect(toggle().title).toContain('Ctrl+B');
      expect(el().querySelector('.frame')?.classList.contains('is-rail')).withContext(url).toBe(!expanded);
    }
  });

  it('labels the toggle in Arabic', async () => {
    await setup(false, 'ar');
    expect(toggle().getAttribute('aria-label')).toBe('إخفاء الشريط الجانبي');
    toggle().click();
    await settle();
    expect(toggle().getAttribute('aria-label')).toBe('إظهار الشريط الجانبي');
    expect(toggle().getAttribute('aria-expanded')).toBe('false');
  });

  it('collapses and expands on click and on Ctrl+B, and keeps focus on the toggle', async () => {
    await setup();
    toggle().focus();
    toggle().click();
    await settle();
    expect(el().querySelector('.frame')?.classList.contains('is-rail')).toBeTrue();
    expect(document.activeElement).toBe(toggle());
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }));
    await settle();
    expect(el().querySelector('.frame')?.classList.contains('is-rail')).toBeFalse();
    expect(toggle().getAttribute('aria-expanded')).toBe('true');
  });

  it('remembers the choice per page group', async () => {
    await setup();
    toggle().click(); // Downloads: collapse
    await settle();
    await open('/settings');
    expect(toggle().getAttribute('aria-expanded')).toBe('false');
    await open('/research');
    expect(toggle().getAttribute('aria-expanded')).toBe('false'); // its own default, untouched
    toggle().click();
    await settle();
    await open('/files');
    expect(toggle().getAttribute('aria-expanded')).toBe('true');
    await open('/');
    expect(toggle().getAttribute('aria-expanded')).toBe('false');
  });

  it('no longer has the old toggle at the bottom of the rail', async () => {
    await setup();
    await open('/research');
    expect(el().querySelector('.rail-toggle')).toBeNull();
    expect(el().querySelector('.nav-foot [aria-expanded]')).toBeNull();
  });

  it('under 900px the full navigation overlays the page with a scrim and closes on Escape or an outside click', async () => {
    await setup(true);
    expect(el().querySelector('.nav-scrim')).toBeNull();
    expect(toggle().getAttribute('aria-expanded')).toBe('false');
    toggle().click();
    await settle();
    expect(toggle().getAttribute('aria-expanded')).toBe('true');
    expect(el().querySelector('.frame')?.classList.contains('is-overlay')).toBeTrue();
    expect(el().querySelector('.nav-scrim')).toBeTruthy();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(el().querySelector('.nav-scrim')).toBeNull();
    expect(document.activeElement).toBe(toggle());
    toggle().click();
    await settle();
    el().querySelector<HTMLElement>('.nav-scrim')?.click();
    await settle();
    expect(el().querySelector('.nav-scrim')).toBeNull();
    expect(toggle().getAttribute('aria-expanded')).toBe('false');
  });
});
