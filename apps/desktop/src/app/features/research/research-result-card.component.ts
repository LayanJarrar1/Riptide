import { ChangeDetectionStrategy, Component, computed, inject, input, output, signal } from '@angular/core';

import { SeenNoticeComponent } from '../../ai/seen-notice.component';
import { CategoryId, safeWebUrl, shortUrl, SourceId } from '../../ai/research.models';
import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent, IconName } from '../../ui/icon.component';
import { isCitable } from './research-export';
import { TurnItem } from './research-history';
import { durationText, platformOf, reasonLines, ReasonLine } from './research-modes';

/** `pdf`, `zip`, ... from the file name or the address; `null` when there is none worth showing. */
export function fileType(it: TurnItem): string | null {
  const name = it.filename ?? '';
  let ext = /\.([A-Za-z0-9]{1,5})$/.exec(name)?.[1];
  if (ext === undefined) {
    try {
      ext = /\.([A-Za-z0-9]{1,5})$/.exec(new URL(it.downloadUrl).pathname)?.[1];
    } catch {
      ext = undefined;
    }
  }
  return ext === undefined ? null : ext.toUpperCase();
}

/** How a card is laid out: by what the user is looking for. */
export type CardVariant = 'video' | 'software' | 'document' | 'image' | 'code' | 'web';

export function variantOf(category: CategoryId): CardVariant {
  switch (category) {
    case 'videos':
      return 'video';
    case 'software':
      return 'software';
    case 'documents':
    case 'academic':
      return 'document';
    case 'images':
      return 'image';
    case 'open-source':
      return 'code';
    default:
      return 'web';
  }
}

const VARIANT_ICON: Readonly<Record<CardVariant, IconName>> = {
  video: 'fileVideo',
  software: 'fileApp',
  document: 'fileDoc',
  image: 'fileImage',
  code: 'code',
  web: 'globe',
};

/**
 * One result, laid out for what is being looked for: a video has its thumbnail and running time, a
 * program its publisher, version and where it comes from, a document its type, authors and year, an
 * image a large thumbnail, a project its owner and licence. Only what the source reported is shown;
 * nothing is guessed. Everything that came from a source is rendered as text (never as HTML) and only
 * `http(s)` addresses are ever acted on.
 */
