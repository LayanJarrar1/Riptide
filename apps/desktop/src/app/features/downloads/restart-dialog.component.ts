import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { Download } from '../../model/download';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';

/**
 * "Restart from the beginning?": confirms before the engine throws away a download's progress
 * (part file, segments, validators) and fetches it again from byte zero.
 */
@Component({
  selector: 'app-restart-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent],
  template: `
    <app-dialog [open]="target() !== null" [title]="i18n.t('restart.title')" (closed)="closed.emit()">
      @if (target(); as d) {
        <p class="text">{{ i18n.t('restart.body').replace('{name}', d.filename) }}</p>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="closed.emit()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="danger" icon="retry" type="button" (click)="confirmed.emit()">{{ i18n.t('restart.confirm') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .text {
      padding-block: var(--space-2) var(--space-3);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
      overflow-wrap: anywhere;
    }
  `,
})
export class RestartDialogComponent {
  protected readonly i18n = inject(I18nService);

  /** The download to restart; `null` keeps the dialog closed. */
  readonly target = input.required<Download | null>();

  /** The user confirmed; the parent calls the engine and clears `target`. */
  readonly confirmed = output<void>();

  /** The user backed out; the parent clears `target`. */
  readonly closed = output<void>();
}
