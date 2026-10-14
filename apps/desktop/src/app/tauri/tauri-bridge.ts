import { InjectionToken } from '@angular/core';

import { isTauri } from './tauri.service';

/** Calls a Tauri command. Argument names are camelCase here; Tauri converts them. */
export type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

/** Subscribes to a Tauri event; resolves with the function that unsubscribes. */
export type ListenFn = <T>(event: string, handler: (payload: T) => void) => Promise<() => void>;

/**
 * Whether the Tauri IPC bridge exists in this page. Defaults to `isTauri()`; specs override it
 * (together with `TAURI_INVOKE` / `TAURI_LISTEN`) to exercise the Tauri code paths in Karma.
 */
export const TAURI_AVAILABLE = new InjectionToken<boolean>('riptide.tauri.available', {
  providedIn: 'root',
  factory: () => isTauri(),
});

/**
 * `invoke` from `@tauri-apps/api/core`, loaded on first use so the Karma bundle never evaluates
 * the Tauri API. Specs provide a fake.
 */
export const TAURI_INVOKE = new InjectionToken<InvokeFn>('riptide.tauri.invoke', {
  providedIn: 'root',
  factory:
    (): InvokeFn =>
    async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      const { invoke } = await import('@tauri-apps/api/core');
      return invoke<T>(cmd, args);
    },
});

/**
 * `listen` from `@tauri-apps/api/event`, loaded on first use; the handler receives the payload
 * only. Specs provide a fake that lets them emit events.
 */
export const TAURI_LISTEN = new InjectionToken<ListenFn>('riptide.tauri.listen', {
  providedIn: 'root',
  factory:
    (): ListenFn =>
    async <T>(event: string, handler: (payload: T) => void): Promise<() => void> => {
      const { listen } = await import('@tauri-apps/api/event');
      return listen<T>(event, (e) => handler(e.payload));
    },
});
