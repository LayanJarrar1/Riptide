import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { NARROW_QUERY, NavRailService } from '../../core/nav-rail.service';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ThemeService } from '../../theme/theme.service';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { AppCommands, DOCS_URL, REPO_URL } from './app-commands.service';
import { UiState } from './ui-state.service';

describe('AppCommands', () => {
  let cmd: AppCommands;
  let store: DownloadsStore;
  let engine: MockEngineService;
  let ui: UiState;
  let settings: SettingsService;
  let invoked: { cmd: string; args?: Record<string, unknown> }[];

  function setup(tauri: boolean): void {
    invoked = [];
    spyOn(console, 'warn');
    TestBed.configureTestingModule({
      providers: [
        provideRouter([]),
        // the simulation even where the bridge is "available": the spec drives its list
        { provide: DownloadEngine, useExisting: MockEngineService },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NARROW_QUERY, useValue: { matches: false, addEventListener: (): void => undefined } },
        { provide: TAURI_AVAILABLE, useValue: tauri },
        {
          provide: TAURI_INVOKE,
          useValue: <T>(name: string, args?: Record<string, unknown>): Promise<T> => {
            if (name === 'get_engine_settings' || name === 'autostart_enabled') {
              return Promise.reject(new Error('no engine in this spec'));
            }
            invoked.push({ cmd: name, args });
            return Promise.resolve(undefined as unknown as T);
          },
        },
      ],
    });
    cmd = TestBed.inject(AppCommands);
    store = TestBed.inject(DownloadsStore);
    engine = TestBed.inject(DownloadEngine) as MockEngineService;
    ui = TestBed.inject(UiState);
    settings = TestBed.inject(SettingsService);
  }

  const first = (state: string): number => engine.downloads().find((d) => d.state === state)?.id ?? -1;

  describe('selection-aware availability', () => {
    beforeEach(() => setup(true));

    it('nothing is available for the selection when nothing is selected', () => {
      expect([cmd.canResume(), cmd.canPause(), cmd.canCancel(), cmd.canRemove()]).toEqual([false, false, false, false]);
      expect(cmd.openable()).toBeNull();
    });

    it('a running download can be paused and cancelled but not resumed', () => {
      store.select(first('downloading'));
      expect([cmd.canResume(), cmd.canPause(), cmd.canCancel(), cmd.canRemove()]).toEqual([false, true, true, true]);
    });

    it('a paused download can be resumed; a finished one can be opened but not cancelled', () => {
      store.select(first('paused'));
      expect(cmd.canResume()).toBeTrue();
      expect(cmd.canPause()).toBeFalse();
      store.select(first('completed'));
      expect(cmd.canCancel()).toBeFalse();
      expect(cmd.openable()?.id).toBe(first('completed'));
      store.toggle(first('paused'));
      expect(cmd.openable()).toBeNull();
    });

    it('Pause all / Resume all follow what is in the list', () => {
      expect(cmd.anyRunning()).toBeTrue();
      expect(cmd.anyResumable()).toBeTrue();
    });
  });

  describe('commands', () => {
    beforeEach(() => setup(true));

    it('pause, resume and cancel act on the selected downloads only', () => {
      const running = first('downloading');
      store.select(running);
      cmd.pauseSelected();
      expect(engine.get(running)?.state).toBe('paused');
      cmd.resumeSelected();
      expect(engine.get(running)?.state).toBe('queued');
      cmd.cancelSelected();
      expect(engine.get(running)?.state).toBe('cancelled');
      expect(engine.downloads().filter((d) => d.state === 'cancelled').length).toBeGreaterThanOrEqual(1);
    });

    it('opens the add, batch (pattern and list tab) and grabber dialogs', () => {
      cmd.addDownload();
      expect(ui.addOpen()).toBeTrue();
      cmd.batchDownload();
      expect([ui.batchOpen(), ui.batchTab()]).toEqual([true, 'pattern']);
      ui.closeBatch();
      cmd.importLinkList();
      expect([ui.batchOpen(), ui.batchTab()]).toEqual([true, 'list']);
      cmd.siteGrabber();
      expect(ui.grabberOpen()).toBeTrue();
      ui.openTorrent({ kind: 'magnet', uri: 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567' });
      expect(ui.torrentSource()?.kind).toBe('magnet');
      ui.closeTorrent();
      expect(ui.torrentSource()).toBeNull();
      cmd.showShortcuts();
      expect(ui.shortcutsOpen()).toBeTrue();
    });

    it('Remove goes through the list page (it owns the confirmation)', async () => {
      const before = ui.removeRequests();
      cmd.removeSelected();
      await TestBed.inject(Router).navigateByUrl('/');
      await new Promise((r) => setTimeout(r, 0));
      expect(ui.removeRequests()).toBe(before + 1);
    });

    it('New queue expands a collapsed sidebar and asks the queues list to open its dialog', () => {
      const rail = TestBed.inject(NavRailService);
      rail.toggle();
      expect(rail.expanded()).toBeFalse();
      const before = ui.newQueueRequests();
      cmd.newQueue();
      expect(rail.expanded()).toBeTrue();
      expect(ui.newQueueRequests()).toBe(before + 1);
    });

    it('View: theme and density are saved settings, the sidebar switch flips the navigation', () => {
      cmd.setTheme('dark');
      expect(TestBed.inject(ThemeService).theme()).toBe('dark');
      cmd.setDensity('compact');
      expect(settings.settings().density).toBe('compact');
      const rail = TestBed.inject(NavRailService);
      cmd.toggleSidebar();
      expect(rail.expanded()).toBeFalse();
      cmd.toggleSidebar();
      expect(rail.expanded()).toBeTrue();
    });

    it('density and sidebar defaults: comfortable rows, sidebar shown', () => {
      expect(settings.settings().density).toBe('comfortable');
      expect(TestBed.inject(NavRailService).expanded()).toBeTrue();
    });

    it('opens the documentation and the repository through the opener command, nothing else', () => {
      cmd.openDocs();
      cmd.openRepo();
      expect(invoked).toEqual([
        { cmd: 'plugin:opener|open_url', args: { url: DOCS_URL } },
        { cmd: 'plugin:opener|open_url', args: { url: REPO_URL } },
      ]);
      expect(DOCS_URL).toBe('https://layanjarrar1.github.io/Riptide/');
      expect(REPO_URL).toBe('https://github.com/LayanJarrar1/Riptide');
    });

    it('opens a finished file or its folder with the path the row menu uses', () => {
      const done = engine.downloads().find((d) => d.state === 'completed');
      store.select(done?.id ?? -1);
      cmd.openFile();
      cmd.openFolder();
      expect(invoked.map((c) => c.cmd)).toEqual(['open_file', 'show_in_folder']);
      expect(String(invoked[0].args?.['path'])).toContain(done?.filename ?? '?');
    });

    it('does not open anything for an unfinished download', () => {
      store.select(first('downloading'));
      cmd.openFile();
      expect(invoked).toEqual([]);
    });

    it('Exit asks first when downloads run and the setting is on, and quits at once otherwise', () => {
      settings.patch({ confirmOnExit: true });
      cmd.exit();
      expect(ui.exitRequest()).toBe(store.totals().active);
      expect(invoked).toEqual([]);
      ui.exitRequest.set(null);

      settings.patch({ confirmOnExit: false });
      cmd.exit();
      expect(invoked.map((c) => c.cmd)).toEqual(['quit_app']);
      expect(ui.exitRequest()).toBeNull();
    });

    it('Exit quits at once when nothing is running', async () => {
      for (const d of engine.downloads()) {
        await engine.cancel(d.id);
      }
      settings.patch({ confirmOnExit: true });
      cmd.exit();
      expect(invoked.map((c) => c.cmd)).toEqual(['quit_app']);
    });

    it('Update video support reports the new version, or the failure', async () => {
      const toasts = TestBed.inject(ToastService);
      spyOn(toasts, 'show');
      await cmd.updateVideoSupport();
      expect(invoked.map((c) => c.cmd)).toEqual(['update_media_tools']);
      expect(toasts.show).toHaveBeenCalledWith('success', jasmine.any(String));
    });
  });

  describe('in a plain browser', () => {
    beforeEach(() => setup(false));

    it('Help links open in a new tab without a referrer', () => {
      const open = spyOn(window, 'open');
      cmd.openDocs();
      expect(open).toHaveBeenCalledWith(DOCS_URL, '_blank', 'noopener,noreferrer');
      expect(invoked).toEqual([]);
    });

    it('opening a file explains it needs the desktop app', () => {
      const toasts = TestBed.inject(ToastService);
      spyOn(toasts, 'show');
      store.select(engine.downloads().find((d) => d.state === 'completed')?.id ?? -1);
      cmd.openFile();
      expect(toasts.show).toHaveBeenCalledWith('info', jasmine.any(String));
    });
  });
});
