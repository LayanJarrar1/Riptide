import { DestroyRef, effect, inject, Injectable, InjectionToken, signal } from '@angular/core';

import { PREFERENCE_STORE } from '../../core/preferences';
import { CATEGORY_KEYS, CategoryKey } from '../../model/download';
import type { EngineSettings } from '../../tauri/bridge/EngineSettings';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';

/** User settings. Persisted as JSON; the engine-side subset is mirrored over IPC under Tauri. */
export interface Settings {
  readonly defaultDir: string;
  readonly categoryDirs: Readonly<Partial<Record<CategoryKey, string>>>;
  /** Sort new downloads into `<root>/<Category>` folders (engine-side; on for a new install). */
  readonly organize: boolean;
  /** Root of the category folders; empty = `<defaultDir>\Riptide`. */
  readonly organizeRoot: string;
  /** Folder for torrents; empty = the organised `Torrents` folder or the default folder. */
  readonly torrentsDir: string;
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
  /** Windows: mark finished downloads as coming from the internet (Mark-of-the-Web). Engine-side. */
  readonly markDownloads: boolean;
  /** Follow redirects from a public site to a private network address (engine-side, off by default). */
  readonly allowPrivateRedirects: boolean;
  /** Keep uploading after a torrent finishes (engine-side, off by default). */
  readonly torrentSeeding: boolean;
  /** Stop seeding at this ratio in percent (100 = 1.0); 0 = no ratio limit. */
  readonly torrentRatioPercent: number;
  /** Stop seeding after this many minutes; 0 = no time limit. */
  readonly torrentMinutes: number;
  /** Upload cap for torrents in bytes per second, `null` = unlimited. */
  readonly torrentUploadBps: number | null;
  /** Find torrent peers through the DHT (applies after a restart). */
  readonly torrentDht: boolean;
  /** Port for incoming torrent peers, 0 = automatic (applies after a restart). */
  readonly torrentPort: number;
  readonly proxyMode: 'system' | 'none' | 'manual';
  readonly proxyUrl: string;
  readonly checkUpdates: boolean;
  readonly channel: 'stable' | 'beta';
  /** Row height of the downloads list. */
  readonly density: 'comfortable' | 'compact';
  /** The sidebar can be hidden from the View menu. */
  readonly sidebarVisible: boolean;
}

/** Folder name per category under the organised root; mirrors `Category::folder_name`. */
export const CATEGORY_FOLDERS: Readonly<Record<CategoryKey, string>> = {
  compressed: 'Archives',
  documents: 'Documents',
  music: 'Audio',
  video: 'Videos',
  programs: 'Applications',
  images: 'Images',
  other: 'Other',
};

/** Torrents' folder name under the organised root (`TORRENTS_FOLDER` in riptide-core). */
export const TORRENTS_FOLDER = 'Torrents';
/** `category_dirs` key of the torrents folder (`TORRENTS_KEY` in riptide-engine). */
const TORRENTS_KEY = 'torrents';

/** The organised root: the chosen one, else `<defaultDir>\Riptide`. */
export function organizedRoot(s: Pick<Settings, 'organizeRoot' | 'defaultDir'>): string {
  const root = s.organizeRoot.trim();
  return root.length > 0 ? root : joinPath(s.defaultDir, 'Riptide');
}

/** Joins a folder and a name with the separator the folder already uses (`\\` on Windows). */
export function joinPath(dir: string, name: string): string {
  const sep = dir.includes('\\') || !dir.includes('/') ? '\\' : '/';
  return dir.endsWith('\\') || dir.endsWith('/') ? `${dir}${name}` : `${dir}${sep}${name}`;
}

/** `localStorage` key. */
export const SETTINGS_STORAGE_KEY = 'riptide.settings';

/** How long after the last change the engine is told (slider drags produce many changes). */
export const ENGINE_SYNC_DEBOUNCE = new InjectionToken<number>('riptide.settings.engineSyncDebounce', {
  providedIn: 'root',
  factory: () => 300,
});

