import { TestBed } from '@angular/core/testing';

import { SettingsService } from '../features/settings/settings.service';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { BrowserIntegrationService, IntegrationStatus } from './browser-integration.service';
import { MemoryPreferenceStore, PREFERENCE_STORE } from './preferences';

function status(registered: boolean, hostFound = true): IntegrationStatus {
  return {
    supported: true,
    registered,
    enabled: registered,
    hostFound,
    hostPath: 'C:\\Riptide\\riptide-host.exe',
    manifestDir: 'C:\\Riptide\\native-messaging',
    browsers: (['chrome', 'edge', 'firefox'] as const).map((browser) => ({ browser, registered })),
  };
}

describe('BrowserIntegrationService', () => {
  let calls: string[];
  let current: IntegrationStatus;
  let fail: boolean;

  function configure(tauri: boolean): BrowserIntegrationService {
    calls = [];
    current = status(false);
    fail = false;
    const invoke: InvokeFn = <T>(cmd: string): Promise<T> => {
      calls.push(cmd);
      if (fail && cmd !== 'integration_status') {
        return Promise.reject(new Error('cannot write HKCU'));
      }
      if (cmd === 'register_browser_host') {
        current = status(true);
      } else if (cmd === 'unregister_browser_host') {
        current = status(false);
      } else if (cmd === 'extension_info') {
        return Promise.resolve({
          id: 'dcoeaodjhaibfchhcbfdkekpcacofnhm',
          browsers: [{ browser: 'chrome', installed: true, folder: 'C:/Riptide/extension/chrome', folderFound: true }],
        } as unknown as T);
      } else if (cmd === 'get_engine_settings') {
        return Promise.resolve({ default_dir: 'D:\\x', connections: 8, per_host: 4, max_active: 3, user_agent: null, category_dirs: {}, global_speed_limit: null, proxy: { mode: 'system', url: '' } } as unknown as T);
      }
      return Promise.resolve(current as unknown as T);
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        { provide: TAURI_INVOKE, useValue: invoke },
      ],
    });
    return TestBed.inject(BrowserIntegrationService);
  }

  it('is the demo setting in a plain browser and never asks the backend', async () => {
    const service = configure(false);
    await service.refresh();
    expect(service.connected()).toBeFalse();
    expect(await service.setEnabled(true)).toBeTrue();
    expect(service.connected()).toBeTrue();
    expect(TestBed.inject(SettingsService).settings().browserIntegration).toBeTrue();
    expect(calls.filter((c) => c.includes('browser') || c.includes('integration'))).toEqual([]);
  });

  it('under Tauri it reads the status and registers / unregisters through the commands', async () => {
    const service = configure(true);
    await service.refresh();
    expect(service.status()?.registered).toBeFalse();
    expect(service.connected()).toBeFalse();

    expect(await service.setEnabled(true)).toBeTrue();
    expect(service.connected()).toBeTrue();
    expect(service.busy()).toBeFalse();

    expect(await service.setEnabled(false)).toBeTrue();
    expect(service.connected()).toBeFalse();
    expect(calls).toContain('register_browser_host');
    expect(calls).toContain('unregister_browser_host');
  });

  it('reports a refusal and a missing host', async () => {
    const service = configure(true);
    fail = true;
    current = status(false, false);
    expect(await service.setEnabled(true)).toBeFalse();
    expect(service.connected()).toBeFalse();
    expect(service.hostMissing()).toBeTrue();
    expect(service.busy()).toBeFalse();
  });

  it('reads the extension folders and the bridge state per browser', async () => {
    const service = configure(true);
    await service.refresh();
    await service.refreshExtension();
    expect(service.extension()?.id).toBe('dcoeaodjhaibfchhcbfdkekpcacofnhm');
    expect(service.extension()?.browsers[0]?.folderFound).toBeTrue();
    expect(service.bridgeRegistered('chrome')).toBeFalse();
    await service.setEnabled(true);
    expect(service.bridgeRegistered('chrome')).toBeTrue();
    expect(service.bridgeRegistered('edge')).toBeTrue();
  });

  it('never calls the extension commands in a plain browser', async () => {
    const service = configure(false);
    await service.refreshExtension();
    expect(await service.openExtensionFolder('chrome')).toBeFalse();
    expect(await service.openExtensionsPage('edge')).toBeFalse();
    expect(service.extension()).toBeNull();
    expect(calls).toEqual([]);
  });

  it('passes only the browser name to the extension commands', async () => {
    const service = configure(true);
    const seen: [string, Record<string, unknown> | undefined][] = [];
    const stub = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      seen.push([cmd, args]);
      return Promise.resolve(null as unknown as T);
    };
    (service as unknown as { invoke: typeof stub }).invoke = stub;
    expect(await service.openExtensionFolder('chrome')).toBeTrue();
    expect(await service.openExtensionsPage('edge')).toBeTrue();
    expect(seen).toEqual([
      ['open_extension_folder', { browser: 'chrome' }],
      ['open_extensions_page', { browser: 'edge' }],
    ]);
  });
});
