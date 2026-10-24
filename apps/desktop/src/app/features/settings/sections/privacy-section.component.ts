import { ChangeDetectionStrategy, Component, computed, inject, OnInit, signal } from '@angular/core';

import { AiMessage, AiSettings, RequestPreview } from '../../../ai/ai.models';
import { AiService } from '../../../ai/ai.service';
import { FormatService } from '../../../core/format.service';
import { I18nService } from '../../../i18n/i18n.service';
import { ButtonComponent } from '../../../ui/button.component';
import { ConfirmDialogComponent } from '../../../ui/confirm-dialog.component';
import { IconComponent } from '../../../ui/icon.component';
import { SwitchComponent } from '../../../ui/switch.component';
import { ToastService } from '../../../ui/toast.service';
import { ResearchHistoryService } from '../../research/research-history.service';
import { SettingGroupComponent, SettingRowComponent } from '../setting-layout.component';
import { SettingsService } from '../settings.service';

/** The destructive actions of this page; each one asks first. */
export type PrivacyAction = 'conversations' | 'memory' | 'aiData' | 'reset';

/**
 * Settings → Privacy & Data: what the AI may see, what Riptide remembers, and the buttons that
 * delete it. Every deletion goes through a confirmation dialog.
 */
@Component({
  selector: 'app-settings-privacy',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SettingGroupComponent, SettingRowComponent, SwitchComponent, ButtonComponent, IconComponent, ConfirmDialogComponent],
  template: `
    <app-setting-group anchor="privacy" [heading]="i18n.t('set.g.aiSees')" [description]="i18n.t('ai.privacyTelemetry')">
      @if (!ai.available) {
        <div class="sr-block"><p class="sr-note">{{ i18n.t('ai.desktopOnly') }}</p></div>
      } @else if (status(); as st) {
        <app-setting-row [clickable]="true" [label]="i18n.t('ai.history')" [description]="i18n.t('ai.historyDesc')">
          <app-switch [checked]="st.settings.useDownloadHistory" [label]="i18n.t('ai.history')" (checkedChange)="patchAi({ useDownloadHistory: $event })" />
        </app-setting-row>
        <app-setting-row [clickable]="true" [label]="i18n.t('ai.names')" [description]="i18n.t('ai.namesDesc')">
          <app-switch [checked]="st.settings.suggestNames" [label]="i18n.t('ai.names')" (checkedChange)="patchAi({ suggestNames: $event })" />
        </app-setting-row>
        <details class="preview" (toggle)="onPreviewToggle($event)">
          <summary class="preview-summary">
            <app-icon name="chevronEnd" [size]="14" class="chev" />
            {{ i18n.t('ai.preview') }}
          </summary>
          <div class="preview-body">
            <p class="sr-note">{{ i18n.t('ai.previewDesc') }}</p>
            <div><button appButton variant="ghost" size="sm" icon="retry" type="button" (click)="loadPreview()">{{ i18n.t('ai.previewRefresh') }}</button></div>
            @if (preview(); as p) {
              <p class="sr-note num meta">{{ previewMeta(p) }}</p>
              @for (m of p.messages; track $index) {
                <div>
                  <span class="msg-role">{{ roleLabel(m) }}</span>
                  <pre class="msg-body" dir="auto">{{ m.content }}</pre>
                </div>
              }
            } @else {
              <p class="sr-note">{{ i18n.t('ai.previewEmpty') }}</p>
            }
          </div>
        </details>
      }
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.memory')" [description]="i18n.t('ai.memoryDesc')">
      @if (ai.available) {
        <app-setting-row [clickable]="true" [label]="i18n.t('ai.memorySkip')" [description]="i18n.t('ai.memorySkipDesc')">
          <app-switch [checked]="skip()" [label]="i18n.t('ai.memorySkip')" (checkedChange)="setSkip($event)" />
        </app-setting-row>
        <app-setting-row [label]="i18n.t('ai.memoryClear')" [description]="i18n.t('set.memoryClearDesc')">
          <button appButton variant="secondary" icon="trash" type="button" (click)="ask('memory')">{{ i18n.t('ai.memoryClear') }}</button>
        </app-setting-row>
      }
      <app-setting-row [label]="i18n.t('set.conversations')" [description]="conversationsText()">
        <button appButton variant="secondary" icon="trash" type="button" [disabled]="history.conversations().length === 0" (click)="ask('conversations')">{{ i18n.t('set.conversationsClear') }}</button>
      </app-setting-row>
    </app-setting-group>

    <app-setting-group [heading]="i18n.t('set.g.data')">
      @if (ai.available) {
        <app-setting-row [label]="i18n.t('ai.forget')" [description]="i18n.t('ai.forgetDesc')">
          <button appButton variant="danger" icon="trash" type="button" [busy]="busy() === 'aiData'" (click)="ask('aiData')">{{ i18n.t('ai.forget') }}</button>
        </app-setting-row>
      }
      <app-setting-row [label]="i18n.t('settings.reset')" [description]="i18n.t('settings.resetDesc')">
        <button appButton variant="danger" type="button" (click)="ask('reset')">{{ i18n.t('settings.resetAction') }}</button>
      </app-setting-row>
    </app-setting-group>

    <app-confirm-dialog
      [open]="pending() !== null"
      [title]="confirmTitle()"
      [message]="confirmMessage()"
      [confirmLabel]="confirmLabel()"
      (confirmed)="confirm()"
      (cancelled)="pending.set(null)"
    />
  `,
  styles: `
    :host {
      display: block;
    }

    .preview {
      border-block-start: 1px solid var(--color-border);
    }

    .preview-summary {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding: var(--space-3) var(--space-5);
      cursor: pointer;
      list-style: none;
      font-weight: var(--weight-medium);
      user-select: none;
    }

    .preview-summary::-webkit-details-marker {
      display: none;
    }

    .preview-summary:focus-visible {
      outline: 2px solid var(--color-focus);
      outline-offset: -2px;
    }

    .preview-body {
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      padding: 0 var(--space-5) var(--space-4);
    }

    .chev {
      transition: transform var(--motion-fast) var(--ease-standard);
    }

    .preview[open] .chev {
      transform: rotate(calc(var(--dir-sign) * 90deg));
    }

    .meta {
      font-size: var(--text-xs);
      unicode-bidi: plaintext;
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
      background: var(--color-surface-2);
      font-family: var(--font-mono, ui-monospace, monospace);
      font-size: var(--text-xs);
      line-height: 1.5;
    }
  `,
})
export class PrivacySectionComponent implements OnInit {
  protected readonly i18n = inject(I18nService);
  protected readonly ai = inject(AiService);
  protected readonly history = inject(ResearchHistoryService);
  private readonly settings = inject(SettingsService);
  private readonly fmt = inject(FormatService);
  private readonly toasts = inject(ToastService);

