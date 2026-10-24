import { computed, DestroyRef, inject, Injectable, InjectionToken, NgZone, signal } from '@angular/core';

import { Settings, SettingsService } from '../features/settings/settings.service';
import { I18nService } from '../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { ToastService } from '../ui/toast.service';
import { NOW } from './clock';
import { PREFERENCE_STORE } from './preferences';
import { effectiveChannel } from './update-channel';

/** An available update, as `check_update` reports it (`src-tauri/src/updater.rs`). */
export interface UpdateInfo {
  readonly version: string;
  readonly current_version: string;
  readonly notes: string | null;
  readonly date: string | null;
}

/**
 * Where the update flow stands. `ready` is a package that is downloaded and signature-checked
 * and waits for the restart that installs it.
 */
export type UpdateStatus = 'idle' | 'checking' | 'none' | 'available' | 'downloading' | 'ready' | 'installing' | 'error';

/** Why a step failed (`UpdateErrorKind` in `src-tauri/src/updater.rs`), or `unknown`. */
export type UpdateFailureKind =
  | 'offline'
  | 'server'
  | 'manifest'
  | 'signature'
  | 'download'
  | 'install'
  | 'downloads_active'
  | 'no_update'
  | 'unknown';

/** The last failed step, kept across restarts. */
export interface UpdateFailure {
  readonly kind: UpdateFailureKind;
  /** Epoch milliseconds. */
  readonly at: number;
}

/** What `install()` did: it started the installer, or stopped because transfers are running. */
export type InstallOutcome = 'installing' | 'blocked';

/** Package download progress (`update-progress` event). */
export interface UpdateProgress {
  readonly downloaded: number;
  readonly total: number | null;
}

/** `localStorage` key of the last automatic or manual check attempt (epoch milliseconds). */
export const LAST_UPDATE_CHECK_KEY = 'riptide.updates.lastCheck';

/** `localStorage` key of the last check that reached the server and got a valid answer. */
export const LAST_UPDATE_SUCCESS_KEY = 'riptide.updates.lastSuccess';

/** `localStorage` key of the last failure (`{ kind, at }` as JSON); removed by a good check. */
export const LAST_UPDATE_ERROR_KEY = 'riptide.updates.lastError';

/** `localStorage` key of the automatic-check interval in hours. */
export const UPDATE_INTERVAL_KEY = 'riptide.updates.intervalHours';

/** Hours between automatic checks unless the user chose another value. */
export const DEFAULT_CHECK_INTERVAL_HOURS = 6;

/** The intervals the card offers, in hours. */
export const CHECK_INTERVAL_OPTIONS_HOURS: readonly number[] = [1, 6, 12, 24];

/** After a failed check the next automatic one is at most this far away (offline laptops). */
export const RETRY_AFTER_ERROR_MS = 30 * 60 * 1000;

/** The start-up check waits this long, so it never competes with the window opening. */
export const STARTUP_CHECK_DELAY_MS = 20_000;

/** `localStorage` key of the newest version the user has already been told about. */
export const LAST_ANNOUNCED_KEY = 'riptide.updates.announced';

/** `localStorage` key of the version that ran last (for the one-time "Updated to" toast). */
export const LAST_RUN_VERSION_KEY = 'riptide.updates.lastRunVersion';

/** How often the running app wakes up to ask whether an automatic check is due. */
export const WAKE_INTERVAL_MS = 15 * 60 * 1000;

/** How often "install when downloads finish" looks at the running downloads. */
export const IDLE_POLL_MS = 3_000;

/** Whether the window is visible and focused; specs provide a fake. */
export const WINDOW_FOCUSED = new InjectionToken<() => boolean>('riptide.window.focused', {
  providedIn: 'root',
  factory: () => () => typeof document !== 'undefined' && document.visibilityState === 'visible' && document.hasFocus(),
});

export { NOW } from './clock';

