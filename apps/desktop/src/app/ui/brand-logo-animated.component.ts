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
import { I18nService } from '../i18n/i18n.service';
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
/** Vertical bands (brand-kit x ranges) that give each "stream" of the R its own drawing pass. */
const STREAMS = [
  { id: 'a', x: 125, w: 56 },
  { id: 'b', x: 181, w: 50 },
  { id: 'c', x: 231, w: 57 },
] as const;

/**
 * The Riptide logo for the Research start page. It plays once on first view: the three
 * streams of the R draw in (stroke-dash of the original outline), the fills fade in as the
 * strokes fade out, the mark settles from 0.96 to 1 and the wordmark slides in from the
 * inline-end. Only transform, opacity and stroke-dashoffset animate; paths and fills are the
 * brand kit's, so the resting state is the brand SVG. Reduced motion shows it at once.
 * The logo never mirrors in RTL; only the wordmark's slide direction follows reading order.
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
        [style.--slide]="slide()"
        role="img"
        aria-label="Riptide"
        xmlns="http://www.w3.org/2000/svg"
      >
        <defs>
          @for (s of streams; track s.id) {
            <clipPath [attr.id]="'rt-logo-' + s.id">
              <rect [attr.x]="s.x" y="40" [attr.width]="s.w" height="230" />
            </clipPath>
          }
        </defs>
        <g class="mark">
          <path class="fill" [attr.d]="markPath" [attr.fill]="markFill()" />
          @for (s of streams; track s.id; let i = $index) {
            <g [attr.clip-path]="'url(#rt-logo-' + s.id + ')'" aria-hidden="true">
              <path
                class="stroke"
                [style.--i]="i"
                [attr.d]="markPath"
                pathLength="1"
                fill="none"
                [attr.stroke]="markFill()"
                stroke-width="2.4"
                stroke-linejoin="round"
              />
            </g>
          }
        </g>
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
      /* Brand easing; timings: draw 900 ms staggered, fills 250 ms, settle 500 ms, wordmark 300 ms. */
      --logo-ease: cubic-bezier(0.2, 0.7, 0.2, 1);
    }
    .logo {
      display: block;
      overflow: visible;
    }
    .stroke {
      opacity: 0;
    }
    .play .stroke {
      opacity: 1;
      stroke-dasharray: 1;
      stroke-dashoffset: 1;
      animation:
        logo-draw 900ms var(--logo-ease) calc(var(--i) * 110ms) forwards,
        logo-stroke-out 250ms var(--logo-ease) 1000ms both;
    }
    .play .fill {
      animation: logo-fade-in 250ms var(--logo-ease) 1000ms backwards;
    }
    .play .mark {
      transform-box: fill-box;
      transform-origin: center;
      animation: logo-settle 500ms var(--logo-ease) 900ms backwards;
    }
    .play .word {
      animation: logo-slide 300ms var(--logo-ease) 1300ms backwards;
    }
    @keyframes logo-draw {
      to {
        stroke-dashoffset: 0;
      }
    }
    @keyframes logo-stroke-out {
      to {
        opacity: 0;
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
        transform: scale(0.96);
      }
      to {
        transform: none;
      }
    }
    @keyframes logo-slide {
      from {
        opacity: 0;
        transform: translateX(var(--slide));
      }
      to {
        opacity: 1;
        transform: none;
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .stroke,
      .fill,
      .mark,
      .word {
        animation: none !important;
      }
      .stroke {
        opacity: 0 !important;
      }
    }
  `,
})
export class BrandLogoAnimatedComponent {
  protected readonly motion = inject(MotionService);
  private readonly theme = inject(ThemeService);
  private readonly i18n = inject(I18nService);

  /** Mark height in px. */
  readonly size = input(72);
  /** Show the wordmark beside the mark. */
  readonly showWordmark = input(true);
  /** Changing this value plays the animation again. */
  readonly replay = input<unknown>(0);

  protected readonly markPath = LOGO_MARK_PATH;
  protected readonly wordmarkPaths = LOGO_WORDMARK_PATHS;
  protected readonly streams = STREAMS;
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
  /** The wordmark starts 8 px (in logo units) toward the inline-end: right in LTR, left in RTL. */
  protected readonly slide = computed(() => {
    const px = (8 * MARK_UNITS) / this.size();
    return `${this.i18n.dir() === 'rtl' ? -px : px}px`;
  });

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
