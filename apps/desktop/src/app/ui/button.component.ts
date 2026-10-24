import { ChangeDetectionStrategy, Component, input } from '@angular/core';

import { IconComponent, IconName } from './icon.component';

/** Visual weight of a button. One primary action per screen. */
export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

/**
 * The button. Content is projected, an optional leading icon is drawn from the icon set, and
 * `iconOnly` buttons must pass an `aria-label` (the template lint enforces it at call sites).
 */
@Component({
  // Attribute selector on purpose: the host stays a native <button>/<a>, so focus, disabled,
  // form submission and screen-reader semantics come from the platform, not from us.
  // eslint-disable-next-line @angular-eslint/component-selector
  selector: 'button[appButton], a[appButton]',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  host: {
    '[class.v-primary]': 'variant() === "primary"',
    '[class.v-secondary]': 'variant() === "secondary"',
    '[class.v-ghost]': 'variant() === "ghost"',
    '[class.v-danger]': 'variant() === "danger"',
    '[class.is-small]': 'size() === "sm"',
    '[class.is-icon-only]': 'iconOnly()',
    '[class.is-busy]': 'busy()',
    '[attr.aria-busy]': 'busy() ? true : null',
  },
  template: `
    @if (busy()) {
      <span class="spinner" aria-hidden="true"></span>
    } @else if (icon(); as name) {
      <app-icon [name]="name" [size]="size() === 'sm' ? 15 : 17" />
    }
    <ng-content />
  `,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: var(--space-2);
      block-size: var(--control-height);
      padding-inline: var(--space-3);
      border: 1px solid transparent;
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      line-height: 1;
      white-space: nowrap;
      cursor: pointer;
      user-select: none;
      text-decoration: none;
      transition:
        background-color var(--motion-fast) var(--ease-standard),
        border-color var(--motion-fast) var(--ease-standard),
        color var(--motion-fast) var(--ease-standard),
        transform var(--motion-fast) var(--ease-standard);
    }

    :host(.is-small) {
      block-size: var(--control-height-sm);
      padding-inline: var(--space-2);
      font-size: var(--text-xs);
      border-radius: var(--radius-xs);
    }

    :host(.is-icon-only) {
      inline-size: var(--control-height);
      padding-inline: 0;
    }

    :host(.is-icon-only.is-small) {
      inline-size: var(--control-height-sm);
    }

    :host(:active:not(:disabled)) {
      transform: translateY(1px);
    }

    :host(:disabled),
    :host([aria-disabled='true']) {
      opacity: 0.45;
      cursor: not-allowed;
      transform: none;
    }

    :host(.v-primary:disabled),
    :host(.v-primary[aria-disabled='true']) {
      opacity: 1;
      background: var(--color-surface-3);
      color: var(--color-text-secondary);
      box-shadow: inset 0 0 0 1px var(--color-border-strong);
    }

    :host(.v-primary) {
      background: var(--color-accent);
      color: var(--color-on-accent);
      box-shadow: var(--shadow-1);
    }

    :host(.v-primary:hover:not(:disabled)) {
      background: var(--color-accent-light);
    }

    :host(.v-secondary) {
      background: var(--color-surface-2);
      border-color: var(--color-border-strong);
      color: var(--color-text);
    }

    :host(.v-secondary:hover:not(:disabled)) {
      background: var(--color-surface-3);
    }

    :host(.v-ghost) {
      color: var(--color-text-secondary);
    }

    :host(.v-ghost:hover:not(:disabled)) {
      background: var(--color-surface-hover);
      color: var(--color-text);
    }

    :host(.v-danger) {
      background: var(--state-failed-soft);
      border-color: color-mix(in srgb, var(--color-danger) 35%, transparent);
      color: var(--color-danger);
    }

    :host(.v-danger:hover:not(:disabled)) {
      background: color-mix(in srgb, var(--color-danger) 22%, transparent);
    }

    .spinner {
      inline-size: 14px;
      block-size: 14px;
      border-radius: var(--radius-full);
      border: 2px solid color-mix(in srgb, currentcolor 30%, transparent);
      border-block-start-color: currentcolor;
      animation: spin 800ms linear infinite;
    }

    @keyframes spin {
      to {
        transform: rotate(360deg);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .spinner {
        animation-duration: 1600ms;
      }
    }
  `,
})
export class ButtonComponent {
  /** Visual weight. */
  readonly variant = input<ButtonVariant>('secondary');

  /** Height: 32px (`md`) or 26px (`sm`). */
  readonly size = input<'md' | 'sm'>('md');

  /** Leading icon. */
  readonly icon = input<IconName | null>(null);

  /** Square button with only the icon; pass `aria-label`. */
  readonly iconOnly = input(false);

  /** Shows a spinner in place of the icon and announces `aria-busy`. */
  readonly busy = input(false);
}
