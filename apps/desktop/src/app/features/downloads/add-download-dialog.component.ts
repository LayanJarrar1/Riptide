import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal } from '@angular/core';

import { DiskSpaceService } from '../../core/disk-space.service';
import { FORMAT_PIPES } from '../../core/format.pipes';
import { FormatService } from '../../core/format.service';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS, CategoryKey, Download, DownloadRequest, isHttpUrl, isMediaUrl, MediaChoice, MediaProbe, ProbeResult } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { CATEGORY_ICONS } from './download-row.component';
import { DuplicateChoice, DuplicateDialogComponent } from './duplicate-dialog.component';

type ProbeState = 'idle' | 'probing' | 'ok' | 'invalid';

const CONNECTION_PRESETS = [1, 4, 8, 16, 32] as const;

/**
 * Add a download. Paste a link and the preview fills in (name, size, resumable, category) while
 * you choose where it goes. The primary button shows a spinner and "Adding…" while the engine
 * accepts the request, so a slow engine never looks like a dead button.
 */
@Component({
  selector: 'app-add-download-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES, ButtonComponent, DialogComponent, DuplicateDialogComponent, IconComponent],
  template: `
    <app-dialog [open]="open()" [title]="i18n.t('add.title')" [wide]="true" (closed)="close()">
      <form class="form" (submit)="submit($event)">
        <label class="field">
          <span class="label">{{ i18n.t('add.url') }}</span>
          <span class="input-wrap" [class.is-invalid]="probe() === 'invalid'">
            <app-icon class="input-icon" name="link" [size]="16" />
            <input
              class="input"
              type="url"
              name="url"
              dir="ltr"
              autocomplete="off"
              spellcheck="false"
              [placeholder]="i18n.t('add.urlPlaceholder')"
              [value]="url()"
              (input)="onUrl($event)"
              (paste)="onPaste()"

            />
            @if (probe() === 'probing') {
              <span class="probe-spinner" aria-hidden="true"></span>
            }
          </span>
          @if (probe() === 'invalid') {
            <span class="help is-error">{{ i18n.t('add.urlInvalid') }}</span>
          } @else if (probe() === 'probing') {
            <span class="help">{{ i18n.t('add.probing') }}</span>
          } @else if (lowSpace()) {
            <span class="help is-warning">
              <app-icon name="alert" [size]="14" />
              {{ i18n.t('add.noSpace').replace('{free}', format.bytes(freeBytes())) }}
            </span>
          } @else if (media(); as m) {
            <span class="help preview">
              <app-icon name="play" [size]="14" class="ok" />
              {{ i18n.t('add.media.detected') }}{{ m.uploader ? ' · ' + m.uploader : '' }}{{ m.duration ? ' · ' + format.duration(m.duration) : '' }}
            </span>
          } @else if (mediaError(); as err) {
            <span class="help is-error">{{ err }}</span>
          } @else if (result(); as r) {
            <span class="help preview">
              <app-icon [name]="r.supportsRanges ? 'bolt' : 'info'" [size]="14" [class.ok]="r.supportsRanges" />
              {{ r.size | bytes }} · {{ i18n.t(r.supportsRanges ? 'add.resumable' : 'add.notResumable') }}
            </span>
          } @else {
            <span class="help">{{ i18n.t('add.urlHint') }}</span>
          }
        </label>

        <div class="row">
          <label class="field grow">
            <span class="label">{{ i18n.t('add.filename') }}</span>
            <input class="input" type="text" name="filename" dir="auto" [value]="filename()" (input)="filename.set(value($event))" [placeholder]="i18n.t('add.filenameHint')" />
          </label>
          <label class="field category">
            <span class="label">{{ i18n.t('add.category') }}</span>
            <span class="select-wrap">
              <app-icon class="input-icon" [name]="categoryIcon()" [size]="16" />
              <select class="input select" name="category" [value]="category()" (change)="onCategory($event)">
                @for (c of categories; track c) {
                  <option [value]="c" [selected]="c === category()">{{ i18n.t('category.' + c) }}</option>
                }
              </select>
            </span>
          </label>
        </div>

        <label class="field">
          <span class="label">{{ i18n.t('add.saveTo') }}</span>
          <span class="input-wrap">
            <app-icon class="input-icon" name="folder" [size]="16" />
            <input class="input" type="text" name="dir" dir="ltr" [value]="dir()" (input)="dir.set(value($event))" />
            <button appButton size="sm" variant="secondary" type="button" class="input-action" (click)="browse()">{{ i18n.t('add.browse') }}</button>
          </span>
        </label>

        @if (media(); as m) {
          <div class="field">
            <span class="label">{{ i18n.t('add.media.quality') }}</span>
            <div class="qualities" role="radiogroup" [attr.aria-label]="i18n.t('add.media.quality')">
              @for (c of m.choices; track c.key) {
                <button type="button" role="radio" class="quality" [class.is-active]="c.key === mediaChoice()" [attr.aria-checked]="c.key === mediaChoice()" (click)="pickQuality(c)">
                  <span class="quality-name">{{ choiceLabel(c) }}</span>
                  <span class="quality-size num">{{ c.approxSize === null ? '' : '≈ ' + format.bytes(c.approxSize) }}</span>
                </button>
              }
            </div>
          </div>
        }

        <div class="row">
          <div class="field grow" [hidden]="media() !== null">
            <span class="label">{{ i18n.t('add.connections') }}</span>
            <div class="presets" role="radiogroup" [attr.aria-label]="i18n.t('add.connections')">
              @for (n of presets; track n) {
                <button type="button" role="radio" class="preset num" [class.is-active]="n === connections()" [attr.aria-checked]="n === connections()" (click)="connections.set(n)">{{ n }}</button>
              }
            </div>
          </div>
          <div class="field">
            <span class="label">{{ i18n.t('add.start') }}</span>
            <div class="presets" role="radiogroup" [attr.aria-label]="i18n.t('add.start')">
              <button type="button" role="radio" class="preset" [class.is-active]="!startPaused()" [attr.aria-checked]="!startPaused()" (click)="startPaused.set(false)">{{ i18n.t('add.startNow') }}</button>
              <button type="button" role="radio" class="preset" [class.is-active]="startPaused()" [attr.aria-checked]="startPaused()" (click)="startPaused.set(true)">{{ i18n.t('add.startLater') }}</button>
            </div>
          </div>
        </div>

        <details class="advanced" [open]="advancedOpen()" (toggle)="onToggle($event)">
          <summary class="advanced-summary">
            <app-icon name="chevronEnd" [size]="14" class="chev" />
            {{ i18n.t('add.advanced') }}
          </summary>
          <div class="advanced-body">
            <label class="field">
              <span class="label">{{ i18n.t('add.checksum') }}</span>
              <input class="input mono" type="text" name="checksum" dir="ltr" spellcheck="false" [value]="checksum()" (input)="checksum.set(value($event))" [placeholder]="i18n.t('add.checksumHint')" />
              @if (checksumError()) {
                <span class="help is-error">{{ i18n.t('add.checksumInvalid') }}</span>
              }
            </label>
            <div class="row">
              <label class="field grow">
                <span class="label">{{ i18n.t('add.username') }}</span>
                <input class="input" type="text" name="username" autocomplete="off" [value]="username()" (input)="username.set(value($event))" />
              </label>
              <label class="field grow">
                <span class="label">{{ i18n.t('add.password') }}</span>
                <input class="input" type="password" name="password" autocomplete="off" [value]="password()" (input)="password.set(value($event))" />
              </label>
            </div>
            <label class="field">
              <span class="label">{{ i18n.t('add.headers') }}</span>
              <textarea class="input textarea mono" name="headers" dir="ltr" rows="3" spellcheck="false" [value]="headers()" (input)="headers.set(value($event))" [placeholder]="i18n.t('add.headersHint')"></textarea>
            </label>
          </div>
        </details>
      </form>

      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="close()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" icon="download" type="button" [busy]="submitting()" [disabled]="!canSubmit()" (click)="submit()">
          {{ i18n.t(submitting() ? 'add.submitting' : startPaused() ? 'add.submitLater' : 'add.submit') }}
        </button>
      </ng-container>
    </app-dialog>

    <app-duplicate-dialog [existing]="duplicate()" (chosen)="onDuplicate($event)" />
  `,
  styles: `
    .form {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
      padding-block: var(--space-2) var(--space-3);
    }

    .row {
      display: flex;
      gap: var(--space-4);
    }

    .grow {
      flex: 1;
    }

    .category {
      inline-size: 200px;
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

    .input-wrap,
    .select-wrap {
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

    .input-wrap .input,
    .select-wrap .input {
      padding-inline-start: 34px;
    }

    .input-action {
      position: absolute;
      inset-inline-end: 4px;
    }

    .input-wrap:has(.input-action) .input {
      padding-inline-end: 96px;
    }

    .probe-spinner {
      position: absolute;
      inset-inline-end: 10px;
      inline-size: 14px;
      block-size: 14px;
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

    .input {
      inline-size: 100%;
      block-size: 36px;
      padding-inline: 12px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
      transition:
        border-color var(--motion-fast) var(--ease-standard),
        box-shadow var(--motion-fast) var(--ease-standard);
    }

    .input::placeholder {
      color: var(--color-text-muted);
    }

    .input:focus {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .is-invalid .input {
      border-color: var(--color-danger);
    }

    .select {
      appearance: none;
      cursor: pointer;
    }

    .textarea {
      block-size: auto;
      padding-block: 8px;
      resize: vertical;
      line-height: 1.4;
    }

    .mono {
      font-family: var(--font-mono);
    }

    .help {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      min-block-size: 18px;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .help.is-error {
      color: var(--color-danger);
    }

    /* The probe's answer (and an error) reveals itself rather than popping into place. */
    .help.is-error,
    .help.preview {
      animation: help-in var(--dur-2) var(--ease-emphasized-out) both;
    }

    @keyframes help-in {
      from {
        opacity: 0;
        transform: translateY(-3px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .help.is-warning {
      color: var(--color-warning);
    }

    .preview {
      color: var(--color-text-secondary);
    }

    @media (prefers-reduced-motion: reduce) {
      .help.is-error,
      .help.preview {
        animation-duration: var(--dur-fade);
        transform: none;
      }
    }

    .preview .ok {
      color: var(--color-accent-fg);
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
      transition:
        background-color var(--motion-fast) var(--ease-standard),
        color var(--motion-fast) var(--ease-standard);
    }

    .preset:hover {
      color: var(--color-text);
    }

    .preset.is-active {
      background: var(--color-accent);
      color: var(--color-on-accent);
    }

    .qualities {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
      gap: var(--space-2);
    }

    .quality {
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: var(--space-2);
      block-size: 36px;
      padding-inline: 12px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
      cursor: pointer;
      transition:
        border-color var(--motion-fast) var(--ease-standard),
        background-color var(--motion-fast) var(--ease-standard);
    }

    .quality:hover {
      border-color: var(--color-accent);
    }

    .quality.is-active {
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .quality-size {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .advanced {
      border-block-start: 1px solid var(--color-border);
      padding-block-start: var(--space-3);
    }

    .advanced-summary {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      list-style: none;
      cursor: pointer;
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      user-select: none;
    }

    .advanced-summary::-webkit-details-marker {
      display: none;
    }

    .chev {
      transition: transform var(--motion-fast) var(--ease-standard);
    }

    .advanced[open] .chev {
      transform: rotate(calc(var(--dir-sign) * 90deg));
    }

    .advanced-body {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
      padding-block-start: var(--space-4);
    }
  `,
})
export class AddDownloadDialogComponent {
  protected readonly i18n = inject(I18nService);
  private readonly engine = inject(DownloadEngine);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly toasts = inject(ToastService);
  private readonly settingsService = inject(SettingsService);
  private readonly disk = inject(DiskSpaceService);
  private readonly store = inject(DownloadsStore);
  protected readonly format = inject(FormatService);

