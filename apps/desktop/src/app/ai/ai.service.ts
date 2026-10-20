import { inject, Injectable, signal } from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import {
  AddRequest,
  AddResponse,
  AiFail,
  AiSettings,
  AiStatus,
  ConnectionView,
  isAiFail,
  ModelView,
  NameSuggestion,
  ProviderId,
  RequestPreview,
  SeenMatch,
  SeenQuery,
} from './ai.models';
import type { AiMessage } from './ai.models';
import type { CategoryId, ResearchSettings, ResearchSettingsView } from './research.models';

/**
 * The webview's side of Settings → AI, the Research page and the download memory. Every call is
 * a Tauri command; outside Tauri (plain browser, tests) `available` is false and nothing is
 * called. The API key travels one way only: from the field to `ai_set_key`. No command ever
 * answers with it, only with the masked hint (`••••abcd`).
 */
@Injectable({ providedIn: 'root' })
export class AiService {
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly i18n = inject(I18nService);
  /** The desktop app (and so the backend) is present. */
  readonly available = inject(TAURI_AVAILABLE);

  /** Latest status from the backend; `null` until the first answer. */
  readonly status = signal<AiStatus | null>(null);

  /** Loads (or reloads) the status. */
  async refresh(): Promise<AiStatus | null> {
    if (!this.available) {
      return null;
    }
    try {
      const s = await this.invoke<AiStatus>('ai_status');
      this.status.set(s);
      return s;
    } catch (e: unknown) {
      console.warn('[ai] ai_status failed', e);
      return null;
    }
  }

  async saveSettings(settings: AiSettings): Promise<AiStatus> {
    const s = await this.invoke<AiStatus>('ai_save_settings', { settings });
    this.status.set(s);
    return s;
  }

  async setKey(provider: ProviderId, key: string): Promise<AiStatus> {
    const s = await this.invoke<AiStatus>('ai_set_key', { provider, key });
    this.status.set(s);
    return s;
  }

  async removeKey(provider: ProviderId): Promise<AiStatus> {
    const s = await this.invoke<AiStatus>('ai_remove_key', { provider });
    this.status.set(s);
    return s;
  }

  listModels(provider: ProviderId, baseUrl: string | null): Promise<ModelView[]> {
    return this.invoke<ModelView[]>('ai_list_models', { provider, baseUrl });
  }

  testConnection(): Promise<ConnectionView> {
    return this.invoke<ConnectionView>('ai_test_connection');
  }

  lastRequest(): Promise<RequestPreview | null> {
    return this.invoke<RequestPreview | null>('ai_last_request');
  }

  async forgetAll(): Promise<AiStatus> {
    const s = await this.invoke<AiStatus>('ai_forget_all');
    this.status.set(s);
    return s;
  }

  suggestName(url: string, size: number | null): Promise<NameSuggestion> {
    return this.invoke<NameSuggestion>('ai_suggest_name', { url, size });
  }

  // ---- research and download memory

  settingsView(): Promise<ResearchSettingsView> {
    return this.invoke<ResearchSettingsView>('research_settings');
  }

  saveResearchSettings(settings: ResearchSettings): Promise<ResearchSettingsView> {
    return this.invoke<ResearchSettingsView>('research_save_settings', { settings });
  }

  setResearchSecret(name: 'brave' | 'jamendo', value: string): Promise<ResearchSettingsView> {
    return this.invoke<ResearchSettingsView>('research_set_secret', { name, value });
  }

  removeResearchSecret(name: 'brave' | 'jamendo'): Promise<ResearchSettingsView> {
    return this.invoke<ResearchSettingsView>('research_remove_secret', { name });
  }

  previewSearch(topic: string, category: CategoryId = 'all'): Promise<AiMessage[]> {
    return this.invoke<AiMessage[]>('research_preview', { topic, category });
  }

  addResults(request: AddRequest): Promise<AddResponse> {
    return this.invoke<AddResponse>('research_add', { request });
  }

  seenCheck(queries: readonly SeenQuery[]): Promise<(SeenMatch | null)[]> {
    return this.invoke<(SeenMatch | null)[]>('seen_check', { queries });
  }

  seenSkip(): Promise<boolean> {
    return this.invoke<boolean>('seen_settings');
  }

  setSeenSkip(on: boolean): Promise<void> {
    return this.invoke<void>('seen_set_skip', { on });
  }

  seenForget(): Promise<number> {
    return this.invoke<number>('seen_forget');
  }

  /** A sentence for an error from any of the calls above, in the UI language. */
  errorText(e: unknown): string {
    if (isAiFail(e)) {
      return this.failText(e);
    }
    return typeof e === 'string' ? e : e instanceof Error ? e.message : this.i18n.t('ai.error.network');
  }

  /** A translated sentence for a typed failure; the provider's own words fill `{detail}`. */
  failText(e: AiFail): string {
    const text = this.i18n.t(`ai.error.${e.code}`);
    return text.startsWith('ai.error.') ? e.message : text.replace('{detail}', e.message);
  }
}
