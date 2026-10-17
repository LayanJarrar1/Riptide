import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, PREFERENCE_STORE } from '../../core/preferences';
import type { EngineSettings } from '../../tauri/bridge/EngineSettings';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { DEFAULT_SETTINGS, ENGINE_SYNC_DEBOUNCE, SettingsService } from './settings.service';

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

function fakeInvoke(engine: EngineSettings, calls: Call[]): InvokeFn {
  return <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    return Promise.resolve((cmd === 'get_engine_settings' ? engine : undefined) as unknown as T);
  };
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

describe('SettingsService', () => {
  const engine: EngineSettings = { default_dir: 'D:\\Engine', connections: 12, per_host: 6, max_active: 5, user_agent: 'Riptide/1', category_dirs: { video: 'D:\\Engine\\Video' } };
  let calls: Call[];

  function configure(tauri: boolean): SettingsService {
    calls = [];
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        { provide: TAURI_INVOKE, useValue: fakeInvoke(engine, calls) },
        { provide: ENGINE_SYNC_DEBOUNCE, useValue: 5 },
      ],
    });
    return TestBed.inject(SettingsService);
  }

  it('in a plain browser it never talks to an engine', async () => {
    const service = configure(false);
    await service.engineLoaded;
    service.patch({ connections: 3 });
    TestBed.tick();
    await wait(20);
    expect(calls.length).toBe(0);
    expect(service.settings().connections).toBe(3);
  });

  it('under Tauri it merges the engine values on start without writing them back', async () => {
    const service = configure(true);
    expect(service.settings().defaultDir).toBe(DEFAULT_SETTINGS.defaultDir);
    await service.engineLoaded;
    TestBed.tick();
    expect(service.settings().defaultDir).toBe('D:\\Engine');
    expect(service.settings().connections).toBe(12);
    expect(service.settings().perHost).toBe(6);
    expect(service.settings().categoryDirs).toEqual({ video: 'D:\\Engine\\Video' });
    expect(service.settings().closeToTray).toBe(DEFAULT_SETTINGS.closeToTray);
    await wait(20);
    await service.whenSynced();
    expect(calls.map((c) => c.cmd)).toEqual(['get_engine_settings']);
  });

  it('under Tauri a change to an engine-owned value is written back once, debounced, in full', async () => {
    const service = configure(true);
    await service.engineLoaded;
    TestBed.tick();
    service.patch({ connections: 2 });
    TestBed.tick();
    service.patch({ connections: 4, perHost: 2 });
    TestBed.tick();
    await wait(20);
    await service.whenSynced();
    const writes = calls.filter((c) => c.cmd === 'set_engine_settings');
    expect(writes.length).toBe(1);
    expect(writes[0].args).toEqual({
      settings: { default_dir: 'D:\\Engine', connections: 4, per_host: 2, max_active: 5, user_agent: null, category_dirs: { video: 'D:\\Engine\\Video' } },
    });
    // a UI-only change does not reach the engine
    service.patch({ closeToTray: false });
    TestBed.tick();
    await wait(20);
    await service.whenSynced();
    expect(calls.filter((c) => c.cmd === 'set_engine_settings').length).toBe(1);
    // a category folder is engine-owned too; clearing one drops its key
    service.setCategoryDir('images', 'D:\\Pics');
    service.setCategoryDir('video', null);
    TestBed.tick();
    await wait(20);
    await service.whenSynced();
    const again = calls.filter((c) => c.cmd === 'set_engine_settings');
    expect(again.length).toBe(2);
    expect((again[1].args?.['settings'] as EngineSettings).category_dirs).toEqual({ images: 'D:\\Pics' });
  });
});
