import { DestroyRef, inject, Injectable, InjectionToken, NgZone, signal } from '@angular/core';

import { TAURI_AVAILABLE } from '../tauri/tauri-bridge';

/** The few things the title bar asks of the native window. */
export interface NativeWindow {
  minimize(): Promise<void>;
  maximize(): Promise<void>;
  unmaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
}

/**
 * The current Tauri window, loaded on first use so Karma and the plain browser never evaluate
 * `@tauri-apps/api/window`. Specs provide a stub; `null` = no native window (browser preview).
 */
export const NATIVE_WINDOW = new InjectionToken<() => Promise<NativeWindow> | null>('riptide.nativeWindow', {
  providedIn: 'root',
  factory: () => {
    const tauri = inject(TAURI_AVAILABLE);
    if (!tauri) {
      return () => null;
    }
    let cached: Promise<NativeWindow> | null = null;
    return () => (cached ??= import('@tauri-apps/api/window').then((m) => m.getCurrentWindow()));
  },
});

/**
 * State and commands of the app window for the custom title bar: whether there is a native
 * window at all (not in a plain browser), whether it is maximized and focused. Maximize state is
 * re-read after every resize; focus follows the document's focus/blur events. Close goes through
 * the native close request, so close-to-tray and the exit confirmation still apply.
 */
@Injectable({ providedIn: 'root' })
export class WindowService {
  private readonly native = inject(NATIVE_WINDOW);
  private readonly zone = inject(NgZone);

  /** `true` when a native window exists (Tauri); the controls are hidden otherwise. */
  readonly available = inject(TAURI_AVAILABLE);
  readonly maximized = signal(false);
  /** `false` while another window has focus: the title bar dims. */
  readonly focused = signal(typeof document === 'undefined' || document.hasFocus());

  constructor() {
    if (typeof window === 'undefined') {
      return;
    }
    const onFocus = (): void => this.focused.set(true);
    const onBlur = (): void => this.focused.set(false);
    let pending: ReturnType<typeof setTimeout> | null = null;
    const onResize = (): void => {
      if (pending !== null) {
        clearTimeout(pending);
      }
      pending = setTimeout(() => void this.refresh(), 120);
    };
    this.zone.runOutsideAngular(() => {
      window.addEventListener('focus', onFocus);
      window.addEventListener('blur', onBlur);
      if (this.available) {
        window.addEventListener('resize', onResize);
      }
    });
    inject(DestroyRef).onDestroy(() => {
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('resize', onResize);
      if (pending !== null) {
        clearTimeout(pending);
      }
    });
    if (this.available) {
      void this.refresh();
    }
  }

  /** Re-reads whether the window is maximized. */
  async refresh(): Promise<void> {
    try {
      const win = await this.native();
      if (win !== null) {
        this.maximized.set(await win.isMaximized());
      }
    } catch (error: unknown) {
      console.warn('[window] isMaximized failed', error);
    }
  }

  minimize(): void {
    void this.run((w) => w.minimize());
  }

  /** Maximizes, or restores a maximized window. */
  toggleMaximize(): void {
    void this.run(async (w) => {
      if (await w.isMaximized()) {
        await w.unmaximize();
      } else {
        await w.maximize();
      }
      this.maximized.set(await w.isMaximized());
    });
  }

  close(): void {
    void this.run((w) => w.close());
  }

  private async run(action: (w: NativeWindow) => Promise<void>): Promise<void> {
    try {
      const win = await this.native();
      if (win !== null) {
        await action(win);
      }
    } catch (error: unknown) {
      console.warn('[window] command failed', error);
    }
  }
}
