import { ChangeDetectionStrategy, Component, computed, inject, input, linkedSignal, output, signal } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { Router } from '@angular/router';

import { AiService } from '../../ai/ai.service';
import { safeWebUrl, SearchMode, SourceId } from '../../ai/research.models';
import { SourceStatus } from '../../ai/research.session';
import { FormatService } from '../../core/format.service';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS } from '../../model/download';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { ToastService } from '../../ui/toast.service';
import { UiState } from '../shell/ui-state.service';
import { exportName, ExportContext, isCitable, responseText, summaryLines, toBibtex, toMarkdown, toRis } from './research-export';
import { Turn, TurnItem } from './research-history';
import { AxIconComponent } from './ax-icon.component';
import { ResearchCardComponent } from './research-result-card.component';
import { deriveSteps, isAddable, Step, StepId } from './research-turn';

/** Cards shown per group before "Show more"; a turn keeps at most 60. */
const FIRST_PAGE = 20;

/** Search the same prompt again: with or without the AI, in this mode. */
export interface RerunRequest {
  readonly useAi: boolean;
  readonly mode: SearchMode;
}

/** The three groups of a result list, best first. */
export type GroupId = 'exact' | 'strong' | 'related';

interface Group {
  readonly id: GroupId;
  readonly count: number;
  readonly shown: readonly TurnItem[];
  readonly more: number;
}

/**
 * One prompt and its answer: the message, the progress steps (driven by the real events), the
 * source chips, a short summary composed from the actual results, the result cards and the
 * actions that work on them. It renders a stored `Turn`, so a live search and one restored from
 * history look the same; only a live turn can be cancelled, retried or added to downloads.
 */
