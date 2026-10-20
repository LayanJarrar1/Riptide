import { DestroyRef, effect, inject, Injectable, untracked } from '@angular/core';
import { Router } from '@angular/router';

import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { I18nService } from '../../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { UiState } from './ui-state.service';

/** Tauri event: a notification was clicked; the payload is the download id. */
export const OPEN_DOWNLOAD_EVENT = 'riptide://open-download';

/** Tauri event: the update notification was clicked (see `notify.rs`). */
export const OPEN_UPDATE_EVENT = 'riptide://open-update';

/** Tauri event: a downloadable link was copied (see `clipboard.rs`). */
export const CLIPBOARD_LINK_EVENT = 'riptide://clipboard-link';

/** Payload of {@link CLIPBOARD_LINK_EVENT}. */
export interface ClipboardLink {
  readonly url: string;
  readonly name: string;
}

/** Mirror of the Rust `ShellPrefs` sent with `set_shell_prefs`. */
export interface ShellPrefs {
  readonly closeToTray: boolean;
  readonly notifyOnComplete: boolean;
  readonly clipboardMonitor: boolean;
  readonly labels: {
    readonly open: string;
    readonly pauseAll: string;
    readonly resumeAll: string;
    readonly quit: string;
    readonly tooltipIdle: string;
    readonly tooltipActive: string;
    readonly completed: string;
    readonly failed: string;
    readonly completedMany: string;
    readonly units: readonly [string, string, string, string, string];
    readonly perSecond: string;
  };
}

/**
 * Connects the settings to the desktop shell (Tauri only; inert in a plain browser):
 * pushes close-to-tray / notifications / clipboard monitor and the translated tray and
 * notification strings to Rust whenever they or the language change; keeps "Start with
 * Windows" in step with the real autostart entry; opens a download's details when its system
 * notification is clicked; and turns a copied link into a "Download {name}?" prompt that opens
 * the add dialog (never a download by itself).
 *
 * The shell injects this service once to start it.
 */
@Injectable({ providedIn: 'root' })
export class DesktopShellService {
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly listen = inject(TAURI_LISTEN);
  private readonly settings = inject(SettingsService);
  private readonly i18n = inject(I18nService);
  private readonly engine = inject(DownloadEngine);
  private readonly store = inject(DownloadsStore);
  private readonly ui = inject(UiState);
  private readonly toasts = inject(ToastService);
  private readonly router = inject(Router);

  /** What the autostart entry is, as last read or written; `null` until read. */
  private autostart: boolean | null = null;
  private readonly unlisten: (() => void)[] = [];
  private destroyed = false;

  constructor() {
    if (!this.tauri) {
      return;
    }
    effect(() => {
      const prefs = this.prefs();
      untracked(() => {
        this.invoke<void>('set_shell_prefs', { prefs }).catch((error: unknown) => console.warn('[shell] set_shell_prefs failed', error));
      });
    });
    effect(() => {
      const want = this.settings.settings().startWithWindows;
      untracked(() => this.applyAutostart(want));
    });
    void this.readAutostart();
    void this.subscribe();
    inject(DestroyRef).onDestroy(() => {
      this.destroyed = true;
      this.unlisten.forEach((u) => u());
    });
  }

  /** The current preferences with every label in the active language. */
  private prefs(): ShellPrefs {
    const s = this.settings.settings();
    this.i18n.lang();
    const t = (key: string): string => this.i18n.t(key);
    return {
      closeToTray: s.closeToTray,
      notifyOnComplete: s.notifyOnComplete,
      clipboardMonitor: s.clipboardMonitor,
      labels: {
        open: t('tray.open'),
        pauseAll: t('tray.pauseAll'),
        resumeAll: t('tray.resumeAll'),
        quit: t('tray.quit'),
        tooltipIdle: t('tray.idle'),
        tooltipActive: t('tray.active'),
        completed: t('notify.completed'),
        failed: t('notify.failed'),
        completedMany: t('notify.completedMany'),
        units: [t('unit.B'), t('unit.KB'), t('unit.MB'), t('unit.GB'), t('unit.TB')],
        perSecond: t('unit.perSecond'),
      },
    };
  }

  private async readAutostart(): Promise<void> {
    try {
      const enabled = await this.invoke<boolean>('autostart_enabled');
      this.autostart = enabled;
      this.settings.patch({ startWithWindows: enabled });
    } catch (error: unknown) {
      console.warn('[shell] autostart_enabled failed', error);
    }
  }

  private applyAutostart(want: boolean): void {
    if (this.autostart === null || this.autostart === want) {
      return;
    }
    this.autostart = want;
    this.invoke<void>('set_autostart', { enabled: want }).catch((error: unknown) => {
      console.warn('[shell] set_autostart failed', error);
      this.autostart = !want;
      this.settings.patch({ startWithWindows: !want });
      this.toasts.show('danger', this.i18n.t('toast.autostartFailed'));
    });
  }

  private async subscribe(): Promise<void> {
    try {
      const handlers = [
        await this.listen<number>(OPEN_DOWNLOAD_EVENT, (id) => this.openDownload(id)),
        await this.listen<null>(OPEN_UPDATE_EVENT, () => this.openAbout()),
        await this.listen<ClipboardLink>(CLIPBOARD_LINK_EVENT, (link) => this.offerLink(link)),
      ];
      if (this.destroyed) {
        handlers.forEach((u) => u());
      } else {
        this.unlisten.push(...handlers);
      }
    } catch (error: unknown) {
      console.warn('[shell] listen failed', error);
    }
  }

  private openDownload(id: number): void {
    void this.router.navigateByUrl('/').then(() => {
      this.store.select(id);
      this.store.openDetails(id);
    });
  }

  /** Opens Settings and scrolls to the About section (where the update card is). */
  openAbout(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('about')?.scrollIntoView({ block: 'start' }), 50);
    });
  }

  /** Prompts for a copied link unless it is already in the list. */
  offerLink(link: ClipboardLink): void {
    if (this.engine.downloads().some((d) => d.url === link.url)) {
      return;
    }
    this.toasts.show('info', this.i18n.t('clipboard.prompt').replace('{name}', link.name), [
      { label: this.i18n.t('clipboard.download'), icon: 'download', run: () => this.ui.openAdd(link.url) },
      { label: this.i18n.t('action.dismiss'), run: () => undefined },
    ]);
  }
}
