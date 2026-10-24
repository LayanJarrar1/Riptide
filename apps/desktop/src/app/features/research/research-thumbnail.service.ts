import { Injectable, Signal, inject, signal } from '@angular/core';

import { TAURI_AVAILABLE, TAURI_INVOKE } from '../../tauri/tauri-bridge';

/** Thumbnails fetched at the same time. */
export const THUMBNAIL_CONCURRENCY = 3;
/** Resolved thumbnails kept (as data URLs); the backend keeps its own, this one saves the round trip. */
const KEEP = 100;

/**
 * Result thumbnails. The webview's content security policy admits only `data:` images, so the
 * backend fetches the image of a result it found itself (`research_thumbnail`, by hit id, never by
 * address) and hands back a `data:` URL. Cards ask only once they scroll into view, at most
 * {@link THUMBNAIL_CONCURRENCY} at a time. A failure is a `null` (the card keeps its icon); nothing
 * is logged, so no address can leak into a log.
 */
@Injectable({ providedIn: 'root' })
export class ResearchThumbnailService {
  private readonly invoke = inject(TAURI_INVOKE);
  private readonly available = inject(TAURI_AVAILABLE);

  private readonly slots = new Map<string, ReturnType<typeof signal<string | null>>>();
  private readonly waiting: string[] = [];
  private running = 0;

  /** The thumbnail of hit `id` as a data URL once it is known; `null` until then or when there is none. */
  request(id: string): Signal<string | null> {
    let slot = this.slots.get(id);
    if (slot === undefined) {
      slot = signal<string | null>(null);
      this.slots.set(id, slot);
      if (this.available) {
        this.waiting.push(id);
        this.pump();
      }
      this.trim();
    }
    return slot.asReadonly();
  }

  /** The card went away before its turn: drop the request when it has not started. */
  release(id: string): void {
    const i = this.waiting.indexOf(id);
    if (i >= 0) {
      this.waiting.splice(i, 1);
      this.slots.delete(id);
    }
  }

  private pump(): void {
    while (this.running < THUMBNAIL_CONCURRENCY && this.waiting.length > 0) {
      const id = this.waiting.shift();
      const slot = id === undefined ? undefined : this.slots.get(id);
      if (id === undefined || slot === undefined) {
        continue;
      }
      this.running++;
      this.invoke<string | null>('research_thumbnail', { id })
        .then((url) => {
          slot.set(typeof url === 'string' && /^data:image\/(jpeg|png|webp|gif);base64,/.test(url) ? url : null);
        })
        .catch(() => slot.set(null))
        .finally(() => {
          this.running--;
          this.pump();
        });
    }
  }

  private trim(): void {
    while (this.slots.size > KEEP) {
      const oldest = this.slots.keys().next();
      if (oldest.done === true) {
        return;
      }
      this.slots.delete(oldest.value);
    }
  }
}
