import { ChangeDetectionStrategy, Component, inject, OnInit, signal } from '@angular/core';

import { AiService } from '../../ai/ai.service';
import { ResearchSettingsView, WebChoice } from '../../ai/research.models';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { IconComponent } from '../../ui/icon.component';
import { ToastService } from '../../ui/toast.service';
import { SettingGroupComponent, SettingRowComponent } from './setting-layout.component';

/**
 * Settings → Research sources: the two optional extras. The open sources need nothing. Jamendo
 * wants the user's own free client id; general web search is "Advanced" because search engines
 * charge for their APIs (Brave's is paid and needs a card; SearXNG is free when someone runs an
 * instance) and is off by default. Tokens and the client id go to the Windows Credential Manager
 * through the backend and come back only as a masked hint.
 */
@Component({
  selector: 'app-research-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, IconComponent, SettingGroupComponent, SettingRowComponent],
  template: `
    <app-setting-group anchor="research-sources" [heading]="i18n.t('rs.title')" [description]="i18n.t('rs.subtitle')">
      @if (!ai.available) {
        <div class="sr-block"><p class="sr-note">{{ i18n.t('ai.desktopOnly') }}</p></div>
      } @else if (view(); as v) {
        <app-setting-row [stack]="true" [label]="i18n.t('rs.jamendo')" [description]="i18n.t('rs.jamendoDesc')">
          @if (v.jamendoHint; as hint) {
            <div class="sr-fields">
              <app-icon name="shieldCheck" [size]="16" class="sr-ok" />
              <span>{{ i18n.t('rs.stored') }}</span>
              <code class="sr-hint num" dir="ltr">{{ hint }}</code>
              <button appButton variant="secondary" size="sm" icon="trash" type="button" (click)="removeSecret('jamendo')">{{ i18n.t('ai.keyRemove') }}</button>
            </div>
          }
          <div class="sr-fields">
            <input class="sr-input" type="password" name="jamendo-id" dir="ltr" autocomplete="off" spellcheck="false" [placeholder]="i18n.t('rs.jamendoPlaceholder')" [attr.aria-label]="i18n.t('rs.jamendo')" [value]="jamendoDraft()" (input)="jamendoDraft.set(value($event))" (keydown.enter)="saveSecret('jamendo')" />
            <button appButton variant="primary" type="button" [disabled]="jamendoDraft().trim().length === 0" (click)="saveSecret('jamendo')">{{ i18n.t('rs.save') }}</button>
          </div>
        </app-setting-row>

        <details class="advanced" [open]="v.settings.web !== 'none'">
          <summary class="advanced-summary">
            <app-icon name="chevronEnd" [size]="14" class="chev" />
            <span>{{ i18n.t('rs.advanced') }}</span>
          </summary>
          <app-setting-row [stack]="true" [label]="i18n.t('rs.web')" [description]="i18n.t('rs.webDesc')">
            <div class="choices" role="radiogroup" [attr.aria-label]="i18n.t('rs.web')">
              @for (c of choices; track c) {
                <label class="choice" [class.is-on]="v.settings.web === c">
                  <input type="radio" name="web-backend" [value]="c" [checked]="v.settings.web === c" (change)="pickWeb(c)" />
                  <span>{{ i18n.t('rs.web.' + c) }}</span>
                </label>
              }
            </div>

            @if (v.settings.web === 'brave') {
              <p class="sr-note">{{ i18n.t('rs.braveDesc') }}</p>
              @if (v.braveHint; as hint) {
                <div class="sr-fields">
                  <app-icon name="shieldCheck" [size]="16" class="sr-ok" />
                  <span>{{ i18n.t('rs.stored') }}</span>
                  <code class="sr-hint num" dir="ltr">{{ hint }}</code>
                  <button appButton variant="secondary" size="sm" icon="trash" type="button" (click)="removeSecret('brave')">{{ i18n.t('ai.keyRemove') }}</button>
                </div>
              }
              <div class="sr-fields">
                <input class="sr-input" type="password" name="brave-key" dir="ltr" autocomplete="off" spellcheck="false" [placeholder]="i18n.t('rs.bravePlaceholder')" [attr.aria-label]="i18n.t('rs.braveKey')" [value]="braveDraft()" (input)="braveDraft.set(value($event))" (keydown.enter)="saveSecret('brave')" />
                <button appButton variant="primary" type="button" [disabled]="braveDraft().trim().length === 0" (click)="saveSecret('brave')">{{ i18n.t('rs.save') }}</button>
              </div>
            }

            @if (v.settings.web === 'searxng') {
              <p class="sr-note">{{ i18n.t('rs.searxDesc') }}</p>
              <div class="sr-fields">
                <input class="sr-input" type="url" name="searxng-url" dir="ltr" autocomplete="off" spellcheck="false" [class.is-invalid]="urlError() !== null" [placeholder]="'https://search.example.org'" [attr.aria-label]="i18n.t('rs.searxUrl')" [value]="urlDraft()" (input)="urlDraft.set(value($event))" (keydown.enter)="saveUrl()" />
                <button appButton variant="primary" type="button" (click)="saveUrl()">{{ i18n.t('rs.save') }}</button>
              </div>
              @if (urlError(); as e) {
                <p class="sr-error" role="alert">{{ e }}</p>
              }
            }

            <span class="sr-pill" [class.is-ok]="v.webReady" role="status">
              <span class="sr-dot"></span>{{ i18n.t(v.webReady ? 'rs.webReady' : 'rs.webOff') }}
            </span>
          </app-setting-row>
        </details>
      }
    </app-setting-group>
  `,
  styles: `
    :host {
      display: block;
    }

    .choices {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
    }

    .choice {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      padding: 6px 14px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-full);
      font-size: var(--text-sm);
      cursor: pointer;
    }

    .choice.is-on {
      border-color: var(--color-accent);
      background: var(--color-accent-soft, var(--color-surface-3));
    }

    .choice input {
      accent-color: var(--color-accent);
    }

    .choice:has(input:focus-visible) {
      outline: 2px solid var(--color-focus);
      outline-offset: 2px;
    }

    .advanced-summary {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding: var(--space-3) var(--space-5);
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
      outline-offset: -2px;
    }

    .chev {
      transition: transform var(--motion-fast) var(--ease-standard);
    }

    .advanced[open] .chev {
      transform: rotate(calc(var(--dir-sign) * 90deg));
    }
  `,
})
export class ResearchSettingsComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  private readonly toasts = inject(ToastService);

  protected readonly choices: readonly WebChoice[] = ['none', 'brave', 'searxng'];
  protected readonly view = signal<ResearchSettingsView | null>(null);
  protected readonly jamendoDraft = signal('');
  protected readonly braveDraft = signal('');
  protected readonly urlDraft = signal('');
  protected readonly urlError = signal<string | null>(null);

  ngOnInit(): void {
    if (!this.ai.available) {
      return;
    }
    void this.ai
      .settingsView()
      .then((v) => {
        this.view.set(v);
        this.urlDraft.set(v.settings.searxngUrl ?? '');
      })
      .catch(() => undefined);
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected async pickWeb(web: WebChoice): Promise<void> {
    const v = this.view();
    if (v === null) {
      return;
    }
    try {
      this.view.set(await this.ai.saveResearchSettings({ ...v.settings, web }));
      this.urlError.set(null);
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }

  protected async saveUrl(): Promise<void> {
    const v = this.view();
    if (v === null) {
      return;
    }
    try {
      this.view.set(await this.ai.saveResearchSettings({ ...v.settings, searxngUrl: this.urlDraft().trim() || null }));
      this.urlError.set(null);
      this.toasts.show('success', this.i18n.t('rs.saved'));
    } catch (e: unknown) {
      this.urlError.set(this.ai.errorText(e));
    }
  }

  protected async saveSecret(name: 'brave' | 'jamendo'): Promise<void> {
    const draft = name === 'brave' ? this.braveDraft : this.jamendoDraft;
    const raw = draft().trim();
    if (raw.length === 0) {
      return;
    }
    try {
      this.view.set(await this.ai.setResearchSecret(name, raw));
      draft.set('');
      this.toasts.show('success', this.i18n.t('rs.saved'));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }

  protected async removeSecret(name: 'brave' | 'jamendo'): Promise<void> {
    try {
      this.view.set(await this.ai.removeResearchSecret(name));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }
}
