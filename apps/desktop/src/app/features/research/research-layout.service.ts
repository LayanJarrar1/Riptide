import { inject, Injectable, signal } from '@angular/core';

import { CATEGORY_IDS, CategoryId } from '../../ai/research.models';
import { PREFERENCE_STORE } from '../../core/preferences';

/** Where the research page keeps its layout preferences (this machine only). */
export const SIDE_KEY = 'riptide.research.side.v1';
export const LAST_CATEGORY_KEY = 'riptide.research.lastCategory';

/** The history sidebar can be dragged between these widths. */
export const SIDE_MIN = 220;
export const SIDE_MAX = 420;
export const SIDE_DEFAULT = 264;

/** A width held inside the allowed range (a stored or dragged value is never trusted as it is). */
export function clampSide(width: number): number {
  return Number.isFinite(width) ? Math.min(SIDE_MAX, Math.max(SIDE_MIN, Math.round(width))) : SIDE_DEFAULT;
}

/**
 * The research page's layout preferences: whether the history sidebar is open, how wide it is, and the
 * category used last (offered as a suggestion on a new conversation, never chosen for the user).
 */
@Injectable({ providedIn: 'root' })
export class ResearchLayoutService {
  private readonly store = inject(PREFERENCE_STORE);
  private readonly saved = this.read();

  readonly sideOpen = signal(this.saved.open);
  readonly sideWidth = signal(this.saved.width);
  readonly lastCategory = signal<CategoryId | null>(this.readCategory());

  toggleSide(): void {
    this.sideOpen.update((v) => !v);
    this.save();
  }

  /** Sets the width; a drag passes `persist: false` until it ends, so storage is written once. */
  setSideWidth(width: number, persist = true): void {
    this.sideWidth.set(clampSide(width));
    if (persist) {
      this.save();
    }
  }

  /** Remembers the category a search used, as the next conversation's suggestion. */
  rememberCategory(category: CategoryId): void {
    this.lastCategory.set(category);
    this.store.set(LAST_CATEGORY_KEY, category);
  }

  private save(): void {
    this.store.set(SIDE_KEY, JSON.stringify({ open: this.sideOpen(), width: this.sideWidth() }));
  }

  private read(): { open: boolean; width: number } {
    try {
      const raw = JSON.parse(this.store.get(SIDE_KEY) ?? 'null') as { open?: unknown; width?: unknown } | null;
      return { open: raw?.open !== false, width: typeof raw?.width === 'number' ? clampSide(raw.width) : SIDE_DEFAULT };
    } catch {
      return { open: true, width: SIDE_DEFAULT };
    }
  }

  private readCategory(): CategoryId | null {
    const raw = this.store.get(LAST_CATEGORY_KEY);
    return (CATEGORY_IDS as readonly string[]).includes(raw ?? '') ? (raw as CategoryId) : null;
  }
}