/** Defaults for a fresh install. */
export const DEFAULT_SETTINGS: Settings = {
  defaultDir: 'D:\\Downloads',
  categoryDirs: {},
  organize: true,
  organizeRoot: '',
  torrentsDir: '',
  connections: 8,
  perHost: 4,
  speedLimitBps: null,
  confirmOnExit: true,
  startWithWindows: false,
  closeToTray: true,
  clipboardMonitor: true,
  browserIntegration: false,
  notifyOnComplete: true,
  markDownloads: true,
  allowPrivateRedirects: false,
  torrentSeeding: false,
  torrentRatioPercent: 100,
  torrentMinutes: 1440,
  torrentUploadBps: null,
  torrentDht: true,
  torrentPort: 0,
  proxyMode: 'system',
  proxyUrl: '',
  checkUpdates: true,
  // alpha: all releases are pre-releases; switch back to stable at the first stable release
  channel: 'beta',
  density: 'comfortable',
  sidebarVisible: true,
};

/**
 * Settings as a signal, persisted on every change.
 *
 * Inside Tauri the values the engine owns (`default_dir`, `connections`, `per_host`,
 * `category_dirs`, `global_speed_limit`, `proxy`) are read from `get_engine_settings` at
 * start-up and written back with `set_engine_settings` (debounced) whenever they change;
 * `max_active` and `user_agent` are passed through as the engine reported them.
 * `confirmOnExit` is mirrored to the window's close handler with `set_exit_guard`. In a plain
 * browser nothing talks to an engine.
 */
@Injectable({ providedIn: 'root' })
export class SettingsService {
  private readonly store = inject(PREFERENCE_STORE);
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly debounceMs = inject(ENGINE_SYNC_DEBOUNCE);

  /** What the engine last confirmed; `null` until `get_engine_settings` answered (browser: never). */
  private engine: EngineSettings | null = null;
  private syncTimer: ReturnType<typeof setTimeout> | null = null;
  private syncing: Promise<void> = Promise.resolve();

  readonly settings = signal<Settings>(this.load());

  /** Resolves once the engine's values are merged in (at once in a plain browser). */
  readonly engineLoaded: Promise<void>;

