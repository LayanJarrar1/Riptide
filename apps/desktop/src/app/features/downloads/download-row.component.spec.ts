import { ComponentFixture, TestBed } from '@angular/core/testing';

import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from '../../core/preferences';
import { DownloadRow } from '../../data/downloads.store';
import { seedDownloads } from '../../data/mock-data';
import { FailureKind } from '../../model/download';
import { DownloadRowComponent } from './download-row.component';

describe('DownloadRowComponent failure text and fix', () => {
  let fixture: ComponentFixture<DownloadRowComponent>;

  function failedRow(failure: FailureKind): DownloadRow {
    const base = seedDownloads(Date.now())[0];
    return { d: { ...base, state: 'failed', failure }, downloaded: 0, progress: 0, eta: null, activeMs: null, avgBps: null };
  }

  function render(failure: FailureKind, lang = 'en-US'): HTMLElement {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [DownloadRowComponent],
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: [lang] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    });
    fixture = TestBed.createComponent(DownloadRowComponent);
    fixture.componentRef.setInput('row', failedRow(failure));
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  const text = (el: HTMLElement, sel: string): string | undefined => el.querySelector(sel)?.textContent?.trim();

  it('a 401 says sign-in is required and offers credentials, not Retry', () => {
    const el = render('auth');
    expect(text(el, '.reason-text')).toBe('Sign-in required');
    expect(text(el, '.reason-action')).toBe('Enter credentials');
  });

  it('a 403 says access denied and does not claim the link expired', () => {
    const el = render('denied');
    expect(text(el, '.reason-text')).toBe('Access denied');
    expect(text(el, '.reason-action')).toBe('Enter credentials');
  });

  it('a 404 says the file is not found and offers no Retry anywhere in the row', () => {
    const el = render('notfound');
    expect(text(el, '.reason-text')).toBe('File not found on the server');
    expect(el.querySelector('.reason-action')).toBeNull();
    expect(el.textContent).not.toContain('Retry');
    expect(el.querySelector('[aria-label="Retry"]')).toBeNull();
  });

  it('a lost connection keeps its text and Retry', () => {
    const el = render('network');
    expect(text(el, '.reason-text')).toBe('Connection lost');
    expect(text(el, '.reason-action')).toBe('Retry');
  });

  it('labels the pill just "Failed" with the cause in its tooltip, and keeps the reason on one line', () => {
    const el = render('network');
    const pill = el.querySelector('app-status-pill');
    expect(pill?.textContent?.trim()).toBe('Failed');
    expect(pill?.getAttribute('title')).toBe('Connection lost');
    const t = el.querySelector<HTMLElement>('.reason-text');
    expect(t ? getComputedStyle(t).whiteSpace : '').toBe('nowrap');
    expect(t ? getComputedStyle(t).textOverflow : '').toBe('ellipsis');
  });

  it('is translated to Arabic', () => {
    const el = render('notfound', 'ar-SA');
    expect(text(el, '.reason-text')).toBe('الملف غير موجود على الخادم');
  });
});
