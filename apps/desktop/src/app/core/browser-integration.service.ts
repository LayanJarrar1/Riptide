import { computed, inject, Injectable, signal } from '@angular/core';

import { SettingsService } from '../features/settings/settings.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';

/** A browser the Native Messaging host is registered with. */
export type BrowserName = 'chrome' | 'edge' | 'firefox';

/** Mirror of the Rust `BrowserStatus`. */
export interface BrowserStatus {
  readonly browser: BrowserName;
  readonly registered: boolean;
}

/** Mirror of the Rust `IntegrationStatus` (`integration_status` and friends). */
export interface IntegrationStatus {
  /** This platform supports the integration (Windows in v1). */
  readonly supported: boolean;
  /** Every browser is registered and the host executable exists. */
  readonly registered: boolean;
  /** The user wants it on (registered again on every start while true). */
  readonly enabled: boolean;
  /** `riptide-host.exe` sits next to the app. */
  readonly hostFound: boolean;
  readonly hostPath: string;
  readonly manifestDir: string;
  readonly browsers: readonly BrowserStatus[];
}

/**
 * The *Browser extension* row of Settings → Integration. Under Tauri the state comes from
 * `integration_status` and the button calls `register_browser_host` / `unregister_browser_host`
 * (per-user registry keys and manifests, no elevation). In a plain browser it is the demo
 * setting `browserIntegration`.
 */
@Injectable({ providedIn: 'root' })
export class BrowserIntegrationService {
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly settings = inject(SettingsService);

  /** Last status from the backend; `null` until it answered (and always in a plain browser). */
  readonly status = signal<IntegrationStatus | null>(null);
  /** A register / unregister call is in flight. */
  readonly busy = signal(false);

  /** Whether the browsers can reach the app. */
  readonly connected = computed(() =>
    this.tauri ? (this.status()?.registered ?? false) : this.settings.settings().browserIntegration,
  );

  /** The host executable is missing next to the app (a broken install). */
  readonly hostMissing = computed(() => {
    const s = this.status();
    return s !== null && s.supported && !s.hostFound;
  });

  /** Re-reads the status. Never throws. */
  async refresh(): Promise<void> {
    if (!this.tauri) {
      return;
    }
    try {
      this.status.set(await this.invoke<IntegrationStatus>('integration_status'));
    } catch (error: unknown) {
      console.warn('[integration] integration_status failed', error);
    }
  }

  /** Registers (`true`) or unregisters the host. Resolves `false` when the backend refused. */
  async setEnabled(on: boolean): Promise<boolean> {
    if (!this.tauri) {
      this.settings.patch({ browserIntegration: on });
      return true;
    }
    this.busy.set(true);
    try {
      const status = await this.invoke<IntegrationStatus>(on ? 'register_browser_host' : 'unregister_browser_host');
      this.status.set(status);
      this.settings.patch({ browserIntegration: status.registered });
      return status.registered === on;
    } catch (error: unknown) {
      console.warn('[integration] registration failed', error);
      await this.refresh();
      return false;
    } finally {
      this.busy.set(false);
    }
  }
}
