import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

/**
 * On/off switch on a native checkbox (`role="switch"`), so keyboard, focus and screen readers
 * come for free. Put it inside a `<label>` (a settings row does) or give it a `label`.
 */
@Component({
  selector: 'app-switch',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span class="switch" [class.is-on]="checked()" [class.is-disabled]="disabled()">
      <input
        type="checkbox"
        role="switch"
        [checked]="checked()"
        [disabled]="disabled()"
        [attr.aria-label]="label()"
        (change)="checkedChange.emit(asInput($event).checked)"
      />
      <span class="knob"></span>
    </span>
  `,
  styles: `
    :host {
      display: inline-flex;
      flex-shrink: 0;
    }

    .switch {
      position: relative;
      inline-size: 40px;
      block-size: 22px;
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
      border: 1px solid var(--color-border-strong);
      transition: background-color var(--motion-base) var(--ease-standard);
    }

    .switch.is-disabled {
      opacity: 0.5;
    }

    .switch input {
      position: absolute;
      inset: 0;
      margin: 0;
      opacity: 0;
      cursor: pointer;
    }

    .switch.is-disabled input {
      cursor: not-allowed;
    }

    .knob {
      position: absolute;
      inset-block-start: 2px;
      inset-inline-start: 2px;
      inline-size: 16px;
      block-size: 16px;
      border-radius: var(--radius-full);
      background: var(--color-text-muted);
      pointer-events: none;
      transition:
        transform var(--motion-base) var(--ease-emphasized),
        background-color var(--motion-base) var(--ease-standard);
    }

    .switch.is-on {
      background: var(--color-accent);
      border-color: transparent;
    }

    .switch.is-on .knob {
      background: var(--color-on-accent);
      transform: translateX(calc(var(--dir-sign) * 18px));
    }

    .switch:has(input:focus-visible) {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }
  `,
})
export class SwitchComponent {
  readonly checked = input.required<boolean>();
  /** Accessible name when the switch is not inside a labelling element. */
  readonly label = input<string | null>(null);
  readonly disabled = input(false);
  readonly checkedChange = output<boolean>();

  protected asInput(event: Event): HTMLInputElement {
    return event.target as HTMLInputElement;
  }
}
