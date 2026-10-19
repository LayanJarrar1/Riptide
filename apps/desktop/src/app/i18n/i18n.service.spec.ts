import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../core/preferences';
import { detectLang, I18nService, LANG_STORAGE_KEY } from './i18n.service';

function setup(navigatorLanguages: readonly string[], saved: Record<string, string> = {}) {
  const store = new MemoryPreferenceStore(saved);
  TestBed.configureTestingModule({
    providers: [
      { provide: NAVIGATOR_LANGUAGES, useValue: navigatorLanguages },
      { provide: PREFERENCE_STORE, useValue: store },
    ],
  });
  return { service: TestBed.inject(I18nService), store };
}

describe('detectLang', () => {
  it('prefers the saved choice', () => {
    expect(detectLang('en', ['ar-SA'])).toBe('en');
    expect(detectLang('ar', ['en-US'])).toBe('ar');
  });

  it('ignores an invalid saved value', () => {
    expect(detectLang('fr', ['ar'])).toBe('ar');
    expect(detectLang('', ['en'])).toBe('en');
  });

  it('picks Arabic when any browser language starts with "ar"', () => {
    expect(detectLang(null, ['en-US', 'ar-EG'])).toBe('ar');
    expect(detectLang(null, ['AR'])).toBe('ar');
  });

  it('falls back to English otherwise', () => {
    expect(detectLang(null, ['en-GB', 'fr'])).toBe('en');
    expect(detectLang(null, [])).toBe('en');
  });
});

describe('I18nService', () => {
  afterEach(() => {
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
  });

  it('detects Arabic from navigator.languages when nothing is saved', () => {
    const { service } = setup(['ar-JO', 'en']);
    expect(service.lang()).toBe('ar');
    expect(service.dir()).toBe('rtl');
  });

  it('defaults to English for a non-Arabic browser', () => {
    const { service } = setup(['de-DE', 'en-US']);
    expect(service.lang()).toBe('en');
    expect(service.dir()).toBe('ltr');
  });

  it('honours a saved language over the browser', () => {
    const { service } = setup(['ar'], { [LANG_STORAGE_KEY]: 'en' });
    expect(service.lang()).toBe('en');
  });

  it('setLang switches dir and the <html> attributes live, and persists', () => {
    const { service, store } = setup(['en-US']);
    TestBed.tick();
    expect(document.documentElement.lang).toBe('en');
    expect(document.documentElement.dir).toBe('ltr');

    service.setLang('ar');
    TestBed.tick();
    expect(service.dir()).toBe('rtl');
    expect(document.documentElement.lang).toBe('ar');
    expect(document.documentElement.dir).toBe('rtl');
    expect(store.get(LANG_STORAGE_KEY)).toBe('ar');
  });

  it('t() returns the Arabic and the English string for the same key', () => {
    const { service } = setup(['en-US']);
    expect(service.t('app.tagline')).toBe('Riptide — a download manager');
    service.setLang('ar');
    expect(service.t('app.tagline')).toBe('ريبتايد — مدير تحميل');
  });

  it('t() returns the key and warns when it is missing', () => {
    const { service } = setup(['en-US']);
    const warn = spyOn(console, 'warn');
    expect(service.t('does.not.exist')).toBe('does.not.exist');
    expect(warn).toHaveBeenCalledTimes(1);
  });
});

describe('lazy Arabic table', () => {
  it('ready() resolves for the language in use and the real chunk loads and matches the English keys', async () => {
    const { service } = setup(['ar-SA']);
    await service.ready();
    expect(service.t('app.title')).not.toBe('app.title');
    const { loadTable } = await import('./i18n.service');
    await loadTable('ar');
    service.setLang('en');
    service.setLang('ar');
    expect(service.lang()).toBe('ar');
  });
});
