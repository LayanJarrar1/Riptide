import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal } from '@angular/core';

import { DiskSpaceService } from '../../core/disk-space.service';
import { FORMAT_PIPES } from '../../core/format.pipes';
import { FormatService } from '../../core/format.service';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS, CategoryKey, Download, DownloadRequest, isHttpUrl, isMediaUrl, MediaChoice, MediaProbe, ProbeResult } from '../../model/download';
import { parseMediaError } from '../../model/media-error';
import { isMagnetLink } from '../../model/torrent';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { UiState } from '../shell/ui-state.service';
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
          } @else if (isMagnet()) {
            <span class="help preview">
              <app-icon name="magnet" [size]="14" class="ok" />
              {{ i18n.t('torrent.detected') }}
            </span>
          } @else if (lowSpace()) {
            <span class="help is-warning">
              <app-icon name="alert" [size]="14" />
              {{ i18n.t('add.noSpace').replace('{free}', format.bytes(freeBytes())) }}
            </span>
          } @else if (mediaError(); as e) {
            <span class="help is-error">
              <app-icon name="alert" [size]="14" />
              {{ i18n.t(e) }}{{ result() ? ' ' + i18n.t('add.mediaPlainFallback') : '' }}
            </span>
            <span class="help media-actions">
              <button type="button" class="link" (click)="retryProbe()">{{ i18n.t('add.mediaTryAgain') }}</button>
              @if (mediaDetails(); as details) {
                <button type="button" class="link" [attr.aria-expanded]="showDetails()" (click)="showDetails.set(!showDetails())">
                  {{ i18n.t('add.mediaDetails') }}
                </button>
              }
            </span>
            @if (showDetails() && mediaDetails(); as details) {
              <code class="media-details" dir="ltr">{{ details }}</code>
            }
          } @else if (media()) {
            <span class="help preview">
              <app-icon name="bolt" [size]="14" class="ok" />
              {{ i18n.t('add.mediaReady') }}
            </span>
          } @else if (result(); as r) {
            <span class="help preview">
              <app-icon [name]="r.supportsRanges ? 'bolt' : 'info'" [size]="14" [class.ok]="r.supportsRanges" />
              {{ r.size | bytes }} · {{ i18n.t(r.supportsRanges ? 'add.resumable' : 'add.notResumable') }}
            </span>
          } @else {
            <span class="help">{{ i18n.t('add.urlHint') }}</span>
          }
        </label>

        @if (media(); as m) {
          <section class="media" aria-live="polite">
            <div class="media-head">
              @if (m.thumbnail) {
                <img class="media-thumb" [src]="m.thumbnail" alt="" loading="lazy" referrerpolicy="no-referrer" (error)="hideThumb($event)" />
              } @else {
                <span class="media-thumb media-thumb-empty" aria-hidden="true"><app-icon name="play" [size]="22" /></span>
              }
              <span class="media-text">
                <span class="media-title" dir="auto">{{ m.title }}</span>
                <span class="media-sub" dir="auto">
                  @if (m.uploader) {
                    <span>{{ m.uploader }}</span>
                  }
                  @if (m.uploader && m.duration !== null) {
                    <span aria-hidden="true">·</span>
                  }
                  @if (m.duration !== null) {
                    <span class="num">{{ m.duration | duration }}</span>
                  }
                </span>
              </span>
            </div>
            <div class="field">
              <span class="label">{{ i18n.t('add.quality') }}</span>
              <div class="qualities" role="radiogroup" [attr.aria-label]="i18n.t('add.quality')">
                @for (c of m.choices; track c.key) {
                  <button type="button" role="radio" class="quality" [class.is-active]="c.key === choiceKey()" [attr.aria-checked]="c.key === choiceKey()" (click)="chooseQuality(c)">
                    <span class="quality-name">{{ choiceLabel(c) }}</span>
                    @if (c.approxSize !== null) {
                      <span class="quality-size num">~{{ c.approxSize | bytes }}</span>
                    }
                  </button>
                }
              </div>
            </div>
          </section>
        }

        @if (!isMagnet()) {
        <div class="row">
          <label class="field grow">
            <span class="label">{{ i18n.t('add.filename') }}</span>
            <input class="input" type="text" name="filename" dir="auto" [value]="filename()" (input)="onFilename($event)" [placeholder]="i18n.t('add.filenameHint')" />
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

        <div class="row">
          @if (!media()) {
          <div class="field grow">
            <span class="label">{{ i18n.t('add.connections') }}</span>
            <div class="presets" role="radiogroup" [attr.aria-label]="i18n.t('add.connections')">
              @for (n of presets; track n) {
                <button type="button" role="radio" class="preset num" [class.is-active]="n === connections()" [attr.aria-checked]="n === connections()" (click)="connections.set(n)">{{ n }}</button>
              }
            </div>
          </div>
          }
          <div class="field" [class.grow]="media()">
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
            @if (!media()) {
            <label class="field">
              <span class="label">{{ i18n.t('add.checksum') }}</span>
              <input class="input mono" type="text" name="checksum" dir="ltr" spellcheck="false" [value]="checksum()" (input)="checksum.set(value($event))" [placeholder]="i18n.t('add.checksumHint')" />
              @if (checksumError()) {
                <span class="help is-error">{{ i18n.t('add.checksumInvalid') }}</span>
              }
            </label>
            }
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
        }
      </form>

      <ng-container dialogActions>
        <button appButton variant="ghost" icon="magnet" type="button" class="torrent-file" (click)="chooseTorrentFile()">{{ i18n.t('torrent.choose') }}</button>
        <button appButton variant="ghost" type="button" (click)="close()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" [icon]="isMagnet() ? 'magnet' : 'download'" type="button" [busy]="submitting()" [disabled]="!canSubmit()" (click)="submit()">
          {{ i18n.t(submitting() ? 'add.submitting' : isMagnet() ? 'torrent.chooseFiles' : startPaused() ? 'add.submitLater' : 'add.submit') }}
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

    /* the torrent file picker sits at the start of the footer, away from Cancel / Download */
    .torrent-file {
      margin-inline-end: auto;
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

    /*
     * The path field is forced LTR (paths and URLs read left to right) while its icon and
     * button follow the page direction, so in RTL they sit on the opposite physical sides.
     * Without this the Browse button covers the start of the path.
     */
    :host-context([dir='rtl']) .input-wrap .input[dir='ltr'] {
      padding-inline-start: 12px;
      padding-inline-end: 34px;
    }

    :host-context([dir='rtl']) .input-wrap:has(.input-action) .input[dir='ltr'] {
      padding-inline-start: 96px;
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

    .media-actions {
      gap: 12px;
    }

    .media-actions .link {
      padding: 0;
      border: 0;
      background: none;
      color: var(--color-accent);
      font: inherit;
      cursor: pointer;
      text-decoration: underline;
    }

    .media-details {
      display: block;
      max-inline-size: 100%;
      padding: 6px 8px;
      border-radius: var(--radius-sm, 6px);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      overflow-wrap: anywhere;
      user-select: text;
    }

    .preview {
      color: var(--color-text-secondary);
    }

    @media (prefers-reduced-motion: reduce) {
      .help.is-error,
      .media,
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

    .media {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      padding: var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
      animation: help-in var(--dur-2) var(--ease-emphasized-out) both;
    }

    .media-head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-inline-size: 0;
    }

    .media-thumb {
      flex: none;
      inline-size: 96px;
      block-size: 54px;
      border-radius: var(--radius-sm);
      object-fit: cover;
    }

    .media-thumb-empty {
      display: grid;
      place-items: center;
      color: var(--color-text-muted);
      border: 1px solid var(--color-border);
    }

    .media-text {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-inline-size: 0;
    }

    .media-title {
      font-weight: var(--weight-medium);
      overflow: hidden;
      display: -webkit-box;
      -webkit-line-clamp: 2;
      -webkit-box-orient: vertical;
    }

    .media-sub {
      display: inline-flex;
      gap: 6px;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .qualities {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
    }

    .quality {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      block-size: 32px;
      padding-inline: 12px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      cursor: pointer;
      transition:
        background-color var(--motion-fast) var(--ease-standard),
        color var(--motion-fast) var(--ease-standard),
        border-color var(--motion-fast) var(--ease-standard);
    }

    .quality:hover {
      color: var(--color-text);
    }

    .quality.is-active {
      border-color: var(--color-accent);
      background: var(--color-accent);
      color: var(--color-on-accent);
    }

    .quality-size {
      font-size: var(--text-xs);
      opacity: 0.8;
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
  private readonly ui = inject(UiState);
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
  /** A record the engine says this request would duplicate; the prompt shows while set. */
  protected readonly duplicate = signal<Download | null>(null);

  /** The video page's title, uploader and quality menu; `null` for a plain link. */
  protected readonly media = signal<MediaProbe | null>(null);
  /** The picked quality's key (a [`MediaChoice`] key). */
  protected readonly choiceKey = signal('best');
  /** i18n key of why the page is not a downloadable video; `null` when there is no problem. */
  protected readonly mediaError = signal<string | null>(null);
  /** yt-dlp's own error line (plain text, at most 300 characters) behind the "Details" link. */
  protected readonly mediaDetails = signal<string | null>(null);
  protected readonly showDetails = signal(false);

  private probeToken = 0;
  /** `true` once the user typed a name, so a quality change no longer rewrites it. */
  private filenameEdited = false;
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
  /** A pasted `magnet:` link: the torrent dialog takes over for the file choice. */
  protected readonly isMagnet = computed(() => isMagnetLink(this.url()));
  protected readonly canSubmit = computed(() => (this.isMagnet() || (isHttpUrl(this.url()) && this.probe() !== 'probing' && !this.checksumError())) && !this.submitting());

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

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  }

  protected onUrl(event: Event): void {
    this.setUrl(this.value(event));
  }

  protected onPaste(): void {
    // the input event fires right after and carries the pasted text; nothing else to do
  }

  protected onFilename(event: Event): void {
    this.filename.set(this.value(event));
    this.filenameEdited = true;
  }

  protected hideThumb(event: Event): void {
    (event.target as HTMLElement).style.display = 'none';
  }

  /** `Best`, `1080p`, `Audio · M4A`. */
  protected choiceLabel(c: MediaChoice): string {
    if (c.kind === 'best') {
      return this.i18n.t('add.qualityBest');
    }
    if (c.kind === 'audio') {
      return this.i18n.t('add.qualityAudio').replace('{ext}', c.ext.toUpperCase());
    }
    return `${c.height ?? ''}p`;
  }

  protected chooseQuality(c: MediaChoice): void {
    this.choiceKey.set(c.key);
    this.applyChoice(c);
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

  /** "Choose .torrent file": the native dialog, then the file-choice dialog takes over. */
  protected chooseTorrentFile(): void {
    this.engine
      .pickTorrentFile()
      .then((path) => {
        if (path !== null) {
          this.ui.openTorrent({ kind: 'file', path });
          this.closed.emit();
        } else if (!this.tauriAvailable) {
          this.toasts.show('info', this.i18n.t('torrent.desktopOnly'));
        }
      })
      .catch((error: unknown) => this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error))));
  }

  protected async submit(event?: Event): Promise<void> {
    event?.preventDefault();
    if (!this.canSubmit()) {
      return;
    }
    if (this.isMagnet()) {
      this.ui.openTorrent({ kind: 'magnet', uri: this.url().trim() });
      this.closed.emit();
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
      connections: isMedia ? undefined : this.connections(),
      startPaused: this.startPaused(),
      checksum: !isMedia && hex.length > 0 ? { algo, hex } : undefined,
      kind: isMedia ? 'media' : undefined,
      mediaFormat: isMedia ? this.choiceKey() : undefined,
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
      const id = await this.engine.add(request, this.media() === null ? (this.result() ?? undefined) : undefined);
      this.toasts.show('success', this.i18n.t(this.startPaused() ? 'toast.addedLater' : 'toast.added').replace('{name}', request.filename ?? this.media()?.title ?? this.result()?.filename ?? request.url));
      this.added.emit(id);
      this.closed.emit();
    } catch (error: unknown) {
      this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
    } finally {
      this.submitting.set(false);
    }
  }

  private setUrl(raw: string): void {
    this.url.set(raw);
    if (this.probeTimer !== null) {
      clearTimeout(this.probeTimer);
      this.probeTimer = null;
    }
    const trimmed = raw.trim();
    const token = ++this.probeToken;
    this.clearMedia();
    if (trimmed.length === 0) {
      this.probe.set('idle');
      this.result.set(null);
      return;
    }
    if (isMagnetLink(trimmed)) {
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
    this.probeTimer = setTimeout(() => void this.runProbe(trimmed, token), 250);
  }

  /**
   * The plain probe, plus the video probe right away for known video sites and for any page the
   * plain probe finds to be HTML. A stale answer (the URL changed meanwhile) is dropped.
   */
  private async runProbe(url: string, token: number): Promise<void> {
    const plainPromise = this.engine.probe(url).then(
      (r): ProbeResult | null => r,
      () => null,
    );
    const knownVideo = isMediaUrl(url);
    const earlyMedia = knownVideo ? this.fetchMedia(url) : null;
    const plain = await plainPromise;
    if (token !== this.probeToken) {
      return;
    }
    const html = plain?.contentType?.toLowerCase().includes('html') === true;
    const outcome = earlyMedia !== null ? await earlyMedia : html ? await this.fetchMedia(url) : null;
    if (token !== this.probeToken) {
      return;
    }
    this.result.set(plain);
    this.probe.set('ok');
    if (outcome !== null && 'probe' in outcome) {
      this.showMedia(outcome.probe);
      return;
    }
    // an HTML page that is not a video is no error worth showing; a known video site is
    const shown = outcome !== null && 'error' in outcome && (knownVideo || plain === null) ? outcome : null;
    this.mediaError.set(shown === null ? null : shown.error);
    this.mediaDetails.set(shown === null || shown.details.length === 0 ? null : shown.details);
    if (plain !== null) {
      if (this.filename().trim().length === 0) {
        this.filename.set(plain.filename);
      }
      this.category.set(plain.category);
      this.dir.set(this.settingsService.dirFor(plain.category));
    }
  }

  private async fetchMedia(url: string): Promise<{ probe: MediaProbe } | { error: string; details: string }> {
    try {
      return { probe: await this.engine.probeMedia(url) };
    } catch (error: unknown) {
      return { error: mediaErrorKey(error), details: parseMediaError(error).details };
    }
  }

  /** "Try again" after a video error: asks the engine about the same address once more. */
  protected retryProbe(): void {
    this.setUrl(this.url());
  }

  private showMedia(m: MediaProbe): void {
    this.mediaError.set(null);
    this.media.set(m);
    const first = m.choices[0];
    if (first === undefined) {
      return;
    }
    this.choiceKey.set(first.key);
    this.applyChoice(first);
  }

  /** The chosen quality decides the extension and the category (unless the user typed a name). */
  private applyChoice(c: MediaChoice): void {
    const m = this.media();
    if (m === null) {
      return;
    }
    const category: CategoryKey = c.kind === 'audio' ? 'music' : 'video';
    this.category.set(category);
    this.dir.set(this.settingsService.dirFor(category));
    if (!this.filenameEdited) {
      this.filename.set(`${safeTitle(m.title)}.${c.ext}`);
    }
  }

  private clearMedia(): void {
    this.media.set(null);
    this.mediaError.set(null);
    this.mediaDetails.set(null);
    this.showDetails.set(false);
    this.choiceKey.set('best');
    this.filenameEdited = false;
  }

  private reset(): void {
    this.probeToken++;
    this.clearMedia();
    this.url.set('');
    this.probe.set('idle');
    this.result.set(null);
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

/** A title as a file name: characters Windows forbids become spaces. */
function safeTitle(title: string): string {
  const cleaned = Array.from(title, (ch) => (ch.charCodeAt(0) < 32 ? ' ' : ch))
    .join('')
    .replace(/[\\/:*?"<>|]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  return cleaned.length > 0 ? cleaned : 'video';
}

/** Maps the engine's refusal to a translated, friendly message key. */
export function mediaErrorKey(error: unknown): string {
  const text = String(error instanceof Error ? error.message : error).toLowerCase();
  const refusal = parseMediaError(error).kind;
  if (refusal === 'login') {
    return 'add.mediaLogin';
  }
  if (refusal === 'refused') {
    return 'add.mediaRefused';
  }
  if (text.includes('not installed') || text.includes('was not found')) {
    return 'add.mediaNoTools';
  }
  if (text.includes('drm')) {
    return 'add.mediaDrm';
  }
  if (text.includes('live')) {
    return 'add.mediaLive';
  }
  return 'add.mediaUnsupported';
}
