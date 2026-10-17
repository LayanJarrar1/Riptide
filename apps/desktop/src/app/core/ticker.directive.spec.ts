import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FRAME_SCHEDULER, FrameScheduler, REDUCED_MOTION_QUERY, ReducedMotionQuery } from './motion.service';
import { TickerDirective } from './ticker.directive';

/** A frame scheduler the test steps by hand. */
class ManualFrames implements FrameScheduler {
  private next = 1;
  readonly pending = new Map<number, (now: number) => void>();

  request(callback: (now: number) => void): number {
    const handle = this.next++;
    this.pending.set(handle, callback);
    return handle;
  }

  cancel(handle: number): void {
    this.pending.delete(handle);
  }

  /** Runs every pending frame callback once with the given clock. */
  flush(now: number): void {
    const callbacks = [...this.pending.values()];
    this.pending.clear();
    for (const callback of callbacks) {
      callback(now);
    }
  }
}

class FakeMotionQuery implements ReducedMotionQuery {
  private listeners: ((event: { matches: boolean }) => void)[] = [];

  constructor(public matches: boolean) {}

  addEventListener(_type: 'change', listener: (event: { matches: boolean }) => void): void {
    this.listeners.push(listener);
  }

  flip(matches: boolean): void {
    this.matches = matches;
    for (const listener of this.listeners) {
      listener({ matches });
    }
  }
}

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TickerDirective],
  template: `<span class="num" [appTicker]="value()" [tickerFormat]="format()" [tickerDuration]="100"></span>`,
})
class HostComponent {
  readonly value = signal(0);
  readonly format = signal<(n: number) => string>((n) => `${Math.round(n)} MB/s`);
}

describe('TickerDirective', () => {
  let fixture: ComponentFixture<HostComponent>;
  let host: HostComponent;
  let frames: ManualFrames;
  let motion: FakeMotionQuery;

  function text(): string {
    return (fixture.nativeElement as HTMLElement).querySelector('span')?.textContent ?? '';
  }

  function number(): number {
    return Number.parseInt(text(), 10);
  }

  beforeEach(async () => {
    frames = new ManualFrames();
    motion = new FakeMotionQuery(false);
    await TestBed.configureTestingModule({
      imports: [HostComponent],
      providers: [
        { provide: FRAME_SCHEDULER, useValue: frames },
        { provide: REDUCED_MOTION_QUERY, useValue: motion },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(HostComponent);
    host = fixture.componentInstance;
    host.value.set(40);
    fixture.detectChanges();
  });

  it('writes the first value at once without scheduling a frame', () => {
    expect(text()).toBe('40 MB/s');
    expect(frames.pending.size).toBe(0);
  });

  it('tweens to a new value over the duration, easing out, and lands exactly on the target', () => {
    host.value.set(140);
    fixture.detectChanges();
    expect(frames.pending.size).toBe(1);
    frames.flush(1000); // first frame: records the start time
    expect(number()).toBe(40);
    frames.flush(1050); // halfway through 100 ms: ease-out is past the midpoint
    const mid = number();
    expect(mid).toBeGreaterThan(90);
    expect(mid).toBeLessThan(140);
    frames.flush(1100);
    expect(text()).toBe('140 MB/s');
    expect(frames.pending.size).toBe(0);
  });

  it('retargets from the current displayed value when the target changes mid-tween', () => {
    host.value.set(100);
    fixture.detectChanges();
    frames.flush(0);
    frames.flush(50);
    const partway = number();
    expect(partway).toBeGreaterThan(40);
    host.value.set(0);
    fixture.detectChanges();
    frames.flush(60);
    // the new tween starts from where the old one was, not from 100
    expect(number()).toBeLessThanOrEqual(partway);
    frames.flush(200);
    expect(text()).toBe('0 MB/s');
  });

  it('snaps under reduced motion and re-renders when the formatter changes', () => {
    motion.flip(true);
    host.value.set(99);
    fixture.detectChanges();
    expect(frames.pending.size).toBe(0);
    expect(text()).toBe('99 MB/s');
    host.format.set((n) => `${Math.round(n)} م.ب/ث`);
    fixture.detectChanges();
    expect(text()).toBe('99 م.ب/ث');
  });

  it('cancels a pending frame when the host is destroyed', () => {
    host.value.set(500);
    fixture.detectChanges();
    expect(frames.pending.size).toBe(1);
    fixture.destroy();
    expect(frames.pending.size).toBe(0);
  });
});
