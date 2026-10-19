import {
  ALL_DAYS,
  dayCount,
  firstDayOf,
  hasDay,
  inputToLocalDateTime,
  isNightPreset,
  isUnscheduled,
  localDateTimeToInput,
  minutesToTime,
  nightSchedule,
  scheduleIssues,
  timeToMinutes,
  toggleDay,
  toLocalDateTime,
  UNSCHEDULED,
  weekdayOrder,
  wrapsMidnight,
} from './schedule';

describe('schedule helpers', () => {
  it('the night preset is 23:00 to 07:00 on every day and nothing else', () => {
    const night = nightSchedule();
    expect(night.window).toEqual({ start_min: 1380, stop_min: 420 });
    expect(night.days).toBe(ALL_DAYS);
    expect(night.start_at).toBeNull();
    expect(isNightPreset(night)).toBeTrue();
    expect(wrapsMidnight(night.window ?? { start_min: 0, stop_min: 1 })).toBeTrue();
    // changing anything makes it a custom schedule
    expect(isNightPreset({ ...night, days: toggleDay(night.days, 'sat') })).toBeFalse();
    expect(isNightPreset({ ...night, window: { start_min: 1380, stop_min: 480 } })).toBeFalse();
    expect(isNightPreset({ ...night, start_at: 5 })).toBeFalse();
    expect(isNightPreset(UNSCHEDULED)).toBeFalse();
  });

  it('the unscheduled default restricts nothing', () => {
    expect(isUnscheduled(UNSCHEDULED)).toBeTrue();
    expect(isUnscheduled(nightSchedule())).toBeFalse();
    expect(isUnscheduled({ ...UNSCHEDULED, start_at: 1 })).toBeFalse();
  });

  it('toggles days in the Monday-first bit mask the engine uses', () => {
    expect(ALL_DAYS).toBe(0b1111111);
    const noMonday = toggleDay(ALL_DAYS, 'mon');
    expect(noMonday).toBe(0b1111110);
    expect(hasDay(noMonday, 'mon')).toBeFalse();
    expect(hasDay(noMonday, 'sun')).toBeTrue();
    expect(toggleDay(noMonday, 'mon')).toBe(ALL_DAYS);
    expect(toggleDay(0, 'sun')).toBe(0b1000000);
    expect(dayCount(noMonday)).toBe(6);
  });

  it('orders the week from the locale first day (ISO numbers, 1 = Monday)', () => {
    expect(weekdayOrder(1)).toEqual(['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']);
    expect(weekdayOrder(6)).toEqual(['sat', 'sun', 'mon', 'tue', 'wed', 'thu', 'fri']);
    expect(weekdayOrder(7)[0]).toBe('sun');
    expect(weekdayOrder(7)).toHaveSize(7);
    expect(weekdayOrder(0)[0]).toBe('mon');
  });

  it('reads the first day of the week from the locale data, with a sane fallback', () => {
    for (const tag of ['ar', 'ar-SA', 'en-US', 'en-GB', 'xx-not-a-locale']) {
      const day = firstDayOf(tag);
      expect(day).toBeGreaterThanOrEqual(1);
      expect(day).toBeLessThanOrEqual(7);
    }
    expect(firstDayOf('en-GB')).toBe(1);
  });

  it('converts times of day both ways and rejects junk', () => {
    expect(minutesToTime(0)).toBe('00:00');
    expect(minutesToTime(425)).toBe('07:05');
    expect(minutesToTime(1380)).toBe('23:00');
    expect(minutesToTime(99999)).toBe('23:59');
    expect(timeToMinutes('07:05')).toBe(425);
    expect(timeToMinutes('23:00:30')).toBe(1380);
    expect(timeToMinutes('24:00')).toBeNull();
    expect(timeToMinutes('7:5')).toBeNull();
    expect(timeToMinutes('')).toBeNull();
  });

  it('converts datetime-local values to engine local seconds and back', () => {
    const at = inputToLocalDateTime('2026-10-20T08:30');
    expect(at).toBe(Date.UTC(2026, 9, 20, 8, 30) / 1000);
    expect(localDateTimeToInput(at ?? 0)).toBe('2026-10-20T08:30');
    expect(inputToLocalDateTime('')).toBeNull();
    expect(inputToLocalDateTime('2026-02-30T08:30')).toBeNull();
    expect(inputToLocalDateTime('2026-10-20T25:00')).toBeNull();
  });

  it('turns epoch milliseconds into the local wall clock the engine compares against', () => {
    const ms = Date.UTC(2026, 9, 20, 12, 0);
    const local = toLocalDateTime(ms);
    expect(local).toBe(Math.floor(ms / 1000) - new Date(ms).getTimezoneOffset() * 60);
    // the engine and this agree on what "08:30 today" means in the same frame
    const wall = new Date(ms);
    const sameWall = Date.UTC(wall.getFullYear(), wall.getMonth(), wall.getDate(), wall.getHours(), wall.getMinutes()) / 1000;
    expect(local).toBe(sameWall);
  });

  describe('validation (mirrors Schedule::validate)', () => {
    const now = 1_800_000_000;

    it('accepts the default and the night preset', () => {
      expect(scheduleIssues(UNSCHEDULED, now)).toEqual([]);
      expect(scheduleIssues(nightSchedule(), now)).toEqual([]);
    });

    it('rejects a window that starts and stops at the same minute', () => {
      expect(scheduleIssues({ ...UNSCHEDULED, window: { start_min: 600, stop_min: 600 } }, now)).toEqual(['sameTime']);
    });

    it('rejects an empty set of days', () => {
      expect(scheduleIssues({ ...UNSCHEDULED, days: 0 }, now)).toEqual(['noDays']);
    });

    it('rejects a start time that has already passed, accepts one ahead', () => {
      expect(scheduleIssues({ ...UNSCHEDULED, start_at: now - 60 }, now)).toEqual(['startInPast']);
      expect(scheduleIssues({ ...UNSCHEDULED, start_at: now }, now)).toEqual(['startInPast']);
      expect(scheduleIssues({ ...UNSCHEDULED, start_at: now + 60 }, now)).toEqual([]);
    });

    it('reports every problem at once', () => {
      const bad = { window: { start_min: 5, stop_min: 5 }, days: 0, start_at: 1 };
      expect(scheduleIssues(bad, now)).toEqual(['sameTime', 'noDays', 'startInPast']);
    });
  });
});
