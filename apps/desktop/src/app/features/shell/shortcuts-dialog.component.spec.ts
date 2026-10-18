import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { I18nService } from '../../i18n/i18n.service';
import { ShortcutsDialogComponent } from './shortcuts-dialog.component';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ShortcutsDialogComponent],
  template: `<app-shortcuts-dialog [open]="open()" (closed)="closedCount.set(closedCount() + 1)" />`,
})
class HostComponent {
  readonly open = signal(true);
  readonly closedCount = signal(0);
}

describe('ShortcutsDialogComponent', () => {
  let fixture: ComponentFixture<HostComponent>;
  let host: HostComponent;

  function el(): HTMLElement {
    return fixture.nativeElement as HTMLElement;
  }

  function texts(selector: string): string[] {
    return Array.from(el().querySelectorAll(selector)).map((n) => n.textContent?.trim() ?? '');
  }

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [HostComponent],
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(HostComponent);
    host = fixture.componentInstance;
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  });

  afterEach(() => {
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
  });

  it('lists every shortcut in two groups with its key chords, in English', () => {
    expect(el().querySelector('h2')?.textContent?.trim()).toBe('Keyboard shortcuts');
    expect(texts('.group-title')).toEqual(['Anywhere', 'In the list']);
    const labels = texts('.label');
    expect(labels).toContain('Add a download');
    expect(labels).toContain('Pause or resume');
    expect(labels).toContain('Move the selected download in its queue');
    expect(labels.length).toBe(14);
    const add = Array.from(el().querySelectorAll('.row')).find((r) => r.querySelector('.label')?.textContent?.trim() === 'Add a download');
    expect(Array.from(add?.querySelectorAll('kbd') ?? []).map((k) => k.textContent)).toEqual(['Ctrl', 'N']);
    const help = Array.from(el().querySelectorAll('.row')).find((r) => r.querySelector('.label')?.textContent?.trim() === 'Show this list');
    expect(help?.querySelectorAll('.chord').length).toBe(2);
    expect(el().querySelector<HTMLDialogElement>('dialog')?.open).toBeTrue();
  });

  it('switches to Arabic live, keeping key caps Latin', () => {
    TestBed.inject(I18nService).setLang('ar');
    fixture.detectChanges();
    expect(el().querySelector('h2')?.textContent?.trim()).toBe('اختصارات لوحة المفاتيح');
    expect(texts('.group-title')).toEqual(['في كل مكان', 'في القائمة']);
    expect(texts('.label')).toContain('إيقاف مؤقت أو استئناف');
    expect(texts('kbd')).toContain('Ctrl');
    expect(texts('kbd')).toContain('مسافة');
  });

  it('asks to close on Escape and closes when the parent flips open', () => {
    const dialog = el().querySelector<HTMLDialogElement>('dialog');
    dialog?.dispatchEvent(new Event('cancel', { cancelable: true }));
    expect(host.closedCount()).toBe(1);
    host.open.set(false);
    fixture.detectChanges();
    expect(dialog?.open).toBeFalse();
  });
});
