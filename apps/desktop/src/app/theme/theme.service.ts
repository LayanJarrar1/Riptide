import { computed, DOCUMENT, effect, inject, Injectable, signal } from '@angular/core';

import { DARK_SCHEME_QUERY, PREFERENCE_STORE } from '../core/preferences';

/** What the user asked for. */
export type Theme = 'system' | 'dark' | 'light';

/** What is actually applied, after resolving `system` against the OS preference. */
export type ResolvedTheme = 'dark' | 'light';

/** `localStorage` key for the saved theme. */
export const THEME_STORAGE_KEY = 'riptide.theme';

/** All theme choices, in menu order. */
export const THEMES: readonly Theme[] = ['system', 'dark', 'light'];

/** Type guard for a persisted or user-supplied theme value. */
export function isTheme(value: unknown): value is Theme {
  return value === 'system' || value === 'dark' || value === 'light';
}

/** What a fresh install gets: the light theme (`system` and `dark` remain a Settings choice). */
export const DEFAULT_THEME: Theme = 'light';

/**
 * Theme preference with live OS tracking. Writes `data-theme="dark|light"` on `<html>`,
 * which is the only selector tokens.css switches on. Light by default; `system` follows the
 * OS only when the user picks it.
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly document = inject(DOCUMENT);
  private readonly store = inject(PREFERENCE_STORE);
  private readonly query = inject(DARK_SCHEME_QUERY);

  /** The user's choice. */
  readonly theme = signal<Theme>(this.readSaved());

  private readonly systemPrefersDark = signal<boolean>(this.query.matches);

  /** The theme in effect right now. */
  readonly resolved = computed<ResolvedTheme>(() => {
    const theme = this.theme();
    if (theme === 'system') {
      return this.systemPrefersDark() ? 'dark' : 'light';
    }
    return theme;
  });

  constructor() {
    this.query.addEventListener('change', (event) => this.systemPrefersDark.set(event.matches));
    effect(() => {
      this.document.documentElement.dataset['theme'] = this.resolved();
    });
  }

  /** Applies a theme live and remembers the choice. */
  setTheme(theme: Theme): void {
    this.theme.set(theme);
    this.store.set(THEME_STORAGE_KEY, theme);
  }

  private readSaved(): Theme {
    const saved = this.store.get(THEME_STORAGE_KEY);
    return isTheme(saved) ? saved : DEFAULT_THEME;
  }
}
