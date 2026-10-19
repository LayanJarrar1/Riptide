import { TestBed } from '@angular/core/testing';

import { blankSpec, QueueFinished, UiQueue } from '../model/queue';
import type { Event as EngineEvent } from '../tauri/bridge/Event';
import type { Queue } from '../tauri/bridge/Queue';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { MemoryQueueBackend, QUEUE_BACKEND } from './queue-backend';
import { QueuesService } from './queues.service';
import { ENGINE_EVENT } from './tauri-engine.service';

const NO_SCHEDULE = { window: null, days: 127, start_at: null };

function wire(id: number, name: string, position: number, extra: Partial<Queue> = {}): Queue {
  return { id, name, max_concurrent: 2, position, enabled: true, schedule: NO_SCHEDULE, on_done: null, ...extra };
}

/** A fake Rust side: a table of queues the commands read and write, and an event pipe. */
class FakeRust {
  queues: Queue[] = [wire(1, 'Main', 0), wire(2, 'Night', 1), wire(3, 'Work', 2)];
  readonly calls: { cmd: string; args: Record<string, unknown> | undefined }[] = [];
  /** Commands that should be refused, with the reason. */
  readonly refuse = new Map<string, string>();
  private handler: ((e: EngineEvent) => void) | null = null;
  listened: string[] = [];

  readonly invoke: InvokeFn = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    this.calls.push({ cmd, args });
    const reason = this.refuse.get(cmd);
    if (reason !== undefined) {
      return Promise.reject(new Error(reason));
    }
    const id = typeof args?.['id'] === 'number' ? args['id'] : -1;
    switch (cmd) {
      case 'list_queues':
        return Promise.resolve(structuredClone(this.queues) as unknown as T);
      case 'update_queue': {
        const spec = args?.['spec'] as { name: string; max_concurrent: number; schedule: Queue['schedule']; on_done: Queue['on_done'] };
        this.queues = this.queues.map((q) => (q.id === id ? { ...q, ...spec, max_concurrent: spec.max_concurrent } : q));
        return Promise.resolve(this.queues.find((q) => q.id === id) as unknown as T);
      }
      case 'create_queue':
        this.queues.push(wire(9, (args?.['spec'] as { name: string }).name, this.queues.length));
        return Promise.resolve(9 as unknown as T);
      case 'delete_queue':
        this.queues = this.queues.filter((q) => q.id !== id);
        return Promise.resolve(undefined as unknown as T);
      case 'stop_queue':
      case 'start_queue':
        this.queues = this.queues.map((q) => (q.id === id ? { ...q, enabled: cmd === 'start_queue' } : q));
        return Promise.resolve(undefined as unknown as T);
      case 'reorder_queues': {
        const order = args?.['order'] as number[];
        this.queues = order.map((qid, i) => ({ ...(this.queues.find((q) => q.id === qid) as Queue), position: i }));
        return Promise.resolve(undefined as unknown as T);
      }
      default:
        return Promise.resolve(undefined as unknown as T);
    }
  };

  readonly listen: ListenFn = <T>(event: string, handler: (payload: T) => void): Promise<() => void> => {
    this.listened.push(event);
    this.handler = handler as unknown as (e: EngineEvent) => void;
    return Promise.resolve(() => {
      this.handler = null;
    });
  };

  emit(e: EngineEvent): void {
    this.handler?.(e);
  }

  commands(cmd: string): unknown[] {
    return this.calls.filter((c) => c.cmd === cmd).map((c) => c.args);
  }
}

async function settle(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    await Promise.resolve();
  }
}

