import { CategoryId, ConstraintId, MatchReason, SearchMode } from '../../ai/research.models';
import { IconName } from '../../ui/icon.component';

/** How the page shows each category (the id is the wire value; the texts are `research.cat.<id>` and `research.catDesc.<id>`). */
export const CATEGORY_ICONS: Readonly<Record<CategoryId, IconName>> = {
  videos: 'fileVideo',
  software: 'fileApp',
  documents: 'fileDoc',
  images: 'fileImage',
  academic: 'book',
  'open-source': 'code',
  web: 'globe',
};

/** How the page shows each mode (texts: `research.mode.<id>` and `research.modeDesc.<id>`). */
export const MODE_ICONS: Readonly<Record<SearchMode, IconName>> = {
  'quick-search': 'bolt',
  'deep-research': 'layers',
  'video-search': 'fileVideo',
  'source-finder': 'shieldCheck',
};

/** The mode and category as the composer holds them, plus a one-line note when choosing one changed the other. */
export interface Selection {
  readonly category: CategoryId | null;
  readonly mode: SearchMode;
  /** Translation key of the note, or `null` when nothing was changed behind the user's back. */
  readonly note: string | null;
}

/** Video Search only works with Videos: choosing it sets Videos. */
export function chooseMode(mode: SearchMode, category: CategoryId | null): Selection {
  if (mode === 'video-search' && category !== 'videos') {
    return { mode, category: 'videos', note: category === null ? null : 'research.note.videoSets' };
  }
  return { mode, category, note: null };
}

/** Choosing another category while Video Search is on puts the mode back to Quick Search. */
export function chooseCategory(category: CategoryId, mode: SearchMode): Selection {
  if (mode === 'video-search' && category !== 'videos') {
    return { mode: 'quick-search', category, note: 'research.note.videoOff' };
  }
  return { mode, category, note: null };
}

/** A reason as the page words it: the translation key, its values, and whether it speaks for or against the result. */
export interface ReasonLine {
  readonly key: string;
  readonly params: Readonly<Record<string, string>>;
  readonly tone: 'good' | 'bad' | 'neutral';
}

const SHOWN = 3;

/** The most telling first: a conflict, then what was confirmed, then what is missing, then the word-level facts. */
const ORDER: Readonly<Record<MatchReason['kind'], number>> = {
  conflict: 0,
  excluded_word: 1,
  matched: 2,
  phrase_matched: 3,
  missing: 4,
  phrase_missing: 5,
  phrase_only_in_description: 6,
  title_all_words: 7,
  title_partial: 8,
  words_out_of_order: 9,
  words_only_in_description: 10,
  loose_title: 11,
};

function line(r: MatchReason): ReasonLine {
  const c = (id: ConstraintId): string => id;
  switch (r.kind) {
    case 'matched':
      return { key: `research.reason.matched.${c(r.constraint)}`, params: { value: r.value }, tone: 'good' };
    case 'missing':
      return { key: `research.reason.missing.${c(r.constraint)}`, params: {}, tone: 'bad' };
    case 'conflict':
      return { key: `research.reason.conflict.${c(r.constraint)}`, params: { wanted: r.wanted, found: r.found }, tone: 'bad' };
    case 'title_all_words':
      return { key: 'research.reason.titleAll', params: {}, tone: 'good' };
    case 'title_partial':
      return { key: 'research.reason.titlePartial', params: { found: String(r.found), of: String(r.of) }, tone: 'neutral' };
    case 'words_only_in_description':
      return { key: 'research.reason.descOnly', params: {}, tone: 'bad' };
    case 'loose_title':
      return { key: 'research.reason.loose', params: {}, tone: 'neutral' };
    case 'words_out_of_order':
      return { key: 'research.reason.order', params: {}, tone: 'neutral' };
    case 'phrase_matched':
      return { key: 'research.reason.phrase', params: { phrase: r.phrase }, tone: 'good' };
    case 'phrase_only_in_description':
      return { key: 'research.reason.phraseDesc', params: { phrase: r.phrase }, tone: 'bad' };
    case 'phrase_missing':
      return { key: 'research.reason.phraseMissing', params: { phrase: r.phrase }, tone: 'bad' };
    case 'excluded_word':
      return { key: 'research.reason.excluded', params: { word: r.word }, tone: 'bad' };
  }
}

/** Up to three reasons, most telling first. Nothing is added that the backend did not report. */
export function reasonLines(reasons: readonly MatchReason[]): readonly ReasonLine[] {
  return [...reasons]
    .sort((a, b) => ORDER[a.kind] - ORDER[b.kind])
    .slice(0, SHOWN)
    .map(line);
}

/** `1:05:09` or `4:07`; `null` when the source gave no running time. */
export function durationText(secs: number | null): string | null {
  if (secs === null || !Number.isFinite(secs) || secs <= 0) {
    return null;
  }
  const total = Math.round(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const two = (n: number): string => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}

/** The platform a software file is for, only when its extension says so (never guessed from the title). */
export function platformOf(filename: string | null, url: string): 'windows' | 'macos' | 'linux' | 'android' | null {
  const name = (filename ?? url.split('?')[0]).toLowerCase();
  if (/\.(exe|msi|msix)$/.test(name)) {
    return 'windows';
  }
  if (/\.(dmg|pkg)$/.test(name)) {
    return 'macos';
  }
  if (/\.(deb|rpm|appimage|flatpakref)$/.test(name)) {
    return 'linux';
  }
  if (/\.apk$/.test(name)) {
    return 'android';
  }
  return null;
}
