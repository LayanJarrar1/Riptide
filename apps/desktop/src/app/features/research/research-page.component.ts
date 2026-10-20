import { AfterViewChecked, ChangeDetectionStrategy, Component, computed, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { Router } from '@angular/router';

import { FORM_STYLES } from '../../ai/ai-styles';
import { AiMessage } from '../../ai/ai.models';
import { AiService } from '../../ai/ai.service';
import { CATEGORY_IDS, CategoryId, ResultItem, safeWebUrl, shortUrl, SourceId } from '../../ai/research.models';
import { ResearchSession, SourceStatus } from '../../ai/research.session';
import { SeenNoticeComponent } from '../../ai/seen-notice.component';
import { FormatService } from '../../core/format.service';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { CATEGORY_KEYS } from '../../model/download';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { UiState } from '../shell/ui-state.service';

/**
 * Research: type a topic, pick what kind of thing you want (papers, books, music, video, games,
 * software, images, datasets or the web) and get open and public sources' results with direct
 * download links. Nothing waits: every source's results appear the moment that source answers,
 * and with an AI provider set up the query plan and then the ranking (one line of "why it
 * matches" per result) stream in behind them while the list re-sorts in place. Selected results
 * become normal downloads; a web result opens the site grabber on its page to list the files.
 */
@Component({
  selector: 'app-research-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, SeenNoticeComponent],
  providers: [ResearchSession],
  template: `
    <div class="page">
      <header class="page-head">
        <h1 class="page-title">{{ i18n.t('research.title') }}</h1>
        <p class="page-sub">{{ i18n.t('research.subtitle') }}</p>
      </header>

      @if (!ai.available) {
        <p class="note">{{ i18n.t('research.desktopOnly') }}</p>
      } @else {
        <p class="rights-note" role="note"><app-icon name="shieldCheck" [size]="16" /> {{ i18n.t('research.notice') }}</p>

        <form class="search" role="search" (submit)="search($event)">
          <div class="search-box">
            <app-icon class="search-icon" name="search" [size]="18" />
            <input
              class="search-input"
              type="search"
              name="topic"
              dir="auto"
              autocomplete="off"
              [placeholder]="i18n.t('research.placeholder')"
              [attr.aria-label]="i18n.t('research.title')"
              [value]="topic()"
              (input)="topic.set(value($event))"
            />
          </div>
          <button appButton variant="primary" icon="search" type="submit" [busy]="session.busy()" [disabled]="topic().trim().length === 0 || session.busy()">
            {{ i18n.t(session.busy() ? 'research.searching' : 'research.search') }}
          </button>
        </form>

        <div class="tabs" role="tablist" [attr.aria-label]="i18n.t('research.categories')">
          @for (c of categoryIds; track c) {
            <button type="button" role="tab" class="tab" [id]="'rtab-' + c" [class.is-active]="category() === c" [attr.aria-selected]="category() === c" [attr.tabindex]="category() === c ? 0 : -1" (click)="pickCategory(c)" (keydown)="onTabKey($event)">
              {{ i18n.t('research.cat.' + c) }}
            </button>
          }
        </div>

        <div class="options">
          @if (aiOn()) {
            <label class="check">
              <span class="switch" [class.is-on]="useAi()">
                <input type="checkbox" role="switch" [checked]="useAi()" (change)="useAi.set(checked($event))" />
                <span class="knob"></span>
              </span>
              <span>{{ i18n.t('research.useAi') }}</span>
            </label>
          } @else {
            <div class="keyword-mode" role="note">
              <app-icon name="info" [size]="16" />
              <span>{{ i18n.t('research.keywordMode') }}</span>
              <button appButton size="sm" variant="secondary" icon="sparkles" type="button" (click)="openAiSettings()">{{ i18n.t('research.openAiSettings') }}</button>
            </div>
          }
        </div>

        @if (aiOn() && useAi()) {
          <details class="preview" (toggle)="onPreviewToggle($event)">
            <summary class="preview-summary">
              <app-icon name="chevronEnd" [size]="14" class="chev" />
              {{ i18n.t('research.preview') }}
            </summary>
            <p class="hint">{{ i18n.t('research.previewDesc') }}</p>
            @for (m of preview(); track $index) {
              <pre class="msg" dir="auto">{{ m.content }}</pre>
            }
          </details>
        }

        @if (session.phase() !== 'idle') {
          <section class="status" [attr.aria-label]="i18n.t('research.sources')">
            <div class="status-head" role="status" aria-live="polite">
              @if (session.busy()) {
                <div class="progress" aria-hidden="true"><span class="bar"></span></div>
              }
              <span class="status-text">{{ statusText() }}</span>
              @if (session.busy()) {
                <button appButton size="sm" variant="secondary" icon="stop" type="button" (click)="cancel()">{{ i18n.t('research.cancel') }}</button>
              }
            </div>

            <div class="chips" role="group" [attr.aria-label]="i18n.t('research.sources')">
              @for (s of session.sources(); track s.id) {
                <span class="chip" [class.is-busy]="s.state === 'searching'" [class.is-failed]="s.state === 'failed'" [class.is-stopped]="s.state === 'stopped'" [class.is-active]="session.onlySource() === s.id">
                  <button type="button" class="chip-main" [attr.aria-pressed]="session.onlySource() === s.id" [attr.aria-label]="chipLabel(s)" [disabled]="s.found === 0" (click)="toggleOnly(s.id)">
                    <span class="chip-dot" aria-hidden="true"></span>
                    <span class="chip-name">{{ sourceLabel(s.id) }}</span>
                    <span class="chip-state num">{{ chipState(s) }}</span>
                  </button>
                  @if (s.state === 'failed') {
                    <button type="button" class="chip-retry" [attr.aria-label]="i18n.t('research.retry').replace('{source}', sourceLabel(s.id))" (click)="retry(s.id)">
                      <app-icon name="retry" [size]="13" />
                    </button>
                  }
                </span>
              }
              @for (u of session.unavailable(); track u.source) {
                <span class="chip is-setup">
                  <span class="chip-name">{{ sourceLabel(u.source) }}</span>
                  <button type="button" class="chip-setup" (click)="openResearchSettings()">{{ i18n.t('research.unavailable.' + u.source) }}</button>
                </span>
              }
            </div>

            @if (session.plan().length > 0) {
              <p class="plan">
                <app-icon name="sparkles" [size]="14" />
                <span>{{ i18n.t('research.queries') }}:</span>
                @for (q of session.plan(); track q) {
                  <span class="query" dir="auto">{{ q }}</span>
                }
              </p>
            }
            @if (session.ai(); as a) {
              @if (a.error; as e) {
                <p class="warn" role="alert"><app-icon name="alert" [size]="14" /> {{ i18n.t('research.ai.failed') }} {{ ai.failText(e) }}</p>
              }
              @if (a.dropped > 0) {
                <p class="warn"><app-icon name="shield" [size]="14" /> {{ i18n.t('research.ai.dropped').replace('{n}', fmt.int(a.dropped)) }}</p>
              }
            }
          </section>

          @if (session.items().length === 0 && !session.busy()) {
            <p class="empty">{{ i18n.t('research.none') }}</p>
          }

          @if (session.items().length > 0) {
            <div class="summary" role="status">
              <span class="count num">{{ i18n.t('research.count').replace('{n}', fmt.int(session.ordered().length)) }}</span>
              @if (session.onlySource(); as only) {
                <button appButton size="sm" variant="ghost" type="button" (click)="session.onlySource.set(null)">{{ i18n.t('research.showAll') }} ({{ sourceLabel(only) }} ×)</button>
              }
            </div>
            <ul class="results" #list>
              @for (it of session.ordered(); track it.id) {
                <li class="card" [attr.data-id]="it.id" [class.is-selected]="selected().has(it.id)" [class.is-page]="it.kind === 'page'">
                  @if (it.kind === 'file') {
                    <label class="pick">
                      <input type="checkbox" [checked]="selected().has(it.id)" [attr.aria-label]="i18n.t('research.selectCard').replace('{title}', it.title)" (change)="toggle(it.id)" />
                    </label>
                  } @else {
                    <span class="pick page-mark" aria-hidden="true"><app-icon name="globe" [size]="16" /></span>
                  }
                  <div class="card-body">
                    <h3 class="card-title" dir="auto">{{ it.title }}</h3>
                    <div class="meta">
                      <span class="badge source">{{ sourceLabel(it.source) }}</span>
                      <span class="badge rights" [class]="'rights-' + it.rights.kind" dir="auto">{{ rightsText(it) }}</span>
                      @if (it.seen) {
                        <span class="badge seen"><app-icon name="check" [size]="12" /> {{ i18n.t('seen.badge') }}</span>
                      }
                      @if (byline(it); as b) {
                        <span class="byline" dir="auto">{{ b }}</span>
                      }
                      @if (it.size !== null) {
                        <span class="num">{{ fmt.bytes(it.size) }}</span>
                      }
                      @if (it.score !== null) {
                        <span class="score num">{{ i18n.t('research.score').replace('{n}', fmt.int(it.score)) }}</span>
                      }
                    </div>
                    @if (it.kind === 'page') {
                      <p class="url" dir="ltr">{{ pageUrl(it) }}</p>
                    }
                    @if (it.why) {
                      <p class="why" dir="auto"><app-icon name="sparkles" [size]="14" /> <span><strong>{{ i18n.t('research.why') }}:</strong> {{ it.why }}</span></p>
                    } @else if (it.summary) {
                      <p class="summary-text" dir="auto">{{ it.summary }}</p>
                    }
                    @if (it.kind === 'page') {
                      <div class="page-actions">
                        <button appButton size="sm" variant="secondary" icon="folderOpen" type="button" (click)="findFiles(it)">{{ i18n.t('research.findFiles') }}</button>
                        <button appButton size="sm" variant="ghost" icon="copy" type="button" (click)="copyLink(it)">{{ i18n.t('research.copyLink') }}</button>
                      </div>
                    }
                    @if (it.seen) {
                      <app-seen-notice [match]="it.seen" [compact]="true" />
                    }
                  </div>
                </li>
              }
            </ul>

            <div class="bar-actions" role="group">
              <div class="bar-left">
                <button appButton size="sm" variant="ghost" type="button" (click)="selectAll()">{{ i18n.t('research.selectAll') }}</button>
                <button appButton size="sm" variant="ghost" type="button" [disabled]="selected().size === 0" (click)="selected.set(emptySet)">{{ i18n.t('research.selectNone') }}</button>
              </div>
              <div class="bar-right">
                <label class="mini">
                  <span>{{ i18n.t('research.queue') }}</span>
                  <select class="input small" (change)="queueId.set(numberOrNull($event))">
                    <option value="">{{ i18n.t('queue.main') }}</option>
                    @for (q of queues.queues(); track q.id) {
                      <option [value]="q.id">{{ q.name }}</option>
                    }
                  </select>
                </label>
                <label class="mini">
                  <span>{{ i18n.t('research.category') }}</span>
                  <select class="input small" (change)="saveAs.set(value($event))">
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
                <button appButton variant="primary" icon="download" type="button" [busy]="adding()" [disabled]="selected().size === 0 || adding()" (click)="addSelected()">
                  {{ i18n.t('research.addSelected').replace('{n}', fmt.int(selected().size)) }}
                </button>
              </div>
            </div>
          }
        } @else {
          <p class="empty">{{ i18n.t('research.start') }}</p>
        }

        <footer class="policy">{{ i18n.t('research.footer') }}</footer>
      }
    </div>
  `,
  styles: [
    FORM_STYLES,
    `
      :host {
        display: block;
        block-size: 100%;
        overflow: auto;
        /* keep focused or scrolled-to cards clear of the sticky action bar */
        scroll-padding-block-end: 96px;
      }

      .page {
        max-inline-size: 980px;
        padding: var(--space-8) var(--space-8) var(--space-16);
        margin-inline: auto;
      }

      .page-title {
        font-size: var(--text-2xl);
        font-weight: var(--weight-semibold);
        letter-spacing: var(--tracking-tight);
      }

      .page-sub,
      .hint,
      .empty,
      .note {
        color: var(--color-text-muted);
        margin-block-start: var(--space-1);
      }

      .empty {
        padding-block: var(--space-10, 48px);
        text-align: center;
      }

      .rights-note {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        margin-block-start: var(--space-4);
        padding: var(--space-2) var(--space-3);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        background: var(--color-surface-2);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
      }

      .rights-note app-icon {
        color: var(--color-accent);
        flex-shrink: 0;
      }

      .search {
        display: flex;
        gap: var(--space-3);
        margin-block-start: var(--space-4);
      }

      .search-box {
        position: relative;
        flex: 1;
        display: flex;
        align-items: center;
      }

      .search-icon {
        position: absolute;
        inset-inline-start: 12px;
        color: var(--color-text-muted);
        pointer-events: none;
      }

      .search-input {
        inline-size: 100%;
        block-size: 44px;
        padding-inline: 40px 14px;
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-md);
        background: var(--color-surface-2);
        color: var(--color-text);
        font: inherit;
      }

      .search-input:focus {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px var(--color-accent-ring);
      }

      .tabs {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-1);
        margin-block-start: var(--space-4);
        padding-block-end: var(--space-2);
        border-block-end: 1px solid var(--color-border);
      }

      .tab {
        block-size: 32px;
        padding-inline: 14px;
        border: 1px solid transparent;
        border-radius: var(--radius-full);
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: var(--text-sm);
        font-weight: var(--weight-medium);
        cursor: pointer;
        transition:
          background var(--motion-fast) var(--ease-standard),
          color var(--motion-fast) var(--ease-standard);
      }

      .tab:hover {
        background: var(--color-surface-2);
        color: var(--color-text);
      }

      .tab.is-active {
        border-color: var(--color-accent);
        background: var(--color-accent-soft, var(--color-surface-3));
        color: var(--color-text);
      }

      .tab:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
      }

      .options {
        margin-block-start: var(--space-3);
      }

      .check {
        display: inline-flex;
        align-items: center;
        gap: var(--space-3);
        cursor: pointer;
        font-size: var(--text-sm);
      }

      .keyword-mode {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        flex-wrap: wrap;
        padding: var(--space-3);
        border: 1px dashed var(--color-border-strong);
        border-radius: var(--radius-sm);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
      }

      .preview {
        margin-block-start: var(--space-3);
        font-size: var(--text-sm);
      }

      .preview-summary {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        cursor: pointer;
        list-style: none;
        color: var(--color-text-secondary);
        user-select: none;
      }

      .preview-summary::-webkit-details-marker {
        display: none;
      }

      .chev {
        transition: transform var(--motion-fast) var(--ease-standard);
      }

      .preview[open] .chev {
        transform: rotate(calc(var(--dir-sign) * 90deg));
      }

      .msg {
        margin: var(--space-2) 0 0;
        padding: var(--space-2) var(--space-3);
        max-block-size: 12rem;
        overflow: auto;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        background: var(--color-surface-2);
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: var(--text-xs);
      }

      .status {
        margin-block-start: var(--space-4);
        padding: var(--space-3) var(--space-4);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-md);
        background: var(--color-surface-1, var(--color-surface-2));
      }

      .status-head {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-3);
        min-block-size: 32px;
      }

      .status-text {
        font-size: var(--text-sm);
        color: var(--color-text-secondary);
        flex: 1;
      }

      .progress {
        position: absolute;
        inset-inline: 0;
        inset-block-end: -6px;
        block-size: 2px;
        border-radius: 2px;
        background: var(--color-surface-3);
        overflow: hidden;
      }

      .bar {
        display: block;
        block-size: 100%;
        inline-size: 35%;
        background: var(--color-accent);
        animation: slide 1.1s var(--ease-standard) infinite;
      }

      @keyframes slide {
        from {
          transform: translateX(calc(var(--dir-sign) * -100%));
        }
        to {
          transform: translateX(calc(var(--dir-sign) * 290%));
        }
      }

      .chips {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-2);
        margin-block-start: var(--space-4);
      }

      .chip {
        display: inline-flex;
        align-items: center;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-full);
        font-size: var(--text-xs);
        color: var(--color-text-secondary);
        background: transparent;
        overflow: hidden;
      }

      .chip.is-active {
        border-color: var(--color-accent);
        background: var(--color-accent-soft, var(--color-surface-3));
      }

      .chip.is-failed {
        border-color: var(--color-danger);
      }

      .chip.is-setup {
        border-style: dashed;
        gap: var(--space-2);
        padding-inline-start: 10px;
      }

      .chip-main,
      .chip-retry,
      .chip-setup {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        block-size: 26px;
        padding-inline: 10px;
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
        border-inline-start: 1px solid var(--color-border);
        color: var(--color-danger);
      }

      .chip-setup {
        color: var(--color-accent);
        text-decoration: underline;
      }

      .chip-main:focus-visible,
      .chip-retry:focus-visible,
      .chip-setup:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
      }

      .chip-name {
        font-weight: var(--weight-medium);
        color: var(--color-text);
      }

      .chip-dot {
        inline-size: 7px;
        block-size: 7px;
        border-radius: 50%;
        background: var(--color-success);
      }

      .chip.is-busy .chip-dot {
        background: var(--color-accent);
        animation: pulse 1.1s ease-in-out infinite;
      }

      .chip.is-failed .chip-dot {
        background: var(--color-danger);
      }

      .chip.is-stopped .chip-dot {
        background: var(--color-text-muted);
      }

      @keyframes pulse {
        50% {
          opacity: 0.25;
        }
      }

      .plan {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--space-2);
        margin-block-start: var(--space-3);
        font-size: var(--text-sm);
        color: var(--color-text-secondary);
      }

      .plan app-icon {
        color: var(--color-accent);
      }

      .query {
        padding: 1px 8px;
        border-radius: var(--radius-full);
        background: var(--color-surface-3);
      }

      .summary {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-4);
        margin-block-start: var(--space-5);
        font-size: var(--text-sm);
        color: var(--color-text-secondary);
      }

      .count {
        font-weight: var(--weight-semibold);
        color: var(--color-text);
      }

      .warn {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        margin-block-start: var(--space-2);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
      }

      .results {
        position: relative;
        list-style: none;
        margin: var(--space-4) 0 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
      }

      .card {
        display: flex;
        gap: var(--space-3);
        padding: var(--space-3) var(--space-4);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-md);
        background: var(--color-surface-1, var(--color-surface-2));
      }

      .card.is-selected {
        border-color: var(--color-accent);
        background: var(--color-surface-2);
      }

      .pick {
        padding-block-start: 2px;
      }

      .page-mark {
        color: var(--color-text-muted);
        inline-size: 16px;
      }

      .pick input {
        inline-size: 16px;
        block-size: 16px;
        accent-color: var(--color-accent);
        cursor: pointer;
      }

      .card-body {
        flex: 1;
        min-inline-size: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
      }

      .card-title {
        font-size: var(--text-md);
        font-weight: var(--weight-semibold);
        line-height: var(--leading-snug, 1.35);
        overflow-wrap: anywhere;
      }

      .meta {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-2) var(--space-3);
        font-size: var(--text-xs);
        color: var(--color-text-muted);
      }

      .badge {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 1px 8px;
        border-radius: var(--radius-full);
        font-weight: var(--weight-semibold);
        border: 1px solid var(--color-border-strong);
        color: var(--color-text-secondary);
      }

      .badge.source {
        border-color: var(--color-accent);
        color: var(--color-accent);
      }

      .badge.seen {
        border-color: var(--color-success);
        color: var(--color-success);
      }

      .badge.rights-public-domain {
        border-color: var(--color-success);
        color: var(--color-success);
      }

      .badge.rights-cc {
        border-color: var(--color-accent);
        color: var(--color-text);
      }

      .badge.rights-open-source {
        border-color: var(--color-info, var(--color-accent));
        color: var(--color-text);
      }

      .badge.rights-unknown {
        border-style: dashed;
        font-weight: var(--weight-medium);
        color: var(--color-text-muted);
      }

      .score {
        margin-inline-start: auto;
        color: var(--color-accent);
        font-weight: var(--weight-semibold);
      }

      .url {
        font-size: var(--text-xs);
        color: var(--color-text-muted);
        overflow-wrap: anywhere;
        text-align: start;
      }

      .why {
        display: flex;
        align-items: flex-start;
        gap: var(--space-2);
        font-size: var(--text-sm);
        color: var(--color-text-secondary);
        line-height: var(--leading-normal);
      }

      .why app-icon {
        color: var(--color-accent);
        margin-block-start: 2px;
        flex-shrink: 0;
      }

      .summary-text {
        font-size: var(--text-sm);
        color: var(--color-text-muted);
        line-height: var(--leading-normal);
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
      }

      .page-actions {
        display: flex;
        gap: var(--space-2);
        margin-block-start: var(--space-1);
      }

      .bar-actions {
        position: sticky;
        inset-block-end: 0;
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: var(--space-3);
        margin-block-start: var(--space-4);
        padding: var(--space-3) var(--space-4);
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-md);
        background: var(--color-surface-2);
        box-shadow: 0 -6px 18px rgb(0 0 0 / 0.12);
      }

      .bar-left,
      .bar-right {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-3);
      }

      .mini {
        display: inline-flex;
        align-items: center;
        gap: var(--space-2);
        font-size: var(--text-xs);
        color: var(--color-text-muted);
      }

      .input.small {
        block-size: 30px;
        inline-size: auto;
        min-inline-size: 120px;
      }

      .check-mini input {
        accent-color: var(--color-accent);
      }

      .policy {
        margin-block-start: var(--space-10, 48px);
        padding-block-start: var(--space-3);
        border-block-start: 1px solid var(--color-border);
        color: var(--color-text-muted);
        font-size: var(--text-xs);
        line-height: var(--leading-normal);
      }

      @media (prefers-reduced-motion: reduce) {
        .bar {
          animation: none;
          inline-size: 100%;
          opacity: 0.5;
        }

        .chip.is-busy .chip-dot {
          animation: none;
        }
      }

      @media (max-width: 760px) {
        .page {
          padding-inline: var(--space-4);
        }

        .search {
          flex-direction: column;
        }
      }
    `,
  ],
})
export class ResearchPageComponent implements OnInit, AfterViewChecked {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly fmt = inject(FormatService);
  protected readonly queues = inject(QueuesService);
  protected readonly session = inject(ResearchSession);
  private readonly router = inject(Router);
  private readonly toasts = inject(ToastService);
  private readonly ui = inject(UiState);

  protected readonly categoryIds = CATEGORY_IDS;
  protected readonly fileCategories = CATEGORY_KEYS;
  protected readonly emptySet: ReadonlySet<string> = new Set<string>();

  protected readonly topic = signal('');
  protected readonly category = signal<CategoryId>('all');
  protected readonly useAi = signal(true);
  protected readonly adding = signal(false);
  protected readonly selected = signal<ReadonlySet<string>>(this.emptySet);
  protected readonly preview = signal<readonly AiMessage[]>([]);
  protected readonly queueId = signal<number | null>(null);
  protected readonly saveAs = signal('');
  protected readonly later = signal(false);

  /** The AI is set up (provider + key + model). */
  protected readonly aiOn = computed(() => this.ai.status()?.configured === true);

  private readonly list = viewChild<ElementRef<HTMLUListElement>>('list');
  private lastTops = new Map<string, number>();
  private lastSignature = '';

  /** One line under the progress bar: what is going on right now. */
  protected readonly statusText = computed(() => {
    const total = this.session.sources().length;
    const done = this.session.sources().filter((s) => s.state !== 'searching').length;
    const phase = this.session.phase();
    if (phase === 'cancelled') {
      return this.i18n.t('research.cancelled');
    }
    if (phase === 'ranking') {
      return this.i18n.t('research.ai.ranking').replace('{n}', this.fmt.int(this.session.rankedCount()));
    }
    if (phase === 'searching') {
      const base = this.i18n.t('research.progress').replace('{done}', this.fmt.int(done)).replace('{total}', this.fmt.int(total));
      return this.session.aiActive() && this.session.plan().length === 0 ? `${base} · ${this.i18n.t('research.ai.planning')}` : base;
    }
    const a = this.session.ai();
    if (a?.used) {
      return `${this.i18n.t('research.done')} · ${this.i18n.t('research.ai.used').replace('{n}', this.fmt.int(a.tokens))}`;
    }
    return this.i18n.t('research.done');
  });

  ngOnInit(): void {
    if (this.ai.available) {
      void this.ai.refresh();
    }
  }

  /**
   * Moves the cards smoothly when the order changes (FLIP): each card that moved starts at its old
   * position and slides to the new one; new cards fade in. Nothing else shifts, and with reduced
   * motion the list simply re-orders.
   */
  ngAfterViewChecked(): void {
    const ul = this.list()?.nativeElement;
    if (!ul) {
      this.lastTops.clear();
      this.lastSignature = '';
      return;
    }
    const signature = this.session
      .ordered()
      .map((i) => i.id)
      .join('|');
    if (signature === this.lastSignature) {
      return;
    }
    this.lastSignature = signature;
    const reduce = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
    const cards = Array.from(ul.children) as HTMLElement[];
    const next = new Map<string, number>();
    const animate = !reduce && cards.length <= 200;
    for (const el of cards) {
      const id = el.dataset['id'];
      if (!id) {
        continue;
      }
      const top = el.offsetTop;
      next.set(id, top);
      if (!animate || typeof el.animate !== 'function') {
        continue;
      }
      const prev = this.lastTops.get(id);
      if (prev !== undefined && prev !== top) {
        el.animate([{ transform: `translateY(${prev - top}px)` }, { transform: 'none' }], { duration: 320, easing: 'cubic-bezier(0.2, 0.7, 0.2, 1)' });
      } else if (prev === undefined && this.lastTops.size > 0) {
        el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 220 });
      }
    }
    this.lastTops = next;
  }

  /** Settings, scrolled to the AI section. */
  protected openAiSettings(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('ai')?.scrollIntoView({ block: 'start' }), 50);
    });
  }

  protected openResearchSettings(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('research-sources')?.scrollIntoView({ block: 'start' }), 50);
    });
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

  protected sourceLabel(s: SourceId): string {
    return this.i18n.t(`research.source.${s}`);
  }

  protected rightsText(it: ResultItem): string {
    switch (it.rights.kind) {
      case 'public-domain':
        return this.i18n.t('research.rights.public-domain');
      case 'cc':
        return it.rights.label;
      case 'open-source':
        return this.i18n.t('research.rights.open-source').replace('{label}', it.rights.label);
      default:
        return this.i18n.t('research.rights.unknown');
    }
  }

  protected byline(it: ResultItem): string {
    const who = it.authors.slice(0, 3).join(', ') + (it.authors.length > 3 ? ' …' : '');
    return [who, it.year === null ? '' : String(it.year)].filter((p) => p.length > 0).join(' · ');
  }

  protected pageUrl(it: ResultItem): string {
    const safe = safeWebUrl(it.downloadUrl);
    return safe === null ? '' : shortUrl(safe);
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
        return this.i18n.t('research.state.done').replace('{n}', this.fmt.int(s.found));
    }
  }

  protected chipLabel(s: SourceStatus): string {
    return `${this.sourceLabel(s.id)}: ${this.chipState(s)}`;
  }

  protected toggleOnly(id: SourceId): void {
    this.session.onlySource.set(this.session.onlySource() === id ? null : id);
  }

  protected retry(id: SourceId): void {
    void this.session.retry(id).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected cancel(): void {
    void this.session.cancel();
  }

  protected pickCategory(c: CategoryId): void {
    if (this.category() === c) {
      return;
    }
    this.category.set(c);
    if (this.topic().trim().length > 0 && this.session.phase() !== 'idle') {
      void this.run();
    }
  }

  protected onTabKey(event: KeyboardEvent): void {
    const dir = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
    if (dir === 0) {
      return;
    }
    // arrows follow the reading direction
    const flip = this.i18n.dir() === 'rtl' ? -1 : 1;
    const i = CATEGORY_IDS.indexOf(this.category());
    const next = CATEGORY_IDS[(i + dir * flip + CATEGORY_IDS.length) % CATEGORY_IDS.length];
    event.preventDefault();
    this.pickCategory(next);
    setTimeout(() => document.getElementById(`rtab-${next}`)?.focus(), 0);
  }

  protected toggle(id: string): void {
    const next = new Set(this.selected());
    if (!next.delete(id)) {
      next.add(id);
    }
    this.selected.set(next);
  }

  protected selectAll(): void {
    this.selected.set(new Set(this.session.ordered().filter((i) => i.kind === 'file').map((i) => i.id)));
  }

  protected onPreviewToggle(event: Event): void {
    if ((event.target as HTMLDetailsElement).open) {
      void this.loadPreview();
    }
  }

  private async loadPreview(): Promise<void> {
    try {
      this.preview.set(await this.ai.previewSearch(this.topic().trim() || '…', this.category()));
    } catch {
      this.preview.set([]);
    }
  }

  protected search(event: Event): void {
    event.preventDefault();
    void this.run();
  }

  private async run(): Promise<void> {
    const topic = this.topic().trim();
    if (topic.length === 0 || this.session.busy()) {
      if (this.session.busy()) {
        await this.session.cancel();
      } else {
        return;
      }
    }
    this.selected.set(this.emptySet);
    try {
      await this.session.start({
        topic,
        useAi: this.useAi() && this.aiOn(),
        lang: this.i18n.lang(),
        category: this.category(),
      });
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }

  /** A web result: the site grabber opens on its page and lists the files (robots.txt obeyed). */
  protected findFiles(it: ResultItem): void {
    const url = safeWebUrl(it.downloadUrl);
    if (url === null) {
      return;
    }
    this.ui.grabberPreset.set(url);
    this.ui.openGrabber();
  }

  protected copyLink(it: ResultItem): void {
    const url = safeWebUrl(it.downloadUrl);
    if (url === null) {
      return;
    }
    void navigator.clipboard
      ?.writeText(url)
      .then(() => this.toasts.show('success', this.i18n.t('research.linkCopied')))
      .catch(() => undefined);
  }

  protected async addSelected(): Promise<void> {
    const ids = [...this.selected()];
    if (ids.length === 0 || this.adding()) {
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
      const text =
        r.skipped.length === 0
          ? this.i18n.t('research.added').replace('{n}', this.fmt.int(r.added.length))
          : this.i18n.t('research.addedSome').replace('{n}', this.fmt.int(r.added.length)).replace('{skipped}', this.fmt.int(r.skipped.length));
      this.toasts.show(r.added.length > 0 ? 'success' : 'danger', r.added.length > 0 ? text : this.i18n.t('research.addFailed'));
      this.selected.set(this.emptySet);
      await this.recheckSeen();
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.adding.set(false);
    }
  }

  /** After adding, the cards of what was just added are not "downloaded" yet; refresh the badges. */
  private async recheckSeen(): Promise<void> {
    const items = this.session.items();
    if (items.length === 0) {
      return;
    }
    try {
      const matches = await this.ai.seenCheck(
        items.map((i) => ({
          url: i.downloadUrl,
          doi: i.doi ?? undefined,
          arxiv: i.arxiv ?? undefined,
          pmid: i.pmid ?? undefined,
          pmcid: i.pmcid ?? undefined,
          filename: i.size !== null ? (i.filename ?? undefined) : undefined,
          size: i.size ?? undefined,
        })),
      );
      const byId = new Map(items.map((it, n) => [it.id, matches[n] ?? null] as const));
      this.session.replaceSeen((it) => ({ ...it, seen: byId.get(it.id) ?? it.seen }));
    } catch {
      /* the badges stay as they were */
    }
  }
}
