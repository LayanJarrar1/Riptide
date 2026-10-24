import { ChangeDetectionStrategy, Component, computed, inject, OnInit, signal } from '@angular/core';

import { BrowserIntegrationService, ExtensionBrowser, ExtensionBrowserName } from '../../../core/browser-integration.service';
import { I18nService } from '../../../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../../tauri/tauri-bridge';
import { ButtonComponent } from '../../../ui/button.component';
import { ToastService } from '../../../ui/toast.service';
import { SettingGroupComponent, SettingRowComponent } from '../setting-layout.component';

/** What `protocol_status` reports (`src-tauri/src/protocol.rs`). */
export interface ProtocolStatus {
  readonly supported: boolean;
  /** `magnet:` links open Riptide. */
  readonly magnet: boolean;
  /** `.torrent` files offer (and may default to) Riptide. */
  readonly torrent: boolean;
}

/** Settings → Browser integration: the extension and its bridge, magnet links, and diagnostics. */
@Component({
  selector: 'app-settings-browser',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SettingGroupComponent, SettingRowComponent, ButtonComponent],
  template: `
    <app-setting-group anchor="integration" [heading]="i18n.t('set.g.extension')" [description]="i18n.t('settings.browserDesc')">
      <app-setting-row [label]="i18n.t('settings.browser')" [description]="i18n.t(browserStateKey())">
        <span class="sr-pill" [class.is-ok]="browser.connected()" [class.is-warn]="!browser.connected() && browser.hostMissing()" role="status">
          <span class="sr-dot"></span>{{ i18n.t(browser.connected() ? 'set.connected' : 'set.notConnected') }}
        </span>
        <button appButton variant="secondary" type="button" [disabled]="browser.busy()" (click)="toggleBrowser()">{{ i18n.t(browser.connected() ? 'settings.browserRemove' : 'settings.browserInstall') }}</button>
      </app-setting-row>
      <app-setting-row [stack]="true" [label]="i18n.t('settings.ext')" [description]="i18n.t('settings.extDesc')">
        <ol class="steps" id="extension">
          <li>{{ i18n.t('settings.extStep1') }}</li>
          <li>{{ i18n.t('settings.extStep2') }}</li>
          <li>{{ i18n.t('settings.extStep3') }}</li>
        </ol>
      </app-setting-row>
      @for (b of extBrowsers(); track b.browser) {
        <app-setting-row [label]="i18n.t(b.browser === 'chrome' ? 'settings.extChrome' : 'settings.extEdge')" [description]="i18n.t(browser.bridgeRegistered(b.browser) ? 'settings.extBridgeOn' : 'settings.extBridgeOff')">
          <button appButton variant="secondary" size="sm" type="button" [disabled]="!tauriAvailable || !b.folderFound" (click)="openExtensionFolder(b.browser)">{{ i18n.t('settings.extOpenFolder') }}</button>
          <button appButton variant="secondary" size="sm" type="button" [disabled]="!tauriAvailable || !b.installed" (click)="openExtensionsPage(b.browser)">{{ i18n.t(b.browser === 'chrome' ? 'settings.extOpenChrome' : 'settings.extOpenEdge') }}</button>
        </app-setting-row>
      }
      <app-setting-row [label]="i18n.t('set.extId')" [description]="i18n.t('set.extIdDesc')">
        <code class="id num" dir="ltr">{{ extId() }}</code>
      </app-setting-row>
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.links')">
      <app-setting-row [label]="i18n.t('settings.protocols')" [description]="i18n.t('settings.protocolsDesc')">
        <span class="sr-pill" [class.is-ok]="protocols()?.magnet === true" role="status"><span class="sr-dot"></span>{{ i18n.t(protocolStateKey()) }}</span>
        <button appButton variant="secondary" type="button" [disabled]="!tauriAvailable || protocolsBusy() || protocols()?.supported === false" (click)="toggleProtocols()">
          {{ i18n.t(protocols()?.magnet ? 'settings.protocolsRemove' : 'settings.protocolsInstall') }}
        </button>
      </app-setting-row>
    </app-setting-group>

    <app-setting-group anchor="diagnostics" [heading]="i18n.t('set.g.diagnostics')" [description]="i18n.t('set.diagnosticsDesc')">
      <app-setting-row [label]="i18n.t('set.diagHost')" [description]="hostPath()">
        <span class="sr-pill" [class.is-ok]="browser.status()?.hostFound === true" [class.is-bad]="browser.status() !== null && browser.status()?.hostFound !== true" role="status">
          <span class="sr-dot"></span>{{ i18n.t(browser.status()?.hostFound ? 'set.diagFound' : 'set.diagMissing') }}
        </span>
      </app-setting-row>
      <app-setting-row [label]="i18n.t('set.diagManifest')" [description]="browser.status()?.manifestDir || '—'">
        <span class="sr-value">{{ i18n.t(browser.status()?.registered ? 'settings.extBridgeOn' : 'settings.extBridgeOff') }}</span>
      </app-setting-row>
      <app-setting-row [label]="i18n.t('set.diagRefresh')" [description]="i18n.t('set.diagRefreshDesc')">
        <button appButton variant="secondary" size="sm" icon="retry" type="button" (click)="recheck()">{{ i18n.t('set.diagRefresh') }}</button>
      </app-setting-row>
    </app-setting-group>
  `,
  styles: `
    .steps {
      margin: 0;
      padding-inline-start: var(--space-5);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      line-height: 1.7;
    }

    .id {
      user-select: all;
      unicode-bidi: isolate;
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }
  `,
})
export class BrowserSectionComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly browser = inject(BrowserIntegrationService);
  private readonly toasts = inject(ToastService);
  protected readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);

  /** What `protocol_status` said (magnet and .torrent handlers); `null` while loading or in the browser preview. */
  protected readonly protocols = signal<ProtocolStatus | null>(null);
  protected readonly protocolsBusy = signal(false);
  protected readonly protocolStateKey = computed(() => {
    if (!this.tauriAvailable) {
      return 'settings.protocolsPreview';
    }
    return this.protocols()?.magnet ? 'settings.protocolsOn' : 'settings.protocolsOff';
  });

  /** Connected / not installed / host missing (a broken install). */
  protected readonly browserStateKey = computed(() => {
    if (this.browser.connected()) {
      return 'settings.browserOn';
    }
    return this.browser.hostMissing() ? 'settings.browserNoHost' : 'settings.browserOff';
  });

  /** Chrome and Edge rows; both are listed even before the backend answered. */
  protected readonly extBrowsers = computed<readonly ExtensionBrowser[]>(
    () =>
      this.browser.extension()?.browsers ??
      (['chrome', 'edge'] as const).map((name) => ({ browser: name, installed: false, folder: '', folderFound: false })),
  );

  protected readonly extId = computed(() => this.browser.extension()?.id ?? 'dcoeaodjhaibfchhcbfdkekpcacofnhm');
  protected readonly hostPath = computed(() => this.browser.status()?.hostPath || '—');

  ngOnInit(): void {
    void this.loadProtocols();
    void this.browser.refresh();
    void this.browser.refreshExtension();
  }

  protected recheck(): void {
    void this.browser.refresh();
    void this.browser.refreshExtension();
    void this.loadProtocols();
  }

  private async loadProtocols(): Promise<void> {
    if (!this.tauriAvailable) {
      return;
    }
    try {
      this.protocols.set(await this.invoke<ProtocolStatus>('protocol_status'));
    } catch {
      this.protocols.set({ supported: false, magnet: false, torrent: false });
    }
  }

  /** Registers (or removes) Riptide as the handler of magnet links and .torrent files, for this user only. */
  protected async toggleProtocols(): Promise<void> {
    if (this.protocolsBusy()) {
      return;
    }
    const on = this.protocols()?.magnet !== true;
    this.protocolsBusy.set(true);
    try {
      this.protocols.set(await this.invoke<ProtocolStatus>(on ? 'register_protocols' : 'unregister_protocols'));
      this.toasts.show('info', this.i18n.t(on ? 'toast.protocolsOn' : 'toast.protocolsOff'));
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.protocolsFailed'));
    } finally {
      this.protocolsBusy.set(false);
    }
  }

  protected async openExtensionFolder(name: ExtensionBrowserName): Promise<void> {
    if (!(await this.browser.openExtensionFolder(name))) {
      this.toasts.show('danger', this.i18n.t('toast.extFolderFailed'));
    }
  }

  protected async openExtensionsPage(name: ExtensionBrowserName): Promise<void> {
    if (!(await this.browser.openExtensionsPage(name))) {
      this.toasts.show('danger', this.i18n.t('toast.extPageFailed'));
    }
  }

  protected async toggleBrowser(): Promise<void> {
    const on = !this.browser.connected();
    if (await this.browser.setEnabled(on)) {
      this.toasts.show('info', this.i18n.t(on ? 'toast.browserOn' : 'toast.browserOff'));
    } else {
      this.toasts.show('danger', this.i18n.t('toast.browserFailed'));
    }
  }
}
