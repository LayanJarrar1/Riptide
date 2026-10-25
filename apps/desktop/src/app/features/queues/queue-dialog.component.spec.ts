import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { NOW } from '../../core/clock';
import { IS_MAC } from '../../core/platform';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { MemoryQueueBackend, QUEUE_BACKEND } from '../../data/queue-backend';
import { QueuesService } from '../../data/queues.service';
import { UiQueue, UiQueueSpec } from '../../model/queue';
import type { PostAction } from '../../tauri/bridge/PostAction';
import { TAURI_AVAILABLE } from '../../tauri/tauri-bridge';
import { QueueDialogComponent } from './queue-dialog.component';

/** The memory backend, with a validator that knows which programs exist. */
class TestBackend extends MemoryQueueBackend {
  readonly created: UiQueueSpec[] = [];
  readonly existing = new Set(['C:\\Tools\\done.exe']);

  override create(spec: UiQueueSpec): Promise<number> {
    this.created.push(spec);
    return super.create(spec);
  }

  override validate(action: PostAction): Promise<void> {
    return action.action === 'run_command' && !this.existing.has(action.command) ? Promise.reject(new Error('the program does not exist')) : Promise.resolve();
  }
}

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [QueueDialogComponent],
  template: `<app-queue-dialog [open]="open()" [queue]="queue()" (closed)="closes.set(closes() + 1); open.set(false)" />`,
})
class HostComponent {
  readonly open = signal(true);
  readonly queue = signal<UiQueue | null>(null);
  readonly closes = signal(0);
}

