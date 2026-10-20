import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal } from '@angular/core';

import { FORMAT_PIPES } from '../../core/format.pipes';
import { FormatService } from '../../core/format.service';
import { TickerDirective } from '../../core/ticker.directive';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadRow } from '../../data/downloads.store';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { canPause, canResume, failureFix } from '../../model/download';
import { MAIN_QUEUE_ID } from '../../model/queue';
import { formatRatio, TorrentLiveView } from '../../model/torrent';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ProgressFlowComponent } from '../../ui/progress-flow.component';
import { SegmentMapComponent } from '../../ui/segment-map.component';
import { SparklineComponent } from '../../ui/sparkline.component';
import { StatusPillComponent } from '../../ui/status-pill.component';
import { CATEGORY_ICONS, hostOf, RowAction } from './download-row.component';
import { TorrentFileListComponent } from './torrent-file-list.component';
import { DownloadSpeedLimitComponent } from './download-speed-limit.component';

interface LogLine {
  readonly at: number;
  readonly text: string;
}

/**
 * The details drawer: what is happening to one file, right now. Numbers first (downloaded,
 * speed, time left, connections), then the segment map and the speed line, then the facts
 * about the source and the destination, then the event log.
 *
 * On open the stat tiles arrive one after another, the sparkline draws its line in and every
 * segment stripe grows to its width; the numbers tick rather than jump. Switching rows while
 * the drawer is open keeps the instance, so none of that replays.
 */
