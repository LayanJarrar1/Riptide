import { DestroyRef, inject, Injectable } from '@angular/core';

import { UiState } from '../features/shell/ui-state.service';
import type { TorrentSource } from '../model/torrent';
import { TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';

/** The Tauri event a second launch (a magnet link or a `.torrent` file opened from outside) arrives on. */
export const OPEN_TORRENT_EVENT = 'riptide://open-torrent';

/**
 * Opens the file-choice dialog for a torrent that came from outside the app: a magnet link or a
 * `.torrent` file the operating system handed to Riptide (Settings, Integration registers that),
 * either with the first launch (`take_pending_open`, asked once at start-up) or while the app
 * runs (`riptide://open-torrent`). The path or link only opens the dialog; nothing is added until
 * the user presses Download. In a plain browser there is nothing to listen to.
 */
@Injectable({ providedIn: 'root' })
export class TorrentLaunchService {
  private readonly ui = inject(UiState);

  constructor() {
    if (!inject(TAURI_AVAILABLE)) {
      return;
    }
    const invoke = inject(TAURI_INVOKE);
    const listen = inject(TAURI_LISTEN);
    let unlisten: (() => void) | null = null;
    let destroyed = false;
    inject(DestroyRef).onDestroy(() => {
      destroyed = true;
      unlisten?.();
    });
    listen<TorrentSource>(OPEN_TORRENT_EVENT, (source) => this.open(source)).then(
      (off) => {
        if (destroyed) {
          off();
        } else {
          unlisten = off;
        }
      },
      (error: unknown) => console.warn('[torrent] cannot listen for hand-overs', error),
    );
    invoke<TorrentSource | null>('take_pending_open').then(
      (source) => this.open(source),
      (error: unknown) => console.warn('[torrent] take_pending_open failed', error),
    );
  }

  private open(source: TorrentSource | null): void {
    if (source !== null && (source.kind === 'magnet' || source.kind === 'file')) {
      this.ui.openTorrent(source);
    }
  }
}
