import { inject, Injectable, signal } from '@angular/core';

import { FormatService } from '../../core/format.service';
import { IS_MAC } from '../../core/platform';
import { I18nService } from '../../i18n/i18n.service';
import { ToastService } from '../../ui/toast.service';
import { ConflictChoice, FILE_BACKEND, Progress, TransferKind, TransferPlan, TransferReport } from './file-backend';
import { FolderColorsService } from './folder-colors.service';

/** What Copy / Cut put on Riptide's file clipboard. */
export interface FileClipboard {
  readonly kind: TransferKind;
  readonly paths: readonly string[];
}

/** The transfer shown in the progress bar. */
export interface RunningTransfer {
  readonly id: number;
  readonly kind: TransferKind;
  readonly count: number;
  readonly dest: string;
  readonly progress: Progress | null;
}

/** A question for the conflict dialog; `answer` resolves it (`null` = cancel). */
export interface ConflictQuestion {
  readonly plan: TransferPlan;
  readonly kind: TransferKind;
  readonly dest: string;
  readonly answer: (choice: ConflictChoice | null) => void;
}

/** Parent folder of a path, for refreshes and undo. */
export function parentPath(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  const i = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'));
  if (i < 0) {
    return trimmed;
  }
  const parent = trimmed.slice(0, i);
  return /^[A-Za-z]:$/.test(parent) || parent === '' ? parent + (trimmed.includes('\\') ? '\\' : '/') : parent;
}

export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  return trimmed.slice(Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/')) + 1);
}

/**
 * Everything the file manager panes share: the clipboard, the one running transfer, the
 * conflict question, and a `version` that ticks after every change so each pane reloads the
 * folder it shows. Every change offers Undo where it can be undone safely.
 */
@Injectable({ providedIn: 'root' })
export class FileOpsService {
  private readonly backend = inject(FILE_BACKEND);
  private readonly toasts = inject(ToastService);
  private readonly i18n = inject(I18nService);
  private readonly format = inject(FormatService);
  private readonly colors = inject(FolderColorsService);
  private readonly isMac = inject(IS_MAC);

  /** Items can be put back from the Recycle Bin here (not on macOS, where Finder does it). */
  readonly canRestore = signal(true);

  readonly clipboard = signal<FileClipboard | null>(null);
  readonly running = signal<RunningTransfer | null>(null);
  readonly conflict = signal<ConflictQuestion | null>(null);
  /** Ticks after anything changed on disk; panes reload when it moves. */
  readonly version = signal(0);
  private nextId = 1;

  constructor() {
    this.backend
      .capabilities()
      .then((c) => this.canRestore.set(c.canRestore))
      .catch(() => this.canRestore.set(false));
  }

  /**
   * The words the platform uses: Trash and Finder on macOS, Recycle Bin and Explorer on Windows.
   * Takes the Windows key and returns the key to show.
   */
  platformKey(key: string): string {
    if (!this.isMac) {
      return key;
    }
    const mac: Readonly<Record<string, string>> = {
      'files.recycleBin': 'files.trash',
      'files.binEmpty': 'files.trashEmpty',
      'files.trashedOne': 'files.trashedOneMac',
      'files.trashedMany': 'files.trashedManyMac',
      'files.replaceNote': 'files.replaceNoteMac',
      'files.showInExplorer': 'files.showInFinder',
    };
    return mac[key] ?? key;
  }

  /** A fresh id for a cancellable backend call. */
  newId(): number {
    return this.nextId++;
  }

  copy(paths: readonly string[]): void {
    this.clipboard.set({ kind: 'copy', paths });
  }

  cut(paths: readonly string[]): void {
    this.clipboard.set({ kind: 'move', paths });
  }

  /** Pastes the clipboard into `dest`; a cut is used up once it went through. */
  async paste(dest: string): Promise<void> {
    const clip = this.clipboard();
    if (clip === null) {
      return;
    }
    const report = await this.transfer(clip.paths, dest, clip.kind);
    if (report !== null && clip.kind === 'move' && report.failed.length === 0 && !report.cancelled) {
      this.clipboard.set(null);
    }
  }

  /**
   * Copies or moves into `dest`: plans first, asks about name conflicts, refuses when the
   * destination lacks space, then runs with progress. `null` when the user backed out.
   */
  async transfer(sources: readonly string[], dest: string, kind: TransferKind): Promise<TransferReport | null> {
    if (sources.length === 0 || this.running() !== null) {
      if (this.running() !== null) {
        this.toasts.show('info', this.i18n.t('files.busy'));
      }
      return null;
    }
    let plan: TransferPlan;
    try {
      plan = await this.backend.plan(sources, dest, kind);
    } catch (e: unknown) {
      this.fail(e);
      return null;
    }
    if (plan.unchanged.length === sources.length) {
      return null;
    }
    if (plan.free !== null && plan.needs_space > plan.free) {
      this.toasts.show('danger', this.i18n.t('files.noSpace').replace('{need}', this.format.bytes(plan.needs_space)).replace('{free}', this.format.bytes(plan.free)));
      return null;
    }
    let choice: ConflictChoice = 'keepBoth';
    if (plan.conflicts.length > 0) {
      const answer = await new Promise<ConflictChoice | null>((resolve) => this.conflict.set({ plan, kind, dest, answer: resolve }));
      this.conflict.set(null);
      if (answer === null) {
        return null;
      }
      choice = answer;
    }
    const id = this.newId();
    this.running.set({ id, kind, count: sources.length, dest, progress: null });
    let report: TransferReport;
    try {
      report = await this.backend.transfer(id, sources, dest, kind, choice, (progress) => this.running.update((r) => (r === null || r.id !== id ? r : { ...r, progress })));
    } catch (e: unknown) {
      this.running.set(null);
      this.fail(e);
      this.bump();
      return null;
    }
    this.running.set(null);
    if (kind === 'move') {
      for (const d of report.done) {
        this.colors.move(d.from, d.to);
      }
    }
    this.bump();
    this.announce(report, kind);
    return report;
  }

