import { ChangeDetectionStrategy, Component, computed, inject, OnInit, signal } from '@angular/core';
import { Router } from '@angular/router';

import { FORM_STYLES } from '../../ai/ai-styles';
import { AiMessage, ResultItem, SearchResponse, SOURCE_IDS, SourceFailure, SourceId } from '../../ai/ai.models';
import { AiService } from '../../ai/ai.service';
import { SeenNoticeComponent } from '../../ai/seen-notice.component';
import { FormatService } from '../../core/format.service';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS } from '../../model/download';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';

/**
 * Research: type a topic, get open-access papers, datasets and files with direct download
 * links, from OpenAlex, Crossref, arXiv, Europe PMC, Internet Archive, Wikimedia Commons,
 * GitHub releases and Data.gov. With an AI provider set up, the question is expanded into
 * queries and the results are ranked with a one-line reason each; without one, the same
 * sources are searched with the user's own words. Selected results become normal downloads.
 */
@Component({
  selector: 'app-research-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, SeenNoticeComponent],
  template: `
    <div class="page">
      <header class="page-head">
        <h1 class="page-title">{{ i18n.t('research.title') }}</h1>
        <p class="page-sub">{{ i18n.t('research.subtitle') }}</p>
      </header>

      @if (!ai.available) {
        <p class="note">{{ i18n.t('research.desktopOnly') }}</p>
      } @else {
        <form class="search" role="search" (submit)="search($event)">
          <div class="search-box">
            <app-icon class="search-icon" name="search" [size]="18" />
            <input
              class="search-input"
              type="search"
              name="topic"
              dir="auto"
              autocomplete="off"
              [placeholder]="i18n.t('research.placeholder')"
              [attr.aria-label]="i18n.t('research.title')"
              [value]="topic()"
              (input)="topic.set(value($event))"
            />
          </div>
          <button appButton variant="primary" icon="search" type="submit" [busy]="searching()" [disabled]="topic().trim().length === 0 || searching()">
            {{ i18n.t(searching() ? 'research.searching' : 'research.search') }}
          </button>
        </form>

        <div class="options">
          @if (aiOn()) {
            <label class="check">
              <span class="switch" [class.is-on]="useAi()">
                <input type="checkbox" role="switch" [checked]="useAi()" (change)="useAi.set(checked($event))" />
                <span class="knob"></span>
              </span>
              <span>{{ i18n.t('research.useAi') }}</span>
            </label>
          } @else {
            <div class="keyword-mode" role="note">
              <app-icon name="info" [size]="16" />
              <span>{{ i18n.t('research.keywordMode') }}</span>
              <button appButton size="sm" variant="secondary" icon="sparkles" type="button" (click)="openAiSettings()">{{ i18n.t('research.openAiSettings') }}</button>
            </div>
          }
        </div>

        <div class="sources" role="group" [attr.aria-label]="i18n.t('research.sources')">
          @for (s of sourceIds; track s) {
            <button type="button" class="source-chip" [class.is-active]="activeSources().has(s)" [attr.aria-pressed]="activeSources().has(s)" (click)="toggleSource(s)">{{ sourceLabel(s) }}</button>
          }
        </div>

        @if (aiOn() && useAi()) {
          <details class="preview" (toggle)="onPreviewToggle($event)">
            <summary class="preview-summary">
              <app-icon name="chevronEnd" [size]="14" class="chev" />
              {{ i18n.t('research.preview') }}
            </summary>
            <p class="hint">{{ i18n.t('research.previewDesc') }}</p>
            @for (m of preview(); track $index) {
              <pre class="msg" dir="auto">{{ m.content }}</pre>
            }
          </details>
        }

        @if (searching()) {
          <div class="progress" role="progressbar" [attr.aria-label]="i18n.t('research.searching')"><span class="bar"></span></div>
        }

        @if (response(); as r) {
          <div class="summary" role="status">
            <span class="count num">{{ i18n.t('research.count').replace('{n}', fmt.int(r.items.length)) }}</span>
            @if (r.queries.length > 0 && (r.queries.length > 1 || r.queries[0] !== lastTopic())) {
              <span class="queries">{{ i18n.t('research.queries') }}:
                @for (q of r.queries; track q) {
                  <span class="query" dir="auto">{{ q }}</span>
                }
              </span>
            }
            @if (r.ai.used) {
              <span class="ai-note"><app-icon name="sparkles" [size]="14" /> {{ i18n.t('research.ai.used').replace('{n}', fmt.int(r.ai.tokens)) }}</span>
            }
          </div>
          @if (r.ai.error; as e) {
            <p class="warn" role="alert"><app-icon name="alert" [size]="14" /> {{ i18n.t('research.ai.failed') }} {{ ai.failText(e) }}</p>
          }
          @if (r.ai.dropped > 0) {
            <p class="warn"><app-icon name="shield" [size]="14" /> {{ i18n.t('research.ai.dropped').replace('{n}', fmt.int(r.ai.dropped)) }}</p>
          }
          @for (f of r.failures; track f.source) {
            <p class="warn"><app-icon name="alert" [size]="14" /> {{ failureText(f) }}</p>
          }

          @if (r.items.length === 0) {
            <p class="empty">{{ i18n.t('research.none') }}</p>
          } @else {
            <ul class="results">
              @for (it of r.items; track it.id) {
                <li class="card" [class.is-selected]="selected().has(it.id)">
                  <label class="pick">
                    <input type="checkbox" [checked]="selected().has(it.id)" [attr.aria-label]="i18n.t('research.selectCard').replace('{title}', it.title)" (change)="toggle(it.id)" />
                  </label>
                  <div class="card-body">
                    <h3 class="card-title" dir="auto">{{ it.title }}</h3>
                    <div class="meta">
                      <span class="badge source">{{ sourceLabel(it.source) }}</span>
                      @if (it.seen) {
                        <span class="badge seen"><app-icon name="check" [size]="12" /> {{ i18n.t('seen.badge') }}</span>
                      }
                      @if (byline(it); as b) {
                        <span class="byline" dir="auto">{{ b }}</span>
                      }
                      @if (it.size !== null) {
                        <span class="num">{{ fmt.bytes(it.size) }}</span>
                      }
                      <span class="license" [class.is-none]="!it.license" dir="auto">{{ it.license ? i18n.t('research.licence') + ': ' + it.license : i18n.t('research.noLicence') }}</span>
                      @if (it.score !== null) {
                        <span class="score num">{{ i18n.t('research.score').replace('{n}', fmt.int(it.score)) }}</span>
                      }
                    </div>
                    @if (it.why) {
                      <p class="why" dir="auto"><app-icon name="sparkles" [size]="14" /> <span><strong>{{ i18n.t('research.why') }}:</strong> {{ it.why }}</span></p>
                    } @else if (it.summary) {
                      <p class="summary-text" dir="auto">{{ it.summary }}</p>
                    }
                    @if (it.seen) {
                      <app-seen-notice [match]="it.seen" [compact]="true" />
                    }
                  </div>
                </li>
              }
            </ul>

            <div class="bar-actions" role="group">
              <div class="bar-left">
                <button appButton size="sm" variant="ghost" type="button" (click)="selectAll()">{{ i18n.t('research.selectAll') }}</button>
                <button appButton size="sm" variant="ghost" type="button" [disabled]="selected().size === 0" (click)="selected.set(emptySet)">{{ i18n.t('research.selectNone') }}</button>
              </div>
              <div class="bar-right">
                <label class="mini">
                  <span>{{ i18n.t('research.queue') }}</span>
                  <select class="input small" (change)="queueId.set(numberOrNull($event))">
                    <option value="">{{ i18n.t('queue.main') }}</option>
                    @for (q of queues.queues(); track q.id) {
                      <option [value]="q.id">{{ q.name }}</option>
                    }
                  </select>
                </label>
                <label class="mini">
                  <span>{{ i18n.t('research.category') }}</span>
                  <select class="input small" (change)="category.set(value($event))">
                    <option value="">{{ i18n.t('research.categoryAuto') }}</option>
                    @for (c of categories; track c) {
                      <option [value]="c">{{ i18n.t('category.' + c) }}</option>
                    }
                  </select>
                </label>
                <label class="mini check-mini">
                  <input type="checkbox" [checked]="later()" (change)="later.set(checked($event))" />
                  <span>{{ i18n.t('research.startLater') }}</span>
                </label>
                <button appButton variant="primary" icon="download" type="button" [busy]="adding()" [disabled]="selected().size === 0 || adding()" (click)="addSelected()">
                  {{ i18n.t('research.addSelected').replace('{n}', fmt.int(selected().size)) }}
                </button>
              </div>
            </div>
          }
        } @else if (!searching()) {
          <p class="empty">{{ i18n.t('research.start') }}</p>
        }
      }
    </div>
  `,
  styles: [
    FORM_STYLES,
    `
      :host {
        display: block;
        block-size: 100%;
        overflow: auto;
      }

      .page {
        max-inline-size: 980px;
        padding: var(--space-8) var(--space-8) var(--space-16);
        margin-inline: auto;
      }

      .page-title {
        font-size: var(--text-2xl);
        font-weight: var(--weight-semibold);
        letter-spacing: var(--tracking-tight);
      }

      .page-sub,
      .hint,
      .empty,
      .note {
        color: var(--color-text-muted);
        margin-block-start: var(--space-1);
      }

      .empty {
        padding-block: var(--space-10, 48px);
        text-align: center;
      }

      .search {
        display: flex;
        gap: var(--space-3);
        margin-block-start: var(--space-6);
      }

      .search-box {
        position: relative;
        flex: 1;
        display: flex;
        align-items: center;
      }

      .search-icon {
        position: absolute;
        inset-inline-start: 12px;
        color: var(--color-text-muted);
        pointer-events: none;
      }

      .search-input {
        inline-size: 100%;
        block-size: 44px;
        padding-inline: 40px 14px;
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-md);
        background: var(--color-surface-2);
        color: var(--color-text);
        font: inherit;
      }

      .search-input:focus {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px var(--color-accent-ring);
      }

      .options {
        margin-block-start: var(--space-3);
      }

      .check {
        display: inline-flex;
        align-items: center;
        gap: var(--space-3);
        cursor: pointer;
        font-size: var(--text-sm);
      }

      .keyword-mode {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        flex-wrap: wrap;
        padding: var(--space-3);
        border: 1px dashed var(--color-border-strong);
        border-radius: var(--radius-sm);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
      }

      .sources {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-2);
        margin-block-start: var(--space-3);
      }

      .source-chip {
        block-size: 28px;
        padding-inline: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: var(--text-xs);
        font-weight: var(--weight-medium);
        cursor: pointer;
      }

      .source-chip.is-active {
        border-color: var(--color-accent);
        color: var(--color-text);
        background: var(--color-accent-soft, var(--color-surface-3));
      }

      .preview {
        margin-block-start: var(--space-3);
        font-size: var(--text-sm);
      }

      .preview-summary {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        cursor: pointer;
        list-style: none;
        color: var(--color-text-secondary);
        user-select: none;
      }

      .preview-summary::-webkit-details-marker {
        display: none;
      }

      .chev {
        transition: transform var(--motion-fast) var(--ease-standard);
      }

      .preview[open] .chev {
        transform: rotate(calc(var(--dir-sign) * 90deg));
      }

      .msg {
        margin: var(--space-2) 0 0;
        padding: var(--space-2) var(--space-3);
        max-block-size: 12rem;
        overflow: auto;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        background: var(--color-surface-2);
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: var(--text-xs);
      }

      .progress {
        block-size: 3px;
        margin-block-start: var(--space-4);
        border-radius: 2px;
        background: var(--color-surface-3);
        overflow: hidden;
      }

      .bar {
        display: block;
        block-size: 100%;
        inline-size: 35%;
        background: var(--color-accent);
        animation: slide 1.1s var(--ease-standard) infinite;
      }

      @keyframes slide {
        from {
          transform: translateX(calc(var(--dir-sign) * -100%));
        }
        to {
          transform: translateX(calc(var(--dir-sign) * 290%));
        }
      }

      @media (prefers-reduced-motion: reduce) {
        .bar {
          animation: none;
          inline-size: 100%;
          opacity: 0.5;
        }
      }

      .summary {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-4);
        margin-block-start: var(--space-5);
        font-size: var(--text-sm);
        color: var(--color-text-secondary);
      }

      .count {
        font-weight: var(--weight-semibold);
        color: var(--color-text);
      }

      .query {
        display: inline-block;
        margin-inline-start: var(--space-2);
        padding: 1px 8px;
        border-radius: var(--radius-full);
        background: var(--color-surface-3);
      }

      .ai-note {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        color: var(--color-accent);
      }

      .warn {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        margin-block-start: var(--space-2);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
      }

      .results {
        list-style: none;
        margin: var(--space-4) 0 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
      }

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

      .pick {
        padding-block-start: 2px;
      }

      .pick input {
        inline-size: 16px;
        block-size: 16px;
        accent-color: var(--color-accent);
        cursor: pointer;
      }

      .card-body {
        flex: 1;
        min-inline-size: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
      }

      .card-title {
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

      .badge.source {
        border-color: var(--color-accent);
        color: var(--color-accent);
      }

      .badge.seen {
        border-color: var(--color-success);
        color: var(--color-success);
      }

      .license.is-none {
        font-style: italic;
      }

      .score {
        margin-inline-start: auto;
        color: var(--color-accent);
        font-weight: var(--weight-semibold);
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

      .summary-text {
        font-size: var(--text-sm);
        color: var(--color-text-muted);
        line-height: var(--leading-normal);
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
      }

      .bar-actions {
        position: sticky;
        inset-block-end: 0;
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: var(--space-3);
        margin-block-start: var(--space-4);
        padding: var(--space-3) var(--space-4);
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-md);
        background: var(--color-surface-2);
        box-shadow: 0 -6px 18px rgb(0 0 0 / 0.12);
      }

      .bar-left,
      .bar-right {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-3);
      }

      .mini {
        display: inline-flex;
        align-items: center;
        gap: var(--space-2);
        font-size: var(--text-xs);
        color: var(--color-text-muted);
      }

      .input.small {
        block-size: 30px;
        inline-size: auto;
        min-inline-size: 120px;
      }

      .check-mini input {
        accent-color: var(--color-accent);
      }

      @media (max-width: 760px) {
        .page {
          padding-inline: var(--space-4);
        }

        .search {
          flex-direction: column;
        }
      }
    `,
  ],
})
export class ResearchPageComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly fmt = inject(FormatService);
  protected readonly queues = inject(QueuesService);
  private readonly router = inject(Router);
  private readonly toasts = inject(ToastService);

  protected readonly sourceIds = SOURCE_IDS;
  protected readonly categories = CATEGORY_KEYS;
  protected readonly emptySet: ReadonlySet<string> = new Set<string>();

  protected readonly topic = signal('');
  protected readonly lastTopic = signal('');
  protected readonly useAi = signal(true);
  protected readonly activeSources = signal<ReadonlySet<SourceId>>(new Set(SOURCE_IDS));
  protected readonly searching = signal(false);
  protected readonly adding = signal(false);
  protected readonly response = signal<SearchResponse | null>(null);
  protected readonly selected = signal<ReadonlySet<string>>(this.emptySet);
  protected readonly preview = signal<readonly AiMessage[]>([]);
  protected readonly queueId = signal<number | null>(null);
  protected readonly category = signal('');
  protected readonly later = signal(false);

  /** The AI is set up (provider + key + model). */
  protected readonly aiOn = computed(() => this.ai.status()?.configured === true);

  ngOnInit(): void {
    if (this.ai.available) {
      void this.ai.refresh();
    }
  }

  /** Settings, scrolled to the AI section. */
  protected openAiSettings(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('ai')?.scrollIntoView({ block: 'start' }), 50);
    });
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLSelectElement).value;
  }

  protected checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked;
  }

  protected numberOrNull(event: Event): number | null {
    const n = Number.parseInt(this.value(event), 10);
    return Number.isFinite(n) ? n : null;
  }

  protected sourceLabel(s: SourceId): string {
    return this.i18n.t(`research.source.${s}`);
  }

  protected byline(it: ResultItem): string {
    const who = it.authors.slice(0, 3).join(', ') + (it.authors.length > 3 ? ' …' : '');
    return [who, it.year === null ? '' : String(it.year)].filter((p) => p.length > 0).join(' · ');
  }

  protected failureText(f: SourceFailure): string {
    return this.i18n
      .t('research.source.failed')
      .replace('{source}', this.sourceLabel(f.source))
      .replace('{reason}', this.i18n.t(`research.err.${f.code}`));
  }

  protected toggleSource(s: SourceId): void {
    const next = new Set(this.activeSources());
    if (next.has(s)) {
      if (next.size > 1) {
        next.delete(s);
      }
    } else {
      next.add(s);
    }
    this.activeSources.set(next);
  }

  protected toggle(id: string): void {
    const next = new Set(this.selected());
    if (!next.delete(id)) {
      next.add(id);
    }
    this.selected.set(next);
  }

  protected selectAll(): void {
    this.selected.set(new Set(this.response()?.items.map((i) => i.id) ?? []));
  }

  protected onPreviewToggle(event: Event): void {
    if ((event.target as HTMLDetailsElement).open) {
      void this.loadPreview();
    }
  }

  private async loadPreview(): Promise<void> {
    try {
      this.preview.set(await this.ai.previewSearch(this.topic().trim() || '…'));
    } catch {
      this.preview.set([]);
    }
  }

  protected async search(event: Event): Promise<void> {
    event.preventDefault();
    const topic = this.topic().trim();
    if (topic.length === 0 || this.searching()) {
      return;
    }
    this.searching.set(true);
    this.selected.set(this.emptySet);
    try {
      const all = this.activeSources().size === SOURCE_IDS.length;
      const r = await this.ai.search({
        topic,
        useAi: this.useAi() && this.aiOn(),
        lang: this.i18n.lang(),
        sources: all ? undefined : [...this.activeSources()],
      });
      this.lastTopic.set(topic);
      this.response.set(r);
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.searching.set(false);
    }
  }

  protected async addSelected(): Promise<void> {
    const ids = [...this.selected()];
    if (ids.length === 0 || this.adding()) {
      return;
    }
    this.adding.set(true);
    try {
      const r = await this.ai.addResults({
        ids,
        queueId: this.queueId(),
        category: this.category() || null,
        startPaused: this.later(),
        allowDuplicates: true,
      });
      const text =
        r.skipped.length === 0
          ? this.i18n.t('research.added').replace('{n}', this.fmt.int(r.added.length))
          : this.i18n.t('research.addedSome').replace('{n}', this.fmt.int(r.added.length)).replace('{skipped}', this.fmt.int(r.skipped.length));
      this.toasts.show(r.added.length > 0 ? 'success' : 'danger', r.added.length > 0 ? text : this.i18n.t('research.addFailed'));
      this.selected.set(this.emptySet);
      await this.recheckSeen();
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.adding.set(false);
    }
  }

  /** After adding, the cards of what was just added are not "downloaded" yet; refresh the badges. */
  private async recheckSeen(): Promise<void> {
    const r = this.response();
    if (r === null) {
      return;
    }
    try {
      const matches = await this.ai.seenCheck(
        r.items.map((i) => ({
          url: i.downloadUrl,
          doi: i.doi ?? undefined,
          arxiv: i.arxiv ?? undefined,
          pmid: i.pmid ?? undefined,
          pmcid: i.pmcid ?? undefined,
          filename: i.size !== null ? (i.filename ?? undefined) : undefined,
          size: i.size ?? undefined,
        })),
      );
      this.response.set({ ...r, items: r.items.map((it, n) => ({ ...it, seen: matches[n] ?? null })) });
    } catch {
      /* the badges stay as they were */
    }
  }
}
