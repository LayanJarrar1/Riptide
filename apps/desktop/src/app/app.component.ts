import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  NgZone,
  OnInit,
  signal,
  Type,
  untracked,
  viewChild,
} from '@angular/core';
import { NgComponentOutlet } from '@angular/common';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { filter } from 'rxjs';

import { DiskSpaceService } from './core/disk-space.service';
import { FormatService } from './core/format.service';
import { TickerDirective } from './core/ticker.directive';
import { UpdateService } from './core/update.service';
import { DownloadsStore, LIST_VIEWS, ListView } from './data/downloads.store';
import { AddDownloadDialogComponent } from './features/downloads/add-download-dialog.component';
import { DropHandlerService } from './core/drop-handler.service';
import { TorrentLaunchService } from './core/torrent-launch.service';
import { TorrentDialogComponent } from './features/downloads/torrent-dialog.component';
import { CompletionToastService } from './features/downloads/completion-toast.service';
import { DesktopShellService } from './features/shell/desktop-shell.service';
import { SettingsService } from './features/settings/settings.service';
import { TitleBarComponent } from './features/shell/title-bar.component';
import { CATEGORY_ICONS } from './features/downloads/download-row.component';
import { PostActionDialogComponent } from './features/queues/post-action-dialog.component';
import { PostActionCountdown } from './features/queues/post-action-countdown.service';
import { QueuesNavComponent } from './features/queues/queues-nav.component';
import { ExitConfirmDialogComponent } from './features/shell/exit-confirm-dialog.component';
import { ShortcutsDialogComponent } from './features/shell/shortcuts-dialog.component';
import { UiState } from './features/shell/ui-state.service';
import { I18nService } from './i18n/i18n.service';
import { TranslatePipe } from './i18n/translate.pipe';
import { CATEGORY_KEYS, CategoryKey } from './model/download';
import { TauriService } from './tauri/tauri.service';
import { ThemeService } from './theme/theme.service';
import { ButtonComponent } from './ui/button.component';
import { IconComponent, IconName } from './ui/icon.component';
import { SparklineComponent } from './ui/sparkline.component';
import { ToastsComponent } from './ui/toasts.component';

const VIEW_ICONS: Readonly<Record<ListView, IconName>> = {
  all: 'layers',
  active: 'download',
  queued: 'clock',
  completed: 'check',
  failed: 'alert',
};

/**
 * How long the first-paint orchestration stays armed: shell parts and the first rows that
 * render inside this window cascade in; everything after enters the ordinary way.
 */
const BOOT_MS = 900;

/**
 * The application frame: top bar (brand, search, add), sidebar (views, categories, queues,
 * settings), the routed page, the status bar, and the overlays (add dialog, shortcuts,
 * toasts). Keyboard: Ctrl+N add, Ctrl+F search, Ctrl+, settings, ? or Ctrl+/ shortcuts.
 */
/** Lists at least this long debounce the search box (see `onSearch`). */
const SEARCH_DEBOUNCE_ROWS = 5000;
const SEARCH_DEBOUNCE_MS = 120;

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    AddDownloadDialogComponent,
    TorrentDialogComponent,
    NgComponentOutlet,
    ButtonComponent,
    IconComponent,
    RouterLink,
    RouterLinkActive,
    RouterOutlet,
    ShortcutsDialogComponent,
    ExitConfirmDialogComponent,
    PostActionDialogComponent,
    QueuesNavComponent,
    SparklineComponent,
    TickerDirective,
    TitleBarComponent,
    ToastsComponent,
    TranslatePipe,
  ],
  templateUrl: './app.component.html',
  styleUrl: './app.component.css',
  host: { '(document:keydown)': 'onKey($event)' },
})
export class AppComponent implements OnInit {
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  protected readonly i18n = inject(I18nService);
  protected readonly theme = inject(ThemeService);
  protected readonly tauri = inject(TauriService);
  protected readonly store = inject(DownloadsStore);
  protected readonly ui = inject(UiState);
  protected readonly settings = inject(SettingsService);
  protected readonly format = inject(FormatService);
  private readonly router = inject(Router);
  protected readonly updates = inject(UpdateService);
  private readonly searchRef = viewChild<ElementRef<HTMLInputElement>>('search');

  protected readonly views = LIST_VIEWS;
  protected readonly viewIcons = VIEW_ICONS;
  protected readonly categories = CATEGORY_KEYS;
  protected readonly categoryIcons = CATEGORY_ICONS;
  /** Real free space under Tauri (polled), the demo figure in a plain browser. */
  protected readonly disk = inject(DiskSpaceService);

  /** Stable formatters for the number tickers (they follow the language through the service). */
  protected readonly intFormat = (n: number): string => this.format.int(n);
  protected readonly speedFormat = (n: number): string => this.format.speed(n);

  private readonly url = signal(this.router.url);
  /** The batch dialog's class once its chunk has loaded. */
  protected readonly batchDialog = signal<Type<unknown> | null>(null);
  /** The site grabber dialog's class once its chunk has loaded. */
  protected readonly grabberDialog = signal<Type<unknown> | null>(null);

  /** Only categories that have at least one download are listed. */
  protected readonly visibleCategories = computed<readonly CategoryKey[]>(() => {
    const counts = this.store.categoryCounts();
    return this.categories.filter((c) => (counts.get(c) ?? 0) > 0);
  });

