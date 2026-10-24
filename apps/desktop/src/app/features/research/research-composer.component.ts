import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, model, output, signal, viewChild } from '@angular/core';

import { FORM_STYLES } from '../../ai/ai-styles';
import { CATEGORY_IDS, CategoryId } from '../../ai/research.models';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { MAX_PROMPT } from './research-history';

/**
 * The prompt box: a multiline field (Enter sends, Shift+Enter adds a line, and Enter while an
 * input method is composing does neither), the kind of result, the AI switch and a provider/model
 * indicator that opens the AI settings. While a search runs the send button becomes Stop.
 */
@Component({
  selector: 'app-research-composer',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent],
  template: `
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

      <div class="row">
        <label class="mini">
          <span>{{ i18n.t('research.categories') }}</span>
          <select class="select" [value]="category()" (change)="category.set(pick($event))">
            @for (c of categoryIds; track c) {
              <option [value]="c" [selected]="c === category()">{{ i18n.t('research.cat.' + c) }}</option>
            }
          </select>
        </label>

        @if (aiReady()) {
          <label class="switch-row">
            <span class="switch" [class.is-on]="useAi()">
              <input type="checkbox" role="switch" [checked]="useAi()" (change)="useAi.set(checked($event))" />
              <span class="knob"></span>
            </span>
            <span>{{ i18n.t('research.useAi') }}</span>
          </label>
          <button type="button" class="provider" [attr.title]="i18n.t('research.provider.open')" (click)="openAiSettings.emit()">
            <app-icon name="sparkles" [size]="13" />
            <span dir="auto">{{ providerLabel() }}</span>
          </button>
        } @else {
          <button type="button" class="provider is-off" (click)="setupAi.emit()">
            <app-icon name="sparkles" [size]="13" />
            <span>{{ i18n.t('research.keywordMode.short') }}</span>
          </button>
        }

        <span class="grow"></span>
        <span class="hint">{{ i18n.t('research.composer.hint') }}</span>
        @if (busy()) {
          <button appButton variant="secondary" icon="stop" type="button" (click)="stop.emit()">{{ i18n.t('research.stop') }}</button>
        } @else {
          <button appButton variant="primary" icon="arrowUp" type="submit" [disabled]="text().trim().length === 0">{{ i18n.t('research.send') }}</button>
        }
      </div>
    </form>
  `,
  styles: [
    FORM_STYLES,
    `
    :host {
      display: block;
    }

    .composer {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      padding: var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-lg, 14px);
      background: var(--color-surface-2);
    }

    .composer:focus-within {
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .field {
      inline-size: 100%;
      min-block-size: 44px;
      max-block-size: 180px;
      padding: var(--space-2);
      border: 0;
      background: transparent;
      color: var(--color-text);
      font: inherit;
      resize: none;
      outline: none;
    }

    .row {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: var(--space-3);
    }

    .grow {
      flex: 1;
    }

    .mini,
    .switch-row {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-sm);
      color: var(--color-text-secondary);
    }

    .switch-row {
      cursor: pointer;
    }

    .select {
      block-size: 30px;
      padding-inline: 8px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-1, var(--color-surface-2));
      color: var(--color-text);
      font: inherit;
      font-size: var(--text-sm);
    }

    .provider {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      max-inline-size: 240px;
      padding: 3px 10px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-full);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-xs);
      cursor: pointer;
    }

    .provider span {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .provider:hover {
      color: var(--color-text);
      border-color: var(--color-accent);
    }

    .provider:focus-visible,
    .select:focus-visible {
      outline: 2px solid var(--color-accent);
      outline-offset: 2px;
    }

    .provider.is-off {
      border-style: dashed;
    }

    .hint {
      font-size: var(--text-xs);
      color: var(--color-text-muted);
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
  `,
  ],
})
export class ResearchComposerComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly categoryIds = CATEGORY_IDS;
  protected readonly maxPrompt = MAX_PROMPT;

  /** Kind of result to look for. */
  readonly category = model<CategoryId>('documents');
  /** Ask the AI to plan and rank (only offered when a provider is set up). */
  readonly useAi = model(true);
  readonly aiReady = input(false);
  /** `Provider · model`, or empty. */
  readonly providerLabel = input('');
  readonly busy = input(false);

  readonly send = output<string>();
  readonly stop = output<void>();
  readonly openAiSettings = output<void>();
  /** The "Keyword search · Add AI" chip: opens the guided setup. */
  readonly setupAi = output<void>();

  protected readonly text = signal('');
  private readonly field = viewChild.required<ElementRef<HTMLTextAreaElement>>('field');
  protected readonly canSend = computed(() => this.text().trim().length > 0 && !this.busy());

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

  protected pick(event: Event): CategoryId {
    return (event.target as HTMLSelectElement).value as CategoryId;
  }

  protected checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked;
  }
}
