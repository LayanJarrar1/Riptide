import { escapeBib, escapeMd, exportName, isCitable, responseText, toBibtex, toMarkdown, toRis, ExportContext } from './research-export';
import { Conversation, emptyTurn, MAX_BYTES, MAX_CONVERSATIONS, MAX_RESULTS, MAX_TURNS, parseHistory, promptKey, serialize, Turn, TurnItem } from './research-history';
import { deriveSteps, summarize } from './research-turn';

function item(id: string, over: Partial<TurnItem> = {}): TurnItem {
  return {
    id,
    source: 'arxiv',
    title: `Title ${id}`,
    authors: [],
    year: null,
    summary: null,
    license: null,
    size: null,
    pageUrl: null,
    downloadUrl: `https://example.org/${id}.pdf`,
    filename: `${id}.pdf`,
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
    score: null,
    why: null,
    wasSeen: false,
    ...over,
  };
}

function turn(over: Partial<Turn> = {}): Turn {
  return { ...emptyTurn('frogs', 'documents', false, 1000), id: 't1', ...over };
}

const ctx: ExportContext = {
  t: (k) =>
    ({
      'research.none': 'Nothing matched.',
      'research.sum.found': 'Found {n} results from {sources}.',
      'research.sum.source': '{source} ({n})',
      'research.sum.seen': '{n} already in your downloads.',
      'research.sum.pages': '{n} are pages.',
      'research.sum.top': 'Top picks',
      'research.sum.first': 'First results',
      'research.source.arxiv': 'arXiv',
      'research.source.gutenberg': 'Gutenberg',
      'research.md.meta': 'Riptide research · {date} · {category}',
      'research.cat.documents': 'Documents',
      'research.md.results': 'Results',
      'research.md.page': 'Source page',
      'research.md.download': 'Download',
      'research.licence': 'Licence',
      'research.why': 'Why it matches',
      'research.rights.unknown': 'Licence unknown',
    })[k] ?? k,
  bytes: (n) => `${n} B`,
  date: '2026-10-24',
};

describe('research history storage', () => {
  it('round-trips a conversation and strips local paths from the download memory', () => {
    const seen = { downloadId: 1, title: 'x', filename: 'f', dir: 'C:\\Users\\bob', path: 'C:\\Users\\bob\\f', completedAt: 1, stale: false, matchedBy: 'url' };
    const c: Conversation = { id: 'c1', title: 'Frogs', titled: false, createdAt: 1, updatedAt: 2, turns: [turn({ status: 'done', items: [item('a', { seen, why: 'on topic', score: 80 })] })] };
    const text = serialize([c]);
    expect(text).not.toContain('bob');
    const back = parseHistory(text);
    expect(back[0].title).toBe('Frogs');
    expect(back[0].turns[0].items[0].seen).toBeNull();
    expect(back[0].turns[0].items[0].wasSeen).toBe(true);
    expect(back[0].turns[0].items[0].why).toBe('on topic');
  });

  it('caps conversations, turns and results', () => {
    const many = Array.from({ length: MAX_CONVERSATIONS + 5 }, (_, i): Conversation => ({ id: `c${i}`, title: `c${i}`, titled: false, createdAt: i, updatedAt: i, turns: [] }));
    expect(parseHistory(serialize(many)).length).toBe(MAX_CONVERSATIONS);
    const turns = Array.from({ length: MAX_TURNS + 3 }, (_, i) => turn({ id: `t${i}`, status: 'done' }));
    const items = Array.from({ length: MAX_RESULTS + 10 }, (_, i) => item(`i${i}`));
    const c: Conversation = { id: 'c', title: 't', titled: false, createdAt: 1, updatedAt: 1, turns: [...turns.slice(0, -1), turn({ id: 'last', status: 'done', items })] };
    const back = parseHistory(serialize([c]))[0];
    expect(back.turns.length).toBe(MAX_TURNS);
    expect(back.turns[back.turns.length - 1].items.length).toBe(MAX_RESULTS);
  });

  it('stays within the byte budget by dropping the oldest turns first', () => {
    const fat = (i: number): TurnItem => item(`i${i}`, { summary: 'x'.repeat(390), title: 'T'.repeat(390) });
    const mk = (id: string, updatedAt: number): Conversation => ({
      id,
      title: id,
      titled: false,
      createdAt: 0,
      updatedAt,
      turns: Array.from({ length: 30 }, (_, n) => turn({ id: `${id}-t${n}`, status: 'done', items: Array.from({ length: 60 }, (_, k) => fat(k)) })),
    });
    const list = [mk('new', 3), mk('mid', 2), mk('old', 1)];
    const text = serialize(list);
    expect(text.length).toBeLessThanOrEqual(MAX_BYTES);
    const back = parseHistory(text);
    const turnsOf = (id: string): number => back.find((c) => c.id === id)?.turns.length ?? 0;
    expect(turnsOf('new')).toBeGreaterThan(turnsOf('old'));
  });

  it('survives garbage, drops unsafe links and marks an interrupted turn as stopped', () => {
    expect(parseHistory(null)).toEqual([]);
    expect(parseHistory('not json')).toEqual([]);
    expect(parseHistory('{"conversations":"x"}')).toEqual([]);
    const evil = {
      v: 1,
      conversations: [
        {
          id: 'c1',
          title: 'x',
          turns: [
            {
              id: 't1',
              prompt: 'p',
              status: 'running',
              phase: 'searching',
              sources: [{ id: 'arxiv', state: 'searching', found: 0, code: null }],
              items: [
                { id: 'ok', source: 'arxiv', title: 'T', downloadUrl: 'https://e.org/a.pdf', pageUrl: 'javascript:alert(1)' },
                { id: 'bad', source: 'arxiv', title: 'T', downloadUrl: 'javascript:alert(1)' },
                { id: 'bad2', source: 'nope', title: 'T', downloadUrl: 'https://e.org/b' },
                'junk',
              ],
            },
          ],
        },
      ],
    };
    const t = parseHistory(JSON.stringify(evil))[0].turns[0];
    expect(t.status).toBe('cancelled');
    expect(t.sources[0].state).toBe('stopped');
    expect(t.items.map((i) => i.id)).toEqual(['ok']);
    expect(t.items[0].pageUrl).toBeNull();
  });

  it('treats prompts that differ only in case and spacing as the same search', () => {
    expect(promptKey('  Frogs   of Peru ', 'documents', true)).toBe(promptKey('frogs of peru', 'documents', true));
    expect(promptKey('frogs', 'documents', true)).not.toBe(promptKey('frogs', 'academic', true));
    expect(promptKey('frogs', 'documents', true)).not.toBe(promptKey('frogs', 'documents', false));
  });
});

