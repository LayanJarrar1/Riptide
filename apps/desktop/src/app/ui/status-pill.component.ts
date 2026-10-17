import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  inject,
  input,
  signal,
  untracked,
} from '@angular/core';

import { I18nService } from '../i18n/i18n.service';
import { DownloadState, FailureKind } from '../model/download';
import { IconComponent, IconName } from './icon.component';

const ICONS: Readonly<Record<DownloadState, IconName>> = {
  queued: 'clock',
  probing: 'globe',
  downloading: 'download',
  paused: 'pause',
  verifying: 'shield',
  completed: 'check',
  failed: 'alert',
  cancelled: 'close',
};

/** How the pill reacts to a state change: a pop, the check drawing in, or a single shake. */
type Morph = 'none' | 'pop' | 'draw' | 'shake';

/**
 * State badge: hue + icon + translated label, so the state is never carried by colour alone.
 * For failures the label names the cause ("Link expired", "Checksum mismatch").
 *
 * On a state change the pill morphs: the label crossfades and the badge pops; arriving at
 * `completed` draws the check mark in; arriving at `failed` shakes the pill once (only the
 * pill, never the row). Nothing plays on first render, so a scrolled-in row stays still.
 */
@Component({
  selector: 'app-status-pill',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  host: {
    '[attr.data-state]': 'state()',
    '[class.is-compact]': 'compact()',
    '[class.m-pop]': 'morph() === "pop"',
    '[class.m-draw]': 'morph() === "draw"',
    '[class.m-shake]': 'morph() === "shake"',
    '(animationend)': 'onAnimationEnd($event)',
  },
  template: `
    <app-icon [name]="icon()" [size]="compact() ? 13 : 14" [draw]="morph() === 'draw'" />
    <span class="label">{{ label() }}</span>
  `,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      max-inline-size: 100%;
      block-size: 24px;
      padding-inline: 8px 10px;
      border-radius: var(--radius-full);
      background: var(--pill-soft, var(--color-surface-3));
      color: var(--pill-fg, var(--color-text-secondary));
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
      line-height: 1;
      white-space: nowrap;
      transition:
        background-color var(--dur-2) var(--ease-standard),
        color var(--dur-2) var(--ease-standard);
    }

    :host(.is-compact) {
      block-size: 20px;
      padding-inline: 6px 8px;
    }

    /* In a narrow column the label shortens rather than spilling over the next cell. */
    .label {
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    :host([data-state='downloading']),
    :host([data-state='probing']) {
      --pill-soft: var(--state-downloading-soft);
      --pill-fg: var(--color-accent-fg);
    }

    :host([data-state='queued']) {
      --pill-soft: var(--state-queued-soft);
      --pill-fg: var(--state-queued);
    }

    :host([data-state='paused']) {
      --pill-soft: var(--state-paused-soft);
      --pill-fg: var(--state-paused);
    }

    :host([data-state='verifying']) {
      --pill-soft: var(--state-verifying-soft);
      --pill-fg: var(--state-verifying);
    }

    :host([data-state='completed']) {
      --pill-soft: var(--state-completed-soft);
      --pill-fg: var(--state-completed);
    }

    :host([data-state='failed']) {
      --pill-soft: var(--state-failed-soft);
      --pill-fg: var(--state-failed);
    }

    :host([data-state='cancelled']) {
      --pill-soft: var(--state-cancelled-soft);
      --pill-fg: var(--state-cancelled);
    }

    :host([data-state='verifying']) app-icon {
      animation: breathe 1200ms var(--ease-standard) infinite alternate;
    }

    @keyframes breathe {
      from {
        opacity: 0.55;
      }

      to {
        opacity: 1;
      }
    }

    /* ---- Morphs ---------------------------------------------------------------------- */
    :host(.m-pop) {
      animation: pill-pop var(--dur-3) var(--ease-overshoot) both;
    }

    :host(.m-draw) {
      animation: pill-pop var(--dur-4) var(--ease-overshoot) both;
    }

    :host(.m-shake) {
      animation: pill-shake var(--dur-3) var(--ease-standard) both;
    }

    :host(.m-pop) .label,
    :host(.m-draw) .label,
    :host(.m-shake) .label {
      animation: label-in var(--dur-2) var(--ease-emphasized-out) both;
    }

    @keyframes pill-pop {
      from {
        transform: scale(0.9);
      }

      to {
        transform: scale(1);
      }
    }

    @keyframes pill-shake {
      0%,
      100% {
        transform: translateX(0);
      }

      25% {
        transform: translateX(-3px);
      }

      55% {
        transform: translateX(3px);
      }

      80% {
        transform: translateX(-1px);
      }
    }

    @keyframes label-in {
      from {
        opacity: 0;
        transform: translateY(3px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    @media (prefers-reduced-motion: reduce) {
      :host([data-state='verifying']) app-icon {
        animation: none;
      }

      :host(.m-shake) {
        animation: none;
      }

      :host(.m-pop) .label,
      :host(.m-draw) .label,
      :host(.m-shake) .label {
        animation-duration: var(--dur-fade);
        transform: none;
      }
    }
  `,
})
export class StatusPillComponent {
  private readonly i18n = inject(I18nService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;

  readonly state = input.required<DownloadState>();

  /** Names the cause for failed downloads. */
  readonly failure = input<FailureKind | null>(null);

  /** 20px tall for dense rows. */
  readonly compact = input(false);

  protected readonly icon = computed(() => ICONS[this.state()]);

  /** The morph currently playing; cleared on `animationend`. */
  protected readonly morph = signal<Morph>('none');

  private previous: DownloadState | null = null;

  protected readonly label = computed(() => {
    const state = this.state();
    const failure = this.failure();
    if (state === 'failed' && failure !== null) {
      return this.i18n.t(`failure.${failure}`);
    }
    return this.i18n.t(`state.${state}`);
  });

  constructor() {
    effect((onCleanup) => {
      const state = this.state();
      const previous = this.previous;
      this.previous = state;
      if (previous === null || previous === state) {
        return;
      }
      const kind: Morph = state === 'completed' ? 'draw' : state === 'failed' ? 'shake' : 'pop';
      // drop the class first so a repeat of the same morph restarts its animation
      untracked(() => this.morph.set('none'));
      const handle = setTimeout(() => this.morph.set(kind), 0);
      onCleanup(() => clearTimeout(handle));
    });
  }

  protected onAnimationEnd(event: AnimationEvent): void {
    // the icon's own draw-in bubbles up too; only the pill's animation ends the morph
    if (event.target === this.host) {
      this.morph.set('none');
    }
  }
}
