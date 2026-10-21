import { ChangeDetectionStrategy, Component, computed, effect, input, signal, untracked } from '@angular/core';

import { DownloadState, Segment } from '../model/download';

/** One rendered piece of the bar: where the segment sits and how full it is. */
interface Piece {
  readonly key: number;
  /** Start offset as a percentage of the whole file. */
  readonly left: number;
  /** Width as a percentage of the whole file. */
  readonly width: number;
  /** Fill 0..1. */
  readonly fill: number;
  readonly state: Segment['state'];
}

/**
 * The segment flow bar: Riptide's signature. Every byte range of the file is a piece of the
 * track, laid out at its true position, and fills from its own start. Watching several pieces
 * fill at once *is* the explanation of multi-connection downloading and work stealing, so the
 * component shows exactly the engine's segments rather than one averaged bar.
 *
 * Unknown size (no `Content-Length`) renders an indeterminate stream; paused and failed
 * downloads keep their fills but change hue; completed collapses to one full bar, which wipes
 * in once with an ease the moment the download completes (not when a completed row is merely
 * scrolled into view). Active pieces carry a slow sheen.
 */
@Component({
  selector: 'app-progress-flow',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.s-active]': 'state() === "downloading" || state() === "probing"',
    '[class.s-paused]': 'state() === "paused" || state() === "queued"',
    '[class.s-failed]': 'state() === "failed" || state() === "cancelled"',
    '[class.s-done]': 'state() === "completed" || state() === "verifying"',
    '[class.is-indeterminate]': 'indeterminate()',
    '[class.is-large]': 'large()',
    '[class.is-completing]': 'completing()',
    '(animationend)': 'completing.set(false)',
    role: 'progressbar',
    '[attr.aria-valuemin]': '0',
    '[attr.aria-valuemax]': '100',
    '[attr.aria-valuenow]': 'ariaValue()',
  },
  template: `
    @if (indeterminate()) {
      <span class="track stream"><span class="pulse motion-ambient"></span></span>
    } @else {
      <span class="track">
        @for (p of pieces(); track p.key) {
          <span
            class="piece"
            [class.p-active]="p.state === 'active'"
            [class.p-done]="p.state === 'done'"
            [class.p-failed]="p.state === 'failed'"
            [style.inset-inline-start.%]="p.left"
            [style.inline-size.%]="p.width"
          >
            <span class="fill motion-ambient" [style.transform]="'scaleX(' + p.fill + ')'"></span>
          </span>
        }
      </span>
    }
  `,
  styles: `
    :host {
      display: block;
      inline-size: 100%;
      --flow-height: 6px;
      --flow-radius: 3px;
    }

    :host(.is-large) {
      --flow-height: 14px;
      --flow-radius: 4px;
    }

    .track {
      position: relative;
      display: block;
      block-size: var(--flow-height);
      border-radius: var(--flow-radius);
      background: var(--flow-track);
      overflow: hidden;
    }

    .piece {
      position: absolute;
      inset-block: 0;
      /* a hairline of track between pieces makes the segments countable */
      padding-inline-end: var(--flow-gap);
      box-sizing: border-box;
    }

    .fill {
      display: block;
      block-size: 100%;
      inline-size: 100%;
      background: var(--flow-fill);
      transform-origin: var(--origin, left center);
      transition: background-color var(--dur-2) var(--ease-standard);
    }

    /* Progress only ever grows; in the drawer a transition smooths the engine ticks. In the list
       it would restart every 100 ms on every active piece of every visible row (a style
       recalculation each) to smooth a 6 px bar, so the rows step instead. */
    :host(.is-large) .fill {
      transition: transform 260ms linear, background-color var(--dur-2) var(--ease-standard);
    }

    :host-context([dir='rtl']) .fill {
      --origin: right center;
    }

    .p-done .fill {
      background: var(--flow-fill-done);
    }

    .p-failed .fill,
    :host(.s-failed) .fill {
      background: var(--flow-fill-failed);
    }

    :host(.s-paused) .fill {
      background: var(--flow-fill-paused);
    }

    :host(.s-done) .fill {
      background: var(--state-completed);
    }

    /* The current: a faint highlight drifting slowly along each active piece — the one ambient
       motion in a row, and the only sign besides the hue that this stripe is receiving bytes.
       It moves by transform (compositor only): animating background-position repainted every
       active piece on every frame, which dominated the cost of a busy list. */
    :host(.s-active) .p-active .fill::after {
      content: '';
      position: absolute;
      inset: 0;
      background: linear-gradient(
        90deg,
        transparent 0%,
        rgb(255 255 255 / 6%) 35%,
        rgb(255 255 255 / 18%) 50%,
        rgb(255 255 255 / 6%) 65%,
        transparent 100%
      );
      transform: translateX(calc(var(--dir-sign) * -100%));
      animation: current var(--dur-ambient) linear infinite;
    }

    :host(.s-active) .p-active .fill {
      position: relative;
      overflow: hidden;
    }

    @keyframes current {
      from {
        transform: translateX(calc(var(--dir-sign) * -100%));
      }

      to {
        transform: translateX(calc(var(--dir-sign) * 100%));
      }
    }

    /* Completed: the single full bar wipes in once, start to end (mirrored in RTL). */
    .piece {
      --wipe-right: 100%;
      --wipe-left: 0%;
    }

    :host-context([dir='rtl']) .piece {
      --wipe-right: 0%;
      --wipe-left: 100%;
    }

    :host(.is-completing) .piece {
      animation: complete-wipe var(--dur-4) var(--ease-emphasized-out) both;
    }

    @keyframes complete-wipe {
      from {
        clip-path: inset(0 var(--wipe-right) 0 var(--wipe-left));
      }

      to {
        clip-path: inset(0 0% 0 0%);
      }
    }

    .stream .pulse {
      position: absolute;
      inset-block: 0;
      inset-inline-start: 0;
      inline-size: 30%;
      border-radius: inherit;
      background: linear-gradient(90deg, transparent, var(--flow-fill), transparent);
      animation: stream 1600ms var(--ease-standard) infinite;
    }

    @keyframes stream {
      from {
        transform: translateX(calc(var(--dir-sign) * -100%));
      }

      to {
        transform: translateX(calc(var(--dir-sign) * 340%));
      }
    }

    @media (prefers-reduced-motion: reduce) {
      :host(.s-active) .p-active .fill::after,
      .stream .pulse {
        animation: none;
      }

      .stream .pulse {
        inset-inline-start: 35%;
      }

      .fill {
        transition: none;
      }
    }
  `,
})
export class ProgressFlowComponent {
  /** The engine's segments, any order. */
  readonly segments = input.required<readonly Segment[]>();

