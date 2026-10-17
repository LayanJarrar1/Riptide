import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';

import { FormatService } from '../../core/format.service';
import { DUR, MotionService } from '../../core/motion.service';
import { finalPath } from '../../data/bridge-mapping';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadRow, DownloadsStore, SortKey } from '../../data/downloads.store';
import { I18nService } from '../../i18n/i18n.service';
import { canPause, canResume, Download } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { EmptyStateComponent, EmptyStep } from '../../ui/empty-state.component';
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
/** How many rows take part in the first-paint cascade. */
const STAGGER_ROWS = 12;
/** Skeleton rows shown while the engine's list is still loading. */
const SKELETON_ROWS = [0, 1, 2, 3, 4, 5] as const;
/** A row added during the session counts as "fresh" (slides in) for this long. */
const FRESH_MS = 600;

/**
 * The main screen: toolbar, sortable header, a virtual-scrolled grid of rows, the details
 * drawer, the row context menu and the confirmation for removal. Keyboard: ↑/↓ move the
 * selection, Enter opens details, Space pauses/resumes, Delete removes, Escape clears.
 *
 * Loading: while the engine has not delivered its list, six skeleton rows shimmer in place of
 * the table, so "Nothing here yet" never flashes at start. Motion: the first rows cascade in
 * once on boot; a changed view, filter, search or sort crossfades the list; rows removed by
 * the user fade and collapse before the engine is asked to drop them.
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
      <div class="toolbar" [class.boot]="ui.booting()">
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
            <button appButton size="sm" variant="ghost" icon="close" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('toolbar.clearSelection')" [attr.title]="i18n.t('toolbar.clearSelection')" (click)="store.clearSelection()"></button>
          </div>
        } @else {
          <div class="bulk">
            <button appButton size="sm" variant="ghost" icon="pause" type="button" [disabled]="store.totals().active === 0 && store.totals().queued === 0" (click)="pauseAll()">{{ i18n.t('toolbar.pauseAll') }}</button>
            <button appButton size="sm" variant="ghost" icon="play" type="button" [disabled]="store.viewCounts().queued === 0 && store.viewCounts().failed === 0" (click)="resumeAll()">{{ i18n.t('toolbar.resumeAll') }}</button>
          </div>
        }
      </div>

      <div class="grid" role="grid" [attr.aria-rowcount]="store.rows().length" [attr.aria-multiselectable]="true">
        <div class="header" role="row" [class.boot]="ui.booting()">
          @for (c of columns; track c.key) {
            <button
              type="button"
              role="columnheader"
              [class]="'th th-' + c.key"
              [class.is-end]="c.align === 'end'"
              [class.is-sorted]="store.sortKey() === c.key"
              [attr.aria-sort]="store.sortKey() === c.key ? (store.sortDir() === 'asc' ? 'ascending' : 'descending') : 'none'"
              [attr.title]="store.sortKey() === c.key ? i18n.t(store.sortDir() === 'asc' ? 'sort.asc' : 'sort.desc') : i18n.t('sort.by').replace('{column}', i18n.t(c.labelKey))"
              (click)="store.sortBy(c.key)"
            >
              <span>{{ i18n.t(c.labelKey) }}</span>
              <app-icon class="sort-icon" name="arrowDown" [size]="12" [class.is-asc]="store.sortDir() === 'asc'" />
            </button>
          }
          <span class="th th-actions" role="columnheader"></span>
        </div>

        @if (!engine.loaded()) {
          <div class="skeleton" role="status" aria-busy="true" [attr.aria-label]="i18n.t('loading.list')">
            @for (n of skeletonRows; track n) {
              <div class="sk-row" [style.--i]="n" aria-hidden="true">
                <span class="sk-cell sk-name">
                  <span class="sk-type bone"></span>
                  <span class="sk-lines"><span class="bone sk-line"></span><span class="bone sk-line is-short"></span></span>
                </span>
                <span class="sk-cell sk-size"><span class="bone sk-num"></span></span>
                <span class="sk-cell sk-progress"><span class="bone sk-bar"></span></span>
                <span class="sk-cell sk-speed"><span class="bone sk-num"></span></span>
                <span class="sk-cell sk-eta"><span class="bone sk-num is-short"></span></span>
                <span class="sk-cell sk-status"><span class="bone sk-pill"></span></span>
                <span class="sk-cell sk-added"><span class="bone sk-num is-short"></span></span>
              </div>
            }
          </div>
        } @else if (store.rows().length === 0) {
          <app-empty-state
            [icon]="emptyIcon()"
            [illustration]="emptyKind() === 'all' ? 'segments' : null"
            [heading]="i18n.t('empty.' + emptyKind() + '.heading')"
            [body]="i18n.t('empty.' + emptyKind() + '.body')"
            [steps]="emptyKind() === 'all' ? steps() : []"
            [stepsLabel]="i18n.t('empty.steps.title')"
          >
            @if (emptyKind() === 'all') {
              <button appButton variant="primary" icon="plus" type="button" (click)="ui.openAdd()">{{ i18n.t('action.add') }}</button>
            } @else if (emptyKind() === 'search') {
              <button appButton variant="secondary" type="button" (click)="store.query.set('')">{{ i18n.t('empty.search.clear') }}</button>
            }
          </app-empty-state>
        } @else {
          <div #viewport class="viewport" (scroll)="onScroll()" tabindex="0" [attr.aria-label]="i18n.t('view.' + store.view())">
            <div class="spacer" [style.block-size.px]="window().top"></div>
            @for (row of window().rows; track row.d.id; let i = $index) {
              <app-download-row
                [row]="row"
                [selected]="store.selected().has(row.d.id)"
                [open]="store.detailsId() === row.d.id"
                [staggerIndex]="ui.booting() && window().top === 0 && i < staggerRows ? i : null"
                [fresh]="freshIds().has(row.d.id)"
                [leaving]="leavingIds().has(row.d.id)"
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
      transition: grid-template-columns var(--dur-3) var(--ease-emphasized-out);
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

    /* First paint: toolbar and header arrive right after the shell, before the rows. */
    .toolbar.boot {
      animation: boot-in var(--dur-3) var(--ease-emphasized-out) both;
      animation-delay: calc(1 * var(--stagger-step));
    }

    .header.boot {
      animation: boot-in var(--dur-3) var(--ease-emphasized-out) both;
      animation-delay: calc(2 * var(--stagger-step));
    }

    @keyframes boot-in {
      from {
        opacity: 0;
        transform: translateY(-4px);
      }

      to {
        opacity: 1;
        transform: none;
      }
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
      animation: bulk-in var(--dur-2) var(--ease-emphasized-out) both;
    }

    @keyframes bulk-in {
      from {
        opacity: 0;
        transform: translateY(-3px);
      }

      to {
        opacity: 1;
        transform: none;
      }
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
         switches to the compact set in the same container query (see download-row). The full
         set needs 1034px plus the scrollbar gutter, so it fits a 1280px window with the sidebar
         open; --grid-break is where it stops fitting. */
      --grid-columns: minmax(190px, 2.2fr) 150px minmax(150px, 1.6fr) 96px 84px 172px 104px 64px;
      --grid-columns-compact: minmax(180px, 2fr) 128px minmax(120px, 1.4fr) 92px 150px 60px;
      --grid-columns-narrow: minmax(160px, 2fr) 110px minmax(100px, 1.4fr) 140px 52px;
      --grid-columns-tiny: minmax(140px, 1fr) minmax(90px, 1fr) 44px;
      container-type: inline-size;
      container-name: grid;
    }

    .header {
      display: grid;
      grid-template-columns: var(--grid-columns);
      align-items: center;
      block-size: var(--header-height);
      padding-inline: var(--space-3);
      border-block: 1px solid var(--color-border);
      background: var(--color-surface-1);
      /* the same gutter the viewport reserves, so labels sit exactly over their cells */
      overflow: hidden;
      scrollbar-gutter: stable;
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
      transition: color var(--dur-1) var(--ease-standard);
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

    /* The sort arrow lives in every header; it only shows (in the accent) on the sorted one,
       and turns over rather than swapping glyphs when the direction flips. */
    .sort-icon {
      color: var(--color-accent-fg);
      opacity: 0;
      transform: translateY(2px);
      transition:
        opacity var(--dur-2) var(--ease-standard),
        transform var(--dur-3) var(--ease-emphasized-out);
    }

    .th:hover .sort-icon {
      opacity: 0.45;
    }

    .th.is-sorted .sort-icon {
      opacity: 1;
      transform: none;
    }

    .th.is-sorted .sort-icon.is-asc {
      transform: rotate(180deg);
    }

    .th-actions {
      cursor: default;
    }

    /* Narrower lists (details drawer open, small window) drop columns in order of usefulness:
       time left and added, then speed, then size and status (which the drawer still shows). */
    @container grid (max-width: 1050px) {
      .header,
      .sk-row {
        grid-template-columns: var(--grid-columns-compact);
      }

      .th-eta,
      .th-added,
      .sk-eta,
      .sk-added {
        display: none;
      }
    }

    @container grid (max-width: 780px) {
      .header,
      .sk-row {
        grid-template-columns: var(--grid-columns-narrow);
      }

      .th-speed,
      .sk-speed {
        display: none;
      }
    }

    @container grid (max-width: 600px) {
      .header,
      .sk-row {
        grid-template-columns: var(--grid-columns-tiny);
      }

      .th-size,
      .th-status,
      .sk-size,
      .sk-status {
        display: none;
      }
    }

    .viewport {
      overflow-y: auto;
      overflow-x: hidden;
      outline-offset: -2px;
      scrollbar-gutter: stable;
    }

    /* ---- Skeleton: the table's silhouette; the bones breathe (flat, no sweep) ------------ */
    .skeleton {
      overflow: hidden;
    }

    .sk-row {
      display: grid;
      grid-template-columns: var(--grid-columns);
      align-items: center;
      block-size: var(--row-height);
      padding-inline: var(--space-3);
      border-block-end: 1px solid var(--color-border);
      animation: sk-in var(--dur-3) var(--ease-standard) both;
      animation-delay: calc(var(--i) * 40ms);
    }

    @keyframes sk-in {
      from {
        opacity: 0;
      }

      to {
        opacity: 1;
      }
    }

    .sk-cell {
      display: flex;
      align-items: center;
      min-inline-size: 0;
      padding-inline: var(--space-2);
    }

    .sk-name {
      gap: var(--space-3);
      padding-inline-start: 0;
    }

    .sk-size,
    .sk-speed,
    .sk-eta,
    .sk-added {
      justify-content: flex-end;
    }

    .bone {
      display: block;
      block-size: 10px;
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
      animation: breathe 1600ms var(--ease-standard) infinite alternate;
      animation-delay: calc(var(--i) * 90ms);
    }

    @keyframes breathe {
      from {
        opacity: 0.55;
      }

      to {
        opacity: 1;
      }
    }

    .sk-type {
      flex-shrink: 0;
      inline-size: 32px;
      block-size: 32px;
      border-radius: var(--radius-sm);
    }

    .sk-lines {
      display: flex;
      flex: 1;
      flex-direction: column;
      gap: 6px;
      min-inline-size: 0;
    }

    .sk-line {
      inline-size: 70%;
    }

    .sk-line.is-short {
      inline-size: 40%;
      block-size: 8px;
    }

    .sk-num {
      inline-size: 60%;
    }

    .sk-num.is-short {
      inline-size: 40%;
    }

    .sk-bar {
      block-size: 6px;
      inline-size: 80%;
    }

    .sk-pill {
      inline-size: 80px;
      block-size: 20px;
    }

    .drawer {
      min-inline-size: 0;
      overflow: hidden;
      animation: drawer-in var(--dur-3) var(--ease-emphasized-out) both;
    }

    @keyframes drawer-in {
      from {
        opacity: 0;
        transform: translateX(calc(var(--dir-sign) * 32px));
      }

      to {
        opacity: 1;
        transform: none;
      }
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

    @media (prefers-reduced-motion: reduce) {
      :host {
        transition: none;
      }

      .bone {
        animation: none;
        opacity: 0.8;
      }

      .sk-row {
        animation-delay: 0ms;
      }

      .toolbar.boot,
      .header.boot,
      .bulk,
      .drawer {
        animation-duration: var(--dur-fade);
        animation-delay: 0ms;
        animation-name: sk-in;
      }

      .sort-icon {
        transition: none;
      }
    }
  `,
})
export class DownloadsPageComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly store = inject(DownloadsStore);
  protected readonly ui = inject(UiState);
  protected readonly engine = inject(DownloadEngine);
  private readonly motion = inject(MotionService);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly toasts = inject(ToastService);
  private readonly destroyRef = inject(DestroyRef);
  private readonly viewportRef = viewChild<ElementRef<HTMLElement>>('viewport');

  protected readonly columns = COLUMNS;
  protected readonly skeletonRows = SKELETON_ROWS;
  protected readonly staggerRows = STAGGER_ROWS;

  private readonly scrollTop = signal(0);
  private readonly viewportHeight = signal(600);

  protected readonly menu = signal<{ open: boolean; x: number; y: number; row: DownloadRow | null }>({ open: false, x: 0, y: 0, row: null });
  protected readonly removal = signal<readonly DownloadRow[] | null>(null);
  protected readonly deleteFile = signal(false);

  /** Ids the user is removing; their rows play the leave animation before the engine drops them. */
  protected readonly leavingIds = signal<ReadonlySet<number>>(new Set());

  /** Ids seen since the list first arrived; a download outside this set is new and slides in. */
  private readonly seenIds = signal<ReadonlySet<number> | null>(null);

  protected readonly freshIds = computed<ReadonlySet<number>>(() => {
    const seen = this.seenIds();
    const fresh = new Set<number>();
    if (seen !== null) {
      for (const d of this.engine.downloads()) {
        if (!seen.has(d.id)) {
          fresh.add(d.id);
        }
      }
    }
    return fresh;
  });

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

  /** "How to start", for a list with nothing in it at all. */
  protected readonly steps = computed<readonly EmptyStep[]>(() =>
    ([1, 2, 3] as const).map((n) => ({ title: this.i18n.t(`empty.step${n}.title`), body: this.i18n.t(`empty.step${n}.body`) })),
  );

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

  /** What the list is showing; a change of any part crossfades the viewport. */
  private readonly listKey = computed(
    () => `${this.store.view()}|${this.store.category() ?? ''}|${this.store.query().trim().toLowerCase()}|${this.store.sortKey()}|${this.store.sortDir()}`,
  );
  private lastListKey: string | null = null;

  constructor() {
    afterNextRender(() => this.observeViewport());

    // the first list to arrive is the baseline: nothing in it is "new"
    effect(() => {
      const loaded = this.engine.loaded();
      const ids = this.engine.downloads().map((d) => d.id);
      untracked(() => {
        if (loaded && this.seenIds() === null) {
          this.seenIds.set(new Set(ids));
        }
      });
    });

    // a fresh row has slid in after FRESH_MS; from then on it is an ordinary row
    effect((onCleanup) => {
      const fresh = this.freshIds();
      if (fresh.size === 0) {
        return;
      }
      const handle = setTimeout(() => {
        this.seenIds.update((seen) => new Set([...(seen ?? []), ...fresh]));
      }, FRESH_MS);
      onCleanup(() => clearTimeout(handle));
    });

    effect(() => {
      const key = this.listKey();
      untracked(() => this.onListChanged(key));
    });
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

  /** View, filter, search or sort changed: back to the top, and the list crossfades. */
  private onListChanged(key: string): void {
    if (this.lastListKey === null) {
      this.lastListKey = key;
      return;
    }
    if (key === this.lastListKey) {
      return;
    }
    this.lastListKey = key;
    this.scrollTop.set(0);
    const el = this.viewportRef()?.nativeElement;
    if (!el || typeof el.animate !== 'function') {
      return;
    }
    el.scrollTop = 0;
    const reduced = this.motion.reduced();
    el.animate(
      reduced
        ? [{ opacity: 0.4 }, { opacity: 1 }]
        : [
            { opacity: 0.3, transform: 'translateY(4px)' },
            { opacity: 1, transform: 'none' },
          ],
      { duration: reduced ? 80 : DUR.d2, easing: 'cubic-bezier(0.05, 0.7, 0.1, 1)' },
    );
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

  /**
   * Removal in two beats: the rows fade and collapse (the toast and the closed drawer are
   * immediate), then the engine is asked to drop them. Under reduced motion the second beat
   * follows at once.
   */
  protected confirmRemove(): void {
    const rows = this.removal();
    if (rows === null) {
      return;
    }
    const removed = rows.map((r) => r.d);
    const deleteFile = this.deleteFile();
    this.store.clearSelection();
    if (removed.some((d) => d.id === this.store.detailsId())) {
      this.store.closeDetails();
    }
    this.removal.set(null);
    const text = removed.length === 1 ? this.i18n.t('toast.removed').replace('{name}', removed[0].filename) : this.i18n.t('toast.removedMany').replace('{n}', this.format.int(removed.length));
    this.toasts.show('info', text);

    const ids = removed.map((d) => d.id);
    this.leavingIds.update((s) => new Set([...s, ...ids]));
    const drop = (): void => {
      for (const d of removed) {
        this.run(this.engine.remove(d.id, deleteFile));
      }
      this.leavingIds.update((s) => {
        const next = new Set(s);
        for (const id of ids) {
          next.delete(id);
        }
        return next;
      });
    };
    if (this.motion.reduced()) {
      drop();
    } else {
      const handle = setTimeout(drop, DUR.d2);
      this.destroyRef.onDestroy(() => clearTimeout(handle));
    }
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
