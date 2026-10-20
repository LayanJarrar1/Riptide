import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { TauriService } from '../../tauri/tauri.service';
import { SettingsPageComponent } from './settings-page.component';

describe('SettingsPageComponent update channel', () => {
  function render(version: string, channel: 'stable' | 'beta') {
    TestBed.configureTestingModule({
      providers: [
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore({ 'riptide.settings': JSON.stringify({ channel }) }) },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      ],
    });
    TestBed.inject(TauriService).appInfo.set({ name: 'Riptide', version, build: 'x' });
    const fixture = TestBed.createComponent(SettingsPageComponent);
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  function selected(el: HTMLElement): string {
    const on = el.querySelector('[role="radiogroup"][aria-label="Update channel"] [aria-checked="true"]');
    return on?.textContent?.trim() ?? '';
  }

  it('shows Beta and the pre-release note on a pre-release build even when stable is saved', () => {
    const el = render('0.1.0-alpha.4', 'stable');
    expect(selected(el)).toBe('Beta');
    expect(el.textContent).toContain('Pre-release builds follow the beta channel.');
  });

  it('honours the saved channel on a stable build, without the note', () => {
    const el = render('1.0.0', 'stable');
    expect(selected(el)).toBe('Stable');
    expect(el.textContent).not.toContain('Pre-release builds follow the beta channel.');
  });
});