  readonly open = input.required<boolean>();
  /** Optional link to pre-fill (clipboard, deep link). */
  readonly initialUrl = input<string | null>(null);
  readonly closed = output<void>();
  readonly added = output<number>();

  protected readonly categories = CATEGORY_KEYS;
  protected readonly presets = CONNECTION_PRESETS;

  protected readonly url = signal('');
  protected readonly probe = signal<ProbeState>('idle');
  protected readonly result = signal<ProbeResult | null>(null);
  protected readonly filename = signal('');
  protected readonly category = signal<CategoryKey>('other');
  protected readonly dir = signal(this.settingsService.settings().defaultDir);
  protected readonly connections = signal<number>(this.settingsService.settings().connections);
  protected readonly startPaused = signal(false);
  protected readonly advancedOpen = signal(false);
  protected readonly checksum = signal('');
  protected readonly username = signal('');
  protected readonly password = signal('');
  protected readonly headers = signal('');
  /** `true` while the engine is accepting the request (guards a double submit). */
  protected readonly submitting = signal(false);
  /** Free bytes on the chosen folder's volume; `null` while unknown. */
  protected readonly freeBytes = signal<number | null>(null);
  /** What yt-dlp found when the link is a video page; the quality picker shows while set. */
  protected readonly media = signal<MediaProbe | null>(null);
  /** The picked quality key (`best`, `h720`, `audio-mp3`, ...). */
  protected readonly mediaChoice = signal('best');
  /** Why a video page cannot be downloaded (DRM, live stream), shown under the link. */
  protected readonly mediaError = signal<string | null>(null);
  /** A record the engine says this request would duplicate; the prompt shows while set. */
  protected readonly duplicate = signal<Download | null>(null);

