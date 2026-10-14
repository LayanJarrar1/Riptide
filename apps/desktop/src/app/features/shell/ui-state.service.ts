import { Injectable, signal } from '@angular/core';

/**
 * Cross-cutting UI state: the add dialog (opened from the top bar, the empty state, a context
 * menu or a keyboard shortcut) and focus requests for the search field.
 */
@Injectable({ providedIn: 'root' })
export class UiState {
  readonly addOpen = signal(false);
  /** Link to pre-fill when the dialog opens (refresh-link flow, clipboard). */
  readonly addUrl = signal<string | null>(null);
  /** Incremented whenever something asks the search box to take focus. */
  readonly searchFocusRequests = signal(0);

  openAdd(url: string | null = null): void {
    this.addUrl.set(url);
    this.addOpen.set(true);
  }

  closeAdd(): void {
    this.addOpen.set(false);
    this.addUrl.set(null);
  }

  focusSearch(): void {
    this.searchFocusRequests.update((n) => n + 1);
  }
}
