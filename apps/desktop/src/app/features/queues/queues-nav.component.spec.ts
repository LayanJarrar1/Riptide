import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadsStore } from '../../data/downloads.store';
import { provideDownloadEngine } from '../../data/engine.provider';
import { MemoryQueueBackend, QUEUE_BACKEND } from '../../data/queue-backend';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { QueuesService } from '../../data/queues.service';
import { blankSpec } from '../../model/queue';
import { TAURI_AVAILABLE } from '../../tauri/tauri-bridge';
import { DOWNLOAD_DRAG_TYPE, QUEUE_DRAG_TYPE } from './dnd';
import { QueuesNavComponent } from './queues-nav.component';

describe('QueuesNavComponent', () => {
  let fixture: ComponentFixture<QueuesNavComponent>;
  let queues: QueuesService;
  let engine: MockEngineService;
  let store: DownloadsStore;
  let opened: number[];

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const items = (): HTMLElement[] => Array.from(el().querySelectorAll<HTMLElement>('li.item'));
  const names = (): string[] => items().map((li) => li.querySelector('.name')?.textContent?.trim() ?? '');
  const flush = async (): Promise<void> => {
    await fixture.whenStable();
    fixture.detectChanges();
  };

  beforeEach(async () => {
    TestBed.configureTestingModule({
      providers: [
        provideDownloadEngine(),
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: TAURI_AVAILABLE, useValue: false },
        { provide: QUEUE_BACKEND, useValue: new MemoryQueueBackend() },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      ],
    });
    queues = TestBed.inject(QueuesService);
    engine = TestBed.inject(MockEngineService);
    store = TestBed.inject(DownloadsStore);
    await queues.ready;
    await queues.create({ ...blankSpec(), name: 'Night', maxConcurrent: 2 });
    await queues.create({ ...blankSpec(), name: 'Work', maxConcurrent: 1 });
    fixture = TestBed.createComponent(QueuesNavComponent);
    opened = [];
    fixture.componentInstance.open.subscribe((id) => opened.push(id));
    fixture.detectChanges();
  });

  it('lists Main and the custom queues in order, each with a state dot and "running/limit"', () => {
    expect(names()).toEqual(['Main queue', 'Night', 'Work']);
    const counts = items().map((li) => li.querySelector('.count')?.textContent?.trim());
    expect(counts[1]).toBe('0/2');
    expect(counts[2]).toBe('0/1');
    expect(items().every((li) => li.querySelector('.dot')?.classList.contains('is-running'))).toBeTrue();
    expect(items()[1].querySelector('.select')?.getAttribute('aria-label')).toBe('Night, Running, 0 of 2 running');
  });

  it('counts the downloads that hold a slot', async () => {
    await engine.moveToQueue([1, 2, 3, 4, 5, 6, 7, 8], 2);
    fixture.detectChanges();
    const running = engine.downloads().filter((d) => d.queueId === 2 && ['probing', 'downloading', 'verifying'].includes(d.state)).length;
    expect(items()[1].querySelector('.count')?.textContent?.trim()).toBe(`${running}/2`);
  });

  it('choosing a queue reports it', () => {
    items()[1].querySelector<HTMLButtonElement>('.select')?.click();
    expect(opened).toEqual([2]);
  });

  it('highlights the queue the list is showing', () => {
    fixture.componentRef.setInput('current', 3);
    fixture.detectChanges();
    expect(items().map((li) => li.classList.contains('is-active'))).toEqual([false, false, true]);
    expect(items()[2].querySelector('.select')?.getAttribute('aria-current')).toBe('page');
  });

  it('the start/stop button stops a running queue and shows it at once', async () => {
    items()[1].querySelector<HTMLButtonElement>('.toggle')?.click();
    fixture.detectChanges();
    expect(queues.get(2)?.enabled).toBeFalse();
    expect(items()[1].querySelector('.dot')?.classList.contains('is-running')).toBeFalse();
    expect(items()[1].querySelector('.select')?.getAttribute('aria-label')).toContain('Stopped');
    await flush();
    items()[1].querySelector<HTMLButtonElement>('.toggle')?.click();
    fixture.detectChanges();
    expect(queues.get(2)?.enabled).toBeTrue();
  });

  describe('reordering', () => {
    function key(index: number, key: string, altKey: boolean): void {
      items()[index].querySelector('.select')?.dispatchEvent(new KeyboardEvent('keydown', { key, altKey, bubbles: true, cancelable: true }));
      fixture.detectChanges();
    }

    it('Alt + arrow moves a queue up and down without a mouse', async () => {
      key(2, 'ArrowUp', true);
      expect(names()).toEqual(['Main queue', 'Work', 'Night']);
      await flush();
      key(0, 'ArrowDown', true);
      expect(names()).toEqual(['Work', 'Main queue', 'Night']);
      await flush();
      // the ends stay put
      key(0, 'ArrowUp', true);
      expect(names()).toEqual(['Work', 'Main queue', 'Night']);
    });

    it('a plain arrow only moves focus', () => {
      key(0, 'ArrowDown', false);
      expect(names()).toEqual(['Main queue', 'Night', 'Work']);
    });

    it('the menu offers move up / down, disabled at the ends', () => {
      items()[0].querySelector<HTMLButtonElement>('.more')?.click();
      fixture.detectChanges();
      const menuItems = Array.from(el().querySelectorAll<HTMLButtonElement>('app-menu .item'));
      const labels = menuItems.map((b) => b.querySelector('.label')?.textContent?.trim());
      expect(labels).toEqual(['Stop queue', 'Queue settings…', 'Move up', 'Move down']);
      expect(menuItems[2].disabled).toBeTrue();
      expect(menuItems[3].disabled).toBeFalse();
      menuItems[3].click();
      fixture.detectChanges();
      expect(names()).toEqual(['Night', 'Main queue', 'Work']);
    });
  });

  describe('delete', () => {
    function openMenuOf(index: number): HTMLButtonElement[] {
      items()[index].querySelector<HTMLButtonElement>('.more')?.click();
      fixture.detectChanges();
      return Array.from(el().querySelectorAll<HTMLButtonElement>('app-menu .item'));
    }

    it('Main cannot be deleted', () => {
      const labels = openMenuOf(0).map((b) => b.querySelector('.label')?.textContent?.trim());
      expect(labels).not.toContain('Delete queue');
    });

    it('asks first, says the downloads move to Main, and deletes on confirm', async () => {
      await engine.moveToQueue([1, 2], 3);
      fixture.detectChanges();
      const menu = openMenuOf(2);
      expect(menu.at(-1)?.querySelector('.label')?.textContent?.trim()).toBe('Delete queue');
      menu.at(-1)?.click();
      fixture.detectChanges();
      const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
      expect(dialog?.textContent).toContain('“Work” will be deleted');
      expect(dialog?.textContent).toContain('Its 2 downloads move to the Main queue');
      expect(names()).toContain('Work');

      dialog?.querySelector<HTMLButtonElement>('.foot .v-danger')?.click();
      await flush();
      expect(names()).toEqual(['Main queue', 'Night']);
      expect(queues.get(3)).toBeUndefined();
    });

    it('cancel keeps the queue', () => {
      openMenuOf(1).at(-1)?.click();
      fixture.detectChanges();
      const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
      dialog?.querySelector<HTMLButtonElement>('.foot .v-ghost')?.click();
      fixture.detectChanges();
      expect(names()).toContain('Night');
    });

    it('leaves the queue filter when the shown queue is deleted', async () => {
      store.setQueue(3);
      fixture.detectChanges();
      openMenuOf(2).at(-1)?.click();
      fixture.detectChanges();
      Array.from(el().querySelectorAll<HTMLDialogElement>('dialog'))
        .find((d) => d.open)
        ?.querySelector<HTMLButtonElement>('.foot .v-danger')
        ?.click();
      await flush();
      expect(store.queue()).toBeNull();
    });
  });

  it('the plus button opens the new-queue dialog', async () => {
    el().querySelector<HTMLButtonElement>('.add')?.click();
    await flush();
    await flush();
    const dialog = Array.from(el().querySelectorAll<HTMLDialogElement>('dialog')).find((d) => d.open);
    expect(dialog?.querySelector('h2')?.textContent?.trim()).toBe('New queue');
  });

  describe('dragging', () => {
    function stub(li: HTMLElement): void {
      spyOn(li, 'getBoundingClientRect').and.returnValue({ top: 0, bottom: 30, height: 30, left: 0, right: 100, width: 100, x: 0, y: 0, toJSON: () => ({}) });
    }

    it('a queue dragged onto another reorders them, with a mark where it will land', async () => {
      const dt = new DataTransfer();
      stub(items()[0]);
      items()[2].dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
      expect(dt.types).toContain(QUEUE_DRAG_TYPE);
      items()[0].dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt, clientY: 3 }));
      fixture.detectChanges();
      expect(items()[0].classList).toContain('drop-before');
      items()[0].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
      await flush();
      expect(names()).toEqual(['Work', 'Main queue', 'Night']);
      expect(items().some((li) => li.classList.contains('drop-before'))).toBeFalse();
    });

    it('a queue dropped on itself changes nothing', async () => {
      const dt = new DataTransfer();
      stub(items()[1]);
      items()[1].dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
      items()[1].dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt, clientY: 25 }));
      items()[1].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
      await flush();
      expect(names()).toEqual(['Main queue', 'Night', 'Work']);
    });

    it('download rows dropped on a queue move there', async () => {
      const dt = new DataTransfer();
      dt.setData(DOWNLOAD_DRAG_TYPE, JSON.stringify([1, 2]));
      items()[2].dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt }));
      fixture.detectChanges();
      expect(items()[2].classList).toContain('drop-into');
      items()[2].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
      await flush();
      expect(engine.get(1)?.queueId).toBe(3);
      expect(engine.get(2)?.queueId).toBe(3);
      expect(items()[2].classList).not.toContain('drop-into');
    });

    it('ignores drops that carry something else', async () => {
      const dt = new DataTransfer();
      dt.setData('text/plain', 'hello');
      const over = new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt });
      items()[1].dispatchEvent(over);
      expect(over.defaultPrevented).toBeFalse();
    });
  });
});
