import type { Queue } from '../tauri/bridge/Queue';
import { actionOf, blankSpec, kindOf, moveBeside, moveBy, moveManyBeside, sameOrder, specOf, toBridgeSpec, toUiQueue } from './queue';
import { UNSCHEDULED } from './schedule';

describe('queue model', () => {
  const wire: Queue = {
    id: 4,
    name: 'Night',
    max_concurrent: 2,
    position: 3,
    enabled: false,
    schedule: { window: { start_min: 1380, stop_min: 420 }, days: 127, start_at: null },
    on_done: { action: 'shutdown' },
  };

  it('maps a wire queue to the UI model and its spec back to the wire', () => {
    const q = toUiQueue(wire);
    expect(q).toEqual({ id: 4, name: 'Night', maxConcurrent: 2, position: 3, enabled: false, schedule: wire.schedule, onDone: { action: 'shutdown' } });
    expect(toBridgeSpec({ ...specOf(q), name: '  Day  ' })).toEqual({
      name: 'Day',
      max_concurrent: 2,
      schedule: wire.schedule,
      on_done: { action: 'shutdown' },
    });
  });

  it('starts a new queue unscheduled, with no action', () => {
    const blank = blankSpec();
    expect(blank.name).toBe('');
    expect(blank.schedule).toEqual(UNSCHEDULED);
    expect(blank.onDone).toBeNull();
  });

  it('maps between the menu choice and the stored action', () => {
    expect(kindOf(null)).toBe('nothing');
    expect(kindOf({ action: 'sleep' })).toBe('sleep');
    expect(kindOf({ action: 'run_command', command: 'C:\\x.exe' })).toBe('run_command');
    expect(actionOf('nothing', 'ignored')).toBeNull();
    expect(actionOf('hibernate', 'ignored')).toEqual({ action: 'hibernate' });
    expect(actionOf('run_command', 'C:\\x.exe')).toEqual({ action: 'run_command', command: 'C:\\x.exe' });
  });

  describe('ordering', () => {
    const ids = [1, 2, 3, 4, 5];

    it('moves one item before or after another', () => {
      expect(moveBeside(ids, 4, 2, false)).toEqual([1, 4, 2, 3, 5]);
      expect(moveBeside(ids, 1, 3, true)).toEqual([2, 3, 1, 4, 5]);
      expect(moveBeside(ids, 5, 1, false)).toEqual([5, 1, 2, 3, 4]);
    });

    it('moves a block and keeps its internal order', () => {
      expect(moveManyBeside(ids, [5, 2], 4, false)).toEqual([1, 3, 2, 5, 4]);
      expect(moveManyBeside(ids, [1, 2], 5, true)).toEqual([3, 4, 5, 1, 2]);
    });

    it('leaves the order alone for a drop on itself, on the block or on an unknown target', () => {
      expect(moveBeside(ids, 3, 3, true)).toBe(ids);
      expect(moveManyBeside(ids, [2, 3], 3, true)).toBe(ids);
      expect(moveBeside(ids, 3, 99, true)).toBe(ids);
      expect(moveBeside(ids, 99, 3, true)).toBe(ids);
    });

    it('moves one place up or down and stops at the ends', () => {
      expect(moveBy(ids, 3, -1)).toEqual([1, 3, 2, 4, 5]);
      expect(moveBy(ids, 3, 1)).toEqual([1, 2, 4, 3, 5]);
      expect(moveBy(ids, 1, -1)).toBe(ids);
      expect(moveBy(ids, 5, 1)).toBe(ids);
      expect(moveBy(ids, 99, 1)).toBe(ids);
    });

    it('compares orders', () => {
      expect(sameOrder([1, 2], [1, 2])).toBeTrue();
      expect(sameOrder([1, 2], [2, 1])).toBeFalse();
      expect(sameOrder([1], [1, 2])).toBeFalse();
    });
  });
});
