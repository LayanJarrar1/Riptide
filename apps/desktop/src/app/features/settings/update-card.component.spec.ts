import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { NOW, UPDATE_INTERVAL_KEY, UpdateInfo, UpdateService } from '../../core/update.service';
import { I18nService, loadTable } from '../../i18n/i18n.service';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';
import { UpdateCardComponent } from './update-card.component';

// the translated sentences these specs check must not depend on which spec ran before
beforeAll(async () => {
  await loadTable('en');
});

const T0 = 1_800_000_000_000;
const UPDATE: UpdateInfo = { version: '0.1.0-beta.2', current_version: '0.1.0-beta.1', notes: 'Faster downloads.', date: null };

describe('UpdateCardComponent', () => {
  let answer: UpdateInfo | null;
  let failing: Record<string, unknown>;
  let calls: string[];
  let store: MemoryPreferenceStore;
  let fixture: ComponentFixture<UpdateCardComponent>;
  let updates: UpdateService;

  function render(prefs: Record<string, string> = {}, version = '0.1.0-beta.1'): HTMLElement {
    answer = UPDATE;
    failing = {};
    calls = [];
    store = new MemoryPreferenceStore(prefs);
    const invoke: InvokeFn = <T>(cmd: string): Promise<T> => {
      calls.push(cmd);
      if (cmd === 'app_info') {
        return Promise.resolve({ name: 'Riptide', version, build: 'x' } as unknown as T);
      }
      if (failing[cmd] !== undefined) {
        return Promise.reject(failing[cmd]);
      }
      return Promise.resolve((cmd === 'check_update' ? answer : cmd === 'active_download_count' ? 0 : undefined) as unknown as T);
    };
    const listen: ListenFn = () => Promise.resolve(() => undefined);
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: store },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: TAURI_AVAILABLE, useValue: true },
        { provide: TAURI_INVOKE, useValue: invoke },
        { provide: TAURI_LISTEN, useValue: listen },
        { provide: NOW, useValue: () => T0 },
      ],
    });
    TestBed.inject(I18nService);
    updates = TestBed.inject(UpdateService);
    fixture = TestBed.createComponent(UpdateCardComponent);
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  async function refresh(): Promise<string> {
    await fixture.whenStable();
    for (let i = 0; i < 20; i++) {
      await Promise.resolve();
    }
    fixture.detectChanges();
    return (fixture.nativeElement as HTMLElement).textContent?.replace(/\s+/g, ' ') ?? '';
  }

  const button = (label: string): HTMLButtonElement | undefined =>
    Array.from((fixture.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes(label));

  it('shows the version, the channel and "never checked" at first', async () => {
    render();
    const text = await refresh();
    expect(text).toContain('Version 0.1.0-beta.1');
    expect(text).toContain('Channel: Beta');
    expect(text).toContain('Never checked');
    expect(text).toContain('Not checked yet');
    expect(button('Check now')).toBeDefined();
  });

  it('says up to date when there is no update', async () => {
    render();
    answer = null;
    await updates.check();
    expect(await refresh()).toContain('Riptide is up to date');
    expect(await refresh()).toContain('Last checked');
  });

  it('shows the new version with its release notes and the update button', async () => {
    render();
    await updates.check();
    const text = await refresh();
    expect(text).toContain('Version 0.1.0-beta.2 is available');
    expect(text).toContain('Faster downloads.');
    expect(button('Update and restart')).toBeDefined();
  });

  it('"Check now" runs a check', async () => {
    render();
    button('Check now')?.click();
    await refresh();
    expect(calls).toContain('check_update');
  });

  it('explains an offline failure and offers a retry', async () => {
    render();
    failing['check_update'] = { kind: 'offline', message: 'x' };
    await updates.check().catch(() => undefined);
    const text = await refresh();
    expect(text).toContain('The update did not finish');
    expect(text).toContain('No connection to the update server.');
    expect(button('Try again')).toBeDefined();
  });

  it('says a bad signature was refused and nothing was installed', async () => {
    render();
    await updates.check();
    failing['download_update'] = { kind: 'signature', message: 'x' };
    await updates.download().catch(() => undefined);
    const text = await refresh();
    expect(text).toContain('not signed by Riptide, so it was not installed');
    expect(button('Try again')).toBeDefined();
    expect(calls).not.toContain('install_update');
  });

  it('shows download progress', async () => {
    render();
    await updates.check();
    updates.status.set('downloading');
    updates.progress.set({ downloaded: 250, total: 1000 });
    const text = await refresh();
    expect(text).toContain('Downloading version 0.1.0-beta.2 (25%)');
    const bar = (fixture.nativeElement as HTMLElement).querySelector('[role="progressbar"]');
    expect(bar?.getAttribute('aria-valuenow')).toBe('25');
  });

  it('says a verified package waits for the restart', async () => {
    render();
    await updates.check();
    await updates.download();
    expect(await refresh()).toContain('downloaded and verified. Restart to finish.');
  });

  it('with downloads running offers "when downloads finish" or "now", never installs by itself', async () => {
    render();
    await updates.check();
    failing['install_update'] = { kind: 'downloads_active', message: '2 downloads are active' };
    await refresh();
    button('Update and restart')?.click();
    const text = await refresh();
    expect(text).toContain('Downloads are running. Choose when to install the update.');
    expect(button('Install when downloads finish')).toBeDefined();
    expect(button('Install now')).toBeDefined();
    expect(calls.filter((c) => c === 'install_update').length).toBe(1); // the refused attempt only
  });

  it('shows the armed wait and lets the user cancel it', async () => {
    render();
    await updates.check();
    updates.installScheduled.set(true);
    const text = await refresh();
    expect(text).toContain('will install when your downloads finish');
    button('Cancel')?.click();
    await refresh();
    expect(updates.installScheduled()).toBeFalse();
  });

  it('remembers the failure of an earlier session', async () => {
    render({ 'riptide.updates.lastError': JSON.stringify({ kind: 'server', at: T0 - 60_000 }) });
    const text = await refresh();
    expect(text).toContain('The update server did not answer with a release.');
  });

  it('changes the interval of the automatic checks', async () => {
    const el = render();
    const select = el.querySelector<HTMLSelectElement>('select');
    expect(select?.value).toBe('6');
    select!.value = '24';
    select!.dispatchEvent(new Event('change'));
    expect(store.get(UPDATE_INTERVAL_KEY)).toBe('24');
    expect(updates.intervalHours()).toBe(24);
  });

  it('shows the new version after an update', async () => {
    render({ 'riptide.updates.lastRunVersion': '0.1.0-beta.1' }, '0.1.0-beta.2');
    await updates.announceUpdated();
    const text = await refresh();
    expect(text).toContain('Version 0.1.0-beta.2');
  });
});
