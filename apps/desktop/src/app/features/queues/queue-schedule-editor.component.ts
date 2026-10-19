import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';

import { NOW } from '../../core/clock';
import { NAVIGATOR_LANGUAGES } from '../../core/preferences';
import { I18nService } from '../../i18n/i18n.service';
import {
  ALL_DAYS,
  firstDayOf,
  hasDay,
  inputToLocalDateTime,
  isNightPreset,
  localDateTimeToInput,
  minutesToTime,
  nightSchedule,
  NIGHT_WINDOW,
  scheduleIssues,
  timeToMinutes,
  toggleDay,
  toLocalDateTime,
  UNSCHEDULED,
  weekdayOrder,
  wrapsMidnight,
} from '../../model/schedule';
import type { Schedule } from '../../tauri/bridge/Schedule';
import type { Weekday } from '../../tauri/bridge/Weekday';
import { IconComponent } from '../../ui/icon.component';
import { TimeFieldComponent } from '../../ui/time-field.component';

/** One day button: its position in the week (0 = Monday, for the reference date) and labels. */
interface DayButton {
  readonly day: Weekday;
  readonly short: string;
  readonly long: string;
  readonly on: boolean;
}

/** A fresh "start at" begins at the next full hour at least this far ahead. */
const DEFAULT_START_DELAY_SECS = 3600;

/**
 * The schedule of a queue: an optional daily window (start and stop time, with the "Only at
 * night" preset), the days it applies to (first day of the week and the day names follow the
 * language), and an optional one-shot "start at" date and time. It edits a `Schedule` value
 * (controlled: `value` in, `valueChange` out) and shows what is wrong with it; the parent also
 * checks `scheduleIssues` before it lets the user save.
 *
 * A sleeping or switched-off PC is not woken (the engine has no wake timer): the note under the
 * editor says so.
 */
