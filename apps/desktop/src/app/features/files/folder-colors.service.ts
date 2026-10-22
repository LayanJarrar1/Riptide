import { inject, Injectable, signal } from '@angular/core';

import { PREFERENCE_STORE } from '../../core/preferences';

/** Preset folder colours; each maps to `--folder-<key>` in the file manager's styles. */
export const FOLDER_COLORS = ['red', 'orange', 'amber', 'green', 'teal', 'blue', 'violet', 'pink', 'slate'] as const;
export type FolderColor = (typeof FOLDER_COLORS)[number];

/** Preference key; the colours are Riptide's own and never touch Windows' folder icons. */
export const FOLDER_COLORS_KEY = 'riptide.folderColors';

/** Paths compare case-insensitively (Windows) and without a trailing separator. */
function keyOf(path: string): string {
  return path.replace(/[\\/]+$/, '').toLowerCase();
}

/**
 * Colours the user gave folders in the file manager, by path, kept in Riptide's preferences.
 * A renamed or moved folder keeps its colour (see `move`).
 */
@Injectable({ providedIn: 'root' })
export class FolderColorsService {
  private readonly store = inject(PREFERENCE_STORE);
  private readonly colors = signal<Readonly<Record<string, FolderColor>>>(this.load());

  /** The colour of a folder, `null` for the default. */
  colorOf(path: string): FolderColor | null {
    return this.colors()[keyOf(path)] ?? null;
  }

  set(path: string, color: FolderColor | null): void {
    const next = { ...this.colors() };
    if (color === null) {
      delete next[keyOf(path)];
    } else {
      next[keyOf(path)] = color;
    }
    this.save(next);
  }

  /** A folder was renamed or moved: its colour, and its subfolders' colours, follow it. */
  move(from: string, to: string): void {
    const src = keyOf(from);
    const dst = keyOf(to);
    const current = this.colors();
    let changed = false;
    const next: Record<string, FolderColor> = {};
    for (const [k, v] of Object.entries(current)) {
      if (k === src || k.startsWith(src + '\\') || k.startsWith(src + '/')) {
        next[dst + k.slice(src.length)] = v;
        changed = true;
      } else {
        next[k] = v;
      }
    }
    if (changed) {
      this.save(next);
    }
  }

  private load(): Record<string, FolderColor> {
    try {
      const parsed: unknown = JSON.parse(this.store.get(FOLDER_COLORS_KEY) ?? '{}');
      if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
        return {};
      }
      const out: Record<string, FolderColor> = {};
      for (const [k, v] of Object.entries(parsed)) {
        if ((FOLDER_COLORS as readonly string[]).includes(v as string)) {
          out[keyOf(k)] = v as FolderColor;
        }
      }
      return out;
    } catch {
      return {};
    }
  }

  private save(next: Record<string, FolderColor>): void {
    this.colors.set(next);
    this.store.set(FOLDER_COLORS_KEY, JSON.stringify(next));
  }
}
