import type { Download } from './download';
import { queueMembers, withMovedToQueue, withQueueOrder } from './queue-order';

function dl(id: number, queueId: number, queuePosition: number): Download {
  return {
    id,
    url: `https://x/${id}`,
    filename: `f${id}.bin`,
    dir: 'D:\\',
    size: 10,
    supportsRanges: true,
    state: 'queued',
    category: 'other',
    queueId,
    queuePosition,
    priority: 'normal',
    connections: 4,
    segments: [],
    speedBps: 0,
    speedHistory: [],
    addedAt: 0,
    startedAt: null,
    completedAt: null,
    failure: null,
    checksum: null,
  };
}

describe('queue order helpers', () => {
  const all = [dl(1, 1, 0), dl(2, 1, 1), dl(3, 2, 0), dl(4, 1, 2), dl(5, 2, 1)];
  const positions = (list: readonly Download[], queue: number): [number, number | undefined][] =>
    queueMembers(list, queue).map((d) => [d.id, d.queuePosition]);

  it('lists a queue by position, then id', () => {
    expect(queueMembers(all, 1).map((d) => d.id)).toEqual([1, 2, 4]);
    expect(queueMembers([dl(9, 1, 0), dl(8, 1, 0)], 1).map((d) => d.id)).toEqual([8, 9]);
  });

  it('renumbers a queue so the listed ids come first and the rest follow', () => {
    expect(positions(withQueueOrder(all, 1, [4, 1]), 1)).toEqual([[4, 0], [1, 1], [2, 2]]);
  });

  it('ignores ids outside the queue and repeated ids, and leaves other queues alone', () => {
    const next = withQueueOrder(all, 1, [3, 2, 2]);
    expect(positions(next, 1)).toEqual([[2, 0], [1, 1], [4, 2]]);
    expect(positions(next, 2)).toEqual(positions(all, 2));
  });

  it('appends moved downloads to the end of the target, in the order given', () => {
    const next = withMovedToQueue(all, [4, 1], 2);
    expect(positions(next, 2)).toEqual([[3, 0], [5, 1], [4, 2], [1, 3]]);
    expect(queueMembers(next, 1).map((d) => d.id)).toEqual([2]);
  });

  it('does not move what is already in the target queue', () => {
    expect(withMovedToQueue(all, [3], 2)).toEqual(all);
  });

  it('starts at place 0 in an empty queue', () => {
    expect(positions(withMovedToQueue(all, [2], 7), 7)).toEqual([[2, 0]]);
  });
});
