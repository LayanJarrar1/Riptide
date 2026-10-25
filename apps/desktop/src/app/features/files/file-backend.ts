import { inject, InjectionToken } from '@angular/core';

import type { ConflictChoice } from '../../tauri/bridge/ConflictChoice';
import type { Entry } from '../../tauri/bridge/Entry';
import type { Listing } from '../../tauri/bridge/Listing';
import type { Plan } from '../../tauri/bridge/Plan';
import type { Progress } from '../../tauri/bridge/Progress';
import type { Properties } from '../../tauri/bridge/Properties';
import type { TransferKind } from '../../tauri/bridge/TransferKind';
import type { TransferReport } from '../../tauri/bridge/TransferReport';
import type { TrashedItem } from '../../tauri/bridge/TrashedItem';
import { InvokeFn, ListenFn, TAURI_AVAILABLE, TAURI_INVOKE, TAURI_LISTEN } from '../../tauri/tauri-bridge';

export type { ConflictChoice, Entry, Listing, Plan, Progress, Properties, TransferKind, TransferReport, TrashedItem };

/** The webview event transfer progress arrives on (`FS_PROGRESS_EVENT` in `files.rs`). */
export const FS_PROGRESS_EVENT = 'riptide://fs-progress';

/** A starting point in the sidebar (`Place` in `files.rs`). */
export interface Place {
  readonly key: 'downloads' | 'organized' | 'home' | 'desktop' | 'documents' | 'pictures' | 'music' | 'videos' | 'drive';
  readonly label: string;
  readonly path: string;
}

/** What the file manager can do on this system (`FsCapabilities` in `files.rs`). */
export interface FsCapabilities {
  readonly canRestore: boolean;
}

/** A plan plus the destination's free bytes (`TransferPlan` in `files.rs`). */
export interface TransferPlan extends Plan {
  readonly free: number | null;
}

/**
 * The file manager's view of the disk. `TauriFileBackend` calls the commands in
 * `src-tauri/src/files.rs`; `MemoryFileBackend` is an in-memory disk for the plain-browser
 * build and for specs.
 */
export interface FileBackend {
  places(): Promise<readonly Place[]>;
  /** What this system supports (restoring from the Recycle Bin is not possible on macOS). */
  capabilities(): Promise<FsCapabilities>;
  list(path: string): Promise<Listing>;
  createFolder(parent: string, name: string): Promise<Entry>;
  rename(path: string, name: string): Promise<Entry>;
  trash(paths: readonly string[]): Promise<void>;
  trashed(parent: string | null): Promise<readonly TrashedItem[]>;
  restore(ids: readonly string[]): Promise<void>;
  properties(id: number, path: string): Promise<Properties>;
  search(id: number, dir: string, query: string): Promise<readonly Entry[]>;
  plan(sources: readonly string[], dest: string, kind: TransferKind): Promise<TransferPlan>;
  /** Runs a transfer; `onProgress` sees it advance until the report resolves. */
  transfer(id: number, sources: readonly string[], dest: string, kind: TransferKind, choice: ConflictChoice, onProgress: (p: Progress) => void): Promise<TransferReport>;
  cancel(id: number): Promise<void>;
  /** Opens a file with its application, or a folder in Explorer. */
  open(path: string): Promise<void>;
  /** Shows an item selected in Explorer. */
  reveal(path: string): Promise<void>;
}

export const FILE_BACKEND = new InjectionToken<FileBackend>('riptide.files.backend', {
  providedIn: 'root',
  factory: () => (inject(TAURI_AVAILABLE) ? new TauriFileBackend(inject(TAURI_INVOKE), inject(TAURI_LISTEN)) : new MemoryFileBackend()),
});

/** The real disk, through the Rust commands. */
export class TauriFileBackend implements FileBackend {
  constructor(
    private readonly invoke: InvokeFn,
    private readonly listen: ListenFn,
  ) {}

  places(): Promise<readonly Place[]> {
    return this.invoke<Place[]>('fs_places');
  }

  capabilities(): Promise<FsCapabilities> {
    return this.invoke<FsCapabilities>('fs_capabilities');
  }

  list(path: string): Promise<Listing> {
    return this.invoke<Listing>('fs_list', { path });
  }

  createFolder(parent: string, name: string): Promise<Entry> {
    return this.invoke<Entry>('fs_create_folder', { parent, name });
  }

