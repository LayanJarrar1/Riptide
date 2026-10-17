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

  it('opens the add dialog with Ctrl+N', () => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }));
    fixture.detectChanges();
    const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
    expect(dialog?.querySelector('h2')?.textContent?.trim()).toBe('Add download');
  });

  it('opens the keyboard shortcuts with ? and Ctrl+/, but not while typing in the search box', () => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
    fixture.detectChanges();
    expect(ui.shortcutsOpen()).toBeTrue();
    const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
    expect(dialog?.querySelector('h2')?.textContent?.trim()).toBe('Keyboard shortcuts');
    expect(dialog?.querySelectorAll('.row').length).toBe(12);

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
