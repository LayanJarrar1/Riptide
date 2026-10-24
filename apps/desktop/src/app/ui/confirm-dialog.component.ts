import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { ButtonComponent } from './button.component';
import { DialogComponent } from './dialog.component';

/**
 * Asks "are you sure" before something that cannot be undone. Cancel is the default action
 * (Escape, the backdrop and the close button all cancel); the confirm button is the danger one.
 */
@Component({
  selector: 'app-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent],
  template: `
    <app-dialog [open]="open()" [title]="title()" (closed)="cancelled.emit()">
      <p class="message">{{ message() }}</p>
      <ng-container dialogActions>
        <button appButton variant="secondary" type="button" (click)="cancelled.emit()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="danger" type="button" (click)="confirmed.emit()">{{ confirmLabel() }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .message {
      margin: 0;
      color: var(--color-text-secondary);
      line-height: var(--leading-normal);
    }
  `,
})
export class ConfirmDialogComponent {
  protected readonly i18n = inject(I18nService);

  readonly open = input.required<boolean>();
  readonly title = input.required<string>();
  readonly message = input.required<string>();
  readonly confirmLabel = input.required<string>();
  readonly confirmed = output<void>();
  readonly cancelled = output<void>();
}
