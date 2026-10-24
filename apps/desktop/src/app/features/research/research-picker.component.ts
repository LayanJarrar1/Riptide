import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, input, output, signal, untracked, viewChild } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { IconComponent, IconName } from '../../ui/icon.component';

/** One entry of the picker: an icon, a name and a one-line description. */
export interface PickerOption {
  readonly id: string;
  readonly label: string;
  readonly description: string;
  readonly icon: IconName;
}

let uid = 0;

/**
 * A compact select with a popover list (a WAI-ARIA listbox): the button shows the choice with its
 * icon, the list shows every option with a one-line description. Keyboard: Enter, Space, ArrowDown or
 * ArrowUp on the button open it; arrows, Home and End move; Enter or Space choose; Escape closes and
 * gives the focus back; Tab closes. A click outside closes it.
 */
@Component({
  selector: 'app-research-picker',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  host: { '(document:pointerdown)': 'onOutside($event)' },
  template: `
    <button
      #trigger
      type="button"
      class="trigger"
      [class.is-empty]="selected() === null"
      aria-haspopup="listbox"
      [attr.aria-expanded]="open()"
      [attr.aria-controls]="open() ? listId : null"
      [attr.aria-label]="triggerLabel()"
      [attr.title]="label()"
      (click)="toggle()"
      (keydown)="onTriggerKey($event)"
    >
      <app-icon [name]="selected()?.icon ?? placeholderIcon()" [size]="15" />
      <span class="trigger-value">{{ selected()?.label ?? placeholder() }}</span>
      <app-icon class="caret" name="chevronDown" [size]="14" />
    </button>

    @if (open()) {
      <div
        #list
        class="list"
        [class.is-up]="up()"
        [style.inset-inline-start.px]="startPx()"
        [style.max-block-size.px]="maxPx()"
        [style.--list-top.px]="topPx()"
        [style.--list-bottom.px]="bottomPx()"
        role="listbox"
        tabindex="-1"
        [id]="listId"
        [attr.aria-label]="label()"
        (keydown)="onListKey($event)"
      >
        @for (o of options(); track o.id; let n = $index) {
          <button
            type="button"
            role="option"
            class="option"
            tabindex="-1"
            [attr.aria-selected]="o.id === value()"
            [class.is-selected]="o.id === value()"
            (click)="choose(o)"
            (mouseenter)="focusAt(n)"
          >
            <app-icon [name]="o.icon" [size]="18" />
            <span class="option-text">
              <span class="option-name">{{ o.label }}</span>
              <span class="option-desc">{{ o.description }}</span>
            </span>
            @if (o.id === value()) {
              <app-icon class="tick" name="check" [size]="14" />
            }
          </button>
        }
      </div>
    }
  `,
  styles: `
    :host {
      position: relative;
      display: inline-flex;
    }

    .trigger {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      max-inline-size: 100%;
      block-size: 34px;
      padding-inline: var(--space-3) var(--space-2);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-full);
      background: transparent;
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
      cursor: pointer;
      transition:
        background-color var(--dur-1) var(--ease-standard),
        border-color var(--dur-1) var(--ease-standard);
    }

    .trigger:hover,
    .trigger[aria-expanded='true'] {
      background: var(--color-surface-hover);
      border-color: var(--color-text-muted);
    }

    .trigger:focus-visible,
    .option:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .trigger.is-empty {
      border-style: dashed;
      color: var(--color-text-secondary);
    }

    .trigger > app-icon:first-child {
      color: var(--color-accent-fg);
      flex: none;
    }

    .trigger.is-empty > app-icon:first-child {
      color: var(--color-text-muted);
    }

    .trigger-value {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-weight: var(--weight-medium);
    }

    .caret {
      color: var(--color-text-muted);
      flex: none;
    }

    /* fixed: the page's scroll containers would otherwise clip the list; position and height are set when it opens */
    .list {
      position: fixed;
      inset-block-start: var(--list-top, auto);
      inset-block-end: var(--list-bottom, auto);
      overflow-y: auto;
      overscroll-behavior: contain;
      scrollbar-gutter: stable;
      z-index: var(--z-overlay);
      inline-size: max-content;
      min-inline-size: 280px;
      max-inline-size: min(360px, 86vw);
      padding: var(--space-1);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
      box-shadow: var(--shadow-3);
      animation: pop var(--dur-1) var(--ease-emphasized-out) both;
    }


    @keyframes pop {
      from {
        opacity: 0;
        transform: translateY(2px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .option {
      display: flex;
      align-items: flex-start;
      gap: var(--space-3);
      inline-size: 100%;
      padding: var(--space-2) var(--space-3);
      border: 0;
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text);
      font: inherit;
      text-align: start;
      cursor: pointer;
    }

    .option > app-icon:first-child {
      flex: none;
      margin-block-start: 1px;
      color: var(--color-text-secondary);
    }

    .option:hover,
    .option:focus-visible {
      background: var(--color-surface-hover);
      outline: none;
    }

    .option:focus-visible {
      box-shadow: inset 0 0 0 2px var(--color-focus);
    }

    .option.is-selected > app-icon:first-child,
    .tick {
      color: var(--color-accent-fg);
    }

    .option-text {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-inline-size: 0;
    }

    .option-name {
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .option-desc {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .tick {
      flex: none;
      margin-block-start: 2px;
    }

    @media (prefers-reduced-motion: reduce) {
      .list {
        animation: none;
      }
    }
  `,
})
export class ResearchPickerComponent {
  protected readonly i18n = inject(I18nService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly trigger = viewChild.required<ElementRef<HTMLButtonElement>>('trigger');
  private readonly list = viewChild<ElementRef<HTMLElement>>('list');

  /** The short name of what is being chosen ("Mode", "Category"). */
  readonly label = input.required<string>();
  readonly placeholder = input('');
  readonly placeholderIcon = input<IconName>('search');
  readonly options = input.required<readonly PickerOption[]>();
  readonly value = input<string | null>(null);
  /** Where the list opens: above the button for a control that sits at the bottom of the page. */
  readonly placement = input<'up' | 'down'>('down');

  readonly picked = output<string>();

  protected readonly listId = `research-picker-${++uid}`;
  protected readonly open = signal(false);
  protected readonly selected = computed(() => this.options().find((o) => o.id === this.value()) ?? null);

  protected readonly triggerLabel = computed(() => {
    const s = this.selected();
    return s === null ? this.label() : `${this.label()}: ${s.label}`;
  });

  /** Where the open list sits, worked out from the button's place in the window when it opens. */
  protected readonly up = signal(false);
  protected readonly startPx = signal(0);
  protected readonly topPx = signal<number | null>(null);
  protected readonly bottomPx = signal<number | null>(null);
  protected readonly maxPx = signal(320);

  private readonly closeOnResize = (): void => this.close(false);

  private readonly closeOnScroll = (event: Event): void => {
    // only a scroller that carries the button moves it away from the (fixed) list
    const target = event.target;
    if (target instanceof Node && target.contains(this.trigger().nativeElement)) {
      this.close(false);
    }
  };

  /** Opens below the button, or above it when there is more room there; the height follows the room. */
  private place(): void {
    const r = this.trigger().nativeElement.getBoundingClientRect();
    const gap = 6;
    const edge = 8;
    const below = window.innerHeight - r.bottom - gap - edge;
    const above = r.top - gap - edge;
    const wantUp = this.placement() === 'up';
    const fits = 340; // roughly the height of a full list
    const up = wantUp ? above >= Math.min(fits, below) : below < Math.min(fits, above);
    this.up.set(up);
    this.maxPx.set(Math.max(120, Math.floor(up ? above : below)));
    this.topPx.set(up ? null : r.bottom + gap);
    this.bottomPx.set(up ? window.innerHeight - r.top + gap : null);
    const rtl = this.i18n.dir() === 'rtl';
    const start = rtl ? window.innerWidth - r.right : r.left;
    this.startPx.set(Math.max(edge, start));
  }

  protected toggle(): void {
    if (this.open()) {
      this.close(false);
    } else {
      this.openList();
    }
  }

  /** The list was just opened: its chosen option takes the focus as soon as it is drawn. */
  private focusPending = false;

  constructor() {
    effect(() => {
      if (this.list() !== undefined && this.focusPending) {
        untracked(() => {
          this.focusPending = false;
          this.focusAt(Math.max(0, this.options().findIndex((o) => o.id === this.value())));
        });
      }
    });
  }

  private openList(): void {
    this.place();
    this.focusPending = true;
    this.open.set(true);
    window.addEventListener('scroll', this.closeOnScroll, true);
    window.addEventListener('resize', this.closeOnResize);
  }

  private close(refocus: boolean): void {
    this.open.set(false);
    window.removeEventListener('scroll', this.closeOnScroll, true);
    window.removeEventListener('resize', this.closeOnResize);
    if (refocus) {
      this.trigger().nativeElement.focus();
    }
  }

  protected choose(o: PickerOption): void {
    this.picked.emit(o.id);
    this.close(true);
  }

  protected focusAt(index: number): void {
    this.items()[index]?.focus();
  }

  private items(): HTMLElement[] {
    return Array.from(this.list()?.nativeElement.querySelectorAll<HTMLElement>('[role="option"]') ?? []);
  }

  protected onTriggerKey(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!this.open()) {
        this.openList();
      }
    } else if (event.key === 'Escape' && this.open()) {
      event.stopPropagation();
      this.close(true);
    }
  }

  protected onListKey(event: KeyboardEvent): void {
    const items = this.items();
    const index = items.indexOf(document.activeElement as HTMLElement);
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        this.focusAt((index + 1) % items.length);
        break;
      case 'ArrowUp':
        event.preventDefault();
        this.focusAt((index - 1 + items.length) % items.length);
        break;
      case 'Home':
        event.preventDefault();
        this.focusAt(0);
        break;
      case 'End':
        event.preventDefault();
        this.focusAt(items.length - 1);
        break;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        this.close(true);
        break;
      case 'Tab':
        this.close(false);
        break;
    }
  }

  protected onOutside(event: PointerEvent): void {
    if (this.open() && !this.host.nativeElement.contains(event.target as Node)) {
      this.close(false);
    }
  }
}
