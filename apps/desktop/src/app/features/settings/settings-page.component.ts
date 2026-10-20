import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import {
  BrowserIntegrationService,
  ExtensionBrowser,
  ExtensionBrowserName,
} from '../../core/browser-integration.service';
import { FORMAT_PIPES } from '../../core/format.pipes';
import { effectiveChannel, isPrerelease } from '../../core/update-channel';
import { UpdateService } from '../../core/update.service';
import { I18nService, Lang, LANGS } from '../../i18n/i18n.service';
import { CATEGORY_KEYS, CategoryKey } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { TauriService } from '../../tauri/tauri.service';
import { Theme, THEMES, ThemeService } from '../../theme/theme.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { SegmentedControlComponent, SegmentedOption } from '../../ui/segmented-control.component';
import { SpeedLimitControlComponent } from '../../ui/speed-limit-control.component';
import { ToastService } from '../../ui/toast.service';
import { CATEGORY_ICONS } from '../downloads/download-row.component';
import { Settings, SettingsService } from './settings.service';

/** What `protocol_status` reports (`src-tauri/src/protocol.rs`). */
export interface ProtocolStatus {
  readonly supported: boolean;
  /** `magnet:` links open Riptide. */
  readonly magnet: boolean;
  /** `.torrent` files offer (and may default to) Riptide. */
  readonly torrent: boolean;
}

/** What `media_tools_info` reports (`src-tauri/src/media.rs`). */
export interface MediaToolsInfo {
  readonly available: boolean;
  readonly version: string | null;
  readonly ffmpeg: boolean;
}

/** `http://`, `https://` or `socks5(h)://` + host[:port], optional `user:pass@`, nothing after. */
export function isValidProxyUrl(raw: string): boolean {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return false;
  }
  return (
    ['http:', 'https:', 'socks5:', 'socks5h:'].includes(url.protocol) &&
    url.hostname.length > 0 &&
    (url.pathname === '' || url.pathname === '/') &&
    url.search === '' &&
    url.hash === ''
  );
}

/**
 * Settings, one scrolling page in sections. Every control writes straight to the settings
 * signal; there is no save button because nothing here is dangerous to change.
 */
