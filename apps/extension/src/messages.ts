/**
 * Messages exchanged inside the extension (popup / options / content script <-> background).
 * Nothing here ever leaves the browser; the host only sees `protocol.ts` frames.
 */
import type { DownloadId, DownloadState, MediaInfo } from './protocol';
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
  /** Content -> background: the page has a video (`count` of them) worth a badge. */
  | { kind: 'media-found'; count: number }
  /** Content -> background: has the network seen media on this tab? */
  | { kind: 'media-query' }
  /** Content -> background: ask the app for the quality menu of a video page. */
  | { kind: 'media-probe'; url: string }
  /** Content -> background: add a video page with a quality key. */
  | { kind: 'media-add'; url: string; quality: string; title?: string }
  /** Content -> background: never show the video button on this site. */
  | { kind: 'video-never'; site: string }
  /** Background -> content script. */
  | { kind: 'show-panel'; selectionOnly: boolean }
  /** Background -> content script: open the quality picker for this page (context menu). */
  | { kind: 'show-video'; url: string }
  /** Background -> content script: the network saw a media response on this tab. */
  | { kind: 'media-hint' };

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

/** Reply to `media-probe`. */
export interface MediaProbeReply {
  ok: boolean;
  status: HostStatus;
  info?: MediaInfo;
  /** The host's or app's own message, shown as is. */
  error?: string;
}

/** Reply to `media-query`. */
export interface MediaQueryReply {
  seen: number;
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
  'media-found',
  'media-query',
  'media-probe',
  'media-add',
  'video-never',
  'show-video',
  'media-hint',
]);

/** Type guard for anything that arrives through `runtime.onMessage`. */
export function isMessage(value: unknown): value is Message {
  if (typeof value !== 'object' || value === null) return false;
  const kind = (value as { kind?: unknown }).kind;
  return typeof kind === 'string' && KINDS.has(kind as Message['kind']);
}
