import { PreferenceStore } from '../core/preferences';
import type { SortKey } from './row-index';

/** A data column of the downloads table (the actions column is not resizable). */
export type ColumnKey = Exclude<SortKey, 'queue'>;

/** User-chosen widths in px; a missing key keeps the layout's default track. */
export type ColumnWidths = Partial<Readonly<Record<ColumnKey, number>>>;

/** The table's four responsive layouts, widest first (see the container queries in the page and row). */
export type GridLayout = 'full' | 'compact' | 'narrow' | 'tiny';

/** A default track: `min` px, growing by `fr` when set. */
interface Track {
  readonly min: number;
  readonly fr?: number;
}

interface LayoutSpec {
  readonly columns: readonly (readonly [ColumnKey, Track])[];
  readonly actions: number;
}

/**
 * Default tracks per layout. The narrower layouts drop columns in order of usefulness: time left,
 * added and the optional columns, then speed, then size and status (which the details drawer
 * still shows). The default full set needs 1038px, so it fits a 1280px window with the sidebar
 * open; turning on optional columns can make the list scroll sideways.
 */
export const GRID_LAYOUTS: Readonly<Record<GridLayout, LayoutSpec>> = {
  full: {
    columns: [
      ['name', { min: 190, fr: 2.2 }],
      ['size', { min: 150 }],
      ['progress', { min: 150, fr: 1.6 }],
      ['speed', { min: 96 }],
      ['avgSpeed', { min: 104 }],
      ['eta', { min: 112 }],
      ['duration', { min: 96 }],
      ['status', { min: 172 }],
      ['category', { min: 112 }],
      ['added', { min: 104 }],
    ],
    actions: 64,
  },
  compact: {
    columns: [
      ['name', { min: 180, fr: 2 }],
      ['size', { min: 128 }],
      ['progress', { min: 120, fr: 1.4 }],
      ['speed', { min: 92 }],
      ['status', { min: 150 }],
    ],
    actions: 60,
  },
  narrow: {
    columns: [
      ['name', { min: 160, fr: 2 }],
      ['size', { min: 110 }],
      ['progress', { min: 100, fr: 1.4 }],
      ['status', { min: 140 }],
    ],
    actions: 52,
  },
  tiny: {
    columns: [
      ['name', { min: 140, fr: 1 }],
      ['progress', { min: 90, fr: 1 }],
    ],
    actions: 44,
  },
};

/** Every resizable column, in table order. */
export const COLUMN_KEYS: readonly ColumnKey[] = GRID_LAYOUTS.full.columns.map(([k]) => k);

/** Narrowest a user can drag a column: the name and progress need room for their content. */
const MIN_WIDTH: Readonly<Record<ColumnKey, number>> = {
  name: 120,
  size: 64,
  progress: 80,
  speed: 64,
  eta: 56,
  status: 88,
  added: 72,
  avgSpeed: 64,
  duration: 56,
  category: 72,
};

/** Columns that can be turned off (the name always shows). */
export const OPTIONAL_COLUMNS: readonly ColumnKey[] = COLUMN_KEYS.filter((k) => k !== 'name');

/** Off until the user turns them on, so the default table still fits a 1280px window. */
export const DEFAULT_HIDDEN: ReadonlySet<ColumnKey> = new Set<ColumnKey>(['avgSpeed', 'duration', 'category']);

/** Preference key the hidden columns are saved under. */
export const HIDDEN_COLUMNS_KEY = 'riptide.hiddenColumns';

/** Widest a column can be dragged. */
export const MAX_COLUMN_WIDTH = 1600;

/** Preference key the widths are saved under. */
export const COLUMN_WIDTHS_KEY = 'riptide.columnWidths';

/** A width clamped to the column's limits and rounded to whole pixels. */
export function clampWidth(key: ColumnKey, px: number): number {
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_WIDTH[key], px)));
}

/** The `grid-template-columns` value for one layout with the user's widths applied. */
export function gridTracks(layout: GridLayout, widths: ColumnWidths, hidden: ReadonlySet<ColumnKey> = DEFAULT_HIDDEN): string {
  const spec = GRID_LAYOUTS[layout];
  const tracks = shown(spec, hidden).map(([key, t]) => {
    const w = widths[key];
    if (w !== undefined) {
      return `${w}px`;
    }
    return t.fr === undefined ? `${t.min}px` : `minmax(${t.min}px, ${t.fr}fr)`;
  });
  return [...tracks, `${spec.actions}px`].join(' ');
}

/** The narrowest the layout's tracks can get, in px (without the row's inline padding). */
export function gridMinWidth(layout: GridLayout, widths: ColumnWidths, hidden: ReadonlySet<ColumnKey> = DEFAULT_HIDDEN): number {
  const spec = GRID_LAYOUTS[layout];
  return shown(spec, hidden).reduce((sum, [key, t]) => sum + (widths[key] ?? t.min), spec.actions);
}

function shown(spec: LayoutSpec, hidden: ReadonlySet<ColumnKey>): LayoutSpec['columns'] {
  return spec.columns.filter(([key]) => key === 'name' || !hidden.has(key));
}

/** Whether a layout shows a column at all (the narrower layouts drop some). */
export function inLayout(layout: GridLayout, key: ColumnKey): boolean {
  return GRID_LAYOUTS[layout].columns.some(([k]) => k === key);
}

/**
 * The CSS custom properties the header, skeleton and rows read: one track list and one minimum
 * width per layout. The rows overflow (and the viewport scrolls sideways) only when the user has
 * widened columns past what the window holds.
 */
export function gridVars(widths: ColumnWidths, hidden: ReadonlySet<ColumnKey> = DEFAULT_HIDDEN): Record<string, string> {
  const vars: Record<string, string> = {};
  for (const layout of Object.keys(GRID_LAYOUTS) as GridLayout[]) {
    const suffix = layout === 'full' ? '' : `-${layout}`;
    vars[`--grid-columns${suffix}`] = gridTracks(layout, widths, hidden);
    vars[`--grid-min${suffix}`] = `calc(${gridMinWidth(layout, widths, hidden)}px + 2 * var(--space-3))`;
  }
  return vars;
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
