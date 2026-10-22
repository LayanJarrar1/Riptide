import { MemoryPreferenceStore } from '../core/preferences';
import {
  clampWidth,
  ColumnKey,
  COLUMN_WIDTHS_KEY,
  gridMinWidth,
  gridTracks,
  gridVars,
  DEFAULT_HIDDEN,
  HIDDEN_COLUMNS_KEY,
  loadColumnWidths,
  loadHiddenColumns,
  MAX_COLUMN_WIDTH,
  saveColumnWidths,
  saveHiddenColumns,
} from './column-widths';

describe('column widths', () => {
  it('keeps the shipped tracks when nothing was customised', () => {
    expect(gridTracks('full', {})).toBe('minmax(190px, 2.2fr) 150px minmax(150px, 1.6fr) 96px 84px 172px 104px 64px');
    expect(gridTracks('compact', {})).toBe('minmax(180px, 2fr) 128px minmax(120px, 1.4fr) 92px 150px 60px');
    expect(gridTracks('narrow', {})).toBe('minmax(160px, 2fr) 110px minmax(100px, 1.4fr) 140px 52px');
    expect(gridTracks('tiny', {})).toBe('minmax(140px, 1fr) minmax(90px, 1fr) 44px');
    expect(gridMinWidth('full', {})).toBe(1010);
  });

  it('a custom width becomes a fixed track in every layout that shows the column', () => {
    const widths = { name: 420, speed: 130 };
    expect(gridTracks('full', widths)).toBe('420px 150px minmax(150px, 1.6fr) 130px 84px 172px 104px 64px');
    expect(gridTracks('narrow', widths)).toBe('420px 110px minmax(100px, 1.4fr) 140px 52px');
    expect(gridTracks('tiny', widths)).toBe('420px minmax(90px, 1fr) 44px');
    expect(gridMinWidth('tiny', widths)).toBe(420 + 90 + 44);
  });

  it('exposes one track list and one minimum per layout', () => {
    const vars = gridVars({ name: 300 });
    expect(Object.keys(vars).sort()).toEqual(
      ['--grid-columns', '--grid-columns-compact', '--grid-columns-narrow', '--grid-columns-tiny', '--grid-min', '--grid-min-compact', '--grid-min-narrow', '--grid-min-tiny'].sort(),
    );
    expect(vars['--grid-min-tiny']).toBe('calc(434px + 2 * var(--space-3))');
  });

  it('clamps to the per-column minimum and the global maximum', () => {
    expect(clampWidth('name', 10)).toBe(120);
    expect(clampWidth('size', 10)).toBe(64);
    expect(clampWidth('status', 99999)).toBe(MAX_COLUMN_WIDTH);
    expect(clampWidth('eta', 100.6)).toBe(101);
  });

  it('round-trips through the preference store', () => {
    const store = new MemoryPreferenceStore();
    saveColumnWidths(store, { name: 500, added: 140 });
    expect(loadColumnWidths(store)).toEqual({ name: 500, added: 140 });
  });

  it('ignores malformed, unknown and out-of-range saved values', () => {
    expect(loadColumnWidths(new MemoryPreferenceStore({ [COLUMN_WIDTHS_KEY]: 'not json' }))).toEqual({});
    expect(loadColumnWidths(new MemoryPreferenceStore({ [COLUMN_WIDTHS_KEY]: '[1,2]' }))).toEqual({});
    const odd = JSON.stringify({ name: 'wide', size: 5, bogus: 300, status: 1e9, eta: null });
    expect(loadColumnWidths(new MemoryPreferenceStore({ [COLUMN_WIDTHS_KEY]: odd }))).toEqual({ size: 64, status: MAX_COLUMN_WIDTH });
  });

  it('shows the optional columns only when turned on, and only in the full layout', () => {
    const shown = new Set<ColumnKey>();
    expect(gridTracks('full', {}, shown)).toBe(
      'minmax(190px, 2.2fr) 150px minmax(150px, 1.6fr) 96px 104px 84px 96px 172px 112px 104px 64px',
    );
    expect(gridTracks('compact', {}, shown)).toBe(gridTracks('compact', {}));
    expect(gridTracks('full', {}, new Set<ColumnKey>(['size', 'eta', 'avgSpeed', 'duration', 'category']))).toBe(
      'minmax(190px, 2.2fr) minmax(150px, 1.6fr) 96px 172px 104px 64px',
    );
    // the name can never be hidden
    expect(gridTracks('tiny', {}, new Set<ColumnKey>(['name', 'progress']))).toBe('minmax(140px, 1fr) 44px');
  });

  it('round-trips the hidden columns and falls back to the defaults', () => {
    const store = new MemoryPreferenceStore();
    expect(loadHiddenColumns(store)).toBe(DEFAULT_HIDDEN);
    saveHiddenColumns(store, new Set<ColumnKey>(['added', 'name', 'eta']));
    expect(store.get(HIDDEN_COLUMNS_KEY)).toBe('["eta","added"]');
    expect([...loadHiddenColumns(store)]).toEqual(['eta', 'added']);
    expect(loadHiddenColumns(new MemoryPreferenceStore({ [HIDDEN_COLUMNS_KEY]: '{"a":1}' }))).toBe(DEFAULT_HIDDEN);
    expect(loadHiddenColumns(new MemoryPreferenceStore({ [HIDDEN_COLUMNS_KEY]: 'nope' }))).toBe(DEFAULT_HIDDEN);
    expect(loadHiddenColumns(new MemoryPreferenceStore({ [HIDDEN_COLUMNS_KEY]: '[]' })).size).toBe(0);
  });
});
