import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { SeenNoticeComponent } from '../../ai/seen-notice.component';
import { safeWebUrl, shortUrl } from '../../ai/research.models';
import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { TurnItem } from './research-history';

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

/**
 * One result: title, source, description, file type, size, year, licence, why it matches, an
 * "already downloaded" badge, the source page and a download button. Everything that came from a
 * source is rendered as text (never as HTML) and only `http(s)` addresses are ever acted on.
 */
@Component({
  selector: 'app-research-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, SeenNoticeComponent],
  template: `
    @let it = item();
    <article class="card" [class.is-selected]="selected()" [class.is-page]="it.kind === 'page'">
      @if (it.kind === 'file' && canSelect()) {
        <label class="pick">
          <input type="checkbox" [checked]="selected()" [attr.aria-label]="i18n.t('research.selectCard').replace('{title}', it.title)" (change)="pick.emit()" />
        </label>
      } @else {
        <span class="pick page-mark" aria-hidden="true"><app-icon [name]="it.kind === 'page' ? 'globe' : 'file'" [size]="16" /></span>
      }
      <div class="body">
        <h4 class="title" dir="auto">{{ it.title }}</h4>
        <div class="meta">
          <span class="badge source">{{ i18n.t('research.source.' + it.source) }}</span>
          @if (type(); as ty) {
            <span class="badge">{{ ty }}</span>
          }
          <span class="badge rights" [class]="'rights-' + it.rights.kind" dir="auto">{{ rightsText() }}</span>
          @if (it.seen || it.wasSeen) {
            <span class="badge seen"><app-icon name="check" [size]="12" /> {{ i18n.t('seen.badge') }}</span>
          }
          @if (byline(); as b) {
            <span dir="auto">{{ b }}</span>
          }
          @if (it.size !== null) {
            <span class="num">{{ fmt.bytes(it.size) }}</span>
          }
          @if (it.score !== null) {
            <span class="score num">{{ i18n.t('research.score').replace('{n}', fmt.int(it.score)) }}</span>
          }
        </div>
        @if (it.kind === 'page') {
          <p class="url" dir="ltr">{{ pageText() }}</p>
        }
        @if (it.why) {
          <p class="why" dir="auto"><app-icon name="sparkles" [size]="14" /> <span><strong>{{ i18n.t('research.why') }}:</strong> {{ it.why }}</span></p>
        }
        @if (it.summary) {
          <p class="desc" dir="auto">{{ it.summary }}</p>
        }
        @if (it.seen) {
          <app-seen-notice [match]="it.seen" [compact]="true" />
        }
        <div class="actions">
          @if (it.kind === 'file') {
            <button appButton size="sm" variant="secondary" icon="download" type="button" [disabled]="!canDownload()" (click)="download.emit()">{{ i18n.t('research.download') }}</button>
          } @else {
            <button appButton size="sm" variant="secondary" icon="folderOpen" type="button" (click)="findFiles.emit()">{{ i18n.t('research.findFiles') }}</button>
          }
          @if (pageLink()) {
            <button appButton size="sm" variant="ghost" icon="external" type="button" (click)="openPage.emit()">{{ i18n.t('research.openPage') }}</button>
          }
          <button appButton size="sm" variant="ghost" icon="copy" type="button" (click)="copyLink.emit()">{{ i18n.t('research.copyLink') }}</button>
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

    .body {
      flex: 1;
      min-inline-size: 0;
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
    }

    .title {
      font-size: var(--text-md);
      font-weight: var(--weight-semibold);
      line-height: var(--leading-snug, 1.35);
      overflow-wrap: anywhere;
    }

    .meta {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-2) var(--space-3);
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
      color: var(--color-accent);
    }

    .badge.seen,
    .badge.rights-public-domain {
      border-color: var(--color-success);
      color: var(--color-success);
    }

    .badge.rights-unknown {
      border-style: dashed;
      font-weight: var(--weight-medium);
      color: var(--color-text-muted);
    }

    .score {
      margin-inline-start: auto;
      color: var(--color-accent);
      font-weight: var(--weight-semibold);
    }

    .url {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
      overflow-wrap: anywhere;
      text-align: start;
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
      color: var(--color-accent);
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

    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      margin-block-start: var(--space-1);
    }
  `,
})
export class ResearchCardComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly fmt = inject(FormatService);

  readonly item = input.required<TurnItem>();
  readonly selected = input(false);
  /** The results of this turn can still be added (the backend holds them). */
  readonly canDownload = input(true);
  readonly canSelect = input(true);

  readonly pick = output<void>();
  readonly download = output<void>();
  readonly findFiles = output<void>();
  readonly openPage = output<void>();
  readonly copyLink = output<void>();

  protected readonly type = computed(() => fileType(this.item()));
  protected readonly pageLink = computed(() => safeWebUrl(this.item().pageUrl ?? (this.item().kind === 'page' ? this.item().downloadUrl : null)));

  protected readonly pageText = computed(() => {
    const safe = safeWebUrl(this.item().downloadUrl);
    return safe === null ? '' : shortUrl(safe);
  });

  protected readonly byline = computed(() => {
    const it = this.item();
    const who = it.authors.slice(0, 3).join(', ') + (it.authors.length > 3 ? ' …' : '');
    return [who, it.year === null ? '' : String(it.year)].filter((p) => p.length > 0).join(' · ');
  });

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
