import { ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, input, output, signal, untracked } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { ButtonComponent } from '../ui/button.component';
import { DialogComponent } from '../ui/dialog.component';
import { IconComponent } from '../ui/icon.component';
import { ToastService } from '../ui/toast.service';
import { cleanKey, detectProviderFromKey, FREE_PROVIDERS, KEY_PAGES, keyProblem, LOCAL_ENDPOINTS, LOCAL_PLACEHOLDER_KEY, LocalKind, OLLAMA_PAGE, OTHER_PROVIDERS, pickDefaultModel } from './ai-setup';
import { FORM_STYLES } from './ai-styles';
import { isAiFail, ProviderId } from './ai.models';
import { AiService } from './ai.service';

type Step = 'choose' | 'key' | 'test';
type Choice = 'gemini' | 'groq' | 'local' | 'other';
type Stage = 'prepare' | 'key' | 'models' | 'settings' | 'test';
type Phase = 'idle' | 'running' | 'ok' | 'fail';
type FailKind = 'error' | 'local-down' | 'no-models';

/** Shown when the backend's provider list has not arrived yet. */
const FALLBACK_NAMES: Readonly<Record<ProviderId, string>> = {
  gemini: 'Google Gemini',
  groq: 'Groq',
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  openrouter: 'OpenRouter',
  deepseek: 'DeepSeek',
  mistral: 'Mistral',
  together: 'Together',
  fireworks: 'Fireworks',
  deepinfra: 'DeepInfra',
};

const ROWS: readonly Exclude<Stage, 'prepare'>[] = ['key', 'models', 'settings', 'test'];

/**
 * Guided AI setup in three steps: choose where the AI runs, get and paste a key (skipped for the
 * local option), then an automatic test. The key travels one way: from the field to `ai_set_key`.
 * It is emptied the moment it is stored, it is never rendered as text, and the privacy switches
 * (download history, name suggestions) are written back exactly as they were.
 */
