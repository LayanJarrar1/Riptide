import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { effectiveChannel, isPrerelease } from '../../../core/update-channel';
import { I18nService } from '../../../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../../tauri/tauri-bridge';
import { TauriService } from '../../../tauri/tauri.service';
import { ButtonComponent } from '../../../ui/button.component';
import { SegmentedControlComponent, SegmentedOption } from '../../../ui/segmented-control.component';
import { SwitchComponent } from '../../../ui/switch.component';
import { DOCS_URL, REPO_URL } from '../../shell/app-commands.service';
import { SettingGroupComponent, SettingRowComponent } from '../setting-layout.component';
import { Settings, SettingsService } from '../settings.service';
import { UpdateCardComponent } from '../update-card.component';

/**
 * Settings → Updates & About. The update card (version, last check, state, Check now, release
 * notes, install actions, check interval) is the updater's own component; this section adds the
 * channel and the on/off switch around it, then the app's identity and licences.
 */
@Component({
  selector: 'app-settings-updates',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SettingGroupComponent, SettingRowComponent, SwitchComponent, SegmentedControlComponent, ButtonComponent, UpdateCardComponent],
  template: `
    <app-setting-group anchor="about" [heading]="i18n.t('set.g.updates')">
      <app-update-card />
      <app-setting-row [clickable]="true" [label]="i18n.t('settings.autoUpdate')" [description]="i18n.t('settings.autoUpdateDesc')">
        <app-switch [checked]="s().checkUpdates" [label]="i18n.t('settings.autoUpdate')" (checkedChange)="patch({ checkUpdates: $event })" />
      </app-setting-row>
      <app-setting-row [label]="i18n.t('settings.channel')" [description]="prerelease() ? i18n.t('settings.channel.prerelease') : i18n.t('set.channelDesc')">
        <app-segmented-control [label]="i18n.t('settings.channel')" [options]="channelOptions()" [value]="channel()" [disabled]="prerelease()" (valueChange)="setChannel($event)" />
      </app-setting-row>
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.about')">
      <app-setting-row [label]="'Riptide ' + (tauri.appInfo()?.version ?? '0.1.0')" [description]="i18n.t('settings.aboutDesc')">
        @if (tauri.appInfo()?.build; as build) {
          <span class="sr-value num" dir="ltr">{{ i18n.t('set.build') }} {{ build }}</span>
        }
      </app-setting-row>
      <app-setting-row [label]="i18n.t('set.licences')" [description]="i18n.t('set.licencesDesc')">
        <button appButton variant="secondary" size="sm" icon="external" type="button" (click)="open(repoUrl)">{{ i18n.t('set.sourceCode') }}</button>
      </app-setting-row>
      <app-setting-row [label]="i18n.t('set.guide')" [description]="i18n.t('set.guideDesc')">
        <button appButton variant="secondary" size="sm" icon="book" type="button" (click)="open(docsUrl)">{{ i18n.t('set.openGuide') }}</button>
      </app-setting-row>
    </app-setting-group>
  `,
})
export class UpdatesSectionComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly tauri = inject(TauriService);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  protected readonly repoUrl = REPO_URL;
  protected readonly docsUrl = DOCS_URL;
  private readonly settings = inject(SettingsService);

  protected readonly s = this.settings.settings;

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

  /** Opens one of the two fixed links in the system browser. */
  protected open(url: string): void {
    if (!this.tauriAvailable) {
      window.open(url, '_blank', 'noopener,noreferrer');
      return;
    }
    this.invoke<void>('plugin:opener|open_url', { url }).catch((error: unknown) => console.warn('[settings] open_url failed', error));
  }

  protected patch(change: Partial<Settings>): void {
    this.settings.patch(change);
  }
}
