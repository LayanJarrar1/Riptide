import { ComponentFixture, TestBed } from '@angular/core/testing';

import { AiStatus } from '../../ai/ai.models';
import { ResearchBatch, ResearchEvent, StartResponse, WireItem } from '../../ai/research.models';
import { RESEARCH_EVENT, ResearchSession } from '../../ai/research.session';
import { loadTable } from '../../i18n/i18n.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { UiState } from '../shell/ui-state.service';
import { HISTORY_KEY } from './research-history';
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

function setup(opts: { configured?: boolean; start?: StartResponse; onStart?: () => void; failStart?: boolean } = {}): Harness {
  const calls: Harness['calls'] = [];
  // one handler per event name: other services listen to their own events through the same fake
  const handlers = new Map<string, (payload: never) => void>();
  const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    switch (cmd) {
      case 'ai_status':
        return Promise.resolve(status(opts.configured ?? false) as T);
      case 'research_start':
        opts.onStart?.();
        if (opts.failStart) {
          return Promise.reject('boom');
        }
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
  const listen: ListenFn = <T>(event: string, h: (payload: T) => void): Promise<() => void> => {
    handlers.set(event, h);
    return Promise.resolve(() => {
      handlers.delete(event);
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
  return { calls, emit: (b) => (handlers.get(RESEARCH_EVENT) as ((b: ResearchBatch) => void) | undefined)?.(b) };
}

async function settle(fixture: ComponentFixture<unknown>): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await fixture.whenStable();
    fixture.detectChanges();
  }
}

/** Like `settle`, but does not wait for timers (a toast's auto-dismiss would hold `whenStable` for seconds). */
async function flush(fixture: ComponentFixture<unknown>): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise<void>((r) => setTimeout(r, 0));
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
  beforeAll(async () => {
    await loadTable('en');
  });

  type Fx = ComponentFixture<ResearchPageComponent>;

  async function mount(): Promise<{ fixture: Fx; el: HTMLElement }> {
    const fixture = TestBed.createComponent(ResearchPageComponent);
    fixture.detectChanges();
    await settle(fixture);
    return { fixture, el: fixture.nativeElement as HTMLElement };
  }

  function type(el: HTMLElement, text: string): HTMLTextAreaElement {
    const field = el.querySelector<HTMLTextAreaElement>('#research-prompt') as HTMLTextAreaElement;
    field.value = text;
    field.dispatchEvent(new Event('input'));
    return field;
  }

  async function ask(fixture: Fx, el: HTMLElement, text: string): Promise<void> {
    const field = type(el, text);
    fixture.detectChanges();
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await settle(fixture);
  }

  const button = (el: HTMLElement, text: string): HTMLButtonElement | undefined => [...el.querySelectorAll('button')].find((b) => b.textContent?.includes(text));

  it('starts empty with the notice, the policy line and the ten kinds of result', async () => {
    setup();
    const { el } = await mount();
    const kinds = [...el.querySelectorAll('select option')].slice(0, 10).map((o) => o.textContent?.trim());
    expect(kinds).toEqual(['All', 'Papers', 'Books', 'Music', 'Video', 'Games', 'Software', 'Images', 'Datasets', 'Web']);
    expect(el.textContent).toContain('Only download what you have the right to download.');
    expect(el.textContent).toContain('does not search torrent indexes, piracy or paywall-bypass sites');
    expect(el.textContent).toContain('What do you want to find?');
    expect(el.querySelector('input[role="switch"]')).toBeNull();
    expect(el.querySelectorAll('app-research-turn').length).toBe(0);
  });

  it('Enter sends, Shift+Enter does not, and only the prompt travels to the backend', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    const field = type(el, 'jazz');
    const shift = new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, cancelable: true });
    field.dispatchEvent(shift);
    await settle(fixture);
    expect(shift.defaultPrevented).toBe(false);
    expect(h.calls.some((c) => c.cmd === 'research_start')).toBe(false);
    const select = el.querySelector('select') as HTMLSelectElement;
    select.value = 'music';
    select.dispatchEvent(new Event('change'));
    await ask(fixture, el, 'jazz');
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'jazz', useAi: false, lang: 'en', category: 'music' } });
    expect((el.querySelector('#research-prompt') as HTMLTextAreaElement).value).toBe('');
  });

  it('shows the progress steps from the real events, chips per source, retry and cancel', async () => {
    const h = setup({ configured: true, start: { searchId: 5, sources: ['arxiv', 'gutenberg'], unavailable: [], ai: true } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    const state = (id: string): string => (el.querySelector(`.step:nth-child(${id})`) as HTMLElement).className;
    // nothing has been reported yet: understanding and searching are in progress
    expect(state('1')).toContain('is-active');
    expect(state('2')).toContain('is-active');
    expect(state('3')).toContain('is-pending');
    h.emit(batch(5, { t: 'plan', queries: ['frog habitats'] }, { t: 'hits', source: 'gutenberg', items: [item('g1', 'gutenberg'), item('g2', 'gutenberg')] }, { t: 'source', source: 'gutenberg', state: 'done', found: 2, code: null }, { t: 'source', source: 'arxiv', state: 'failed', found: 0, code: 'timeout' }));
    await settle(fixture);
    expect(state('1')).toContain('is-done');
    expect(el.textContent).toContain('frog habitats');
    expect(state('2')).toContain('is-done');
    expect(el.querySelector('.chip-retry')).not.toBeNull();
    (el.querySelector('.chip-retry') as HTMLButtonElement).click();
    expect(h.calls.find((c) => c.cmd === 'research_retry_source')?.args).toEqual({ searchId: 5, source: 'arxiv' });
    h.emit(batch(5, { t: 'phase', phase: 'ranking' }, { t: 'rank', items: [{ id: 'g2', score: 88, why: 'Covers frog habitats' }] }));
    await settle(fixture);
    expect(state('3')).toContain('is-active');
    expect(el.querySelectorAll('app-research-card')[0].textContent).toContain('Covers frog habitats');
    expect(el.querySelectorAll('app-research-card')[0].textContent).toContain('Match 88%');
    (el.querySelector('.stop') as HTMLButtonElement).click();
    expect(h.calls.some((c) => c.cmd === 'research_cancel')).toBe(true);
    h.emit(batch(5, { t: 'phase', phase: 'done' }, { t: 'ai', outcome: { requested: true, used: true, error: null, tokens: 10, dropped: 0 } }, { t: 'end' }));
    await settle(fixture);
    expect(state('3')).toContain('is-done');
    expect(state('5')).toContain('is-done');
    // the summary is composed from the results
    expect(el.querySelector('.answer')?.textContent).toContain('Found 2 results from Project Gutenberg (2).');
    expect(el.querySelector('.answer')?.textContent).toContain('Covers frog habitats');
  });

  it('skips the AI steps in keyword mode and never invents them', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'source', source: 'arxiv', state: 'done', found: 1, code: null }, { t: 'source', source: 'gutenberg', state: 'done', found: 0, code: null }, { t: 'phase', phase: 'done' }, { t: 'end' }));
    await settle(fixture);
    const steps = [...el.querySelectorAll('.step')].map((s) => s.className);
    expect(steps[0]).toContain('is-skipped');
    expect(steps[2]).toContain('is-skipped');
    expect(steps[1]).toContain('is-done');
  });

  it('renders cards with licence, size, year and a badge for what was downloaded, and escapes hostile text', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(
      batch(5, {
        t: 'hits',
        source: 'gutenberg',
        items: [
          item('g1', 'gutenberg', {
            title: '<img src=x onerror="window.pwned=1"> Frogs',
            rights: { kind: 'public-domain', label: 'Public domain' },
            size: 2048,
            year: 1901,
            filename: 'frogs.pdf',
            summary: 'About <b>frogs</b>',
            seen: { downloadId: 1, title: 'Frogs', filename: 'frogs.pdf', dir: 'C:\\Users\\bob', path: 'C:\\Users\\bob\\frogs.pdf', completedAt: 1, stale: false, matchedBy: 'url' },
          }),
        ],
      }),
    );
    await settle(fixture);
    const card = el.querySelector('app-research-card') as HTMLElement;
    expect(card.querySelector('img')).toBeNull();
    expect(card.querySelector('b')).toBeNull();
    expect((window as unknown as { pwned?: number }).pwned).toBeUndefined();
    expect(card.textContent).toContain('<img src=x onerror="window.pwned=1"> Frogs');
    expect(card.textContent).toContain('PDF');
    expect(card.textContent).toContain('1901');
    expect(card.querySelector('.badge.rights-public-domain')).not.toBeNull();
    expect(card.querySelector('.badge.seen')).not.toBeNull();
    expect(card.querySelector('a')).toBeNull();
  });

  it('a web result has no checkbox, shows only a safe address, opens the grabber and its page', async () => {
    const h = setup({ start: { searchId: 5, sources: ['web'], unavailable: [], ai: false } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'files');
    const ui = TestBed.inject(UiState);
    h.emit(
      batch(5, {
        t: 'hits',
        source: 'web',
        items: [
          item('w1', 'web', { kind: 'page', downloadUrl: 'https://files.example.org/downloads/?utm=1', pageUrl: 'https://files.example.org/', summary: 'A page of files' }),
          item('w2', 'web', { kind: 'page', downloadUrl: 'javascript:alert(1)' }),
        ],
      }),
    );
    await settle(fixture);
    const cards = [...el.querySelectorAll('app-research-card')];
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
    // only the page with a safe address can be opened, and only through the app
    expect([...cards[1].querySelectorAll('button')].some((b) => b.textContent?.includes('Open source page'))).toBe(false);
    [...cards[0].querySelectorAll('button')].find((b) => b.textContent?.includes('Open source page'))?.click();
    expect(h.calls.find((c) => c.cmd === 'open_web_url')?.args).toEqual({ url: 'https://files.example.org/' });
  });

  it('adds the selected files only, and one card downloads on its own', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv'), item('p1', 'web', { kind: 'page' })] }, { t: 'phase', phase: 'done' }));
    await settle(fixture);
    button(el, 'Select all')?.click();
    fixture.detectChanges();
    button(el, 'Add selected to Riptide (1)')?.click();
    await flush(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_add')?.args).toEqual({ request: { ids: ['a1'], queueId: null, category: null, startPaused: false, allowDuplicates: true } });
    h.calls.length = 0;
    const card = el.querySelector('app-research-card') as HTMLElement;
    [...card.querySelectorAll('button')].find((b) => b.textContent?.includes('Download'))?.click();
    await flush(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_add')?.args).toEqual({ request: { ids: ['a1'], queueId: null, category: null, startPaused: false, allowDuplicates: true } });
  });

  it('keeps conversations: rename, delete and clear all, stored without local paths', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(
      batch(
        5,
        {
          t: 'hits',
          source: 'arxiv',
          items: [item('a1', 'arxiv', { seen: { downloadId: 1, title: 'Frogs', filename: 'f.bin', dir: 'C:\\Users\\bob', path: 'C:\\Users\\bob\\f.bin', completedAt: 1, stale: false, matchedBy: 'url' } })],
        },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(fixture);
    const stored = TestBed.inject(PREFERENCE_STORE).get(HISTORY_KEY) ?? '';
    expect(stored).toContain('frogs');
    expect(stored).not.toContain('bob');
    expect(stored).not.toContain('C:\\\\Users');
    expect(el.querySelector('app-research-sidebar .name')?.textContent).toContain('frogs');
    // rename
    (el.querySelector('app-research-sidebar .tool') as HTMLButtonElement).click();
    fixture.detectChanges();
    const input = el.querySelector<HTMLInputElement>('app-research-sidebar .rename') as HTMLInputElement;
    input.value = 'Amphibians';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    input.dispatchEvent(new Event('input'));
    await settle(fixture);
    expect(el.querySelector('app-research-sidebar .name')?.textContent).toContain('Amphibians');
    // a second conversation, then open the first again: its turn is back
    button(el, 'New research')?.click();
    await settle(fixture);
    expect(el.querySelectorAll('app-research-turn').length).toBe(0);
    (el.querySelector('app-research-sidebar .open') as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelectorAll('app-research-turn').length).toBe(1);
    // delete asks first
    (el.querySelectorAll('app-research-sidebar .tool')[1] as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(el.querySelectorAll('app-research-sidebar .row').length).toBe(1);
    const confirm = [...el.querySelectorAll<HTMLButtonElement>('app-research-sidebar dialog button')].find((b) => b.textContent?.trim() === 'Delete');
    confirm?.click();
    await settle(fixture);
    expect(el.querySelectorAll('app-research-sidebar .row').length).toBe(0);
    expect(TestBed.inject(PREFERENCE_STORE).get(HISTORY_KEY)).not.toContain('frogs');
  });

  it('reuses the results of an identical prompt instead of searching and sending again', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'phase', phase: 'done' }, { t: 'end' }));
    await settle(fixture);
    await ask(fixture, el, '  Frogs ');
    expect(h.calls.filter((c) => c.cmd === 'research_start').length).toBe(1);
    expect(el.querySelectorAll('app-research-turn').length).toBe(2);
    expect(el.textContent).toContain('Nothing was searched or sent again.');
  });

  it('recovers from a failed search with Try again', async () => {
    const h = setup({ failStart: true });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    expect(el.querySelector('.warn.is-error')?.textContent).toContain('boom');
    h.calls.length = 0;
    button(el, 'Try again')?.click();
    await settle(fixture);
    expect(h.calls.filter((c) => c.cmd === 'research_start').length).toBe(1);
    expect(el.querySelectorAll('app-research-turn').length).toBe(2);
  });

  it('offers the AI switch, the provider and model, and a keyword fallback when the AI fails', async () => {
    const h = setup({ configured: true, start: { searchId: 5, sources: ['arxiv'], unavailable: [], ai: true } });
    const { fixture, el } = await mount();
    expect(el.querySelector('input[role="switch"]')).not.toBeNull();
    expect(el.querySelector('.provider')?.textContent).toContain('DeepInfra · m');
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'phase', phase: 'done' }, { t: 'ai', outcome: { requested: true, used: false, error: { code: 'network', message: 'down' }, tokens: 0, dropped: 0 } }));
    await settle(fixture);
    expect(el.querySelector('.warn[role="alert"]')?.textContent).toContain('The AI step failed');
    h.calls.length = 0;
    button(el, 'Search without AI')?.click();
    await settle(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'frogs', useAi: false, lang: 'en', category: 'all' } });
  });
});

describe('ResearchSession cancel', () => {
  it('shows the sources that had not answered as stopped', async () => {
    const h = setup();
    TestBed.configureTestingModule({ providers: [ResearchSession] });
    const s = TestBed.inject(ResearchSession);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'all' });
    h.emit({ searchId: 5, events: [{ t: 'source', source: 'gutenberg', state: 'done', found: 2, code: null }, { t: 'phase', phase: 'cancelled' }] });
    expect(s.sources().map((c) => [c.id, c.state])).toEqual([
      ['arxiv', 'stopped'],
      ['gutenberg', 'done'],
    ]);
    expect(s.busy()).toBe(false);
  });
});