  /** Whether the list page is showing (sidebar highlights follow the view only then). */
  protected readonly onDownloads = computed(() => /^\/?(?:[?#].*)?$/.test(this.url()));

  protected readonly statusText = computed(() => {
    const t = this.store.totals();
    if (t.active === 0 && t.queued === 0) {
      return this.i18n.t('status.idle');
    }
    const parts = [this.i18n.t('status.active').replace('{n}', this.format.int(t.active))];
    if (t.queued > 0) {
      parts.push(this.i18n.t('status.queued').replace('{n}', this.format.int(t.queued)));
    }
    return parts.join(this.i18n.t('status.separator'));
  });

  constructor() {
    // starts watching the engine for completions (it shows the "is complete" toast)
    inject(CompletionToastService);
    inject(DesktopShellService);
    // listens for finished queues; the dialog shows its countdown
    inject(PostActionCountdown);
    // a magnet link or .torrent file the system handed to Riptide opens the file choice
    inject(TorrentLaunchService);
    inject(DropHandlerService);

    this.router.events
      .pipe(
        filter((e): e is NavigationEnd => e instanceof NavigationEnd),
        takeUntilDestroyed(),
      )
      .subscribe((e) => this.url.set(e.urlAfterRedirects));

    // the batch dialog is its own chunk: fetched the first time it is asked for
    effect(() => {
      if (this.ui.batchOpen() && untracked(this.batchDialog) === null) {
        void import('./features/downloads/batch-download-dialog.component').then((m) => this.batchDialog.set(m.BatchDownloadDialogComponent));
      }
    });

    // so is the site grabber
    effect(() => {
      if (this.ui.grabberOpen() && untracked(this.grabberDialog) === null) {
        void import('./features/grabber/site-grabber-dialog.component').then((m) => this.grabberDialog.set(m.SiteGrabberDialogComponent));
      }
    });

    // display density: the stylesheet turns it into the row height
    effect(() => {
      document.documentElement.dataset['density'] = this.settings.settings().density;
    });

    effect(() => {
      // every request bumps the counter; focus on each bump (skip the initial value)
      if (this.ui.searchFocusRequests() > 0) {
        this.searchRef()?.nativeElement.focus();
      }
    });

    // outside the zone: a signal write schedules its own refresh, and nothing (whenStable, the
    // first interaction) has to wait for the orchestration window to close
    const zone = inject(NgZone);
    const boot = zone.runOutsideAngular(() => setTimeout(() => this.ui.markBooted(), BOOT_MS));
    inject(DestroyRef).onDestroy(() => {
      clearTimeout(boot);
      clearTimeout(this.searchTimer);
    });
  }

  ngOnInit(): void {
    void this.tauri.loadAppInfo();
    // at most once a day and only while enabled in Settings (the one non-download call)
    this.updates.startChecks();
    void this.updates.announceUpdated();
  }

  /**
   * Typing filters at once in an ordinary list; past SEARCH_DEBOUNCE_ROWS the filter waits for a
   * short pause in typing, so a long history is filtered once per word rather than per key.
   */
  protected onSearch(event: Event): void {
    const value = (event.target as HTMLInputElement).value;
    clearTimeout(this.searchTimer);
    if (this.store.allRows().length < SEARCH_DEBOUNCE_ROWS || value.length === 0) {
      this.store.query.set(value);
      return;
    }
    this.searchTimer = setTimeout(() => this.store.query.set(value), SEARCH_DEBOUNCE_MS);
  }

  protected clearSearch(): void {
    clearTimeout(this.searchTimer);
    this.store.query.set('');
    this.searchRef()?.nativeElement.focus();
  }

  protected goView(view: ListView): void {
    this.store.setView(view);
    void this.router.navigateByUrl('/');
  }

  protected goQueue(queue: number): void {
    this.store.setQueue(queue);
    void this.router.navigateByUrl('/');
  }

  protected goCategory(category: CategoryKey): void {
    this.store.setView('all');
    this.store.setCategory(category);
    void this.router.navigateByUrl('/');
  }

  protected toggleLang(): void {
    this.i18n.setLang(this.i18n.lang() === 'ar' ? 'en' : 'ar');
  }

  protected onKey(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    const typing =
      target !== null &&
      (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT' || target.isContentEditable);
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) {
      // "?" on a Latin layout, "؟" on an Arabic one
      if (!typing && (event.key === '?' || event.key === '؟')) {
        event.preventDefault();
        this.ui.toggleShortcuts();
      }
      return;
    }
    if (event.key === 'n' || event.key === 'N') {
      event.preventDefault();
      if (event.shiftKey) {
        this.ui.openBatch();
      } else {
        this.ui.openAdd();
      }
    } else if (event.key === 'f' || event.key === 'F') {
      event.preventDefault();
      void this.router.navigateByUrl('/').then(() => this.ui.focusSearch());
    } else if (event.key === 'b' || event.key === 'B') {
      event.preventDefault();
      this.settings.patch({ sidebarVisible: !this.settings.settings().sidebarVisible });
    } else if (event.key === ',') {
      event.preventDefault();
      void this.router.navigateByUrl('/settings');
    } else if (event.key === '/') {
      event.preventDefault();
      this.ui.toggleShortcuts();
    }
  }
}
