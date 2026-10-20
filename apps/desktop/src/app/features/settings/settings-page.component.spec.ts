import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';
import { TauriService } from '../../tauri/tauri.service';
import { SettingsPageComponent } from './settings-page.component';

describe('SettingsPageComponent update channel', () => {
  function render(version: string, channel: 'stable' | 'beta') {
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore({ 'riptide.settings': JSON.stringify({ channel }) }) },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      ],
    });
    TestBed.inject(TauriService).appInfo.set({ name: 'Riptide', version, build: 'x' });
    const fixture = TestBed.createComponent(SettingsPageComponent);
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  function selected(el: HTMLElement): string {
    const on = el.querySelector('[role="radiogroup"][aria-label="Update channel"] [aria-checked="true"]');
    return on?.textContent?.trim() ?? '';
  }

  it('shows Beta and the pre-release note on a pre-release build even when stable is saved', () => {
    const el = render('0.1.0-alpha.4', 'stable');
    expect(selected(el)).toBe('Beta');
    expect(el.textContent).toContain('Pre-release builds follow the beta channel.');
  });

  it('honours the saved channel on a stable build, without the note', () => {
    const el = render('1.0.0', 'stable');
    expect(selected(el)).toBe('Stable');
    expect(el.textContent).not.toContain('Pre-release builds follow the beta channel.');
  });
});

describe('SettingsPageComponent magnet and torrent handlers', () => {
  interface Call {
    readonly cmd: string;
  }

  function render(status: { supported: boolean; magnet: boolean; torrent: boolean }, fail = false) {
    const calls: Call[] = [];
    let current = status;
    const invoke: InvokeFn = <T>(cmd: string): Promise<T> => {
      calls.push({ cmd });
      if (cmd === 'register_protocols' || cmd === 'unregister_protocols') {
        if (fail) {
          return Promise.reject(new Error('cannot write HKCU'));
        }
        current = { supported: true, magnet: cmd === 'register_protocols', torrent: cmd === 'register_protocols' };
        return Promise.resolve(current as T);
      }
      if (cmd === 'protocol_status') {
        return Promise.resolve(current as T);
      }
      // the other rows of the page (browser bridge, video tools) are not under test here
      return Promise.reject(new Error('not in this spec'));
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: TAURI_AVAILABLE, useValue: true },
        { provide: TAURI_INVOKE, useValue: invoke },
      ],
    });
    const fixture = TestBed.createComponent(SettingsPageComponent);
    fixture.detectChanges();
    return { fixture, calls, el: fixture.nativeElement as HTMLElement };
  }

  const settle = async (fixture: { detectChanges(): void }): Promise<void> => {
    for (let i = 0; i < 15; i++) {
      await Promise.resolve();
    }
    fixture.detectChanges();
  };

  const row = (el: HTMLElement): HTMLElement => Array.from(el.querySelectorAll<HTMLElement>('.setting')).find((s) => s.textContent?.includes('Magnet links and .torrent files')) as HTMLElement;

  it('shows whether Riptide opens magnet links and offers to register it, per user', async () => {
    const { fixture, el } = render({ supported: true, magnet: false, torrent: false });
    await settle(fixture);
    expect(row(el).textContent).toContain('Not registered');
    expect(row(el).textContent).toContain('no administrator rights');
    expect(row(el).querySelector('button')?.textContent?.trim()).toBe('Use Riptide');
  });

  it('registers and unregisters through the commands and shows the new state', async () => {
    const { fixture, calls, el } = render({ supported: true, magnet: false, torrent: false });
    await settle(fixture);
    (row(el).querySelector('button') as HTMLButtonElement).click();
    await settle(fixture);
    expect(calls.map((c) => c.cmd)).toContain('register_protocols');
    expect(row(el).textContent).toContain('Registered');
    expect(row(el).querySelector('button')?.textContent?.trim()).toBe('Stop using Riptide');
    (row(el).querySelector('button') as HTMLButtonElement).click();
    await settle(fixture);
    expect(calls.map((c) => c.cmd)).toContain('unregister_protocols');
    expect(row(el).textContent).toContain('Not registered');
  });

  it('keeps the old state when the registry refuses', async () => {
    const { fixture, el } = render({ supported: true, magnet: false, torrent: false }, true);
    await settle(fixture);
    (row(el).querySelector('button') as HTMLButtonElement).click();
    await settle(fixture);
    expect(row(el).textContent).toContain('Not registered');
  });
});
