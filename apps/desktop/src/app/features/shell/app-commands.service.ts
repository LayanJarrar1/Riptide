import { computed, inject, Injectable } from '@angular/core';
import { Router } from '@angular/router';

import { UpdateService } from '../../core/update.service';
import { WindowService } from '../../core/window.service';
import { finalPath } from '../../data/bridge-mapping';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { I18nService, Lang } from '../../i18n/i18n.service';
import { canPause, canResume, Download } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { Theme, ThemeService } from '../../theme/theme.service';
import { ToastService } from '../../ui/toast.service';
import { Settings, SettingsService } from '../settings/settings.service';
import { UiState } from './ui-state.service';

/** The documentation site (fixed; the webview may open nothing else, see `capabilities/default.json`). */
export const DOCS_URL = 'https://layanjarrar1.github.io/Riptide/';

/** The source repository (fixed, same scope rule). */
export const REPO_URL = 'https://github.com/LayanJarrar1/Riptide';

/**
 * What the title-bar menus can do. Every entry calls the code the rest of the app already uses
 * (the same dialogs, engine commands and Rust commands as the buttons and the row menu); this
 * service only gathers them in one place and knows which ones apply to the current selection.
 */
@Injectable({ providedIn: 'root' })
export class AppCommands {
  private readonly ui = inject(UiState);
  private readonly store = inject(DownloadsStore);
  private readonly engine = inject(DownloadEngine);
  private readonly settings = inject(SettingsService);
  private readonly theme = inject(ThemeService);
  private readonly i18n = inject(I18nService);
  private readonly updates = inject(UpdateService);
  private readonly toasts = inject(ToastService);
  private readonly router = inject(Router);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly win = inject(WindowService);
  /** Whether the Tauri shell is around (some commands only exist there). */
  readonly tauri = inject(TAURI_AVAILABLE);

  private readonly selected = computed(() => this.store.selectedRows());

  /** At least one selected download can be started or resumed. */
  readonly canResume = computed(() => this.selected().some((r) => canResume(r.d.state)));
  /** At least one selected download is running or waiting. */
  readonly canPause = computed(() => this.selected().some((r) => canPause(r.d.state)));
  /** At least one selected download is not finished or cancelled yet. */
  readonly canCancel = computed(() => this.selected().some((r) => r.d.state !== 'completed' && r.d.state !== 'cancelled'));
  /** Something is selected. */
  readonly canRemove = computed(() => this.selected().length > 0);
  /** Exactly one finished download is selected: its file and folder can be opened. */
  readonly openable = computed<Download | null>(() => {
    const rows = this.selected();
    return rows.length === 1 && rows[0].d.state === 'completed' ? rows[0].d : null;
  });
  /** Anything is running or waiting (for Pause all). */
  readonly anyRunning = computed(() => this.store.totals().active > 0 || this.store.totals().queued > 0);
  /** Anything is paused or failed (for Resume all). */
  readonly anyResumable = computed(() => this.store.viewCounts().queued > 0 || this.store.viewCounts().failed > 0);

  // ---- File ----------------------------------------------------------------------------
  addDownload(): void {
    this.ui.openAdd();
  }

  batchDownload(): void {
    this.ui.openBatch();
  }

  siteGrabber(): void {
    this.ui.openGrabber();
  }

  importLinkList(): void {
    this.ui.openBatch('list');
  }

  /** File menu: pick a `.torrent` in the native dialog, then choose its files. */
  openTorrentFile(): void {
    if (!this.tauri) {
      this.toasts.show('info', this.i18n.t('torrent.desktopOnly'));
      return;
    }
    this.run(
      this.engine.pickTorrentFile().then((path) => {
        if (path !== null) {
          this.ui.openTorrent({ kind: 'file', path });
        }
      }),
    );
  }

  openFile(): void {
    this.openPath('open_file');
  }

  openFolder(): void {
    this.openPath('show_in_folder');
  }

  /** Quits for real (not to the tray); asks first when downloads run and the setting is on. */
  exit(): void {
    const active = this.store.totals().active;
    if (this.settings.settings().confirmOnExit && active > 0) {
      this.ui.exitRequest.set(active);
      return;
    }
    this.invoke<void>('quit_app').catch((error: unknown) => console.warn('[exit] quit_app failed', error));
  }

  // ---- Downloads -----------------------------------------------------------------------
  resumeSelected(): void {
    for (const r of this.selected()) {
      if (canResume(r.d.state)) {
        this.run(this.engine.resume(r.d.id));
      }
    }
  }

