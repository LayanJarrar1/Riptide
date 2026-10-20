import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../core/preferences';
import { AiSettingsComponent } from '../features/settings/ai-settings.component';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';
import { AiSettings, AiStatus } from './ai.models';
import { AiService } from './ai.service';
import { SeenNoticeComponent } from './seen-notice.component';

const SECRET = 'sk-SECRET-1234567890-wxyz';

function settings(over: Partial<AiSettings> = {}): AiSettings {
  return { provider: 'deepinfra', model: '', baseUrl: null, useDownloadHistory: false, suggestNames: false, dailyTokenLimit: 100000, dailyRequestLimit: 200, maxTokens: 1024, ...over };
}

function status(over: Partial<AiStatus> = {}): AiStatus {
  return {
    settings: settings(),
    keyHint: null,
    configured: false,
    usage: { day: '2026-10-20', tokens: 0, requests: 0 },
    providers: [
      { id: 'openai', name: 'OpenAI', defaultBaseUrl: 'https://api.openai.com/v1' },
      { id: 'deepinfra', name: 'DeepInfra', defaultBaseUrl: 'https://api.deepinfra.com/v1/openai' },
    ],
    ...over,
  };
}

/** A handler result that makes the fake `invoke` reject with `reason`. */
class Reject {
  constructor(readonly reason: unknown) {}
}

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

function setup(opts: { tauri?: boolean; handler?: (cmd: string, args?: Record<string, unknown>) => unknown } = {}): { calls: Call[] } {
  const calls: Call[] = [];
  const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    const out = opts.handler?.(cmd, args);
    return out instanceof Reject ? Promise.reject(out.reason) : Promise.resolve(out as T);
  };
  TestBed.configureTestingModule({
    providers: [
      { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      { provide: TAURI_AVAILABLE, useValue: opts.tauri ?? true },
      { provide: TAURI_INVOKE, useValue: invoke },
    ],
  });
  return { calls };
}

async function settle(fixture: ComponentFixture<unknown>): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await fixture.whenStable();
    fixture.detectChanges();
  }
}

describe('AiService', () => {
  it('does nothing outside the desktop app', async () => {
    const { calls } = setup({ tauri: false });
    const ai = TestBed.inject(AiService);
    expect(ai.available).toBe(false);
    expect(await ai.refresh()).toBeNull();
    expect(calls).toEqual([]);
  });

  it('sends the key once to ai_set_key and keeps only the masked hint it gets back', async () => {
    const { calls } = setup({ handler: (cmd) => (cmd === 'ai_set_key' ? status({ keyHint: '••••wxyz', configured: false }) : undefined) });
    const ai = TestBed.inject(AiService);
    const s = await ai.setKey('deepinfra', SECRET);
    expect(calls).toEqual([{ cmd: 'ai_set_key', args: { provider: 'deepinfra', key: SECRET } }]);
    expect(JSON.stringify(s)).not.toContain(SECRET);
    expect(JSON.stringify(ai.status())).not.toContain(SECRET);
    expect(ai.status()?.keyHint).toBe('••••wxyz');
  });

  it('turns typed failures into translated sentences', () => {
    setup();
    const ai = TestBed.inject(AiService);
    expect(ai.errorText({ code: 'unauthorized', message: 'x' })).toContain('rejected the key');
    expect(ai.errorText({ code: 'bad_request', message: 'context too long' })).toContain('context too long');
    expect(ai.errorText(new Error('boom'))).toBe('boom');
  });
});

