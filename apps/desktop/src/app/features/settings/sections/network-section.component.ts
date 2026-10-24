import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { I18nService } from '../../../i18n/i18n.service';
import { SegmentedControlComponent, SegmentedOption } from '../../../ui/segmented-control.component';
import { SwitchComponent } from '../../../ui/switch.component';
import { SettingGroupComponent, SettingRowComponent } from '../setting-layout.component';
import { Settings, SettingsService } from '../settings.service';

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

/** Settings → Network: the proxy and what redirects may reach. */
@Component({
  selector: 'app-settings-network',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SettingGroupComponent, SettingRowComponent, SwitchComponent, SegmentedControlComponent],
  template: `
    <app-setting-group anchor="network" [heading]="i18n.t('settings.proxy')">
      <app-setting-row [label]="i18n.t('set.proxyMode')" [description]="i18n.t('settings.proxyDesc')">
        <app-segmented-control [label]="i18n.t('settings.proxy')" [options]="proxyOptions()" [value]="s().proxyMode" (valueChange)="patch({ proxyMode: $event })" />
      </app-setting-row>
      @if (s().proxyMode === 'manual') {
        <app-setting-row [stack]="true" [label]="i18n.t('settings.proxyUrl')" [description]="i18n.t('set.proxyUrlDesc')">
          <input
            class="sr-input"
            type="text"
            dir="ltr"
            placeholder="socks5://127.0.0.1:1080"
            [attr.aria-label]="i18n.t('settings.proxyUrl')"
            [class.is-invalid]="proxyInvalid()"
            [attr.aria-invalid]="proxyInvalid()"
            [value]="s().proxyUrl"
            (input)="proxyInvalid.set(false)"
            (change)="setProxyUrl(value($event))"
          />
          @if (proxyInvalid()) {
            <p class="sr-error" role="alert">{{ i18n.t('settings.proxyUrlInvalid') }}</p>
          }
        </app-setting-row>
      }
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.redirects')">
      <app-setting-row [clickable]="true" [label]="i18n.t('settings.allowPrivateRedirects')" [description]="i18n.t('settings.allowPrivateRedirectsDesc')">
        <app-switch [checked]="s().allowPrivateRedirects" [label]="i18n.t('settings.allowPrivateRedirects')" (checkedChange)="patch({ allowPrivateRedirects: $event })" />
      </app-setting-row>
    </app-setting-group>
  `,
})
export class NetworkSectionComponent {
  protected readonly i18n = inject(I18nService);
  private readonly settings = inject(SettingsService);

  protected readonly s = this.settings.settings;
  /** The typed proxy URL was refused (not saved, so the engine keeps the last good one). */
  protected readonly proxyInvalid = signal(false);

  protected readonly proxyOptions = computed<readonly SegmentedOption<Settings['proxyMode']>[]>(() =>
    (['system', 'none', 'manual'] as const).map((value) => ({ value, label: this.i18n.t(`settings.proxy.${value}`) })),
  );

  protected patch(change: Partial<Settings>): void {
    this.settings.patch(change);
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
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
}
