import { ApplicationConfig, provideBrowserGlobalErrorListeners, provideZoneChangeDetection } from '@angular/core';
import { provideRouter, withHashLocation } from '@angular/router';

import { routes } from './app.routes';
import { provideDownloadEngine } from './data/engine.provider';

/**
 * Hash routing on purpose: inside the Tauri webview the page is served from a custom
 * protocol where deep paths have no server to fall back to `index.html`.
 *
 * The download engine is the real one over IPC inside Tauri and the simulation in a plain
 * browser; the UI injects `DownloadEngine` and cannot tell the difference.
 */
export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZoneChangeDetection({ eventCoalescing: true }),
    provideRouter(routes, withHashLocation()),
    provideDownloadEngine(),
  ],
};
