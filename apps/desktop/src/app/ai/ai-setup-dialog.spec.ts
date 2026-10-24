import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../core/preferences';
import { loadTable } from '../i18n/i18n.service';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { AiSetupDialogComponent } from './ai-setup-dialog.component';
import { AiSettings, AiStatus, ModelView } from './ai.models';

const GEMINI_KEY = 'AIzaSyA-SECRET-1234567890abcdefghij';
const GROQ_KEY = 'gsk_SECRETSECRETSECRETSECRET1234';

const PROVIDERS = [
  { id: 'openai', name: 'OpenAI', defaultBaseUrl: 'https://api.openai.com/v1' },
  { id: 'gemini', name: 'Google Gemini', defaultBaseUrl: 'https://generativelanguage.googleapis.com/v1beta' },
  { id: 'groq', name: 'Groq', defaultBaseUrl: 'https://api.groq.com/openai/v1' },
  { id: 'deepinfra', name: 'DeepInfra', defaultBaseUrl: 'https://api.deepinfra.com/v1/openai' },
] as const;

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

interface World {
  /** Models `ai_list_models` answers with, or a rejection. */
  models: readonly ModelView[] | { reject: unknown };
  /** What `ai_test_connection` does. */
  test: 'ok' | { reject: unknown };
}

function setup(world: Partial<World> = {}) {
  const w: World = { models: [{ id: 'gemini-2.5-flash-lite', name: null }, { id: 'gemini-2.5-flash', name: null }], test: 'ok', ...world };
  const calls: Call[] = [];
  // the user had turned both privacy switches ON before this setup: they must stay that way
  let settings: AiSettings = { provider: 'deepinfra', model: '', baseUrl: null, useDownloadHistory: true, suggestNames: true, dailyTokenLimit: 100000, dailyRequestLimit: 200, maxTokens: 1024 };
  let hint: string | null = null;
  const status = (): AiStatus => ({ settings, keyHint: hint, configured: settings.model !== '' && hint !== null, usage: { day: '2026-10-24', tokens: 0, requests: 0 }, providers: PROVIDERS });
  const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    switch (cmd) {
      case 'ai_status':
        return Promise.resolve(status() as T);
      case 'ai_save_settings':
        settings = (args as { settings: AiSettings }).settings;
        return Promise.resolve(status() as T);
      case 'ai_set_key':
        hint = '••••wxyz';
        return Promise.resolve(status() as T);
      case 'ai_remove_key':
        hint = null;
        return Promise.resolve(status() as T);
      case 'ai_list_models':
        return 'reject' in w.models ? Promise.reject(w.models.reject) : Promise.resolve(w.models as T);
      case 'ai_test_connection':
        return w.test === 'ok' ? Promise.resolve({ listMs: 1, modelCount: 2, completeMs: 1, model: 'm', promptTokens: 1, completionTokens: 1, estimated: false } as T) : Promise.reject(w.test.reject);
      default:
        return Promise.resolve(undefined as T);
    }
  };
  TestBed.configureTestingModule({
    providers: [
      { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      { provide: TAURI_AVAILABLE, useValue: true },
      { provide: TAURI_INVOKE, useValue: invoke },
    ],
  });
  return { calls, world: w };
}

async function flush(fixture: ComponentFixture<unknown>): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await new Promise<void>((r) => setTimeout(r, 0));
    fixture.detectChanges();
  }
}

