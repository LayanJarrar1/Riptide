import { inject, Pipe, PipeTransform } from '@angular/core';

import { FormatService } from './format.service';

/*
 * All formatting pipes are impure on purpose: their output depends on the active language,
 * which lives in a signal the pipe reads through FormatService. Each transform is a cached
 * Intl formatter call, so re-running on every check is cheap.
 */

/** `{{ bytes | bytes }}` → `1.5 GB`. */
@Pipe({ name: 'bytes', pure: false })
export class BytesPipe implements PipeTransform {
  private readonly format = inject(FormatService);
  transform(value: number | null | undefined): string {
    return this.format.bytes(value);
  }
}

/** `{{ bps | speed }}` → `48.2 MB/s`. */
@Pipe({ name: 'speed', pure: false })
export class SpeedPipe implements PipeTransform {
  private readonly format = inject(FormatService);
  transform(value: number | null | undefined): string {
    return this.format.speed(value);
  }
}

/** `{{ seconds | duration }}` → `1m 20s`. */
@Pipe({ name: 'duration', pure: false })
export class DurationPipe implements PipeTransform {
  private readonly format = inject(FormatService);
  transform(value: number | null | undefined): string {
    return this.format.duration(value);
  }
}

/** `{{ fraction | percent }}` → `62%`. */
@Pipe({ name: 'pct', pure: false })
export class PercentPipe implements PipeTransform {
  private readonly format = inject(FormatService);
  transform(value: number | null | undefined): string {
    return this.format.percent(value);
  }
}

/** `{{ timestamp | relTime }}` → `3 minutes ago`. */
@Pipe({ name: 'relTime', pure: false })
export class RelativeTimePipe implements PipeTransform {
  private readonly format = inject(FormatService);
  transform(value: number | null | undefined): string {
    return value === null || value === undefined ? '—' : this.format.relative(value);
  }
}

/** `{{ timestamp | dateTime }}` → `12 Oct 2026, 14:03`. */
@Pipe({ name: 'dateTime', pure: false })
export class DateTimePipe implements PipeTransform {
  private readonly format = inject(FormatService);
  transform(value: number | null | undefined): string {
    return value === null || value === undefined ? '—' : this.format.dateTime(value);
  }
}

/** Every formatting pipe, for `imports: [...FORMAT_PIPES]`. */
export const FORMAT_PIPES = [BytesPipe, SpeedPipe, DurationPipe, PercentPipe, RelativeTimePipe, DateTimePipe] as const;
