import { inject, Provider } from '@angular/core';

import { TAURI_AVAILABLE } from '../tauri/tauri-bridge';
import { DownloadEngine } from './download-engine';
import { MockEngineService } from './mock-engine.service';
import { TauriEngineService } from './tauri-engine.service';

/**
 * Binds `DownloadEngine` to the real engine inside the Tauri webview and to the simulation
 * everywhere else (`ng serve`, Karma). Specs that pull in the store or a page add this provider
 * and get the mock, exactly as `ng serve` does.
 */
export function provideDownloadEngine(): Provider {
  return {
    provide: DownloadEngine,
    useFactory: (): DownloadEngine =>
      inject(TAURI_AVAILABLE) ? inject(TauriEngineService) : inject(MockEngineService),
  };
}
