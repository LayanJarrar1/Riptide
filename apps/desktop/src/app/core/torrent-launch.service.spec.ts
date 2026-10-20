import { TestBed } from '@angular/core/testing';

import { UiState } from '../features/shell/ui-state.service';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { OPEN_TORRENT_EVENT, TorrentLaunchService } from './torrent-launch.service';

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
  }
}

describe('TorrentLaunchService', () => {
  let pending: unknown;
  let handler: ((payload: unknown) => void) | null;
  let listened: string[];

  function configure(tauri: boolean): UiState {
    handler = null;
    listened = [];
    const invoke: InvokeFn = <T>(cmd: string): Promise<T> => Promise.resolve((cmd === 'take_pending_open' ? pending : undefined) as T);
    const listen: ListenFn = <T>(event: string, h: (payload: T) => void): Promise<() => void> => {
      listened.push(event);
      handler = h as (payload: unknown) => void;
      return Promise.resolve(() => {
        handler = null;
      });
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: tauri },
        { provide: TAURI_INVOKE, useValue: invoke },
        { provide: TAURI_LISTEN, useValue: listen },
      ],
    });
    const ui = TestBed.inject(UiState);
    TestBed.inject(TorrentLaunchService);
    return ui;
  }

  beforeEach(() => {
    pending = null;
  });

  it('opens the torrent the app was launched with, once, when the webview starts', async () => {
    pending = { kind: 'magnet', uri: 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567' };
    const ui = configure(true);
    await settle();
    expect(ui.torrentSource()).toEqual(pending as never);
    expect(listened).toEqual([OPEN_TORRENT_EVENT]);
  });

  it('opens a .torrent file handed over while the app runs', async () => {
    const ui = configure(true);
    await settle();
    expect(ui.torrentSource()).toBeNull();
    handler?.({ kind: 'file', path: 'C:/Users/me/Big Buck Bunny.torrent' });
    expect(ui.torrentSource()).toEqual({ kind: 'file', path: 'C:/Users/me/Big Buck Bunny.torrent' });
  });

  it('ignores anything that is not a magnet link or a file', async () => {
    const ui = configure(true);
    await settle();
    handler?.({ kind: 'script', code: 'alert(1)' });
    handler?.(null);
    expect(ui.torrentSource()).toBeNull();
  });

  it('does nothing in a plain browser', async () => {
    pending = { kind: 'magnet', uri: 'magnet:?x' };
    const ui = configure(false);
    await settle();
    expect(ui.torrentSource()).toBeNull();
    expect(listened).toEqual([]);
  });
});