describe('research progress steps', () => {
  const state = (t: Turn): string[] => deriveSteps(t).map((s) => s.state);

  it('skips the AI steps when the AI is off', () => {
    const t = turn({ status: 'done', phase: 'done', sources: [{ id: 'arxiv', state: 'done', found: 1, code: null }], items: [item('a')] });
    expect(state(t)).toEqual(['skipped', 'done', 'skipped', 'done', 'done']);
  });

  it('follows plan, sources, rank and the end of the search', () => {
    const running = turn({ useAi: true, phase: 'searching', sources: [{ id: 'arxiv', state: 'searching', found: 0, code: null }] });
    expect(state(running)).toEqual(['active', 'active', 'pending', 'pending', 'pending']);
    const planned = { ...running, plan: ['q'], sources: [{ id: 'arxiv' as const, state: 'done' as const, found: 1, code: null }], items: [item('a')] };
    expect(state(planned)).toEqual(['done', 'done', 'pending', 'done', 'active']);
    const ranking = { ...planned, phase: 'ranking' as const };
    expect(state(ranking)[2]).toBe('active');
    const done = { ...planned, status: 'done' as const, phase: 'done' as const, rankedCount: 1 };
    expect(state(done)).toEqual(['done', 'done', 'done', 'done', 'done']);
  });

  it('shows a failed AI step and a failed search without calling them done', () => {
    const ai = { requested: true, used: false, failed: true, failMessage: 'x', tokens: 0, dropped: 0 };
    const t = turn({ useAi: true, status: 'done', phase: 'done', ai, sources: [{ id: 'arxiv', state: 'done', found: 1, code: null }], items: [item('a')] });
    expect(state(t)[0]).toBe('failed');
    expect(state(t)[2]).toBe('failed');
    const err = turn({ status: 'error', error: 'boom' });
    expect(state(err)[1]).toBe('failed');
    expect(state(err)[4]).toBe('failed');
  });

  it('marks what had not finished as stopped when cancelled', () => {
    const t = turn({ useAi: true, status: 'cancelled', phase: 'cancelled', sources: [{ id: 'arxiv', state: 'stopped', found: 0, code: null }] });
    expect(state(t)).toEqual(['stopped', 'stopped', 'stopped', 'done', 'stopped']);
  });
});

