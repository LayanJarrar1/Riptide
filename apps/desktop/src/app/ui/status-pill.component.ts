import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { DownloadState, FailureKind } from '../model/download';
import { IconComponent, IconName } from './icon.component';

const ICONS: Readonly<Record<DownloadState, IconName>> = {
  queued: 'clock',
  probing: 'globe',
  downloading: 'download',
  paused: 'pause',
  verifying: 'shield',
  completed: 'check',
  failed: 'alert',
  cancelled: 'close',
};

/**
 * State badge: hue + icon + translated label, so the state is never carried by colour alone.
 * For failures the label names the cause ("Link expired", "Checksum mismatch").
 */
@Component({
  selector: 'app-status-pill',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  host: {
    '[attr.data-state]': 'state()',
    '[class.is-compact]': 'compact()',
  },
  template: `
    <app-icon [name]="icon()" [size]="compact() ? 13 : 14" />
    <span class="label">{{ label() }}</span>
  `,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      block-size: 24px;
      padding-inline: 8px 10px;
      border-radius: var(--radius-full);
      background: var(--pill-soft, var(--color-surface-3));
      color: var(--pill-fg, var(--color-text-secondary));
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      line-height: 1;
      white-space: nowrap;
      transition:
        background-color var(--motion-base) var(--ease-standard),
        color var(--motion-base) var(--ease-standard);
    }

    :host(.is-compact) {
      block-size: 20px;
      padding-inline: 6px 8px;
    }

    :host([data-state='downloading']),
    :host([data-state='probing']) {
      --pill-soft: var(--state-downloading-soft);
      --pill-fg: var(--color-accent-fg);
    }

    :host([data-state='queued']) {
      --pill-soft: var(--state-queued-soft);
      --pill-fg: var(--state-queued);
    }

    :host([data-state='paused']) {
      --pill-soft: var(--state-paused-soft);
      --pill-fg: var(--state-paused);
    }

    :host([data-state='verifying']) {
      --pill-soft: var(--state-verifying-soft);
      --pill-fg: var(--state-verifying);
    }

    :host([data-state='completed']) {
      --pill-soft: var(--state-completed-soft);
      --pill-fg: var(--state-completed);
    }

    :host([data-state='failed']) {
      --pill-soft: var(--state-failed-soft);
      --pill-fg: var(--state-failed);
    }

    :host([data-state='cancelled']) {
      --pill-soft: var(--state-cancelled-soft);
      --pill-fg: var(--state-cancelled);
    }

    :host([data-state='verifying']) app-icon {
      animation: breathe 1200ms var(--ease-standard) infinite alternate;
    }

    @keyframes breathe {
      from {
        opacity: 0.55;
      }

      to {
        opacity: 1;
      }
    }

    @media (prefers-reduced-motion: reduce) {
      :host([data-state='verifying']) app-icon {
        animation: none;
      }
    }
  `,
})
export class StatusPillComponent {
  private readonly i18n = inject(I18nService);

  readonly state = input.required<DownloadState>();

  /** Names the cause for failed downloads. */
  readonly failure = input<FailureKind | null>(null);

  /** 20px tall for dense rows. */
  readonly compact = input(false);

  protected readonly icon = computed(() => ICONS[this.state()]);

  protected readonly label = computed(() => {
    const state = this.state();
    const failure = this.failure();
    if (state === 'failed' && failure !== null) {
      return this.i18n.t(`failure.${failure}`);
    }
    return this.i18n.t(`state.${state}`);
  });
}
