import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { FORMAT_PIPES } from '../../core/format.pipes';
import { FormatService } from '../../core/format.service';
import { TickerDirective } from '../../core/ticker.directive';
import { I18nService } from '../../i18n/i18n.service';
import { ColumnKey, DEFAULT_HIDDEN } from '../../data/column-widths';
import { DownloadRow } from '../../data/downloads.store';
import { canPause, canResume, CategoryKey, failureFix } from '../../model/download';
import { formatRatio } from '../../model/torrent';
import { ButtonComponent } from '../../ui/button.component';
import { FileNameComponent } from '../../ui/file-name.component';
import { IconComponent, IconName } from '../../ui/icon.component';
import { ProgressFlowComponent } from '../../ui/progress-flow.component';
import { StatusPillComponent } from '../../ui/status-pill.component';

/** Actions a row (or the details drawer) can ask for. */
export type RowAction = 'pause' | 'resume' | 'retry' | 'restart' | 'cancel' | 'remove' | 'openFile' | 'openFolder' | 'copyLink' | 'details' | 'refreshLink' | 'moveToQueue' | 'moveUp' | 'moveDown';

/** File-type glyph per category. */
export const CATEGORY_ICONS: Readonly<Record<CategoryKey, IconName>> = {
  compressed: 'fileArchive',
  documents: 'fileDoc',
  music: 'fileMusic',
  video: 'fileVideo',
  programs: 'fileApp',
  images: 'fileImage',
  other: 'file',
};

/** Host name of a URL for the secondary line; the raw string if it does not parse. */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

/**
 * One row of the downloads table. Pure presentation: the page owns selection and dispatches
 * actions to the engine. Rendered as a grid row with the same column template as the header.
 *
 * Motion: a row added during the session slides in from the inline-start side; on the first
 * paint the page hands the first rows a `staggerIndex` so they arrive one after another; a row
 * being removed fades and collapses before it leaves the DOM. A row that merely scrolls into
 * view does nothing. The speed cell is a number ticker. A failed row explains itself under
 * the name and offers the fix inline.
 */