describe('research summary and exports', () => {
  const items = [
    item('a', { title: 'Frogs *of* Peru [1]', authors: ['Maria Lopez', 'Jo Kim'], year: 2021, arxiv: '2101.00001', why: 'Covers frogs', score: 90 }),
    item('b', { source: 'gutenberg', title: 'Ponds', wasSeen: true }),
    item('c', { title: 'Pages', kind: 'page' }),
  ];

  it('counts results, sources, pages, what was downloaded and the top picks', () => {
    const s = summarize(turn({ items }));
    expect(s.total).toBe(3);
    expect(s.files).toBe(2);
    expect(s.pages).toBe(1);
    expect(s.seen).toBe(1);
    expect(s.ranked).toBe(true);
    expect(s.sources[0]).toEqual({ id: 'arxiv', count: 2 });
    expect(s.top.map((i) => i.id)).toEqual(['a', 'b']);
  });

  it('writes Markdown that escapes text from sources and brackets links', () => {
    const md = toMarkdown(turn({ status: 'done', items }), ctx);
    expect(md).toContain('# frogs');
    expect(md).toContain('Frogs \\*of\\* Peru \\[1\\]');
    expect(md).toContain('Found 3 results from arXiv (2), Gutenberg (1).');
    expect(md).toContain('<https://example.org/a.pdf>');
    expect(md).toContain('Why it matches: Covers frogs');
    expect(md).not.toMatch(/\n{3,}/);
    expect(escapeMd('<script>#')).toBe('\\<script\\>\\#');
  });

  it('copies a plain response with the top picks and their reasons', () => {
    const text = responseText(turn({ status: 'done', items }), ctx);
    expect(text).toContain('Top picks');
    expect(text).toContain('1. Frogs *of* Peru [1]: Covers frogs');
  });

  it('keeps identifiers and addresses raw in BibTeX', () => {
    const r = toBibtex([item('z', { doi: '10.1007/s_1%x', arxiv: '2101.00001', authors: ['A B'], year: 2020, pageUrl: 'https://e.org/a_b?x=1&y=2' })]);
    expect(r.text).toContain('doi = {10.1007/s_1%x}');
    expect(r.text).toContain('url = {https://e.org/a_b?x=1&y=2}');
  });

  it('exports BibTeX only for what can be cited, with escaped fields and unique keys', () => {
    expect(isCitable(items[0])).toBe(true);
    expect(isCitable(items[1])).toBe(false);
    const twin = item('d', { title: 'Frogs of Peru', authors: ['Maria Lopez'], year: 2021 });
    const r = toBibtex([items[0], items[1], twin, { ...twin, id: 'e' }]);
    expect(r.count).toBe(3);
    expect(r.skipped).toBe(1);
    expect(r.text).toContain('@misc{lopez2021frogs,');
    expect(r.text).toContain('@misc{lopez2021frogsa,');
    expect(r.text).toContain('eprint = {2101.00001}');
    expect(r.text).toContain('archivePrefix = {arXiv}');
    expect(r.text).toContain('author = {Maria Lopez and Jo Kim}');
    expect(escapeBib('50% {x} & y_z #1 $')).toBe('50\\% \\{x\\} \\& y\\_z \\#1 \\$');
    expect(toBibtex([items[1]])).toEqual({ text: '', count: 0, skipped: 1 });
  });

  it('exports RIS with a record per citable result and no line breaks inside a field', () => {
    const r = toRis([item('a', { title: 'Line\nbreak', authors: ['A B'], year: 2020, doi: '10.1/x', source: 'crossref' }), items[1]]);
    expect(r.count).toBe(1);
    expect(r.skipped).toBe(1);
    expect(r.text).toContain('TY  - JOUR');
    expect(r.text).toContain('AU  - A B');
    expect(r.text).toContain('TI  - Line break');
    expect(r.text).toContain('DO  - 10.1/x');
    expect(r.text.trim().endsWith('ER  -')).toBe(true);
  });

  it('names exported files after the prompt without path characters', () => {
    expect(exportName('../../etc: frogs?', 'md')).toBe('riptide-research-etc-frogs.md');
    expect(exportName('???', 'bib')).toBe('riptide-research-results.bib');
    expect(exportName('ضفادع بيرو', 'ris')).toBe('riptide-research-ضفادع-بيرو.ris');
  });
});
