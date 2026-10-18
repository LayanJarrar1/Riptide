import { computed, DestroyRef, inject, Injectable, signal } from '@angular/core';
import { Subject } from 'rxjs';

import { QueueFinished, UiQueue, UiQueueSpec } from '../model/queue';
import type { Event as EngineEvent } from '../tauri/bridge/Event';
import type { PostAction } from '../tauri/bridge/PostAction';
import { TAURI_AVAILABLE, TAURI_LISTEN } from '../tauri/tauri-bridge';
import { QUEUE_BACKEND } from './queue-backend';
import { ENGINE_EVENT } from './tauri-engine.service';

/**
 * The queues, as signals. The list is loaded once and kept current from the engine's queue
 * events; the user's own changes are applied at once (optimistic) and rolled back to the
 * engine's truth when the engine refuses them.
 *
 * - `queue_created`, `queue_updated`, `queues_reordered`: re-read the list (answers of an older
 *   read never overwrite a newer one).
 * - `queue_deleted`: drop the queue from the list.
 * - `queue_finished`: announced on {@link QueuesService.finished} (the countdown listens).
 *
 * Item-level events (`download_moved`, `queue_order_changed`) belong to the download engine.
 */
@Injectable({ providedIn: 'root' })
export class QueuesService {
  private readonly backend = inject(QUEUE_BACKEND);
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly listen = inject(TAURI_LISTEN);
  private readonly list = signal<readonly UiQueue[]>([]);
  private reads = 0;
  private unlisten: (() => void) | null = null;

  /** Queues in display order. */
  readonly queues = computed(() => [...this.list()].sort((a, b) => a.position - b.position || a.id - b.id));

  /** `true` once the first read has come back (successfully or not). */
  readonly loaded = signal(false);

  /** A queue ran out of work; `action` is what it was set to do next. */
  readonly finished = new Subject<QueueFinished>();

  /** Resolves when the first read and the event subscription are done; never rejects. */
  readonly ready: Promise<void>;

  constructor() {
    this.ready = this.start();
    inject(DestroyRef).onDestroy(() => {
      this.unlisten?.();
      this.unlisten = null;
      this.finished.complete();
    });
  }

  /** One queue, if it exists. */
  get(id: number): UiQueue | undefined {
    return this.list().find((q) => q.id === id);
  }

  /** Creates a queue and resolves with its id; rejects with the engine's reason (a taken name). */
  async create(spec: UiQueueSpec): Promise<number> {
    const id = await this.backend.create(spec);
    await this.reload();
    return id;
  }

  /** Saves name, limit, schedule and post action; shown at once, restored if refused. */
  async update(id: number, spec: UiQueueSpec): Promise<void> {
    const before = this.list();
    this.patch(id, (q) => ({ ...q, ...spec, name: spec.name.trim() }));
    try {
      const stored = await this.backend.update(id, spec);
      this.patch(id, () => stored);
    } catch (error: unknown) {
      this.list.set(before);
      throw error;
    }
  }

  /** Starts (`true`) or stops (`false`) a queue; shown at once, restored if refused. */
  async setRunning(id: number, running: boolean): Promise<void> {
    const before = this.list();
    this.patch(id, (q) => ({ ...q, enabled: running }));
    try {
      await (running ? this.backend.start(id) : this.backend.stop(id));
    } catch (error: unknown) {
      this.list.set(before);
      throw error;
    }
  }

  /** Deletes a queue (its downloads go to Main); the queue leaves the list at once. */
  async remove(id: number): Promise<void> {
    const before = this.list();
    this.list.update((all) => all.filter((q) => q.id !== id));
    try {
      await this.backend.remove(id);
    } catch (error: unknown) {
      this.list.set(before);
      throw error;
    }
  }

  /** Puts the queues in this order; shown at once, restored if refused. */
  async reorder(ids: readonly number[]): Promise<void> {
    const before = this.list();
    const position = new Map(ids.map((id, i) => [id, i]));
    this.list.update((all) => all.map((q) => (position.has(q.id) ? { ...q, position: position.get(q.id) ?? q.position } : q)));
    try {
      await this.backend.reorder(ids);
    } catch (error: unknown) {
      this.list.set(before);
      throw error;
    }
  }

  /** Asks the engine side whether the action could run (rejects with the reason). */
  validateAction(action: PostAction): Promise<void> {
    return this.backend.validate(action);
  }

  /** Performs the action saved on the queue; call it once the countdown has ended. */
  perform(queueId: number): Promise<void> {
    return this.backend.perform(queueId);
  }

  /** Re-reads the list from the engine. */
  async reload(): Promise<void> {
    const seq = ++this.reads;
    try {
      const fresh = await this.backend.list();
      if (seq === this.reads) {
        this.list.set(fresh);
      }
    } catch (error: unknown) {
      console.warn('[queues] list_queues failed', error);
    }
  }

  private async start(): Promise<void> {
    if (this.tauri) {
      try {
        this.unlisten = await this.listen<EngineEvent>(ENGINE_EVENT, (e) => this.onEvent(e));
      } catch (error: unknown) {
        console.warn('[queues] could not subscribe to events', error);
      }
    }
    await this.reload();
    this.loaded.set(true);
  }

  private onEvent(e: EngineEvent): void {
    switch (e.type) {
      case 'queue_created':
      case 'queue_updated':
      case 'queues_reordered':
        void this.reload();
        break;
      case 'queue_deleted':
        this.list.update((all) => all.filter((q) => q.id !== e.queue_id));
        break;
      case 'queue_finished':
        this.finished.next({ queueId: e.queue_id, action: e.action });
        break;
    }
  }

  private patch(id: number, fn: (q: UiQueue) => UiQueue): void {
    this.list.update((all) => all.map((q) => (q.id === id ? fn(q) : q)));
  }
}
