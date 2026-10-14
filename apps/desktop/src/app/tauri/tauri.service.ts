import { Injectable, signal } from '@angular/core';

/** Mirror of the Rust `AppInfo` struct returned by the `app_info` command. */
export interface AppInfo {
  readonly name: string;
  readonly version: string;
  readonly build: string;
}

/** True only inside the Tauri webview, where the IPC bridge is injected. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Thin bridge to the Rust side. Outside Tauri (plain browser, Karma) every call is a no-op,
 * and `@tauri-apps/api` is only loaded on demand so the test bundle never evaluates it.
 */
@Injectable({ providedIn: 'root' })
export class TauriService {
  /** Whether the IPC bridge exists in this page. */
  readonly available = isTauri();

  /** Result of `app_info`, or `null` until it answers / when not running under Tauri. */
  readonly appInfo = signal<AppInfo | null>(null);

  /** Asks the backend for name/version/milestone. Resolves without throwing in all cases. */
  async loadAppInfo(): Promise<void> {
    if (!this.available) {
      return;
    }
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      this.appInfo.set(await invoke<AppInfo>('app_info'));
    } catch (error: unknown) {
      console.warn('[tauri] app_info failed', error);
    }
  }
}
