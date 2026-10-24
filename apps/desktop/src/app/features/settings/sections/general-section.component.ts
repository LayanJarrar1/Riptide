import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { I18nService, Lang, LANGS } from '../../../i18n/i18n.service';
import { Theme, THEMES, ThemeService } from '../../../theme/theme.service';
import { SegmentedControlComponent, SegmentedOption } from '../../../ui/segmented-control.component';
import { SwitchComponent } from '../../../ui/switch.component';
import { SettingGroupComponent, SettingRowComponent } from '../setting-layout.component';
import { Settings, SettingsService } from '../settings.service';

type GeneralToggle = 'startWithWindows' | 'closeToTray' | 'confirmOnExit' | 'clipboardMonitor' | 'notifyOnComplete';

/** Settings → General: language and look, how Riptide starts and closes, clipboard and notifications. */
@Component({
  selector: 'app-settings-general',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SettingGroupComponent, SettingRowComponent, SwitchComponent, SegmentedControlComponent],
  template: `
    <app-setting-group anchor="appearance" [heading]="i18n.t('set.g.look')">
      <app-setting-row [label]="i18n.t('lang.label')" [description]="i18n.t('settings.langDesc')">
        <app-segmented-control [label]="i18n.t('lang.label')" [options]="langOptions()" [optionLang]="langAttr" [value]="i18n.lang()" (valueChange)="i18n.setLang($event)" />
      </app-setting-row>
      <app-setting-row [label]="i18n.t('set.theme')" [description]="i18n.t('settings.themeDesc')">
        <app-segmented-control [label]="i18n.t('set.theme')" [options]="themeOptions()" [value]="theme.theme()" (valueChange)="theme.setTheme($event)" />
      </app-setting-row>
      <app-setting-row [label]="i18n.t('menu.density')" [description]="i18n.t('set.densityDesc')">
        <app-segmented-control [label]="i18n.t('menu.density')" [options]="densityOptions()" [value]="s().density" (valueChange)="patch({ density: $event })" />
      </app-setting-row>
    </app-setting-group>

    <app-setting-group anchor="behaviour" [heading]="i18n.t('set.g.startup')">
      @for (key of startup; track key) {
        <app-setting-row [clickable]="true" [label]="i18n.t('settings.' + key)" [description]="i18n.t('settings.' + key + 'Desc')">
          <app-switch [checked]="s()[key]" [label]="i18n.t('settings.' + key)" (checkedChange)="toggle(key, $event)" />
        </app-setting-row>
      }
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.notify')">
      @for (key of notify; track key) {
        <app-setting-row [clickable]="true" [label]="i18n.t('settings.' + key)" [description]="i18n.t('settings.' + key + 'Desc')">
          <app-switch [checked]="s()[key]" [label]="i18n.t('settings.' + key)" (checkedChange)="toggle(key, $event)" />
        </app-setting-row>
      }
    </app-setting-group>
  `,
})
export class GeneralSectionComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly theme = inject(ThemeService);
  private readonly settings = inject(SettingsService);

  protected readonly s = this.settings.settings;
  protected readonly startup: readonly GeneralToggle[] = ['startWithWindows', 'closeToTray', 'confirmOnExit'];
  protected readonly notify: readonly GeneralToggle[] = ['clipboardMonitor', 'notifyOnComplete'];
  protected readonly langAttr: Readonly<Record<Lang, string>> = { ar: 'ar', en: 'en' };

  protected readonly langOptions = computed<readonly SegmentedOption<Lang>[]>(() =>
    LANGS.map((value) => ({ value, label: this.i18n.t(`lang.${value}`) })),
  );

  protected readonly themeOptions = computed<readonly SegmentedOption<Theme>[]>(() =>
    THEMES.map((value) => ({ value, label: this.i18n.t(`theme.${value}`) })),
  );

  protected readonly densityOptions = computed<readonly SegmentedOption<Settings['density']>[]>(() => [
    { value: 'comfortable', label: this.i18n.t('menu.densityComfortable') },
    { value: 'compact', label: this.i18n.t('menu.densityCompact') },
  ]);

  protected toggle(key: GeneralToggle, on: boolean): void {
    this.settings.patch({ [key]: on });
  }

  protected patch(change: Partial<Settings>): void {
    this.settings.patch(change);
  }
}
