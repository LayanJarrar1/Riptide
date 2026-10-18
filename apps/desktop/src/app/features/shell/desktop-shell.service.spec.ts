import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { I18nService } from '../../i18n/i18n.service';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { CLIPBOARD_LINK_EVENT, DesktopShellService, OPEN_DOWNLOAD_EVENT, ShellPrefs } from './desktop-shell.service';
import { UiState } from './ui-state.service';

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

describe('DesktopShellService', () => {
  let calls: Call[];
  let handlers: Map<string, (payload: unknown) => void>;
  let autostart: boolean;

  beforeEach(() => {
    calls = [];
    handlers = new Map();
    autostart = true;
    const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      if (cmd === 'autostart_enabled') {
        return Promise.resolve(autostart as unknown as T);
      }
      if (cmd === 'get_engine_settings') {
        return Promise.reject(new Error('no engine in this spec'));
      }
      return Promise.resolve(undefined as unknown as T);
    };
    const listen: ListenFn = <T>(event: string, handler: (payload: T) => void): Promise<() => void> => {
      handlers.set(event, handler as (payload: unknown) => void);
      return Promise.resolve(() => handlers.delete(event));
    };
    TestBed.configureTestingModule({
      providers: [
        provideRouter([]),
        // the mock engine even though the bridge is "available": the spec drives its list
        { provide: DownloadEngine, useExisting: MockEngineService },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: TAURI_AVAILABLE, useValue: true },
        { provide: TAURI_INVOKE, useValue: invoke },
        { provide: TAURI_LISTEN, useValue: listen },
      ],
    });
    spyOn(console, 'warn');
    TestBed.inject(DesktopShellService);
  });

  function lastPrefs(): ShellPrefs {
    const pushes = calls.filter((c) => c.cmd === 'set_shell_prefs');
    expect(pushes.length).toBeGreaterThan(0);
    return pushes[pushes.length - 1].args?.['prefs'] as ShellPrefs;
  }

  it('pushes the shell settings with labels in the active language, again on a switch', async () => {
    TestBed.tick();
    await flush();
    expect(lastPrefs().labels.quit).toBe('Quit');
    expect(lastPrefs().closeToTray).toBeTrue();

    TestBed.inject(I18nService).setLang('ar');
    TestBed.inject(SettingsService).patch({ closeToTray: false });
    TestBed.tick();
    const prefs = lastPrefs();
    expect(prefs.labels.quit).toBe('خروج');
    expect(prefs.labels.units[2]).toBe('م.ب');
    expect(prefs.closeToTray).toBeFalse();
  });

  it('reflects the real autostart entry and writes changes back', async () => {
    TestBed.tick();
    await flush();
    TestBed.tick();
    const settings = TestBed.inject(SettingsService);
    expect(settings.settings().startWithWindows).toBeTrue();
    expect(calls.some((c) => c.cmd === 'set_autostart')).toBeFalse();

    settings.patch({ startWithWindows: false });
    TestBed.tick();
    expect(calls.filter((c) => c.cmd === 'set_autostart').map((c) => c.args?.['enabled'])).toEqual([false]);
  });

  it('offers a copied link once per prompt and opens the add dialog prefilled', async () => {
    await flush();
    const toasts = TestBed.inject(ToastService);
    const ui = TestBed.inject(UiState);
    handlers.get(CLIPBOARD_LINK_EVENT)?.({ url: 'https://example.com/new.zip', name: 'new.zip' });
    expect(toasts.toasts().length).toBe(1);
    const toast = toasts.toasts()[0];
    expect(toast.text).toBe('Download new.zip?');
    expect(ui.addOpen()).toBeFalse();
    toast.actions[0].run();
    expect(ui.addOpen()).toBeTrue();
    expect(ui.addUrl()).toBe('https://example.com/new.zip');
  });

  it('ignores a copied link already in the list', async () => {
    await flush();
    const engine = TestBed.inject(MockEngineService);
    const existing = engine.downloads()[0];
    handlers.get(CLIPBOARD_LINK_EVENT)?.({ url: existing.url, name: existing.filename });
    expect(TestBed.inject(ToastService).toasts().length).toBe(0);
  });

  it('opens the details of a clicked notification', async () => {
    await flush();
    const store = TestBed.inject(DownloadsStore);
    const id = TestBed.inject(MockEngineService).downloads()[0].id;
    handlers.get(OPEN_DOWNLOAD_EVENT)?.(id);
    await flush();
    await flush();
    expect(store.detailsId()).toBe(id);
  });
});
