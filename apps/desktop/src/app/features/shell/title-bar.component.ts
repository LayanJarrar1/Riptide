import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, signal } from '@angular/core';

import { WindowService } from '../../core/window.service';
import { I18nService } from '../../i18n/i18n.service';
import { TranslatePipe } from '../../i18n/translate.pipe';
import { TitleBarMenuComponent } from './title-bar-menu.component';
import type { MenuFocus, MenuId } from './title-bar-menus';

/** Top-level menus in bar order. */
const MENU_IDS: readonly MenuId[] = ['file', 'downloads', 'tools', 'view', 'window', 'help'];

/** A top-level item as the bar draws it. */
interface Top {
  readonly id: MenuId;
  readonly label: string;
  readonly mnemonic: string;
  /** The label split around the mnemonic letter, for the underline. */
  readonly before: string;
  readonly mn: string;
  readonly after: string;
}

/**
 * The integrated title bar: app mark, the menu bar (File, Downloads, Tools, View, Window, Help),
 * a drag region and the window controls.
 *
 * Window controls: the native frame is off (`decorations: false`), so minimize / maximize or
 * restore / close are ours. They stay at the physical right in both languages, where Windows puts
 * them whatever the app language is; the mark and the menus mirror and sit next to them in
 * Arabic. In a plain browser there is no window to control and they are not drawn.
 *
 * Menus follow the WAI-ARIA menubar pattern: one tab stop (roving tabindex), Left / Right move
 * along the bar (mirrored in RTL) and, while a menu is open, open the neighbour; Up / Down / Home /
 * End move inside a menu; Enter or Space choose; Escape closes and gives focus back to where it
 * was. F10, or Alt on its own, focuses the bar; Alt + the underlined letter opens a menu. A menu
 * opens on click, switches on hover while one is open, and closes on an outside click or when
 * focus leaves the bar. The drop-downs themselves are a lazy chunk (`TitleBarMenuComponent`).
 */
