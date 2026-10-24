import { ChangeDetectionStrategy, Component, computed, inject, OnInit } from '@angular/core';

import { effectiveChannel } from '../../core/update-channel';
import { CHECK_INTERVAL_OPTIONS_HOURS, UpdateService } from '../../core/update.service';
import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { ToastService } from '../../ui/toast.service';
import { SettingRowComponent } from './setting-layout.component';
import { SettingsService } from './settings.service';

/**
 * Settings → Updates: everything about self-update in one card. It shows the running version,
 * the channel, when the last check happened and how it ended, the state of the update flow
 * (available, downloading with progress, downloaded and verified, installing, failed with the
 * reason and a retry), the release notes, and the actions: check now, update and restart,
 * and, while downloads are running, the choice between "install when downloads finish" and
 * "install now" (downloads stop at a checkpoint and resume after the restart). It also holds the
 * interval of the automatic checks. All state lives in {@link UpdateService}, so the card can be
 * dropped anywhere (`<app-update-card />`).
 */
@Component({
  selector: 'app-update-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, SettingRowComponent],
  template: `
    <div class="update-card">
      <app-setting-row
        [label]="i18n.t('update.version') + ' ' + versionText()"
        [description]="i18n.t('update.channel') + ': ' + channelLabel() + ' · ' + lastCheckText()"
      >
        <button appButton variant="secondary" size="sm" icon="retry" type="button" [disabled]="busy()" (click)="check()">
          {{ i18n.t(updates.status() === 'checking' ? 'settings.checkingUpdates' : 'update.checkNow') }}
        </button>
      </app-setting-row>

      <div class="state" role="status" aria-live="polite" [class.is-error]="updates.status() === 'error'" [class.is-update]="updates.available() !== null">
        <app-setting-row [stack]="true" [label]="stateText()" [description]="errorText()">
          @if (updates.available(); as update) {
            @if (update.notes) {
              <span class="sr-note full update-notes" dir="auto">{{ update.notes }}</span>
            }
          }

          @if (updates.status() === 'downloading') {
            <div class="bar full" role="progressbar" aria-valuemin="0" aria-valuemax="100" [attr.aria-valuenow]="percent() ?? null" [attr.aria-label]="i18n.t('update.downloading')">
              <div class="bar-fill" [class.is-unknown]="percent() === null" [style.inline-size.%]="percent() ?? 40"></div>
            </div>
          }

          @if (updates.blockedByDownloads()) {
            <span class="sr-note full">{{ i18n.t('update.blocked') }}</span>
          }
          @if (updates.installScheduled()) {
            <span class="sr-note full">{{ i18n.t('update.scheduled') }}</span>
          }

          @if (updates.available() !== null) {
            @if (updates.installScheduled()) {
              <button appButton variant="secondary" type="button" (click)="updates.cancelScheduledInstall()">{{ i18n.t('update.cancelScheduled') }}</button>
            } @else if (updates.blockedByDownloads()) {
              <button appButton variant="primary" type="button" (click)="updates.installWhenIdle()">{{ i18n.t('update.installWhenIdle') }}</button>
              <button appButton variant="secondary" type="button" (click)="install(true)">{{ i18n.t('update.installNow') }}</button>
            } @else if (updates.status() === 'error') {
              <button appButton variant="primary" icon="retry" type="button" (click)="retry()">{{ i18n.t('update.retry') }}</button>
            } @else {
              <button appButton variant="primary" type="button" [disabled]="updates.status() === 'installing' || updates.status() === 'downloading'" (click)="install(false)">
                {{ i18n.t(updates.status() === 'installing' ? 'settings.updateInstalling' : 'settings.updateRestart') }}
              </button>
            }
          } @else if (updates.status() === 'error') {
            <button appButton variant="primary" icon="retry" type="button" (click)="retry()">{{ i18n.t('update.retry') }}</button>
          }
        </app-setting-row>
      </div>

      <app-setting-row [clickable]="true" [label]="i18n.t('update.interval')" [description]="i18n.t('update.intervalDesc')">
        <select class="sr-input interval" [disabled]="!settings.settings().checkUpdates" [value]="updates.intervalHours()" (change)="setInterval($event)">
          @for (h of intervals; track h) {
            <option [value]="h" [selected]="h === updates.intervalHours()">{{ everyLabel(h) }}</option>
          }
        </select>
      </app-setting-row>
    </div>
  `,
  styles: [
    `
      .update-card > * + * {
        border-block-start: 1px solid var(--color-border);
      }

      .state.is-update ::ng-deep .sr-name {
        color: var(--color-accent);
      }

      .state.is-error ::ng-deep .sr-name,
      .state.is-error ::ng-deep .sr-desc {
        color: var(--color-danger);
      }

      .full {
        flex: 1 1 100%;
      }

      .update-notes {
        white-space: pre-line;
        max-block-size: 12rem;
        overflow: auto;
      }

      .bar {
        block-size: 6px;
        border-radius: 3px;
        background: var(--color-border);
        overflow: hidden;
      }

      .bar-fill {
        block-size: 100%;
        background: var(--color-accent);
        transition: inline-size 120ms linear;
      }

      .bar-fill.is-unknown {
        animation: update-slide 1.2s ease-in-out infinite alternate;
      }

      @keyframes update-slide {
        from {
          transform: translateX(-60%);
        }
        to {
          transform: translateX(160%);
        }
      }

      @media (prefers-reduced-motion: reduce) {
        .bar-fill.is-unknown {
          animation: none;
        }
      }

      .interval {
        inline-size: auto;
        min-inline-size: 9rem;
      }
    `,
  ],
})
export class UpdateCardComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly updates = inject(UpdateService);
  protected readonly settings = inject(SettingsService);
  private readonly format = inject(FormatService);
  private readonly toasts = inject(ToastService);

  /** The running version as shown in the row label. */
  protected readonly versionText = computed(() => this.updates.currentVersion() ?? '—');

  protected readonly intervals = CHECK_INTERVAL_OPTIONS_HOURS;

  protected readonly channel = computed(() => effectiveChannel(this.settings.settings().channel, this.updates.currentVersion()));

  protected readonly channelLabel = computed(() => this.i18n.t(`settings.channel.${this.channel()}`));

  protected readonly busy = computed(() => ['checking', 'downloading', 'installing'].includes(this.updates.status()));

  /** Download progress as a percentage, `null` while the size is unknown. */
  protected readonly percent = computed(() => {
    const p = this.updates.progress();
    return p?.total ? Math.min(100, Math.round((p.downloaded / p.total) * 100)) : null;
  });

  /** "Last checked 3 minutes ago" / "Never checked". */
  protected readonly lastCheckText = computed(() => {
    const at = this.updates.lastSuccessAt();
    return at === null ? this.i18n.t('update.neverChecked') : this.i18n.t('update.lastChecked').replace('{when}', this.format.relative(at));
  });

  /** One line about where the flow stands. */
  protected readonly stateText = computed(() => {
    const version = this.updates.available()?.version ?? '';
    const fill = (key: string): string => this.i18n.t(key).replace('{version}', version).replace('{percent}', String(this.percent() ?? 0));
    if (this.updates.installScheduled() && this.updates.status() !== 'error') {
      return fill('update.state.scheduled');
    }
    switch (this.updates.status()) {
      case 'checking':
        return fill('update.state.checking');
      case 'none':
        return fill('update.state.none');
      case 'available':
        return fill('update.state.available');
      case 'downloading':
        return fill(this.percent() === null ? 'update.state.downloading' : 'update.state.downloadingPercent');
      case 'ready':
        return fill('update.state.ready');
      case 'installing':
        return fill('update.state.installing');
      case 'error':
        return this.i18n.t('update.state.error');
      default:
        return fill('update.state.idle');
    }
  });

  /** Why the last step failed, in plain words (also after a restart), or `null`. */
  protected readonly errorText = computed(() => {
    const error = this.updates.lastError();
    if (error === null) {
      return null;
    }
    return this.i18n.t(`update.error.${error.kind}`).concat(' ', this.i18n.t('update.errorWhen').replace('{when}', this.format.relative(error.at)));
  });

  protected everyLabel(hours: number): string {
    return this.i18n.t(`update.every.${hours}`);
  }

  ngOnInit(): void {
    void this.updates.refreshVersion();
  }

  protected async check(): Promise<void> {
    try {
      const found = await this.updates.check(this.settings.settings().channel);
      if (found === null) {
        this.toasts.show('success', this.i18n.t('toast.upToDate'));
      }
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.updateCheckFailed'));
    }
  }

  protected async install(interrupt: boolean): Promise<void> {
    try {
      await this.updates.install(interrupt);
    } catch {
      this.toasts.show('danger', this.i18n.t(`toast.updateFailed.${this.updates.lastError()?.kind ?? 'unknown'}`));
    }
  }

  protected async retry(): Promise<void> {
    await this.updates.retry();
  }

  protected setInterval(event: Event): void {
    this.updates.setIntervalHours(Number((event.target as HTMLSelectElement).value));
  }
}