  private probeToken = 0;
  private probeTimer: ReturnType<typeof setTimeout> | null = null;
  private diskToken = 0;
  /** The request waiting for the duplicate prompt's answer. */
  private pending: DownloadRequest | null = null;

  protected readonly categoryIcon = computed(() => CATEGORY_ICONS[this.category()]);
  protected readonly checksumError = computed(() => {
    const hex = this.checksum().trim();
    return hex.length > 0 && ![32, 40, 64].includes(hex.length);
  });
  /** The probe knows the size and the folder's volume cannot hold it. */
  protected readonly lowSpace = computed(() => {
    const size = this.result()?.size ?? null;
    const free = this.freeBytes();
    return size !== null && free !== null && size > free;
  });
  protected readonly canSubmit = computed(() => isHttpUrl(this.url()) && this.probe() !== 'probing' && !this.checksumError() && !this.submitting());

  constructor() {
    effect(() => {
      if (this.open()) {
        this.reset();
        const initial = this.initialUrl();
        if (initial !== null) {
          this.setUrl(initial);
        }
      }
    });
    // free space follows the folder the user picks (or the category picks for them)
    effect(() => {
      const dir = this.dir().trim();
      const token = ++this.diskToken;
      void this.disk.freeFor(dir).then((free) => {
        if (token === this.diskToken) {
          this.freeBytes.set(free);
        }
      });
    });
  }

