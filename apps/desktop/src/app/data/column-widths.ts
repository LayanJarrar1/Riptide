import { PreferenceStore } from '../core/preferences';
import type { SortKey } from './row-index';

/** A data column of the downloads table (the actions column is not resizable). */
export type ColumnKey = Exclude<SortKey, 'queue'>;

/** User-chosen widths in px, kept as preferences; a missing key keeps the column's default track. */
export type ColumnWidths = Partial<Readonly<Record<ColumnKey, number>>>;

/** A column's track: its hard `min` px, the `base` px it takes by default, and `fr` when it grows. */
interface Track {
  readonly min: number;
  readonly base: number;
  readonly fr?: number;
}

/**
 * Every column's track. The name and progress grow into spare room; the rest take their base
 * width and give way (down to `min`) when the window is narrow. The list never scrolls
 * sideways: see `fitGrid`.
 */
const TRACKS: Readonly<Record<ColumnKey, Track>> = {
  name: { min: 160, base: 160, fr: 2.2 },
  size: { min: 96, base: 150 },
  progress: { min: 120, base: 120, fr: 1.6 },
  speed: { min: 80, base: 96 },
  avgSpeed: { min: 88, base: 104 },
  eta: { min: 84, base: 112 },
  duration: { min: 80, base: 96 },
  status: { min: 132, base: 160 },
  category: { min: 96, base: 112 },
  added: { min: 96, base: 104 },
};

/** Width of the trailing actions column. */
export const ACTIONS_PX = 60;

/** Every resizable column, in table order. */
export const COLUMN_KEYS: readonly ColumnKey[] = ['name', 'size', 'progress', 'speed', 'avgSpeed', 'eta', 'duration', 'status', 'category', 'added'];

/** Columns dropped first when even the minimums do not fit; the name, progress and status stay. */
export const AUTO_HIDE_ORDER: readonly ColumnKey[] = ['duration', 'avgSpeed', 'added', 'category', 'eta', 'speed', 'size'];

/** Narrowest a column can be: its track's minimum (also the narrowest a user can drag it). */
function minOf(key: ColumnKey): number {
  return TRACKS[key].min;
}

/** Columns that can be turned off (the name always shows). */
export const OPTIONAL_COLUMNS: readonly ColumnKey[] = COLUMN_KEYS.filter((k) => k !== 'name');

/** Off until the user turns them on. */
export const DEFAULT_HIDDEN: ReadonlySet<ColumnKey> = new Set<ColumnKey>(['avgSpeed', 'duration', 'category']);

/** Preference key the hidden columns are saved under. */
export const HIDDEN_COLUMNS_KEY = 'riptide.hiddenColumns';

/** Widest a column can be dragged. */
export const MAX_COLUMN_WIDTH = 1600;

/** Preference key the widths are saved under. */
export const COLUMN_WIDTHS_KEY = 'riptide.columnWidths';

/** A width clamped to the column's limits and rounded to whole pixels. */
export function clampWidth(key: ColumnKey, px: number): number {
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(minOf(key), px)));
}

/** The result of fitting the columns into a width. */
export interface GridFit {
  /** Columns shown, in table order (the user's choice minus the auto-hidden ones). */
  readonly visible: readonly ColumnKey[];
  /** Columns the user wants but that are hidden for now because they do not fit. */
  readonly autoHidden: readonly ColumnKey[];
  /** The `grid-template-columns` value; never wider than the available width. */
  readonly template: string;
}

/**
 * Fits the columns into `available` px (the width inside the rows' padding). User widths are
 * preferences: over budget, the flexible columns (name, progress) shrink first down to their
 * minimum, then the other columns scale down proportionally to theirs. If the minimums alone do
 * not fit, low-priority columns are hidden (not forgotten) in `AUTO_HIDE_ORDER`.
 */