@Component({
  selector: 'app-download-row',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES, FileNameComponent, ButtonComponent, IconComponent, ProgressFlowComponent, StatusPillComponent, TickerDirective],
  host: {
    role: 'row',
    '[attr.aria-selected]': 'selected()',
    '[attr.data-state]': 'row().d.state',
    '[class.is-selected]': 'selected()',
    '[class.is-open]': 'open()',
    '[class.is-done]': 'row().d.state === "completed"',
    '[class.is-cancelled]': 'row().d.state === "cancelled"',
    '[class.is-failed]': 'row().d.state === "failed"',
    '[class.is-entering]': 'fresh()',
    '[class.is-staggered]': 'staggerIndex() !== null',
    '[class.is-leaving]': 'leaving()',
    '[style.--i]': 'staggerIndex()',
    '(click)': 'rowClick.emit($event)',
    '(dblclick)': 'rowDblClick.emit()',
    '(contextmenu)': 'onContext($event)',
  },
  template: `
    <div class="cell c-name" role="gridcell">
      <span class="type" [attr.data-category]="row().d.category">
        <app-icon [name]="icon()" [size]="18" />
      </span>
      <span class="name-block">
        <span class="name-line">
          <app-file-name class="name" [name]="row().d.filename" />
          @if (row().d.kind === 'torrent') {
            <span class="media-badge" role="img" [attr.title]="i18n.t('torrent.badge')" [attr.aria-label]="i18n.t('torrent.badge')"><app-icon name="magnet" [size]="10" /></span>
          }
          @if (row().d.kind === 'media') {
            <span class="media-badge" role="img" [attr.title]="i18n.t('details.video')" [attr.aria-label]="i18n.t('details.video')"><app-icon name="play" [size]="10" /></span>
          }
        </span>
        @if (row().d.state === 'failed') {
          <span class="reason">
            <app-icon name="alert" [size]="12" />
            <span class="reason-text" [attr.title]="failureTitle()">{{ failureLabel() }}</span>
            @if (fix(); as f) {
              <span class="reason-sep" aria-hidden="true">·</span>
              <button type="button" class="reason-action" (click)="act($event, f.action)">
                {{ i18n.t(f.labelKey) }}
              </button>
            }
          </span>
        } @else if (torrentLine(); as line) {
          <span class="host torrent-line" dir="auto" [attr.title]="line">{{ line }}</span>
        } @else {
          <span class="host" dir="ltr">{{ host() }}</span>
        }
      </span>
    </div>

    @if (!hidden().has('size')) {
      <div class="cell c-size num" role="gridcell">
        @if (showDownloaded()) {
          <span class="size-done">{{ row().downloaded | bytes }}</span>
          <span class="size-sep">/</span>
        }
        <span>{{ row().d.size | bytes }}</span>
      </div>
    }

    @if (!hidden().has('progress')) {
      <div class="cell c-progress" role="gridcell">
        <app-progress-flow [segments]="row().d.segments" [size]="row().d.size" [state]="row().d.state" />
        <span class="pct num">{{ row().progress | pct }}</span>
      </div>
    }

    @if (!hidden().has('speed')) {
      <span class="cell c-speed num" role="gridcell" [appTicker]="row().d.speedBps" [tickerFormat]="speedFormat" [tickerStream]="true"></span>
    }

    @if (!hidden().has('avgSpeed')) {
      <div class="cell c-avgSpeed num" role="gridcell" [attr.title]="row().avgBps === null ? i18n.t('column.unrecorded') : null">{{ row().avgBps | speed }}</div>
    }

    @if (!hidden().has('eta')) {
      <div class="cell c-eta num" role="gridcell">{{ row().eta | duration }}</div>
    }

    @if (!hidden().has('duration')) {
      <div class="cell c-duration num" role="gridcell" [attr.title]="row().activeMs === null ? i18n.t('column.unrecorded') : null">{{ activeSeconds() | duration }}</div>
    }

    @if (!hidden().has('status')) {
      <div class="cell c-status" role="gridcell">
        <app-status-pill [state]="row().d.state" [failure]="row().d.failure" [compact]="true" />
      </div>
    }

    @if (!hidden().has('category')) {
      <div class="cell c-category" role="gridcell">{{ i18n.t(categoryLabel()) }}</div>
    }

    @if (!hidden().has('added')) {
      <div class="cell c-added num" role="gridcell" [attr.title]="row().d.addedAt | dateTime">
        {{ row().d.addedAt | relTime }}
      </div>
    }

    <div class="cell c-actions" role="gridcell">
      @if (canPause(row().d.state)) {
        <button appButton size="sm" variant="ghost" icon="pause" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('action.pause')" [attr.title]="i18n.t('action.pause')" (click)="act($event, 'pause')"></button>
      } @else if (canResume(row().d.state) && (row().d.state !== 'failed' || fix()?.action === 'retry')) {
        <button appButton size="sm" variant="ghost" [icon]="row().d.state === 'failed' ? 'retry' : 'play'" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t(row().d.state === 'failed' ? 'action.retry' : 'action.resume')" [attr.title]="i18n.t(row().d.state === 'failed' ? 'action.retry' : 'action.resume')" (click)="act($event, row().d.state === 'failed' ? 'retry' : 'resume')"></button>
      } @else if (row().d.state === 'completed') {
        <button appButton size="sm" variant="ghost" icon="folderOpen" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('action.openFolder')" [attr.title]="i18n.t('action.openFolder')" (click)="act($event, 'openFolder')"></button>
      }
      <button appButton size="sm" variant="ghost" icon="more" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('action.more')" [attr.title]="i18n.t('action.more')" (click)="more($event)"></button>
    </div>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: var(--grid-columns);
      min-inline-size: var(--grid-min, 0);
      align-items: center;
      block-size: var(--row-height);
      padding-inline: var(--space-3);
      border-block-end: 1px solid var(--color-border);
      color: var(--color-text);
      font-size: var(--text-sm);
      cursor: default;
      transition: background-color var(--dur-1) var(--ease-standard);
      outline-offset: -2px;
    }

    :host(:hover) {
      background: var(--color-surface-hover);
    }

    :host(.is-selected) {
      background: var(--color-accent-soft);
    }

    :host(.is-open) {
      box-shadow: inset calc(var(--dir-sign) * 3px) 0 0 var(--color-accent);
    }

    /* ---- Enter / leave: transform and opacity only ------------------------------------- */
    :host(.is-entering) {
      animation: row-in var(--dur-2) var(--ease-emphasized-out) both;
    }

    :host(.is-staggered) {
      animation: row-in var(--dur-1) var(--ease-emphasized-out) both;
      animation-delay: calc(2 * var(--stagger-step) + var(--i, 0) * var(--stagger-step));
    }

    :host(.is-leaving) {
      animation: row-out var(--dur-2) var(--ease-emphasized-in) both;
      pointer-events: none;
    }

    @keyframes row-in {
      from {
        opacity: 0;
        transform: translateX(calc(var(--dir-sign) * -8px));
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    @keyframes row-out {
      from {
        opacity: 1;
        transform: none;
      }

      to {
        opacity: 0;
        transform: translateX(calc(var(--dir-sign) * 16px)) scaleY(0.6);
      }
    }

    :host(.is-cancelled) .name,
    :host(.is-cancelled) .host {
      color: var(--color-text-muted);
      text-decoration: line-through;
    }

    .cell {
      min-inline-size: 0;
      padding-inline: var(--space-2);
    }

    .c-name {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding-inline-start: 0;
    }

    .type {
      display: grid;
      place-items: center;
      inline-size: 32px;
      block-size: 32px;
      flex-shrink: 0;
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
    }

    .type[data-category='video'] {
      color: #c58cff;
    }

    .type[data-category='music'] {
      color: #ff9ab8;
    }

    .type[data-category='images'] {
      color: #ffb86b;
    }

    .type[data-category='documents'] {
      color: #7fb3ff;
    }

    .type[data-category='programs'] {
      color: #7ee0a2;
    }

    .type[data-category='compressed'] {
      color: #e3c56a;
    }

    .name-block {
      display: flex;
      flex-direction: column;
      min-inline-size: 0;
      line-height: 1.25;
    }

    .name-line {
      display: flex;
      align-items: center;
      gap: 6px;
      min-inline-size: 0;
    }

    .media-badge {
      flex: none;
      display: inline-grid;
      place-items: center;
      inline-size: 18px;
      block-size: 14px;
      border-radius: var(--radius-xs);
      background: var(--color-accent-ring);
      color: var(--color-accent-fg);
    }

    .name {
      font-weight: var(--weight-medium);
    }

    .host {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      text-align: start;
    }

    /* The reason line of a failed row: what went wrong, and the fix, right where you look. */
    .reason {
      display: inline-flex;
      align-items: center;
      gap: 5px;
      min-inline-size: 0;
      color: var(--color-danger);
      font-size: var(--text-xs);
      white-space: nowrap;
      max-inline-size: 100%;
      animation: reason-in var(--dur-2) var(--ease-emphasized-out) both;
    }

    @keyframes reason-in {
      from {
        opacity: 0;
        transform: translateY(-2px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    /* two lines at most: the full text is in the tooltip, and the row keeps its fixed height */
    .reason-text {
      display: -webkit-box;
      -webkit-box-orient: vertical;
      -webkit-line-clamp: 2;
      line-clamp: 2;
      overflow: hidden;
      white-space: normal;
      line-height: 1.15;
      overflow-wrap: anywhere;
    }

    .reason-sep {
      color: var(--color-text-muted);
    }

    .reason-action {
      padding: 0;
      border: 0;
      background: none;
      color: var(--color-accent-fg);
      font: inherit;
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      cursor: pointer;
      text-decoration: underline;
      text-decoration-color: transparent;
      text-underline-offset: 2px;
      transition: text-decoration-color var(--dur-1) var(--ease-standard);
    }

    .reason-action:hover,
    .reason-action:focus-visible {
      text-decoration-color: currentcolor;
    }

    .c-size,
    .c-speed,
    .c-avgSpeed,
    .c-eta,
    .c-duration {
      text-align: end;
      color: var(--color-text-secondary);
      white-space: nowrap;
    }

    .c-speed {
      display: block;
    }

    :host([data-state='downloading']) .c-speed {
      color: var(--color-text);
    }

    .size-done {
      color: var(--color-text);
    }

    .size-sep {
      margin-inline: 4px;
      color: var(--color-text-muted);
    }

    .c-progress {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }

    .c-progress app-progress-flow {
      flex: 1;
    }

    .pct {
      inline-size: 3.2em;
      text-align: end;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    :host(.is-done) .pct {
      color: var(--color-success);
    }

    .c-added {
      color: var(--color-text-muted);
      white-space: nowrap;
    }

    .c-category {
      overflow: hidden;
      color: var(--color-text-secondary);
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .c-actions {
      display: flex;
      justify-content: flex-end;
      gap: 2px;
      opacity: 0;
      transition: opacity var(--dur-1) var(--ease-standard);
    }

    :host(:hover) .c-actions,
    :host(:focus-within) .c-actions,
    :host(.is-selected) .c-actions {
      opacity: 1;
    }

    /* Mirrors the header's container queries in the page: same breakpoints, same columns. */
    @container grid (max-width: 1050px) {
      :host {
        grid-template-columns: var(--grid-columns-compact);
        min-inline-size: var(--grid-min-compact, 0);
      }

      .c-eta,
      .c-added,
      .c-avgSpeed,
      .c-duration,
      .c-category {
        display: none;
      }
    }

    @container grid (max-width: 780px) {
      :host {
        grid-template-columns: var(--grid-columns-narrow);
        min-inline-size: var(--grid-min-narrow, 0);
      }

      .c-speed {
        display: none;
      }
    }

    @container grid (max-width: 600px) {
      :host {
        grid-template-columns: var(--grid-columns-tiny);
        min-inline-size: var(--grid-min-tiny, 0);
      }

      .c-size,
      .c-status {
        display: none;
      }
    }

    @media (prefers-reduced-motion: reduce) {
      :host(.is-entering),
      :host(.is-staggered),
      :host(.is-leaving),
      .reason {
        animation-duration: var(--dur-fade);
        animation-delay: 0ms;
        animation-name: row-fade;
      }

      :host(.is-leaving) {
        animation-direction: reverse;
      }

      @keyframes row-fade {
        from {
          opacity: 0;
        }

        to {
          opacity: 1;
        }
      }
    }
  `,
})
export class DownloadRowComponent {
  protected readonly i18n = inject(I18nService);
  private readonly format = inject(FormatService);
  protected readonly canPause = canPause;
  protected readonly canResume = canResume;

