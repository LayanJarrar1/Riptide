/**
 * Messages exchanged inside the extension (popup / options / content script <-> background).
 * Nothing here ever leaves the browser; the host only sees `protocol.ts` frames.
 */
import type { DownloadId, DownloadState } from './protocol';
import type { HostStatus } from './status';

/** One download the extension handed to Riptide this session. */
export interface RecentItem {
  download_id: DownloadId | null;
  url: string;
  filename: string;
  at: number;
  /** Live state from `list`, when the host forwards it; `null` = unknown ("sent"). */
  state: DownloadState | null;
}

/** Internal message union, discriminated by `kind`. */
export type Message =
  | { kind: 'handoff'; url: string; referrer?: string; filename?: string }
  | { kind: 'handoff-many'; urls: string[]; referrer?: string }
  | { kind: 'alt-bypass'; url: string }
  | { kind: 'get-status' }
  | { kind: 'refresh' }
  | { kind: 'open-app' }
  | { kind: 'set-paused'; paused: boolean }
  | { kind: 'never-ask'; site: string }
  /** Background -> content script. */
  | { kind: 'show-panel'; selectionOnly: boolean };

/** Reply to `get-status`, `refresh`, `open-app`, `set-paused`. */
export interface StatusReply {
  status: HostStatus;
  version: string | null;
  captured: number;
  recent: RecentItem[];
  paused: boolean;
  rulesEnabled: boolean;
  /** `false` when the host refused `list` (an older host forwarded only ping/probe/add/refresh). */
  listAvailable: boolean;
}

/** Reply to `handoff`. */
export interface HandoffReply {
  ok: boolean;
  status: HostStatus;
  download_id?: DownloadId;
  error?: string;
}

/** Reply to `handoff-many`. */
export interface HandoffManyReply {
  ok: number;
  failed: number;
  status: HostStatus;
}

const KINDS = new Set<Message['kind']>([
  'handoff',
  'handoff-many',
  'alt-bypass',
  'get-status',
  'refresh',
  'open-app',
  'set-paused',
  'never-ask',
  'show-panel',
]);

/** Type guard for anything that arrives through `runtime.onMessage`. */
export function isMessage(value: unknown): value is Message {
  if (typeof value !== 'object' || value === null) return false;
  const kind = (value as { kind?: unknown }).kind;
  return typeof kind === 'string' && KINDS.has(kind as Message['kind']);
}
