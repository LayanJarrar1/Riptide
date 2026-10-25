import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, signal, viewChild, viewChildren } from '@angular/core';

import { FORMAT_PIPES } from '../../core/format.pipes';
import { FormatService } from '../../core/format.service';
import { PREFERENCE_STORE } from '../../core/preferences';
import { I18nService } from '../../i18n/i18n.service';
import { ButtonComponent } from '../../ui/button.component';
import { DialogComponent } from '../../ui/dialog.component';
import { IconComponent } from '../../ui/icon.component';
import { ConflictChoice } from './file-backend';
import { baseName, FileOpsService } from './file-ops.service';
import { FilePaneComponent } from './file-pane.component';

/** Preference keys: one or two panes, the split, and the folders each pane last showed. */
export const FILES_LAYOUT_KEY = 'riptide.files.layout';

interface Layout {
  readonly dual: boolean;
  /** Width of the first pane as a fraction of the page, 0.2..0.8. */
  readonly split: number;
  readonly paths: readonly (string | null)[];
}

const DEFAULT_LAYOUT: Layout = { dual: false, split: 0.5, paths: [null, null] };
const MIN_SPLIT = 0.2;
const MAX_SPLIT = 0.8;

/**
 * Riptide's file manager: one pane, or two side by side with a draggable divider. Items move
 * between panes by drag and drop (Ctrl copies), by Copy / Cut / Paste, or with the buttons
 * that copy or move the active pane's selection to the other pane. Name conflicts are asked
 * once per transfer; progress shows at the bottom with Cancel.
 */
