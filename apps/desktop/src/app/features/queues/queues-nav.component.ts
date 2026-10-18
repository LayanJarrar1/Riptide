import { afterNextRender, ChangeDetectionStrategy, Component, computed, inject, Injector, input, output, signal } from '@angular/core';

import { FormatService } from '../../core/format.service';
import { DownloadEngine } from '../../data/download-engine';
import { DownloadsStore } from '../../data/downloads.store';
import { QueuesService } from '../../data/queues.service';
import { I18nService } from '../../i18n/i18n.service';
import { MAIN_QUEUE_ID, moveBeside, moveBy, sameOrder, UiQueue } from '../../model/queue';
import { isUnscheduled } from '../../model/schedule';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { MenuComponent, MenuItem } from '../../ui/menu.component';
import { ToastService } from '../../ui/toast.service';
import { DOWNLOAD_DRAG_TYPE, dragHas, droppedDownloadIds, inLowerHalf, QUEUE_DRAG_TYPE } from './dnd';
import { QueueDialogComponent } from './queue-dialog.component';

/** Where a dragged queue would land: next to `id`, before or after it. */
interface DropMark {
  readonly id: number;
  readonly after: boolean;
}

/**
 * The "Queues" group of the sidebar: Main and the custom queues, each with a running / stopped
 * dot, the number of running downloads against its limit, and a menu (start / stop, settings,
 * move, delete). Queues are reordered by dragging, or with Alt + ↑ / ↓ (or the menu) for the
 * keyboard. Download rows dropped on a queue move there. Choosing a queue filters the list.
 */
