/** A keyboard move through the visible rows. */
export type ListNav = 'next' | 'prev' | 'first' | 'last' | 'pageDown' | 'pageUp';

/** Maps a `KeyboardEvent.key` to a move; `null` for every other key. */
export function navFromKey(key: string): ListNav | null {
  switch (key) {
    case 'ArrowDown':
      return 'next';
    case 'ArrowUp':
      return 'prev';
    case 'Home':
      return 'first';
    case 'End':
      return 'last';
    case 'PageDown':
      return 'pageDown';
    case 'PageUp':
      return 'pageUp';
    default:
      return null;
  }
}

/**
 * Where a move lands: an index into the `count` visible rows, clamped to both ends (the list
 * does not wrap). `current` is the row the cursor is on, `null` when there is none: then
 * "next" and the page-down move start at the top, "prev" and page-up at the bottom.
 * `pageSize` is how many rows fit the viewport; a page move jumps one page less a row so the
 * last row stays in view as context. Returns `null` for an empty list.
 */
export function navigateIndex(nav: ListNav, current: number | null, count: number, pageSize: number): number | null {
  if (count <= 0) {
    return null;
  }
  const last = count - 1;
  const page = Math.max(1, pageSize - 1);
  const clamp = (n: number): number => Math.min(last, Math.max(0, n));
  switch (nav) {
    case 'first':
      return 0;
    case 'last':
      return last;
    case 'next':
      return current === null ? 0 : clamp(current + 1);
    case 'prev':
      return current === null ? last : clamp(current - 1);
    case 'pageDown':
      return current === null ? 0 : clamp(current + page);
    case 'pageUp':
      return current === null ? last : clamp(current - page);
  }
}

/**
 * The scroll offset that brings row `index` fully into a viewport that is now at `scrollTop`,
 * moving as little as possible; `null` when it is already in view.
 */
export function scrollToReveal(index: number, rowHeight: number, scrollTop: number, viewportHeight: number): number | null {
  const top = index * rowHeight;
  if (top < scrollTop) {
    return top;
  }
  if (top + rowHeight > scrollTop + viewportHeight) {
    return top + rowHeight - viewportHeight;
  }
  return null;
}

/** The first and last row index (end exclusive) to render, with `overscan` rows on both sides. */
export function visibleRange(scrollTop: number, viewportHeight: number, rowHeight: number, count: number, overscan: number): { start: number; end: number } {
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(count, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan);
  return { start: Math.min(start, end), end };
}
