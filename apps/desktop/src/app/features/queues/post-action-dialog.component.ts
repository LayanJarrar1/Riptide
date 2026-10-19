import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';

import { FormatService } from '../../core/format.service';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { MAIN_QUEUE_ID } from '../../model/queue';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { COUNTDOWN_SECONDS, PostActionCountdown } from './post-action-countdown.service';

/**
 * "Shutting down in 27 s": the 30-second window between a finished queue and its post-queue
 * action. Cancel (also Escape and the close button) stops it; "Do it now" skips the wait. The
 * focus starts on the close button, so a stray Enter cancels rather than shuts down. Screen
 * readers are told at the start and then at every tenth second and in the last five, not on
 * every tick.
 */
@Component({
  selector: 'app-post-action-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent],
  template: `
    <app-dialog [open]="countdown.pending() !== null" [title]="i18n.t('postaction.countdownTitle')" (closed)="countdown.cancel()">
      @if (countdown.pending(); as p) {
        <p class="done">{{ i18n.t('postaction.queueDone').replace('{name}', queueName()) }}</p>
        <p class="count" aria-hidden="true">{{ text() }}</p>
        <div class="bar" aria-hidden="true"><span class="fill" [style.inline-size.%]="percent()"></span></div>
        <p class="sr-only" role="status" aria-live="polite">{{ announcement() }}</p>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="countdown.cancel()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" type="button" (click)="countdown.doNow()">{{ i18n.t('postaction.now') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .done {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      overflow-wrap: anywhere;
    }

    .count {
      margin-block: var(--space-3);
      font-size: var(--text-xl);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
    }

    .bar {
      block-size: 4px;
      margin-block-end: var(--space-3);
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
      overflow: hidden;
    }

    .fill {
      display: block;
      block-size: 100%;
      background: var(--color-accent);
      transition: inline-size 1s linear;
    }

    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      margin: -1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    @media (prefers-reduced-motion: reduce) {
      .fill {
        transition: none;
      }
    }
  `,
})
export class PostActionDialogComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly countdown = inject(PostActionCountdown);
  private readonly format = inject(FormatService);
  private readonly queues = inject(QueuesService);

  protected readonly queueName = computed(() => {
    const id = this.countdown.pending()?.queueId;
    const queue = id === undefined ? undefined : this.queues.get(id);
    return id === MAIN_QUEUE_ID ? this.i18n.t('queue.main') : (queue?.name ?? '');
  });

  /** "Shutting down in 27 s". */
  protected readonly text = computed(() => {
    const p = this.countdown.pending();
    return p === null ? '' : this.i18n.t(`postaction.countdown.${p.action.action}`).replace('{n}', this.format.int(p.remaining));
  });

  protected readonly percent = computed(() => {
    const p = this.countdown.pending();
    return p === null ? 0 : Math.max(0, ((p.remaining - 1) / COUNTDOWN_SECONDS) * 100);
  });

  /** What the live region says; changes only at the start, every ten seconds and in the last five. */
  protected readonly announcement = signal('');

  constructor() {
    effect(() => {
      const p = this.countdown.pending();
      if (p === null) {
        untracked(() => this.announcement.set(''));
      } else if (p.remaining === COUNTDOWN_SECONDS || p.remaining % 10 === 0 || p.remaining <= 5) {
        const text = this.text();
        untracked(() => this.announcement.set(text));
      }
    });
  }
}
