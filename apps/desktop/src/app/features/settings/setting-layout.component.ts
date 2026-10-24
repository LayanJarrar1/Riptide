import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, input, ViewEncapsulation } from '@angular/core';

/**
 * Styles shared by every Settings section: groups (a titled card), rows (text on the start side,
 * control on the end side) and the form controls that go in a row. They are global to the page on
 * purpose (`ViewEncapsulation.None`, `sg-` / `sr-` prefixes) so controls projected into a row
 * pick them up.
 */
export const SETTING_LAYOUT_STYLES = `
  .sg {
    margin-block-end: var(--space-8);
    scroll-margin-block-start: var(--space-4);
  }

  .sg-head {
    margin-block-end: var(--space-3);
  }

  .sg-title {
    margin: 0;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }

  .sg-desc {
    margin: var(--space-1) 0 0;
    max-inline-size: 70ch;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
  }

  .sg-card {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-1);
    overflow: clip;
  }

  .sg-card > * + * {
    border-block-start: 1px solid var(--color-border);
  }

  app-setting-row {
    display: block;
  }

  .sr {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-2) var(--space-8);
    padding: var(--space-4) var(--space-5);
    min-block-size: 56px;
  }

  label.sr {
    cursor: pointer;
  }

  .sr.is-stack {
    grid-template-columns: minmax(0, 1fr);
    align-items: stretch;
  }

  .sr-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .sr-name {
    font-weight: var(--weight-medium);
  }

  .sr-desc {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
  }

  .sr-control {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
    min-inline-size: 0;
  }

  .sr-control.is-field {
    inline-size: 280px;
  }

  .sr.is-stack .sr-control {
    justify-content: flex-start;
    flex-wrap: wrap;
  }

  .sr.is-stack .sr-control.is-field {
    inline-size: auto;
  }

  .sr-block {
    padding: var(--space-4) var(--space-5);
  }

  .sr-input {
    inline-size: 100%;
    block-size: 34px;
    padding-inline: 12px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-sm);
    min-inline-size: 0;
  }

  .sr-input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-ring);
  }

  .sr-input.is-invalid {
    border-color: var(--color-danger);
  }

  .sr-input::placeholder {
    color: var(--color-text-muted);
  }

  .sr-input:disabled {
    opacity: 0.6;
  }

  .sr-error {
    margin: 0;
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .sr-note {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
  }

  .sr-value {
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
  }

  .sr-pill {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 3px 12px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-full);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    white-space: nowrap;
  }

  .sr-pill.is-ok {
    border-color: var(--color-success);
  }

  .sr-pill.is-bad {
    border-color: var(--color-danger);
  }

  .sr-pill.is-warn {
    border-color: var(--color-warning);
  }

  .sr-dot {
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-full);
    background: var(--color-text-muted);
    flex-shrink: 0;
  }

  .is-ok > .sr-dot,
  .sr-dot.is-ok {
    background: var(--color-success);
  }

  .is-bad > .sr-dot {
    background: var(--color-danger);
  }

  .is-warn > .sr-dot {
    background: var(--color-warning);
  }

  .sr-range {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    inline-size: 100%;
  }

  .sr-range input {
    flex: 1;
    min-inline-size: 0;
    accent-color: var(--color-accent);
  }

  .sr-range output {
    inline-size: 2.5ch;
    text-align: end;
    font-weight: var(--weight-semibold);
  }

  .sr-hint {
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-3);
    letter-spacing: 0.04em;
    unicode-bidi: isolate;
    font-size: var(--text-sm);
  }

  .sr-ok {
    color: var(--color-success);
    flex-shrink: 0;
  }

  .sr-fields {
    display: flex;
    gap: var(--space-2);
    inline-size: 100%;
    align-items: center;
  }

  .sr-fields > .sr-input {
    flex: 1;
  }

  .sr-folders {
    display: grid;
    grid-template-columns: minmax(120px, 160px) minmax(0, 1fr);
    align-items: center;
    gap: var(--space-2) var(--space-3);
    inline-size: 100%;
  }

  .sr-folders-name {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  @container settings (max-inline-size: 560px) {
    .sr {
      grid-template-columns: minmax(0, 1fr);
      align-items: stretch;
    }

    .sr-control,
    .sr-control.is-field {
      justify-content: flex-start;
      inline-size: auto;
    }

    .sr-folders {
      grid-template-columns: minmax(0, 1fr);
    }
  }
`;

/**
 * A titled card of rows. `anchor` becomes the element id, so old deep links such as
 * `getElementById('research-sources')` and `/settings#research-sources` find the group.
 */
@Component({
  selector: 'app-setting-group',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  styles: SETTING_LAYOUT_STYLES,
  template: `
    <section class="sg" [attr.id]="anchor()" [attr.aria-labelledby]="titleId">
      <header class="sg-head">
        <h3 class="sg-title" [id]="titleId">{{ heading() }}</h3>
        @if (description(); as text) {
          <p class="sg-desc">{{ text }}</p>
        }
      </header>
      <div class="sg-card"><ng-content /></div>
    </section>
  `,
})
export class SettingGroupComponent {
  private static nextId = 0;

  readonly heading = input.required<string>();
  readonly description = input<string | null>(null);
  readonly anchor = input<string | null>(null);

  protected readonly titleId = `sg-title-${SettingGroupComponent.nextId++}`;
}

/**
 * One setting: name and description on the start side, the control (projected) on the end side.
 * `stack` puts the control under the text for wide controls; `field` gives it the standard
 * control width; `clickable` makes the whole row a `<label>` (switches).
 */
@Component({
  selector: 'app-setting-row',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [NgTemplateOutlet],
  template: `
    <ng-template #body>
      <div class="sr-text">
        <span class="sr-name">{{ label() }}</span>
        @if (description(); as text) {
          <span class="sr-desc">{{ text }}</span>
        }
      </div>
      <div class="sr-control" [class.is-field]="field()"><ng-content /></div>
    </ng-template>
    @if (clickable()) {
      <label class="sr" [class.is-stack]="stack()"><ng-container [ngTemplateOutlet]="body" /></label>
    } @else {
      <div class="sr" [class.is-stack]="stack()"><ng-container [ngTemplateOutlet]="body" /></div>
    }
  `,
})
export class SettingRowComponent {
  readonly label = input.required<string>();
  readonly description = input<string | null>(null);
  readonly stack = input(false);
  readonly field = input(false);
  readonly clickable = input(false);
}
