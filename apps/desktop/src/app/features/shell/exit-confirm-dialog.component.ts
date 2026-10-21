import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, signal, untracked } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { UiState } from './ui-state.service';

/** Tauri event sent by the close handler when downloads are active and `confirmOnExit` is on. */
export const CONFIRM_EXIT_EVENT = 'riptide://confirm-exit';

/**
 * "Pause N downloads and quit?" — shown when the window is closed while downloads run and the
 * user asked to be warned. Quit stops the transfers at a checkpoint and exits (they resume on
 * the next start); Cancel keeps the window open. Only the Tauri shell ever opens it.
 */
@Component({
  selector: 'app-exit-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent],
  template: `
    <app-dialog [open]="count() !== null" [title]="i18n.t('exit.title')" (closed)="cancel()">
      <p class="text">{{ i18n.t('exit.body').replace('{count}', format(count() ?? 0)) }}</p>
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="cancel()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" type="button" [disabled]="quitting()" (click)="quit()">{{ i18n.t('exit.quit') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .text {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
    }
  `,
})
export class ExitConfirmDialogComponent {
  protected readonly i18n = inject(I18nService);
  private readonly invoke = inject(TAURI_INVOKE);

  /** Active downloads when the close was requested; `null` keeps the dialog closed. */
  readonly count = signal<number | null>(null);
  protected readonly quitting = signal(false);
  private readonly ui = inject(UiState);

  constructor() {
    // File > Exit with downloads running asks the same question the close button does
    effect(() => {
      const n = this.ui.exitRequest();
      if (n !== null) {
        this.count.set(n);
        untracked(() => this.ui.exitRequest.set(null));
      }
    });
    if (!inject(TAURI_AVAILABLE)) {
      return;
    }
    const listen = inject(TAURI_LISTEN);
    let unlisten: (() => void) | null = null;
    let destroyed = false;
    inject(DestroyRef).onDestroy(() => {
      destroyed = true;
      unlisten?.();
    });
    listen<number>(CONFIRM_EXIT_EVENT, (n) => this.count.set(n))
      .then((off) => {
        if (destroyed) {
          off();
        } else {
          unlisten = off;
        }
      })
      .catch((error: unknown) => console.warn('[exit] listen failed', error));
  }

  protected format(n: number): string {
    return new Intl.NumberFormat(this.i18n.lang()).format(n);
  }

  protected cancel(): void {
    if (!this.quitting()) {
      this.count.set(null);
    }
  }

  protected async quit(): Promise<void> {
    this.quitting.set(true);
    try {
      await this.invoke<void>('quit_app');
    } catch (error: unknown) {
      console.warn('[exit] quit_app failed', error);
      this.quitting.set(false);
      this.count.set(null);
    }
  }
}