describe('QueuesService (engine side)', () => {
  let rust: FakeRust;
  let service: QueuesService;

  beforeEach(async () => {
    rust = new FakeRust();
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: true },
        { provide: TAURI_INVOKE, useValue: rust.invoke },
        { provide: TAURI_LISTEN, useValue: rust.listen },
      ],
    });
    service = TestBed.inject(QueuesService);
    await service.ready;
  });

  const names = (): string[] => service.queues().map((q) => q.name);

  it('loads the queues in display order and listens to riptide://event', () => {
    expect(rust.listened).toEqual([ENGINE_EVENT]);
    expect(service.loaded()).toBeTrue();
    expect(names()).toEqual(['Main', 'Night', 'Work']);
    expect(service.get(2)?.maxConcurrent).toBe(2);
  });

  describe('events', () => {
    it('queue_created and queue_updated re-read the list', async () => {
      rust.queues = [...rust.queues, wire(4, 'Weekend', 3)];
      rust.emit({ type: 'queue_created', queue_id: 4 });
      await settle();
      expect(names()).toEqual(['Main', 'Night', 'Work', 'Weekend']);
      rust.queues = rust.queues.map((q) => (q.id === 2 ? { ...q, enabled: false, name: 'Late' } : q));
      rust.emit({ type: 'queue_updated', queue_id: 2 });
      await settle();
      expect(service.get(2)).toEqual(jasmine.objectContaining({ name: 'Late', enabled: false }));
    });

    it('queues_reordered re-reads the list in its new order', async () => {
      rust.queues = [wire(3, 'Work', 0), wire(1, 'Main', 1), wire(2, 'Night', 2)];
      rust.emit({ type: 'queues_reordered' });
      await settle();
      expect(names()).toEqual(['Work', 'Main', 'Night']);
    });

    it('queue_deleted drops the queue without asking again', async () => {
      const reads = rust.commands('list_queues').length;
      rust.emit({ type: 'queue_deleted', queue_id: 2 });
      expect(names()).toEqual(['Main', 'Work']);
      expect(rust.commands('list_queues').length).toBe(reads);
    });

    it('queue_finished is announced with its action, and an unrelated event changes nothing', () => {
      const seen: QueueFinished[] = [];
      service.finished.subscribe((e) => seen.push(e));
      rust.emit({ type: 'queue_finished', queue_id: 2, action: { action: 'shutdown' } });
      rust.emit({ type: 'queue_finished', queue_id: 3, action: null });
      rust.emit({ type: 'queue_order_changed', queue_id: 2 });
      expect(seen).toEqual([
        { queueId: 2, action: { action: 'shutdown' } },
        { queueId: 3, action: null },
      ]);
      expect(names()).toEqual(['Main', 'Night', 'Work']);
    });

    it('a slow older read never overwrites a newer one', async () => {
      const first = service.reload();
      rust.queues = [wire(1, 'Main', 0)];
      const second = service.reload();
      await Promise.all([first, second]);
      expect(names()).toEqual(['Main']);
    });
  });

  describe('optimistic updates', () => {
    it('start and stop show at once and send the command', async () => {
      const pending = service.setRunning(2, false);
      expect(service.get(2)?.enabled).toBeFalse();
      await pending;
      expect(rust.commands('stop_queue')).toEqual([{ id: 2 }]);
      await service.setRunning(2, true);
      expect(service.get(2)?.enabled).toBeTrue();
      expect(rust.commands('start_queue')).toEqual([{ id: 2 }]);
    });

    it('a refused stop puts the queue back to running', async () => {
      rust.refuse.set('stop_queue', 'queue not found');
      const pending = service.setRunning(2, false);
      expect(service.get(2)?.enabled).toBeFalse();
      await expectAsync(pending).toBeRejectedWithError('queue not found');
      expect(service.get(2)?.enabled).toBeTrue();
    });

    it('rename shows at once, keeps the engine\'s stored copy, and is undone on refusal', async () => {
      const spec = { name: '  Late night  ', maxConcurrent: 4, schedule: NO_SCHEDULE, onDone: { action: 'sleep' } as const };
      const pending = service.update(2, spec);
      expect(service.get(2)?.name).toBe('Late night');
      expect(service.get(2)?.maxConcurrent).toBe(4);
      await pending;
      expect(rust.commands('update_queue')).toEqual([
        { id: 2, spec: { name: 'Late night', max_concurrent: 4, schedule: NO_SCHEDULE, on_done: { action: 'sleep' } } },
      ]);

      rust.refuse.set('update_queue', 'a queue with this name already exists');
      const before = service.get(2) as UiQueue;
      const refused = service.update(2, { ...spec, name: 'Work' });
      expect(service.get(2)?.name).toBe('Work');
      await expectAsync(refused).toBeRejectedWithError('a queue with this name already exists');
      expect(service.get(2)).toEqual(before);
    });

    it('delete removes the queue at once and brings it back if refused', async () => {
      const pending = service.remove(3);
      expect(names()).toEqual(['Main', 'Night']);
      await pending;
      expect(rust.commands('delete_queue')).toEqual([{ id: 3 }]);

      rust.refuse.set('delete_queue', 'busy');
      const refused = service.remove(2);
      expect(names()).toEqual(['Main']);
      await expectAsync(refused).toBeRejectedWithError('busy');
      expect(names()).toEqual(['Main', 'Night']);
    });

    it('reorder shows the new order at once and restores the old one if refused', async () => {
      const pending = service.reorder([3, 1, 2]);
      expect(names()).toEqual(['Work', 'Main', 'Night']);
      await pending;
      expect(rust.commands('reorder_queues')).toEqual([{ order: [3, 1, 2] }]);

      rust.refuse.set('reorder_queues', 'unknown queue');
      const refused = service.reorder([2, 3, 1]);
      expect(names()).toEqual(['Night', 'Work', 'Main']);
      await expectAsync(refused).toBeRejectedWithError('unknown queue');
      expect(names()).toEqual(['Work', 'Main', 'Night']);
    });

    it('create sends the wire spec, then shows the stored queue', async () => {
      const id = await service.create({ name: 'Weekend', maxConcurrent: 1, schedule: NO_SCHEDULE, onDone: null });
      expect(id).toBe(9);
      expect(rust.commands('create_queue')).toEqual([{ spec: { name: 'Weekend', max_concurrent: 1, schedule: NO_SCHEDULE, on_done: null } }]);
      expect(names()).toContain('Weekend');
    });

    it('asks the engine side to validate and to perform an action', async () => {
      await service.validateAction({ action: 'run_command', command: 'C:\\x.exe' });
      await service.perform(2);
      expect(rust.commands('validate_post_action')).toEqual([{ action: { action: 'run_command', command: 'C:\\x.exe' } }]);
      expect(rust.commands('run_post_action')).toEqual([{ queueId: 2 }]);
    });
  });
});

describe('QueuesService (browser simulation)', () => {
  it('works on the in-memory backend without any Tauri bridge', async () => {
    const backend = new MemoryQueueBackend();
    TestBed.configureTestingModule({
      providers: [
        { provide: TAURI_AVAILABLE, useValue: false },
        { provide: QUEUE_BACKEND, useValue: backend },
      ],
    });
    const service = TestBed.inject(QueuesService);
    await service.ready;
    expect(service.queues().map((q) => q.id)).toEqual([1]);
    const id = await service.create({ ...blankSpec(), name: 'Night' });
    expect(service.queues().map((q) => q.name)).toEqual(['Main', 'Night']);
    await expectAsync(service.create({ ...blankSpec(), name: 'night' })).toBeRejectedWithError('a queue with this name already exists');
    await expectAsync(service.remove(1)).toBeRejected();
    expect(service.get(1)).toBeDefined();
    await service.remove(id);
    expect(service.queues().map((q) => q.id)).toEqual([1]);
  });
});
