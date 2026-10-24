import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, OnInit, signal, untracked, viewChild } from '@angular/core';
import { Router } from '@angular/router';

import { AiSetupDialogComponent } from '../../ai/ai-setup-dialog.component';
import { AiService } from '../../ai/ai.service';
import { CATEGORY_IDS, CategoryId, SearchMode, SourceId } from '../../ai/research.models';
import { ResearchSession } from '../../ai/research.session';
import { MotionService } from '../../core/motion.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { emptyTurn, keepItems, MAX_RESULTS, promptKey, Turn, turnAi } from './research-history';
import { ResearchHistoryService } from './research-history.service';
import { ResearchComposerComponent } from './research-composer.component';
import { SIDE_DEFAULT, SIDE_MAX, SIDE_MIN, ResearchLayoutService } from './research-layout.service';
import { CATEGORY_ICONS, chooseCategory, chooseMode } from './research-modes';
import { ResearchSidebarComponent } from './research-sidebar.component';
import { RerunRequest, ResearchTurnComponent } from './research-turn.component';

/** How long the results of an identical prompt are reused instead of searching again. */
const REUSE_MS = 10 * 60 * 1000;

/** Arrow keys on the sidebar's resize handle move it this far (Shift: three times as far). */
const KEY_STEP = 16;

/** The turn the running search belongs to. */
interface LiveRef {
  readonly conversationId: string;
  readonly turnId: string;
}

/**
 * Research as a conversation. The left list holds the conversations (kept on this machine only);
 * each prompt is a turn that shows the real progress of its search, a short summary and the result
 * cards. One search runs at a time and each sends only its own prompt (no earlier turns go to the
 * AI). The search itself is the unchanged `ResearchSession`; this page mirrors its signals into the
 * stored turn, so a running turn and one restored from history are rendered by the same component.
 *
 * A search needs a category, chosen on purpose: a new conversation starts without one (the one used
 * last is only suggested). Video Search is tied to Videos.
 */
