import { TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { provideDownloadEngine } from '../../data/engine.provider';
import { MOCK_ENGINE_AUTOSTART, MockEngineService } from '../../data/mock-engine.service';
import { I18nService } from '../../i18n/i18n.service';
import { ToastService } from '../../ui/toast.service';
import { CompletionToastService } from './completion-toast.service';

describe('CompletionToastService', () => {
  let engine: MockEngineService;
  let toasts: ToastService;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideDownloadEngine(),
        { provide: MOCK_ENGINE_AUTOSTART, useValue: false },
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    });
    TestBed.inject(CompletionToastService);
    engine = TestBed.inject(MockEngineService);
    toasts = TestBed.inject(ToastService);
    TestBed.tick();
  });

  /** Adds a tiny download with the queue to itself and walks it to `completed`. */
  async function completeOne(): Promise<number> {
    const id = await engine.add(
      { url: 'https://example.com/notes.pdf', connections: 2 },
      { finalUrl: 'https://example.com/notes.pdf', filename: 'notes.pdf', size: 2 * 1024 * 1024, supportsRanges: true, contentType: null, category: 'documents' },
    );
    for (const d of engine.downloads()) {
      if (d.id !== id) {
        // the seed list has one download mid-verification; it would complete too and announce
        void (d.state === 'verifying' ? engine.cancel(d.id) : engine.pause(d.id));
      }
    }
    for (let i = 0; i < 400 && engine.get(id)?.state !== 'completed'; i++) {
      engine.tick(250);
    }
    expect(engine.get(id)?.state).toBe('completed');
    return id;
  }

  it('announces nothing for the downloads that were already complete at start', () => {
    expect(engine.downloads().some((d) => d.state === 'completed')).toBeTrue();
    expect(toasts.toasts().length).toBe(0);
    engine.tick(250);
    TestBed.tick();
    expect(toasts.toasts().length).toBe(0);
  });

  it('shows one success toast with Open and Show in folder when a download completes', async () => {
    await completeOne();
    TestBed.tick();
    const list = toasts.toasts();
    expect(list.length).toBe(1);
    expect(list[0].tone).toBe('success');
    expect(list[0].text).toBe('notes.pdf is complete');
    expect(list[0].actions.map((a) => a.label)).toEqual(['Open', 'Show in folder']);
    engine.tick(250);
    TestBed.tick();
    expect(toasts.toasts().length).toBe(1); // completed stays completed: no repeat
  });

  it('speaks Arabic, and in a plain browser the actions explain that opening comes later', async () => {
    TestBed.inject(I18nService).setLang('ar');
    await completeOne();
    TestBed.tick();
    const toast = toasts.toasts()[0];
    expect(toast.text).toBe('اكتمل notes.pdf');
    expect(toast.actions.map((a) => a.label)).toEqual(['فتح', 'إظهار في المجلد']);
    toast.actions[0].run();
    const latest = toasts.toasts().at(-1);
    expect(latest?.tone).toBe('info');
    expect(latest?.text).toBe('فتح الملفات يأتي مع المرحلة القادمة.');
  });
});