@Component({
  selector: 'app-queue-schedule-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, TimeFieldComponent],
  template: `
    <fieldset class="editor">
      <legend class="legend">{{ i18n.t('schedule.title') }}</legend>

      <div class="presets" role="group" [attr.aria-label]="i18n.t('schedule.title')">
        <button type="button" class="chip" [class.is-active]="night()" [attr.aria-pressed]="night()" (click)="applyNight()">
          <app-icon name="moon" [size]="14" />
          {{ i18n.t('schedule.presetNight') }}
        </button>
        <button type="button" class="chip" [class.is-active]="unrestricted()" [attr.aria-pressed]="unrestricted()" (click)="applyAnytime()">
          {{ i18n.t('schedule.presetAnytime') }}
        </button>
      </div>

      <label class="check">
        <input type="checkbox" name="restrict" [checked]="value().window !== null" (change)="setRestricted($event)" />
        <span>{{ i18n.t('schedule.restrict') }}</span>
      </label>

      @if (value().window; as window) {
        <div class="times">
          <app-time-field name="from" [label]="i18n.t('schedule.from')" [value]="time(window.start_min)" (valueChange)="setTime('start_min', $event)" />
          <app-time-field name="to" [label]="i18n.t('schedule.to')" [value]="time(window.stop_min)" (valueChange)="setTime('stop_min', $event)" />
        </div>
        @if (wraps()) {
          <p class="hint"><app-icon name="moon" [size]="13" /> {{ i18n.t('schedule.wraps') }}</p>
        }
      }

      <div class="days-block">
        <div class="days-head">
          <span class="label" id="schedule-days-label">{{ i18n.t('schedule.days') }}</span>
          <button type="button" class="link" [disabled]="value().days === allDays" (click)="setDays(allDays)">{{ i18n.t('schedule.everyDay') }}</button>
        </div>
        <div class="days" role="group" aria-labelledby="schedule-days-label">
          @for (d of days(); track d.day) {
            <button type="button" class="day" [class.is-on]="d.on" [attr.aria-pressed]="d.on" [attr.aria-label]="d.long" [attr.title]="d.long" (click)="toggle(d.day)">
              {{ d.short }}
            </button>
          }
        </div>
      </div>

      <label class="check">
        <input type="checkbox" name="startAtOn" [checked]="oneShot()" (change)="setOneShot($event)" />
        <span>{{ i18n.t('schedule.startAt') }}</span>
      </label>
      @if (oneShot()) {
        <div class="once">
          <div class="once-row">
            <input class="input" type="date" name="startAtDate" dir="ltr" [attr.aria-label]="i18n.t('schedule.startAtDate')" [value]="startAtDate()" (change)="setStartAtDate($event)" />
            <app-time-field name="startAtTime" [label]="i18n.t('schedule.startAtTime')" [value]="startAtTime()" (valueChange)="setStartAtTime($event)" />
          </div>
          <p class="hint">{{ i18n.t('schedule.startAtHint') }}</p>
        </div>
      }

      @for (issue of issues(); track issue) {
        <p class="issue" role="alert"><app-icon name="alert" [size]="14" /> {{ i18n.t('schedule.issue.' + issue) }}</p>
      }

      <p class="note"><app-icon name="info" [size]="14" /> <span>{{ i18n.t('schedule.sleepNote') }}</span></p>
    </fieldset>
  `,
  styles: `
    .editor {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      min-inline-size: 0;
      margin: 0;
      padding: var(--space-4);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
    }

    .legend {
      padding-inline: var(--space-2);
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .presets {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
    }

    .chip {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      block-size: 30px;
      padding-inline: var(--space-3);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-full);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-sm);
      cursor: pointer;
      transition:
        background-color var(--motion-fast) var(--ease-standard),
        color var(--motion-fast) var(--ease-standard);
    }

    .chip:hover {
      color: var(--color-text);
      background: var(--color-surface-hover);
    }

    .chip.is-active {
      border-color: var(--color-accent);
      background: var(--color-accent-soft);
      color: var(--color-text);
    }

    .check {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-sm);
      cursor: pointer;
    }

    .check input {
      accent-color: var(--color-accent);
    }

    .times {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-3);
    }

    .once-row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      gap: var(--space-3);
      align-items: end;
    }

    .label {
      color: var(--color-text-secondary);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .input {
      inline-size: 100%;
      block-size: 36px;
      padding-inline: 12px;
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text);
      color-scheme: inherit;
      font: inherit;
      font-size: var(--text-sm);
    }

    .input:focus {
      outline: none;
      border-color: var(--color-accent);
      box-shadow: 0 0 0 3px var(--color-accent-ring);
    }

    .days-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-block-end: 6px;
    }

    .link {
      padding: 0;
      border: 0;
      background: none;
      color: var(--color-accent-fg);
      font: inherit;
      font-size: var(--text-xs);
      cursor: pointer;
    }

    .link:disabled {
      color: var(--color-text-muted);
      cursor: default;
    }

    .days {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1);
    }

    .day {
      flex: 1 1 52px;
      block-size: 34px;
      padding-inline: var(--space-2);
      border: 1px solid var(--color-border-strong);
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--color-text-secondary);
      font: inherit;
      font-size: var(--text-sm);
      cursor: pointer;
      transition:
        background-color var(--motion-fast) var(--ease-standard),
        color var(--motion-fast) var(--ease-standard);
    }

    .day:hover {
      color: var(--color-text);
    }

    .day.is-on {
      border-color: var(--color-accent);
      background: var(--color-accent-soft);
      color: var(--color-text);
      font-weight: var(--weight-semibold);
    }

    .once {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .hint,
    .note,
    .issue {
      display: flex;
      align-items: flex-start;
      gap: 6px;
      margin: 0;
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .hint {
      color: var(--color-text-muted);
    }

    .note {
      padding: var(--space-3);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      color: var(--color-text-secondary);
    }

    .note app-icon,
    .hint app-icon,
    .issue app-icon {
      flex: none;
      margin-block-start: 1px;
    }

    .issue {
      color: var(--color-danger);
    }
  `,
})
export class QueueScheduleEditorComponent {
  protected readonly i18n = inject(I18nService);
  private readonly now = inject(NOW);
  private readonly languages = inject(NAVIGATOR_LANGUAGES);