  /** "Best quality", "720p", "Audio only (MP3)". */
  protected choiceLabel(c: MediaChoice): string {
    if (c.kind === 'best') {
      return this.i18n.t('add.media.best');
    }
    if (c.kind === 'audio') {
      return this.i18n.t('add.media.audio').replace('{ext}', c.ext.toUpperCase());
    }
    return `${c.height ?? '?'}p`;
  }

  protected pickQuality(c: MediaChoice): void {
    this.mediaChoice.set(c.key);
    const category: CategoryKey = c.kind === 'audio' ? 'music' : 'video';
    this.category.set(category);
    this.dir.set(this.settingsService.dirFor(category));
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  }

  protected onUrl(event: Event): void {
    this.setUrl(this.value(event));
  }

  protected onPaste(): void {
    // the input event fires right after and carries the pasted text; nothing else to do
  }

  protected onCategory(event: Event): void {
    const key = (event.target as HTMLSelectElement).value as CategoryKey;
    this.category.set(key);
    this.dir.set(this.settingsService.dirFor(key));
  }

  protected onToggle(event: Event): void {
    this.advancedOpen.set((event.target as HTMLDetailsElement).open);
  }

  /** The native folder picker (Rust side) inside Tauri; a note in a plain browser. */
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

  protected close(): void {
    this.closed.emit();
  }

  protected async submit(event?: Event): Promise<void> {
    event?.preventDefault();
    if (!this.canSubmit()) {
      return;
    }
    const hex = this.checksum().trim().toLowerCase();
    const algo = hex.length === 32 ? 'md5' : hex.length === 40 ? 'sha1' : 'sha256';
    const isMedia = this.media() !== null;
    const request: DownloadRequest = {
      url: this.url().trim(),
      filename: this.filename().trim() || undefined,
      dir: this.dir().trim() || undefined,
      category: this.category(),
      connections: isMedia ? 1 : this.connections(),
      startPaused: this.startPaused(),
      checksum: !isMedia && hex.length > 0 ? { algo, hex } : undefined,
      kind: isMedia ? 'media' : 'http',
      mediaFormat: isMedia ? this.mediaChoice() : undefined,
    };
    this.submitting.set(true);
    try {
      // the same link, or the same name in the same folder, already in the list: ask first
      const existing = await this.engine.findDuplicate(request.url, request.filename ?? this.result()?.filename, request.dir);
      if (existing !== undefined) {
        this.pending = request;
        this.duplicate.set(existing);
        return;
      }
    } catch (error: unknown) {
      console.warn('[add] find_duplicate failed', error);
    } finally {
      this.submitting.set(false);
    }
    await this.send(request);
  }

