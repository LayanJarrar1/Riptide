import { Injectable, signal } from '@angular/core';

/**
 * Cross-cutting UI state: the add dialog (opened from the top bar, the empty state, a context
 * menu or a keyboard shortcut), the shortcuts dialog, focus requests for the search field and
 * the one-time boot flag that drives the orchestrated first paint.
 */
@Injectable({ providedIn: 'root' })
export class UiState {
  readonly addOpen = signal(false);
  /** Link to pre-fill when the dialog opens (refresh-link flow, clipboard). */
  readonly addUrl = signal<string | null>(null);
  /** The batch-download dialog (pattern or list). */
  readonly batchOpen = signal(false);
  /** The keyboard-shortcuts dialog. */
  readonly shortcutsOpen = signal(false);
  /** Incremented whenever something asks the search box to take focus. */
  readonly searchFocusRequests = signal(0);
  /**
   * `true` during the first paint only: shell parts and the first rows enter with a stagger.
   * The shell clears it once the orchestration has played; filters and sorts never set it again.
   */
  readonly booting = signal(true);

  openAdd(url: string | null = null): void {
    this.addUrl.set(url);
    this.addOpen.set(true);
  }

  closeAdd(): void {
    this.addOpen.set(false);
    this.addUrl.set(null);
  }

  openBatch(): void {
    this.batchOpen.set(true);
  }

  closeBatch(): void {
    this.batchOpen.set(false);
  }

  openShortcuts(): void {
    this.shortcutsOpen.set(true);
  }

  closeShortcuts(): void {
    this.shortcutsOpen.set(false);
  }

  toggleShortcuts(): void {
    this.shortcutsOpen.update((open) => !open);
  }

  focusSearch(): void {
    this.searchFocusRequests.update((n) => n + 1);
  }

  /** The first-paint orchestration has finished; later arrivals use the ordinary enter. */
  markBooted(): void {
    this.booting.set(false);
  }
}
