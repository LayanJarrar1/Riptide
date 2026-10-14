import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';

import { FormatService } from '../../core/format.service';
import { finalPath } from '../../data/bridge-mapping';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadRow, DownloadsStore, SortKey } from '../../data/downloads.store';
import { I18nService } from '../../i18n/i18n.service';
import { canPause, canResume, Download } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { EmptyStateComponent } from '../../ui/empty-state.component';
import { IconComponent } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { ToastService } from '../../ui/toast.service';
import { UiState } from '../shell/ui-state.service';
import { DownloadDetailsComponent } from './download-details.component';
import { DownloadRowComponent, RowAction } from './download-row.component';

/** Table columns in order; `grow` ones share the leftover width. */
interface Column {
  readonly key: SortKey;
  readonly labelKey: string;
  readonly align: 'start' | 'end';
}

const COLUMNS: readonly Column[] = [
  { key: 'name', labelKey: 'column.name', align: 'start' },
  { key: 'size', labelKey: 'column.size', align: 'end' },
  { key: 'progress', labelKey: 'column.progress', align: 'start' },
  { key: 'speed', labelKey: 'column.speed', align: 'end' },
  { key: 'eta', labelKey: 'column.eta', align: 'end' },
  { key: 'status', labelKey: 'column.status', align: 'start' },
  { key: 'added', labelKey: 'column.added', align: 'start' },
];

const ROW_HEIGHT = 46;
/** Rows rendered above and below the viewport so fast scrolling never shows a gap. */
const OVERSCAN = 6;

/**
 * The main screen: toolbar, sortable header, a virtual-scrolled grid of rows, the details
 * drawer, the row context menu and the confirmation for removal. Keyboard: ↑/↓ move the
 * selection, Enter opens details, Space pauses/resumes, Delete removes, Escape clears.
 */
