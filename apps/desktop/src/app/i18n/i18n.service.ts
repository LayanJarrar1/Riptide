import { computed, DOCUMENT, effect, inject, Injectable, signal } from '@angular/core';

import ar from '../../i18n/ar.json';
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

const TABLES: Readonly<Record<Lang, Readonly<Record<string, string>>>> = { ar, en };

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

  /** Switches the language live and remembers the choice. */
  setLang(lang: Lang): void {
    this.lang.set(lang);
    this.store.set(LANG_STORAGE_KEY, lang);
  }

  /**
   * Returns the translation of `key` in the active language.
   * A missing key returns the key itself and warns once per lookup; the i18n:lint script
   * is what keeps that from ever happening in a shipped build.
   */
  t(key: string): string {
    const value = TABLES[this.lang()][key];
    if (value === undefined) {
      console.warn(`[i18n] missing key "${key}" for language "${this.lang()}"`);
      return key;
    }
    return value;
  }
}