  rename(path: string, name: string): Promise<Entry> {
    return this.invoke<Entry>('fs_rename', { path, name });
  }

  trash(paths: readonly string[]): Promise<void> {
    return this.invoke<void>('fs_trash', { paths });
  }

  trashed(parent: string | null): Promise<readonly TrashedItem[]> {
    return this.invoke<TrashedItem[]>('fs_trashed', { parent });
  }

  restore(ids: readonly string[]): Promise<void> {
    return this.invoke<void>('fs_restore', { ids });
  }

  properties(id: number, path: string): Promise<Properties> {
    return this.invoke<Properties>('fs_properties', { id, path });
  }

  search(id: number, dir: string, query: string): Promise<readonly Entry[]> {
    return this.invoke<Entry[]>('fs_search', { id, dir, query });
  }

  plan(sources: readonly string[], dest: string, kind: TransferKind): Promise<TransferPlan> {
    return this.invoke<TransferPlan>('fs_plan', { sources, dest, kind });
  }

  async transfer(id: number, sources: readonly string[], dest: string, kind: TransferKind, choice: ConflictChoice, onProgress: (p: Progress) => void): Promise<TransferReport> {
    const unlisten = await this.listen<Progress & { id: number }>(FS_PROGRESS_EVENT, (p) => {
      if (p.id === id) {
        onProgress(p);
      }
    });
    try {
      return await this.invoke<TransferReport>('fs_transfer', { id, sources, dest, kind, choice });
    } finally {
      unlisten();
    }
  }

  cancel(id: number): Promise<void> {
    return this.invoke<void>('fs_cancel', { id });
  }

  open(path: string): Promise<void> {
    return this.invoke<void>('open_file', { path });
  }

  reveal(path: string): Promise<void> {
    return this.invoke<void>('show_in_folder', { path });
  }
}

/** One node of the in-memory disk. */
interface Node {
  name: string;
  kind: 'file' | 'folder';
  size: number;
  modified: number;
  children?: Map<string, Node>;
}

