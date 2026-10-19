import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../core/preferences';
import { I18nService } from '../i18n/i18n.service';
import { TimeFieldComponent } from './time-field.component';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TimeFieldComponent],
  template: `<app-time-field name="t" label="Start" [value]="value()" (valueChange)="value.set($event)" />`,
})
class HostComponent {
  readonly value = signal('23:00');
}

describe('TimeFieldComponent', () => {
  let fixture: ComponentFixture<HostComponent>;

  function setup(languages: string[]): void {
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
        { provide: NAVIGATOR_LANGUAGES, useValue: languages },
      ],
    });
    fixture = TestBed.createComponent(HostComponent);
    fixture.detectChanges();
  }

  const select = (name: string): HTMLSelectElement => (fixture.nativeElement as HTMLElement).querySelector<HTMLSelectElement>(`select[name="${name}"]`) as HTMLSelectElement;
  function pick(el: HTMLSelectElement, value: string): void {
    el.value = value;
    el.dispatchEvent(new Event('change'));
    fixture.detectChanges();
  }

  it('shows 24-hour values, never AM/PM, in English and in Arabic', () => {
    setup(['en-US']);
    expect(select('t-h').value).toBe('23');
    expect(select('t-m').value).toBe('00');
    expect(select('t-h').options).toHaveSize(24);
    expect(select('t-m').options).toHaveSize(60);
    TestBed.inject(I18nService).setLang('ar');
    fixture.detectChanges();
    const text = (fixture.nativeElement as HTMLElement).textContent ?? '';
    expect(text).not.toMatch(/AM|PM|ص|م\b/);
    expect(select('t-h').value).toBe('23');
    expect(select('t-h').closest('[dir]')?.getAttribute('dir')).toBe('ltr');
  });

  it('emits HH:mm when the hour or the minute changes', () => {
    setup(['ar-SA']);
    pick(select('t-h'), '07');
    expect(fixture.componentInstance.value()).toBe('07:00');
    pick(select('t-m'), '45');
    expect(fixture.componentInstance.value()).toBe('07:45');
  });

  it('gives the selects accessible names in the active language', () => {
    setup(['ar']);
    expect(select('t-h').getAttribute('aria-label')).toBe('Start: الساعات');
    expect(select('t-m').getAttribute('aria-label')).toBe('Start: الدقائق');
  });
});
