import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, ElementRef, inject, input, NgZone, output, signal, untracked, viewChild } from '@angular/core';

import { FORMAT_PIPES } from '../../core/format.pipes';
import { FormatService } from '../../core/format.service';
import { IS_MAC } from '../../core/platform';
import { scrollToReveal } from '../../data/list-nav';
import { I18nService } from '../../i18n/i18n.service';
import { categoryOf } from '../../model/download';
import { FileNameComponent } from '../../ui/file-name.component';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { EmptyStateComponent } from '../../ui/empty-state.component';
import { IconComponent, IconName } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { CATEGORY_ICONS } from '../downloads/download-row.component';
import { Entry, FILE_BACKEND, Place, Properties, TrashedItem } from './file-backend';
import { FileOpsService, parentPath } from './file-ops.service';
import { FOLDER_COLORS, FolderColor, FolderColorsService } from './folder-colors.service';

/** Sortable columns. */
export type FileSortKey = 'name' | 'type' | 'size' | 'modified';

/** Row height of the file list in px. */
const ROW_PX = 36;
/** Rows rendered beyond the viewport on each side. */
const OVERSCAN = 8;
/** Drag payload type for items dragged out of a pane. */
export const FILES_DRAG_TYPE = 'application/x-riptide-files';

const COLLATOR = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Extension in upper case, `''` for none. */
function extOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toUpperCase() : '';
}

/** Compares two entries by `key`; folders always come first. */
export function compareEntries(a: Entry, b: Entry, key: FileSortKey): number {
  const folders = Number(a.kind !== 'folder') - Number(b.kind !== 'folder');
  if (folders !== 0) {
    return folders;
  }
  switch (key) {
    case 'name':
      return COLLATOR.compare(a.name, b.name);
    case 'type':
      return COLLATOR.compare(extOf(a.name), extOf(b.name)) || COLLATOR.compare(a.name, b.name);
    case 'size':
      return (a.size ?? -1) - (b.size ?? -1) || COLLATOR.compare(a.name, b.name);
    case 'modified':
      return (a.modified ?? 0) - (b.modified ?? 0) || COLLATOR.compare(a.name, b.name);
  }
}

/**
 * One file-manager pane: a folder with history and breadcrumbs, a sortable virtual list,
 * filter-as-you-type plus a deeper search, rename in place, a context menu, keyboard
 * shortcuts, folder colours, properties and the Recycle Bin for this folder. Clipboard,
 * transfers and conflicts are shared through `FileOpsService`, so two panes work together.
 */
