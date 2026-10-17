import { inject, Injectable, InjectionToken, signal } from '@angular/core';

/**
 * The subset of `MediaQueryList` the motion service needs for `prefers-reduced-motion`;
 * tests provide a fake.
 */
export interface ReducedMotionQuery {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: (event: { matches: boolean }) => void): void;
}

export const REDUCED_MOTION_QUERY = new InjectionToken<ReducedMotionQuery>('riptide.reducedMotionQuery', {
  providedIn: 'root',
  factory: () => globalThis.matchMedia('(prefers-reduced-motion: reduce)'),
});

/** Schedules work for the next frame. Tests provide a manual one and step it by hand. */
export interface FrameScheduler {
  request(callback: (now: number) => void): number;
  cancel(handle: number): void;
}

export const FRAME_SCHEDULER = new InjectionToken<FrameScheduler>('riptide.frameScheduler', {
  providedIn: 'root',
  factory: (): FrameScheduler => ({
    request: (callback) => requestAnimationFrame(callback),
    cancel: (handle) => cancelAnimationFrame(handle),
  }),
});

/** Durations in milliseconds, mirroring `--dur-*` in tokens.css for script-driven motion. */
export const DUR = { d1: 120, d2: 200, d3: 320, d4: 600 } as const;

/**
 * What the OS says about motion, as a signal, for the few animations driven from script
 * (number tickers, the list crossfade). CSS reads the same preference through the tokens.
 */
@Injectable({ providedIn: 'root' })
export class MotionService {
  private readonly query = inject(REDUCED_MOTION_QUERY);

  /** `true` when the user asked for reduced motion. */
  readonly reduced = signal(this.query.matches);

  constructor() {
    this.query.addEventListener('change', (event) => this.reduced.set(event.matches));
  }
}
