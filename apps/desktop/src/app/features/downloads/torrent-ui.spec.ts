import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadRow } from '../../data/downloads.store';
import { seedDownloads } from '../../data/mock-data';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { I18nService, registerTable } from '../../i18n/i18n.service';
import { Download } from '../../model/download';
import { magnetLabel, magnetLinesOf, TorrentPreview, TorrentSource, TorrentView } from '../../model/torrent';
import { UiState } from '../shell/ui-state.service';
import { AddDownloadDialogComponent } from './add-download-dialog.component';
import { DownloadRowComponent } from './download-row.component';
import { TorrentDialogComponent } from './torrent-dialog.component';
import { TorrentFileListComponent } from './torrent-file-list.component';

const HASH = '0123456789abcdef0123456789abcdef01234567';
const MAGNET = `magnet:?xt=urn:btih:${HASH}&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Ftracker.example%3A80`;

async function flush(): Promise<void> {
  for (let i = 0; i < 25; i++) {
    await Promise.resolve();
  }
}

function providers(): unknown[] {
  return [
    { provide: DownloadEngine, useClass: MockEngineService },
    { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
    { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
    { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
  ];
}

describe('Add dialog with a magnet link', () => {
  let fixture: ComponentFixture<AddDownloadDialogComponent>;
  let engine: MockEngineService;
  let ui: UiState;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;

  function type(url: string): void {
    const input = el().querySelector<HTMLInputElement>('input[name="url"]') as HTMLInputElement;
    input.value = url;
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({ imports: [AddDownloadDialogComponent], providers: providers() as never[] }).compileComponents();
    engine = TestBed.inject(DownloadEngine) as MockEngineService;
    ui = TestBed.inject(UiState);
    fixture = TestBed.createComponent(AddDownloadDialogComponent);
    fixture.componentRef.setInput('open', true);
    fixture.detectChanges();
  });

  it('recognises a pasted magnet link, does not probe it, and hands over to the file choice', async () => {
    const probe = spyOn(engine, 'probe').and.callThrough();
    const add = spyOn(engine, 'add').and.callThrough();
    const closed = jasmine.createSpy('closed');
    fixture.componentInstance.closed.subscribe(closed);
    type(MAGNET);
    expect(el().textContent).toContain('This is a torrent link');
    expect(probe).not.toHaveBeenCalled();
    const submit = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('primary')) as HTMLButtonElement;
    expect(submit.disabled).toBeFalse();
    expect(submit.textContent).toContain('Choose files');
    submit.click();
    await flush();
    expect(ui.torrentSource()).toEqual({ kind: 'magnet', uri: MAGNET });
    expect(closed).toHaveBeenCalled();
    expect(add).not.toHaveBeenCalled();
  });

  it('hides the fields that do not apply to a torrent (name, category, connections, checksum)', () => {
    type(MAGNET);
    expect(el().querySelector('input[name="filename"]')).toBeNull();
    expect(el().querySelector('select[name="category"]')).toBeNull();
    expect(el().querySelector('input[name="checksum"]')).toBeNull();
    type('https://example.com/a.zip');
    expect(el().querySelector('input[name="filename"]')).not.toBeNull();
  });

  it('still refuses text that is neither a web address nor a magnet link', () => {
    type('not a link');
    const submit = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('primary')) as HTMLButtonElement;
    expect(submit.disabled).toBeTrue();
    expect(el().textContent).not.toContain('This is a torrent link');
  });

  it('has a Choose .torrent file button that opens the picked file in the file choice', async () => {
    spyOn(engine, 'pickTorrentFile').and.resolveTo('C:\\Users\\me\\bbb.torrent');
    const button = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes('Choose .torrent file')) as HTMLButtonElement;
    expect(button).toBeDefined();
    button.click();
    await flush();
    expect(ui.torrentSource()).toEqual({ kind: 'file', path: 'C:\\Users\\me\\bbb.torrent' });
  });
});

