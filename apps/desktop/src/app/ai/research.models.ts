/** Mirrors of the Rust types in `src-tauri/src/research.rs` (camelCase over IPC). */

import type { AiFail, SeenMatch } from './ai.models';

/** What a search looks for. The user must pick one; there is no "all". */
export type CategoryId = 'videos' | 'software' | 'documents' | 'images' | 'academic' | 'open-source' | 'web';

/** The categories, in the order the page offers them. */
export const CATEGORY_IDS: readonly CategoryId[] = ['videos', 'software', 'documents', 'images', 'academic', 'open-source', 'web'];

/** Ids that older versions stored in the search history. */
export type LegacyCategoryId = 'all' | 'papers' | 'books' | 'music' | 'video' | 'games' | 'datasets';

/**
 * The category an id stands for: the current ids as they are, the old ones mapped
 * (`papers` is `academic`, `books` is `documents`, `video` is `videos`, `games` is `open-source`).
 * `all`, `music` and `datasets` have no category any more and become `documents`, which is only a
 * label for an old history entry: running such an entry again searches that category.
 */
export function normalizeCategory(raw: unknown): CategoryId {
  if (typeof raw !== 'string') {
    return 'documents';
  }
  if ((CATEGORY_IDS as readonly string[]).includes(raw)) {
    return raw as CategoryId;
  }
  const legacy: Partial<Record<string, CategoryId>> = { video: 'videos', papers: 'academic', books: 'documents', games: 'open-source' };
  return legacy[raw] ?? 'documents';
}

/** How a search runs (budget, strictness, extras). */
export type SearchMode = 'quick-search' | 'deep-research' | 'video-search' | 'source-finder';

export const MODE_IDS: readonly SearchMode[] = ['quick-search', 'deep-research', 'video-search', 'source-finder'];

/** How well a result matches the query, computed by the program (the AI cannot change it). */
export type MatchClass = 'exact' | 'strong' | 'related';

/** Where a result comes from (the source finder shows it). */
export type Trust = 'official' | 'primary' | 'unknown' | 'mirror';

export type ConstraintId = 'episode' | 'season' | 'year' | 'version' | 'file-type';

/** One verified (or failed) fact behind a result's class. */
export type MatchReason =
  | { readonly kind: 'title_all_words' }
  | { readonly kind: 'title_partial'; readonly found: number; readonly of: number }
  | { readonly kind: 'words_only_in_description' }
  | { readonly kind: 'loose_title' }
  | { readonly kind: 'words_out_of_order' }
  | { readonly kind: 'phrase_matched'; readonly phrase: string }
  | { readonly kind: 'phrase_only_in_description'; readonly phrase: string }
  | { readonly kind: 'phrase_missing'; readonly phrase: string }
  | { readonly kind: 'excluded_word'; readonly word: string }
  | { readonly kind: 'matched'; readonly constraint: ConstraintId; readonly value: string }
  | { readonly kind: 'missing'; readonly constraint: ConstraintId }
  | { readonly kind: 'conflict'; readonly constraint: ConstraintId; readonly wanted: string; readonly found: string };

/** What was understood of the query. */
export interface QueryIntent {
  readonly raw: string;
  readonly normalized: string;
  readonly phrases: readonly string[];
  readonly exclusions: readonly string[];
  readonly titleTerms: readonly string[];
  readonly episode: number | null;
  readonly season: number | null;
  readonly year: number | null;
  readonly version: string | null;
  readonly fileType: string | null;
  readonly language: string | null;
  readonly script: 'arabic' | 'latin' | 'mixed' | 'other';
}

export type SourceId =
  | 'open-alex'
  | 'crossref'
  | 'arxiv'
  | 'europe-pmc'
  | 'internet-archive'
  | 'commons'
  | 'github'
  | 'data-gov'
  | 'gutenberg'
  | 'open-library'
  | 'libri-vox'
  | 'jamendo'
  | 'openverse'
  | 'blender'
  | 'open-game-art'
  | 'nasa'
  | 'web'
  | 'youtube';

export const SOURCE_IDS: readonly SourceId[] = [
  'open-alex',
  'crossref',
  'arxiv',
  'europe-pmc',
  'internet-archive',
  'commons',
  'github',
  'data-gov',
  'gutenberg',
  'open-library',
  'libri-vox',
  'jamendo',
  'openverse',
  'blender',
  'open-game-art',
  'nasa',
  'web',
  'youtube',
];

export type RightsKind = 'public-domain' | 'cc' | 'open-source' | 'unknown';

export interface Rights {
  readonly kind: RightsKind;
  readonly label: string;
}

export type HitKind = 'file' | 'page';

