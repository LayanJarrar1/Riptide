import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { CRAWL_BACKEND, CrawlBackend } from '../../data/crawl-backend';
import { CrawlAddOptions, CrawlFile, CrawlSpecInput, CrawlUiEvent, CrawlUpdate } from '../../model/crawl';
import { BatchResult } from '../../model/download';
import { UiState } from '../shell/ui-state.service';
import { GrabberSession } from './grabber.session';
import { SiteGrabberDialogComponent } from './site-grabber-dialog.component';

/** Lets chained promises settle. */
async function flush(): Promise<void> {
  for (let i = 0; i < 30; i++) {
    await Promise.resolve();
  }
}

/** An engine that does what the test tells it: events go in through `emit`, calls are recorded. */
class FakeCrawlBackend implements CrawlBackend {
  started: CrawlSpecInput[] = [];
  cancelled: number[] = [];
  discarded: number[] = [];
  added: { id: number; urls: readonly string[]; options: CrawlAddOptions }[] = [];
  refuse: string | null = null;
  private sink: ((u: CrawlUpdate) => void) | null = null;

  start(spec: CrawlSpecInput, onUpdate: (update: CrawlUpdate) => void): Promise<number> {
    if (this.refuse !== null) {
      return Promise.reject(new Error(this.refuse));
    }
    this.started.push(spec);
    this.sink = onUpdate;
    return Promise.resolve(7);
  }

  cancel(id: number): Promise<void> {
    this.cancelled.push(id);
    this.emit([], { stop: 'cancelled', pageCapHit: false, fileCapHit: false, error: null });
    return Promise.resolve();
  }

  discard(id: number): Promise<void> {
    this.discarded.push(id);
    return Promise.resolve();
  }

  add(id: number, urls: readonly string[], options: CrawlAddOptions): Promise<BatchResult> {
    this.added.push({ id, urls, options });
    return Promise.resolve({ added: urls.map((_, i) => i + 1), skipped: [] });
  }

  emit(events: readonly CrawlUiEvent[], end?: CrawlUpdate['end']): void {
    this.sink?.({ events, end });
  }
}

function file(name: string, category: CrawlFile['category'], host = 'example.com', size: number | null = null): CrawlFile {
  return { url: `https://${host}/f/${name}`, name, host, category, size };
}

const fileEvent = (f: CrawlFile): CrawlUiEvent => ({ kind: 'file', file: f });
const END = { stop: 'finished', pageCapHit: false, fileCapHit: false, error: null } as const;