@Component({
  selector: 'app-settings-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES, ButtonComponent, IconComponent, SegmentedControlComponent, SpeedLimitControlComponent],
  template: `
    <div class="page">
      <header class="page-head">
        <h1 class="page-title">{{ i18n.t('settings.title') }}</h1>
        <p class="page-sub">{{ i18n.t('settings.subtitle') }}</p>
      </header>

      @if (updates.badgeVersion(); as version) {
        <a class="update-banner" href="#about" role="status" (click)="goAbout($event)">
          <span class="update-dot" aria-hidden="true"></span>
          <span>{{ i18n.t('settings.updateBanner').replace('{version}', version) }}</span>
          <span class="update-banner-link">{{ i18n.t('settings.updateBannerLink') }}</span>
        </a>
      }

      <section class="section" id="appearance">
        <h2 class="section-title">{{ i18n.t('settings.appearance') }}</h2>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('lang.label') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.langDesc') }}</span>
          </div>
          <app-segmented-control [label]="i18n.t('lang.label')" [options]="langOptions()" [optionLang]="langAttr" [value]="i18n.lang()" (valueChange)="i18n.setLang($event)" />
        </div>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('theme.label') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.themeDesc') }}</span>
          </div>
          <app-segmented-control [label]="i18n.t('theme.label')" [options]="themeOptions()" [value]="theme.theme()" (valueChange)="theme.setTheme($event)" />
        </div>
      </section>

      <section class="section" id="downloads">
        <h2 class="section-title">{{ i18n.t('settings.downloads') }}</h2>
        <div class="setting column">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.defaultDir') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.defaultDirDesc') }}</span>
          </div>
          <div class="input-row">
            <app-icon name="folder" [size]="16" class="input-icon" />
            <input class="input" type="text" dir="ltr" [value]="s().defaultDir" (change)="patch({ defaultDir: value($event) })" />
          </div>
        </div>

        <div class="setting column">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.categoryDirs') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.categoryDirsDesc') }}</span>
          </div>
          <div class="table">
            @for (c of categories; track c) {
              <div class="table-row">
                <span class="table-cat"><app-icon [name]="icons[c]" [size]="16" />{{ i18n.t('category.' + c) }}</span>
                <input class="input small" type="text" dir="ltr" [value]="s().categoryDirs[c] ?? ''" [placeholder]="s().defaultDir" (change)="settings.setCategoryDir(c, value($event))" />
              </div>
            }
          </div>
        </div>

        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.connections') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.connectionsDesc') }}</span>
          </div>
          <div class="range">
            <input class="slider" type="range" min="1" max="32" step="1" [value]="s().connections" [attr.aria-label]="i18n.t('settings.connections')" (input)="patch({ connections: number($event) })" />
            <span class="range-value num">{{ s().connections }}</span>
          </div>
        </div>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.perHost') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.perHostDesc') }}</span>
          </div>
          <div class="range">
            <input class="slider" type="range" min="1" max="16" step="1" [value]="s().perHost" [attr.aria-label]="i18n.t('settings.perHost')" (input)="patch({ perHost: number($event) })" />
            <span class="range-value num">{{ s().perHost }}</span>
          </div>
        </div>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.speedLimit') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.speedLimitDesc') }}</span>
          </div>
          <app-speed-limit-control [value]="s().speedLimitBps" [label]="i18n.t('settings.speedLimit')" (valueChange)="patch({ speedLimitBps: $event })" />
        </div>
      </section>

      <section class="section" id="behaviour">
        <h2 class="section-title">{{ i18n.t('settings.behaviour') }}</h2>
        @for (item of toggles; track item.key) {
          <label class="setting toggle-row">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('settings.' + item.key) }}</span>
              <span class="setting-desc">{{ i18n.t('settings.' + item.key + 'Desc') }}</span>
            </div>
            <span class="switch" [class.is-on]="s()[item.key]">
              <input type="checkbox" role="switch" [checked]="s()[item.key]" (change)="toggle(item.key, $event)" />
              <span class="knob"></span>
            </span>
          </label>
        }
      </section>

      <section class="section" id="torrents">
        <h2 class="section-title">{{ i18n.t('settings.torrents') }}</h2>
        <label class="setting toggle-row">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.torrentSeeding') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.torrentSeedingDesc') }}</span>
          </div>
          <span class="switch" [class.is-on]="s().torrentSeeding">
            <input type="checkbox" role="switch" [checked]="s().torrentSeeding" (change)="patch({ torrentSeeding: checked($event) })" />
            <span class="knob"></span>
          </span>
        </label>
        @if (s().torrentSeeding) {
          <div class="setting">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('settings.torrentRatio') }}</span>
              <span class="setting-desc">{{ i18n.t('settings.torrentRatioDesc') }}</span>
            </div>
            <input class="input small" type="number" min="0" max="1000" step="0.1" dir="ltr" [value]="s().torrentRatioPercent / 100" [attr.aria-label]="i18n.t('settings.torrentRatio')" (change)="patch({ torrentRatioPercent: Math.round(number($event) * 100) })" />
          </div>
          <div class="setting">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('settings.torrentMinutes') }}</span>
              <span class="setting-desc">{{ i18n.t('settings.torrentMinutesDesc') }}</span>
            </div>
            <input class="input small" type="number" min="0" max="525600" step="10" dir="ltr" [value]="s().torrentMinutes" [attr.aria-label]="i18n.t('settings.torrentMinutes')" (change)="patch({ torrentMinutes: Math.max(0, Math.round(number($event))) })" />
          </div>
        }
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.torrentUpload') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.torrentUploadDesc') }}</span>
          </div>
          <app-speed-limit-control [value]="s().torrentUploadBps" [label]="i18n.t('settings.torrentUpload')" (valueChange)="patch({ torrentUploadBps: $event })" />
        </div>
        <label class="setting toggle-row">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.torrentDht') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.torrentDhtDesc') }}</span>
          </div>
          <span class="switch" [class.is-on]="s().torrentDht">
            <input type="checkbox" role="switch" [checked]="s().torrentDht" (change)="patch({ torrentDht: checked($event) })" />
            <span class="knob"></span>
          </span>
        </label>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.torrentPort') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.torrentPortDesc') }}</span>
          </div>
          <input class="input small" type="number" min="0" max="65535" step="1" dir="ltr" [value]="s().torrentPort" [attr.aria-label]="i18n.t('settings.torrentPort')" (change)="patch({ torrentPort: Math.min(65535, Math.max(0, Math.round(number($event)))) })" />
        </div>
      </section>

      <section class="section" id="integration">
        <h2 class="section-title">{{ i18n.t('settings.integration') }}</h2>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.browser') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.browserDesc') }}</span>
          </div>
          <div class="browser-row">
            <span class="dot" [class.is-on]="browser.connected()"></span>
            <span class="browser-state">{{ i18n.t(browserStateKey()) }}</span>
            <button appButton variant="secondary" type="button" [disabled]="browser.busy()" (click)="toggleBrowser()">{{ i18n.t(browser.connected() ? 'settings.browserRemove' : 'settings.browserInstall') }}</button>
          </div>
        </div>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.protocols') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.protocolsDesc') }}</span>
          </div>
          <div class="browser-row">
            <span class="dot" [class.is-on]="protocols()?.magnet === true"></span>
            <span class="browser-state">{{ i18n.t(protocolStateKey()) }}</span>
            <button appButton variant="secondary" type="button" [disabled]="!tauriAvailable || protocolsBusy() || protocols()?.supported === false" (click)="toggleProtocols()">
              {{ i18n.t(protocols()?.magnet ? 'settings.protocolsRemove' : 'settings.protocolsInstall') }}
            </button>
          </div>
        </div>
        <div class="setting column ext-card" id="extension">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.ext') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.extDesc') }}</span>
          </div>
          <ol class="ext-steps">
            <li>{{ i18n.t('settings.extStep1') }}</li>
            <li>{{ i18n.t('settings.extStep2') }}</li>
            <li>{{ i18n.t('settings.extStep3') }}</li>
          </ol>
          @for (b of extBrowsers(); track b.browser) {
            <div class="ext-browser">
              <span class="ext-name">{{ i18n.t(b.browser === 'chrome' ? 'settings.extChrome' : 'settings.extEdge') }}</span>
              <span class="dot" [class.is-on]="browser.bridgeRegistered(b.browser)"></span>
              <span class="browser-state">{{ i18n.t(browser.bridgeRegistered(b.browser) ? 'settings.extBridgeOn' : 'settings.extBridgeOff') }}</span>
              <span class="ext-actions">
                <button appButton variant="secondary" type="button" [disabled]="!tauriAvailable || !b.folderFound" (click)="openExtensionFolder(b.browser)">{{ i18n.t('settings.extOpenFolder') }}</button>
                <button appButton variant="secondary" type="button" [disabled]="!tauriAvailable || !b.installed" (click)="openExtensionsPage(b.browser)">{{ i18n.t(b.browser === 'chrome' ? 'settings.extOpenChrome' : 'settings.extOpenEdge') }}</button>
              </span>
            </div>
          }
          <p class="ext-id">{{ i18n.t('settings.extId') }} <code class="num" dir="ltr">{{ extId() }}</code></p>
        </div>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.proxy') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.proxyDesc') }}</span>
          </div>
          <app-segmented-control [label]="i18n.t('settings.proxy')" [options]="proxyOptions()" [value]="s().proxyMode" (valueChange)="patch({ proxyMode: $event })" />
        </div>
        @if (s().proxyMode === 'manual') {
          <div class="setting column">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('settings.proxyUrl') }}</span>
            </div>
            <input
              class="input"
              type="text"
              dir="ltr"
              placeholder="socks5://127.0.0.1:1080"
              [class.is-invalid]="proxyInvalid()"
              [attr.aria-invalid]="proxyInvalid()"
              [value]="s().proxyUrl"
              (input)="proxyInvalid.set(false)"
              (change)="setProxyUrl(value($event))"
            />
            @if (proxyInvalid()) {
              <p class="field-error" role="alert">{{ i18n.t('settings.proxyUrlInvalid') }}</p>
            }
          </div>
        }
      </section>

      <section class="section" id="about">
        <h2 class="section-title">{{ i18n.t('settings.about') }}</h2>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">Riptide <span class="num">{{ tauri.appInfo()?.version ?? '0.1.0' }}</span></span>
            <span class="setting-desc">{{ i18n.t('settings.aboutDesc') }}</span>
            @if (prerelease()) {
              <span class="setting-desc">{{ i18n.t('settings.channel.prerelease') }}</span>
            }
          </div>
          <div class="about-actions">
            <app-segmented-control [label]="i18n.t('settings.channel')" [options]="channelOptions()" [value]="channel()" (valueChange)="setChannel($event)" />
            <button appButton variant="secondary" icon="retry" type="button" [disabled]="updates.status() === 'checking' || updates.status() === 'installing'" (click)="checkUpdates()">{{ i18n.t(updates.status() === 'checking' ? 'settings.checkingUpdates' : 'settings.checkUpdates') }}</button>
          </div>
        </div>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.video') }}</span>
            <span class="setting-desc">{{ videoStatus() }}</span>
          </div>
          <div class="about-actions">
            <button appButton variant="secondary" icon="retry" type="button" [busy]="videoBusy()" [disabled]="!tauriAvailable || videoBusy()" (click)="updateVideo()">{{ i18n.t(videoBusy() ? 'settings.videoUpdating' : 'settings.videoUpdate') }}</button>
          </div>
        </div>
        @if (updates.available(); as update) {
          <div class="setting column update-card" role="status">
            <div class="setting-text">
              <span class="setting-name">{{ updateTitle(update.version) }}</span>
              @if (update.notes) {
                <span class="setting-desc update-notes" dir="auto">{{ update.notes }}</span>
              }
            </div>
            <div>
              <button appButton variant="primary" type="button" [disabled]="updates.status() === 'installing'" (click)="installUpdate()">{{ i18n.t(updates.status() === 'installing' ? 'settings.updateInstalling' : 'settings.updateRestart') }}</button>
            </div>
          </div>
        }
        <label class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.autoUpdate') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.autoUpdateDesc') }}</span>
          </div>
          <span class="switch" [class.is-on]="s().checkUpdates">
            <input type="checkbox" role="switch" [checked]="s().checkUpdates" (change)="patch({ checkUpdates: checked($event) })" />
            <span class="knob"></span>
          </span>
        </label>
        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('settings.reset') }}</span>
            <span class="setting-desc">{{ i18n.t('settings.resetDesc') }}</span>
          </div>
          <button appButton variant="danger" type="button" (click)="reset()">{{ i18n.t('settings.resetAction') }}</button>
        </div>
      </section>
    </div>
  `,
  styles: `
    :host {
      display: block;
      block-size: 100%;
      overflow: auto;
    }

    .page {
      max-inline-size: 760px;
      padding: var(--space-8) var(--space-8) var(--space-16);
      margin-inline: auto;
    }

    .page-title {
      font-size: var(--text-2xl);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
    }

    .page-sub {
      margin-block-start: var(--space-1);
      color: var(--color-text-muted);
    }

    .section {
      margin-block-start: var(--space-8);
      padding-block-start: var(--space-5);
      border-block-start: 1px solid var(--color-border);
    }

    .section-title {
      font-size: var(--text-md);
      font-weight: var(--weight-semibold);
      margin-block-end: var(--space-2);
    }

    .setting {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-6);
      padding-block: var(--space-4);
    }

    .setting + .setting {
      border-block-start: 1px solid var(--color-border);
    }

    .setting.column {
      flex-direction: column;
      align-items: stretch;
      gap: var(--space-3);
    }

    .setting-text {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-inline-size: 0;
    }

    .setting-name {
      font-weight: var(--weight-medium);
    }

    .setting-desc {
      color: var(--color-text-muted);
      font-size: var(--text-sm);
      line-height: var(--leading-normal);
    }

    .input-row {
      position: relative;
      display: flex;
      align-items: center;
    }

    .input-icon {
      position: absolute;
      inset-inline-start: 10px;
      color: var(--color-text-muted);
      pointer-events: none;
    }

    .input-row .input {
      padding-inline-start: 34px;
    }

    .input {
      inline-size: 100%;
      block-size: 36px;
      padding-inline: 12px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .input.is-invalid {
      border-color: var(--color-danger);
    }

    .field-error {
      color: var(--color-danger);
      font-size: var(--text-sm);
    }

    .input.small {
      block-size: 30px;
    }

    .setting > .input.small[type='number'] {
      inline-size: 140px;
      flex: none;
    }

    .input:focus {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .input::placeholder {
      color: var(--color-text-muted);
    }

    .table {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }

    .table-row {
      display: grid;
      grid-template-columns: 160px 1fr;
      align-items: center;
      gap: var(--space-3);
    }

    .table-cat {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .range {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-inline-size: 240px;
    }

    .slider {
      flex: 1;
      accent-color: var(--color-accent);
    }

    .range-value {
      inline-size: 2ch;
      text-align: end;
      font-weight: var(--weight-semibold);
    }

    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1);
      justify-content: flex-end;
    }

    .chip {
      block-size: 28px;
      padding-inline: 10px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-full);
      background: transparent;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      cursor: pointer;
    }

    .chip.is-active {
      border-color: transparent;
      background: var(--color-accent);
      color: var(--color-on-accent);
    }

    .toggle-row {
      cursor: pointer;
    }

    .switch {
      position: relative;
      flex-shrink: 0;
      inline-size: 40px;
      block-size: 22px;
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
      border: 1px solid var(--color-border-strong);
      transition: background-color var(--motion-base) var(--ease-standard);
    }

    .switch input {
      position: absolute;
      inset: 0;
      margin: 0;
      opacity: 0;
      cursor: pointer;
    }

    .knob {
      position: absolute;
      inset-block-start: 2px;
      inset-inline-start: 2px;
      inline-size: 16px;
      block-size: 16px;
      border-radius: var(--radius-full);
      background: var(--color-text-muted);
      transition:
        transform var(--motion-base) var(--ease-emphasized),
        background-color var(--motion-base) var(--ease-standard);
    }

    .switch.is-on {
      background: var(--color-accent);
      border-color: transparent;
    }

    .switch.is-on .knob {
      background: var(--color-on-accent);
      transform: translateX(calc(var(--dir-sign) * 18px));
    }

    .switch:has(input:focus-visible) {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .browser-row {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }

    .dot {
      inline-size: 8px;
      block-size: 8px;
      border-radius: var(--radius-full);
      background: var(--color-text-muted);
    }

    .dot.is-on {
      background: var(--color-success);
      box-shadow: 0 0 0 3px var(--state-completed-soft);
    }

    .browser-state {
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .ext-steps {
      margin: 0;
      padding-inline-start: var(--space-5);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      line-height: 1.7;
    }

    .ext-browser {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-3);
    }

    .ext-name {
      min-inline-size: 7rem;
      font-weight: 500;
    }

    .ext-actions {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      margin-inline-start: auto;
    }

    .ext-id {
      margin: 0;
      color: var(--color-text-muted);
      font-size: var(--text-sm);
    }

    .ext-id code {
      user-select: all;
      unicode-bidi: isolate;
    }

    .about-actions {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }

    .update-banner {
      display: flex;
      align-items: center;
      gap: var(--space-2, 8px);
      padding: 10px 14px;
      margin-block-end: 16px;
      border: 1px solid var(--color-accent);
      border-radius: var(--radius-md, 8px);
      color: var(--color-text);
      text-decoration: none;
    }

    .update-dot {
      inline-size: 8px;
      block-size: 8px;
      border-radius: 50%;
      background: var(--color-accent);
    }

    .update-banner-link {
      margin-inline-start: auto;
      color: var(--color-accent);
    }

    .update-card .setting-name {
      color: var(--color-accent);
    }

    .update-notes {
      white-space: pre-line;
      max-block-size: 12rem;
      overflow: auto;
    }
  `,
})
export class SettingsPageComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly theme = inject(ThemeService);
  protected readonly tauri = inject(TauriService);
  protected readonly settings = inject(SettingsService);
  private readonly toasts = inject(ToastService);
  protected readonly browser = inject(BrowserIntegrationService);
  protected readonly updates = inject(UpdateService);
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

  /** What `media_tools_info` said; `null` while loading or in the browser preview. */
  protected readonly videoInfo = signal<MediaToolsInfo | null>(null);
  protected readonly videoBusy = signal(false);
  protected readonly videoStatus = computed(() => {
    if (!this.tauriAvailable) {
      return this.i18n.t('settings.videoPreview');
    }
    const info = this.videoInfo();
    if (info === null) {
      return this.i18n.t('settings.videoChecking');
    }
    if (!info.available) {
      return this.i18n.t('settings.videoMissing');
    }
    return this.i18n
      .t(info.ffmpeg ? 'settings.videoReady' : 'settings.videoNoFfmpeg')
      .replace('{version}', info.version ?? '?');
  });

  protected readonly s = this.settings.settings;

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

  constructor() {
    void this.loadProtocols();
    void this.browser.refresh();
    void this.browser.refreshExtension();
    void this.loadVideoInfo();
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

  private async loadVideoInfo(): Promise<void> {
    if (!this.tauriAvailable) {
      return;
    }
    try {
      this.videoInfo.set(await this.invoke<MediaToolsInfo>('media_tools_info'));
    } catch {
      this.videoInfo.set({ available: false, version: null, ffmpeg: false });
    }
  }

  protected async updateVideo(): Promise<void> {
    if (this.videoBusy()) {
      return;
    }
    this.videoBusy.set(true);
    try {
      const version = await this.invoke<string>('update_media_tools');
      this.toasts.show('success', this.i18n.t('toast.videoUpdated').replace('{version}', version));
      await this.loadVideoInfo();
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.videoUpdateFailed'));
    } finally {
      this.videoBusy.set(false);
    }
  }
  protected readonly categories = CATEGORY_KEYS;
  protected readonly icons = CATEGORY_ICONS;
  protected readonly Math = Math;
  /** The typed proxy URL was refused (not saved, so the engine keeps the last good one). */
  protected readonly proxyInvalid = signal(false);
  protected readonly langAttr: Readonly<Record<Lang, string>> = { ar: 'ar', en: 'en' };

  protected readonly toggles: readonly { key: 'confirmOnExit' | 'startWithWindows' | 'closeToTray' | 'clipboardMonitor' | 'notifyOnComplete' | 'markDownloads' | 'allowPrivateRedirects' }[] = [
    { key: 'clipboardMonitor' },
    { key: 'notifyOnComplete' },
    { key: 'markDownloads' },
    { key: 'allowPrivateRedirects' },
    { key: 'confirmOnExit' },
    { key: 'closeToTray' },
    { key: 'startWithWindows' },
  ];

  protected readonly langOptions = computed<readonly SegmentedOption<Lang>[]>(() =>
    LANGS.map((value) => ({ value, label: this.i18n.t(`lang.${value}`) })),
  );

  protected readonly themeOptions = computed<readonly SegmentedOption<Theme>[]>(() =>
    THEMES.map((value) => ({ value, label: this.i18n.t(`theme.${value}`) })),
  );

  protected readonly proxyOptions = computed<readonly SegmentedOption<Settings['proxyMode']>[]>(() =>
    (['system', 'none', 'manual'] as const).map((value) => ({ value, label: this.i18n.t(`settings.proxy.${value}`) })),
  );

  protected readonly channelOptions = computed<readonly SegmentedOption<Settings['channel']>[]>(() =>
    (['stable', 'beta'] as const).map((value) => ({ value, label: this.i18n.t(`settings.channel.${value}`) })),
  );

  /** A pre-release build only has the beta feed, so the selector shows (and keeps) Beta. */
  protected readonly prerelease = computed(() => isPrerelease(this.tauri.appInfo()?.version));
  /** The channel the selector shows: Beta for a pre-release build, the saved choice otherwise. */
  protected readonly channel = computed(() => effectiveChannel(this.s().channel, this.tauri.appInfo()?.version));

  protected setChannel(channel: Settings['channel']): void {
    // the saved setting stays untouched on a pre-release build: only a stable build has a choice
    if (!this.prerelease()) {
      this.patch({ channel });
    }
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected number(event: Event): number {
    return Number((event.target as HTMLInputElement).value);
  }

  protected patch(change: Partial<Settings>): void {
    this.settings.patch(change);
  }

  protected setProxyUrl(raw: string): void {
    const url = raw.trim();
    if (url !== '' && !isValidProxyUrl(url)) {
      this.proxyInvalid.set(true);
      return;
    }
    this.proxyInvalid.set(false);
    this.settings.patch({ proxyUrl: url });
  }

  protected toggle(key: (typeof this.toggles)[number]['key'], event: Event): void {
    this.settings.patch({ [key]: (event.target as HTMLInputElement).checked });
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

  protected checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked;
  }

  protected updateTitle(version: string): string {
    return this.i18n.t('settings.updateAvailable').replace('{version}', version);
  }

  protected async checkUpdates(): Promise<void> {
    try {
      const update = await this.updates.check(this.channel());
      if (update === null) {
        this.toasts.show('success', this.i18n.t('toast.upToDate'));
      }
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.updateCheckFailed'));
    }
  }

  protected goAbout(event: Event): void {
    event.preventDefault();
    document.getElementById('about')?.scrollIntoView({ block: 'start' });
  }

  protected installUpdate(): void {
    void this.updates.installOrToast();
  }

  protected reset(): void {
    this.settings.reset();
    this.toasts.show('info', this.i18n.t('toast.settingsReset'));
  }

  protected categoryKey(c: string): CategoryKey {
    return c as CategoryKey;
  }
}