  /** Total size; `null` renders the indeterminate stream. */
  readonly size = input.required<number | null>();

  /** Download state, which picks the hue. */
  readonly state = input.required<DownloadState>();

  /** 14px tall variant for the details drawer. */
  readonly large = input(false);

  /** `true` while the completion wipe plays (only after a real transition to `completed`). */
  protected readonly completing = signal(false);

  private previousState: DownloadState | null = null;

  protected readonly indeterminate = computed(() => {
    const s = this.state();
    return (this.size() === null || this.size() === 0) && s !== 'completed';
  });

  protected readonly pieces = computed<readonly Piece[]>(() => {
    const size = this.size();
    const state = this.state();
    if (size === null || size <= 0) {
      return [];
    }
    if (state === 'completed') {
      return [{ key: -1, left: 0, width: 100, fill: 1, state: 'done' }];
    }
    return [...this.segments()]
      .filter((s) => s.end > s.start)
      .sort((a, b) => a.start - b.start)
      .map((s) => {
        const len = s.end - s.start;
        return {
          key: s.index,
          left: (s.start / size) * 100,
          width: (len / size) * 100,
          fill: Math.min(1, s.written / len),
          state: s.state,
        };
      });
  });

  protected readonly ariaValue = computed(() => {
    const size = this.size();
    if (size === null || size <= 0) {
      return null;
    }
    if (this.state() === 'completed') {
      return 100;
    }
    let written = 0;
    for (const s of this.segments()) {
      written += s.written;
    }
    return Math.round((written / size) * 100);
  });

  constructor() {
    effect(() => {
      const state = this.state();
      const previous = this.previousState;
      this.previousState = state;
      if (previous !== null && previous !== 'completed' && state === 'completed') {
        untracked(() => this.completing.set(true));
      }
    });
  }
}