@Component({
  selector: 'app-title-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TitleBarMenuComponent, TranslatePipe],
  host: {
    '(document:keydown)': 'onDocumentKey($event)',
    '(document:keyup)': 'onDocumentKeyUp($event)',
    '(document:pointerdown)': 'onDocumentPointer($event)',
    '(window:blur)': 'close(false)',
  },
  template: `
    <header
      class="bar"
      data-tauri-drag-region
      [class.is-inactive]="!win.focused()"
      [class.is-maximized]="win.maximized()"
      [class.is-keyboard]="keyboard()"
      (focusout)="onFocusOut($event)"
    >
      <div class="start" data-tauri-drag-region [attr.dir]="i18n.dir()">
        <span class="mark" data-tauri-drag-region>
          <img src="brand-mark.svg" alt="" width="18" height="18" data-tauri-drag-region />
        </span>
        <div class="menubar" data-tauri-drag-region role="menubar" [attr.aria-label]="'menu.label' | t">
          @for (m of tops(); track m.id) {
            <div class="top" (pointerenter)="onTopHover(m.id)">
              <button
                type="button"
                role="menuitem"
                aria-haspopup="menu"
                class="top-button"
                [id]="'menu-button-' + m.id"
                [class.is-open]="openId() === m.id"
                [attr.aria-keyshortcuts]="'Alt+' + m.mnemonic"
                [attr.aria-expanded]="openId() === m.id"
                [attr.aria-controls]="openId() === m.id ? 'menu-' + m.id : null"
                [attr.tabindex]="tabStop() === m.id ? 0 : -1"
                (focus)="tabStop.set(m.id)"
                (click)="onTopClick(m.id)"
                (keydown)="onBarKey($event, m.id)"
              >
                @if (m.mn) {
                  {{ m.before }}<span class="mn">{{ m.mn }}</span
                  >{{ m.after }}
                } @else {
                  {{ m.label }}
                }
              </button>
              @if (openId() === m.id) {
                @defer (on immediate; prefetch on idle) {
                  <app-title-bar-menu [menuId]="m.id" [focus]="menuFocus()" (chosen)="close(true)" (keydown)="onMenuKey($event, m.id)" />
                }
              }
            </div>
          }
        </div>
      </div>

      <div class="drag" data-tauri-drag-region></div>

      @if (win.available) {
        <div class="controls">
          <button type="button" class="ctl" [attr.aria-label]="'window.minimize' | t" [attr.title]="'window.minimize' | t" (click)="win.minimize()">
            <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true"><path d="M0 5.5h10" stroke="currentColor" stroke-width="1" fill="none" /></svg>
          </button>
          <button
            type="button"
            class="ctl"
            [attr.aria-label]="(win.maximized() ? 'window.restore' : 'window.maximize') | t"
            [attr.title]="(win.maximized() ? 'window.restore' : 'window.maximize') | t"
            (click)="win.toggleMaximize()"
          >
            @if (win.maximized()) {
              <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
                <path d="M2.5 2.5v-2h7v7h-2M0.5 2.5h7v7h-7z" stroke="currentColor" stroke-width="1" fill="none" />
              </svg>
            } @else {
              <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true"><path d="M0.5 0.5h9v9h-9z" stroke="currentColor" stroke-width="1" fill="none" /></svg>
            }
          </button>
          <button type="button" class="ctl ctl-close" [attr.aria-label]="'window.close' | t" [attr.title]="'window.close' | t" (click)="win.close()">
            <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" stroke="currentColor" stroke-width="1" fill="none" /></svg>
          </button>
        </div>
      }
    </header>
  `,
  styleUrl: './title-bar.component.css',
})
export class TitleBarComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly win = inject(WindowService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  /** The top-level items. The Window menu needs a native window, so it is absent in a browser. */
  protected readonly tops = computed<readonly Top[]>(() =>
    MENU_IDS.filter((id) => id !== 'window' || this.win.available).map((id) => {
      const label = this.i18n.t(`menu.${id}`);
      const mnemonic = this.i18n.t(`menu.${id}.key`);
      // no underline in Arabic: a span inside a word cuts the joining of its letters
      const at = this.i18n.lang() === 'ar' ? -1 : label.toLowerCase().indexOf(mnemonic.toLowerCase());
      return at < 0
        ? { id, label, mnemonic, before: label, mn: '', after: '' }
        : { id, label, mnemonic, before: label.slice(0, at), mn: label.slice(at, at + mnemonic.length), after: label.slice(at + mnemonic.length) };
    }),
  );

  /** The open menu, if any. */
  protected readonly openId = signal<MenuId | null>(null);
  /** Where focus goes inside the menu that is about to appear. */
  protected readonly menuFocus = signal<MenuFocus>('menu');
  /** The top-level item that is the bar's tab stop (roving tabindex). */
  protected readonly tabStop = signal<MenuId>('file');
  /** The bar was reached by keyboard: mnemonics are underlined. */
  protected readonly keyboard = signal(false);

  /** Where focus was when the menus took it; Escape and choosing give it back. */
  private returnTo: HTMLElement | null = null;
  /** Alt went down and nothing else has been pressed since: releasing it focuses the bar. */
  private altAlone = false;

  // ---- opening and closing --------------------------------------------------------------
  protected onTopClick(id: MenuId): void {
    if (this.openId() === id) {
      this.close(true);
    } else {
      this.open(id, 'menu');
    }
  }

  /** While a menu is open, hovering another top item switches to it. */
  protected onTopHover(id: MenuId): void {
    if (this.openId() !== null && this.openId() !== id) {
      this.open(id, 'menu');
    }
  }

  private open(id: MenuId, focus: MenuFocus): void {
    if (this.openId() === null && !this.host.nativeElement.contains(document.activeElement)) {
      this.returnTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    this.tabStop.set(id);
    this.menuFocus.set(focus);
    this.openId.set(id);
  }

  /** Closes the open menu; `restore` gives focus back to where it came from. */
  protected close(restore: boolean): void {
    if (this.openId() === null) {
      return;
    }
    this.openId.set(null);
    if (restore) {
      this.restoreFocus();
    }
  }

  private restoreFocus(): void {
    const target = this.returnTo;
    this.returnTo = null;
    if (target !== null && target.isConnected) {
      target.focus({ preventScroll: true });
    } else {
      (document.activeElement as HTMLElement | null)?.blur();
    }
    this.keyboard.set(false);
  }

  // ---- document-level triggers ----------------------------------------------------------
  protected onDocumentPointer(event: PointerEvent): void {
    if (this.openId() !== null && !this.host.nativeElement.contains(event.target as Node)) {
      this.close(false);
    }
  }

  /** Focus left the bar (Tab away, another window): the menu goes. */
  protected onFocusOut(event: FocusEvent): void {
    const next = event.relatedTarget as Node | null;
    if (this.openId() !== null && next !== null && !this.host.nativeElement.contains(next)) {
      this.close(false);
    }
    if (next === null || !this.host.nativeElement.contains(next)) {
      this.keyboard.set(false);
    }
  }

  protected onDocumentKey(event: KeyboardEvent): void {
    if (event.key === 'Alt') {
      this.altAlone = !event.repeat;
      return;
    }
    this.altAlone = false;
    if (event.key === 'F10' && !event.shiftKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      this.focusBar();
      return;
    }
    // Alt + the underlined letter opens that menu
    if (event.altKey && !event.ctrlKey && !event.metaKey && event.key.length === 1) {
      const key = event.key.toLowerCase();
      const target = this.tops().find((m) => m.mnemonic.toLowerCase() === key);
      if (target !== undefined) {
        event.preventDefault();
        this.keyboard.set(true);
        this.open(target.id, 'first');
      }
    }
  }

  protected onDocumentKeyUp(event: KeyboardEvent): void {
    if (event.key === 'Alt' && this.altAlone) {
      this.altAlone = false;
      event.preventDefault();
      if (this.host.nativeElement.contains(document.activeElement)) {
        this.close(true);
      } else {
        this.focusBar();
      }
    }
  }

  /** Moves focus to the bar's tab stop (F10, Alt). */
  private focusBar(): void {
    if (!this.host.nativeElement.contains(document.activeElement)) {
      this.returnTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    this.keyboard.set(true);
    this.host.nativeElement.querySelector<HTMLElement>(`#menu-button-${this.tabStop()}`)?.focus();
  }

  // ---- keyboard: the bar ---------------------------------------------------------------
  /** Keys on a top-level item (events from inside a menu come through `onMenuKey`). */
  protected onBarKey(event: KeyboardEvent, id: MenuId): void {
    const rtl = this.i18n.dir() === 'rtl';
    const forward = rtl ? 'ArrowLeft' : 'ArrowRight';
    const backward = rtl ? 'ArrowRight' : 'ArrowLeft';
    switch (event.key) {
      case forward:
        event.preventDefault();
        this.moveTop(id, 1);
        break;
      case backward:
        event.preventDefault();
        this.moveTop(id, -1);
        break;
      case 'Home':
        event.preventDefault();
        this.goTop(this.tops()[0].id);
        break;
      case 'End':
        event.preventDefault();
        this.goTop(this.tops()[this.tops().length - 1].id);
        break;
      case 'ArrowDown':
      case 'Enter':
      case ' ':
        event.preventDefault();
        this.keyboard.set(true);
        this.open(id, 'first');
        break;
      case 'ArrowUp':
        event.preventDefault();
        this.keyboard.set(true);
        this.open(id, 'last');
        break;
      case 'Escape':
        event.preventDefault();
        this.close(true);
        break;
    }
  }

  // ---- keyboard: inside a menu ---------------------------------------------------------
  protected onMenuKey(event: KeyboardEvent, id: MenuId): void {
    const items = this.rows();
    const at = items.indexOf(document.activeElement as HTMLElement);
    const focusAt = (i: number): void => items[(i + items.length) % items.length]?.focus();
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        focusAt(at + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        focusAt(at < 0 ? -1 : at - 1);
        break;
      case 'Home':
        event.preventDefault();
        focusAt(0);
        break;
      case 'End':
        event.preventDefault();
        focusAt(-1);
        break;
      case 'ArrowRight':
      case 'ArrowLeft':
        event.preventDefault();
        this.moveTop(id, (event.key === 'ArrowRight') !== (this.i18n.dir() === 'rtl') ? 1 : -1);
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
    // Enter and Space are the button's own click, handled by the menu
  }

  /** The focusable rows of the open menu, in order (disabled ones are skipped). */
  private rows(): HTMLElement[] {
    return Array.from(this.host.nativeElement.querySelectorAll<HTMLElement>('[role="menu"] .item:not([aria-disabled="true"])'));
  }

  private moveTop(from: MenuId, delta: 1 | -1): void {
    const ids = this.tops().map((m) => m.id);
    this.goTop(ids[(ids.indexOf(from) + delta + ids.length) % ids.length]);
  }

  /** To another top-level item; when a menu is open, that one opens (focus on its first row). */
  private goTop(id: MenuId): void {
    if (this.openId() !== null) {
      this.open(id, 'first');
    } else {
      this.tabStop.set(id);
      this.host.nativeElement.querySelector<HTMLElement>(`#menu-button-${id}`)?.focus();
    }
  }
}