describe('QueueDialogComponent', () => {
  let fixture: ComponentFixture<HostComponent>;
  let host: HostComponent;
  let backend: TestBackend;
  let queues: QueuesService;

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const q = <T extends HTMLElement>(selector: string): T => el().querySelector<T>(selector) as T;
  const save = async (): Promise<void> => {
    q<HTMLButtonElement>('.foot .v-primary').click();
    await fixture.whenStable();
    fixture.detectChanges();
  };
  function type(selector: string, value: string, event = 'input'): void {
    const input = q<HTMLInputElement | HTMLSelectElement>(selector);
    input.value = value;
    input.dispatchEvent(new Event(event));
    fixture.detectChanges();
  }

  beforeEach(async () => {
    backend = new TestBackend();
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: false },
        { provide: QUEUE_BACKEND, useValue: backend },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: NOW, useValue: () => Date.UTC(2026, 9, 19, 12, 0) },
      ],
    });
    queues = TestBed.inject(QueuesService);
    await queues.ready;
    fixture = TestBed.createComponent(HostComponent);
    host = fixture.componentInstance;
    fixture.detectChanges();
  });

  it('offers Hibernate after a queue on Windows', () => {
    const options = Array.from(q<HTMLSelectElement>('select[name="after"]').options).map((o) => o.value);
    expect(options).toContain('hibernate');
  });

  it('does not offer Hibernate on a Mac, where the system decides it', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: false },
        { provide: QUEUE_BACKEND, useValue: new TestBackend() },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: NOW, useValue: () => Date.UTC(2026, 9, 19, 12, 0) },
        { provide: IS_MAC, useValue: true },
      ],
    });
    const mac = TestBed.createComponent(HostComponent);
    mac.detectChanges();
    const options = Array.from((mac.nativeElement as HTMLElement).querySelector<HTMLSelectElement>('select[name="after"]')?.options ?? []).map((o) => o.value);
    expect(options).toContain('sleep');
    expect(options).not.toContain('hibernate');
  });

  it('opens as "New queue" with a blank form', () => {
    expect(q('h2').textContent?.trim()).toBe('New queue');
    expect(q<HTMLInputElement>('input[name="name"]').value).toBe('');
    expect(q<HTMLSelectElement>('select[name="after"]').value).toBe('nothing');
  });

  it('will not create a queue without a name', async () => {
    await save();
    expect(q('.error').textContent).toContain('Enter a name');
    expect(backend.created).toEqual([]);
    expect(host.closes()).toBe(0);
  });

  it('creates a queue with its limit, the night schedule and an action, then closes', async () => {
    type('input[name="name"]', '  Night downloads ');
    type('input[name="max"]', '4');
    q<HTMLButtonElement>('.chip').click();
    fixture.detectChanges();
    type('select[name="after"]', 'shutdown', 'change');
    await save();
    expect(backend.created.length).toBe(1);
    expect(backend.created[0]).toEqual({
      name: 'Night downloads',
      maxConcurrent: 4,
      schedule: { window: { start_min: 1380, stop_min: 420 }, days: 127, start_at: null },
      onDone: { action: 'shutdown' },
    });
    expect(host.closes()).toBe(1);
    expect(queues.queues().map((x) => x.name)).toEqual(['Main', 'Night downloads']);
  });

  it('keeps the limit between 1 and 32', async () => {
    type('input[name="name"]', 'A');
    type('input[name="max"]', '99');
    await save();
    expect(backend.created[0].maxConcurrent).toBe(32);
  });

  it('a program action needs a path that exists, and says so next to the field', async () => {
    type('input[name="name"]', 'Done');
    type('select[name="after"]', 'run_command', 'change');
    await save();
    expect(q('.error').textContent).toContain('full path');

    type('input[name="command"]', 'C:\\Missing\\x.exe');
    await save();
    expect(q('.error').textContent).toContain('the program does not exist');
    expect(backend.created).toEqual([]);
    expect(host.closes()).toBe(0);

    type('input[name="command"]', 'C:\\Tools\\done.exe');
    await save();
    expect(backend.created[0].onDone).toEqual({ action: 'run_command', command: 'C:\\Tools\\done.exe' });
    expect(host.closes()).toBe(1);
  });

  it('will not save a schedule the engine would refuse', async () => {
    type('input[name="name"]', 'Odd');
    q<HTMLInputElement>('input[name="restrict"]').click();
    fixture.detectChanges();
    type('select[name="to-h"]', '23', 'change');
    type('select[name="to-m"]', '00', 'change');
    await save();
    expect(el().textContent).toContain('Start and stop time must be different');
    expect(backend.created).toEqual([]);
  });

  it('shows the engine\'s refusal (a taken name) and stays open', async () => {
    await queues.create({ name: 'Taken', maxConcurrent: 1, schedule: { window: null, days: 127, start_at: null }, onDone: null });
    type('input[name="name"]', 'taken');
    await save();
    expect(q('.error').textContent).toContain('already exists');
    expect(host.closes()).toBe(0);
  });

  describe('editing', () => {
    it('starts from the queue and saves changes to it', async () => {
      const id = await queues.create({ name: 'Work', maxConcurrent: 2, schedule: { window: null, days: 127, start_at: null }, onDone: { action: 'sleep' } });
      host.queue.set(queues.get(id) ?? null);
      host.open.set(false);
      fixture.detectChanges();
      host.open.set(true);
      fixture.detectChanges();
      expect(q('h2').textContent?.trim()).toBe('Queue settings');
      expect(q<HTMLInputElement>('input[name="name"]').value).toBe('Work');
      expect(q<HTMLSelectElement>('select[name="after"]').value).toBe('sleep');
      type('input[name="name"]', 'Office');
      await save();
      expect(queues.get(id)?.name).toBe('Office');
      expect(host.closes()).toBe(1);
    });

    it('Main keeps its name and follows the limit in Settings', () => {
      host.queue.set(queues.get(1) ?? null);
      host.open.set(false);
      fixture.detectChanges();
      host.open.set(true);
      fixture.detectChanges();
      expect(q<HTMLInputElement>('input[name="name"]').disabled).toBeTrue();
      expect(q<HTMLInputElement>('input[name="name"]').value).toBe('Main queue');
      expect(q<HTMLInputElement>('input[name="max"]').disabled).toBeTrue();
      expect(el().textContent).toContain('follows the limit in Settings');
    });
  });
});
