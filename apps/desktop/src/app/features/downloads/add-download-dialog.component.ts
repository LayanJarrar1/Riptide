import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal } from '@angular/core';

import { FORMAT_PIPES } from '../../core/format.pipes';
import { DownloadEngine } from '../../data/download-engine';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS, CategoryKey, DownloadRequest, isHttpUrl, ProbeResult } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { SettingsService } from '../settings/settings.service';
import { CATEGORY_ICONS } from './download-row.component';

type ProbeState = 'idle' | 'probing' | 'ok' | 'invalid';

const CONNECTION_PRESETS = [1, 4, 8, 16, 32] as const;

/**
 * Add a download. Paste a link and the preview fills in (name, size, resumable, category) while
 * you choose where it goes; the primary action reads "Add" until the probe answers, then
 * "Download" because now the app knows what it will fetch.
 */
@Component({
  selector: 'app-add-download-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES, ButtonComponent, DialogComponent, IconComponent],
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

        <div class="row">
          <div class="field grow">
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
        <button appButton variant="primary" icon="download" type="button" [disabled]="!canSubmit()" (click)="submit()">
          {{ i18n.t(startPaused() ? 'add.submitLater' : 'add.submit') }}
        </button>
      </ng-container>
    </app-dialog>
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

    .preview {
      color: var(--color-text-secondary);
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

  private probeToken = 0;
  private probeTimer: ReturnType<typeof setTimeout> | null = null;

  protected readonly categoryIcon = computed(() => CATEGORY_ICONS[this.category()]);
  protected readonly checksumError = computed(() => {
    const hex = this.checksum().trim();
    return hex.length > 0 && ![32, 40, 64].includes(hex.length);
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
    const request: DownloadRequest = {
      url: this.url().trim(),
      filename: this.filename().trim() || undefined,
      dir: this.dir().trim() || undefined,
      category: this.category(),
      connections: this.connections(),
      startPaused: this.startPaused(),
      checksum: hex.length > 0 ? { algo, hex } : undefined,
    };
    this.submitting.set(true);
    try {
      const id = await this.engine.add(request, this.result() ?? undefined);
      this.toasts.show('success', this.i18n.t(this.startPaused() ? 'toast.addedLater' : 'toast.added').replace('{name}', request.filename ?? this.result()?.filename ?? request.url));
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
    this.probeTimer = setTimeout(() => {
      void this.engine.probe(trimmed).then((r) => {
        if (token !== this.probeToken) {
          return;
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
  }
}
