import { EnvironmentProviders, inject, Injector, provideAppInitializer, Provider, Type } from '@angular/core';

import { TAURI_AVAILABLE } from '../tauri/tauri-bridge';
import { DownloadEngine } from './download-engine';
import { TauriEngineService } from './tauri-engine.service';

/** The simulation's class once its chunk has been fetched (plain browser only). */
let simulation: Type<DownloadEngine> | null = null;

/**
 * Binds `DownloadEngine` for the running app: the real engine over IPC inside the Tauri
 * webview, the simulation everywhere else. The simulation (and its demo data, about 14 kB) is a
 * lazy chunk that the desktop build never fetches, so it does not count against the initial
 * bundle; an initializer loads it before the first paint of the browser preview. Specs use
 * `provideDownloadEngine()` from `engine.provider.ts`, which links the simulation statically.
 */
export function provideAppDownloadEngine(): (Provider | EnvironmentProviders)[] {
  return [
    provideAppInitializer(async () => {
      if (!inject(TAURI_AVAILABLE)) {
        simulation = (await import('./mock-engine.service')).MockEngineService;
      }
    }),
    {
      provide: DownloadEngine,
      useFactory: (): DownloadEngine => {
        if (inject(TAURI_AVAILABLE)) {
          return inject(TauriEngineService);
        }
        if (simulation === null) {
          throw new Error('the simulated engine was not loaded');
        }
        return inject(Injector).get(simulation);
      },
    },
  ];
}
