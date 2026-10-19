import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { NOW } from '../../core/clock';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { I18nService } from '../../i18n/i18n.service';
import { ALL_DAYS, nightSchedule, toLocalDateTime, UNSCHEDULED } from '../../model/schedule';
import type { Schedule } from '../../tauri/bridge/Schedule';
import { QueueScheduleEditorComponent } from './queue-schedule-editor.component';

const NOW_MS = Date.UTC(2026, 9, 19, 12, 0);

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [QueueScheduleEditorComponent],
  template: `<app-queue-schedule-editor [value]="value()" (valueChange)="value.set($event)" />`,
})
class HostComponent {
  readonly value = signal<Schedule>(UNSCHEDULED);
}

describe('QueueScheduleEditorComponent', () => {
  let fixture: ComponentFixture<HostComponent>;
  let host: HostComponent;

  function setup(languages: string[] = ['en-US']): void {
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: languages },
        { provide: NOW, useValue: () => NOW_MS },
      ],
    });
    fixture = TestBed.createComponent(HostComponent);
    host = fixture.componentInstance;
    fixture.detectChanges();
  }

  const el = (): HTMLElement => fixture.nativeElement as HTMLElement;
  const q = <T extends HTMLElement>(selector: string): T => el().querySelector<T>(selector) as T;
  const dayButtons = (): HTMLButtonElement[] => Array.from(el().querySelectorAll<HTMLButtonElement>('.day'));
  const click = (node: HTMLElement): void => {
    node.click();
    fixture.detectChanges();
  };
  function type(input: HTMLInputElement, value: string): void {
    input.value = value;
    input.dispatchEvent(new Event('change'));
    fixture.detectChanges();
  }
  /** Pick an `HH:mm` time in the 24 h field called `name`. */
  function pickTime(name: string, value: string): void {
    const [h, m] = value.split(':');
    for (const [suffix, v] of [['h', h], ['m', m]] as const) {
      const select = q<HTMLSelectElement>(`select[name="${name}-${suffix}"]`);
      select.value = v;
      select.dispatchEvent(new Event('change'));
      fixture.detectChanges();
    }
  }
  const timeOf = (name: string): string => `${q<HTMLSelectElement>(`select[name="${name}-h"]`).value}:${q<HTMLSelectElement>(`select[name="${name}-m"]`).value}`;

  it('starts unrestricted: no times, every day on, no one-shot, and the sleeping-PC note is there', () => {
    setup();
    expect(q('select[name="from-h"]')).toBeNull();
    expect(dayButtons().every((b) => b.getAttribute('aria-pressed') === 'true')).toBeTrue();
    expect(q('input[name="startAtDate"]')).toBeNull();
    expect(q('.note').textContent).toContain('A sleeping or switched-off PC is not woken up');
    expect(q('.issue')).toBeNull();
  });

  describe('Only at night preset', () => {
    it('sets 23:00 to 07:00 on every day and shows both times', () => {
      setup();
      const chips = el().querySelectorAll<HTMLButtonElement>('.chip');
      click(chips[0]);
      expect(host.value()).toEqual(nightSchedule());
      expect(host.value().window).toEqual({ start_min: 1380, stop_min: 420 });
      expect(timeOf('from')).toBe('23:00');
      expect(timeOf('to')).toBe('07:00');
      expect(chips[0].getAttribute('aria-pressed')).toBe('true');
      expect(q('.hint').textContent).toContain('overnight');
    });

    it('stops being "pressed" once anything is changed, and "Any time" clears everything', () => {
      setup();
      const chips = el().querySelectorAll<HTMLButtonElement>('.chip');
      click(chips[0]);
      pickTime('to', '08:30');
      expect(host.value().window).toEqual({ start_min: 1380, stop_min: 510 });
      expect(chips[0].getAttribute('aria-pressed')).toBe('false');
      click(chips[1]);
      expect(host.value()).toEqual(UNSCHEDULED);
      expect(q('select[name="from-h"]')).toBeNull();
    });

    it('the checkbox switches the window on (at night hours) and off', () => {
      setup();
      click(q('input[name="restrict"]'));
      expect(host.value().window).toEqual({ start_min: 1380, stop_min: 420 });
      click(q('input[name="restrict"]'));
      expect(host.value().window).toBeNull();
    });
  });

  describe('days', () => {
    it('toggles single days in the engine bit mask', () => {
      setup(['en-GB']);
      const names = dayButtons().map((b) => b.getAttribute('aria-label'));
      expect(names[0]).toBe('Monday');
      click(dayButtons()[0]);
      expect(host.value().days).toBe(ALL_DAYS & ~1);
      expect(dayButtons()[0].getAttribute('aria-pressed')).toBe('false');
      click(dayButtons()[6]);
      expect(host.value().days).toBe(0b0111110);
      click(q('.link'));
      expect(host.value().days).toBe(ALL_DAYS);
    });

    it('starts the week on the locale first day with the language day names', () => {
      setup(['en-GB']);
      expect(dayButtons().map((b) => b.getAttribute('aria-label'))).toEqual(['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']);
      TestBed.inject(I18nService).setLang('ar');
      fixture.detectChanges();
      const arabic = dayButtons().map((b) => b.getAttribute('aria-label') ?? '');
      expect(arabic).toHaveSize(7);
      // Arabic week data starts on Saturday, and the names are Arabic script
      expect(arabic[0]).toBe(new Intl.DateTimeFormat('ar', { weekday: 'long', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, 0, 6))));
      expect(arabic.every((n) => /[؀-ۿ]/.test(n))).toBeTrue();
    });

    it('turning every day off is reported and a day brings the schedule back to valid', () => {
      setup();
      for (const b of dayButtons()) {
        click(b);
      }
      expect(host.value().days).toBe(0);
      expect(q('.issue').textContent).toContain('Choose at least one day');
      click(dayButtons()[2]);
      expect(q('.issue')).toBeNull();
    });
  });

  describe('validation', () => {
    it('reports a window with the same start and stop time', () => {
      setup();
      click(q('input[name="restrict"]'));
      pickTime('to', '23:00');
      expect(q('.issue').textContent).toContain('must be different');
    });

    it('shows times in 24 h, with no AM/PM, in the Arabic UI too', () => {
      setup(['ar-SA']);
      TestBed.inject(I18nService).setLang('ar');
      click(q('input[name="restrict"]'));
      expect(timeOf('from')).toBe('23:00');
      expect(el().textContent).not.toMatch(/AM|PM/);
      expect(el().querySelector('input[type="time"], input[type="datetime-local"]')).toBeNull();
    });
  });

  describe('one-shot "Start at"', () => {
    it('ticking the box proposes the next full hour from now and shows the date field', () => {
      setup();
      click(q('input[name="startAtOn"]'));
      const at = host.value().start_at;
      expect(at).not.toBeNull();
      const now = toLocalDateTime(NOW_MS);
      expect(at ?? 0).toBeGreaterThan(now);
      expect((at ?? 0) % 3600).toBe(0);
      expect(q<HTMLInputElement>('input[name="startAtDate"]').value).toMatch(/^2026-10-\d\d$/);
      expect(timeOf('startAtTime')).toMatch(/^\d\d:00$/);
      expect(q('.issue')).toBeNull();
    });

    it('takes the typed date and time, flags the past, and unticking clears it', () => {
      setup();
      click(q('input[name="startAtOn"]'));
      type(q('input[name="startAtDate"]'), '2026-12-24');
      pickTime('startAtTime', '03:15');
      expect(host.value().start_at).toBe(Date.UTC(2026, 11, 24, 3, 15) / 1000);
      type(q('input[name="startAtDate"]'), '2020-01-01');
      pickTime('startAtTime', '00:00');
      expect(q('.issue').textContent).toContain('in the future');
      click(q('input[name="startAtOn"]'));
      expect(host.value().start_at).toBeNull();
      expect(q('.issue')).toBeNull();
    });

    it('clearing the field switches the one-shot off', () => {
      setup();
      click(q('input[name="startAtOn"]'));
      type(q('input[name="startAtDate"]'), '');
      expect(host.value().start_at).toBeNull();
      expect(q('input[name="startAtDate"]')).toBeNull();
    });
  });
});
