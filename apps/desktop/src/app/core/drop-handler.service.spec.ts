import { TestBed } from '@angular/core/testing';

import { UiState } from '../features/shell/ui-state.service';
import { DropHandlerService } from './drop-handler.service';

const OWNER = 'magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D';

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