export interface ResearchHit {
  readonly id: string;
  readonly source: SourceId;
  readonly title: string;
  readonly authors: readonly string[];
  readonly year: number | null;
  readonly summary: string | null;
  readonly license: string | null;
  readonly size: number | null;
  readonly pageUrl: string | null;
  readonly downloadUrl: string;
  readonly filename: string | null;
  readonly doi: string | null;
  readonly arxiv: string | null;
  readonly pmid: string | null;
  readonly pmcid: string | null;
  readonly kind: HitKind;
  /** Running time, only when the source reported it. */
  readonly durationSecs: number | null;
  /** Thumbnail address (http/https), only when the source offered one. */
  readonly thumbnail: string | null;
  /** Publisher, channel or owner, only when the source named one. */
  readonly publisher: string | null;
  /** Release version, only when the source stated one. */
  readonly version: string | null;
}

/** A hit plus what the licence classifier, the relevance check and the download index know about it. */
export interface WireItem extends ResearchHit {
  readonly rights: Rights;
  readonly seen: SeenMatch | null;
  /** `related` means: same subject, but not verified to be what was asked for. */
  readonly confidence: MatchClass;
  /** Share (0-1) of the title words found in the title, file name or by-line. */
  readonly coverage: number;
  /** 0-100, orders results within one `confidence` class. */
  readonly matchScore: number;
  readonly reasons: readonly MatchReason[];
  readonly trust: Trust;
  /** Looks like a mirror or aggregator copy. */
  readonly mirror: boolean;
}

/** A card on the page: the wire item plus the AI's verdict, once it arrives. */
export interface ResultItem extends WireItem {
  readonly score: number | null;
  readonly why: string | null;
}

export interface Ranked {
  readonly id: string;
  /** 0-100, already confined to the band of the result's class: sorting by it never crosses a class. */
  readonly score: number;
  readonly why: string;
  /** The class the program computed for this result. */
  readonly confidence?: MatchClass;
}

/** Which other sources returned the same item, and where sources disagree. */
export interface SourceComparison {
  readonly alsoIn: Readonly<Record<string, readonly SourceId[]>>;
  readonly notes: readonly { readonly field: 'year' | 'version' | 'size'; readonly id: string; readonly values: readonly (readonly [SourceId, string])[] }[];
}

export type StopReason = 'max-rounds' | 'enough-exact' | 'deadline' | 'token-budget' | 'no-more-queries';

export interface AiOutcome {
  readonly requested: boolean;
  readonly used: boolean;
  readonly error: AiFail | null;
  readonly tokens: number;
  readonly dropped: number;
}

export type SourceState = 'searching' | 'done' | 'failed' | 'stopped';
export type Phase = 'searching' | 'ranking' | 'done' | 'cancelled';

/** One event of a search (the `t` tag is the Rust enum's). */
export type ResearchEvent =
  | { readonly t: 'phase'; readonly phase: Phase }
  | { readonly t: 'source'; readonly source: SourceId; readonly state: SourceState; readonly found: number; readonly code: string | null }
  | { readonly t: 'hits'; readonly source: SourceId; readonly items: readonly WireItem[] }
  | { readonly t: 'plan'; readonly queries: readonly string[] }
  | { readonly t: 'rank'; readonly items: readonly Ranked[] }
  | { readonly t: 'compare'; readonly comparison: SourceComparison }
  | { readonly t: 'summary'; readonly exact: number; readonly strong: number; readonly hidden: number; readonly stop: StopReason | null }
  | { readonly t: 'ai'; readonly outcome: AiOutcome }
  | { readonly t: 'end' };

export interface ResearchBatch {
  readonly searchId: number;
  readonly events: readonly ResearchEvent[];
}

export interface Unavailable {
  readonly source: SourceId;
  readonly reason: 'needs_setup';
}

export interface StartResponse {
  readonly searchId: number;
  readonly sources: readonly SourceId[];
  readonly unavailable: readonly Unavailable[];
  readonly ai: boolean;
  readonly mode: SearchMode;
  readonly category: CategoryId;
  readonly intent: QueryIntent;
  /** The phrasings that run first (more may follow). */
  readonly queries: readonly string[];
}

export interface SearchRequest {
  readonly topic: string;
  readonly useAi: boolean;
  readonly lang: string;
  /** Required: the user picks one of the seven categories. */
  readonly category: CategoryId;
  /** Defaults to quick search. */
  readonly mode?: SearchMode;
  readonly sources?: readonly SourceId[];
}

export type WebChoice = 'none' | 'brave' | 'searxng';

export interface ResearchSettings {
  readonly web: WebChoice;
  readonly searxngUrl: string | null;
}

export interface ResearchSettingsView {
  readonly settings: ResearchSettings;
  readonly braveHint: string | null;
  readonly jamendoHint: string | null;
  readonly webReady: boolean;
}

/** Only `http` and `https` links are ever shown as links or handed on; everything else is dropped. */
export function safeWebUrl(raw: string | null | undefined): string | null {
  if (!raw) {
    return null;
  }
  try {
    const u = new URL(raw);
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.href : null;
  } catch {
    return null;
  }
}

/** `host/path` of a link for display, without the query string (which may hold tracking ids). */
export function shortUrl(raw: string): string {
  try {
    const u = new URL(raw);
    const path = u.pathname === '/' ? '' : u.pathname;
    return `${u.host}${path}`;
  } catch {
    return raw;
  }
}
