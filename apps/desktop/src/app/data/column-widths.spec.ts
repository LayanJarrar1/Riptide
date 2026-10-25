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
  const nums = (template: string): number[] => template.split(' ').map((t) => Number.parseInt(t.replace('minmax(', ''), 10));
  const fixedWidth = (template: string): number =>
    template.split(' ').reduce((sum, t) => sum + (t.startsWith('minmax') || t.endsWith('fr)') || t.endsWith('px)') ? 0 : Number.parseInt(t, 10)), 0);

  it('exposes the fitted template as the one grid variable', () => {
    expect(gridVars(fitGrid(1300, {}))).toEqual({ '--grid-columns': fitGrid(1300, {}).template });
  });

  it('keeps the default tracks when there is room and nothing was customised', () => {
    const fit = fitGrid(1300, {});
    expect(fit.visible).toEqual(['name', 'size', 'progress', 'speed', 'eta', 'status', 'added']);
    expect(fit.autoHidden).toEqual([]);
    expect(fit.template).toBe('minmax(160px, 2.2fr) 120px minmax(120px, 1.6fr) 96px 112px 160px 104px 60px');
  });

  it('the default columns fit a 1024px list without hiding anything', () => {
    const fit = fitGrid(1024 - 24, {});
    expect(fit.autoHidden).toEqual([]);
    // fixed tracks plus the flexible minimums leave the tracks inside the list
    expect(fixedWidth(fit.template) + 160 + 120).toBeLessThanOrEqual(1000);
  });

  it('a user width becomes a fixed track while it fits', () => {
    expect(fitGrid(1300, { name: 420, speed: 130 }).template).toBe('420px 120px minmax(120px, 1.6fr) 130px 112px 160px 104px 60px');
  });

  it('shrinks the flexible columns first, down to their minimum, before touching the others', () => {
    const fit = nums(fitGrid(1500, { name: 700, progress: 400 }).template);
    const [name, size, progress, speed, eta, status, added] = fit;
    expect([size, speed, eta, status, added]).toEqual([120, 96, 112, 160, 104]);
    expect(name + progress + 120 + 96 + 112 + 160 + 104 + ACTIONS_PX).toBeLessThanOrEqual(1500);
    expect(name).toBeGreaterThanOrEqual(160);
    expect(progress).toBeGreaterThanOrEqual(120);
    expect(name).toBeLessThan(700);
    // a tighter list takes the flexible ones to the floor, then scales the others proportionally
    const tight = nums(fitGrid(980, { name: 700, progress: 400, size: 300, status: 300 }).template);
    expect(tight[0]).toBe(160);
    expect(tight[2]).toBe(120);
    expect(tight[1]).toBeGreaterThanOrEqual(96);
    expect(tight[1]).toBeLessThan(300);
    expect(tight[5]).toBeGreaterThanOrEqual(132);
    expect(tight[5]).toBeLessThan(300);
    expect(tight.slice(0, 7).reduce((a, b) => a + b, ACTIONS_PX)).toBeLessThanOrEqual(980);
    // proportional: the column with the larger slack (status: 300-132) gives up more than size (300-96)? no, share is by slack
    const sizeCut = (300 - tight[1]) / (300 - 96);
    const statusCut = (300 - tight[5]) / (300 - 132);
    expect(Math.abs(sizeCut - statusCut)).toBeLessThan(0.05);
  });

  it('never produces tracks wider than the available width, for any width and preference', () => {
    const wide = { name: 1600, size: 900, progress: 1600, speed: 500, eta: 700, status: 1000, added: 400, avgSpeed: 300, duration: 300, category: 300 };
    for (let available = 300; available <= 2400; available += 37) {
      for (const widths of [{}, wide, { name: 1600 }]) {
        const fit = fitGrid(available, widths, none);
        if (available >= 160 + 120 + 132 + ACTIONS_PX) {
          const mins = { name: 160, progress: 120, size: 96, speed: 80, avgSpeed: 88, eta: 84, duration: 80, status: 132, category: 96, added: 96 } as Record<string, number>;
          const flexMins = fit.visible.filter((k) => (k === 'name' || k === 'progress') && widths[k as keyof typeof widths] === undefined).reduce((sum, k) => sum + mins[k], 0);
          expect(fixedWidth(fit.template) + flexMins).toBeLessThanOrEqual(available);
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
    const minimums: Record<string, number> = { name: 160, progress: 120, speed: 80, eta: 84, status: 132, added: 96 };
    const others = visible.filter((k) => k !== 'size').reduce((sum, k) => sum + minimums[k], 0);
    expect(maxDragWidth('size', 1000, visible)).toBe(1000 - ACTIONS_PX - others);
    expect(maxDragWidth('size', 100, visible)).toBe(96);
    expect(maxDragWidth('name', 5000, ['name', 'progress', 'status'])).toBe(1600);
  });

  it('clamps to the per-column minimum and the global maximum', () => {
    expect(clampWidth('name', 10)).toBe(160);
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