  /** Stops the running transfer; finished items stay, the one in progress is cleaned up. */
  cancelRunning(): void {
    const r = this.running();
    if (r !== null) {
      void this.backend.cancel(r.id);
    }
  }

  async createFolder(parent: string, name: string): Promise<string | null> {
    try {
      const e = await this.backend.createFolder(parent, name);
      this.bump();
      return e.path;
    } catch (err: unknown) {
      this.fail(err);
      return null;
    }
  }

  async rename(path: string, name: string): Promise<string | null> {
    try {
      const e = await this.backend.rename(path, name);
      this.colors.move(path, e.path);
      this.bump();
      const before = baseName(path);
      this.toasts.show('success', this.i18n.t('files.renamed').replace('{name}', e.name), {
        label: this.i18n.t('files.undo'),
        icon: 'retry',
        run: () => void this.rename(e.path, before),
      });
      return e.path;
    } catch (err: unknown) {
      this.fail(err);
      return null;
    }
  }

  /** Sends items to the Recycle Bin; Undo brings them back where the system can restore. */
  async trash(paths: readonly string[]): Promise<boolean> {
    if (paths.length === 0) {
      return false;
    }
    try {
      await this.backend.trash(paths);
    } catch (err: unknown) {
      this.fail(err);
      return false;
    }
    this.bump();
    const text =
      paths.length === 1
        ? this.i18n.t(this.platformKey('files.trashedOne')).replace('{name}', baseName(paths[0]))
        : this.i18n.t(this.platformKey('files.trashedMany')).replace('{n}', this.format.int(paths.length));
    const undo = this.canRestore() ? { label: this.i18n.t('files.undo'), icon: 'retry' as const, run: () => void this.untrash(paths) } : undefined;
    this.toasts.show('success', text, undo);
    return true;
  }

  /** Restores the newest Recycle Bin item of each path's name in its folder. */
  async untrash(paths: readonly string[]): Promise<void> {
    try {
      const ids: string[] = [];
      for (const p of paths) {
        const items = await this.backend.trashed(parentPath(p));
        const match = items.find((i) => i.name === baseName(p));
        if (match !== undefined) {
          ids.push(match.id);
        }
      }
      await this.backend.restore(ids);
      this.bump();
    } catch (err: unknown) {
      this.fail(err);
    }
  }

  async restore(ids: readonly string[]): Promise<void> {
    try {
      await this.backend.restore(ids);
      this.bump();
    } catch (err: unknown) {
      this.fail(err);
    }
  }

  open(path: string): void {
    this.backend.open(path).catch((e: unknown) => this.fail(e));
  }

  reveal(path: string): void {
    this.backend.reveal(path).catch((e: unknown) => this.fail(e));
  }

  private announce(report: TransferReport, kind: TransferKind): void {
    if (report.failed.length > 0) {
      const first = report.failed[0];
      this.toasts.show('danger', this.i18n.t('files.failed').replace('{name}', baseName(first.path)).replace('{error}', first.error), undefined, 8000);
    }
    if (report.done.length === 0) {
      if (report.cancelled) {
        this.toasts.show('info', this.i18n.t('files.cancelled'));
      }
      return;
    }
    const key = kind === 'copy' ? 'files.copied' : 'files.moved';
    const text = this.i18n.t(key).replace('{n}', this.format.int(report.done.length));
    // undo: a copy goes to the Recycle Bin, a move goes back where it came from
    const undo = (): void => {
      if (kind === 'copy') {
        void this.trash(report.done.map((d) => d.to));
      } else {
        void this.moveBack(report);
      }
    };
    this.toasts.show(report.cancelled ? 'info' : 'success', report.cancelled ? `${text} · ${this.i18n.t('files.cancelled')}` : text, { label: this.i18n.t('files.undo'), icon: 'retry', run: undo });
  }

  private async moveBack(report: TransferReport): Promise<void> {
    const byFolder = new Map<string, string[]>();
    for (const d of report.done) {
      const folder = parentPath(d.from);
      byFolder.set(folder, [...(byFolder.get(folder) ?? []), d.to]);
    }
    for (const [folder, items] of byFolder) {
      await this.transfer(items, folder, 'move');
    }
  }

  private bump(): void {
    this.version.update((v) => v + 1);
  }

  private fail(e: unknown): void {
    const message = e instanceof Error ? e.message : String(e);
    this.toasts.show('danger', message, undefined, 6000);
  }
}
