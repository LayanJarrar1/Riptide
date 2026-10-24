import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, signal, untracked, viewChild } from '@angular/core';

import { AiService } from '../../ai/ai.service';
import { BatchExpansion, exportUrlList, MAX_EXPANSION, UrlListResult } from '../../data/batch-pattern';
import { DownloadEngine } from '../../data/download-engine';
import { I18nService } from '../../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { UiState } from '../shell/ui-state.service';
import { SettingsService } from '../settings/settings.service';

type Tab = 'pattern' | 'list';

/** Height of one row of the preview list; the window math below depends on it. */
const ROW_HEIGHT = 32;
/** Height of the list viewport. */
const VIEWPORT_HEIGHT = 224;
/** Rows drawn above and below the viewport so a fast scroll never shows a gap. */
const OVERSCAN = 6;
/** Wait after the last keystroke before asking the engine to expand. */
const EXPAND_DEBOUNCE_MS = 150;

const EMPTY_EXPANSION: BatchExpansion = { urls: [], error: null };
const EMPTY_LIST: UrlListResult = { urls: [], ignored: 0, duplicates: 0 };

/**
 * Batch download: many links at once, from a pattern (`file[001-120].zip`, `img_[a-z].jpg`,
 * several ranges) or from a pasted or loaded list. The engine does the expanding (so the
 * preview is exactly what will be added), the dialog shows the result in a virtual-scrolled
 * list with a check box per link, and "Download" adds the checked ones in one go. The checked
 * links can also be copied or exported as a text file.
 *
 * It is lazy-loaded from the shell (a dynamic `import()` on the first request), so it costs
 * the first paint nothing. It opens and closes through [`UiState`].
 */