@Component({
  selector: 'app-files-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [...FORMAT_PIPES, ButtonComponent, DialogComponent, FilePaneComponent, IconComponent],
  host: { '(keydown)': 'onKey($event)' },
  template: `
    <header class="top">
      <h1 class="title">{{ i18n.t('files.title') }}</h1>
      <span class="spacer"></span>
      @if (layout().dual) {
        <button appButton size="sm" variant="secondary" icon="copy" type="button" [disabled]="busy()" (click)="toOther('copy')">{{ i18n.t('files.copyToOther') }}</button>
        <button appButton size="sm" variant="secondary" type="button" [disabled]="busy()" (click)="toOther('move')">{{ i18n.t('files.moveToOther') }}</button>
      }
      <button appButton size="sm" variant="ghost" icon="layers" type="button" [attr.aria-pressed]="layout().dual" (click)="toggleDual()">{{ i18n.t(layout().dual ? 'files.onePane' : 'files.twoPanes') }}</button>
    </header>

    <div class="panes" #panes [class.is-dual]="layout().dual" [style.--split]="layout().split">
      <div class="pane-slot">
        <app-file-pane [initialPath]="layout().paths[0] ?? null" [active]="!layout().dual || activePane() === 0" (focused)="activePane.set(0)" (pathChange)="remember(0, $event)" />
      </div>
      @if (layout().dual) {
        <div
          class="divider"
          role="separator"
          tabindex="0"
          aria-orientation="vertical"
          [attr.aria-label]="i18n.t('files.resizePanes')"
          [attr.aria-valuenow]="splitPercent()"
          aria-valuemin="20"
          aria-valuemax="80"
          (pointerdown)="startSplit($event)"
          (keydown)="onSplitKey($event)"
          (dblclick)="setSplit(0.5)"
        ></div>
        <div class="pane-slot">
          <app-file-pane [initialPath]="layout().paths[1] ?? null" [active]="activePane() === 1" (focused)="activePane.set(1)" (pathChange)="remember(1, $event)" />
        </div>
      }
    </div>

    @if (ops.running(); as r) {
      <div class="progress" role="status" aria-live="polite">
        <app-icon [name]="r.kind === 'copy' ? 'copy' : 'folderOpen'" [size]="16" />
        <span class="progress-text">
          {{ i18n.t(r.kind === 'copy' ? 'files.copying' : 'files.moving').replace('{n}', format.int(r.count)).replace('{dest}', destName(r.dest)) }}
          @if (r.progress; as p) {
            <span class="progress-sub num">{{ p.current }} · {{ p.done_bytes | bytes }} / {{ p.total_bytes | bytes }}</span>
          }
        </span>
        <span class="bar" aria-hidden="true"><span class="bar-fill" [style.inline-size.%]="percent()"></span></span>
        <button appButton size="sm" variant="ghost" icon="stop" type="button" (click)="ops.cancelRunning()">{{ i18n.t('action.cancel') }}</button>
      </div>
    }

    <app-dialog [open]="ops.conflict() !== null" [title]="i18n.t('files.conflictTitle')" (closed)="answer(null)">
      @if (ops.conflict(); as q) {
        <p class="conflict-text">
          {{ q.plan.conflicts.length === 1 ? i18n.t('files.conflictOne').replace('{name}', destName(q.plan.conflicts[0].target)) : i18n.t('files.conflictMany').replace('{n}', format.int(q.plan.conflicts.length)) }}
        </p>
        <ul class="conflict-list">
          @for (c of q.plan.conflicts.slice(0, 5); track c.target) {
            <li dir="auto">{{ destName(c.target) }}</li>
          }
          @if (q.plan.conflicts.length > 5) {
            <li class="more">{{ i18n.t('files.andMore').replace('{n}', format.int(q.plan.conflicts.length - 5)) }}</li>
          }
        </ul>
        <p class="conflict-note">{{ i18n.t(ops.platformKey('files.replaceNote')) }}</p>
      }
      <ng-container dialogActions>
        <button appButton variant="ghost" type="button" (click)="answer(null)">{{ i18n.t('action.cancel') }}</button>
        <button appButton variant="secondary" type="button" (click)="answer('skip')">{{ i18n.t('files.skip') }}</button>
        <button appButton variant="secondary" type="button" (click)="answer('replace')">{{ i18n.t('files.replace') }}</button>
        <button appButton variant="primary" type="button" (click)="answer('keepBoth')">{{ i18n.t('files.keepBoth') }}</button>
      </ng-container>
    </app-dialog>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-rows: auto minmax(0, 1fr) auto;
      gap: var(--space-3);
      block-size: 100%;
      min-block-size: 0;
      padding: 0 var(--space-4) var(--space-4);
    }

    .top {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      block-size: 56px;
    }

    .title {
      font-size: var(--text-lg);
      font-weight: var(--weight-semibold);
      letter-spacing: var(--tracking-tight);
    }

    .spacer {
      flex: 1;
    }

    .panes {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      min-block-size: 0;
    }

    .panes.is-dual {
      grid-template-columns: minmax(0, calc(var(--split) * 100%)) 10px minmax(0, 1fr);
    }

    .pane-slot {
      display: grid;
      min-inline-size: 0;
      min-block-size: 0;
      container-type: inline-size;
      container-name: files;
    }

    .divider {
      position: relative;
      cursor: col-resize;
      touch-action: none;
    }

    .divider::after {
      content: '';
      position: absolute;
      inset-block: 30%;
      inset-inline-start: 4px;
      inline-size: 2px;
      border-radius: 1px;
      background: var(--color-border-strong);
      transition: background-color var(--dur-1) var(--ease-standard);
    }

    .divider:hover::after,
    .divider:focus-visible::after {
      background: var(--color-accent);
    }

    .divider:focus-visible {
      outline: none;
    }

    .progress {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-2) var(--space-3);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      background: var(--color-surface-1);
      box-shadow: var(--shadow-1);
      font-size: var(--text-sm);
      animation: up var(--dur-2) var(--ease-emphasized-out) both;
    }

    @keyframes up {
      from {
        opacity: 0;
        transform: translateY(6px);
      }
    }

    .progress-text {
      display: flex;
      flex-direction: column;
      min-inline-size: 0;
    }

    .progress-sub {
      overflow: hidden;
      color: var(--color-text-muted);
      font-size: var(--text-xs);
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .bar {
      flex: 1;
      block-size: 6px;
      min-inline-size: 80px;
      overflow: hidden;
      border-radius: 3px;
      background: var(--color-surface-2);
    }

    .bar-fill {
      display: block;
      block-size: 100%;
      background: var(--color-accent);
      transition: inline-size var(--dur-2) var(--ease-standard);
    }

    .conflict-text {
      margin-block-end: var(--space-2);
      color: var(--color-text-secondary);
    }

    .conflict-list {
      display: flex;
      flex-direction: column;
      gap: 2px;
      margin-block-end: var(--space-3);
      padding-inline-start: var(--space-4);
      font-size: var(--text-sm);
    }

    .conflict-list .more,
    .conflict-note {
      color: var(--color-text-muted);
      font-size: var(--text-xs);
    }

    @media (prefers-reduced-motion: reduce) {
      .progress {
        animation: none;
      }
    }
  `,
})
export class FilesPageComponent {
  protected readonly i18n = inject(I18nService);
  protected readonly format = inject(FormatService);
  protected readonly ops = inject(FileOpsService);
  private readonly prefs = inject(PREFERENCE_STORE);
  private readonly panesRef = viewChild<ElementRef<HTMLElement>>('panes');
  private readonly panes = viewChildren(FilePaneComponent);