  protected readonly status = this.ai.status;
  protected readonly skip = signal(false);
  protected readonly preview = signal<RequestPreview | null>(null);
  protected readonly busy = signal<PrivacyAction | null>(null);
  /** The destructive action waiting for a yes. */
  protected readonly pending = signal<PrivacyAction | null>(null);

  protected readonly conversationsText = computed(() => {
    const n = this.history.conversations().length;
    return n === 0 ? this.i18n.t('set.conversationsNone') : this.i18n.t('set.conversationsDesc').replace('{n}', this.fmt.int(n));
  });

  protected readonly confirmTitle = computed(() => this.i18n.t(`set.confirm.${this.pending() ?? 'reset'}.title`));
  protected readonly confirmMessage = computed(() => this.i18n.t(`set.confirm.${this.pending() ?? 'reset'}.message`));
  protected readonly confirmLabel = computed(() => this.i18n.t(`set.confirm.${this.pending() ?? 'reset'}.action`));

  async ngOnInit(): Promise<void> {
    if (!this.ai.available) {
      return;
    }
    if (this.ai.status() === null) {
      await this.ai.refresh();
    }
    try {
      this.skip.set(await this.ai.seenSkip());
    } catch {
      /* the switch stays off */
    }
  }

  protected ask(action: PrivacyAction): void {
    this.pending.set(action);
  }

  /** The user said yes in the dialog. */
  protected async confirm(): Promise<void> {
    const action = this.pending();
    this.pending.set(null);
    switch (action) {
      case 'conversations':
        this.history.clearAll();
        this.toasts.show('success', this.i18n.t('set.conversationsCleared'));
        break;
      case 'memory':
        await this.clearMemory();
        break;
      case 'aiData':
        await this.forgetAi();
        break;
      case 'reset':
        this.settings.reset();
        this.toasts.show('info', this.i18n.t('toast.settingsReset'));
        break;
      case null:
        break;
    }
  }

  private async clearMemory(): Promise<void> {
    try {
      const n = await this.ai.seenForget();
      this.toasts.show('success', this.i18n.t('ai.memoryCleared').replace('{n}', this.fmt.int(n)));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    }
  }

  private async forgetAi(): Promise<void> {
    this.busy.set('aiData');
    try {
      await this.ai.forgetAll();
      this.preview.set(null);
      this.toasts.show('success', this.i18n.t('ai.forgetDone'));
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
    } finally {
      this.busy.set(null);
    }
  }

  protected async patchAi(change: Partial<AiSettings>): Promise<void> {
    const st = this.status();
    if (st === null) {
      return;
    }
    try {
      await this.ai.saveSettings({ ...st.settings, ...change });
    } catch (e: unknown) {
      this.toasts.show('danger', this.ai.errorText(e));
      await this.ai.refresh();
    }
  }

  protected async setSkip(on: boolean): Promise<void> {
    this.skip.set(on);
    try {
      await this.ai.setSeenSkip(on);
    } catch (e: unknown) {
      this.skip.set(!on);
      this.toasts.show('danger', this.ai.errorText(e));
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
}