@Component({
  selector: 'app-batch-download-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent, IconComponent],
  template: `
    <app-dialog [open]="open()" [title]="i18n.t('batch.title')" [wide]="true" (closed)="close()">
      <div class="form">
        <div class="tabs" role="tablist" [attr.aria-label]="i18n.t('batch.title')">
          <button type="button" role="tab" class="tab" id="batch-tab-pattern" [class.is-active]="tab() === 'pattern'" [attr.aria-selected]="tab() === 'pattern'" aria-controls="batch-panel" (click)="tab.set('pattern')">
            {{ i18n.t('batch.tabPattern') }}
          </button>
          <button type="button" role="tab" class="tab" id="batch-tab-list" [class.is-active]="tab() === 'list'" [attr.aria-selected]="tab() === 'list'" aria-controls="batch-panel" (click)="tab.set('list')">
            {{ i18n.t('batch.tabList') }}
          </button>
        </div>

        <div id="batch-panel" role="tabpanel" class="panel" [attr.aria-labelledby]="'batch-tab-' + tab()">
          @if (tab() === 'pattern') {
            <label class="field">
              <span class="label">{{ i18n.t('batch.pattern') }}</span>
              <span class="input-wrap" [class.is-invalid]="patternError() !== null">
                <app-icon class="input-icon" name="layers" [size]="16" />
                <input
                  class="input mono"
                  type="text"
                  name="pattern"
                  dir="ltr"
                  autocomplete="off"
                  spellcheck="false"
                  [placeholder]="i18n.t('batch.patternPlaceholder')"
                  [value]="pattern()"
                  (input)="pattern.set(value($event))"
                />
              </span>
              @if (patternError(); as e) {
                <span class="help is-error" role="alert"><app-icon name="alert" [size]="14" />{{ e }}</span>
              } @else {
                <span class="help">{{ i18n.t('batch.patternHint') }}</span>
              }
            </label>
          } @else {
            <div class="field">
              <div class="label-row">
                <label class="label" for="batch-list">{{ i18n.t('batch.listLabel') }}</label>
                <button appButton size="sm" variant="secondary" icon="folderOpen" type="button" (click)="loadFile()">{{ i18n.t('batch.loadFile') }}</button>
              </div>
              <textarea
                id="batch-list"
                class="input textarea mono"
                name="list"
                dir="ltr"
                rows="4"
                spellcheck="false"
                [placeholder]="i18n.t('batch.listPlaceholder')"
                [value]="listText()"
                (input)="listText.set(value($event))"
              ></textarea>
              @if (listText().trim().length > 0) {
                <span class="help list-summary">
                  <span>{{ i18n.plural('batch.found', list().urls.length).replace('{count}', '' + list().urls.length) }}</span>
                  @if (list().duplicates > 0) {
                    <span>{{ i18n.t('batch.duplicates').replace('{count}', '' + list().duplicates) }}</span>
                  }
                  @if (list().ignored > 0) {
                    <span class="is-warning">{{ i18n.t('batch.ignored').replace('{count}', '' + list().ignored) }}</span>
                  }
                </span>
              }
              <input #fileInput class="file-input" type="file" accept=".txt,.list,.lst,.csv,text/plain" tabindex="-1" aria-hidden="true" (change)="onFilePicked($event)" />
            </div>
          }

          @if (seenCount() > 0) {
            <p class="help seen-note" role="status"><app-icon name="info" [size]="14" /> {{ i18n.t('batch.seenBefore').replace('{count}', '' + seenCount()) }}</p>
          }

          <div class="preview">
            <div class="preview-head">
              <span class="count num">{{ i18n.t('batch.selected').replace('{selected}', '' + selection().length).replace('{total}', '' + items().length) }}</span>
              <span class="head-actions">
                <button appButton size="sm" variant="ghost" type="button" [disabled]="items().length === 0" (click)="selectAll()">{{ i18n.t('batch.selectAll') }}</button>
                <button appButton size="sm" variant="ghost" type="button" [disabled]="items().length === 0" (click)="selectNone()">{{ i18n.t('batch.selectNone') }}</button>
              </span>
            </div>
            <div #viewport class="viewport" role="list" tabindex="0" [attr.aria-label]="i18n.t('batch.items')" [style.block-size.px]="viewportHeight" (scroll)="onScroll()">
              @if (items().length === 0) {
                <p class="empty">{{ tab() === 'pattern' ? i18n.t('batch.empty') : i18n.t('batch.listEmpty') }}</p>
              } @else {
                <div class="spacer" [style.block-size.px]="window().top"></div>
                @for (row of window().rows; track row.index) {
                  <label class="row" role="listitem" [style.block-size.px]="rowHeight">
                    <input type="checkbox" class="check" [checked]="row.checked" [attr.aria-label]="i18n.t('batch.include')" (change)="toggle(row.url, $event)" />
                    <span class="index num">{{ row.index + 1 }}</span>
                    <span class="url" dir="ltr" [attr.title]="row.url">{{ row.url }}</span>
                  </label>
                }
                <div class="spacer" [style.block-size.px]="window().bottom"></div>
              }
            </div>
          </div>
        </div>

        <label class="field">
          <span class="label">{{ i18n.t('add.saveTo') }}</span>
          <span class="input-wrap">
            <app-icon class="input-icon" name="folder" [size]="16" />
            <input class="input" type="text" name="dir" dir="ltr" [value]="dir()" (input)="dir.set(value($event))" />
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
      </div>

      <ng-container dialogActions>
        <button appButton variant="ghost" icon="copy" type="button" class="first-action" [disabled]="selection().length === 0" (click)="copyList()">{{ i18n.t('batch.copyList') }}</button>
        <button appButton variant="ghost" icon="arrowDown" type="button" [disabled]="selection().length === 0" (click)="exportList()">{{ i18n.t('batch.exportList') }}</button>
        <button appButton variant="ghost" type="button" (click)="close()">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="primary" icon="download" type="button" [busy]="submitting()" [disabled]="!canSubmit()" (click)="submit()">
          {{ submitLabel() }}
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

    .tabs {
      display: inline-flex;
      align-self: flex-start;
      padding: 3px;
      gap: 2px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
    }

    .tab {
      block-size: 28px;
      padding-inline: 14px;
      border: 0;
      border-radius: var(--radius-xs);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      cursor: pointer;
      transition:
        background-color var(--motion-fast) var(--ease-standard),
        color var(--motion-fast) var(--ease-standard);
    }

    .tab:hover {
      color: var(--color-text);
    }

    .tab.is-active {
      background: var(--color-accent);
      color: var(--color-on-accent);
    }

    .panel {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
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

    .label-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
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

    .input-wrap .input {
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

    .textarea {
      block-size: auto;
      padding-block: 8px;
      resize: vertical;
      line-height: 1.4;
    }

    .mono {
      font-family: var(--font-mono);
    }

    .file-input {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      opacity: 0;
      pointer-events: none;
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

    .list-summary {
      flex-wrap: wrap;
      column-gap: var(--space-3);
    }

    .is-warning {
      color: var(--color-warning);
    }

    .preview {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }

    .preview-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      min-block-size: 28px;
    }

    .count {
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .head-actions {
      display: inline-flex;
      gap: var(--space-1);
    }

    .viewport {
      overflow-y: auto;
      overflow-x: hidden;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
    }

    .viewport:focus-visible {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .empty {
      display: grid;
      place-items: center;
      block-size: 100%;
      margin: 0;
      padding: var(--space-4);
      color: var(--color-text-muted);
      font-size: var(--text-sm);
      text-align: center;
    }

    .row {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding-inline: var(--space-3);
      cursor: pointer;
      font-size: var(--text-sm);
      box-sizing: border-box;
    }

    .row:hover {
      background: var(--color-surface-3, var(--color-surface-1));
    }

    .check {
      flex: none;
      inline-size: 16px;
      block-size: 16px;
      margin: 0;
      accent-color: var(--color-accent);
    }

    .index {
      flex: none;
      min-inline-size: 4ch;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      text-align: end;
    }

    .url {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-family: var(--font-mono);
      font-size: var(--text-xs);
      /* the URL is LTR text even inside an RTL layout; keep its start at the row's start */
      text-align: start;
      unicode-bidi: plaintext;
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
      font: inherit;
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

    .first-action {
      margin-inline-end: auto;
    }
  `,
})
export class BatchDownloadDialogComponent {
  protected readonly i18n = inject(I18nService);
  private readonly engine = inject(DownloadEngine);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly toasts = inject(ToastService);
  private readonly settingsService = inject(SettingsService);
  private readonly aiService = inject(AiService);

