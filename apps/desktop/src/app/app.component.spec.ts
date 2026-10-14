import { ComponentFixture, TestBed } from '@angular/core/testing';

import { AppComponent } from './app.component';
import {
  DARK_SCHEME_QUERY,
  MemoryPreferenceStore,
  NAVIGATOR_LANGUAGES,
  PREFERENCE_STORE,
} from './core/preferences';
import { I18nService } from './i18n/i18n.service';

describe('AppComponent', () => {
  let fixture: ComponentFixture<AppComponent>;
  let i18n: I18nService;

  function text(selector: string): string {
    const el = fixture.nativeElement as HTMLElement;
    return el.querySelector(selector)?.textContent?.trim() ?? '';
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [AppComponent],
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        {
          provide: DARK_SCHEME_QUERY,
          useValue: { matches: true, addEventListener: (): void => undefined },
        },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(AppComponent);
    i18n = TestBed.inject(I18nService);
    fixture.detectChanges();
  });

  afterEach(() => {
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
    delete document.documentElement.dataset['theme'];
  });

  it('renders the English heading, status and tagline', () => {
    expect(text('h1')).toBe('Welcome to Riptide');
    expect(text('.eyebrow')).toBe('Milestone 0 — skeleton');
    expect(text('.brand-tagline')).toBe('Riptide — a download manager');
  });

  it('renders the Arabic heading after a live switch and flips the direction', () => {
    i18n.setLang('ar');
    fixture.detectChanges();
    expect(text('h1')).toBe('أهلاً بك في ريبتايد');
    expect(text('.eyebrow')).toBe('المرحلة 0 — الهيكل');
    expect(document.documentElement.dir).toBe('rtl');
  });

  it('switches language through the segmented control', () => {
    const el = fixture.nativeElement as HTMLElement;
    const arabic = Array.from(el.querySelectorAll<HTMLButtonElement>('.segment')).find(
      (b) => b.textContent?.trim() === 'العربية',
    );
    expect(arabic).withContext('Arabic segment present').toBeDefined();
    arabic?.click();
    fixture.detectChanges();
    expect(i18n.lang()).toBe('ar');
    expect(text('h1')).toBe('أهلاً بك في ريبتايد');
  });

  it('shows no version line outside Tauri', () => {
    expect(text('.footer-version')).toBe('');
  });
});
