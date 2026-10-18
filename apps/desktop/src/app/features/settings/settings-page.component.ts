import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { BrowserIntegrationService } from '../../core/browser-integration.service';
import { FORMAT_PIPES } from '../../core/format.pipes';
import { UpdateService } from '../../core/update.service';
import { I18nService, Lang, LANGS } from '../../i18n/i18n.service';
import { CATEGORY_KEYS, CategoryKey } from '../../model/download';
import { TauriService } from '../../tauri/tauri.service';
import { Theme, THEMES, ThemeService } from '../../theme/theme.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { SegmentedControlComponent, SegmentedOption } from '../../ui/segmented-control.component';
import { SpeedLimitControlComponent } from '../../ui/speed-limit-control.component';
import { ToastService } from '../../ui/toast.service';
import { CATEGORY_ICONS } from '../downloads/download-row.component';
import { Settings, SettingsService } from './settings.service';

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
          </div>
          <div class="about-actions">
            <app-segmented-control [label]="i18n.t('settings.channel')" [options]="channelOptions()" [value]="s().channel" (valueChange)="patch({ channel: $event })" />
            <button appButton variant="secondary" icon="retry" type="button" [disabled]="updates.status() === 'checking' || updates.status() === 'installing'" (click)="checkUpdates()">{{ i18n.t(updates.status() === 'checking' ? 'settings.checkingUpdates' : 'settings.checkUpdates') }}</button>
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

    .about-actions {
      display: flex;
      align-items: center;
      gap: var(--space-3);
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

  protected readonly s = this.settings.settings;

  /** Connected / not installed / host missing (a broken install). */
  protected readonly browserStateKey = computed(() => {
    if (this.browser.connected()) {
      return 'settings.browserOn';
    }
    return this.browser.hostMissing() ? 'settings.browserNoHost' : 'settings.browserOff';
  });

  constructor() {
    void this.browser.refresh();
  }
  protected readonly categories = CATEGORY_KEYS;
  protected readonly icons = CATEGORY_ICONS;
  /** The typed proxy URL was refused (not saved, so the engine keeps the last good one). */
  protected readonly proxyInvalid = signal(false);
  protected readonly langAttr: Readonly<Record<Lang, string>> = { ar: 'ar', en: 'en' };

  protected readonly toggles: readonly { key: 'confirmOnExit' | 'startWithWindows' | 'closeToTray' | 'clipboardMonitor' | 'notifyOnComplete' }[] = [
    { key: 'clipboardMonitor' },
    { key: 'notifyOnComplete' },
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
      const update = await this.updates.check(this.s().channel);
      if (update === null) {
        this.toasts.show('success', this.i18n.t('toast.upToDate'));
      }
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.updateCheckFailed'));
    }
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
