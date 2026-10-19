import { TestBed } from '@angular/core/testing';

import { SETTINGS_STORAGE_KEY } from '../features/settings/settings.service';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { ToastService } from '../ui/toast.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from './preferences';
import {
  AUTO_CHECK_INTERVAL_MS,
  LAST_ANNOUNCED_KEY,
  LAST_RUN_VERSION_KEY,
  LAST_UPDATE_CHECK_KEY,
  NOW,
  UpdateInfo,
  UpdateService,
  WINDOW_FOCUSED,
} from './update.service';

const T0 = 1_800_000_000_000;
const UPDATE: UpdateInfo = { version: '0.1.0-alpha.3', current_version: '0.1.0-alpha.2', notes: 'Faster.', date: null };

describe('UpdateService', () => {
  let calls: { cmd: string; args?: Record<string, unknown> }[];
  let answer: UpdateInfo | null;
  let fail: boolean;
  let now: number;
  let focused: boolean;
  let appVersion: string;
  let store: MemoryPreferenceStore;

  function configure(tauri: boolean, prefs: Record<string, string> = {}): UpdateService {
    calls = [];
    answer = UPDATE;
    fail = false;
    now = T0;
    focused = true;
    appVersion = '0.1.0-alpha.3';
    store = new MemoryPreferenceStore(prefs);
    const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      if (cmd === 'get_engine_settings') {
        return Promise.resolve({ default_dir: 'D:\\x', connections: 8, per_host: 4, max_active: 3, user_agent: null, category_dirs: {} } as unknown as T);
      }
      if (cmd === 'app_info') {
        return Promise.resolve({ name: 'Riptide', version: appVersion, build: 'x' } as unknown as T);
      }
      if (fail) {
        return Promise.reject(new Error('offline'));
      }
      return Promise.resolve((cmd === 'check_update' ? answer : undefined) as unknown as T);
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: store },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        { provide: TAURI_INVOKE, useValue: invoke },
        { provide: NOW, useValue: () => now },
        { provide: WINDOW_FOCUSED, useValue: () => focused },
      ],
    });
    return TestBed.inject(UpdateService);
  }

  const updateCalls = (): string[] => calls.map((c) => c.cmd).filter((c) => c !== 'get_engine_settings');

  it('checks the beta channel by default', async () => {
    const updates = configure(true);
    await updates.check();
    expect(calls.find((c) => c.cmd === 'check_update')?.args).toEqual({ channel: 'beta' });
  });

  it('checks the selected channel and keeps what it found', async () => {
    const updates = configure(true);
    const found = await updates.check('beta');
    expect(found).toEqual(UPDATE);
    expect(calls.find((c) => c.cmd === 'check_update')?.args).toEqual({ channel: 'beta' });
    expect(updates.status()).toBe('available');
    expect(updates.available()).toEqual(UPDATE);
    expect(store.get(LAST_UPDATE_CHECK_KEY)).toBe(String(T0));
  });

  it('reports up to date and errors', async () => {
    const updates = configure(true);
    answer = null;
    expect(await updates.check()).toBeNull();
    expect(updates.status()).toBe('none');
    fail = true;
    await expectAsync(updates.check()).toBeRejected();
    expect(updates.status()).toBe('error');
  });

  it('never touches the network outside Tauri', async () => {
    const updates = configure(false);
    expect(await updates.check()).toBeNull();
    expect(updates.autoCheckDue()).toBeFalse();
    expect(updateCalls()).toEqual([]);
  });

  it('checks automatically at most once a day', async () => {
    const updates = configure(true);
    expect(updates.autoCheckDue()).toBeTrue();
    await updates.autoCheck();
    expect(updateCalls()).toEqual(['check_update']);
    now = T0 + AUTO_CHECK_INTERVAL_MS - 1;
    expect(await updates.autoCheck()).toBeNull();
    expect(updateCalls()).toEqual(['check_update']);
    now = T0 + AUTO_CHECK_INTERVAL_MS;
    expect(updates.autoCheckDue()).toBeTrue();
  });

  it('does not check automatically when switched off', async () => {
    const updates = configure(true, { [SETTINGS_STORAGE_KEY]: JSON.stringify({ checkUpdates: false }) });
    expect(updates.autoCheckDue()).toBeFalse();
    await updates.checkAndAnnounce();
    expect(updateCalls()).toEqual([]);
  });

  it('offers the update on start-up and installs only on request', async () => {
    const updates = configure(true);
    const toasts = TestBed.inject(ToastService);
    await updates.checkAndAnnounce();
    expect(updateCalls()).toEqual(['check_update']);
    const toast = toasts.toasts()[0];
    expect(toast.text).toContain('0.1.0-alpha.3');
    expect(toast.actions.length).toBe(1);
    toast.actions[0].run();
    await Promise.resolve();
    expect(updateCalls()).toEqual(['check_update', 'install_update']);
  });

  it('sends a system notification instead of a toast while the window is not focused', async () => {
    const updates = configure(true);
    focused = false;
    const toasts = TestBed.inject(ToastService);
    await updates.checkAndAnnounce();
    expect(toasts.toasts().length).toBe(0);
    const note = calls.find((c) => c.cmd === 'notify_update');
    expect(note?.args).toEqual({ title: 'Riptide 0.1.0-alpha.3 is available', body: 'Open Riptide to update' });
    expect(store.get(LAST_ANNOUNCED_KEY)).toBe('0.1.0-alpha.3');
  });

  it('announces each version once, and a newer one again', async () => {
    const updates = configure(true);
    focused = false;
    await updates.checkAndAnnounce();
    now += AUTO_CHECK_INTERVAL_MS;
    await updates.checkAndAnnounce();
    now += AUTO_CHECK_INTERVAL_MS;
    focused = true;
    await updates.checkAndAnnounce();
    expect(TestBed.inject(ToastService).toasts().length).toBe(0);
    expect(calls.filter((c) => c.cmd === 'notify_update').length).toBe(1);
    expect(updateCalls().filter((c) => c === 'check_update').length).toBe(3);
    answer = { ...UPDATE, version: '0.1.0-alpha.4' };
    now += AUTO_CHECK_INTERVAL_MS;
    await updates.checkAndAnnounce();
    expect(TestBed.inject(ToastService).toasts().length).toBe(1);
  });

  it('does not announce a version already announced in an earlier session', async () => {
    const updates = configure(true, { [LAST_ANNOUNCED_KEY]: '0.1.0-alpha.3' });
    focused = false;
    await updates.checkAndAnnounce();
    expect(calls.some((c) => c.cmd === 'notify_update')).toBeFalse();
  });

  it('makes no extra network call when woken again within the day', async () => {
    const updates = configure(true);
    await updates.checkAndAnnounce();
    now += 60 * 60 * 1000;
    await updates.checkAndAnnounce();
    await updates.checkAndAnnounce();
    expect(updateCalls()).toEqual(['check_update']);
  });

  it('exposes the badge while an update is available', async () => {
    const updates = configure(true);
    expect(updates.badgeVersion()).toBeNull();
    await updates.check();
    expect(updates.badgeVersion()).toBe('0.1.0-alpha.3');
    answer = null;
    await updates.check();
    expect(updates.badgeVersion()).toBeNull();
  });

  it("shows 'Updated to' once after the version changed, with a What's new action", async () => {
    const updates = configure(true, { [LAST_RUN_VERSION_KEY]: '0.1.0-alpha.2' });
    const toasts = TestBed.inject(ToastService);
    await updates.announceUpdated();
    expect(toasts.toasts().length).toBe(1);
    expect(toasts.toasts()[0].text).toContain('0.1.0-alpha.3');
    toasts.toasts()[0].actions[0].run();
    await Promise.resolve();
    expect(calls.find((c) => c.cmd === 'open_release_page')?.args).toEqual({ version: '0.1.0-alpha.3' });
    expect(store.get(LAST_RUN_VERSION_KEY)).toBe('0.1.0-alpha.3');
    const before = toasts.toasts().length;
    await updates.announceUpdated();
    expect(toasts.toasts().length).toBe(before);
  });

  it('stays quiet on a fresh install', async () => {
    const updates = configure(true);
    await updates.announceUpdated();
    expect(TestBed.inject(ToastService).toasts().length).toBe(0);
    expect(store.get(LAST_RUN_VERSION_KEY)).toBe('0.1.0-alpha.3');
  });
});
