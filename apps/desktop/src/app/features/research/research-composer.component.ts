import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, model, output, signal, viewChild } from '@angular/core';

import { CATEGORY_IDS, CategoryId, MODE_IDS, SearchMode } from '../../ai/research.models';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { MAX_PROMPT } from './research-history';
import { CATEGORY_ICONS, MODE_ICONS } from './research-modes';
import { PickerOption, ResearchPickerComponent } from './research-picker.component';

/**
 * The prompt box: a large multiline field (Enter sends, Shift+Enter adds a line, and Enter while an
 * input method is composing does neither), the mode and category choices as compact pickers, a small
 * provider chip (it turns the AI on or off and opens the AI settings) and the send button. A search
 * needs a category, so Send stays disabled until one is chosen and says so. While a search runs the
 * send button becomes Stop.
 */
@Component({
  selector: 'app-research-composer',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, MenuComponent, ResearchPickerComponent],
  template: `
    <form class="composer" (submit)="submit($event)">
      <label class="sr-only" for="research-prompt">{{ i18n.t('research.title') }}</label>
      <textarea
        #field
        id="research-prompt"
        class="field"
        dir="auto"
        rows="2"
        [attr.maxlength]="maxPrompt"
        [placeholder]="i18n.t('research.placeholder')"
        [value]="text()"
        (input)="onInput($event)"
        (keydown)="onKey($event)"
      ></textarea>

      <div class="row">
        <div class="choices">
          <app-research-picker
            [label]="i18n.t('research.modeLabel')"
            [placeholder]="i18n.t('research.modeLabel')"
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
          @if (aiReady()) {
            <button type="button" class="provider" [class.is-dim]="!useAi()" aria-haspopup="menu" [attr.aria-expanded]="chipMenu() !== null" [attr.title]="i18n.t('research.provider.open')" (click)="openChipMenu($event)">
              <app-icon name="sparkles" [size]="13" />
              <span dir="auto">{{ useAi() ? providerLabel() : i18n.t('research.aiOff') }}</span>
            </button>
          } @else {
            <button type="button" class="provider is-off" (click)="setupAi.emit()">
              <app-icon name="sparkles" [size]="13" />
              <span>{{ i18n.t('research.keywordMode.short') }}</span>
            </button>
          }
        </div>

        <div class="send">
          @if (!category() && !busy()) {
            <span class="need" id="research-need" role="status">{{ i18n.t('research.needCategory') }}</span>
          } @else {
            <span class="hint">{{ i18n.t('research.composer.hint') }}</span>
          }
          @if (busy()) {
            <button appButton variant="secondary" icon="stop" type="button" (click)="stop.emit()">{{ i18n.t('research.stop') }}</button>
          } @else {
            <span class="send-wrap" [attr.title]="category() ? null : i18n.t('research.needCategory')">
              <button appButton variant="primary" icon="arrowUp" type="submit" [disabled]="!canSend()" [attr.aria-describedby]="category() ? null : 'research-need'">{{ i18n.t('research.send') }}</button>
            </span>
          }
        </div>
      </div>

      @if (note()) {
        <p class="note" role="status"><app-icon name="info" [size]="13" /> {{ note() }}</p>
      }
    </form>
    <app-menu [open]="chipMenu() !== null" [items]="chipItems()" [x]="chipMenu()?.x ?? 0" [y]="chipMenu()?.y ?? 0" (selected)="onChipItem($event)" (closed)="chipMenu.set(null)" />
  `,
  styles: `
    :host {
      display: block;
    }

    .composer {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      padding: var(--space-3) var(--space-4) var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-xl);
      background: var(--color-surface-2);
      transition:
        border-color var(--dur-2) var(--ease-standard),
        box-shadow var(--dur-2) var(--ease-standard);
    }

    .composer:focus-within {
      border-color: var(--color-accent);
      box-shadow: 0 0 0 4px var(--color-accent-ring);
    }

    .field {
      inline-size: 100%;
      min-block-size: 56px;
      max-block-size: 200px;
      padding: var(--space-1) 0;
      border: 0;
      background: transparent;
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-lg);
      line-height: var(--leading-normal);
      resize: none;
      outline: none;
    }

    .field::placeholder {
      color: var(--color-text-muted);
    }

    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      flex-wrap: wrap;
      gap: var(--space-2) var(--space-3);
    }

    .choices,
    .send {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-2);
      min-inline-size: 0;
    }

    .provider {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      max-inline-size: 220px;
      block-size: 34px;
      padding-inline: var(--space-3);
      border: 1px solid transparent;
      border-radius: var(--radius-full);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-xs);
      cursor: pointer;
    }

    .provider app-icon {
      color: var(--color-accent-fg);
      flex: none;
    }

    .provider span {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .provider:hover {
      color: var(--color-text);
      background: var(--color-surface-hover);
    }

    .provider:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .provider.is-dim app-icon {
      color: var(--color-text-muted);
    }

    .provider.is-off {
      border: 1px dashed var(--color-border-strong);
    }

    .hint,
    .need {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
    }

    .need {
      color: var(--color-warning);
    }

    .note {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
    }

    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    @media (max-width: 760px) {
      .hint {
        display: none;
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .composer {
        transition: none;
      }
    }
  `,
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
  /** The pickers open above the box when it sits at the bottom of the page. */
  readonly placement = input<'up' | 'down'>('up');

  readonly send = output<string>();
  readonly stop = output<void>();
  readonly pickMode = output<SearchMode>();
  readonly pickCategory = output<CategoryId>();
  readonly openAiSettings = output<void>();
  /** The "Keyword search · Add AI" chip: opens the guided setup. */
  readonly setupAi = output<void>();

  protected readonly text = signal('');
  protected readonly chipMenu = signal<{ x: number; y: number } | null>(null);
  private readonly field = viewChild.required<ElementRef<HTMLTextAreaElement>>('field');
  protected readonly canSend = computed(() => this.text().trim().length > 0 && this.category() !== null && !this.busy());

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
    // grow with the text, up to the CSS maximum
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
    // above the chip: the box usually sits at the bottom of the page
    this.chipMenu.set({ x: rtl ? r.right : r.left, y: Math.max(8, r.top - 2 * 30 - 20) });
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
    el.focus();
  }

  /** Moves the focus into the field. */
  focus(): void {
    this.field().nativeElement.focus();
  }
}
