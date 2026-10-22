/**
 * Pure conversions between the generated IPC bindings (`src/app/tauri/bridge`, snake_case,
 * state objects) and the UI model (`src/app/model/download`, camelCase, state strings).
 * No Angular, no IO: everything here is unit-tested in isolation.
 */

import {
  detectCategory,
  MediaProbe,
  Download as UiDownload,
  DownloadRequest as UiDownloadRequest,
  DownloadState as UiDownloadState,
  FailureKind,
  ProbeResult,
  Segment as UiSegment,
} from '../model/download';
import { parseMediaError } from '../model/media-error';
import type { Download } from '../tauri/bridge/Download';
import type { DownloadRequest } from '../tauri/bridge/DownloadRequest';
import type { DownloadState } from '../tauri/bridge/DownloadState';
import type { MediaInfo } from '../tauri/bridge/MediaInfo';
import type { ProbeInfo } from '../tauri/bridge/ProbeInfo';
import type { Segment } from '../tauri/bridge/Segment';
import type { TorrentView } from '../model/torrent';

/** Samples kept per speed history (one per second). */
export const HISTORY_LENGTH = 60;

/**
 * A 64-bit integer that may be `None` on the Rust side (`number | null` in the bindings).
 * `undefined` is tolerated so a partially filled record from a test or an older build maps too.
 */
type Maybe = number | null | undefined;

/** Below this a timestamp is taken to be in seconds (5138 CE) rather than milliseconds (1973). */
const SECONDS_LIMIT = 1e11;

/** A fresh, flat speed history. */
export function emptyHistory(): readonly number[] {
  return new Array<number>(HISTORY_LENGTH).fill(0);
}

/** `{ state: "failed", retryable }` and friends → the UI's string union. */
export function toUiState(state: DownloadState): UiDownloadState {
  return state.state;
}

/** Query words that make a 403 plausibly an expired signed link rather than a plain refusal. */
const SIGNED_URL = /[?&](expires?|signature|sig|token|x-amz-[a-z-]+|se|exp|hdnts|policy|key-pair-id)=|\/expires\b/i;

/**
 * The UI's failure kind, derived from the engine's human-readable error text (case-insensitive):
 * "checksum" → `checksum`; "authentication failed" or 401/407 → `auth`; 404/410 → `notfound`;
 * "expired", or a 403 on a URL that looks signed (`url`) → `expired`; any other 403 → `denied`;
 * "Content-Length" or "declared" → `server`; "disk" or "space" → `disk`; anything else
 * (including no text) → `network`.
 */
export function failureKindOf(error: string | null | undefined, url?: string | null): FailureKind {
  const text = (error ?? '').toLowerCase();
  // a tagged video refusal first: its raw text may say "403" without the link having expired
  const tagged = parseMediaError(error ?? '').kind;
  if (tagged !== null) {
    return tagged;
  }
  if (text.includes('checksum')) {
    return 'checksum';
  }
  if (text.includes('expired')) {
    return 'expired';
  }
  if (text.includes('authentication failed') || /\b(401|407)\b/.test(text)) {
    return 'auth';
  }
  if (/\b(404|410)\b/.test(text) || text.includes('not found')) {
    return 'notfound';
  }
  if (text.includes('403')) {
    return SIGNED_URL.test(url ?? '') ? 'expired' : 'denied';
  }
  if (text.includes('content-length') || text.includes('declared')) {
    return 'server';
  }
  if (text.includes('disk') || text.includes('space')) {
    return 'disk';
  }
  return 'network';
}

/** The plain-text details of a failed video download (yt-dlp's own line), `null` when there are none. */
export function errorDetailOf(error: string | null | undefined): string | null {
  const details = parseMediaError(error ?? '').details;
  return details.length === 0 ? null : details;
}

/** A byte count, `null` when the engine does not know it. */
export function toUiSize(n: Maybe): number | null {
  return typeof n === 'number' && Number.isFinite(n) ? n : null;
}

/**
 * Engine timestamp (`UnixMillis`) → milliseconds since the epoch, which is what the UI's date
 * pipes expect. Values that look like seconds are promoted as a safety net; missing, zero or
 * negative → `null` ("not yet").
 */
export function toMillis(n: Maybe): number | null {
  if (typeof n !== 'number' || !Number.isFinite(n) || n <= 0) {
    return null;
  }
  return n < SECONDS_LIMIT ? Math.round(n * 1000) : n;
}

/** One persisted segment → the UI segment (drops `download_id`). */
export function toUiSegment(s: Segment): UiSegment {
  return { index: s.index, start: s.start, end: s.end, written: s.written, state: s.state };
}

/**
 * Segments in index order. When the engine has none yet (not probed, or already cleaned up
 * after completion) the UI gets a placeholder so progress and the segment map still render:
 * one done range for a completed file of known size, otherwise one empty pending range like
 * an unknown-size stream.
 */
function toUiSegments(segments: readonly Segment[], state: UiDownloadState, size: number | null): readonly UiSegment[] {
  if (segments.length > 0) {
    return [...segments].sort((a, b) => a.index - b.index).map(toUiSegment);
  }
  if (state === 'completed' && size !== null && size > 0) {
    return [{ index: 0, start: 0, end: size, written: size, state: 'done' }];
  }
  return [{ index: 0, start: 0, end: 0, written: 0, state: 'pending' }];
}

