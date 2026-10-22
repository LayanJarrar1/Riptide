import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  NgZone,
  signal,
  untracked,
  viewChild,
} from '@angular/core';

import { FormatService } from '../../core/format.service';
import { DUR, MotionService } from '../../core/motion.service';
import { PREFERENCE_STORE } from '../../core/preferences';
import { finalPath } from '../../data/bridge-mapping';
import {
  clampWidth,
  ColumnKey,
  ColumnWidths,
  gridVars,
  loadColumnWidths,
  loadHiddenColumns,
  MAX_COLUMN_WIDTH,
  OPTIONAL_COLUMNS,
  saveColumnWidths,
  saveHiddenColumns,
} from '../../data/column-widths';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadRow, DownloadsStore } from '../../data/downloads.store';
import { navFromKey, scrollToReveal } from '../../data/list-nav';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { canPause, canResume, Download, failureFix } from '../../model/download';
import { MAIN_QUEUE_ID, moveBy, moveManyBeside, sameOrder } from '../../model/queue';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { EmptyStateComponent } from '../../ui/empty-state.component';
import type { EmptyStep } from '../../ui/empty-state.component';
import { IconComponent } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { ToastService } from '../../ui/toast.service';
import { DOWNLOAD_DRAG_TYPE, dragHas, inLowerHalf } from '../queues/dnd';
import { MoveToQueueDialogComponent } from '../queues/move-to-queue-dialog.component';
import { SettingsService } from '../settings/settings.service';
import { UiState } from '../shell/ui-state.service';
import { DownloadDetailsComponent } from './download-details.component';
import { DownloadRowComponent, RowAction } from './download-row.component';
import { RestartDialogComponent } from './restart-dialog.component';

/** Table columns in order; `grow` ones share the leftover width. */
interface Column {
  readonly key: ColumnKey;
  readonly labelKey: string;
  readonly align: 'start' | 'end';
}

const COLUMNS: readonly Column[] = [
  { key: 'name', labelKey: 'column.name', align: 'start' },
  { key: 'size', labelKey: 'column.size', align: 'end' },
  { key: 'progress', labelKey: 'column.progress', align: 'start' },
  { key: 'speed', labelKey: 'column.speed', align: 'end' },
  { key: 'avgSpeed', labelKey: 'column.avgSpeed', align: 'end' },
  { key: 'eta', labelKey: 'column.eta', align: 'end' },
  { key: 'duration', labelKey: 'column.duration', align: 'end' },
  { key: 'status', labelKey: 'column.status', align: 'start' },
  { key: 'category', labelKey: 'column.category', align: 'start' },
  { key: 'added', labelKey: 'column.added', align: 'start' },
];