  readonly row = input.required<DownloadRow>();
  readonly selected = input(false);
  /** Shown in the details drawer. */
  readonly open = input(false);
  /** Position in the first-paint cascade; `null` outside the first paint. */
  readonly staggerIndex = input<number | null>(null);
  /** Added during this session: slides in. */
  readonly fresh = input(false);
  /** About to be removed: fades and collapses. */
  readonly leaving = input(false);
  /** Columns the user turned off (the page's header shows the same set). */
  readonly hidden = input<ReadonlySet<ColumnKey>>(DEFAULT_HIDDEN);

  readonly rowClick = output<MouseEvent>();
  readonly rowDblClick = output<void>();
  readonly rowContext = output<MouseEvent>();
  readonly action = output<RowAction>();

  protected readonly categoryLabel = computed(() => `category.${this.row().d.category}`);
  /** Seconds spent downloading, for the duration pipe; `null` when never recorded. */
  protected readonly activeSeconds = computed(() => {
    const ms = this.row().activeMs;
    return ms === null ? null : ms / 1000;
  });
  protected readonly icon = computed<IconName>(() => (this.row().d.kind === 'torrent' ? 'magnet' : CATEGORY_ICONS[this.row().d.category]));
  /** The second line of a torrent row: peers (or seeding) and the share ratio; `null` for other rows. */
  protected readonly torrentLine = computed(() => {
    const d = this.row().d;
    const t = d.torrent;
    if (d.kind !== 'torrent' || t === undefined) {
      return null;
    }
    // the ratio only shows once something was uploaded: the line is short and a 0.00 says nothing
    const ratio = t.ratio > 0 ? ` · ${this.i18n.t('torrent.ratio').replace('{ratio}', formatRatio(t.ratio))}` : '';
    if (t.seeding) {
      return `${this.i18n.t('torrent.seeding')}${ratio}`;
    }
    if (d.state === 'downloading' || d.state === 'probing') {
      return `${this.i18n.t('torrent.peersOf').replace('{n}', '' + t.peers).replace('{seen}', '' + t.peersSeen)}${ratio}`;
    }
    return `${this.i18n.t('torrent.badge')}${ratio}`;
  });
  protected readonly host = computed(() => hostOf(this.row().d.url));
  protected readonly showDownloaded = computed(() => {
    const s = this.row().d.state;
    return s === 'downloading' || s === 'paused' || s === 'verifying' || s === 'failed';
  });

  /** Why it failed, from the failure kind (the engine's raw error text is not in the model). */
  protected readonly failureLabel = computed(() => {
    const failure = this.row().d.failure;
    return failure === null ? this.i18n.t('state.failed') : this.i18n.t(`failure.${failure}`);
  });

  /** The tooltip of the reason: the long explanation where there is one. */
  protected readonly failureTitle = computed(() => {
    const failure = this.row().d.failure;
    return failure === 'denied' || failure === 'auth' || failure === 'notfound' ? this.i18n.t(`failure.${failure}.long`) : this.failureLabel();
  });

  /** The fix for a failed row (see `failureFix`); `null` when nothing helps, e.g. a 404. */
  protected readonly fix = computed(() => failureFix(this.row().d.failure));

  /** Stable formatter for the speed ticker (follows the language through the service). */
  protected readonly speedFormat = (bps: number): string => this.format.speed(bps);

  protected act(event: Event, action: RowAction): void {
    event.stopPropagation();
    this.action.emit(action);
  }

  protected more(event: MouseEvent): void {
    event.stopPropagation();
    this.rowContext.emit(event);
  }

  protected onContext(event: MouseEvent): void {
    event.preventDefault();
    this.rowContext.emit(event);
  }
}
