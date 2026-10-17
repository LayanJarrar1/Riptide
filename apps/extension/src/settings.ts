/** User settings (pure shape + normalisation; storage IO lives in `storage.ts`). */
import { DEFAULT_RULES, normaliseRules } from './rules';
import type { Rules } from './rules';

/** `auto` follows the browser UI language. */
export type LanguagePref = 'auto' | 'en' | 'ar';

/** Everything stored under `storage.local.settings`. */
export interface Settings {
  rules: Rules;
  language: LanguagePref;
  /** Toast when a download is sent to Riptide. */
  notify: boolean;
  /** Content-script badge when hovering a direct file link. */
  hoverBadge: boolean;
}

/** Defaults for a fresh install. */
export const DEFAULT_SETTINGS: Readonly<Settings> = Object.freeze({
  rules: DEFAULT_RULES,
  language: 'auto',
  notify: true,
  hoverBadge: true,
});

/** Merge whatever is in storage with the defaults. */
export function normaliseSettings(raw: unknown): Settings {
  const r = (typeof raw === 'object' && raw !== null ? raw : {}) as Record<string, unknown>;
  const language = r['language'];
  return {
    rules: normaliseRules(r['rules']),
    language: language === 'en' || language === 'ar' ? language : 'auto',
    notify: typeof r['notify'] === 'boolean' ? r['notify'] : DEFAULT_SETTINGS.notify,
    hoverBadge: typeof r['hoverBadge'] === 'boolean' ? r['hoverBadge'] : DEFAULT_SETTINGS.hoverBadge,
  };
}