const SEP = '\\';
const ROOT = 'C:\\';
const INVALID = /[<>:"/\\|?*]/;
/** Control characters (U+0000 to U+001F), which no Windows name may contain. */
const hasControl = (name: string): boolean => [...name].some((c) => c.charCodeAt(0) < 0x20);

/** Splits `C:\a\b` into `['a', 'b']`. */
function parts(path: string): string[] {
  if (!path.startsWith(ROOT)) {
    throw new Error(`not a usable path: ${path}`);
  }
  return path.slice(ROOT.length).split(SEP).filter((p) => p.length > 0 && p !== '.');
}

function join(dir: string, name: string): string {
  return dir.endsWith(SEP) ? `${dir}${name}` : `${dir}${SEP}${name}`;
}

function parentOf(path: string): string | null {
  const p = parts(path);
  if (p.length === 0) {
    return null;
  }
  return p.length === 1 ? ROOT : `${ROOT}${p.slice(0, -1).join(SEP)}`;
}

function nameOf(path: string): string {
  return parts(path).at(-1) ?? path;
}

/** Same rules as `riptide_fs::validate_name`. */
export function nameProblem(name: string): string | null {
  if (name.trim().length === 0) {
    return 'a name cannot be empty';
  }
  if (name === '.' || name === '..') {
    return '"." and ".." are not names';
  }
  if (INVALID.test(name) || hasControl(name)) {
    return 'a name cannot contain < > : " / \\ | ? * or control characters';
  }
  if (name.endsWith('.') || name.endsWith(' ')) {
    return 'a name cannot end with a dot or a space';
  }
  if (/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i.test(name.split('.')[0].trimEnd())) {
    return 'this name is reserved by Windows';
  }
  return null;
}

/**
 * An in-memory disk with a Windows-like layout, for `ng serve` and specs. Behaves like the
 * Rust side where it matters: conflicts are reported, never overwritten; a folder cannot go
 * into itself; delete goes to a restorable bin.
 */
export class MemoryFileBackend implements FileBackend {
  private readonly root: Node = { name: ROOT, kind: 'folder', size: 0, modified: Date.now(), children: new Map() };
  private readonly bin: { id: string; path: string; node: Node; deletedAt: number }[] = [];
  private nextBin = 1;
  private readonly cancelled = new Set<number>();
  /** Milliseconds a transfer waits per item, so specs and demos can see progress. */
  transferDelayMs = 0;

  constructor(seed = true) {
    if (seed) {
      this.seed();
    }
  }

  /** Adds a file (and its folders); for specs and the demo. */
  addFile(path: string, size = 1024, modified = Date.now()): void {
    const p = parts(path);
    const dir = this.ensureFolder(p.slice(0, -1));
    dir.children?.set(p[p.length - 1], { name: p[p.length - 1], kind: 'file', size, modified });
  }

  /** Adds a folder (and its parents). */
  addFolder(path: string): void {
    this.ensureFolder(parts(path));
  }

  exists(path: string): boolean {
    return this.find(path) !== null;
  }

  /** Whether this disk can restore from its bin; specs turn it off to act like macOS. */
  canRestore = true;

  capabilities(): Promise<FsCapabilities> {
    return Promise.resolve({ canRestore: this.canRestore });
  }

  places(): Promise<readonly Place[]> {
    return Promise.resolve([
      { key: 'downloads', label: 'C:\\Users\\You\\Downloads', path: 'C:\\Users\\You\\Downloads' },
      { key: 'organized', label: 'C:\\Users\\You\\Downloads\\Riptide', path: 'C:\\Users\\You\\Downloads\\Riptide' },
      { key: 'documents', label: 'C:\\Users\\You\\Documents', path: 'C:\\Users\\You\\Documents' },
      { key: 'pictures', label: 'C:\\Users\\You\\Pictures', path: 'C:\\Users\\You\\Pictures' },
      { key: 'drive', label: ROOT, path: ROOT },
    ]);
  }

  list(path: string): Promise<Listing> {
    const node = this.find(path);
    if (node === null || node.kind !== 'folder') {
      return Promise.reject(new Error(`"${path}" was not found`));
    }
    const entries = [...(node.children?.values() ?? [])]
      .map((n) => this.entry(join(path, n.name), n))
      .sort((a, b) => Number(a.kind !== 'folder') - Number(b.kind !== 'folder') || a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
    return Promise.resolve({ path, parent: parentOf(path), entries, skipped: 0 });
  }

  createFolder(parent: string, name: string): Promise<Entry> {
    const problem = nameProblem(name);
    const dir = this.find(parent);
    if (problem !== null) {
      return Promise.reject(new Error(problem));
    }
    if (dir?.kind !== 'folder') {
      return Promise.reject(new Error(`"${parent}" was not found`));
    }
    if (this.taken(dir, name)) {
      return Promise.reject(new Error(`"${name}" already exists`));
    }
    const node: Node = { name, kind: 'folder', size: 0, modified: Date.now(), children: new Map() };
    dir.children?.set(name, node);
    return Promise.resolve(this.entry(join(parent, name), node));
  }

  rename(path: string, name: string): Promise<Entry> {
    const problem = nameProblem(name);
    if (problem !== null) {
      return Promise.reject(new Error(problem));
    }
    const parent = parentOf(path);
    const dir = parent === null ? null : this.find(parent);
    const node = this.find(path);
    if (parent === null || dir === null || node === null) {
      return Promise.reject(new Error(`"${path}" was not found`));
    }
    const caseOnly = node.name.toLowerCase() === name.toLowerCase();
    if (!caseOnly && this.taken(dir, name)) {
      return Promise.reject(new Error(`"${name}" already exists`));
    }
    dir.children?.delete(node.name);
    node.name = name;
    dir.children?.set(name, node);
    return Promise.resolve(this.entry(join(parent, name), node));
  }

  trash(paths: readonly string[]): Promise<void> {
    for (const p of paths) {
      if (this.find(p) === null) {
        return Promise.reject(new Error(`"${p}" was not found`));
      }
    }
    for (const p of paths) {
      const node = this.find(p) as Node;
      this.detach(p);
      this.bin.push({ id: String(this.nextBin++), path: p, node, deletedAt: Date.now() });
    }
    return Promise.resolve();
  }

  trashed(parent: string | null): Promise<readonly TrashedItem[]> {
    return Promise.resolve(
      this.bin
        .filter((b) => parent === null || parentOf(b.path) === parent)
        .map((b) => ({ id: b.id, name: b.node.name, original_parent: parentOf(b.path) ?? ROOT, deleted_at: b.deletedAt }))
        .reverse(),
    );
  }

  restore(ids: readonly string[]): Promise<void> {
    for (const id of ids) {
      const i = this.bin.findIndex((b) => b.id === id);
      if (i < 0) {
        return Promise.reject(new Error('some items are no longer in the Recycle Bin'));
      }
      const item = this.bin[i];
      const dir = this.find(parentOf(item.path) ?? ROOT);
      if (dir?.kind !== 'folder' || this.taken(dir, item.node.name)) {
        return Promise.reject(new Error(`"${item.node.name}" cannot go back: its place is taken`));
      }
      dir.children?.set(item.node.name, item.node);
      this.bin.splice(i, 1);
    }
    return Promise.resolve();
  }

  properties(_id: number, path: string): Promise<Properties> {
    const node = this.find(path);
    if (node === null) {
      return Promise.reject(new Error(`"${path}" was not found`));
    }
    const entry = this.entry(path, node);
    if (node.kind === 'file') {
      return Promise.resolve({ entry, files: null, folders: null, total_size: node.size, truncated: false });
    }
    let files = 0;
    let folders = 0;
    let total = 0;
    const walk = (n: Node): void => {
      for (const c of n.children?.values() ?? []) {
        if (c.kind === 'folder') {
          folders++;
          walk(c);
        } else {
          files++;
          total += c.size;
        }
      }
    };
    walk(node);
    return Promise.resolve({ entry, files, folders, total_size: total, truncated: false });
  }

  search(_id: number, dir: string, query: string): Promise<readonly Entry[]> {
    const needle = query.trim().toLowerCase();
    const out: Entry[] = [];
    const start = this.find(dir);
    if (needle.length === 0 || start === null) {
      return Promise.resolve(out);
    }
    const walk = (path: string, n: Node): void => {
      for (const c of n.children?.values() ?? []) {
        const p = join(path, c.name);
        if (c.name.toLowerCase().includes(needle)) {
          out.push(this.entry(p, c));
        }
        if (c.kind === 'folder') {
          walk(p, c);
        }
      }
    };
    walk(dir, start);
    return Promise.resolve(out);
  }

  plan(sources: readonly string[], dest: string, kind: TransferKind): Promise<TransferPlan> {
    const dir = this.find(dest);
    if (dir?.kind !== 'folder') {
      return Promise.reject(new Error(`"${dest}" was not found`));
    }
    let files = 0;
    let bytes = 0;
    const conflicts: Plan['conflicts'] = [];
    const unchanged: string[] = [];
    for (const s of sources) {
      const node = this.find(s);
      if (node === null) {
        return Promise.reject(new Error(`"${nameOf(s)}" was not found`));
      }
      if (this.inside(dest, s)) {
        return Promise.reject(new Error('a folder cannot be copied or moved into itself'));
      }
      if (kind === 'move' && parentOf(s) === dest) {
        unchanged.push(s);
        continue;
      }
      const [f, b] = this.size(node);
      files += f;
      bytes += b;
      const existing = dir.children?.get(node.name);
      if (existing !== undefined) {
        conflicts.push({ source: s, target: join(dest, node.name), target_is_folder: existing.kind === 'folder' });
      }
    }
    return Promise.resolve({ files, bytes, needs_space: kind === 'copy' ? bytes : 0, conflicts, unchanged, free: 512 * 1024 ** 3 });
  }

  async transfer(id: number, sources: readonly string[], dest: string, kind: TransferKind, choice: ConflictChoice, onProgress: (p: Progress) => void): Promise<TransferReport> {
    const plan = await this.plan(sources, dest, kind);
    const dir = this.find(dest) as Node;
    const report: TransferReport = { done: [], skipped: [], failed: [], cancelled: false };
    let done = 0;
    for (const s of sources) {
      if (this.cancelled.has(id)) {
        this.cancelled.delete(id);
        report.cancelled = true;
        break;
      }
      const node = this.find(s) as Node;
      if (kind === 'move' && parentOf(s) === dest) {
        report.skipped.push(s);
        continue;
      }
      let name = node.name;
      if (this.taken(dir, name)) {
        if (choice === 'skip') {
          report.skipped.push(s);
          continue;
        }
        if (choice === 'keepBoth' || join(dest, name) === s) {
          name = this.free(dir, name);
        } else {
          await this.trash([join(dest, name)]);
        }
      }
      if (this.transferDelayMs > 0) {
        await new Promise((r) => setTimeout(r, this.transferDelayMs));
      }
      const copy = kind === 'copy' ? this.clone(node) : node;
      if (kind === 'move') {
        this.detach(s);
      }
      copy.name = name;
      dir.children?.set(name, copy);
      done += this.size(node)[1];
      onProgress({ done_bytes: done, total_bytes: plan.bytes, current: node.name });
      report.done.push({ from: s, to: join(dest, name) });
    }
    return report;
  }

  cancel(id: number): Promise<void> {
    this.cancelled.add(id);
    return Promise.resolve();
  }

  open(): Promise<void> {
    return Promise.resolve();
  }

  reveal(): Promise<void> {
    return Promise.resolve();
  }

  private seed(): void {
    const now = Date.now();
    const day = 86_400_000;
    const d = 'C:\\Users\\You\\Downloads';
    this.addFile(`${d}\\Riptide\\Documents\\annual-report-2026.pdf`, 4_200_000, now - 2 * day);
    this.addFile(`${d}\\Riptide\\Documents\\التقرير-السنوي.docx`, 820_000, now - 5 * day);
    this.addFile(`${d}\\Riptide\\Videos\\keynote-2025-1080p.mp4`, 3_900_000_000, now - 9 * day);
    this.addFile(`${d}\\Riptide\\Archives\\ubuntu-24.04.2-desktop-amd64.iso`, 6_200_000_000, now - 12 * day);
    this.addFile(`${d}\\Riptide\\Applications\\blender-4.3.2-windows-x64.msi`, 300_000_000, now - day);
    this.addFolder(`${d}\\Riptide\\Audio`);
    this.addFolder(`${d}\\Riptide\\Images`);
    this.addFolder(`${d}\\Riptide\\Torrents`);
    this.addFolder(`${d}\\Riptide\\Other`);
    this.addFile(`${d}\\invoice-0042.pdf`, 120_000, now - 3 * day);
    this.addFile('C:\\Users\\You\\Documents\\notes.txt', 2_400, now - 30 * day);
    this.addFile('C:\\Users\\You\\Documents\\Projects\\thesis\\chapter-1.docx', 64_000, now - 40 * day);
    this.addFile('C:\\Users\\You\\Pictures\\holiday\\IMG_0001.jpg', 3_100_000, now - 60 * day);
  }

  private ensureFolder(p: readonly string[]): Node {
    let n = this.root;
    for (const name of p) {
      let next = n.children?.get(name);
      if (next === undefined) {
        next = { name, kind: 'folder', size: 0, modified: Date.now(), children: new Map() };
        n.children?.set(name, next);
      }
      n = next;
    }
    return n;
  }

  private find(path: string): Node | null {
    let n: Node | undefined = this.root;
    try {
      for (const name of parts(path)) {
        n = n?.children?.get(name);
        if (n === undefined) {
          return null;
        }
      }
    } catch {
      return null;
    }
    return n ?? null;
  }

  private detach(path: string): void {
    const parent = parentOf(path);
    const dir = parent === null ? null : this.find(parent);
    dir?.children?.delete(nameOf(path));
  }

  private taken(dir: Node, name: string): boolean {
    return [...(dir.children?.keys() ?? [])].some((k) => k.toLowerCase() === name.toLowerCase());
  }

  private free(dir: Node, name: string): string {
    const dot = name.lastIndexOf('.');
    const stem = dot > 0 ? name.slice(0, dot) : name;
    const ext = dot > 0 ? name.slice(dot) : '';
    for (let i = 1; ; i++) {
      const candidate = `${stem} (${i})${ext}`;
      if (!this.taken(dir, candidate)) {
        return candidate;
      }
    }
  }

  private inside(dest: string, source: string): boolean {
    const d = dest.toLowerCase();
    const s = source.toLowerCase();
    return d === s || d.startsWith(s.endsWith(SEP) ? s : s + SEP);
  }

  private size(n: Node): [number, number] {
    if (n.kind === 'file') {
      return [1, n.size];
    }
    let files = 0;
    let bytes = 0;
    for (const c of n.children?.values() ?? []) {
      const [f, b] = this.size(c);
      files += f;
      bytes += b;
    }
    return [files, bytes];
  }

  private clone(n: Node): Node {
    return { ...n, children: n.children === undefined ? undefined : new Map([...n.children].map(([k, v]) => [k, this.clone(v)])) };
  }

  private entry(path: string, n: Node): Entry {
    return { name: n.name, path, kind: n.kind, size: n.kind === 'file' ? n.size : null, modified: n.modified, created: n.modified, hidden: false, readonly: false };
  }
}
