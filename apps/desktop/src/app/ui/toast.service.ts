import { Injectable, signal } from '@angular/core';

import { IconName } from './icon.component';

/** Tone of a toast; decides icon and hue. */
export type ToastTone = 'info' | 'success' | 'danger';

/** One notification. */
export interface Toast {
  readonly id: number;
  readonly tone: ToastTone;
  readonly text: string;
  /** Optional one-action button (for example "Undo"). */
  readonly action?: { readonly label: string; readonly run: () => void };
}

const ICONS: Readonly<Record<ToastTone, IconName>> = { info: 'info', success: 'check', danger: 'alert' };

/** Icon for a tone. */
export function toastIcon(tone: ToastTone): IconName {
  return ICONS[tone];
}

/** Queue of short-lived notifications shown bottom-end by `ToastsComponent`. */
@Injectable({ providedIn: 'root' })
export class ToastService {
  private nextId = 1;
  private readonly list = signal<readonly Toast[]>([]);

  /** Visible toasts, oldest first. */
  readonly toasts = this.list.asReadonly();

  /** Shows a toast for `ms` (default 4 s, 7 s when it has an action). Returns its id. */
  show(tone: ToastTone, text: string, action?: Toast['action'], ms?: number): number {
    const id = this.nextId++;
    this.list.update((l) => [...l.slice(-3), { id, tone, text, action }]);
    setTimeout(() => this.dismiss(id), ms ?? (action ? 7000 : 4000));
    return id;
  }

  /** Removes a toast early. */
  dismiss(id: number): void {
    this.list.update((l) => l.filter((t) => t.id !== id));
  }
}