describe('AiSetupDialogComponent', () => {
  beforeAll(async () => {
    await loadTable('en');
  });

  let closed = 0;

  async function mount(): Promise<{ fixture: ComponentFixture<AiSetupDialogComponent>; el: HTMLElement }> {
    closed = 0;
    const fixture = TestBed.createComponent(AiSetupDialogComponent);
    fixture.componentInstance.closed.subscribe(() => closed++);
    fixture.componentRef.setInput('open', true);
    fixture.detectChanges();
    await flush(fixture);
    return { fixture, el: fixture.nativeElement as HTMLElement };
  }

  const button = (el: HTMLElement, text: string): HTMLButtonElement | undefined => [...el.querySelectorAll('button')].find((b) => b.textContent?.includes(text));
  const radio = (el: HTMLElement, value: string): HTMLInputElement => el.querySelector(`input[name="ai-choice"][value="${value}"]`) as HTMLInputElement;
  const keyField = (el: HTMLElement): HTMLInputElement => el.querySelector('#ai-setup-key') as HTMLInputElement;

  function typeKey(fixture: ComponentFixture<unknown>, el: HTMLElement, text: string): void {
    const f = keyField(el);
    f.value = text;
    f.dispatchEvent(new Event('input'));
    fixture.detectChanges();
  }

  async function toKeyStep(fixture: ComponentFixture<unknown>, el: HTMLElement, card = 'gemini'): Promise<void> {
    radio(el, card).click();
    fixture.detectChanges();
    button(el, 'Next')?.click();
    await flush(fixture);
  }

  function withClipboard(readText: () => Promise<string>): () => void {
    const before = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { readText } });
    return () => {
      if (before) {
        Object.defineProperty(navigator, 'clipboard', before);
      } else {
        delete (navigator as unknown as Record<string, unknown>)['clipboard'];
      }
    };
  }

  it('step 1 offers the four choices with honest cost and privacy lines, Gemini preselected', async () => {
    setup();
    const { el } = await mount();
    expect(el.textContent).toContain('Step 1 of 3');
    expect([...el.querySelectorAll('input[name="ai-choice"]')].map((i) => (i as HTMLInputElement).value)).toEqual(['gemini', 'groq', 'local', 'other']);
    expect(radio(el, 'gemini').checked).toBeTrue();
    expect(el.textContent).toContain('Free, about 1 minute');
    expect(el.textContent).toContain('Free, very fast');
    expect(el.textContent).toContain('Free and private, no key');
    expect(el.textContent).toContain('OpenAI, Anthropic, OpenRouter, DeepSeek, Mistral, Together, Fireworks, DeepInfra');
    expect(el.textContent).toContain('never your download history unless you turn that on in Settings');
    expect(el.textContent).toContain('Riptide takes no cut and has no account');
    expect(el.textContent).toContain('may use your prompts to improve its products');
    // the dropdown lists the other providers the backend knows
    const options = [...el.querySelectorAll('select option')].map((o) => o.textContent?.trim());
    expect(options).toContain('OpenAI');
  });

  it('step 2 gives three instructions and opens only the hard-coded key page', async () => {
    const { calls } = setup();
    const { fixture, el } = await mount();
    await toKeyStep(fixture, el);
    expect(el.textContent).toContain('Step 2 of 3');
    expect(el.textContent).toContain('Get your Google Gemini key');
    expect(el.querySelectorAll('ol.howto li').length).toBe(3);
    const field = keyField(el);
    expect(field.type).toBe('password');
    expect(field.getAttribute('autocomplete')).toBe('off');
    expect(field.getAttribute('spellcheck')).toBe('false');
    expect(document.activeElement).toBe(field);
    button(el, 'Open the page')?.click();
    expect(calls.find((c) => c.cmd === 'open_web_url')?.args).toEqual({ url: 'https://aistudio.google.com/apikey' });
  });

  it('reads the clipboard only when Paste is clicked, cleans the text and switches provider by prefix', async () => {
    const reads: number[] = [];
    const restore = withClipboard(() => {
      reads.push(1);
      return Promise.resolve(`  "${GROQ_KEY}"  \n`);
    });
    try {
      setup();
      const { fixture, el } = await mount();
      await toKeyStep(fixture, el, 'gemini');
      expect(reads.length).toBe(0);
      button(el, 'Paste')?.click();
      await flush(fixture);
      expect(reads.length).toBe(1);
      expect(el.textContent).toContain('That looks like a Groq key, so I switched to Groq.');
      expect(radio(fixture.nativeElement, 'groq')).toBeNull(); // step 2 shows no cards
      expect(el.textContent).toContain('Get your Groq key');
    } finally {
      restore();
    }
  });

  it('handles a refused clipboard and an empty one calmly', async () => {
    let mode: 'deny' | 'empty' = 'deny';
    const restore = withClipboard(() => (mode === 'deny' ? Promise.reject(new DOMException('no', 'NotAllowedError')) : Promise.resolve('   ')));
    try {
      setup();
      const { fixture, el } = await mount();
      await toKeyStep(fixture, el);
      button(el, 'Paste')?.click();
      await flush(fixture);
      expect(el.textContent).toContain('Riptide cannot read the clipboard here');
      expect(el.querySelector('[role="alert"]')).toBeNull();
      mode = 'empty';
      button(el, 'Paste')?.click();
      await flush(fixture);
      expect(el.textContent).toContain('The clipboard is empty');
    } finally {
      restore();
    }
  });

  it('keeps the chosen card for a plain sk- key, and rejects spaces and short input with a friendly message', async () => {
    const { calls } = setup();
    const { fixture, el } = await mount();
    await toKeyStep(fixture, el, 'other');
    typeKey(fixture, el, 'sk-proj-abcdefghijklmnopqrstuvwxyz');
    expect(el.textContent).not.toContain('I switched');
    expect(el.textContent).toContain('Get your OpenAI key');
    typeKey(fixture, el, 'sk-abc def ghi jkl mno pqr stu');
    button(el, 'Next')?.click();
    await flush(fixture);
    expect(el.textContent).toContain('has spaces inside');
    expect(keyField(el).getAttribute('aria-invalid')).toBe('true');
    typeKey(fixture, el, 'sk-short');
    button(el, 'Next')?.click();
    await flush(fixture);
    expect(el.textContent).toContain('too short to be a key');
    expect(calls.some((c) => c.cmd === 'ai_set_key')).toBeFalse();
  });

  it('runs set key, models, save, test in order, keeps the privacy switches, and never shows the key', async () => {
    const { calls } = setup();
    const { fixture, el } = await mount();
    await toKeyStep(fixture, el);
    typeKey(fixture, el, ` '${GEMINI_KEY}' `);
    // Enter in the field advances
    keyField(el).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await flush(fixture);
    const cmds = calls.map((c) => c.cmd).filter((c) => c.startsWith('ai_') && c !== 'ai_status');
    // the provider and address are saved first so the key is bound to the right endpoint
    expect(cmds).toEqual(['ai_save_settings', 'ai_set_key', 'ai_list_models', 'ai_save_settings', 'ai_test_connection']);
    expect(calls.find((c) => c.cmd === 'ai_set_key')?.args).toEqual({ provider: 'gemini', key: GEMINI_KEY });
    expect(calls.find((c) => c.cmd === 'ai_list_models')?.args).toEqual({ provider: 'gemini' });
    for (const c of calls.filter((x) => x.cmd === 'ai_save_settings')) {
      const s = (c.args as { settings: AiSettings }).settings;
      expect(s.useDownloadHistory).toBeTrue();
      expect(s.suggestNames).toBeTrue();
      expect(s.provider).toBe('gemini');
    }
    const last = calls.filter((x) => x.cmd === 'ai_save_settings').pop()?.args as { settings: AiSettings };
    expect(last.settings.model).toBe('gemini-2.5-flash');
    expect(last.settings.baseUrl).toBeNull();
    expect(el.textContent).toContain('AI is ready: Research will now plan, rank and explain results');
    expect(el.textContent).toContain('Google Gemini · gemini-2.5-flash');
    expect(el.textContent).not.toContain('SECRET');
    expect(el.innerHTML).not.toContain('SECRET');
    expect([...el.querySelectorAll('input')].every((i) => !i.value.includes('SECRET'))).toBeTrue();
    expect(keyField(el)).toBeNull();
    button(el, 'Done')?.click();
    expect(closed).toBe(1);
  });

  it('on a rejected key shows the translated reason, then Try again, Use another key and Remove key', async () => {
    const { calls, world } = setup({ test: { reject: { code: 'unauthorized', message: 'nope' } } });
    const { fixture, el } = await mount();
    await toKeyStep(fixture, el);
    typeKey(fixture, el, GEMINI_KEY);
    button(el, 'Next')?.click();
    await flush(fixture);
    expect(el.textContent).toContain('The provider rejected the key');
    expect(el.textContent).not.toContain('AI is ready');
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    for (const label of ['Try again', 'Use another key', 'Remove key']) {
      expect(button(el, label)).toBeDefined();
    }
    // Try again does not store the key a second time
    world.test = 'ok';
    button(el, 'Try again')?.click();
    await flush(fixture);
    expect(calls.filter((c) => c.cmd === 'ai_set_key').length).toBe(1);
    expect(el.textContent).toContain('AI is ready');
  });

  it('Remove key calls ai_remove_key and returns to the key step with an empty field', async () => {
    const { calls } = setup({ test: { reject: { code: 'unauthorized', message: 'nope' } } });
    const { fixture, el } = await mount();
    await toKeyStep(fixture, el);
    typeKey(fixture, el, GEMINI_KEY);
    button(el, 'Next')?.click();
    await flush(fixture);
    button(el, 'Remove key')?.click();
    await flush(fixture);
    expect(calls.find((c) => c.cmd === 'ai_remove_key')?.args).toEqual({ provider: 'gemini' });
    expect(el.textContent).toContain('Step 2 of 3');
    expect(keyField(el).value).toBe('');
  });

  it('local: sets the OpenAI adapter on the Ollama address with a placeholder key, no key step', async () => {
    const { calls } = setup({ models: [{ id: 'nomic-embed-text:latest', name: null }, { id: 'llama3.2:latest', name: null }] });
    const { fixture, el } = await mount();
    radio(el, 'local').click();
    fixture.detectChanges();
    expect(el.textContent).toContain('Step 1 of 2');
    button(el, 'Next')?.click();
    await flush(fixture);
    expect(keyField(el)).toBeNull();
    const saves = calls.filter((c) => c.cmd === 'ai_save_settings').map((c) => (c.args as { settings: AiSettings }).settings);
    expect(saves[0].provider).toBe('openai');
    expect(saves[0].baseUrl).toBe('http://127.0.0.1:11434/v1');
    expect(saves[saves.length - 1].model).toBe('llama3.2:latest');
    expect(calls.find((c) => c.cmd === 'ai_set_key')?.args).toEqual({ provider: 'openai', key: 'local-no-key' });
    expect(el.textContent).toContain('AI is ready');
  });

  it('local: LM Studio uses its own address', async () => {
    const { calls } = setup({ models: [{ id: 'qwen', name: null }] });
    const { fixture, el } = await mount();
    radio(el, 'local').click();
    fixture.detectChanges();
    (el.querySelector('input[name="ai-local-kind"][value="lmstudio"]') as HTMLInputElement).click();
    fixture.detectChanges();
    button(el, 'Next')?.click();
    await flush(fixture);
    const first = calls.find((c) => c.cmd === 'ai_save_settings')?.args as { settings: AiSettings };
    expect(first.settings.baseUrl).toBe('http://127.0.0.1:1234/v1');
  });

  it('local: when the server does not answer, says to install and start Ollama, then succeeds on Try again', async () => {
    const { calls, world } = setup({ models: { reject: { code: 'network', message: 'connection refused' } } });
    const { fixture, el } = await mount();
    radio(el, 'local').click();
    fixture.detectChanges();
    button(el, 'Next')?.click();
    await flush(fixture);
    expect(el.textContent).toContain('Ollama is not running: install it from ollama.com');
    expect(el.textContent).not.toContain('AI is ready');
    button(el, 'Open ollama.com')?.click();
    expect(calls.find((c) => c.cmd === 'open_web_url')?.args).toEqual({ url: 'https://ollama.com' });
    world.models = [{ id: 'llama3.2:latest', name: null }];
    button(el, 'Try again')?.click();
    await flush(fixture);
    expect(el.textContent).toContain('AI is ready');
    expect(calls.filter((c) => c.cmd === 'ai_set_key').length).toBe(1);
  });

  it('a provider that lists no models is a failure, not a success', async () => {
    setup({ models: [] });
    const { fixture, el } = await mount();
    await toKeyStep(fixture, el);
    typeKey(fixture, el, GEMINI_KEY);
    button(el, 'Next')?.click();
    await flush(fixture);
    expect(el.textContent).toContain('listed no models');
    expect(el.textContent).not.toContain('AI is ready');
    expect(button(el, 'Remove key')).toBeDefined();
  });

  it('Escape or the close button asks the parent to close', async () => {
    setup();
    const { fixture, el } = await mount();
    const dialog = el.querySelector('dialog') as HTMLDialogElement;
    dialog.dispatchEvent(new Event('cancel', { cancelable: true }));
    fixture.detectChanges();
    expect(closed).toBe(1);
  });
});
