import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { DialogComponent } from '../../ui/dialog.component';

/** One shortcut: what it does and the key chords that trigger it (alternatives). */
interface Shortcut {
  readonly label: string;
  readonly chords: readonly (readonly string[])[];
}

interface ShortcutGroup {
  readonly title: string;
  readonly items: readonly Shortcut[];
}

/**
 * The keyboard-shortcuts sheet, opened with `?` or Ctrl+/ from anywhere. Everything the app
 * answers to, in two groups, in the active language.
 */
@Component({
  selector: 'app-shortcuts-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DialogComponent],
  template: `
    <app-dialog [open]="open()" [title]="i18n.t('shortcuts.title')" (closed)="closed.emit()">
      <div class="groups">
        @for (group of groups(); track group.title; let g = $index) {
          <section class="group" [style.--i]="g">
            <h3 class="group-title">{{ group.title }}</h3>
            <dl class="rows">
              @for (item of group.items; track item.label; let i = $index) {
                <div class="row" [style.--i]="g * 6 + i">
                  <dt class="label">{{ item.label }}</dt>
                  <dd class="keys">
                    @for (chord of item.chords; track $index; let last = $last) {
                      <span class="chord">
                        @for (key of chord; track $index; let lastKey = $last) {
                          <kbd class="key">{{ key }}</kbd>
                          @if (!lastKey) {
                            <span class="plus" aria-hidden="true">+</span>
                          }
                        }
                      </span>
                      @if (!last) {
                        <span class="or">/</span>
                      }
                    }
                  </dd>
                </div>
              }
            </dl>
          </section>
        }
      </div>
      <p class="hint">{{ i18n.t('shortcuts.hint') }}</p>
    </app-dialog>
  `,
  styles: `
    .groups {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-6);
      padding-block: var(--space-2);
    }

    @media (max-width: 640px) {
      .groups {
        grid-template-columns: 1fr;
      }
    }

    .group-title {
      margin-block-end: var(--space-2);
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      letter-spacing: var(--tracking-wide);
      text-transform: uppercase;
    }

    .rows {
      display: flex;
      flex-direction: column;
      margin: 0;
    }

    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-4);
      min-block-size: 36px;
      padding-block: var(--space-1);
      border-block-start: 1px solid var(--color-border);
      animation: row-in var(--dur-2) var(--ease-emphasized-out) both;
      animation-delay: calc(var(--i) * 18ms);
    }

    @keyframes row-in {
      from {
        opacity: 0;
        transform: translateY(4px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .label {
      font-size: var(--text-sm);
      color: var(--color-text);
    }

    .keys {
      display: flex;
      align-items: center;
      gap: var(--space-1);
      margin: 0;
      flex-shrink: 0;
    }

    .chord {
      display: inline-flex;
      align-items: center;
      gap: 3px;
    }

    .key {
      display: inline-grid;
      place-items: center;
      min-inline-size: 24px;
      block-size: 22px;
      padding-inline: 6px;
      border: 1px solid var(--color-border-strong);
      border-block-end-width: 2px;
      border-radius: var(--radius-xs);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      line-height: 1;
      direction: ltr;
    }

    .plus,
    .or {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .or {
      padding-inline: 3px;
    }

    .hint {
      padding-block: var(--space-3) var(--space-2);
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    @media (prefers-reduced-motion: reduce) {
      .row {
        animation-duration: var(--dur-fade);
        animation-delay: 0ms;
        transform: none;
      }
    }
  `,
})
export class ShortcutsDialogComponent {
  protected readonly i18n = inject(I18nService);

  readonly open = input.required<boolean>();
  readonly closed = output<void>();

  protected readonly groups = computed<readonly ShortcutGroup[]>(() => {
    const t = (key: string): string => this.i18n.t(key);
    return [
      {
        title: t('shortcuts.general'),
        items: [
          { label: t('shortcuts.add'), chords: [['Ctrl', 'N']] },
          { label: t('shortcuts.batch'), chords: [['Ctrl', 'Shift', 'N']] },
          { label: t('shortcuts.search'), chords: [['Ctrl', 'F']] },
          { label: t('shortcuts.settings'), chords: [['Ctrl', ',']] },
          { label: t('shortcuts.help'), chords: [['?'], ['Ctrl', '/']] },
          { label: t('shortcuts.escape'), chords: [['Esc']] },
        ],
      },
      {
        title: t('shortcuts.list'),
        items: [
          { label: t('shortcuts.move'), chords: [['↑'], ['↓']] },
          { label: t('shortcuts.details'), chords: [['Enter'], [t('key.dblclick')]] },
          { label: t('shortcuts.toggle'), chords: [[t('key.space')]] },
          { label: t('shortcuts.remove'), chords: [['Del']] },
          { label: t('shortcuts.multi'), chords: [['Ctrl', t('key.click')]] },
          { label: t('shortcuts.range'), chords: [['Shift', t('key.click')]] },
          { label: t('shortcuts.context'), chords: [[t('key.rightclick')]] },
          { label: t('shortcuts.moveInQueue'), chords: [['Alt', '↑'], ['Alt', '↓']] },
        ],
      },
    ];
  });
}
