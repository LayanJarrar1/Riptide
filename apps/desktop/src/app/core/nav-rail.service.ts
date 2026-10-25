import { computed, DestroyRef, inject, Injectable, InjectionToken, signal } from '@angular/core';
import { NavigationEnd, Router } from '@angular/router';

import { PREFERENCE_STORE } from './preferences';

/** Remembered choice for Research and Files (`expanded` | `rail`); the original key, kept so no value is lost. */
export const NAV_RAIL_KEY = 'riptide.nav.rail';
/** Remembered choice for Downloads and Settings (`expanded` | `rail`). */
export const NAV_MAIN_KEY = 'riptide.nav.main';

/** Below this width the expanded navigation is an overlay on top of the page. */
export const NARROW_QUERY_TEXT = '(max-width: 900px)';

/** The part of `MediaQueryList` the navigation needs (a spec supplies its own). */
export interface NarrowQuery {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: (event: { matches: boolean }) => void): void;
  removeEventListener?(type: 'change', listener: (event: { matches: boolean }) => void): void;
}

export const NARROW_QUERY = new InjectionToken<NarrowQuery>('riptide.narrowQuery', {
  providedIn: 'root',
  factory: () => globalThis.matchMedia(NARROW_QUERY_TEXT),
});

/** Pages that give their width to their own content: the navigation starts as an icon rail there. */
export function prefersRail(url: string): boolean {
  return /^\/(?:research|files)(?:[/?#]|$)/.test(url);
}

/**
 * State of the main navigation, driven by the one toggle in the top bar.
 *
 * Collapsed is the compact icon rail, expanded is the full navigation. Research and Files start
 * compact, Downloads and Settings start expanded, and the user's choice is remembered for each of
 * those two groups. Under 900 px the navigation is always a rail, and the toggle opens the full
 * navigation as a temporary overlay instead (not remembered).
 */
@Injectable({ providedIn: 'root' })
export class NavRailService {
  private readonly store = inject(PREFERENCE_STORE);
  private readonly router = inject(Router);
  private readonly query = inject(NARROW_QUERY);

  private readonly url = signal(this.router.url);
  private readonly wideExpanded = signal(this.store.get(NAV_RAIL_KEY) === 'expanded');
  private readonly mainExpanded = signal(this.store.get(NAV_MAIN_KEY) !== 'rail');

  /** The window is narrower than 900 px. */
  readonly narrow = signal(this.query.matches);
  /** The temporary full navigation is showing (narrow windows only). */
  private readonly opened = signal(false);

  /** The full navigation is showing (a remembered choice, or the open overlay). */
  readonly expanded = computed(() => {
    if (this.narrow()) {
      return this.opened();
    }
    return prefersRail(this.url()) ? this.wideExpanded() : this.mainExpanded();
  });
  /** The navigation takes only the rail's width in the layout. */
  readonly rail = computed(() => this.narrow() || !this.expanded());
  /** The full navigation floats over the page (with a scrim). */
  readonly overlay = computed(() => this.narrow() && this.opened());

  constructor() {
    const sub = this.router.events.subscribe((e) => {
      if (e instanceof NavigationEnd) {
        this.url.set(e.urlAfterRedirects);
        this.opened.set(false);
      }
    });
    const onChange = (e: { matches: boolean }): void => {
      this.narrow.set(e.matches);
      this.opened.set(false);
    };
    this.query.addEventListener('change', onChange);
    inject(DestroyRef).onDestroy(() => {
      sub.unsubscribe();
      this.query.removeEventListener?.('change', onChange);
    });
  }

  /** Flips the navigation: the remembered choice for this page group, or the overlay when narrow. */
  toggle(): void {
    if (this.narrow()) {
      this.opened.update((v) => !v);
      return;
    }
    if (prefersRail(this.url())) {
      this.wideExpanded.update((v) => !v);
      this.store.set(NAV_RAIL_KEY, this.wideExpanded() ? 'expanded' : 'rail');
    } else {
      this.mainExpanded.update((v) => !v);
      this.store.set(NAV_MAIN_KEY, this.mainExpanded() ? 'expanded' : 'rail');
    }
  }

  /** Makes sure the full navigation is showing (something in it was asked for). */
  reveal(): void {
    if (!this.expanded()) {
      this.toggle();
    }
  }

  /** Closes the overlay (Escape, a click outside, a navigation). */
  closeOverlay(): void {
    this.opened.set(false);
  }
}
