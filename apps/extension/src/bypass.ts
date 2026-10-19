/**
 * Short-lived "Alt was held" flags. The Chromium `onDeterminingFilename` event carries no
 * modifier keys, so the content script reports an Alt-click and the background remembers it for
 * a few seconds. Pure (the clock is a parameter) so it can be unit-tested.
 */

/** How long a flag for one URL stays valid. */
export const BYPASS_URL_MS = 4_000;
/** How long the "some Alt-click just happened" flag stays valid (for redirects and `blob:` hops). */
export const BYPASS_ANY_MS = 1_500;
/** Most URLs remembered at once. */
const MAX_FLAGS = 64;

/** Tracks recent Alt-clicks. */
export class BypassTracker {
  private readonly urls = new Map<string, number>();
  private lastAny = Number.NEGATIVE_INFINITY;

  /** Record an Alt-click on `url` (`''` when the target was not a link). */
  mark(url: string, now: number): void {
    this.lastAny = now;
    if (url) this.urls.set(url, now);
    for (const [u, at] of this.urls) if (now - at > BYPASS_URL_MS) this.urls.delete(u);
    while (this.urls.size > MAX_FLAGS) {
      const oldest = this.urls.keys().next().value;
      if (oldest === undefined) break;
      this.urls.delete(oldest);
    }
  }

  /** Was Alt held for this URL (or for any click a moment ago)? */
  was(url: string, now: number): boolean {
    const at = this.urls.get(url);
    if (at !== undefined && now - at < BYPASS_URL_MS) return true;
    return now - this.lastAny < BYPASS_ANY_MS;
  }
}