@Component({
  selector: 'app-download-details',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    ...FORMAT_PIPES,
    ButtonComponent,
    DownloadSpeedLimitComponent,
    IconComponent,
    ProgressFlowComponent,
    SegmentMapComponent,
    SparklineComponent,
    StatusPillComponent,
    TickerDirective,
    TorrentFileListComponent,
  ],
  host: { role: 'complementary', '[attr.aria-label]': 'i18n.t("details.title")' },
  template: `
    <header class="head">
      <span class="type"><app-icon [name]="icon()" [size]="20" /></span>
      <div class="head-text">
        <h2 class="name" dir="auto" [attr.title]="d().filename">{{ d().filename }}</h2>
        <app-status-pill [state]="d().state" [failure]="d().failure" />
      </div>
      <button
        appButton
        variant="ghost"
        icon="close"
        [iconOnly]="true"
        type="button"
        [attr.aria-label]="i18n.t('action.close')"
        [attr.title]="i18n.t('action.close')"
        (click)="closed.emit()"
      ></button>
    </header>

    <div class="scroll">
      @if (d().state === 'failed' && (friendlyError() !== null || d().errorDetail)) {
        <section class="failure" role="alert">
          @if (friendlyError(); as friendly) {
            <p class="failure-text">{{ i18n.t(friendly) }}</p>
          }
          @if (d().errorDetail; as detail) {
            <details class="failure-details">
              <summary>{{ i18n.t('add.mediaDetails') }}</summary>
              <code dir="ltr">{{ detail }}</code>
            </details>
          }
        </section>
      }
      <section class="numbers" [attr.aria-label]="i18n.t('details.progress')">
        <div class="stat" style="--i: 0">
          <span class="stat-value num" [appTicker]="row().downloaded" [tickerFormat]="bytesFormat"></span>
          <span class="stat-label">{{ i18n.t('details.of') }} {{ d().size | bytes }}</span>
        </div>
        <div class="stat" style="--i: 1">
          <span class="stat-value num" [appTicker]="d().speedBps" [tickerFormat]="speedFormat"></span>
          <span class="stat-label">{{ i18n.t('column.speed') }}</span>
        </div>
        <div class="stat" style="--i: 2">
          <span class="stat-value num">{{ row().eta | duration }}</span>
          <span class="stat-label">{{ i18n.t('column.eta') }}</span>
        </div>
        @if (isTorrent()) {
          <div class="stat" style="--i: 3">
            <span class="stat-value num">{{ live()?.peers ?? d().torrent?.peers ?? 0 }}<span class="stat-dim">/{{ live()?.peersSeen ?? d().torrent?.peersSeen ?? 0 }}</span></span>
            <span class="stat-label">{{ i18n.t('torrent.peersLabel') }}</span>
          </div>
        } @else if (!isMedia()) {
          <div class="stat" style="--i: 3">
            <span class="stat-value num">{{ activeConnections() }}<span class="stat-dim">/{{ d().connections }}</span></span>
            <span class="stat-label">{{ i18n.t('details.connections') }}</span>
          </div>
        }
      </section>

      <section class="block" style="--i: 4">
        <div class="block-head">
          <h3 class="block-title">{{ i18n.t(isMedia() || isTorrent() ? 'details.mediaProgress' : 'details.segments') }}</h3>
          @if (isTorrent()) {
            <span class="block-meta media-badge"><app-icon name="magnet" [size]="12" /> {{ i18n.t('torrent.badge') }}</span>
          } @else if (isMedia()) {
            <span class="block-meta media-badge"><app-icon name="play" [size]="12" /> {{ i18n.t('details.video') }}</span>
          } @else {
            <span class="block-meta num">{{ segmentSummary() }}</span>
          }
        </div>
        <app-progress-flow [segments]="d().segments" [size]="d().size" [state]="d().state" [large]="true" />
        @if (isTorrent()) {
          <p class="hint">{{ i18n.t(d().torrent?.sequential ? 'torrent.sequentialOn' : 'torrent.hint') }}</p>
        } @else if (isMedia()) {
          <p class="hint">{{ i18n.t('details.mediaHint') }}</p>
        } @else {
          @if (d().size !== null) {
            <app-segment-map class="map" [segments]="d().segments" [size]="d().size" />
          }
          <p class="hint">{{ i18n.t(d().supportsRanges ? 'details.rangesYes' : 'details.rangesNo') }}</p>
        }
      </section>

      <section class="block" style="--i: 5">
        <div class="block-head">
          <h3 class="block-title">{{ i18n.t('details.speedChart') }}</h3>
          <span class="block-meta num">{{ i18n.t('details.peak') }} {{ peak() | speed }}</span>
        </div>
        <app-sparkline [samples]="d().speedHistory" [draw]="true" />
      </section>

      @if (isTorrent() && d().torrent; as t) {
        <section class="block" style="--i: 5">
          <div class="block-head">
            <h3 class="block-title">{{ i18n.t('torrent.shareTitle') }}</h3>
            @if (t.seeding) {
              <span class="block-meta media-badge">{{ i18n.t('torrent.seeding') }}</span>
            }
          </div>
          <div class="share">
            <span class="share-item"><span class="share-value num">{{ ratioText() }}</span><span class="stat-label">{{ i18n.t('torrent.ratioLabel') }}</span></span>
            <span class="share-item"><span class="share-value num">{{ format.bytes(live()?.uploaded ?? t.uploaded) }}</span><span class="stat-label">{{ i18n.t('torrent.uploaded') }}</span></span>
            <span class="share-item"><span class="share-value num">{{ format.speed(live()?.upBps ?? t.upBps) }}</span><span class="stat-label">{{ i18n.t('torrent.upSpeed') }}</span></span>
          </div>
        </section>

        <section class="block" style="--i: 6">
          <div class="block-head">
            <h3 class="block-title">{{ i18n.t('torrent.filesLabel') }}</h3>
            <span class="block-meta num">{{ i18n.t('torrent.files').replace('{n}', '' + t.files.length) }}</span>
          </div>
          <app-torrent-file-list [files]="t.files" [selected]="selectedSet()" [progress]="live()?.fileProgress ?? null" />
        </section>
      } @else {
        <section class="block" style="--i: 5">
          <app-download-speed-limit [download]="d()" />
        </section>
      }

      <section class="block facts" style="--i: 6">
        <h3 class="block-title">{{ i18n.t('details.source') }}</h3>
        <dl class="fact-list">
          <dt>{{ i18n.t('details.url') }}</dt>
          <dd class="fact-url">
            <span dir="ltr" class="truncate" [attr.title]="d().url">{{ d().url }}</span>
            <button appButton size="sm" variant="ghost" icon="copy" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('action.copyLink')" [attr.title]="i18n.t('action.copyLink')" (click)="action.emit('copyLink')"></button>
          </dd>
          @if (isTorrent() && d().torrent; as t) {
            <dt>{{ i18n.t('torrent.hash') }}</dt>
            <dd class="mono truncate" dir="ltr" [attr.title]="t.infoHash">{{ t.infoHash }}</dd>
          } @else {
            <dt>{{ i18n.t('details.server') }}</dt>
            <dd dir="ltr">{{ host() }}</dd>
          }
          <dt>{{ i18n.t('details.saveTo') }}</dt>
          <dd class="fact-url">
            <span dir="ltr" class="truncate" [attr.title]="d().dir">{{ d().dir }}</span>
            <button appButton size="sm" variant="ghost" icon="folderOpen" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('action.openFolder')" [attr.title]="i18n.t('action.openFolder')" (click)="action.emit('openFolder')"></button>
          </dd>
          <dt>{{ i18n.t('queue.detailsLabel') }}</dt>
          <dd class="fact-url">
            <span class="truncate" dir="auto" [attr.title]="queueName()">{{ queueName() }}</span>
            <button appButton size="sm" variant="ghost" type="button" (click)="action.emit('moveToQueue')">{{ i18n.t('queue.detailsMove') }}</button>
          </dd>
          <dt>{{ i18n.t('details.checksum') }}</dt>
          <dd>
            @if (d().checksum; as c) {
              <span class="checksum">
                <app-icon [name]="d().failure === 'checksum' ? 'alert' : d().state === 'completed' ? 'shieldCheck' : 'shield'" [size]="14" [class.ok]="d().state === 'completed'" [class.bad]="d().failure === 'checksum'" />
                <span class="mono truncate" dir="ltr" [attr.title]="c.hex">{{ c.algo.toUpperCase() }} {{ c.hex }}</span>
              </span>
            } @else {
              <span class="muted">{{ i18n.t('details.noChecksum') }}</span>
            }
          </dd>
          <dt>{{ i18n.t('column.added') }}</dt>
          <dd class="num">{{ d().addedAt | dateTime }}</dd>
          @if (d().completedAt; as at) {
            <dt>{{ i18n.t('details.completedAt') }}</dt>
            <dd class="num">{{ at | dateTime }}</dd>
          }
        </dl>
      </section>

      <section class="block" style="--i: 7">
        <h3 class="block-title">{{ i18n.t('details.log') }}</h3>
        <ol class="log">
          @for (line of log(); track line.at + line.text) {
            <li class="log-line">
              <span class="log-at num">{{ line.at | dateTime }}</span>
              <span class="log-text">{{ line.text }}</span>
            </li>
          }
        </ol>
      </section>
    </div>

    <footer class="actions">
      @if (canPause(d().state)) {
        <button appButton variant="secondary" icon="pause" type="button" (click)="action.emit('pause')">{{ i18n.t('action.pause') }}</button>
      } @else if (d().state === 'failed' && fix()?.action === 'refreshLink') {
        <button appButton variant="primary" icon="link" type="button" (click)="action.emit('refreshLink')">{{ i18n.t(fix()?.labelKey ?? 'action.refreshLink') }}</button>
      } @else if (d().state === 'failed' && fix() === null) {
        <!-- permanent failure (404/410): retrying cannot help -->
      } @else if (canResume(d().state)) {
        <button appButton variant="primary" [icon]="d().state === 'failed' ? 'retry' : 'play'" type="button" (click)="action.emit(d().state === 'failed' ? 'retry' : 'resume')">{{ i18n.t(d().state === 'failed' ? 'action.retry' : 'action.resume') }}</button>
      } @else if (d().state === 'completed') {
        <button appButton variant="primary" icon="external" type="button" (click)="action.emit('openFile')">{{ i18n.t('action.openFile') }}</button>
      }
      @if (d().state !== 'completed' && d().state !== 'cancelled') {
        <button appButton variant="ghost" icon="stop" type="button" (click)="action.emit('cancel')">{{ i18n.t('action.cancel') }}</button>
      }
      <span class="spacer"></span>
      <button appButton variant="ghost" icon="trash" type="button" (click)="action.emit('remove')">{{ i18n.t('action.remove') }}</button>
    </footer>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-rows: auto 1fr auto;
      block-size: 100%;
      min-block-size: 0;
      border-inline-start: 1px solid var(--color-border);
      background: var(--color-surface-1);
    }

    .head {
      display: flex;
      align-items: flex-start;
      gap: var(--space-3);
      padding: var(--space-4) var(--space-4) var(--space-3);
      border-block-end: 1px solid var(--color-border);
    }

    .type {
      display: grid;
      place-items: center;
      flex-shrink: 0;
      inline-size: 36px;
      block-size: 36px;
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
    }

    .head-text {
      display: flex;
      flex: 1;
      flex-direction: column;
      gap: var(--space-2);
      min-inline-size: 0;
    }

    .name {
      font-size: var(--text-md);
      font-weight: var(--weight-semibold);
      line-height: 1.3;
      overflow-wrap: anywhere;
    }

    .scroll {
      overflow: auto;
      padding: var(--space-4);
      display: flex;
      flex-direction: column;
      gap: var(--space-6);
    }

    .numbers {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: var(--space-3);
    }

    .stat {
      display: flex;
      flex-direction: column;
      gap: 2px;
      padding: var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
    }

    /* Open: tiles, then the blocks below, each a beat after the previous one. */
    .stat,
    .block {
      animation: tile-in var(--dur-3) var(--ease-emphasized-out) both;
      animation-delay: calc(80ms + var(--i, 0) * 45ms);
    }

    @keyframes tile-in {
      from {
        opacity: 0;
        transform: translateY(8px) scale(0.98);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .stat-value {
      font-size: var(--text-xl);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
      line-height: 1.1;
    }

    .stat-dim {
      color: var(--color-text-muted);
      font-weight: var(--weight-regular);
    }

    .stat-label {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .block {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
    }

    .block-head {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: var(--space-3);
    }

    .block-title {
      font-size: var(--text-sm);
      font-weight: var(--weight-semibold);
      color: var(--color-text);
    }

    .block-meta {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .map {
      margin-block-start: var(--space-1);
    }

    .hint {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .fact-list {
      display: grid;
      grid-template-columns: max-content 1fr;
      column-gap: var(--space-4);
      row-gap: var(--space-2);
      margin: 0;
      font-size: var(--text-sm);
    }

    dt {
      color: var(--color-text-muted);
    }

    dd {
      margin: 0;
      min-inline-size: 0;
      color: var(--color-text);
    }

    .fact-url {
      display: flex;
      align-items: center;
      gap: var(--space-1);
    }

    .share {
      display: flex;
      gap: var(--space-5);
    }

    .share-item {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-inline-size: 0;
    }

    .share-value {
      font-size: var(--text-lg, 1.125rem);
      font-weight: var(--weight-semibold, 600);
    }

    .truncate {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      text-align: start;
    }

    .checksum {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      min-inline-size: 0;
    }

    .checksum .ok {
      color: var(--color-success);
    }

    .checksum .bad {
      color: var(--color-danger);
    }

    .mono {
      font-family: var(--font-mono);
      font-size: var(--text-xs);
    }

    .muted {
      color: var(--color-text-muted);
    }

    .log {
      margin: 0;
      padding: 0;
      list-style: none;
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      font-size: var(--text-xs);
    }

    .failure {
      display: grid;
      gap: 8px;
      margin-block-end: var(--space-4, 16px);
      padding: 10px 12px;
      border: 1px solid var(--color-danger);
      border-radius: var(--radius-md, 8px);
      color: var(--color-text-primary);
      font-size: var(--text-sm);
    }

    .failure-text {
      margin: 0;
    }

    .failure-details summary {
      cursor: pointer;
      color: var(--color-text-secondary);
    }

    .failure-details code {
      display: block;
      margin-block-start: 6px;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      overflow-wrap: anywhere;
      user-select: text;
    }

    .log-line {
      display: grid;
      grid-template-columns: max-content 1fr;
      gap: var(--space-3);
    }

    .log-at {
      color: var(--color-text-muted);
      white-space: nowrap;
    }

    .log-text {
      color: var(--color-text-secondary);
    }

    .actions {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding: var(--space-3) var(--space-4);
      border-block-start: 1px solid var(--color-border);
      background: var(--color-surface-1);
    }

    .spacer {
      flex: 1;
    }

    .media-badge {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      color: var(--color-accent-fg);
    }

    @media (prefers-reduced-motion: reduce) {
      .stat,
      .block {
        animation-duration: var(--dur-fade);
        animation-delay: 0ms;
        animation-name: tile-fade;
      }

      @keyframes tile-fade {
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
export class DownloadDetailsComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  private readonly queues = inject(QueuesService);
  private readonly engine = inject(DownloadEngine);
  protected readonly canPause = canPause;
  protected readonly canResume = canResume;

  readonly row = input.required<DownloadRow>();
  readonly closed = output<void>();
  readonly action = output<RowAction>();

  protected readonly d = computed(() => this.row().d);
  /** The i18n key of the long, friendly explanation for a refused video; `null` for other failures. */
  protected readonly friendlyError = computed(() => {
    const failure = this.d().failure;
    return failure === 'login' || failure === 'refused' || failure === 'auth' || failure === 'denied' || failure === 'notfound' ? `failure.${failure}.long` : null;
  });
  protected readonly fix = computed(() => failureFix(this.d().failure));
  protected readonly isMedia = computed(() => this.d().kind === 'media');
  protected readonly isTorrent = computed(() => this.d().kind === 'torrent');
  /** Per-file progress, peers and speeds of a torrent, polled once a second while the drawer shows it. */
  protected readonly live = signal<TorrentLiveView | null>(null);
  protected readonly selectedSet = computed<ReadonlySet<number> | null>(() => {
    const s = this.d().torrent?.selected ?? null;
    return s === null ? null : new Set(s);
  });
  protected readonly ratioText = computed(() => formatRatio(this.live()?.ratio ?? this.d().torrent?.ratio ?? 0));
  protected readonly icon = computed(() => CATEGORY_ICONS[this.d().category]);
  protected readonly host = computed(() => hostOf(this.d().url));
  protected readonly queueName = computed(() => {
    const id = this.d().queueId;
    return id === MAIN_QUEUE_ID ? this.i18n.t('queue.main') : (this.queues.get(id)?.name ?? '');
  });
  protected readonly activeConnections = computed(() => this.d().segments.filter((s) => s.state === 'active').length);
  protected readonly peak = computed(() => Math.max(0, ...this.d().speedHistory));

  /** Stable formatters for the tickers (they follow the language through the service). */
  protected readonly bytesFormat = (n: number): string => this.format.bytes(n);
  protected readonly speedFormat = (n: number): string => this.format.speed(n);

  protected readonly segmentSummary = computed(() => {
    const segs = this.d().segments;
    const done = segs.filter((s) => s.state === 'done').length;
    return this.i18n
      .t('details.segmentSummary')
      .replace('{done}', this.format.int(done))
      .replace('{total}', this.format.int(segs.length));
  });

  constructor() {
    effect((onCleanup) => {
      const d = this.d();
      if (d.kind !== 'torrent') {
        this.live.set(null);
        return;
      }
      const id = d.id;
      let alive = true;
      const poll = (): void => {
        this.engine.torrentLive(id).then(
          (v) => {
            if (alive) {
              this.live.set(v);
            }
          },
          () => undefined,
        );
      };
      poll();
      const timer = setInterval(poll, 1000);
      onCleanup(() => {
        alive = false;
        clearInterval(timer);
      });
    });
  }

  /** A readable timeline reconstructed from the record. */
  protected readonly log = computed<readonly LogLine[]>(() => {
    const d = this.d();
    const t = (key: string, vars: Record<string, string> = {}): string =>
      Object.entries(vars).reduce((s, [k, v]) => s.replace(`{${k}}`, v), this.i18n.t(key));
    const lines: LogLine[] = [{ at: d.addedAt, text: t('log.added', { host: this.host() }) }];
    if (d.startedAt !== null) {
      lines.push({ at: d.startedAt, text: t('log.probed', { size: this.format.bytes(d.size), ranges: this.i18n.t(d.supportsRanges ? 'log.rangesYes' : 'log.rangesNo') }) });
      lines.push({ at: d.startedAt + 300, text: t('log.planned', { n: this.format.int(d.segments.length), c: this.format.int(d.connections) }) });
    }
    if (d.state === 'paused') {
      lines.push({ at: Date.now(), text: t('log.paused') });
    }
    if (d.state === 'failed' && d.failure !== null) {
      lines.push({ at: Date.now(), text: t(`failure.${d.failure}`) });
    }
    if (d.state === 'verifying') {
      lines.push({ at: Date.now(), text: t('log.verifying') });
    }
    if (d.completedAt !== null) {
      lines.push({ at: d.completedAt, text: t('log.completed', { name: d.filename }) });
    }
    return lines.sort((a, b) => a.at - b.at);
  });
}
