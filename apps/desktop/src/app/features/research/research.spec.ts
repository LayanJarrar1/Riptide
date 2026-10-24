import { ComponentFixture, TestBed } from '@angular/core/testing';

import { AiStatus } from '../../ai/ai.models';
import { ResearchBatch, ResearchEvent, StartResponse, WireItem } from '../../ai/research.models';
import { RESEARCH_EVENT, ResearchSession } from '../../ai/research.session';
import { loadTable } from '../../i18n/i18n.service';
import { REDUCED_MOTION_QUERY } from '../../core/motion.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { UiState } from '../shell/ui-state.service';
import { HISTORY_KEY } from './research-history';
import { LAST_CATEGORY_KEY, SIDE_KEY } from './research-layout.service';
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
    durationSecs: null,
    thumbnail: null,
    publisher: null,
    version: null,
    rights: { kind: 'unknown', label: '' },
    seen: null,
    confidence: 'exact',
    coverage: 1,
    matchScore: 100,
    reasons: [],
    trust: 'unknown',
    mirror: false,
    ...over,
  };
}

/** What a start response carries besides the ids and sources. */
const START_EXTRA = {
  mode: 'quick-search',
  category: 'documents',
  intent: { raw: 'x', normalized: 'x', phrases: [], exclusions: [], titleTerms: ['x'], episode: null, season: null, year: null, version: null, fileType: null, language: null, script: 'latin' },
  queries: ['x'],
} as const;

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