describe('TorrentDialogComponent', () => {
  let fixture: ComponentFixture<TorrentDialogComponent>;
  let engine: MockEngineService;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;

  async function open(source = { kind: 'magnet', uri: MAGNET } as const): Promise<void> {
    fixture.componentRef.setInput('open', true);
    fixture.componentRef.setInput('source', source);
    fixture.detectChanges();
    await flush();
    fixture.detectChanges();
  }

  const primary = (): HTMLButtonElement => Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('primary')) as HTMLButtonElement;

  beforeEach(async () => {
    await TestBed.configureTestingModule({ imports: [TorrentDialogComponent], providers: providers() as never[] }).compileComponents();
    engine = TestBed.inject(DownloadEngine) as MockEngineService;
    fixture = TestBed.createComponent(TorrentDialogComponent);
  });

  it('shows a waiting state while a magnet link finds peers, then the files with sizes', async () => {
    let release: (() => void) | undefined;
    const real = engine.torrentPreview.bind(engine);
    spyOn(engine, 'torrentPreview').and.callFake(
      (s: TorrentSource) =>
        new Promise<TorrentPreview>((resolve) => {
          release = () => void real(s).then(resolve);
        }),
    );
    fixture.componentRef.setInput('open', true);
    fixture.componentRef.setInput('source', { kind: 'magnet', uri: MAGNET });
    fixture.detectChanges();
    await flush();
    fixture.detectChanges();
    expect(el().textContent).toContain('Finding peers');
    expect(el().querySelector('app-torrent-file-list')).toBeNull();
    release?.();
    await flush();
    fixture.detectChanges();
    expect(el().textContent).toContain('Big Buck Bunny');
    expect(el().querySelectorAll('.row').length).toBe(3);
    expect(el().textContent).toContain('3 of 3 files selected');
  });

  it('adds only the checked files, in order when asked, and reports the choice to the engine', async () => {
    const add = spyOn(engine, 'addTorrent').and.callThrough();
    await open();
    const boxes = el().querySelectorAll<HTMLInputElement>('app-torrent-file-list input[type="checkbox"]');
    expect(boxes.length).toBe(3);
    boxes[0].click(); // the 276 MB movie off
    fixture.detectChanges();
    expect(el().textContent).toContain('2 of 3 files selected');
    const sequential = el().querySelector<HTMLInputElement>('.check-row input') as HTMLInputElement;
    sequential.click();
    fixture.detectChanges();
    primary().click();
    await flush();
    const request = add.calls.mostRecent().args[0];
    expect(request.onlyFiles).toEqual([1, 2]);
    expect(request.sequential).toBeTrue();
    expect(request.startPaused).toBeFalse();
    expect(engine.downloads().some((d) => d.kind === 'torrent')).toBeTrue();
  });

  it('sends null for "every file" and cannot add with nothing chosen', async () => {
    const add = spyOn(engine, 'addTorrent').and.callThrough();
    await open();
    const links = Array.from(el().querySelectorAll<HTMLButtonElement>('.list-actions .link'));
    links[1].click(); // select none
    fixture.detectChanges();
    expect(primary().disabled).toBeTrue();
    expect(el().textContent).toContain('Choose at least one file');
    links[0].click(); // select all
    fixture.detectChanges();
    expect(primary().disabled).toBeFalse();
    primary().click();
    await flush();
    expect(add.calls.mostRecent().args[0].onlyFiles).toBeNull();
  });

  it('says so when the torrent is already in the list', async () => {
    const preview = await engine.torrentPreview({ kind: 'magnet', uri: MAGNET });
    await engine.addTorrent({ token: preview.token, onlyFiles: null, sequential: false, startPaused: true });
    await open();
    expect(el().textContent).toContain('already in your list');
    expect(primary().disabled).toBeTrue();
  });

  it('turns a failed lookup into a readable message with Try again', async () => {
    spyOn(engine, 'torrentPreview').and.rejectWith('timed out waiting for torrent metadata (no peers found?)');
    await open();
    expect(el().querySelector('[role="alert"]')?.textContent).toContain('No peer answered in time');
    expect(Array.from(el().querySelectorAll('button')).some((b) => b.textContent?.includes('Try again'))).toBeTrue();
  });

  it('a malformed magnet link is reported as such', async () => {
    await open({ kind: 'magnet', uri: 'https://not-a-magnet' } as never);
    expect(el().querySelector('[role="alert"]')).not.toBeNull();
  });
});

