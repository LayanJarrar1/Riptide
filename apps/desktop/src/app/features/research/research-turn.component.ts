import { ChangeDetectionStrategy, Component, computed, inject, input, output, signal } from '@angular/core';
import { Router } from '@angular/router';

import { AiService } from '../../ai/ai.service';
import { safeWebUrl, SourceId } from '../../ai/research.models';
import { SourceStatus } from '../../ai/research.session';
import { FormatService } from '../../core/format.service';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { ToastService } from '../../ui/toast.service';
import { UiState } from '../shell/ui-state.service';
import { exportName, ExportContext, responseText, summaryLines, toBibtex, toMarkdown, toRis } from './research-export';
import { Turn, TurnItem } from './research-history';
import { ResearchCardComponent } from './research-result-card.component';
import { deriveSteps, summarize } from './research-turn';

/** Cards shown before "Show all"; a turn keeps at most 60. */
const FIRST_PAGE = 20;

/**
 * One prompt and its answer: the message, the progress steps (driven by the real events), the
 * source chips, a short summary composed from the actual results, the result cards and the
 * actions that work on them. It renders a stored `Turn`, so a live search and one restored from
 * history look the same; only a live turn can be cancelled, retried or added to downloads.
 */
@Component({
  selector: 'app-research-turn',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, MenuComponent, ResearchCardComponent],
  template: `
    @let t = turn();
    <section class="turn" [attr.aria-label]="t.prompt">
      <div class="user">
        <p class="bubble" dir="auto">{{ t.prompt }}</p>
        <span class="kind">{{ i18n.t('research.cat.' + t.category) }}{{ t.useAi ? ' · ' + i18n.t('research.aiBadge') : '' }}</span>
      </div>

      <div class="assistant">
        <ol class="steps" [attr.aria-label]="i18n.t('research.steps')" aria-live="polite">
          @for (s of steps(); track s.id) {
            <li class="step" [class]="'is-' + s.state">
              <span class="mark" aria-hidden="true">
                @switch (s.state) {
                  @case ('done') {
                    <app-icon name="check" [size]="12" />
                  }
                  @case ('failed') {
                    <app-icon name="alert" [size]="12" />
                  }
                  @case ('active') {
                    <span class="spin"></span>
                  }
                  @case ('skipped') {
                    <span class="dash"></span>
                  }
                  @case ('stopped') {
                    <app-icon name="stop" [size]="10" />
                  }
                  @default {
                    <span class="dot"></span>
                  }
                }
              </span>
              <span class="label">{{ stepLabel(s.id, s.state) }}</span>
              @if (s.count !== null && s.state !== 'skipped') {
                <span class="count num">{{ fmt.int(s.count) }}</span>
              }
              <span class="sr-only">{{ i18n.t('research.stepState.' + s.state) }}</span>
            </li>
          }
        </ol>

        @if (running() && live()) {
          <button appButton size="sm" variant="secondary" icon="stop" type="button" class="stop" (click)="stopSearch.emit()">{{ i18n.t('research.cancel') }}</button>
        }

        @if (t.sources.length > 0 || t.unavailable.length > 0) {
          <div class="chips" role="group" [attr.aria-label]="i18n.t('research.sources')">
            @for (s of t.sources; track s.id) {
              <span class="chip" [class.is-busy]="s.state === 'searching'" [class.is-failed]="s.state === 'failed'" [class.is-stopped]="s.state === 'stopped'" [class.is-active]="only() === s.id">
                <button type="button" class="chip-main" [attr.aria-pressed]="only() === s.id" [attr.aria-label]="chipLabel(s)" [disabled]="s.found === 0" (click)="toggleOnly(s.id)">
                  <span class="chip-dot" aria-hidden="true"></span>
                  <span>{{ i18n.t('research.source.' + s.id) }}</span>
                  <span class="num chip-state">{{ chipState(s) }}</span>
                </button>
                @if (s.state === 'failed' && live()) {
                  <button type="button" class="chip-retry" [attr.aria-label]="i18n.t('research.retry').replace('{source}', i18n.t('research.source.' + s.id))" (click)="retrySource.emit(s.id)">
                    <app-icon name="retry" [size]="13" />
                  </button>
                }
              </span>
            }
            @for (u of t.unavailable; track u) {
              <span class="chip is-setup">
                <span>{{ i18n.t('research.source.' + u) }}</span>
                <button type="button" class="chip-setup" (click)="openResearchSettings()">{{ i18n.t('research.unavailable.' + u) }}</button>
              </span>
            }
          </div>
        }

        @if (t.plan.length > 0) {
          <p class="plan">
            <app-icon name="sparkles" [size]="14" />
            <span>{{ i18n.t('research.queries') }}:</span>
            @for (q of t.plan; track q) {
              <span class="query" dir="auto">{{ q }}</span>
            }
          </p>
        }

        @if (t.ai?.failed && t.ai?.failMessage; as msg) {
          <div class="warn" role="alert">
            <app-icon name="alert" [size]="14" />
            <span>{{ i18n.t('research.ai.failed') }} <span dir="auto">{{ msg }}</span></span>
            @if (live() || !running()) {
              <button appButton size="sm" variant="ghost" type="button" (click)="rerun.emit(false)">{{ i18n.t('research.retryNoAi') }}</button>
            }
          </div>
        }
        @if (t.ai && t.ai.dropped > 0) {
          <p class="warn"><app-icon name="shield" [size]="14" /> {{ i18n.t('research.ai.dropped').replace('{n}', fmt.int(t.ai.dropped)) }}</p>
        }

        @if (t.error) {
          <div class="warn is-error" role="alert">
            <app-icon name="alert" [size]="14" />
            <span dir="auto">{{ t.error }}</span>
            <button appButton size="sm" variant="secondary" icon="retry" type="button" (click)="rerun.emit(t.useAi)">{{ i18n.t('research.tryAgain') }}</button>
          </div>
        }

        @if (t.reused) {
          <p class="note"><app-icon name="info" [size]="14" /> {{ i18n.t('research.reused') }} <button appButton size="sm" variant="ghost" icon="retry" type="button" (click)="rerun.emit(t.useAi)">{{ i18n.t('research.searchAgain') }}</button></p>
        }

        @if (t.status === 'cancelled') {
          <p class="note">{{ i18n.t('research.cancelled') }}</p>
        }

        @if (t.items.length > 0 || (!running() && !t.error)) {
          <div class="answer" role="status">
            @for (line of lines(); track $index) {
              <p dir="auto">{{ line }}</p>
            }
            @if (top().length > 0 && !running()) {
              <p class="top-title">{{ i18n.t(ranked() ? 'research.sum.top' : 'research.sum.first') }}</p>
              <ol class="top">
                @for (p of top(); track p.id) {
                  <li dir="auto"><strong>{{ p.title }}</strong>@if (p.why) {: {{ p.why }}}</li>
                }
              </ol>
            }
          </div>
        }

        @if (t.items.length > 0) {
          @if (only(); as o) {
            <div class="filter">
              <button appButton size="sm" variant="ghost" type="button" (click)="only.set(null)">{{ i18n.t('research.showAll') }} ({{ i18n.t('research.source.' + o) }} ×)</button>
            </div>
          }
          <div class="results">
            @for (it of shown(); track it.id) {
              <app-research-card
                [item]="it"
                [selected]="selected().has(it.id)"
                [canDownload]="canAdd() && !adding()"
                [canSelect]="canAdd()"
                (pick)="toggle(it.id)"
                (download)="addIds([it.id])"
                (findFiles)="findFiles(it)"
                (openPage)="openPage(it)"
                (copyLink)="copyLink(it)"
              />
            }
          </div>
          @if (hidden() > 0) {
            <button appButton size="sm" variant="ghost" type="button" class="more" (click)="showAll.set(true)">{{ i18n.t('research.showMore').replace('{n}', fmt.int(hidden())) }}</button>
          }

          @if (!canAdd() && !running()) {
            <p class="note"><app-icon name="info" [size]="14" /> {{ i18n.t('research.expired') }} <button appButton size="sm" variant="secondary" icon="retry" type="button" (click)="rerun.emit(t.useAi)">{{ i18n.t('research.searchAgain') }}</button></p>
          }

          <div class="bar" role="group" [attr.aria-label]="i18n.t('research.actions')">
            @if (canAdd()) {
              <div class="bar-row">
                <label class="mini">
                  <span>{{ i18n.t('research.queue') }}</span>
                  <select class="select" (change)="queueId.set(numberOrNull($event))">
                    <option value="">{{ i18n.t('queue.main') }}</option>
                    @for (q of queues.queues(); track q.id) {
                      <option [value]="q.id">{{ q.name }}</option>
                    }
                  </select>
                </label>
                <label class="mini">
                  <span>{{ i18n.t('research.category') }}</span>
                  <select class="select" (change)="saveAs.set(value($event))">
                    <option value="">{{ i18n.t('research.categoryAuto') }}</option>
                    @for (c of fileCategories; track c) {
                      <option [value]="c">{{ i18n.t('category.' + c) }}</option>
                    }
                  </select>
                </label>
                <label class="mini check-mini">
                  <input type="checkbox" [checked]="later()" (change)="later.set(checked($event))" />
                  <span>{{ i18n.t('research.startLater') }}</span>
                </label>
              </div>
            }
            <div class="bar-row">
              @if (canAdd()) {
                <button appButton variant="primary" size="sm" icon="download" type="button" [busy]="adding()" [disabled]="selected().size === 0 || adding()" (click)="addIds(selectedIds())">
                  {{ i18n.t('research.addSelected').replace('{n}', fmt.int(selected().size)) }}
                </button>
                <button appButton size="sm" variant="ghost" type="button" (click)="selectAll()">{{ i18n.t('research.selectAll') }}</button>
                <button appButton size="sm" variant="ghost" type="button" [disabled]="selected().size === 0" (click)="selected.set(none)">{{ i18n.t('research.selectNone') }}</button>
              }
              <span class="grow"></span>
              <button appButton size="sm" variant="ghost" icon="copy" type="button" (click)="copyResponse()">{{ i18n.t('research.copyResponse') }}</button>
              <button appButton size="sm" variant="ghost" icon="fileDoc" type="button" (click)="saveResearch()">{{ i18n.t('research.save') }}</button>
              <button appButton size="sm" variant="ghost" icon="book" type="button" aria-haspopup="menu" (click)="openCiteMenu($event)">{{ i18n.t('research.cite') }}</button>
            </div>
          </div>
        }
      </div>
    </section>
    <app-menu [open]="citeMenu() !== null" [items]="citeItems()" [x]="citeMenu()?.x ?? 0" [y]="citeMenu()?.y ?? 0" (selected)="exportCitations($event)" (closed)="citeMenu.set(null)" />
  `,
  styles: `
    :host {
      display: block;
    }

    .turn {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
    }

    .user {
      align-self: flex-end;
      max-inline-size: min(640px, 90%);
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      gap: 2px;
    }

    .bubble {
      padding: var(--space-2) var(--space-4);
      border-radius: var(--radius-lg, 14px);
      background: var(--color-accent-soft, var(--color-surface-3));
      border: 1px solid var(--color-accent);
      white-space: pre-wrap;
      overflow-wrap: anywhere;
    }

    .kind {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .assistant {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      min-inline-size: 0;
    }

    .steps {
      list-style: none;
      margin: 0;
      padding: var(--space-2) var(--space-3);
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1) var(--space-4);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1, var(--color-surface-2));
      font-size: var(--text-sm);
    }

    .step {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      color: var(--color-text-muted);
    }

    .step.is-active,
    .step.is-done {
      color: var(--color-text);
    }

    .step.is-failed {
      color: var(--color-danger);
    }

    .mark {
      display: inline-grid;
      place-items: center;
      inline-size: 18px;
      block-size: 18px;
      border-radius: 50%;
      border: 1px solid var(--color-border-strong);
    }

    .is-done .mark {
      background: var(--color-accent);
      border-color: var(--color-accent);
      color: var(--color-on-accent, #04201d);
    }

    .is-failed .mark {
      border-color: var(--color-danger);
    }

    .dot,
    .dash {
      background: var(--color-text-muted);
      border-radius: 2px;
    }

    .dot {
      inline-size: 4px;
      block-size: 4px;
      border-radius: 50%;
    }

    .dash {
      inline-size: 8px;
      block-size: 2px;
    }

    .spin {
      inline-size: 10px;
      block-size: 10px;
      border-radius: 50%;
      border: 2px solid var(--color-accent);
      border-inline-end-color: transparent;
      animation: turn 0.8s linear infinite;
    }

    @keyframes turn {
      to {
        transform: rotate(360deg);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .spin {
        animation-duration: 3s;
      }
    }

    .count {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    .stop {
      align-self: flex-start;
    }

    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
    }

    .chip {
      display: inline-flex;
      align-items: center;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-full);
      font-size: var(--text-xs);
      color: var(--color-text-secondary);
      overflow: hidden;
    }

    .chip.is-active {
      border-color: var(--color-accent);
    }

    .chip.is-failed {
      border-color: var(--color-danger);
    }

    .chip.is-stopped,
    .chip.is-setup {
      border-style: dashed;
    }

    .chip-main,
    .chip-retry,
    .chip-setup {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      padding: 3px 10px;
      border: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      cursor: pointer;
    }

    .chip-main:disabled {
      cursor: default;
    }

    .chip-setup {
      color: var(--color-accent);
      text-decoration: underline;
    }

    .chip-retry {
      padding-inline: 8px;
      border-inline-start: 1px solid var(--color-border);
    }

    .chip-dot {
      inline-size: 6px;
      block-size: 6px;
      border-radius: 50%;
      background: var(--color-success);
    }

    .is-busy .chip-dot {
      background: var(--color-accent);
    }

    .is-failed .chip-dot {
      background: var(--color-danger);
    }

    .is-stopped .chip-dot {
      background: var(--color-text-muted);
    }

    .chip-state {
      color: var(--color-text-muted);
    }

    .chip button:focus-visible {
      outline: 2px solid var(--color-accent);
      outline-offset: -2px;
    }

    .plan,
    .note {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-2);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .plan app-icon {
      color: var(--color-accent);
    }

    .query {
      padding: 1px 8px;
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
    }

    .warn {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-2);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .warn.is-error {
      border-color: var(--color-danger);
    }

    .answer {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      line-height: var(--leading-normal);
    }

    .top-title {
      font-weight: var(--weight-semibold);
      margin-block-start: var(--space-1);
    }

    .top {
      margin: 0;
      padding-inline-start: var(--space-5);
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
      font-size: var(--text-sm);
    }

    .results {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }

    .more {
      align-self: center;
    }

    .bar {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      padding: var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
    }

    .bar-row {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-3);
    }

    .grow {
      flex: 1;
    }

    .mini {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .select {
      block-size: 30px;
      padding-inline: 8px;
      min-inline-size: 120px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .check-mini input {
      accent-color: var(--color-accent);
    }
  `,
})
export class ResearchTurnComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly fmt = inject(FormatService);
  protected readonly queues = inject(QueuesService);
  private readonly ai = inject(AiService);
  private readonly ui = inject(UiState);
  private readonly toasts = inject(ToastService);
  private readonly router = inject(Router);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly tauri = inject(TAURI_AVAILABLE);

  readonly turn = input.required<Turn>();
  /** This is the turn the running (or just finished) search belongs to: it can be cancelled and retried. */
  readonly live = input(false);
  /** The backend still holds this turn's results (searched during this run), so they can be added. */
  readonly addable = input(false);

  readonly stopSearch = output<void>();
  readonly retrySource = output<SourceId>();
  /** Search the prompt again, with or without the AI. */
  readonly rerun = output<boolean>();
  /** Some results were added to the downloads. */
  readonly added = output<void>();

  protected readonly fileCategories = CATEGORY_KEYS;
  protected readonly none: ReadonlySet<string> = new Set<string>();
  protected readonly selected = signal<ReadonlySet<string>>(this.none);
  protected readonly only = signal<SourceId | null>(null);
  protected readonly showAll = signal(false);
  protected readonly adding = signal(false);
  protected readonly queueId = signal<number | null>(null);
  protected readonly saveAs = signal('');
  protected readonly later = signal(false);
  protected readonly citeMenu = signal<{ x: number; y: number } | null>(null);

  protected readonly running = computed(() => this.turn().status === 'running');
  protected readonly canAdd = computed(() => this.addable() && this.turn().items.some((i) => i.kind === 'file'));
  protected readonly steps = computed(() => deriveSteps(this.turn()));
  protected readonly top = computed(() => summarize(this.turn()).top);
  protected readonly ranked = computed(() => summarize(this.turn()).ranked);
  protected readonly lines = computed(() => (this.running() && this.turn().items.length === 0 ? [] : summaryLines(this.turn(), this.exportContext())));

  protected readonly visible = computed<readonly TurnItem[]>(() => {
    const only = this.only();
    const items = this.turn().items;
    return only === null ? items : items.filter((i) => i.source === only);
  });
  protected readonly shown = computed(() => (this.showAll() ? this.visible() : this.visible().slice(0, FIRST_PAGE)));
  protected readonly hidden = computed(() => this.visible().length - this.shown().length);
  protected readonly selectedIds = computed(() => [...this.selected()]);

  protected readonly citeItems = computed<readonly MenuItem[]>(() => [
    { id: 'bib', label: this.i18n.t('research.cite.bibtex'), icon: 'file' },
    { id: 'ris', label: this.i18n.t('research.cite.ris'), icon: 'file' },
  ]);

  protected stepLabel(id: string, state: string): string {
    return this.i18n.t(id === 'prepare' && state === 'done' ? 'research.step.prepared' : `research.step.${id}`);
  }

  protected chipState(s: SourceStatus): string {
    switch (s.state) {
      case 'searching':
        return s.found > 0 ? this.i18n.t('research.state.partial').replace('{n}', this.fmt.int(s.found)) : this.i18n.t('research.state.searching');
      case 'failed':
        return this.i18n.t(`research.err.${s.code ?? 'network'}`);
      case 'stopped':
        return this.i18n.t('research.state.stopped');
      default:
        return this.i18n.plural('research.state.done', s.found).replace('{n}', this.fmt.int(s.found));
    }
  }

  protected chipLabel(s: SourceStatus): string {
    return `${this.i18n.t(`research.source.${s.id}`)}: ${this.chipState(s)}`;
  }

  protected toggleOnly(id: SourceId): void {
    this.only.set(this.only() === id ? null : id);
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

  protected toggle(id: string): void {
    const next = new Set(this.selected());
    if (!next.delete(id)) {
      next.add(id);
    }
    this.selected.set(next);
  }

  protected selectAll(): void {
    this.selected.set(new Set(this.visible().filter((i) => i.kind === 'file').map((i) => i.id)));
  }

  protected openResearchSettings(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('research-sources')?.scrollIntoView({ block: 'start' }), 50);
    });
  }

  private exportContext(): ExportContext {
    return { t: (k) => this.i18n.t(k), plural: (k, n) => this.i18n.plural(k, n), bytes: (n) => this.fmt.bytes(n), date: new Date().toISOString().slice(0, 10) };
  }

  // ---------------------------------------------------------------- results

  /** A web result: the site grabber opens on its page and lists the files (robots.txt obeyed). */
  protected findFiles(it: TurnItem): void {
    const url = safeWebUrl(it.downloadUrl);
    if (url === null) {
      return;
    }
    this.ui.grabberPreset.set(url);
    this.ui.openGrabber();
  }

  /** Opens the result's own page in the default browser (http and https only; the app checks again). */
  protected openPage(it: TurnItem): void {
    const url = safeWebUrl(it.pageUrl ?? (it.kind === 'page' ? it.downloadUrl : null));
    if (url === null) {
      return;
    }
    if (!this.tauri) {
      window.open(url, '_blank', 'noopener,noreferrer');
      return;
    }
    this.invoke<void>('open_web_url', { url }).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected copyLink(it: TurnItem): void {
    const url = safeWebUrl(it.downloadUrl);
    if (url !== null) {
      this.copy(url, 'research.linkCopied');
    }
  }

  protected async addIds(ids: readonly string[]): Promise<void> {
    if (ids.length === 0 || this.adding() || !this.canAdd()) {
      return;
    }
    this.adding.set(true);
    try {
      const r = await this.ai.addResults({
        ids,
        queueId: this.queueId(),
        category: this.saveAs() || null,
        startPaused: this.later(),
        allowDuplicates: true,
      });
      if (r.added.length === 0 && r.skipped.every((s) => s.reason === 'unknown')) {
        this.toasts.show('danger', this.i18n.t('research.expired'));
      } else {
        const text =
          r.skipped.length === 0
            ? this.i18n.t('research.added').replace('{n}', this.fmt.int(r.added.length))
            : this.i18n.t('research.addedSome').replace('{n}', this.fmt.int(r.added.length)).replace('{skipped}', this.fmt.int(r.skipped.length));
        this.toasts.show(r.added.length > 0 ? 'success' : 'danger', r.added.length > 0 ? text : this.i18n.t('research.addFailed'));
      }
      this.selected.set(this.none);
      if (r.added.length > 0) {
        this.added.emit();
      }
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.adding.set(false);
    }
  }

  // ---------------------------------------------------------------- turn actions

  protected copyResponse(): void {
    this.copy(responseText(this.turn(), this.exportContext()), 'research.responseCopied');
  }

  protected saveResearch(): void {
    const t = this.turn();
    void this.saveFile(exportName(t.prompt, 'md'), toMarkdown(t, this.exportContext()), 'md', 'research.saved');
  }

  protected openCiteMenu(event: MouseEvent): void {
    const r = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.citeMenu.set({ x: this.i18n.dir() === 'rtl' ? r.right - 190 : r.left, y: r.bottom + 4 });
  }

  protected exportCitations(id: string): void {
    this.citeMenu.set(null);
    const t = this.turn();
    const r = id === 'ris' ? toRis(t.items) : toBibtex(t.items);
    if (r.count === 0) {
      this.toasts.show('info', this.i18n.t('research.cite.none'));
      return;
    }
    const done = r.skipped > 0 ? this.i18n.t('research.cite.savedSome').replace('{n}', this.fmt.int(r.count)).replace('{skipped}', this.fmt.int(r.skipped)) : this.i18n.t('research.cite.saved').replace('{n}', this.fmt.int(r.count));
    void this.saveFile(exportName(t.prompt, id === 'ris' ? 'ris' : 'bib'), r.text, id === 'ris' ? 'ris' : 'bib', done, true);
  }

  private copy(text: string, doneKey: string): void {
    const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard;
    if (!clipboard) {
      return;
    }
    clipboard.writeText(text).then(
      () => this.toasts.show('success', this.i18n.t(doneKey)),
      () => this.toasts.show('danger', this.i18n.t('research.copyFailed')),
    );
  }

  /** Native "save as" in the app, a plain download in a browser. `message` is a key, or the text itself when `literal`. */
  private async saveFile(name: string, text: string, kind: 'md' | 'bib' | 'ris', message: string, literal = false): Promise<void> {
    const done = literal ? message : this.i18n.t(message);
    if (this.tauri) {
      try {
        if (await this.invoke<boolean>('save_text_file', { name, text, kind })) {
          this.toasts.show('success', done);
        }
      } catch (e: unknown) {
        this.toasts.show('danger', this.ai.errorText(e));
      }
      return;
    }
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    // the download starts after the click returns: keep the address alive for a moment
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
    this.toasts.show('success', done);
  }
}
