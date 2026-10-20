import { ComponentFixture, TestBed } from '@angular/core/testing';

import { AiStatus } from '../../ai/ai.models';
import { ResearchBatch, ResearchEvent, StartResponse, WireItem } from '../../ai/research.models';
import { ResearchSession } from '../../ai/research.session';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { UiState } from '../shell/ui-state.service';
import { ResearchPageComponent } from './research-page.component';

function item(id: string, source: WireItem['source'], over: Partial<WireItem> = {}): WireItem {
  return {
    id,
    source,
    title: `Title ${id}`,
    authors: [],
    year: null,
    summary: null,
    license: null,
    size: null,
    pageUrl: null,
    downloadUrl: `https://example.org/${id}.bin`,
    filename: `${id}.bin`,
    doi: null,
    arxiv: null,
    pmid: null,
    pmcid: null,
    kind: 'file',
    rights: { kind: 'unknown', label: '' },
    seen: null,
    ...over,
  };
}

function status(configured: boolean): AiStatus {
  return {
    settings: { provider: 'deepinfra', model: configured ? 'm' : '', baseUrl: null, useDownloadHistory: false, suggestNames: false, dailyTokenLimit: 100000, dailyRequestLimit: 200, maxTokens: 1024 },
    keyHint: configured ? '••••wxyz' : null,
    configured,
    usage: { day: '2026-10-20', tokens: 0, requests: 0 },
    providers: [{ id: 'deepinfra', name: 'DeepInfra', defaultBaseUrl: 'https://api.deepinfra.com/v1/openai' }],
  };
}

interface Harness {
  readonly calls: { cmd: string; args: Record<string, unknown> | undefined }[];
  emit(batch: ResearchBatch): void;
}

function setup(opts: { configured?: boolean; start?: StartResponse; onStart?: () => void } = {}): Harness {
  const calls: Harness['calls'] = [];
  let handler: ((b: ResearchBatch) => void) | null = null;
  const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    switch (cmd) {
      case 'ai_status':
        return Promise.resolve(status(opts.configured ?? false) as T);
      case 'research_start':
        opts.onStart?.();
        return Promise.resolve((opts.start ?? { searchId: 5, sources: ['arxiv', 'gutenberg'], unavailable: [], ai: false }) as T);
      case 'research_add':
        return Promise.resolve({ added: [11], skipped: [] } as T);
      case 'seen_check':
        return Promise.resolve([] as T);
      case 'list_queues':
        return Promise.resolve([] as T);
      default:
        return Promise.resolve(undefined as T);
    }
  };
  const listen: ListenFn = <T>(_event: string, h: (payload: T) => void): Promise<() => void> => {
    handler = h as unknown as (b: ResearchBatch) => void;
    return Promise.resolve(() => {
      handler = null;
    });
  };
  TestBed.configureTestingModule({
    providers: [
      { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      { provide: TAURI_AVAILABLE, useValue: true },
      { provide: TAURI_INVOKE, useValue: invoke },
      { provide: TAURI_LISTEN, useValue: listen },
    ],
  });
  return { calls, emit: (b) => handler?.(b) };
}

async function settle(fixture: ComponentFixture<unknown>): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await fixture.whenStable();
    fixture.detectChanges();
  }
}

const batch = (searchId: number, ...events: ResearchEvent[]): ResearchBatch => ({ searchId, events });

