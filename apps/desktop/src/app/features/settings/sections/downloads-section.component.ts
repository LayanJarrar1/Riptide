import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { IS_MAC } from '../../../core/platform';
import { I18nService } from '../../../i18n/i18n.service';
import { CATEGORY_KEYS, CategoryKey } from '../../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../../tauri/tauri-bridge';
import { ButtonComponent } from '../../../ui/button.component';
import { IconComponent } from '../../../ui/icon.component';
import { SpeedLimitControlComponent } from '../../../ui/speed-limit-control.component';
import { SwitchComponent } from '../../../ui/switch.component';
import { ToastService } from '../../../ui/toast.service';
import { CATEGORY_ICONS } from '../../downloads/download-row.component';
import { SettingGroupComponent, SettingRowComponent } from '../setting-layout.component';
import { CATEGORY_FOLDERS, joinPath, organizedRoot, Settings, SettingsService, TORRENTS_FOLDER } from '../settings.service';

/** What `media_tools_info` reports (`src-tauri/src/media.rs`). */
export interface MediaToolsInfo {
  readonly available: boolean;
  readonly version: string | null;
  readonly ffmpeg: boolean;
}

/** Settings → Downloads: where files go, speed and connections, the queue, torrents, video support. */
@Component({
  selector: 'app-settings-downloads',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SettingGroupComponent, SettingRowComponent, SwitchComponent, ButtonComponent, IconComponent, SpeedLimitControlComponent],
  template: `
    <app-setting-group anchor="downloads" [heading]="i18n.t('set.g.location')">
      <app-setting-row [stack]="true" [label]="i18n.t('settings.defaultDir')" [description]="i18n.t('settings.defaultDirDesc')">
        <input class="sr-input" type="text" dir="ltr" [attr.aria-label]="i18n.t('settings.defaultDir')" [value]="s().defaultDir" (change)="patch({ defaultDir: value($event) })" />
      </app-setting-row>
      <app-setting-row [clickable]="true" [label]="i18n.t('settings.organize')" [description]="i18n.t('settings.organizeDesc')">
        <app-switch [checked]="s().organize" [label]="i18n.t('settings.organize')" (checkedChange)="patch({ organize: $event })" />
      </app-setting-row>
      @if (s().organize) {
        <app-setting-row [stack]="true" [label]="i18n.t('settings.organizeRoot')" [description]="i18n.t('settings.organizeRootDesc')">
          <input class="sr-input" type="text" dir="ltr" [attr.aria-label]="i18n.t('settings.organizeRoot')" [value]="s().organizeRoot" [placeholder]="defaultRoot()" (change)="patch({ organizeRoot: value($event) })" />
        </app-setting-row>
      }
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('settings.categoryDirs')" [description]="i18n.t(s().organize ? 'settings.categoryDirsOrganizedDesc' : 'settings.categoryDirsDesc')">
      <div class="sr-block">
        <div class="sr-folders">
          @for (c of categories; track c) {
            <span class="sr-folders-name"><app-icon [name]="icons[c]" [size]="16" />{{ i18n.t('category.' + c) }}</span>
            <input class="sr-input" type="text" dir="ltr" [attr.aria-label]="i18n.t('category.' + c)" [value]="s().categoryDirs[c] ?? ''" [placeholder]="placeholderFor(c)" (change)="settings.setCategoryDir(c, value($event))" />
          }
          <span class="sr-folders-name"><app-icon name="magnet" [size]="16" />{{ i18n.t('settings.torrentsFolder') }}</span>
          <input class="sr-input" type="text" dir="ltr" [attr.aria-label]="i18n.t('settings.torrentsFolder')" [value]="s().torrentsDir" [placeholder]="torrentPlaceholder()" (change)="patch({ torrentsDir: value($event) })" />
        </div>
      </div>
      <app-setting-row [label]="i18n.t('settings.restoreFolders')" [description]="i18n.t('set.restoreFoldersDesc')">
        <button appButton size="sm" variant="secondary" icon="retry" type="button" (click)="settings.restoreDefaultFolders()">{{ i18n.t('settings.restoreFolders') }}</button>
      </app-setting-row>
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.speed')">
      <app-setting-row [field]="true" [label]="i18n.t('settings.connections')" [description]="i18n.t('settings.connectionsDesc')">
        <div class="sr-range">
          <input type="range" min="1" max="32" step="1" [value]="s().connections" [attr.aria-label]="i18n.t('settings.connections')" (input)="patch({ connections: number($event) })" />
          <output class="num">{{ s().connections }}</output>
        </div>
      </app-setting-row>
      <app-setting-row [field]="true" [label]="i18n.t('settings.perHost')" [description]="i18n.t('settings.perHostDesc')">
        <div class="sr-range">
          <input type="range" min="1" max="16" step="1" [value]="s().perHost" [attr.aria-label]="i18n.t('settings.perHost')" (input)="patch({ perHost: number($event) })" />
          <output class="num">{{ s().perHost }}</output>
        </div>
      </app-setting-row>
      <app-setting-row [stack]="true" [label]="i18n.t('settings.speedLimit')" [description]="i18n.t('settings.speedLimitDesc')">
        <app-speed-limit-control [value]="s().speedLimitBps" [label]="i18n.t('settings.speedLimit')" (valueChange)="patch({ speedLimitBps: $event })" />
      </app-setting-row>
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.queue')">
      <app-setting-row [field]="true" [label]="i18n.t('set.maxActive')" [description]="i18n.t('set.maxActiveDesc')">
        <div class="sr-range">
          <input type="range" min="1" max="32" step="1" [value]="s().maxActive" [attr.aria-label]="i18n.t('set.maxActive')" (input)="patch({ maxActive: number($event) })" />
          <output class="num">{{ s().maxActive }}</output>
        </div>
      </app-setting-row>
      <app-setting-row [clickable]="true" [label]="i18n.t('set.autoRetry')" [description]="i18n.t('set.autoRetryDesc')">
        <app-switch [checked]="s().autoRetry" [label]="i18n.t('set.autoRetry')" (checkedChange)="patch({ autoRetry: $event })" />
      </app-setting-row>
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.safety')">
      <app-setting-row [clickable]="true" [label]="i18n.t('settings.markDownloads')" [description]="i18n.t(isMac ? 'settings.markDownloadsDescMac' : 'settings.markDownloadsDesc')">
        <app-switch [checked]="s().markDownloads" [label]="i18n.t('settings.markDownloads')" (checkedChange)="patch({ markDownloads: $event })" />
      </app-setting-row>
    </app-setting-group>

    <app-setting-group anchor="torrents" [heading]="i18n.t('settings.torrents')">
      <app-setting-row [clickable]="true" [label]="i18n.t('settings.torrentSeeding')" [description]="i18n.t('settings.torrentSeedingDesc')">
        <app-switch [checked]="s().torrentSeeding" [label]="i18n.t('settings.torrentSeeding')" (checkedChange)="patch({ torrentSeeding: $event })" />
      </app-setting-row>
      @if (s().torrentSeeding) {
        <app-setting-row [field]="true" [label]="i18n.t('settings.torrentRatio')" [description]="i18n.t('settings.torrentRatioDesc')">
          <input class="sr-input" type="number" min="0" max="1000" step="0.1" dir="ltr" [value]="s().torrentRatioPercent / 100" [attr.aria-label]="i18n.t('settings.torrentRatio')" (change)="patch({ torrentRatioPercent: Math.round(number($event) * 100) })" />
        </app-setting-row>
        <app-setting-row [field]="true" [label]="i18n.t('settings.torrentMinutes')" [description]="i18n.t('settings.torrentMinutesDesc')">
          <input class="sr-input" type="number" min="0" max="525600" step="10" dir="ltr" [value]="s().torrentMinutes" [attr.aria-label]="i18n.t('settings.torrentMinutes')" (change)="patch({ torrentMinutes: Math.max(0, Math.round(number($event))) })" />
        </app-setting-row>
      }
      <app-setting-row [stack]="true" [label]="i18n.t('settings.torrentUpload')" [description]="i18n.t('settings.torrentUploadDesc')">
        <app-speed-limit-control [value]="s().torrentUploadBps" [label]="i18n.t('settings.torrentUpload')" (valueChange)="patch({ torrentUploadBps: $event })" />
      </app-setting-row>
      <app-setting-row [clickable]="true" [label]="i18n.t('settings.torrentDht')" [description]="i18n.t('settings.torrentDhtDesc')">
        <app-switch [checked]="s().torrentDht" [label]="i18n.t('settings.torrentDht')" (checkedChange)="patch({ torrentDht: $event })" />
      </app-setting-row>
      <app-setting-row [field]="true" [label]="i18n.t('settings.torrentPort')" [description]="i18n.t('settings.torrentPortDesc')">
        <input class="sr-input" type="number" min="0" max="65535" step="1" dir="ltr" [value]="s().torrentPort" [attr.aria-label]="i18n.t('settings.torrentPort')" (change)="patch({ torrentPort: Math.min(65535, Math.max(0, Math.round(number($event)))) })" />
      </app-setting-row>
    </app-setting-group>

    <app-setting-group anchor="video" [heading]="i18n.t('set.g.video')">
      <app-setting-row [label]="i18n.t('settings.video')" [description]="videoStatus()">
        <button appButton variant="secondary" icon="retry" type="button" [busy]="videoBusy()" [disabled]="!tauriAvailable || videoBusy()" (click)="updateVideo()">{{ i18n.t(videoBusy() ? 'settings.videoUpdating' : 'settings.videoUpdate') }}</button>
      </app-setting-row>
    </app-setting-group>
  `,
})
export class DownloadsSectionComponent {
  protected readonly i18n = inject(I18nService);
  /** macOS marks downloads with the quarantine attribute instead of Windows' stream. */
  protected readonly isMac = inject(IS_MAC);
  protected readonly settings = inject(SettingsService);
  private readonly toasts = inject(ToastService);
  protected readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);

  protected readonly s = this.settings.settings;
  protected readonly categories = CATEGORY_KEYS;
  protected readonly icons = CATEGORY_ICONS;
  protected readonly Math = Math;

  /** Where the category folders go when no root is chosen. */
  protected readonly defaultRoot = computed(() => organizedRoot({ organizeRoot: '', defaultDir: this.s().defaultDir }));
  protected readonly torrentPlaceholder = computed(() => {
    const s = this.s();
    return s.organize ? joinPath(organizedRoot(s), TORRENTS_FOLDER) : s.defaultDir;
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
    return this.i18n.t(info.ffmpeg ? 'settings.videoReady' : 'settings.videoNoFfmpeg').replace('{version}', info.version ?? '?');
  });

  constructor() {
    void this.loadVideoInfo();
  }

  /** What an empty category row means: its organised folder, or the default folder. */
  protected placeholderFor(c: CategoryKey): string {
    const s = this.s();
    return s.organize ? joinPath(organizedRoot(s), CATEGORY_FOLDERS[c]) : s.defaultDir;
  }

  protected patch(change: Partial<Settings>): void {
    this.settings.patch(change);
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected number(event: Event): number {
    return Number((event.target as HTMLInputElement).value);
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
}
