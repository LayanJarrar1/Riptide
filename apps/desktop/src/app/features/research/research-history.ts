import { AiOutcome, CategoryId, CATEGORY_IDS, ResultItem, safeWebUrl, SOURCE_IDS, SourceId, SourceState } from '../../ai/research.models';
import { SourceStatus } from '../../ai/research.session';

/** Where the conversations live (browser storage of this machine only; never sent anywhere). */
export const HISTORY_KEY = 'riptide.research.history.v1';

/** Limits that keep the stored history small. Older conversations and turns fall off first. */
export const MAX_CONVERSATIONS = 30;
export const MAX_TURNS = 40;
/** Results kept per turn (the best ones: ranked first). */
export const MAX_RESULTS = 60;
export const MAX_PROMPT = 500;
export const MAX_TITLE = 80;
const MAX_TEXT = 400;
const MAX_AUTHORS = 6;

export type TurnStatus = 'running' | 'done' | 'cancelled' | 'error';

/** A result as a turn keeps it. `seen` is the live match (with local paths) and is never stored. */
export interface TurnItem extends ResultItem {
  /** The download memory knew this result when the turn ran (kept after the paths are dropped). */
  readonly wasSeen: boolean;
}

/** What the AI step did, minus anything that is not needed to show it again. */
export interface TurnAi {
  readonly requested: boolean;
  readonly used: boolean;
  readonly failed: boolean;
  readonly failMessage: string | null;
  readonly tokens: number;
  readonly dropped: number;
}

/** One prompt and the answer to it. */
export interface Turn {
  readonly id: string;
  readonly prompt: string;
  readonly category: CategoryId;
  /** The AI was asked to plan and rank this turn. */
  readonly useAi: boolean;
  readonly createdAt: number;
  readonly status: TurnStatus;
  /** The results were copied from an earlier identical turn: nothing was searched or sent again. */
  readonly reused: boolean;
  /** The search call itself failed (as opposed to a source failing). */
  readonly error: string | null;
  readonly sources: readonly SourceStatus[];
  /** Sources that need setting up (a key or an address) and so were not searched. */
  readonly unavailable: readonly SourceId[];
  readonly plan: readonly string[];
  readonly ai: TurnAi | null;
  readonly rankedCount: number;
  readonly items: readonly TurnItem[];
  /** The backend's phase when the turn was last updated (drives the progress steps). */
  readonly phase: 'searching' | 'ranking' | 'done' | 'cancelled' | 'idle';
}

export interface Conversation {
  readonly id: string;
  readonly title: string;
  /** The title was typed by the user, so a first prompt no longer renames it. */
  readonly titled: boolean;
  readonly createdAt: number;
  readonly updatedAt: number;
  readonly turns: readonly Turn[];
}

let counter = 0;

