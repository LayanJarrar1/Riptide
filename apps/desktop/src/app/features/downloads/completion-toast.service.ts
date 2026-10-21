import { effect, inject, Injectable, untracked } from '@angular/core';

import { finalPath } from '../../data/bridge-mapping';
import { DownloadEngine } from '../../data/download-engine';
import { I18nService } from '../../i18n/i18n.service';
import { Download, DownloadState } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ToastService } from '../../ui/toast.service';

/**
 * Watches the engine's list and shows a toast the moment a download reaches `completed`, with
 * "Open" and "Show in folder" actions. The first list to arrive is only remembered, never
 * announced, so a restart does not replay old news; after that, every download that is
 * completed now and was not before (including one that appears already complete, say added
 * by the CLI over IPC) gets its toast once.
 *
 * The shell injects this service once to start it; it has no public surface.
 */
@Injectable({ providedIn: 'root' })
export class CompletionToastService {
  private readonly engine = inject(DownloadEngine);
  private readonly toasts = inject(ToastService);
  private readonly i18n = inject(I18nService);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);

  /** The list as of the last look; `null` until the engine has delivered its list. */
  private previous: readonly Download[] | null = null;

  constructor() {
    effect(() => {
      const list = this.engine.downloads();
      const loaded = this.engine.loaded();
      untracked(() => this.look(list, loaded));
    });
  }

  /**
   * Announces what completed since the last look. The engine replaces only the downloads that
   * changed, so with an unchanged shape one pointer compare per download finds them (a map of
   * every state on every tick would be the single most expensive thing the app does with 50 000
   * downloads); a list that was added to, removed from or reordered is compared by id.
   */
  private look(list: readonly Download[], loaded: boolean): void {
    if (!loaded) {
      return;
    }
    const previous = this.previous;
    this.previous = list;
    if (previous === null || previous === list) {
      return;
    }
    const completed: Download[] = [];
    let aligned = previous.length === list.length;
    for (let i = 0; aligned && i < list.length; i++) {
      const d = list[i];
      const p = previous[i];
      if (d === p) {
        continue;
      }
      if (d.id !== p.id) {
        aligned = false;
      } else if (d.state === 'completed' && p.state !== 'completed') {
        completed.push(d);
      }
    }
    if (!aligned) {
      completed.length = 0;
      const before = new Map<number, DownloadState>(previous.map((d) => [d.id, d.state]));
      for (const d of list) {
        if (d.state === 'completed' && before.get(d.id) !== 'completed') {
          completed.push(d);
        }
      }
    }
    for (const d of completed) {
      this.announce(d);
    }
  }

  private announce(d: Download): void {
    this.toasts.show('success', this.i18n.t('toast.completed').replace('{name}', d.filename), [
      { label: this.i18n.t('toast.openFile'), icon: 'external', run: () => this.open('open_file', d) },
      { label: this.i18n.t('toast.openFolder'), icon: 'folderOpen', run: () => this.open('show_in_folder', d) },
    ]);
  }

  /** Same path as the downloads page: the Rust side inside Tauri, a note in a plain browser. */
  private open(command: 'open_file' | 'show_in_folder', d: Download): void {
    if (!this.tauriAvailable) {
      this.toasts.show('info', this.i18n.t('toast.openSoon'));
      return;
    }
    this.invoke<void>(command, { path: finalPath(d.dir, d.filename) }).catch((error: unknown) => {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
    });
  }
}
