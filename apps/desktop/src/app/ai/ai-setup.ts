import type { ProviderId } from './ai.models';

/**
 * Pure helpers for the guided AI setup: where each provider issues keys, what a key looks like,
 * and which model to pick by default. No Angular, no IO, so every rule here has a unit test.
 */

/** Where a person creates an API key. Hard-coded on purpose: nothing from the network or a file ever becomes a link. */
export const KEY_PAGES: Readonly<Record<ProviderId, string>> = {
  gemini: 'https://aistudio.google.com/apikey',
  groq: 'https://console.groq.com/keys',
  openai: 'https://platform.openai.com/api-keys',
  // console.anthropic.com redirects here since 2026
  anthropic: 'https://platform.claude.com/settings/keys',
  openrouter: 'https://openrouter.ai/keys',
  deepseek: 'https://platform.deepseek.com/api_keys',
  mistral: 'https://console.mistral.ai/api-keys',
  together: 'https://api.together.ai/settings/api-keys',
  fireworks: 'https://app.fireworks.ai/settings/users/api-keys',
  deepinfra: 'https://deepinfra.com/dash/api_keys',
};

/** Where to get the local app. */
export const OLLAMA_PAGE = 'https://ollama.com';

/** Providers whose plan is free without a card. */
export const FREE_PROVIDERS: readonly ProviderId[] = ['gemini', 'groq'];

/** The providers behind the "Another provider" card, in the order the dropdown shows them. */
export const OTHER_PROVIDERS: readonly ProviderId[] = ['openai', 'anthropic', 'openrouter', 'deepseek', 'mistral', 'together', 'fireworks', 'deepinfra'];

export type LocalKind = 'ollama' | 'lmstudio';

/** The OpenAI-compatible endpoints the local apps serve on this computer. */
export const LOCAL_ENDPOINTS: Readonly<Record<LocalKind, string>> = {
  ollama: 'http://127.0.0.1:11434/v1',
  lmstudio: 'http://127.0.0.1:1234/v1',
};

/**
 * The backend wants a key for every provider. Local servers ignore the Authorization header, so
 * this is a placeholder that protects nothing and is not a secret. It has no whitespace (the
 * backend rejects that).
 */
export const LOCAL_PLACEHOLDER_KEY = 'local-no-key';

/** Shortest string we accept as an API key (the shortest real ones are about 30 characters). */
export const MIN_KEY_LENGTH = 20;

/** Strips whitespace, surrounding quotes and a pasted `Bearer ` prefix. */
export function cleanKey(raw: string): string {
  let s = raw.trim();
  const quotes = `"'\`“”‘’`;
  while (s.length >= 2 && quotes.includes(s[0]) && quotes.includes(s[s.length - 1])) {
    s = s.slice(1, -1).trim();
  }
  if (/^bearer\s+/i.test(s)) {
    s = s.replace(/^bearer\s+/i, '').trim();
  }
  return s;
}

export type KeyProblem = 'empty' | 'spaces' | 'short';

/** `null` when the (cleaned) key can be tried; otherwise why not. */
export function keyProblem(cleaned: string): KeyProblem | null {
  if (cleaned.length === 0) {
    return 'empty';
  }
  if (/\s/.test(cleaned)) {
    return 'spaces';
  }
  if (cleaned.length < MIN_KEY_LENGTH) {
    return 'short';
  }
  return null;
}

/**
 * The provider a key's prefix gives away, or `null` when it does not say. A plain `sk-` is OpenAI
 * or DeepSeek (and others), so it says nothing: the card the user chose stays.
 */
export function detectProviderFromKey(key: string): ProviderId | null {
  const k = key.trim();
  if (k.startsWith('AIza')) {
    return 'gemini';
  }
  if (k.startsWith('gsk_')) {
    return 'groq';
  }
  if (k.startsWith('sk-ant-')) {
    return 'anthropic';
  }
  if (k.startsWith('sk-or-')) {
    return 'openrouter';
  }
  if (k.startsWith('fw_')) {
    return 'fireworks';
  }
  return null;
}

const NOT_CHAT = /embed|rerank|whisper|tts|speech|transcribe|moderation|guard|dall-e|image|realtime|audio|vision-preview/i;

/** Numbers in an id, for "latest": `claude-haiku-4-5-20251001` is [4, 5, 20251001]. */
function versionOf(id: string): number[] {
  return (id.match(/\d+/g) ?? []).map(Number);
}

function newest(ids: readonly string[]): string | null {
  let best: string | null = null;
  let bestV: number[] = [];
  for (const id of ids) {
    const v = versionOf(id);
    let cmp = 0;
    for (let i = 0; i < Math.max(v.length, bestV.length) && cmp === 0; i++) {
      cmp = (v[i] ?? -1) - (bestV[i] ?? -1);
    }
    if (best === null || cmp > 0) {
      best = id;
      bestV = v;
    }
  }
  return best;
}

/** Drops dated snapshots (`...-2025-04-14`) when an undated alias exists. */
function preferAlias(ids: readonly string[]): readonly string[] {
  const undated = ids.filter((i) => !/-\d{4}-?\d{2}-?\d{2}$/.test(i));
  return undated.length > 0 ? undated : ids;
}

/**
 * The model a first-time user gets. `local` is for Ollama / LM Studio (provider `openai` with a
 * loopback address). Returns `null` for an empty list. The user can change it in Settings → AI →
 * Advanced.
 */
export function pickDefaultModel(provider: ProviderId, models: readonly string[], local = false): string | null {
  const ids = models.filter((m) => m.length > 0);
  if (ids.length === 0) {
    return null;
  }
  const chat = ids.filter((m) => !NOT_CHAT.test(m));
  const pool = chat.length > 0 ? chat : ids;
  const first = pool[0];
  if (local) {
    return first;
  }
  switch (provider) {
    case 'gemini': {
      const flash = pool.filter((m) => /(^|\/)gemini-[\w.-]*flash/i.test(m) || /flash/i.test(m));
      const plain = flash.filter((m) => !/lite|preview|image|tts|live|audio|exp|thinking/i.test(m));
      return newest(preferAlias(plain.length > 0 ? plain : flash)) ?? first;
    }
    case 'groq':
      return pick(pool, 'llama-3.3-70b-versatile') ?? pick(pool, 'llama-3.1-8b-instant') ?? first;
    case 'openai': {
      const mini = pool.filter((m) => /^gpt-[\d.]+-mini$/.test(m));
      if (mini.length > 0) {
        return newest(mini);
      }
      const anyMini = pool.filter((m) => /-mini/.test(m) && !/codex|search/.test(m));
      return newest(preferAlias(anyMini)) ?? first;
    }
    case 'anthropic': {
      const haiku = pool.filter((m) => /haiku/i.test(m));
      return newest(preferAlias(haiku)) ?? first;
    }
    case 'openrouter':
      return pool.find((m) => m.endsWith(':free')) ?? first;
    case 'deepseek':
      return pick(pool, 'deepseek-chat') ?? first;
    default:
      return first;
  }
}

function pick(ids: readonly string[], wanted: string): string | null {
  return ids.find((m) => m === wanted) ?? null;
}
