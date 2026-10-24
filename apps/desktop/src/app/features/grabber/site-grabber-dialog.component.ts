import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, signal, untracked, viewChild } from '@angular/core';

import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { CRAWL_SCOPES, MAX_CRAWL_DEPTH, CrawlFile } from '../../model/crawl';
import { CATEGORY_KEYS } from '../../model/download';
import { MAIN_QUEUE_ID } from '../../model/queue';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { FileNameComponent } from '../../ui/file-name.component';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { CATEGORY_ICONS } from '../downloads/download-row.component';
import { UiState } from '../shell/ui-state.service';
import { GrabberSession, GrabStep } from './grabber.session';

/** Height of one row of the results list (group headers and files alike). */
const ROW_HEIGHT = 34;
/** Height of the list viewport. */
const VIEWPORT_HEIGHT = 272;
/** Rows drawn above and below the viewport so a fast scroll never shows a gap. */
const OVERSCAN = 6;

const STEPS: readonly GrabStep[] = ['spec', 'crawl', 'add'];

/**
 * Site grabber: give it a start page, watch it find files, tick the ones you want and add them.
 * Three steps in one dialog — what to look for, the live results (a virtual-scrolled list
 * grouped by file type with a text filter), and where the files go. The engine does the
 * crawling (it obeys robots.txt; there is no switch for that), the dialog only shows what it
 * reports; the state lives in {@link GrabberSession}.
 *
 * Lazy-loaded from the shell on first use and opened through [`UiState`].
 */
