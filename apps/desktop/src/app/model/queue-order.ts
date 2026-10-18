import type { Download } from './download';

/** Downloads of `queueId` in their manual order (position, then id). */
export function queueMembers(all: readonly Download[], queueId: number): readonly Download[] {
  return all.filter((d) => d.queueId === queueId).sort((a, b) => (a.queuePosition ?? 0) - (b.queuePosition ?? 0) || a.id - b.id);
}

/**
 * `all` with the members of `queueId` renumbered so the listed ids come first, in that order,
 * and the members not listed follow in their current order. Ids outside the queue are ignored.
 * Mirrors what the engine does for `reorder`, so the list can show the result before it answers.
 */
export function withQueueOrder(all: readonly Download[], queueId: number, ids: readonly number[]): readonly Download[] {
  const members = queueMembers(all, queueId);
  const memberIds = new Set(members.map((d) => d.id));
  const listed = ids.filter((id, i) => memberIds.has(id) && ids.indexOf(id) === i);
  const order = [...listed, ...members.map((d) => d.id).filter((id) => !listed.includes(id))];
  const position = new Map(order.map((id, i) => [id, i]));
  return all.map((d) => (d.queueId === queueId && position.has(d.id) ? { ...d, queuePosition: position.get(d.id) } : d));
}

/** `all` with `ids` appended to the end of `queueId`, in the order given. */
export function withMovedToQueue(all: readonly Download[], ids: readonly number[], queueId: number): readonly Download[] {
  let next = Math.max(-1, ...queueMembers(all, queueId).map((d) => d.queuePosition ?? 0)) + 1;
  const moving = new Map<number, number>();
  for (const id of ids) {
    if (all.some((d) => d.id === id && d.queueId !== queueId)) {
      moving.set(id, next++);
    }
  }
  return all.map((d) => (moving.has(d.id) ? { ...d, queueId, queuePosition: moving.get(d.id) } : d));
}
