import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';

import { FormatService } from '../core/format.service';
import { I18nService } from '../i18n/i18n.service';
import { Segment } from '../model/download';

interface Cell {
  readonly key: number;
  readonly left: number;
  readonly width: number;
  readonly fill: number;
  readonly state: Segment['state'];
  readonly title: string;
}

/**
 * The details-drawer view of the segments: the same geometry as the flow bar, taller, with a
 * tooltip per segment (`#3 · 120 MB of 256 MB · active`) and a connections readout. It is the
 * one place the product explains itself: every stripe is a live connection.
 */
@Component({
  selector: 'app-segment-map',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="map" role="list" [attr.aria-label]="i18n.t('details.segments')">
      @for (c of cells(); track c.key) {
        <div
          role="listitem"
          class="cell"
          [class.c-active]="c.state === 'active'"
          [class.c-done]="c.state === 'done'"
          [class.c-failed]="c.state === 'failed'"
          [style.inset-inline-start.%]="c.left"
          [style.inline-size.%]="c.width"
          [attr.title]="c.title"
        >
          <span class="cell-fill" [style.transform]="'scaleX(' + c.fill + ')'"></span>
        </div>
      }
    </div>
  `,
  styles: `
    :host {
      display: block;
      inline-size: 100%;
    }

    .map {
      position: relative;
      block-size: 28px;
      border-radius: var(--radius-sm);
      background: var(--flow-track);
      overflow: hidden;
    }

    .cell {
      position: absolute;
      inset-block: 0;
      padding-inline-end: var(--flow-gap);
      box-sizing: border-box;
    }

    .cell-fill {
      display: block;
      block-size: 100%;
      inline-size: 100%;
      background: var(--flow-fill);
      transform-origin: var(--origin, left center);
      transition: transform 260ms linear;
      opacity: 0.9;
    }

    :host-context([dir='rtl']) .cell-fill {
      --origin: right center;
    }

    .c-done .cell-fill {
      background: var(--flow-fill-done);
      opacity: 1;
    }

    .c-failed .cell-fill {
      background: var(--flow-fill-failed);
    }

    .cell:hover .cell-fill {
      filter: brightness(1.15);
    }

    @media (prefers-reduced-motion: reduce) {
      .cell-fill {
        transition: none;
      }
    }
  `,
})
export class SegmentMapComponent {
  protected readonly i18n = inject(I18nService);
  private readonly format = inject(FormatService);

  readonly segments = input.required<readonly Segment[]>();
  readonly size = input.required<number | null>();

  protected readonly cells = computed<readonly Cell[]>(() => {
    const size = this.size();
    if (size === null || size <= 0) {
      return [];
    }
    return [...this.segments()]
      .filter((s) => s.end > s.start)
      .sort((a, b) => a.start - b.start)
      .map((s) => {
        const len = s.end - s.start;
        const title = this.i18n
          .t('details.segmentTitle')
          .replace('{n}', this.format.int(s.index + 1))
          .replace('{written}', this.format.bytes(s.written))
          .replace('{total}', this.format.bytes(len))
          .replace('{state}', this.i18n.t(`segment.${s.state}`));
        return {
          key: s.index,
          left: (s.start / size) * 100,
          width: (len / size) * 100,
          fill: Math.min(1, s.written / len),
          state: s.state,
          title,
        };
      });
  });
}
