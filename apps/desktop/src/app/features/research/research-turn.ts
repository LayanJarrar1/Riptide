import { SourceId } from '../../ai/research.models';
import { Turn, TurnItem } from './research-history';

export type StepId = 'understand' | 'search' | 'evaluate' | 'seen' | 'prepare';
export type StepState = 'pending' | 'active' | 'done' | 'skipped' | 'failed' | 'stopped';

export interface Step {
  readonly id: StepId;
  readonly state: StepState;
  /** A number worth showing next to the label (queries planned, results found, matches, ...). */
  readonly count: number | null;
}

/** A result a user can add to the downloads: a file, or a YouTube page (the engine downloads it as media). */
export function isAddable(i: TurnItem): boolean {
  return i.kind === 'file' || i.source === 'youtube';
}

/** How many sources have not answered yet. */
function pendingSources(t: Turn): number {
  return t.sources.filter((s) => s.state === 'searching').length;
}

/**
 * The progress steps of a turn, derived only from what the backend reported: the plan event
 * (understanding), the source events (searching), the rank events (evaluating), the seen
 * matches on the hits (checking what was downloaded before) and the end of the search.
 * Nothing here is invented: a step the search never ran is `skipped`, not `done`.
 */
export function deriveSteps(t: Turn): Step[] {
  const running = t.status === 'running';
  const cancelled = t.status === 'cancelled';
  const failed = t.status === 'error';
  const searching = running && (t.sources.length === 0 || pendingSources(t) > 0);
  const seenCount = t.items.filter((i) => i.seen !== null || i.wasSeen).length;

  let understand: StepState;
  if (!t.useAi) {
    understand = 'skipped';
  } else if (t.plan.length > 0) {
    understand = 'done';
  } else if (t.ai?.failed) {
    understand = 'failed';
  } else if (!running) {
    understand = cancelled || failed ? 'stopped' : 'done';
  } else {
    understand = 'active';
  }

  let search: StepState;
  if (failed) {
    search = 'failed';
  } else if (searching) {
    search = 'active';
  } else if (cancelled && t.sources.some((s) => s.state === 'stopped')) {
    search = 'stopped';
  } else {
    search = 'done';
  }

  let evaluate: StepState;
  if (!t.useAi) {
    evaluate = 'skipped';
  } else if (running && t.phase === 'ranking') {
    evaluate = 'active';
  } else if (t.rankedCount > 0) {
    evaluate = 'done';
  } else if (t.ai?.failed) {
    evaluate = 'failed';
  } else if (running) {
    evaluate = 'pending';
  } else {
    evaluate = cancelled || failed ? 'stopped' : 'skipped';
  }

  let seen: StepState;
  if (failed && t.items.length === 0) {
    seen = 'skipped';
  } else if (searching) {
    seen = t.items.length > 0 ? 'active' : 'pending';
  } else {
    seen = 'done';
  }

  let prepare: StepState;
  if (failed) {
    prepare = 'failed';
  } else if (cancelled) {
    prepare = 'stopped';
  } else if (!running) {
    prepare = 'done';
  } else {
    prepare = searching ? 'pending' : 'active';
  }

  return [
    { id: 'understand', state: understand, count: t.plan.length > 0 ? t.plan.length : null },
    { id: 'search', state: search, count: t.items.length },
    { id: 'evaluate', state: evaluate, count: t.rankedCount > 0 ? t.rankedCount : null },
    { id: 'seen', state: seen, count: seen === 'done' || seen === 'active' ? seenCount : null },
    { id: 'prepare', state: prepare, count: null },
  ];
}

export interface Summary {
  readonly total: number;
  readonly files: number;
  readonly pages: number;
  /** Sources that gave results, most results first. */
  readonly sources: readonly { readonly id: SourceId; readonly count: number }[];
  /** Results the download memory already knew. */
  readonly seen: number;
  readonly ranked: boolean;
  /** Up to three results worth looking at first (ranked ones when there are ranks). */
  readonly top: readonly TurnItem[];
}

/** The numbers behind the short answer; the sentence itself is composed from these. */
export function summarize(t: Turn): Summary {
  const counts = new Map<SourceId, number>();
  for (const i of t.items) {
    counts.set(i.source, (counts.get(i.source) ?? 0) + 1);
  }
  const files = t.items.filter(isAddable);
  const ranked = t.items.some((i) => i.score !== null);
  const pool = files.length > 0 ? files : t.items;
  return {
    total: t.items.length,
    files: files.length,
    pages: t.items.length - files.length,
    sources: [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([id, count]) => ({ id, count })),
    seen: t.items.filter((i) => i.seen !== null || i.wasSeen).length,
    ranked,
    top: pool.slice(0, 3),
  };
}
