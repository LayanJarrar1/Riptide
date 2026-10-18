import { TestBed } from '@angular/core/testing';

import { SETTINGS_STORAGE_KEY } from '../features/settings/settings.service';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { ToastService } from '../ui/toast.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from './preferences';
import { AUTO_CHECK_INTERVAL_MS, LAST_UPDATE_CHECK_KEY, NOW, UpdateInfo, UpdateService } from './update.service';

const T0 = 1_800_000_000_000;
const UPDATE: UpdateInfo = { version: '0.1.0-alpha.3', current_version: '0.1.0-alpha.2', notes: 'Faster.', date: null };

describe('UpdateService', () => {
  let calls: { cmd: string; args?: Record<string, unknown> }[];
  let answer: UpdateInfo | null;
  let fail: boolean;
  let now: number;
  let store: MemoryPreferenceStore;

  function configure(tauri: boolean, prefs: Record<string, string> = {}): UpdateService {
    calls = [];
    answer = UPDATE;
    fail = false;
    now = T0;
    store = new MemoryPreferenceStore(prefs);
    const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      if (cmd === 'get_engine_settings') {
        return Promise.resolve({ default_dir: 'D:\\x', connections: 8, per_host: 4, max_active: 3, user_agent: null, category_dirs: {} } as unknown as T);
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
      ],
    });
    return TestBed.inject(UpdateService);
  }

  const updateCalls = (): string[] => calls.map((c) => c.cmd).filter((c) => c !== 'get_engine_settings');

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
    await updates.startupCheck();
    expect(updateCalls()).toEqual([]);
  });

  it('offers the update on start-up and installs only on request', async () => {
    const updates = configure(true);
    const toasts = TestBed.inject(ToastService);
    await updates.startupCheck();
    expect(updateCalls()).toEqual(['check_update']);
    const toast = toasts.toasts()[0];
    expect(toast.text).toContain('0.1.0-alpha.3');
    expect(toast.actions.length).toBe(1);
    toast.actions[0].run();
    await Promise.resolve();
    expect(updateCalls()).toEqual(['check_update', 'install_update']);
  });
});
