import { TestBed } from '@angular/core/testing';

import { loadTable } from '../i18n/i18n.service';
import { SETTINGS_STORAGE_KEY } from '../features/settings/settings.service';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { ToastService } from '../ui/toast.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from './preferences';
import {
  DEFAULT_CHECK_INTERVAL_HOURS,
  IDLE_POLL_MS,
  LAST_ANNOUNCED_KEY,
  LAST_RUN_VERSION_KEY,
  LAST_UPDATE_CHECK_KEY,
  LAST_UPDATE_ERROR_KEY,
  LAST_UPDATE_SUCCESS_KEY,
  NOW,
  RETRY_AFTER_ERROR_MS,
  STARTUP_CHECK_DELAY_MS,
  UPDATE_INTERVAL_KEY,
  UpdateInfo,
  UpdateService,
  WAKE_INTERVAL_MS,
  WINDOW_FOCUSED,
  failureKind,
} from './update.service';

// the translated sentences these specs check must not depend on which spec ran before
beforeAll(async () => {
  await loadTable('en');
});

const T0 = 1_800_000_000_000;
const HOUR = 3_600_000;
const UPDATE: UpdateInfo = { version: '0.1.0-alpha.3', current_version: '0.1.0-alpha.2', notes: 'Faster.', date: null };

/** A rejection the way the Rust side sends it. */
const failure = (kind: string): { kind: string; message: string } => ({ kind, message: `technical detail (${kind})` });

