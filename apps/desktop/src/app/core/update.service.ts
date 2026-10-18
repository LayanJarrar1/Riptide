import { inject, Injectable, InjectionToken, signal } from '@angular/core';

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

  readonly status = signal<UpdateStatus>('idle');
  readonly available = signal<UpdateInfo | null>(null);

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
   * The start-up check (at most once a day, only while enabled): an available update shows a
   * toast with "Update and restart". Called once by the app shell.
   */
  async startupCheck(): Promise<void> {
    const info = await this.autoCheck();
    if (info !== null) {
      this.toasts.show(
        'info',
        this.i18n.t('toast.updateAvailable').replace('{version}', info.version),
        { label: this.i18n.t('settings.updateRestart'), run: () => void this.installOrToast() },
        15_000,
      );
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