  /** The duplicate prompt's answer: show the existing row, add anyway, or back to the form. */
  protected onDuplicate(choice: DuplicateChoice): void {
    const existing = this.duplicate();
    const request = this.pending;
    this.duplicate.set(null);
    this.pending = null;
    if (choice === 'add' && request !== null) {
      void this.send(request);
    } else if (choice === 'show' && existing !== null) {
      this.closed.emit();
      this.store.openDetails(existing.id);
    }
  }

  private async send(request: DownloadRequest): Promise<void> {
    this.submitting.set(true);
    try {
      const id = await this.engine.add(request, request.kind === 'media' ? undefined : (this.result() ?? undefined));
      this.toasts.show('success', this.i18n.t(this.startPaused() ? 'toast.addedLater' : 'toast.added').replace('{name}', request.filename ?? this.result()?.filename ?? request.url));
      this.added.emit(id);
      this.closed.emit();
    } catch (error: unknown) {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
    } finally {
      this.submitting.set(false);
    }
  }

  /** Asks yt-dlp about a video page; `quiet` keeps a failure silent (the page was only HTML). */
  private probeMedia(url: string, token: number, quiet: boolean): void {
    void this.engine
      .probeMedia(url)
      .then((m) => {
        if (token !== this.probeToken) {
          return;
        }
        this.media.set(m);
        this.mediaError.set(null);
        this.probe.set('ok');
        if (this.filename().trim().length === 0) {
          this.filename.set(m.title);
        }
        const first = m.choices[0];
        if (first !== undefined) {
          this.pickQuality(first);
        }
      })
      .catch((error: unknown) => {
        if (token !== this.probeToken) {
          return;
        }
        this.probe.set('ok');
        if (!quiet) {
          this.mediaError.set(String(error instanceof Error ? error.message : error));
        }
      });
  }

  private setUrl(raw: string): void {
    this.url.set(raw);
    this.media.set(null);
    this.mediaError.set(null);
    if (this.probeTimer !== null) {
      clearTimeout(this.probeTimer);
      this.probeTimer = null;
    }
    const trimmed = raw.trim();
    if (trimmed.length === 0) {
      this.probe.set('idle');
      this.result.set(null);
      return;
    }
    if (!isHttpUrl(trimmed)) {
      this.probe.set('invalid');
      this.result.set(null);
      return;
    }
    this.probe.set('probing');
    const token = ++this.probeToken;
    if (isMediaUrl(trimmed)) {
      // a video page: there is no file at this address, ask yt-dlp for the qualities
      this.result.set(null);
      this.probeTimer = setTimeout(() => this.probeMedia(trimmed, token, false), 250);
      return;
    }
    this.probeTimer = setTimeout(() => {
      void this.engine.probe(trimmed).then((r) => {
        if (token !== this.probeToken) {
          return;
        }
        if (r.contentType?.startsWith('text/html') === true) {
          // a web page, not a file: it may still be a video page yt-dlp knows
          this.probeMedia(trimmed, token, true);
        }
        this.result.set(r);
        this.probe.set('ok');
        if (this.filename().trim().length === 0) {
          this.filename.set(r.filename);
        }
        this.category.set(r.category);
        this.dir.set(this.settingsService.dirFor(r.category));
      });
    }, 250);
  }

  private reset(): void {
    this.probeToken++;
    this.url.set('');
    this.probe.set('idle');
    this.result.set(null);
    this.media.set(null);
    this.mediaError.set(null);
    this.mediaChoice.set('best');
    this.filename.set('');
    this.category.set('other');
    this.dir.set(this.settingsService.settings().defaultDir);
    this.connections.set(this.settingsService.settings().connections);
    this.startPaused.set(false);
    this.checksum.set('');
    this.username.set('');
    this.password.set('');
    this.headers.set('');
    this.submitting.set(false);
    this.duplicate.set(null);
    this.pending = null;
  }
}
