import { TestBed } from '@angular/core/testing';

import { MemoryQueueBackend, QUEUE_BACKEND } from '../../data/queue-backend';
import { QueuesService } from '../../data/queues.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { TAURI_AVAILABLE } from '../../tauri/tauri-bridge';
import { ToastService } from '../../ui/toast.service';
import { COUNTDOWN_SECONDS, PostActionCountdown } from './post-action-countdown.service';

/** Records `perform` calls; never touches the machine. */
class RecordingBackend extends MemoryQueueBackend {
  fail: string | null = null;

  override perform(queueId: number): Promise<void> {
    this.performed.push(queueId);
    return this.fail === null ? Promise.resolve() : Promise.reject(new Error(this.fail));
  }
}

describe('PostActionCountdown', () => {
  let backend: RecordingBackend;
  let queues: QueuesService;
  let countdown: PostActionCountdown;
  let toasts: ToastService;

  beforeEach(async () => {
    jasmine.clock().install();
    backend = new RecordingBackend();
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: false },
        { provide: QUEUE_BACKEND, useValue: backend },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      ],
    });
    queues = TestBed.inject(QueuesService);
    await queues.ready;
    toasts = TestBed.inject(ToastService);
    countdown = TestBed.inject(PostActionCountdown);
  });

  afterEach(() => {
    jasmine.clock().uninstall();
  });

  const finish = (queueId: number, action: Parameters<typeof countdown.start>[1] | null): void => queues.finished.next({ queueId, action });
  const seconds = (n: number): void => jasmine.clock().tick(n * 1000);

  it('stays idle for a queue that finishes without an action', () => {
    finish(1, null);
    expect(countdown.pending()).toBeNull();
    seconds(60);
    expect(backend.performed).toEqual([]);
  });

  it('starts at 30 seconds when a queue with an action finishes and counts down once a second', () => {
    finish(2, { action: 'shutdown' });
    expect(countdown.pending()).toEqual({ queueId: 2, action: { action: 'shutdown' }, remaining: COUNTDOWN_SECONDS });
    expect(COUNTDOWN_SECONDS).toBe(30);
    seconds(3);
    expect(countdown.pending()?.remaining).toBe(27);
    seconds(1);
    expect(countdown.pending()?.remaining).toBe(26);
    expect(backend.performed).toEqual([]);
  });

  it('cancel stops the countdown for good: nothing is performed, not even later', () => {
    finish(2, { action: 'shutdown' });
    seconds(10);
    countdown.cancel();
    expect(countdown.pending()).toBeNull();
    seconds(120);
    expect(backend.performed).toEqual([]);
    expect(countdown.pending()).toBeNull();
  });

  it('fires the action exactly once when it reaches zero', async () => {
    finish(2, { action: 'sleep' });
    seconds(29);
    expect(countdown.pending()?.remaining).toBe(1);
    expect(backend.performed).toEqual([]);
    seconds(1);
    expect(backend.performed).toEqual([2]);
    expect(countdown.pending()).toBeNull();
    seconds(300);
    await Promise.resolve();
    expect(backend.performed).toEqual([2]);
  });

  it('"Do it now" performs at once, once, and ends the countdown', () => {
    finish(3, { action: 'hibernate' });
    seconds(5);
    countdown.doNow();
    countdown.doNow();
    expect(backend.performed).toEqual([3]);
    expect(countdown.pending()).toBeNull();
    seconds(60);
    expect(backend.performed).toEqual([3]);
  });

  it('a second queue finishing replaces the pending action and restarts the clock', () => {
    finish(2, { action: 'shutdown' });
    seconds(20);
    finish(3, { action: 'exit' });
    expect(countdown.pending()).toEqual({ queueId: 3, action: { action: 'exit' }, remaining: 30 });
    seconds(29);
    expect(backend.performed).toEqual([]);
    seconds(1);
    expect(backend.performed).toEqual([3]);
  });

  it('cancel with nothing pending is harmless', () => {
    expect(() => countdown.cancel()).not.toThrow();
    expect(countdown.pending()).toBeNull();
  });

  it('tells the user when the action could not be carried out', async () => {
    backend.fail = 'the program does not exist';
    finish(2, { action: 'run_command', command: 'C:\\gone.exe' });
    countdown.doNow();
    await Promise.resolve();
    await Promise.resolve();
    const toast = toasts.toasts().at(-1);
    expect(toast?.tone).toBe('danger');
    expect(toast?.text).toContain('the program does not exist');
  });
});