describe('TorrentFileListComponent', () => {
  let fixture: ComponentFixture<TorrentFileListComponent>;

  const many = (n: number): { index: number; path: string; len: number }[] =>
    Array.from({ length: n }, (_, i) => ({ index: i, path: `set/part ${String(i).padStart(5, '0')}.bin`, len: 1000 + i }));

  beforeEach(async () => {
    await TestBed.configureTestingModule({ imports: [TorrentFileListComponent], providers: providers() as never[] }).compileComponents();
    fixture = TestBed.createComponent(TorrentFileListComponent);
  });

  it('draws only the rows in view for a torrent with thousands of files', () => {
    fixture.componentRef.setInput('files', many(20_000));
    fixture.detectChanges();
    const rows = (fixture.nativeElement as HTMLElement).querySelectorAll('.row').length;
    expect(rows).toBeGreaterThan(0);
    expect(rows).toBeLessThan(40);
  });

  it('scrolls to the later files', () => {
    fixture.componentRef.setInput('files', many(2000));
    fixture.detectChanges();
    const viewport = (fixture.nativeElement as HTMLElement).querySelector<HTMLElement>('.viewport') as HTMLElement;
    viewport.scrollTop = 36 * 1500;
    viewport.dispatchEvent(new Event('scroll'));
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).textContent).toContain('part 01500.bin');
    expect((fixture.nativeElement as HTMLElement).textContent).not.toContain('part 00001.bin');
  });

  it('shows progress per file and marks files that are not downloaded', () => {
    fixture.componentRef.setInput('files', [
      { index: 0, path: 'a/one.bin', len: 1000 },
      { index: 1, path: 'a/two.bin', len: 1000 },
    ]);
    fixture.componentRef.setInput('selected', new Set([0]));
    fixture.componentRef.setInput('progress', [500, 0]);
    fixture.detectChanges();
    const text = (fixture.nativeElement as HTMLElement).textContent ?? '';
    expect(text).toContain('50%');
    expect(text).toContain('not selected');
  });

  it('shows only the file name in the read-only list and the full path as its tooltip', () => {
    fixture.componentRef.setInput('files', [{ index: 0, path: 'Big Buck Bunny/Big Buck Bunny.en.srt', len: 140 }]);
    fixture.detectChanges();
    const name = (fixture.nativeElement as HTMLElement).querySelector('.name') as HTMLElement;
    expect(name.textContent?.trim()).toBe('Big Buck Bunny.en.srt');
    expect(name.getAttribute('title')).toBe('Big Buck Bunny/Big Buck Bunny.en.srt');
  });

  it('keeps Arabic file names bidi-safe', () => {
    fixture.componentRef.setInput('files', [{ index: 0, path: 'مجلد/ملف.mp4', len: 10 }]);
    fixture.detectChanges();
    const name = (fixture.nativeElement as HTMLElement).querySelector('.name') as HTMLElement;
    expect(name.getAttribute('dir')).toBe('auto');
    expect(name.textContent).toContain('ملف.mp4');
  });
});

