import { DestroyRef, DOCUMENT, inject, Injectable } from '@angular/core';

import { UiState } from '../features/shell/ui-state.service';
import { magnetLinesOf } from '../model/torrent';
import { isHttpUrl } from '../model/download';
import type { TorrentSource } from '../model/torrent';
import { TAURI_AVAILABLE, TAURI_LISTEN } from '../tauri/tauri-bridge';

/** What the desktop shell sends when files are dropped on the window (Tauri's native drag-and-drop). */
export const NATIVE_DROP_EVENT = 'tauri://drag-drop';

/** The `.torrent` files among dropped paths, as torrent sources. */
export function torrentFilesIn(paths: readonly string[]): TorrentSource[] {
  return paths.filter((p) => /\.torrent$/i.test(p)).slice(0, 20).map((path): TorrentSource => ({ kind: 'file', path }));
}

/**
 * `.torrent` files dropped on the window open the file choice (one after the other when there
 * are several; other files are ignored). Text dropped on the window: magnet links (one per line) open the file choice, a single web
 * address opens the Add dialog with it filled in. Nothing is added by the drop itself, and
 * anything else (files, other text) is left to the page.
 */
@Injectable({ providedIn: 'root' })
export class DropHandlerService {
  private readonly ui = inject(UiState);
  private readonly doc = inject(DOCUMENT);

  constructor() {
    const target = this.doc.defaultView;
    if (target === null) {
      return;
    }
    if (inject(TAURI_AVAILABLE)) {
      let off: (() => void) | null = null;
      let gone = false;
      inject(DestroyRef).onDestroy(() => {
        gone = true;
        off?.();
      });
      inject(TAURI_LISTEN)<{ readonly paths?: readonly string[] }>(NATIVE_DROP_EVENT, (e) => {
        this.ui.openSources(torrentFilesIn(e.paths ?? []));
      }).then(
        (unlisten) => {
          if (gone) {
            unlisten();
          } else {
            off = unlisten;
          }
        },
        (error: unknown) => console.warn('[drop] cannot listen for dropped files', error),
      );
    }
    const over = (e: DragEvent): void => {
      if (this.hasText(e)) {
        e.preventDefault();
        if (e.dataTransfer) {
          e.dataTransfer.dropEffect = 'copy';
        }
      }
    };
    const drop = (e: DragEvent): void => {
      if (!this.hasText(e)) {
        return;
      }
      const text = e.dataTransfer?.getData('text/plain') || e.dataTransfer?.getData('text/uri-list') || '';
      if (this.handle(text)) {
        e.preventDefault();
      }
    };
    target.addEventListener('dragover', over);
    target.addEventListener('drop', drop);
    inject(DestroyRef).onDestroy(() => {
      target.removeEventListener('dragover', over);
      target.removeEventListener('drop', drop);
    });
  }

  private hasText(e: DragEvent): boolean {
    const types = Array.from(e.dataTransfer?.types ?? []);
    return types.includes('text/plain') || types.includes('text/uri-list');
  }

  /** `true` when the text was something Riptide opens. */
  handle(text: string): boolean {
    const magnets = magnetLinesOf(text);
    if (magnets.length > 0) {
      this.ui.openTorrents(magnets);
      return true;
    }
    const one = text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l.length > 0 && !l.startsWith('#'));
    if (one.length === 1 && isHttpUrl(one[0])) {
      this.ui.openAdd(one[0]);
      return true;
    }
    return false;
  }
}
