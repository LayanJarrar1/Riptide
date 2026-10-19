import { DestroyRef, inject, Injectable, signal } from '@angular/core';

import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import type { PostAction } from '../../tauri/bridge/PostAction';
import { ToastService } from '../../ui/toast.service';

/** Seconds the user has to cancel a post-queue action. */
export const COUNTDOWN_SECONDS = 30;

/** An action waiting for its countdown. */
export interface PendingAction {
  readonly queueId: number;
  readonly action: PostAction;
  /** Whole seconds left. */
  readonly remaining: number;
}

/**
 * The countdown between "the queue finished" and the post-queue action. When a queue with an
 * action drains, the engine announces it; this service counts down from 30 seconds one second
 * at a time, and the dialog shows it. Cancel stops it for good; "Do it now" and zero both end
 * in a single call to `QueuesService.perform`, which makes the Rust side carry out the action
 * saved on that queue. A second queue finishing while one is counting down replaces it, so only
 * one action is ever pending.
 */
@Injectable({ providedIn: 'root' })
export class PostActionCountdown {
  private readonly queues = inject(QueuesService);
  private readonly toasts = inject(ToastService);
  private readonly i18n = inject(I18nService);

  private timer: ReturnType<typeof setInterval> | null = null;

  /** The pending action, or `null` when none is counting down. */
  readonly pending = signal<PendingAction | null>(null);

  constructor() {
    const subscription = this.queues.finished.subscribe((e) => {
      if (e.action !== null) {
        this.start(e.queueId, e.action);
      }
    });
    inject(DestroyRef).onDestroy(() => {
      subscription.unsubscribe();
      this.stopTimer();
    });
  }

  /** Starts (or restarts) the countdown for `queueId`'s action. */
  start(queueId: number, action: PostAction): void {
    this.stopTimer();
    this.pending.set({ queueId, action, remaining: COUNTDOWN_SECONDS });
    this.timer = setInterval(() => this.tick(), 1000);
  }

  /** Stops the countdown; nothing is done. */
  cancel(): void {
    if (this.pending() !== null) {
      this.stopTimer();
      this.pending.set(null);
    }
  }

  /** Skips the rest of the countdown and carries the action out now. */
  doNow(): void {
    this.fire();
  }

  private tick(): void {
    const p = this.pending();
    if (p === null) {
      this.stopTimer();
      return;
    }
    const remaining = p.remaining - 1;
    if (remaining <= 0) {
      this.fire();
    } else {
      this.pending.set({ ...p, remaining });
    }
  }

  /** Ends the countdown and performs the action exactly once. */
  private fire(): void {
    const p = this.pending();
    if (p === null) {
      return;
    }
    this.stopTimer();
    this.pending.set(null);
    this.queues.perform(p.queueId).catch((error: unknown) => {
      const reason = error instanceof Error ? error.message : String(error);
      this.toasts.show('danger', this.i18n.t('postaction.failed').replace('{error}', reason), undefined, 12_000);
    });
  }

  private stopTimer(): void {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }
}
