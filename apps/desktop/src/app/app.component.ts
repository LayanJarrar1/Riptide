import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { filter } from 'rxjs';

import { DiskSpaceService } from './core/disk-space.service';
import { FormatService } from './core/format.service';
import { DownloadsStore, LIST_VIEWS, ListView } from './data/downloads.store';
import { AddDownloadDialogComponent } from './features/downloads/add-download-dialog.component';
import { CATEGORY_ICONS } from './features/downloads/download-row.component';
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
 * The application frame: top bar (brand, search, add), sidebar (views, categories, queues,
 * settings), the routed page, the status bar, and the overlays (add dialog, toasts).
 * Keyboard: Ctrl+N add, Ctrl+F search, Ctrl+, settings.
 */
@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    AddDownloadDialogComponent,
    ButtonComponent,
    IconComponent,
    RouterLink,
    RouterLinkActive,
    RouterOutlet,
    SparklineComponent,
    ToastsComponent,
    TranslatePipe,
  ],
  templateUrl: './app.component.html',
  styleUrl: './app.component.css',
  host: { '(document:keydown)': 'onKey($event)' },
})
export class AppComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly theme = inject(ThemeService);
  protected readonly tauri = inject(TauriService);
  protected readonly store = inject(DownloadsStore);
  protected readonly ui = inject(UiState);
  protected readonly format = inject(FormatService);
  private readonly router = inject(Router);
  private readonly searchRef = viewChild<ElementRef<HTMLInputElement>>('search');

  protected readonly views = LIST_VIEWS;
  protected readonly viewIcons = VIEW_ICONS;
  protected readonly categories = CATEGORY_KEYS;
  protected readonly categoryIcons = CATEGORY_ICONS;
  /** Real free space under Tauri (polled), the demo figure in a plain browser. */
  protected readonly disk = inject(DiskSpaceService);

  private readonly url = signal(this.router.url);

  /** Only categories that have at least one download are listed. */
  protected readonly visibleCategories = computed<readonly CategoryKey[]>(() => {
    const counts = this.store.categoryCounts();
    return this.categories.filter((c) => (counts.get(c) ?? 0) > 0);
  });

  /** Whether the list page is showing (sidebar highlights follow the view only then). */
  protected readonly onDownloads = computed(() => !this.url().startsWith('/settings'));

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
    this.router.events
      .pipe(
        filter((e): e is NavigationEnd => e instanceof NavigationEnd),
        takeUntilDestroyed(),
      )
      .subscribe((e) => this.url.set(e.urlAfterRedirects));

    effect(() => {
      // every request bumps the counter; focus on each bump (skip the initial value)
      if (this.ui.searchFocusRequests() > 0) {
        this.searchRef()?.nativeElement.focus();
      }
    });
  }

  ngOnInit(): void {
    void this.tauri.loadAppInfo();
  }

  protected onSearch(event: Event): void {
    this.store.query.set((event.target as HTMLInputElement).value);
  }

  protected clearSearch(): void {
    this.store.query.set('');
    this.searchRef()?.nativeElement.focus();
  }

  protected goView(view: ListView): void {
    this.store.setView(view);
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
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) {
      return;
    }
    if (event.key === 'n' || event.key === 'N') {
      event.preventDefault();
      this.ui.openAdd();
    } else if (event.key === 'f' || event.key === 'F') {
      event.preventDefault();
      void this.router.navigateByUrl('/').then(() => this.ui.focusSearch());
    } else if (event.key === ',') {
      event.preventDefault();
      void this.router.navigateByUrl('/settings');
    }
  }
}
