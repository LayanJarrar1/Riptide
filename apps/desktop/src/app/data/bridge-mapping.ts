/**
 * Pure conversions between the generated IPC bindings (`src/app/tauri/bridge`, snake_case,
 * state objects) and the UI model (`src/app/model/download`, camelCase, state strings).
 * No Angular, no IO: everything here is unit-tested in isolation.
 */

import {
  categoryOf,
  Download as UiDownload,
  DownloadRequest as UiDownloadRequest,
  DownloadState as UiDownloadState,
  FailureKind,
  ProbeResult,
  Segment as UiSegment,
} from '../model/download';
import type { Download } from '../tauri/bridge/Download';
import type { DownloadRequest } from '../tauri/bridge/DownloadRequest';
import type { DownloadState } from '../tauri/bridge/DownloadState';
import type { ProbeInfo } from '../tauri/bridge/ProbeInfo';
import type { Segment } from '../tauri/bridge/Segment';

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

/**
 * The UI's failure kind, derived from the engine's human-readable error text (case-insensitive):
 * "checksum" → `checksum`; "403" or "expired" → `expired`; "Content-Length" or "declared" →
 * `server`; "disk" or "space" → `disk`; anything else (including no text) → `network`.
 */
export function failureKindOf(error: string | null | undefined): FailureKind {
  const text = (error ?? '').toLowerCase();
  if (text.includes('checksum')) {
    return 'checksum';
  }
  if (text.includes('403') || text.includes('expired')) {
    return 'expired';
  }
  if (text.includes('content-length') || text.includes('declared')) {
    return 'server';
  }
  if (text.includes('disk') || text.includes('space')) {
    return 'disk';
  }
  return 'network';
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
export function toUiDownload(d: Download, segments: readonly Segment[], previous?: UiDownload): UiDownload {
  const state = toUiState(d.state);
  const size = toUiSize(d.size);
  return {
    id: d.id,
    url: d.url,
    filename: d.filename,
    dir: d.dir,
    size,
    supportsRanges: d.supports_ranges,
    state,
    category: categoryOf(d.filename),
    queueId: d.queue_id,
    priority: d.priority,
    connections: d.connections,
    speedLimitBps: toUiSize(d.speed_limit),
    segments: toUiSegments(segments, state, size),
    speedBps: state === 'downloading' ? (previous?.speedBps ?? 0) : 0,
    speedHistory: previous?.speedHistory ?? emptyHistory(),
    addedAt: toMillis(d.added_at) ?? previous?.addedAt ?? 0,
    startedAt: toMillis(d.started_at),
    completedAt: toMillis(d.completed_at),
    failure: state === 'failed' ? failureKindOf(d.error) : null,
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
    category: categoryOf(p.filename),
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
