import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, ElementRef, inject, input, output, signal, untracked, viewChild } from '@angular/core';

import { DiskSpaceService } from '../../core/disk-space.service';
import { FormatService } from '../../core/format.service';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { I18nService } from '../../i18n/i18n.service';
import { selectedBytes, TorrentPreview, TorrentSource, webSeedUrl } from '../../model/torrent';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { UiState } from '../shell/ui-state.service';
import { TorrentFileListComponent } from './torrent-file-list.component';

type Phase = 'resolving' | 'ready' | 'error';

/**
 * Add a torrent: resolves the magnet link (or reads the `.torrent` file), lists the files with
 * their sizes and lets the user choose which to download, in order or all at once, before
 * anything is written. Nothing is added until "Download" is pressed; closing the dialog while the
 * magnet link is still looking for peers just drops the answer.
 */
@Component({
  selector: 'app-torrent-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent, IconComponent, TorrentFileListComponent],
  template: `
    <app-dialog [open]="open()" [title]="i18n.t('torrent.title')" [wide]="true" (closed)="close()">
      <div class="body" tabindex="-1" #body>
        @if (ui.torrentQueue().length > 0) {
          <p class="queue-note" role="status">
            {{ i18n.t('torrent.queueMore').replace('{n}', '' + ui.torrentQueue().length) }}
            <button type="button" class="queue-skip" (click)="skipQueue()">{{ i18n.t('torrent.queueSkip') }}</button>
          </p>
        }
        @switch (phase()) {
          @case ('resolving') {
            <div class="state" role="status" aria-live="polite">
              <span class="spinner" aria-hidden="true"></span>
              <div class="state-text">
                <p class="state-title">{{ i18n.t('torrent.resolving') }}</p>
                <p class="state-hint">{{ i18n.t('torrent.resolvingHint') }}</p>
                <p class="state-hint num">{{ i18n.t('torrent.elapsed').replace('{s}', '' + elapsed()) }}</p>
              </div>
            </div>
          }
          @case ('error') {
            <div class="state is-error" role="alert">
              <app-icon name="alert" [size]="20" />
              <div class="state-text">
                <p class="state-title">{{ errorText() }}</p>
                <button appButton size="sm" variant="secondary" type="button" (click)="resolve()">{{ i18n.t('torrent.retry') }}</button>
              </div>
            </div>
          }
          @case ('ready') {
            @if (preview(); as p) {
              <div class="head">
                <span class="head-icon" aria-hidden="true"><app-icon name="magnet" [size]="22" /></span>
                <div class="head-text">
                  <p class="name" dir="auto">{{ p.name }}</p>
                  <p class="sub">
                    {{ i18n.t('torrent.files').replace('{n}', '' + p.files.length) }} · <span class="num">{{ format.bytes(p.totalBytes) }}</span>
                    @if (p.isPrivate) {
                      · {{ i18n.t('torrent.private') }}
                    }
                  </p>
                </div>
              </div>

              @if (p.existing !== null) {
                <p class="notice">
                  <app-icon name="info" [size]="14" />
                  {{ i18n.t('torrent.existing') }}
                  <button type="button" class="link" (click)="showExisting(p.existing)">{{ i18n.t('torrent.showExisting') }}</button>
                </p>
              }

              <div class="list-head">
                <span class="count num">
                  {{ i18n.t('torrent.selectedCount').replace('{n}', '' + chosenCount()).replace('{total}', '' + p.files.length).replace('{size}', format.bytes(chosenBytes())) }}
                </span>
                <span class="list-actions">
                  <button type="button" class="link" (click)="selectAll()">{{ i18n.t('torrent.selectAll') }}</button>
                  <button type="button" class="link" (click)="selectNone()">{{ i18n.t('torrent.selectNone') }}</button>
                </span>
              </div>
              <app-torrent-file-list [files]="p.files" [selectable]="true" [(selected)]="selection" />
              @if (chosenCount() === 0) {
                <p class="help is-error">{{ i18n.t('torrent.noneSelected') }}</p>
              } @else if (lowSpace()) {
                <p class="help is-warning">
                  <app-icon name="alert" [size]="14" />
                  {{ i18n.t('add.noSpace').replace('{free}', format.bytes(freeBytes())) }}
                </p>
              }

              @if (p.webSeeds.length > 0 && p.existing === null) {
                <div class="webseed" role="note">
                  <app-icon name="globe" [size]="16" />
                  <div class="webseed-text">
                    <span>{{ i18n.t('torrent.webSeed') }}</span>
                    <button appButton size="sm" variant="secondary" icon="download" type="button" [busy]="seedBusy()" [disabled]="chosenCount() === 0 || seedBusy()" (click)="downloadFromWebSeed()">{{ i18n.t('torrent.webSeedDownload') }}</button>
                  </div>
                </div>
              }

              <label class="check-row">
                <input type="checkbox" [checked]="sequential()" (change)="sequential.set(!sequential())" />
                <span>
                  <span class="check-title">{{ i18n.t('torrent.sequential') }}</span>
                  <span class="check-hint">{{ i18n.t('torrent.sequentialHint') }}</span>
                </span>
              </label>

              <label class="field">
                <span class="label">{{ i18n.t('add.saveTo') }}</span>
                <span class="input-wrap">
                  <app-icon class="input-icon" name="folder" [size]="16" />
                  <input class="input" type="text" name="dir" dir="ltr" [value]="dir()" (input)="onDir($event)" />
                  <button appButton size="sm" variant="secondary" type="button" class="input-action" (click)="browse()">{{ i18n.t('add.browse') }}</button>
                </span>
              </label>

              <div class="field">
                <span class="label">{{ i18n.t('add.start') }}</span>
                <div class="presets" role="radiogroup" [attr.aria-label]="i18n.t('add.start')">
                  <button type="button" role="radio" class="preset" [class.is-active]="!startPaused()" [attr.aria-checked]="!startPaused()" (click)="startPaused.set(false)">{{ i18n.t('add.startNow') }}</button>
                  <button type="button" role="radio" class="preset" [class.is-active]="startPaused()" [attr.aria-checked]="startPaused()" (click)="startPaused.set(true)">{{ i18n.t('add.startLater') }}</button>
                </div>
              </div>
            }
          }
        }
      </div>

      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="close()">{{ i18n.t('action.cancel') }}</button>
        @if (phase() === 'ready') {
          <button appButton variant="primary" icon="download" type="button" [busy]="submitting()" [disabled]="!canSubmit()" (click)="submit()">
            {{ i18n.t(submitting() ? 'add.submitting' : startPaused() ? 'add.submitLater' : 'add.submit') }}
          </button>
        }
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .webseed {
      display: flex;
      gap: var(--space-3);
      align-items: flex-start;
      padding: var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .webseed app-icon {
      color: var(--color-accent);
      margin-block-start: 2px;
    }

    .webseed-text {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: var(--space-2);
    }

    .queue-note {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .body:focus {
      outline: none;
    }

    .queue-skip {
      border: 0;
      background: transparent;
      color: var(--color-accent);
      font: inherit;
      text-decoration: underline;
      cursor: pointer;
    }

    .body {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
      padding-block: var(--space-2) var(--space-3);
      min-inline-size: 0;
    }

    .state {
      display: flex;
      align-items: flex-start;
      gap: var(--space-4);
      padding: var(--space-5);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
    }

    .state.is-error {
      color: var(--color-danger);
    }

    .state-text {
      display: flex;
      flex-direction: column;
      gap: 6px;
      align-items: flex-start;
    }

    .state-title {
      margin: 0;
      font-weight: var(--weight-medium);
      color: var(--color-text);
    }

    .state.is-error .state-title {
      color: var(--color-danger);
    }

    .state-hint {
      margin: 0;
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .spinner {
      flex: none;
      inline-size: 20px;
      block-size: 20px;
      border-radius: var(--radius-full);
      border: 2px solid var(--color-border-strong);
      border-block-start-color: var(--color-accent);
      animation: spin 800ms linear infinite;
    }

    @keyframes spin {
      to {
        transform: rotate(360deg);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .spinner {
        animation-duration: 2400ms;
      }
    }

    .head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-inline-size: 0;
    }

    .head-icon {
      flex: none;
      display: grid;
      place-items: center;
      inline-size: 44px;
      block-size: 44px;
      border-radius: var(--radius-md);
      background: var(--color-accent-soft, var(--color-surface-2));
      color: var(--color-accent-fg);
    }

    .head-text {
      min-inline-size: 0;
    }

    .name {
      margin: 0;
      font-weight: var(--weight-medium);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .sub {
      margin: 2px 0 0;
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .notice {
      display: flex;
      align-items: center;
      gap: 6px;
      margin: 0;
      color: var(--color-warning);
      font-size: var(--text-sm);
    }

    .list-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .list-actions {
      display: inline-flex;
      gap: var(--space-3);
    }

    .link {
      padding: 0;
      border: 0;
      background: none;
      color: var(--color-accent);
      font: inherit;
      cursor: pointer;
      text-decoration: underline;
    }

    .help {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      margin: 0;
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .help.is-error {
      color: var(--color-danger);
    }

    .help.is-warning {
      color: var(--color-warning);
    }

    .check-row {
      display: flex;
      align-items: flex-start;
      gap: var(--space-3);
      cursor: pointer;
    }

    .check-row input {
      flex: none;
      inline-size: 16px;
      block-size: 16px;
      margin-block-start: 2px;
      accent-color: var(--color-accent);
    }

    .check-title {
      display: block;
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .check-hint {
      display: block;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .field {
      display: flex;
      flex-direction: column;
      gap: 6px;
      min-inline-size: 0;
    }

    .label {
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      color: var(--color-text-secondary);
    }

    .input-wrap {
      position: relative;
      display: flex;
      align-items: center;
    }

    .input-icon {
      position: absolute;
      inset-inline-start: 10px;
      color: var(--color-text-muted);
      pointer-events: none;
    }

    .input-action {
      position: absolute;
      inset-inline-end: 4px;
    }

    .input {
      inline-size: 100%;
      block-size: 36px;
      padding-inline: 34px 96px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .input:focus {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    /* the path is forced LTR while the icon and the button follow the page direction */
    :host-context([dir='rtl']) .input {
      padding-inline: 96px 34px;
    }

    .presets {
      display: inline-flex;
      padding: 3px;
      gap: 2px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      inline-size: fit-content;
    }

    .preset {
      min-inline-size: 40px;
      block-size: 28px;
      padding-inline: 10px;
      border: 0;
      border-radius: var(--radius-xs);
      background: transparent;
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      cursor: pointer;
    }

    .preset:hover {
      color: var(--color-text);
    }

    .preset.is-active {
      background: var(--color-accent);
      color: var(--color-on-accent);
    }
  `,
})
export class TorrentDialogComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  private readonly engine = inject(DownloadEngine);
  private readonly toasts = inject(ToastService);
  protected readonly ui = inject(UiState);
  private readonly settingsService = inject(SettingsService);
  private readonly disk = inject(DiskSpaceService);
  private readonly store = inject(DownloadsStore);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);

  private readonly bodyRef = viewChild<ElementRef<HTMLElement>>('body');

  readonly open = input.required<boolean>();
  /** The magnet link or `.torrent` file to look into. */
  readonly source = input.required<TorrentSource | null>();
  readonly closed = output<void>();
  readonly added = output<number>();

  protected readonly phase = signal<Phase>('resolving');
  protected readonly preview = signal<TorrentPreview | null>(null);
  protected readonly errorText = signal('');
  protected readonly elapsed = signal(0);
  /** The chosen file indices. */
  protected readonly selection = signal<ReadonlySet<number> | null>(null);
  protected readonly sequential = signal(false);
  protected readonly startPaused = signal(false);
  protected readonly dir = signal(this.settingsService.settings().defaultDir);
  protected readonly submitting = signal(false);
  protected readonly freeBytes = signal<number | null>(null);
  protected readonly seedBusy = signal(false);

  private token = 0;
  private diskToken = 0;
  private timer: ReturnType<typeof setInterval> | null = null;

  protected readonly chosenCount = computed(() => {
    const p = this.preview();
    const s = this.selection();
    return p === null ? 0 : s === null ? p.files.length : s.size;
  });
  protected readonly chosenBytes = computed(() => {
    const p = this.preview();
    return p === null ? 0 : selectedBytes(p.files, this.selectionArray());
  });
  protected readonly lowSpace = computed(() => {
    const free = this.freeBytes();
    return free !== null && this.chosenBytes() > free;
  });
  /** A torrent that is already in the list is shown, not added a second time. */
  protected readonly canSubmit = computed(
    () => this.phase() === 'ready' && this.chosenCount() > 0 && !this.submitting() && (this.preview()?.existing ?? null) === null,
  );

  constructor() {
    effect(() => {
      if (this.open() && this.source() !== null) {
        untracked(() => {
          this.dir.set(this.settingsService.settings().defaultDir);
          this.sequential.set(false);
          this.startPaused.set(false);
          this.submitting.set(false);
          this.resolve();
        });
      }
    });
    effect(() => {
      const dir = this.dir().trim();
      const token = ++this.diskToken;
      void this.disk.freeFor(dir).then((free) => {
        if (token === this.diskToken) {
          this.freeBytes.set(free);
        }
      });
    });
    inject(DestroyRef).onDestroy(() => this.stopTimer());
  }

  /** Asks the engine what the torrent holds (again, after an error). */
  protected resolve(): void {
    const source = this.source();
    if (source === null) {
      return;
    }
    const token = ++this.token;
    this.phase.set('resolving');
    this.preview.set(null);
    this.selection.set(null);
    this.elapsed.set(0);
    this.stopTimer();
    this.timer = setInterval(() => this.elapsed.update((n) => n + 1), 1000);
    this.engine.torrentPreview(source, this.dir().trim() || undefined).then(
      (p) => {
        if (token !== this.token) {
          return;
        }
        this.stopTimer();
        this.preview.set(p);
        this.selection.set(new Set(p.files.map((f) => f.index)));
        this.phase.set('ready');
      },
      (error: unknown) => {
        if (token !== this.token) {
          return;
        }
        this.stopTimer();
        this.errorText.set(this.describe(error));
        this.phase.set('error');
      },
    );
  }

  protected selectAll(): void {
    const p = this.preview();
    if (p !== null) {
      this.selection.set(new Set(p.files.map((f) => f.index)));
    }
  }

  protected selectNone(): void {
    this.selection.set(new Set());
  }

  protected onDir(event: Event): void {
    this.dir.set((event.target as HTMLInputElement).value);
  }

  protected browse(): void {
    if (!this.tauriAvailable) {
      this.toasts.show('info', this.i18n.t('add.browseSoon'));
      return;
    }
    this.invoke<string | null>('pick_folder')
      .then((dir) => {
        if (dir !== null && dir.length > 0) {
          this.dir.set(dir);
        }
      })
      .catch((error: unknown) => this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error))));
  }

  protected showExisting(id: number): void {
    this.close();
    this.store.openDetails(id);
  }

  /** Forget the waiting magnet links; focus stays inside the dialog (the button that was pressed disappears with its note), so Escape still closes it. */
  protected skipQueue(): void {
    this.ui.skipQueuedTorrents();
    queueMicrotask(() => this.bodyRef()?.nativeElement.focus());
  }

  /**
   * The torrent names HTTP mirrors (web seeds) and few peers may be around: download the chosen files from the
   * first mirror as ordinary downloads, into the same folder layout. They are not checked against the
   * torrent's piece hashes (a torrent download is); each is a normal download with all of Riptide's HTTP
   * safeguards.
   */
  protected async downloadFromWebSeed(): Promise<void> {
    const p = this.preview();
    if (p === null || this.seedBusy()) {
      return;
    }
    const seed = p.webSeeds[0];
    const chosen = this.selectionArray();
    const files = p.files.filter((f) => chosen === null || chosen.includes(f.index));
    const base = this.dir().trim();
    const sep = base.includes('\\') ? '\\' : '/';
    this.seedBusy.set(true);
    let added = 0;
    try {
      for (const f of files) {
        const url = webSeedUrl(seed, f.path, p.files.length > 1);
        if (url === null) {
          continue;
        }
        const parts = f.path.split('/').filter((x) => x.length > 0);
        const name = parts.pop() ?? p.name;
        const dir = base.length > 0 ? [base, ...parts].join(sep) : undefined;
        await this.engine.add({ url, filename: name, dir, allowDuplicate: true });
        added++;
      }
      this.toasts.show(added > 0 ? 'success' : 'danger', this.i18n.t('torrent.webSeedAdded').replace('{n}', String(added)));
      if (added > 0) {
        this.closed.emit();
      }
    } catch (error: unknown) {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', this.describe(error)));
    } finally {
      this.seedBusy.set(false);
    }
  }

  protected close(): void {
    this.token++;
    this.stopTimer();
    this.closed.emit();
  }

  protected async submit(): Promise<void> {
    const p = this.preview();
    if (p === null || !this.canSubmit()) {
      return;
    }
    this.submitting.set(true);
    try {
      const all = this.chosenCount() === p.files.length;
      const id = await this.engine.addTorrent({
        token: p.token,
        dir: this.dir().trim() || undefined,
        onlyFiles: all ? null : this.selectionArray(),
        sequential: this.sequential(),
        startPaused: this.startPaused(),
      });
      this.toasts.show('success', this.i18n.t(this.startPaused() ? 'toast.addedLater' : 'toast.added').replace('{name}', p.name));
      this.added.emit(id);
      this.closed.emit();
    } catch (error: unknown) {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', this.describe(error)));
    } finally {
      this.submitting.set(false);
    }
  }

  private selectionArray(): readonly number[] | null {
    const s = this.selection();
    return s === null ? null : [...s].sort((a, b) => a - b);
  }

  private stopTimer(): void {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  /** The engine's refusal in the user's language where it is a known case, else its own words. */
  private describe(error: unknown): string {
    const text = String(error instanceof Error ? error.message : error);
    const lower = text.toLowerCase();
    if (lower.includes('timed out')) {
      return this.i18n.t('torrent.errTimeout');
    }
    if (lower.includes('invalid magnet')) {
      return this.i18n.t('torrent.errMagnet');
    }
    if (lower.includes('invalid torrent') || lower.includes('traversal') || lower.includes('separator')) {
      return this.i18n.t('torrent.errTorrent');
    }
    return this.i18n.t('torrent.errGeneric').replace('{error}', text);
  }
}