@Component({
  selector: 'app-file-pane',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES, FileNameComponent, ButtonComponent, DialogComponent, EmptyStateComponent, IconComponent, MenuComponent],
  host: {
    '[class.is-active]': 'active()',
    '(focusin)': 'focused.emit()',
    '(pointerdown)': 'focused.emit()',
  },
  template: `
    <div class="bar">
      <div class="nav">
        <button appButton size="sm" variant="ghost" icon="chevronStart" [iconOnly]="true" type="button" [disabled]="back().length === 0" [attr.aria-label]="i18n.t('files.back')" [attr.title]="i18n.t('files.back')" (click)="goBack()"></button>
        <button appButton size="sm" variant="ghost" icon="chevronEnd" [iconOnly]="true" type="button" [disabled]="forward().length === 0" [attr.aria-label]="i18n.t('files.forward')" [attr.title]="i18n.t('files.forward')" (click)="goForward()"></button>
        <button appButton size="sm" variant="ghost" icon="arrowUp" [iconOnly]="true" type="button" [disabled]="!listing()?.parent" [attr.aria-label]="i18n.t('files.up')" [attr.title]="i18n.t('files.up')" (click)="goUp()"></button>
      </div>
      <nav class="crumbs" [attr.aria-label]="i18n.t('files.location')">
        @for (c of shownCrumbs(); track c.path; let last = $last) {
          @if (c.more) {
            <button type="button" class="crumb crumb-more" [attr.aria-label]="i18n.t('files.crumbMore')" [attr.title]="i18n.t('files.crumbMore')" aria-haspopup="menu" (click)="openCrumbMenu($event)">…</button>
          } @else {
            <button type="button" class="crumb" [class.is-current]="last" [attr.aria-current]="last ? 'location' : null" [attr.title]="last ? currentPath() : c.path" (click)="navigate(c.path)" (dragover)="onDragOver($event, c.path)" (drop)="onDrop($event, c.path)">
              <span dir="auto">{{ c.label }}</span>
            </button>
          }
          @if (!last) {
            <app-icon class="crumb-sep" name="chevronEnd" [size]="12" />
          }
        }
      </nav>
      <label class="search">
        <app-icon name="search" [size]="14" />
        <input
          type="search"
          [placeholder]="i18n.t('files.searchHere')"
          [attr.aria-label]="i18n.t('files.searchHere')"
          [value]="filter()"
          (input)="onFilter($event)"
          (keydown.enter)="deepSearch()"
          (keydown.escape)="clearSearch()"
        />
      </label>
    </div>

    <div class="actions">
      <button appButton size="sm" variant="ghost" icon="plus" type="button" (click)="startNewFolder()">{{ i18n.t('files.newFolder') }}</button>
      <button appButton size="sm" variant="ghost" icon="copy" type="button" [disabled]="ops.clipboard() === null" (click)="paste()">{{ i18n.t('files.paste') }}</button>
      <button appButton size="sm" variant="ghost" icon="trash" type="button" (click)="openBin()">{{ i18n.t(ops.platformKey('files.recycleBin')) }}</button>
      <span class="spacer"></span>
      @if (searchResults() !== null) {
        <span class="hint">{{ i18n.t('files.searchResults').replace('{n}', format.int(rows().length)) }}</span>
        <button appButton size="sm" variant="ghost" icon="close" type="button" (click)="clearSearch()">{{ i18n.t('files.clearSearch') }}</button>
      } @else if (listing(); as l) {
        <span class="hint">{{ i18n.plural('files.itemCount', l.entries.length).replace('{n}', format.int(l.entries.length)) }}</span>
      }
    </div>

    <div class="grid" role="grid" tabindex="0" #grid [attr.aria-label]="listing()?.path ?? ''" [attr.aria-rowcount]="rows().length + 1" [attr.aria-multiselectable]="true" [attr.aria-activedescendant]="cursorId()" (keydown)="onKey($event)" [class.is-drop-here]="dropHere()" (contextmenu)="onBackgroundContext($event)" (dragover)="onDragOver($event, currentPath())" (dragleave)="onDragLeave($event)" (drop)="onDrop($event, currentPath())">
      <div class="head" role="row">
        @for (c of columns; track c.key) {
          <button type="button" role="columnheader" [class]="'th th-' + c.key" [class.is-sorted]="sortKey() === c.key" [attr.aria-sort]="sortKey() === c.key ? (sortDir() === 'asc' ? 'ascending' : 'descending') : 'none'" (click)="sortBy(c.key)">
            {{ i18n.t(c.label) }}
            @if (sortKey() === c.key) {
              <app-icon name="arrowDown" [size]="12" [class.is-asc]="sortDir() === 'asc'" class="sort-icon" />
            }
          </button>
        }
      </div>

      @if (error(); as e) {
        <app-empty-state icon="alert" [heading]="i18n.t('files.cannotOpen')" [body]="e">
          <button appButton variant="secondary" type="button" (click)="reload()">{{ i18n.t('files.tryAgain') }}</button>
        </app-empty-state>
      } @else if (loading() && listing() === null) {
        <div class="loading" role="status">{{ i18n.t('files.loading') }}</div>
      } @else if (rows().length === 0 && !creating()) {
        <app-empty-state
          [icon]="filter() || searchResults() !== null ? 'search' : 'folderOpen'"
          [heading]="i18n.t(filter() || searchResults() !== null ? 'files.noMatches' : 'files.emptyFolder')"
          [body]="filter() && searchResults() === null ? i18n.t('files.deepSearchHint') : null"
        />
      } @else {
        <div class="viewport" #viewport role="presentation">
          @if (creating()) {
            <div class="row is-editing" role="row">
              <span class="cell c-name"><app-icon name="folder" [size]="16" class="folder-icon" /><input class="rename" type="text" #newName [value]="i18n.t('files.newFolderName')" (keydown.enter)="commitNewFolder(newName.value)" (keydown.escape)="creating.set(false)" (blur)="commitNewFolder(newName.value)" /></span>
            </div>
          }
          <div [style.block-size.px]="win().top"></div>
          @for (e of win().rows; track e.path; let i = $index) {
            <div
              role="row"
              class="row"
              [id]="rowId(e)"
              [attr.aria-rowindex]="win().start + i + 2"
              [attr.aria-selected]="selected().has(e.path)"
              [class.is-selected]="selected().has(e.path)"
              [class.is-cursor]="cursor() === e.path"
              [class.is-cut]="isCut(e.path)"
              [class.is-drop]="dropTarget() === e.path"
              [class.is-hidden]="e.hidden"
              tabindex="-1"
              draggable="true"
              (click)="onRowClick($event, e)"
              (keydown.enter)="activate(e)"
              (dblclick)="activate(e)"
              (contextmenu)="onRowContext($event, e)"
              (dragstart)="onDragStart($event, e)"
              (dragover)="e.kind === 'folder' ? onDragOver($event, e.path) : null"
              (dragleave)="dropTarget.set(null)"
              (drop)="e.kind === 'folder' ? onDrop($event, e.path) : null"
            >
              <span class="cell c-name" role="gridcell">
                <app-icon [name]="iconOf(e)" [size]="16" [class.folder-icon]="e.kind === 'folder'" [attr.data-color]="colorOf(e)" />
                @if (renaming() === e.path) {
                  <input class="rename" type="text" #renameBox [value]="e.name" [attr.aria-label]="i18n.t('files.rename')" (keydown.enter)="commitRename(e, renameBox.value)" (keydown.escape)="renaming.set(null)" (blur)="commitRename(e, renameBox.value)" (click)="$event.stopPropagation()" />
                } @else {
                  <app-file-name class="name" [name]="e.name" [label]="searchResults() !== null ? e.path : e.name" />
                  @if (searchResults() !== null) {
                    <span class="where" dir="ltr">{{ parentOf(e.path) }}</span>
                  }
                }
              </span>
              <span class="cell c-modified num" role="gridcell" [attr.title]="e.modified === null ? null : (e.modified | dateTime)">{{ e.modified === null ? '—' : (e.modified | relTime) }}</span>
              <span class="cell c-type" role="gridcell">{{ typeOf(e) }}</span>
              <span class="cell c-size num" role="gridcell">{{ e.kind === 'file' ? (e.size | bytes) : '' }}</span>
            </div>
          }
          <div [style.block-size.px]="win().bottom"></div>
        </div>
      }
    </div>

    <aside class="places" [attr.aria-label]="i18n.t('files.places')">
      @for (p of places(); track p.path) {
        <button type="button" class="place" [class.is-current]="p.path === currentPath()" (click)="navigate(p.path)" (dragover)="onDragOver($event, p.path)" (drop)="onDrop($event, p.path)">
          <app-icon [name]="p.key === 'drive' ? 'monitor' : p.key === 'organized' ? 'layers' : 'folder'" [size]="14" />
          <span dir="auto">{{ p.key === 'drive' ? p.label : i18n.t('files.place.' + p.key) }}</span>
        </button>
      }
    </aside>

    <app-menu [open]="crumbMenu() !== null" [items]="crumbMenuItems()" [x]="crumbMenu()?.x ?? 0" [y]="crumbMenu()?.y ?? 0" (selected)="navigate($event)" (closed)="crumbMenu.set(null)" />

    <app-menu [open]="menu() !== null" [items]="menuItems()" [x]="menu()?.x ?? 0" [y]="menu()?.y ?? 0" (selected)="onMenu($event)" (closed)="menu.set(null)" />

    @if (palette(); as p) {
      <div class="palette" role="dialog" [attr.aria-label]="i18n.t('files.folderColor')" [style.inset-inline-start.px]="p.x" [style.inset-block-start.px]="p.y" (keydown.escape)="palette.set(null)">
        @for (c of colors; track c) {
          <button type="button" class="swatch" [attr.data-color]="c" [class.is-current]="colorOf(p.entry) === c" [attr.aria-label]="i18n.t('files.color.' + c)" [attr.title]="i18n.t('files.color.' + c)" (click)="setColor(p.entry, c)"></button>
        }
        <button type="button" class="swatch is-reset" [attr.aria-label]="i18n.t('files.color.reset')" [attr.title]="i18n.t('files.color.reset')" (click)="setColor(p.entry, null)"><app-icon name="close" [size]="12" /></button>
      </div>
    }

    <app-dialog [open]="props() !== null" [title]="i18n.t('files.properties')" (closed)="props.set(null)">
      @if (props(); as p) {
        <dl class="props">
          <dt>{{ i18n.t('files.col.name') }}</dt><dd dir="auto">{{ p.entry.name }}</dd>
          <dt>{{ i18n.t('files.location') }}</dt><dd dir="ltr" class="mono">{{ parentOf(p.entry.path) }}</dd>
          <dt>{{ i18n.t('files.col.type') }}</dt><dd>{{ typeOf(p.entry) }}</dd>
          <dt>{{ i18n.t('files.col.size') }}</dt><dd class="num">{{ p.total_size | bytes }}{{ p.truncated ? '+' : '' }}</dd>
          @if (p.files !== null) {
            <dt>{{ i18n.t('files.contains') }}</dt><dd class="num">{{ i18n.t('files.containsValue').replace('{files}', format.int(p.files)).replace('{folders}', format.int(p.folders ?? 0)) }}</dd>
          }
          @if (p.entry.created !== null) {
            <dt>{{ i18n.t('files.created') }}</dt><dd class="num">{{ p.entry.created | dateTime }}</dd>
          }
          @if (p.entry.modified !== null) {
            <dt>{{ i18n.t('files.col.modified') }}</dt><dd class="num">{{ p.entry.modified | dateTime }}</dd>
          }
          <dt>{{ i18n.t('files.attributes') }}</dt><dd>{{ attributes(p.entry) }}</dd>
        </dl>
      }
    </app-dialog>

    <app-dialog [open]="bin() !== null" [title]="i18n.t(ops.platformKey('files.recycleBin'))" (closed)="bin.set(null)">
      @if (bin(); as items) {
        @if (!ops.canRestore()) {
          <p class="bin-empty">{{ i18n.t('files.trashInFinder') }}</p>
        } @else if (items.length === 0) {
          <p class="bin-empty">{{ i18n.t(ops.platformKey('files.binEmpty')) }}</p>
        } @else {
          <ul class="bin">
            @for (t of items; track t.id) {
              <li>
                <span class="bin-name" dir="auto">{{ t.name }}</span>
                <span class="bin-when num">{{ t.deleted_at | relTime }}</span>
                <button appButton size="sm" variant="secondary" icon="retry" type="button" (click)="restore(t)">{{ i18n.t('files.restore') }}</button>
              </li>
            }
          </ul>
        }
      }
    </app-dialog>
  `,
  styles: `
    :host {
      --folder-red: #d64545;
      --folder-orange: #e07a2e;
      --folder-amber: #c99a06;
      --folder-green: #2f9e5b;
      --folder-teal: #14a39a;
      --folder-blue: #3b7ddd;
      --folder-violet: #8a5cd6;
      --folder-pink: #d4558f;
      --folder-slate: #6b7a8c;
      position: relative;
      display: grid;
      grid-template-columns: auto minmax(0, 1fr);
      container-type: inline-size;
      container-name: pane;
      grid-template-rows: auto auto minmax(0, 1fr);
      grid-template-areas: 'places bar' 'places actions' 'places grid';
      min-block-size: 0;
      min-inline-size: 0;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1);
      overflow: hidden;
    }

    :host(.is-active) {
      border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }

    .bar {
      grid-area: bar;
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding: var(--space-2) var(--space-3);
      border-block-end: 1px solid var(--color-border);
      min-inline-size: 0;
    }

    .nav {
      display: flex;
      flex: none;
    }

    .crumbs {
      display: flex;
      align-items: center;
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      gap: 2px;
      font-size: var(--text-sm);
    }

    /* when the path does not fit, the outer folders shrink first; the one you are in never does */
    .crumb {
      flex: 0 1 auto;
      min-inline-size: 36px;
      max-inline-size: 140px;
      overflow: hidden;
      padding: 2px 6px;
      border: 0;
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      text-overflow: ellipsis;
      white-space: nowrap;
      cursor: pointer;
    }

    .crumb:hover {
      background: var(--color-surface-hover);
      color: var(--color-text);
    }

    .crumb-more {
      flex: none;
      min-inline-size: 0;
    }

    .crumb.is-current {
      flex: 0 0 auto;
      min-inline-size: 12ch;
      max-inline-size: none;
      color: var(--color-text);
      font-weight: var(--weight-medium);
    }

    .crumb-sep {
      flex: none;
      color: var(--color-text-muted);
    }

    .search {
      display: flex;
      align-items: center;
      gap: 6px;
      flex: 0 1 180px;
      min-inline-size: 110px;
      block-size: 28px;
      padding-inline: 8px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-muted);
    }

    .search input {
      flex: 1;
      min-inline-size: 0;
      border: 0;
      background: transparent;
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
      outline: none;
    }

    .actions {
      grid-area: actions;
      display: flex;
      align-items: center;
      gap: var(--space-1);
      padding: var(--space-1) var(--space-2);
      border-block-end: 1px solid var(--color-border);
    }

    .spacer {
      flex: 1;
    }

    .hint {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      white-space: nowrap;
    }

    .places {
      grid-area: places;
      inline-size: 168px;
      display: flex;
      flex-direction: column;
      gap: 1px;
      padding: var(--space-2);
      border-inline-end: 1px solid var(--color-border);
      overflow-y: auto;
    }

    .place {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding: 6px 8px;
      border: 0;
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-sm);
      text-align: start;
      cursor: pointer;
    }

    .place span {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .place:hover {
      background: var(--color-surface-hover);
      color: var(--color-text);
    }

    .place.is-current {
      background: var(--color-accent-soft);
      color: var(--color-text);
    }

    .grid {
      grid-area: grid;
      display: grid;
      grid-template-rows: auto minmax(0, 1fr);
      min-block-size: 0;
      outline: none;
      --file-columns: minmax(180px, 1fr) 132px 112px 96px;
    }

    .head,
    .row {
      display: grid;
      grid-template-columns: var(--file-columns);
      align-items: center;
      padding-inline: var(--space-3);
    }

    .head {
      block-size: var(--header-height);
      border-block-end: 1px solid var(--color-border);
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
      font: inherit;
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      cursor: pointer;
      white-space: nowrap;
    }

    .th-name {
      padding-inline-start: 0;
    }

    .th-size {
      justify-content: flex-end;
    }

    .th.is-sorted {
      color: var(--color-text);
    }

    .sort-icon {
      color: var(--color-accent-fg);
    }

    .sort-icon.is-asc {
      transform: rotate(180deg);
    }

    .viewport {
      overflow-y: auto;
      scrollbar-gutter: stable;
    }

    .row {
      block-size: 36px;
      border-block-end: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
      font-size: var(--text-sm);
      cursor: default;
      user-select: none;
    }

    .row:hover {
      background: var(--color-surface-hover);
    }

    .row.is-selected {
      background: var(--color-accent-soft);
    }

    .grid:focus-visible .row.is-cursor {
      outline: 2px solid var(--color-focus, var(--color-accent));
      outline-offset: -2px;
    }

    .row.is-cut {
      opacity: 0.55;
    }

    .row.is-hidden .name {
      color: var(--color-text-muted);
    }

    .grid.is-drop-here .viewport {
      box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--color-accent) 60%, transparent);
      background: color-mix(in srgb, var(--color-accent) 5%, transparent);
    }

    .row.is-drop {
      box-shadow: inset 0 0 0 2px var(--color-accent);
    }

    .cell {
      min-inline-size: 0;
      padding-inline: var(--space-2);
      overflow: hidden;
      color: var(--color-text-secondary);
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .c-name {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding-inline-start: 0;
      color: var(--color-text);
    }

    .c-size {
      text-align: end;
    }

    .name {
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .where {
      overflow: hidden;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      text-overflow: ellipsis;
    }

    .folder-icon {
      color: var(--color-accent);
    }

    [data-color='red'] { color: var(--folder-red); }
    [data-color='orange'] { color: var(--folder-orange); }
    [data-color='amber'] { color: var(--folder-amber); }
    [data-color='green'] { color: var(--folder-green); }
    [data-color='teal'] { color: var(--folder-teal); }
    [data-color='blue'] { color: var(--folder-blue); }
    [data-color='violet'] { color: var(--folder-violet); }
    [data-color='pink'] { color: var(--folder-pink); }
    [data-color='slate'] { color: var(--folder-slate); }

    .rename {
      flex: 1;
      min-inline-size: 0;
      block-size: 26px;
      padding-inline: 6px;
      border: 1px solid var(--color-accent);
      border-radius: var(--radius-sm);
      background: var(--color-surface-1);
      color: var(--color-text);
      font: inherit;
    }

    .loading {
      padding: var(--space-6);
      color: var(--color-text-muted);
      text-align: center;
    }

    .palette {
      position: fixed;
      z-index: var(--z-overlay);
      display: grid;
      grid-template-columns: repeat(5, 24px);
      gap: 6px;
      padding: var(--space-2);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1);
      box-shadow: var(--shadow-2);
    }

    .swatch {
      inline-size: 24px;
      block-size: 24px;
      border: 2px solid transparent;
      border-radius: 50%;
      background: currentcolor;
      cursor: pointer;
    }

    .swatch.is-current {
      border-color: var(--color-text);
    }

    .swatch.is-reset {
      display: grid;
      place-items: center;
      border-color: var(--color-border-strong);
      background: transparent;
      color: var(--color-text-secondary);
    }

    .props {
      display: grid;
      grid-template-columns: max-content minmax(0, 1fr);
      gap: var(--space-2) var(--space-4);
      font-size: var(--text-sm);
    }

    .props dt {
      color: var(--color-text-muted);
    }

    .props dd {
      overflow-wrap: anywhere;
    }

    .mono {
      font-family: var(--font-mono, monospace);
      font-size: var(--text-xs);
    }

    .bin {
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
      max-block-size: 50vh;
      overflow-y: auto;
      list-style: none;
    }

    .bin li {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }

    .bin-name {
      flex: 1;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .bin-when,
    .bin-empty {
      color: var(--color-text-muted);
      font-size: var(--text-sm);
    }

    /* Columns follow the width of this pane, not of the page: two panes share the window. */
    @container pane (max-width: 759px) {
      .grid {
        --file-columns: minmax(120px, 1fr) 96px 104px;
      }

      .c-type,
      .th-type {
        display: none;
      }
    }

    /* a narrow pane gives the path the whole first line and moves the search below it */
    @container pane (max-width: 679px) {
      .bar {
        flex-wrap: wrap;
      }

      .crumbs {
        flex: 1 1 0;
      }

      .search {
        order: 3;
        flex: 1 1 100%;
      }
    }

    @container pane (max-width: 559px) {
      .places {
        display: none;
      }
    }

    @container pane (max-width: 419px) {
      .grid {
        --file-columns: minmax(100px, 1fr) 84px;
      }

      .c-modified,
      .th-modified {
        display: none;
      }
    }
  `,
})
export class FilePaneComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly ops = inject(FileOpsService);
  private readonly backend = inject(FILE_BACKEND);
  private readonly folderColors = inject(FolderColorsService);
  private readonly isMac = inject(IS_MAC);
  private readonly zone = inject(NgZone);
  private readonly destroyRef = inject(DestroyRef);
  private readonly viewportRef = viewChild<ElementRef<HTMLElement>>('viewport');
  private readonly gridRef = viewChild<ElementRef<HTMLElement>>('grid');
  private readonly renameBox = viewChild<ElementRef<HTMLInputElement>>('renameBox');
  private readonly newNameBox = viewChild<ElementRef<HTMLInputElement>>('newName');

  /** Folder to open first; the first place when `null`. */
  readonly initialPath = input<string | null>(null);
  /** The pane the keyboard and Paste act on (dual pane). */
  readonly active = input(true);
  readonly focused = output<void>();
  /** The folder shown changed (for the page to remember it). */
  readonly pathChange = output<string>();

  protected readonly colors = FOLDER_COLORS;
  protected readonly columns: readonly { key: FileSortKey; label: string }[] = [
    { key: 'name', label: 'files.col.name' },
    { key: 'modified', label: 'files.col.modified' },
    { key: 'type', label: 'files.col.type' },
    { key: 'size', label: 'files.col.size' },
  ];

  protected readonly places = signal<readonly Place[]>([]);
  protected readonly listing = signal<{ path: string; parent: string | null; entries: readonly Entry[] } | null>(null);
  protected readonly loading = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly back = signal<readonly string[]>([]);
  protected readonly forward = signal<readonly string[]>([]);
  protected readonly sortKey = signal<FileSortKey>('name');
  protected readonly sortDir = signal<'asc' | 'desc'>('asc');
  protected readonly selected = signal<ReadonlySet<string>>(new Set());
  protected readonly cursor = signal<string | null>(null);
  private anchor: string | null = null;
  protected readonly filter = signal('');
  protected readonly searchResults = signal<readonly Entry[] | null>(null);
  protected readonly renaming = signal<string | null>(null);
  protected readonly creating = signal(false);
  protected readonly menu = signal<{ x: number; y: number; entry: Entry | null } | null>(null);
  protected readonly palette = signal<{ x: number; y: number; entry: Entry } | null>(null);
  protected readonly props = signal<Properties | null>(null);
  protected readonly bin = signal<readonly TrashedItem[] | null>(null);
  protected readonly dropTarget = signal<string | null>(null);
  /** Something droppable is over the list itself (it would land in the folder shown). */
  protected readonly dropHere = signal(false);
  private readonly scrollTop = signal(0);
  private readonly viewportHeight = signal(400);
  private searchId: number | null = null;
  private loadToken = 0;

  /** The folder shown. */
  readonly currentPath = computed(() => this.listing()?.path ?? '');

  /** The rows shown: the search results, or the folder filtered by the search box, sorted. */
  protected readonly rows = computed<readonly Entry[]>(() => {
    const results = this.searchResults();
    const base = results ?? this.listing()?.entries ?? [];
    const needle = results === null ? this.filter().trim().toLowerCase() : '';
    const filtered = needle.length === 0 ? base : base.filter((e) => e.name.toLowerCase().includes(needle));
    const key = this.sortKey();
    const sign = this.sortDir() === 'asc' ? 1 : -1;
    return [...filtered].sort((a, b) => {
      const folders = Number(a.kind !== 'folder') - Number(b.kind !== 'folder');
      if (folders !== 0) {
        return folders;
      }
      // folders have no size or type of their own: they stay in name order for those columns
      if (a.kind === 'folder' && (key === 'size' || key === 'type')) {
        return COLLATOR.compare(a.name, b.name);
      }
      return sign * compareEntries(a, b, key);
    });
  });

  /** DOM id of the keyboard row while it is rendered, for `aria-activedescendant`. */
  protected readonly cursorId = computed(() => {
    const c = this.cursor();
    const row = c === null ? undefined : this.win().rows.find((r) => r.path === c);
    return row === undefined ? null : this.rowId(row);
  });

  protected readonly win = computed(() => {
    const rows = this.rows();
    const start = Math.max(0, Math.floor(this.scrollTop() / ROW_PX) - OVERSCAN);
    const end = Math.min(rows.length, Math.ceil((this.scrollTop() + this.viewportHeight()) / ROW_PX) + OVERSCAN);
    return { rows: rows.slice(start, end), start, top: start * ROW_PX, bottom: Math.max(0, (rows.length - end) * ROW_PX) };
  });

  protected readonly crumbs = computed(() => {
    const path = this.currentPath();
    if (path === '') {
      return [];
    }
    const sep = path.includes('\\') ? '\\' : '/';
    const parts = path.split(/[\\/]/).filter((p) => p.length > 0);
    const out: { label: string; path: string }[] = [];
    let acc = '';
    parts.forEach((p, i) => {
      acc = i === 0 ? (sep === '\\' ? `${p}\\` : `/${p}`) : acc.endsWith(sep) ? `${acc}${p}` : `${acc}${sep}${p}`;
      out.push({ label: p, path: acc });
    });
    if (sep === '/') {
      out.unshift({ label: '/', path: '/' });
    }
    return out;
  });

  /** The path as shown: root and the last two folders stay, the middle collapses into one `…` item. */
  protected readonly shownCrumbs = computed<readonly { label: string; path: string; more: boolean }[]>(() => {
    const all = this.crumbs().map((c) => ({ ...c, more: false }));
    if (all.length <= 3) {
      return all;
    }
    return [all[0], { label: '…', path: '…', more: true }, ...all.slice(-2)];
  });

  protected readonly crumbMenu = signal<{ x: number; y: number } | null>(null);

  protected readonly crumbMenuItems = computed<readonly MenuItem[]>(() => {
    const all = this.crumbs();
    return all.length <= 3 ? [] : all.slice(1, -2).map((c) => ({ id: c.path, label: c.label, icon: 'folder' as const }));
  });

  protected openCrumbMenu(ev: MouseEvent): void {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    this.crumbMenu.set({ x: ev.clientX || r.left, y: r.bottom });
  }

  protected readonly menuItems = computed<readonly MenuItem[]>(() => {
    const m = this.menu();
    if (m === null) {
      return [];
    }
    const t = (k: string): string => this.i18n.t(k);
    const e = m.entry;
    if (e === null) {
      return [
        { id: 'newFolder', label: t('files.newFolder'), icon: 'plus', shortcut: 'Ctrl+Shift+N' },
        { id: 'paste', label: t('files.paste'), shortcut: 'Ctrl+V', disabled: this.ops.clipboard() === null },
        { id: 'refresh', label: t('files.refresh'), icon: 'retry', shortcut: 'F5', separator: true },
        { id: 'reveal', label: t(this.ops.platformKey('files.showInExplorer')), icon: 'external' },
      ];
    }
    const many = this.selected().size > 1;
    const items: MenuItem[] = [{ id: 'open', label: t('files.open'), icon: e.kind === 'folder' ? 'folderOpen' : 'external', shortcut: 'Enter', disabled: many }];
    if (this.searchResults() !== null) {
      items.push({ id: 'openLocation', label: t('files.openLocation'), icon: 'folder', disabled: many });
    }
    items.push({ id: 'reveal', label: t(this.ops.platformKey('files.showInExplorer')), icon: 'external', disabled: many });
    items.push({ id: 'copy', label: t('files.copy'), icon: 'copy', shortcut: 'Ctrl+C', separator: true });
    items.push({ id: 'cut', label: t('files.cut'), shortcut: 'Ctrl+X' });
    if (e.kind === 'folder') {
      items.push({ id: 'pasteInto', label: t('files.pasteInto'), disabled: this.ops.clipboard() === null || many });
    }
    items.push({ id: 'rename', label: t('files.rename'), shortcut: 'F2', separator: true, disabled: many });
    if (e.kind === 'folder' && !many) {
      items.push({ id: 'color', label: t('files.folderColor'), icon: 'sparkles' });
    }
    items.push({ id: 'properties', label: t('files.properties'), icon: 'info', disabled: many });
    items.push({ id: 'trash', label: t('files.delete'), icon: 'trash', danger: true, shortcut: 'Del', separator: true });
    return items;
  });

  constructor() {
    // the first folder comes from the input, which is only set once change detection has run:
    // an effect (not the constructor) starts the pane
    let started = false;
    effect(() => {
      const initial = this.initialPath();
      if (!started) {
        started = true;
        untracked(() => void this.start(initial));
      }
    });

    // reload the folder after any change made through the shared service (this pane or the other)
    let seenVersion = this.ops.version();
    effect(() => {
      const v = this.ops.version();
      if (v !== seenVersion) {
        seenVersion = v;
        untracked(() => void this.reload());
      }
    });

    effect((onCleanup) => {
      const el = this.viewportRef()?.nativeElement;
      if (!el) {
        return;
      }
      const onScroll = (): void => this.scrollTop.set(el.scrollTop);
      this.zone.runOutsideAngular(() => el.addEventListener('scroll', onScroll, { passive: true }));
      onCleanup(() => el.removeEventListener('scroll', onScroll));
      if (typeof ResizeObserver !== 'undefined') {
        const ro = new ResizeObserver(() => this.viewportHeight.set(el.clientHeight));
        ro.observe(el);
        onCleanup(() => ro.disconnect());
      }
      this.viewportHeight.set(el.clientHeight);
    });

    // the rename box takes the stem selected, like Explorer
    effect(() => {
      const box = this.renameBox()?.nativeElement;
      if (box) {
        box.focus();
        const dot = box.value.lastIndexOf('.');
        box.setSelectionRange(0, dot > 0 ? dot : box.value.length);
      }
    });
    effect(() => {
      const box = this.newNameBox()?.nativeElement;
      if (box) {
        box.focus();
        box.select();
      }
    });

    this.destroyRef.onDestroy(() => {
      if (this.searchId !== null) {
        void this.backend.cancel(this.searchId);
      }
    });
  }

  /** Moves keyboard focus into the list. */
  focusList(): void {
    this.gridRef()?.nativeElement.focus();
  }

  /** Opens a folder, recording it in the history. */
  navigate(path: string, record = true): void {
    const from = this.listing()?.path;
    if (record && from !== undefined && from !== path) {
      this.back.update((b) => [...b, from].slice(-50));
      this.forward.set([]);
    }
    void this.load(path);
  }

  protected goBack(): void {
    const b = this.back();
    const to = b.at(-1);
    if (to === undefined) {
      return;
    }
    this.back.set(b.slice(0, -1));
    const from = this.listing()?.path;
    if (from !== undefined) {
      this.forward.update((f) => [...f, from]);
    }
    void this.load(to);
  }

  protected goForward(): void {
    const f = this.forward();
    const to = f.at(-1);
    if (to === undefined) {
      return;
    }
    this.forward.set(f.slice(0, -1));
    const from = this.listing()?.path;
    if (from !== undefined) {
      this.back.update((b) => [...b, from]);
    }
    void this.load(to);
  }

  protected goUp(): void {
    const parent = this.listing()?.parent;
    if (parent) {
      const child = this.listing()?.path ?? null;
      this.navigate(parent);
      this.cursor.set(child);
    }
  }

  async reload(): Promise<void> {
    const path = this.listing()?.path;
    if (path !== undefined) {
      await this.load(path, true);
      if (this.searchResults() !== null) {
        await this.deepSearch();
      }
    }
  }

  protected sortBy(key: FileSortKey): void {
    if (this.sortKey() === key) {
      this.sortDir.update((d) => (d === 'asc' ? 'desc' : 'asc'));
    } else {
      this.sortKey.set(key);
      this.sortDir.set(key === 'modified' || key === 'size' ? 'desc' : 'asc');
    }
  }

  protected onFilter(event: Event): void {
    this.filter.set((event.target as HTMLInputElement).value);
    if (this.searchResults() !== null) {
      this.searchResults.set(null);
    }
  }

  /** Enter in the search box: look through every subfolder too. */
  async deepSearch(): Promise<void> {
    const query = this.filter().trim();
    const dir = this.listing()?.path;
    if (query.length === 0 || dir === undefined) {
      return;
    }
    if (this.searchId !== null) {
      void this.backend.cancel(this.searchId);
    }
    const id = this.ops.newId();
    this.searchId = id;
    this.loading.set(true);
    try {
      const found = await this.backend.search(id, dir, query);
      if (this.searchId === id) {
        this.searchResults.set(found);
      }
    } catch {
      // cancelled by a newer search, or the folder went away
    } finally {
      if (this.searchId === id) {
        this.searchId = null;
        this.loading.set(false);
      }
    }
  }

  protected clearSearch(): void {
    this.filter.set('');
    this.searchResults.set(null);
    this.gridRef()?.nativeElement.focus();
  }

  protected onRowClick(event: MouseEvent, e: Entry): void {
    this.gridRef()?.nativeElement.focus();
    const rows = this.rows();
    if (event.shiftKey && this.anchor !== null) {
      const a = rows.findIndex((r) => r.path === this.anchor);
      const b = rows.findIndex((r) => r.path === e.path);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        this.selected.set(new Set(rows.slice(lo, hi + 1).map((r) => r.path)));
      }
    } else if (event.ctrlKey || event.metaKey) {
      const next = new Set(this.selected());
      if (!next.delete(e.path)) {
        next.add(e.path);
      }
      this.selected.set(next);
      this.anchor = e.path;
    } else {
      this.selected.set(new Set([e.path]));
      this.anchor = e.path;
    }
    this.cursor.set(e.path);
  }

  /** Double-click or Enter: into a folder, or open a file with its application. */
  protected activate(e: Entry): void {
    if (e.kind === 'folder') {
      this.clearSearchState();
      this.navigate(e.path);
    } else {
      this.ops.open(e.path);
    }
  }

  protected onRowContext(event: MouseEvent, e: Entry): void {
    event.preventDefault();
    event.stopPropagation();
    if (!this.selected().has(e.path)) {
      this.selected.set(new Set([e.path]));
      this.anchor = e.path;
    }
    this.cursor.set(e.path);
    this.menu.set({ x: event.clientX, y: event.clientY, entry: e });
  }

  protected onBackgroundContext(event: MouseEvent): void {
    event.preventDefault();
    this.selected.set(new Set());
    this.menu.set({ x: event.clientX, y: event.clientY, entry: null });
  }

  protected onMenu(id: string): void {
    const m = this.menu();
    this.menu.set(null);
    const e = m?.entry ?? null;
    switch (id) {
      case 'open':
        if (e) {
          this.activate(e);
        }
        break;
      case 'openLocation':
        if (e) {
          this.clearSearchState();
          this.navigate(parentPath(e.path));
          this.cursor.set(e.path);
          this.selected.set(new Set([e.path]));
        }
        break;
      case 'reveal':
        this.ops.reveal(e?.path ?? this.currentPath());
        break;
      case 'copy':
        this.ops.copy(this.selectedPaths());
        break;
      case 'cut':
        this.ops.cut(this.selectedPaths());
        break;
      case 'paste':
        void this.paste();
        break;
      case 'pasteInto':
        if (e) {
          void this.ops.paste(e.path);
        }
        break;
      case 'rename':
        if (e) {
          this.renaming.set(e.path);
        }
        break;
      case 'color':
        if (e && m) {
          this.palette.set({ x: Math.min(m.x, (globalThis.innerWidth || 1280) - 200), y: m.y, entry: e });
        }
        break;
      case 'properties':
        if (e) {
          void this.showProperties(e);
        }
        break;
      case 'trash':
        void this.ops.trash(this.selectedPaths());
        break;
      case 'newFolder':
        this.startNewFolder();
        break;
      case 'refresh':
        void this.reload();
        break;
    }
  }

  protected paste(): Promise<void> {
    return this.ops.paste(this.currentPath());
  }

  protected startNewFolder(): void {
    this.clearSearchState();
    this.creating.set(true);
    this.viewportRef()?.nativeElement.scrollTo({ top: 0 });
  }

  protected async commitNewFolder(name: string): Promise<void> {
    if (!this.creating()) {
      return;
    }
    this.creating.set(false);
    const path = await this.ops.createFolder(this.currentPath(), name.trim());
    if (path !== null) {
      this.selected.set(new Set([path]));
      this.cursor.set(path);
    }
  }

  protected async commitRename(e: Entry, name: string): Promise<void> {
    if (this.renaming() !== e.path) {
      return;
    }
    this.renaming.set(null);
    const trimmed = name.trim();
    if (trimmed === e.name || trimmed.length === 0) {
      return;
    }
    const path = await this.ops.rename(e.path, trimmed);
    if (path !== null) {
      this.selected.set(new Set([path]));
      this.cursor.set(path);
    }
    this.gridRef()?.nativeElement.focus();
  }

  protected setColor(e: Entry, color: FolderColor | null): void {
    this.folderColors.set(e.path, color);
    this.palette.set(null);
  }

  protected async showProperties(e: Entry): Promise<void> {
    try {
      this.props.set(await this.backend.properties(this.ops.newId(), e.path));
    } catch (err: unknown) {
      this.error.set(err instanceof Error ? err.message : String(err));
    }
  }

  protected async openBin(): Promise<void> {
    try {
      this.bin.set(await this.backend.trashed(this.currentPath()));
    } catch {
      this.bin.set([]);
    }
  }

  protected async restore(t: TrashedItem): Promise<void> {
    await this.ops.restore([t.id]);
    this.bin.update((items) => items?.filter((i) => i.id !== t.id) ?? null);
  }

  protected onKey(event: KeyboardEvent): void {
    if ((event.target as HTMLElement).closest('input')) {
      return;
    }
    const mod = event.ctrlKey || event.metaKey;
    const rows = this.rows();
    const at = rows.findIndex((r) => r.path === this.cursor());
    const move = (to: number): void => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, to))];
      if (target === undefined) {
        return;
      }
      event.preventDefault();
      this.cursor.set(target.path);
      if (event.shiftKey && this.anchor !== null) {
        const a = rows.findIndex((r) => r.path === this.anchor);
        const [lo, hi] = a < to ? [a, to] : [to, a];
        this.selected.set(new Set(rows.slice(Math.max(0, lo), hi + 1).map((r) => r.path)));
      } else {
        this.selected.set(new Set([target.path]));
        this.anchor = target.path;
      }
      this.reveal(Math.max(0, Math.min(rows.length - 1, to)));
    };
    const page = Math.max(1, Math.floor(this.viewportHeight() / ROW_PX) - 1);
    if (event.altKey && event.key === 'ArrowLeft') {
      event.preventDefault();
      this.goBack();
      return;
    }
    if (event.altKey && event.key === 'ArrowRight') {
      event.preventDefault();
      this.goForward();
      return;
    }
    if (event.altKey && event.key === 'ArrowUp') {
      event.preventDefault();
      this.goUp();
      return;
    }
    switch (event.key) {
      case 'ArrowDown':
        move(at < 0 ? 0 : at + 1);
        return;
      case 'ArrowUp':
        move(at < 0 ? rows.length - 1 : at - 1);
        return;
      case 'PageDown':
        move(at + page);
        return;
      case 'PageUp':
        move(at - page);
        return;
      case 'Home':
        move(0);
        return;
      case 'End':
        move(rows.length - 1);
        return;
      case 'Enter': {
        const e = rows[at];
        if (e) {
          event.preventDefault();
          this.activate(e);
        }
        return;
      }
      case 'Backspace':
        event.preventDefault();
        this.goUp();
        return;
      case 'F2': {
        const e = rows[at];
        if (e) {
          event.preventDefault();
          this.renaming.set(e.path);
        }
        return;
      }
      case 'F5':
        event.preventDefault();
        void this.reload();
        return;
      case 'Delete':
        event.preventDefault();
        void this.ops.trash(this.selectedPaths());
        return;
      case 'Escape':
        this.selected.set(new Set());
        return;
    }
    if (mod && !event.shiftKey) {
      switch (event.key.toLowerCase()) {
        case 'a':
          event.preventDefault();
          this.selected.set(new Set(rows.map((r) => r.path)));
          return;
        case 'c':
          event.preventDefault();
          this.ops.copy(this.selectedPaths());
          return;
        case 'x':
          event.preventDefault();
          this.ops.cut(this.selectedPaths());
          return;
        case 'v':
          event.preventDefault();
          void this.paste();
          return;
      }
    }
    if (mod && event.shiftKey && event.key.toLowerCase() === 'n') {
      event.preventDefault();
      this.startNewFolder();
    }
  }

  protected onDragStart(event: DragEvent, e: Entry): void {
    if (!this.selected().has(e.path)) {
      this.selected.set(new Set([e.path]));
    }
    const paths = this.selectedPaths();
    event.dataTransfer?.setData(FILES_DRAG_TYPE, JSON.stringify(paths));
    event.dataTransfer?.setData('text/plain', paths.join('\n'));
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'copyMove';
    }
  }

  protected onDragOver(event: DragEvent, target: string): void {
    if (!event.dataTransfer?.types.includes(FILES_DRAG_TYPE)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    // the copy modifier (Ctrl, or Option on a Mac) copies, otherwise move (Explorer's and Finder's rule within a volume)
    event.dataTransfer.dropEffect = this.copies(event) ? 'copy' : 'move';
    const here = target === this.currentPath();
    this.dropTarget.set(here ? null : target);
    this.dropHere.set(here);
  }

  protected onDragLeave(event: DragEvent): void {
    const to = event.relatedTarget as Node | null;
    if (to === null || !(event.currentTarget as HTMLElement).contains(to)) {
      this.dropHere.set(false);
      this.dropTarget.set(null);
    }
  }

  protected onDrop(event: DragEvent, target: string): void {
    const raw = event.dataTransfer?.getData(FILES_DRAG_TYPE);
    this.dropTarget.set(null);
    this.dropHere.set(false);
    if (!raw) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    let paths: string[];
    try {
      const parsed: unknown = JSON.parse(raw);
      paths = Array.isArray(parsed) ? parsed.filter((p): p is string => typeof p === 'string') : [];
    } catch {
      return;
    }
    // dropping items on themselves or their own folder does nothing
    paths = paths.filter((p) => p !== target);
    if (paths.length > 0) {
      void this.ops.transfer(paths, target, this.copies(event) ? 'copy' : 'move');
    }
  }

  /** Whether a drop copies: Ctrl held on Windows, Option on a Mac (as in Finder). */
  private copies(event: DragEvent): boolean {
    return this.isMac ? event.altKey : event.ctrlKey;
  }

  protected iconOf(e: Entry): IconName {
    if (e.kind === 'folder') {
      return 'folder';
    }
    if (e.kind === 'link') {
      return 'link';
    }
    return CATEGORY_ICONS[categoryOf(e.name)];
  }

  protected colorOf(e: Entry): FolderColor | null {
    return e.kind === 'folder' ? this.folderColors.colorOf(e.path) : null;
  }

  protected typeOf(e: Entry): string {
    if (e.kind === 'folder') {
      return this.i18n.t('files.type.folder');
    }
    if (e.kind === 'link') {
      return this.i18n.t('files.type.link');
    }
    const ext = extOf(e.name);
    return ext === '' ? this.i18n.t('files.type.file') : this.i18n.t('files.type.ext').replace('{ext}', ext);
  }

  protected attributes(e: Entry): string {
    const parts = [e.readonly ? this.i18n.t('files.readonly') : null, e.hidden ? this.i18n.t('files.hidden') : null].filter((p): p is string => p !== null);
    return parts.length === 0 ? '—' : parts.join(', ');
  }

  protected parentOf(path: string): string {
    return parentPath(path);
  }

  protected isCut(path: string): boolean {
    const clip = this.ops.clipboard();
    return clip !== null && clip.kind === 'move' && clip.paths.includes(path);
  }

  protected rowId(e: Entry): string {
    return `file-${Math.abs(hash(e.path))}`;
  }

  /** The selected items in list order, or the item under the keyboard cursor. */
  selectedPaths(): string[] {
    const sel = this.selected();
    if (sel.size > 0) {
      return this.rows().filter((r) => sel.has(r.path)).map((r) => r.path);
    }
    const c = this.cursor();
    return c === null ? [] : [c];
  }

  private reveal(index: number): void {
    const el = this.viewportRef()?.nativeElement;
    if (!el) {
      return;
    }
    const to = scrollToReveal(index, ROW_PX, el.scrollTop, el.clientHeight);
    if (to !== null) {
      el.scrollTop = to;
      this.scrollTop.set(to);
    }
  }

  private clearSearchState(): void {
    this.filter.set('');
    this.searchResults.set(null);
  }

  private async start(initial: string | null): Promise<void> {
    let places: readonly Place[] = [];
    try {
      places = await this.backend.places();
    } catch {
      // no places: the pane still opens the folder it was given
    }
    this.places.set(places);
    const first = initial ?? places[0]?.path;
    if (first && this.listing() === null) {
      await this.load(first);
    }
  }

  private async load(path: string, keepSelection = false): Promise<void> {
    const token = ++this.loadToken;
    this.loading.set(true);
    try {
      const l = await this.backend.list(path);
      if (token !== this.loadToken) {
        return;
      }
      const changedFolder = this.listing()?.path !== l.path;
      this.listing.set(l);
      this.error.set(null);
      if (changedFolder) {
        this.pathChange.emit(l.path);
        this.scrollTop.set(0);
        this.viewportRef()?.nativeElement.scrollTo({ top: 0 });
      }
      if (!keepSelection || changedFolder) {
        this.selected.set(new Set());
        this.anchor = null;
      } else {
        // keep only what still exists
        const live = new Set(l.entries.map((e) => e.path));
        this.selected.update((s) => new Set([...s].filter((p) => live.has(p))));
      }
    } catch (e: unknown) {
      if (token === this.loadToken) {
        this.error.set(e instanceof Error ? e.message : String(e));
        this.listing.set({ path, parent: parentPath(path) === path ? null : parentPath(path), entries: [] });
      }
    } finally {
      if (token === this.loadToken) {
        this.loading.set(false);
      }
    }
  }
}

/** A stable small number for a DOM id. */
function hash(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  }
  return h;
}

