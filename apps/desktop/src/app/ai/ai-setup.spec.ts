import { cleanKey, detectProviderFromKey, KEY_PAGES, keyProblem, LOCAL_ENDPOINTS, OTHER_PROVIDERS, pickDefaultModel } from './ai-setup';

describe('detectProviderFromKey', () => {
  it('reads the provider from the prefix and keeps plain sk- undecided', () => {
    expect(detectProviderFromKey('AIzaSyA1234567890abcdefghij')).toBe('gemini');
    expect(detectProviderFromKey('gsk_abcdefghijklmnopqrstuvwx')).toBe('groq');
    expect(detectProviderFromKey('sk-ant-api03-abcdefghijklmnop')).toBe('anthropic');
    expect(detectProviderFromKey('sk-or-v1-abcdefghijklmnopqrstu')).toBe('openrouter');
    expect(detectProviderFromKey('fw_abcdefghijklmnopqrstuvwx')).toBe('fireworks');
    expect(detectProviderFromKey('sk-proj-abcdefghijklmnopqrstu')).toBeNull();
    expect(detectProviderFromKey('sk-abcdefghijklmnopqrstuvwx')).toBeNull();
    expect(detectProviderFromKey('whatever')).toBeNull();
  });
});

describe('cleanKey and keyProblem', () => {
  it('trims whitespace, quotes and a Bearer prefix', () => {
    expect(cleanKey('  abc  ')).toBe('abc');
    expect(cleanKey('"abc"')).toBe('abc');
    expect(cleanKey(" 'abc' \n")).toBe('abc');
    expect(cleanKey('“abc”')).toBe('abc');
    expect(cleanKey('Bearer abc')).toBe('abc');
    expect(cleanKey('"Bearer abc"')).toBe('abc');
  });

  it('rejects empty, spaced and short input only', () => {
    expect(keyProblem('')).toBe('empty');
    expect(keyProblem('abc def ghi jkl mno pqr')).toBe('spaces');
    expect(keyProblem('short-key')).toBe('short');
    expect(keyProblem('gsk_abcdefghijklmnopqrstuvwx')).toBeNull();
  });
});

describe('pickDefaultModel', () => {
  it('returns null for an empty list', () => {
    expect(pickDefaultModel('openai', [])).toBeNull();
  });

  it('Gemini: the newest plain flash, not lite, preview, image or tts', () => {
    const ids = ['models/gemini-2.5-pro', 'models/gemini-2.5-flash-lite', 'models/gemini-2.5-flash', 'models/gemini-2.5-flash-preview-09-2025', 'models/gemini-2.5-flash-image', 'models/gemini-2.5-flash-preview-tts', 'models/gemini-2.0-flash', 'models/text-embedding-004'];
    expect(pickDefaultModel('gemini', ids)).toBe('models/gemini-2.5-flash');
  });

  it('Gemini: falls back to a lite or preview flash, then to the first chat model', () => {
    expect(pickDefaultModel('gemini', ['gemini-2.5-pro', 'gemini-2.5-flash-lite'])).toBe('gemini-2.5-flash-lite');
    expect(pickDefaultModel('gemini', ['gemini-2.5-pro', 'embedding-001'])).toBe('gemini-2.5-pro');
  });

  it('Groq: llama-3.3-70b-versatile, else llama-3.1-8b-instant, else the first', () => {
    expect(pickDefaultModel('groq', ['llama-3.1-8b-instant', 'llama-3.3-70b-versatile', 'whisper-large-v3'])).toBe('llama-3.3-70b-versatile');
    expect(pickDefaultModel('groq', ['qwen3-32b', 'llama-3.1-8b-instant'])).toBe('llama-3.1-8b-instant');
    expect(pickDefaultModel('groq', ['whisper-large-v3', 'qwen3-32b', 'gemma2'])).toBe('qwen3-32b');
  });

  it('OpenAI: the latest -mini, ignoring dated snapshots and audio models', () => {
    const ids = ['gpt-4o', 'gpt-4o-mini', 'gpt-4.1-mini', 'gpt-5-mini', 'gpt-5-mini-2025-08-07', 'gpt-4o-mini-tts', 'text-embedding-3-small'];
    expect(pickDefaultModel('openai', ids)).toBe('gpt-5-mini');
    expect(pickDefaultModel('openai', ['gpt-4o', 'gpt-4o-mini', 'gpt-4o-mini-2024-07-18'])).toBe('gpt-4o-mini');
    expect(pickDefaultModel('openai', ['gpt-4o', 'o3'])).toBe('gpt-4o');
  });

  it('Anthropic: the latest haiku', () => {
    const ids = ['claude-sonnet-4-5-20250929', 'claude-3-5-haiku-20241022', 'claude-haiku-4-5-20251001', 'claude-opus-4-1-20250805'];
    expect(pickDefaultModel('anthropic', ids)).toBe('claude-haiku-4-5-20251001');
  });

  it('OpenRouter: a :free model, else the first', () => {
    expect(pickDefaultModel('openrouter', ['a/paid', 'b/model:free', 'c/other:free'])).toBe('b/model:free');
    expect(pickDefaultModel('openrouter', ['a/paid', 'b/model'])).toBe('a/paid');
  });

  it('DeepSeek: deepseek-chat; others and local: the first non-embedding model', () => {
    expect(pickDefaultModel('deepseek', ['deepseek-reasoner', 'deepseek-chat'])).toBe('deepseek-chat');
    expect(pickDefaultModel('mistral', ['mistral-embed', 'mistral-small-latest'])).toBe('mistral-small-latest');
    expect(pickDefaultModel('openai', ['nomic-embed-text:latest', 'llama3.2:latest'], true)).toBe('llama3.2:latest');
  });
});

describe('constants', () => {
  it('every key page is a hard-coded https URL and every provider has one', () => {
    for (const p of OTHER_PROVIDERS) {
      expect(KEY_PAGES[p].startsWith('https://')).toBeTrue();
    }
    expect(KEY_PAGES.gemini).toBe('https://aistudio.google.com/apikey');
    expect(KEY_PAGES.groq).toBe('https://console.groq.com/keys');
    expect(LOCAL_ENDPOINTS.ollama).toBe('http://127.0.0.1:11434/v1');
    expect(LOCAL_ENDPOINTS.lmstudio).toBe('http://127.0.0.1:1234/v1');
  });
});
