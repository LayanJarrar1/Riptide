import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

/**
 * Inline icon set, 24-unit grid, stroke only, drawn to match Inter's weight at 1.75px.
 * Keeping the paths here (not an icon font or a CDN) means they ship with the app, pick up
 * `currentcolor`, and can be mirrored in RTL per icon.
 */
const PATHS: Readonly<Record<string, string>> = {
  download: 'M12 4v10m0 0 4-4m-4 4-4-4M5 19h14',
  plus: 'M12 5v14M5 12h14',
  pause: 'M8 6v12M16 6v12',
  play: 'M8 5.5v13l10-6.5z',
  stop: 'M7 7h10v10H7z',
  retry: 'M4 12a8 8 0 0 1 13.7-5.6M20 4v5h-5M20 12a8 8 0 0 1-13.7 5.6M4 20v-5h5',
  close: 'M6 6l12 12M18 6 6 18',
  check: 'M5 12.5 10 17l9-10',
  trash: 'M5 7h14M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
  folder: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z',
  folderOpen: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1H7l-3 9',
  link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
  copy: 'M9 9h10v11H9zM5 15V4h10',
  search: 'M11 4a7 7 0 1 1 0 14 7 7 0 0 1 0-14zM20 20l-4-4',
  settings: 'M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h0a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v0a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
  more: 'M6 12h.01M12 12h.01M18 12h.01',
  chevronDown: 'M6 9l6 6 6-6',
  chevronEnd: 'M9 6l6 6-6 6',
  chevronStart: 'M15 6l-6 6 6 6',
  arrowUp: 'M12 19V5m0 0-6 6m6-6 6 6',
  arrowDown: 'M12 5v14m0 0 6-6m-6 6-6-6',
  alert: 'M12 9v4m0 4h.01M10.3 3.9 2.5 18a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z',
  info: 'M12 16v-4m0-4h.01M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z',
  clock: 'M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0z',
  shield: 'M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z',
  shieldCheck: 'M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6zM9 12l2 2 4-4',
  globe: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3z',
  layers: 'M12 4l9 5-9 5-9-5zM3 14l9 5 9-5',
  list: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01',
  inbox: 'M3 13l2.5-8h13L21 13v6H3zM3 13h5l1.5 2h5L16 13h5',
  bolt: 'M13 2 4 14h7l-1 8 9-12h-7z',
  wave: 'M3 12c2-3 4-3 6 0s4 3 6 0 4-3 6 0',
  sun: 'M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4',
  moon: 'M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z',
  monitor: 'M3 5h18v11H3zM9 20h6M12 16v4',
  language: 'M4 5h10M9 3v2m2 11-5-5m0 0-3 5m5-6c-1 3-3 5-6 7M14 20l4-9 4 9m-7-3h6',
  external: 'M14 4h6v6M20 4l-9 9M19 14v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5',
  keyboard: 'M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M7 14h10',
  // file types
  fileArchive: 'M6 3h9l5 5v13H6zM15 3v5h5M11 7h2M11 10h2M11 13h2M11 16h2',
  fileDoc: 'M6 3h9l5 5v13H6zM15 3v5h5M9 13h6M9 17h6',
  fileMusic: 'M6 3h9l5 5v13H6zM15 3v5h5M11 17a2 2 0 1 0 0-4 2 2 0 0 0 0 4zm2-2V9l3 1',
  fileVideo: 'M6 3h9l5 5v13H6zM15 3v5h5M10 12l5 2.5-5 2.5z',
  fileApp: 'M6 3h9l5 5v13H6zM15 3v5h5M9 12h2v2H9zM13 12h2v2h-2zM9 16h2v2H9zM13 16h2v2h-2z',
  fileImage: 'M6 3h9l5 5v13H6zM15 3v5h5M8 19l4-5 3 3 2-2 2 4M9.5 13a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z',
  file: 'M6 3h9l5 5v13H6zM15 3v5h5',
  magnet: 'M6 3v8a6 6 0 0 0 12 0V3h-4v8a2 2 0 0 1-4 0V3zM6 7h4M14 7h4',
  eye: 'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  sparkles: 'M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z',
  edit: 'M4 20h4L19 9l-4-4L4 16zM13 7l4 4',
  book: 'M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2zM4 19V5M8 7h7M8 11h5',
  code: 'M8 7l-5 5 5 5M16 7l5 5-5 5M14 4l-4 16',
  panelStart: 'M3 5h18v14H3zM9 5v14',
  sidebar: 'M3 5h18v14H3zM9 5v14M5.2 9h1.6M5.2 12h1.6M5.2 15h1.6',
};

/** Icon names available to templates. */
export type IconName = keyof typeof PATHS;

/** Icons that point along the reading direction and must mirror in RTL. */
const DIRECTIONAL = new Set<IconName>(['chevronEnd', 'chevronStart', 'external', 'folderOpen', 'panelStart', 'sidebar']);

@Component({
  selector: 'app-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <svg
      viewBox="0 0 24 24"
      [attr.width]="size()"
      [attr.height]="size()"
      [class.icon-directional]="directional()"
      [class.is-drawing]="draw()"
      aria-hidden="true"
      focusable="false"
    >
      <path [attr.d]="path()" pathLength="1" />
    </svg>
  `,
  styles: `
    :host {
      display: inline-flex;
      flex-shrink: 0;
      line-height: 0;
      color: inherit;
    }

    svg {
      fill: none;
      stroke: currentcolor;
      stroke-width: 1.75;
      stroke-linecap: round;
      stroke-linejoin: round;
    }

    /* The stroke draws itself from start to end (pathLength="1" normalises every glyph). */
    svg.is-drawing path {
      stroke-dasharray: 1;
      stroke-dashoffset: 1;
      animation: icon-draw var(--dur-4) var(--ease-emphasized-out) forwards;
    }

    @keyframes icon-draw {
      to {
        stroke-dashoffset: 0;
      }
    }
  `,
})
export class IconComponent {
  /** Which glyph. */
  readonly name = input.required<IconName>();

  /** Rendered size in CSS pixels. */
  readonly size = input(18);

  /** Plays the stroke draw-in once when set (the status pill uses it for the check mark). */
  readonly draw = input(false);

  protected readonly path = computed(() => PATHS[this.name()] ?? PATHS['file']);

  protected readonly directional = computed(() => DIRECTIONAL.has(this.name()));
}