@Component({
  selector: 'app-site-grabber-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FileNameComponent, ButtonComponent, DialogComponent, IconComponent],
  providers: [GrabberSession],
  template: `
    <app-dialog [open]="open()" [title]="i18n.t('grab.title')" [wide]="true" (closed)="close()">
      <ol class="steps" [attr.aria-label]="i18n.t('grab.title')">
        @for (s of steps; track s; let i = $index) {
          <li class="step" [class.is-current]="session.step() === s" [class.is-done]="stepIndex() > i" [attr.aria-current]="session.step() === s ? 'step' : null">
            <span class="step-n num">{{ i + 1 }}</span>
            <span class="step-label">{{ i18n.t('grab.step.' + s) }}</span>
          </li>
        }
      </ol>

      @switch (session.step()) {
        @case ('spec') {
          <div class="form">
            <label class="field">
              <span class="label">{{ i18n.t('grab.url') }}</span>
              <span class="input-wrap" [class.is-invalid]="showUrlError()">
                <app-icon class="input-icon" name="globe" [size]="16" />
                <input
                  class="input mono"
                  type="url"
                  name="url"
                  dir="ltr"
                  autocomplete="off"
                  spellcheck="false"
                  [placeholder]="i18n.t('grab.urlPlaceholder')"
                  [value]="session.url()"
                  (input)="session.url.set(value($event))"
                  (blur)="urlTouched.set(true)"
                  (keydown.enter)="startCrawl()"
                />
              </span>
              @if (showUrlError()) {
                <span class="help is-error" role="alert"><app-icon name="alert" [size]="14" />{{ i18n.t('grab.error.url') }}</span>
              }
              @if (session.startError(); as e) {
                <span class="help is-error" role="alert"><app-icon name="alert" [size]="14" />{{ i18n.t('grab.failed').replace('{error}', e) }}</span>
              }
            </label>

            <div class="two">
              <div class="field">
                <span class="label" id="grab-depth-label">{{ i18n.t('grab.depth') }}</span>
                <div class="presets" role="radiogroup" aria-labelledby="grab-depth-label">
                  @for (d of depths; track d) {
                    <button type="button" role="radio" class="preset num" [class.is-active]="session.depth() === d" [attr.aria-checked]="session.depth() === d" (click)="session.setDepth(d)">{{ d }}</button>
                  }
                </div>
                <span class="help">{{ i18n.t('grab.depthHint') }}</span>
              </div>

              <div class="field">
                <span class="label" id="grab-scope-label">{{ i18n.t('grab.scope') }}</span>
                <div class="presets is-wrap" role="radiogroup" aria-labelledby="grab-scope-label">
                  @for (s of scopes; track s) {
                    <button type="button" role="radio" class="preset" [class.is-active]="session.scope() === s" [attr.aria-checked]="session.scope() === s" (click)="session.scope.set(s)">{{ i18n.t('grab.scope.' + s) }}</button>
                  }
                </div>
                <label class="check-line">
                  <input type="checkbox" class="check" name="offsite" [checked]="session.offsiteFiles()" (change)="session.offsiteFiles.set(checked($event))" />
                  <span>{{ i18n.t('grab.offsite') }}</span>
                </label>
              </div>
            </div>

            <fieldset class="field types">
              <legend class="label">{{ i18n.t('grab.types') }}</legend>
              <div class="type-list">
                @for (c of categories; track c) {
                  <label class="type" [class.is-on]="session.categories().has(c)">
                    <input type="checkbox" class="check" [name]="'type-' + c" [checked]="session.categories().has(c)" (change)="session.toggleCategory(c)" />
                    <app-icon [name]="categoryIcons[c]" [size]="14" />
                    <span>{{ i18n.t('category.' + c) }}</span>
                  </label>
                }
              </div>
              <span class="help">{{ i18n.t('grab.typesHint') }}</span>
            </fieldset>

            <div class="two">
              <label class="field">
                <span class="label">{{ i18n.t('grab.include') }}</span>
                <textarea class="input textarea mono" name="include" dir="ltr" rows="2" spellcheck="false" [placeholder]="i18n.t('grab.includePlaceholder')" [value]="session.includeText()" (input)="session.includeText.set(value($event))"></textarea>
              </label>
              <label class="field">
                <span class="label">{{ i18n.t('grab.exclude') }}</span>
                <textarea class="input textarea mono" name="exclude" dir="ltr" rows="2" spellcheck="false" [placeholder]="i18n.t('grab.excludePlaceholder')" [value]="session.excludeText()" (input)="session.excludeText.set(value($event))"></textarea>
              </label>
            </div>
            <span class="help patterns-hint">{{ i18n.t('grab.patternsHint') }}</span>

            <div class="two">
              <label class="field">
                <span class="label">{{ i18n.t('grab.minSize') }}</span>
                <input class="input num" type="text" inputmode="decimal" name="min" dir="ltr" autocomplete="off" [placeholder]="i18n.t('grab.anySize')" [value]="session.minMb()" (input)="session.minMb.set(value($event))" />
              </label>
              <label class="field">
                <span class="label">{{ i18n.t('grab.maxSize') }}</span>
                <input class="input num" type="text" inputmode="decimal" name="max" dir="ltr" autocomplete="off" [placeholder]="i18n.t('grab.anySize')" [value]="session.maxMb()" (input)="session.maxMb.set(value($event))" />
              </label>
            </div>
            @if (session.specProblem() === 'grab.error.size') {
              <span class="help is-error" role="alert"><app-icon name="alert" [size]="14" />{{ i18n.t('grab.error.size') }}</span>
            } @else {
              <span class="help">{{ i18n.t('grab.sizeHint') }}</span>
            }

            <p class="note"><app-icon name="shieldCheck" [size]="16" />{{ i18n.t('grab.robots') }}</p>
          </div>
        }

        @case ('crawl') {
          <div class="form">
            <div class="progress" role="status" aria-live="polite">
              <div class="counters">
                <span class="counter"><span class="count num">{{ format.int(session.pages()) }}</span><span class="counter-label">{{ i18n.t('grab.pages') }}</span></span>
                <span class="counter"><span class="count num">{{ format.int(session.files().length) }}</span><span class="counter-label">{{ i18n.t('grab.filesFound') }}</span></span>
                @if (session.skipped() > 0) {
                  <span class="counter is-quiet"><span class="count num">{{ format.int(session.skipped()) }}</span><span class="counter-label">{{ i18n.t('grab.skipped') }}</span></span>
                }
              </div>
              <div class="status">
                @if (session.running()) {
                  <span class="spinner" aria-hidden="true"></span>
                  <span class="status-text">{{ i18n.t('grab.running').replace('{host}', session.host()) }}</span>
                  <button appButton size="sm" variant="secondary" icon="stop" type="button" (click)="session.cancel()">{{ i18n.t('grab.stop') }}</button>
                } @else {
                  <span class="status-text" [class.is-error]="session.end()?.error">{{ endText() }}</span>
                }
              </div>
            </div>

            <div class="preview">
              <div class="preview-head">
                <label class="search">
                  <app-icon class="search-icon" name="search" [size]="14" />
                  <input class="input search-input" type="search" name="filter" autocomplete="off" [attr.aria-label]="i18n.t('grab.filter')" [placeholder]="i18n.t('grab.filter')" [value]="session.filter()" (input)="session.filter.set(value($event))" />
                </label>
                <span class="head-actions">
                  <span class="selected num">{{ i18n.t('batch.selected').replace('{selected}', format.int(session.selection().length)).replace('{total}', format.int(session.files().length)) }}</span>
                  <button appButton size="sm" variant="ghost" type="button" [disabled]="session.visible().length === 0" (click)="session.selectAll()">{{ i18n.t('batch.selectAll') }}</button>
                  <button appButton size="sm" variant="ghost" type="button" [disabled]="session.visible().length === 0" (click)="session.selectNone()">{{ i18n.t('batch.selectNone') }}</button>
                </span>
              </div>
              <div #viewport class="viewport" role="list" tabindex="0" [attr.aria-label]="i18n.t('grab.results')" [style.block-size.px]="viewportHeight" (scroll)="onScroll()">
                @if (session.rows().length === 0) {
                  <p class="empty">{{ emptyText() }}</p>
                } @else {
                  <div class="spacer" [style.block-size.px]="window().top"></div>
                  @for (row of window().rows; track row.key) {
                    @if (row.group; as g) {
                      <label class="row is-group" role="listitem" [style.block-size.px]="rowHeight">
                        <input type="checkbox" class="check" [checked]="row.checked" [attr.aria-label]="i18n.t('category.' + g.category)" (change)="session.toggleMany(g.urls, checked($event))" />
                        <app-icon [name]="categoryIcons[g.category]" [size]="16" />
                        <span class="group-name">{{ i18n.t('category.' + g.category) }}</span>
                        <span class="group-count num">{{ format.int(g.count) }}</span>
                      </label>
                    } @else if (row.file; as f) {
                      <label class="row" role="listitem" [style.block-size.px]="rowHeight">
                        <input type="checkbox" class="check" [checked]="row.checked" [attr.aria-label]="f.name" (change)="session.toggle(f.url, checked($event))" />
                        <app-file-name class="name" [name]="f.name" [label]="f.url" />
                        <span class="host" dir="ltr">{{ f.host }}</span>
                        <span class="size num">{{ f.size === null ? '' : format.bytes(f.size) }}</span>
                      </label>
                    }
                  }
                  <div class="spacer" [style.block-size.px]="window().bottom"></div>
                }
              </div>
            </div>
          </div>
        }

        @case ('add') {
          <div class="form">
            <p class="summary">{{ i18n.plural('grab.summary', session.selection().length).replace('{count}', format.int(session.selection().length)) }}</p>

            <label class="field">
              <span class="label">{{ i18n.t('add.saveTo') }}</span>
              <span class="input-wrap">
                <app-icon class="input-icon" name="folder" [size]="16" />
                <input class="input" type="text" name="dir" dir="ltr" [value]="session.dir()" (input)="session.dir.set(value($event))" />
                <button appButton size="sm" variant="secondary" type="button" class="input-action" (click)="browse()">{{ i18n.t('add.browse') }}</button>
              </span>
            </label>

            <label class="field">
              <span class="label">{{ i18n.t('queue.detailsLabel') }}</span>
              <span class="input-wrap">
                <app-icon class="input-icon" name="layers" [size]="16" />
                <select class="input select" name="queue" [value]="session.queueId()" (change)="session.queueId.set(numberValue($event))">
                  @for (q of session.queues(); track q.id) {
                    <option [value]="q.id" [selected]="q.id === session.queueId()">{{ q.id === mainQueueId ? i18n.t('queue.main') : q.name }}</option>
                  }
                </select>
                <app-icon class="select-caret" name="chevronDown" [size]="14" />
              </span>
            </label>

            <div class="field">
              <span class="label" id="grab-start-label">{{ i18n.t('add.start') }}</span>
              <div class="presets" role="radiogroup" aria-labelledby="grab-start-label">
                <button type="button" role="radio" class="preset" [class.is-active]="!session.startPaused()" [attr.aria-checked]="!session.startPaused()" (click)="session.startPaused.set(false)">{{ i18n.t('add.startNow') }}</button>
                <button type="button" role="radio" class="preset" [class.is-active]="session.startPaused()" [attr.aria-checked]="session.startPaused()" (click)="session.startPaused.set(true)">{{ i18n.t('add.startLater') }}</button>
              </div>
            </div>
          </div>
        }
      }

      <ng-container dialogActions>
        @if (session.step() !== 'spec') {
          <button appButton variant="ghost" icon="chevronStart" type="button" class="first-action" (click)="session.back()">{{ i18n.t('grab.back') }}</button>
        }
        <button appButton variant="ghost" type="button" (click)="close()">{{ i18n.t('action.cancel') }}</button>
        @switch (session.step()) {
          @case ('spec') {
            <button appButton variant="primary" icon="search" type="button" [disabled]="!session.canStart()" (click)="startCrawl()">{{ i18n.t('grab.start') }}</button>
          }
          @case ('crawl') {
            <button appButton variant="primary" icon="chevronEnd" type="button" [disabled]="!session.canNext()" (click)="session.next()">{{ i18n.t('grab.next') }}</button>
          }
          @case ('add') {
            <button appButton variant="primary" icon="download" type="button" [busy]="session.adding()" [disabled]="!session.canAdd()" (click)="submit()">{{ submitLabel() }}</button>
          }
        }
      </ng-container>
    </app-dialog>
  `,
  styles: `
    .steps {
      display: flex;
      gap: var(--space-4);
      margin: 0;
      padding: 0 0 var(--space-3);
      list-style: none;
    }

    .step {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      color: var(--color-text-muted);
      font-size: var(--text-sm);
    }

    .step-n {
      display: grid;
      place-items: center;
      inline-size: 22px;
      block-size: 22px;
      border: 1px solid var(--color-border-strong);
      border-radius: 50%;
      font-size: var(--text-xs);
    }

    .step.is-current {
      color: var(--color-text);
      font-weight: var(--weight-medium);
    }

    .step.is-current .step-n {
      border-color: var(--color-accent);
      background: var(--color-accent);
      color: var(--color-on-accent);
    }

    .step.is-done .step-n {
      border-color: var(--color-accent);
      color: var(--color-accent);
    }

    .form {
      display: flex;
      flex-direction: column;
      gap: var(--space-4);
      padding-block: var(--space-2) var(--space-3);
    }

    .two {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-4);
    }

    @media (max-width: 640px) {
      .two {
        grid-template-columns: minmax(0, 1fr);
      }
    }

    .field {
      display: flex;
      flex-direction: column;
      gap: 6px;
      min-inline-size: 0;
      margin: 0;
      padding: 0;
      border: 0;
    }

    .label {
      padding: 0;
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

    /* paths and URLs are LTR even in an RTL layout: keep the icon and button off the text */
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

    .select {
      appearance: none;
      cursor: pointer;
      padding-inline-end: 34px;
    }

    .select-caret {
      position: absolute;
      inset-inline-end: 10px;
      color: var(--color-text-muted);
      pointer-events: none;
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

    .patterns-hint {
      margin-block-start: calc(-1 * var(--space-2));
    }

    .note {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin: 0;
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .note app-icon {
      flex: none;
      color: var(--color-accent);
    }

    .presets {
      display: inline-flex;
      padding: 3px;
      gap: 2px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      inline-size: fit-content;
      max-inline-size: 100%;
    }

    .presets.is-wrap {
      flex-wrap: wrap;
    }

    .preset {
      min-inline-size: 36px;
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

    .check {
      flex: none;
      inline-size: 16px;
      block-size: 16px;
      margin: 0;
      accent-color: var(--color-accent);
    }

    .check-line {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
      cursor: pointer;
    }

    .type-list {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
    }

    .type {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 5px 10px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      cursor: pointer;
    }

    .type.is-on {
      border-color: var(--color-accent);
      color: var(--color-text);
    }

    .progress {
      display: flex;
      align-items: center;
      justify-content: space-between;
      flex-wrap: wrap;
      gap: var(--space-3);
      padding: var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
    }

    .counters {
      display: flex;
      gap: var(--space-5);
    }

    .counter {
      display: flex;
      flex-direction: column;
    }

    .count {
      font-size: var(--text-lg);
      font-weight: var(--weight-semibold);
    }

    .counter-label {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .counter.is-quiet .count {
      color: var(--color-text-secondary);
    }

    .status {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-inline-size: 0;
    }

    .status-text {
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .status-text.is-error {
      color: var(--color-danger);
    }

    .spinner {
      flex: none;
      inline-size: 14px;
      block-size: 14px;
      border: 2px solid var(--color-border-strong);
      border-top-color: var(--color-accent);
      border-radius: 50%;
      animation: grab-spin 0.8s linear infinite;
    }

    @keyframes grab-spin {
      to {
        transform: rotate(360deg);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .spinner {
        animation: none;
      }
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
      flex-wrap: wrap;
      gap: var(--space-3);
    }

    .search {
      position: relative;
      display: flex;
      align-items: center;
      flex: 1 1 200px;
      max-inline-size: 280px;
    }

    .search-icon {
      position: absolute;
      inset-inline-start: 10px;
      color: var(--color-text-muted);
      pointer-events: none;
    }

    .search-input {
      block-size: 30px;
      padding-inline-start: 30px;
    }

    .head-actions {
      display: inline-flex;
      align-items: center;
      gap: var(--space-1);
    }

    .selected {
      margin-inline-end: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
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
      box-sizing: border-box;
      cursor: pointer;
      font-size: var(--text-sm);
    }

    .row:hover {
      background: var(--color-surface-3, var(--color-surface-1));
    }

    .row.is-group {
      background: var(--color-surface-1);
      border-block-end: 1px solid var(--color-border);
      color: var(--color-text-secondary);
      font-weight: var(--weight-medium);
    }

    .group-count {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      font-weight: var(--weight-regular, 400);
    }

    .name {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      text-align: start;
    }

    .host {
      flex: 0 1 30%;
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--color-text-muted);
      font-family: var(--font-mono);
      font-size: var(--text-xs);
      text-align: start;
      unicode-bidi: plaintext;
    }

    .size {
      flex: none;
      min-inline-size: 9ch;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      text-align: end;
    }

    .summary {
      margin: 0;
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .first-action {
      margin-inline-end: auto;
    }
  `,
})
export class SiteGrabberDialogComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly session = inject(GrabberSession);
  private readonly ui = inject(UiState);
  private readonly tauriAvailable = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly toasts = inject(ToastService);

  /** Whether the dialog is showing; the shell flips it through [`UiState`]. */
  protected readonly open = this.ui.grabberOpen;

  protected readonly steps = STEPS;
  protected readonly depths = Array.from({ length: MAX_CRAWL_DEPTH + 1 }, (_, i) => i);
  protected readonly scopes = CRAWL_SCOPES;
  protected readonly categories = CATEGORY_KEYS;
  protected readonly categoryIcons = CATEGORY_ICONS;
  protected readonly rowHeight = ROW_HEIGHT;
  protected readonly viewportHeight = VIEWPORT_HEIGHT;
  protected readonly mainQueueId = MAIN_QUEUE_ID;

  /** The address field shows its error only after the user has left it (or tried to start). */
  protected readonly urlTouched = signal(false);
  private readonly scrollTop = signal(0);
  private readonly viewport = viewChild<ElementRef<HTMLElement>>('viewport');

  protected readonly stepIndex = computed(() => STEPS.indexOf(this.session.step()));

  protected readonly showUrlError = computed(() => this.urlTouched() && this.session.specProblem() === 'grab.error.url');

  /** The rows inside the scroll window plus spacers that stand in for the rest. */
  protected readonly window = computed(() => {
    const all = this.session.rows();
    const off = this.session.unchecked();
    const top = this.scrollTop();
    const start = Math.max(0, Math.floor(top / ROW_HEIGHT) - OVERSCAN);
    const end = Math.min(all.length, Math.ceil((top + VIEWPORT_HEIGHT) / ROW_HEIGHT) + OVERSCAN);
    const rows: { key: string; checked: boolean; group: Extract<(typeof all)[number], { kind: 'group' }> | null; file: CrawlFile | null }[] = [];
    for (let i = start; i < end; i++) {
      const row = all[i];
      if (row.kind === 'group') {
        rows.push({ key: 'g:' + row.category, checked: row.urls.every((u) => !off.has(u)), group: row, file: null });
      } else {
        rows.push({ key: row.file.url, checked: !off.has(row.file.url), group: null, file: row.file });
      }
    }
    return { rows, top: start * ROW_HEIGHT, bottom: Math.max(0, (all.length - end) * ROW_HEIGHT) };
  });

  protected readonly emptyText = computed(() => {
    if (this.session.files().length > 0) {
      return this.i18n.t('grab.noMatch');
    }
    return this.session.running() ? this.i18n.t('grab.searching') : this.i18n.t('grab.none');
  });

  /** How the crawl ended, in words. */
  protected readonly endText = computed(() => {
    const end = this.session.end();
    if (end === null) {
      return '';
    }
    if (end.error !== null) {
      return this.i18n.t('grab.failed').replace('{error}', end.error);
    }
    const parts = [this.i18n.t(end.stop === 'cancelled' ? 'grab.stopped' : end.stop === 'time-budget' ? 'grab.timeUp' : 'grab.finished')];
    if (end.pageCapHit) {
      parts.push(this.i18n.t('grab.pageCap'));
    }
    if (end.fileCapHit) {
      parts.push(this.i18n.t('grab.fileCap'));
    }
    return parts.join(' ');
  });

  protected readonly submitLabel = computed(() => {
    if (this.session.adding()) {
      return this.i18n.t('add.submitting');
    }
    return this.i18n.plural('grab.submit', this.session.selection().length).replace('{count}', this.format.int(this.session.selection().length));
  });

  constructor() {
    effect(() => {
      if (this.open()) {
        untracked(() => {
          this.session.reset();
          this.urlTouched.set(false);
          this.scrollTop.set(0);
          // "Find files on this page" from Research: that one page, listed at once
          const preset = this.ui.grabberPreset();
          if (preset !== null) {
            this.ui.grabberPreset.set(null);
            this.session.url.set(preset);
            this.session.depth.set(0);
            this.session.scope.set('same-host');
            this.urlTouched.set(true);
            void this.session.start();
          }
        });
      }
    });
    // a different list starts at the top
    effect(() => {
      this.session.filter();
      untracked(() => {
        this.scrollTop.set(0);
        const el = this.viewport()?.nativeElement;
        if (el) {
          el.scrollTop = 0;
        }
      });
    });
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  }

  protected numberValue(event: Event): number {
    return Number((event.target as HTMLSelectElement).value);
  }

  protected checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked;
  }

  protected onScroll(): void {
    const el = this.viewport()?.nativeElement;
    if (el) {
      this.scrollTop.set(el.scrollTop);
    }
  }

  protected startCrawl(): void {
    this.urlTouched.set(true);
    void this.session.start();
  }

  protected close(): void {
    this.session.close();
    this.ui.closeGrabber();
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
          this.session.dir.set(dir);
        }
      })
      .catch((error: unknown) => this.engineError(error));
  }

  protected async submit(): Promise<void> {
    try {
      const result = await this.session.add();
      if (result === null) {
        return;
      }
      const skipped = result.skipped.length;
      if (result.added.length === 0) {
        this.toasts.show('info', this.i18n.t('batch.nothingAdded').replace('{skipped}', '' + skipped));
        return;
      }
      const key = skipped > 0 ? 'batch.addedSkipped' : 'batch.added';
      this.toasts.show('success', this.i18n.t(key).replace('{added}', '' + result.added.length).replace('{skipped}', '' + skipped));
      this.ui.closeGrabber();
    } catch (error: unknown) {
      this.engineError(error);
    }
  }

  private engineError(error: unknown): void {
    this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error)));
  }
}