  /** The schedule being edited. */
  readonly value = input.required<Schedule>();

  /** The user changed something; the parent stores the new schedule and passes it back in. */
  readonly valueChange = output<Schedule>();

  protected readonly allDays = ALL_DAYS;
  protected readonly time = minutesToTime;

  /** The one-shot is on exactly when a start time is set (ticking the box sets a default one). */
  protected readonly oneShot = computed(() => this.value().start_at !== null);

  protected readonly night = computed(() => isNightPreset(this.value()));
  protected readonly unrestricted = computed(() => {
    const s = this.value();
    return s.window === null && s.days === ALL_DAYS && s.start_at === null;
  });
  protected readonly wraps = computed(() => {
    const w = this.value().window;
    return w !== null && wrapsMidnight(w);
  });
  protected readonly startAtDate = computed(() => {
    const at = this.value().start_at;
    return at === null ? '' : localDateTimeToInput(at).slice(0, 10);
  });
  protected readonly startAtTime = computed(() => {
    const at = this.value().start_at;
    return at === null ? '00:00' : localDateTimeToInput(at).slice(11, 16);
  });
  protected readonly issues = computed(() => scheduleIssues(this.value(), toLocalDateTime(this.now())));

  /** Day buttons in the week order of the active language. */
  protected readonly days = computed<readonly DayButton[]>(() => {
    const lang = this.i18n.lang();
    const tag = this.languages.find((t) => t.toLowerCase().startsWith(lang)) ?? lang;
    const short = new Intl.DateTimeFormat(tag, { weekday: 'short', timeZone: 'UTC' });
    const long = new Intl.DateTimeFormat(tag, { weekday: 'long', timeZone: 'UTC' });
    const mask = this.value().days;
    return weekdayOrder(firstDayOf(tag)).map((day) => {
      // 2024-01-01 was a Monday
      const index = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].indexOf(day);
      const date = new Date(Date.UTC(2024, 0, 1 + index));
      return { day, short: short.format(date), long: long.format(date), on: hasDay(mask, day) };
    });
  });

  protected applyNight(): void {
    this.valueChange.emit(nightSchedule());
  }

  protected applyAnytime(): void {
    this.valueChange.emit({ ...UNSCHEDULED });
  }

  protected setRestricted(event: Event): void {
    const on = (event.target as HTMLInputElement).checked;
    this.valueChange.emit({ ...this.value(), window: on ? { ...NIGHT_WINDOW } : null });
  }

  protected setTime(which: 'start_min' | 'stop_min', value: string): void {
    const window = this.value().window;
    const minutes = timeToMinutes(value);
    if (window !== null && minutes !== null) {
      this.valueChange.emit({ ...this.value(), window: { ...window, [which]: minutes } });
    }
  }

  protected toggle(day: Weekday): void {
    this.setDays(toggleDay(this.value().days, day));
  }

  protected setDays(days: number): void {
    this.valueChange.emit({ ...this.value(), days });
  }

  protected setOneShot(event: Event): void {
    const on = (event.target as HTMLInputElement).checked;
    if (on) {
      const hour = Math.ceil((toLocalDateTime(this.now()) + DEFAULT_START_DELAY_SECS) / 3600) * 3600;
      this.valueChange.emit({ ...this.value(), start_at: this.value().start_at ?? hour });
    } else {
      this.valueChange.emit({ ...this.value(), start_at: null });
    }
  }

  protected setStartAtDate(event: Event): void {
    const date = (event.target as HTMLInputElement).value;
    // cleared: the one-shot is off
    this.valueChange.emit({ ...this.value(), start_at: date === '' ? null : inputToLocalDateTime(`${date}T${this.startAtTime()}`) });
  }

  protected setStartAtTime(time: string): void {
    const date = this.startAtDate();
    if (date !== '') {
      this.valueChange.emit({ ...this.value(), start_at: inputToLocalDateTime(`${date}T${time}`) });
    }
  }
}