describe('SiteGrabberDialogComponent', () => {
  let fixture: ComponentFixture<SiteGrabberDialogComponent>;
  let backend: FakeCrawlBackend;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const session = (): GrabberSession => fixture.debugElement.injector.get(GrabberSession);
  const primary = (): HTMLButtonElement => Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('v-primary')) as HTMLButtonElement;
  const rowNames = (): string[] => Array.from(el().querySelectorAll('.row .name')).map((e) => e.textContent?.trim() ?? '');
  const groupNames = (): string[] => Array.from(el().querySelectorAll('.row.is-group .group-name')).map((e) => e.textContent?.trim() ?? '');

  async function setup(lang = 'en-US'): Promise<void> {
    backend = new FakeCrawlBackend();
    await TestBed.configureTestingModule({
      imports: [SiteGrabberDialogComponent],
      providers: [
        { provide: CRAWL_BACKEND, useValue: backend },
        { provide: NAVIGATOR_LANGUAGES, useValue: [lang] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(SiteGrabberDialogComponent);
    TestBed.inject(UiState).openGrabber();
    fixture.detectChanges();
  }

  async function type(selector: string, text: string): Promise<void> {
    const input = el().querySelector<HTMLInputElement | HTMLTextAreaElement>(selector);
    expect(input).withContext(selector).not.toBeNull();
    (input as HTMLInputElement).value = text;
    (input as HTMLInputElement).dispatchEvent(new Event('input'));
    fixture.detectChanges();
    await flush();
  }

  async function startCrawl(url = 'https://example.com/files/'): Promise<void> {
    await type('input[name="url"]', url);
    primary().click();
    fixture.detectChanges();
    await flush();
    fixture.detectChanges();
  }

  function emit(events: CrawlUiEvent[], end?: CrawlUpdate['end']): void {
    backend.emit(events, end);
    fixture.detectChanges();
  }

  it('starts on the form; Start is disabled until the address is an http(s) link', async () => {
    await setup();
    expect(primary().disabled).toBeTrue();
    await type('input[name="url"]', 'example.com');
    expect(primary().disabled).toBeTrue();
    await type('input[name="url"]', 'https://example.com/files/');
    expect(primary().disabled).toBeFalse();
    expect(el().querySelector('.note')?.textContent).toContain('robots.txt');
  });

  it('sends the form as the crawl spec', async () => {
    await setup();
    await type('input[name="url"]', ' https://example.com/files/ ');
    session().setDepth(3);
    session().scope.set('same-domain');
    session().toggleCategory('documents');
    session().toggleCategory('images');
    session().includeText.set('*.pdf\n*.png, *.jpg');
    session().excludeText.set('/thumbs/**');
    session().minMb.set('1');
    session().maxMb.set('0.5');
    fixture.detectChanges();
    expect(primary().disabled).withContext('smallest above largest').toBeTrue();
    session().maxMb.set('2');
    fixture.detectChanges();
    primary().click();
    await flush();
    expect(backend.started).toEqual([
      {
        url: 'https://example.com/files/',
        depth: 3,
        scope: 'same-domain',
        offsiteFiles: true,
        categories: ['documents', 'images'],
        include: ['*.pdf', '*.png', '*.jpg'],
        exclude: ['/thumbs/**'],
        minSize: 1024 * 1024,
        maxSize: 2 * 1024 * 1024,
      },
    ]);
  });

  it('stays on the form with the reason when the engine refuses the spec', async () => {
    await setup();
    backend.refuse = 'the depth is at most 5';
    await startCrawl();
    expect(session().step()).toBe('spec');
    expect(el().querySelector('.help.is-error')?.textContent).toContain('the depth is at most 5');
  });

  it('updates the counters and the grouped list as events arrive, in batches', async () => {
    await setup();
    await startCrawl();
    expect(session().step()).toBe('crawl');
    expect(el().querySelector('.empty')?.textContent).toContain('No files yet');

    emit([{ kind: 'page', url: 'https://example.com/files/', depth: 0 }, fileEvent(file('a.pdf', 'documents', 'example.com', 2048)), fileEvent(file('b.zip', 'compressed', 'cdn.example.com'))]);
    emit([{ kind: 'page', url: 'https://example.com/files/2', depth: 1 }, { kind: 'skipped', url: 'mailto:x@y.z' }, fileEvent(file('c.pdf', 'documents'))]);

    const counts = Array.from(el().querySelectorAll('.counter .count')).map((e) => e.textContent?.trim());
    expect(counts).toEqual(['2', '3', '1']);
    // groups in the sidebar's order: Archives before Documents
    expect(groupNames()).toEqual(['Archives', 'Documents']);
    expect(rowNames()).toEqual(['b.zip', 'a.pdf', 'c.pdf']);
    expect(el().querySelector('.row .size')?.textContent?.trim()).toBe('');
    expect(el().querySelector('.selected')?.textContent).toContain('3 of 3 selected');
    // still running: Next waits, Stop is offered
    expect(primary().disabled).toBeTrue();
    expect(el().querySelector('.status button')).not.toBeNull();

    emit([{ kind: 'done', counters: { pages: 2, files: 3, skipped: 1 } }], END);
    expect(el().querySelector('.status-text')?.textContent).toContain('Finished');
    expect(el().querySelector('.status button')).toBeNull();
    expect(primary().disabled).toBeFalse();
  });

  it('Stop cancels the crawl and keeps what was found', async () => {
    await setup();
    await startCrawl();
    emit([fileEvent(file('a.pdf', 'documents'))]);
    (el().querySelector('.status button') as HTMLButtonElement).click();
    await flush();
    fixture.detectChanges();
    expect(backend.cancelled).toEqual([7]);
    expect(session().running()).toBeFalse();
    expect(el().querySelector('.status-text')?.textContent).toContain('Stopped');
    expect(rowNames()).toEqual(['a.pdf']);
    expect(primary().disabled).toBeFalse();
  });

  it('checks and unchecks files, groups, all and none; the filter limits Select all and none', async () => {
    await setup();
    await startCrawl();
    emit([fileEvent(file('a.pdf', 'documents')), fileEvent(file('b.pdf', 'documents')), fileEvent(file('song.mp3', 'music')), fileEvent(file('c.zip', 'compressed'))], END);
    const boxes = (): HTMLInputElement[] => Array.from(el().querySelectorAll<HTMLInputElement>('.row .check'));
    const selected = (): string => el().querySelector('.selected')?.textContent?.trim() ?? '';

    expect(selected()).toBe('4 of 4 selected');
    // untick one file
    const aBox = Array.from(el().querySelectorAll<HTMLElement>('.row')).find((r) => r.textContent?.includes('a.pdf'))?.querySelector('input') as HTMLInputElement;
    aBox.click();
    fixture.detectChanges();
    expect(selected()).toBe('3 of 4 selected');
    // the Documents group is no longer fully checked
    const docs = Array.from(el().querySelectorAll<HTMLElement>('.row.is-group')).find((r) => r.textContent?.includes('Documents'))?.querySelector('input') as HTMLInputElement;
    expect(docs.checked).toBeFalse();
    docs.click();
    fixture.detectChanges();
    expect(selected()).toBe('4 of 4 selected');

    const [selectAll, selectNone] = Array.from(el().querySelectorAll<HTMLButtonElement>('.head-actions button'));
    // a filter narrows both buttons to the visible files
    await type('input[name="filter"]', '.pdf');
    expect(rowNames()).toEqual(['a.pdf', 'b.pdf']);
    selectNone.click();
    fixture.detectChanges();
    expect(selected()).toBe('2 of 4 selected');
    selectAll.click();
    fixture.detectChanges();
    expect(selected()).toBe('4 of 4 selected');
    await type('input[name="filter"]', '');
    selectNone.click();
    fixture.detectChanges();
    expect(selected()).toBe('0 of 4 selected');
    expect(boxes().every((b) => !b.checked)).toBeTrue();
    expect(primary().disabled).withContext('nothing selected').toBeTrue();
    await type('input[name="filter"]', 'nothing like it');
    expect(el().querySelector('.empty')?.textContent).toContain('No file matches');
  });

  it('files that arrive after the user unchecked others start checked', async () => {
    await setup();
    await startCrawl();
    emit([fileEvent(file('a.pdf', 'documents'))]);
    session().toggle(file('a.pdf', 'documents').url, false);
    emit([fileEvent(file('b.pdf', 'documents'))], END);
    expect(session().selection()).toEqual([file('b.pdf', 'documents').url]);
  });

  it('renders only a window of a very long list', async () => {
    await setup();
    await startCrawl();
    emit(Array.from({ length: 3000 }, (_, i) => fileEvent(file(`f${String(i).padStart(4, '0')}.pdf`, 'documents'))), END);
    expect(rowNames().length).toBeGreaterThan(5);
    expect(rowNames().length).toBeLessThan(60);
    const viewport = el().querySelector<HTMLElement>('.viewport') as HTMLElement;
    viewport.scrollTop = 34 * 1500;
    viewport.dispatchEvent(new Event('scroll'));
    fixture.detectChanges();
    expect(rowNames().some((n) => n === 'f1499.pdf')).toBeTrue();
  });

  it('adds the checked files with the folder, queue and start mode, then closes', async () => {
    await setup();
    await startCrawl();
    const a = file('a.pdf', 'documents');
    const b = file('b.pdf', 'documents');
    emit([fileEvent(a), fileEvent(b)], END);
    session().toggle(b.url, false);
    primary().click(); // Next
    fixture.detectChanges();
    expect(session().step()).toBe('add');
    expect(primary().textContent).toContain('Add 1 downloads');
    expect(el().querySelector<HTMLInputElement>('input[name="dir"]')?.value).toBe('D:\\Downloads');
    // Main is the default queue
    expect(el().querySelector<HTMLSelectElement>('select[name="queue"]')?.selectedOptions[0].textContent?.trim()).toBe('Main queue');
    await type('input[name="dir"]', 'E:\\Grabbed');
    session().startPaused.set(true);
    fixture.detectChanges();
    primary().click();
    await flush();
    fixture.detectChanges();
    expect(backend.added).toEqual([{ id: 7, urls: [a.url], options: { dir: 'E:\\Grabbed', queueId: 1, startPaused: true } }]);
    expect(TestBed.inject(UiState).grabberOpen()).toBeFalse();
    // the engine forgot the crawl once its files were in: nothing more to discard on close
    expect(backend.discarded).toEqual([]);
  });

  it('closing the dialog stops and discards the crawl', async () => {
    await setup();
    await startCrawl();
    (el().querySelector('.foot .v-ghost:not(.first-action)') as HTMLButtonElement | null)?.click();
    fixture.detectChanges();
    expect(TestBed.inject(UiState).grabberOpen()).toBeFalse();
    expect(backend.discarded).toEqual([7]);
  });

  it('Back returns to the previous step with the results intact', async () => {
    await setup();
    await startCrawl();
    emit([fileEvent(file('a.pdf', 'documents'))], END);
    primary().click();
    fixture.detectChanges();
    (el().querySelector('.first-action') as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(session().step()).toBe('crawl');
    expect(rowNames()).toEqual(['a.pdf']);
  });

  it('speaks Arabic in a right-to-left page and keeps links left-to-right', async () => {
    await setup('ar');
    expect(el().querySelector('.label')?.textContent).toContain('صفحة البداية');
    expect(el().querySelector('input[name="url"]')?.getAttribute('dir')).toBe('ltr');
    expect(primary().textContent).toContain('ابدأ البحث');
  });
});
