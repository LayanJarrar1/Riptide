import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from '@angular/core';

import { MotionService } from '../core/motion.service';
import { ThemeService } from '../theme/theme.service';
import {
  LOGO_MARK_FILL,
  LOGO_MARK_PATH,
  LOGO_WORDMARK_FILL,
  LOGO_WORDMARK_PATHS,
} from './brand-logo-paths';

/** Mark box in brand-kit units: the R spans x 131..282 and y 54..254 of the full logo. */
const MARK_BOX = { x: 125, y: 48, w: 163, h: 212 } as const;
/** Mark plus wordmark: x 125..830, y 48..306 of the full logo (the "p" descends to 300). */
const FULL_BOX = { x: 125, y: 48, w: 705, h: 258 } as const;
/** The mark is 200 units tall; `size` is that height in px. */
const MARK_UNITS = 200;
/**
 * Centre lines of the three streams of the R, from the bottom of each vertical up and around
 * the bowl. They only drive a reveal mask (thick strokes that uncover the original filled
 * path); they are never painted, so they are not part of the logo artwork.
 */
const STREAMS = [
  {
    id: 'a',
    w: 20,
    delay: 0,
    dur: 1050,
    d: 'M138.6 268 L138.6 118 C138.6 85 165 61.8 200 61.8 C235 61.8 264.5 88 264.5 122 C264.5 150 250 170 238 184 L262 232 L270 262',
  },
  {
    id: 'b',
    w: 21,
    delay: 90,
    dur: 1050,
    d: 'M157.8 268 L157.8 118 C157.8 96 176 81 199 81 C228 81 249.5 100 249.5 122 C249.5 140 238 152 224 160 C214 166 213 176 213 188 C213 196 216 202 222 212 L244 262',
  },
  {
    id: 'c',
    w: 19,
    delay: 180,
    dur: 1050,
    d: 'M177 268 L177 118 C177 107 187 100.3 199 100.3 C218 100.3 228 110 228 122 C228 134 216 142 208 148 C198 154 194 168 194 185 C194 196 198 205 204 215 L224 262',
  },
  { id: 'leg', w: 44, delay: 560, dur: 500, d: 'M234 170 L268 270' },
] as const;
/** The ripple ring starts at the foot of the leg, outside the mark. */
const RIPPLE = { cx: 258, cy: 236, r: 14 } as const;

let nextLogoId = 0;

/**
 * The Riptide logo for the Research start page. It plays once on first view, like the brand
 * sting: the three streams of the filled R grow up from its foot and around the bowl (a
 * stroke-dash mask uncovers the original filled path, no outline stage), one faint ripple ring
 * leaves the leg, then the wordmark is revealed left to right. Only transform, opacity,
 * stroke-dashoffset and clip-path animate; paths and fills are the brand kit's, so the
 * resting state is the brand SVG. Reduced motion shows it at once. The logo never mirrors in
 * RTL: the wordmark is Latin and always reveals left to right.
 */
