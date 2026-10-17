import { TestBed } from '@angular/core/testing';

import { SettingsService } from '../features/settings/settings.service';
import type { EngineSettings } from '../tauri/bridge/EngineSettings';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { DEMO_FREE_BYTES, DISK_POLL_INTERVAL, DiskSpaceService, volumeOf } from './disk-space.service';
import { MemoryPreferenceStore, PREFERENCE_STORE } from './preferences';

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

/** Lets every promise chain started by a fake `invoke` settle. */
async function settle(): Promise<void> {
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
  }
}

describe('DiskSpaceService', () => {
  const engine: EngineSettings = { default_dir: 'D:\\Engine', connections: 8, per_host: 4, max_active: 3, user_agent: null, category_dirs: {} };
  let calls: Call[];
  let free: number;

  function configure(tauri: boolean): DiskSpaceService {
    calls = [];
    free = 10_000;
    const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      if (cmd === 'get_engine_settings') {
        return Promise.resolve(engine as unknown as T);
      }
      if (cmd === 'disk_free') {
        return Promise.resolve(free as unknown as T);
      }
      return Promise.resolve(undefined as unknown as T);
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        { provide: TAURI_INVOKE, useValue: invoke },
        { provide: DISK_POLL_INTERVAL, useValue: 50 },
      ],
    });
    return TestBed.inject(DiskSpaceService);
  }

  it('reports the demo figure in a plain browser and never asks', async () => {
    const service = configure(false);
    expect(service.freeBytes()).toBe(DEMO_FREE_BYTES);
    expect(await service.freeFor('D:\\x')).toBe(DEMO_FREE_BYTES);
    expect(calls.filter((c) => c.cmd === 'disk_free').length).toBe(0);
  });

  it('under Tauri it asks disk_free for the default folder, on change, and on the interval', async () => {
    const service = configure(true);
    const settings = TestBed.inject(SettingsService);
    await settings.engineLoaded;
    TestBed.tick();
    await settle();
    expect(service.freeBytes()).toBe(10_000);
    const asked = (): string[] => calls.filter((c) => c.cmd === 'disk_free').map((c) => String(c.args?.['path']));
    expect(asked()).toContain('D:\\Engine');
    expect(service.volume()).toBe('D:');

    free = 20_000;
    settings.patch({ defaultDir: 'E:\\Other' });
    TestBed.tick();
    await settle();
    expect(asked().slice(-1)).toEqual(['E:\\Other']);
    expect(service.freeBytes()).toBe(20_000);
    expect(service.volume()).toBe('E:');

    free = 30_000;
    const before = asked().length;
    await new Promise((resolve) => setTimeout(resolve, 120));
    await settle();
    expect(asked().length).toBeGreaterThan(before);
    expect(service.freeBytes()).toBe(30_000);
  });

  it('freeFor answers for any folder and swallows a refusal', async () => {
    const service = configure(true);
    expect(await service.freeFor('C:\\Somewhere')).toBe(10_000);
    expect(calls.filter((c) => c.cmd === 'disk_free').slice(-1)[0].args).toEqual({ path: 'C:\\Somewhere' });
    expect(await service.freeFor('   ')).toBeNull();
  });

  it('volumeOf picks the drive letter on Windows and the folder elsewhere', () => {
    expect(volumeOf('C:\\Users\\x')).toBe('C:');
    expect(volumeOf('d:/dl')).toBe('D:');
    expect(volumeOf('/home/x/Downloads')).toBe('/home/x/Downloads');
  });
});