@Component({
  selector: 'app-ai-setup-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent, IconComponent],
  template: `
    <app-dialog [open]="open()" [wide]="true" [title]="i18n.t('aisetup.title')" (closed)="requestClose()">
      <p class="stepno">{{ stepLabel() }}</p>

      @switch (step()) {
        @case ('choose') {
          <h3 class="heading">{{ i18n.t('aisetup.choose.title') }}</h3>
          <p class="lead">{{ i18n.t('aisetup.choose.intro') }}</p>

          <div class="cards" role="radiogroup" [attr.aria-label]="i18n.t('aisetup.choose.title')">
            @for (c of cardIds; track c) {
              <div class="card" [class.is-on]="choice() === c">
                <label class="card-label">
                  <input type="radio" class="sr-only" name="ai-choice" [value]="c" [checked]="choice() === c" [attr.data-autofocus]="choice() === c ? '' : null" (change)="choice.set(c)" (keydown.enter)="next()" />
                  <span class="radio" aria-hidden="true"></span>
                  <span class="card-text">
                    <span class="card-name">
                      {{ i18n.t('aisetup.' + c) }}
                      @if (c === 'gemini') {
                        <span class="badge">{{ i18n.t('aisetup.recommended') }}</span>
                      }
                    </span>
                    <span class="card-line">{{ i18n.t('aisetup.' + c + '.line') }}</span>
                    <span class="card-note">{{ i18n.t('aisetup.' + c + '.note') }}</span>
                  </span>
                </label>

                @if (c === 'other') {
                  <div class="card-extra">
                    <select class="input" [attr.aria-label]="i18n.t('aisetup.other.pick')" (focus)="choice.set('other')" (change)="onOther($event)">
                      @for (p of otherList(); track p.id) {
                        <option [value]="p.id" [selected]="p.id === other()">{{ p.name }}</option>
                      }
                    </select>
                  </div>
                }
                @if (c === 'local' && choice() === 'local') {
                  <div class="card-extra kinds" role="radiogroup" [attr.aria-label]="i18n.t('aisetup.local.kind')">
                    @for (k of kindIds; track k) {
                      <label class="kind" [class.is-on]="localKind() === k">
                        <input type="radio" name="ai-local-kind" [value]="k" [checked]="localKind() === k" (change)="localKind.set(k)" />
                        <span>{{ i18n.t('aisetup.kind.' + k) }}</span>
                      </label>
                    }
                  </div>
                }
              </div>
            }
          </div>

          <p class="privacy"><app-icon name="shieldCheck" [size]="16" />{{ i18n.t('aisetup.privacy') }}</p>
        }

        @case ('key') {
          <h3 class="heading">{{ i18n.t('aisetup.key.title').replace('{provider}', providerName()) }}</h3>
          <ol class="howto">
            <li>{{ i18n.t('aisetup.key.s1').replace('{provider}', providerName()) }}</li>
            <li>{{ i18n.t(free() ? 'aisetup.key.s2free' : 'aisetup.key.s2paid') }}</li>
            <li>{{ i18n.t('aisetup.key.s3') }}</li>
          </ol>
          <div class="open-row">
            <button appButton variant="secondary" icon="external" type="button" (click)="openKeyPage()">{{ i18n.t('aisetup.openPage') }}</button>
          </div>

          <label class="field-label" for="ai-setup-key">{{ i18n.t('aisetup.key.label') }}</label>
          <div class="key-row">
            <input
              id="ai-setup-key"
              class="input key-input"
              type="password"
              name="ai-setup-key"
              dir="ltr"
              autocomplete="off"
              spellcheck="false"
              data-autofocus
              [class.is-invalid]="keyError() !== null"
              [attr.aria-invalid]="keyError() !== null"
              [attr.aria-describedby]="keyError() !== null ? 'ai-setup-key-msg' : null"
              [placeholder]="i18n.t('aisetup.key.placeholder')"
              [value]="keyDraft()"
              (input)="onKeyInput($event)"
              (keydown.enter)="next()"
            />
            <button appButton variant="secondary" icon="copy" type="button" (click)="paste()">{{ i18n.t('aisetup.paste') }}</button>
          </div>
          <div id="ai-setup-key-msg" class="msgs" aria-live="polite">
            @if (keyError(); as e) {
              <p class="field-error" role="alert">{{ e }}</p>
            }
            @if (notice(); as n) {
              <p class="calm">{{ n }}</p>
            }
            @if (pasteMsg(); as m) {
              <p class="calm">{{ m }}</p>
            }
          </div>
          <p class="privacy"><app-icon name="shieldCheck" [size]="16" />{{ i18n.t('aisetup.key.stored') }}</p>
        }

        @case ('test') {
          <h3 class="heading" tabindex="-1" data-autofocus>{{ i18n.t(phase() === 'ok' ? 'aisetup.test.titleOk' : phase() === 'fail' ? 'aisetup.test.titleFail' : 'aisetup.test.title') }}</h3>

          <ul class="progress" [attr.aria-label]="i18n.t('aisetup.test.title')">
            @for (r of rows; track r) {
              <li class="prog" [class.is-done]="rowState(r) === 'done'" [class.is-active]="rowState(r) === 'active'" [class.is-failed]="rowState(r) === 'failed'">
                <span class="prog-mark" aria-hidden="true">
                  @switch (rowState(r)) {
                    @case ('done') {
                      <app-icon name="check" [size]="14" />
                    }
                    @case ('failed') {
                      <app-icon name="alert" [size]="14" />
                    }
                    @case ('active') {
                      <span class="spin"></span>
                    }
                    @default {
                      <span class="pending"></span>
                    }
                  }
                </span>
                <span>{{ rowLabel(r) }}</span>
              </li>
            }
          </ul>

          <div class="live" role="status" aria-live="polite">
            @if (phase() === 'running') {
              <span class="sr-only">{{ rowLabel(currentRow()) }}</span>
            }
            @if (phase() === 'ok') {
              <p class="result ok-result"><app-icon name="check" [size]="18" /><span>{{ i18n.t('aisetup.ready') }}</span></p>
              <p class="lead" dir="auto">{{ usingLine() }}</p>
            }
          </div>
          @if (phase() === 'fail') {
            <div class="result bad-result" role="alert">
              <app-icon name="alert" [size]="18" />
              <div>
                <p>{{ failText() }}</p>
                @if (failKind() === 'local-down') {
                  <p class="lead">{{ i18n.t(localKind() === 'ollama' ? 'aisetup.local.down' : 'aisetup.local.downLm') }}</p>
                }
              </div>
            </div>
          }
        }
      }

      <ng-container dialogActions>
        @switch (step()) {
          @case ('choose') {
            <button appButton variant="ghost" type="button" (click)="requestClose()">{{ i18n.t('action.cancel') }}</button>
            <button appButton variant="primary" type="button" (click)="next()">{{ i18n.t('aisetup.next') }}</button>
          }
          @case ('key') {
            <button appButton variant="ghost" type="button" (click)="back()">{{ i18n.t('aisetup.back') }}</button>
            <button appButton variant="primary" type="button" (click)="next()">{{ i18n.t('aisetup.next') }}</button>
          }
          @case ('test') {
            @switch (phase()) {
              @case ('ok') {
                <button appButton variant="primary" icon="check" type="button" data-done (click)="requestClose()">{{ i18n.t('aisetup.done') }}</button>
              }
              @case ('fail') {
                @if (keyStored()) {
                  <button appButton variant="ghost" icon="trash" type="button" [busy]="removing()" (click)="removeKey()">{{ i18n.t('ai.keyRemove') }}</button>
                }
                @if (failKind() === 'local-down') {
                  <button appButton variant="secondary" icon="external" type="button" (click)="openOllama()">{{ i18n.t('aisetup.local.get') }}</button>
                } @else if (!isLocal()) {
                  <button appButton variant="secondary" type="button" (click)="useAnotherKey()">{{ i18n.t('aisetup.anotherKey') }}</button>
                } @else {
                  <button appButton variant="secondary" type="button" (click)="back()">{{ i18n.t('aisetup.back') }}</button>
                }
                <button appButton variant="primary" icon="retry" type="button" data-done (click)="run()">{{ i18n.t('aisetup.tryAgain') }}</button>
              }
              @default {
                <button appButton variant="ghost" type="button" (click)="requestClose()">{{ i18n.t('action.cancel') }}</button>
              }
            }
          }
        }
      </ng-container>
    </app-dialog>
  `,
  styles: [
    FORM_STYLES,
    `
      .sr-only {
        position: absolute;
        inline-size: 1px;
        block-size: 1px;
        overflow: hidden;
        clip-path: inset(50%);
        white-space: nowrap;
      }

      .stepno {
        color: var(--color-text-muted);
        font-size: var(--text-xs);
        letter-spacing: 0.02em;
        margin-block-end: var(--space-1);
      }

      .heading {
        font-size: var(--text-md);
        font-weight: var(--weight-semibold);
        outline: none;
      }

      .lead {
        margin-block: var(--space-1) var(--space-3);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
        line-height: var(--leading-normal);
      }

      .cards {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
      }

      .card {
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-md);
        background: var(--color-surface-2);
      }

      .card.is-on {
        border-color: var(--color-accent);
        background: var(--color-accent-soft, var(--color-surface-3));
      }

      .card:has(input:focus-visible) {
        outline: 2px solid var(--color-focus);
        outline-offset: 2px;
      }

      .card-label {
        display: flex;
        align-items: flex-start;
        gap: var(--space-3);
        padding: var(--space-3) var(--space-4);
        cursor: pointer;
      }

      .radio {
        flex-shrink: 0;
        inline-size: 16px;
        block-size: 16px;
        margin-block-start: 3px;
        border: 2px solid var(--color-border-strong);
        border-radius: var(--radius-full);
      }

      .card.is-on .radio {
        border-color: var(--color-accent);
        background: radial-gradient(circle, var(--color-accent) 0 38%, transparent 42%);
      }

      .card-text {
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-inline-size: 0;
      }

      .card-name {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-2);
        font-weight: var(--weight-semibold);
      }

      .badge {
        padding: 1px 8px;
        border: 1px solid var(--color-accent);
        border-radius: var(--radius-full);
        color: var(--color-accent);
        font-size: var(--text-xs);
        font-weight: var(--weight-medium);
      }

      .card-line {
        font-size: var(--text-sm);
        color: var(--color-text);
      }

      .card-note {
        font-size: var(--text-xs);
        color: var(--color-text-muted);
        line-height: var(--leading-normal);
      }

      .card-extra {
        padding: 0 var(--space-4) var(--space-3);
        padding-inline-start: calc(var(--space-4) + 16px + var(--space-3));
      }

      .kinds {
        display: flex;
        gap: var(--space-2);
        flex-wrap: wrap;
      }

      .kind {
        display: inline-flex;
        align-items: center;
        gap: var(--space-2);
        padding: 4px 12px;
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-full);
        font-size: var(--text-sm);
        cursor: pointer;
      }

      .kind.is-on {
        border-color: var(--color-accent);
      }

      .kind input {
        accent-color: var(--color-accent);
      }

      .privacy {
        display: flex;
        align-items: flex-start;
        gap: var(--space-2);
        margin-block: var(--space-4) var(--space-2);
        color: var(--color-text-secondary);
        font-size: var(--text-xs);
        line-height: var(--leading-normal);
      }

      .privacy app-icon {
        color: var(--color-accent);
        flex-shrink: 0;
      }

      .howto {
        margin-block: var(--space-3);
        padding-inline-start: var(--space-6);
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
        line-height: var(--leading-normal);
      }

      .open-row {
        margin-block-end: var(--space-4);
      }

      .field-label {
        display: block;
        margin-block-end: var(--space-1);
        font-weight: var(--weight-medium);
        font-size: var(--text-sm);
      }

      .key-row {
        display: flex;
        align-items: center;
        gap: var(--space-2);
      }

      .key-input {
        flex: 1;
        min-inline-size: 0;
      }

      .msgs {
        min-block-size: 1.5em;
        margin-block-start: var(--space-2);
      }

      .calm {
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
      }

      .progress {
        list-style: none;
        margin-block: var(--space-4);
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
      }

      .prog {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        color: var(--color-text-muted);
        font-size: var(--text-sm);
      }

      .prog.is-active,
      .prog.is-done {
        color: var(--color-text);
      }

      .prog.is-failed {
        color: var(--color-danger);
      }

      .prog-mark {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        inline-size: 20px;
        block-size: 20px;
        flex-shrink: 0;
      }

      .prog.is-done .prog-mark {
        color: var(--color-success);
      }

      .pending {
        inline-size: 8px;
        block-size: 8px;
        border-radius: var(--radius-full);
        background: var(--color-border-strong);
      }

      .spin {
        inline-size: 14px;
        block-size: 14px;
        border: 2px solid var(--color-border-strong);
        border-block-start-color: var(--color-accent);
        border-radius: var(--radius-full);
        animation: ai-spin 0.8s linear infinite;
      }

      @keyframes ai-spin {
        to {
          transform: rotate(360deg);
        }
      }

      @media (prefers-reduced-motion: reduce) {
        .spin {
          animation-duration: 3s;
        }
      }

      .result {
        display: flex;
        align-items: flex-start;
        gap: var(--space-2);
        padding: var(--space-3);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        background: var(--color-surface-2);
        font-size: var(--text-sm);
        line-height: var(--leading-normal);
      }

      .ok-result {
        border-color: var(--color-success);
        font-weight: var(--weight-medium);
      }

      .ok-result app-icon {
        color: var(--color-success);
      }

      .bad-result {
        border-color: var(--color-danger);
      }

      .bad-result app-icon {
        color: var(--color-danger);
      }

      .bad-result .lead {
        margin-block-end: 0;
      }
    `,
  ],
})
export class AiSetupDialogComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly toasts = inject(ToastService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  /** Open state, controlled by the parent. */
  readonly open = input.required<boolean>();
  /** The user closed the dialog (Done, Cancel, Escape, backdrop). The parent clears `open`. */
  readonly closed = output<void>();

  protected readonly cardIds: readonly Choice[] = ['gemini', 'groq', 'local', 'other'];
  protected readonly kindIds: readonly LocalKind[] = ['ollama', 'lmstudio'];
  protected readonly rows = ROWS;

  protected readonly step = signal<Step>('choose');
  protected readonly choice = signal<Choice>('gemini');
  protected readonly other = signal<ProviderId>('openai');
  protected readonly localKind = signal<LocalKind>('ollama');
  protected readonly keyDraft = signal('');
  protected readonly keyError = signal<string | null>(null);
  protected readonly notice = signal<string | null>(null);
  protected readonly pasteMsg = signal<string | null>(null);

  protected readonly phase = signal<Phase>('idle');
  private readonly stage = signal<Stage>('key');
  protected readonly failKind = signal<FailKind>('error');
  protected readonly failText = signal('');
  /** A key (or the local placeholder) has been stored by this run, so Remove key is offered. */
  protected readonly keyStored = signal(false);
  protected readonly removing = signal(false);
  private readonly usedModel = signal('');
  private runId = 0;

  protected readonly isLocal = computed(() => this.choice() === 'local');
  /** Local servers are reached through the OpenAI-compatible adapter. */
  protected readonly provider = computed<ProviderId>(() => {
    switch (this.choice()) {
      case 'gemini':
        return 'gemini';
      case 'groq':
        return 'groq';
      case 'local':
        return 'openai';
      default:
        return this.other();
    }
  });
  protected readonly free = computed(() => !this.isLocal() && FREE_PROVIDERS.includes(this.provider()));
  protected readonly providerName = computed(() => this.nameOf(this.provider()));
  protected readonly otherList = computed(() => {
    const known = this.ai.status()?.providers ?? [];
    return OTHER_PROVIDERS.map((id) => ({ id, name: known.find((p) => p.id === id)?.name ?? FALLBACK_NAMES[id] }));
  });
  private readonly order = computed<readonly Step[]>(() => (this.isLocal() ? ['choose', 'test'] : ['choose', 'key', 'test']));
  protected readonly stepLabel = computed(() =>
    this.i18n
      .t('aisetup.step')
      .replace('{n}', String(this.order().indexOf(this.step()) + 1))
      .replace('{total}', String(this.order().length)),
  );
  protected readonly currentRow = computed<Exclude<Stage, 'prepare'>>(() => {
    const s = this.stage();
    return s === 'prepare' ? 'key' : s;
  });
  protected readonly usingLine = computed(() => this.i18n.t('aisetup.using').replace('{provider}', this.isLocal() ? this.i18n.t(`aisetup.kind.${this.localKind()}`) : this.providerName()).replace('{model}', this.usedModel()));

  constructor() {
    // every opening starts at the first step with nothing left over from the last one
    effect(() => {
      if (this.open()) {
        untracked(() => this.reset());
      }
    });
    // focus the first field of each step
    effect(() => {
      this.step();
      if (this.open()) {
        this.focusSoon('[data-autofocus]');
      }
    });
  }

  private reset(): void {
    this.runId++;
    this.step.set('choose');
    this.choice.set('gemini');
    this.other.set('openai');
    this.localKind.set('ollama');
    this.keyDraft.set('');
    this.keyError.set(null);
    this.notice.set(null);
    this.pasteMsg.set(null);
    this.phase.set('idle');
    this.keyStored.set(false);
    this.removing.set(false);
    if (this.ai.available && this.ai.status() === null) {
      void this.ai.refresh();
    }
  }

  private focusSoon(selector: string): void {
    setTimeout(() => this.host.nativeElement.querySelector<HTMLElement>(selector)?.focus(), 0);
  }

  private nameOf(id: ProviderId): string {
    return this.ai.status()?.providers.find((p) => p.id === id)?.name ?? FALLBACK_NAMES[id];
  }

  protected requestClose(): void {
    // a run that is still going finishes quietly; nothing it reports is shown any more
    this.runId++;
    this.keyDraft.set('');
    this.closed.emit();
  }

  protected onOther(event: Event): void {
    this.other.set((event.target as HTMLSelectElement).value as ProviderId);
    this.choice.set('other');
  }

  protected back(): void {
    this.runId++;
    this.phase.set('idle');
    this.keyStored.set(false);
    this.step.set(this.step() === 'test' && !this.isLocal() ? 'key' : 'choose');
  }

  protected next(): void {
    if (this.step() === 'choose') {
      this.notice.set(null);
      this.pasteMsg.set(null);
      this.keyError.set(null);
      if (this.isLocal()) {
        this.step.set('test');
        void this.run();
      } else {
        this.step.set('key');
      }
      return;
    }
    if (this.step() !== 'key') {
      return;
    }
    const cleaned = cleanKey(this.keyDraft());
    const problem = keyProblem(cleaned);
    if (problem !== null) {
      this.keyError.set(this.i18n.t(`aisetup.key.${problem}`));
      this.focusSoon('#ai-setup-key');
      return;
    }
    this.adoptProviderOf(cleaned);
    this.step.set('test');
    void this.run();
  }

  protected onKeyInput(event: Event): void {
    this.setDraft((event.target as HTMLInputElement).value);
  }

  private setDraft(raw: string): void {
    this.keyDraft.set(raw);
    this.keyError.set(null);
    this.pasteMsg.set(null);
    this.adoptProviderOf(cleanKey(raw));
  }

  /** A key whose prefix names another provider moves the choice there and says so once. */
  private adoptProviderOf(cleaned: string): void {
    const found = detectProviderFromKey(cleaned);
    if (found === null || found === this.provider()) {
      return;
    }
    if (found === 'gemini' || found === 'groq') {
      this.choice.set(found);
    } else {
      this.other.set(found);
      this.choice.set('other');
    }
    this.notice.set(this.i18n.t('aisetup.key.switched').replace(/\{provider\}/g, this.nameOf(found)));
  }

  /** Reads the clipboard only here, on the user's click. */
  protected async paste(): Promise<void> {
    this.pasteMsg.set(null);
    try {
      const text = await navigator.clipboard.readText();
      if (text.trim().length === 0) {
        this.pasteMsg.set(this.i18n.t('aisetup.paste.empty'));
        return;
      }
      this.setDraft(text);
    } catch {
      this.pasteMsg.set(this.i18n.t('aisetup.paste.denied'));
    }
    this.focusSoon('#ai-setup-key');
  }

  protected openKeyPage(): void {
    this.openUrl(KEY_PAGES[this.provider()]);
  }

  protected openOllama(): void {
    this.openUrl(OLLAMA_PAGE);
  }

  /** Only the hard-coded https addresses in `ai-setup.ts` reach this. */
  private openUrl(url: string): void {
    if (!this.tauri) {
      window.open(url, '_blank', 'noopener,noreferrer');
      return;
    }
    this.invoke<void>('open_web_url', { url }).catch((e: unknown) => this.toasts.show('danger', this.ai.errorText(e)));
  }

  protected rowState(id: Exclude<Stage, 'prepare'>): 'done' | 'active' | 'failed' | 'pending' {
    if (this.phase() === 'ok') {
      return 'done';
    }
    if (this.phase() === 'idle') {
      return 'pending';
    }
    const at = ROWS.indexOf(this.currentRow());
    const me = ROWS.indexOf(id);
    if (me < at) {
      return 'done';
    }
    if (me === at) {
      return this.phase() === 'fail' ? 'failed' : 'active';
    }
    return 'pending';
  }

  protected rowLabel(id: Exclude<Stage, 'prepare'>): string {
    return this.i18n.t(id === 'key' && this.isLocal() ? 'aisetup.test.local' : `aisetup.test.${id}`);
  }

  /** Saves the key, finds a model, saves the choice, tests it. Every failure ends in a readable state. */
  protected async run(): Promise<void> {
    const id = ++this.runId;
    const alive = (): boolean => id === this.runId;
    const local = this.isLocal();
    const provider = this.provider();
    const baseUrl = local ? LOCAL_ENDPOINTS[this.localKind()] : null;
    this.phase.set('running');
    this.stage.set('key');
    this.failText.set('');
    this.failKind.set('error');
    try {
      const st = this.ai.status() ?? (await this.ai.refresh());
      if (st === null) {
        throw new Error(this.i18n.t('ai.error.network'));
      }
      // what the user had before: the two privacy switches go back exactly like this
      const prev = st.settings;
      if (prev.provider !== provider || (prev.baseUrl ?? null) !== baseUrl) {
        // the backend binds a key to the address it is saved for, so the address is set first
        this.stage.set('key');
        await this.ai.saveSettings({ ...prev, provider, baseUrl, model: '' });
      }
      if (!this.keyStored()) {
        // Local servers ignore the Authorization header; the backend still wants a key, so it gets a placeholder.
        const key = local ? LOCAL_PLACEHOLDER_KEY : cleanKey(this.keyDraft());
        await this.ai.setKey(provider, key);
        this.keyStored.set(true);
        // the secret leaves the page now, whatever happens next
        this.keyDraft.set('');
      }
      if (!alive()) {
        return;
      }
      this.stage.set('models');
      const models = await this.ai.listModels(provider);
      const model = pickDefaultModel(provider, models.map((m) => m.id), local);
      if (model === null) {
        this.fail(id, 'no-models', this.i18n.t(local ? (this.localKind() === 'ollama' ? 'aisetup.local.noModels' : 'aisetup.local.noModelsLm') : 'aisetup.noModels'));
        return;
      }
      if (!alive()) {
        return;
      }
      this.stage.set('settings');
      const now = this.ai.status()?.settings ?? prev;
      await this.ai.saveSettings({ ...now, provider, baseUrl, model, useDownloadHistory: prev.useDownloadHistory, suggestNames: prev.suggestNames });
      if (!alive()) {
        return;
      }
      this.stage.set('test');
      await this.ai.testConnection();
      const after = await this.ai.refresh();
      if (!alive()) {
        return;
      }
      if (after?.configured !== true) {
        this.fail(id, 'error', this.i18n.t('ai.error.not_configured'));
        return;
      }
      this.usedModel.set(model);
      this.phase.set('ok');
      this.focusSoon('[data-done]');
    } catch (e: unknown) {
      // a local server that does not answer is the most common first-run problem: say what to do
      const down = local && isAiFail(e) && (e.code === 'network' || e.code === 'timeout');
      this.fail(id, down ? 'local-down' : 'error', this.ai.errorText(e));
    }
  }

  private fail(id: number, kind: FailKind, text: string): void {
    if (id !== this.runId) {
      return;
    }
    this.failKind.set(kind);
    this.failText.set(text);
    this.phase.set('fail');
    this.focusSoon('[data-done]');
  }

  protected useAnotherKey(): void {
    this.runId++;
    this.keyStored.set(false);
    this.keyDraft.set('');
    this.keyError.set(null);
    this.notice.set(null);
    this.pasteMsg.set(null);
    this.phase.set('idle');
    this.step.set('key');
  }

  protected async removeKey(): Promise<void> {
    this.removing.set(true);
    try {
      await this.ai.removeKey(this.provider());
      this.keyStored.set(false);
      this.toasts.show('success', this.i18n.t('ai.keyRemoved'));
      this.useAnotherKey();
      if (this.isLocal()) {
        this.step.set('choose');
      }
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.removing.set(false);
    }
  }
}
