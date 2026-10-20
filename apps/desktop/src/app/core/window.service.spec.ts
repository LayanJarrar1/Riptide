import { TestBed } from '@angular/core/testing';

import { TAURI_AVAILABLE } from '../tauri/tauri-bridge';
import { NATIVE_WINDOW, NativeWindow, WindowService } from './window.service';

function fake(state: { maximized: boolean; fail?: boolean }): NativeWindow & { calls: string[] } {
  const calls: string[] = [];
  const rec = (name: string, then?: () => void): Promise<void> => {
    calls.push(name);
    if (state.fail) {
      return Promise.reject(new Error('refused'));
    }
    then?.();
    return Promise.resolve();
  };
  return {
    calls,
    minimize: () => rec('minimize'),
    maximize: () => rec('maximize', () => (state.maximized = true)),
    unmaximize: () => rec('unmaximize', () => (state.maximized = false)),
    close: () => rec('close'),
    isMaximized: () => Promise.resolve(state.maximized),
  };
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

describe('WindowService', () => {
  it('has no native window in a plain browser and every command is a no-op', async () => {
    TestBed.configureTestingModule({ providers: [{ provide: TAURI_AVAILABLE, useValue: false }] });
    const win = TestBed.inject(WindowService);
    expect(win.available).toBeFalse();
    win.minimize();
    win.toggleMaximize();
    win.close();
    await flush();
    expect(win.maximized()).toBeFalse();
  });

  describe('inside the desktop app', () => {
    let state: { maximized: boolean; fail?: boolean };
    let native: NativeWindow & { calls: string[] };
    let win: WindowService;

    beforeEach(async () => {
      state = { maximized: false };
      native = fake(state);
      TestBed.configureTestingModule({
        providers: [
          { provide: TAURI_AVAILABLE, useValue: true },
          { provide: NATIVE_WINDOW, useValue: () => Promise.resolve(native) },
        ],
      });
      win = TestBed.inject(WindowService);
      await flush();
    });

    it('reads whether the window starts maximized', async () => {
      TestBed.resetTestingModule();
      state.maximized = true;
      TestBed.configureTestingModule({
        providers: [
          { provide: TAURI_AVAILABLE, useValue: true },
          { provide: NATIVE_WINDOW, useValue: () => Promise.resolve(native) },
        ],
      });
      const started = TestBed.inject(WindowService);
      await flush();
      expect(started.maximized()).toBeTrue();
    });

    it('toggles between maximize and restore and tracks the state', async () => {
      win.toggleMaximize();
      await flush();
      expect(native.calls).toEqual(['maximize']);
      expect(win.maximized()).toBeTrue();
      win.toggleMaximize();
      await flush();
      expect(native.calls).toEqual(['maximize', 'unmaximize']);
      expect(win.maximized()).toBeFalse();
    });

    it('minimize and close go straight to the window (close keeps close-to-tray and the exit prompt: it is a close request)', async () => {
      win.minimize();
      win.close();
      await flush();
      expect(native.calls).toEqual(['minimize', 'close']);
    });

    it('re-reads the maximized state after a resize (the user snapped or double-clicked the bar)', async () => {
      state.maximized = true;
      window.dispatchEvent(new Event('resize'));
      await new Promise((r) => setTimeout(r, 200));
      expect(win.maximized()).toBeTrue();
    });

    it('follows focus and blur of the window', () => {
      window.dispatchEvent(new Event('blur'));
      expect(win.focused()).toBeFalse();
      window.dispatchEvent(new Event('focus'));
      expect(win.focused()).toBeTrue();
    });

    it('survives a refused command (it is only logged)', async () => {
      spyOn(console, 'warn');
      state.fail = true;
      win.minimize();
      await flush();
      expect(console.warn).toHaveBeenCalled();
    });
  });
});
