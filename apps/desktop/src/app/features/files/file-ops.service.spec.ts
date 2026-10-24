import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { ToastService } from '../../ui/toast.service';
import { FILE_BACKEND, MemoryFileBackend, nameProblem } from './file-backend';
import { baseName, FileOpsService, parentPath } from './file-ops.service';
import { FOLDER_COLORS_KEY, FolderColorsService } from './folder-colors.service';

const D = 'C:\\Data';

describe('file manager services', () => {
  let disk: MemoryFileBackend;
  let ops: FileOpsService;
  let prefs: MemoryPreferenceStore;
  let toasts: ToastService;

  beforeEach(() => {
    disk = new MemoryFileBackend(false);
    disk.addFile(`${D}\\src\\a.txt`, 10);
    disk.addFile(`${D}\\src\\album\\01.mp3`, 3000);
    disk.addFile(`${D}\\dest\\a.txt`, 99);
    prefs = new MemoryPreferenceStore();
    TestBed.configureTestingModule({
      providers: [
        { provide: FILE_BACKEND, useValue: disk },
        { provide: PREFERENCE_STORE, useValue: prefs },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      ],
    });
    ops = TestBed.inject(FileOpsService);
    toasts = TestBed.inject(ToastService);
  });

  it('path helpers handle drive roots and trailing separators', () => {
    expect(parentPath('C:\\Data\\src')).toBe('C:\\Data');
    expect(parentPath('C:\\Data')).toBe('C:\\');
    expect(parentPath('/home/me/x/')).toBe('/home/me');
    expect(baseName('C:\\Data\\src\\a.txt')).toBe('a.txt');
    expect(baseName('/home/me/x/')).toBe('x');
  });

  it('validates names like Windows does', () => {
    expect(nameProblem('Report 2026.pdf')).toBeNull();
    expect(nameProblem('مشروع')).toBeNull();
    for (const bad of ['', ' ', '..', 'a/b', 'a\\b', 'x?', 'con', 'NUL.txt', 'dot.']) {
      expect(nameProblem(bad)).withContext(bad).not.toBeNull();
    }
  });

  it('asks before overwriting: keep both adds a numbered copy and leaves the original', async () => {
    const pending = ops.transfer([`${D}\\src\\a.txt`, `${D}\\src\\album`], `${D}\\dest`, 'copy');
    await waitFor(() => ops.conflict() !== null);
    expect(ops.conflict()?.plan.conflicts.length).toBe(1);
    ops.conflict()?.answer('keepBoth');
    const report = await pending;
    expect(report?.done.map((d) => baseName(d.to)).sort()).toEqual(['a (1).txt', 'album']);
    const names = (await disk.list(`${D}\\dest`)).entries.map((e) => e.name).sort();
    expect(names).toEqual(['a (1).txt', 'a.txt', 'album']);
    expect(disk.exists(`${D}\\src\\a.txt`)).toBeTrue();
    expect(ops.running()).toBeNull();
  });

  it('cancelling the conflict question changes nothing', async () => {
    const pending = ops.transfer([`${D}\\src\\a.txt`], `${D}\\dest`, 'move');
    await waitFor(() => ops.conflict() !== null);
    ops.conflict()?.answer(null);
    expect(await pending).toBeNull();
    expect(disk.exists(`${D}\\src\\a.txt`)).toBeTrue();
  });

  it('replace sends the existing item to the Recycle Bin, where it can be restored', async () => {
    const pending = ops.transfer([`${D}\\src\\a.txt`], `${D}\\dest`, 'move');
    await waitFor(() => ops.conflict() !== null);
    ops.conflict()?.answer('replace');
    await pending;
    expect(disk.exists(`${D}\\src\\a.txt`)).toBeFalse();
    const binned = await disk.trashed(`${D}\\dest`);
    expect(binned.map((b) => b.name)).toEqual(['a.txt']);
  });

  it('refuses a folder into itself and reports it instead of failing silently', async () => {
    const show = spyOn(toasts, 'show').and.callThrough();
    expect(await ops.transfer([`${D}\\src`], `${D}\\src\\album`, 'move')).toBeNull();
    expect(show).toHaveBeenCalledWith('danger', jasmine.stringMatching(/into itself/), undefined, 6000);
  });

  it('a cut is pasted once and then the clipboard is empty', async () => {
    ops.cut([`${D}\\src\\album`]);
    await ops.paste(`${D}\\dest`);
    expect(disk.exists(`${D}\\dest\\album\\01.mp3`)).toBeTrue();
    expect(disk.exists(`${D}\\src\\album`)).toBeFalse();
    expect(ops.clipboard()).toBeNull();
  });

  it('delete goes to the Recycle Bin and Undo brings it back', async () => {
    const show = spyOn(toasts, 'show').and.callThrough();
    expect(await ops.trash([`${D}\\src\\a.txt`])).toBeTrue();
    expect(disk.exists(`${D}\\src\\a.txt`)).toBeFalse();
    const undo = show.calls.mostRecent().args[2] as { run: () => void };
    undo.run();
    await waitFor(() => disk.exists(`${D}\\src\\a.txt`));
    expect(disk.exists(`${D}\\src\\a.txt`)).toBeTrue();
  });

  it('rename can be undone and a renamed folder keeps its colour', async () => {
    const colors = TestBed.inject(FolderColorsService);
    colors.set(`${D}\\src\\album`, 'violet');
    const show = spyOn(toasts, 'show').and.callThrough();
    const renamed = await ops.rename(`${D}\\src\\album`, 'Album 2026');
    expect(renamed).toBe(`${D}\\src\\Album 2026`);
    expect(colors.colorOf(`${D}\\src\\Album 2026`)).toBe('violet');
    expect(colors.colorOf(`${D}\\src\\album`)).toBeNull();
    expect(JSON.parse(prefs.get(FOLDER_COLORS_KEY) ?? '{}')).toEqual({ 'c:\\data\\src\\album 2026': 'violet' });
    (show.calls.mostRecent().args[2] as { run: () => void }).run();
    await waitFor(() => disk.exists(`${D}\\src\\album`));
    expect(disk.exists(`${D}\\src\\album`)).toBeTrue();
  });

  it('rejects a bad name with the reason', async () => {
    const show = spyOn(toasts, 'show').and.callThrough();
    expect(await ops.createFolder(`${D}\\src`, 'a:b')).toBeNull();
    expect(show).toHaveBeenCalledWith('danger', jasmine.stringMatching(/cannot contain/), undefined, 6000);
    expect(await ops.createFolder(`${D}\\src`, 'album')).toBeNull();
    expect(await ops.createFolder(`${D}\\src`, 'New')).toBe(`${D}\\src\\New`);
  });

  it('a second transfer waits for the first instead of running alongside it', async () => {
    disk.transferDelayMs = 30;
    const first = ops.transfer([`${D}\\src\\album`], `${D}\\dest`, 'copy');
    await waitFor(() => ops.running() !== null);
    expect(await ops.transfer([`${D}\\src\\a.txt`], `${D}\\dest\\album`, 'copy')).toBeNull();
    await first;
  });

  it('folder colours ignore unknown values and survive a reload', () => {
    prefs.set(FOLDER_COLORS_KEY, JSON.stringify({ 'C:\\X': 'blue', 'C:\\Y': 'neon', 'C:\\Z\\': 'teal' }));
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: PREFERENCE_STORE, useValue: prefs }] });
    const colors = TestBed.inject(FolderColorsService);
    expect(colors.colorOf('c:\\x')).toBe('blue');
    expect(colors.colorOf('C:\\Y')).toBeNull();
    expect(colors.colorOf('C:\\Z')).toBe('teal');
    colors.set('C:\\X', null);
    expect(colors.colorOf('C:\\X')).toBeNull();
  });
});

async function waitFor(cond: () => boolean): Promise<void> {
  for (let i = 0; i < 100 && !cond(); i++) {
    await new Promise((r) => setTimeout(r, 5));
  }
}
