import { computed, DestroyRef, inject, Injectable, InjectionToken, NgZone, signal } from '@angular/core';

import { Settings, SettingsService } from '../features/settings/settings.service';
import { I18nService } from '../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { ToastService } from '../ui/toast.service';
import { PREFERENCE_STORE } from './preferences';

/** An available update, as `check_update` reports it (`src-tauri/src/updater.rs`). */
export interface UpdateInfo {
  readonly version: string;
  readonly current_version: string;
  readonly notes: string | null;
  readonly date: string | null;
}

/** Where the update flow stands. */
export type UpdateStatus = 'idle' | 'checking' | 'none' | 'available' | 'installing' | 'error';

/** `localStorage` key of the last automatic or manual check (epoch milliseconds). */
export const LAST_UPDATE_CHECK_KEY = 'riptide.updates.lastCheck';

/** Automatic checks happen at most this often. */
export const AUTO_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/** `localStorage` key of the newest version the user has already been told about. */
export const LAST_ANNOUNCED_KEY = 'riptide.updates.announced';

/** `localStorage` key of the version that ran last (for the one-time "Updated to" toast). */
export const LAST_RUN_VERSION_KEY = 'riptide.updates.lastRunVersion';

/** How often the running app wakes up to ask whether an automatic check is due. */
export const WAKE_INTERVAL_MS = 60 * 60 * 1000;

/** Whether the window is visible and focused; specs provide a fake. */
export const WINDOW_FOCUSED = new InjectionToken<() => boolean>('riptide.window.focused', {
  providedIn: 'root',
  factory: () => () => typeof document !== 'undefined' && document.visibilityState === 'visible' && document.hasFocus(),
});

/** The clock; specs provide a fixed one. */
export const NOW = new InjectionToken<() => number>('riptide.now', {
  providedIn: 'root',
  factory: () => () => Date.now(),
});

/**
 * Self-update. The only network call that is not a download (privacy): nothing happens unless
 * the user clicks "Check for updates", or "check for updates automatically" is on and the last
 * check is more than a day old. Installing always waits for the user's click. Outside Tauri
 * (plain browser, specs without a fake) there is nothing to update and a check reports
 * "up to date".
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly store = inject(PREFERENCE_STORE);
  private readonly settings = inject(SettingsService);
  private readonly now = inject(NOW);
  private readonly i18n = inject(I18nService);
  private readonly toasts = inject(ToastService);
  private readonly focused = inject(WINDOW_FOCUSED);
  private readonly zone = inject(NgZone);
  private readonly destroyRef = inject(DestroyRef);

  readonly status = signal<UpdateStatus>('idle');
  readonly available = signal<UpdateInfo | null>(null);

  /** The version to badge (Settings nav item, "Update available" line), or `null`. */
  readonly badgeVersion = computed(() => this.available()?.version ?? null);

  /** Asks the selected channel for a newer version. Resolves with it, or `null`. */
  async check(channel: Settings['channel'] = this.settings.settings().channel): Promise<UpdateInfo | null> {
    if (this.status() === 'checking' || this.status() === 'installing') {
      return this.available();
    }
    this.status.set('checking');
    this.store.set(LAST_UPDATE_CHECK_KEY, String(this.now()));
    if (!this.tauri) {
      this.available.set(null);
      this.status.set('none');
      return null;
    }
    try {
      const info = await this.invoke<UpdateInfo | null>('check_update', { channel });
      this.available.set(info ?? null);
      this.status.set(info ? 'available' : 'none');
      return info ?? null;
    } catch (error: unknown) {
      console.warn('[updates] check failed', error);
      this.available.set(null);
      this.status.set('error');
      throw error;
    }
  }

  /** Whether an automatic check is due: enabled, in Tauri, and none in the last day. */
  autoCheckDue(): boolean {
    if (!this.tauri || !this.settings.settings().checkUpdates) {
      return false;
    }
    const last = Number(this.store.get(LAST_UPDATE_CHECK_KEY) ?? '0');
    return !Number.isFinite(last) || this.now() - last >= AUTO_CHECK_INTERVAL_MS || this.now() < last;
  }

  /** Runs the automatic check when due; never throws. Resolves with an available update. */
  async autoCheck(): Promise<UpdateInfo | null> {
    if (!this.autoCheckDue()) {
      return null;
    }
    try {
      return await this.check();
    } catch {
      return null;
    }
  }

  /**
   * Runs the automatic check when due (the once-a-day gate decides) and tells the user about a
   * version they have not been told about yet: a toast with "Update and restart" while the
   * window is focused, a system notification while it is hidden or in the background. The
   * same version is never announced twice.
   */
  async checkAndAnnounce(): Promise<void> {
    const info = await this.autoCheck();
    if (info === null || this.store.get(LAST_ANNOUNCED_KEY) === info.version) {
      return;
    }
    this.store.set(LAST_ANNOUNCED_KEY, info.version);
    const text = this.i18n.t('toast.updateAvailable').replace('{version}', info.version);
    if (this.focused()) {
      this.toasts.show('info', text, { label: this.i18n.t('settings.updateRestart'), run: () => void this.installOrToast() }, 15_000);
      return;
    }
    try {
      await this.invoke<void>('notify_update', { title: text, body: this.i18n.t('notify.updateBody') });
    } catch (error: unknown) {
      console.warn('[updates] notification failed', error);
    }
  }

  /**
   * Called once by the app shell: checks now (if due), again every hour (the daily gate keeps
   * it to one request a day) and whenever the window regains focus. Stops with the caller.
   */
  startChecks(): void {
    if (!this.tauri) {
      return;
    }
    void this.checkAndAnnounce();
    const wake = (): void => void this.checkAndAnnounce();
    const timer = this.zone.runOutsideAngular(() => setInterval(wake, WAKE_INTERVAL_MS));
    globalThis.addEventListener('focus', wake);
    this.destroyRef.onDestroy(() => {
      clearInterval(timer);
      globalThis.removeEventListener('focus', wake);
    });
  }

  /**
   * On the first start after the version changed, a one-time "Updated to {version}" toast with a
   * "What's new" action that opens the release page. A fresh install only records the version.
   */
  async announceUpdated(): Promise<void> {
    if (!this.tauri) {
      return;
    }
    let version: string;
    try {
      version = (await this.invoke<{ version: string }>('app_info')).version;
    } catch {
      return;
    }
    const previous = this.store.get(LAST_RUN_VERSION_KEY);
    if (previous === version) {
      return;
    }
    this.store.set(LAST_RUN_VERSION_KEY, version);
    if (previous === null) {
      return;
    }
    this.toasts.show(
      'success',
      this.i18n.t('toast.updated').replace('{version}', version),
      { label: this.i18n.t('toast.whatsNew'), run: () => void this.openReleasePage(version) },
      12_000,
    );
  }

  private async openReleasePage(version: string): Promise<void> {
    try {
      await this.invoke<void>('open_release_page', { version });
    } catch (error: unknown) {
      console.warn('[updates] release page not opened', error);
    }
  }

  /** {@link install}, with a toast when it fails. */
  async installOrToast(): Promise<void> {
    try {
      await this.install();
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.updateFailed'));
    }
  }

  /** Downloads, verifies and installs the update found by the last check, then restarts. */
  async install(): Promise<void> {
    if (this.available() === null || this.status() === 'installing') {
      return;
    }
    this.status.set('installing');
    try {
      await this.invoke<void>('install_update');
    } catch (error: unknown) {
      console.warn('[updates] install failed', error);
      this.status.set('error');
      throw error;
    }
  }
}
