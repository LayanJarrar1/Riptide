import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { I18nService, loadTable } from '../../i18n/i18n.service';
import { TAURI_AVAILABLE } from '../../tauri/tauri-bridge';
import { AiSettingsComponent } from './ai-settings.component';

beforeAll(async () => {
  await loadTable('en');
  await loadTable('ar');
});

type Usage = (tokens: number, requests: number) => string;

/** `usageText` is the component's own (protected) formatter; the template only prints its result. */
function usageIn(lang: 'en' | 'ar'): Usage {
  TestBed.configureTestingModule({
    providers: [
      { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
      { provide: TAURI_AVAILABLE, useValue: false },
    ],
  });
  TestBed.inject(I18nService).setLang(lang);
  const c = TestBed.createComponent(AiSettingsComponent).componentInstance as unknown as { usageText: Usage };
  return (t, r) => c.usageText(t, r);
}

describe('AiSettingsComponent usage text', () => {
  it('uses the English singular and plural forms', () => {
    const usage = usageIn('en');
    expect(usage(1200, 1)).toContain('1 request');
    expect(usage(1200, 1)).not.toContain('1 requests');
    expect(usage(1200, 3)).toContain('3 requests');
    expect(usage(1200, 3)).toContain('1,200');
  });

  it('uses the Arabic one form for 1 and the few form for 3', () => {
    const usage = usageIn('ar');
    expect(usage(500, 1)).toContain('طلب واحد');
    const few = usage(500, 3);
    expect(few).toContain('3 طلبات');
    expect(few).not.toContain('{n}');
  });
});
