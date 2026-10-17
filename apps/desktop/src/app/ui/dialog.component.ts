import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  input,
  output,
  viewChild,
} from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { ButtonComponent } from './button.component';

/**
 * Modal dialog on the native `<dialog>` element: the browser gives us the top layer, focus
 * containment, Escape handling and an inert background for free. The panel scales in from
 * 96% with a fade on open; a click on the backdrop closes it.
 */
@Component({
  selector: 'app-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent],
  template: `
    <dialog #dialog class="dialog" [class.is-wide]="wide()" [attr.aria-labelledby]="titleId" (cancel)="onCancel($event)">
      <div class="panel">
        <header class="head">
          <h2 class="title" [id]="titleId">{{ title() }}</h2>
          <button
            appButton
            variant="ghost"
            icon="close"
            [iconOnly]="true"
            type="button"
            [attr.aria-label]="i18n.t('action.close')"
            (click)="requestClose()"
          ></button>
        </header>
        <div class="body"><ng-content /></div>
        <footer class="foot"><ng-content select="[dialogActions]" /></footer>
      </div>
    </dialog>
  `,
  styles: `
    .dialog {
      inline-size: min(560px, calc(100vw - 2 * var(--space-6)));
      max-block-size: calc(100vh - 2 * var(--space-8));
      padding: 0;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-lg);
      background: var(--color-surface-1);
      color: var(--color-text);
      box-shadow: var(--shadow-3);
      overflow: visible;
    }

    .dialog.is-wide {
      inline-size: min(720px, calc(100vw - 2 * var(--space-6)));
    }

    .dialog::backdrop {
      background: rgb(4 8 10 / 45%);
    }

    .dialog[open] {
      animation: dialog-in var(--dur-3) var(--ease-emphasized-out) both;
    }

    .dialog[open]::backdrop {
      animation: backdrop-in var(--dur-2) var(--ease-standard) both;
    }

    @keyframes dialog-in {
      from {
        opacity: 0;
        transform: translateY(4px) scale(0.96);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .dialog[open] {
        animation-duration: var(--dur-fade);
        animation-name: backdrop-in;
      }
    }

    @keyframes backdrop-in {
      from {
        opacity: 0;
      }

      to {
        opacity: 1;
      }
    }

    .panel {
      display: flex;
      flex-direction: column;
      max-block-size: calc(100vh - 2 * var(--space-8));
    }

    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-4);
      padding: var(--space-5) var(--space-6) var(--space-3);
    }

    .title {
      font-size: var(--text-xl);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
    }

    .body {
      padding: var(--space-2) var(--space-6);
      overflow: auto;
    }

    .foot {
      display: flex;
      justify-content: flex-end;
      gap: var(--space-2);
      padding: var(--space-4) var(--space-6) var(--space-5);
    }

    .foot:empty {
      display: none;
    }
  `,
})
export class DialogComponent {
  private static nextId = 0;

  protected readonly i18n = inject(I18nService);
  private readonly destroyRef = inject(DestroyRef);
  private readonly dialogRef = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');

  /** Open state, controlled by the parent. */
  readonly open = input.required<boolean>();

  /** Heading text. */
  readonly title = input.required<string>();

  /** 720px wide variant. */
  readonly wide = input(false);

  /** The user asked to close (Escape, backdrop click, close button). The parent sets `open`. */
  readonly closed = output<void>();

  protected readonly titleId = `dialog-title-${DialogComponent.nextId++}`;

  constructor() {
    afterNextRender(() => {
      const el = this.dialogRef().nativeElement;
      // A click that lands on the <dialog> itself (not the panel) is a click on the backdrop.
      const onClick = (event: MouseEvent): void => {
        if (event.target === el) {
          this.closed.emit();
        }
      };
      el.addEventListener('click', onClick);
      this.destroyRef.onDestroy(() => el.removeEventListener('click', onClick));
      this.sync();
    });
    effect(() => {
      this.open();
      this.sync();
    });
  }

  private sync(): void {
    const el = this.dialogRef().nativeElement;
    if (!el.isConnected) {
      return;
    }
    if (this.open() && !el.open) {
      el.showModal();
      // focus the first field rather than the close button
      el.querySelector<HTMLElement>('input, textarea, select')?.focus();
    } else if (!this.open() && el.open) {
      el.close();
    }
  }

  protected requestClose(): void {
    this.closed.emit();
  }

  protected onCancel(event: Event): void {
    // Escape: keep the element open until the parent flips `open`, so state stays in one place
    event.preventDefault();
    this.closed.emit();
  }
}
