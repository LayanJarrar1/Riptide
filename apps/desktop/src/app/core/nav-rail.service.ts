import { inject, Injectable, signal } from '@angular/core';

import { PREFERENCE_STORE } from './preferences';

export const NAV_RAIL_KEY = 'riptide.nav.rail';

/** Pages that give their width to their own content: the main navigation shrinks to an icon rail there. */
export function prefersRail(url: string): boolean {
  return /^\/(?:research|files)(?:[/?#]|$)/.test(url);
}

/**
 * Whether the main navigation is shown as an icon rail on the pages that prefer one (Research, Files).
 * The choice is remembered. Downloads and Settings always keep the expanded navigation.
 */
@Injectable({ providedIn: 'root' })
export class NavRailService {
  private readonly store = inject(PREFERENCE_STORE);

  /** `true` when the user expanded the navigation on a rail page. */
  readonly expanded = signal(this.store.get(NAV_RAIL_KEY) === 'expanded');

  /** The rail applies on this address, unless the user asked for the full navigation. */
  isRail(url: string): boolean {
    return prefersRail(url) && !this.expanded();
  }

  toggle(): void {
    this.expanded.update((v) => !v);
    this.store.set(NAV_RAIL_KEY, this.expanded() ? 'expanded' : 'rail');
  }
}
