import { Injectable, signal } from '@angular/core';

import { IconName } from './icon.component';

/** Tone of a toast; decides icon and hue. */
export type ToastTone = 'info' | 'success' | 'danger';

/** A button on a toast ("Undo", "Open file"). */
export interface ToastAction {
  readonly label: string;
  readonly run: () => void;
  readonly icon?: IconName;
}

/** One notification. */
export interface Toast {
  readonly id: number;
  readonly tone: ToastTone;
  readonly text: string;
  /** Zero, one or two action buttons. */
  readonly actions: readonly ToastAction[];
  /** How long it stays, for the auto-dismiss line. */
  readonly ms: number;
  /** `true` while the leave animation plays; the toast is gone a moment later. */
  readonly leaving: boolean;
}

const ICONS: Readonly<Record<ToastTone, IconName>> = { info: 'info', success: 'check', danger: 'alert' };

/** How long a toast is given to fade out before it is dropped from the list. */
export const TOAST_LEAVE_MS = 160;

/** Icon for a tone. */
export function toastIcon(tone: ToastTone): IconName {
  return ICONS[tone];
}

interface Timer {
  handle: ReturnType<typeof setTimeout> | null;
  startedAt: number;
  remaining: number;
}

/**
 * Queue of short-lived notifications shown bottom-end by `ToastsComponent`. At most four are
 * visible. A toast's clock can be paused (the component does so while the pointer is over it)
 * and dismissal first marks it `leaving` so the component can play the exit before removal.
 */
@Injectable({ providedIn: 'root' })
export class ToastService {
  private nextId = 1;
  private readonly list = signal<readonly Toast[]>([]);
  private readonly timers = new Map<number, Timer>();

  /** Visible toasts, oldest first. */
  readonly toasts = this.list.asReadonly();

  /**
   * Shows a toast for `ms` (default 4 s, 7 s when it has actions). Returns its id.
   * `actions` may be one action or a list of up to two.
   */
  show(tone: ToastTone, text: string, actions?: ToastAction | readonly ToastAction[], ms?: number): number {
    const id = this.nextId++;
    const list = actions === undefined ? [] : Array.isArray(actions) ? actions.slice(0, 2) : [actions as ToastAction];
    const duration = ms ?? (list.length > 0 ? 7000 : 4000);
    this.list.update((l) => [...l.slice(-3), { id, tone, text, actions: list, ms: duration, leaving: false }]);
    this.timers.set(id, { handle: null, startedAt: 0, remaining: duration });
    this.resume(id);
    return id;
  }

  /** Starts the leave animation and drops the toast once it has played. */
  dismiss(id: number): void {
    const toast = this.list().find((t) => t.id === id);
    if (toast === undefined || toast.leaving) {
      return;
    }
    this.clearTimer(id);
    this.list.update((l) => l.map((t) => (t.id === id ? { ...t, leaving: true } : t)));
    setTimeout(() => this.list.update((l) => l.filter((t) => t.id !== id)), TOAST_LEAVE_MS);
  }

  /** Stops the auto-dismiss clock (pointer over the toast). */
  pause(id: number): void {
    const timer = this.timers.get(id);
    if (timer === undefined || timer.handle === null) {
      return;
    }
    clearTimeout(timer.handle);
    timer.handle = null;
    timer.remaining = Math.max(0, timer.remaining - (Date.now() - timer.startedAt));
  }

  /** Restarts the clock with whatever time was left. */
  resume(id: number): void {
    const timer = this.timers.get(id);
    if (timer === undefined || timer.handle !== null) {
      return;
    }
    timer.startedAt = Date.now();
    timer.handle = setTimeout(() => {
      this.timers.delete(id);
      this.dismiss(id);
    }, timer.remaining);
  }

  private clearTimer(id: number): void {
    const timer = this.timers.get(id);
    if (timer?.handle != null) {
      clearTimeout(timer.handle);
    }
    this.timers.delete(id);
  }
}
