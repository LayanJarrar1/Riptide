import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, input, output, signal, untracked, viewChild } from '@angular/core';

import { I18nService } from '../../i18n/i18n.service';
import { IconComponent, IconName } from '../../ui/icon.component';
import { AxIconComponent, AxIconName } from './ax-icon.component';

/** One entry of the picker: an icon, a name and a one-line description. */
export interface PickerOption {
  readonly id: string;
  readonly label: string;
  readonly description: string;
  readonly icon: IconName;
}

let uid = 0;

/**
 * A footer trigger with a popover menu, drawn like the Astryx ChatComposer's "Auto" / "Settings"
 * triggers and dropdown menu (a WAI-ARIA menu of radio items): the button shows the choice with its
 * icon and a chevron, the menu shows every option as one line of text (its description is the tooltip and the accessible description). Keyboard: Enter,
 * Space, ArrowDown or ArrowUp on the button open it; arrows, Home and End move; Enter or Space
 * choose; Escape closes and gives the focus back; Tab closes. A click outside closes it.
 */
@Component({
  selector: 'app-research-picker',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, AxIconComponent],
  host: { '(document:pointerdown)': 'onOutside($event)' },
  template: `
    <button
      #trigger
      type="button"
      class="trigger"
      [class.is-empty]="selected() === null"
      aria-haspopup="menu"
      [attr.aria-expanded]="open()"
      [attr.aria-controls]="open() ? listId : null"
      [attr.aria-label]="triggerLabel()"
      (click)="toggle()"
      (keydown)="onTriggerKey($event)"
    >
      @if (fixedIcon(); as fixed) {
        <app-ax-icon [name]="fixed" [size]="16" />
      } @else {
        <app-icon [name]="selected()?.icon ?? placeholderIcon()" [size]="16" />
      }
      <span class="trigger-value">{{ selected()?.label ?? placeholder() }}</span>
      <app-ax-icon name="chevronDown" [size]="16" />
    </button>

    @if (open()) {
      <div
        #list
        class="list"
        [style.inset-inline-start.px]="startPx()"
        [style.max-block-size.px]="maxPx()"
        [style.--list-top.px]="topPx()"
        [style.--list-bottom.px]="bottomPx()"
        role="menu"
        tabindex="-1"
        [id]="listId"
        [attr.aria-label]="label()"
        (keydown)="onListKey($event)"
      >
        @for (o of options(); track o.id; let n = $index) {
          <button
            type="button"
            role="menuitemradio"
            class="option"
            tabindex="-1"
            [attr.aria-checked]="o.id === value()"
            [attr.aria-describedby]="listId + '-d' + n"
            [attr.title]="o.description"
            (click)="choose(o)"
            (mouseenter)="focusAt(n)"
          >
            <span class="option-name">{{ o.label }}</span>
            <span class="sr-only" [id]="listId + '-d' + n">{{ o.description }}</span>
          </button>
        }
      </div>
    }
  `,
  styles: `
    :host {
      position: relative;
      display: inline-flex;
      max-inline-size: 100%;
      min-inline-size: 0;
    }

    /* the trigger: 32 high, 8px 12px, radius 16, icon / label / chevron 8 apart, label 14/20 medium */
    .trigger {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 8px;
      max-inline-size: 100%;
      min-inline-size: 0;
      block-size: 32px;
      padding: 8px 12px;
      border: 0;
      border-radius: 16px;
      background-color: transparent;
      background-image: linear-gradient(transparent, transparent);
      color: var(--ax-text);
      font: 500 14px/20px var(--font-chat);
      cursor: pointer;
      transition:
        background-image 125ms cubic-bezier(0.24, 1, 0.4, 1),
        background-color 125ms cubic-bezier(0.24, 1, 0.4, 1),
        color 125ms cubic-bezier(0.24, 1, 0.4, 1);
    }

    .trigger:hover,
    .trigger[aria-expanded='true'] {
      background-image: linear-gradient(var(--ax-overlay), var(--ax-overlay));
    }

    .trigger:focus-visible {
      outline: 2px solid var(--ax-focus);
      outline-offset: 3px;
    }

    .trigger-value {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    /* the popover: 200 wide, 4px padding, radius 12, 2px between items, the composer's surface and shadow */
    .list {
      position: fixed;
      inset-block-start: var(--list-top, auto);
      inset-block-end: var(--list-bottom, auto);
      z-index: var(--z-overlay);
      display: flex;
      flex-direction: column;
      gap: 2px;
      box-sizing: border-box;
      inline-size: 200px;
      padding: 4px;
      overflow-y: auto;
      overscroll-behavior: contain;
      border-radius: 12px;
      background: var(--ax-surface);
      color: var(--ax-text);
      box-shadow: var(--ax-shadow);
      font: 400 14px/20px var(--font-chat);
      animation: pop 125ms cubic-bezier(0.24, 1, 0.4, 1) both;
    }

    @keyframes pop {
      from {
        opacity: 0;
      }

      to {
        opacity: 1;
      }
    }

    /* an item: one line of text, 32 high, 6px 8px, radius 8, 14/20 regular (the description is its tooltip) */
    .option {
      display: flex;
      align-items: center;
      gap: 8px;
      box-sizing: border-box;
      inline-size: 100%;
      block-size: 32px;
      padding: 6px 8px;
      border: 0;
      border-radius: 8px;
      background-color: transparent;
      color: var(--ax-text);
      font: inherit;
      text-align: start;
      cursor: pointer;
      transition: background-color 95ms cubic-bezier(0.24, 1, 0.4, 1);
    }

    .option:hover,
    .option:focus-visible {
      background-color: var(--ax-overlay);
      outline: none;
    }

    .option:focus-visible {
      box-shadow: inset 0 0 0 2px var(--ax-focus);
    }

    .option-name {
      min-inline-size: 0;
    }

    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    @media (prefers-reduced-motion: reduce) {
      .list {
        animation: none;
      }

      .trigger,
      .option {
        transition: none;
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
  /** A fixed trigger icon (the mode trigger always shows the sparkle); otherwise the choice's own icon shows. */
  readonly fixedIcon = input<AxIconName | null>(null);
  readonly options = input.required<readonly PickerOption[]>();
  readonly value = input<string | null>(null);
  /** Where the menu opens: above the button for a control that sits at the bottom of the page. */
  readonly placement = input<'up' | 'down'>('down');

  readonly picked = output<string>();

  protected readonly listId = `research-picker-${++uid}`;
  protected readonly open = signal(false);
  protected readonly selected = computed(() => this.options().find((o) => o.id === this.value()) ?? null);

  protected readonly triggerLabel = computed(() => {
    const s = this.selected();
    return s === null ? this.label() : `${this.label()}: ${s.label}`;
  });

  /** Where the open menu sits, worked out from the button's place in the window when it opens. */
  protected readonly up = signal(false);
  protected readonly startPx = signal(0);
  protected readonly topPx = signal<number | null>(null);
  protected readonly bottomPx = signal<number | null>(null);
  protected readonly maxPx = signal(320);

  private readonly closeOnResize = (): void => this.close(false);

  private readonly closeOnScroll = (event: Event): void => {
    // only a scroller that carries the button moves it away from the (fixed) menu
    const target = event.target;
    if (target instanceof Node && target.contains(this.trigger().nativeElement)) {
      this.close(false);
    }
  };

  /** Opens 4px below the button, or above it when there is more room there; the height follows the room. */
  private place(): void {
    const r = this.trigger().nativeElement.getBoundingClientRect();
    const gap = 4;
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
    this.startPx.set(Math.max(edge, Math.min(start, window.innerWidth - 200 - edge)));
  }

  protected toggle(): void {
    if (this.open()) {
      this.close(false);
    } else {
      this.openList();
    }
  }

  /** The menu was just opened: its chosen option takes the focus as soon as it is drawn. */
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
    return Array.from(this.list()?.nativeElement.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? []);
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