  protected readonly layout = signal<Layout>(this.loadLayout());
  protected readonly activePane = signal<0 | 1>(0);
  protected readonly busy = computed(() => this.ops.running() !== null);
  protected readonly splitPercent = computed(() => Math.round(this.layout().split * 100));
  protected readonly percent = computed(() => {
    const p = this.ops.running()?.progress;
    return p === null || p === undefined || p.total_bytes === 0 ? 0 : Math.min(100, (100 * p.done_bytes) / p.total_bytes);
  });

  protected toggleDual(): void {
    this.update({ dual: !this.layout().dual });
    this.activePane.set(0);
  }

  /** Copies or moves the active pane's selection into the folder the other pane shows. */
  protected toOther(kind: 'copy' | 'move'): void {
    const panes = this.panes();
    const from = panes[this.activePane()];
    const to = panes[this.activePane() === 0 ? 1 : 0];
    if (from === undefined || to === undefined) {
      return;
    }
    const paths = from.selectedPaths();
    if (paths.length > 0 && to.currentPath() !== '') {
      void this.ops.transfer(paths, to.currentPath(), kind);
    }
  }

  protected answer(choice: ConflictChoice | null): void {
    this.ops.conflict()?.answer(choice);
  }

  protected remember(index: 0 | 1, path: string): void {
    const paths = [...this.layout().paths];
    paths[index] = path;
    this.update({ paths });
  }

  protected destName(path: string): string {
    return baseName(path) || path;
  }

  protected startSplit(event: PointerEvent): void {
    const host = this.panesRef()?.nativeElement;
    if (!host || event.button !== 0) {
      return;
    }
    event.preventDefault();
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture?.(event.pointerId);
    const rect = host.getBoundingClientRect();
    const rtl = getComputedStyle(host).direction === 'rtl';
    const move = (e: PointerEvent): void => {
      const x = rtl ? rect.right - e.clientX : e.clientX - rect.left;
      this.setSplit(x / rect.width);
    };
    const end = (): void => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', end);
      target.removeEventListener('pointercancel', end);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', end);
    target.addEventListener('pointercancel', end);
  }

  protected onSplitKey(event: KeyboardEvent): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') {
      return;
    }
    event.preventDefault();
    const rtl = getComputedStyle(event.currentTarget as HTMLElement).direction === 'rtl';
    const grow = (event.key === 'ArrowRight') !== rtl;
    this.setSplit(this.layout().split + (grow ? 0.05 : -0.05));
  }

  setSplit(fraction: number): void {
    this.update({ split: Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, fraction)) });
  }

  /** F6 moves between the panes, like Explorer moves between its regions. */
  protected onKey(event: KeyboardEvent): void {
    if (event.key === 'F6' && this.layout().dual) {
      event.preventDefault();
      const next = this.activePane() === 0 ? 1 : 0;
      this.activePane.set(next);
      this.panes()[next]?.focusList();
    }
  }

  private update(change: Partial<Layout>): void {
    const next = { ...this.layout(), ...change };
    this.layout.set(next);
    this.prefs.set(FILES_LAYOUT_KEY, JSON.stringify(next));
  }

  private loadLayout(): Layout {
    try {
      const parsed = JSON.parse(this.prefs.get(FILES_LAYOUT_KEY) ?? 'null') as Partial<Layout> | null;
      if (parsed === null || typeof parsed !== 'object') {
        return DEFAULT_LAYOUT;
      }
      const split = typeof parsed.split === 'number' && Number.isFinite(parsed.split) ? Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, parsed.split)) : 0.5;
      const paths = Array.isArray(parsed.paths) ? parsed.paths.slice(0, 2).map((p) => (typeof p === 'string' ? p : null)) : [null, null];
      return { dual: parsed.dual === true, split, paths };
    } catch {
      return DEFAULT_LAYOUT;
    }
  }
}
