import { InjectionToken } from '@angular/core';

/** The navigator fields the platform is read from (a fake in specs). */
interface NavigatorLike {
  readonly platform?: string;
  readonly userAgent?: string;
  readonly userAgentData?: { readonly platform?: string };
}

/** Whether a navigator belongs to a Mac (WebKit's webview on macOS, or a browser on a Mac). */
export function detectMac(nav: NavigatorLike | undefined): boolean {
  const platform = nav?.userAgentData?.platform ?? nav?.platform ?? '';
  return /mac/i.test(platform) || /Macintosh|Mac OS X/.test(nav?.userAgent ?? '');
}

/**
 * The app runs on macOS: ⌘ instead of Ctrl in shortcut labels, the native traffic lights
 * instead of drawn window buttons, Finder and Trash instead of Explorer and Recycle Bin.
 */
export const IS_MAC = new InjectionToken<boolean>('riptide.isMac', {
  providedIn: 'root',
  factory: () => detectMac(globalThis.navigator as NavigatorLike | undefined),
});

/** macOS' symbols for the modifier keys. */
const MAC_KEYS: Readonly<Record<string, string>> = { Ctrl: '⌘', Alt: '⌥', Shift: '⇧' };

/** One key as the platform names it: `Ctrl` is `⌘` on a Mac, `Alt` is `⌥`, `Shift` is `⇧`. */
export function keyLabel(key: string, mac: boolean): string {
  return mac ? (MAC_KEYS[key] ?? key) : key;
}

/**
 * A shortcut written the Windows way (`Ctrl+Shift+N`, `Ctrl+,`, `Alt ↑`) as the platform shows
 * it: unchanged on Windows, `⇧⌘N` style on a Mac (modifiers first, in Apple's order).
 */
export function shortcutLabel(shortcut: string, mac: boolean): string {
  if (!mac) {
    return shortcut;
  }
  // `Ctrl+,` ends in a comma key: split only on the plus signs between keys
  const parts = shortcut.split(/\+(?=.)/);
  const order = ['Ctrl', 'Alt', 'Shift'];
  const mods = parts.filter((p) => order.includes(p)).sort((a, b) => macOrder(a) - macOrder(b));
  const rest = parts.filter((p) => !order.includes(p)).map((p) => p.replace(/^Alt /, '⌥ '));
  return mods.map((m) => MAC_KEYS[m]).join('') + rest.join('+');
}

/** Apple's modifier order: ⌥ before ⇧ before ⌘. */
function macOrder(key: string): number {
  return { Alt: 0, Shift: 1, Ctrl: 2 }[key] ?? 3;
}
