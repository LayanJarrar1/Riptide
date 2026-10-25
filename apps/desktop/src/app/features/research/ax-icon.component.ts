import { ChangeDetectionStrategy, Component, input } from '@angular/core';

/** The glyphs of the chat area (the same drawings the Astryx chat components use). */
export type AxIconName =
  | 'sparkle'
  | 'cpu'
  | 'arrowUp'
  | 'stop'
  | 'chevronDown'
  | 'x'
  | 'wrench'
  | 'checkCircle'
  | 'xCircle'
  | 'minusCircle'
  | 'stopCircle'
  | 'alert'
  | 'check'
  | 'copy'
  | 'spinner';

/**
 * One icon of the chat area: stroke only, `currentColor`, no filters. The Heroicons-style glyphs
 * (sparkle, cpu) are drawn at 1.5 stroke and the Lucide-style ones at 2, exactly as Astryx does.
 */
@Component({
  selector: 'app-ax-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true', '[class.is-flip]': 'flip()', '[style.inline-size.px]': 'size()', '[style.block-size.px]': 'size()' },
  template: `
    @switch (name()) {
      @case ('spinner') {
        <svg [attr.width]="size()" [attr.height]="size()" class="spin-svg">
          <circle cx="50%" cy="50%" r="5" stroke-width="2" fill="none" stroke="currentColor" opacity="0.25" />
          <circle class="arc" cx="50%" cy="50%" r="5" stroke-width="2" fill="none" stroke="currentColor" stroke-linecap="round" stroke-dasharray="11.780972450961723 19.634954084936208" />
        </svg>
      }
      @case ('sparkle') {
        <svg [attr.width]="size()" [attr.height]="size()" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904 9 18.75l-.813-2.846a4.5 4.5 0 0 0-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 0 0 3.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 0 0 3.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 0 0-3.09 3.09ZM18.259 8.715 18 9.75l-.259-1.035a3.375 3.375 0 0 0-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 0 0 2.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 0 0 2.456 2.456L21.75 6l-1.035.259a3.375 3.375 0 0 0-2.456 2.456ZM16.894 20.567 16.5 21.75l-.394-1.183a2.25 2.25 0 0 0-1.423-1.423L13.5 18.75l1.183-.394a2.25 2.25 0 0 0 1.423-1.423l.394-1.183.394 1.183a2.25 2.25 0 0 0 1.423 1.423l1.183.394-1.183.394a2.25 2.25 0 0 0-1.423 1.423Z" />
        </svg>
      }
      @case ('cpu') {
        <svg [attr.width]="size()" [attr.height]="size()" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 0 0 2.25-2.25V6.75a2.25 2.25 0 0 0-2.25-2.25H6.75A2.25 2.25 0 0 0 4.5 6.75v10.5a2.25 2.25 0 0 0 2.25 2.25Zm.75-12h9v9h-9v-9Z" />
        </svg>
      }
      @default {
        <svg [attr.width]="size()" [attr.height]="size()" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          @switch (name()) {
            @case ('arrowUp') {
              <path d="m5 12 7-7 7 7" />
              <path d="M12 19V5" />
            }
            @case ('stop') {
              <rect width="18" height="18" x="3" y="3" rx="2" />
            }
            @case ('chevronDown') {
              <path d="m6 9 6 6 6-6" />
            }
            @case ('x') {
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            }
            @case ('wrench') {
              <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z" />
            }
            @case ('checkCircle') {
              <path d="M21.801 10A10 10 0 1 1 17 3.335" />
              <path d="m9 11 3 3L22 4" />
            }
            @case ('xCircle') {
              <circle cx="12" cy="12" r="10" />
              <path d="m15 9-6 6" />
              <path d="m9 9 6 6" />
            }
            @case ('minusCircle') {
              <circle cx="12" cy="12" r="10" />
              <path d="M8 12h8" />
            }
            @case ('stopCircle') {
              <circle cx="12" cy="12" r="10" />
              <rect x="9" y="9" width="6" height="6" rx="1" />
            }
            @case ('alert') {
              <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" />
              <path d="M12 9v4" />
              <path d="M12 17h.01" />
            }
            @case ('check') {
              <path d="M20 6 9 17l-5-5" />
            }
            @case ('copy') {
              <rect x="8" y="8" width="14" height="14" rx="2" />
              <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
            }
          }
        </svg>
      }
    }
  `,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      flex: none;
    }

    svg {
      display: block;
    }

    :host(.is-flip) {
      transform: scaleX(var(--dir-sign, 1));
    }

    .arc {
      transform-origin: center;
      transform-box: fill-box;
      animation: ax-spin 0.9s linear infinite;
    }

    :host-context(.is-still) .arc {
      animation: none;
    }

    @keyframes ax-spin {
      to {
        transform: rotate(360deg);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .arc {
        animation-duration: 3s;
      }
    }
  `,
})
export class AxIconComponent {
  readonly name = input.required<AxIconName>();
  readonly size = input(16);
  /** Mirror in right-to-left layouts (icons that imply a direction). */
  readonly flip = input(false);
}
