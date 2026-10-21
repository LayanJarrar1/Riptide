/** Mirrors of the Rust types in `src-tauri/src/ai.rs` and `research.rs` (camelCase over IPC). */

export type ProviderId = 'openai' | 'anthropic' | 'gemini' | 'deepseek' | 'openrouter' | 'deepinfra' | 'mistral' | 'groq' | 'together' | 'fireworks';

export interface AiSettings {
  readonly provider: ProviderId;
  readonly model: string;
  readonly baseUrl: string | null;
  readonly useDownloadHistory: boolean;
  readonly suggestNames: boolean;
  readonly dailyTokenLimit: number | null;
  readonly dailyRequestLimit: number | null;
  readonly maxTokens: number;
}

export interface AiUsage {
  readonly day: string;
  readonly tokens: number;
  readonly requests: number;
}

export interface ProviderView {
  readonly id: ProviderId;
  readonly name: string;
  readonly defaultBaseUrl: string;
}

/** Everything Settings → AI shows. There is no key in here, only the masked hint. */
export interface AiStatus {
  readonly settings: AiSettings;
  readonly keyHint: string | null;
  readonly configured: boolean;
  readonly usage: AiUsage;
  readonly providers: readonly ProviderView[];
}

export interface ModelView {
  readonly id: string;
  readonly name: string | null;
}

export interface ConnectionView {
  readonly listMs: number;
  readonly modelCount: number;
  readonly completeMs: number;
  readonly model: string;
  readonly promptTokens: number;
  readonly completionTokens: number;
  readonly estimated: boolean;
}

export type AiRole = 'system' | 'user' | 'assistant';

export interface AiMessage {
  readonly role: AiRole;
  readonly content: string;
}

export interface RequestPreview {
  readonly feature: string;
  readonly endpoint: string;
  readonly model: string;
  readonly messages: readonly AiMessage[];
  readonly at: string;
}

export interface AiFail {
  readonly code: string;
  readonly message: string;
}

export interface NameSuggestion {
  readonly filename: string;
  readonly category: string | null;
}

/** `true` for the `{ code, message }` object the backend rejects with. */
export function isAiFail(e: unknown): e is AiFail {
  return typeof e === 'object' && e !== null && typeof (e as { code?: unknown }).code === 'string' && typeof (e as { message?: unknown }).message === 'string';
}

// ---------------------------------------------------------------- research / seen

export interface SeenMatch {
  readonly downloadId: number;
  readonly title: string;
  readonly filename: string;
  readonly dir: string;
  readonly path: string;
  readonly completedAt: number;
  readonly stale: boolean;
  readonly matchedBy: string;
}

export interface SeenQuery {
  readonly url?: string;
  readonly doi?: string;
  readonly arxiv?: string;
  readonly pmid?: string;
  readonly pmcid?: string;
  readonly filename?: string;
  readonly size?: number;
  readonly sha256?: string;
}

export interface AddRequest {
  readonly ids: readonly string[];
  readonly queueId?: number | null;
  readonly category?: string | null;
  readonly startPaused?: boolean;
  readonly allowDuplicates?: boolean;
}

export interface AddResponse {
  readonly added: readonly number[];
  readonly skipped: readonly { readonly id: string; readonly reason: 'unknown' | 'page' | 'already_downloaded' | 'private_address' | 'failed' }[];
}
