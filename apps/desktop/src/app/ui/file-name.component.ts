import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

const RTL = /[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFC]/u;
const STRONG = /\p{L}/u;

/** Base direction of a name: the first strong letter decides, like `dir="auto"`. */
export function nameDirection(name: string): 'ltr' | 'rtl' {
  for (const ch of name) {
    if (RTL.test(ch)) return 'rtl';
    if (STRONG.test(ch)) return 'ltr';
  }
  return 'ltr';
}

/** Splits `report.pdf` into `['report', '.pdf']`; dotfiles and odd suffixes stay whole. */
export function splitExtension(name: string): [string, string] {
  const i = name.lastIndexOf('.');
  if (i <= 0 || name.length - i > 12 || /\s/.test(name.slice(i))) return [name, ''];
  return [name.slice(0, i), name.slice(i)];
}

/**
 * A file name that reads right in either UI language. The name takes its own direction from its
 * first letter (so an Arabic name inside the LTR list reads right to left), the extension is a
 * separate left-to-right isolate that stays visible, and when the name is too long only the stem
 * is cut, with the ellipsis at its logical end.
 */
@Component({
  selector: 'app-file-name',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[attr.dir]': 'dir()', '[attr.title]': 'tip()' },
  template: `<span class="stem">{{ parts()[0] }}</span>@if (parts()[1]) {<span class="ext" dir="ltr">{{ parts()[1] }}</span>}`,
  styles: `
    :host {
      display: flex;
      min-inline-size: 0;
      max-inline-size: 100%;
      white-space: nowrap;
      unicode-bidi: isolate;
    }

    .stem {
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      unicode-bidi: isolate;
    }

    .ext {
      flex: none;
      unicode-bidi: isolate;
    }
  `,
})
export class FileNameComponent {
  /** The file name as the server or disk gave it. */
  readonly name = input.required<string>();
  /** Tooltip; defaults to the full name. */
  readonly label = input<string | null>(null);

  protected readonly dir = computed(() => nameDirection(this.name()));
  protected readonly parts = computed(() => splitExtension(this.name()));
  protected readonly tip = computed(() => this.label() ?? this.name());
}