  private readonly ui = inject(UiState);
  /** Whether the dialog is showing; the shell flips it through [`UiState`]. */
  protected readonly open = this.ui.batchOpen;

  protected readonly rowHeight = ROW_HEIGHT;
  protected readonly viewportHeight = VIEWPORT_HEIGHT;

  protected readonly tab = signal<Tab>('pattern');
  protected readonly pattern = signal('');
  protected readonly listText = signal('');
  protected readonly expansion = signal<BatchExpansion>(EMPTY_EXPANSION);
  protected readonly list = signal<UrlListResult>(EMPTY_LIST);
  /** Links the user unchecked. Everything else is included, so a new preview starts all checked. */
  protected readonly unchecked = signal<ReadonlySet<string>>(new Set());
  protected readonly dir = signal(this.settingsService.settings().defaultDir);
  protected readonly startPaused = signal(false);
  protected readonly submitting = signal(false);
  private readonly scrollTop = signal(0);
  /** How many of the links the download memory already knows (finished and still on disk). */
  protected readonly seenCount = signal(0);
  private seenToken = 0;

  private readonly viewport = viewChild<ElementRef<HTMLElement>>('viewport');
  private readonly fileInput = viewChild<ElementRef<HTMLInputElement>>('fileInput');
  private expandToken = 0;
  private expandTimer: ReturnType<typeof setTimeout> | null = null;
  private listToken = 0;
  private listTimer: ReturnType<typeof setTimeout> | null = null;

  /** The links of the active tab. */
  protected readonly items = computed<readonly string[]>(() => (this.tab() === 'pattern' ? this.expansion().urls : this.list().urls));

