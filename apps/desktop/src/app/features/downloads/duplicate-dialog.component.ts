import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { Download } from '../../model/download';
import { FileNameComponent } from '../../ui/file-name.component';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { StatusPillComponent } from '../../ui/status-pill.component';

/** What the user decided about a duplicate. */
export type DuplicateChoice = 'show' | 'add' | 'cancel';

/**
 * "Already in the list": shown by the add dialog when the engine knows a download with the
 * same link or the same name in the same folder. Three ways out: show the existing row, add
 * anyway (the engine gives the new file its own name), or go back.
 */
@Component({
  selector: 'app-duplicate-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FileNameComponent, ButtonComponent, DialogComponent, IconComponent, StatusPillComponent],
  template: `
    <app-dialog [open]="existing() !== null" [title]="i18n.t('duplicate.title')" (closed)="choose('cancel')">
      @if (existing(); as d) {
        <p class="text">{{ i18n.t('duplicate.body').replace('{name}', d.filename) }}</p>
        <div class="existing">
          <app-icon name="file" [size]="16" />
          <app-file-name class="name" [name]="d.filename" />
          <app-status-pill [state]="d.state" [failure]="d.failure" [compact]="true" />
        </div>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="choose('cancel')">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="secondary" icon="info" type="button" (click)="choose('show')">{{ i18n.t('duplicate.show') }}</button>
        <button appButton variant="primary" icon="plus" type="button" (click)="choose('add')">{{ i18n.t('duplicate.addAnyway') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .text {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
      overflow-wrap: anywhere;
    }

    .existing {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin-block: var(--space-2) var(--space-3);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      font-size: var(--text-sm);
    }

    .name {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  `,
})
export class DuplicateDialogComponent {
  protected readonly i18n = inject(I18nService);

  /** The record that already exists; `null` keeps the dialog closed. */
  readonly existing = input.required<Download | null>();

  /** The user's choice; the parent closes the dialog by clearing `existing`. */
  readonly chosen = output<DuplicateChoice>();

  protected choose(choice: DuplicateChoice): void {
    this.chosen.emit(choice);
  }
}