describe('DownloadRowComponent for a torrent', () => {
  let fixture: ComponentFixture<DownloadRowComponent>;

  function torrent(over: Partial<TorrentView> = {}, state: Download['state'] = 'downloading'): DownloadRow {
    const base = seedDownloads(Date.now())[0];
    const view: TorrentView = {
      infoHash: HASH,
      files: [{ index: 0, path: 'x/a', len: 10 }],
      selected: null,
      sequential: false,
      downloaded: 0,
      seeding: false,
      peers: 5,
      peersSeen: 17,
      upBps: 0,
      uploaded: 0,
      ratio: 0.5,
      ...over,
    };
    return { d: { ...base, kind: 'torrent', state, filename: 'Big Buck Bunny', url: `magnet:?xt=urn:btih:${HASH}`, torrent: view }, downloaded: 0, progress: 0, eta: null };
  }

  function render(row: DownloadRow, lang = 'en-US'): HTMLElement {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [DownloadRowComponent],
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: [lang] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    });
    fixture = TestBed.createComponent(DownloadRowComponent);
    fixture.componentRef.setInput('row', row);
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  it('shows the torrent badge, peers and ratio instead of the host name', () => {
    const el = render(torrent());
    expect(el.querySelector('.media-badge')).not.toBeNull();
    expect(el.querySelector('.torrent-line')?.textContent).toContain('5 of 17 peers');
    expect(el.querySelector('.torrent-line')?.textContent).toContain('Ratio 0.50');
    expect(el.querySelector('.torrent-line')?.getAttribute('title')).toContain('5 of 17 peers');
    expect(el.querySelector('.host:not(.torrent-line)')).toBeNull();
  });

  it('leaves the ratio out while nothing was uploaded', () => {
    const el = render(torrent({ ratio: 0 }));
    expect(el.querySelector('.torrent-line')?.textContent?.trim()).toBe('5 of 17 peers');
  });

  it('says Seeding while it shares a finished download', () => {
    const el = render(torrent({ seeding: true, ratio: 1.25 }, 'completed'));
    expect(el.querySelector('.torrent-line')?.textContent).toContain('Seeding');
    expect(el.querySelector('.torrent-line')?.textContent).toContain('1.25');
  });

  it('is translated to Arabic', async () => {
    const ar = (await import('../../../i18n/ar.json')).default as Record<string, string>;
    registerTable('ar', ar);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [DownloadRowComponent],
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: ['ar'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    });
    expect(TestBed.inject(I18nService).lang()).toBe('ar');
    fixture = TestBed.createComponent(DownloadRowComponent);
    fixture.componentRef.setInput('row', torrent());
    fixture.detectChanges();
    const line = (fixture.nativeElement as HTMLElement).querySelector('.torrent-line')?.textContent ?? '';
    expect(line).toContain('مصدر');
    expect(line).toContain('النسبة');
  });
});

const OWNER_MAGNET = 'magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D';
const SECOND_MAGNET = `magnet:?xt=urn:btih:${HASH}&dn=Big+Buck+Bunny`;

describe('magnet link text helpers', () => {
  it('finds one magnet link per line and ignores everything else', () => {
    const text = `notes\r\n${OWNER_MAGNET}\r\n\r\n  ${SECOND_MAGNET}  \n${OWNER_MAGNET}\nhttps://example.com/a.zip\nmagnet:?xt=urn:sha1:abc`;
    expect(magnetLinesOf(text)).toEqual([OWNER_MAGNET, SECOND_MAGNET]);
    expect(magnetLinesOf('')).toEqual([]);
    expect(magnetLinesOf('one two three')).toEqual([]);
  });

  it('labels a link by its name or hash, never by a tracker', () => {
    expect(magnetLabel(SECOND_MAGNET)).toBe('Big Buck Bunny');
    expect(magnetLabel(OWNER_MAGNET)).toBe('428c72ff…');
    expect(magnetLabel(OWNER_MAGNET)).not.toContain('rutracker');
  });
});

