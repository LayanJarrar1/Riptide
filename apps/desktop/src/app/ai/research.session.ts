import { computed, inject, Injectable, OnDestroy, signal } from '@angular/core';

import { TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import {
  AiOutcome,
  Phase,
  Ranked,
  ResearchBatch,
  ResearchEvent,
  ResultItem,
  SearchRequest,
  SourceId,
  SourceState,
  StartResponse,
  Unavailable,
} from './research.models';

/** The webview event the backend streams a search on (`EVENT` in `research.rs`). */
export const RESEARCH_EVENT = 'riptide://research';

/** What one source chip shows. */
export interface SourceStatus {
  readonly id: SourceId;
  readonly state: SourceState;
  readonly found: number;
  readonly code: string | null;
}

/**
 * One Research page's search: starts it, listens to the events the backend streams, and keeps
 * the state the page renders. Results are appended in arrival order (neutral order); once the
 * AI's ranking entries arrive the ranked ones move to the top, best first. The page never waits
 * for anything: every signal fills in as events come.
 */
@Injectable()
export class ResearchSession implements OnDestroy {
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly listen = inject(TAURI_LISTEN);

  /** `idle` until the first search; `searching` and `ranking` are the busy phases. */
  readonly phase = signal<Phase | 'idle'>('idle');
  readonly sources = signal<readonly SourceStatus[]>([]);
  readonly unavailable = signal<readonly Unavailable[]>([]);
  /** Results in arrival order, with the AI's verdict merged in as it arrives. */
  readonly items = signal<readonly ResultItem[]>([]);
  readonly plan = signal<readonly string[]>([]);
  readonly ai = signal<AiOutcome | null>(null);
  /** The AI will plan and rank this search. */
  readonly aiActive = signal(false);
  readonly rankedCount = signal(0);
  /** Only this source's results are shown, when set. */
  readonly onlySource = signal<SourceId | null>(null);

  /** The search is still working (searching or ranking). */
  readonly busy = computed(() => this.phase() === 'searching' || this.phase() === 'ranking');

  /** Ranked results first (best first), then the rest in arrival order; filtered by source. */
  readonly ordered = computed<readonly ResultItem[]>(() => {
    const only = this.onlySource();
    const all = only === null ? this.items() : this.items().filter((i) => i.source === only);
    const ranked = all.filter((i) => i.score !== null).sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
    const rest = all.filter((i) => i.score === null);
    return [...ranked, ...rest];
  });

  private searchId: number | null = null;
  private readonly watchers = new Set<() => void>();
  private unlisten: (() => void) | null = null;
  /** Batches that arrived before `research_start` answered with its id. */
  private early: ResearchBatch[] = [];
  private readonly ranks = new Map<string, Ranked>();
  /** Bumped by every start, so a late answer of an older search is ignored. */
  private generation = 0;

  /** Calls `fn` after every change to the search (a start, a batch of events, a retry). Returns the way to stop. */
  watch(fn: () => void): () => void {
    this.watchers.add(fn);
    return () => this.watchers.delete(fn);
  }

  private notify(): void {
    for (const fn of this.watchers) {
      fn();
    }
  }

  ngOnDestroy(): void {
    this.watchers.clear();
    this.unlisten?.();
    this.unlisten = null;
    if (this.searchId !== null && this.busy()) {
      void this.invoke('research_cancel', { searchId: this.searchId }).catch(() => undefined);
    }
  }

  private async ensureListening(): Promise<void> {
    if (this.unlisten !== null) {
      return;
    }
    this.unlisten = await this.listen<ResearchBatch>(RESEARCH_EVENT, (batch) => {
      if (this.searchId === null) {
        this.early.push(batch);
      } else if (batch.searchId === this.searchId) {
        this.applyBatch(batch);
      }
    });
  }

  /** Starts a search; resolves as soon as the backend accepted it (results follow as events). */
  async start(request: SearchRequest): Promise<StartResponse> {
    const gen = ++this.generation;
    if (this.searchId !== null && this.busy()) {
      void this.invoke('research_cancel', { searchId: this.searchId }).catch(() => undefined);
    }
    this.reset();
    this.phase.set('searching');
    await this.ensureListening();
    let res: StartResponse;
    try {
      res = await this.invoke<StartResponse>('research_start', { request });
    } catch (e: unknown) {
      if (gen === this.generation) {
        this.phase.set('idle');
      }
      throw e;
    }
    if (gen !== this.generation) {
      return res;
    }
    this.searchId = res.searchId;
    this.aiActive.set(res.ai);
    this.unavailable.set(res.unavailable);
    this.sources.set(res.sources.map((id) => ({ id, state: 'searching' as const, found: 0, code: null })));
    if (res.sources.length === 0) {
      this.phase.set('done');
    }
    const early = this.early;
    this.early = [];
    for (const b of early) {
      if (b.searchId === res.searchId) {
        this.applyBatch(b);
      }
    }
    this.notify();
    return res;
  }

  /** Stops the search; what arrived stays on the page. */
  async cancel(): Promise<void> {
    if (this.searchId !== null) {
      await this.invoke('research_cancel', { searchId: this.searchId });
    }
  }

  /** Searches one source again. */
  async retry(source: SourceId): Promise<void> {
    if (this.searchId === null) {
      return;
    }
    this.patchSource(source, { state: 'searching', code: null });
    this.notify();
    try {
      await this.invoke('research_retry_source', { searchId: this.searchId, source });
    } catch (e: unknown) {
      this.patchSource(source, { state: 'failed', code: 'network' });
      this.notify();
      throw e;
    }
  }

  /** Forgets the current search (used before a new one and by the page's reset). */
  reset(): void {
    this.searchId = null;
    this.early = [];
    this.ranks.clear();
    this.sources.set([]);
    this.unavailable.set([]);
    this.items.set([]);
    this.plan.set([]);
    this.ai.set(null);
    this.aiActive.set(false);
    this.rankedCount.set(0);
    this.onlySource.set(null);
    this.phase.set('idle');
  }

  /** Marks items as downloaded / not (after "Add" the cards' badges are refreshed). */
  replaceSeen(update: (item: ResultItem) => ResultItem): void {
    this.items.update((list) => list.map(update));
    this.notify();
  }

  applyBatch(batch: ResearchBatch): void {
    for (const e of batch.events) {
      this.apply(e);
    }
    this.notify();
  }

  private patchSource(id: SourceId, patch: Partial<SourceStatus>): void {
    this.sources.update((list) => {
      const known = list.some((s) => s.id === id);
      const base = known ? list : [...list, { id, state: 'searching' as const, found: 0, code: null }];
      return base.map((s) => (s.id === id ? { ...s, ...patch } : s));
    });
  }

  private apply(e: ResearchEvent): void {
    switch (e.t) {
      case 'phase':
        // `done` / `cancelled` can arrive while a retried source is still running; the chips show that
        this.phase.set(e.phase);
        if (e.phase === 'cancelled') {
          this.sources.update((list) => list.map((s) => (s.state === 'searching' ? { ...s, state: 'stopped' as const } : s)));
        }
        break;
      case 'source':
        this.patchSource(e.source, { state: e.state, found: e.found, code: e.code });
        break;
      case 'hits': {
        const have = new Set(this.items().map((i) => i.id));
        const fresh = e.items
          .filter((i) => !have.has(i.id))
          .map((i): ResultItem => {
            const r = this.ranks.get(i.id);
            return { ...i, score: r?.score ?? null, why: r && r.why.length > 0 ? r.why : null };
          });
        if (fresh.length > 0) {
          this.items.update((list) => [...list, ...fresh]);
        }
        break;
      }
      case 'plan':
        this.plan.set(e.queries);
        break;
      case 'rank': {
        for (const r of e.items) {
          this.ranks.set(r.id, r);
        }
        const byId = new Map(e.items.map((r) => [r.id, r] as const));
        this.items.update((list) =>
          list.map((i) => {
            const r = byId.get(i.id);
            return r ? { ...i, score: r.score, why: r.why.length > 0 ? r.why : null } : i;
          }),
        );
        this.rankedCount.set(this.ranks.size);
        break;
      }
      case 'compare':
      case 'summary':
        // the comparison and the count of hidden results are for the rebuilt page
        break;
      case 'ai':
        this.ai.set(e.outcome);
        break;
      case 'end':
        break;
    }
  }
}
