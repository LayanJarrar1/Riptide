/** Mirrors of the Rust types in `src-tauri/src/research.rs` (camelCase over IPC). */

import type { AiFail, SeenMatch } from './ai.models';

export type CategoryId = 'all' | 'papers' | 'books' | 'music' | 'video' | 'games' | 'software' | 'images' | 'datasets' | 'web';

/** The tabs, in order. */
export const CATEGORY_IDS: readonly CategoryId[] = ['all', 'papers', 'books', 'music', 'video', 'games', 'software', 'images', 'datasets', 'web'];

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
  | 'web';

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
}

/** A hit plus what the licence classifier and the download index know about it. */
export interface WireItem extends ResearchHit {
  readonly rights: Rights;
  readonly seen: SeenMatch | null;
}

/** A card on the page: the wire item plus the AI's verdict, once it arrives. */
export interface ResultItem extends WireItem {
  readonly score: number | null;
  readonly why: string | null;
}

export interface Ranked {
  readonly id: string;
  readonly score: number;
  readonly why: string;
}

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
}

export interface SearchRequest {
  readonly topic: string;
  readonly useAi: boolean;
  readonly lang: string;
  readonly category: CategoryId;
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
