import { computed, inject, Injectable } from '@angular/core';

import { I18nService, Lang } from '../i18n/i18n.service';

const UNITS: readonly string[] = ['B', 'KB', 'MB', 'GB', 'TB'];

/**
 * Number and time formatting that follows the active language. Western (latn) digits are used
 * in both languages: sizes, speeds and hashes are read against English-language sources
 * (file names, terminal output, checksums), and mixing digit systems in one row hurts scanning.
 */
@Injectable({ providedIn: 'root' })
export class FormatService {
  private readonly i18n = inject(I18nService);

  private readonly locale = computed(() => (this.i18n.lang() === 'ar' ? 'ar-u-nu-latn' : 'en-US'));

  private readonly numbers = computed(() => ({
    int: new Intl.NumberFormat(this.locale(), { maximumFractionDigits: 0 }),
    one: new Intl.NumberFormat(this.locale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
    two: new Intl.NumberFormat(this.locale(), { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    percent: new Intl.NumberFormat(this.locale(), { style: 'percent', maximumFractionDigits: 0 }),
    relative: new Intl.RelativeTimeFormat(this.locale(), { numeric: 'auto' }),
    dateTime: new Intl.DateTimeFormat(this.locale(), { dateStyle: 'medium', timeStyle: 'short' }),
    time: new Intl.DateTimeFormat(this.locale(), { hour: 'numeric', minute: '2-digit' }),
    day: new Intl.DateTimeFormat(this.locale(), { dateStyle: 'medium' }),
  }));

  /** Active language (for callers that branch on it). */
  get lang(): Lang {
    return this.i18n.lang();
  }

  /** `1.5 GB`, `820 MB`, `12 KB`. Unit labels are translated. */
  bytes(n: number | null | undefined): string {
    if (n === null || n === undefined || !Number.isFinite(n)) {
      return '—';
    }
    let value = Math.max(0, n);
    let unit = 0;
    while (value >= 1024 && unit < UNITS.length - 1) {
      value /= 1024;
      unit++;
    }
    const f = this.numbers();
    const text = unit === 0 ? f.int.format(value) : value >= 100 ? f.int.format(value) : value >= 10 ? f.one.format(value) : f.two.format(value);
    return `${text} ${this.i18n.t(`unit.${UNITS[unit]}`)}`;
  }

  /** `48.2 MB/s`; `—` when idle. */
  speed(bps: number | null | undefined): string {
    if (bps === null || bps === undefined || bps <= 0) {
      return '—';
    }
    return `${this.bytes(bps)}${this.i18n.t('unit.perSecond')}`;
  }

  /** `1m 20s`, `2h 05m`, `—` when unknown. */
  duration(seconds: number | null | undefined): string {
    if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) {
      return '—';
    }
    const s = Math.max(0, Math.round(seconds));
    const t = (key: string, n: number): string => this.i18n.t(key).replace('{n}', this.numbers().int.format(n));
    if (s < 60) {
      return t('time.seconds', s);
    }
    const m = Math.floor(s / 60);
    if (m < 60) {
      return `${t('time.minutes', m)} ${t('time.seconds', s % 60)}`;
    }
    const h = Math.floor(m / 60);
    if (h < 24) {
      return `${t('time.hours', h)} ${t('time.minutes', m % 60)}`;
    }
    return `${t('time.days', Math.floor(h / 24))} ${t('time.hours', h % 24)}`;
  }

  /** `62%`; `—` when unknown. */
  percent(fraction: number | null | undefined): string {
    if (fraction === null || fraction === undefined) {
      return '—';
    }
    return this.numbers().percent.format(Math.min(1, Math.max(0, fraction)));
  }

  /** `3 minutes ago`, `yesterday`, `in 2 hours`. */
  relative(timestamp: number, now = Date.now()): string {
    const diff = Math.round((timestamp - now) / 1000);
    const abs = Math.abs(diff);
    const r = this.numbers().relative;
    if (abs < 45) {
      return r.format(0, 'second');
    }
    if (abs < 3600) {
      return r.format(Math.round(diff / 60), 'minute');
    }
    if (abs < 86_400) {
      return r.format(Math.round(diff / 3600), 'hour');
    }
    if (abs < 86_400 * 30) {
      return r.format(Math.round(diff / 86_400), 'day');
    }
    return this.numbers().dateTime.format(new Date(timestamp));
  }

  /** `12 Oct 2026, 14:03`. */
  dateTime(timestamp: number): string {
    return this.numbers().dateTime.format(new Date(timestamp));
  }

  /** `2:03 PM`: the time of day. */
  time(timestamp: number): string {
    return this.numbers().time.format(new Date(timestamp));
  }

  /** `12 Oct 2026`: the date. */
  day(timestamp: number): string {
    return this.numbers().day.format(new Date(timestamp));
  }

  /** Plain integer with grouping. */
  int(n: number): string {
    return this.numbers().int.format(n);
  }
}
