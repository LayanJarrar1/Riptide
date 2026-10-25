import { MemoryPreferenceStore } from '../core/preferences';
import {
  clampWidth,
  ColumnKey,
  COLUMN_WIDTHS_KEY,
  ACTIONS_PX,
  AUTO_HIDE_ORDER,
  fitGrid,
  maxDragWidth,
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
  const none = new Set<ColumnKey>();
  /** [floor px, is flexible] of every track but the actions one. */
  const tracks = (template: string): { px: number; flex: boolean }[] =>
    (template.match(/minmax\([^)]*\)|\d+px/g) ?? [])
      .slice(0, -1)
      .map((t) => ({ px: Number.parseInt(t.replace('minmax(', ''), 10), flex: t.startsWith('minmax') }));
  /** The least width the template can take: every floor plus the actions track. */
  const floorSum = (template: string): number => tracks(template).reduce((sum, t) => sum + t.px, 0) + ACTIONS_PX;

  it('exposes the fitted template as the one grid variable', () => {
    expect(gridVars(fitGrid(1300, {}))).toEqual({ '--grid-columns': fitGrid(1300, {}).template });
  });

  it('keeps the default tracks when there is room and nothing was customised', () => {
    const fit = fitGrid(1300, {});
    expect(fit.visible).toEqual(['name', 'size', 'progress', 'speed', 'eta', 'status', 'added']);
    expect(fit.autoHidden).toEqual([]);
    expect(fit.template).toBe('minmax(280px, 3fr) 150px minmax(120px, 1fr) 96px 112px 160px 104px 60px');
  });

  it('a 1024px window (785px list) keeps the name at 240px or more and ends exactly at the edge', () => {
    const available = 785 - 24;
    const fit = fitGrid(available, {});
    expect(fit.visible).toContain('status');
    expect(floorSum(fit.template)).toBeLessThanOrEqual(available);
    const t = tracks(fit.template);
    const spare = available - floorSum(fit.template);
    // spare room goes to the name first: its share (3fr of 4) is the larger one
    expect(t[0].px + (spare * 3) / 4).toBeGreaterThanOrEqual(240);
  });

  it('a user width keeps its floor while it fits', () => {
    expect(fitGrid(1300, { name: 420, speed: 130 }).template).toBe('420px 150px minmax(120px, 1fr) 130px 112px 160px 104px 60px');
  });

  it('shrinks every other column before the progress, and the name last', () => {
    // preferences: name 700 + progress 400 + size 300 + status 300
    const widths = { name: 700, progress: 400, size: 300, status: 300 };
    const roomy = tracks(fitGrid(2200, widths).template);
    expect(roomy.map((t) => t.px)).toEqual([700, 300, 400, 96, 112, 300, 104]);
    // 1000px: the others drop toward their minimums first, name and progress keep their wants
    const mid = tracks(fitGrid(1900, widths).template).map((t) => t.px);
    expect(mid[0]).toBe(700);
    expect(mid[2]).toBe(400);
    expect(mid[1]).toBeLessThan(300);
    expect(mid[5]).toBeLessThan(300);
    expect(floorSum(fitGrid(1900, widths).template)).toBeLessThanOrEqual(1900);
    // tighter: the others sit at their minimums, then the progress gives way, then the name
    const tight = tracks(fitGrid(1050, widths).template).map((t) => t.px);
    expect(tight[1]).toBe(96);
    expect(tight[5]).toBe(132);
    expect(tight[2]).toBeLessThan(400);
    expect(tight[0]).toBeGreaterThan(200);
    const tighter = tracks(fitGrid(861, widths).template).map((t) => t.px);
    expect(tighter[2]).toBe(120);
    expect(floorSum(fitGrid(861, widths).template)).toBeLessThanOrEqual(861);
  });

  it('never produces tracks wider than the available width, for any width and preference', () => {
    const wide = { name: 1600, size: 900, progress: 1600, speed: 500, eta: 700, status: 1000, added: 400, avgSpeed: 300, duration: 300, category: 300 };
    for (let available = 300; available <= 2400; available += 37) {
      for (const widths of [{}, wide, { name: 1600 }]) {
        const fit = fitGrid(available, widths, none);
        if (available >= 200 + 120 + 132 + ACTIONS_PX) {
          expect(floorSum(fit.template)).toBeLessThanOrEqual(available);
        }
        expect(fit.visible).toContain('name');
        expect(fit.visible).toContain('progress');
        expect(fit.visible).toContain('status');
      }
    }
  });

  it('hides columns in the fixed priority order and never the name, progress or status', () => {
    expect(AUTO_HIDE_ORDER).toEqual(['duration', 'avgSpeed', 'added', 'category', 'eta', 'speed', 'size']);
    expect(fitGrid(5000, {}, none).visible.length).toBe(10);
    let previous = 0;
    for (let available = 1300; available >= 400; available -= 5) {
      const gone = fitGrid(available, {}, none).autoHidden;
      // always a prefix of the order, and it only grows as the list narrows
      expect(AUTO_HIDE_ORDER.slice(0, gone.length).every((k) => gone.includes(k))).toBeTrue();
      expect(gone.length).toBeGreaterThanOrEqual(previous);
      previous = gone.length;
    }
    expect(fitGrid(300, {}, none).visible).toEqual(['name', 'progress', 'status']);
  });

  it('auto-hidden columns come back when the window widens, and the preferences are untouched', () => {
    const widths = { size: 140, eta: 130 };
    const narrow = fitGrid(520, widths, none);
    expect(narrow.autoHidden.length).toBeGreaterThan(0);
    expect(narrow.autoHidden).not.toContain('status');
    const wide = fitGrid(1900, widths, none);
    expect(wide.autoHidden).toEqual([]);
    expect(wide.visible.length).toBe(10);
    expect(widths).toEqual({ size: 140, eta: 130 });
    // a column the user turned off is just off, not auto-hidden
    expect(fitGrid(520, {}, new Set<ColumnKey>(['duration', 'avgSpeed', 'added', 'category'])).autoHidden).not.toContain('duration');
  });

  it('clamps a drag so the others keep their minimums', () => {
    const visible = fitGrid(1000, {}).visible;
    const minimums: Record<string, number> = { name: 200, progress: 120, speed: 80, eta: 84, status: 132, added: 96 };
    const others = visible.filter((k) => k !== 'size').reduce((sum, k) => sum + minimums[k], 0);
    expect(maxDragWidth('size', 1000, visible)).toBe(1000 - ACTIONS_PX - others);
    expect(maxDragWidth('size', 100, visible)).toBe(96);
    expect(maxDragWidth('name', 5000, ['name', 'progress', 'status'])).toBe(1600);
  });

  it('clamps to the per-column minimum and the global maximum', () => {
    expect(clampWidth('name', 10)).toBe(200);
    expect(clampWidth('size', 10)).toBe(96);
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
    expect(loadColumnWidths(new MemoryPreferenceStore({ [COLUMN_WIDTHS_KEY]: odd }))).toEqual({ size: 96, status: MAX_COLUMN_WIDTH });
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
