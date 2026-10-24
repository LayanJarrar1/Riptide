import { ChangeDetectionStrategy, Component, computed, inject, input, output, signal } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { Conversation, MAX_TITLE } from './research-history';

type Confirm = { readonly kind: 'one'; readonly id: string; readonly title: string } | { readonly kind: 'all' };

/**
 * The list of research conversations: start a new one, switch, rename (inline: Enter saves,
 * Escape cancels), delete one and clear all (both ask first). Keyboard and screen-reader friendly:
 * every row is a button with its own rename and delete buttons.
 */
@Component({
  selector: 'app-research-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent, IconComponent],
  template: `
    <nav class="side" [attr.aria-label]="i18n.t('research.history')">
      <button appButton variant="primary" icon="plus" type="button" class="new" (click)="created.emit()">{{ i18n.t('research.new') }}</button>

      @if (conversations().length === 0) {
        <p class="empty">{{ i18n.t('research.history.empty') }}</p>
      } @else {
        <div class="search">
          <app-icon name="search" [size]="14" />
          <input
            class="search-input"
            type="search"
            dir="auto"
            [attr.aria-label]="i18n.t('research.history.search')"
            [placeholder]="i18n.t('research.history.search')"
            [value]="query()"
            (input)="query.set(text($event))"
            (keydown.escape)="query.set('')"
          />
        </div>
        @if (shown().length === 0) {
          <p class="empty" role="status">{{ i18n.t('research.history.noMatch') }}</p>
        }
        <ul class="list">
          @for (c of shown(); track c.id) {
            <li class="row" [class.is-active]="c.id === activeId()">
              @if (editing() === c.id) {
                <input
                  class="rename"
                  dir="auto"
                  type="text"
                  [attr.maxlength]="maxTitle"
                  [attr.aria-label]="i18n.t('research.rename')"
                  [value]="c.title"
                  (keydown.enter)="commit(c.id, $event)"
                  (keydown.escape)="editing.set(null)"
                  (blur)="commit(c.id, $event)"
                />
              } @else {
                <button type="button" class="open" [attr.aria-current]="c.id === activeId() ? 'true' : null" (click)="selected.emit(c.id)">
                  <span class="name" dir="auto">{{ c.title || i18n.t('research.untitled') }}</span>
                </button>
                <span class="tools">
                  <button type="button" class="tool" [attr.aria-label]="i18n.t('research.rename') + ': ' + c.title" (click)="startEdit(c.id)"><app-icon name="edit" [size]="14" /></button>
                  <button type="button" class="tool" [attr.aria-label]="i18n.t('research.delete') + ': ' + c.title" (click)="confirm.set({ kind: 'one', id: c.id, title: c.title })"><app-icon name="trash" [size]="14" /></button>
                </span>
              }
            </li>
          }
        </ul>
        <button appButton variant="ghost" size="sm" icon="trash" type="button" class="clear" (click)="confirm.set({ kind: 'all' })">{{ i18n.t('research.clearAll') }}</button>
      }
      <p class="local"><app-icon name="shield" [size]="12" /> {{ i18n.t('research.history.local') }}</p>
    </nav>

    <app-dialog [open]="confirm() !== null" [title]="confirmTitle()" (closed)="confirm.set(null)">
      <p class="text">{{ confirmBody() }}</p>
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="confirm.set(null)">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" type="button" (click)="doConfirm()">{{ i18n.t('research.delete') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    :host {
      display: block;
      block-size: 100%;
    }

    .side {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      block-size: 100%;
      padding: var(--space-4) var(--space-3);
      overflow-y: auto;
    }

    .new {
      inline-size: 100%;
    }

    .search {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      block-size: 32px;
      padding-inline: var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-muted);
    }

    .search:focus-within {
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .search-input {
      flex: 1;
      min-inline-size: 0;
      border: 0;
      background: transparent;
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
      outline: none;
    }

    .empty {
      padding: var(--space-3);
      color: var(--color-text-muted);
      font-size: var(--text-sm);
      line-height: var(--leading-normal);
    }

    .list {
      list-style: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-direction: column;
      gap: 2px;
    }

    .row {
      display: flex;
      align-items: center;
      border-radius: var(--radius-sm);
    }

    .row:hover,
    .row.is-active {
      background: var(--color-surface-2);
    }

    .row.is-active {
      box-shadow: inset 2px 0 0 var(--color-accent);
    }

    :host-context([dir='rtl']) .row.is-active {
      box-shadow: inset -2px 0 0 var(--color-accent);
    }

    .open {
      flex: 1;
      min-inline-size: 0;
      padding: var(--space-2) var(--space-3);
      border: 0;
      background: transparent;
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
      text-align: start;
      cursor: pointer;
    }

    .name {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .tools {
      display: none;
      padding-inline-end: var(--space-1);
    }

    .row:hover .tools,
    .row:focus-within .tools {
      display: inline-flex;
    }

    .tool {
      display: inline-grid;
      place-items: center;
      inline-size: 26px;
      block-size: 26px;
      border: 0;
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text-muted);
      cursor: pointer;
    }

    .tool:hover {
      color: var(--color-text);
      background: var(--color-surface-3);
    }

    .open:focus-visible,
    .tool:focus-visible {
      outline: 2px solid var(--color-accent);
      outline-offset: -2px;
    }

    .rename {
      flex: 1;
      min-inline-size: 0;
      block-size: 32px;
      margin: 2px;
      padding-inline: var(--space-2);
      border: 1px solid var(--color-accent);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .clear {
      align-self: flex-start;
    }

    .local {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin-block-start: auto;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .text {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
    }
  `,
})
export class ResearchSidebarComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly maxTitle = MAX_TITLE;

  readonly conversations = input.required<readonly Conversation[]>();
  readonly activeId = input<string | null>(null);

  readonly created = output<void>();
  readonly selected = output<string>();
  readonly renamed = output<{ id: string; title: string }>();
  readonly deleted = output<string>();
  readonly cleared = output<void>();

  protected readonly query = signal('');
  /** The conversations whose title or any prompt contains the search text. */
  protected readonly shown = computed(() => {
    const q = this.query().trim().toLocaleLowerCase();
    if (q.length === 0) {
      return this.conversations();
    }
    return this.conversations().filter((c) => c.title.toLocaleLowerCase().includes(q) || c.turns.some((t) => t.prompt.toLocaleLowerCase().includes(q)));
  });
  protected readonly editing = signal<string | null>(null);
  protected readonly confirm = signal<Confirm | null>(null);

  protected readonly confirmTitle = computed(() => this.i18n.t(this.confirm()?.kind === 'all' ? 'research.clearAll' : 'research.delete'));
  protected readonly confirmBody = computed(() => {
    const c = this.confirm();
    return c?.kind === 'one' ? this.i18n.t('research.deleteBody').replace('{title}', c.title) : this.i18n.t('research.clearAllBody');
  });

  protected text(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected startEdit(id: string): void {
    this.editing.set(id);
    setTimeout(() => document.querySelector<HTMLInputElement>('app-research-sidebar .rename')?.select(), 0);
  }

  protected commit(id: string, event: Event): void {
    if (this.editing() !== id) {
      return;
    }
    this.editing.set(null);
    const title = (event.target as HTMLInputElement).value.trim();
    if (title.length > 0) {
      this.renamed.emit({ id, title });
    }
  }

  protected doConfirm(): void {
    const c = this.confirm();
    this.confirm.set(null);
    if (c?.kind === 'one') {
      this.deleted.emit(c.id);
    } else if (c?.kind === 'all') {
      this.cleared.emit();
    }
  }
}