@Component({
  selector: 'app-brand-logo-animated',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (run of [runs()]; track run) {
      <svg
        class="logo"
        [class.play]="!motion.reduced()"
        [attr.width]="width()"
        [attr.height]="height()"
        [attr.viewBox]="viewBox()"
        role="img"
        aria-label="Riptide"
        xmlns="http://www.w3.org/2000/svg"
      >
        <defs>
          <mask
            [attr.id]="maskId"
            maskUnits="userSpaceOnUse"
            x="120"
            y="40"
            width="180"
            height="230"
          >
            @for (s of streams; track s.id) {
              <path
                class="reveal"
                [style.--delay]="s.delay + 'ms'"
                [style.--dur]="s.dur + 'ms'"
                [attr.d]="s.d"
                pathLength="1"
                stroke-dasharray="1 2"
                fill="none"
                stroke="#fff"
                [attr.stroke-width]="s.w"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            }
          </mask>
        </defs>
        <g class="mark">
          <path
            class="fill"
            [attr.d]="markPath"
            [attr.fill]="markFill()"
            [attr.mask]="'url(#' + maskId + ')'"
          />
        </g>
        <circle
          class="ripple"
          [attr.cx]="ripple.cx"
          [attr.cy]="ripple.cy"
          [attr.r]="ripple.r"
          fill="none"
          [attr.stroke]="markFill()"
          stroke-width="1.5"
          vector-effect="non-scaling-stroke"
          aria-hidden="true"
        />
        @if (showWordmark()) {
          <g class="word" [attr.fill]="wordFill()">
            @for (d of wordmarkPaths; track $index) {
              <path [attr.d]="d" />
            }
          </g>
        }
      </svg>
    }
  `,
  styles: `
    :host {
      display: inline-block;
      line-height: 0;
      /* Streams grow over ~1 s on a gentle curve; ripple 550 ms; wordmark reveal 450 ms. */
      --logo-ease: cubic-bezier(0.2, 0.7, 0.2, 1);
      --logo-draw: cubic-bezier(0.45, 0, 0.2, 1);
    }
    .logo {
      display: block;
      overflow: visible;
    }
    .ripple {
      opacity: 0;
    }
    .play .reveal {
      stroke-dashoffset: 0;
      animation: logo-draw var(--dur) var(--logo-draw) var(--delay) backwards;
    }
    .play .mark {
      transform-box: fill-box;
      transform-origin: center;
      animation: logo-settle 500ms var(--logo-ease) 900ms backwards;
    }
    .play .ripple {
      transform-box: fill-box;
      transform-origin: center;
      animation: logo-ripple 1550ms var(--logo-ease) backwards;
    }
    .play .word {
      animation: logo-word 450ms var(--logo-draw) 1400ms backwards;
    }
    @keyframes logo-draw {
      from {
        stroke-dashoffset: 1;
      }
      to {
        stroke-dashoffset: 0;
      }
    }
    @keyframes logo-fade-in {
      from {
        opacity: 0;
      }
      to {
        opacity: 1;
      }
    }
    @keyframes logo-settle {
      from {
        transform: scale(0.97);
      }
      to {
        transform: none;
      }
    }
    /* Idle for the first 1000 ms (the streams grow), then one 550 ms ring. */
    @keyframes logo-ripple {
      0%,
      64.5% {
        opacity: 0;
        transform: scale(0.35);
      }
      65% {
        opacity: 0.4;
        transform: scale(0.35);
      }
      100% {
        opacity: 0;
        transform: scale(2.6);
      }
    }
    @keyframes logo-word {
      from {
        opacity: 0.2;
        clip-path: inset(0 100% 0 0);
      }
      to {
        opacity: 1;
        clip-path: inset(0 0 0 0);
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .reveal,
      .mark,
      .ripple,
      .word {
        animation: none !important;
      }
    }
  `,
})
export class BrandLogoAnimatedComponent {
  protected readonly motion = inject(MotionService);
  private readonly theme = inject(ThemeService);

  /** Mark height in px. */
  readonly size = input(72);
  /** Show the wordmark beside the mark. */
  readonly showWordmark = input(true);
  /** Changing this value plays the animation again. */
  readonly replay = input<unknown>(0);

  protected readonly markPath = LOGO_MARK_PATH;
  protected readonly wordmarkPaths = LOGO_WORDMARK_PATHS;
  protected readonly streams = STREAMS;
  protected readonly ripple = RIPPLE;
  protected readonly maskId = `rt-logo-mask-${nextLogoId++}`;
  protected readonly runs = signal(0);

  private firstReplay = true;

  protected readonly box = computed(() => (this.showWordmark() ? FULL_BOX : MARK_BOX));
  protected readonly viewBox = computed(() => {
    const b = this.box();
    return `${b.x} ${b.y} ${b.w} ${b.h}`;
  });
  protected readonly width = computed(() => (this.box().w * this.size()) / MARK_UNITS);
  protected readonly height = computed(() => (this.box().h * this.size()) / MARK_UNITS);
  protected readonly markFill = computed(() => LOGO_MARK_FILL[this.theme.resolved()]);
  protected readonly wordFill = computed(() => LOGO_WORDMARK_FILL[this.theme.resolved()]);
  constructor() {
    effect(() => {
      this.replay();
      if (this.firstReplay) {
        this.firstReplay = false;
        return;
      }
      this.runs.update((n) => n + 1);
    });
  }
}
