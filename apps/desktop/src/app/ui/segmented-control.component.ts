import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

/** One choice in a segmented control. */
export interface SegmentedOption<T extends string> {
  readonly value: T;
  readonly label: string;
}

/**
 * A small radio group rendered as adjoining pills: used for the language and theme
 * switches. Fully keyboard accessible (each segment is a real button) and direction-agnostic
 * (flex + logical properties, so RTL mirrors it for free).
 */
@Component({
  selector: 'app-segmented-control',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="segmented" role="radiogroup" [attr.aria-label]="label()">
      @for (option of options(); track option.value) {
        <button
          type="button"
          role="radio"
          class="segment"
          [class.is-active]="option.value === value()"
          [attr.aria-checked]="option.value === value()"
          [disabled]="disabled()"
          [attr.lang]="optionLang()[option.value] ?? null"
          (click)="select(option.value)"
        >
          {{ option.label }}
        </button>
      }
    </div>
  `,
  styles: `
    .segmented {
      display: inline-flex;
      padding: 3px;
      gap: 2px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-full);
      background: var(--color-surface-1);
    }

    .segment {
      border: 0;
      background: transparent;
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      line-height: 1;
      padding: var(--space-2) var(--space-3);
      border-radius: var(--radius-full);
      cursor: pointer;
      transition:
        color var(--motion-fast) var(--ease-standard),
        background-color var(--motion-fast) var(--ease-standard);
    }

    .segment:disabled {
      cursor: default;
      opacity: 0.6;
    }

    /* a locked group still shows what is selected at full strength; only the other choices dim */
    .segment.is-active:disabled {
      opacity: 1;
    }

    .segment:hover:not(:disabled) {
      color: var(--color-text);
      background: var(--color-surface-hover);
    }

    .segment.is-active {
      color: var(--color-on-accent);
      background: var(--color-accent);
    }

    .segment:focus-visible {
      outline-offset: 0;
    }
  `,
})
export class SegmentedControlComponent<T extends string> {
  /** Accessible name of the group. */
  readonly label = input.required<string>();

  /** Choices in display order. */
  readonly options = input.required<readonly SegmentedOption<T>[]>();

  /** The selected value. */
  readonly value = input.required<T>();

  /** Optional per-option `lang` attribute (used so "العربية" / "English" shape correctly). */
  readonly optionLang = input<Partial<Record<T, string>>>({});

  /** Greys the whole group out and ignores clicks. */
  readonly disabled = input(false);

  /** Emits when the user picks a different segment. */
  readonly valueChange = output<T>();

  protected select(next: T): void {
    if (!this.disabled() && next !== this.value()) {
      this.valueChange.emit(next);
    }
  }
}
