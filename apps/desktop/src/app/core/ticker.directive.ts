import { DestroyRef, Directive, effect, ElementRef, inject, input, NgZone, untracked } from '@angular/core';

import { DUR, FRAME_SCHEDULER, MotionService } from './motion.service';

/** Ease-out cubic: fast start, settles gently, which is how a number should land. */
function easeOut(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

/**
 * Number ticker: writes a formatted number into the host element and, when the value
 * changes, tweens the displayed number to the new one over `tickerDuration` ms with
 * `requestAnimationFrame`. The host owns no other content.
 *
 *     <span class="num" [appTicker]="row().d.speedBps" [tickerFormat]="speedFormat"></span>
 *
 * With `tickerStream` set, a value that changes again before the last change's tween could have
 * finished (a speed refreshed several times a second) is written at once: a tween that is always
 * restarting costs a frame of layout per row and shows nothing. Only elements inside the
 * browser viewport tween (an `IntersectionObserver` watches the host); hidden ones snap, so a long virtual list never schedules a frame per row. Frames are
 * cancelled on destroy and never scheduled under `prefers-reduced-motion`. The formatter is
 * called inside an effect, so one that follows the language signal re-renders on a switch.
 */
@Directive({ selector: '[appTicker]' })
export class TickerDirective {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly frames = inject(FRAME_SCHEDULER);
  private readonly motion = inject(MotionService);
  private readonly zone = inject(NgZone);

  /** The value to show. */
  readonly appTicker = input.required<number>();

  /** Formats a (possibly fractional, mid-tween) value for display. */
  readonly tickerFormat = input.required<(value: number) => string>();

  /** The value arrives as a stream of updates: tween only when it has been quiet for `tickerDuration`. */
  readonly tickerStream = input<boolean>(false);

  /** Tween length in milliseconds. */
  readonly tickerDuration = input<number>(DUR.d3);

  /** What is on screen right now (fractional while tweening). */
  private displayed: number | null = null;
  private frame: number | null = null;
  private visible = true;
  /** `performance.now()` of the last change of the target. */
  private changedAt = Number.NEGATIVE_INFINITY;
  private observer: IntersectionObserver | null = null;

  constructor() {
    if (typeof IntersectionObserver !== 'undefined') {
      this.observer = new IntersectionObserver((entries) => {
        for (const entry of entries) {
          this.visible = entry.isIntersecting;
        }
        if (!this.visible && this.frame !== null) {
          // scrolled away mid-tween: finish at once, no frame for an invisible element
          this.snap(this.appTicker(), this.tickerFormat());
        }
      });
      this.observer.observe(this.host);
    }

    effect(() => {
      const target = this.appTicker();
      const format = this.tickerFormat();
      // call the formatter here so its signal reads (the language) are tracked
      const text = format(target);
      untracked(() => this.retarget(target, text, format));
    });

    inject(DestroyRef).onDestroy(() => {
      this.cancel();
      this.observer?.disconnect();
      this.observer = null;
    });
  }

  private retarget(target: number, text: string, format: (value: number) => string): void {
    const from = this.displayed;
    const duration = this.tickerDuration();
    const now = performance.now();
    const sinceChange = now - this.changedAt;
    if (from !== target) {
      this.changedAt = now;
    }
    if (from === null || from === target || !this.visible || this.motion.reduced() || duration <= 0 || (this.tickerStream() && sinceChange < duration)) {
      this.cancel();
      this.displayed = target;
      this.host.textContent = text;
      return;
    }
    this.cancel();
    let start: number | null = null;
    const step = (now: number): void => {
      if (start === null) {
        start = now;
      }
      const t = Math.min(1, (now - start) / duration);
      const value = from + (target - from) * easeOut(t);
      this.displayed = value;
      this.host.textContent = t >= 1 ? text : format(value);
      if (t < 1) {
        this.frame = this.frames.request(step);
      } else {
        this.frame = null;
        this.displayed = target;
      }
    };
    // frames write straight to the DOM and never need change detection: keep them out of the
    // zone so a tween in progress does not hold the app (or a test's whenStable) "unstable"
    this.zone.runOutsideAngular(() => {
      this.frame = this.frames.request(step);
    });
  }

  private snap(target: number, format: (value: number) => string): void {
    this.cancel();
    this.displayed = target;
    this.host.textContent = format(target);
  }

  private cancel(): void {
    if (this.frame !== null) {
      this.frames.cancel(this.frame);
      this.frame = null;
    }
  }
}