@Component({
  selector: 'app-research-turn',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet, AxIconComponent, ButtonComponent, IconComponent, MenuComponent, ResearchCardComponent],
  template: `
    @let t = turn();
    <section class="turn" [attr.aria-label]="t.prompt">
      <div class="user">
        <p class="bubble" dir="auto">{{ t.prompt }}</p>
        <p class="meta is-end">
          <time [attr.datetime]="iso()">{{ fmt.time(t.createdAt) }}</time>
          <span aria-hidden="true">·</span>
          <span>{{ i18n.t('research.cat.' + t.category) }}</span>
          <span aria-hidden="true">·</span>
          <span>{{ i18n.t('research.mode.' + t.mode) }}</span>
          @if (t.useAi) {
            <span aria-hidden="true">·</span>
            <span>{{ i18n.t('research.aiBadge') }}</span>
          }
        </p>
      </div>

      <div class="assistant">

        @if (running() && live()) {
          <button appButton size="sm" variant="secondary" icon="stop" type="button" class="stop" (click)="stopSearch.emit()">{{ i18n.t('research.cancel') }}</button>
        }

        @if (t.sources.length > 0) {
          <div class="chips" role="group" [attr.aria-label]="i18n.t('research.sources')">
            @for (s of t.sources; track s.id) {
              <span class="chip" [class.is-busy]="s.state === 'searching'" [class.is-failed]="s.state === 'failed'" [class.is-stopped]="s.state === 'stopped'" [class.is-active]="only() === s.id">
                <button type="button" class="chip-main" [attr.aria-pressed]="only() === s.id" [attr.aria-label]="chipLabel(s)" [disabled]="s.found === 0" (click)="toggleOnly(s.id)">
                  <span class="chip-dot" aria-hidden="true"></span>
                  <span>{{ i18n.t('research.source.' + s.id) }}</span>
                  <span class="num chip-state">{{ chipState(s) }}</span>
                </button>
                @if (s.state === 'failed' && live()) {
                  <button type="button" class="chip-retry" [attr.aria-label]="i18n.t('research.retry').replace('{source}', i18n.t('research.source.' + s.id))" (click)="retrySource.emit(s.id)">
                    <app-icon name="retry" [size]="13" />
                  </button>
                }
              </span>
            }
          </div>
        }

        @for (u of t.unavailable; track u) {
          <p class="optional-note" [attr.data-source]="u">
            <app-icon name="info" [size]="14" />
            <span>{{ i18n.t('research.optional.' + u) }}</span>
            <button type="button" class="note-link" (click)="openResearchSettings()">{{ i18n.t('research.optionalSetup.' + u) }}</button>
          </p>
        }

        <div class="tools">
          <button type="button" class="tools-head" [attr.aria-expanded]="stepsOpen()" [attr.aria-controls]="'steps-' + t.id" (click)="stepsOpen.set(!stepsOpen())">
            <app-ax-icon name="wrench" [size]="16" />
            <span class="tools-label">{{ i18n.plural('research.stepsCount', steps().length).replace('{n}', fmt.int(steps().length)) }}</span>
            <span class="chev" [class.is-open]="stepsOpen()"><app-ax-icon name="chevronDown" [size]="12" /></span>
          </button>
          <ol class="steps" [id]="'steps-' + t.id" [attr.aria-label]="i18n.t('research.steps')" aria-live="polite" [hidden]="!stepsOpen()">
            @for (s of steps(); track s.id) {
              <li class="step" [class]="'is-' + s.state">
                @if (panels()[s.id]; as panel) {
                  <button type="button" class="row is-button" [attr.aria-expanded]="openRow() === s.id" [attr.aria-controls]="'panel-' + t.id + '-' + s.id" (click)="toggleRow(s.id)">
                    <ng-container *ngTemplateOutlet="rowBody; context: { $implicit: s, expandable: true }" />
                  </button>
                  @if (openRow() === s.id) {
                    <div class="panel" [id]="'panel-' + t.id + '-' + s.id">
                      <div class="code">
                        <div class="code-head">
                          <span class="code-label">{{ panel.label }}</span>
                          <button type="button" class="code-copy" [attr.aria-label]="i18n.t('research.copy')" [attr.title]="i18n.t('research.copy')" (click)="copyLines(panel.lines)">
                            <app-ax-icon name="copy" [size]="16" />
                          </button>
                        </div>
                        <div class="code-body">
                          @for (line of panel.lines; track $index) {
                            <div class="code-line" dir="auto">{{ line }}</div>
                          }
                        </div>
                      </div>
                    </div>
                  }
                } @else {
                  <div class="row">
                    <ng-container *ngTemplateOutlet="rowBody; context: { $implicit: s, expandable: false }" />
                  </div>
                }
              </li>
            }
          </ol>
        </div>

        <ng-template #rowBody let-s let-expandable="expandable">
          <span class="mark" aria-hidden="true">
            @switch (s.state) {
              @case ('done') {
                <app-ax-icon name="checkCircle" [size]="12" />
              }
              @case ('failed') {
                <app-ax-icon name="xCircle" [size]="12" />
              }
              @case ('active') {
                <app-ax-icon name="spinner" [size]="14" />
              }
              @case ('skipped') {
                <app-ax-icon name="minusCircle" [size]="12" />
              }
              @case ('stopped') {
                <app-ax-icon name="stopCircle" [size]="12" />
              }
              @default {
                <app-ax-icon class="is-still" name="spinner" [size]="14" />
              }
            }
          </span>
          <span class="token" aria-hidden="true">{{ stepToken(s.id) }}</span>
          <span class="name">{{ stepLabel(s.id, s.state) }}</span>
          @if (stepDetail(s); as detail) {
            <span class="detail" dir="auto">{{ detail }}</span>
          }
          <span class="sr-only">{{ i18n.t('research.stepState.' + s.state) }}</span>
          @if (expandable) {
            <span class="chev" [class.is-open]="openRow() === s.id"><app-ax-icon name="chevronDown" [size]="12" /></span>
          }
        </ng-template>



        @if (t.ai?.failed && t.ai?.failMessage; as msg) {
          <div class="warn" role="alert">
            <app-icon name="alert" [size]="14" />
            <span>{{ i18n.t('research.ai.failed') }} <span dir="auto">{{ msg }}</span></span>
            @if (live() || !running()) {
              <button appButton size="sm" variant="ghost" type="button" (click)="rerun.emit({ useAi: false, mode: t.mode })">{{ i18n.t('research.retryNoAi') }}</button>
            }
          </div>
        }
        @if (t.ai && t.ai.dropped > 0) {
          <p class="warn"><app-icon name="shield" [size]="14" /> {{ i18n.t('research.ai.dropped').replace('{n}', fmt.int(t.ai.dropped)) }}</p>
        }

        @if (t.error) {
          <div class="warn is-error" role="alert">
            <app-icon name="alert" [size]="14" />
            <span dir="auto">{{ t.error }}</span>
            <button appButton size="sm" variant="secondary" icon="retry" type="button" (click)="rerun.emit({ useAi: t.useAi, mode: t.mode })">{{ i18n.t('research.tryAgain') }}</button>
          </div>
        }

        @if (t.reused) {
          <p class="note"><app-icon name="info" [size]="14" /> {{ i18n.t('research.reused') }} <button appButton size="sm" variant="ghost" icon="retry" type="button" (click)="rerun.emit({ useAi: t.useAi, mode: t.mode })">{{ i18n.t('research.searchAgain') }}</button></p>
        }

        @if (t.status === 'cancelled') {
          <p class="note">{{ i18n.t('research.cancelled') }}</p>
        }

        @if (lines().length > 0) {
          <div class="answer" role="status">
            @for (line of lines(); track $index) {
              <p dir="auto">{{ line }}</p>
            }
            <p class="meta">
              <time [attr.datetime]="iso()">{{ fmt.time(t.createdAt) }}</time>
              <span aria-hidden="true">·</span>
              <span>{{ i18n.t(t.useAi ? 'research.aiBadge' : 'research.keyword') }}</span>
              <span aria-hidden="true">·</span>
              <span>{{ i18n.plural('research.state.done', t.items.length).replace('{n}', fmt.int(t.items.length)) }}</span>
            </p>
          </div>
        }

        @if (t.summary && t.summary.hidden > 0 && !running()) {
          <p class="note hidden-note">
            <app-icon name="info" [size]="14" />
            <span>{{ i18n.plural('research.hidden', t.summary.hidden).replace('{n}', fmt.int(t.summary.hidden)) }}</span>
            @if (t.mode === 'quick-search' && !noExact()) {
              <button appButton size="sm" variant="ghost" type="button" (click)="deeper()">{{ i18n.t('research.tryDeep') }}</button>
            }
          </p>
        }

        @if (noExact()) {
          <div class="noexact" role="status">
            <p class="noexact-title">{{ i18n.t(t.items.length > 0 ? 'research.noExact.some' : 'research.noExact.none') }}</p>
            <p class="noexact-searched">
              <span>{{ i18n.t('research.noExact.searched').replace('{sources}', sourceNames()) }}</span>
              @for (q of searched().length > 0 ? searched() : [t.prompt]; track q) {
                <span class="query" dir="auto">{{ q }}</span>
              }
            </p>
            @if (t.mode === 'quick-search') {
              <button appButton size="sm" variant="secondary" icon="layers" type="button" (click)="deeper()">{{ i18n.t('research.tryDeep') }}</button>
            }
          </div>
        }

        @if (comparisonNotes().length > 0) {
          <details class="compare">
            <summary>{{ i18n.plural('research.compare', comparisonNotes().length).replace('{n}', fmt.int(comparisonNotes().length)) }}</summary>
            <ul>
              @for (n of comparisonNotes(); track $index) {
                <li><strong dir="auto">{{ n.title }}</strong> · {{ i18n.t('research.field.' + n.field) }}: {{ n.values }}</li>
              }
            </ul>
          </details>
        }

        @if (t.items.length > 0) {
          @if (only(); as o) {
            <div class="filter">
              <button appButton size="sm" variant="ghost" type="button" (click)="only.set(null)">{{ i18n.t('research.showAll') }} ({{ i18n.t('research.source.' + o) }} ×)</button>
            </div>
          }
          @for (g of groups(); track g.id) {
            <section class="group" [attr.aria-label]="i18n.t('research.group.' + g.id)">
              @if (g.id === 'related') {
                <button type="button" class="group-head is-toggle" [attr.aria-expanded]="relatedOpen()" (click)="relatedOpen.set(!relatedOpen())">
                  <app-icon [name]="relatedOpen() ? 'chevronDown' : 'chevronEnd'" [size]="14" />
                  <span class="group-name">{{ i18n.t('research.group.related') }}</span>
                  <span class="group-count num">{{ fmt.int(g.count) }}</span>
                </button>
              } @else {
                <h3 class="group-head">
                  <span class="group-name">{{ i18n.t('research.group.' + g.id) }}</span>
                  <span class="group-count num">{{ fmt.int(g.count) }}</span>
                </h3>
              }
              @if (g.id !== 'related' || relatedOpen()) {
                <div class="results" [class.is-grid]="t.category === 'images'">
                  @for (it of g.shown; track it.id) {
                    <app-research-card
                      [item]="it"
                      [category]="t.category"
                      [alsoIn]="alsoFor(it)"
                      [selected]="selected().has(it.id)"
                      [canDownload]="canAdd() && !adding()"
                      [canSelect]="canAdd()"
                      (pick)="toggle(it.id)"
                      (download)="addIds([it.id])"
                      (findFiles)="findFiles(it)"
                      (openPage)="openPage(it)"
                      (copyLink)="copyLink(it)"
                      (copyCite)="copyCite(it)"
                    />
                  }
                </div>
                @if (g.more > 0) {
                  <button appButton size="sm" variant="ghost" type="button" class="more" (click)="expand(g.id)">{{ i18n.t('research.showMore').replace('{n}', fmt.int(g.more)) }}</button>
                }
              }
            </section>
          }

          @if (!canAdd() && !running()) {
            <p class="note"><app-icon name="info" [size]="14" /> {{ i18n.t('research.expired') }} <button appButton size="sm" variant="secondary" icon="retry" type="button" (click)="rerun.emit({ useAi: t.useAi, mode: t.mode })">{{ i18n.t('research.searchAgain') }}</button></p>
          }

          <div class="bar" role="group" [attr.aria-label]="i18n.t('research.actions')">
            <div class="bar-row">
              @if (canAdd()) {
                <button appButton variant="primary" size="sm" icon="download" type="button" [busy]="adding()" [disabled]="selected().size === 0 || adding()" (click)="addIds(selectedIds())">
                  {{ i18n.t('research.addSelected').replace('{n}', fmt.int(selected().size)) }}
                </button>
                <button appButton size="sm" variant="ghost" type="button" (click)="selectAll()">{{ i18n.t('research.selectAll') }}</button>
              }
              <span class="grow"></span>
              <button appButton size="sm" variant="ghost" icon="more" type="button" [iconOnly]="true" aria-haspopup="menu" [attr.aria-label]="i18n.t('research.moreActions')" [attr.title]="i18n.t('research.moreActions')" (click)="openMore($event)"></button>
            </div>
            @if (canAdd()) {
              <details class="opts">
                <summary>{{ i18n.t('research.downloadOptions') }}</summary>
                <div class="bar-row">
                  <label class="mini">
                    <span>{{ i18n.t('research.queue') }}</span>
                    <select class="select" (change)="queueId.set(numberOrNull($event))">
                      <option value="">{{ i18n.t('queue.main') }}</option>
                      @for (q of queues.queues(); track q.id) {
                        <option [value]="q.id">{{ q.name }}</option>
                      }
                    </select>
                  </label>
                  <label class="mini">
                    <span>{{ i18n.t('research.category') }}</span>
                    <select class="select" (change)="saveAs.set(value($event))">
                      <option value="">{{ i18n.t('research.categoryAuto') }}</option>
                      @for (c of fileCategories; track c) {
                        <option [value]="c">{{ i18n.t('category.' + c) }}</option>
                      }
                    </select>
                  </label>
                  <label class="mini check-mini">
                    <input type="checkbox" [checked]="later()" (change)="later.set(checked($event))" />
                    <span>{{ i18n.t('research.startLater') }}</span>
                  </label>
                </div>
              </details>
            }
          </div>
        }
      </div>
    </section>
    <app-menu [open]="moreMenu() !== null" [items]="moreItems()" [x]="moreMenu()?.x ?? 0" [y]="moreMenu()?.y ?? 0" (selected)="onMore($event)" (closed)="moreMenu.set(null)" />
  `,
  styles: `
    :host {
      display: block;
      font-family: var(--font-chat);
    }

    /* the thread: messages are 16 apart (Astryx ChatLayout) */
    .turn {
      display: flex;
      flex-direction: column;
      gap: 16px;
    }

    /* the user message: an end-aligned bubble (radius 28, 12px 16px, 14/20) at most 80% of the column */
    .user {
      align-self: stretch;
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      gap: 4px;
    }

    .bubble {
      margin: 0;
      box-sizing: border-box;
      max-inline-size: max(80%, 280px);
      padding: 12px 16px;
      border-radius: 28px;
      background: var(--ax-bubble);
      color: var(--ax-text);
      font: 400 14px/20px var(--font-chat);
      white-space: pre-wrap;
      overflow-wrap: anywhere;
    }

    /* the meta line: 12/20, 16px in from the edge of the bubble, 2px under it (-6 + 4 as Astryx has it) */
    .meta {
      display: flex;
      align-items: center;
      gap: 4px;
      margin: -2px 0 0;
      padding-inline: 16px;
      color: var(--ax-text-muted);
      font: 400 12px/20px var(--font-chat);
    }

    .assistant {
      display: flex;
      flex-direction: column;
      gap: 12px;
      min-inline-size: 0;
    }

    /* the progress steps: Astryx ChatToolCalls (a header, then one 24px row per step) */
    .tools {
      display: flex;
      flex-direction: column;
      align-self: flex-start;
      max-inline-size: 100%;
      color: var(--ax-text-muted);
    }

    .tools-head {
      display: flex;
      align-items: center;
      gap: 6px;
      min-block-size: 24px;
      padding: 2px 0;
      border: 0;
      background: transparent;
      color: var(--ax-text-muted);
      font: 500 12px/20px var(--font-chat);
      text-align: start;
      cursor: pointer;
      align-self: flex-start;
    }

    .tools-head:focus-visible,
    .row.is-button:focus-visible,
    .code-copy:focus-visible {
      outline: 2px solid var(--ax-focus);
      outline-offset: 3px;
      border-radius: 10px;
    }

    .chev {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      inline-size: 14px;
      block-size: 14px;
      color: var(--ax-text-muted);
      transition: transform 125ms cubic-bezier(0.24, 1, 0.4, 1);
    }

    .chev.is-open {
      transform: rotate(180deg);
    }

    .steps {
      list-style: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-direction: column;
    }

    .steps[hidden] {
      display: none;
    }

    /* rows hug their content (the chevron follows the text); only an opened panel gives the group a width */
    .step {
      display: flex;
      flex-direction: column;
      align-self: flex-start;
      max-inline-size: 100%;
      min-inline-size: 0;
    }

    .row {
      display: flex;
      align-items: center;
      gap: 6px;
      box-sizing: border-box;
      max-inline-size: 100%;
      min-block-size: 24px;
      padding: 2px 0;
      color: var(--ax-text-muted);
      font: 400 12px/20px var(--font-chat);
      text-align: start;
    }

    /* an expandable row: a button that reaches 4px past the column so its hover wash has room */
    .row.is-button {
      margin-inline: -4px;
      max-inline-size: calc(100% + 8px);
      padding-inline: 4px;
      border: 0;
      border-radius: 10px;
      background-color: transparent;
      cursor: pointer;
      transition: background-color 125ms cubic-bezier(0.24, 1, 0.4, 1);
    }

    .row.is-button:hover {
      background-color: var(--ax-overlay);
    }

    .mark {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      flex: none;
      inline-size: 16px;
      block-size: 16px;
      border-radius: 9999px;
      color: var(--ax-pending);
    }

    .is-done .mark {
      color: var(--ax-ok);
    }

    .is-failed .mark {
      color: var(--ax-fail);
    }

    .is-active .mark {
      color: var(--ax-run);
    }

    /* the short id in the mono style (Astryx puts the tool name there); the step's name follows in the regular sans */
    .token {
      font: 500 12px/20px var(--font-chat-mono);
      white-space: nowrap;
    }

    .name {
      font: 400 12px/20px var(--font-chat);
      white-space: nowrap;
    }

    .detail {
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    /* the expanded panel: indented under the label, a bordered code surface with a header bar */
    .panel {
      box-sizing: border-box;
      inline-size: min(560px, 100%);
      padding-block-end: 8px;
      padding-inline-start: 22px;
      min-inline-size: 320px;
    }

    .code {
      box-sizing: border-box;
      overflow: hidden;
      border: 1px solid var(--ax-border-code);
      border-radius: 10px;
      background: var(--ax-surface-code);
      color: var(--ax-code-text);
      font: 400 14px/20px var(--font-chat-mono);
    }

    .code-head {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 8px 16px;
      background: var(--ax-surface-code);
    }

    .code-label {
      color: var(--ax-code-muted);
      font: 500 12px/20px var(--font-chat-mono);
    }

    .code-copy {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      inline-size: 28px;
      block-size: 28px;
      padding: 0;
      border: 0;
      border-radius: 10px;
      background-color: transparent;
      color: var(--ax-code-muted);
      cursor: pointer;
      transition: background-color 125ms cubic-bezier(0.24, 1, 0.4, 1);
    }

    .code-copy:hover {
      background-color: var(--ax-overlay);
    }

    .code-body {
      margin-block-start: -8px;
      padding: 12px 16px;
    }

    .code-line {
      white-space: pre-wrap;
      overflow-wrap: anywhere;
    }

    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    .stop {
      align-self: flex-start;
    }

    /* source chips: Astryx pills (radius 9999, 12/20 medium); a dot says how the source did */
    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }

    .chip {
      display: inline-flex;
      align-items: center;
      border-radius: 9999px;
      background: var(--ax-pill-bg);
      color: var(--ax-pill-fg);
      font: 500 12px/20px var(--font-chat);
      overflow: hidden;
    }

    .chip.is-active {
      box-shadow: inset 0 0 0 1px var(--ax-focus);
    }

    .chip.is-stopped {
      opacity: 0.7;
    }

    .chip-main,
    .chip-retry {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      min-block-size: 24px;
      padding: 2px 8px;
      border: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      cursor: pointer;
    }

    .chip-main:disabled {
      cursor: default;
    }

    .chip-retry {
      padding-inline: 8px;
      border-inline-start: 1px solid var(--ax-border-code);
    }

    .chip-dot {
      inline-size: 6px;
      block-size: 6px;
      border-radius: 50%;
      background: var(--ax-ok);
    }

    .is-busy .chip-dot {
      background: var(--ax-run);
    }

    .is-failed .chip-dot {
      background: var(--ax-fail);
    }

    .is-stopped .chip-dot {
      background: var(--ax-pending);
    }

    .chip-state {
      font-weight: 400;
      opacity: 0.8;
    }

    .chip button:focus-visible {
      outline: 2px solid var(--ax-focus);
      outline-offset: -2px;
    }

    .optional-note {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
      margin: 0;
      color: var(--ax-text-muted);
      font: 400 12px/20px var(--font-chat);
    }

    .note-link {
      padding: 0;
      border: 0;
      background: transparent;
      color: var(--ax-text);
      font: inherit;
      text-decoration: underline;
      cursor: pointer;
    }

    .note-link:focus-visible {
      outline: 2px solid var(--ax-focus);
      outline-offset: 2px;
    }

    .note {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
      margin: 0;
      color: var(--ax-text-muted);
      font: 400 12px/20px var(--font-chat);
    }

    /* a quoted query: the ChatTokenizedText pill (radius 9999, 12/20 medium, 0 8px) */
    .query {
      display: inline-flex;
      align-items: center;
      padding: 0 8px;
      border-radius: 9999px;
      background: var(--ax-token-blue-bg);
      color: var(--ax-token-blue-fg);
      font: 500 12px/20px var(--font-chat);
    }

    /* an error or a warning in the thread: the Astryx banner colours */
    .warn {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
      margin: 0;
      padding: 12px 16px;
      border-radius: 16px;
      background: var(--ax-warn-bg);
      color: var(--ax-warn-fg);
      font: 400 12px/20px var(--font-chat);
    }

    .warn app-icon {
      color: var(--ax-warn-icon);
    }

    .warn.is-error {
      background: var(--ax-error-bg);
      color: var(--ax-error-fg);
    }

    .warn.is-error app-icon {
      color: var(--ax-error-icon);
    }

    /* the answer: plain 14/20 text, no bubble (Astryx ghost layout), paragraphs 4px apart */
    .answer {
      display: flex;
      flex-direction: column;
      gap: 0;
      color: var(--ax-text);
      font: 400 14px/20px var(--font-chat);
    }

    .answer p {
      max-inline-size: 680px;
      margin: 4px 0;
    }

    .answer p.meta {
      max-inline-size: none;
      margin: 4px 0 0;
      padding-inline: 0;
    }

    .noexact {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: var(--space-2);
      padding: var(--space-3) var(--space-4);
      border: 1px dashed var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-1, var(--color-surface-2));
    }

    .noexact-title {
      font-weight: var(--weight-semibold);
    }

    .noexact-searched {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
    }

    .compare {
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .compare summary,
    .opts summary {
      cursor: pointer;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .compare ul {
      margin: var(--space-2) 0 0;
      padding-inline-start: var(--space-5);
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
    }

    .opts {
      margin-block-start: var(--space-1);
    }

    .opts .bar-row {
      margin-block-start: var(--space-2);
    }

    .group {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }

    .group-head {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin: 0;
      padding: 0;
      border: 0;
      background: transparent;
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-md);
      font-weight: var(--weight-semibold);
      text-align: start;
    }

    .group-head.is-toggle {
      color: var(--color-text-secondary);
      cursor: pointer;
      border-radius: var(--radius-sm);
    }

    .group-head.is-toggle:hover {
      color: var(--color-text);
    }

    .group-head.is-toggle:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .group-count {
      padding: 0 8px;
      border-radius: var(--radius-full);
      background: var(--color-surface-3);
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
    }

    .results {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }

    .results.is-grid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    }

    .more {
      align-self: center;
    }

    .bar {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      padding: var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-md);
      background: var(--color-surface-2);
    }

    .bar-row {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-3);
    }

    .grow {
      flex: 1;
    }

    .mini {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .select {
      block-size: 30px;
      padding-inline: 8px;
      min-inline-size: 120px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .check-mini input {
      accent-color: var(--color-accent);
    }
  `,
})
export class ResearchTurnComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly fmt = inject(FormatService);
  protected readonly queues = inject(QueuesService);
  private readonly ai = inject(AiService);
  private readonly ui = inject(UiState);
  private readonly toasts = inject(ToastService);
  private readonly router = inject(Router);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly tauri = inject(TAURI_AVAILABLE);

  readonly turn = input.required<Turn>();
  /** This is the turn the running (or just finished) search belongs to: it can be cancelled and retried. */
  readonly live = input(false);
  /** The backend still holds this turn's results (searched during this run), so they can be added. */
  readonly addable = input(false);

  readonly stopSearch = output<void>();
  readonly retrySource = output<SourceId>();
  /** Search the prompt again, with or without the AI. */
  readonly rerun = output<RerunRequest>();
  /** Some results were added to the downloads. */
  readonly added = output<void>();

  protected readonly fileCategories = CATEGORY_KEYS;
  protected readonly none: ReadonlySet<string> = new Set<string>();
  protected readonly selected = signal<ReadonlySet<string>>(this.none);
  protected readonly only = signal<SourceId | null>(null);
  protected readonly relatedOpen = signal(false);
  /** Groups whose "Show more" was used. */
  private readonly expanded = signal<ReadonlySet<GroupId>>(new Set());
  protected readonly adding = signal(false);
  protected readonly queueId = signal<number | null>(null);
  protected readonly saveAs = signal('');
  protected readonly later = signal(false);
  protected readonly moreMenu = signal<{ x: number; y: number } | null>(null);

  protected readonly running = computed(() => this.turn().status === 'running');
  protected readonly canAdd = computed(() => this.addable() && this.turn().items.some(isAddable));
  protected readonly steps = computed(() => deriveSteps(this.turn()));
  /** The steps list is open while the search runs and folds away once it is done; a click overrides that until the state changes. */
  protected readonly stepsOpen = linkedSignal(() => this.running());
  /** The step whose details are open (at most one). */
  protected readonly openRow = signal<StepId | null>(null);
  /** What a step shows when it is opened: the phrasings that were searched, the sources and how each did. */
  protected readonly panels = computed<Partial<Record<StepId, { readonly label: string; readonly lines: readonly string[] }>>>(() => {
    const t = this.turn();
    const out: Partial<Record<StepId, { readonly label: string; readonly lines: readonly string[] }>> = {};
    const queries = this.searched();
    if (t.useAi && t.plan.length > 0 && queries.length > 0) {
      out.understand = { label: this.i18n.t('research.queries'), lines: queries };
    }
    if (t.sources.length > 0) {
      out.search = { label: this.i18n.t('research.sources'), lines: t.sources.map((s) => `${this.i18n.t(`research.source.${s.id}`)} · ${this.chipState(s)}`) };
    }
    return out;
  });
  protected readonly lines = computed(() => {
    const t = this.turn();
    return (this.running() && t.items.length === 0) || t.items.length === 0 ? [] : summaryLines(t, this.exportContext());
  });

  /** What was searched: the AI's phrasings, else the ones the search started with. */
  protected readonly searched = computed(() => (this.turn().plan.length > 0 ? this.turn().plan : this.turn().queries));
  protected readonly sourceNames = computed(() => this.turn().sources.map((s) => this.i18n.t(`research.source.${s.id}`)).join(', '));

  protected readonly visible = computed<readonly TurnItem[]>(() => {
    const only = this.only();
    const items = this.turn().items;
    return only === null ? items : items.filter((i) => i.source === only);
  });

  /** Exact, likely and related results (in that order); a group with nothing in it does not exist. */
  protected readonly groups = computed<readonly Group[]>(() => {
    const open = this.expanded();
    return (['exact', 'strong', 'related'] as const)
      .map((id) => {
        const all = this.visible().filter((i) => i.confidence === id);
        const shown = open.has(id) ? all : all.slice(0, FIRST_PAGE);
        return { id, count: all.length, shown, more: all.length - shown.length };
      })
      .filter((g) => g.count > 0);
  });

  /** The search is over and nothing in it is an exact match. */
  protected readonly noExact = computed(() => {
    const t = this.turn();
    return t.status === 'done' && !t.error && !t.items.some((i) => i.confidence === 'exact');
  });

  /** Deep research: where sources disagree, with the result's title (only for results on the page). */
  protected readonly comparisonNotes = computed(() => {
    const c = this.turn().comparison;
    if (c === null) {
      return [];
    }
    const byId = new Map(this.turn().items.map((i) => [i.id, i] as const));
    return c.notes.flatMap((n) => {
      const it = byId.get(n.id);
      return it === undefined ? [] : [{ title: it.title, field: n.field, values: n.values.map(([src, v]) => `${this.i18n.t(`research.source.${src}`)} ${v}`).join(', ') }];
    });
  });
  protected readonly selectedIds = computed(() => [...this.selected()]);

  protected readonly moreItems = computed<readonly MenuItem[]>(() => [
    { id: 'none', label: this.i18n.t('research.selectNone'), icon: 'close', disabled: this.selected().size === 0 },
    { id: 'copy', label: this.i18n.t('research.copyResponse'), icon: 'copy', separator: true },
    { id: 'save', label: this.i18n.t('research.save'), icon: 'fileDoc' },
    { id: 'bib', label: this.i18n.t('research.cite.bibtex'), icon: 'book', separator: true },
    { id: 'ris', label: this.i18n.t('research.cite.ris'), icon: 'book' },
  ]);

  protected alsoFor(it: TurnItem): readonly SourceId[] {
    return this.turn().comparison?.alsoIn[it.id] ?? [];
  }

  protected expand(id: GroupId): void {
    this.expanded.update((s) => new Set([...s, id]));
  }

  /** The same prompt in Deep Research mode. */
  protected deeper(): void {
    this.rerun.emit({ useAi: this.turn().useAi, mode: 'deep-research' });
  }

  protected iso(): string {
    return new Date(this.turn().createdAt).toISOString();
  }

  /** The short fact next to a step's name (how many phrasings, results, ranked, seen before), or nothing. */
  protected stepDetail(s: Step): string | null {
    if (s.count === null || s.state === 'skipped' || s.state === 'pending') {
      return null;
    }
    const n = this.fmt.int(s.count);
    switch (s.id) {
      case 'understand':
        return this.i18n.plural('research.stepDetail.queries', s.count).replace('{n}', n);
      case 'search':
        return this.i18n.plural('research.state.done', s.count).replace('{n}', n);
      case 'evaluate':
        return this.i18n.plural('research.stepDetail.ranked', s.count).replace('{n}', n);
      case 'seen':
        return s.count > 0 ? this.i18n.plural('research.stepDetail.seen', s.count).replace('{n}', n) : null;
      default:
        return null;
    }
  }

  protected toggleRow(id: StepId): void {
    this.openRow.set(this.openRow() === id ? null : id);
  }

  protected copyLines(lines: readonly string[]): void {
    this.copy(lines.join('\n'), 'research.copied');
  }

  /** The short id shown in the mono style before a step's name. */
  protected stepToken(id: StepId): string {
    return { understand: 'plan', search: 'search', evaluate: 'rank', seen: 'seen', prepare: 'done' }[id];
  }

  protected stepLabel(id: string, state: string): string {
    return this.i18n.t(id === 'prepare' && state === 'done' ? 'research.step.prepared' : `research.step.${id}`);
  }

  protected chipState(s: SourceStatus): string {
    switch (s.state) {
      case 'searching':
        return s.found > 0 ? this.i18n.t('research.state.partial').replace('{n}', this.fmt.int(s.found)) : this.i18n.t('research.state.searching');
      case 'failed':
        return this.i18n.t(`research.err.${s.code ?? 'network'}`);
      case 'stopped':
        return this.i18n.t('research.state.stopped');
      default:
        return this.i18n.plural('research.state.done', s.found).replace('{n}', this.fmt.int(s.found));
    }
  }

  protected chipLabel(s: SourceStatus): string {
    return `${this.i18n.t(`research.source.${s.id}`)}: ${this.chipState(s)}`;
  }

  protected toggleOnly(id: SourceId): void {
    this.only.set(this.only() === id ? null : id);
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLSelectElement).value;
  }

  protected checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked;
  }

  protected numberOrNull(event: Event): number | null {
    const n = Number.parseInt(this.value(event), 10);
    return Number.isFinite(n) ? n : null;
  }

  protected toggle(id: string): void {
    const next = new Set(this.selected());
    if (!next.delete(id)) {
      next.add(id);
    }
    this.selected.set(next);
  }

  protected selectAll(): void {
    this.selected.set(new Set(this.visible().filter((i) => isAddable(i) && (i.confidence !== 'related' || this.relatedOpen())).map((i) => i.id)));
  }

  protected openResearchSettings(): void {
    void this.router.navigateByUrl('/settings#research-sources');
  }

  private exportContext(): ExportContext {
    return { t: (k) => this.i18n.t(k), plural: (k, n) => this.i18n.plural(k, n), bytes: (n) => this.fmt.bytes(n), date: new Date().toISOString().slice(0, 10) };
  }

  // ---------------------------------------------------------------- results

  /** A web result: the site grabber opens on its page and lists the files (robots.txt obeyed). */
  protected findFiles(it: TurnItem): void {
    const url = safeWebUrl(it.downloadUrl);
    if (url === null) {
      return;
    }
    this.ui.grabberPreset.set(url);
    this.ui.openGrabber();
  }

  /** Opens the result's own page in the default browser (http and https only; the app checks again). */
  protected openPage(it: TurnItem): void {
    const url = safeWebUrl(it.pageUrl ?? (it.kind === 'page' ? it.downloadUrl : null));
    if (url === null) {
      return;
    }
    if (!this.tauri) {
      window.open(url, '_blank', 'noopener,noreferrer');
      return;
    }
    this.invoke<void>('open_web_url', { url }).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected copyLink(it: TurnItem): void {
    const url = safeWebUrl(it.downloadUrl);
    if (url !== null) {
      this.copy(url, 'research.linkCopied');
    }
  }

  protected async addIds(ids: readonly string[]): Promise<void> {
    if (ids.length === 0 || this.adding() || !this.canAdd()) {
      return;
    }
    this.adding.set(true);
    try {
      const r = await this.ai.addResults({
        ids,
        queueId: this.queueId(),
        category: this.saveAs() || null,
        startPaused: this.later(),
        allowDuplicates: true,
      });
      if (r.added.length === 0 && r.skipped.every((s) => s.reason === 'unknown')) {
        this.toasts.show('danger', this.i18n.t('research.expired'));
      } else {
        const text =
          r.skipped.length === 0
            ? this.i18n.t('research.added').replace('{n}', this.fmt.int(r.added.length))
            : this.i18n.t('research.addedSome').replace('{n}', this.fmt.int(r.added.length)).replace('{skipped}', this.fmt.int(r.skipped.length));
        this.toasts.show(r.added.length > 0 ? 'success' : 'danger', r.added.length > 0 ? text : this.i18n.t('research.addFailed'));
      }
      this.selected.set(this.none);
      if (r.added.length > 0) {
        this.added.emit();
      }
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.adding.set(false);
    }
  }

  // ---------------------------------------------------------------- turn actions

  protected copyResponse(): void {
    this.copy(responseText(this.turn(), this.exportContext()), 'research.responseCopied');
  }

  protected saveResearch(): void {
    const t = this.turn();
    void this.saveFile(exportName(t.prompt, 'md'), toMarkdown(t, this.exportContext()), 'md', 'research.saved');
  }

  protected openMore(event: MouseEvent): void {
    const r = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.moreMenu.set({ x: this.i18n.dir() === 'rtl' ? r.left + 220 : r.right - 220, y: r.top - 5 * 30 - 20 });
  }

  protected onMore(id: string): void {
    switch (id) {
      case 'none':
        this.selected.set(this.none);
        break;
      case 'copy':
        this.copyResponse();
        break;
      case 'save':
        this.saveResearch();
        break;
      default:
        this.exportCitations(id);
    }
  }

  protected exportCitations(id: string): void {
    this.moreMenu.set(null);
    const t = this.turn();
    const r = id === 'ris' ? toRis(t.items) : toBibtex(t.items);
    if (r.count === 0) {
      this.toasts.show('info', this.i18n.t('research.cite.none'));
      return;
    }
    const done = r.skipped > 0 ? this.i18n.t('research.cite.savedSome').replace('{n}', this.fmt.int(r.count)).replace('{skipped}', this.fmt.int(r.skipped)) : this.i18n.t('research.cite.saved').replace('{n}', this.fmt.int(r.count));
    void this.saveFile(exportName(t.prompt, id === 'ris' ? 'ris' : 'bib'), r.text, id === 'ris' ? 'ris' : 'bib', done, true);
  }

  /** One result's reference as plain text on the clipboard. */
  protected copyCite(it: TurnItem): void {
    if (!isCitable(it)) {
      return;
    }
    const who = it.authors.slice(0, 6).join(', ');
    const id = it.doi !== null ? `https://doi.org/${it.doi}` : it.arxiv !== null ? `arXiv:${it.arxiv}` : (it.pageUrl ?? it.downloadUrl);
    const text = [who, it.year === null ? '' : `(${it.year})`, `${it.title}.`, `${this.i18n.t(`research.source.${it.source}`)}.`, id].filter((p) => p.length > 0).join(' ');
    this.copy(text, 'research.citeCopied');
  }

  private copy(text: string, doneKey: string): void {
    const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard;
    if (!clipboard) {
      return;
    }
    clipboard.writeText(text).then(
      () => this.toasts.show('success', this.i18n.t(doneKey)),
      () => this.toasts.show('danger', this.i18n.t('research.copyFailed')),
    );
  }

  /** Native "save as" in the app, a plain download in a browser. `message` is a key, or the text itself when `literal`. */
  private async saveFile(name: string, text: string, kind: 'md' | 'bib' | 'ris', message: string, literal = false): Promise<void> {
    const done = literal ? message : this.i18n.t(message);
    if (this.tauri) {
      try {
        if (await this.invoke<boolean>('save_text_file', { name, text, kind })) {
          this.toasts.show('success', done);
        }
      } catch (e: unknown) {
        this.toasts.show('danger', this.ai.errorText(e));
      }
      return;
    }
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    // the download starts after the click returns: keep the address alive for a moment
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
    this.toasts.show('success', done);
  }
}