  constructor() {
    effect(() => {
      this.store.set(SETTINGS_STORAGE_KEY, JSON.stringify(this.settings()));
    });
    this.engineLoaded = this.tauri ? this.loadEngineSettings() : Promise.resolve();
    if (this.tauri) {
      effect(() => {
        this.scheduleSync(this.settings());
      });
      effect(() => {
        const enabled = this.settings().confirmOnExit;
        this.invoke<void>('set_exit_guard', { enabled }).catch((error: unknown) => console.warn('[settings] set_exit_guard failed', error));
      });
      inject(DestroyRef).onDestroy(() => {
        if (this.syncTimer !== null) {
          clearTimeout(this.syncTimer);
          this.syncTimer = null;
        }
      });
    }
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

  /** Folder a new download of `category` goes to; mirrors `EngineSettings::dir_for`. */
  dirFor(category: CategoryKey): string {
    const s = this.settings();
    const own = s.categoryDirs[category]?.trim();
    if (own) {
      return own;
    }
    return s.organize ? joinPath(organizedRoot(s), CATEGORY_FOLDERS[category]) : s.defaultDir;
  }

  /** Folder a new torrent goes to; mirrors `EngineSettings::torrent_dir`. */
  torrentDir(): string {
    const s = this.settings();
    const own = s.torrentsDir.trim();
    if (own) {
      return own;
    }
    return s.organize ? joinPath(organizedRoot(s), TORRENTS_FOLDER) : s.defaultDir;
  }

  /** The organised folders back to the shipped layout: on, under the default folder, no overrides. */
  restoreDefaultFolders(): void {
    this.patch({ organize: true, organizeRoot: '', torrentsDir: '', categoryDirs: {} });
  }

  /** Back to defaults. */
  reset(): void {
    this.settings.set(DEFAULT_SETTINGS);
  }

  /** Resolves when the pending (debounced) engine write, if any, has been sent. Specs use it. */
  whenSynced(): Promise<void> {
    return this.syncing;
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

  private async loadEngineSettings(): Promise<void> {
    try {
      const engine = await this.invoke<EngineSettings>('get_engine_settings');
      this.engine = engine;
      const categoryDirs: Partial<Record<CategoryKey, string>> = { ...this.settings().categoryDirs };
      for (const key of CATEGORY_KEYS) {
        const dir = engine.category_dirs?.[key];
        if (typeof dir === 'string' && dir.trim().length > 0) {
          categoryDirs[key] = dir;
        }
      }
      const torrentsDir = engine.category_dirs?.[TORRENTS_KEY];
      this.settings.update((s) => ({
        ...s,
        defaultDir: engine.default_dir,
        connections: engine.connections,
        perHost: engine.per_host,
        categoryDirs,
        organize: engine.organize ?? false,
        organizeRoot: engine.organize_root ?? '',
        torrentsDir: typeof torrentsDir === 'string' ? torrentsDir : '',
        speedLimitBps: engine.global_speed_limit ?? null,
        markDownloads: engine.mark_of_the_web ?? true,
        allowPrivateRedirects: engine.allow_private_redirects ?? false,
        torrentSeeding: engine.torrent?.seeding ?? false,
        torrentRatioPercent: engine.torrent?.seed_ratio_percent ?? 100,
        torrentMinutes: engine.torrent?.seed_minutes ?? 1440,
        torrentUploadBps: engine.torrent?.upload_limit ?? null,
        torrentDht: engine.torrent?.dht ?? true,
        torrentPort: engine.torrent?.listen_port ?? 0,
        proxyMode: engine.proxy.mode,
        proxyUrl: engine.proxy.url,
      }));
    } catch (error: unknown) {
      console.warn('[settings] get_engine_settings failed', error);
    }
  }

  private scheduleSync(current: Settings): void {
    if (this.syncTimer !== null) {
      clearTimeout(this.syncTimer);
    }
    this.syncTimer = setTimeout(() => {
      this.syncTimer = null;
      this.syncing = this.pushEngineSettings(current);
    }, this.debounceMs);
  }

  private async pushEngineSettings(current: Settings): Promise<void> {
    const { defaultDir, connections, perHost, categoryDirs } = current;
    const known = this.engine;
    // not loaded yet: the load merges the engine's values and the effect runs again
    if (known === null) {
      return;
    }
    // the engine keeps the same keys (`CategoryKey` === `riptide_core::Category::key`); blanks are "use the default"
    const category_dirs: EngineSettings['category_dirs'] = {};
    for (const [key, dir] of Object.entries(categoryDirs)) {
      if (typeof dir === 'string' && dir.trim().length > 0) {
        category_dirs[key] = dir;
      }
    }
    if (current.torrentsDir.trim().length > 0) {
      category_dirs[TORRENTS_KEY] = current.torrentsDir.trim();
    }
    const organize_root = current.organizeRoot.trim();
    const global_speed_limit = current.speedLimitBps !== null && current.speedLimitBps > 0 ? current.speedLimitBps : null;
    // a manual proxy without an address is not applied (the engine would refuse it)
    const proxy: EngineSettings['proxy'] =
      current.proxyMode === 'manual' && current.proxyUrl.trim() === '' ? known.proxy : { mode: current.proxyMode, url: current.proxyUrl.trim() };
    const torrent: EngineSettings['torrent'] = {
      seeding: current.torrentSeeding,
      seed_ratio_percent: Math.max(0, Math.round(current.torrentRatioPercent)),
      seed_minutes: Math.max(0, Math.round(current.torrentMinutes)),
      upload_limit: current.torrentUploadBps !== null && current.torrentUploadBps > 0 ? current.torrentUploadBps : null,
      dht: current.torrentDht,
      listen_port: Math.min(65535, Math.max(0, Math.round(current.torrentPort))),
    };
    if (
      JSON.stringify(known.torrent ?? null) === JSON.stringify(torrent) &&
      known.default_dir === defaultDir &&
      known.connections === connections &&
      known.per_host === perHost &&
      JSON.stringify(known.category_dirs ?? {}) === JSON.stringify(category_dirs) &&
      (known.organize ?? false) === current.organize &&
      (known.organize_root ?? '') === organize_root &&
      (known.global_speed_limit ?? null) === global_speed_limit &&
      known.proxy.mode === proxy.mode &&
      known.proxy.url === proxy.url &&
      (known.mark_of_the_web ?? true) === current.markDownloads &&
      (known.allow_private_redirects ?? false) === current.allowPrivateRedirects
    ) {
      return;
    }
    const settings: EngineSettings = {
      default_dir: defaultDir,
      connections,
      per_host: perHost,
      max_active: known.max_active,
      user_agent: known.user_agent,
      category_dirs,
      organize: current.organize,
      organize_root,
      global_speed_limit,
      proxy,
      mark_of_the_web: current.markDownloads,
      allow_private_redirects: current.allowPrivateRedirects,
      torrent,
    };
    try {
      await this.invoke<void>('set_engine_settings', { settings });
      this.engine = settings;
    } catch (error: unknown) {
      console.warn('[settings] set_engine_settings failed', error);
    }
  }
}