@Component({
  selector: 'app-queues-nav',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ButtonComponent, DialogComponent, IconComponent, MenuComponent, QueueDialogComponent],
  template: `
    <div class="head">
      <p class="heading" id="queues-heading">{{ i18n.t('nav.queues') }}</p>
      <button type="button" class="add" [attr.aria-label]="i18n.t('queue.add')" [attr.title]="i18n.t('queue.add')" (click)="create()">
        <app-icon name="plus" [size]="14" />
      </button>
    </div>

    <ul class="list" role="list" aria-labelledby="queues-heading">
      @for (q of queues(); track q.id) {
        <li
          class="item"
          [attr.data-queue-id]="q.id"
          [class.is-active]="current() === q.id"
          [class.is-dragging]="dragId() === q.id"
          [class.drop-before]="mark()?.id === q.id && !mark()?.after"
          [class.drop-after]="mark()?.id === q.id && mark()?.after"
          [class.drop-into]="intoId() === q.id"
          draggable="true"
          (dragstart)="onDragStart($event, q)"
          (dragover)="onDragOver($event, q)"
          (dragleave)="onDragLeave(q)"
          (drop)="onDrop($event, q)"
          (dragend)="clearDrag()"
          (contextmenu)="openMenu($event, q)"
        >
          <button
            type="button"
            class="select"
            [attr.aria-current]="current() === q.id ? 'page' : null"
            [attr.aria-label]="label(q)"
            [attr.title]="label(q) + ' — ' + i18n.t('queue.reorderHint')"
            (click)="open.emit(q.id)"
            (keydown)="onKey($event, q)"
          >
            <span class="dot" [class.is-running]="q.enabled" aria-hidden="true"></span>
            <span class="name">{{ nameOf(q) }}</span>
            @if (scheduled(q)) {
              <app-icon class="clock" name="clock" [size]="12" />
            }
            <span class="count num">{{ format.int(activeOf(q)) }}/{{ format.int(q.maxConcurrent) }}</span>
          </button>
          <button
            type="button"
            class="mini toggle"
            [attr.aria-label]="i18n.t(q.enabled ? 'queue.stop' : 'queue.start') + ': ' + nameOf(q)"
            [attr.title]="i18n.t(q.enabled ? 'queue.stop' : 'queue.start')"
            (click)="toggle(q)"
          >
            <app-icon [name]="q.enabled ? 'stop' : 'play'" [size]="12" />
          </button>
          <button
            type="button"
            class="mini more"
            [attr.aria-label]="i18n.t('queue.actions').replace('{name}', nameOf(q))"
            [attr.title]="i18n.t('action.more')"
            (click)="openMenuAt($event, q)"
          >
            <app-icon name="more" [size]="14" />
          </button>
        </li>
      }
    </ul>

    <app-menu [open]="menu().open" [items]="menuItems()" [x]="menu().x" [y]="menu().y" (selected)="onMenu($event)" (closed)="closeMenu()" />

    <!-- the editor (and the schedule editor inside it) is its own chunk, fetched the first time it is wanted -->
    @defer (when dialog().open) {
      <app-queue-dialog [open]="dialog().open" [queue]="dialog().queue" (closed)="dialog.set({ open: false, queue: dialog().queue })" />
    }

    <app-dialog [open]="deletion() !== null" [title]="i18n.t('queue.deleteTitle')" (closed)="deletion.set(null)">
      @if (deletion(); as q) {
        <p class="delete-text">{{ deleteText(q) }}</p>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="deletion.set(null)">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="danger" icon="trash" type="button" (click)="confirmDelete()">{{ i18n.t('queue.delete') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    :host {
      display: block;
    }

    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding-inline: var(--space-2) var(--space-1);
      padding-block-end: var(--space-1);
    }

    .heading {
      margin: 0;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      font-weight: var(--weight-medium);
    }

    .add,
    .mini {
      display: inline-grid;
      place-items: center;
      inline-size: 22px;
      block-size: 22px;
      padding: 0;
      border: 0;
      border-radius: var(--radius-xs);
      background: transparent;
      color: var(--color-text-muted);
      cursor: pointer;
    }

    .add:hover,
    .mini:hover {
      background: var(--color-surface-hover);
      color: var(--color-text);
    }

    .list {
      display: flex;
      flex-direction: column;
      gap: 1px;
      max-block-size: 40vh;
      margin: 0;
      padding: 0;
      overflow-y: auto;
      list-style: none;
    }

    .item {
      position: relative;
      display: flex;
      align-items: center;
      border-radius: var(--radius-sm);
      color: var(--color-text-secondary);
    }

    .item:hover {
      background: var(--color-surface-hover);
      color: var(--color-text);
    }

    .item.is-active {
      background: var(--color-accent-soft);
      color: var(--color-text);
    }

    .item.is-dragging {
      opacity: 0.45;
    }

    .item.drop-before::before,
    .item.drop-after::after {
      content: '';
      position: absolute;
      inset-inline: 4px;
      block-size: 2px;
      border-radius: 1px;
      background: var(--color-accent);
    }

    .item.drop-before::before {
      inset-block-start: -1px;
    }

    .item.drop-after::after {
      inset-block-end: -1px;
    }

    .item.drop-into {
      outline: 2px solid var(--color-accent);
      outline-offset: -2px;
    }

    .select {
      display: flex;
      flex: 1;
      align-items: center;
      gap: var(--space-2);
      min-inline-size: 0;
      block-size: 32px;
      padding-inline: var(--space-2);
      border: 0;
      border-radius: var(--radius-sm);
      background: transparent;
      color: inherit;
      font: inherit;
      font-size: var(--text-sm);
      text-align: start;
      cursor: pointer;
    }

    .dot {
      flex: none;
      inline-size: 8px;
      block-size: 8px;
      border-radius: 50%;
      border: 1.5px solid var(--color-text-muted);
    }

    .dot.is-running {
      border-color: var(--color-accent);
      background: var(--color-accent);
    }

    .name {
      flex: 1;
      min-inline-size: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .clock {
      flex: none;
      color: var(--color-text-muted);
    }

    .count {
      flex: none;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    .mini {
      flex: none;
      opacity: 0;
    }

    .item:hover .mini,
    .item:focus-within .mini {
      opacity: 1;
    }

    .delete-text {
      padding-block: var(--space-2);
      color: var(--color-text-secondary);
      line-height: var(--leading-relaxed);
    }

    /* Narrow windows: the sidebar is icons only, so the queue list shows just the dots. */
    @media (max-width: 900px) {
      .heading,
      .name,
      .count,
      .clock,
      .mini {
        display: none;
      }

      .select {
        justify-content: center;
        padding-inline: 0;
      }
    }

    @media (prefers-reduced-motion: reduce) {
      .item.is-dragging {
        opacity: 0.6;
      }
    }
  `,
})
export class QueuesNavComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  private readonly queuesService = inject(QueuesService);
  private readonly store = inject(DownloadsStore);
  private readonly engine = inject(DownloadEngine);
  private readonly toasts = inject(ToastService);
  private readonly injector = inject(Injector);

  /** The queue the list is showing (highlighted), or `null`. */
  readonly current = input<number | null>(null);

  /** The user chose a queue. */
  readonly open = output<number>();

  protected readonly queues = this.queuesService.queues;

  protected readonly dragId = signal<number | null>(null);
  protected readonly mark = signal<DropMark | null>(null);
  protected readonly intoId = signal<number | null>(null);

  protected readonly menu = signal<{ open: boolean; x: number; y: number; queue: UiQueue | null }>({ open: false, x: 0, y: 0, queue: null });
  protected readonly dialog = signal<{ open: boolean; queue: UiQueue | null }>({ open: false, queue: null });
  protected readonly deletion = signal<UiQueue | null>(null);

  protected readonly menuItems = computed<readonly MenuItem[]>(() => {
    const q = this.menu().queue;
    if (q === null) {
      return [];
    }
    const all = this.queues();
    const index = all.findIndex((x) => x.id === q.id);
    const t = (k: string): string => this.i18n.t(k);
    const items: MenuItem[] = [
      { id: 'toggle', label: t(q.enabled ? 'queue.stop' : 'queue.start'), icon: q.enabled ? 'stop' : 'play' },
      { id: 'settings', label: t('queue.settings'), icon: 'settings' },
      { id: 'up', label: t('queue.moveUp'), icon: 'arrowUp', shortcut: 'Alt ↑', disabled: index <= 0, separator: true },
      { id: 'down', label: t('queue.moveDown'), icon: 'arrowDown', shortcut: 'Alt ↓', disabled: index < 0 || index >= all.length - 1 },
    ];
    if (q.id !== MAIN_QUEUE_ID) {
      items.push({ id: 'delete', label: t('queue.delete'), icon: 'trash', danger: true, separator: true });
    }
    return items;
  });

  protected nameOf(q: UiQueue): string {
    return q.id === MAIN_QUEUE_ID ? this.i18n.t('queue.main') : q.name;
  }

  protected activeOf(q: UiQueue): number {
    return this.store.queueCounts().get(q.id)?.active ?? 0;
  }

  protected scheduled(q: UiQueue): boolean {
    return !isUnscheduled(q.schedule);
  }

  /** The accessible name: queue, state and running count. */
  protected label(q: UiQueue): string {
    const slots = this.i18n.t('queue.slots').replace('{active}', this.format.int(this.activeOf(q))).replace('{limit}', this.format.int(q.maxConcurrent));
    return `${this.nameOf(q)}, ${this.i18n.t(q.enabled ? 'queue.running' : 'queue.stopped')}, ${slots}`;
  }

  protected deleteText(q: UiQueue): string {
    const n = this.store.queueCounts().get(q.id)?.total ?? 0;
    return this.i18n
      .t(n === 0 ? 'queue.deleteBodyEmpty' : 'queue.deleteBody')
      .replace('{name}', q.name)
      .replace('{n}', this.format.int(n));
  }

  protected create(): void {
    this.dialog.set({ open: true, queue: null });
  }

  protected toggle(q: UiQueue): void {
    this.run(this.queuesService.setRunning(q.id, !q.enabled));
  }

  protected openMenu(event: MouseEvent, q: UiQueue): void {
    event.preventDefault();
    this.menu.set({ open: true, x: event.clientX, y: event.clientY, queue: q });
  }

  protected openMenuAt(event: MouseEvent, q: UiQueue): void {
    event.stopPropagation();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.menu.set({ open: true, x: rect.left, y: rect.bottom + 4, queue: q });
  }

  protected closeMenu(): void {
    this.menu.update((m) => ({ ...m, open: false }));
  }

  protected onMenu(id: string): void {
    const q = this.menu().queue;
    if (q === null) {
      return;
    }
    switch (id) {
      case 'toggle':
        this.toggle(q);
        break;
      case 'settings':
        this.dialog.set({ open: true, queue: q });
        break;
      case 'up':
        this.moveBy(q, -1);
        break;
      case 'down':
        this.moveBy(q, 1);
        break;
      case 'delete':
        this.deletion.set(q);
        break;
    }
  }

  protected confirmDelete(): void {
    const q = this.deletion();
    this.deletion.set(null);
    if (q === null) {
      return;
    }
    if (this.store.queue() === q.id) {
      this.store.setQueue(null);
    }
    this.run(this.queuesService.remove(q.id), () => this.toasts.show('info', this.i18n.t('toast.queueDeleted')));
  }

  /** Alt + ↑ / ↓ on a queue moves it; plain arrows move focus between queues. */
  protected onKey(event: KeyboardEvent, q: UiQueue): void {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') {
      return;
    }
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    event.preventDefault();
    if (event.altKey) {
      this.moveBy(q, delta);
      this.focusQueue(q.id);
      return;
    }
    const all = this.queues();
    const next = all[all.findIndex((x) => x.id === q.id) + delta];
    if (next !== undefined) {
      this.focusQueue(next.id);
    }
  }

  protected onDragStart(event: DragEvent, q: UiQueue): void {
    if (event.dataTransfer === null) {
      return;
    }
    event.dataTransfer.setData(QUEUE_DRAG_TYPE, String(q.id));
    event.dataTransfer.effectAllowed = 'move';
    this.dragId.set(q.id);
  }

  protected onDragOver(event: DragEvent, q: UiQueue): void {
    if (dragHas(event, QUEUE_DRAG_TYPE)) {
      event.preventDefault();
      const after = inLowerHalf(event, event.currentTarget as HTMLElement);
      const dragged = this.dragId();
      this.mark.set(dragged === null || dragged === q.id ? null : { id: q.id, after });
    } else if (dragHas(event, DOWNLOAD_DRAG_TYPE)) {
      event.preventDefault();
      this.intoId.set(q.id);
    }
  }

  protected onDragLeave(q: UiQueue): void {
    if (this.mark()?.id === q.id) {
      this.mark.set(null);
    }
    if (this.intoId() === q.id) {
      this.intoId.set(null);
    }
  }

  protected onDrop(event: DragEvent, q: UiQueue): void {
    event.preventDefault();
    const dragged = this.dragId();
    const mark = this.mark();
    this.clearDrag();
    if (dragHas(event, DOWNLOAD_DRAG_TYPE)) {
      const ids = droppedDownloadIds(event).filter((id) => this.engine.get(id)?.queueId !== q.id);
      if (ids.length > 0) {
        this.run(this.engine.moveToQueue(ids, q.id), () => this.toasts.show('info', this.movedText(ids.length, q)));
      }
    } else if (dragged !== null && mark !== null) {
      this.reorder(moveBeside(this.queues().map((x) => x.id), dragged, q.id, mark.after));
    }
  }

  protected clearDrag(): void {
    this.dragId.set(null);
    this.mark.set(null);
    this.intoId.set(null);
  }

  private moveBy(q: UiQueue, delta: 1 | -1): void {
    this.reorder(moveBy(this.queues().map((x) => x.id), q.id, delta));
  }

  private reorder(order: readonly number[]): void {
    if (!sameOrder(order, this.queues().map((q) => q.id))) {
      this.run(this.queuesService.reorder(order));
    }
  }

  private movedText(n: number, q: UiQueue): string {
    return n === 1
      ? this.i18n.t('toast.queueMoved').replace('{queue}', this.nameOf(q))
      : this.i18n.t('toast.queueMovedMany').replace('{n}', this.format.int(n)).replace('{queue}', this.nameOf(q));
  }

  /** Puts keyboard focus back on a queue's button once the list has re-rendered. */
  private focusQueue(id: number): void {
    afterNextRender(() => document.querySelector<HTMLElement>(`[data-queue-id="${id}"] .select`)?.focus(), { injector: this.injector });
  }

  private run(command: Promise<unknown>, done?: () => void): void {
    command.then(
      () => done?.(),
      (error: unknown) => this.toasts.show('danger', this.i18n.t('toast.engineError').replace('{error}', String(error))),
    );
  }
}
