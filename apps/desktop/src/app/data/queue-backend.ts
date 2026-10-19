import { inject, InjectionToken } from '@angular/core';

import { MAIN_QUEUE_ID, toBridgeSpec, toUiQueue, UiQueue, UiQueueSpec } from '../model/queue';
import type { PostAction } from '../tauri/bridge/PostAction';
import type { Queue } from '../tauri/bridge/Queue';
import { InvokeFn, TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';

/**
 * What the queue service needs from the engine side. `TauriQueueBackend` talks to the Rust
 * commands in `src-tauri/src/queues.rs`; `MemoryQueueBackend` is the plain-browser simulation
 * (`ng serve`, Karma). Every method resolves when the engine accepted the change.
 */
export interface QueueBackend {
  /** Every queue. */
  list(): Promise<readonly UiQueue[]>;
  /** Creates a queue; resolves with its id. */
  create(spec: UiQueueSpec): Promise<number>;
  /** Replaces the editable part of a queue; resolves with the stored queue. */
  update(id: number, spec: UiQueueSpec): Promise<UiQueue>;
  /** Deletes a queue (its downloads move to Main). */
  remove(id: number): Promise<void>;
  /** Starts a queue. */
  start(id: number): Promise<void>;
  /** Stops a queue. */
  stop(id: number): Promise<void>;
  /** Puts the queues in this order. */
  reorder(ids: readonly number[]): Promise<void>;
  /** Rejects with the reason when the action could not run (a missing program). */
  validate(action: PostAction): Promise<void>;
  /** Performs the action saved on the queue (after the countdown). */
  perform(queueId: number): Promise<void>;
}

/** The backend for this page: the real engine inside Tauri, the simulation elsewhere. */
export const QUEUE_BACKEND = new InjectionToken<QueueBackend>('riptide.queue.backend', {
  providedIn: 'root',
  factory: () => (inject(TAURI_AVAILABLE) ? new TauriQueueBackend(inject(TAURI_INVOKE)) : new MemoryQueueBackend()),
});

/** The real thing: one Tauri command per method. */
export class TauriQueueBackend implements QueueBackend {
  constructor(private readonly invoke: InvokeFn) {}

  async list(): Promise<readonly UiQueue[]> {
    return (await this.invoke<Queue[]>('list_queues')).map(toUiQueue);
  }

  create(spec: UiQueueSpec): Promise<number> {
    return this.invoke<number>('create_queue', { spec: toBridgeSpec(spec) });
  }

  async update(id: number, spec: UiQueueSpec): Promise<UiQueue> {
    return toUiQueue(await this.invoke<Queue>('update_queue', { id, spec: toBridgeSpec(spec) }));
  }

  remove(id: number): Promise<void> {
    return this.invoke<void>('delete_queue', { id });
  }

  start(id: number): Promise<void> {
    return this.invoke<void>('start_queue', { id });
  }

  stop(id: number): Promise<void> {
    return this.invoke<void>('stop_queue', { id });
  }

  reorder(ids: readonly number[]): Promise<void> {
    return this.invoke<void>('reorder_queues', { order: ids });
  }

  validate(action: PostAction): Promise<void> {
    return this.invoke<void>('validate_post_action', { action });
  }

  perform(queueId: number): Promise<void> {
    return this.invoke<void>('run_post_action', { queueId });
  }
}

/** In-memory queues for the browser build and the specs; never performs a post action. */
export class MemoryQueueBackend implements QueueBackend {
  private queues: UiQueue[] = [
    { id: MAIN_QUEUE_ID, name: 'Main', maxConcurrent: 3, position: 0, enabled: true, schedule: { window: null, days: 0b111_1111, start_at: null }, onDone: null },
  ];
  private nextId = MAIN_QUEUE_ID + 1;
  /** Post actions that were "performed" (queue ids), for specs. */
  readonly performed: number[] = [];

  list(): Promise<readonly UiQueue[]> {
    return Promise.resolve([...this.queues]);
  }

  create(spec: UiQueueSpec): Promise<number> {
    this.checkName(spec.name, null);
    const id = this.nextId++;
    this.queues.push({ id, position: this.queues.length, enabled: true, ...spec, name: spec.name.trim() });
    return Promise.resolve(id);
  }

  update(id: number, spec: UiQueueSpec): Promise<UiQueue> {
    this.checkName(spec.name, id);
    const old = this.find(id);
    const next: UiQueue = { ...old, ...spec, name: id === MAIN_QUEUE_ID ? old.name : spec.name.trim() };
    this.queues = this.queues.map((q) => (q.id === id ? next : q));
    return Promise.resolve(next);
  }

  remove(id: number): Promise<void> {
    if (id === MAIN_QUEUE_ID) {
      return Promise.reject(new Error('the Main queue cannot be deleted'));
    }
    this.find(id);
    this.queues = this.queues.filter((q) => q.id !== id).map((q, i) => ({ ...q, position: i }));
    return Promise.resolve();
  }

  start(id: number): Promise<void> {
    return this.setEnabled(id, true);
  }

  stop(id: number): Promise<void> {
    return this.setEnabled(id, false);
  }

  reorder(ids: readonly number[]): Promise<void> {
    const byId = new Map(this.queues.map((q) => [q.id, q]));
    const listed = ids.flatMap((id) => byId.get(id) ?? []);
    const rest = this.queues.filter((q) => !ids.includes(q.id));
    this.queues = [...listed, ...rest].map((q, i) => ({ ...q, position: i }));
    return Promise.resolve();
  }

  validate(action: PostAction): Promise<void> {
    return action.action === 'run_command' && action.command.trim() === '' ? Promise.reject(new Error('no program is set')) : Promise.resolve();
  }

  perform(queueId: number): Promise<void> {
    this.performed.push(queueId);
    return Promise.resolve();
  }

  private setEnabled(id: number, enabled: boolean): Promise<void> {
    this.find(id);
    this.queues = this.queues.map((q) => (q.id === id ? { ...q, enabled } : q));
    return Promise.resolve();
  }

  private find(id: number): UiQueue {
    const q = this.queues.find((x) => x.id === id);
    if (q === undefined) {
      throw new Error('queue not found');
    }
    return q;
  }

  private checkName(name: string, except: number | null): void {
    const clean = name.trim().toLowerCase();
    if (clean.length === 0) {
      throw new Error('queue name is empty');
    }
    if (this.queues.some((q) => q.id !== except && q.name.toLowerCase() === clean)) {
      throw new Error('a queue with this name already exists');
    }
  }
}
