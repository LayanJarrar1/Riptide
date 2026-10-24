import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { Router } from '@angular/router';

import { AiSetupDialogComponent } from '../../ai/ai-setup-dialog.component';
import { AiService } from '../../ai/ai.service';
import { CategoryId, SourceId } from '../../ai/research.models';
import { ResearchSession } from '../../ai/research.session';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { emptyTurn, keepItems, MAX_RESULTS, promptKey, Turn, turnAi } from './research-history';
import { ResearchHistoryService } from './research-history.service';
import { ResearchComposerComponent } from './research-composer.component';
import { ResearchSidebarComponent } from './research-sidebar.component';
import { ResearchTurnComponent } from './research-turn.component';

/** How long the results of an identical prompt are reused instead of searching again. */
const REUSE_MS = 10 * 60 * 1000;

/** The turn the running search belongs to. */
interface LiveRef {
  readonly conversationId: string;
  readonly turnId: string;
}

/**
 * Research as a conversation. The left list holds the conversations (kept on this machine only);
 * each prompt is a turn that shows the real progress of its search, a short summary and the result
 * cards. One search runs at a time and each sends only its own prompt (no earlier turns go to the
 * AI). The search itself is the unchanged `ResearchSession`; this page mirrors its signals into the
 * stored turn, so a running turn and one restored from history are rendered by the same component.
 */