describe('ResearchSession', () => {
  function session(h: ReturnType<typeof setup>): ResearchSession {
    TestBed.configureTestingModule({ providers: [ResearchSession] });
    void h;
    return TestBed.inject(ResearchSession);
  }

  it('shows each source as soon as it answers, in arrival order, and ignores other searches', async () => {
    const h = setup();
    const s = session(h);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'all' });
    expect(s.busy()).toBe(true);
    expect(s.sources().map((c) => c.state)).toEqual(['searching', 'searching']);
    h.emit(batch(5, { t: 'hits', source: 'gutenberg', items: [item('g1', 'gutenberg')] }, { t: 'source', source: 'gutenberg', state: 'done', found: 1, code: null }));
    expect(s.items().map((i) => i.id)).toEqual(['g1']);
    expect(s.sources().find((c) => c.id === 'gutenberg')?.state).toBe('done');
    expect(s.sources().find((c) => c.id === 'arxiv')?.state).toBe('searching');
    h.emit(batch(99, { t: 'hits', source: 'arxiv', items: [item('other', 'arxiv')] }));
    expect(s.items().length).toBe(1);
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv'), item('g1', 'gutenberg')] }, { t: 'source', source: 'arxiv', state: 'failed', found: 0, code: 'timeout' }));
    // a repeated id is not added twice
    expect(s.items().map((i) => i.id)).toEqual(['g1', 'a1']);
    expect(s.sources().find((c) => c.id === 'arxiv')?.code).toBe('timeout');
    h.emit(batch(5, { t: 'phase', phase: 'done' }, { t: 'end' }));
    expect(s.busy()).toBe(false);
  });

  it('keeps events that arrive before the start call has answered', async () => {
    const h = setup({ onStart: () => h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] })) });
    const s = session(h);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'papers' });
    expect(s.items().map((i) => i.id)).toEqual(['a1']);
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'x', useAi: false, lang: 'en', category: 'papers' } });
  });

  it('moves ranked results to the top as ranking entries stream in, best first, and keeps the reason', async () => {
    const h = setup({ start: { searchId: 5, sources: ['arxiv'], unavailable: [], ai: true } });
    const s = session(h);
    await s.start({ topic: 'x', useAi: true, lang: 'en', category: 'all' });
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a', 'arxiv'), item('b', 'arxiv'), item('c', 'arxiv')] }));
    expect(s.ordered().map((i) => i.id)).toEqual(['a', 'b', 'c']);
    h.emit(batch(5, { t: 'phase', phase: 'ranking' }, { t: 'rank', items: [{ id: 'c', score: 70, why: 'on topic' }] }));
    expect(s.ordered().map((i) => i.id)).toEqual(['c', 'a', 'b']);
    expect(s.busy()).toBe(true);
    h.emit(batch(5, { t: 'rank', items: [{ id: 'b', score: 90, why: '' }] }));
    expect(s.ordered().map((i) => i.id)).toEqual(['b', 'c', 'a']);
    expect(s.ordered()[1].why).toBe('on topic');
    expect(s.ordered()[0].why).toBeNull();
    expect(s.rankedCount()).toBe(2);
    // a result that arrives later (a retried source) gets its rank if it already has one, else sits below
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('d', 'arxiv')] }));
    expect(s.ordered().map((i) => i.id)).toEqual(['b', 'c', 'a', 'd']);
  });

  it('cancel and retry call the backend with the search id', async () => {
    const h = setup();
    const s = session(h);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'all' });
    await s.cancel();
    await s.retry('arxiv');
    expect(h.calls.find((c) => c.cmd === 'research_cancel')?.args).toEqual({ searchId: 5 });
    expect(h.calls.find((c) => c.cmd === 'research_retry_source')?.args).toEqual({ searchId: 5, source: 'arxiv' });
    expect(s.sources().find((c) => c.id === 'arxiv')?.state).toBe('searching');
  });
});

