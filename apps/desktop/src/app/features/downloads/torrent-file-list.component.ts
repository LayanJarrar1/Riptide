import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, model, signal, viewChild } from '@angular/core';

import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { fileFraction } from '../../model/torrent';
import { IconComponent } from '../../ui/icon.component';

/** One row of the list. */
export interface TorrentFileRow {
  readonly index: number;
  readonly path: string;
  readonly len: number;
  /** The file already exists in the folder (add dialog only). */
  readonly exists?: boolean;
}

/** Height of one row in pixels; the window below renders only the rows in view. */
const ROW_PX = 36;
/** Rows drawn above and below the visible ones, so a fast scroll never shows a gap. */
const OVERSCAN = 6;

/**
 * The files of a torrent as a scrolling list that draws only the rows in view (a torrent can
 * hold tens of thousands of files). With `selectable` each row has a check box and the chosen
 * indices are the two-way `selected` set; with `progress` each row shows how much of the file is
 * verified. Names are bidi-isolated, so an Arabic file name reads right inside an English UI and
 * the other way round.
 */
@Component({
  selector: 'app-torrent-file-list',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div #viewport class="viewport" role="list" [attr.aria-label]="i18n.t('torrent.filesLabel')" (scroll)="onScroll()">
      <div class="window" [style.padding-block-start.px]="start() * rowPx" [style.padding-block-end.px]="(files().length - end()) * rowPx">
        @for (f of visible(); track f.index) {
          <div class="row" role="listitem" [class.is-off]="selectable() && !isSelected(f.index)" [style.block-size.px]="rowPx">
            @if (selectable()) {
              <input
                class="check"
                type="checkbox"
                [checked]="isSelected(f.index)"
                [attr.aria-label]="f.path"
                (change)="toggle(f.index)"
              />
            } @else {
              <app-icon class="file-icon" name="file" [size]="16" />
            }
            <span class="name" dir="auto" [attr.title]="f.path">
              @if (folderOf(f.path); as folder) {
                <span class="folder">{{ folder }}/</span>
              }
              <span class="base">{{ baseOf(f.path) }}</span>
            </span>
            @if (f.exists) {
              <span class="note">{{ i18n.t('torrent.fileExists') }}</span>
            }
            @if (progress(); as p) {
              @if (!selectable() && isSelected(f.index)) {
                <span class="bar" aria-hidden="true"><span class="fill" [style.inline-size.%]="percent(f, p)"></span></span>
                <span class="pct num">{{ percent(f, p) }}%</span>
              } @else if (!selectable()) {
                <span class="pct skipped">{{ i18n.t('torrent.skipped') }}</span>
              }
            }
            <span class="size num">{{ format.bytes(f.len) }}</span>
          </div>
        }
      </div>
    </div>
  `,
  styles: `
    :host {
      display: block;
      min-inline-size: 0;
    }

    .viewport {
      max-block-size: 252px;
      overflow: auto;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
    }

    .row {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding-inline: var(--space-3);
      border-block-end: 1px solid var(--color-border);
      font-size: var(--text-sm);
      box-sizing: border-box;
    }

    .row.is-off {
      color: var(--color-text-muted);
    }

    .check {
      flex: none;
      inline-size: 16px;
      block-size: 16px;
      margin: 0;
      accent-color: var(--color-accent);
      cursor: pointer;
    }

    .file-icon {
      flex: none;
      color: var(--color-text-muted);
    }

    .name {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      unicode-bidi: plaintext;
    }

    .folder {
      color: var(--color-text-muted);
    }

    .note {
      flex: none;
      color: var(--color-warning);
      font-size: var(--text-xs);
    }

    .size {
      flex: none;
      min-inline-size: 72px;
      text-align: end;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .bar {
      flex: none;
      inline-size: 64px;
      block-size: 4px;
      border-radius: var(--radius-full);
      background: var(--color-border-strong);
      overflow: hidden;
    }

    .fill {
      display: block;
      block-size: 100%;
      background: var(--color-accent);
    }

    .pct {
      flex: none;
      min-inline-size: 36px;
      text-align: end;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .pct.skipped {
      min-inline-size: 0;
      color: var(--color-text-muted);
    }
  `,
})
export class TorrentFileListComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly rowPx = ROW_PX;

  readonly files = input.required<readonly TorrentFileRow[]>();
  /** Show check boxes; the chosen indices are `selected`. */
  readonly selectable = input(false);
  /** The chosen file indices (two-way). For a read-only list, the files being downloaded; `null` = all. */
  readonly selected = model<ReadonlySet<number> | null>(null);
  /** Verified bytes per file, torrent order; `null` hides the progress column. */
  readonly progress = input<readonly number[] | null>(null);

  private readonly viewport = viewChild<ElementRef<HTMLElement>>('viewport');
  private readonly scrollTop = signal(0);
  private readonly height = signal(252);

  protected readonly start = computed(() => Math.max(0, Math.floor(this.scrollTop() / ROW_PX) - OVERSCAN));
  protected readonly end = computed(() => Math.min(this.files().length, Math.ceil((this.scrollTop() + this.height()) / ROW_PX) + OVERSCAN));
  protected readonly visible = computed(() => this.files().slice(this.start(), this.end()));

  protected onScroll(): void {
    const el = this.viewport()?.nativeElement;
    if (el !== undefined) {
      this.scrollTop.set(el.scrollTop);
      this.height.set(el.clientHeight || 252);
    }
  }

  protected isSelected(index: number): boolean {
    const s = this.selected();
    return s === null || s.has(index);
  }

  protected toggle(index: number): void {
    const next = new Set(this.selected() ?? this.files().map((f) => f.index));
    if (next.has(index)) {
      next.delete(index);
    } else {
      next.add(index);
    }
    this.selected.set(next);
  }

  protected percent(f: TorrentFileRow, progress: readonly number[]): number {
    return Math.floor(fileFraction(f.len, progress[f.index]) * 100);
  }

  protected folderOf(path: string): string {
    const i = path.lastIndexOf('/');
    return i < 0 ? '' : path.slice(0, i);
  }

  protected baseOf(path: string): string {
    return path.slice(path.lastIndexOf('/') + 1);
  }
}
