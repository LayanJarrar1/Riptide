import { Injectable, signal } from '@angular/core';

import type { TorrentSource } from '../../model/torrent';

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
  /** Which tab the batch dialog opens on (the File menu's "Import link list" opens the list tab). */
  readonly batchTab = signal<'pattern' | 'list'>('pattern');
  /** The site grabber dialog (crawl a site for files). */
  readonly grabberOpen = signal(false);
  /** A page to look into when the grabber opens (Research "Find files on this page"); the grabber takes it and clears it. */
  readonly grabberPreset = signal<string | null>(null);
  /** The torrent being added (magnet link or `.torrent` file); the file-choice dialog shows while set. */
  readonly torrentSource = signal<TorrentSource | null>(null);
  /** Magnet links waiting for their turn in the file-choice dialog (several were pasted or dropped at once). */
  readonly torrentQueue = signal<readonly TorrentSource[]>([]);
  /** The keyboard-shortcuts dialog. */
  readonly shortcutsOpen = signal(false);
  /** Incremented when the menus ask the queues list to open its "New queue" dialog. */
  readonly newQueueRequests = signal(0);
  /** Incremented when the menus ask the list page to remove the selected downloads (it owns the confirmation). */
  readonly removeRequests = signal(0);
  /** Active downloads to warn about when the menus ask to quit; the exit dialog takes it and resets it. */
  readonly exitRequest = signal<number | null>(null);
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

  openBatch(tab: 'pattern' | 'list' = 'pattern'): void {
    this.batchTab.set(tab);
    this.batchOpen.set(true);
  }

  closeBatch(): void {
    this.batchOpen.set(false);
  }

  openTorrent(source: TorrentSource): void {
    this.torrentSource.set(source);
  }

  /** Several magnet links: the file choice opens for the first, the others follow one by one. */
  openTorrents(uris: readonly string[]): void {
    this.openSources(uris.map((uri): TorrentSource => ({ kind: 'magnet', uri })));
  }

  /** Several torrents (magnet links, `.torrent` files): the file choice opens for each in turn. */
  openSources(sources: readonly TorrentSource[]): void {
    const [first, ...rest] = sources;
    if (first === undefined) {
      return;
    }
    this.torrentQueue.set(rest);
    this.torrentSource.set(first);
  }

  /** The dialog closed (added, cancelled or Esc): the next waiting magnet link opens, if any. */
  closeTorrent(): void {
    const [next, ...rest] = this.torrentQueue();
    if (next !== undefined) {
      this.torrentQueue.set(rest);
      this.torrentSource.set(next);
      return;
    }
    this.torrentSource.set(null);
  }

  /** Forget the magnet links that were still waiting. */
  skipQueuedTorrents(): void {
    this.torrentQueue.set([]);
  }

  openGrabber(): void {
    this.grabberOpen.set(true);
  }

  closeGrabber(): void {
    this.grabberOpen.set(false);
    this.grabberPreset.set(null);
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

  requestNewQueue(): void {
    this.newQueueRequests.update((n) => n + 1);
  }

  requestRemoveSelected(): void {
    this.removeRequests.update((n) => n + 1);
  }

  focusSearch(): void {
    this.searchFocusRequests.update((n) => n + 1);
  }

  /** The first-paint orchestration has finished; later arrivals use the ordinary enter. */
  markBooted(): void {
    this.booting.set(false);
  }
}