describe('AiSettingsComponent', () => {
  it('shows AI as off with the explanation until a provider is configured, and no key anywhere', async () => {
    setup({ handler: (cmd) => (cmd === 'ai_status' ? status() : false) });
    const fixture = TestBed.createComponent(AiSettingsComponent);
    fixture.detectChanges();
    await settle(fixture);
    const el = fixture.nativeElement as HTMLElement;
    expect(el.textContent).toContain('AI is off');
    expect(el.textContent).toContain('nothing is ever sent anywhere');
    expect(el.querySelector('#ai')).not.toBeNull();
    const key = el.querySelector<HTMLInputElement>('input[name="ai-api-key"]');
    expect(key?.type).toBe('password');
    // the privacy toggles start off
    const toggles = [...el.querySelectorAll<HTMLInputElement>('input[role="switch"]')];
    expect(toggles.length).toBeGreaterThanOrEqual(3);
    expect(toggles.filter((t) => t.checked).length).toBe(0);
  });

  it('saves the key, empties the field at once and renders only the hint', async () => {
    let current = status();
    const { calls } = setup({
      handler: (cmd) => {
        if (cmd === 'ai_set_key') {
          current = status({ keyHint: '••••wxyz' });
          return current;
        }
        if (cmd === 'ai_status') {
          return current;
        }
        if (cmd === 'ai_list_models') {
          return [{ id: 'google/gemma-4-26B-A4B-it', name: null }];
        }
        return false;
      },
    });
    const fixture = TestBed.createComponent(AiSettingsComponent);
    fixture.detectChanges();
    await settle(fixture);
    const el = fixture.nativeElement as HTMLElement;
    const input = el.querySelector<HTMLInputElement>('input[name="ai-api-key"]') as HTMLInputElement;
    input.value = SECRET;
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    const save = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Save key')) as HTMLButtonElement;
    save.click();
    await settle(fixture);
    expect(calls.find((c) => c.cmd === 'ai_set_key')?.args).toEqual({ provider: 'deepinfra', key: SECRET });
    expect(input.value).toBe('');
    expect(el.textContent).toContain('••••wxyz');
    expect(el.innerHTML).not.toContain(SECRET);
    // the model list was fetched with the stored key (the page never passes it again)
    expect(calls.find((c) => c.cmd === 'ai_list_models')?.args).toEqual({ provider: 'deepinfra', baseUrl: null });
  });

  it('shows the test result with latency and provider-reported tokens, and a clear error', async () => {
    let fail = false;
    setup({
      handler: (cmd) => {
        if (cmd === 'ai_status') {
          return status({ keyHint: '••••wxyz', configured: true, settings: settings({ model: 'm' }) });
        }
        if (cmd === 'ai_test_connection') {
          return fail ? new Reject({ code: 'unauthorized', message: 'nope' }) : { listMs: 120, modelCount: 42, completeMs: 800, model: 'm', promptTokens: 10, completionTokens: 3, estimated: false };
        }
        return false;
      },
    });
    const fixture = TestBed.createComponent(AiSettingsComponent);
    fixture.detectChanges();
    await settle(fixture);
    const el = fixture.nativeElement as HTMLElement;
    const test = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Test connection')) as HTMLButtonElement;
    expect(test.disabled).toBe(false);
    test.click();
    await settle(fixture);
    expect(el.textContent).toContain('Connected');
    expect(el.textContent).toContain('42 models');
    expect(el.textContent).toContain('120 ms');
    expect(el.textContent).toContain('13 tokens');
    fail = true;
    test.click();
    await settle(fixture);
    // a rejection arrives as the typed object; the sentence is the translated one
    expect(el.textContent).not.toContain('Connected.');
    expect(el.textContent).toContain('The provider rejected the key');
  });
});

describe('SeenNoticeComponent', () => {
  it('offers Open file and Download again for a live file, only the note for a stale one', () => {
    setup();
    const live = { downloadId: 1, title: 'T', filename: 'a.pdf', dir: 'C:\\d', path: 'C:\\d\\a.pdf', completedAt: 1_760_000_000_000, stale: false, matchedBy: 'url' };
    const fixture = TestBed.createComponent(SeenNoticeComponent);
    fixture.componentRef.setInput('match', live);
    fixture.componentRef.setInput('offerAgain', true);
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    expect(el.textContent).toContain('You already downloaded this on');
    expect(el.textContent).toContain('Open file');
    expect(el.textContent).toContain('Open folder');
    expect(el.textContent).toContain('Download again');
    fixture.componentRef.setInput('match', { ...live, stale: true });
    fixture.detectChanges();
    expect(el.textContent).toContain('no longer on disk');
    expect(el.textContent).not.toContain('Open file');
  });
});