describe('UpdateService', () => {
  let calls: { cmd: string; args?: Record<string, unknown> }[];
  let answer: UpdateInfo | null;
  /** Per command: what to reject with (consumed one per call when an array). */
  let failing: Record<string, unknown[]>;
  let activeDownloads: number;
  let now: number;
  let focused: boolean;
  let appVersion: string;
  let store: MemoryPreferenceStore;
  let emit: (event: string, payload: unknown) => void;

  function configure(tauri: boolean, prefs: Record<string, string> = {}): UpdateService {
    calls = [];
    answer = UPDATE;
    failing = {};
    activeDownloads = 0;
    now = T0;
    focused = true;
    appVersion = '0.1.0-alpha.3';
    store = new MemoryPreferenceStore(prefs);
    const handlers = new Map<string, ((p: unknown) => void)[]>();
    emit = (event, payload) => (handlers.get(event) ?? []).forEach((h) => h(payload));
    const listen: ListenFn = <T>(event: string, handler: (payload: T) => void): Promise<() => void> => {
      handlers.set(event, [...(handlers.get(event) ?? []), handler as (p: unknown) => void]);
      return Promise.resolve(() => undefined);
    };
    const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      if (cmd === 'get_engine_settings') {
        return Promise.resolve({ default_dir: 'D:\\x', connections: 8, per_host: 4, max_active: 3, user_agent: null, category_dirs: {} } as unknown as T);
      }
      if (cmd === 'app_info') {
        return Promise.resolve({ name: 'Riptide', version: appVersion, build: 'x' } as unknown as T);
      }
      const queue = failing[cmd];
      if (queue && queue.length > 0) {
        return Promise.reject(queue.length > 1 ? queue.shift() : queue[0]);
      }
      if (cmd === 'active_download_count') {
        return Promise.resolve(activeDownloads as unknown as T);
      }
      return Promise.resolve((cmd === 'check_update' ? answer : undefined) as unknown as T);
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: store },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        { provide: TAURI_INVOKE, useValue: invoke },
        { provide: TAURI_LISTEN, useValue: listen },
        { provide: NOW, useValue: () => now },
        { provide: WINDOW_FOCUSED, useValue: () => focused },
      ],
    });
    return TestBed.inject(UpdateService);
  }

  const UPDATE_COMMANDS = ['check_update', 'download_update', 'install_update', 'notify_update', 'open_release_page'];
  const updateCalls = (): string[] => calls.map((c) => c.cmd).filter((c) => UPDATE_COMMANDS.includes(c));

  /** Lets chained promises (and fake-clock callbacks they started) finish. */
  async function settle(): Promise<void> {
    for (let i = 0; i < 20; i++) {
      await Promise.resolve();
    }
  }

  describe('check', () => {
    it('checks the beta channel by default', async () => {
      const updates = configure(true);
      await updates.check();
      expect(calls.find((c) => c.cmd === 'check_update')?.args).toEqual({ channel: 'beta' });
    });

    it('sends a pre-release build to the beta feed even when stable is saved', async () => {
      const updates = configure(true);
      appVersion = '0.1.0-alpha.4';
      await updates.check('stable');
      expect(calls.find((c) => c.cmd === 'check_update')?.args).toEqual({ channel: 'beta' });
    });

    it('lets a stable build keep the saved channel', async () => {
      const updates = configure(true);
      appVersion = '1.0.0';
      await updates.check('stable');
      expect(calls.find((c) => c.cmd === 'check_update')?.args).toEqual({ channel: 'stable' });
    });

    it('keeps what a newer release brought and remembers the successful check', async () => {
      const updates = configure(true);
      const found = await updates.check('beta');
      expect(found).toEqual(UPDATE);
      expect(updates.status()).toBe('available');
      expect(updates.available()).toEqual(UPDATE);
      expect(updates.currentVersion()).toBe('0.1.0-alpha.3');
      expect(store.get(LAST_UPDATE_CHECK_KEY)).toBe(String(T0));
      expect(updates.lastSuccessAt()).toBe(T0);
      expect(store.get(LAST_UPDATE_SUCCESS_KEY)).toBe(String(T0));
    });

    it('reports "no update" as up to date', async () => {
      const updates = configure(true);
      answer = null;
      expect(await updates.check()).toBeNull();
      expect(updates.status()).toBe('none');
      expect(updates.available()).toBeNull();
      expect(updates.lastError()).toBeNull();
    });

    it('never touches the network outside Tauri', async () => {
      const updates = configure(false);
      expect(await updates.check()).toBeNull();
      expect(updates.autoCheckDue()).toBeFalse();
      expect(updates.autoCheckDue(true)).toBeFalse();
      expect(updateCalls()).toEqual([]);
    });

    it('exposes the badge while an update is available', async () => {
      const updates = configure(true);
      expect(updates.badgeVersion()).toBeNull();
      await updates.check();
      expect(updates.badgeVersion()).toBe('0.1.0-alpha.3');
      updates.status.set('idle');
      answer = null;
      await updates.check();
      expect(updates.badgeVersion()).toBeNull();
    });

    for (const kind of ['offline', 'server', 'manifest']) {
      it(`records a failed check (${kind}) with its kind and keeps it across a restart`, async () => {
        const updates = configure(true);
        failing['check_update'] = [failure(kind)];
        await expectAsync(updates.check()).toBeRejected();
        expect(updates.status()).toBe('error');
        expect(updates.available()).toBeNull();
        expect(updates.lastError()).toEqual({ kind: kind as never, at: T0 });
        expect(updates.lastSuccessAt()).toBeNull();

        // a new session (same storage) still knows what happened
        TestBed.resetTestingModule();
        const prefs = { [LAST_UPDATE_ERROR_KEY]: store.get(LAST_UPDATE_ERROR_KEY) ?? '' };
        const next = configure(true, prefs);
        expect(next.lastError()).toEqual({ kind: kind as never, at: T0 });
      });
    }

    it('clears the last error after a good check and keeps the time of the last success', async () => {
      const updates = configure(true);
      failing['check_update'] = [failure('offline')];
      await expectAsync(updates.check()).toBeRejected();
      expect(updates.lastError()?.kind).toBe('offline');
      failing = {};
      now = T0 + 5 * 60_000;
      await updates.check();
      expect(updates.lastError()).toBeNull();
      expect(updates.lastSuccessAt()).toBe(T0 + 5 * 60_000);
      expect(store.get(LAST_UPDATE_ERROR_KEY)).toBe('');
    });

    it('reads unknown rejections as the unknown kind', () => {
      expect(failureKind('boom')).toBe('unknown');
      expect(failureKind(new Error('x'))).toBe('unknown');
      expect(failureKind(null)).toBe('unknown');
      expect(failureKind({ kind: 'weird' })).toBe('unknown');
      expect(failureKind({ kind: 'signature' })).toBe('signature');
    });
  });

  describe('automatic checks', () => {
    afterEach(() => jasmine.clock().uninstall());

    it('defaults to every six hours and remembers a chosen interval', () => {
      const updates = configure(true);
      expect(DEFAULT_CHECK_INTERVAL_HOURS).toBe(6);
      expect(updates.intervalHours()).toBe(6);
      updates.setIntervalHours(12);
      expect(store.get(UPDATE_INTERVAL_KEY)).toBe('12');
      updates.setIntervalHours(7); // not offered: ignored
      expect(updates.intervalHours()).toBe(12);
      TestBed.resetTestingModule();
      expect(configure(true, { [UPDATE_INTERVAL_KEY]: '12' }).intervalHours()).toBe(12);
      TestBed.resetTestingModule();
      expect(configure(true, { [UPDATE_INTERVAL_KEY]: '5' }).intervalHours()).toBe(6);
    });

    it('checks when the interval has passed, not before', async () => {
      const updates = configure(true);
      expect(updates.autoCheckDue()).toBeTrue();
      await updates.autoCheck();
      expect(updateCalls()).toEqual(['check_update']);
      updates.status.set('idle');
      now = T0 + 6 * HOUR - 1;
      expect(await updates.autoCheck()).toBeNull();
      expect(updateCalls()).toEqual(['check_update']);
      now = T0 + 6 * HOUR;
      expect(updates.autoCheckDue()).toBeTrue();
    });

    it('follows the chosen interval', async () => {
      const updates = configure(true);
      updates.setIntervalHours(24);
      await updates.autoCheck();
      now = T0 + 23 * HOUR;
      expect(updates.autoCheckDue()).toBeFalse();
      now = T0 + 24 * HOUR;
      expect(updates.autoCheckDue()).toBeTrue();
    });

    it('tries again sooner after a failed check (offline laptops)', async () => {
      const updates = configure(true);
      failing['check_update'] = [failure('offline')];
      await updates.autoCheck();
      now = T0 + RETRY_AFTER_ERROR_MS - 1;
      expect(updates.autoCheckDue()).toBeFalse();
      now = T0 + RETRY_AFTER_ERROR_MS;
      expect(updates.autoCheckDue()).toBeTrue();
    });

    it('does not check automatically when switched off, not even at start-up', async () => {
      const updates = configure(true, { [SETTINGS_STORAGE_KEY]: JSON.stringify({ checkUpdates: false }) });
      expect(updates.autoCheckDue()).toBeFalse();
      expect(updates.autoCheckDue(true)).toBeFalse();
      await updates.checkAndAnnounce(true);
      expect(updateCalls()).toEqual([]);
    });

    it('checks shortly after start, never during start, and again when the interval passed', async () => {
      jasmine.clock().install();
      const updates = configure(true);
      updates.startChecks();
      await settle();
      expect(updateCalls()).toEqual([]); // the window opens first
      jasmine.clock().tick(STARTUP_CHECK_DELAY_MS - 1);
      await settle();
      expect(updateCalls()).toEqual([]);
      jasmine.clock().tick(1);
      await settle();
      expect(updateCalls()).toEqual(['check_update']);

      // woken every 15 minutes: nothing is due until the interval has passed
      updates.status.set('idle');
      for (let i = 0; i < 4; i++) {
        now += WAKE_INTERVAL_MS;
        jasmine.clock().tick(WAKE_INTERVAL_MS);
      }
      await settle();
      expect(updateCalls()).toEqual(['check_update']);
      now = T0 + 6 * HOUR;
      jasmine.clock().tick(WAKE_INTERVAL_MS);
      await settle();
      expect(updateCalls()).toEqual(['check_update', 'check_update']);
    });

    it('checks at every start, whatever the interval says', async () => {
      const updates = configure(true, { [LAST_UPDATE_CHECK_KEY]: String(T0 - 60_000) });
      expect(updates.autoCheckDue()).toBeFalse();
      expect(updates.autoCheckDue(true)).toBeTrue();
    });

    it('offers the update on start-up and installs only on request', async () => {
      const updates = configure(true);
      const toasts = TestBed.inject(ToastService);
      await updates.checkAndAnnounce(true);
      expect(updateCalls()).toEqual(['check_update']);
      const toast = toasts.toasts()[0];
      expect(toast.text).toContain('0.1.0-alpha.3');
      expect(toast.actions.length).toBe(1);
      toast.actions[0].run();
      await settle();
      expect(updateCalls()).toEqual(['check_update', 'install_update']);
      expect(calls.find((c) => c.cmd === 'install_update')?.args).toEqual({ interruptDownloads: false });
    });

    it('sends a system notification instead of a toast while the window is not focused', async () => {
      const updates = configure(true);
      focused = false;
      const toasts = TestBed.inject(ToastService);
      await updates.checkAndAnnounce(true);
      expect(toasts.toasts().length).toBe(0);
      const note = calls.find((c) => c.cmd === 'notify_update');
      expect(note?.args).toEqual({ title: 'Riptide 0.1.0-alpha.3 is available', body: 'Open Riptide to update' });
      expect(store.get(LAST_ANNOUNCED_KEY)).toBe('0.1.0-alpha.3');
    });

    it('announces each version once, and a newer one again', async () => {
      const updates = configure(true);
      focused = false;
      await updates.checkAndAnnounce(true);
      updates.status.set('idle');
      await updates.checkAndAnnounce(true);
      updates.status.set('idle');
      focused = true;
      await updates.checkAndAnnounce(true);
      expect(TestBed.inject(ToastService).toasts().length).toBe(0);
      expect(calls.filter((c) => c.cmd === 'notify_update').length).toBe(1);
      expect(updateCalls().filter((c) => c === 'check_update').length).toBe(3);
      updates.status.set('idle');
      answer = { ...UPDATE, version: '0.1.0-alpha.4' };
      await updates.checkAndAnnounce(true);
      expect(TestBed.inject(ToastService).toasts().length).toBe(1);
    });

    it('does not announce a version already announced in an earlier session', async () => {
      const updates = configure(true, { [LAST_ANNOUNCED_KEY]: '0.1.0-alpha.3' });
      focused = false;
      await updates.checkAndAnnounce(true);
      expect(calls.some((c) => c.cmd === 'notify_update')).toBeFalse();
    });

    it('makes no extra network call when woken again within the interval', async () => {
      const updates = configure(true);
      await updates.checkAndAnnounce();
      updates.status.set('idle');
      now += HOUR;
      await updates.checkAndAnnounce();
      await updates.checkAndAnnounce();
      expect(updateCalls()).toEqual(['check_update']);
    });
  });

  describe('download, verification and install', () => {
    async function withUpdate(): Promise<UpdateService> {
      const updates = configure(true);
      await updates.check();
      return updates;
    }

    it('downloads the package, shows progress and ends "downloaded and verified, restart to finish"', async () => {
      const updates = await withUpdate();
      const pending = updates.download();
      expect(updates.status()).toBe('downloading');
      await settle();
      emit('update-progress', { downloaded: 500, total: 1000 });
      expect(updates.progress()).toEqual({ downloaded: 500, total: 1000 });
      await pending;
      expect(updates.status()).toBe('ready'); // restart required to install
      expect(updateCalls()).toEqual(['check_update', 'download_update']);
      // a ready package is not downloaded again
      await updates.download();
      expect(updateCalls()).toEqual(['check_update', 'download_update']);
    });

    it('refuses an unsigned or tampered package: explicit error, update kept, nothing installed', async () => {
      const updates = await withUpdate();
      failing['download_update'] = [failure('signature')];
      await expectAsync(updates.download()).toBeRejected();
      expect(updates.status()).toBe('error');
      expect(updates.lastError()?.kind).toBe('signature');
      expect(updates.available()).toEqual(UPDATE);
      expect(updateCalls()).not.toContain('install_update');
    });

    it('treats a corrupted package (signature mismatch during the install) as an error too', async () => {
      const updates = await withUpdate();
      failing['install_update'] = [failure('signature')];
      await expectAsync(updates.install()).toBeRejected();
      expect(updates.status()).toBe('error');
      expect(updates.lastError()?.kind).toBe('signature');
      expect(updates.available()).toEqual(UPDATE);
    });

    it('recovers from an interrupted download with a retry', async () => {
      const updates = await withUpdate();
      failing['download_update'] = [failure('download')];
      await expectAsync(updates.download()).toBeRejected();
      expect(updates.status()).toBe('error');
      expect(updates.lastError()?.kind).toBe('download');
      failing = {};
      await updates.retry();
      expect(updates.status()).toBe('ready');
      expect(updates.lastError()).toBeNull();
    });

    it('retries a failed check by checking again', async () => {
      const updates = configure(true);
      failing['check_update'] = [failure('server')];
      await expectAsync(updates.check()).toBeRejected();
      failing = {};
      await updates.retry();
      expect(updates.status()).toBe('available');
    });

    it('starts the installer with the downloads untouched when nothing is running', async () => {
      const updates = await withUpdate();
      expect(await updates.install()).toBe('installing');
      expect(updates.status()).toBe('installing');
      expect(calls.find((c) => c.cmd === 'install_update')?.args).toEqual({ interruptDownloads: false });
    });

    it('does nothing without an available update', async () => {
      const updates = configure(true);
      await updates.install();
      await updates.download();
      expect(updateCalls()).toEqual([]);
    });
  });

  describe('downloads in progress', () => {
    afterEach(() => jasmine.clock().uninstall());

    async function blocked(): Promise<UpdateService> {
      const updates = configure(true);
      await updates.check();
      failing['install_update'] = [failure('downloads_active')];
      return updates;
    }

    it('never installs over running downloads: it stops and offers the choice', async () => {
      const updates = await blocked();
      expect(await updates.install()).toBe('blocked');
      expect(updates.blockedByDownloads()).toBeTrue();
      expect(updates.status()).toBe('available');
      expect(updates.lastError()).toBeNull(); // not a failure
    });

    it('"install now" interrupts the downloads at a checkpoint on request only', async () => {
      const updates = await blocked();
      await updates.install();
      failing = {};
      await updates.install(true);
      const installs = calls.filter((c) => c.cmd === 'install_update').map((c) => c.args);
      expect(installs).toEqual([{ interruptDownloads: false }, { interruptDownloads: true }]);
      expect(updates.status()).toBe('installing');
      expect(updates.blockedByDownloads()).toBeFalse();
    });

    it('"install when downloads finish" downloads now and installs once nothing is running', async () => {
      jasmine.clock().install();
      const updates = await blocked();
      await updates.install();
      failing = {};
      activeDownloads = 2;
      updates.installWhenIdle();
      expect(updates.installScheduled()).toBeTrue();
      await settle();
      expect(updateCalls()).toContain('download_update');
      expect(updates.status()).toBe('ready');
      for (let i = 0; i < 3; i++) {
        jasmine.clock().tick(IDLE_POLL_MS);
        await settle();
      }
      expect(calls.filter((c) => c.cmd === 'install_update').length).toBe(1); // only the refused one
      expect(updates.installScheduled()).toBeTrue();

      activeDownloads = 0;
      jasmine.clock().tick(IDLE_POLL_MS);
      await settle();
      const installs = calls.filter((c) => c.cmd === 'install_update').map((c) => c.args);
      expect(installs).toEqual([{ interruptDownloads: false }, { interruptDownloads: false }]);
      expect(updates.installScheduled()).toBeFalse();
      expect(updates.status()).toBe('installing');
    });

    it('can be cancelled', async () => {
      jasmine.clock().install();
      const updates = await blocked();
      activeDownloads = 1;
      updates.installWhenIdle();
      await settle();
      updates.cancelScheduledInstall();
      activeDownloads = 0;
      jasmine.clock().tick(IDLE_POLL_MS * 3);
      await settle();
      expect(calls.some((c) => c.cmd === 'install_update')).toBeFalse();
      expect(updates.installScheduled()).toBeFalse();
    });

    it('drops the schedule when the download fails', async () => {
      jasmine.clock().install();
      const updates = await blocked();
      failing['download_update'] = [failure('offline')];
      updates.installWhenIdle();
      await settle();
      expect(updates.installScheduled()).toBeFalse();
      expect(updates.status()).toBe('error');
    });

    it('a toast install with downloads running arms the wait instead of interrupting', async () => {
      jasmine.clock().install();
      const updates = await blocked();
      activeDownloads = 1;
      await updates.installOrToast();
      expect(updates.installScheduled()).toBeTrue();
      expect(TestBed.inject(ToastService).toasts()[0].text).toBe('The update will install when your downloads finish');
      updates.cancelScheduledInstall();
    });
  });

  describe('after the install', () => {
    it("shows 'Updated to' once after the version changed, with a What's new action", async () => {
      const updates = configure(true, { [LAST_RUN_VERSION_KEY]: '0.1.0-alpha.2' });
      const toasts = TestBed.inject(ToastService);
      await updates.announceUpdated();
      expect(toasts.toasts().length).toBe(1);
      expect(toasts.toasts()[0].text).toContain('0.1.0-alpha.3');
      expect(updates.currentVersion()).toBe('0.1.0-alpha.3');
      toasts.toasts()[0].actions[0].run();
      await settle();
      expect(calls.find((c) => c.cmd === 'open_release_page')?.args).toEqual({ version: '0.1.0-alpha.3' });
      expect(store.get(LAST_RUN_VERSION_KEY)).toBe('0.1.0-alpha.3');
      const before = toasts.toasts().length;
      await updates.announceUpdated();
      expect(toasts.toasts().length).toBe(before);
    });

    it('stays quiet on a fresh install but still knows the version', async () => {
      const updates = configure(true);
      await updates.announceUpdated();
      expect(TestBed.inject(ToastService).toasts().length).toBe(0);
      expect(store.get(LAST_RUN_VERSION_KEY)).toBe('0.1.0-alpha.3');
      expect(updates.currentVersion()).toBe('0.1.0-alpha.3');
    });
  });
});
