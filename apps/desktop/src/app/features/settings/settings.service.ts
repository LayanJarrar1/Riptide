import { effect, inject, Injectable, signal } from '@angular/core';

import { PREFERENCE_STORE } from '../../core/preferences';
import { CategoryKey } from '../../model/download';

/** User settings. Persisted as JSON; the engine reads the same values over IPC in M1. */
export interface Settings {
  readonly defaultDir: string;
  readonly categoryDirs: Readonly<Partial<Record<CategoryKey, string>>>;
  /** Connections per download (1..32). */
  readonly connections: number;
  /** Connections per host across downloads. */
  readonly perHost: number;
  /** Global cap in bytes per second, `null` = unlimited. */
  readonly speedLimitBps: number | null;
  readonly confirmOnExit: boolean;
  readonly startWithWindows: boolean;
  readonly closeToTray: boolean;
  readonly clipboardMonitor: boolean;
  readonly browserIntegration: boolean;
  readonly notifyOnComplete: boolean;
  readonly proxyMode: 'system' | 'none' | 'manual';
  readonly proxyUrl: string;
  readonly checkUpdates: boolean;
  readonly channel: 'stable' | 'beta';
}

/** `localStorage` key. */
export const SETTINGS_STORAGE_KEY = 'riptide.settings';

/** Defaults for a fresh install. */
export const DEFAULT_SETTINGS: Settings = {
  defaultDir: 'C:\\Users\\layan\\Downloads',
  categoryDirs: {},
  connections: 8,
  perHost: 4,
  speedLimitBps: null,
  confirmOnExit: true,
  startWithWindows: false,
  closeToTray: true,
  clipboardMonitor: true,
  browserIntegration: false,
  notifyOnComplete: true,
  proxyMode: 'system',
  proxyUrl: '',
  checkUpdates: true,
  channel: 'stable',
};

/** Settings as a signal, persisted on every change. */
@Injectable({ providedIn: 'root' })
export class SettingsService {
  private readonly store = inject(PREFERENCE_STORE);

  readonly settings = signal<Settings>(this.load());

  constructor() {
    effect(() => {
      this.store.set(SETTINGS_STORAGE_KEY, JSON.stringify(this.settings()));
    });
  }

  /** Applies a partial change. */
  patch(change: Partial<Settings>): void {
    this.settings.update((s) => ({ ...s, ...change }));
  }

  /** Sets (or clears, with `null`) the folder of a category. */
  setCategoryDir(category: CategoryKey, dir: string | null): void {
    this.settings.update((s) => {
      const categoryDirs = { ...s.categoryDirs };
      if (dir === null || dir.trim() === '') {
        delete categoryDirs[category];
      } else {
        categoryDirs[category] = dir;
      }
      return { ...s, categoryDirs };
    });
  }

  /** Folder a new download of `category` goes to. */
  dirFor(category: CategoryKey): string {
    return this.settings().categoryDirs[category] ?? this.settings().defaultDir;
  }

  /** Back to defaults. */
  reset(): void {
    this.settings.set(DEFAULT_SETTINGS);
  }

  private load(): Settings {
    try {
      const raw = this.store.get(SETTINGS_STORAGE_KEY);
      if (raw === null) {
        return DEFAULT_SETTINGS;
      }
      const parsed: unknown = JSON.parse(raw);
      if (parsed === null || typeof parsed !== 'object') {
        return DEFAULT_SETTINGS;
      }
      return { ...DEFAULT_SETTINGS, ...(parsed as Partial<Settings>) };
    } catch {
      return DEFAULT_SETTINGS;
    }
  }
}