describe('several magnet links at once', () => {
  let fixture: ComponentFixture<AddDownloadDialogComponent>;
  let engine: MockEngineService;
  let ui: UiState;
  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;

  function paste(text: string): ClipboardEvent {
    const input = el().querySelector<HTMLInputElement>('input[name="url"]') as HTMLInputElement;
    const data = new DataTransfer();
    data.setData('text', text);
    const event = new ClipboardEvent('paste', { clipboardData: data, cancelable: true, bubbles: true });
    input.dispatchEvent(event);
    fixture.detectChanges();
    return event;
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({ imports: [AddDownloadDialogComponent], providers: providers() as never[] }).compileComponents();
    engine = TestBed.inject(DownloadEngine) as MockEngineService;
    ui = TestBed.inject(UiState);
    fixture = TestBed.createComponent(AddDownloadDialogComponent);
    fixture.componentRef.setInput('open', true);
    fixture.detectChanges();
  });

  it('a paste of the owner link alone is a single magnet that leads to the file choice', async () => {
    const e = paste(OWNER_MAGNET);
    expect(e.defaultPrevented).toBeFalse();
    const input = el().querySelector<HTMLInputElement>('input[name="url"]') as HTMLInputElement;
    input.value = OWNER_MAGNET;
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    expect(el().textContent).toContain('This is a torrent link');
    const go = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes('Choose files')) as HTMLButtonElement;
    go.click();
    await flush();
    expect(ui.torrentSource()).toEqual({ kind: 'magnet', uri: OWNER_MAGNET });
  });

  it('a paste with several magnet links lists them (no trackers) and opens the file choice for each in turn', async () => {
    const e = paste(`${OWNER_MAGNET}\n${SECOND_MAGNET}\n`);
    expect(e.defaultPrevented).toBeTrue();
    expect(el().textContent).toContain('2 magnet links found');
    expect(el().textContent).toContain('428c72ff');
    expect(el().textContent).toContain('Big Buck Bunny');
    expect(el().textContent).not.toContain('rutracker');
    const go = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes('Choose files')) as HTMLButtonElement;
    expect(go.disabled).toBeFalse();
    go.click();
    await flush();
    expect(ui.torrentSource()).toEqual({ kind: 'magnet', uri: OWNER_MAGNET });
    expect(ui.torrentQueue()).toEqual([{ kind: 'magnet', uri: SECOND_MAGNET }]);
    // closing the first one opens the second; closing that one ends the sequence
    ui.closeTorrent();
    expect(ui.torrentSource()).toEqual({ kind: 'magnet', uri: SECOND_MAGNET });
    ui.closeTorrent();
    expect(ui.torrentSource()).toBeNull();
  });

  it('"Add all" adds every link with all files and says what happened', async () => {
    const previews: string[] = [];
    spyOn(engine, 'torrentPreview').and.callFake(async (source: TorrentSource) => {
      const uri = source.kind === 'magnet' ? source.uri : '';
      previews.push(uri);
      if (uri === SECOND_MAGNET) {
        throw new Error('timed out');
      }
      return { token: 't1', infoHash: 'x', name: 'n', files: [], totalBytes: 0, isPrivate: false, existing: null } as TorrentPreview;
    });
    const add = spyOn(engine, 'addTorrent').and.resolveTo(7);
    paste(`${OWNER_MAGNET}\n${SECOND_MAGNET}`);
    const all = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes('Add all')) as HTMLButtonElement;
    all.click();
    await flush();
    expect(previews.sort()).toEqual([OWNER_MAGNET, SECOND_MAGNET].sort());
    expect(add).toHaveBeenCalledTimes(1);
    expect(add.calls.mostRecent().args[0]).toEqual({ token: 't1', onlyFiles: null, sequential: false, startPaused: false });
  });

  it('the skip button drops the links that were still waiting', () => {
    ui.openTorrents([OWNER_MAGNET, SECOND_MAGNET]);
    expect(ui.torrentQueue().length).toBe(1);
    ui.skipQueuedTorrents();
    ui.closeTorrent();
    expect(ui.torrentSource()).toBeNull();
  });
});