@Component({
  selector: 'app-research-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AiSetupDialogComponent, ButtonComponent, IconComponent, ResearchComposerComponent, ResearchSidebarComponent, ResearchTurnComponent],
  providers: [ResearchSession],
  template: `
    @if (!ai.available) {
      <div class="solo">
        <h1 class="title">{{ i18n.t('research.title') }}</h1>
        <p class="muted">{{ i18n.t('research.desktopOnly') }}</p>
      </div>
    } @else {
      <div class="shell" [class.is-side-open]="sideOpen()">
        <aside class="sidebar">
          <app-research-sidebar
            [conversations]="history.conversations()"
            [activeId]="history.activeId()"
            (created)="newConversation()"
            (selected)="open($event)"
            (renamed)="history.rename($event.id, $event.title)"
            (deleted)="remove($event)"
            (cleared)="clearAll()"
          />
        </aside>

        <main class="main">
          <header class="head">
            <button appButton variant="ghost" icon="list" [iconOnly]="true" type="button" class="side-toggle" [attr.aria-expanded]="sideOpen()" [attr.aria-label]="i18n.t('research.history')" (click)="sideOpen.set(!sideOpen())"></button>
            <div class="head-text">
              <h1 class="title">{{ history.active()?.title || i18n.t('research.title') }}</h1>
              <p class="rights" role="note"><app-icon name="shieldCheck" [size]="14" /> {{ i18n.t('research.notice') }}</p>
            </div>
          </header>

          <div class="thread" #thread>
            <div class="column">
              @if (turns().length === 0) {
                <div class="hello">
                  <h2 class="hello-title">{{ i18n.t('research.hello') }}</h2>
                  <p class="muted">{{ i18n.t('research.subtitle') }}</p>
                  <p class="ready"><app-icon name="check" [size]="14" /> {{ i18n.t('research.hello.ready') }}</p>
                  @if (ai.status() !== null && !aiOn()) {
                    <p class="upgrade">
                      <app-icon name="sparkles" [size]="14" />
                      <span>{{ i18n.t('research.hello.upgrade') }}</span>
                      <button type="button" class="upgrade-link" (click)="setupOpen.set(true)">{{ i18n.t('research.hello.setup') }}</button>
                    </p>
                  }
                  <div class="examples" role="group" [attr.aria-label]="i18n.t('research.examples')">
                    @for (n of examples; track n) {
                      <button type="button" class="example" dir="auto" (click)="useExample(n)">{{ i18n.t('research.example.' + n) }}</button>
                    }
                  </div>
                </div>
              }
              @for (t of turns(); track t.id) {
                <app-research-turn
                  [turn]="t"
                  [live]="liveRef()?.turnId === t.id"
                  [addable]="history.isLive(t.id)"
                  (stopSearch)="cancel()"
                  (retrySource)="retrySource($event)"
                  (rerun)="rerun(t, $event)"
                  (added)="recheckSeen(t.id)"
                />
              }
            </div>
          </div>

          <div class="dock">
            <div class="column">
              <app-research-composer
                #composer
                [(category)]="category"
                [(useAi)]="useAi"
                [aiReady]="aiOn()"
                [providerLabel]="providerLabel()"
                [busy]="session.busy()"
                (send)="send($event)"
                (stop)="cancel()"
                (openAiSettings)="openAiSettings()"
                (setupAi)="setupOpen.set(true)"
              />
              <p class="policy">{{ i18n.t('research.footer') }}</p>
            </div>
          </div>
        </main>
      </div>
      <app-ai-setup-dialog [open]="setupOpen()" (closed)="setupOpen.set(false)" />
    }
  `,
  styles: `
    :host {
      display: block;
      block-size: 100%;
      overflow: hidden;
    }

    .solo {
      max-inline-size: 640px;
      margin: var(--space-10, 48px) auto;
      padding-inline: var(--space-6);
    }

    .shell {
      display: grid;
      grid-template-columns: 264px minmax(0, 1fr);
      block-size: 100%;
    }

    .sidebar {
      border-inline-end: 1px solid var(--color-border);
      background: var(--color-surface-1, var(--color-surface-2));
      min-block-size: 0;
    }

    .main {
      display: flex;
      flex-direction: column;
      min-inline-size: 0;
      min-block-size: 0;
    }

    .head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-6);
      border-block-end: 1px solid var(--color-border);
    }

    .head-text {
      min-inline-size: 0;
    }

    .title {
      font-size: var(--text-xl, 1.25rem);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .muted {
      color: var(--color-text-muted);
      line-height: var(--leading-normal);
    }

    .rights {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .rights app-icon {
      color: var(--color-accent);
      flex-shrink: 0;
    }

    .side-toggle {
      display: none;
    }

    .thread {
      flex: 1;
      min-block-size: 0;
      overflow-y: auto;
      padding: var(--space-6);
      scroll-padding-block-end: var(--space-6);
    }

    .column {
      max-inline-size: 860px;
      margin-inline: auto;
      display: flex;
      flex-direction: column;
      gap: var(--space-8, 32px);
    }

    .dock {
      padding: var(--space-3) var(--space-6) var(--space-4);
      border-block-start: 1px solid var(--color-border);
      background: var(--color-bg, var(--color-surface-1));
    }

    .dock .column {
      gap: var(--space-2);
    }

    .policy {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .hello {
      padding-block: var(--space-10, 48px) var(--space-4);
      text-align: center;
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: var(--space-3);
    }

    .hello-title {
      font-size: var(--text-2xl);
      font-weight: var(--weight-semibold);
    }

    .ready,
    .upgrade {
      display: flex;
      align-items: center;
      justify-content: center;
      flex-wrap: wrap;
      gap: var(--space-2);
      font-size: var(--text-sm);
    }

    .ready {
      color: var(--color-text-secondary);
    }

    .ready app-icon {
      color: var(--color-success);
    }

    .upgrade {
      color: var(--color-text-muted);
    }

    .upgrade app-icon {
      color: var(--color-accent);
    }

    .upgrade-link {
      padding: 0;
      border: 0;
      background: transparent;
      color: var(--color-accent);
      font: inherit;
      text-decoration: underline;
      cursor: pointer;
    }

    .upgrade-link:focus-visible {
      outline: 2px solid var(--color-accent);
      outline-offset: 2px;
    }

    .examples {
      display: flex;
      flex-wrap: wrap;
      justify-content: center;
      gap: var(--space-2);
      margin-block-start: var(--space-3);
    }

    .example {
      padding: var(--space-2) var(--space-4);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-full);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-sm);
      cursor: pointer;
    }

    .example:hover {
      border-color: var(--color-accent);
      color: var(--color-text);
    }

    .example:focus-visible {
      outline: 2px solid var(--color-accent);
      outline-offset: 2px;
    }

    @media (max-width: 900px) {
      .shell {
        grid-template-columns: minmax(0, 1fr);
        position: relative;
      }

      .sidebar {
        position: absolute;
        inset-block: 0;
        inset-inline-start: 0;
        z-index: 5;
        inline-size: min(300px, 86%);
        transform: translateX(calc(var(--dir-sign) * -100%));
        visibility: hidden;
        box-shadow: 0 0 24px rgb(0 0 0 / 0.3);
        transition:
          transform var(--motion-fast) var(--ease-standard),
          visibility 0s linear var(--motion-fast);
      }

      .is-side-open .sidebar {
        transform: none;
        visibility: visible;
        transition-delay: 0s;
      }

      .side-toggle {
        display: inline-flex;
      }

      .head,
      .thread,
      .dock {
        padding-inline: var(--space-4);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .sidebar {
        transition: none;
      }
    }
  `,
})
export class ResearchPageComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly history = inject(ResearchHistoryService);
  protected readonly session = inject(ResearchSession);
  private readonly router = inject(Router);
  private readonly toasts = inject(ToastService);

  private readonly thread = viewChild<ElementRef<HTMLElement>>('thread');
  private readonly composer = viewChild<ResearchComposerComponent>('composer');

  protected readonly examples = [1, 2, 3] as const;
  protected readonly category = signal<CategoryId>('documents');
  protected readonly useAi = signal(true);
  protected readonly sideOpen = signal(false);
  protected readonly setupOpen = signal(false);
  protected readonly liveRef = signal<LiveRef | null>(null);

  /** The AI is set up (provider + key + model). */
  protected readonly aiOn = computed(() => this.ai.status()?.configured === true);
  protected readonly turns = computed<readonly Turn[]>(() => this.history.active()?.turns ?? []);
  /** `Provider · model` for the indicator next to the AI switch. */
  protected readonly providerLabel = computed(() => {
    const s = this.ai.status();
    if (!s) {
      return '';
    }
    const name = s.providers.find((p) => p.id === s.settings.provider)?.name ?? s.settings.provider;
    return s.settings.model ? `${name} · ${s.settings.model}` : name;
  });

  constructor() {
    // mirror the running search into its turn, whenever the search changes
    this.session.watch(() => this.mirror());
    // the newest turn comes into view
    effect(() => {
      const n = this.turns().length;
      if (n > 0) {
        setTimeout(() => {
          const el = this.thread()?.nativeElement;
          el?.scrollTo({ top: el.scrollHeight });
        }, 0);
      }
    });
  }

  /** Copies what the session knows into the turn it belongs to (the stored turn is what the page renders). */
  private mirror(): void {
    const ref = this.liveRef();
    const phase = this.session.phase();
    if (ref === null || phase === 'idle') {
      return;
    }
    const status = phase === 'done' ? 'done' : phase === 'cancelled' ? 'cancelled' : 'running';
    this.history.patchTurn(ref.conversationId, ref.turnId, {
      phase,
      status,
      sources: this.session.sources(),
      unavailable: this.session.unavailable().map((u) => u.source),
      plan: this.session.plan(),
      ai: turnAi(this.session.ai(), (o) => (o.error ? this.ai.failText(o.error) : '')),
      rankedCount: this.session.rankedCount(),
      items: keepItems(this.session.ordered(), MAX_RESULTS),
    });
  }

  ngOnInit(): void {
    if (this.ai.available) {
      void this.ai.refresh();
    }
  }

  protected newConversation(): void {
    this.history.select(null);
    this.sideOpen.set(false);
    this.composer()?.clear();
  }

  protected open(id: string): void {
    this.history.select(id);
    this.sideOpen.set(false);
  }

  protected remove(id: string): void {
    this.stopIfLive(id);
    this.history.remove(id);
  }

  protected clearAll(): void {
    this.stopIfLive(null);
    this.history.clearAll();
  }

  /** Deleting a conversation stops its search (`null`: whatever is running). */
  private stopIfLive(conversationId: string | null): void {
    const ref = this.liveRef();
    if (ref !== null && (conversationId === null || ref.conversationId === conversationId)) {
      if (this.session.busy()) {
        void this.session.cancel().catch(() => undefined);
      }
      this.session.reset();
      this.liveRef.set(null);
    }
  }

  protected useExample(n: number): void {
    this.composer()?.fill(this.i18n.t(`research.example.${n}`));
  }

  protected openAiSettings(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('ai')?.scrollIntoView({ block: 'start' }), 50);
    });
  }

  protected cancel(): void {
    void this.session.cancel().catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected retrySource(source: SourceId): void {
    void this.session.retry(source).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected send(prompt: string): void {
    if (this.session.busy()) {
      return;
    }
    const useAi = this.useAi() && this.aiOn();
    const category = this.category();
    const conv = this.history.active() ?? this.history.startConversation(prompt);
    const reusable = this.findReusable(conv.turns, prompt, category, useAi);
    if (reusable !== null) {
      // the copy keeps the original's time, so repeating a prompt never stretches the reuse window
      this.history.addTurn(conv.id, { ...emptyTurn(prompt, category, useAi, reusable.createdAt), ...pickResults(reusable), reused: true, status: 'done', phase: 'done' });
    } else {
      void this.run(conv.id, emptyTurn(prompt, category, useAi));
    }
    this.composer()?.clear();
  }

  /** Searches the same prompt again, with or without the AI. */
  protected rerun(turn: Turn, withAi: boolean): void {
    if (this.session.busy()) {
      return;
    }
    const conv = this.history.active();
    if (conv === null) {
      return;
    }
    void this.run(conv.id, emptyTurn(turn.prompt, turn.category, withAi && this.aiOn()));
  }

  /** An identical, finished turn whose results the backend still holds: no new search, nothing sent. */
  private findReusable(turns: readonly Turn[], prompt: string, category: CategoryId, useAi: boolean): Turn | null {
    const key = promptKey(prompt, category, useAi);
    const now = Date.now();
    return (
      [...turns].reverse().find((t) => t.status === 'done' && t.items.length > 0 && t.sources.every((s) => s.state === 'done') && now - t.createdAt < REUSE_MS && this.history.isLive(t.id) && promptKey(t.prompt, t.category, t.useAi) === key) ?? null
    );
  }

  private async run(conversationId: string, turn: Turn): Promise<void> {
    this.history.addTurn(conversationId, turn);
    this.liveRef.set({ conversationId, turnId: turn.id });
    try {
      // only the prompt travels: no earlier turn, no history, nothing local
      await this.session.start({ topic: turn.prompt, useAi: turn.useAi, lang: this.i18n.lang(), category: turn.category });
    } catch (e: unknown) {
      this.liveRef.set(null);
      this.history.patchTurn(conversationId, turn.id, { status: 'error', error: this.ai.errorText(e) });
    }
  }

  /** After adding, the cards of what was just added are not "downloaded" yet; refresh the badges. */
  protected async recheckSeen(turnId: string): Promise<void> {
    const conv = this.history.active();
    const turn = conv?.turns.find((t) => t.id === turnId);
    if (!conv || !turn || turn.items.length === 0) {
      return;
    }
    try {
      const matches = await this.ai.seenCheck(
        turn.items.map((i) => ({
          url: i.downloadUrl,
          doi: i.doi ?? undefined,
          arxiv: i.arxiv ?? undefined,
          pmid: i.pmid ?? undefined,
          pmcid: i.pmcid ?? undefined,
          filename: i.size !== null ? (i.filename ?? undefined) : undefined,
          size: i.size ?? undefined,
        })),
      );
      const byId = new Map(turn.items.map((it, n) => [it.id, matches[n] ?? null] as const));
      if (this.liveRef()?.turnId === turnId) {
        // the live turn is rebuilt from the session, so the session is what changes
        this.session.replaceSeen((it) => ({ ...it, seen: byId.get(it.id) ?? it.seen }));
      } else {
        this.history.patchTurn(conv.id, turn.id, { items: turn.items.map((it) => ({ ...it, seen: byId.get(it.id) ?? it.seen, wasSeen: it.wasSeen || byId.get(it.id) != null })) });
      }
    } catch {
      /* the badges stay as they were */
    }
  }
}

/** The parts of a finished turn that a reused turn shares. */
function pickResults(t: Turn): Partial<Turn> {
  return { sources: t.sources, unavailable: t.unavailable, plan: t.plan, ai: t.ai, rankedCount: t.rankedCount, items: t.items };
}
