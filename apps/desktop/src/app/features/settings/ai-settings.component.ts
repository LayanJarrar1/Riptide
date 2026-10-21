import { ChangeDetectionStrategy, Component, computed, inject, OnInit, signal } from '@angular/core';

import { FORM_STYLES } from '../../ai/ai-styles';
import { AiMessage, AiSettings, ConnectionView, ModelView, ProviderId, RequestPreview } from '../../ai/ai.models';
import { AiService } from '../../ai/ai.service';
import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';

/**
 * Settings → AI. Provider, key, model, endpoint, budget, and the privacy panel. The key field is
 * a password input that is emptied the moment the key is saved; the page only ever shows the
 * masked hint the backend returns. Everything else is saved as soon as it changes.
 */
@Component({
  selector: 'app-ai-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent],
  template: `
    <section class="section" id="ai">
      <h2 class="section-title">{{ i18n.t('ai.title') }}</h2>
      <p class="intro">{{ i18n.t('ai.subtitle') }}</p>

      @if (!ai.available) {
        <p class="setting-desc note">{{ i18n.t('ai.desktopOnly') }}</p>
      } @else if (status(); as st) {
        <div class="state" [class.is-on]="st.configured" role="status">
          <span class="dot" [class.is-on]="st.configured"></span>
          <span class="state-text">{{ i18n.t(st.configured ? 'ai.status.on' : 'ai.status.off') }}</span>
        </div>
        @if (!st.configured) {
          <p class="setting-desc off-help">{{ i18n.t('ai.offHelp') }}</p>
        }

        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('ai.provider') }}</span>
            <span class="setting-desc">{{ i18n.t('ai.providerDesc') }}</span>
          </div>
          <select class="input provider" [attr.aria-label]="i18n.t('ai.provider')" (change)="onProvider($event)">
            @for (p of st.providers; track p.id) {
              <option [value]="p.id" [selected]="p.id === st.settings.provider">{{ p.name }}</option>
            }
          </select>
        </div>

        <div class="setting column">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('ai.key') }}</span>
            <span class="setting-desc">{{ i18n.t('ai.keyDesc') }}</span>
          </div>
          @if (st.keyHint; as hint) {
            <div class="key-stored">
              <app-icon name="shieldCheck" [size]="16" class="ok" />
              <span>{{ i18n.t('ai.keyStored').replace('{hint}', '') }}</span>
              <code class="hint num" dir="ltr">{{ hint }}</code>
              <button appButton variant="secondary" size="sm" icon="trash" type="button" [busy]="busy() === 'remove'" (click)="removeKey()">{{ i18n.t('ai.keyRemove') }}</button>
            </div>
          } @else {
            <p class="setting-desc">{{ i18n.t('ai.keyNone') }}</p>
          }
          <div class="key-row">
            <input
              class="input key-input"
              [type]="reveal() ? 'text' : 'password'"
              name="ai-api-key"
              dir="ltr"
              autocomplete="off"
              spellcheck="false"
              [placeholder]="i18n.t('ai.keyPlaceholder')"
              [attr.aria-label]="i18n.t('ai.key')"
              [value]="keyDraft()"
              (input)="keyDraft.set(value($event))"
              (keydown.enter)="saveKey()"
            />
            <button appButton variant="ghost" size="sm" icon="eye" [iconOnly]="true" type="button" [attr.aria-label]="i18n.t('ai.keyShow')" [attr.aria-pressed]="reveal()" (click)="reveal.set(!reveal())"></button>
            <button appButton variant="primary" type="button" [busy]="busy() === 'key'" [disabled]="keyDraft().trim().length === 0" (click)="saveKey()">{{ i18n.t('ai.keySave') }}</button>
          </div>
        </div>

        <div class="setting column">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('ai.model') }}</span>
            <span class="setting-desc">{{ i18n.t('ai.modelDesc') }}</span>
          </div>
          <div class="model-row">
            <select class="input model-pick" [attr.aria-label]="i18n.t('ai.modelPick')" [disabled]="models().length === 0" (change)="onModelPick($event)">
              <option value="">{{ models().length === 0 ? i18n.t('ai.modelPick') : i18n.t('ai.modelLoaded').replace('{n}', models().length.toString()) }}</option>
              @for (m of models(); track m.id) {
                <option [value]="m.id" [selected]="m.id === st.settings.model">{{ m.id }}</option>
              }
            </select>
            <button appButton variant="secondary" icon="retry" type="button" [busy]="busy() === 'models'" [disabled]="st.keyHint === null" (click)="loadModels()">{{ i18n.t(busy() === 'models' ? 'ai.modelLoading' : 'ai.modelLoad') }}</button>
          </div>
          <input class="input" type="text" dir="ltr" spellcheck="false" autocomplete="off" [placeholder]="i18n.t('ai.modelCustom')" [attr.aria-label]="i18n.t('ai.modelCustom')" [value]="st.settings.model" (change)="patch({ model: value($event) })" />
          @if (modelsError(); as e) {
            <p class="field-error" role="alert">{{ e }}</p>
          }
        </div>

        <div class="setting column">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('ai.baseUrl') }}</span>
            <span class="setting-desc">{{ i18n.t('ai.baseUrlDesc') }}</span>
          </div>
          <input class="input" type="text" dir="ltr" spellcheck="false" autocomplete="off" [class.is-invalid]="baseUrlError()" [attr.aria-invalid]="baseUrlError()" [placeholder]="defaultBase()" [attr.aria-label]="i18n.t('ai.baseUrl')" [value]="st.settings.baseUrl ?? ''" (change)="patchBaseUrl($event)" />
          @if (baseUrlError()) {
            <p class="field-error" role="alert">{{ i18n.t('ai.baseUrlInvalid') }}</p>
          }
        </div>

        <div class="setting column">
          <div class="test-row">
            <button appButton variant="secondary" icon="bolt" type="button" [busy]="busy() === 'test'" [disabled]="!st.configured" (click)="test()">{{ i18n.t(busy() === 'test' ? 'ai.testing' : 'ai.test') }}</button>
          </div>
          @if (connection(); as c) {
            <div class="result ok-result" role="status">
              <app-icon name="check" [size]="16" class="ok" />
              <div>
                <strong>{{ i18n.t('ai.testOk') }}.</strong>
                {{ testText(c) }}
                @if (c.estimated) {
                  <div class="setting-desc">{{ i18n.t('ai.testEstimated') }}</div>
                }
              </div>
            </div>
          }
          @if (testError(); as e) {
            <div class="result bad-result" role="alert">
              <app-icon name="alert" [size]="16" />
              <div>{{ e }}</div>
            </div>
          }
        </div>

        <div class="setting column">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('ai.budget') }}</span>
            <span class="setting-desc">{{ i18n.t('ai.budgetDesc') }}</span>
          </div>
          <div class="budget-row">
            <label class="budget-field">
              <span class="setting-desc">{{ i18n.t('ai.budgetTokens') }}</span>
              <input class="input num" type="number" min="0" step="1000" [value]="st.settings.dailyTokenLimit ?? ''" (change)="patchLimit('dailyTokenLimit', $event)" />
            </label>
            <label class="budget-field">
              <span class="setting-desc">{{ i18n.t('ai.budgetRequests') }}</span>
              <input class="input num" type="number" min="0" step="10" [value]="st.settings.dailyRequestLimit ?? ''" (change)="patchLimit('dailyRequestLimit', $event)" />
            </label>
          </div>
          <span class="setting-desc num">{{ i18n.t('ai.budgetUsed').replace('{tokens}', fmt.int(st.usage.tokens)).replace('{requests}', fmt.int(st.usage.requests)) }}</span>
        </div>

        <div class="privacy" role="group" [attr.aria-label]="i18n.t('ai.privacy')">
          <div class="privacy-head">
            <app-icon name="shieldCheck" [size]="20" class="ok" />
            <h3 class="privacy-title">{{ i18n.t('ai.privacy') }}</h3>
          </div>
          <p class="privacy-text">{{ i18n.t('ai.privacyTelemetry') }}</p>

          <label class="setting toggle-row">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('ai.history') }}</span>
              <span class="setting-desc">{{ i18n.t('ai.historyDesc') }}</span>
            </div>
            <span class="switch" [class.is-on]="st.settings.useDownloadHistory">
              <input type="checkbox" role="switch" [checked]="st.settings.useDownloadHistory" (change)="patch({ useDownloadHistory: checked($event) })" />
              <span class="knob"></span>
            </span>
          </label>
          <label class="setting toggle-row">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('ai.names') }}</span>
              <span class="setting-desc">{{ i18n.t('ai.namesDesc') }}</span>
            </div>
            <span class="switch" [class.is-on]="st.settings.suggestNames">
              <input type="checkbox" role="switch" [checked]="st.settings.suggestNames" (change)="patch({ suggestNames: checked($event) })" />
              <span class="knob"></span>
            </span>
          </label>

          <details class="preview" (toggle)="onPreviewToggle($event)">
            <summary class="preview-summary">
              <app-icon name="chevronEnd" [size]="14" class="chev" />
              {{ i18n.t('ai.preview') }}
            </summary>
            <p class="setting-desc">{{ i18n.t('ai.previewDesc') }}</p>
            <div class="preview-actions">
              <button appButton variant="ghost" size="sm" icon="retry" type="button" (click)="loadPreview()">{{ i18n.t('ai.previewRefresh') }}</button>
            </div>
            @if (preview(); as p) {
              <p class="preview-meta num">{{ previewMeta(p) }}</p>
              @for (m of p.messages; track $index) {
                <div class="msg">
                  <span class="msg-role">{{ roleLabel(m) }}</span>
                  <pre class="msg-body" dir="auto">{{ m.content }}</pre>
                </div>
              }
            } @else {
              <p class="setting-desc">{{ i18n.t('ai.previewEmpty') }}</p>
            }
          </details>
        </div>

        <div class="privacy" role="group" [attr.aria-label]="i18n.t('ai.memory')">
          <div class="privacy-head">
            <app-icon name="inbox" [size]="20" />
            <h3 class="privacy-title">{{ i18n.t('ai.memory') }}</h3>
          </div>
          <p class="privacy-text">{{ i18n.t('ai.memoryDesc') }}</p>
          <label class="setting toggle-row">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('ai.memorySkip') }}</span>
              <span class="setting-desc">{{ i18n.t('ai.memorySkipDesc') }}</span>
            </div>
            <span class="switch" [class.is-on]="skip()">
              <input type="checkbox" role="switch" [checked]="skip()" (change)="setSkip($event)" />
              <span class="knob"></span>
            </span>
          </label>
          <div class="setting">
            <div class="setting-text">
              <span class="setting-name">{{ i18n.t('ai.memoryClear') }}</span>
            </div>
            <button appButton variant="secondary" type="button" (click)="clearMemory()">{{ i18n.t('ai.memoryClear') }}</button>
          </div>
        </div>

        <div class="setting">
          <div class="setting-text">
            <span class="setting-name">{{ i18n.t('ai.forget') }}</span>
            <span class="setting-desc">{{ i18n.t('ai.forgetDesc') }}</span>
          </div>
          <button appButton variant="danger" icon="trash" type="button" [busy]="busy() === 'forget'" (click)="forget()">{{ i18n.t(confirmForget() ? 'ai.forgetConfirm' : 'ai.forget') }}</button>
        </div>
      }
    </section>
  `,
  styles: [
    FORM_STYLES,
    `
      .section {
        margin-block-start: var(--space-8);
        padding-block-start: var(--space-5);
        border-block-start: 1px solid var(--color-border);
      }

      .section-title {
        font-size: var(--text-md);
        font-weight: var(--weight-semibold);
        margin-block-end: var(--space-1);
      }

      .intro {
        color: var(--color-text-muted);
        font-size: var(--text-sm);
        margin-block-end: var(--space-3);
      }

      .note {
        padding-block: var(--space-3);
      }

      .state {
        display: inline-flex;
        align-items: center;
        gap: var(--space-2);
        padding: 4px 12px;
        border: 1px solid var(--color-border-strong);
        border-radius: var(--radius-full);
        font-size: var(--text-sm);
        font-weight: var(--weight-medium);
      }

      .state.is-on {
        border-color: var(--color-success);
      }

      .off-help {
        margin-block-start: var(--space-2);
        max-inline-size: 60ch;
      }

      .provider {
        inline-size: 220px;
      }

      .ok {
        color: var(--color-success);
        flex-shrink: 0;
      }

      .key-stored {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-2);
        font-size: var(--text-sm);
      }

      .hint {
        padding: 2px 8px;
        border-radius: var(--radius-sm);
        background: var(--color-surface-3);
        letter-spacing: 0.04em;
        unicode-bidi: isolate;
      }

      .key-row,
      .model-row,
      .test-row {
        display: flex;
        align-items: center;
        gap: var(--space-2);
      }

      .key-input,
      .model-pick {
        flex: 1;
        min-inline-size: 0;
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

      .bad-result {
        border-color: var(--color-danger);
        color: var(--color-danger);
      }

      .budget-row {
        display: flex;
        gap: var(--space-4);
      }

      .budget-field {
        display: flex;
        flex-direction: column;
        gap: 4px;
        flex: 1;
      }

      .privacy {
        margin-block-start: var(--space-5);
        padding: var(--space-4) var(--space-5);
        border: 1px solid var(--color-accent);
        border-radius: var(--radius-md);
        background: var(--color-surface-2);
      }

      .privacy-head {
        display: flex;
        align-items: center;
        gap: var(--space-2);
      }

      .privacy-title {
        font-size: var(--text-md);
        font-weight: var(--weight-semibold);
      }

      .privacy-text {
        margin-block: var(--space-2) var(--space-2);
        color: var(--color-text-secondary);
        font-size: var(--text-sm);
        line-height: var(--leading-normal);
      }

      .preview {
        margin-block-start: var(--space-3);
        border-block-start: 1px solid var(--color-border);
        padding-block-start: var(--space-3);
      }

      .preview-summary {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        cursor: pointer;
        list-style: none;
        font-weight: var(--weight-medium);
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

      .preview-actions {
        margin-block: var(--space-2);
      }

      .preview-meta {
        color: var(--color-text-muted);
        font-size: var(--text-xs);
        margin-block-end: var(--space-2);
        unicode-bidi: plaintext;
      }

      .msg {
        margin-block-end: var(--space-2);
      }

      .msg-role {
        display: block;
        font-size: var(--text-xs);
        font-weight: var(--weight-semibold);
        color: var(--color-accent);
        margin-block-end: 2px;
      }

      .msg-body {
        margin: 0;
        padding: var(--space-2) var(--space-3);
        max-block-size: 14rem;
        overflow: auto;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        background: var(--color-surface-1, var(--color-surface-3));
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: var(--text-xs);
        line-height: 1.5;
      }
    `,
  ],
})
export class AiSettingsComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly fmt = inject(FormatService);
  private readonly toasts = inject(ToastService);

  protected readonly status = this.ai.status;
  protected readonly keyDraft = signal('');
  protected readonly reveal = signal(false);
  protected readonly busy = signal<'key' | 'remove' | 'models' | 'test' | 'forget' | null>(null);
  protected readonly models = signal<readonly ModelView[]>([]);
  protected readonly modelsError = signal<string | null>(null);
  protected readonly baseUrlError = signal(false);
  protected readonly connection = signal<ConnectionView | null>(null);
  protected readonly testError = signal<string | null>(null);
  protected readonly preview = signal<RequestPreview | null>(null);
  protected readonly skip = signal(false);
  protected readonly confirmForget = signal(false);

  protected readonly defaultBase = computed(() => {
    const st = this.status();
    return st?.providers.find((p) => p.id === st.settings.provider)?.defaultBaseUrl ?? '';
  });

  async ngOnInit(): Promise<void> {
    if (!this.ai.available) {
      return;
    }
    await this.ai.refresh();
    try {
      this.skip.set(await this.ai.seenSkip());
    } catch {
      /* the switch stays off */
    }
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked;
  }

  protected testText(c: ConnectionView): string {
    return this.i18n
      .t('ai.testResult')
      .replace('{n}', this.fmt.int(c.modelCount))
      .replace('{list}', this.fmt.int(c.listMs))
      .replace('{model}', c.model)
      .replace('{complete}', this.fmt.int(c.completeMs))
      .replace('{total}', this.fmt.int(c.promptTokens + c.completionTokens))
      .replace('{prompt}', this.fmt.int(c.promptTokens))
      .replace('{completion}', this.fmt.int(c.completionTokens));
  }

  protected roleLabel(m: AiMessage): string {
    return this.i18n.t(`ai.role.${m.role}`);
  }

  protected previewMeta(p: RequestPreview): string {
    return this.i18n
      .t('ai.previewMeta')
      .replace('{feature}', this.i18n.t(`ai.feature.${p.feature}`))
      .replace('{host}', p.endpoint)
      .replace('{model}', p.model)
      .replace('{time}', p.at);
  }

  protected async patch(change: Partial<AiSettings>): Promise<void> {
    const st = this.status();
    if (st === null) {
      return;
    }
    try {
      await this.ai.saveSettings({ ...st.settings, ...change });
      this.baseUrlError.set(false);
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
      await this.ai.refresh();
    }
  }

  protected async patchBaseUrl(event: Event): Promise<void> {
    const raw = this.value(event).trim();
    const st = this.status();
    if (st === null) {
      return;
    }
    try {
      await this.ai.saveSettings({ ...st.settings, baseUrl: raw.length === 0 ? null : raw });
      this.baseUrlError.set(false);
      this.models.set([]);
    } catch {
      this.baseUrlError.set(true);
    }
  }

  protected patchLimit(key: 'dailyTokenLimit' | 'dailyRequestLimit', event: Event): void {
    const n = Number.parseInt(this.value(event), 10);
    void this.patch({ [key]: Number.isFinite(n) && n > 0 ? n : null });
  }

  protected onProvider(event: Event): void {
    const provider = (event.target as HTMLSelectElement).value as ProviderId;
    this.models.set([]);
    this.connection.set(null);
    this.testError.set(null);
    // a different provider needs a different model id
    void this.patch({ provider, model: '', baseUrl: null });
  }

  protected onModelPick(event: Event): void {
    const id = (event.target as HTMLSelectElement).value;
    if (id.length > 0) {
      void this.patch({ model: id });
    }
  }

  protected async saveKey(): Promise<void> {
    const st = this.status();
    const key = this.keyDraft().trim();
    if (st === null || key.length === 0 || this.busy() !== null) {
      return;
    }
    this.busy.set('key');
    try {
      await this.ai.setKey(st.settings.provider, key);
      this.toasts.show('success', this.i18n.t('ai.keySaved'));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      // the secret leaves the page whether or not it was accepted
      this.keyDraft.set('');
      this.reveal.set(false);
      this.busy.set(null);
    }
    if (this.status()?.keyHint != null && this.models().length === 0) {
      void this.loadModels();
    }
  }

  protected async removeKey(): Promise<void> {
    const st = this.status();
    if (st === null) {
      return;
    }
    this.busy.set('remove');
    try {
      await this.ai.removeKey(st.settings.provider);
      this.models.set([]);
      this.connection.set(null);
      this.toasts.show('success', this.i18n.t('ai.keyRemoved'));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.busy.set(null);
    }
  }

  protected async loadModels(): Promise<void> {
    const st = this.status();
    if (st === null) {
      return;
    }
    this.busy.set('models');
    this.modelsError.set(null);
    try {
      this.models.set(await this.ai.listModels(st.settings.provider));
    } catch (e: unknown) {
      this.modelsError.set(this.ai.errorText(e));
    } finally {
      this.busy.set(null);
    }
  }

  protected async test(): Promise<void> {
    this.busy.set('test');
    this.connection.set(null);
    this.testError.set(null);
    try {
      this.connection.set(await this.ai.testConnection());
      await this.ai.refresh();
    } catch (e: unknown) {
      this.testError.set(this.ai.errorText(e));
    } finally {
      this.busy.set(null);
    }
  }

  protected onPreviewToggle(event: Event): void {
    if ((event.target as HTMLDetailsElement).open) {
      void this.loadPreview();
    }
  }

  protected async loadPreview(): Promise<void> {
    try {
      this.preview.set(await this.ai.lastRequest());
    } catch {
      this.preview.set(null);
    }
  }

  protected async setSkip(event: Event): Promise<void> {
    const on = this.checked(event);
    this.skip.set(on);
    try {
      await this.ai.setSeenSkip(on);
    } catch (e: unknown) {
      this.skip.set(!on);
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }

  protected async clearMemory(): Promise<void> {
    try {
      const n = await this.ai.seenForget();
      this.toasts.show('success', this.i18n.t('ai.memoryCleared').replace('{n}', this.fmt.int(n)));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }

  /** Two clicks: the first arms the button for three seconds, the second deletes. */
  protected async forget(): Promise<void> {
    if (!this.confirmForget()) {
      this.confirmForget.set(true);
      setTimeout(() => this.confirmForget.set(false), 3000);
      return;
    }
    this.confirmForget.set(false);
    this.busy.set('forget');
    try {
      await this.ai.forgetAll();
      this.models.set([]);
      this.connection.set(null);
      this.testError.set(null);
      this.preview.set(null);
      this.keyDraft.set('');
      this.toasts.show('success', this.i18n.t('ai.forgetDone'));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.busy.set(null);
    }
  }
}
