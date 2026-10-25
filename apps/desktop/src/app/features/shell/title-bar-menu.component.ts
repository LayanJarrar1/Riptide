import { afterNextRender, ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, output } from '@angular/core';

import { NavRailService } from '../../core/nav-rail.service';
import { WindowService } from '../../core/window.service';
import { I18nService } from '../../i18n/i18n.service';
import { ThemeService } from '../../theme/theme.service';
import { SettingsService } from '../settings/settings.service';
import { AppCommands } from './app-commands.service';
import { ActionEntry, buildMenus, isEnabled, MenuFocus, MenuId } from './title-bar-menus';

/**
 * The drop-down of one title-bar menu. A separate chunk (fetched when idle, long before the first
 * click) so the menu definitions, the command wiring and the popup styles stay out of the initial
 * bundle. The title bar owns open / close and the arrow keys; this component draws the rows,
 * keeps their enabled / checked state live and runs the chosen command.
 */
@Component({
  selector: 'app-title-bar-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'menu',
    tabindex: '-1',
    '[id]': '"menu-" + menuId()',
    '[attr.aria-labelledby]': '"menu-button-" + menuId()',
  },
  template: `
    @for (e of entries(); track $index) {
      @switch (e.kind) {
        @case ('separator') {
          <div class="rule" role="separator"></div>
        }
        @case ('heading') {
          <div class="heading" role="presentation">{{ e.label }}</div>
        }
        @default {
          <button
            type="button"
            class="item"
            tabindex="-1"
            [attr.role]="e.kind === 'radio' ? 'menuitemradio' : e.kind === 'check' ? 'menuitemcheckbox' : 'menuitem'"
            [attr.aria-checked]="e.kind === 'item' ? null : e.checked"
            [attr.aria-label]="e.kind === 'radio' ? e.group + ': ' + e.label : null"
            [attr.aria-disabled]="e.kind === 'item' && e.disabled ? true : null"
            [class.is-disabled]="e.kind === 'item' && e.disabled"
            (pointermove)="focusOnHover($event)"
            (click)="choose(e)"
          >
            <span class="slot" aria-hidden="true">
              @if (e.kind === 'radio' && e.checked) {
                <span class="dot"></span>
              } @else if (e.kind === 'check' && e.checked) {
                <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 8.5l3 3 6-7" /></svg>
              }
            </span>
            <span class="label">{{ e.label }}</span>
            @if (e.kind !== 'radio' && e.shortcut) {
              <kbd class="key" dir="ltr">{{ e.shortcut }}</kbd>
            }
          </button>
        }
      }
    }
  `,
  styleUrl: './title-bar-menu.component.css',
})
export class TitleBarMenuComponent {
  private readonly i18n = inject(I18nService);
  private readonly win = inject(WindowService);
  private readonly cmd = inject(AppCommands);
  private readonly theme = inject(ThemeService);
  private readonly settings = inject(SettingsService);
  private readonly navRail = inject(NavRailService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  /** Which menu this is. */
  readonly menuId = input.required<MenuId>();
  /** Where focus goes when the menu appears. */
  readonly focus = input<MenuFocus>('menu');
  /** A row was chosen; the bar closes the menu and returns focus before the command runs. */
  readonly chosen = output<void>();

  protected readonly entries = computed(() => {
    const menus = buildMenus(
      {
        t: (key) => this.i18n.t(key),
        tauri: this.win.available,
        lang: this.i18n.lang(),
        theme: this.theme.theme(),
        density: this.settings.settings().density,
        sidebarVisible: this.navRail.expanded(),
        maximized: this.win.maximized(),
        canResume: this.cmd.canResume(),
        canPause: this.cmd.canPause(),
        canCancel: this.cmd.canCancel(),
        canRemove: this.cmd.canRemove(),
        openable: this.cmd.openable() !== null,
        anyRunning: this.cmd.anyRunning(),
        anyResumable: this.cmd.anyResumable(),
      },
      this.cmd,
    );
    return menus.find((m) => m.id === this.menuId())?.entries ?? [];
  });

  constructor() {
    afterNextRender(() => {
      const root = this.host.nativeElement;
      const rows = root.querySelectorAll<HTMLElement>('.item:not([aria-disabled="true"])');
      const row = this.focus() === 'first' ? rows[0] : this.focus() === 'last' ? rows[rows.length - 1] : undefined;
      (row ?? root).focus({ preventScroll: true });
    });
  }

  protected choose(entry: ActionEntry): void {
    if (!isEnabled(entry)) {
      return;
    }
    this.chosen.emit();
    entry.run();
  }

  protected focusOnHover(event: Event): void {
    const el = event.currentTarget as HTMLElement;
    if (document.activeElement !== el && el.getAttribute('aria-disabled') !== 'true') {
      el.focus({ preventScroll: true });
    }
  }
}
