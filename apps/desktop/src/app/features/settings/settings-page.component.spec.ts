import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { RouterTestingHarness } from '@angular/router/testing';

import { AiStatus } from '../../ai/ai.models';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { I18nService, loadTable } from '../../i18n/i18n.service';
import type { EngineSettings } from '../../tauri/bridge/EngineSettings';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { TauriService } from '../../tauri/tauri.service';
import { ResearchHistoryService } from '../research/research-history.service';
import { SETTINGS_SECTION_KEY, SettingsPageComponent, sectionOf } from './settings-page.component';
import { ENGINE_SYNC_DEBOUNCE, SettingsService } from './settings.service';

beforeAll(async () => {
  await loadTable('en');
});

const ENGINE: EngineSettings = {
  default_dir: 'D:\\Downloads',
  connections: 8,
  per_host: 4,
  max_active: 3,
  user_agent: null,
  category_dirs: {},
  organize: true,
  organize_root: '',
  global_speed_limit: null,
  proxy: { mode: 'system', url: '' },
  mark_of_the_web: true,
  allow_private_redirects: false,
  auto_retry: true,
  torrent: { seeding: false, seed_ratio_percent: 100, seed_minutes: 1440, upload_limit: null, dht: true, listen_port: 0 },
};

function aiStatus(configured: boolean): AiStatus {
  return {
    settings: { provider: 'openai', model: 'gpt-x', baseUrl: null, useDownloadHistory: false, suggestNames: false, dailyTokenLimit: null, dailyRequestLimit: null, maxTokens: 1000 },
    keyHint: configured ? '••••abcd' : null,
    configured,
    usage: { day: '2026-10-25', tokens: 1200, requests: 3 },
    providers: [{ id: 'openai', name: 'OpenAI', defaultBaseUrl: 'https://api.openai.com/v1' }],
  };
}

/** A command answer that rejects. */
class Reject {
  constructor(readonly reason: unknown) {}
}

interface Call {
  readonly cmd: string;
  readonly args: Record<string, unknown> | undefined;
}

interface Options {
  readonly url?: string;
  readonly tauri?: boolean;
  readonly store?: MemoryPreferenceStore;
  readonly version?: string;
  readonly channel?: 'stable' | 'beta';
  /** Overrides for single commands: a value answers, a `Reject` rejects. */
  readonly answers?: Readonly<Record<string, unknown>>;
}

const settle = async (): Promise<void> => {
  for (let i = 0; i < 25; i++) {
    await Promise.resolve();
  }
};

async function render(options: Options = {}) {
  const calls: Call[] = [];
  let protocols = { supported: true, magnet: false, torrent: false };
  const answers: Record<string, unknown> = {
    get_engine_settings: ENGINE,
    ai_status: aiStatus(false),
    media_tools_info: { available: true, version: '2026.1.1', ffmpeg: true },
    seen_settings: false,
    seen_forget: 4,
    ...options.answers,
  };
  const invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    if (cmd === 'register_protocols' || cmd === 'unregister_protocols') {
      if (answers[cmd] instanceof Reject) {
        return Promise.reject((answers[cmd] as Reject).reason);
      }
      protocols = { supported: true, magnet: cmd === 'register_protocols', torrent: cmd === 'register_protocols' };
      return Promise.resolve(protocols as T);
    }
    if (cmd === 'protocol_status') {
      return Promise.resolve(protocols as T);
    }
    const answer = answers[cmd];
    if (answer instanceof Reject) {
      return Promise.reject(answer.reason);
    }
    return answer === undefined ? Promise.reject(new Error('not in this spec')) : Promise.resolve(answer as T);
  };
  const store = options.store ?? new MemoryPreferenceStore(options.channel ? { 'riptide.settings': JSON.stringify({ channel: options.channel }) } : {});
  TestBed.configureTestingModule({
    providers: [
      provideRouter([{ path: 'settings', component: SettingsPageComponent }]),
      { provide: PREFERENCE_STORE, useValue: store },
      { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      { provide: TAURI_AVAILABLE, useValue: options.tauri ?? true },
      { provide: TAURI_INVOKE, useValue: invoke },
      { provide: ENGINE_SYNC_DEBOUNCE, useValue: 5 },
    ],
  });
  TestBed.inject(I18nService);
  if (options.version !== undefined) {
    TestBed.inject(TauriService).appInfo.set({ name: 'Riptide', version: options.version, build: 'x' });
  }
  const harness = await RouterTestingHarness.create();
  const page = await harness.navigateByUrl(options.url ?? '/settings', SettingsPageComponent);
  harness.detectChanges();
  await settle();
  harness.detectChanges();
  const el = harness.routeNativeElement as HTMLElement;
  return { harness, page, el, calls, store, answers };
}

