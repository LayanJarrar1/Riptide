import { TestBed } from '@angular/core/testing';

import { I18nService } from '../i18n/i18n.service';
import { FormatService } from './format.service';
import { MemoryPreferenceStore, NAVIGATOR_LANGUAGES, PREFERENCE_STORE } from './preferences';

describe('FormatService', () => {
  let format: FormatService;
  let i18n: I18nService;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: NAVIGATOR_LANGUAGES, useValue: ['en-US'] },
        { provide: PREFERENCE_STORE, useValue: new MemoryPreferenceStore() },
      ],
    });
    format = TestBed.inject(FormatService);
    i18n = TestBed.inject(I18nService);
  });

  afterEach(() => {
    document.documentElement.lang = 'en';
    document.documentElement.dir = 'ltr';
  });

  it('formats bytes with the right unit and precision', () => {
    expect(format.bytes(0)).toBe('0 B');
    expect(format.bytes(512)).toBe('512 B');
    expect(format.bytes(1536)).toBe('1.50 KB');
    expect(format.bytes(48.2 * 1024 * 1024)).toBe('48.2 MB');
    expect(format.bytes(5.8 * 1024 ** 3)).toBe('5.80 GB');
    expect(format.bytes(631 * 1024 * 1024)).toBe('631 MB');
    expect(format.bytes(null)).toBe('—');
  });

  it('formats speed, duration and percent', () => {
    expect(format.speed(0)).toBe('—');
    expect(format.speed(12.6 * 1024 * 1024)).toBe('12.6 MB/s');
    expect(format.duration(45)).toBe('45s');
    expect(format.duration(105)).toBe('1m 45s');
    expect(format.duration(3700)).toBe('1h 1m');
    expect(format.duration(null)).toBe('—');
    expect(format.percent(0.617)).toBe('62%');
    expect(format.percent(null)).toBe('—');
  });

  it('formats relative time from a fixed "now"', () => {
    const now = 1_760_000_000_000;
    expect(format.relative(now - 10_000, now)).toBe('now');
    expect(format.relative(now - 3 * 60_000, now)).toBe('3 minutes ago');
    expect(format.relative(now - 2 * 3_600_000, now)).toBe('2 hours ago');
    expect(format.relative(now - 86_400_000, now)).toBe('yesterday');
  });

  it('switches units and digits with the language, keeping Western digits in Arabic', () => {
    i18n.setLang('ar');
    expect(format.bytes(48.2 * 1024 * 1024)).toBe('48.2 م.ب');
    expect(format.speed(12.6 * 1024 * 1024)).toBe('12.6 م.ب/ث');
    expect(format.duration(105)).toBe('1د 45ث');
    expect(format.percent(0.5)).toContain('50');
    expect(format.int(1234)).toMatch(/1[,٬]?234/);
  });
});
