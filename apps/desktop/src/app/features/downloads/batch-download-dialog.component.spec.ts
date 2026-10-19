import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { BatchExpansion, expandPattern, importUrlList, UrlListResult } from '../../data/batch-pattern';
import { DownloadEngine } from '../../data/download-engine';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { UiState } from '../shell/ui-state.service';
import { BatchDownloadDialogComponent } from './batch-download-dialog.component';

/** Lets the chained promises (the engine call) settle. */
async function flush(): Promise<void> {
  for (let i = 0; i < 30; i++) {
    await Promise.resolve();
  }
}

/**
 * The simulated engine loads its pattern port with a dynamic `import()`, which a faked clock
 * cannot wait for; this one has the port linked in.
 */
class SyncPortEngine extends MockEngineService {
  override expandBatch(pattern: string): Promise<BatchExpansion> {
    return Promise.resolve(expandPattern(pattern));
  }

  override importUrlList(text: string): Promise<UrlListResult> {
    return Promise.resolve(importUrlList(text));
  }
}

describe('BatchDownloadDialogComponent', () => {
  let fixture: ComponentFixture<BatchDownloadDialogComponent>;
  let engine: MockEngineService;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;

  async function setup(lang = 'en-US'): Promise<void> {
    await TestBed.configureTestingModule({
      imports: [BatchDownloadDialogComponent],
      providers: [
        { provide: DownloadEngine, useClass: SyncPortEngine },
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: [lang] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    }).compileComponents();
    engine = TestBed.inject(DownloadEngine) as MockEngineService;
    fixture = TestBed.createComponent(BatchDownloadDialogComponent);
    TestBed.inject(UiState).openBatch();
    fixture.detectChanges();
  }

  async function type(selector: string, text: string): Promise<void> {
    const input = el().querySelector<HTMLInputElement | HTMLTextAreaElement>(selector);
    expect(input).withContext(selector).not.toBeNull();
    (input as HTMLInputElement).value = text;
    (input as HTMLInputElement).dispatchEvent(new Event('input'));
    fixture.detectChanges();
    jasmine.clock().tick(200);
    await flush();
    fixture.detectChanges();
  }

  const primary = (): HTMLButtonElement => Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.className.includes('v-primary')) as HTMLButtonElement;
  const count = (): string => el().querySelector('.count')?.textContent?.trim() ?? '';
  const rows = (): string[] => Array.from(el().querySelectorAll('.row .url')).map((e) => e.textContent?.trim() ?? '');

  beforeEach(() => jasmine.clock().install());
  afterEach(() => jasmine.clock().uninstall());

  it('previews a pattern: count, first rows (windowed), and a button that says how many will be added', async () => {
    await setup();
    expect(el().querySelector('.empty')).not.toBeNull();
    await type('input[name="pattern"]', 'https://h/file[001-120].zip');
    expect(count()).toBe('120 of 120 selected');
    expect(rows()[0]).toBe('https://h/file001.zip');
    // virtual list: only the window is in the DOM
    expect(rows().length).toBeGreaterThan(5);
    expect(rows().length).toBeLessThan(40);
    expect(primary().textContent).toContain('Download 120');
    expect(primary().disabled).toBeFalse();
  });

  it('scrolling moves the window and keeps the spacer math right', async () => {
    await setup();
    await type('input[name="pattern"]', 'https://h/file[001-120].zip');
    const viewport = el().querySelector<HTMLElement>('.viewport') as HTMLElement;
    viewport.scrollTop = 32 * 100;
    viewport.dispatchEvent(new Event('scroll'));
    fixture.detectChanges();
    expect(rows()).toContain('https://h/file101.zip');
    expect(rows()).not.toContain('https://h/file001.zip');
    const spacers = Array.from(el().querySelectorAll<HTMLElement>('.spacer')).map((s) => s.getBoundingClientRect().height);
    const drawn = rows().length;
    expect(spacers[0] + drawn * 32 + spacers[1]).toBe(120 * 32);
  });

  it('shows a readable error for a bad pattern and disables the button', async () => {
    await setup();
    await type('input[name="pattern"]', 'https://h/file[1-');
    expect(el().querySelector('.help.is-error')?.textContent).toContain('no closing');
    expect(primary().disabled).toBeTrue();
    await type('input[name="pattern"]', 'https://h/[1-99999]');
    expect(el().querySelector('.help.is-error')?.textContent).toContain('10000');
    await type('input[name="pattern"]', 'ftp://h/[1-2]');
    expect(el().querySelector('.help.is-error')?.textContent).toContain('http://');
  });

  it('words the errors in Arabic when the interface is Arabic', async () => {
    await setup('ar');
    await type('input[name="pattern"]', 'https://h/file[1-');
    expect(el().querySelector('.help.is-error')?.textContent).toContain('قوس');
    expect(primary().textContent).toContain('تنزيل');
  });

  it('unchecked links are not added; options are passed along', async () => {
    await setup();
    const add = spyOn(engine, 'addBatch').and.callThrough();
    await type('input[name="pattern"]', 'https://h/img_[a-e].jpg');
    el().querySelectorAll<HTMLInputElement>('.row .check')[1].click();
    fixture.detectChanges();
    expect(count()).toBe('4 of 5 selected');
    expect(primary().textContent).toContain('Download 4');
    el().querySelectorAll<HTMLButtonElement>('.presets .preset')[1].click();
    fixture.detectChanges();
    expect(primary().textContent).toContain('Add 4 to the list');

    primary().click();
    await flush();
    const [urls, options] = add.calls.mostRecent().args;
    expect(urls).toEqual(['https://h/img_a.jpg', 'https://h/img_c.jpg', 'https://h/img_d.jpg', 'https://h/img_e.jpg']);
    expect(options.startPaused).toBeTrue();
    expect((options.dir ?? '').length).toBeGreaterThan(0);
    expect(TestBed.inject(UiState).batchOpen()).toBeFalse();
    expect(engine.downloads().filter((d) => d.url.includes('img_')).length).toBe(4);
  });

  it('select none / select all', async () => {
    await setup();
    await type('input[name="pattern"]', 'https://h/[1-3]');
    const buttons = Array.from(el().querySelectorAll<HTMLButtonElement>('.head-actions button'));
    buttons[1].click();
    fixture.detectChanges();
    expect(count()).toBe('0 of 3 selected');
    expect(primary().disabled).toBeTrue();
    buttons[0].click();
    fixture.detectChanges();
    expect(count()).toBe('3 of 3 selected');
  });

  it('list tab: imports pasted text, drops comments, repeats and non-web lines, and says so', async () => {
    await setup();
    el().querySelectorAll<HTMLButtonElement>('.tab')[1].click();
    fixture.detectChanges();
    await type('textarea[name="list"]', '# links\nhttps://h/a.zip\nhttps://h/a.zip\nftp://h/b\nhttps://h/c.zip\n');
    expect(count()).toBe('2 of 2 selected');
    expect(rows()).toEqual(['https://h/a.zip', 'https://h/c.zip']);
    const summary = el().querySelector('.list-summary')?.textContent ?? '';
    expect(summary).toContain('2 links found');
    expect(summary).toContain('1 repeated links removed');
    expect(summary).toContain('1 lines are not web addresses');
  });

  it('copies the checked links as text, one per line', async () => {
    await setup();
    const writeText = jasmine.createSpy('writeText').and.resolveTo();
    spyOnProperty(navigator, 'clipboard', 'get').and.returnValue({ writeText } as unknown as Clipboard);
    await type('input[name="pattern"]', 'https://h/[1-3]');
    el().querySelectorAll<HTMLInputElement>('.row .check')[0].click();
    fixture.detectChanges();
    const copy = Array.from(el().querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes('Copy list')) as HTMLButtonElement;
    copy.click();
    await flush();
    expect(writeText).toHaveBeenCalledOnceWith('https://h/2\nhttps://h/3\n');
  });
});
