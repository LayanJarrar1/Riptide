import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { MemoryQueueBackend, QUEUE_BACKEND } from '../../data/queue-backend';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { TAURI_AVAILABLE } from '../../tauri/tauri-bridge';
import { PostActionDialogComponent } from './post-action-dialog.component';

describe('PostActionDialogComponent', () => {
  let fixture: ComponentFixture<PostActionDialogComponent>;
  let backend: MemoryQueueBackend;
  let queues: QueuesService;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const dialog = (): HTMLDialogElement => el().querySelector('dialog') as HTMLDialogElement;
  const buttons = (): HTMLButtonElement[] => Array.from(el().querySelectorAll<HTMLButtonElement>('.foot button'));
  const count = (): string => el().querySelector('.count')?.textContent?.trim() ?? '';

  /** Moves the fake clock and lets the view catch up. */
  function seconds(n: number): void {
    jasmine.clock().tick(n * 1000);
    fixture.detectChanges();
  }

  beforeEach(async () => {
    jasmine.clock().install();
    backend = new MemoryQueueBackend();
    TestBed.configureTestingModule({
      imports: [PostActionDialogComponent],
      providers: [
        { provide: TAURI_AVAILABLE, useValue: false },
        { provide: QUEUE_BACKEND, useValue: backend },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      ],
    });
    queues = TestBed.inject(QueuesService);
    await queues.ready;
    fixture = TestBed.createComponent(PostActionDialogComponent);
    fixture.detectChanges();
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  it('stays closed until a queue with an action finishes', () => {
    expect(dialog().open).toBeFalse();
    queues.finished.next({ queueId: 1, action: null });
    fixture.detectChanges();
    expect(dialog().open).toBeFalse();
  });

  it('shows "Shutting down in 30 s" and counts down', () => {
    queues.finished.next({ queueId: 1, action: { action: 'shutdown' } });
    fixture.detectChanges();
    expect(dialog().open).toBeTrue();
    expect(count()).toBe('Shutting down in 30 s');
    seconds(3);
    expect(count()).toBe('Shutting down in 27 s');
    expect(el().textContent).toContain('Main queue');
  });

  it('words every action in its own way', () => {
    const cases: [Parameters<typeof queues.finished.next>[0]['action'], string][] = [
      [{ action: 'exit' }, 'Exiting Riptide in 30 s'],
      [{ action: 'sleep' }, 'Going to sleep in 30 s'],
      [{ action: 'hibernate' }, 'Hibernating in 30 s'],
      [{ action: 'run_command', command: 'C:\\x.exe' }, 'Running your program in 30 s'],
    ];
    for (const [action, text] of cases) {
      queues.finished.next({ queueId: 1, action });
      fixture.detectChanges();
      expect(count()).toBe(text);
    }
  });

  it('Cancel closes it and nothing happens afterwards', () => {
    queues.finished.next({ queueId: 1, action: { action: 'shutdown' } });
    fixture.detectChanges();
    buttons()[0].click();
    fixture.detectChanges();
    expect(dialog().open).toBeFalse();
    seconds(90);
    expect(backend.performed).toEqual([]);
  });

  it('"Do it now" performs the action once', () => {
    queues.finished.next({ queueId: 1, action: { action: 'sleep' } });
    fixture.detectChanges();
    buttons()[1].click();
    fixture.detectChanges();
    expect(dialog().open).toBeFalse();
    seconds(60);
    expect(backend.performed).toEqual([1]);
  });

  it('runs out on its own after 30 seconds, once', () => {
    queues.finished.next({ queueId: 1, action: { action: 'hibernate' } });
    fixture.detectChanges();
    seconds(29);
    expect(count()).toBe('Hibernating in 1 s');
    seconds(1);
    expect(dialog().open).toBeFalse();
    seconds(60);
    expect(backend.performed).toEqual([1]);
  });

  it('is announced to screen readers at the start, every ten seconds and in the last five only', () => {
    queues.finished.next({ queueId: 1, action: { action: 'shutdown' } });
    fixture.detectChanges();
    const said = (): string => el().querySelector('.sr-only')?.textContent?.trim() ?? '';
    expect(said()).toBe('Shutting down in 30 s');
    seconds(3);
    expect(said()).toBe('Shutting down in 30 s');
    seconds(7);
    expect(said()).toBe('Shutting down in 20 s');
    seconds(16);
    expect(said()).toBe('Shutting down in 4 s');
  });

  it('speaks Arabic when the language is Arabic', () => {
    TestBed.inject(I18nService).setLang('ar');
    queues.finished.next({ queueId: 1, action: { action: 'shutdown' } });
    fixture.detectChanges();
    expect(count()).toContain('سيُطفأ الجهاز بعد');
    expect(count()).toContain('30');
  });
});
