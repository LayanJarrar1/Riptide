import { ChangeDetectionStrategy, Component, computed, inject, OnInit, signal } from '@angular/core';

import { AiSettings, ConnectionView, ModelView, ProviderId } from '../../ai/ai.models';
import { AiSetupDialogComponent } from '../../ai/ai-setup-dialog.component';
import { AiService } from '../../ai/ai.service';
import { FormatService } from '../../core/format.service';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { SettingGroupComponent, SettingRowComponent } from './setting-layout.component';

/** The two things the status shows apart: AI being switched on (a key is saved) and the provider answering. */
export type ProviderState = 'off' | 'untested' | 'working' | 'failing';

/**
 * Settings → AI & Research, the assistant half: status, quick setup, provider, model, key and
 * the advanced endpoint and budget. The key field is a password input that is emptied the moment
 * the key is saved; the page only ever shows the masked hint the backend returns. Everything
 * else is saved as soon as it changes. The privacy switches live in Settings → Privacy & Data.
 */
@Component({
  selector: 'app-ai-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AiSetupDialogComponent, ButtonComponent, IconComponent, SettingGroupComponent, SettingRowComponent],
  template: `
    <app-setting-group anchor="ai" [heading]="i18n.t('ai.title')" [description]="i18n.t('ai.subtitle')">
      @if (!ai.available) {
        <div class="sr-block"><p class="sr-note">{{ i18n.t('ai.desktopOnly') }}</p></div>
      } @else if (status(); as st) {
        <app-setting-row [label]="i18n.t('set.ai.enabled')" [description]="i18n.t(st.configured ? 'set.ai.enabledOn' : 'ai.offHelp')">
          <span class="sr-pill" [class.is-ok]="st.configured" role="status" data-testid="ai-enabled">
            <span class="sr-dot"></span>{{ i18n.t(st.configured ? 'set.ai.stateOn' : 'set.ai.stateOff') }}
          </span>
          <button appButton [variant]="st.configured ? 'secondary' : 'primary'" icon="sparkles" type="button" (click)="setupOpen.set(true)">{{ i18n.t(st.configured ? 'aisetup.quickChange' : 'aisetup.quick') }}</button>
        </app-setting-row>

        <app-setting-row [label]="i18n.t('set.ai.provider')" [description]="providerText()">
          <span class="sr-pill" [class.is-ok]="providerState() === 'working'" [class.is-bad]="providerState() === 'failing'" [class.is-warn]="providerState() === 'untested'" role="status" data-testid="ai-provider">
            <span class="sr-dot"></span>{{ i18n.t('set.ai.provider.' + providerState()) }}
          </span>
          <button appButton variant="secondary" icon="bolt" type="button" [busy]="busy() === 'test'" [disabled]="!st.configured" (click)="test()">{{ i18n.t(busy() === 'test' ? 'ai.testing' : 'ai.test') }}</button>
        </app-setting-row>

        @if (connection(); as c) {
          <div class="sr-block result" role="status">
            <app-icon name="check" [size]="16" class="sr-ok" />
            <div>
              {{ testText(c) }}
              @if (c.estimated) {
                <div class="sr-note">{{ i18n.t('ai.testEstimated') }}</div>
              }
            </div>
          </div>
        }

        <app-setting-row [label]="i18n.t('set.ai.usage')" [description]="i18n.t('set.ai.usageDesc')">
          <span class="sr-value num" data-testid="ai-usage">{{ usageText(st.usage.tokens, st.usage.requests) }}</span>
        </app-setting-row>
      }
    </app-setting-group>

    @if (ai.available && status(); as st) {
      <app-setting-group [heading]="i18n.t('set.g.provider')">
        <app-setting-row [field]="true" [label]="i18n.t('ai.provider')" [description]="i18n.t('ai.providerDesc')">
          <select class="sr-input" [attr.aria-label]="i18n.t('ai.provider')" (change)="onProvider($event)">
            @for (p of st.providers; track p.id) {
              <option [value]="p.id" [selected]="p.id === st.settings.provider">{{ p.name }}</option>
            }
          </select>
        </app-setting-row>

        <app-setting-row [stack]="true" [label]="i18n.t('ai.model')" [description]="i18n.t('ai.modelDesc')">
          <div class="sr-fields">
            <select class="sr-input" [attr.aria-label]="i18n.t('ai.modelPick')" [disabled]="models().length === 0" (change)="onModelPick($event)">
              <option value="">{{ models().length === 0 ? i18n.t('ai.modelPick') : i18n.t('ai.modelLoaded').replace('{n}', models().length.toString()) }}</option>
              @for (m of models(); track m.id) {
                <option [value]="m.id" [selected]="m.id === st.settings.model">{{ m.id }}</option>
              }
            </select>
            <button appButton variant="secondary" icon="retry" type="button" [busy]="busy() === 'models'" [disabled]="st.keyHint === null" (click)="loadModels()">{{ i18n.t(busy() === 'models' ? 'ai.modelLoading' : 'ai.modelLoad') }}</button>
          </div>
          <input class="sr-input" type="text" dir="ltr" spellcheck="false" autocomplete="off" [placeholder]="i18n.t('ai.modelCustom')" [attr.aria-label]="i18n.t('ai.modelCustom')" [value]="st.settings.model" (change)="patch({ model: value($event) })" />
          @if (modelsError(); as e) {
            <p class="sr-error" role="alert">{{ e }}</p>
          }
        </app-setting-row>

        <app-setting-row [stack]="true" [label]="i18n.t('ai.key')" [description]="i18n.t('ai.keyDesc')">
          @if (st.keyHint; as hint) {
            <div class="sr-fields" data-testid="ai-key-stored">
              <app-icon name="shieldCheck" [size]="16" class="sr-ok" />
              <span>{{ i18n.t('ai.keyStored').replace('{hint}', '') }}</span>
              <code class="sr-hint num" dir="ltr">{{ hint }}</code>
              <button appButton variant="secondary" size="sm" icon="trash" type="button" [busy]="busy() === 'remove'" (click)="removeKey()">{{ i18n.t('ai.keyRemove') }}</button>
            </div>
          } @else {
            <p class="sr-note">{{ i18n.t('ai.keyNone') }}</p>
          }
          <div class="sr-fields">
            <input
              class="sr-input"
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
        </app-setting-row>
      </app-setting-group>

      <details class="advanced">
        <summary class="advanced-summary">
          <app-icon name="chevronEnd" [size]="14" class="chev" />
          <span>{{ i18n.t('ai.advanced') }}</span>
          <span class="sr-note">{{ i18n.t('set.ai.advancedDesc') }}</span>
        </summary>
        <app-setting-group [heading]="i18n.t('set.g.advancedAi')">
          <app-setting-row [stack]="true" [label]="i18n.t('ai.baseUrl')" [description]="i18n.t('ai.baseUrlDesc')">
            <input class="sr-input" type="text" dir="ltr" spellcheck="false" autocomplete="off" [class.is-invalid]="baseUrlError()" [attr.aria-invalid]="baseUrlError()" [placeholder]="defaultBase()" [attr.aria-label]="i18n.t('ai.baseUrl')" [value]="st.settings.baseUrl ?? ''" (change)="patchBaseUrl($event)" />
            @if (baseUrlError()) {
              <p class="sr-error" role="alert">{{ i18n.t('ai.baseUrlInvalid') }}</p>
            }
          </app-setting-row>
          <app-setting-row [field]="true" [label]="i18n.t('ai.budgetTokens')" [description]="i18n.t('ai.budgetDesc')">
            <input class="sr-input num" type="number" min="0" step="1000" dir="ltr" [attr.aria-label]="i18n.t('ai.budgetTokens')" [value]="st.settings.dailyTokenLimit ?? ''" (change)="patchLimit('dailyTokenLimit', $event)" />
          </app-setting-row>
          <app-setting-row [field]="true" [label]="i18n.t('ai.budgetRequests')">
            <input class="sr-input num" type="number" min="0" step="10" dir="ltr" [attr.aria-label]="i18n.t('ai.budgetRequests')" [value]="st.settings.dailyRequestLimit ?? ''" (change)="patchLimit('dailyRequestLimit', $event)" />
          </app-setting-row>
        </app-setting-group>
      </details>
    }
    <app-ai-setup-dialog [open]="setupOpen()" (closed)="onSetupClosed()" />
  `,
  styles: `
    :host {
      display: block;
    }

    .result {
      display: flex;
      align-items: flex-start;
      gap: var(--space-2);
      font-size: var(--text-sm);
      line-height: var(--leading-normal);
    }

    .advanced {
      margin-block-end: var(--space-8);
    }

    .advanced-summary {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin-block-end: var(--space-3);
      cursor: pointer;
      list-style: none;
      font-weight: var(--weight-medium);
      user-select: none;
    }

    .advanced-summary::-webkit-details-marker {
      display: none;
    }

    .advanced-summary:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
      border-radius: var(--radius-sm);
    }

    .chev {
      transition: transform var(--motion-fast) var(--ease-standard);
    }

    .advanced[open] > summary .chev {
      transform: rotate(calc(var(--dir-sign) * 90deg));
    }
  `,
})
export class AiSettingsComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly fmt = inject(FormatService);

  /** "Today: 1,200 tokens in 3 requests", each count in the plural form of the language. */
  protected usageText(tokens: number, requests: number): string {
    return this.i18n
      .t('ai.budgetUsed')
      .replace('{tokens}', this.i18n.plural('ai.usageTokens', tokens).replace('{n}', this.fmt.int(tokens)))
      .replace('{requests}', this.i18n.plural('ai.usageRequests', requests).replace('{n}', this.fmt.int(requests)));
  }
  private readonly toasts = inject(ToastService);

  protected readonly status = this.ai.status;
  protected readonly keyDraft = signal('');
  protected readonly reveal = signal(false);
  protected readonly busy = signal<'key' | 'remove' | 'models' | 'test' | null>(null);
  protected readonly models = signal<readonly ModelView[]>([]);
  protected readonly modelsError = signal<string | null>(null);
  protected readonly baseUrlError = signal(false);
  protected readonly connection = signal<ConnectionView | null>(null);
  protected readonly setupOpen = signal(false);

  /**
   * Whether the provider is actually answering, apart from AI being on: `off` (no key), `untested`
   * (a key is saved but no test ran this session), `working` / `failing` after the last test.
   */
  protected readonly providerState = computed<ProviderState>(() => {
    if (this.status()?.configured !== true) {
      return 'off';
    }
    const last = this.ai.lastTest();
    if (last === null) {
      return 'untested';
    }
    return last.ok ? 'working' : 'failing';
  });

  protected readonly providerText = computed(() => {
    const last = this.ai.lastTest();
    switch (this.providerState()) {
      case 'off':
        return this.i18n.t('set.ai.providerOffDesc');
      case 'untested':
        return this.i18n.t('set.ai.providerUntestedDesc');
      case 'working':
        return this.i18n
          .t('set.ai.providerWorkingDesc')
          .replace('{model}', last?.model ?? '?')
          .replace('{ms}', this.fmt.int(last?.ms ?? 0))
          .replace('{time}', this.timeOf(last?.at ?? 0));
      case 'failing':
        return this.i18n
          .t('set.ai.providerFailingDesc')
          .replace('{error}', last?.error ?? '')
          .replace('{time}', this.timeOf(last?.at ?? 0));
    }
  });

  protected readonly defaultBase = computed(() => {
    const st = this.status();
    return st?.providers.find((p) => p.id === st.settings.provider)?.defaultBaseUrl ?? '';
  });

  async ngOnInit(): Promise<void> {
    if (!this.ai.available) {
      return;
    }
    await this.ai.refresh();
  }

  private timeOf(at: number): string {
    return new Date(at).toLocaleTimeString(this.i18n.lang(), { hour: '2-digit', minute: '2-digit' });
  }

  /** The guided setup closed: what it saved is already in the status, so the cached lists are dropped. */
  protected onSetupClosed(): void {
    this.setupOpen.set(false);
    this.models.set([]);
    this.modelsError.set(null);
    this.connection.set(null);
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
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
    try {
      // the service records the outcome in `lastTest`, which the status reads
      this.connection.set(await this.ai.testConnection());
      await this.ai.refresh();
    } catch {
      /* shown by the status: the reason is in lastTest */
    } finally {
      this.busy.set(null);
    }
  }
}