/**
 * A persisted download plus its segments → the UI model. `speedHistory` is carried over from
 * `previous` (the record on the wire has no speed); `speedBps` too, but only while the download
 * is still `downloading` — any other state means it is not moving.
 */
export function toUiDownload(d: Download, segments: readonly Segment[], previous?: UiDownload, torrent?: TorrentView): UiDownload {
  const state = toUiState(d.state);
  const size = toUiSize(d.size);
  const torrentView = d.kind === 'torrent' ? (torrent ?? previous?.torrent) : undefined;
  // a torrent has no segments either: its verified byte count (kept in the torrents table) is the progress
  const torrentSegments: readonly UiSegment[] | null =
    d.kind === 'torrent' && segments.length === 0
      ? [{ index: 0, start: 0, end: size ?? 0, written: state === 'completed' ? (size ?? 0) : Math.min(torrentView?.downloaded ?? previous?.segments[0]?.written ?? 0, size ?? Number.MAX_SAFE_INTEGER), state: state === 'completed' ? 'done' : 'active' }]
      : null;
  // a media download has no segments; keep the progress the events built while it is unfinished
  const keepMediaProgress =
    d.kind === 'media' && segments.length === 0 && state !== 'completed' && previous !== undefined && previous.segments.some((s) => s.written > 0);
  return {
    kind: d.kind,
    id: d.id,
    url: d.url,
    filename: d.filename,
    dir: d.dir,
    size,
    supportsRanges: d.supports_ranges,
    state,
    category: detectCategory(d.filename, d.content_type),
    queueId: d.queue_id,
    queuePosition: d.queue_position,
    priority: d.priority,
    connections: d.connections,
    speedLimitBps: toUiSize(d.speed_limit),
    segments: torrentSegments ?? (keepMediaProgress && previous !== undefined ? previous.segments : toUiSegments(segments, state, size)),
    ...(torrentView === undefined ? {} : { torrent: torrentView }),
    speedBps: state === 'downloading' ? (previous?.speedBps ?? 0) : 0,
    speedHistory: previous?.speedHistory ?? emptyHistory(),
    addedAt: toMillis(d.added_at) ?? previous?.addedAt ?? 0,
    startedAt: toMillis(d.started_at),
    completedAt: toMillis(d.completed_at),
    activeMs: typeof d.active_ms === 'number' && Number.isFinite(d.active_ms) && d.active_ms >= 0 ? d.active_ms : null,
    activeSince: toMillis(d.active_since),
    failure: state === 'failed' ? failureKindOf(d.error, d.url) : null,
    errorDetail: state === 'failed' && d.kind === 'media' ? errorDetailOf(d.error) : null,
    checksum: d.checksum_expected === null ? null : { algo: d.checksum_expected.algo, hex: d.checksum_expected.hex },
  };
}

/**
 * What the add dialog collected → the engine's request. Everything the dialog does not set is
 * `null` ("use the default"): queue, category (the engine assigns one by extension), priority,
 * referrer, user agent and secrets; the speed cap is "unlimited".
 */
export function toBridgeRequest(r: UiDownloadRequest): DownloadRequest {
  return {
    url: r.url,
    dir: r.dir ?? null,
    filename: r.filename ?? null,
    queue_id: null,
    category_id: null,
    priority: null,
    connections: r.connections ?? null,
    speed_limit: null,
    checksum: r.checksum ? { algo: r.checksum.algo, hex: r.checksum.hex } : null,
    start_paused: r.startPaused ?? false,
    referrer: null,
    user_agent: null,
    secrets: null,
    kind: r.kind ?? 'http',
    media_format: r.kind === 'media' ? (r.mediaFormat ?? 'best') : null,
  };
}

/** yt-dlp's view of a video page → what the quality picker shows. */
export function toUiMedia(info: MediaInfo): MediaProbe {
  return {
    title: info.title,
    uploader: info.uploader,
    thumbnail: info.thumbnail,
    duration: info.duration,
    choices: info.choices.map((c) => ({
      key: c.key,
      kind: c.kind,
      height: c.height,
      ext: c.ext,
      approxSize: toUiSize(c.approx_size),
    })),
  };
}

/** The engine's probe answer → what the add dialog previews. */
export function toUiProbe(p: ProbeInfo): ProbeResult {
  return {
    finalUrl: p.final_url,
    filename: p.filename,
    size: toUiSize(p.size),
    supportsRanges: p.supports_ranges,
    contentType: p.content_type,
    category: detectCategory(p.filename, p.content_type),
  };
}

/**
 * Where the finished file lives: `dir` joined with `filename` using the separator the directory
 * already uses (a drive letter or a backslash means Windows). A trailing separator on `dir` is
 * not doubled.
 */
export function finalPath(dir: string, filename: string): string {
  const windows = /^[A-Za-z]:|\\/.test(dir);
  const trimmed = dir.replace(/[\\/]+$/, '');
  return `${trimmed}${windows ? '\\' : '/'}${filename}`;
}