export function fitGrid(available: number, widths: ColumnWidths, hidden: ReadonlySet<ColumnKey> = DEFAULT_HIDDEN): GridFit {
  const wanted = COLUMN_KEYS.filter((k) => k === 'name' || !hidden.has(k));
  let visible = wanted;
  for (const key of AUTO_HIDE_ORDER) {
    if (visible.reduce((sum, k) => sum + minOf(k), ACTIONS_PX) <= available) {
      break;
    }
    visible = visible.filter((k) => k !== key);
  }
  const autoHidden = wanted.filter((k) => !visible.includes(k));

  const pref = new Map<ColumnKey, number>();
  for (const key of visible) {
    const t = TRACKS[key];
    const w = widths[key];
    pref.set(key, w !== undefined ? Math.max(t.min, w) : t.fr !== undefined ? t.min : t.base);
  }
  const grows = (k: ColumnKey): boolean => TRACKS[k].fr !== undefined && widths[k] === undefined;
  let deficit = [...pref.values()].reduce((a, b) => a + b, ACTIONS_PX) - available;
  const shrink = (keys: readonly ColumnKey[]): void => {
    const slack = keys.reduce((sum, k) => sum + ((pref.get(k) ?? 0) - minOf(k)), 0);
    if (deficit <= 0 || slack <= 0) {
      return;
    }
    const take = Math.min(deficit, slack);
    for (const k of keys) {
      const w = pref.get(k) ?? 0;
      pref.set(k, Math.floor(w - (take * (w - minOf(k))) / slack));
    }
    deficit -= take;
  };
  shrink(visible.filter((k) => TRACKS[k].fr !== undefined && widths[k] !== undefined));
  shrink(visible.filter((k) => TRACKS[k].fr === undefined));

  const tracks = visible.map((key) => (grows(key) ? `minmax(${TRACKS[key].min}px, ${TRACKS[key].fr}fr)` : `${pref.get(key)}px`));
  return { visible, autoHidden, template: [...tracks, `${ACTIONS_PX}px`].join(' ') };
}

/**
 * The widest `key` can be dragged without the row outgrowing `available`: every other visible
 * column keeps at least its minimum, so the neighbouring flexible columns give way.
 */
export function maxDragWidth(key: ColumnKey, available: number, visible: readonly ColumnKey[]): number {
  const others = visible.filter((k) => k !== key).reduce((sum, k) => sum + minOf(k), ACTIONS_PX);
  return Math.max(minOf(key), Math.min(MAX_COLUMN_WIDTH, Math.floor(available - others)));
}

/** The CSS custom properties the header, skeleton and rows read. */
export function gridVars(fit: GridFit): Record<string, string> {
  return { '--grid-columns': fit.template };
}

/** Saved widths; anything malformed or out of range is dropped rather than trusted. */
export function loadColumnWidths(store: PreferenceStore): ColumnWidths {
  const raw = store.get(COLUMN_WIDTHS_KEY);
  if (raw === null) {
    return {};
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return {};
  }
  const out: Partial<Record<ColumnKey, number>> = {};
  for (const key of COLUMN_KEYS) {
    const v = (parsed as Record<string, unknown>)[key];
    if (typeof v === 'number' && Number.isFinite(v)) {
      out[key] = clampWidth(key, v);
    }
  }
  return out;
}

/** Saves the widths; an empty set removes the customisation. */
export function saveColumnWidths(store: PreferenceStore, widths: ColumnWidths): void {
  store.set(COLUMN_WIDTHS_KEY, JSON.stringify(widths));
}

/** The hidden columns; the defaults when nothing (or nothing valid) was saved. */
export function loadHiddenColumns(store: PreferenceStore): ReadonlySet<ColumnKey> {
  const raw = store.get(HIDDEN_COLUMNS_KEY);
  if (raw === null) {
    return DEFAULT_HIDDEN;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return DEFAULT_HIDDEN;
    }
    return new Set(OPTIONAL_COLUMNS.filter((k) => parsed.includes(k)));
  } catch {
    return DEFAULT_HIDDEN;
  }
}

/** Saves the hidden columns, in table order. */
export function saveHiddenColumns(store: PreferenceStore, hidden: ReadonlySet<ColumnKey>): void {
  store.set(HIDDEN_COLUMNS_KEY, JSON.stringify(OPTIONAL_COLUMNS.filter((k) => hidden.has(k))));
}