  /** The checked links, in order. */
  protected readonly selection = computed(() => {
    const off = this.unchecked();
    const all = this.items();
    return off.size === 0 ? all : all.filter((u) => !off.has(u));
  });

  /** The pattern's problem in words, or `null`. */
  protected readonly patternError = computed<string | null>(() => {
    const e = this.expansion().error;
    if (e === null || this.pattern().trim().length === 0) {
      return null;
    }
    return this.i18n.t(`batch.error.${e.code}`).replace('{limit}', '' + (e.code === 'too_many' ? e.limit : MAX_EXPANSION));
  });

  /** The rows inside the scroll window plus spacers that stand in for the rest. */
  protected readonly window = computed(() => {
    const all = this.items();
    const off = this.unchecked();
    const top = this.scrollTop();
    const start = Math.max(0, Math.floor(top / ROW_HEIGHT) - OVERSCAN);
    const end = Math.min(all.length, Math.ceil((top + VIEWPORT_HEIGHT) / ROW_HEIGHT) + OVERSCAN);
    const rows = [];
    for (let index = start; index < end; index++) {
      rows.push({ index, url: all[index], checked: !off.has(all[index]) });
    }
    return { rows, top: start * ROW_HEIGHT, bottom: Math.max(0, (all.length - end) * ROW_HEIGHT) };
  });

  protected readonly canSubmit = computed(() => this.selection().length > 0 && this.dir().trim().length > 0 && !this.submitting());

  protected readonly submitLabel = computed(() => {
    if (this.submitting()) {
      return this.i18n.t('add.submitting');
    }
    const template = this.startPaused() ? this.i18n.t('batch.submitLater') : this.i18n.t('batch.submit');
    return template.replace('{count}', '' + this.selection().length);
  });

  constructor() {
    effect(() => {
      if (this.open()) {
        untracked(() => {
          this.reset();
          this.tab.set(this.ui.batchTab());
        });
      }
    });
    effect(() => this.scheduleExpand(this.pattern()));
    effect(() => {
      const items = this.items();
      untracked(() => void this.checkSeen(items));
    });
    effect(() => this.scheduleImport(this.listText()));
    // a different list starts at the top
    effect(() => {
      this.items();
      this.scrollTop.set(0);
      const el = this.viewport()?.nativeElement;
      if (el) {
        el.scrollTop = 0;
      }
    });
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  }

  protected onScroll(): void {
    const el = this.viewport()?.nativeElement;
    if (el) {
      this.scrollTop.set(el.scrollTop);
    }
  }

  protected toggle(url: string, event: Event): void {
    const on = (event.target as HTMLInputElement).checked;
    this.unchecked.update((set) => {
      const next = new Set(set);
      if (on) {
        next.delete(url);
      } else {
        next.add(url);
      }
      return next;
    });
  }

  protected selectAll(): void {
    this.unchecked.set(new Set());
  }

  protected selectNone(): void {
    this.unchecked.set(new Set(this.items()));
  }

  protected close(): void {
    this.ui.closeBatch();
  }