@Component({
  selector: 'app-research-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AiSetupDialogComponent, ButtonComponent, IconComponent, ResearchComposerComponent, ResearchSidebarComponent, ResearchTurnComponent],
  providers: [ResearchSession],
  template: `
    @if (!ai.available) {
      <div class="solo">
        <h1 class="title">{{ i18n.t('research.title') }}</h1>
        <p class="muted">{{ i18n.t('research.desktopOnly') }}</p>
      </div>
    } @else {
      <div class="shell" [class.is-collapsed]="!layout.sideOpen()" [class.is-side-open]="drawerOpen()" [style.--side-w]="layout.sideWidth() + 'px'">
        <aside class="sidebar" id="research-side">
          <app-research-sidebar
            [conversations]="history.conversations()"
            [activeId]="history.activeId()"
            (created)="newConversation()"
            (selected)="open($event)"
            (renamed)="history.rename($event.id, $event.title)"
            (deleted)="remove($event)"
            (cleared)="clearAll()"
          />
          @if (layout.sideOpen()) {
            <div
              class="handle"
              role="separator"
              aria-orientation="vertical"
              tabindex="0"
              [attr.aria-label]="i18n.t('research.history.resize')"
              [attr.aria-valuemin]="sideMin"
              [attr.aria-valuemax]="sideMax"
              [attr.aria-valuenow]="layout.sideWidth()"
              (pointerdown)="startDrag($event)"
              (pointermove)="onDrag($event)"
              (pointerup)="endDrag()"
              (pointercancel)="endDrag()"
              (keydown)="onHandleKey($event)"
              (dblclick)="layout.setSideWidth(sideDefault)"
            ></div>
          }
        </aside>

        <main class="main">
          <header class="head">
            <button appButton variant="ghost" icon="panelStart" [iconOnly]="true" type="button" class="side-toggle is-wide" aria-controls="research-side" [attr.aria-expanded]="layout.sideOpen()" [attr.aria-label]="i18n.t('research.history')" [attr.title]="i18n.t('research.history')" (click)="layout.toggleSide()"></button>
            <button appButton variant="ghost" icon="list" [iconOnly]="true" type="button" class="side-toggle is-narrow" aria-controls="research-side" [attr.aria-expanded]="drawerOpen()" [attr.aria-label]="i18n.t('research.history')" (click)="drawerOpen.set(!drawerOpen())"></button>
            <div class="head-text">
              <h1 class="title">{{ history.active()?.title || i18n.t('research.title') }}</h1>
              <p class="rights" role="note"><app-icon name="shieldCheck" [size]="14" /> {{ i18n.t('research.notice') }}</p>
            </div>
          </header>

          <div class="stage" [class.is-empty]="empty()">
            <div class="thread" #thread>
              @if (empty()) {
                <div class="hello" [class.is-animated]="animate()">
                  <div class="mark-wrap">
                    @if (animate()) {
                      <span class="ripple r1" aria-hidden="true"></span>
                      <span class="ripple r2" aria-hidden="true"></span>
                    }
                    <img class="mark" src="brand-mark.svg" alt="" width="64" height="64" />
                  </div>
                  <h2 class="hello-title">{{ i18n.t('research.hello') }}</h2>
                  <p class="muted">{{ i18n.t('research.hello.sub') }}</p>
                </div>
              } @else {
                <div class="column">
                  @for (t of turns(); track t.id) {
                    <app-research-turn
                      [turn]="t"
                      [live]="liveRef()?.turnId === t.id"
                      [addable]="history.isLive(t.id)"
                      (stopSearch)="cancel()"
                      (retrySource)="retrySource($event)"
                      (rerun)="rerun(t, $event)"
                      (added)="recheckSeen(t.id)"
                    />
                  }
                </div>
              }
            </div>

            <div class="dock">
              <div class="column">
                <app-research-composer
                  #composer
                  [category]="category()"
                  [mode]="mode()"
                  [(useAi)]="useAi"
                  [aiReady]="aiOn()"
                  [providerLabel]="providerLabel()"
                  [busy]="session.busy()"
                  [note]="noteText()"
                  [placement]="empty() ? 'down' : 'up'"
                  [compact]="!empty()"
                  (send)="send($event)"
                  (stop)="cancel()"
                  (pickMode)="pickMode($event)"
                  (pickCategory)="pickCategory($event)"
                  (openAiSettings)="openAiSettings()"
                  (setupAi)="setupOpen.set(true)"
                />
                @if (!empty()) {
                  <p class="policy is-line" [attr.title]="i18n.t('research.footer')">{{ i18n.t('research.footer') }}</p>
                }
              </div>
            </div>

            @if (empty()) {
              <div class="cats column">
                <p class="cats-title" id="research-cats">{{ i18n.t('research.categories') }}</p>
                <div class="cat-grid" role="group" aria-labelledby="research-cats">
                  @for (c of categoryIds; track c) {
                    <button type="button" class="cat" [class.is-on]="category() === c" [class.is-suggested]="category() === null && layout.lastCategory() === c" [attr.aria-pressed]="category() === c" (click)="chooseCard(c)">
                      <app-icon [name]="categoryIcons[c]" [size]="18" />
                      <span class="cat-text">
                        <span class="cat-name">{{ i18n.t('research.cat.' + c) }}</span>
                        <span class="cat-desc">{{ i18n.t('research.catDesc.' + c) }}</span>
                      </span>
                      @if (category() === null && layout.lastCategory() === c) {
                        <span class="cat-tag">{{ i18n.t('research.lastUsed') }}</span>
                      }
                    </button>
                  }
                </div>
                <p class="policy">{{ i18n.t('research.footer') }}</p>
              </div>
            }
          </div>
        </main>
      </div>
      <app-ai-setup-dialog [open]="setupOpen()" (closed)="setupOpen.set(false)" />
    }
  `,
  styles: `
    :host {
      display: block;
      block-size: 100%;
      overflow: hidden;
    }

    .solo {
      max-inline-size: 640px;
      margin: var(--space-10, 48px) auto;
      padding-inline: var(--space-6);
    }

    .shell {
      display: grid;
      grid-template-columns: var(--side-w, 264px) minmax(0, 1fr);
      block-size: 100%;
    }

    .shell.is-collapsed {
      grid-template-columns: minmax(0, 1fr);
    }

    .shell.is-collapsed .sidebar {
      display: none;
    }

    .sidebar {
      position: relative;
      border-inline-end: 1px solid var(--color-border);
      background: var(--color-surface-1, var(--color-surface-2));
      min-block-size: 0;
      min-inline-size: 0;
    }

    /* the drag handle straddles the sidebar's edge; it is also a keyboard separator */
    .handle {
      position: absolute;
      inset-block: 0;
      inset-inline-end: -4px;
      inline-size: 8px;
      z-index: 2;
      cursor: col-resize;
      touch-action: none;
    }

    .handle::after {
      content: '';
      position: absolute;
      inset-block: 0;
      inset-inline-start: 3px;
      inline-size: 2px;
      background: transparent;
      transition: background-color var(--dur-1) var(--ease-standard);
    }

    .handle:hover::after,
    .handle:focus-visible::after {
      background: var(--color-accent);
    }

    .handle:focus-visible {
      outline: none;
    }

    .main {
      display: flex;
      flex-direction: column;
      min-inline-size: 0;
      min-block-size: 0;
    }

    .head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-6);
      border-block-end: 1px solid var(--color-border);
    }

    .head-text {
      min-inline-size: 0;
    }

    .title {
      font-size: var(--text-xl, 1.25rem);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .muted {
      color: var(--color-text-muted);
      line-height: var(--leading-normal);
    }

    .rights {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .rights app-icon {
      color: var(--color-accent-fg);
      flex-shrink: 0;
    }

    .side-toggle.is-narrow {
      display: none;
    }

    /* one scroll container: the thread scrolls, the composer stays; on the empty page the stage scrolls instead */
    .stage {
      flex: 1;
      min-block-size: 0;
      display: flex;
      flex-direction: column;
    }

    .thread {
      flex: 1;
      min-block-size: 0;
      overflow-y: auto;
      padding: var(--space-6);
      scroll-padding-block-end: var(--space-6);
    }

    .stage.is-empty {
      overflow-y: auto;
      padding-inline: var(--space-6);
    }

    .stage.is-empty .thread {
      flex: none;
      overflow: visible;
      padding: 0;
      margin-block-start: auto;
    }

    .column {
      max-inline-size: 900px;
      margin-inline: auto;
      inline-size: 100%;
      display: flex;
      flex-direction: column;
      gap: var(--space-8, 32px);
    }

    .dock {
      padding: var(--space-3) var(--space-6) var(--space-4);
      border-block-start: 1px solid var(--color-border);
      background: var(--color-bg, var(--color-surface-1));
    }

    .dock .column {
      gap: var(--space-2);
    }

    .stage.is-empty .dock {
      padding: var(--space-6) 0 var(--space-4);
      border-block-start: 0;
      background: transparent;
    }

    .stage.is-empty .dock .column {
      max-inline-size: 840px;
    }

    .stage.is-empty .cats {
      max-inline-size: 880px;
    }

    .policy {
      margin: 0;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .policy.is-line {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .hello {
      padding-block-start: var(--space-2);
      text-align: center;
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: var(--space-2);
    }

    .mark-wrap {
      position: relative;
      display: grid;
      place-items: center;
      inline-size: 64px;
      block-size: 64px;
      margin-block-end: var(--space-2);
    }

    .mark {
      position: relative;
      display: block;
      inline-size: 64px;
      block-size: 64px;
    }

    /* the logo is shown exactly as the brand kit draws it; only its position and opacity move */
    .is-animated .mark {
      animation: mark-in var(--dur-4) var(--ease-emphasized-out) both;
    }

    /* a ripple behind the mark, played once: it spreads out like a wave and fades */
    .ripple {
      position: absolute;
      inset: 0;
      border-radius: 22px;
      border: 1.5px solid var(--color-accent);
      opacity: 0;
      animation: ripple 1400ms var(--ease-emphasized-out) both;
    }

    .ripple.r1 {
      animation-delay: 220ms;
    }

    .ripple.r2 {
      animation-delay: 520ms;
    }

    @keyframes mark-in {
      from {
        opacity: 0;
        transform: translateY(8px) scale(0.92);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    @keyframes ripple {
      from {
        opacity: 0.5;
        transform: scale(0.95);
      }

      to {
        opacity: 0;
        transform: scale(1.7);
      }
    }

    .is-animated .hello-title,
    .is-animated .muted {
      animation: text-in var(--dur-4) var(--ease-emphasized-out) 160ms both;
    }

    @keyframes text-in {
      from {
        opacity: 0;
        transform: translateY(6px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .hello-title {
      margin: 0;
      font-size: var(--text-2xl);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
    }

    .hello .muted {
      max-inline-size: 52ch;
      font-size: var(--text-sm);
    }

    .cats {
      margin-block-end: auto;
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      padding-block-end: var(--space-4);
    }

    .cats-title {
      margin: 0;
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
    }

    .cat-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
      gap: var(--space-2);
    }

    .cat {
      position: relative;
      display: flex;
      align-items: flex-start;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1, var(--color-surface-2));
      color: var(--color-text);
      font: inherit;
      text-align: start;
      cursor: pointer;
      transition:
        border-color var(--dur-1) var(--ease-standard),
        background-color var(--dur-1) var(--ease-standard);
    }

    .cat app-icon {
      flex: none;
      margin-block-start: 1px;
      color: var(--color-text-secondary);
    }

    .cat:hover {
      border-color: var(--color-border-strong);
      background: var(--color-surface-2);
    }

    .cat:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .cat.is-on {
      border-color: var(--color-accent);
      background: var(--color-accent-soft);
    }

    .cat.is-on app-icon {
      color: var(--color-accent-fg);
    }

    .cat.is-suggested {
      border-style: dashed;
      border-color: var(--color-accent);
    }

    .cat-text {
      display: flex;
      flex-direction: column;
      min-inline-size: 0;
    }

    .cat-name {
      font-size: var(--text-sm);
      font-weight: var(--weight-semibold);
    }

    .cat-desc {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .cat-tag {
      position: absolute;
      inset-block-start: var(--space-2);
      inset-inline-end: var(--space-2);
      padding: 0 6px;
      border-radius: var(--radius-full);
      background: var(--color-accent-soft);
      color: var(--color-accent-fg);
      font-size: 10px;
      font-weight: var(--weight-medium);
    }

    @media (max-width: 900px) {
      .shell,
      .shell.is-collapsed {
        grid-template-columns: minmax(0, 1fr);
        position: relative;
      }

      .shell.is-collapsed .sidebar {
        display: block;
      }

      .sidebar {
        position: absolute;
        inset-block: 0;
        inset-inline-start: 0;
        z-index: 5;
        inline-size: min(300px, 86%);
        transform: translateX(calc(var(--dir-sign) * -100%));
        visibility: hidden;
        box-shadow: 0 0 24px rgb(0 0 0 / 0.3);
        transition:
          transform var(--motion-fast) var(--ease-standard),
          visibility 0s linear var(--motion-fast);
      }

      .is-side-open .sidebar {
        transform: none;
        visibility: visible;
        transition-delay: 0s;
      }

      .handle {
        display: none;
      }

      .side-toggle.is-wide {
        display: none;
      }

      .side-toggle.is-narrow {
        display: inline-flex;
      }

      .head,
      .thread,
      .dock {
        padding-inline: var(--space-4);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .sidebar {
        transition: none;
      }
    }
  `,
})
export class ResearchPageComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly history = inject(ResearchHistoryService);
  protected readonly layout = inject(ResearchLayoutService);
  protected readonly session = inject(ResearchSession);
  private readonly motion = inject(MotionService);
  private readonly router = inject(Router);
  private readonly toasts = inject(ToastService);

  private readonly thread = viewChild<ElementRef<HTMLElement>>('thread');
  private readonly composer = viewChild<ResearchComposerComponent>('composer');

  protected readonly categoryIds = CATEGORY_IDS;
  protected readonly categoryIcons = CATEGORY_ICONS;
  protected readonly sideMin = SIDE_MIN;
  protected readonly sideMax = SIDE_MAX;
  protected readonly sideDefault = SIDE_DEFAULT;

  /** `null` until the user picks one: a new conversation never starts with a category chosen for it. */
  protected readonly category = signal<CategoryId | null>(null);
  protected readonly mode = signal<SearchMode>('quick-search');
  /** Translation key of the line explaining what a choice changed. */
  private readonly noteKey = signal<string | null>(null);
  protected readonly noteText = computed(() => {
    const key = this.noteKey();
    return key === null ? null : this.i18n.t(key);
  });
  protected readonly useAi = signal(true);
  protected readonly drawerOpen = signal(false);
  protected readonly setupOpen = signal(false);
  protected readonly liveRef = signal<LiveRef | null>(null);

  /** The entrance of the empty page plays unless the user asked for reduced motion. */
  protected readonly animate = computed(() => !this.motion.reduced());

  /** The AI is set up (provider + key + model). */
  protected readonly aiOn = computed(() => this.ai.status()?.configured === true);
  protected readonly turns = computed<readonly Turn[]>(() => this.history.active()?.turns ?? []);
  protected readonly empty = computed(() => this.turns().length === 0);
  /** `Provider · model` for the indicator next to the AI switch. */
  protected readonly providerLabel = computed(() => {
    const s = this.ai.status();
    if (!s) {
      return '';
    }
    const name = s.providers.find((p) => p.id === s.settings.provider)?.name ?? s.settings.provider;
    return s.settings.model ? `${name} · ${s.settings.model}` : name;
  });

  /** Where each conversation was scrolled to, so switching back finds the same place. */
  private readonly scrollAt = new Map<string, number>();
  private view: { id: string | null; n: number } = { id: null, n: 0 };
  private drag: { x: number; w: number } | null = null;

  constructor() {
    // mirror the running search into its turn, whenever the search changes
    this.session.watch(() => this.mirror());
    // a new turn comes into view; another conversation comes back to where it was left
    effect(() => {
      const id = this.history.activeId();
      const n = this.turns().length;
      const before = untracked(() => this.view);
      this.view = { id, n };
      if (n === 0) {
        return;
      }
      setTimeout(() => {
        const el = this.thread()?.nativeElement;
        if (!el) {
          return;
        }
        if (id !== before.id) {
          el.scrollTo({ top: (id !== null ? this.scrollAt.get(id) : undefined) ?? el.scrollHeight });
        } else if (n > before.n) {
          el.scrollTo({ top: el.scrollHeight });
        }
      }, 0);
    });
  }

  /** Copies what the session knows into the turn it belongs to (the stored turn is what the page renders). */
  private mirror(): void {
    const ref = this.liveRef();
    const phase = this.session.phase();
    if (ref === null || phase === 'idle') {
      return;
    }
    const status = phase === 'done' ? 'done' : phase === 'cancelled' ? 'cancelled' : 'running';
    this.history.patchTurn(ref.conversationId, ref.turnId, {
      phase,
      status,
      sources: this.session.sources(),
      unavailable: this.session.unavailable().map((u) => u.source),
      plan: this.session.plan(),
      queries: this.session.queries(),
      comparison: this.session.comparison(),
      summary: this.session.summary(),
      ai: turnAi(this.session.ai(), (o) => (o.error ? this.ai.failText(o.error) : '')),
      rankedCount: this.session.rankedCount(),
      items: keepItems(this.session.ordered(), MAX_RESULTS),
    });
  }

  ngOnInit(): void {
    if (this.ai.available) {
      void this.ai.refresh();
    }
  }

  // ---------------------------------------------------------------- choices

  protected pickMode(mode: SearchMode): void {
    const s = chooseMode(mode, this.category());
    this.mode.set(s.mode);
    this.category.set(s.category);
    this.noteKey.set(s.note);
  }

  protected pickCategory(category: CategoryId): void {
    const s = chooseCategory(category, this.mode());
    this.mode.set(s.mode);
    this.category.set(s.category);
    this.noteKey.set(s.note);
  }

  /** A card on the empty page: choosing is explicit, then the cursor goes to the prompt. */
  protected chooseCard(category: CategoryId): void {
    this.pickCategory(category);
    this.composer()?.focus();
  }

  /** The conversation's own context: what its last search used, or nothing chosen yet. */
  private restoreContext(turns: readonly Turn[]): void {
    const last = turns[turns.length - 1];
    this.category.set(last?.category ?? null);
    this.mode.set(last?.mode ?? 'quick-search');
    this.noteKey.set(null);
  }

  // ---------------------------------------------------------------- conversations

  private saveScroll(): void {
    const id = this.history.activeId();
    const el = this.thread()?.nativeElement;
    if (id !== null && el) {
      this.scrollAt.set(id, el.scrollTop);
    }
  }

  protected newConversation(): void {
    this.saveScroll();
    this.history.select(null);
    this.drawerOpen.set(false);
    this.composer()?.clear();
    this.restoreContext([]);
    this.composer()?.focus();
  }

  protected open(id: string): void {
    if (id === this.history.activeId()) {
      this.drawerOpen.set(false);
      return;
    }
    this.saveScroll();
    this.history.select(id);
    this.drawerOpen.set(false);
    this.restoreContext(this.turns());
  }

  protected remove(id: string): void {
    this.stopIfLive(id);
    const wasActive = this.history.activeId() === id;
    this.history.remove(id);
    this.scrollAt.delete(id);
    if (wasActive) {
      this.restoreContext([]);
    }
  }

  protected clearAll(): void {
    this.stopIfLive(null);
    this.history.clearAll();
    this.scrollAt.clear();
    this.restoreContext([]);
  }

  /** Deleting a conversation stops its search (`null`: whatever is running). */
  private stopIfLive(conversationId: string | null): void {
    const ref = this.liveRef();
    if (ref !== null && (conversationId === null || ref.conversationId === conversationId)) {
      if (this.session.busy()) {
        void this.session.cancel().catch(() => undefined);
      }
      this.session.reset();
      this.liveRef.set(null);
    }
  }

  // ---------------------------------------------------------------- sidebar width

  protected startDrag(event: PointerEvent): void {
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    this.drag = { x: event.clientX, w: this.layout.sideWidth() };
  }

  protected onDrag(event: PointerEvent): void {
    if (this.drag === null) {
      return;
    }
    // the sidebar is on the right in RTL, so dragging left widens it
    const sign = this.i18n.dir() === 'rtl' ? -1 : 1;
    this.layout.setSideWidth(this.drag.w + (event.clientX - this.drag.x) * sign, false);
  }

  protected endDrag(): void {
    if (this.drag !== null) {
      this.drag = null;
      this.layout.setSideWidth(this.layout.sideWidth());
    }
  }

  protected onHandleKey(event: KeyboardEvent): void {
    const sign = this.i18n.dir() === 'rtl' ? -1 : 1;
    const step = event.shiftKey ? KEY_STEP * 3 : KEY_STEP;
    switch (event.key) {
      case 'ArrowRight':
        this.layout.setSideWidth(this.layout.sideWidth() + sign * step);
        break;
      case 'ArrowLeft':
        this.layout.setSideWidth(this.layout.sideWidth() - sign * step);
        break;
      case 'Home':
        this.layout.setSideWidth(SIDE_MIN);
        break;
      case 'End':
        this.layout.setSideWidth(SIDE_MAX);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  // ---------------------------------------------------------------- searching

  protected openAiSettings(): void {
    void this.router.navigateByUrl('/settings').then(() => {
      setTimeout(() => document.getElementById('ai')?.scrollIntoView({ block: 'start' }), 50);
    });
  }

  protected cancel(): void {
    void this.session.cancel().catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected retrySource(source: SourceId): void {
    void this.session.retry(source).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected send(prompt: string): void {
    const category = this.category();
    if (this.session.busy() || category === null) {
      return;
    }
    const useAi = this.useAi() && this.aiOn();
    const mode = this.mode();
    const conv = this.history.active() ?? this.history.startConversation(prompt);
    const reusable = this.findReusable(conv.turns, prompt, category, useAi, mode);
    this.layout.rememberCategory(category);
    this.noteKey.set(null);
    if (reusable !== null) {
      // the copy keeps the original's time, so repeating a prompt never stretches the reuse window
      this.history.addTurn(conv.id, { ...emptyTurn(prompt, category, useAi, reusable.createdAt, mode), ...pickResults(reusable), reused: true, status: 'done', phase: 'done' });
    } else {
      void this.run(conv.id, emptyTurn(prompt, category, useAi, Date.now(), mode));
    }
    this.composer()?.clear();
  }

  /** Searches the same prompt again, with or without the AI, in the given mode. */
  protected rerun(turn: Turn, request: RerunRequest): void {
    if (this.session.busy()) {
      return;
    }
    const conv = this.history.active();
    if (conv === null) {
      return;
    }
    this.mode.set(request.mode);
    void this.run(conv.id, emptyTurn(turn.prompt, turn.category, request.useAi && this.aiOn(), Date.now(), request.mode));
  }

  /** An identical, finished turn whose results the backend still holds: no new search, nothing sent. */
  private findReusable(turns: readonly Turn[], prompt: string, category: CategoryId, useAi: boolean, mode: SearchMode): Turn | null {
    const key = promptKey(prompt, category, useAi, mode);
    const now = Date.now();
    return (
      [...turns].reverse().find((t) => t.status === 'done' && t.items.length > 0 && t.sources.every((s) => s.state === 'done') && now - t.createdAt < REUSE_MS && this.history.isLive(t.id) && promptKey(t.prompt, t.category, t.useAi, t.mode) === key) ?? null
    );
  }

  private async run(conversationId: string, turn: Turn): Promise<void> {
    this.history.addTurn(conversationId, turn);
    this.liveRef.set({ conversationId, turnId: turn.id });
    try {
      // only the prompt travels: no earlier turn, no history, nothing local
      await this.session.start({ topic: turn.prompt, useAi: turn.useAi, lang: this.i18n.lang(), category: turn.category, mode: turn.mode });
    } catch (e: unknown) {
      this.liveRef.set(null);
      this.history.patchTurn(conversationId, turn.id, { status: 'error', error: this.ai.errorText(e) });
    }
  }

  /** After adding, the cards of what was just added are not "downloaded" yet; refresh the badges. */
  protected async recheckSeen(turnId: string): Promise<void> {
    const conv = this.history.active();
    const turn = conv?.turns.find((t) => t.id === turnId);
    if (!conv || !turn || turn.items.length === 0) {
      return;
    }
    try {
      const matches = await this.ai.seenCheck(
        turn.items.map((i) => ({
          url: i.downloadUrl,
          doi: i.doi ?? undefined,
          arxiv: i.arxiv ?? undefined,
          pmid: i.pmid ?? undefined,
          pmcid: i.pmcid ?? undefined,
          filename: i.size !== null ? (i.filename ?? undefined) : undefined,
          size: i.size ?? undefined,
        })),
      );
      const byId = new Map(turn.items.map((it, n) => [it.id, matches[n] ?? null] as const));
      if (this.liveRef()?.turnId === turnId) {
        // the live turn is rebuilt from the session, so the session is what changes
        this.session.replaceSeen((it) => ({ ...it, seen: byId.get(it.id) ?? it.seen }));
      } else {
        this.history.patchTurn(conv.id, turn.id, { items: turn.items.map((it) => ({ ...it, seen: byId.get(it.id) ?? it.seen, wasSeen: it.wasSeen || byId.get(it.id) != null })) });
      }
    } catch {
      /* the badges stay as they were */
    }
  }
}

/** The parts of a finished turn that a reused turn shares. */
function pickResults(t: Turn): Partial<Turn> {
  return { sources: t.sources, unavailable: t.unavailable, plan: t.plan, queries: t.queries, comparison: t.comparison, summary: t.summary, ai: t.ai, rankedCount: t.rankedCount, items: t.items };
}
