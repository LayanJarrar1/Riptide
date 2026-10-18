import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';

import { DownloadEngine } from '../../data/download-engine';
import { I18nService } from '../../i18n/i18n.service';
import { Download } from '../../model/download';
import { SpeedLimitControlComponent } from '../../ui/speed-limit-control.component';
import { ToastService } from '../../ui/toast.service';

/**
 * The details drawer's "Speed limit" block: one download's own cap, applied by the engine to a
 * running transfer within a second (no restart).
 */
@Component({
  selector: 'app-download-speed-limit',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SpeedLimitControlComponent],
  template: `
    <h3 class="title">{{ i18n.t('details.speedLimit') }}</h3>
    <app-speed-limit-control [value]="download().speedLimitBps ?? null" [compact]="true" (valueChange)="set($event)" />
    <p class="hint">{{ i18n.t('details.speedLimitDesc') }}</p>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }

    .title {
      font-size: var(--text-sm);
      font-weight: var(--weight-semibold);
    }

    .hint {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }
  `,
})
export class DownloadSpeedLimitComponent {
  protected readonly i18n = inject(I18nService);
  private readonly engine = inject(DownloadEngine);
  private readonly toasts = inject(ToastService);

  /** The download whose cap is edited. */
  readonly download = input.required<Download>();

  protected async set(bps: number | null): Promise<void> {
    try {
      await this.engine.setSpeedLimit(this.download().id, bps);
    } catch (error: unknown) {
      console.warn('[details] set_speed_limit failed', error);
      this.toasts.show('danger', String(error));
    }
  }
}
