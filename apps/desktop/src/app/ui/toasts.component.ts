import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { ButtonComponent } from './button.component';
import { IconComponent } from './icon.component';
import { toastIcon, ToastService } from './toast.service';

/**
 * Renders the toast queue at the bottom-end corner, newest at the bottom. Each toast slides
 * up and fades in, carries a thin line that drains over its lifetime, holds still while the
 * pointer is over it, and fades out when dismissed.
 */
@Component({
  selector: 'app-toasts',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent],
  template: `
    <div class="stack" aria-live="polite" aria-relevant="additions">
      @for (t of toasts.toasts(); track t.id) {
        <div
          class="toast"
          [attr.data-tone]="t.tone"
          [class.is-leaving]="t.leaving"
          role="status"
          (mouseenter)="toasts.pause(t.id)"
          (mouseleave)="toasts.resume(t.id)"
          (focusin)="toasts.pause(t.id)"
          (focusout)="toasts.resume(t.id)"
        >
          <app-icon class="tone" [name]="icon(t.tone)" [size]="16" />
          <span class="text" dir="auto">{{ t.text }}</span>
          @for (action of t.actions; track action.label) {
            <button appButton size="sm" variant="secondary" [icon]="action.icon ?? null" type="button" (click)="run(t.id, action.run)">
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
            [attr.title]="i18n.t('action.dismiss')"
            (click)="toasts.dismiss(t.id)"
          ></button>
          <span class="progress" aria-hidden="true" [style.animation-duration.ms]="t.ms"></span>
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
      position: relative;
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-inline-size: 280px;
      max-inline-size: 540px;
      padding-block: var(--space-2);
      padding-inline: var(--space-3) var(--space-2);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
      color: var(--color-text);
      box-shadow: var(--shadow-3);
      font-size: var(--text-sm);
      pointer-events: auto;
      overflow: hidden;
      animation: toast-in var(--dur-3) var(--ease-emphasized-out) both;
    }

    .toast.is-leaving {
      animation: toast-out var(--dur-1) var(--ease-emphasized-in) both;
    }

    @keyframes toast-in {
      from {
        opacity: 0;
        transform: translateY(10px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    @keyframes toast-out {
      from {
        opacity: 1;
        transform: none;
      }

      to {
        opacity: 0;
        transform: translateY(6px);
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
      overflow-wrap: anywhere;
    }

    /* The clock: a hairline that drains from inline-start to inline-end and holds on hover. */
    .progress {
      position: absolute;
      inset-block-end: 0;
      inset-inline: 0;
      block-size: 2px;
      background: var(--color-accent);
      opacity: 0.7;
      transform-origin: var(--origin, left center);
      animation: toast-clock linear forwards;
    }

    :host-context([dir='rtl']) .progress {
      --origin: right center;
    }

    .toast[data-tone='danger'] .progress {
      background: var(--color-danger);
    }

    .toast[data-tone='success'] .progress {
      background: var(--color-success);
    }

    .toast:hover .progress,
    .toast:focus-within .progress {
      animation-play-state: paused;
    }

    @keyframes toast-clock {
      from {
        transform: scaleX(1);
      }

      to {
        transform: scaleX(0);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .toast {
        animation: toast-fade var(--dur-fade) linear both;
      }

      .toast.is-leaving {
        animation: toast-fade var(--dur-fade) linear reverse both;
      }

      @keyframes toast-fade {
        from {
          opacity: 0;
        }

        to {
          opacity: 1;
        }
      }
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