/** The failure kind of whatever a rejected `invoke` carried (`{ kind, message }` from Rust). */
export function failureKind(error: unknown): UpdateFailureKind {
  const kind = (error as { kind?: unknown } | null)?.kind;
  const known: readonly string[] = ['offline', 'server', 'manifest', 'signature', 'download', 'install', 'downloads_active', 'no_update'];
  return typeof kind === 'string' && known.includes(kind) ? (kind as UpdateFailureKind) : 'unknown';
}

/**
 * Self-update. The only network call that is not a download (privacy): nothing happens unless
 * the user clicks "Check for updates", or "check for updates automatically" is on (a check
 * shortly after start-up, then every few hours, six by default). Installing always waits for the
 * user and never interrupts running downloads on its own. Outside Tauri (plain browser, specs
 * without a fake) there is nothing to update and a check reports "up to date".
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly listen = inject(TAURI_LISTEN);
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

  /** Package download progress while `status` is `downloading` (or installing a fresh package). */
  readonly progress = signal<UpdateProgress | null>(null);

  /** The running version, once known. */
  readonly currentVersion = signal<string | null>(null);

  /** When the last check that got a valid answer ended (epoch ms), kept across restarts. */
  readonly lastSuccessAt = signal<number | null>(readNumber(this.store.get(LAST_UPDATE_SUCCESS_KEY)));

  /** The last failed step, kept across restarts until a check succeeds. */
  readonly lastError = signal<UpdateFailure | null>(readFailure(this.store.get(LAST_UPDATE_ERROR_KEY)));

  /** Hours between automatic checks. */
  readonly intervalHours = signal<number>(readInterval(this.store.get(UPDATE_INTERVAL_KEY)));

  /** `true` after an install was refused because downloads are running (the card offers a choice). */
  readonly blockedByDownloads = signal(false);

  /** `true` while "install when downloads finish" is armed. */
  readonly installScheduled = signal(false);

  /** The version to badge (Settings nav item, "Update available" line), or `null`. */
  readonly badgeVersion = computed(() => this.available()?.version ?? null);

  private idleTimer: ReturnType<typeof setInterval> | null = null;
  private unlisten: (() => void) | null = null;
  private listening = false;

  constructor() {
    this.destroyRef.onDestroy(() => {
      this.stopIdleWatch();
      this.unlisten?.();
      this.unlisten = null;
    });
  }

  /** Asks the selected channel for a newer version. Resolves with it, or `null`. */
  async check(channel: Settings['channel'] = this.settings.settings().channel): Promise<UpdateInfo | null> {
    const busy = this.status();
    if (busy === 'checking' || busy === 'installing' || busy === 'downloading' || busy === 'ready') {
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
      // a pre-release build only ever finds updates on the beta feed, whatever was saved
      const version = await this.runningVersion();
      this.currentVersion.set(version);
      const channelToAsk = effectiveChannel(channel, version);
      const info = await this.invoke<UpdateInfo | null>('check_update', { channel: channelToAsk });
      this.available.set(info ?? null);
      this.status.set(info ? 'available' : 'none');
      this.recordSuccess();
      return info ?? null;
    } catch (error: unknown) {
      console.warn('[updates] check failed', error);
      this.available.set(null);
      this.recordFailure(error);
      this.status.set('error');
      throw error;
    }
  }

  /** The running version, `null` when the backend cannot say. */
  private async runningVersion(): Promise<string | null> {
    try {
      return (await this.invoke<{ version: string }>('app_info')).version;
    } catch {
      return null;
    }
  }

  /** Loads the running version for display. Never throws. */
  async refreshVersion(): Promise<void> {
    if (this.tauri && this.currentVersion() === null) {
      this.currentVersion.set(await this.runningVersion());
    }
  }

  private recordSuccess(): void {
    const at = this.now();
    this.lastSuccessAt.set(at);
    this.store.set(LAST_UPDATE_SUCCESS_KEY, String(at));
    this.clearFailure();
  }

  private clearFailure(): void {
    this.lastError.set(null);
    this.store.set(LAST_UPDATE_ERROR_KEY, '');
  }

  private recordFailure(error: unknown): void {
    const failure: UpdateFailure = { kind: failureKind(error), at: this.now() };
    this.lastError.set(failure);
    this.store.set(LAST_UPDATE_ERROR_KEY, JSON.stringify(failure));
  }

  /** Sets (and remembers) the hours between automatic checks; unknown values are ignored. */
  setIntervalHours(hours: number): void {
    if (!CHECK_INTERVAL_OPTIONS_HOURS.includes(hours)) {
      return;
    }
    this.intervalHours.set(hours);
    this.store.set(UPDATE_INTERVAL_KEY, String(hours));
  }

  /**
   * Whether an automatic check is due: enabled and in Tauri, and either this is the start-up
   * check or the interval has passed (30 minutes at most after a failed check).
   */
  autoCheckDue(startup = false): boolean {
    if (!this.tauri || !this.settings.settings().checkUpdates) {
      return false;
    }
    if (startup) {
      return true;
    }
    const last = Number(this.store.get(LAST_UPDATE_CHECK_KEY) ?? '0');
    if (!Number.isFinite(last) || this.now() < last) {
      return true;
    }
    const failedSince = this.lastError() !== null;
    const wait = failedSince ? Math.min(this.intervalHours() * 3_600_000, RETRY_AFTER_ERROR_MS) : this.intervalHours() * 3_600_000;
    return this.now() - last >= wait;
  }

  /** Runs the automatic check when due; never throws. Resolves with an available update. */
  async autoCheck(startup = false): Promise<UpdateInfo | null> {
    if (!this.autoCheckDue(startup)) {
      return null;
    }
    try {
      return await this.check();
    } catch {
      return null;
    }
  }

  /**
   * Runs the automatic check when due and tells the user about a version they have not been
   * told about yet: a toast with "Update and restart" while the window is focused, a system
   * notification while it is hidden or in the background. The same version is never announced
   * twice.
   */
  async checkAndAnnounce(startup = false): Promise<void> {
    const info = await this.autoCheck(startup);
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
   * Called once by the app shell: one check a short while after start (never in the way of the
   * window opening), then whenever the interval has passed (the shell wakes every 15 minutes)
   * and whenever the window regains focus. Stops with the caller.
   */
  startChecks(): void {
    if (!this.tauri) {
      return;
    }
    const first = this.zone.runOutsideAngular(() => setTimeout(() => void this.checkAndAnnounce(true), STARTUP_CHECK_DELAY_MS));
    const wake = (): void => void this.checkAndAnnounce();
    const timer = this.zone.runOutsideAngular(() => setInterval(wake, WAKE_INTERVAL_MS));
    globalThis.addEventListener('focus', wake);
    this.destroyRef.onDestroy(() => {
      clearTimeout(first);
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
    this.currentVersion.set(version);
    const previous = this.store.get(LAST_RUN_VERSION_KEY);
    if (previous === version) {
      return;
    }
    this.store.set(LAST_RUN_VERSION_KEY, version);
    if (previous === null) {
      return;
    }
    // what was announced or failed before the restart is history now
    this.available.set(null);
    this.status.set('idle');
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

  /**
   * {@link install} from a toast: when downloads are running it arms "install when downloads
   * finish" instead of interrupting them, and says so; any other failure shows a toast.
   */
  async installOrToast(): Promise<void> {
    try {
      if ((await this.install()) === 'blocked') {
        this.installWhenIdle();
        this.toasts.show('info', this.i18n.t('toast.updateWaiting'));
      }
    } catch {
      this.toasts.show('danger', this.i18n.t(`toast.updateFailed.${this.lastError()?.kind ?? 'unknown'}`));
    }
  }

  /** Subscribes to the package download progress (once; failures are not fatal). */
  private async listenForProgress(): Promise<void> {
    if (!this.tauri || this.listening) {
      return;
    }
    this.listening = true;
    try {
      this.unlisten = await this.listen<UpdateProgress>('update-progress', (p) => this.zone.run(() => this.progress.set(p)));
    } catch (error: unknown) {
      this.listening = false;
      console.warn('[updates] progress events unavailable', error);
    }
  }

  /**
   * Downloads the package of the update found by the last check and verifies its signature
   * (nothing is installed). On failure the update stays available and `status` is `error`, so
   * {@link retry} can run it again.
   */
  async download(): Promise<void> {
    if (this.available() === null || this.status() === 'downloading' || this.status() === 'installing' || this.status() === 'ready') {
      return;
    }
    this.status.set('downloading');
    this.progress.set(null);
    void this.listenForProgress();
    try {
      await this.invoke<void>('download_update');
      this.clearFailure();
      this.status.set('ready');
    } catch (error: unknown) {
      console.warn('[updates] download failed', error);
      this.recordFailure(error);
      this.status.set('error');
      throw error;
    }
  }

  /**
   * Installs the update found by the last check and restarts. With downloads running and
   * `interruptDownloads` false nothing is touched: it resolves `blocked` and
   * `blockedByDownloads` is set, so the UI can offer "when downloads finish" or "now". With
   * `interruptDownloads` the engine stops at a checkpoint and the downloads resume after the
   * restart. Other failures throw (`status` is `error`).
   */
  async install(interruptDownloads = false): Promise<InstallOutcome> {
    if (this.available() === null || this.status() === 'installing') {
      return 'installing';
    }
    const before = this.status();
    this.status.set('installing');
    this.blockedByDownloads.set(false);
    void this.listenForProgress();
    try {
      await this.invoke<void>('install_update', { interruptDownloads });
      return 'installing';
    } catch (error: unknown) {
      if (failureKind(error) === 'downloads_active') {
        this.status.set(before === 'installing' ? 'available' : before);
        this.blockedByDownloads.set(true);
        return 'blocked';
      }
      console.warn('[updates] install failed', error);
      this.recordFailure(error);
      this.status.set('error');
      throw error;
    }
  }

  /**
   * Arms "install when downloads finish": the package is fetched now (a download of a few
   * megabytes), and the installer starts as soon as no download is queued, running or being
   * verified. Does nothing without an available update.
   */
  installWhenIdle(): void {
    if (this.available() === null || this.installScheduled()) {
      return;
    }
    this.installScheduled.set(true);
    this.blockedByDownloads.set(false);
    void this.download().catch(() => this.cancelScheduledInstall());
    this.zone.runOutsideAngular(() => {
      this.idleTimer = setInterval(() => void this.installIfIdle(), IDLE_POLL_MS);
    });
  }

  /** Disarms "install when downloads finish". */
  cancelScheduledInstall(): void {
    this.installScheduled.set(false);
    this.stopIdleWatch();
  }

  private stopIdleWatch(): void {
    if (this.idleTimer !== null) {
      clearInterval(this.idleTimer);
      this.idleTimer = null;
    }
  }

  private async installIfIdle(): Promise<void> {
    if (!this.installScheduled() || this.status() === 'downloading' || this.status() === 'installing') {
      return;
    }
    let active: number;
    try {
      active = await this.invoke<number>('active_download_count');
    } catch {
      return;
    }
    if (active > 0 || !this.installScheduled()) {
      return;
    }
    this.cancelScheduledInstall();
    try {
      await this.install(false);
    } catch {
      // `status` and `lastError` already say what happened; the card offers Retry
    }
  }

  /** After an error: runs the failed step again (a failed check checks again). */
  async retry(): Promise<void> {
    if (this.status() !== 'error') {
      return;
    }
    if (this.available() === null) {
      await this.check().catch(() => undefined);
      return;
    }
    this.status.set('available');
    try {
      await this.download();
    } catch {
      // recorded by `download`
    }
  }
}

function readNumber(raw: string | null): number | null {
  const n = Number(raw);
  return raw !== null && raw !== '' && Number.isFinite(n) ? n : null;
}

function readFailure(raw: string | null): UpdateFailure | null {
  if (!raw) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as { kind?: unknown; at?: unknown };
    return typeof parsed.at === 'number' ? { kind: failureKind(parsed), at: parsed.at } : null;
  } catch {
    return null;
  }
}

function readInterval(raw: string | null): number {
  const n = Number(raw);
  return CHECK_INTERVAL_OPTIONS_HOURS.includes(n) ? n : DEFAULT_CHECK_INTERVAL_HOURS;
}
