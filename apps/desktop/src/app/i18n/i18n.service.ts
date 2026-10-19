import { computed, DOCUMENT, effect, inject, Injectable, signal } from '@angular/core';

import en from '../../i18n/en.json';
import { NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../core/preferences';

/** Supported UI languages. */
export type Lang = 'ar' | 'en';

/** Writing direction derived from the language. */
export type Dir = 'rtl' | 'ltr';

/** Every key present in the translation tables (both files carry the same keys; see i18n:lint). */
export type TranslationKey = keyof typeof en;

/** `localStorage` key for the saved language. */
export const LANG_STORAGE_KEY = 'riptide.lang';

/** All languages the app ships, in menu order. */
export const LANGS: readonly Lang[] = ['ar', 'en'];

/** One language's strings. */
export type Table = Readonly<Record<string, string>>;

/**
 * The loaded tables. English ships in the main bundle (it is the fallback and the source of the
 * key type); Arabic is a separate chunk, fetched at startup when it is the language in use and on
 * the first switch otherwise, so it stays out of the initial bundle for English users.
 */
const TABLES: Partial<Record<Lang, Table>> = { en };

const LOADERS: Readonly<Record<Lang, () => Promise<Table>>> = {
  en: () => Promise.resolve(en),
  ar: async () => (await import('../../i18n/ar.json')).default,
};

/** Makes sure the table of `lang` is loaded. */
export async function loadTable(lang: Lang): Promise<void> {
  TABLES[lang] ??= await LOADERS[lang]();
}

/** Puts a table in place without fetching it (the test setup preloads Arabic this way). */
export function registerTable(lang: Lang, table: Table): void {
  TABLES[lang] = table;
}

/** Type guard for a persisted or user-supplied language value. */
export function isLang(value: unknown): value is Lang {
  return value === 'ar' || value === 'en';
}

/**
 * Picks the startup language: the saved choice wins, otherwise the first browser language
 * that starts with `ar` selects Arabic, otherwise English.
 */
export function detectLang(saved: string | null, navigatorLanguages: readonly string[]): Lang {
  if (isLang(saved)) {
    return saved;
  }
  const wantsArabic = navigatorLanguages.some((tag) => tag.toLowerCase().startsWith('ar'));
  return wantsArabic ? 'ar' : 'en';
}

/** Direction for a language. */
export function dirOf(lang: Lang): Dir {
  return lang === 'ar' ? 'rtl' : 'ltr';
}

/**
 * Runtime translation: holds the current language as a signal and looks strings up in the
 * bundled tables. Keeps `<html lang dir>` in sync so a switch is live everywhere.
 */
@Injectable({ providedIn: 'root' })
export class I18nService {
  private readonly document = inject(DOCUMENT);
  private readonly store = inject(PREFERENCE_STORE);

  /** The active language. */
  readonly lang = signal<Lang>(
    detectLang(this.store.get(LANG_STORAGE_KEY), inject(NAVIGATOR_LANGUAGES)),
  );

  /** Writing direction of the active language. */
  readonly dir = computed<Dir>(() => dirOf(this.lang()));

  constructor() {
    effect(() => {
      const root = this.document.documentElement;
      root.lang = this.lang();
      root.dir = this.dir();
    });
  }

  /** Resolves once the table of the language in use is loaded (the app waits for it at startup). */
  ready(): Promise<void> {
    return loadTable(this.lang());
  }

  /**
   * Switches the language live and remembers the choice. When the table is not loaded yet it is
   * fetched first (a local chunk, a few milliseconds) and the switch happens as soon as it lands.
   */
  setLang(lang: Lang): void {
    this.store.set(LANG_STORAGE_KEY, lang);
    if (TABLES[lang] !== undefined) {
      this.lang.set(lang);
      return;
    }
    void loadTable(lang).then(() => {
      // the user may have clicked again while the table was on its way
      if (this.store.get(LANG_STORAGE_KEY) === lang) {
        this.lang.set(lang);
      }
    });
  }

  /**
   * Returns the translation of `key` in the active language.
   * A missing key returns the key itself and warns once per lookup; the i18n:lint script
   * is what keeps that from ever happening in a shipped build.
   */
  t(key: string): string {
    const table: Table = TABLES[this.lang()] ?? en;
    const value: string | undefined = table[key];
    if (value === undefined) {
      console.warn(`[i18n] missing key "${key}" for language "${this.lang()}"`);
      return key;
    }
    return value;
  }
}
