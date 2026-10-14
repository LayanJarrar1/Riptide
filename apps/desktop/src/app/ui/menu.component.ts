import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  input,
  output,
  viewChild,
} from '@angular/core';

import { IconComponent, IconName } from './icon.component';

/** One entry of a menu. */
export interface MenuItem {
  readonly id: string;
  readonly label: string;
  readonly icon?: IconName;
  readonly shortcut?: string;
  readonly danger?: boolean;
  readonly disabled?: boolean;
  /** Draws a rule above this item. */
  readonly separator?: boolean;
}

/**
 * Context / dropdown menu anchored at a viewport point. Keyboard: arrows move, Enter/Space
 * choose, Escape closes. Clicking outside closes. The menu is kept inside the viewport.
 */
@Component({
  selector: 'app-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  host: {
    '(document:keydown.escape)': 'open() && closed.emit()',
    '(document:pointerdown)': 'onDocumentPointer($event)',
  },
  template: `
    @if (open()) {
      <div
        #menu
        class="menu"
        role="menu"
        tabindex="-1"
        [style.inset-inline-start.px]="pos().x"
        [style.inset-block-start.px]="pos().y"
        (keydown)="onKey($event)"
      >
        @for (item of items(); track item.id) {
          @if (item.separator) {
            <div class="rule" role="separator"></div>
          }
          <button
            type="button"
            role="menuitem"
            class="item"
            [class.is-danger]="item.danger"
            [disabled]="item.disabled"
            (click)="choose(item)"
          >
            @if (item.icon; as icon) {
              <app-icon [name]="icon" [size]="15" />
            } @else {
              <span class="icon-gap"></span>
            }
            <span class="label">{{ item.label }}</span>
            @if (item.shortcut; as key) {
              <kbd class="key">{{ key }}</kbd>
            }
          </button>
        }
      </div>
    }
  `,
  styles: `
    .menu {
      position: fixed;
      z-index: var(--z-overlay);
      min-inline-size: 220px;
      padding: var(--space-1);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
      box-shadow: var(--shadow-3);
      outline: none;
      animation: menu-in var(--motion-fast) var(--ease-out) both;
    }

    @keyframes menu-in {
      from {
        opacity: 0;
        transform: scale(0.98);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .item {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      inline-size: 100%;
      block-size: 30px;
      padding-inline: var(--space-2) var(--space-3);
      border: 0;
      border-radius: var(--radius-xs);
      background: transparent;
      color: var(--color-text);
      font-size: var(--text-sm);
      text-align: start;
      cursor: pointer;
    }

    .item:hover:not(:disabled),
    .item:focus-visible {
      background: var(--color-surface-hover);
      outline: none;
    }

    .item:disabled {
      color: var(--color-text-muted);
      cursor: default;
    }

    .item.is-danger:not(:disabled) {
      color: var(--color-danger);
    }

    .icon-gap {
      inline-size: 15px;
    }

    .label {
      flex: 1;
    }

    .key {
      font: inherit;
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .rule {
      block-size: 1px;
      margin: var(--space-1) var(--space-2);
      background: var(--color-border);
    }
  `,
})
export class MenuComponent {
  private readonly menuRef = viewChild<ElementRef<HTMLElement>>('menu');

  readonly open = input.required<boolean>();
  readonly items = input.required<readonly MenuItem[]>();
  /** Anchor point in viewport pixels. */
  readonly x = input(0);
  readonly y = input(0);

  readonly selected = output<string>();
  readonly closed = output<void>();

  /** Anchor nudged so a 240×(items) box stays inside the viewport. */
  protected readonly pos = computed(() => {
    const w = 240;
    const h = this.items().length * 30 + 12;
    const vw = globalThis.innerWidth || 1280;
    const vh = globalThis.innerHeight || 800;
    const rtl = globalThis.document?.documentElement.dir === 'rtl';
    // inset-inline-start: measured from the right edge in RTL
    const xStart = rtl ? vw - this.x() : this.x();
    return { x: Math.max(4, Math.min(xStart, vw - w - 4)), y: Math.max(4, Math.min(this.y(), vh - h - 4)) };
  });

  constructor() {
    effect(() => {
      if (this.open()) {
        queueMicrotask(() => this.menuRef()?.nativeElement.querySelector<HTMLElement>('.item:not(:disabled)')?.focus());
      }
    });
  }

  protected choose(item: MenuItem): void {
    if (!item.disabled) {
      this.selected.emit(item.id);
      this.closed.emit();
    }
  }

  protected onDocumentPointer(event: PointerEvent): void {
    const el = this.menuRef()?.nativeElement;
    if (this.open() && el && !el.contains(event.target as Node)) {
      this.closed.emit();
    }
  }

  protected onKey(event: KeyboardEvent): void {
    const el = this.menuRef()?.nativeElement;
    if (!el) {
      return;
    }
    const items = Array.from(el.querySelectorAll<HTMLButtonElement>('.item:not(:disabled)'));
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next = items[(index + delta + items.length) % items.length];
      next?.focus();
    } else if (event.key === 'Home') {
      items[0]?.focus();
    } else if (event.key === 'End') {
      items[items.length - 1]?.focus();
    }
  }
}
