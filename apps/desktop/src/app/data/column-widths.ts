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
 * Default tracks per layout. The narrower layouts drop columns in order of usefulness: time left
 * and added, then speed, then size and status (which the details drawer still shows). The full
 * set needs 1034px, so it fits a 1280px window with the sidebar open.
 */
export const GRID_LAYOUTS: Readonly<Record<GridLayout, LayoutSpec>> = {
  full: {
    columns: [
      ['name', { min: 190, fr: 2.2 }],
      ['size', { min: 150 }],
      ['progress', { min: 150, fr: 1.6 }],
      ['speed', { min: 96 }],
      ['eta', { min: 84 }],
      ['status', { min: 172 }],
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
};

/** Widest a column can be dragged. */
export const MAX_COLUMN_WIDTH = 1600;

/** Preference key the widths are saved under. */
export const COLUMN_WIDTHS_KEY = 'riptide.columnWidths';

/** A width clamped to the column's limits and rounded to whole pixels. */
export function clampWidth(key: ColumnKey, px: number): number {
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_WIDTH[key], px)));
}

/** The `grid-template-columns` value for one layout with the user's widths applied. */
export function gridTracks(layout: GridLayout, widths: ColumnWidths): string {
  const spec = GRID_LAYOUTS[layout];
  const tracks = spec.columns.map(([key, t]) => {
    const w = widths[key];
    if (w !== undefined) {
      return `${w}px`;
    }
    return t.fr === undefined ? `${t.min}px` : `minmax(${t.min}px, ${t.fr}fr)`;
  });
  return [...tracks, `${spec.actions}px`].join(' ');
}

/** The narrowest the layout's tracks can get, in px (without the row's inline padding). */
export function gridMinWidth(layout: GridLayout, widths: ColumnWidths): number {
  const spec = GRID_LAYOUTS[layout];
  return spec.columns.reduce((sum, [key, t]) => sum + (widths[key] ?? t.min), spec.actions);
}

/**
 * The CSS custom properties the header, skeleton and rows read: one track list and one minimum
 * width per layout. The rows overflow (and the viewport scrolls sideways) only when the user has
 * widened columns past what the window holds.
 */
export function gridVars(widths: ColumnWidths): Record<string, string> {
  const vars: Record<string, string> = {};
  for (const layout of Object.keys(GRID_LAYOUTS) as GridLayout[]) {
    const suffix = layout === 'full' ? '' : `-${layout}`;
    vars[`--grid-columns${suffix}`] = gridTracks(layout, widths);
    vars[`--grid-min${suffix}`] = `calc(${gridMinWidth(layout, widths)}px + 2 * var(--space-3))`;
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