  /** The native folder picker inside Tauri; a note in a plain browser. */
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
      .catch((error: unknown) => this.engineError(error));
  }

  /** Loads a text file into the list tab: the native dialog in Tauri, a file input elsewhere. */
  protected loadFile(): void {
    if (!this.tauriAvailable) {
      this.fileInput()?.nativeElement.click();
      return;
    }
    this.invoke<string | null>('pick_text_file')
      .then((text) => {
        if (text !== null) {
          this.listText.set(text);
        }
      })
      .catch((error: unknown) => this.engineError(error));
  }

  protected onFilePicked(event: Event): void {
    const input = event.target as HTMLInputElement;
    const file = input.files?.item(0);
    input.value = '';
    if (file) {
      void file.text().then((text) => this.listText.set(text));
    }
  }

  protected copyList(): void {
    const text = exportUrlList(this.selection());
    const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard;
    if (!clipboard) {
      return;
    }
    clipboard.writeText(text).then(
      () => this.toasts.show('success', this.i18n.t('batch.copied')),
      (error: unknown) => this.engineError(error),
    );
  }

  protected exportList(): void {
    const text = exportUrlList(this.selection());
    const name = 'riptide-links.txt';
    if (this.tauriAvailable) {
      this.invoke<boolean>('save_text_file', { name, text })
        .then((saved) => {
          if (saved) {
            this.toasts.show('success', this.i18n.t('batch.exported'));
          }
        })
        .catch((error: unknown) => this.engineError(error));
      return;
    }
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.click();
    URL.revokeObjectURL(url);
    this.toasts.show('success', this.i18n.t('batch.exported'));
  }

  protected async submit(): Promise<void> {
    if (!this.canSubmit()) {
      return;
    }
    const urls = this.selection();
    this.submitting.set(true);
    try {
      const result = await this.engine.addBatch(urls, { dir: this.dir().trim(), startPaused: this.startPaused() });
      const skipped = result.skipped.length;
      if (result.added.length === 0) {
        this.toasts.show('info', this.i18n.t('batch.nothingAdded').replace('{skipped}', '' + skipped));
      } else if (skipped > 0) {
        this.toasts.show('success', this.i18n.t('batch.addedSkipped').replace('{added}', '' + result.added.length).replace('{skipped}', '' + skipped));
      } else {
        this.toasts.show('success', this.i18n.t('batch.added').replace('{added}', '' + result.added.length));
      }
      if (result.added.length > 0) {
        this.ui.closeBatch();
      }
    } catch (error: unknown) {
      this.engineError(error);
    } finally {
      this.submitting.set(false);
    }
  }

  private scheduleExpand(pattern: string): void {
    if (this.expandTimer !== null) {
      clearTimeout(this.expandTimer);
      this.expandTimer = null;
    }
    const token = ++this.expandToken;
    if (pattern.trim().length === 0) {
      this.expansion.set(EMPTY_EXPANSION);
      return;
    }
    this.expandTimer = setTimeout(() => {
      this.engine.expandBatch(pattern).then(
        (result) => {
          if (token === this.expandToken) {
            this.expansion.set(result);
          }
        },
        (error: unknown) => this.engineError(error),
      );
    }, EXPAND_DEBOUNCE_MS);
  }

  private scheduleImport(text: string): void {
    if (this.listTimer !== null) {
      clearTimeout(this.listTimer);
      this.listTimer = null;
    }
    const token = ++this.listToken;
    if (text.trim().length === 0) {
      this.list.set(EMPTY_LIST);
      return;
    }
    this.listTimer = setTimeout(() => {
      this.engine.importUrlList(text).then(
        (result) => {
          if (token === this.listToken) {
            this.list.set(result);
          }
        },
        (error: unknown) => this.engineError(error),
      );
    }, EXPAND_DEBOUNCE_MS);
  }

  /** Asks the download memory about (at most the first 1000 of) the links; silent on any failure. */
  private async checkSeen(items: readonly string[]): Promise<void> {
    const token = ++this.seenToken;
    if (!this.aiService.available || items.length === 0) {
      this.seenCount.set(0);
      return;
    }
    try {
      const matches = await this.aiService.seenCheck(items.slice(0, 1000).map((url) => ({ url })));
      if (token === this.seenToken) {
        this.seenCount.set(matches.filter((m) => m !== null && !m.stale).length);
      }
    } catch {
      /* no memory, no note */
    }
  }

  private engineError(error: unknown): void {
    this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
  }

  private reset(): void {
    this.tab.set('pattern');
    this.pattern.set('');
    this.listText.set('');
    this.expansion.set(EMPTY_EXPANSION);
    this.list.set(EMPTY_LIST);
    this.unchecked.set(new Set());
    this.seenCount.set(0);
    this.dir.set(this.settingsService.settings().defaultDir);
    this.startPaused.set(false);
    this.submitting.set(false);
  }
}
