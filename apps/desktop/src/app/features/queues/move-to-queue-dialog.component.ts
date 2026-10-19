import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal, untracked } from '@angular/core';

import { FormatService } from '../../core/format.service';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import type { Download } from '../../model/download';
import { MAIN_QUEUE_ID, UiQueue } from '../../model/queue';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';

/**
 * "Move to queue": lists the queues as a radio group (the queue the download is in is marked
 * and cannot be chosen) and reports the choice. The page performs the move. Downloads go to
 * the end of the chosen queue.
 */
@Component({
  selector: 'app-move-to-queue-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent],
  template: `
    <app-dialog [open]="targets() !== null" [title]="i18n.t('queue.moveTitle')" (closed)="closed.emit()">
      @if (targets(); as list) {
        <p class="text">
          {{
            list.length === 1
              ? i18n.t('queue.moveBodyOne').replace('{name}', list[0].filename)
              : i18n.t('queue.moveBodyMany').replace('{n}', format.int(list.length))
          }}
        </p>
        <div class="choices" role="radiogroup" [attr.aria-label]="i18n.t('queue.moveTitle')">
          @for (q of queues(); track q.id) {
            <label class="choice" [class.is-checked]="choice() === q.id" [class.is-disabled]="isCurrent(q)">
              <input type="radio" name="queue" [value]="q.id" [checked]="choice() === q.id" [disabled]="isCurrent(q)" (change)="choice.set(q.id)" />
              <span class="name">{{ nameOf(q) }}</span>
              @if (isCurrent(q)) {
                <span class="tag">{{ i18n.t('queue.current') }}</span>
              }
            </label>
          }
        </div>
        <p class="note">{{ i18n.t('queue.moveNote') }}</p>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="closed.emit()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" type="button" [disabled]="choice() === null" (click)="confirm()">{{ i18n.t('queue.move') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .text {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      overflow-wrap: anywhere;
    }

    .choices {
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
      max-block-size: 40vh;
      overflow-y: auto;
    }

    .choice {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      font-size: var(--text-sm);
      cursor: pointer;
    }

    .choice:hover:not(.is-disabled) {
      background: var(--color-surface-hover);
    }

    .choice.is-checked {
      border-color: var(--color-accent);
      background: var(--color-accent-soft);
    }

    .choice.is-disabled {
      color: var(--color-text-muted);
      cursor: default;
    }

    .choice input {
      accent-color: var(--color-accent);
    }

    .name {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .tag {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .note {
      margin-block-start: var(--space-3);
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }
  `,
})
export class MoveToQueueDialogComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly queues = inject(QueuesService).queues;

  /** The downloads to move; `null` keeps the dialog closed. */
  readonly targets = input<readonly Download[] | null>(null);

  /** A queue was chosen. */
  readonly chosen = output<number>();

  /** The dialog should close. */
  readonly closed = output<void>();

  protected readonly choice = signal<number | null>(null);

  /** Queues every target is already in (they cannot be chosen). */
  private readonly currentIds = computed(() => {
    const list = this.targets() ?? [];
    return new Set(list.length > 0 && list.every((d) => d.queueId === list[0].queueId) ? [list[0].queueId] : []);
  });

  constructor() {
    // every opening starts with nothing chosen
    effect(() => {
      if (this.targets() !== null) {
        untracked(() => this.choice.set(null));
      }
    });
  }

  protected isCurrent(q: UiQueue): boolean {
    return this.currentIds().has(q.id);
  }

  protected nameOf(q: UiQueue): string {
    return q.id === MAIN_QUEUE_ID ? this.i18n.t('queue.main') : q.name;
  }

  protected confirm(): void {
    const id = this.choice();
    if (id !== null) {
      this.chosen.emit(id);
    }
  }
}