describe('ResearchPageComponent', () => {
  async function open(h: Harness, topic = 'frogs'): Promise<{ fixture: ComponentFixture<ResearchPageComponent>; el: HTMLElement }> {
    const fixture = TestBed.createComponent(ResearchPageComponent);
    fixture.detectChanges();
    await settle(fixture);
    const el = fixture.nativeElement as HTMLElement;
    const input = el.querySelector<HTMLInputElement>('input[name="topic"]') as HTMLInputElement;
    input.value = topic;
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    (el.querySelector('form') as HTMLFormElement).dispatchEvent(new Event('submit'));
    await settle(fixture);
    void h;
    return { fixture, el };
  }

  it('shows the ten tabs, the standing notice and the policy line, and searches the chosen tab', async () => {
    const h = setup();
    const fixture = TestBed.createComponent(ResearchPageComponent);
    fixture.detectChanges();
    await settle(fixture);
    const el = fixture.nativeElement as HTMLElement;
    const tabs = [...el.querySelectorAll('[role="tab"]')].map((t) => t.textContent?.trim());
    expect(tabs).toEqual(['All', 'Papers', 'Books', 'Music', 'Video', 'Games', 'Software', 'Images', 'Datasets', 'Web']);
    expect(el.textContent).toContain('Only download what you have the right to download.');
    expect(el.textContent).toContain('does not search torrent indexes, piracy or paywall-bypass sites');
    expect(el.querySelector('input[role="switch"]')).toBeNull();
    (el.querySelector('#rtab-music') as HTMLButtonElement).click();
    fixture.detectChanges();
    const input = el.querySelector<HTMLInputElement>('input[name="topic"]') as HTMLInputElement;
    input.value = 'jazz';
    input.dispatchEvent(new Event('input'));
    (el.querySelector('form') as HTMLFormElement).dispatchEvent(new Event('submit'));
    await settle(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'jazz', useAi: false, lang: 'en', category: 'music' } });
  });

  it('renders partial results per source with chips, licence badges, and escapes hostile text', async () => {
    const h = setup();
    const { fixture, el } = await open(h);
    h.emit(
      batch(
        5,
        {
          t: 'hits',
          source: 'gutenberg',
          items: [item('g1', 'gutenberg', { title: '<img src=x onerror="window.pwned=1"> Frogs', rights: { kind: 'public-domain', label: 'Public domain' }, license: 'Public domain', size: 2048 })],
        },
        { t: 'source', source: 'gutenberg', state: 'done', found: 1, code: null },
      ),
    );
    await settle(fixture);
    expect(el.querySelectorAll('li.card').length).toBe(1);
    expect(el.querySelector('li.card img')).toBeNull();
    expect((window as unknown as { pwned?: number }).pwned).toBeUndefined();
    expect(el.querySelector('li.card')?.textContent).toContain('<img src=x onerror="window.pwned=1"> Frogs');
    expect(el.querySelector('.badge.rights-public-domain')?.textContent).toContain('Public domain');
    const chips = [...el.querySelectorAll('.chip')].map((c) => c.textContent?.replace(/\s+/g, ' ').trim());
    expect(chips.some((c) => c?.includes('Project Gutenberg') && c.includes('1 results'))).toBe(true);
    expect(chips.some((c) => c?.includes('arXiv') && c.includes('searching'))).toBe(true);
    // the page is busy: Cancel is on screen
    const cancel = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Cancel'));
    expect(cancel).toBeDefined();
    cancel?.click();
    expect(h.calls.some((c) => c.cmd === 'research_cancel')).toBe(true);
    // a failed source can be retried on its own
    h.emit(batch(5, { t: 'source', source: 'arxiv', state: 'failed', found: 0, code: 'timeout' }));
    await settle(fixture);
    const retry = el.querySelector<HTMLButtonElement>('.chip-retry');
    expect(retry).not.toBeNull();
    retry?.click();
    expect(h.calls.find((c) => c.cmd === 'research_retry_source')?.args).toEqual({ searchId: 5, source: 'arxiv' });
  });

  it('a web result has no checkbox, shows only a safe address and opens the grabber on its page', async () => {
    const h = setup({ start: { searchId: 5, sources: ['web'], unavailable: [], ai: false } });
    const { fixture, el } = await open(h);
    const ui = TestBed.inject(UiState);
    h.emit(
      batch(5, {
        t: 'hits',
        source: 'web',
        items: [
          item('w1', 'web', { kind: 'page', downloadUrl: 'https://files.example.org/downloads/?utm=1', summary: 'A page of files' }),
          item('w2', 'web', { kind: 'page', downloadUrl: 'javascript:alert(1)' }),
        ],
      }),
    );
    await settle(fixture);
    const cards = [...el.querySelectorAll('li.card')];
    expect(cards.length).toBe(2);
    expect(cards[0].querySelector('input[type="checkbox"]')).toBeNull();
    expect(cards[0].querySelector('.url')?.textContent).toContain('files.example.org/downloads/');
    expect(cards[0].querySelector('.url')?.textContent).not.toContain('utm');
    expect(cards[1].querySelector('.url')?.textContent?.trim()).toBe('');
    const find = (c: Element): HTMLButtonElement | undefined => [...c.querySelectorAll('button')].find((b) => b.textContent?.includes('Find files on this page'));
    find(cards[1])?.click();
    expect(ui.grabberOpen()).toBe(false);
    find(cards[0])?.click();
    expect(ui.grabberOpen()).toBe(true);
    expect(ui.grabberPreset()).toBe('https://files.example.org/downloads/?utm=1');
    // there is nothing to link: no anchors anywhere in the results
    expect(el.querySelector('li.card a')).toBeNull();
  });

  it('adds the selected files only', async () => {
    const h = setup();
    const { fixture, el } = await open(h);
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv'), item('p1', 'web', { kind: 'page' })] }));
    await settle(fixture);
    [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('Select all'))?.click();
    fixture.detectChanges();
    const add = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Add selected to Riptide (1)')) as HTMLButtonElement;
    add.click();
    await settle(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_add')?.args).toEqual({ request: { ids: ['a1'], queueId: null, category: null, startPaused: false, allowDuplicates: true } });
  });

  it('shows the AI plan and the reason lines as they stream in', async () => {
    const h = setup({ configured: true, start: { searchId: 5, sources: ['arxiv'], unavailable: [], ai: true } });
    const { fixture, el } = await open(h);
    expect(el.querySelector('input[role="switch"]')).not.toBeNull();
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a', 'arxiv'), item('b', 'arxiv')] }));
    await settle(fixture);
    expect(el.textContent).toContain('AI is planning extra searches');
    h.emit(batch(5, { t: 'plan', queries: ['frog habitats'] }, { t: 'phase', phase: 'ranking' }, { t: 'rank', items: [{ id: 'b', score: 88, why: 'Covers frog habitats' }] }));
    await settle(fixture);
    expect(el.textContent).toContain('frog habitats');
    expect(el.textContent).toContain('AI is ranking the results');
    const cards = [...el.querySelectorAll('li.card')];
    expect(cards[0].textContent).toContain('Covers frog habitats');
    expect(cards[0].textContent).toContain('Match 88%');
  });
});
