import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, model, output, signal, viewChild } from '@angular/core';

import { CATEGORY_IDS, CategoryId, MODE_IDS, SearchMode } from '../../ai/research.models';
import { I18nService } from '../../i18n/i18n.service';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { AxIconComponent } from './ax-icon.component';
import { MAX_PROMPT } from './research-history';
import { CATEGORY_ICONS, MODE_ICONS } from './research-modes';
import { PickerOption, ResearchPickerComponent } from './research-picker.component';

/**
 * The prompt box, drawn like the Astryx ChatComposer ("Footer Actions"): a rounded box with the
 * message field on top and one footer row, the mode and category triggers on the start side and the
 * AI button and Send on the end side. Enter sends, Shift+Enter adds a line, and Enter while an input
 * method is composing does neither. A search needs a category, so Send stays disabled until one is
 * chosen and a warning says so; while a search runs Send becomes Stop. An error banner joins the box
 * from above, a warning from below.
 */
@Component({
  selector: 'app-research-composer',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AxIconComponent, MenuComponent, ResearchPickerComponent],
  template: `
    @if (error(); as message) {
      <div class="banner is-error" role="alert">
        <app-ax-icon name="xCircle" [size]="20" />
        <span dir="auto">{{ message }}</span>
      </div>
    }
    <form class="composer" (submit)="submit($event)">
      <label class="sr-only" for="research-prompt">{{ i18n.t('research.title') }}</label>
      <textarea
        #field
        id="research-prompt"
        class="field"
        dir="auto"
        rows="1"
        [attr.maxlength]="maxPrompt"
        [placeholder]="i18n.t('research.placeholder')"
        [value]="text()"
        (input)="onInput($event)"
        (keydown)="onKey($event)"
      ></textarea>

      <div class="footer">
        <div class="group">
          <app-research-picker
            [label]="i18n.t('research.modeLabel')"
            [placeholder]="i18n.t('research.modeLabel')"
            fixedIcon="sparkle"
            [options]="modeOptions()"
            [value]="mode()"
            [placement]="placement()"
            (picked)="onMode($event)"
          />
          <app-research-picker
            [label]="i18n.t('research.categoryLabel')"
            [placeholder]="i18n.t('research.categoryPlaceholder')"
            placeholderIcon="layers"
            [options]="categoryOptions()"
            [value]="category()"
            [placement]="placement()"
            (picked)="onCategory($event)"
          />
        </div>

        <div class="group">
          @if (aiReady()) {
            <button type="button" class="icon-button" [class.is-dim]="!useAi()" aria-haspopup="menu" [attr.aria-expanded]="chipMenu() !== null" [attr.aria-label]="aiTitle()" [attr.title]="aiTitle()" (click)="openChipMenu($event)">
              <app-ax-icon name="cpu" [size]="16" />
            </button>
          } @else {
            <button type="button" class="icon-button is-dim" [attr.aria-label]="i18n.t('research.keywordMode.short')" [attr.title]="i18n.t('research.keywordMode.short')" (click)="setupAi.emit()">
              <app-ax-icon name="cpu" [size]="16" />
            </button>
          }
          @if (busy()) {
            <button type="button" class="send is-stop" [attr.aria-label]="i18n.t('research.stop')" [attr.title]="i18n.t('research.stop')" (click)="stop.emit()">
              <app-ax-icon name="stop" [size]="16" />
            </button>
          } @else {
            <button type="submit" class="send" [disabled]="!canSend()" [attr.aria-label]="i18n.t('research.send')" [attr.title]="category() ? null : i18n.t('research.needCategory')" [attr.aria-describedby]="needCategory() ? 'research-need' : null">
              <app-ax-icon name="arrowUp" [size]="16" />
            </button>
          }
        </div>
      </div>
    </form>
    @if (needCategory()) {
      <div class="banner is-warn" id="research-need" role="status">
        <app-ax-icon name="alert" [size]="20" />
        <span>{{ i18n.t('research.needCategory') }}</span>
      </div>
    } @else if (note(); as line) {
      <div class="banner is-warn" role="status">
        <app-ax-icon name="alert" [size]="20" />
        <span>{{ line }}</span>
      </div>
    }
    <app-menu [open]="chipMenu() !== null" [items]="chipItems()" [x]="chipMenu()?.x ?? 0" [y]="chipMenu()?.y ?? 0" (selected)="onChipItem($event)" (closed)="chipMenu.set(null)" />
  `,
  styles: `
    /* 450 wide and centred on the empty page, the width of the conversation column once there is one */
    :host {
      display: block;
      inline-size: 100%;
      max-inline-size: 450px;
      margin-inline: auto;
      font-family: var(--font-chat);
    }

    :host(.is-wide) {
      max-inline-size: 760px;
    }

    /* the box: radius 28, padding 12, 8 between the field and the footer, the Astryx surface and shadow */
    .composer {
      position: relative;
      z-index: 1;
      display: flex;
      flex-direction: column;
      gap: 8px;
      box-sizing: border-box;
      padding: 12px;
      border-radius: 28px;
      background: var(--ax-surface);
      color: var(--ax-text);
      box-shadow: var(--ax-shadow);
    }

    /* the field: 14/22, 4px inset, one line tall until the text needs more */
    .field {
      display: block;
      box-sizing: border-box;
      inline-size: 100%;
      min-block-size: 22px;
      max-block-size: 200px;
      margin: 0;
      padding: 0 4px;
      border: 0;
      background: transparent;
      color: var(--ax-text);
      font: 400 14px/22px var(--font-chat);
      resize: none;
      outline: none;
      overflow-y: auto;
    }

    .field::placeholder {
      color: var(--ax-text-muted);
      opacity: 1;
    }

    /* the footer row: 32 high, start group and end group 8 apart, 4 between buttons */
    .footer {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      min-block-size: 32px;
    }

    .group {
      display: flex;
      align-items: center;
      gap: 4px;
      min-inline-size: 0;
    }

    .icon-button,
    .send {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      flex: none;
      inline-size: 32px;
      block-size: 32px;
      padding: 0;
      border: 0;
      border-radius: 16px;
      background-color: transparent;
      background-image: linear-gradient(transparent, transparent);
      color: var(--ax-text);
      cursor: pointer;
      transition:
        background-image 125ms cubic-bezier(0.24, 1, 0.4, 1),
        background-color 125ms cubic-bezier(0.24, 1, 0.4, 1),
        color 125ms cubic-bezier(0.24, 1, 0.4, 1),
        opacity 125ms cubic-bezier(0.24, 1, 0.4, 1);
    }

    .icon-button:hover,
    .icon-button[aria-expanded='true'] {
      background-image: linear-gradient(var(--ax-overlay), var(--ax-overlay));
    }

    .icon-button.is-dim {
      color: var(--ax-text-muted);
    }

    .icon-button:focus-visible,
    .send:focus-visible {
      outline: 2px solid var(--ax-focus);
      outline-offset: 3px;
    }

    .send {
      background-color: var(--ax-send-bg);
      color: var(--ax-send-fg);
    }

    .send:disabled {
      opacity: 0.5;
      cursor: default;
    }

    /* while a search runs: the quiet "secondary" fill with the square */
    .send.is-stop {
      background-color: var(--ax-bubble);
      color: var(--ax-text);
    }

    /* the banners: 12/20, 28 radius, joined to the box (they run 28px under or over it) */
    .banner {
      position: relative;
      z-index: 0;
      display: flex;
      align-items: center;
      gap: 8px;
      box-sizing: border-box;
      font: 400 12px/20px var(--font-chat);
    }

    .banner > span {
      min-inline-size: 0;
      overflow-wrap: anywhere;
    }

    .banner.is-error {
      padding: 12px 16px 40px;
      margin-block-end: -28px;
      border-start-start-radius: 28px;
      border-start-end-radius: 28px;
      background: var(--ax-error-bg);
      color: var(--ax-error-fg);
    }

    .banner.is-error app-ax-icon {
      color: var(--ax-error-icon);
    }

    .banner.is-warn {
      padding: 40px 16px 12px;
      margin-block-start: -28px;
      border-end-start-radius: 28px;
      border-end-end-radius: 28px;
      background: var(--ax-warn-bg);
      color: var(--ax-warn-fg);
    }

    .banner.is-warn app-ax-icon {
      color: var(--ax-warn-icon);
    }

    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    @media (prefers-reduced-motion: reduce) {
      .icon-button,
      .send {
        transition: none;
      }
    }
  `,
  host: { '[class.is-wide]': 'wide()' },
})
export class ResearchComposerComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly maxPrompt = MAX_PROMPT;

  /** The chosen category, `null` until the user picks one (a search needs it). */
  readonly category = input<CategoryId | null>(null);
  readonly mode = input<SearchMode>('quick-search');
  /** Ask the AI to plan and rank (only offered when a provider is set up). */
  readonly useAi = model(true);
  readonly aiReady = input(false);
  /** `Provider · model`, or empty. */
  readonly providerLabel = input('');
  readonly busy = input(false);
  /** A line saying what changed when choosing a mode moved the category or the other way round. */
  readonly note = input<string | null>(null);
  /** An error the box could not recover from; it shows as a banner above the box. */
  readonly error = input<string | null>(null);
  /** The menus open above the box when it sits at the bottom of the page. */
  readonly placement = input<'up' | 'down'>('up');
  /** The width of a conversation (up to 760) instead of the 450 of the empty page. */
  readonly wide = input(false);

  readonly send = output<string>();
  readonly stop = output<void>();
  readonly pickMode = output<SearchMode>();
  readonly pickCategory = output<CategoryId>();
  readonly openAiSettings = output<void>();
  /** The AI button while no provider is set up: opens the guided setup. */
  readonly setupAi = output<void>();

  protected readonly text = signal('');
  protected readonly chipMenu = signal<{ x: number; y: number } | null>(null);
  private readonly field = viewChild.required<ElementRef<HTMLTextAreaElement>>('field');
  protected readonly canSend = computed(() => this.text().trim().length > 0 && this.category() !== null && !this.busy());
  /** There is something to send but no category yet. */
  protected readonly needCategory = computed(() => this.category() === null && !this.busy() && this.text().trim().length > 0);
  /** The AI button's tooltip: the provider and model, or that the AI is off. */
  protected readonly aiTitle = computed(() => (this.useAi() ? this.providerLabel() || this.i18n.t('research.provider.open') : this.i18n.t('research.aiOff')));

  protected readonly modeOptions = computed<readonly PickerOption[]>(() =>
    MODE_IDS.map((id) => ({ id, label: this.i18n.t(`research.mode.${id}`), description: this.i18n.t(`research.modeDesc.${id}`), icon: MODE_ICONS[id] })),
  );
  protected readonly categoryOptions = computed<readonly PickerOption[]>(() =>
    CATEGORY_IDS.map((id) => ({ id, label: this.i18n.t(`research.cat.${id}`), description: this.i18n.t(`research.catDesc.${id}`), icon: CATEGORY_ICONS[id] })),
  );
  protected readonly chipItems = computed<readonly MenuItem[]>(() => [
    { id: 'toggle', label: this.i18n.t(this.useAi() ? 'research.ai.turnOff' : 'research.ai.turnOn'), icon: 'sparkles' },
    { id: 'settings', label: this.i18n.t('research.provider.open'), icon: 'settings' },
  ]);

  protected onInput(event: Event): void {
    const el = event.target as HTMLTextAreaElement;
    this.text.set(el.value);
    this.grow(el);
  }

  /** Grows with the text, up to the CSS maximum. */
  private grow(el: HTMLTextAreaElement): void {
    el.style.blockSize = 'auto';
    el.style.blockSize = `${el.scrollHeight}px`;
  }

  protected onKey(event: KeyboardEvent): void {
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault();
      this.submit(event);
    }
  }

  protected submit(event: Event): void {
    event.preventDefault();
    if (!this.canSend()) {
      return;
    }
    this.send.emit(this.text().trim());
  }

  protected openChipMenu(event: MouseEvent): void {
    const r = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const rtl = this.i18n.dir() === 'rtl';
    const y = this.placement() === 'up' ? Math.max(8, r.top - 2 * 30 - 20) : r.bottom + 4;
    this.chipMenu.set({ x: rtl ? r.right : r.left, y });
  }

  protected onMode(id: string): void {
    const mode = MODE_IDS.find((m) => m === id);
    if (mode !== undefined) {
      this.pickMode.emit(mode);
    }
  }

  protected onCategory(id: string): void {
    const category = CATEGORY_IDS.find((c) => c === id);
    if (category !== undefined) {
      this.pickCategory.emit(category);
    }
  }

  protected onChipItem(id: string): void {
    if (id === 'toggle') {
      this.useAi.set(!this.useAi());
    } else if (id === 'settings') {
      this.openAiSettings.emit();
    }
  }

  /** Empties the field (the page calls this once a prompt was accepted). */
  clear(): void {
    this.text.set('');
    const el = this.field().nativeElement;
    el.value = '';
    el.style.blockSize = 'auto';
  }

  /** Puts a prompt in the field and focuses it. */
  fill(text: string): void {
    this.text.set(text);
    const el = this.field().nativeElement;
    el.value = text;
    this.grow(el);
    el.focus();
  }

  /** Moves the focus into the field. */
  focus(): void {
    this.field().nativeElement.focus();
  }
}