@Component({
  selector: 'app-downloads-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    ButtonComponent,
    DialogComponent,
    DownloadDetailsComponent,
    DownloadRowComponent,
    EmptyStateComponent,
    IconComponent,
    MenuComponent,
  ],
  host: {
    '[class.has-details]': 'store.details() !== null',
    '(keydown)': 'onKey($event)',
  },
  template: `
    <section class="list" [attr.aria-label]="i18n.t('view.' + store.view())">
      <div class="toolbar">
        <div class="toolbar-title">
          <h1 class="title">{{ title() }}</h1>
          <span class="count num">{{ format.int(store.rows().length) }}</span>
        </div>

        @if (store.selectedRows().length > 1) {
          <div class="bulk">
            <span class="bulk-count">{{ i18n.t('toolbar.selected').replace('{n}', format.int(store.selectedRows().length)) }}</span>
            <button appButton size="sm" variant="secondary" icon="pause" type="button" [disabled]="!bulk().pause" (click)="bulkAction('pause')">{{ i18n.t('action.pause') }}</button>
            <button appButton size="sm" variant="secondary" icon="play" type="button" [disabled]="!bulk().resume" (click)="bulkAction('resume')">{{ i18n.t('action.resume') }}</button>
            <button appButton size="sm" variant="ghost" icon="trash" type="button" (click)="askRemove(store.selectedRows())">{{ i18n.t('action.remove') }}</button>
            <button appButton size="sm" variant="ghost" icon="close" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('toolbar.clearSelection')" (click)="store.clearSelection()"></button>
          </div>
        } @else {
          <div class="bulk">
            <button appButton size="sm" variant="ghost" icon="pause" type="button" [disabled]="store.totals().active === 0 && store.totals().queued === 0" (click)="pauseAll()">{{ i18n.t('toolbar.pauseAll') }}</button>
            <button appButton size="sm" variant="ghost" icon="play" type="button" [disabled]="store.viewCounts().queued === 0 && store.viewCounts().failed === 0" (click)="resumeAll()">{{ i18n.t('toolbar.resumeAll') }}</button>
          </div>
        }
      </div>

      <div class="grid" role="grid" [attr.aria-rowcount]="store.rows().length" [attr.aria-multiselectable]="true">
        <div class="header" role="row">
          @for (c of columns; track c.key) {
            <button
              type="button"
              role="columnheader"
              [class]="'th th-' + c.key"
              [class.is-end]="c.align === 'end'"
              [class.is-sorted]="store.sortKey() === c.key"
              [attr.aria-sort]="store.sortKey() === c.key ? (store.sortDir() === 'asc' ? 'ascending' : 'descending') : 'none'"
              (click)="store.sortBy(c.key)"
            >
              <span>{{ i18n.t(c.labelKey) }}</span>
              @if (store.sortKey() === c.key) {
                <app-icon [name]="store.sortDir() === 'asc' ? 'arrowUp' : 'arrowDown'" [size]="12" />
              }
            </button>
          }
          <span class="th th-actions" role="columnheader"></span>
        </div>

        @if (store.rows().length === 0) {
          <app-empty-state [icon]="emptyIcon()" [heading]="i18n.t('empty.' + emptyKind() + '.heading')" [body]="i18n.t('empty.' + emptyKind() + '.body')">
            @if (emptyKind() === 'all') {
              <button appButton variant="primary" icon="plus" type="button" (click)="ui.openAdd()">{{ i18n.t('action.add') }}</button>
            } @else if (emptyKind() === 'search') {
              <button appButton variant="secondary" type="button" (click)="store.query.set('')">{{ i18n.t('empty.search.clear') }}</button>
            }
          </app-empty-state>
        } @else {
          <div #viewport class="viewport" (scroll)="onScroll()" tabindex="0" [attr.aria-label]="i18n.t('view.' + store.view())">
            <div class="spacer" [style.block-size.px]="window().top"></div>
            @for (row of window().rows; track row.d.id) {
              <app-download-row
                [row]="row"
                [selected]="store.selected().has(row.d.id)"
                [open]="store.detailsId() === row.d.id"
                (rowClick)="onRowClick(row, $event)"
                (rowDblClick)="store.openDetails(row.d.id)"
                (rowContext)="onContext(row, $event)"
                (action)="act(row, $event)"
              />
            }
            <div class="spacer" [style.block-size.px]="window().bottom"></div>
          </div>
        }
      </div>
    </section>

    @if (store.details(); as details) {
      <app-download-details class="drawer" [row]="details" (closed)="store.closeDetails()" (action)="act(details, $event)" />
    }

    <app-menu [open]="menu().open" [items]="menuItems()" [x]="menu().x" [y]="menu().y" (selected)="onMenu($event)" (closed)="closeMenu()" />

    <app-dialog [open]="removal() !== null" [title]="i18n.t('remove.title')" (closed)="removal.set(null)">
      @if (removal(); as rows) {
        <p class="remove-text">
          {{ rows.length === 1 ? i18n.t('remove.one').replace('{name}', rows[0].d.filename) : i18n.t('remove.many').replace('{n}', format.int(rows.length)) }}
        </p>
        <label class="remove-check">
          <input type="checkbox" [checked]="deleteFile()" (change)="deleteFile.set(!deleteFile())" />
          <span>{{ i18n.t('remove.deleteFile') }}</span>
        </label>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="removal.set(null)">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="danger" icon="trash" type="button" (click)="confirmRemove()">{{ i18n.t('action.remove') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: minmax(0, 1fr) 0;
      block-size: 100%;
      min-block-size: 0;
      transition: grid-template-columns var(--motion-slow) var(--ease-emphasized);
    }

    :host(.has-details) {
      grid-template-columns: minmax(0, 1fr) var(--drawer-width);
    }

    .list {
      display: grid;
      grid-template-rows: auto minmax(0, 1fr);
      min-inline-size: 0;
      min-block-size: 0;
    }

    .toolbar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-4);
      block-size: 56px;
      padding-inline: var(--space-5);
    }

    .toolbar-title {
      display: flex;
      align-items: baseline;
      gap: var(--space-2);
    }

    .title {
      font-size: var(--text-lg);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
    }

    .count {
      color: var(--color-text-muted);
      font-size: var(--text-sm);
    }

    .bulk {
      display: flex;
      align-items: center;
      gap: var(--space-1);
    }

    .bulk-count {
      margin-inline-end: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .grid {
      display: grid;
      grid-template-rows: auto minmax(0, 1fr);
      min-block-size: 0;
      /* rows and header share these; the row component reads them through inheritance and
         switches to the compact set in the same container query (see download-row) */
      --grid-columns: minmax(220px, 2.2fr) 150px minmax(160px, 1.6fr) 100px 90px 172px 104px 64px;
      --grid-columns-compact: minmax(180px, 2fr) 128px minmax(120px, 1.4fr) 92px 150px 60px;
      container-type: inline-size;
      container-name: grid;
    }

    .header {
      display: grid;
      grid-template-columns: var(--grid-columns);
      align-items: center;
      block-size: var(--header-height);
      padding-inline: var(--space-4);
      border-block: 1px solid var(--color-border);
      background: var(--color-surface-1);
    }

    .th {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      block-size: 100%;
      padding-inline: var(--space-2);
      border: 0;
      background: transparent;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      text-align: start;
      cursor: pointer;
      white-space: nowrap;
    }

    .th:first-child {
      padding-inline-start: 0;
    }

    .th:hover {
      color: var(--color-text);
    }

    .th.is-end {
      justify-content: flex-end;
    }

    .th.is-sorted {
      color: var(--color-text);
    }

    .th-actions {
      cursor: default;
    }

    /* Narrow list (details drawer open, small window): drop the two least useful columns. */
    @container grid (max-width: 1040px) {
      .header {
        grid-template-columns: var(--grid-columns-compact);
      }

      .th-eta,
      .th-added {
        display: none;
      }
    }

    .viewport {
      overflow-y: auto;
      overflow-x: hidden;
      outline-offset: -2px;
      scrollbar-gutter: stable;
    }

    .drawer {
      min-inline-size: 0;
      overflow: hidden;
      opacity: 0;
      transition: opacity var(--motion-base) var(--ease-standard);
    }

    :host(.has-details) .drawer {
      opacity: 1;
    }

    .remove-text {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
      overflow-wrap: anywhere;
    }

    .remove-check {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      margin-block: var(--space-2) var(--space-3);
      font-size: var(--text-sm);
      cursor: pointer;
    }

    .remove-check input {
      accent-color: var(--color-accent);
    }
  `,
})
export class DownloadsPageComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly store = inject(DownloadsStore);
  protected readonly ui = inject(UiState);
  private readonly engine = inject(DownloadEngine);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly toasts = inject(ToastService);
  private readonly destroyRef = inject(DestroyRef);
  private readonly viewportRef = viewChild<ElementRef<HTMLElement>>('viewport');

  protected readonly columns = COLUMNS;

  private readonly scrollTop = signal(0);
  private readonly viewportHeight = signal(600);

  protected readonly menu = signal<{ open: boolean; x: number; y: number; row: DownloadRow | null }>({ open: false, x: 0, y: 0, row: null });
  protected readonly removal = signal<readonly DownloadRow[] | null>(null);
  protected readonly deleteFile = signal(false);

  protected readonly title = computed(() => {
    const category = this.store.category();
    return category === null ? this.i18n.t(`view.${this.store.view()}`) : this.i18n.t(`category.${category}`);
  });

  /** The rows inside the scroll window plus spacers that stand in for the rest. */
  protected readonly window = computed(() => {
    const rows = this.store.rows();
    const top = this.scrollTop();
    const height = this.viewportHeight();
    const start = Math.max(0, Math.floor(top / ROW_HEIGHT) - OVERSCAN);
    const end = Math.min(rows.length, Math.ceil((top + height) / ROW_HEIGHT) + OVERSCAN);
    return {
      rows: rows.slice(start, end),
      top: start * ROW_HEIGHT,
      bottom: Math.max(0, (rows.length - end) * ROW_HEIGHT),
    };
  });

  protected readonly emptyKind = computed<'all' | 'search' | 'view'>(() => {
    if (this.store.query().trim().length > 0) {
      return 'search';
    }
    return this.store.viewCounts().all === 0 ? 'all' : 'view';
  });

  protected readonly emptyIcon = computed(() => (this.emptyKind() === 'search' ? 'search' : this.emptyKind() === 'all' ? 'download' : 'inbox'));

  protected readonly bulk = computed(() => {
    const rows = this.store.selectedRows();
    return {
      pause: rows.some((r) => canPause(r.d.state)),
      resume: rows.some((r) => canResume(r.d.state)),
    };
  });

  protected readonly menuItems = computed<readonly MenuItem[]>(() => {
    const row = this.menu().row;
    if (row === null) {
      return [];
    }
    const s = row.d.state;
    const t = (k: string): string => this.i18n.t(k);
    const items: MenuItem[] = [];
    if (canPause(s)) {
      items.push({ id: 'pause', label: t('action.pause'), icon: 'pause', shortcut: 'Space' });
    } else if (s === 'failed' && row.d.failure === 'expired') {
      items.push({ id: 'refreshLink', label: t('action.refreshLink'), icon: 'link' });
      items.push({ id: 'retry', label: t('action.retry'), icon: 'retry', shortcut: 'Space' });
    } else if (canResume(s)) {
      items.push({ id: s === 'failed' ? 'retry' : 'resume', label: t(s === 'failed' ? 'action.retry' : 'action.resume'), icon: s === 'failed' ? 'retry' : 'play', shortcut: 'Space' });
    }
    if (s === 'completed') {
      items.push({ id: 'openFile', label: t('action.openFile'), icon: 'external', shortcut: 'Enter' });
    }
    items.push({ id: 'openFolder', label: t('action.openFolder'), icon: 'folderOpen' });
    items.push({ id: 'details', label: t('action.details'), icon: 'info', shortcut: 'Enter', separator: true });
    items.push({ id: 'copyLink', label: t('action.copyLink'), icon: 'copy' });
    if (s !== 'completed' && s !== 'cancelled') {
      items.push({ id: 'cancel', label: t('action.cancel'), icon: 'stop', separator: true });
    }
    items.push({ id: 'remove', label: t('action.remove'), icon: 'trash', danger: true, shortcut: 'Del', separator: s === 'completed' || s === 'cancelled' });
    return items;
  });

  constructor() {
    afterNextRender(() => this.observeViewport());
  }

  private observeViewport(): void {
    const el = this.viewportRef()?.nativeElement;
    if (!el || typeof ResizeObserver === 'undefined') {
      return;
    }
    const ro = new ResizeObserver(() => this.viewportHeight.set(el.clientHeight));
    ro.observe(el);
    this.viewportHeight.set(el.clientHeight);
    this.destroyRef.onDestroy(() => ro.disconnect());
  }

  protected onScroll(): void {
    const el = this.viewportRef()?.nativeElement;
    if (el) {
      this.scrollTop.set(el.scrollTop);
      if (el.clientHeight !== this.viewportHeight()) {
        this.viewportHeight.set(el.clientHeight);
      }
    }
  }

  protected onRowClick(row: DownloadRow, event: MouseEvent): void {
    if (event.shiftKey) {
      this.store.selectRange(row.d.id);
    } else if (event.ctrlKey || event.metaKey) {
      this.store.toggle(row.d.id);
    } else {
      this.store.select(row.d.id);
      if (this.store.detailsId() !== null) {
        this.store.detailsId.set(row.d.id);
      }
    }
  }

  protected onContext(row: DownloadRow, event: MouseEvent): void {
    if (!this.store.selected().has(row.d.id)) {
      this.store.select(row.d.id);
    }
    this.menu.set({ open: true, x: event.clientX, y: event.clientY, row });
  }

  protected closeMenu(): void {
    this.menu.update((m) => ({ ...m, open: false }));
  }

  protected onMenu(id: string): void {
    const row = this.menu().row;
    if (row !== null) {
      this.act(row, id as RowAction);
    }
  }

  protected act(row: DownloadRow, action: RowAction): void {
    const d = row.d;
    switch (action) {
      case 'pause':
        this.run(this.engine.pause(d.id));
        break;
      case 'resume':
        this.run(this.engine.resume(d.id));
        break;
      case 'retry':
        this.run(this.engine.retry(d.id));
        this.toasts.show('info', this.i18n.t('toast.retrying').replace('{name}', d.filename));
        break;
      case 'cancel':
        this.run(this.engine.cancel(d.id));
        break;
      case 'remove':
        this.askRemove([row]);
        break;
      case 'details':
        this.store.openDetails(d.id);
        break;
      case 'copyLink':
        void this.copy(d.url);
        break;
      case 'openFile':
        this.openPath('open_file', d);
        break;
      case 'openFolder':
        this.openPath('show_in_folder', d);
        break;
      case 'refreshLink':
        this.ui.openAdd(d.url);
        break;
    }
  }

  protected bulkAction(action: 'pause' | 'resume'): void {
    for (const row of this.store.selectedRows()) {
      if (action === 'pause' && canPause(row.d.state)) {
        this.run(this.engine.pause(row.d.id));
      } else if (action === 'resume' && canResume(row.d.state)) {
        this.run(this.engine.resume(row.d.id));
      }
    }
  }

  protected pauseAll(): void {
    this.run(this.engine.pauseAll());
    this.toasts.show('info', this.i18n.t('toast.pausedAll'));
  }

  protected resumeAll(): void {
    this.run(this.engine.resumeAll());
    this.toasts.show('info', this.i18n.t('toast.resumedAll'));
  }

  protected askRemove(rows: readonly DownloadRow[]): void {
    this.deleteFile.set(false);
    this.removal.set(rows);
  }

  protected confirmRemove(): void {
    const rows = this.removal();
    if (rows === null) {
      return;
    }
    const removed = rows.map((r) => r.d);
    const deleteFile = this.deleteFile();
    for (const d of removed) {
      this.run(this.engine.remove(d.id, deleteFile));
    }
    this.store.clearSelection();
    if (removed.some((d) => d.id === this.store.detailsId())) {
      this.store.closeDetails();
    }
    this.removal.set(null);
    const text = removed.length === 1 ? this.i18n.t('toast.removed').replace('{name}', removed[0].filename) : this.i18n.t('toast.removedMany').replace('{n}', this.format.int(removed.length));
    this.toasts.show('info', text);
  }

  protected onKey(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT' || target.isContentEditable)) {
      return;
    }
    const selected = this.store.selectedRows();
    const first = selected[0];
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        event.preventDefault();
        const id = this.store.moveSelection(event.key === 'ArrowDown' ? 1 : -1);
        if (id !== null) {
          this.scrollIntoView(id);
        }
        break;
      }
      case 'Enter':
        if (first) {
          event.preventDefault();
          this.store.openDetails(first.d.id);
        }
        break;
      case ' ':
        if (first) {
          event.preventDefault();
          if (canPause(first.d.state)) {
            this.run(this.engine.pause(first.d.id));
          } else if (canResume(first.d.state)) {
            this.run(this.engine.resume(first.d.id));
          }
        }
        break;
      case 'Delete':
      case 'Backspace':
        if (selected.length > 0) {
          event.preventDefault();
          this.askRemove(selected);
        }
        break;
      case 'Escape':
        if (this.menu().open) {
          this.closeMenu();
        } else if (this.store.detailsId() !== null) {
          this.store.closeDetails();
        } else {
          this.store.clearSelection();
        }
        break;
    }
  }

  private scrollIntoView(id: number): void {
    const el = this.viewportRef()?.nativeElement;
    if (!el) {
      return;
    }
    const index = this.store.rows().findIndex((r) => r.d.id === id);
    if (index < 0) {
      return;
    }
    const top = index * ROW_HEIGHT;
    if (top < el.scrollTop) {
      el.scrollTop = top;
    } else if (top + ROW_HEIGHT > el.scrollTop + el.clientHeight) {
      el.scrollTop = top + ROW_HEIGHT - el.clientHeight;
    }
  }

  private async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.toasts.show('success', this.i18n.t('toast.copied'));
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.copyFailed'));
    }
  }

  /**
   * Opens the finished file or reveals it in its folder through the Rust side. In a plain
   * browser there is no shell to ask, so the "coming soon" note stays.
   */
  private openPath(command: 'open_file' | 'show_in_folder', d: Download): void {
    if (!this.tauriAvailable) {
      this.toasts.show('info', this.i18n.t('toast.openSoon'));
      return;
    }
    this.run(this.invoke<void>(command, { path: finalPath(d.dir, d.filename) }));
  }

  /**
   * Engine commands resolve when accepted and the list follows the engine's events, so nothing
   * waits on them; a refusal is the one thing worth telling the user.
   */
  private run(command: Promise<void>): void {
    command.catch((error: unknown) => {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
    });
  }
}