/** Row heights in px for the two display densities (they match `--row-height` in tokens.css). */
export const ROW_HEIGHTS = { comfortable: 46, compact: 36 } as const;
/** Rows rendered above and below the viewport so fast scrolling never shows a gap. */
const OVERSCAN = 6;
/** How many rows take part in the first-paint cascade. */
const STAGGER_ROWS = 12;
/** Skeleton rows shown while the engine's list is still loading. */
const SKELETON_ROWS = [0, 1, 2, 3, 4, 5] as const;
/** The list keeps at least this much width beside a docked details drawer; below it the drawer overlays. */
const MIN_LIST_BESIDE_DRAWER = 600;
/** Fallback for `--drawer-width` (tokens.css) when the style cannot be read. */
const DRAWER_PX = 420;
/** Arrow-key step when resizing a column from the keyboard (Shift: four times as much). */
const RESIZE_STEP = 16;
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
    MoveToQueueDialogComponent,
    RestartDialogComponent,
  ],
  host: {
    '[class.has-details]': 'store.details() !== null && !detailsOverlay()',
    '[class.details-overlay]': 'store.details() !== null && detailsOverlay()',
    '(keydown)': 'onKey($event)',
  },
  template: `
    <section class="list" [attr.aria-label]="i18n.t('view.' + store.view())">
      <div class="toolbar" [class.boot]="ui.booting()">
        <div class="toolbar-title">
          <h1 class="title">{{ title() }}</h1>
          <span class="count num">{{ format.int(store.rows().length) }}</span>
          @if (selectedQueue(); as q) {
            <span class="queue-tools">
              <span class="queue-state" [class.is-running]="q.enabled">
                <span class="queue-dot" aria-hidden="true"></span>{{ i18n.t(q.enabled ? 'queue.running' : 'queue.stopped') }}
              </span>
              <button appButton size="sm" variant="ghost" type="button" [icon]="q.enabled ? 'stop' : 'play'" (click)="toggleQueue(q.id, q.enabled)">
                {{ i18n.t(q.enabled ? 'queue.stop' : 'queue.start') }}
              </button>
              @if (!store.manualOrder()) {
                <button appButton size="sm" variant="ghost" type="button" icon="list" (click)="store.setQueue(q.id)">{{ i18n.t('queue.orderChip') }}</button>
              }
            </span>
          }
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

      <div
        #grid
        class="grid"
        role="grid"
        tabindex="0"
        [style]="gridStyle()"
        [attr.aria-label]="i18n.t('view.' + store.view())"
        [attr.aria-rowcount]="store.rows().length + 1"
        [attr.aria-multiselectable]="true"
        [attr.aria-activedescendant]="activeRowId()"
      >
        <div #header class="header" role="row" [class.boot]="ui.booting()" [class.is-resizing]="resizing() !== null" (contextmenu)="onHeaderContext($event)">
          @for (c of visibleColumns(); track c.key) {
            <div
              role="columnheader"
              [class]="'hc hc-' + c.key"
              [attr.aria-sort]="store.sortKey() === c.key ? (store.sortDir() === 'asc' ? 'ascending' : 'descending') : 'none'"
            >
              <button
                type="button"
                [class]="'th th-' + c.key"
                [class.is-end]="c.align === 'end'"
                [class.is-sorted]="store.sortKey() === c.key"
                [attr.title]="store.sortKey() === c.key ? i18n.t(store.sortDir() === 'asc' ? 'sort.asc' : 'sort.desc') : i18n.t('sort.by').replace('{column}', i18n.t(c.labelKey))"
                (click)="store.sortBy(c.key)"
              >
                <span class="th-label">{{ i18n.t(c.labelKey) }}</span>
                <app-icon class="sort-icon" name="arrowDown" [size]="12" [class.is-asc]="store.sortDir() === 'asc'" />
              </button>
              <span
                class="col-resize"
                role="separator"
                tabindex="0"
                aria-orientation="vertical"
                [class.is-active]="resizing() === c.key"
                [attr.aria-label]="i18n.t('column.resize').replace('{column}', i18n.t(c.labelKey))"
                [attr.title]="i18n.t('column.resizeHint')"
                [attr.aria-valuenow]="columnWidths()[c.key] ?? null"
                [attr.aria-valuemin]="0"
                [attr.aria-valuemax]="maxColumnWidth"
                (pointerdown)="startResize($event, c.key)"
                (dblclick)="resetColumn(c.key)"
                (keydown)="onResizeKey($event, c.key)"
              ></span>
            </div>
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
          @defer (on immediate; prefetch on idle) {
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
          }
        } @else {
          <div #viewport class="viewport" role="presentation">
            <div class="spacer" [style.block-size.px]="window().top"></div>
            @for (row of window().rows; track row.d.id; let i = $index) {
              <app-download-row
                [id]="'dl-row-' + row.d.id"
                [attr.aria-rowindex]="window().start + i + 2"
                [class.is-active]="store.cursor() === row.d.id"
                [row]="row"
                [hidden]="hiddenColumns()"
                [selected]="store.selected().has(row.d.id)"
                [open]="store.detailsId() === row.d.id"
                [staggerIndex]="ui.booting() && window().top === 0 && i < staggerRows ? i : null"
                [fresh]="freshIds().has(row.d.id)"
                [leaving]="leavingIds().has(row.d.id)"
                [class.drop-before]="rowMark()?.id === row.d.id && !rowMark()?.after"
                [class.drop-after]="rowMark()?.id === row.d.id && rowMark()?.after"
                draggable="true"
                (dragstart)="onRowDragStart($event, row)"
                (dragover)="onRowDragOver($event, row)"
                (dragleave)="onRowDragLeave(row)"
                (drop)="onRowDrop($event, row)"
                (dragend)="rowDragEnd()"
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

    @defer (when store.details() !== null; prefetch on idle) {
      @if (store.details(); as details) {
        @if (detailsOverlay()) {
          <div class="scrim" aria-hidden="true" (click)="store.closeDetails()"></div>
        }
        <app-download-details class="drawer" [class.is-overlay]="detailsOverlay()" [row]="details" (closed)="store.closeDetails()" (action)="act(details, $event)" />
      }
    }

    <app-menu [open]="menu().open" [items]="menuItems()" [x]="menu().x" [y]="menu().y" (selected)="onMenu($event)" (closed)="closeMenu()" />
    <app-menu [open]="headerMenu() !== null" [items]="headerMenuItems()" [x]="headerMenu()?.x ?? 0" [y]="headerMenu()?.y ?? 0" (selected)="onHeaderMenu($event)" (closed)="headerMenu.set(null)" />

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

    @defer (when restartTarget() !== null) {
      <app-restart-dialog [target]="restartTarget()" (confirmed)="confirmRestart()" (closed)="restartTarget.set(null)" />
    }

    @defer (when moveTargets() !== null) {
      <app-move-to-queue-dialog [targets]="moveTargets()" (chosen)="confirmMove($event)" (closed)="moveTargets.set(null)" />
    }
  `,
  styles: `
    :host {
      position: relative;
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

    .queue-tools {
      display: inline-flex;
      align-items: center;
      align-self: center;
      gap: var(--space-2);
      margin-inline-start: var(--space-3);
    }

    .queue-state {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .queue-dot {
      inline-size: 8px;
      block-size: 8px;
      border-radius: 50%;
      border: 1.5px solid var(--color-text-muted);
    }

    .queue-state.is-running .queue-dot {
      border-color: var(--color-accent);
      background: var(--color-accent);
    }

    app-download-row.drop-before {
      box-shadow: inset 0 2px 0 var(--color-accent);
    }

    app-download-row.drop-after {
      box-shadow: inset 0 -2px 0 var(--color-accent);
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
      /* rows, skeleton and header read --grid-columns(-compact|-narrow|-tiny) and --grid-min(...)
         from the [style] binding (see data/column-widths.ts: the defaults plus the user's widths);
         the rows switch set in the same container queries as the header (see download-row). */
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

    /* A header cell: the sort button fills it, the resize grip sits on its trailing edge. */
    .hc {
      position: relative;
      display: flex;
      min-inline-size: 0;
      block-size: 100%;
    }

    .th {
      display: inline-flex;
      flex: 1 1 auto;
      align-items: center;
      gap: 4px;
      min-inline-size: 0;
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

    .hc-name .th {
      padding-inline-start: 0;
    }

    .th-label {
      overflow: hidden;
      text-overflow: ellipsis;
    }

    /* Drag (or arrow keys when focused) to resize; double-click restores the default width.
       The grip is wider than the hairline it draws so it is easy to catch. */
    .col-resize {
      position: absolute;
      z-index: 1;
      inset-block: 0;
      inset-inline-end: -5px;
      inline-size: 10px;
      cursor: col-resize;
      touch-action: none;
    }

    .col-resize::after {
      content: '';
      position: absolute;
      inset-block: 8px;
      inset-inline-start: 4.5px;
      inline-size: 1px;
      background: var(--color-border);
      transition:
        background-color var(--dur-1) var(--ease-standard),
        inset-block var(--dur-1) var(--ease-standard);
    }

    .col-resize:hover::after,
    .col-resize:focus-visible::after,
    .col-resize.is-active::after {
      inset-block: 4px;
      inline-size: 2px;
      background: var(--color-accent);
    }

    .col-resize:focus-visible {
      outline: none;
    }

    .header.is-resizing,
    .header.is-resizing .th {
      cursor: col-resize;
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

      .sk-row {
        min-inline-size: var(--grid-min-compact);
      }

      .hc-eta,
      .hc-added,
      .hc-avgSpeed,
      .hc-duration,
      .hc-category,
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

      .sk-row {
        min-inline-size: var(--grid-min-narrow);
      }

      .hc-speed,
      .sk-speed {
        display: none;
      }
    }

    @container grid (max-width: 600px) {
      .header,
      .sk-row {
        grid-template-columns: var(--grid-columns-tiny);
      }

      .sk-row {
        min-inline-size: var(--grid-min-tiny);
      }

      .hc-size,
      .hc-status,
      .sk-size,
      .sk-status {
        display: none;
      }
    }

    .grid:focus {
      outline: none;
    }

    /* the grid holds focus (aria-activedescendant); the ring is drawn on the active row */
    .grid:focus-visible app-download-row.is-active {
      outline: 2px solid var(--color-focus, var(--color-accent));
      outline-offset: -2px;
    }

    /* Sideways scrolling only appears when the user has widened columns past the window; the
       header follows the viewport's scrollLeft (see the scroll listener). */
    .viewport {
      overflow-y: auto;
      overflow-x: auto;
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
      min-inline-size: var(--grid-min);
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

    /* A window too narrow to dock the drawer beside a usable table: it slides over the list
       instead, which keeps its full width, columns and scroll position underneath. */
    .drawer.is-overlay {
      position: absolute;
      z-index: var(--z-raised);
      inset-block: 0;
      inset-inline-end: 0;
      inline-size: min(var(--drawer-width), calc(100% - 48px));
      box-shadow: var(--shadow-3);
    }

    .scrim {
      position: absolute;
      z-index: var(--z-raised);
      inset: 0;
      background: rgb(0 0 0 / 18%);
      animation: scrim-in var(--dur-2) var(--ease-standard) both;
    }

    @keyframes scrim-in {
      from {
        opacity: 0;
      }
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
  private readonly queues = inject(QueuesService);
  private readonly destroyRef = inject(DestroyRef);
  private readonly viewportRef = viewChild<ElementRef<HTMLElement>>('viewport');
  private readonly gridRef = viewChild<ElementRef<HTMLElement>>('grid');
  private readonly headerRef = viewChild<ElementRef<HTMLElement>>('header');
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly prefs = inject(PREFERENCE_STORE);
  private readonly zone = inject(NgZone);
  private readonly settings = inject(SettingsService);

  protected readonly skeletonRows = SKELETON_ROWS;
  protected readonly staggerRows = STAGGER_ROWS;

  private readonly scrollTop = signal(0);
  private readonly viewportHeight = signal(600);

  /** Width of the page (list plus drawer), for choosing a docked or an overlaid drawer. */
  private readonly hostWidth = signal(Number.POSITIVE_INFINITY);
  private readonly drawerWidth = signal(DRAWER_PX);

  /** Whether the details drawer slides over the list rather than taking a column beside it. */
  protected readonly detailsOverlay = computed(() => this.hostWidth() < this.drawerWidth() + MIN_LIST_BESIDE_DRAWER);

  protected readonly maxColumnWidth = MAX_COLUMN_WIDTH;
  /** The user's column widths (persisted); unset columns keep their responsive defaults. */
  protected readonly columnWidths = signal<ColumnWidths>({});
  /** Columns the user turned off (persisted); the optional new ones start off. */
  protected readonly hiddenColumns = signal<ReadonlySet<ColumnKey>>(new Set());
  protected readonly visibleColumns = computed(() => {
    const hidden = this.hiddenColumns();
    return COLUMNS.filter((c) => !hidden.has(c.key));
  });
  /** Track lists and minimum widths for the header, skeleton and rows. */
  protected readonly gridStyle = computed(() => gridVars(this.columnWidths(), this.hiddenColumns()));
  /** The column being dragged, for the grip's active look. */
  protected readonly resizing = signal<ColumnKey | null>(null);
  /** The header's context menu (reset widths), at a viewport point. */
  protected readonly headerMenu = signal<{ readonly x: number; readonly y: number } | null>(null);
  protected readonly headerMenuItems = computed<readonly MenuItem[]>(() => {
    const hidden = this.hiddenColumns();
    const toggles = OPTIONAL_COLUMNS.map((key): MenuItem => {
      const column = COLUMNS.find((c) => c.key === key);
      return { id: `toggle:${key}`, label: this.i18n.t(column?.labelKey ?? key), ...(hidden.has(key) ? {} : { icon: 'check' }) };
    });
    return [...toggles, { id: 'resetWidths', label: this.i18n.t('column.resetWidths'), icon: 'retry', separator: true, disabled: Object.keys(this.columnWidths()).length === 0 }];
  });

  /** Row height for the display density chosen in the View menu. */
  protected readonly rowHeight = computed(() => ROW_HEIGHTS[this.settings.settings().density]);

  protected readonly menu = signal<{ open: boolean; x: number; y: number; row: DownloadRow | null }>({ open: false, x: 0, y: 0, row: null });
  protected readonly removal = signal<readonly DownloadRow[] | null>(null);
  protected readonly deleteFile = signal(false);
  /** Download awaiting the "restart from the beginning" confirmation. */
  protected readonly restartTarget = signal<Download | null>(null);

  /** Downloads awaiting a queue choice in the "Move to queue" dialog. */
  protected readonly moveTargets = signal<readonly Download[] | null>(null);

  /** Where a dragged row would land in the manual order: next to `id`, before or after. */
  protected readonly rowMark = signal<{ readonly id: number; readonly after: boolean } | null>(null);
  /** Ids being dragged (the selection when the dragged row is part of it). */
  private dragged: readonly number[] = [];

  /** The queue the list is limited to, if any. */
  protected readonly selectedQueue = computed(() => {
    const id = this.store.queue();
    return id === null ? undefined : this.queues.get(id);
  });

  /** Ids the user is removing; their rows play the leave animation before the engine drops them. */
  protected readonly leavingIds = signal<ReadonlySet<number>>(new Set());

  /** Highest download id when the first list arrived; anything above it was added this session. */
  private readonly freshBaseline = signal<number | null>(null);
  /** Session-added ids whose enter animation is over. */
  private readonly settled = signal<ReadonlySet<number>>(new Set());

  /**
   * Ids added since the list first arrived whose slide-in is still running. The engine's list
   * is oldest first, so only its tail is walked: a tick costs nothing however long the list is.
   */
  protected readonly freshIds = computed<ReadonlySet<number>>(
    () => {
      const baseline = this.freshBaseline();
      const fresh = new Set<number>();
      if (baseline === null) {
        return fresh;
      }
      const settled = this.settled();
      const all = this.engine.downloads();
      for (let i = all.length - 1; i >= 0; i--) {
        const id = all[i].id;
        if (id <= baseline) {
          break;
        }
        if (!settled.has(id)) {
          fresh.add(id);
        }
      }
      return fresh;
    },
    { equal: (a, b) => a.size === b.size && [...a].every((id) => b.has(id)) },
  );

  protected readonly title = computed(() => {
    const queue = this.selectedQueue();
    if (queue !== undefined) {
      return queue.id === MAIN_QUEUE_ID ? this.i18n.t('queue.main') : queue.name;
    }
    const category = this.store.category();
    return category === null ? this.i18n.t(`view.${this.store.view()}`) : this.i18n.t(`category.${category}`);
  });

  /**
   * First and last (exclusive) row to render. Two primitive computeds, so a scroll that stays
   * inside the same rows (most scroll events) does not recompute the window at all.
   */
  private readonly firstIndex = computed(() => {
    const count = this.store.rows().length;
    return Math.min(count, Math.max(0, Math.floor(this.scrollTop() / this.rowHeight()) - OVERSCAN));
  });

  private readonly endIndex = computed(() => {
    const count = this.store.rows().length;
    return Math.min(count, Math.ceil((this.scrollTop() + this.viewportHeight()) / this.rowHeight()) + OVERSCAN);
  });

  /** The rows inside the scroll window plus spacers that stand in for the rest. */
  protected readonly window = computed(() => {
    const rows = this.store.rows();
    const start = this.firstIndex();
    const end = Math.max(start, this.endIndex());
    const rowHeight = this.rowHeight();
    return {
      rows: rows.slice(start, end),
      start,
      top: start * rowHeight,
      bottom: Math.max(0, (rows.length - end) * rowHeight),
    };
  });

  /** DOM id of the row the keyboard is on, for `aria-activedescendant` (only while that row is rendered). */
  protected readonly activeRowId = computed(() => {
    const id = this.store.cursor();
    return id !== null && this.window().rows.some((r) => r.d.id === id) ? `dl-row-${id}` : null;
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
    } else if (s === 'failed' && failureFix(row.d.failure)?.action === 'refreshLink') {
      items.push({ id: 'refreshLink', label: t(failureFix(row.d.failure)?.labelKey ?? 'action.refreshLink'), icon: 'link' });
      items.push({ id: 'retry', label: t('action.retry'), icon: 'retry', shortcut: 'Space' });
    } else if (s === 'failed' && failureFix(row.d.failure) === null) {
      // a missing file (404/410) cannot be fixed by retrying: no Retry in the menu
    } else if (canResume(s)) {
      items.push({ id: s === 'failed' ? 'retry' : 'resume', label: t(s === 'failed' ? 'action.retry' : 'action.resume'), icon: s === 'failed' ? 'retry' : 'play', shortcut: 'Space' });
    }
    if (s === 'completed') {
      items.push({ id: 'openFile', label: t('action.openFile'), icon: 'external', shortcut: 'Enter' });
    }
    items.push({ id: 'openFolder', label: t('action.openFolder'), icon: 'folderOpen' });
    items.push({ id: 'details', label: t('action.details'), icon: 'info', shortcut: 'Enter', separator: true });
    items.push({ id: 'copyLink', label: t('action.copyLink'), icon: 'copy' });
    if (this.store.manualOrder()) {
      const order = this.store.queueOrder(row.d.queueId);
      const index = order.indexOf(row.d.id);
      items.push({ id: 'moveUp', label: t('queue.moveUp'), icon: 'arrowUp', shortcut: 'Alt ↑', separator: true, disabled: index <= 0 });
      items.push({ id: 'moveDown', label: t('queue.moveDown'), icon: 'arrowDown', shortcut: 'Alt ↓', disabled: index < 0 || index >= order.length - 1 });
    }
    if (this.queues.queues().length > 1) {
      items.push({ id: 'moveToQueue', label: t('queue.moveTo'), icon: 'list', separator: !this.store.manualOrder() });
    }
    // starting over only means something once bytes exist or the download has ended
    if (row.downloaded > 0 || s === 'completed' || s === 'failed' || s === 'cancelled') {
      // a torrent is never started over: "restart" is a full re-check that keeps every verified piece
      items.push({ id: 'restart', label: t(row.d.kind === 'torrent' ? 'torrent.recheck' : 'action.restart'), icon: 'retry', separator: true });
    }
    if (s !== 'completed' && s !== 'cancelled') {
      items.push({ id: 'cancel', label: t('action.cancel'), icon: 'stop', separator: !(row.downloaded > 0 || s === 'failed') });
    }
    items.push({ id: 'remove', label: t('action.remove'), icon: 'trash', danger: true, shortcut: 'Del', separator: s === 'completed' || s === 'cancelled' });
    return items;
  });

  /** What the list is showing; a change of any part crossfades the viewport. */
  private readonly listKey = computed(
    () => `${this.store.view()}|${this.store.category() ?? ''}|${this.store.queue() ?? ''}|${this.store.query().trim().toLowerCase()}|${this.store.sortKey()}|${this.store.sortDir()}`,
  );
  private lastListKey: string | null = null;

  constructor() {
    this.columnWidths.set(loadColumnWidths(this.prefs));
    this.hiddenColumns.set(loadHiddenColumns(this.prefs));

    // the page's own width decides where the details drawer goes; measured, not guessed from the
    // window, because the sidebar's width varies
    afterNextRender(() => {
      const el = this.host.nativeElement;
      const drawer = Number.parseFloat(getComputedStyle(el).getPropertyValue('--drawer-width'));
      if (Number.isFinite(drawer) && drawer > 0) {
        this.drawerWidth.set(drawer);
      }
      this.hostWidth.set(el.clientWidth);
      if (typeof ResizeObserver !== 'undefined') {
        const ro = new ResizeObserver(() => this.hostWidth.set(el.clientWidth));
        ro.observe(el);
        this.destroyRef.onDestroy(() => ro.disconnect());
      }
    });

    // the first list to arrive is the baseline: nothing in it is "new"
    effect(() => {
      const loaded = this.engine.loaded();
      untracked(() => {
        if (loaded && this.freshBaseline() === null) {
          this.freshBaseline.set(this.engine.downloads().reduce((max, d) => Math.max(max, d.id), 0));
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
        this.settled.update((seen) => new Set([...seen, ...fresh]));
      }, FRESH_MS);
      onCleanup(() => clearTimeout(handle));
    });

    // the viewport element comes and goes (skeleton, empty state): listen to whichever is there.
    // Scroll events run outside the zone; a signal write schedules its own refresh, and only a
    // scroll that crosses a row boundary changes what the window computes.
    effect((onCleanup) => {
      const el = this.viewportRef()?.nativeElement;
      if (!el) {
        return;
      }
      const onScroll = (): void => {
        this.scrollTop.set(el.scrollTop);
        // widened columns scroll sideways: the header follows the rows
        const header = this.headerRef()?.nativeElement;
        if (header && header.scrollLeft !== el.scrollLeft) {
          header.scrollLeft = el.scrollLeft;
        }
      };
      this.zone.runOutsideAngular(() => el.addEventListener('scroll', onScroll, { passive: true }));
      onCleanup(() => el.removeEventListener('scroll', onScroll));
      if (typeof ResizeObserver !== 'undefined') {
        const ro = new ResizeObserver(() => this.viewportHeight.set(el.clientHeight));
        ro.observe(el);
        onCleanup(() => ro.disconnect());
      }
      this.viewportHeight.set(el.clientHeight);
      this.scrollTop.set(el.scrollTop);
    });

    // Downloads > Remove (menu): the page owns the confirmation
    let seenRemoveRequests = this.ui.removeRequests();
    effect(() => {
      const n = this.ui.removeRequests();
      if (n > seenRemoveRequests) {
        seenRemoveRequests = n;
        untracked(() => {
          const rows = this.store.selectedRows();
          if (rows.length > 0) {
            this.askRemove(rows);
          }
        });
      }
    });

    effect(() => {
      const key = this.listKey();
      untracked(() => this.onListChanged(key));
    });
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
      case 'restart':
        if (d.kind === 'torrent') {
          // nothing is thrown away, so no confirmation
          this.run(this.engine.restart(d.id));
          this.toasts.show('info', this.i18n.t('torrent.rechecking').replace('{name}', d.filename));
        } else {
          this.restartTarget.set(d);
        }
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
      case 'moveToQueue': {
        const selected = this.store.selectedRows();
        this.moveTargets.set(selected.some((r) => r.d.id === d.id) ? selected.map((r) => r.d) : [d]);
        break;
      }
      case 'moveUp':
        this.moveInQueue(d, -1);
        break;
      case 'moveDown':
        this.moveInQueue(d, 1);
        break;
    }
  }

  /** The "Move to queue" dialog chose a queue. */
  protected confirmMove(queueId: number): void {
    const targets = this.moveTargets();
    this.moveTargets.set(null);
    if (targets === null) {
      return;
    }
    const queue = this.queues.get(queueId);
    const name = queueId === MAIN_QUEUE_ID ? this.i18n.t('queue.main') : (queue?.name ?? '');
    const ids = targets.map((d) => d.id);
    this.engine.moveToQueue(ids, queueId).then(
      () =>
        this.toasts.show(
          'info',
          ids.length === 1
            ? this.i18n.t('toast.queueMoved').replace('{queue}', name)
            : this.i18n.t('toast.queueMovedMany').replace('{n}', this.format.int(ids.length)).replace('{queue}', name),
        ),
      (error: unknown) => this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error))),
    );
  }

  /** Moves a download one place in its queue's manual order (keyboard and menu). */
  private moveInQueue(d: Download, delta: 1 | -1): void {
    const order = this.store.queueOrder(d.queueId);
    const next = moveBy(order, d.id, delta);
    if (!sameOrder(order, next)) {
      this.run(this.engine.reorderQueue(d.queueId, next));
    }
  }

  protected onRowDragStart(event: DragEvent, row: DownloadRow): void {
    if (event.dataTransfer === null) {
      return;
    }
    const selected = this.store.selected();
    this.dragged = selected.has(row.d.id) ? [...selected] : [row.d.id];
    event.dataTransfer.setData(DOWNLOAD_DRAG_TYPE, JSON.stringify(this.dragged));
    event.dataTransfer.effectAllowed = 'copyMove';
  }

  protected onRowDragOver(event: DragEvent, row: DownloadRow): void {
    if (!this.store.manualOrder() || !dragHas(event, DOWNLOAD_DRAG_TYPE) || this.dragged.includes(row.d.id)) {
      return;
    }
    // only rows of this queue can be reordered; anything else is left to the sidebar
    event.preventDefault();
    this.rowMark.set({ id: row.d.id, after: inLowerHalf(event, event.currentTarget as HTMLElement) });
  }

  protected onRowDragLeave(row: DownloadRow): void {
    if (this.rowMark()?.id === row.d.id) {
      this.rowMark.set(null);
    }
  }

  protected onRowDrop(event: DragEvent, row: DownloadRow): void {
    const mark = this.rowMark();
    const moving = this.dragged;
    this.rowDragEnd();
    if (!this.store.manualOrder() || mark === null || mark.id !== row.d.id) {
      return;
    }
    event.preventDefault();
    const queueId = row.d.queueId;
    const order = this.store.queueOrder(queueId);
    const next = moveManyBeside(order, moving, row.d.id, mark.after);
    if (!sameOrder(order, next)) {
      this.run(this.engine.reorderQueue(queueId, next));
    }
  }

  protected rowDragEnd(): void {
    this.dragged = [];
    this.rowMark.set(null);
  }

  protected toggleQueue(id: number, running: boolean): void {
    this.run(this.queues.setRunning(id, !running));
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

  /** The user confirmed: the engine throws the progress away and the row follows its events. */
  protected confirmRestart(): void {
    const d = this.restartTarget();
    if (d === null) {
      return;
    }
    this.restartTarget.set(null);
    this.run(this.engine.restart(d.id));
    this.toasts.show('info', this.i18n.t('toast.restarted').replace('{name}', d.filename));
  }

  /** Drag a column's grip: the column follows the pointer (mirrored in RTL), saved on release. */
  protected startResize(event: PointerEvent, key: ColumnKey): void {
    if (event.button !== 0) {
      return;
    }
    const grip = event.currentTarget as HTMLElement;
    const cell = grip.parentElement;
    if (cell === null) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = cell.getBoundingClientRect().width;
    const sign = getComputedStyle(cell).direction === 'rtl' ? -1 : 1;
    grip.setPointerCapture?.(event.pointerId);
    this.resizing.set(key);
    const move = (e: PointerEvent): void => this.setColumnWidth(key, startWidth + (e.clientX - startX) * sign);
    const end = (): void => {
      grip.removeEventListener('pointermove', move);
      grip.removeEventListener('pointerup', end);
      grip.removeEventListener('pointercancel', end);
      this.resizing.set(null);
      saveColumnWidths(this.prefs, this.columnWidths());
    };
    this.zone.runOutsideAngular(() => {
      grip.addEventListener('pointermove', move);
      grip.addEventListener('pointerup', end);
      grip.addEventListener('pointercancel', end);
    });
  }

  /** Arrow keys on a focused grip widen or narrow the column; Enter or Delete restores it. */
  protected onResizeKey(event: KeyboardEvent, key: ColumnKey): void {
    const grip = event.currentTarget as HTMLElement;
    const cell = grip.parentElement;
    if (cell === null) {
      return;
    }
    if (event.key === 'Enter' || event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      this.resetColumn(key);
      return;
    }
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') {
      return;
    }
    event.preventDefault();
    const rtl = getComputedStyle(cell).direction === 'rtl';
    const wider = (event.key === 'ArrowRight') !== rtl;
    const step = RESIZE_STEP * (event.shiftKey ? 4 : 1) * (wider ? 1 : -1);
    this.setColumnWidth(key, cell.getBoundingClientRect().width + step);
    saveColumnWidths(this.prefs, this.columnWidths());
  }

  /** Back to the column's responsive default width. */
  protected resetColumn(key: ColumnKey): void {
    const next: Partial<Record<ColumnKey, number>> = { ...this.columnWidths() };
    delete next[key];
    this.columnWidths.set(next);
    saveColumnWidths(this.prefs, next);
  }

  protected onHeaderContext(event: MouseEvent): void {
    event.preventDefault();
    this.headerMenu.set({ x: event.clientX, y: event.clientY });
  }

  protected onHeaderMenu(id: string): void {
    this.headerMenu.set(null);
    if (id === 'resetWidths') {
      this.columnWidths.set({});
      saveColumnWidths(this.prefs, {});
      return;
    }
    const key = OPTIONAL_COLUMNS.find((k) => id === `toggle:${k}`);
    if (key !== undefined) {
      const next = new Set(this.hiddenColumns());
      if (!next.delete(key)) {
        next.add(key);
      }
      this.hiddenColumns.set(next);
      saveHiddenColumns(this.prefs, next);
      // a column sorted on and then hidden would leave an order nobody can see
      if (next.has(key) && this.store.sortKey() === key) {
        this.store.sortBy('added');
      }
    }
  }

  private setColumnWidth(key: ColumnKey, px: number): void {
    const width = clampWidth(key, px);
    if (this.columnWidths()[key] !== width) {
      this.columnWidths.update((w) => ({ ...w, [key]: width }));
    }
  }

  protected onKey(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (target?.closest('input, textarea, select, [contenteditable="true"]')) {
      return;
    }
    if (event.key === 'Escape') {
      if (this.menu().open) {
        this.closeMenu();
      } else if (this.store.detailsId() !== null) {
        this.store.closeDetails();
      } else {
        this.store.clearSelection();
      }
      return;
    }
    // buttons and links inside the page (toolbar, headers, row actions, drawer) keep their own keys
    if (target !== null && target !== this.gridRef()?.nativeElement && target.closest('button, a, [role="menuitem"], [role="separator"], app-download-details')) {
      return;
    }
    const mod = event.ctrlKey || event.metaKey;
    if (mod && (event.key === 'a' || event.key === 'A')) {
      event.preventDefault();
      this.store.selectAll();
      return;
    }
    const selected = this.store.selectedRows();
    const first = selected[0];
    const nav = navFromKey(event.key);
    if (nav !== null && !mod) {
      event.preventDefault();
      if (event.altKey) {
        // Alt + arrow: move the selected download in its queue (manual order only)
        if (first && this.store.manualOrder() && (nav === 'next' || nav === 'prev')) {
          this.moveInQueue(first.d, nav === 'next' ? 1 : -1);
        }
        return;
      }
      const landed = this.store.navigate(nav, this.pageSize(), event.shiftKey);
      if (landed !== null) {
        this.revealIndex(landed.index);
      }
      return;
    }
    switch (event.key) {
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
    }
  }

  /** Rows that fit the viewport, for PageUp / PageDown. */
  private pageSize(): number {
    return Math.max(1, Math.floor(this.viewportHeight() / this.rowHeight()));
  }

  /** Scrolls the viewport just far enough to show row `index` (the row need not be in the DOM). */
  private revealIndex(index: number): void {
    const el = this.viewportRef()?.nativeElement;
    if (!el) {
      return;
    }
    const to = scrollToReveal(index, this.rowHeight(), el.scrollTop, el.clientHeight);
    if (to !== null) {
      el.scrollTop = to;
      this.scrollTop.set(el.scrollTop);
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
