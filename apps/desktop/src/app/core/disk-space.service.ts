import { computed, DestroyRef, effect, inject, Injectable, InjectionToken, signal } from '@angular/core';

import { SettingsService } from '../features/settings/settings.service';
import { TAURI_AVAILABLE, TAURI_INVOKE } from '../tauri/tauri-bridge';

/** Free space shown in a plain browser, where there is no disk to ask. */
export const DEMO_FREE_BYTES = 142 * 1024 * 1024 * 1024;

/** How often the default folder's free space is re-read under Tauri. */
export const DISK_POLL_INTERVAL = new InjectionToken<number>('riptide.disk.pollInterval', {
  providedIn: 'root',
  factory: () => 30_000,
});

/**
 * Free disk space for the download folders. Under Tauri the `disk_free` command is asked for the
 * default folder every 30 s and whenever that folder changes; `freeFor` asks for any other folder
 * (the add dialog's "Save to"). In a plain browser the demo figure is reported and nothing is
 * polled.
 */
@Injectable({ providedIn: 'root' })
export class DiskSpaceService {
  private readonly tauri = inject(TAURI_AVAILABLE);
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly settings = inject(SettingsService);
  private readonly interval = inject(DISK_POLL_INTERVAL);

  private readonly free = signal<number | null>(this.tauri ? null : DEMO_FREE_BYTES);
  private timer: ReturnType<typeof setInterval> | null = null;

  /** Free bytes on the default download folder's volume; `null` until the first answer. */
  readonly freeBytes = this.free.asReadonly();

  /** The volume the default folder is on: a drive letter (`C:`) on Windows, else the folder. */
  readonly volume = computed(() => volumeOf(this.settings.settings().defaultDir));

  constructor() {
    if (!this.tauri) {
      return;
    }
    effect(() => {
      const dir = this.settings.settings().defaultDir;
      void this.refresh(dir);
    });
    this.timer = setInterval(() => void this.refresh(this.settings.settings().defaultDir), this.interval);
    inject(DestroyRef).onDestroy(() => {
      if (this.timer !== null) {
        clearInterval(this.timer);
        this.timer = null;
      }
    });
  }

  /** Free bytes on the volume of `dir`; `null` when it cannot be read (or in a plain browser, the demo figure). */
  async freeFor(dir: string): Promise<number | null> {
    if (!this.tauri) {
      return DEMO_FREE_BYTES;
    }
    if (dir.trim().length === 0) {
      return null;
    }
    try {
      const n = await this.invoke<number>('disk_free', { path: dir });
      return Number.isFinite(n) && n >= 0 ? n : null;
    } catch (error: unknown) {
      console.warn('[disk] disk_free failed', error);
      return null;
    }
  }

  private async refresh(dir: string): Promise<void> {
    const n = await this.freeFor(dir);
    if (n !== null) {
      this.free.set(n);
    }
  }
}

/** `C:` for `C:\Users\…`, `D:` for `d:/x`, else the path itself (POSIX roots have no letter). */
export function volumeOf(dir: string): string {
  const match = /^([A-Za-z]):/.exec(dir.trim());
  return match === null ? dir.trim() : `${match[1].toUpperCase()}:`;
}
