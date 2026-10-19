import { InjectionToken } from '@angular/core';

/** The clock (epoch milliseconds); specs provide a fixed one. */
export const NOW = new InjectionToken<() => number>('riptide.now', {
  providedIn: 'root',
  factory: () => () => Date.now(),
});