function setup(opts: { configured?: boolean; start?: StartResponse; onStart?: () => void; failStart?: boolean; store?: MemoryPreferenceStore; reducedMotion?: boolean } = {}): Harness {
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
        return Promise.resolve((opts.start ?? { ...START_EXTRA, searchId: 5, sources: ['arxiv', 'gutenberg'], unavailable: [], ai: false }) as T);
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
      { provide: PREFERENCE_STORE, useValue: opts.store ?? new MemoryPreferenceStore() },
      { provide: REDUCED_MOTION_QUERY, useValue: { matches: opts.reducedMotion ?? true, addEventListener: () => undefined } },
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
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'documents' });
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
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'academic' });
    expect(s.items().map((i) => i.id)).toEqual(['a1']);
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'x', useAi: false, lang: 'en', category: 'academic' } });
  });

  it('moves ranked results to the top as ranking entries stream in, best first, and keeps the reason', async () => {
    const h = setup({ start: { ...START_EXTRA, searchId: 5, sources: ['arxiv'], unavailable: [], ai: true } });
    const s = session(h);
    await s.start({ topic: 'x', useAi: true, lang: 'en', category: 'documents' });
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

  it('orders by match class first, then the AI ranking, then the program score', async () => {
    const h = setup({ start: { ...START_EXTRA, searchId: 5, sources: ['arxiv'], unavailable: [], ai: true } });
    const s = session(h);
    await s.start({ topic: 'x', useAi: true, lang: 'en', category: 'documents', mode: 'deep-research' });
    expect(s.mode()).toBe('quick-search');
    expect(s.queries()).toEqual(['x']);
    h.emit(
      batch(5, {
        t: 'hits',
        source: 'arxiv',
        items: [item('rel', 'arxiv', { confidence: 'related', matchScore: 95 }), item('s60', 'arxiv', { confidence: 'strong', matchScore: 60 }), item('e50', 'arxiv', { confidence: 'exact', matchScore: 50 }), item('e90', 'arxiv', { confidence: 'exact', matchScore: 90 })],
      }),
    );
    expect(s.ordered().map((i) => i.id)).toEqual(['e90', 'e50', 's60', 'rel']);
    // an AI score moves a result up inside its class only
    h.emit(batch(5, { t: 'rank', items: [{ id: 'e50', score: 80, why: 'x' }, { id: 'rel', score: 99, why: 'y' }] }));
    expect(s.ordered().map((i) => i.id)).toEqual(['e50', 'e90', 's60', 'rel']);
  });

  it('keeps the comparison and the summary of a deep search', async () => {
    const h = setup();
    const s = session(h);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'documents' });
    h.emit(batch(5, { t: 'compare', comparison: { alsoIn: { a: ['crossref'] }, notes: [] } }, { t: 'summary', exact: 1, strong: 2, hidden: 3, stop: 'enough-exact' }));
    expect(s.comparison()?.alsoIn['a']).toEqual(['crossref']);
    expect(s.summary()).toEqual({ exact: 1, strong: 2, hidden: 3, stop: 'enough-exact' });
    s.reset();
    expect(s.summary()).toBeNull();
  });

  it('cancel and retry call the backend with the search id', async () => {
    const h = setup();
    const s = session(h);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'documents' });
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

  /** Clicks a category card of the empty page (the user's explicit choice). */
  function choose(fixture: Fx, el: HTMLElement, name: string): void {
    const card = [...el.querySelectorAll<HTMLButtonElement>('.cat')].find((c) => c.querySelector('.cat-name')?.textContent?.trim() === name);
    card?.click();
    fixture.detectChanges();
  }

  async function ask(fixture: Fx, el: HTMLElement, text: string, category = 'Documents & PDFs'): Promise<void> {
    if (el.querySelector('.cat') !== null && el.querySelector('.cat.is-on') === null) {
      choose(fixture, el, category);
    }
    const field = type(el, text);
    fixture.detectChanges();
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await settle(fixture);
  }

  const button = (el: HTMLElement, text: string): HTMLButtonElement | undefined => [...el.querySelectorAll('button')].find((b) => b.textContent?.includes(text));
  const sendButton = (el: HTMLElement): HTMLButtonElement => el.querySelector('button[type="submit"]') as HTMLButtonElement;
  const trigger = (el: HTMLElement, label: string): HTMLButtonElement => el.querySelector(`app-research-picker .trigger[aria-label^="${label}"]`) as HTMLButtonElement;

  it('starts empty with the logo, the notice, the policy line and the seven category cards', async () => {
    setup();
    const { el } = await mount();
    const cards = [...el.querySelectorAll('.cat .cat-name')].map((o) => o.textContent?.trim());
    // no "All": the user picks what to look for
    expect(cards).toEqual(['Videos', 'Software & apps', 'Documents & PDFs', 'Images', 'Academic research', 'Open-source projects', 'General web']);
    expect([...el.querySelectorAll('.cat .cat-desc')].every((d) => (d.textContent ?? '').trim().length > 0)).toBe(true);
    expect(el.querySelector('img.mark')?.getAttribute('src')).toBe('brand-mark.svg');
    expect(el.textContent).toContain('Only download what you have the right to download.');
    expect(el.textContent).toContain('does not search torrent indexes, piracy or paywall-bypass sites');
    expect(el.textContent).toContain('What do you want to find?');
    expect(el.querySelector('input[type="checkbox"]')).toBeNull();
    expect(el.querySelectorAll('app-research-turn').length).toBe(0);
  });

  it('keeps Send disabled until a category is chosen, and says why', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    type(el, 'jazz');
    fixture.detectChanges();
    expect(sendButton(el).disabled).toBe(true);
    expect(el.querySelector('#research-need')?.textContent).toContain('Choose a category to search');
    expect(el.querySelector('.send-wrap')?.getAttribute('title')).toBe('Choose a category to search');
    // Enter does not search either
    el.querySelector('#research-prompt')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await settle(fixture);
    expect(h.calls.some((c) => c.cmd === 'research_start')).toBe(false);
    choose(fixture, el, 'Images');
    expect(sendButton(el).disabled).toBe(false);
    expect(el.querySelector('#research-need')).toBeNull();
    expect(el.querySelector('.cat.is-on .cat-name')?.textContent?.trim()).toBe('Images');
    expect(trigger(el, 'Category').textContent).toContain('Images');
  });

  it('suggests the category used last but never chooses it for a new conversation', async () => {
    const store = new MemoryPreferenceStore({ [LAST_CATEGORY_KEY]: 'software' });
    const h = setup({ store });
    const { fixture, el } = await mount();
    expect(el.querySelector('.cat.is-suggested .cat-name')?.textContent?.trim()).toBe('Software & apps');
    expect(el.querySelector('.cat-tag')?.textContent).toContain('Last used');
    expect(el.querySelector('.cat.is-on')).toBeNull();
    type(el, 'tool');
    fixture.detectChanges();
    expect(sendButton(el).disabled).toBe(true);
    // a search remembers its category as the next suggestion
    choose(fixture, el, 'Videos');
    await ask(fixture, el, 'film');
    expect(store.get(LAST_CATEGORY_KEY)).toBe('videos');
    h.emit(batch(5, { t: 'phase', phase: 'done' }));
    await settle(fixture);
    // and the next new conversation asks again
    button(el, 'New research')?.click();
    await settle(fixture);
    expect(el.querySelector('.cat.is-on')).toBeNull();
    expect(el.querySelector('.cat.is-suggested .cat-name')?.textContent?.trim()).toBe('Videos');
    expect(sendButton(el).disabled).toBe(true);
  });

  it('ties Video Search to Videos and puts the mode back when another category is chosen', async () => {
    setup();
    const { fixture, el } = await mount();
    trigger(el, 'Mode').click();
    fixture.detectChanges();
    const options = [...el.querySelectorAll<HTMLButtonElement>('[role="option"]')];
    expect(options.map((o) => o.querySelector('.option-name')?.textContent?.trim())).toEqual(['Quick Search', 'Deep Research', 'Video Search', 'Source Finder']);
    expect(options.every((o) => (o.querySelector('.option-desc')?.textContent ?? '').trim().length > 0)).toBe(true);
    options[2].click();
    await settle(fixture);
    expect(trigger(el, 'Mode').textContent).toContain('Video Search');
    expect(trigger(el, 'Category').textContent).toContain('Videos');
    expect(el.querySelector('.cat.is-on .cat-name')?.textContent?.trim()).toBe('Videos');
    choose(fixture, el, 'Images');
    await settle(fixture);
    expect(trigger(el, 'Mode').textContent).toContain('Quick Search');
    expect(el.querySelector('.note')?.textContent).toContain('Video Search is for Videos only');
  });

  it('opens the pickers from the keyboard: arrows move, Enter chooses, Escape closes and returns the focus', async () => {
    setup();
    const { fixture, el } = await mount();
    const t = trigger(el, 'Mode');
    t.focus();
    t.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    await flush(fixture);
    expect(el.querySelector('[role="listbox"]')).not.toBeNull();
    expect(t.getAttribute('aria-expanded')).toBe('true');
    const list = el.querySelector('[role="listbox"]') as HTMLElement;
    list.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await flush(fixture);
    expect(el.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(t);
    // open again, move down twice from the first option and choose with Enter (a click on the focused option)
    t.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    await flush(fixture);
    const opts = [...el.querySelectorAll<HTMLButtonElement>('[role="option"]')];
    expect(document.activeElement).toBe(opts[0]);
    (el.querySelector('[role="listbox"]') as HTMLElement).dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(opts[1]);
    opts[1].click();
    await flush(fixture);
    expect(trigger(el, 'Mode').textContent).toContain('Deep Research');
    expect(el.querySelector('[role="listbox"]')).toBeNull();
  });

  it('shows the AI chip as a quiet option: keyword mode opens the guided setup in place', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    expect((el.querySelector('app-ai-setup-dialog dialog') as HTMLDialogElement).open).toBeFalse();
    (el.querySelector('.provider.is-off') as HTMLButtonElement).click();
    await flush(fixture);
    // opened in place: no trip to Settings
    expect((el.querySelector('app-ai-setup-dialog dialog') as HTMLDialogElement).open).toBeTrue();
    expect(el.textContent).toContain('Where should the AI run?');
    expect(h.calls.some((c) => c.cmd === 'ai_set_key')).toBeFalse();
  });

  it('plays the entrance of the empty page, and not at all under reduced motion', async () => {
    setup({ reducedMotion: false });
    const first = await mount();
    expect(first.el.querySelector('.hello.is-animated')).not.toBeNull();
    expect(first.el.querySelectorAll('.ripple').length).toBe(2);
    TestBed.resetTestingModule();
    setup({ reducedMotion: true });
    const second = await mount();
    expect(second.el.querySelector('.hello.is-animated')).toBeNull();
    expect(second.el.querySelector('.ripple')).toBeNull();
    // the logo itself is the brand-kit file, unfiltered
    expect(second.el.querySelector('img.mark')?.getAttribute('src')).toBe('brand-mark.svg');
  });

  it('shows Web search as a calm optional note with a link to Settings, not a setup warning', async () => {
    setup({ start: { ...START_EXTRA, searchId: 5, sources: ['arxiv'], unavailable: [{ source: 'web', reason: 'needs_setup' }, { source: 'jamendo', reason: 'needs_setup' }], ai: false } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'jazz');
    const web = el.querySelector('.optional-note[data-source="web"]') as HTMLElement;
    expect(web.textContent).toContain('Web search is optional and off. The open sources above find files without any setup.');
    expect(web.querySelector('.note-link')?.textContent).toContain('Set up (advanced)');
    expect(el.querySelector('.optional-note[data-source="jamendo"]')?.textContent).toContain('Jamendo music is optional');
    // no chip, no warning styling, no alert role for an optional source
    expect(el.querySelector('.chip.is-setup')).toBeNull();
    expect(el.querySelector('.chip-setup')).toBeNull();
    expect(web.closest('[role="alert"]')).toBeNull();
    expect(web.getAttribute('role')).toBeNull();
    expect(el.textContent).not.toContain('Choose a web search in Settings');
  });

  it('Enter sends, Shift+Enter does not, and only the prompt, category and mode travel to the backend', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    choose(fixture, el, 'Videos');
    const field = type(el, 'jazz');
    const shift = new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, cancelable: true });
    field.dispatchEvent(shift);
    await settle(fixture);
    expect(shift.defaultPrevented).toBe(false);
    expect(h.calls.some((c) => c.cmd === 'research_start')).toBe(false);
    await ask(fixture, el, 'jazz');
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'jazz', useAi: false, lang: 'en', category: 'videos', mode: 'quick-search' } });
    expect((el.querySelector('#research-prompt') as HTMLTextAreaElement).value).toBe('');
    // the choice stays for the conversation, and the composer is now pinned below the thread
    expect(trigger(el, 'Category').textContent).toContain('Videos');
    expect(el.querySelector('.cat')).toBeNull();
  });

  it('shows the progress steps from the real events, chips per source, retry and cancel', async () => {
    const h = setup({ configured: true, start: { ...START_EXTRA, searchId: 5, sources: ['arxiv', 'gutenberg'], unavailable: [], ai: true } });
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

  it('groups results by how well they match: exact, likely, and related (collapsed), with no empty groups', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('e1', 'arxiv', { confidence: 'exact' })] }, { t: 'phase', phase: 'done' }));
    await settle(fixture);
    // only exact results: one group, no "Likely" or "Related" heading
    expect([...el.querySelectorAll('.group-name')].map((g) => g.textContent?.trim())).toEqual(['Exact matches']);
    TestBed.resetTestingModule();
    const h2 = setup();
    const m = await mount();
    await ask(m.fixture, m.el, 'frogs');
    h2.emit(
      batch(
        5,
        { t: 'hits', source: 'arxiv', items: [item('r1', 'arxiv', { confidence: 'related', matchScore: 20 }), item('s1', 'arxiv', { confidence: 'strong', matchScore: 70 }), item('e1', 'arxiv', { confidence: 'exact' })] },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(m.fixture);
    expect([...m.el.querySelectorAll('.group-name')].map((g) => g.textContent?.trim())).toEqual(['Exact matches', 'Likely matches', 'Related (not exact)']);
    // related is collapsed by default: its card is not rendered until it is opened
    expect(m.el.querySelectorAll('app-research-card').length).toBe(2);
    const toggle = m.el.querySelector('.group-head.is-toggle') as HTMLButtonElement;
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    toggle.click();
    await settle(m.fixture);
    expect(m.el.querySelectorAll('app-research-card').length).toBe(3);
    expect(m.el.querySelector('.noexact')).toBeNull();
    void h;
  });

  it('says plainly when nothing matches exactly, lists what was searched, and offers Deep Research', async () => {
    const h = setup({ start: { ...START_EXTRA, searchId: 5, sources: ['arxiv'], unavailable: [], ai: false, queries: ['frogs of peru', 'frogs peru'] } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs of peru');
    h.emit(
      batch(
        5,
        { t: 'hits', source: 'arxiv', items: [item('r1', 'arxiv', { confidence: 'related' })] },
        { t: 'source', source: 'arxiv', state: 'done', found: 1, code: null },
        { t: 'summary', exact: 0, strong: 0, hidden: 4, stop: null },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(fixture);
    const none = el.querySelector('.noexact') as HTMLElement;
    expect(none.textContent).toContain('No exact match was found. These are the closest results.');
    expect(none.textContent).toContain('Searched arXiv for:');
    expect(none.textContent).toContain('frogs of peru');
    expect(none.textContent).toContain('frogs peru');
    expect(el.querySelector('.hidden-note')?.textContent).toContain('4 weaker results were hidden');
    h.calls.length = 0;
    [...none.querySelectorAll('button')].find((b) => b.textContent?.includes('Try Deep Research'))?.click();
    await settle(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'frogs of peru', useAi: false, lang: 'en', category: 'documents', mode: 'deep-research' } });
    // in Deep Research the button is not offered again
    h.emit(batch(5, { t: 'phase', phase: 'done' }));
    await settle(fixture);
    expect([...el.querySelectorAll('.noexact')].pop()?.textContent).not.toContain('Try Deep Research');
  });

  it('words the reasons for a result in plain language', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'show episode 22', 'Videos');
    h.emit(
      batch(
        5,
        {
          t: 'hits',
          source: 'youtube',
          items: [
            item('y1', 'youtube', {
              kind: 'page',
              reasons: [
                { kind: 'title_all_words' },
                { kind: 'matched', constraint: 'episode', value: '22' },
                { kind: 'missing', constraint: 'season' },
              ],
            }),
          ],
        },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(fixture);
    const reasons = el.querySelector('app-research-card .reasons') as HTMLElement;
    expect(reasons.textContent).toContain('Episode 22 confirmed');
    expect(reasons.textContent).toContain('Season not stated');
    expect(reasons.textContent).toContain('All title words found');
    expect(reasons.querySelector('.is-good')).not.toBeNull();
    expect(reasons.querySelector('.is-bad')).not.toBeNull();
  });

  it('lays video cards out with thumbnail, running time and source, and a YouTube page can be added', async () => {
    const h = setup({ start: { ...START_EXTRA, category: 'videos', searchId: 5, sources: ['youtube'], unavailable: [], ai: false } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'documentary', 'Videos');
    h.emit(
      batch(
        5,
        {
          t: 'hits',
          source: 'youtube',
          items: [item('y1', 'youtube', { kind: 'page', title: 'Reef documentary', downloadUrl: 'https://www.youtube.com/watch?v=abc', pageUrl: 'https://www.youtube.com/watch?v=abc', durationSecs: 3725, thumbnail: 'https://i.ytimg.com/vi/abc/mq.jpg', publisher: 'Ocean Films' })],
        },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(fixture);
    const card = el.querySelector('app-research-card') as HTMLElement;
    expect(card.querySelector('.dur')?.textContent?.trim()).toBe('1:02:05');
    expect(card.querySelector('.thumb img')?.getAttribute('src')).toBe('https://i.ytimg.com/vi/abc/mq.jpg');
    expect(card.querySelector('.thumb img')?.getAttribute('referrerpolicy')).toBe('no-referrer');
    expect(card.textContent).toContain('Ocean Films');
    expect(card.textContent).toContain('Exact match');
    expect(card.textContent).toContain('Open video page');
    expect(card.textContent).not.toContain('Find files on this page');
    const dl = [...card.querySelectorAll('button')].find((b) => b.textContent?.includes('Download')) as HTMLButtonElement;
    expect(dl.disabled).toBe(false);
    dl.click();
    await flush(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_add')?.args).toEqual({ request: { ids: ['y1'], queueId: null, category: null, startPaused: false, allowDuplicates: true } });
  });

  it('shows what a program card needs: publisher, version, platform and whether the source is official or a mirror', async () => {
    const h = setup({ start: { ...START_EXTRA, category: 'software', searchId: 5, sources: ['internet-archive'], unavailable: [], ai: false } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'editor', 'Software & apps');
    h.emit(
      batch(
        5,
        {
          t: 'hits',
          source: 'internet-archive',
          items: [
            item('p1', 'internet-archive', { title: 'Editor 2.1', publisher: 'Editor Project', version: '2.1', filename: 'editor-2.1-setup.exe', trust: 'official' }),
            item('p2', 'internet-archive', { title: 'Editor copy', filename: 'editor.zip', mirror: true, trust: 'mirror' }),
          ],
        },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(fixture);
    const [a, b] = [...el.querySelectorAll('app-research-card')];
    expect(a.textContent).toContain('Editor Project');
    expect(a.textContent).toContain('v2.1');
    expect(a.textContent).toContain('Windows');
    expect(a.querySelector('.badge.trust.is-official')?.textContent).toContain('Official source');
    expect(b.querySelector('.badge.trust.is-mirror')?.textContent).toContain('Mirror');
    // nothing is invented: no platform for a plain archive, no version when the source gave none
    expect(b.textContent).not.toContain('Windows');
    expect(b.textContent).not.toContain(' v');
  });

  it('shows where sources agree and disagree in Deep Research', async () => {
    const h = setup({ start: { ...START_EXTRA, mode: 'deep-research', searchId: 5, sources: ['open-alex', 'crossref'], unavailable: [], ai: false } });
    const { fixture, el } = await mount();
    trigger(el, 'Mode').click();
    fixture.detectChanges();
    [...el.querySelectorAll<HTMLButtonElement>('[role="option"]')][1].click();
    await settle(fixture);
    await ask(fixture, el, 'reef');
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'reef', useAi: false, lang: 'en', category: 'documents', mode: 'deep-research' } });
    h.emit(
      batch(
        5,
        { t: 'hits', source: 'open-alex', items: [item('a1', 'open-alex', { title: 'Reef paper' })] },
        { t: 'compare', comparison: { alsoIn: { a1: ['crossref'] }, notes: [{ field: 'year', id: 'a1', values: [['open-alex', '2019'], ['crossref', '2020']] }] } },
        { t: 'phase', phase: 'done' },
      ),
    );
    await settle(fixture);
    expect(el.querySelector('app-research-card .also')?.textContent).toContain('Also found in Crossref');
    const compare = el.querySelector('.compare') as HTMLElement;
    expect(compare.querySelector('summary')?.textContent).toContain('Sources disagree on 1 result');
    expect(compare.textContent).toContain('Reef paper');
    expect(compare.textContent).toContain('Year: OpenAlex 2019, Crossref 2020');
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
    const h = setup({ start: { ...START_EXTRA, category: 'web', searchId: 5, sources: ['web'], unavailable: [], ai: false } });
    const { fixture, el } = await mount();
    await ask(fixture, el, 'files', 'General web');
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

  it('keeps the secondary actions in an overflow menu', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'phase', phase: 'done' }));
    await settle(fixture);
    // the bar shows only the main actions
    expect(button(el, 'Copy response')).toBeUndefined();
    expect(button(el, 'Save research')).toBeUndefined();
    (el.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await flush(fixture);
    const items = [...el.querySelectorAll('[role="menuitem"]')].map((i) => i.textContent?.trim());
    expect(items).toEqual(['Clear selection', 'Copy response', 'Save research', 'BibTeX (.bib)', 'RIS (.ris)']);
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
    // a second conversation, then open the first again: its turn is back, with the category it used
    button(el, 'New research')?.click();
    await settle(fixture);
    expect(el.querySelectorAll('app-research-turn').length).toBe(0);
    (el.querySelector('app-research-sidebar .open') as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelectorAll('app-research-turn').length).toBe(1);
    expect(trigger(el, 'Category').textContent).toContain('Documents');
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

  it('filters the conversation list by title or prompt', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs of peru');
    h.emit(batch(5, { t: 'phase', phase: 'done' }));
    await settle(fixture);
    button(el, 'New research')?.click();
    await settle(fixture);
    await ask(fixture, el, 'jazz records', 'Videos');
    const names = (): (string | undefined)[] => [...el.querySelectorAll('app-research-sidebar .row .name')].map((n) => n.textContent?.trim());
    expect(names().length).toBe(2);
    const search = el.querySelector('app-research-sidebar .search-input') as HTMLInputElement;
    search.value = 'JAZZ';
    search.dispatchEvent(new Event('input'));
    await settle(fixture);
    expect(names()).toEqual(['jazz records']);
    search.value = 'peru';
    search.dispatchEvent(new Event('input'));
    await settle(fixture);
    expect(names()).toEqual(['frogs of peru']);
    search.value = 'nothing like this';
    search.dispatchEvent(new Event('input'));
    await settle(fixture);
    expect(names()).toEqual([]);
    expect(el.querySelector('app-research-sidebar [role="status"]')?.textContent).toContain('No conversation matches.');
    search.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await settle(fixture);
    expect(names().length).toBe(2);
  });

  it('collapses and resizes the history from the keyboard and remembers both', async () => {
    const store = new MemoryPreferenceStore();
    setup({ store });
    const { fixture, el } = await mount();
    const handle = el.querySelector('.handle') as HTMLElement;
    expect(handle.getAttribute('role')).toBe('separator');
    expect(handle.getAttribute('aria-valuenow')).toBe('264');
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', cancelable: true }));
    fixture.detectChanges();
    expect(handle.getAttribute('aria-valuenow')).toBe('280');
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', cancelable: true }));
    fixture.detectChanges();
    expect(handle.getAttribute('aria-valuenow')).toBe('420');
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', cancelable: true }));
    fixture.detectChanges();
    expect(handle.getAttribute('aria-valuenow')).toBe('420');
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', cancelable: true }));
    fixture.detectChanges();
    expect(handle.getAttribute('aria-valuenow')).toBe('220');
    expect(JSON.parse(store.get(SIDE_KEY) ?? '{}')).toEqual({ open: true, width: 220 });
    (el.querySelector('.side-toggle.is-wide') as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(el.querySelector('.shell')?.classList).toContain('is-collapsed');
    expect(el.querySelector('.handle')).toBeNull();
    expect(JSON.parse(store.get(SIDE_KEY) ?? '{}')).toEqual({ open: false, width: 220 });
    // a new page reads them back
    TestBed.resetTestingModule();
    setup({ store });
    const again = await mount();
    expect(again.el.querySelector('.shell')?.classList).toContain('is-collapsed');
    expect((again.el.querySelector('.shell') as HTMLElement).style.getPropertyValue('--side-w')).toBe('220px');
  });

  it('reuses the results of an identical prompt instead of searching and sending again', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'source', source: 'arxiv', state: 'done', found: 1, code: null }, { t: 'source', source: 'gutenberg', state: 'done', found: 0, code: null }, { t: 'phase', phase: 'done' }, { t: 'end' }));
    await settle(fixture);
    await ask(fixture, el, '  Frogs ');
    expect(h.calls.filter((c) => c.cmd === 'research_start').length).toBe(1);
    expect(el.querySelectorAll('app-research-turn').length).toBe(2);
    expect(el.textContent).toContain('Nothing was searched or sent again.');
  });

  it('does not reuse a search where a source failed', async () => {
    const h = setup();
    const { fixture, el } = await mount();
    await ask(fixture, el, 'frogs');
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'source', source: 'arxiv', state: 'done', found: 1, code: null }, { t: 'source', source: 'gutenberg', state: 'failed', found: 0, code: 'timeout' }, { t: 'phase', phase: 'done' }, { t: 'end' }));
    await settle(fixture);
    await ask(fixture, el, 'frogs');
    expect(h.calls.filter((c) => c.cmd === 'research_start').length).toBe(2);
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

  it('offers the provider and model as a chip with an AI switch, and a keyword fallback when the AI fails', async () => {
    const h = setup({ configured: true, start: { ...START_EXTRA, searchId: 5, sources: ['arxiv'], unavailable: [], ai: true } });
    const { fixture, el } = await mount();
    expect(el.querySelector('.provider')?.textContent).toContain('DeepInfra · m');
    // the chip's menu turns the AI off for the next search
    (el.querySelector('.provider') as HTMLButtonElement).click();
    await flush(fixture);
    const menu = [...el.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    expect(menu.map((m) => m.textContent?.trim())).toEqual(['Turn AI off for this search', 'AI settings']);
    menu[0].click();
    await flush(fixture);
    expect(el.querySelector('.provider')?.textContent).toContain('AI off');
    (el.querySelector('.provider') as HTMLButtonElement).click();
    await flush(fixture);
    [...el.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')][0].click();
    await flush(fixture);
    await ask(fixture, el, 'frogs');
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'frogs', useAi: true, lang: 'en', category: 'documents', mode: 'quick-search' } });
    h.emit(batch(5, { t: 'hits', source: 'arxiv', items: [item('a1', 'arxiv')] }, { t: 'phase', phase: 'done' }, { t: 'ai', outcome: { requested: true, used: false, error: { code: 'network', message: 'down' }, tokens: 0, dropped: 0 } }));
    await settle(fixture);
    expect(el.querySelector('.warn[role="alert"]')?.textContent).toContain('The AI step failed');
    h.calls.length = 0;
    button(el, 'Search without AI')?.click();
    await settle(fixture);
    expect(h.calls.find((c) => c.cmd === 'research_start')?.args).toEqual({ request: { topic: 'frogs', useAi: false, lang: 'en', category: 'documents', mode: 'quick-search' } });
  });
});

describe('ResearchSession cancel', () => {
  it('shows the sources that had not answered as stopped', async () => {
    const h = setup();
    TestBed.configureTestingModule({ providers: [ResearchSession] });
    const s = TestBed.inject(ResearchSession);
    await s.start({ topic: 'x', useAi: false, lang: 'en', category: 'documents' });
    h.emit({ searchId: 5, events: [{ t: 'source', source: 'gutenberg', state: 'done', found: 2, code: null }, { t: 'phase', phase: 'cancelled' }] });
    expect(s.sources().map((c) => [c.id, c.state])).toEqual([
      ['arxiv', 'stopped'],
      ['gutenberg', 'done'],
    ]);
    expect(s.busy()).toBe(false);
  });
});
