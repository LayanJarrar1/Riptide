import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';

const HOURS: readonly string[] = Array.from({ length: 24 }, (_, i) => String(i).padStart(2, '0'));
const MINUTES: readonly string[] = Array.from({ length: 60 }, (_, i) => String(i).padStart(2, '0'));

/**
 * A 24-hour time of day as two selects (hours, minutes), always laid out left to right with Latin
 * digits. The native `<input type="time">` follows the operating system locale, so it showed
 * "11:00 PM" in the Arabic UI; this one looks the same in both languages and in every webview.
 * Value in and out is `HH:mm`.
 */
@Component({
  selector: 'app-time-field',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="field" role="group" [attr.aria-label]="label()">
      <span class="label" aria-hidden="true">{{ label() }}</span>
      <div class="selects" dir="ltr">
        <select class="select" [attr.name]="name() + '-h'" [attr.aria-label]="label() + ': ' + i18n.t('schedule.hours')" (change)="pick($event, 'h')">
          @for (h of hours; track h) {
            <option [value]="h" [selected]="h === parts().h">{{ h }}</option>
          }
        </select>
        <span class="colon" aria-hidden="true">:</span>
        <select class="select" [attr.name]="name() + '-m'" [attr.aria-label]="label() + ': ' + i18n.t('schedule.minutes')" (change)="pick($event, 'm')">
          @for (m of minutes; track m) {
            <option [value]="m" [selected]="m === parts().m">{{ m }}</option>
          }
        </select>
      </div>
    </div>
  `,
  styles: `
    :host {
      display: block;
    }

    .field {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .label {
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .selects {
      display: flex;
      align-items: center;
      gap: var(--space-2);
    }

    .select {
      flex: 1 1 0;
      min-inline-size: 0;
      block-size: 36px;
      padding-inline: 10px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      color-scheme: inherit;
      font: inherit;
      font-size: var(--text-sm);
      font-variant-numeric: tabular-nums;
    }

    .select:focus {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .colon {
      color: var(--color-text-secondary);
    }
  `,
})
export class TimeFieldComponent {
  protected readonly i18n = inject(I18nService);

  /** The time as `HH:mm` (24 h). */
  readonly value = input.required<string>();
  /** Visible label, also the accessible name of the group. */
  readonly label = input.required<string>();
  /** Form-style name prefix for the two selects (`<name>-h`, `<name>-m`). */
  readonly name = input.required<string>();

  /** The user picked a new time (`HH:mm`). */
  readonly valueChange = output<string>();

  protected readonly hours = HOURS;
  protected readonly minutes = MINUTES;
  protected readonly parts = computed(() => {
    const match = /^(\d{2}):(\d{2})/.exec(this.value());
    return { h: match?.[1] ?? '00', m: match?.[2] ?? '00' };
  });

  protected pick(event: Event, part: 'h' | 'm'): void {
    const next = (event.target as HTMLSelectElement).value;
    const { h, m } = this.parts();
    this.valueChange.emit(part === 'h' ? `${next}:${m}` : `${h}:${next}`);
  }
}
