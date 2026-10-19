/**
 * UI-side model of a queue. Mirrors `riptide_core::Queue` / `QueueSpec` (camelCase here,
 * snake_case on the wire); `toUiQueue` / `toBridgeSpec` convert. Also the pure ordering helpers
 * the drag-and-drop and keyboard reordering share.
 */

import type { PostAction } from '../tauri/bridge/PostAction';
import type { Queue } from '../tauri/bridge/Queue';
import type { QueueSpec } from '../tauri/bridge/QueueSpec';
import type { Schedule } from '../tauri/bridge/Schedule';
import { UNSCHEDULED } from './schedule';

/** Id of the built-in *Main* queue (cannot be renamed or deleted). */
export const MAIN_QUEUE_ID = 1;

/** Most downloads one queue may run at once (`riptide_core::MAX_QUEUE_CONCURRENT`). */
export const MAX_QUEUE_CONCURRENT = 32;

/** Longest queue name in characters (`riptide_core::MAX_QUEUE_NAME_CHARS`). */
export const MAX_QUEUE_NAME_CHARS = 80;

/** The post-queue action menu: `nothing` or one of the engine's actions. */
export type PostActionKind = 'nothing' | PostAction['action'];

/** Choices of the post-queue action menu, in display order. */
export const POST_ACTION_KINDS: readonly PostActionKind[] = ['nothing', 'exit', 'sleep', 'hibernate', 'shutdown', 'run_command'];

/** One queue as the UI sees it. */
export interface UiQueue {
  readonly id: number;
  readonly name: string;
  readonly maxConcurrent: number;
  readonly position: number;
  /** Started (`true`) or stopped (`false`). */
  readonly enabled: boolean;
  readonly schedule: Schedule;
  readonly onDone: PostAction | null;
}

/** The editable part of a queue. */
export interface UiQueueSpec {
  readonly name: string;
  readonly maxConcurrent: number;
  readonly schedule: Schedule;
  readonly onDone: PostAction | null;
}

/** A queue that drained, with the action it announced. */
export interface QueueFinished {
  readonly queueId: number;
  readonly action: PostAction | null;
}

/** Wire queue → UI queue. */
export function toUiQueue(q: Queue): UiQueue {
  return {
    id: q.id,
    name: q.name,
    maxConcurrent: q.max_concurrent,
    position: q.position,
    enabled: q.enabled,
    schedule: q.schedule,
    onDone: q.on_done,
  };
}

/** UI spec → wire spec. */
export function toBridgeSpec(spec: UiQueueSpec): QueueSpec {
  return { name: spec.name.trim(), max_concurrent: spec.maxConcurrent, schedule: spec.schedule, on_done: spec.onDone };
}

/** The editable part of a queue. */
export function specOf(q: UiQueue): UiQueueSpec {
  return { name: q.name, maxConcurrent: q.maxConcurrent, schedule: q.schedule, onDone: q.onDone };
}

/** A blank spec for "New queue". */
export function blankSpec(): UiQueueSpec {
  return { name: '', maxConcurrent: 2, schedule: UNSCHEDULED, onDone: null };
}

/** The menu choice for a stored action. */
export function kindOf(action: PostAction | null): PostActionKind {
  return action === null ? 'nothing' : action.action;
}

/** The stored action for a menu choice; `command` only matters for `run_command`. */
export function actionOf(kind: PostActionKind, command: string): PostAction | null {
  switch (kind) {
    case 'nothing':
      return null;
    case 'run_command':
      return { action: 'run_command', command };
    default:
      return { action: kind };
  }
}

/**
 * `ids` with the items of `moving` taken out and put, in their current relative order, just
 * before (`after = false`) or after `targetId`. Unchanged when `targetId` is itself moving or
 * when nothing in `moving` is in `ids`.
 */
export function moveManyBeside<T>(ids: readonly T[], moving: readonly T[], targetId: T, after: boolean): readonly T[] {
  const block = ids.filter((x) => moving.includes(x));
  if (block.length === 0 || moving.includes(targetId) || !ids.includes(targetId)) {
    return ids;
  }
  const rest = ids.filter((x) => !moving.includes(x));
  const at = rest.indexOf(targetId) + (after ? 1 : 0);
  return [...rest.slice(0, at), ...block, ...rest.slice(at)];
}

/** `ids` with `id` moved to sit just before (`after = false`) or after `targetId`. */
export function moveBeside<T>(ids: readonly T[], id: T, targetId: T, after: boolean): readonly T[] {
  return moveManyBeside(ids, [id], targetId, after);
}

/** `ids` with `id` moved one place up (`-1`) or down (`1`); unchanged at the ends. */
export function moveBy<T>(ids: readonly T[], id: T, delta: 1 | -1): readonly T[] {
  const i = ids.indexOf(id);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= ids.length) {
    return ids;
  }
  const next = [...ids];
  [next[i], next[j]] = [next[j], next[i]];
  return next;
}

/** Whether two lists hold the same items in the same order. */
export function sameOrder<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((x, i) => x === b[i]);
}
