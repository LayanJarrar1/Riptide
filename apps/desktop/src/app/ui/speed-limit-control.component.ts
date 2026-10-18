import { ChangeDetectionStrategy, Component, computed, inject, input, linkedSignal, output } from '@angular/core';

import { FORMAT_PIPES } from '../core/format.pipes';
import { I18nService } from '../i18n/i18n.service';

const KB = 1024;
const MB = 1024 * 1024;

/** Presets offered next to "Unlimited" (bytes per second). */
export const SPEED_LIMIT_PRESETS: readonly number[] = [1 * MB, 5 * MB, 10 * MB, 25 * MB, 50 * MB];

/** Units of the custom field. */
export type SpeedUnit = 'kbps' | 'mbps';

const UNIT_BYTES: Readonly<Record<SpeedUnit, number>> = { kbps: KB, mbps: MB };

/** `amount` in `unit` → bytes per second; anything not a positive number is "unlimited". */
export function toLimitBps(amount: number, unit: SpeedUnit): number | null {
  if (!Number.isFinite(amount) || amount <= 0) {
    return null;
  }
  return Math.max(1, Math.round(amount * UNIT_BYTES[unit]));
}

/** The unit a limit reads best in. */
export function unitFor(bps: number | null): SpeedUnit {
  return bps !== null && bps >= MB ? 'mbps' : 'kbps';
}

/**
 * A speed cap: "Unlimited", a few presets and a custom amount in KB/s or MB/s. `compact` drops
 * the presets (the details drawer). Emits bytes per second, `null` for unlimited.
 */
@Component({
  selector: 'app-speed-limit-control',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES],
  template: `
    @if (!compact()) {
      <div class="chips" role="radiogroup" [attr.aria-label]="label()">
        <button type="button" role="radio" class="chip" [class.is-active]="value() === null" [attr.aria-checked]="value() === null" (click)="valueChange.emit(null)">
          {{ i18n.t('settings.unlimited') }}
        </button>
        @for (limit of presets; track limit) {
          <button type="button" role="radio" class="chip num" [class.is-active]="limit === value()" [attr.aria-checked]="limit === value()" (click)="valueChange.emit(limit)">
            {{ limit | speed }}
          </button>
        }
        <button type="button" role="radio" class="chip" [class.is-active]="isCustom()" [attr.aria-checked]="isCustom()" (click)="openCustom()">
          {{ i18n.t('speedLimit.custom') }}
        </button>
      </div>
    }
    @if (compact() || isCustom()) {
      <div class="custom">
        <input
          class="amount num"
          type="number"
          min="0"
          step="any"
          inputmode="decimal"
          dir="ltr"
          [value]="amount()"
          [placeholder]="i18n.t('settings.unlimited')"
          [attr.aria-label]="i18n.t('speedLimit.customLabel')"
          (change)="onAmount($event)"
        />
        <select class="unit" [value]="unit()" [attr.aria-label]="i18n.t('speedLimit.unit')" (change)="onUnit($event)">
          <option value="kbps" [selected]="unit() === 'kbps'">{{ i18n.t('speedLimit.kbps') }}</option>
          <option value="mbps" [selected]="unit() === 'mbps'">{{ i18n.t('speedLimit.mbps') }}</option>
        </select>
        @if (compact() && value() !== null) {
          <button type="button" class="chip" (click)="valueChange.emit(null)">{{ i18n.t('settings.unlimited') }}</button>
        }
      </div>
    }
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      gap: var(--space-2);
    }

    :host(.compact) {
      align-items: flex-start;
    }

    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1);
      justify-content: flex-end;
    }

    .chip {
      block-size: 28px;
      padding-inline: 10px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-full);
      background: transparent;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      cursor: pointer;
    }

    .chip.is-active {
      border-color: transparent;
      background: var(--color-accent);
      color: var(--color-on-accent);
    }

    .chip:focus-visible,
    .amount:focus-visible,
    .unit:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .custom {
      display: flex;
      align-items: center;
      gap: var(--space-2);
    }

    .amount,
    .unit {
      block-size: 30px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .amount {
      inline-size: 110px;
      padding-inline: 10px;
    }

    .unit {
      padding-inline: 6px;
    }
  `,
  host: { '[class.compact]': 'compact()' },
})
export class SpeedLimitControlComponent {
  protected readonly i18n = inject(I18nService);

  /** Current cap in bytes per second, `null` = unlimited. */
  readonly value = input.required<number | null>();
  /** No presets: just the amount, the unit and an "unlimited" button. */
  readonly compact = input(false);
  /** Accessible name of the preset group. */
  readonly label = input('');

  /** The new cap. */
  readonly valueChange = output<number | null>();

  protected readonly presets = SPEED_LIMIT_PRESETS;

  /** The custom field is open (the value is not a preset, or the user asked for it). */
  protected readonly customOpen = linkedSignal(() => {
    const v = this.value();
    return v !== null && !SPEED_LIMIT_PRESETS.includes(v);
  });
  protected readonly isCustom = computed(() => this.customOpen());

  protected readonly unit = linkedSignal<SpeedUnit>(() => unitFor(this.value()));

  /** The value in the chosen unit, blank when unlimited. */
  protected readonly amount = computed(() => {
    const v = this.value();
    if (v === null) {
      return '';
    }
    const n = v / UNIT_BYTES[this.unit()];
    return String(Math.round(n * 100) / 100);
  });

  protected openCustom(): void {
    this.customOpen.set(true);
  }

  protected onAmount(event: Event): void {
    const raw = (event.target as HTMLInputElement).value.trim();
    this.valueChange.emit(toLimitBps(raw === '' ? 0 : Number(raw), this.unit()));
  }

  protected onUnit(event: Event): void {
    const unit = (event.target as HTMLSelectElement).value === 'mbps' ? 'mbps' : 'kbps';
    const amount = Number(this.amount());
    this.unit.set(unit);
    if (this.value() !== null && amount > 0) {
      this.valueChange.emit(toLimitBps(amount, unit));
    }
  }
}
