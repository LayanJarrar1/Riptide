import { ChangeDetectionStrategy, Component, input } from '@angular/core';

import { IconComponent, IconName } from './icon.component';

/** One step of a "how to start" list. */
export interface EmptyStep {
  readonly title: string;
  readonly body: string;
}

/**
 * An empty list is an invitation: a quiet icon (or, for a brand-new install, a small animated
 * picture of segments assembling into a bar), one sentence that says what the state means and
 * one that says what to do, optionally three numbered steps, and (projected) the action itself.
 */
@Component({
  selector: 'app-empty-state',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div class="empty">
      @if (illustration() === 'segments') {
        <div class="art motion-ambient" aria-hidden="true">
          <span class="art-track">
            <span class="art-piece" style="--i: 0; --y: -14px"><span class="art-fill"></span></span>
            <span class="art-piece" style="--i: 1; --y: 12px"><span class="art-fill"></span></span>
            <span class="art-piece" style="--i: 2; --y: -10px"><span class="art-fill"></span></span>
            <span class="art-piece" style="--i: 3; --y: 14px"><span class="art-fill"></span></span>
          </span>
        </div>
      } @else {
        <div class="glyph"><app-icon [name]="icon()" [size]="28" /></div>
      }
      <p class="heading">{{ heading() }}</p>
      @if (body(); as text) {
        <p class="body">{{ text }}</p>
      }
      @if (steps().length > 0) {
        <ol class="steps" [attr.aria-label]="stepsLabel()">
          @for (step of steps(); track $index; let i = $index) {
            <li class="step" [style.--i]="i">
              <span class="step-n num">{{ i + 1 }}</span>
              <span class="step-text">
                <span class="step-title">{{ step.title }}</span>
                <span class="step-body">{{ step.body }}</span>
              </span>
            </li>
          }
        </ol>
      }
      <div class="actions"><ng-content /></div>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      block-size: 100%;
      min-block-size: 240px;
      padding: var(--space-8);
      overflow: auto;
    }

    /* margin:auto centres when there is room and, unlike place-items, lets the top be scrolled to when there is not */
    .empty {
      margin: auto;
      display: flex;
      flex-direction: column;
      align-items: center;
      text-align: center;
      max-inline-size: 40ch;
      animation: empty-in var(--dur-3) var(--ease-emphasized-out) both;
    }

    @keyframes empty-in {
      from {
        opacity: 0;
        transform: translateY(6px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .glyph {
      display: grid;
      place-items: center;
      inline-size: 56px;
      block-size: 56px;
      border-radius: var(--radius-full);
      background: var(--color-surface-2);
      border: 1px solid var(--color-border);
      color: var(--color-text-muted);
    }

    /* ---- The picture: four segments fall into a track and fill, then start over ---------- */
    .art {
      display: grid;
      place-items: center;
      inline-size: 200px;
      block-size: 72px;
    }

    .art-track {
      position: relative;
      display: block;
      inline-size: 168px;
      block-size: 10px;
      border-radius: 5px;
      background: var(--flow-track);
    }

    .art-piece {
      position: absolute;
      inset-block: 0;
      inline-size: 25%;
      inset-inline-start: calc(var(--i) * 25%);
      padding-inline-end: var(--flow-gap);
      box-sizing: border-box;
      animation: art-settle 5200ms var(--ease-emphasized-out) infinite;
      animation-delay: calc(var(--i) * 260ms);
    }

    .art-fill {
      display: block;
      block-size: 100%;
      inline-size: 100%;
      border-radius: 3px;
      background: var(--flow-fill);
      transform-origin: var(--origin, left center);
      animation: art-fill 5200ms var(--ease-standard) infinite;
      animation-delay: calc(var(--i) * 260ms);
    }

    :host-context([dir='rtl']) .art-fill {
      --origin: right center;
    }

    @keyframes art-settle {
      0% {
        opacity: 0;
        transform: translateY(var(--y));
      }

      12%,
      88% {
        opacity: 1;
        transform: none;
      }

      100% {
        opacity: 0;
        transform: none;
      }
    }

    @keyframes art-fill {
      0%,
      14% {
        transform: scaleX(0);
        background: var(--flow-fill);
      }

      60% {
        transform: scaleX(1);
        background: var(--flow-fill);
      }

      68%,
      100% {
        transform: scaleX(1);
        background: var(--state-completed);
      }
    }

    .heading {
      margin-block-start: var(--space-4);
      font-size: var(--text-lg);
      font-weight: var(--weight-semibold);
    }

    .body {
      margin-block-start: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
    }

    /* ---- Steps ---------------------------------------------------------------------------- */
    .steps {
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
      inline-size: 100%;
      margin: var(--space-6) 0 0;
      padding: 0;
      list-style: none;
      text-align: start;
    }

    .step {
      display: flex;
      align-items: flex-start;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-4);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1);
      animation: step-in var(--dur-3) var(--ease-emphasized-out) both;
      animation-delay: calc(100ms + var(--i) * 60ms);
    }

    @keyframes step-in {
      from {
        opacity: 0;
        transform: translateY(6px);
      }

      to {
        opacity: 1;
        transform: none;
      }
    }

    .step-n {
      display: grid;
      place-items: center;
      flex-shrink: 0;
      inline-size: 22px;
      block-size: 22px;
      border-radius: var(--radius-full);
      background: var(--color-accent-soft);
      color: var(--color-accent-fg);
      font-size: var(--text-xs);
      font-weight: var(--weight-semibold);
    }

    .step-text {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-inline-size: 0;
    }

    .step-title {
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }

    .step-body {
      color: var(--color-text-secondary);
      font-size: var(--text-xs);
      line-height: var(--leading-normal);
    }

    .actions {
      display: flex;
      gap: var(--space-2);
      margin-block-start: var(--space-5);
    }

    .actions:empty {
      display: none;
    }

    /* short windows: smaller picture and tighter steps (titles only) so the action stays on screen */
    @media (max-height: 820px) {
      :host {
        padding: var(--space-4);
      }

      .art {
        block-size: 44px;
      }

      .heading {
        margin-block-start: var(--space-2);
      }

      .body {
        margin-block-start: var(--space-1);
      }

      .steps {
        margin-block-start: var(--space-3);
        gap: var(--space-2);
      }

      .step {
        align-items: center;
        padding: var(--space-2) var(--space-3);
      }

      .step-body {
        display: none;
      }

      .actions {
        margin-block-start: var(--space-3);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .art-piece,
      .art-fill {
        animation: none;
      }

      .art-fill {
        transform: scaleX(1);
        background: var(--state-completed);
      }

      .empty,
      .step {
        animation-duration: var(--dur-fade);
        animation-delay: 0ms;
        transform: none;
      }
    }
  `,
})
export class EmptyStateComponent {
  readonly icon = input<IconName>('inbox');
  readonly heading = input.required<string>();
  readonly body = input<string | null>(null);

  /** `segments` replaces the icon with the animated segments-into-a-bar picture. */
  readonly illustration = input<'segments' | null>(null);

  /** Numbered "how to start" steps shown under the body. */
  readonly steps = input<readonly EmptyStep[]>([]);

  /** Accessible name of the steps list. */
  readonly stepsLabel = input<string | null>(null);
}