  pauseSelected(): void {
    for (const r of this.selected()) {
      if (canPause(r.d.state)) {
        this.run(this.engine.pause(r.d.id));
      }
    }
  }

  cancelSelected(): void {
    for (const r of this.selected()) {
      if (r.d.state !== 'completed' && r.d.state !== 'cancelled') {
        this.run(this.engine.cancel(r.d.id));
      }
    }
  }

  /** The list page owns the confirmation (and the "also delete the file" choice). */
  removeSelected(): void {
    void this.router.navigateByUrl('/').then(() => this.ui.requestRemoveSelected());
  }

  pauseAll(): void {
    this.run(this.engine.pauseAll());
    this.toasts.show('info', this.i18n.t('toast.pausedAll'));
  }

  resumeAll(): void {
    this.run(this.engine.resumeAll());
    this.toasts.show('info', this.i18n.t('toast.resumedAll'));
  }

  /** Opens the queue editor on a new queue (the queues list lives in the sidebar, so it is shown). */
  newQueue(): void {
    if (!this.settings.settings().sidebarVisible) {
      this.settings.patch({ sidebarVisible: true });
    }
    this.ui.requestNewQueue();
  }

  // ---- Tools ---------------------------------------------------------------------------
  /** Opens Settings, scrolled to `section` (`appearance`, `downloads`, `behaviour`, `integration`, `about`). */
  openSettings(section?: string): void {
    void this.router.navigateByUrl('/settings').then(() => {
      if (section !== undefined) {
        setTimeout(() => document.getElementById(section)?.scrollIntoView({ block: 'start' }), 50);
      }
    });
  }

  /** Downloads the newest yt-dlp (what "Update video support" in Settings does). */
  async updateVideoSupport(): Promise<void> {
    try {
      const version = await this.invoke<string>('update_media_tools');
      this.toasts.show('success', this.i18n.t('toast.videoUpdated').replace('{version}', version));
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.videoUpdateFailed'));
    }
  }

  // ---- View ----------------------------------------------------------------------------
  setTheme(theme: Theme): void {
    this.theme.setTheme(theme);
  }

  setLanguage(lang: Lang): void {
    this.i18n.setLang(lang);
  }

  toggleSidebar(): void {
    this.settings.patch({ sidebarVisible: !this.settings.settings().sidebarVisible });
  }

  setDensity(density: Settings['density']): void {
    this.settings.patch({ density });
  }

  // ---- Window --------------------------------------------------------------------------
  minimize(): void {
    this.win.minimize();
  }

  toggleMaximize(): void {
    this.win.toggleMaximize();
  }

  closeWindow(): void {
    this.win.close();
  }

  // ---- Help ----------------------------------------------------------------------------
  showShortcuts(): void {
    this.ui.openShortcuts();
  }

  openDocs(): void {
    this.openLink(DOCS_URL);
  }

  openRepo(): void {
    this.openLink(REPO_URL);
  }

  /** The Settings "Check for updates" flow, reported with a toast. */
  async checkForUpdates(): Promise<void> {
    try {
      const update = await this.updates.check();
      if (update === null) {
        this.toasts.show('success', this.i18n.t('toast.upToDate'));
      } else {
        this.toasts.show(
          'info',
          this.i18n.t('toast.updateAvailable').replace('{version}', update.version),
          { label: this.i18n.t('settings.updateRestart'), run: () => void this.updates.installOrToast() },
          15_000,
        );
      }
    } catch {
      this.toasts.show('danger', this.i18n.t('toast.updateCheckFailed'));
    }
  }

  about(): void {
    this.openSettings('about');
  }

  // ---- internals -----------------------------------------------------------------------
  /** Both URLs are constants above; the opener scope in `capabilities/default.json` allows exactly these. */
  private openLink(url: string): void {
    if (!this.tauri) {
      window.open(url, '_blank', 'noopener,noreferrer');
      return;
    }
    this.invoke<void>('plugin:opener|open_url', { url }).catch((error: unknown) => console.warn('[help] open_url failed', error));
  }

  private openPath(command: 'open_file' | 'show_in_folder'): void {
    const d = this.openable();
    if (d === null) {
      return;
    }
    if (!this.tauri) {
      this.toasts.show('info', this.i18n.t('toast.openSoon'));
      return;
    }
    this.run(this.invoke<void>(command, { path: finalPath(d.dir, d.filename) }));
  }

  private run(command: Promise<void>): void {
    command.catch((error: unknown) => {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
    });
  }
}
