/**
 * Pure helpers for queue schedules: the editor, the validation that mirrors
 * `riptide_core::Schedule::validate`, and the conversions between the editor's inputs and the
 * engine's wire values. No Angular, no clock: everything that needs "now" takes it as an argument.
 */

import type { Schedule } from '../tauri/bridge/Schedule';
import type { Weekday } from '../tauri/bridge/Weekday';
import type { Window } from '../tauri/bridge/Window';

/** Minutes in a day; window times are `0..1440`. */
export const MINUTES_PER_DAY = 1440;

/** Every weekday, Monday first (bit 0 = Monday ... bit 6 = Sunday, like `DaySet`). */
export const WEEKDAYS: readonly Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];

/** All seven days selected. */
export const ALL_DAYS = 0b111_1111;

/** The "Only at night" preset: 23:00 to 07:00, every day. */
export const NIGHT_WINDOW: Window = { start_min: 23 * 60, stop_min: 7 * 60 };

/** A schedule that never restricts anything (the engine's default). */
export const UNSCHEDULED: Schedule = { window: null, days: ALL_DAYS, start_at: null };

/** What the editor can complain about; each maps to a translated message. */
export type ScheduleIssue = 'sameTime' | 'noDays' | 'startInPast';

/** Whether `days` contains `day`. */
export function hasDay(days: number, day: Weekday): boolean {
  return (days & (1 << WEEKDAYS.indexOf(day))) !== 0;
}

/** `days` with `day` switched on or off. */
export function toggleDay(days: number, day: Weekday): number {
  return (days ^ (1 << WEEKDAYS.indexOf(day))) & ALL_DAYS;
}

/** How many days are selected. */
export function dayCount(days: number): number {
  return WEEKDAYS.filter((d) => hasDay(days, d)).length;
}

/**
 * The weekdays in display order for a week that starts on `firstDay` (ISO numbering: 1 =
 * Monday ... 7 = Sunday, which is what `Intl.Locale.getWeekInfo()` reports).
 */
export function weekdayOrder(firstDay: number): readonly Weekday[] {
  const start = ((((Math.trunc(firstDay) || 1) - 1) % 7) + 7) % 7;
  return [...WEEKDAYS.slice(start), ...WEEKDAYS.slice(0, start)];
}

interface WeekInfoLocale {
  getWeekInfo?: () => { firstDay: number };
  weekInfo?: { firstDay: number };
}

/**
 * The first day of the week for a language tag: what the platform's locale data says
 * (`Intl.Locale` week info), else Saturday for Arabic and Monday otherwise.
 */
export function firstDayOf(tag: string): number {
  try {
    const locale = new Intl.Locale(tag).maximize() as unknown as WeekInfoLocale;
    const info = typeof locale.getWeekInfo === 'function' ? locale.getWeekInfo() : locale.weekInfo;
    if (info !== undefined && info.firstDay >= 1 && info.firstDay <= 7) {
      return info.firstDay;
    }
  } catch {
    // an engine without week info: fall through to the table below
  }
  return tag.toLowerCase().startsWith('ar') ? 6 : 1;
}

/** `07:05` for 425; clamped into the day. */
export function minutesToTime(minutes: number): string {
  const m = Math.min(MINUTES_PER_DAY - 1, Math.max(0, Math.trunc(minutes)));
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

/** Minutes after midnight for an `<input type="time">` value; `null` for anything else. */
export function timeToMinutes(value: string): number | null {
  const match = /^(\d{2}):(\d{2})(?::\d{2})?$/.exec(value);
  if (match === null) {
    return null;
  }
  const h = Number(match[1]);
  const m = Number(match[2]);
  return h < 24 && m < 60 ? h * 60 + m : null;
}

/** Whether the daily window runs past midnight (23:00 to 07:00). */
export function wrapsMidnight(window: Window): boolean {
  return window.start_min > window.stop_min;
}

/** The "Only at night" preset as a schedule. */
export function nightSchedule(): Schedule {
  return { window: { ...NIGHT_WINDOW }, days: ALL_DAYS, start_at: null };
}

/** Whether `s` is exactly the "Only at night" preset (every day, 23:00 to 07:00, no one-shot). */
export function isNightPreset(s: Schedule): boolean {
  return (
    s.window !== null &&
    s.window.start_min === NIGHT_WINDOW.start_min &&
    s.window.stop_min === NIGHT_WINDOW.stop_min &&
    s.days === ALL_DAYS &&
    s.start_at === null
  );
}

/** Whether the schedule restricts nothing. */
export function isUnscheduled(s: Schedule): boolean {
  return s.window === null && s.days === ALL_DAYS && s.start_at === null;
}

/**
 * `LocalDateTime` (wall-clock seconds as if the local zone were UTC) for a `datetime-local`
 * input value such as `2026-10-20T08:30`; `null` when it is not a real date and time.
 */
export function inputToLocalDateTime(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(value);
  if (match === null) {
    return null;
  }
  const [y, mo, d, h, mi] = match.slice(1, 6).map(Number);
  const s = Number(match[6] ?? 0);
  const ms = Date.UTC(y, mo - 1, d, h, mi, s);
  const check = new Date(ms);
  const real = check.getUTCFullYear() === y && check.getUTCMonth() === mo - 1 && check.getUTCDate() === d && h < 24 && mi < 60;
  return real ? Math.floor(ms / 1000) : null;
}

/** The `datetime-local` value (minute precision) for a `LocalDateTime`. */
export function localDateTimeToInput(local: number): string {
  return new Date(local * 1000).toISOString().slice(0, 16);
}

/** "Now" as a `LocalDateTime`, from epoch milliseconds and the zone offset at that moment. */
export function toLocalDateTime(epochMs: number): number {
  return Math.floor(epochMs / 1000) - new Date(epochMs).getTimezoneOffset() * 60;
}

/** Everything wrong with `s` at `nowLocal` (a `LocalDateTime`); empty when it can be saved. */
export function scheduleIssues(s: Schedule, nowLocal: number): readonly ScheduleIssue[] {
  const issues: ScheduleIssue[] = [];
  if (s.window !== null && s.window.start_min === s.window.stop_min) {
    issues.push('sameTime');
  }
  if ((s.days & ALL_DAYS) === 0) {
    issues.push('noDays');
  }
  if (s.start_at !== null && s.start_at <= nowLocal) {
    issues.push('startInPast');
  }
  return issues;
}
