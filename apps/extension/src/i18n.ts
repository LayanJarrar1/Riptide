/**
 * Runtime strings for the popup, options page, content script and notifications. Manifest
 * strings live in `_locales/`. One table per language; `i18n.test.ts` fails on a missing key.
 */
import ar from './i18n/ar.json';
import en from './i18n/en.json';
import type { LanguagePref } from './settings';

/** Supported UI languages. */
export type Lang = 'en' | 'ar';

/** Every key of the English table (the source of truth). */
export type MessageKey = keyof typeof en;

const TABLES: Record<Lang, Record<string, string>> = { en, ar };

/** Pick the UI language from the preference and the browser's UI language. */
export function resolveLang(pref: LanguagePref, uiLanguage: string): Lang {
  if (pref === 'en' || pref === 'ar') return pref;
  return uiLanguage.toLowerCase().startsWith('ar') ? 'ar' : 'en';
}

/** Text direction for a language. */
export function dirOf(lang: Lang): 'ltr' | 'rtl' {
  return lang === 'ar' ? 'rtl' : 'ltr';
}

/** Translate `key`, substituting `{name}` placeholders from `params`. */
export function t(lang: Lang, key: MessageKey, params?: Record<string, string | number>): string {
  const text = TABLES[lang][key] ?? en[key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match,
  );
}

/** Keys present in English but missing or empty in `lang`. */
export function missingKeys(lang: Lang): string[] {
  const table = TABLES[lang];
  return Object.keys(en).filter((k) => typeof table[k] !== 'string' || table[k].trim() === '');
}

/** Keys present in `lang` that English does not have. */
export function extraKeys(lang: Lang): string[] {
  return Object.keys(TABLES[lang]).filter((k) => !(k in en));
}

/** `{placeholders}` used by a string, sorted. */
export function placeholdersOf(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? '').sort();
}

/** Human-readable size, localised digits and unit. */
export function formatBytes(lang: Lang, bytes: number): string {
  const units: MessageKey[] = ['units.kb', 'units.mb', 'units.gb'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const nf = new Intl.NumberFormat(lang === 'ar' ? 'ar-EG' : 'en-US', {
    maximumFractionDigits: value < 10 ? 1 : 0,
  });
  return `${nf.format(value)} ${t(lang, units[unit] ?? 'units.kb')}`;
}

/** Apply `lang`/`dir` to the document and fill every `[data-i18n]` element. */
export function localiseDocument(lang: Lang): void {
  document.documentElement.lang = lang;
  document.documentElement.dir = dirOf(lang);
  for (const el of document.querySelectorAll<HTMLElement>('[data-i18n]')) {
    const key = el.dataset['i18n'] as MessageKey | undefined;
    if (key && key in en) el.textContent = t(lang, key);
  }
  for (const el of document.querySelectorAll<HTMLElement>('[data-i18n-title]')) {
    const key = el.dataset['i18nTitle'] as MessageKey | undefined;
    if (key && key in en) el.title = t(lang, key);
  }
  for (const el of document.querySelectorAll<HTMLInputElement>('[data-i18n-placeholder]')) {
    const key = el.dataset['i18nPlaceholder'] as MessageKey | undefined;
    if (key && key in en) el.placeholder = t(lang, key);
  }
}
