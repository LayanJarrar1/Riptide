import { ChangeDetectionStrategy, Component, input } from '@angular/core';

import { IconComponent, IconName } from './icon.component';

/**
 * An empty list is an invitation: a quiet icon, one sentence that says what the state means and
 * one that says what to do, and (projected) the action itself.
 */
@Component({
  selector: 'app-empty-state',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div class="empty">
      <div class="glyph"><app-icon [name]="icon()" [size]="28" /></div>
      <p class="heading">{{ heading() }}</p>
      @if (body(); as text) {
        <p class="body">{{ text }}</p>
      }
      <div class="actions"><ng-content /></div>
    </div>
  `,
  styles: `
    :host {
      display: grid;
      place-items: center;
      block-size: 100%;
      min-block-size: 240px;
      padding: var(--space-8);
    }

    .empty {
      display: flex;
      flex-direction: column;
      align-items: center;
      text-align: center;
      max-inline-size: 36ch;
    }

    .glyph {
      display: grid;
      place-items: center;
      inline-size: 56px;
      block-size: 56px;
      border-radius: var(--radius-full);
      background: var(--color-surface-2);
      border: 1px solid var(--color-border);
      color: var(--color-text-muted);
    }

    .heading {
      margin-block-start: var(--space-4);
      font-size: var(--text-lg);
      font-weight: var(--weight-semibold);
    }

    .body {
      margin-block-start: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
    }

    .actions {
      display: flex;
      gap: var(--space-2);
      margin-block-start: var(--space-5);
    }

    .actions:empty {
      display: none;
    }
  `,
})
export class EmptyStateComponent {
  readonly icon = input<IconName>('inbox');
  readonly heading = input.required<string>();
  readonly body = input<string | null>(null);
}
