import { TestBed } from '@angular/core/testing';

import { UiState } from '../features/shell/ui-state.service';
import { TAURI_AVAILABLE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { DropHandlerService, NATIVE_DROP_EVENT, torrentFilesIn } from './drop-handler.service';

const OWNER = 'magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D';

describe('torrent files dropped on the window', () => {
  it('keeps only the .torrent paths (any case), in order', () => {
    expect(torrentFilesIn(['C:\\a\\Ubuntu.TORRENT', 'C:\\a\\notes.txt', 'D:\\b.torrent'])).toEqual([
      { kind: 'file', path: 'C:\\a\\Ubuntu.TORRENT' },
      { kind: 'file', path: 'D:\\b.torrent' },
    ]);
    expect(torrentFilesIn(['C:\\a\\movie.mkv'])).toEqual([]);
  });

  it('the native drop event opens the file choice for each, one after the other', () => {
    const box: { handler: ((p: { paths: string[] }) => void) | null } = { handler: null };
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: true },
        {
          provide: TAURI_LISTEN,
          useValue: (event: string, h: (p: { paths: string[] }) => void): Promise<() => void> => {
            expect(event).toBe(NATIVE_DROP_EVENT);
            box.handler = h;
            return Promise.resolve(() => undefined);
          },
        },
      ],
    });
    TestBed.inject(DropHandlerService);
    const ui = TestBed.inject(UiState);
    box.handler?.({ paths: ['C:\\x\\one.torrent', 'C:\\x\\two.torrent', 'C:\\x\\readme.txt'] });
    expect(ui.torrentSource()).toEqual({ kind: 'file', path: 'C:\\x\\one.torrent' });
    expect(ui.torrentQueue()).toEqual([{ kind: 'file', path: 'C:\\x\\two.torrent' }]);
    // other files do nothing
    ui.skipQueuedTorrents();
    ui.closeTorrent();
    box.handler?.({ paths: ['C:\\x\\readme.txt'] });
    expect(ui.torrentSource()).toBeNull();
  });
});

describe('DropHandlerService', () => {
  function drop(text: string, type = 'text/plain'): DragEvent {
    const data = new DataTransfer();
    data.setData(type, text);
    const event = new DragEvent('drop', { dataTransfer: data, cancelable: true, bubbles: true });
    window.dispatchEvent(event);
    return event;
  }

  it('a dropped magnet link opens the file choice', () => {
    TestBed.inject(DropHandlerService);
    const ui = TestBed.inject(UiState);
    const e = drop(OWNER);
    expect(e.defaultPrevented).toBeTrue();
    expect(ui.torrentSource()).toEqual({ kind: 'magnet', uri: OWNER });
  });

  it('several dropped magnet links queue up, a dropped address fills the Add dialog, other text is ignored', () => {
    TestBed.inject(DropHandlerService);
    const ui = TestBed.inject(UiState);
    drop(`${OWNER}\nmagnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567`);
    expect(ui.torrentQueue().length).toBe(1);
    expect(ui.torrentSource()).toEqual({ kind: 'magnet', uri: OWNER });
    ui.torrentSource.set(null);
    ui.torrentQueue.set([]);
    drop('https://example.org/file.zip', 'text/uri-list');
    expect(ui.addOpen()).toBeTrue();
    expect(ui.addUrl()).toBe('https://example.org/file.zip');
    ui.closeAdd();
    expect(drop('just some words').defaultPrevented).toBeFalse();
    expect(ui.addOpen()).toBeFalse();
  });
});
