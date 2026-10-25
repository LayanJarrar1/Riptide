import { TestBed } from '@angular/core/testing';

import { IS_MAC } from '../../core/platform';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { ToastService } from '../../ui/toast.service';
import { FILE_BACKEND, MemoryFileBackend } from './file-backend';
import { FileOpsService } from './file-ops.service';

/** The file manager as it behaves on a Mac: Finder's words, and no restore from the Trash. */
describe('file manager on macOS', () => {
  let disk: MemoryFileBackend;
  let ops: FileOpsService;
  let toasts: ToastService;

  function setup(mac: boolean, canRestore: boolean): void {
    disk = new MemoryFileBackend(false);
    disk.canRestore = canRestore;
    disk.addFile('C:\\Data\\a.txt', 10);
    TestBed.configureTestingModule({
      providers: [
        { provide: FILE_BACKEND, useValue: disk },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: IS_MAC, useValue: mac },
      ],
    });
    ops = TestBed.inject(FileOpsService);
    toasts = TestBed.inject(ToastService);
  }

  it('speaks of the Trash and Finder on a Mac, of the Recycle Bin and Explorer elsewhere', () => {
    setup(true, false);
    expect(ops.platformKey('files.recycleBin')).toBe('files.trash');
    expect(ops.platformKey('files.showInExplorer')).toBe('files.showInFinder');
    expect(ops.platformKey('files.trashedOne')).toBe('files.trashedOneMac');
    expect(ops.platformKey('files.copy')).toBe('files.copy');
    TestBed.resetTestingModule();
    setup(false, true);
    expect(ops.platformKey('files.recycleBin')).toBe('files.recycleBin');
  });

  it('offers no Undo for a delete when the system cannot restore from its bin', async () => {
    setup(true, false);
    await new Promise((r) => setTimeout(r, 0));
    expect(ops.canRestore()).toBeFalse();
    const show = spyOn(toasts, 'show').and.callThrough();
    expect(await ops.trash(['C:\\Data\\a.txt'])).toBeTrue();
    expect(show.calls.mostRecent().args[2]).toBeUndefined();
  });

  it('keeps Undo where the bin can be restored', async () => {
    setup(false, true);
    await new Promise((r) => setTimeout(r, 0));
    const show = spyOn(toasts, 'show').and.callThrough();
    await ops.trash(['C:\\Data\\a.txt']);
    expect(show.calls.mostRecent().args[2]).toBeDefined();
  });
});
