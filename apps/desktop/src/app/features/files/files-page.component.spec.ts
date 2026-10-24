import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FRAME_SCHEDULER, FrameScheduler, REDUCED_MOTION_QUERY } from '../../core/motion.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { FILE_BACKEND, MemoryFileBackend } from './file-backend';
import { FileOpsService } from './file-ops.service';
import { FILES_LAYOUT_KEY, FilesPageComponent } from './files-page.component';

const NO_FRAMES: FrameScheduler = { request: () => 0, cancel: () => undefined };
const HOME = 'C:\\Users\\You\\Downloads';

describe('FilesPageComponent', () => {
  let fixture: ComponentFixture<FilesPageComponent>;
  let disk: MemoryFileBackend;
  let prefs: MemoryPreferenceStore;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const panes = (): HTMLElement[] => Array.from(el().querySelectorAll<HTMLElement>('app-file-pane'));
  const rows = (pane = 0): HTMLElement[] => Array.from(panes()[pane].querySelectorAll<HTMLElement>('.row:not(.is-editing)'));
  const names = (pane = 0): string[] => rows(pane).map((r) => r.querySelector('.name')?.textContent?.trim() ?? '');
  const row = (name: string, pane = 0): HTMLElement => rows(pane).find((r) => r.querySelector('.name')?.textContent?.trim() === name) as HTMLElement;
  const grid = (pane = 0): HTMLElement => panes()[pane].querySelector<HTMLElement>('.grid') as HTMLElement;
  const key = (k: string, init: KeyboardEventInit = {}, pane = 0): void => {
    grid(pane).dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
  };

  /** Lets the in-memory disk's promises and the effects run; toasts' timers are not waited for. */
  async function settle(): Promise<void> {
    for (let i = 0; i < 6; i++) {
      fixture.detectChanges();
      await new Promise((r) => setTimeout(r, 0));
    }
    fixture.detectChanges();
  }

  async function setup(layout?: object): Promise<void> {
    disk = new MemoryFileBackend(false);
    disk.addFile(`${HOME}\\report.pdf`, 4000, 1_000);
    disk.addFile(`${HOME}\\notes.txt`, 20, 3_000);
    disk.addFile(`${HOME}\\big.iso`, 9_000_000, 2_000);
    disk.addFile(`${HOME}\\Projects\\thesis\\chapter-1.docx`, 640);
    disk.addFolder(`${HOME}\\Archive`);
    prefs = new MemoryPreferenceStore(layout ? { [FILES_LAYOUT_KEY]: JSON.stringify(layout) } : {});
    await TestBed.configureTestingModule({
      imports: [FilesPageComponent],
      providers: [
        { provide: FILE_BACKEND, useValue: disk },
        { provide: PREFERENCE_STORE, useValue: prefs },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: FRAME_SCHEDULER, useValue: NO_FRAMES },
        { provide: REDUCED_MOTION_QUERY, useValue: { matches: true, addEventListener: (): void => undefined } },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(FilesPageComponent);
    fixture.nativeElement.style.height = '640px';
    fixture.nativeElement.style.width = '1200px';
    document.body.appendChild(fixture.nativeElement);
    await settle();
  }

  afterEach(() => fixture?.nativeElement.remove());

  it('opens the Downloads place with folders first, sorted by name', async () => {
    await setup();
    expect(names()).toEqual(['Archive', 'Projects', 'big.iso', 'notes.txt', 'report.pdf']);
  });

  it('sorts by size and by date, folders always first', async () => {
    await setup();
    panes()[0].querySelector<HTMLButtonElement>('.th-size')?.click();
    await settle();
    // biggest first; folders have no size and keep their name order
    expect(names()).toEqual(['Archive', 'Projects', 'big.iso', 'report.pdf', 'notes.txt']);
    panes()[0].querySelector<HTMLButtonElement>('.th-modified')?.click();
    await settle();
    expect(names().slice(2)).toEqual(['notes.txt', 'big.iso', 'report.pdf']);
  });

  it('navigates into folders, back, forward and up, with the breadcrumb following', async () => {
    await setup();
    row('Projects').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await settle();
    expect(names()).toEqual(['thesis']);
    expect(panes()[0].querySelector('.crumb.is-current')?.textContent?.trim()).toBe('Projects');
    // Up is a step in the history, as in Explorer
    key('Backspace');
    await settle();
    expect(names()).toContain('Projects');
    key('ArrowLeft', { altKey: true });
    await settle();
    expect(names()).toEqual(['thesis']);
    key('ArrowRight', { altKey: true });
    await settle();
    expect(names()).toContain('report.pdf');
  });

  it('filters as you type and searches subfolders on Enter', async () => {
    await setup();
    const box = panes()[0].querySelector<HTMLInputElement>('.search input') as HTMLInputElement;
    box.value = 'chapter';
    box.dispatchEvent(new Event('input'));
    await settle();
    expect(rows().length).toBe(0);
    box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await settle();
    expect(names()).toEqual(['chapter-1.docx']);
    expect(rows()[0].querySelector('.where')?.textContent).toContain('thesis');
  });

  it('creates, renames with F2 and deletes with Delete, and Undo restores', async () => {
    await setup();
    panes()[0].querySelector<HTMLButtonElement>('.actions button')?.click();
    await settle();
    const input = panes()[0].querySelector<HTMLInputElement>('.is-editing .rename') as HTMLInputElement;
    input.value = 'Invoices';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await settle();
    expect(names()).toContain('Invoices');

    row('notes.txt').click();
    key('F2');
    await settle();
    const rename = panes()[0].querySelector<HTMLInputElement>('.row .rename') as HTMLInputElement;
    expect(rename.selectionEnd).toBe('notes'.length);
    rename.value = 'notes-2026.txt';
    rename.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await settle();
    expect(names()).toContain('notes-2026.txt');

    row('report.pdf').click();
    key('Delete');
    await settle();
    expect(names()).not.toContain('report.pdf');
    expect((await disk.trashed(HOME)).map((t) => t.name)).toEqual(['report.pdf']);
    await TestBed.inject(FileOpsService).untrash([`${HOME}\\report.pdf`]);
    await settle();
    expect(names()).toContain('report.pdf');
  });

  it('copies and pastes with Ctrl+C / Ctrl+V and asks about the name clash', async () => {
    await setup();
    row('notes.txt').click();
    key('c', { ctrlKey: true });
    key('v', { ctrlKey: true });
    await settle();
    // the same folder: the name is taken by the original
    const keepBoth = Array.from(el().querySelectorAll<HTMLButtonElement>('dialog button')).find((b) => /Keep both|files\.keepBoth/.test(b.textContent ?? ''));
    expect(keepBoth).toBeDefined();
    keepBoth?.click();
    await settle();
    expect(names()).toContain('notes (1).txt');
    expect(names()).toContain('notes.txt');
  });

  it('two panes: drag a file from the left pane onto the right one moves it; Ctrl copies', async () => {
    await setup({ dual: true, split: 0.5, paths: [HOME, `${HOME}\\Archive`] });
    expect(panes().length).toBe(2);
    expect(rows(1).length).toBe(0);

    const drag = (name: string, ctrlKey: boolean): void => {
      const data = new DataTransfer();
      row(name, 0).dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: data }));
      grid(1).dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: data, ctrlKey }));
      grid(1).dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: data, ctrlKey }));
    };
    drag('report.pdf', false);
    await settle();
    expect(names(1)).toEqual(['report.pdf']);
    expect(names(0)).not.toContain('report.pdf');

    drag('notes.txt', true);
    await settle();
    expect(names(1)).toEqual(['notes.txt', 'report.pdf']);
    expect(names(0)).toContain('notes.txt');
  });

  it('reopens the folders each pane showed last time', async () => {
    await setup({ dual: true, split: 0.5, paths: [`${HOME}\\Projects`, `${HOME}\\Archive`] });
    expect(names(0)).toEqual(['thesis']);
    expect(rows(1).length).toBe(0);
    expect(panes()[1].querySelector('.crumb.is-current')?.textContent?.trim()).toBe('Archive');
  });

  it('two panes: the buttons copy the active pane selection to the other pane, F6 switches panes', async () => {
    await setup({ dual: true, split: 0.5, paths: [HOME, `${HOME}\\Archive`] });
    row('big.iso').click();
    const copy = Array.from(el().querySelectorAll<HTMLButtonElement>('.top button')).find((b) => /Copy to other|copyToOther/.test(b.textContent ?? ''));
    copy?.click();
    await settle();
    expect(names(1)).toEqual(['big.iso']);
    el().dispatchEvent(new KeyboardEvent('keydown', { key: 'F6', bubbles: true }));
    await settle();
    expect(panes()[1].classList).toContain('is-active');
  });

  it('two panes: the divider resizes from the keyboard and the layout is remembered', async () => {
    await setup({ dual: true, split: 0.5, paths: [null, null] });
    const divider = el().querySelector<HTMLElement>('.divider') as HTMLElement;
    divider.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    await settle();
    expect(divider.getAttribute('aria-valuenow')).toBe('55');
    expect(JSON.parse(prefs.get(FILES_LAYOUT_KEY) ?? '{}').split).toBeCloseTo(0.55);
    for (let i = 0; i < 10; i++) {
      divider.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    }
    await settle();
    expect(divider.getAttribute('aria-valuenow')).toBe('80');
  });

  it('a folder of 3,000 files keeps the DOM small', async () => {
    await setup();
    for (let i = 0; i < 3000; i++) {
      disk.addFile(`${HOME}\\Huge\\file-${String(i).padStart(4, '0')}.bin`, i);
    }
    row('Archive').click();
    await settle();
    panes()[0].querySelector<HTMLButtonElement>('.place')?.click();
    await settle();
    rows().find((r) => r.textContent?.includes('Huge'))?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await settle();
    expect(panes()[0].querySelector('.grid')?.getAttribute('aria-rowcount')).toBe('3001');
    expect(rows().length).toBeLessThan(60);
    key('End');
    await settle();
    expect(rows().some((r) => r.textContent?.includes('file-2999'))).toBeTrue();
    expect(rows().length).toBeLessThan(60);
  });

  it('shows properties with the folder contents', async () => {
    await setup();
    row('Projects').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 50, clientY: 50 }));
    await settle();
    const props = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((b) => /Properties|files\.properties/.test(b.textContent ?? ''));
    props?.click();
    await settle();
    const text = el().querySelector('.props')?.textContent ?? '';
    expect(text).toContain('Projects');
    expect(text).toMatch(/1 files, 1 folders|files\.containsValue/);
  });
});