/** A short id, unique within a run and (with the clock) across runs. */
export function newId(prefix: string): string {
  counter += 1;
  return `${prefix}${Date.now().toString(36)}${counter.toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

/** Collapses whitespace runs and caps the length (prompts are shown, exported and stored). */
export function cleanText(raw: string, max: number): string {
  const text = raw.replace(/\s+/g, ' ').trim();
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}

/** A conversation's name from its first prompt. */
export function titleFrom(prompt: string): string {
  return cleanText(prompt, MAX_TITLE);
}

/** Two prompts that mean the same search (for reusing the results already on the page). */
export function promptKey(prompt: string, category: CategoryId, useAi: boolean): string {
  return `${category}|${useAi ? 'ai' : 'kw'}|${prompt.replace(/\s+/g, ' ').trim().toLowerCase()}`;
}

export function emptyTurn(prompt: string, category: CategoryId, useAi: boolean, now = Date.now()): Turn {
  return {
    id: newId('t'),
    prompt: cleanText(prompt, MAX_PROMPT),
    category,
    useAi,
    createdAt: now,
    status: 'running',
    reused: false,
    error: null,
    sources: [],
    unavailable: [],
    plan: [],
    ai: null,
    rankedCount: 0,
    items: [],
    phase: 'idle',
  };
}

export function newConversation(now = Date.now()): Conversation {
  return { id: newId('c'), title: '', titled: false, createdAt: now, updatedAt: now, turns: [] };
}

/** The AI outcome as a turn keeps it. */
export function turnAi(o: AiOutcome | null, failText: (o: AiOutcome) => string): TurnAi | null {
  if (o === null) {
    return null;
  }
  return {
    requested: o.requested,
    used: o.used,
    failed: o.error !== null,
    failMessage: o.error === null ? null : cleanText(failText(o), MAX_TEXT),
    tokens: o.tokens,
    dropped: o.dropped,
  };
}

/** The best results first, capped. */
export function keepItems(items: readonly ResultItem[], max = MAX_RESULTS): TurnItem[] {
  return items.slice(0, max).map((i) => ({ ...i, wasSeen: i.seen !== null }));
}

// ---------------------------------------------------------------- persistence

const STATES: readonly SourceState[] = ['searching', 'done', 'failed', 'stopped'];
const STATUSES: readonly TurnStatus[] = ['running', 'done', 'cancelled', 'error'];
const PHASES: readonly Turn['phase'][] = ['searching', 'ranking', 'done', 'cancelled', 'idle'];

/** What is written to storage for a turn: no local paths, no download-memory matches. */
function storeTurn(t: Turn): unknown {
  return {
    ...t,
    items: t.items.slice(0, MAX_RESULTS).map((i) => ({ ...i, seen: null, wasSeen: i.seen !== null || i.wasSeen })),
  };
}

export function serialize(list: readonly Conversation[]): string {
  const kept = list.slice(0, MAX_CONVERSATIONS).map((c) => ({ ...c, turns: c.turns.slice(-MAX_TURNS).map(storeTurn) }));
  return JSON.stringify({ v: 1, conversations: kept });
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const str = (v: unknown, max: number): string => (typeof v === 'string' ? cleanText(v, max) : '');
const num = (v: unknown, fallback = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);
const strOrNull = (v: unknown, max: number): string | null => {
  const s = str(v, max);
  return s.length > 0 ? s : null;
};

function parseItem(raw: unknown): TurnItem | null {
  if (!isObj(raw)) {
    return null;
  }
  const id = str(raw['id'], 300);
  const title = str(raw['title'], MAX_TEXT);
  const source = raw['source'];
  const downloadUrl = safeWebUrl(typeof raw['downloadUrl'] === 'string' ? raw['downloadUrl'] : null);
  if (id === '' || title === '' || typeof source !== 'string' || !SOURCE_IDS.includes(source as SourceId) || downloadUrl === null) {
    return null;
  }
  const rights = isObj(raw['rights']) ? raw['rights'] : {};
  const kind = rights['kind'];
  const authors = Array.isArray(raw['authors']) ? raw['authors'].filter((a): a is string => typeof a === 'string').slice(0, MAX_AUTHORS).map((a) => cleanText(a, 120)) : [];
  return {
    id,
    source: source as SourceId,
    title,
    authors,
    year: typeof raw['year'] === 'number' && Number.isInteger(raw['year']) ? raw['year'] : null,
    summary: strOrNull(raw['summary'], MAX_TEXT),
    license: strOrNull(raw['license'], 120),
    size: typeof raw['size'] === 'number' && raw['size'] >= 0 ? raw['size'] : null,
    pageUrl: safeWebUrl(typeof raw['pageUrl'] === 'string' ? raw['pageUrl'] : null),
    downloadUrl,
    filename: strOrNull(raw['filename'], 200),
    doi: strOrNull(raw['doi'], 200),
    arxiv: strOrNull(raw['arxiv'], 60),
    pmid: strOrNull(raw['pmid'], 30),
    pmcid: strOrNull(raw['pmcid'], 30),
    kind: raw['kind'] === 'page' ? 'page' : 'file',
    rights: {
      kind: kind === 'public-domain' || kind === 'cc' || kind === 'open-source' ? kind : 'unknown',
      label: str(rights['label'], 120),
    },
    seen: null,
    score: typeof raw['score'] === 'number' ? raw['score'] : null,
    why: strOrNull(raw['why'], MAX_TEXT),
    wasSeen: raw['wasSeen'] === true,
  };
}

function parseTurn(raw: unknown): Turn | null {
  if (!isObj(raw)) {
    return null;
  }
  const id = str(raw['id'], 60);
  const prompt = str(raw['prompt'], MAX_PROMPT);
  if (id === '' || prompt === '') {
    return null;
  }
  const category = CATEGORY_IDS.includes(raw['category'] as CategoryId) ? (raw['category'] as CategoryId) : 'all';
  const status = STATUSES.includes(raw['status'] as TurnStatus) ? (raw['status'] as TurnStatus) : 'cancelled';
  const sources: SourceStatus[] = Array.isArray(raw['sources'])
    ? raw['sources'].flatMap((s: unknown) =>
        isObj(s) && SOURCE_IDS.includes(s['id'] as SourceId) && STATES.includes(s['state'] as SourceState)
          ? [{ id: s['id'] as SourceId, state: s['state'] as SourceState, found: num(s['found']), code: strOrNull(s['code'], 40) }]
          : [],
      )
    : [];
  const ai = isObj(raw['ai'])
    ? {
        requested: raw['ai']['requested'] === true,
        used: raw['ai']['used'] === true,
        failed: raw['ai']['failed'] === true,
        failMessage: strOrNull(raw['ai']['failMessage'], MAX_TEXT),
        tokens: num(raw['ai']['tokens']),
        dropped: num(raw['ai']['dropped']),
      }
    : null;
  const items = Array.isArray(raw['items']) ? raw['items'].map(parseItem).filter((i): i is TurnItem => i !== null).slice(0, MAX_RESULTS) : [];
  // a turn that was still running when the app closed was interrupted: nothing is searching any more
  const interrupted = status === 'running';
  return {
    id,
    prompt,
    category,
    useAi: raw['useAi'] === true,
    createdAt: num(raw['createdAt']),
    status: interrupted ? 'cancelled' : status,
    reused: raw['reused'] === true,
    error: strOrNull(raw['error'], MAX_TEXT),
    sources: sources.map((s) => (interrupted && s.state === 'searching' ? { ...s, state: 'stopped' as const } : s)),
    unavailable: Array.isArray(raw['unavailable']) ? raw['unavailable'].filter((u): u is SourceId => SOURCE_IDS.includes(u as SourceId)) : [],
    plan: Array.isArray(raw['plan']) ? raw['plan'].filter((q): q is string => typeof q === 'string').slice(0, 8).map((q) => cleanText(q, 200)) : [],
    ai,
    rankedCount: num(raw['rankedCount']),
    items,
    phase: interrupted ? 'cancelled' : PHASES.includes(raw['phase'] as Turn['phase']) ? (raw['phase'] as Turn['phase']) : 'done',
  };
}

/** Reads the stored history. Anything malformed is dropped; nothing here can throw. */
export function parseHistory(text: string | null): Conversation[] {
  if (text === null) {
    return [];
  }
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    return [];
  }
  if (!isObj(data) || !Array.isArray(data['conversations'])) {
    return [];
  }
  const out: Conversation[] = [];
  for (const raw of data['conversations'].slice(0, MAX_CONVERSATIONS)) {
    if (!isObj(raw)) {
      continue;
    }
    const id = str(raw['id'], 60);
    if (id === '') {
      continue;
    }
    out.push({
      id,
      title: str(raw['title'], MAX_TITLE),
      titled: raw['titled'] === true,
      createdAt: num(raw['createdAt']),
      updatedAt: num(raw['updatedAt']),
      turns: Array.isArray(raw['turns']) ? raw['turns'].map(parseTurn).filter((t): t is Turn => t !== null).slice(-MAX_TURNS) : [],
    });
  }
  return out;
}