type Rendered = Awaited<ReturnType<typeof render>>;

async function open(r: Rendered, id: string): Promise<void> {
  r.el.querySelector<HTMLButtonElement>(`#settings-tab-${id}`)?.click();
  r.harness.detectChanges();
  await settle();
  r.harness.detectChanges();
}

const selectedId = (el: HTMLElement): string | undefined => el.querySelector('[role="tab"][aria-selected="true"]')?.id.replace('settings-tab-', '');
const titles = (el: HTMLElement): string[] => Array.from(el.querySelectorAll('.sg-title')).map((h) => h.textContent?.trim() ?? '');
const rowOf = (el: HTMLElement, text: string): HTMLElement =>
  Array.from(el.querySelectorAll<HTMLElement>('app-setting-row')).find((row) => row.querySelector('.sr-name')?.textContent?.includes(text)) as HTMLElement;
const buttonIn = (root: HTMLElement, text: string): HTMLButtonElement =>
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.trim() === text) as HTMLButtonElement;

describe('SettingsPageComponent layout', () => {
  it('lists the seven sections and renders each with its groups', async () => {
    const r = await render();
    const tabs = Array.from(r.el.querySelectorAll('[role="tab"]')).map((t) => t.textContent?.trim());
    expect(tabs).toEqual(['General', 'Downloads', 'Browser integration', 'Network', 'AI & Research', 'Privacy & Data', 'Updates & About']);
    const expectations: Record<string, string[]> = {
      general: ['Language and appearance', 'Startup and window', 'Clipboard and notifications'],
      downloads: ['Folders', 'Speed and connections', 'Queue', 'File safety', 'Torrents', 'Video support'],
      browser: ['Browser extension', 'Links and files', 'Bridge diagnostics'],
      network: ['Proxy', 'Redirects'],
      ai: ['AI assistant', 'Provider and key', 'Research sources'],
      privacy: ['What the AI can see', 'History and memory', 'Your data'],
      updates: ['Updates', 'About Riptide'],
    };
    for (const [id, expected] of Object.entries(expectations)) {
      await open(r, id);
      expect(selectedId(r.el)).withContext(id).toBe(id);
      const found = titles(r.el);
      for (const title of expected) {
        expect(found).withContext(`${id}: ${title}`).toContain(title);
      }
      expect(r.el.querySelector('[role="tabpanel"]')?.getAttribute('aria-labelledby')).toBe(`settings-tab-${id}`);
    }
  });

  it('starts on General and moves with the arrow keys, Home and End (roving tab stop)', async () => {
    const r = await render();
    expect(selectedId(r.el)).toBe('general');
    const tabs = Array.from(r.el.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
    expect(tabs.map((t) => t.tabIndex)).toEqual([0, -1, -1, -1, -1, -1, -1]);
    tabs[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    r.harness.detectChanges();
    expect(selectedId(r.el)).toBe('downloads');
    tabs[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    r.harness.detectChanges();
    expect(selectedId(r.el)).toBe('updates');
    tabs[6].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    r.harness.detectChanges();
    expect(selectedId(r.el)).toBe('general');
    tabs[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    r.harness.detectChanges();
    expect(selectedId(r.el)).toBe('updates');
    expect(tabs.map((t) => t.tabIndex)).toEqual([-1, -1, -1, -1, -1, -1, 0]);
  });

  it('offers the same sections in a selector for narrow windows', async () => {
    const r = await render();
    const picker = r.el.querySelector<HTMLSelectElement>('select.picker') as HTMLSelectElement;
    expect(picker.options.length).toBe(7);
    picker.value = 'privacy';
    picker.dispatchEvent(new Event('change'));
    r.harness.detectChanges();
    expect(selectedId(r.el)).toBe('privacy');
  });

  it('remembers the selected section', async () => {
    const store = new MemoryPreferenceStore();
    const first = await render({ store });
    await open(first, 'network');
    expect(store.get(SETTINGS_SECTION_KEY)).toBe('network');
    TestBed.resetTestingModule();
    const second = await render({ store });
    expect(selectedId(second.el)).toBe('network');
  });
});

describe('SettingsPageComponent deep links', () => {
  it('maps every old and new fragment to a section', () => {
    expect(sectionOf('ai')).toBe('ai');
    expect(sectionOf('research-sources')).toBe('ai');
    expect(sectionOf('about')).toBe('updates');
    expect(sectionOf('integration')).toBe('browser');
    expect(sectionOf('appearance')).toBe('general');
    expect(sectionOf('behaviour')).toBe('general');
    expect(sectionOf('torrents')).toBe('downloads');
    expect(sectionOf('nope')).toBeNull();
    expect(sectionOf(null)).toBeNull();
  });

  it('/settings#ai opens AI & Research with the group in the page', async () => {
    const r = await render({ url: '/settings#ai' });
    expect(selectedId(r.el)).toBe('ai');
    expect(document.getElementById('ai')?.tagName).toBe('SECTION');
  });

  it('/settings#research-sources opens AI & Research and finds the sources group', async () => {
    const r = await render({ url: '/settings#research-sources' });
    expect(selectedId(r.el)).toBe('ai');
    expect(document.getElementById('research-sources')).not.toBeNull();
  });

  it('/settings#about opens Updates & About; the fragment beats the remembered section', async () => {
    const store = new MemoryPreferenceStore({ [SETTINGS_SECTION_KEY]: 'network' });
    const r = await render({ url: '/settings#about', store });
    expect(selectedId(r.el)).toBe('updates');
    expect(store.get(SETTINGS_SECTION_KEY)).toBe('updates');
  });

  it('selecting a section puts it in the URL', async () => {
    const r = await render();
    await open(r, 'privacy');
    expect(TestBed.inject(Router).url).toContain('#privacy');
  });

});

describe('SettingsPageComponent controls write the same settings', () => {
  it('General: a switch writes its setting and the stored JSON', async () => {
    const r = await render();
    const input = rowOf(r.el, 'Keep running in the tray').querySelector<HTMLInputElement>('input[role="switch"]') as HTMLInputElement;
    expect(input.checked).toBeTrue();
    input.click();
    r.harness.detectChanges();
    expect(TestBed.inject(SettingsService).settings().closeToTray).toBeFalse();
    TestBed.tick();
    expect(JSON.parse(r.store.get('riptide.settings') ?? '{}').closeToTray).toBe(false);
  });

  it('General: the density choice is in Settings and saved', async () => {
    const r = await render();
    const compact = buttonIn(rowOf(r.el, 'Display density'), 'Compact');
    compact.click();
    expect(TestBed.inject(SettingsService).settings().density).toBe('compact');
  });

  it('Downloads: max active and auto-retry reach the engine', async () => {
    const r = await render();
    await open(r, 'downloads');
    const slider = rowOf(r.el, 'Simultaneous downloads').querySelector<HTMLInputElement>('input[type="range"]') as HTMLInputElement;
    slider.value = '6';
    slider.dispatchEvent(new Event('input'));
    (rowOf(r.el, 'Retry after network errors').querySelector('input[role="switch"]') as HTMLInputElement).click();
    r.harness.detectChanges();
    TestBed.tick();
    await new Promise((resolve) => setTimeout(resolve, 30));
    await TestBed.inject(SettingsService).whenSynced();
    const write = r.calls.filter((c) => c.cmd === 'set_engine_settings').pop()?.args?.['settings'] as EngineSettings;
    expect([write.max_active, write.auto_retry]).toEqual([6, false]);
  });

  it('Downloads: connections per download still write the same key', async () => {
    const r = await render();
    await open(r, 'downloads');
    const slider = rowOf(r.el, 'Connections per download').querySelector<HTMLInputElement>('input[type="range"]') as HTMLInputElement;
    slider.value = '12';
    slider.dispatchEvent(new Event('input'));
    expect(TestBed.inject(SettingsService).settings().connections).toBe(12);
  });

  it('Network: a custom proxy is saved, an invalid address is refused', async () => {
    const r = await render();
    await open(r, 'network');
    buttonIn(rowOf(r.el, 'Proxy mode'), 'Custom').click();
    r.harness.detectChanges();
    const field = r.el.querySelector<HTMLInputElement>('input[placeholder="socks5://127.0.0.1:1080"]') as HTMLInputElement;
    field.value = 'not a url';
    field.dispatchEvent(new Event('change'));
    r.harness.detectChanges();
    expect(r.el.querySelector('[role="alert"]')).not.toBeNull();
    expect(TestBed.inject(SettingsService).settings().proxyUrl).toBe('');
    field.value = 'socks5://127.0.0.1:1080';
    field.dispatchEvent(new Event('change'));
    r.harness.detectChanges();
    expect(TestBed.inject(SettingsService).settings().proxyUrl).toBe('socks5://127.0.0.1:1080');
    expect(TestBed.inject(SettingsService).settings().proxyMode).toBe('manual');
  });
});

describe('SettingsPageComponent browser integration', () => {
  const row = (el: HTMLElement): HTMLElement => rowOf(el, 'Magnet links and .torrent files');

  it('shows whether Riptide opens magnet links and offers to register it, per user', async () => {
    const r = await render({ url: '/settings#browser' });
    expect(row(r.el).textContent).toContain('Not registered');
    expect(row(r.el).textContent).toContain('no administrator rights');
    expect(row(r.el).querySelector('button')?.textContent?.trim()).toBe('Use Riptide');
  });

  it('registers and unregisters through the commands and shows the new state', async () => {
    const r = await render({ url: '/settings#browser' });
    row(r.el).querySelector('button')?.click();
    await settle();
    r.harness.detectChanges();
    expect(r.calls.map((c) => c.cmd)).toContain('register_protocols');
    expect(row(r.el).textContent).toContain('Registered');
    expect(row(r.el).querySelector('button')?.textContent?.trim()).toBe('Stop using Riptide');
    row(r.el).querySelector('button')?.click();
    await settle();
    r.harness.detectChanges();
    expect(r.calls.map((c) => c.cmd)).toContain('unregister_protocols');
    expect(row(r.el).textContent).toContain('Not registered');
  });

  it('keeps the old state when the registry refuses', async () => {
    const r = await render({ url: '/settings#browser', answers: { register_protocols: new Reject(new Error('cannot write HKCU')) } });
    row(r.el).querySelector('button')?.click();
    await settle();
    r.harness.detectChanges();
    expect(row(r.el).textContent).toContain('Not registered');
  });

  it('lists the bridge diagnostics', async () => {
    const r = await render({ url: '/settings#diagnostics' });
    expect(titles(r.el)).toContain('Bridge diagnostics');
    expect(rowOf(r.el, 'Bridge program')).toBeTruthy();
  });
});

describe('SettingsPageComponent AI status', () => {
  const enabledPill = (el: HTMLElement): string => el.querySelector('[data-testid="ai-enabled"]')?.textContent?.trim() ?? '';
  const providerPill = (el: HTMLElement): string => el.querySelector('[data-testid="ai-provider"]')?.textContent?.trim() ?? '';

  it('unconfigured: AI is off, there is no provider to test', async () => {
    const r = await render({ url: '/settings#ai' });
    expect(enabledPill(r.el)).toBe('Off');
    expect(providerPill(r.el)).toBe('No key');
    expect(buttonIn(r.el, 'Test connection').disabled).toBeTrue();
    expect(buttonIn(r.el, 'Quick setup')).toBeTruthy();
  });

  it('configured but never tested: enabled, provider not tested, masked key with Remove', async () => {
    const r = await render({ url: '/settings#ai', answers: { ai_status: aiStatus(true) } });
    expect(enabledPill(r.el)).toBe('Enabled');
    expect(providerPill(r.el)).toBe('Not tested');
    expect(r.el.querySelector('[data-testid="ai-key-stored"]')?.textContent).toContain('••••abcd');
    expect(buttonIn(r.el, 'Remove key')).toBeTruthy();
    expect(r.el.querySelector('[data-testid="ai-usage"]')?.textContent).toContain('1,200');
  });

  it('configured but the last test failed: enabled, provider not answering, with the reason', async () => {
    const r = await render({ url: '/settings#ai', answers: { ai_status: aiStatus(true), ai_test_connection: new Reject({ code: 'network', message: 'dns failure' }) } });
    buttonIn(r.el, 'Test connection').click();
    await settle();
    r.harness.detectChanges();
    expect(enabledPill(r.el)).toBe('Enabled');
    expect(providerPill(r.el)).toBe('Not answering');
    expect(rowOf(r.el, 'Provider connection').textContent).toContain('failed');
  });

  it('configured and the last test passed: provider working', async () => {
    const connection = { listMs: 40, modelCount: 3, completeMs: 220, model: 'gpt-x', promptTokens: 5, completionTokens: 2, estimated: false };
    const r = await render({ url: '/settings#ai', answers: { ai_status: aiStatus(true), ai_test_connection: connection } });
    buttonIn(r.el, 'Test connection').click();
    await settle();
    r.harness.detectChanges();
    expect(enabledPill(r.el)).toBe('Enabled');
    expect(providerPill(r.el)).toBe('Working');
    expect(rowOf(r.el, 'Provider connection').textContent).toContain('220');
  });

  it('forgets the test result when the key is removed', async () => {
    const connection = { listMs: 1, modelCount: 1, completeMs: 1, model: 'm', promptTokens: 1, completionTokens: 1, estimated: false };
    const r = await render({ url: '/settings#ai', answers: { ai_status: aiStatus(true), ai_test_connection: connection, ai_remove_key: aiStatus(false) } });
    buttonIn(r.el, 'Test connection').click();
    await settle();
    r.harness.detectChanges();
    expect(providerPill(r.el)).toBe('Working');
    buttonIn(r.el, 'Remove key').click();
    await settle();
    r.harness.detectChanges();
    expect(enabledPill(r.el)).toBe('Off');
    expect(providerPill(r.el)).toBe('No key');
  });
});

describe('SettingsPageComponent privacy and data', () => {
  const dialog = (el: HTMLElement): HTMLElement => el.querySelector('app-confirm-dialog') as HTMLElement;
  const dialogOpen = (el: HTMLElement): boolean => (dialog(el).querySelector('dialog') as HTMLDialogElement).open;

  function seedConversation(): ResearchHistoryService {
    const history = TestBed.inject(ResearchHistoryService);
    history.startConversation('open datasets on tides');
    return history;
  }

  it('clearing conversations asks first; Cancel keeps them, Clear deletes them', async () => {
    const r = await render({ url: '/settings#privacy' });
    const history = seedConversation();
    r.harness.detectChanges();
    buttonIn(rowOf(r.el, 'Research conversations'), 'Clear conversations').click();
    r.harness.detectChanges();
    await settle();
    expect(dialogOpen(r.el)).toBeTrue();
    expect(history.conversations().length).toBe(1);
    buttonIn(dialog(r.el), 'Cancel').click();
    r.harness.detectChanges();
    expect(dialogOpen(r.el)).toBeFalse();
    expect(history.conversations().length).toBe(1);
    buttonIn(rowOf(r.el, 'Research conversations'), 'Clear conversations').click();
    r.harness.detectChanges();
    buttonIn(dialog(r.el), 'Clear conversations').click();
    r.harness.detectChanges();
    expect(history.conversations().length).toBe(0);
  });

  it('forgetting the download memory asks first and then calls seen_forget', async () => {
    const r = await render({ url: '/settings#privacy' });
    buttonIn(rowOf(r.el, 'Clear download memory'), 'Clear download memory').click();
    r.harness.detectChanges();
    expect(r.calls.some((c) => c.cmd === 'seen_forget')).toBeFalse();
    buttonIn(dialog(r.el), 'Clear memory').click();
    await settle();
    expect(r.calls.some((c) => c.cmd === 'seen_forget')).toBeTrue();
  });

  it('deleting AI data and resetting settings each ask first', async () => {
    const r = await render({ url: '/settings#privacy', answers: { ai_forget_all: aiStatus(false) } });
    buttonIn(rowOf(r.el, 'Forget everything'), 'Forget everything').click();
    r.harness.detectChanges();
    expect(r.calls.some((c) => c.cmd === 'ai_forget_all')).toBeFalse();
    buttonIn(dialog(r.el), 'Delete AI data').click();
    await settle();
    expect(r.calls.some((c) => c.cmd === 'ai_forget_all')).toBeTrue();

    const settings = TestBed.inject(SettingsService);
    settings.patch({ connections: 20 });
    buttonIn(rowOf(r.el, 'Reset settings'), 'Reset').click();
    r.harness.detectChanges();
    expect(settings.settings().connections).toBe(20);
    buttonIn(dialog(r.el), 'Cancel').click();
    r.harness.detectChanges();
    expect(settings.settings().connections).toBe(20);
    buttonIn(rowOf(r.el, 'Reset settings'), 'Reset').click();
    r.harness.detectChanges();
    buttonIn(dialog(r.el), 'Reset settings').click();
    r.harness.detectChanges();
    expect(settings.settings().connections).toBe(8);
  });

  it('the AI switches write the AI settings', async () => {
    const r = await render({ url: '/settings#privacy', answers: { ai_status: aiStatus(true), ai_save_settings: aiStatus(true) } });
    (rowOf(r.el, 'Suggest name and category').querySelector('input[role="switch"]') as HTMLInputElement).click();
    await settle();
    const save = r.calls.find((c) => c.cmd === 'ai_save_settings');
    expect((save?.args?.['settings'] as { suggestNames: boolean }).suggestNames).toBeTrue();
  });
});

describe('SettingsPageComponent updates and about', () => {
  function selected(el: HTMLElement): string {
    const on = el.querySelector('[role="radiogroup"][aria-label="Update channel"] [aria-checked="true"]');
    return on?.textContent?.trim() ?? '';
  }

  const channelButtons = (el: HTMLElement): HTMLButtonElement[] =>
    Array.from(el.querySelectorAll<HTMLButtonElement>('[role="radiogroup"][aria-label="Update channel"] button'));

  it('shows the update card, the automatic-check switch and the app identity', async () => {
    const r = await render({ url: '/settings#updates', version: '1.0.0' });
    expect(r.el.querySelector('app-update-card')).not.toBeNull();
    expect(buttonIn(r.el, 'Check now')).toBeTruthy();
    expect(rowOf(r.el, 'Check for updates automatically')).toBeTruthy();
    expect(r.el.textContent).toContain('Riptide 1.0.0');
    expect(rowOf(r.el, 'Licences')).toBeTruthy();
  });

  it('the automatic-check switch writes checkUpdates', async () => {
    const r = await render({ url: '/settings#updates', version: '1.0.0' });
    (rowOf(r.el, 'Check for updates automatically').querySelector('input[role="switch"]') as HTMLInputElement).click();
    expect(TestBed.inject(SettingsService).settings().checkUpdates).toBeFalse();
  });

  it('shows Beta and the pre-release note on a pre-release build even when stable is saved', async () => {
    const r = await render({ url: '/settings#updates', version: '0.1.0-alpha.4', channel: 'stable' });
    expect(selected(r.el)).toBe('Beta');
    expect(r.el.textContent).toContain('Pre-release builds follow the beta channel.');
    expect(channelButtons(r.el).every((b) => b.disabled)).toBeTrue();
  });

  it('keeps the channel selector enabled on a stable build and honours the saved channel', async () => {
    const r = await render({ url: '/settings#updates', version: '1.0.0', channel: 'stable' });
    expect(selected(r.el)).toBe('Stable');
    expect(channelButtons(r.el).some((b) => b.disabled)).toBeFalse();
    expect(r.el.textContent).not.toContain('Pre-release builds follow the beta channel.');
    channelButtons(r.el)[1].click();
    expect(TestBed.inject(SettingsService).settings().channel).toBe('beta');
  });
});
