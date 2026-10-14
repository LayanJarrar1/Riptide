import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';

import { FormatService } from '../core/format.service';
import { I18nService } from '../i18n/i18n.service';

/**
 * Speed over the last minute: one series, a 2px line over a faint area, baseline at zero,
 * no axes or gridlines (the surrounding text names the measure and the latest value is
 * printed next to it). Hovering shows a crosshair and the value at that second.
 *
 * `compact` renders the 24px status-bar version with the tooltip disabled; the detail
 * version is 72px tall and interactive.
 */
@Component({
  selector: 'app-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.is-compact]': 'compact()',
    '[attr.title]': 'compact() ? summary() : null',
  },
  template: `
    <svg
      [attr.viewBox]="'0 0 ' + width + ' ' + height()"
      preserveAspectRatio="none"
      [attr.aria-label]="summary()"
      role="img"
      (pointermove)="onMove($event)"
      (pointerleave)="hover.set(null)"
    >
      <defs>
        <linearGradient [attr.id]="gradientId" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="var(--color-accent)" stop-opacity="0.28" />
          <stop offset="1" stop-color="var(--color-accent)" stop-opacity="0" />
        </linearGradient>
      </defs>
      <path class="area" [attr.d]="areaPath()" [attr.fill]="'url(#' + gradientId + ')'" />
      <path class="line" [attr.d]="linePath()" />
      @if (hover(); as h) {
        <line class="crosshair" [attr.x1]="h.x" [attr.x2]="h.x" y1="0" [attr.y2]="height()" />
        <circle class="dot" [attr.cx]="h.x" [attr.cy]="h.y" r="3.5" />
      }
    </svg>
    @if (!compact() && hover(); as h) {
      <div class="tip" [style.inset-inline-start.%]="h.pct" [class.flip]="h.pct > 70">
        <span class="tip-value num">{{ h.label }}</span>
        <span class="tip-when">{{ h.when }}</span>
      </div>
    }
  `,
  styles: `
    :host {
      position: relative;
      display: block;
      inline-size: 100%;
      block-size: 72px;
    }

    :host(.is-compact) {
      block-size: 24px;
      inline-size: 96px;
    }

    svg {
      display: block;
      inline-size: 100%;
      block-size: 100%;
      overflow: visible;
      touch-action: none;
    }

    .line {
      fill: none;
      stroke: var(--color-accent);
      stroke-width: 2;
      stroke-linejoin: round;
      stroke-linecap: round;
      vector-effect: non-scaling-stroke;
    }

    .crosshair {
      stroke: var(--color-border-strong);
      stroke-width: 1;
      vector-effect: non-scaling-stroke;
    }

    .dot {
      fill: var(--color-accent);
      stroke: var(--color-surface-1);
      stroke-width: 2;
      vector-effect: non-scaling-stroke;
    }

    .tip {
      position: absolute;
      inset-block-start: -6px;
      display: flex;
      flex-direction: column;
      gap: 2px;
      padding: 6px 8px;
      border: 1px solid var(--color-border);
      border-radius: var(--radius-sm);
      background: var(--color-surface-2);
      box-shadow: var(--shadow-2);
      font-size: var(--text-xs);
      line-height: 1.2;
      pointer-events: none;
      transform: translate(calc(var(--dir-sign) * 10px), -100%);
      white-space: nowrap;
      z-index: var(--z-raised);
    }

    .tip.flip {
      transform: translate(calc(var(--dir-sign) * (-100% - 10px)), -100%);
    }

    .tip-value {
      font-weight: var(--weight-semibold);
      color: var(--color-text);
    }

    .tip-when {
      color: var(--color-text-muted);
    }
  `,
})
export class SparklineComponent {
  private static nextId = 0;

  private readonly format = inject(FormatService);
  private readonly i18n = inject(I18nService);

  /** Samples oldest → newest, one per second. */
  readonly samples = input.required<readonly number[]>();

  /** Status-bar size, no tooltip. */
  readonly compact = input(false);

  protected readonly width = 240;
  protected readonly height = computed(() => (this.compact() ? 24 : 72));
  protected readonly gradientId = `spark-${SparklineComponent.nextId++}`;

  protected readonly hover = signal<{ x: number; y: number; pct: number; label: string; when: string } | null>(null);

  private readonly points = computed<readonly { x: number; y: number }[]>(() => {
    const s = this.samples();
    if (s.length === 0) {
      return [];
    }
    const max = Math.max(1, ...s) * 1.1;
    const h = this.height();
    const pad = 3;
    const stepX = s.length > 1 ? this.width / (s.length - 1) : 0;
    return s.map((v, i) => ({ x: i * stepX, y: h - pad - (v / max) * (h - pad * 2) }));
  });

  protected readonly linePath = computed(() => {
    const p = this.points();
    if (p.length === 0) {
      return '';
    }
    return p.map((pt, i) => `${i === 0 ? 'M' : 'L'}${pt.x.toFixed(1)} ${pt.y.toFixed(1)}`).join(' ');
  });

  protected readonly areaPath = computed(() => {
    const p = this.points();
    if (p.length === 0) {
      return '';
    }
    const h = this.height();
    return `${this.linePath()} L${p[p.length - 1].x.toFixed(1)} ${h} L0 ${h} Z`;
  });

  /** Accessible name and compact tooltip: current, peak. */
  protected readonly summary = computed(() => {
    const s = this.samples();
    const now = s.length > 0 ? s[s.length - 1] : 0;
    const peak = s.length > 0 ? Math.max(...s) : 0;
    return this.i18n
      .t('chart.speedSummary')
      .replace('{now}', this.format.speed(now))
      .replace('{peak}', this.format.speed(peak));
  });

  protected onMove(event: PointerEvent): void {
    if (this.compact()) {
      return;
    }
    const svg = event.currentTarget as SVGSVGElement;
    const rect = svg.getBoundingClientRect();
    if (rect.width === 0) {
      return;
    }
    const rtl = getComputedStyle(svg).direction === 'rtl';
    let frac = (event.clientX - rect.left) / rect.width;
    frac = Math.min(1, Math.max(0, frac));
    const p = this.points();
    if (p.length === 0) {
      return;
    }
    // the SVG is not mirrored in RTL (time still runs left→right), so only the tooltip side flips
    const index = Math.round(frac * (p.length - 1));
    const secondsAgo = p.length - 1 - index;
    const when =
      secondsAgo === 0
        ? this.i18n.t('chart.now')
        : this.i18n.t('chart.secondsAgo').replace('{n}', this.format.int(secondsAgo));
    this.hover.set({
      x: p[index].x,
      y: p[index].y,
      pct: rtl ? 100 - frac * 100 : frac * 100,
      label: this.format.speed(this.samples()[index]),
      when,
    });
  }
}