@Component({
  selector: 'app-research-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, SeenNoticeComponent],
  template: `
    @let it = item();
    <article class="card" [class]="'card v-' + variant()" [class.is-selected]="selected()" [class.is-page]="it.kind === 'page'">
      @if (downloadable() && canSelect()) {
        <label class="pick">
          <input type="checkbox" [checked]="selected()" [attr.aria-label]="i18n.t('research.selectCard').replace('{title}', it.title)" (change)="pick.emit()" />
        </label>
      }
      @if (hasMedia()) {
        <div class="thumb" [class.is-empty]="thumb() === null">
          @if (thumb(); as src) {
            <img [src]="src" alt="" loading="lazy" decoding="async" referrerpolicy="no-referrer" (error)="thumbFailed.set(true)" />
          } @else {
            <app-icon [name]="icon()" [size]="26" />
          }
          @if (duration(); as d) {
            <span class="dur num">{{ d }}</span>
          }
        </div>
      } @else if (!(downloadable() && canSelect())) {
        <span class="pick page-mark" aria-hidden="true"><app-icon [name]="it.kind === 'page' && variant() === 'web' ? 'globe' : icon()" [size]="16" /></span>
      }
      <div class="body">
        <h4 class="title" dir="auto">{{ it.title }}</h4>
        <div class="meta">
          <span class="badge source">{{ i18n.t('research.source.' + it.source) }}</span>
          @if (showMatch()) {
            <span class="badge match" [class]="'badge match is-' + it.confidence">{{ i18n.t('research.match.' + it.confidence) }}</span>
          }
          @if (trustText(); as tt) {
            <span class="badge" [class]="'badge trust is-' + (it.mirror ? 'mirror' : it.trust)"><app-icon [name]="it.mirror ? 'alert' : 'shieldCheck'" [size]="12" /> {{ tt }}</span>
          }
          @if (variant() !== 'video' && variant() !== 'software' && variant() !== 'image' && type(); as ty) {
            <span class="badge">{{ ty }}</span>
          }
          @if (variant() === 'software') {
            @if (it.version) {
              <span class="badge" dir="ltr">v{{ it.version }}</span>
            }
            @if (platform(); as p) {
              <span class="badge">{{ i18n.t('research.platform.' + p) }}</span>
            }
          }
          @if (variant() !== 'software' && variant() !== 'video') {
            <span class="badge rights" [class]="'badge rights rights-' + it.rights.kind" dir="auto">{{ rightsText() }}</span>
          }
          @if (it.seen || it.wasSeen) {
            <span class="badge seen"><app-icon name="check" [size]="12" /> {{ i18n.t('seen.badge') }}</span>
          }
          @if (it.score !== null) {
            <span class="score num">{{ i18n.t('research.score').replace('{n}', fmt.int(it.score)) }}</span>
          }
        </div>
        @if (byline(); as b) {
          <p class="by"><bdi>{{ b }}</bdi></p>
        }
        @if (variant() === 'web' && it.kind === 'page') {
          <p class="url" dir="ltr">{{ pageText() }}</p>
        }
        @if (reasons().length > 0) {
          <p class="reasons">
            @for (r of reasons(); track $index) {
              <span class="reason" [class]="'reason is-' + r.tone" dir="auto">{{ reasonText(r) }}</span>
            }
          </p>
        }
        @if (it.why) {
          <p class="why" dir="auto"><app-icon name="sparkles" [size]="14" /> <span><strong>{{ i18n.t('research.why') }}:</strong> {{ it.why }}</span></p>
        }
        @if (it.summary && variant() !== 'image') {
          <p class="desc" dir="auto">{{ it.summary }}</p>
        }
        @if (alsoIn().length > 0) {
          <p class="also">{{ i18n.t('research.alsoIn') }} {{ alsoNames() }}</p>
        }
        @if (it.seen) {
          <app-seen-notice [match]="it.seen" [compact]="true" />
        }
        <div class="actions">
          @if (downloadable()) {
            <button appButton size="sm" variant="secondary" icon="download" type="button" [disabled]="!canDownload()" (click)="download.emit()">{{ i18n.t('research.download') }}</button>
          } @else {
            <button appButton size="sm" variant="secondary" icon="folderOpen" type="button" (click)="findFiles.emit()">{{ i18n.t('research.findFiles') }}</button>
          }
          @if (pageLink()) {
            <button appButton size="sm" variant="ghost" icon="external" type="button" (click)="openPage.emit()">{{ i18n.t(variant() === 'video' ? 'research.openVideo' : variant() === 'code' ? 'research.openRepo' : 'research.openPage') }}</button>
          }
          @if (variant() === 'document' && citable()) {
            <button appButton size="sm" variant="ghost" icon="book" type="button" (click)="copyCite.emit()">{{ i18n.t('research.citeOne') }}</button>
          }
          <button appButton size="sm" variant="ghost" icon="copy" type="button" [iconOnly]="true" [attr.aria-label]="i18n.t('research.copyLink')" [attr.title]="i18n.t('research.copyLink')" (click)="copyLink.emit()"></button>
        </div>
      </div>
    </article>
  `,
  styles: `
    .card {
      display: flex;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-4);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1, var(--color-surface-2));
      transition: border-color var(--dur-1) var(--ease-standard);
    }

    .card:hover {
      border-color: var(--color-border-strong);
    }

    .card.is-selected {
      border-color: var(--color-accent);
      background: var(--color-surface-2);
    }

    /* the box sits on the first line of the title, not above it */
    .pick {
      display: flex;
      align-items: center;
      flex: none;
      block-size: calc(var(--text-md) * var(--leading-snug, 1.35));
    }

    .page-mark {
      color: var(--color-text-muted);
      inline-size: 16px;
    }

    .pick input {
      margin: 0;
      inline-size: 16px;
      block-size: 16px;
      accent-color: var(--color-accent);
      cursor: pointer;
    }

    .thumb {
      position: relative;
      flex: none;
      inline-size: 148px;
      aspect-ratio: 16 / 9;
      align-self: flex-start;
      overflow: hidden;
      border-radius: var(--radius-sm);
      background: var(--color-surface-3);
      display: grid;
      place-items: center;
      color: var(--color-text-muted);
    }

    .thumb img {
      inline-size: 100%;
      block-size: 100%;
      object-fit: cover;
      display: block;
    }

    .dur {
      position: absolute;
      inset-block-end: 4px;
      inset-inline-end: 4px;
      padding: 1px 6px;
      border-radius: var(--radius-xs);
      background: rgb(0 0 0 / 78%);
      color: #fff;
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
    }

    .v-image {
      flex-direction: column;
    }

    .v-image .thumb {
      inline-size: 100%;
      aspect-ratio: 4 / 3;
    }

    .v-image .pick {
      position: absolute;
      z-index: 1;
      inset-block-start: calc(var(--space-3) + 6px);
      inset-inline-start: calc(var(--space-4) + 6px);
    }

    .v-image {
      position: relative;
    }

    .body {
      flex: 1;
      min-inline-size: 0;
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
    }

    .title {
      margin: 0;
      font-size: var(--text-md);
      font-weight: var(--weight-semibold);
      line-height: var(--leading-snug, 1.35);
      overflow-wrap: anywhere;
    }

    .meta {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-2);
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .badge {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 1px 8px;
      border-radius: var(--radius-full);
      font-weight: var(--weight-semibold);
      border: 1px solid var(--color-border-strong);
      color: var(--color-text-secondary);
    }

    .badge.source,
    .badge.rights-cc {
      border-color: var(--color-accent);
    }

    .badge.source {
      color: var(--color-accent-fg);
    }

    .badge.match.is-exact {
      background: var(--color-accent-soft);
      border-color: var(--color-accent);
      color: var(--color-accent-fg);
    }

    .badge.match.is-related {
      border-style: dashed;
      font-weight: var(--weight-medium);
      color: var(--color-text-muted);
    }

    .badge.trust.is-official,
    .badge.trust.is-primary,
    .badge.seen,
    .badge.rights-public-domain {
      border-color: var(--color-success);
      color: var(--color-success);
    }

    .badge.trust.is-mirror {
      border-color: var(--color-warning);
      color: var(--color-warning);
    }

    .badge.rights-unknown {
      border-style: dashed;
      font-weight: var(--weight-medium);
      color: var(--color-text-muted);
    }

    .score {
      margin-inline-start: auto;
      color: var(--color-accent-fg);
      font-weight: var(--weight-semibold);
    }

    .by {
      font-size: var(--text-xs);
      color: var(--color-text-secondary);
    }

    .url {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
      overflow-wrap: anywhere;
      text-align: start;
    }

    .reasons {
      display: flex;
      flex-wrap: wrap;
      gap: 0 var(--space-3);
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .reason.is-good {
      color: var(--color-success);
    }

    .reason.is-bad {
      color: var(--color-warning);
    }

    .why {
      display: flex;
      align-items: flex-start;
      gap: var(--space-2);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
      line-height: var(--leading-normal);
    }

    .why app-icon {
      color: var(--color-accent-fg);
      margin-block-start: 2px;
      flex-shrink: 0;
    }

    .desc {
      font-size: var(--text-sm);
      color: var(--color-text-muted);
      line-height: var(--leading-normal);
      display: -webkit-box;
      -webkit-line-clamp: 2;
      line-clamp: 2;
      -webkit-box-orient: vertical;
      overflow: hidden;
    }

    .also {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      margin-block-start: var(--space-1);
    }

    @media (max-width: 760px) {
      .thumb {
        inline-size: 108px;
      }
    }
  `,
})
export class ResearchCardComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly fmt = inject(FormatService);

  readonly item = input.required<TurnItem>();
  /** What is being looked for; it decides how the card is laid out. */
  readonly category = input<CategoryId>('documents');
  readonly selected = input(false);
  /** The results of this turn can still be added (the backend holds them). */
  readonly canDownload = input(true);
  readonly canSelect = input(true);
  /** Other sources that returned the same result (deep research). */
  readonly alsoIn = input<readonly SourceId[]>([]);

  readonly pick = output<void>();
  readonly download = output<void>();
  readonly findFiles = output<void>();
  readonly openPage = output<void>();
  readonly copyLink = output<void>();
  readonly copyCite = output<void>();

  protected readonly thumbFailed = signal(false);
  protected readonly variant = computed(() => variantOf(this.category()));
  protected readonly icon = computed(() => VARIANT_ICON[this.variant()]);
  /** A file can be added as it is; so can a YouTube page (the engine downloads it as media). */
  protected readonly downloadable = computed(() => this.item().kind === 'file' || this.item().source === 'youtube');
  protected readonly type = computed(() => fileType(this.item()));
  protected readonly pageLink = computed(() => safeWebUrl(this.item().pageUrl ?? (this.item().kind === 'page' ? this.item().downloadUrl : null)));
  protected readonly thumb = computed(() => (this.thumbFailed() ? null : safeWebUrl(this.item().thumbnail)));
  protected readonly hasMedia = computed(() => this.variant() === 'video' || this.variant() === 'image');
  protected readonly duration = computed(() => (this.variant() === 'video' ? durationText(this.item().durationSecs) : null));
  protected readonly platform = computed(() => platformOf(this.item().filename, this.item().downloadUrl));
  protected readonly citable = computed(() => isCitable(this.item()));
  protected readonly reasons = computed(() => reasonLines(this.item().reasons));
  protected readonly showMatch = computed(() => this.variant() === 'video' || this.item().confidence !== 'exact');
  protected readonly alsoNames = computed(() => this.alsoIn().map((s) => this.i18n.t(`research.source.${s}`)).join(', '));

  protected readonly pageText = computed(() => {
    const safe = safeWebUrl(this.item().downloadUrl);
    return safe === null ? '' : shortUrl(safe);
  });

  /** Official or mirror, only when the program established it. */
  protected readonly trustText = computed(() => {
    const it = this.item();
    if (this.variant() !== 'software') {
      return null;
    }
    if (it.mirror || it.trust === 'mirror') {
      return this.i18n.t('research.trust.mirror');
    }
    return it.trust === 'official' || it.trust === 'primary' ? this.i18n.t(`research.trust.${it.trust}`) : null;
  });

  /** The line under the title: who made it and, where it matters, when and how big. */
  protected readonly byline = computed(() => {
    const it = this.item();
    const v = this.variant();
    const authors = it.authors.slice(0, 3).join(', ') + (it.authors.length > 3 ? ' …' : '');
    const who = v === 'document' || v === 'web' ? authors : (it.publisher ?? authors);
    const year = v === 'software' || v === 'code' || it.year === null ? '' : String(it.year);
    const size = v === 'video' || v === 'code' || it.size === null ? '' : this.fmt.bytes(it.size);
    return [who, year, size].filter((p) => p.length > 0).join(' · ');
  });

  protected reasonText(r: ReasonLine): string {
    return Object.entries(r.params).reduce((text, [k, v]) => text.replace(`{${k}}`, v), this.i18n.t(r.key));
  }

  protected rightsText(): string {
    const r = this.item().rights;
    switch (r.kind) {
      case 'public-domain':
        return this.i18n.t('research.rights.public-domain');
      case 'cc':
        return r.label;
      case 'open-source':
        return this.i18n.t('research.rights.open-source').replace('{label}', r.label);
      default:
        return this.i18n.t('research.rights.unknown');
    }
  }
}
