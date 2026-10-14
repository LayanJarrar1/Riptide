import { InjectionToken } from '@angular/core';

/**
 * Minimal key/value store for user preferences (language, theme).
 *
 * Backed by `localStorage` in the app; tests provide an in-memory implementation.
 * Every access is guarded: storage can throw (privacy mode, quota, sandboxed webview).
 */
export interface PreferenceStore {
  get(key: string): string | null;
  set(key: string, value: string): void;
}

/** `localStorage` wrapped so that failures degrade to "no saved preference". */
export class LocalPreferenceStore implements PreferenceStore {
  get(key: string): string | null {
    try {
      return globalThis.localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  set(key: string, value: string): void {
    try {
      globalThis.localStorage.setItem(key, value);
    } catch {
      // Not fatal: the preference simply does not survive a restart.
    }
  }
}

/** In-memory store for unit tests. */
export class MemoryPreferenceStore implements PreferenceStore {
  private readonly map = new Map<string, string>();

  constructor(initial: Record<string, string> = {}) {
    for (const [k, v] of Object.entries(initial)) {
      this.map.set(k, v);
    }
  }

  get(key: string): string | null {
    return this.map.get(key) ?? null;
  }

  set(key: string, value: string): void {
    this.map.set(key, value);
  }
}

export const PREFERENCE_STORE = new InjectionToken<PreferenceStore>('riptide.preferenceStore', {
  providedIn: 'root',
  factory: () => new LocalPreferenceStore(),
});

/** The user's preferred languages, most preferred first (BCP 47 tags). */
export const NAVIGATOR_LANGUAGES = new InjectionToken<readonly string[]>(
  'riptide.navigatorLanguages',
  {
    providedIn: 'root',
    factory: () => [...globalThis.navigator.languages],
  },
);

/** The subset of `MediaQueryList` the theme service needs; tests provide a fake. */
export interface DarkSchemeQuery {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: (event: { matches: boolean }) => void): void;
}

export const DARK_SCHEME_QUERY = new InjectionToken<DarkSchemeQuery>('riptide.darkSchemeQuery', {
  providedIn: 'root',
  factory: () => globalThis.matchMedia('(prefers-color-scheme: dark)'),
});
