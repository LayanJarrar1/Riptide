import { navFromKey, navigateIndex, scrollToReveal, visibleRange } from './list-nav';

describe('list navigation', () => {
  it('maps the navigation keys and nothing else', () => {
    expect(['ArrowDown', 'ArrowUp', 'Home', 'End', 'PageDown', 'PageUp'].map(navFromKey)).toEqual(['next', 'prev', 'first', 'last', 'pageDown', 'pageUp']);
    expect(navFromKey('a')).toBeNull();
    expect(navFromKey('Enter')).toBeNull();
  });

  describe('navigateIndex', () => {
    it('does nothing in an empty list', () => {
      expect(navigateIndex('next', null, 0, 10)).toBeNull();
      expect(navigateIndex('last', 3, 0, 10)).toBeNull();
    });

    it('steps one row and stops at both ends (no wrapping)', () => {
      expect(navigateIndex('next', 4, 100, 10)).toBe(5);
      expect(navigateIndex('prev', 4, 100, 10)).toBe(3);
      expect(navigateIndex('prev', 0, 100, 10)).toBe(0);
      expect(navigateIndex('next', 99, 100, 10)).toBe(99);
    });

    it('starts at the top for next / page down and at the bottom for prev / page up when there is no cursor', () => {
      expect(navigateIndex('next', null, 50, 10)).toBe(0);
      expect(navigateIndex('pageDown', null, 50, 10)).toBe(0);
      expect(navigateIndex('prev', null, 50, 10)).toBe(49);
      expect(navigateIndex('pageUp', null, 50, 10)).toBe(49);
    });

    it('jumps to the first and last row from anywhere, with or without a cursor', () => {
      expect(navigateIndex('first', 30, 50, 10)).toBe(0);
      expect(navigateIndex('last', 3, 50, 10)).toBe(49);
      expect(navigateIndex('last', null, 50, 10)).toBe(49);
    });

    it('moves a page less one row, clamped to the list', () => {
      expect(navigateIndex('pageDown', 5, 100, 10)).toBe(14);
      expect(navigateIndex('pageUp', 20, 100, 10)).toBe(11);
      expect(navigateIndex('pageDown', 95, 100, 10)).toBe(99);
      expect(navigateIndex('pageUp', 3, 100, 10)).toBe(0);
      // a one-row viewport still moves
      expect(navigateIndex('pageDown', 0, 100, 1)).toBe(1);
    });
  });

  describe('scrollToReveal', () => {
    it('leaves the scroll alone when the row is fully in view', () => {
      expect(scrollToReveal(5, 46, 0, 460)).toBeNull();
      expect(scrollToReveal(9, 46, 0, 460)).toBeNull();
    });

    it('scrolls up just enough to show a row above the viewport', () => {
      expect(scrollToReveal(2, 46, 200, 460)).toBe(92);
    });

    it('scrolls down just enough to show a row below the viewport', () => {
      expect(scrollToReveal(10, 46, 0, 460)).toBe(46);
      expect(scrollToReveal(49_999, 46, 0, 460)).toBe(49_999 * 46 + 46 - 460);
    });
  });

  describe('visibleRange', () => {
    it('is the rows in view plus the overscan on both sides, clamped to the list', () => {
      expect(visibleRange(0, 460, 46, 1000, 6)).toEqual({ start: 0, end: 16 });
      expect(visibleRange(4600, 460, 46, 1000, 6)).toEqual({ start: 94, end: 116 });
      expect(visibleRange(45_540, 460, 46, 1000, 6)).toEqual({ start: 984, end: 1000 });
    });

    it('stays small however long the list is', () => {
      const { start, end } = visibleRange(1_000_000, 780, 46, 50_000, 6);
      expect(end - start).toBeLessThan(40);
    });

    it('is empty for an empty list', () => {
      expect(visibleRange(0, 460, 46, 0, 6)).toEqual({ start: 0, end: 0 });
    });
  });
});
