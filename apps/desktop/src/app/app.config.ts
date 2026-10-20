import { ApplicationConfig, inject, provideAppInitializer, provideBrowserGlobalErrorListeners, provideZoneChangeDetection } from '@angular/core';
import { provideRouter, withHashLocation } from '@angular/router';

import { routes } from './app.routes';
import { provideAppDownloadEngine } from './data/engine.app-provider';
import { I18nService } from './i18n/i18n.service';

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
    // the first paint waits for the table of the language in use (Arabic is a separate chunk)
    provideAppInitializer(() => inject(I18nService).ready()),
    provideZoneChangeDetection({ eventCoalescing: true }),
    provideRouter(routes, withHashLocation()),
    provideAppDownloadEngine(),
  ],
};
