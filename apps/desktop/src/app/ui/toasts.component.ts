import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { ButtonComponent } from './button.component';
import { IconComponent } from './icon.component';
import { toastIcon, ToastService } from './toast.service';

/** Renders the toast queue at the bottom-end corner, newest at the bottom. */
@Component({
  selector: 'app-toasts',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent],
  template: `
    <div class="stack" aria-live="polite" aria-relevant="additions">
      @for (t of toasts.toasts(); track t.id) {
        <div class="toast" [attr.data-tone]="t.tone" role="status">
          <app-icon class="tone" [name]="icon(t.tone)" [size]="16" />
          <span class="text">{{ t.text }}</span>
          @if (t.action; as action) {
            <button appButton size="sm" variant="secondary" type="button" (click)="run(t.id, action.run)">
              {{ action.label }}
            </button>
          }
          <button
            appButton
            size="sm"
            variant="ghost"
            icon="close"
            [iconOnly]="true"
            type="button"
            [attr.aria-label]="i18n.t('action.dismiss')"
            (click)="toasts.dismiss(t.id)"
          ></button>
        </div>
      }
    </div>
  `,
  styles: `
    .stack {
      position: fixed;
      inset-block-end: calc(var(--statusbar-height) + var(--space-4));
      inset-inline-end: var(--space-4);
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      z-index: var(--z-toast);
      pointer-events: none;
    }

    .toast {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-inline-size: 280px;
      max-inline-size: 420px;
      padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
      padding-inline: var(--space-3) var(--space-2);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
      color: var(--color-text);
      box-shadow: var(--shadow-3);
      font-size: var(--text-sm);
      pointer-events: auto;
      animation: toast-in var(--motion-slow) var(--ease-emphasized) both;
    }

    @keyframes toast-in {
      from {
        opacity: 0;
        transform: translateY(8px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .tone {
      color: var(--color-text-secondary);
    }

    .toast[data-tone='success'] .tone {
      color: var(--color-success);
    }

    .toast[data-tone='danger'] .tone {
      color: var(--color-danger);
    }

    .text {
      flex: 1;
      min-inline-size: 0;
    }
  `,
})
export class ToastsComponent {
  protected readonly toasts = inject(ToastService);
  protected readonly i18n = inject(I18nService);
  protected readonly icon = toastIcon;

  protected run(id: number, action: () => void): void {
    action();
    this.toasts.dismiss(id);
  }
}
