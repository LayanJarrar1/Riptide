/**
 * The Riptide IPC message schema, mirrored by hand from `crates/riptide-ipc/src/message.rs` and
 * the `riptide-core` types it embeds. The exact JSON shapes are pinned by `protocol.fixtures.json`
 * and `protocol.test.ts`; when the Rust side changes, update both.
 *
 * Wire format: tagged JSON (`"type": "..."`, snake_case) with the request/response `id`
 * flattened next to the tag, so `{ "id": 7, "type": "ping" }`. Downloads are always referred to
 * as `download_id`, never `id`, so the two never collide after flattening. The Rust side is
 * lenient about unknown fields on input; so is this module.
 *
 * Over Native Messaging the browser adds the 4-byte length prefix itself: the extension only ever
 * sees the JSON object.
 */

/** Bumped only for incompatible changes; reported by the server in `pong`. */
export const PROTOCOL_VERSION = 1;

/** Native Messaging host name; the app registers a manifest with this name under `HKCU`. */
export const HOST_NAME = 'app.riptide.host';

// ---------------------------------------------------------------------------------------------
// riptide-core types (as serde serialises them)
// ---------------------------------------------------------------------------------------------

/** Row id of a download (`DownloadId(i64)`, transparent). */
export type DownloadId = number;
/** Row id of a queue; `1` is the built-in Main queue. */
export type QueueId = number;
/** Row id of a category. */
export type CategoryId = number;
/** Scheduling priority inside a queue. */
export type Priority = 'low' | 'normal' | 'high';
/** Hash algorithm for post-completion verification. */
export type ChecksumAlgo = 'md5' | 'sha1' | 'sha256';

/** An expected or computed digest, lower-case hex. */
export interface Checksum {
  algo: ChecksumAlgo;
  hex: string;
}

/** Username / password for HTTP Basic or Digest auth. */
export interface Credentials {
  username: string;
  password: string;
}

/**
 * Per-download secrets. Every field has a serde default, so the extension only sends what it
 * has. Never logged on either side; only ever sent to the origin they were captured for.
 */
export interface Secrets {
  /** Raw `Cookie` header value. */
  cookies?: string | null;
  /** Extra request headers as `[name, value]` pairs (serde serialises a tuple as an array). */
  headers?: [string, string][];
  /** HTTP auth credentials. */
  auth?: Credentials | null;
}

/**
 * What a caller provides to create a download. Everything but `url` is optional on the wire
 * (`#[serde(default)]`), so omitted fields fall back to settings / probe results.
 */
export interface DownloadRequest {
  url: string;
  dir?: string | null;
  filename?: string | null;
  queue_id?: QueueId | null;
  category_id?: CategoryId | null;
  priority?: Priority | null;
  connections?: number | null;
  speed_limit?: number | null;
  checksum?: Checksum | null;
  start_paused?: boolean;
  referrer?: string | null;
  user_agent?: string | null;
  secrets?: Secrets | null;
}

/** Lifecycle state (`#[serde(tag = "state")]`). */
export type DownloadState =
  | { state: 'queued' }
  | { state: 'probing' }
  | { state: 'downloading' }
  | { state: 'paused' }
  | { state: 'verifying' }
  | { state: 'completed' }
  | { state: 'failed'; retryable: boolean }
  | { state: 'cancelled' };

/** Every state name, for exhaustive UI tables. */
export const DOWNLOAD_STATES = [
  'queued',
  'probing',
  'downloading',
  'paused',
  'verifying',
  'completed',
  'failed',
  'cancelled',
] as const;

/** One download as the app persists it (secrets are never included). */
export interface Download {
  id: DownloadId;
  url: string;
  final_url: string | null;
  filename: string;
  dir: string;
  size: number | null;
  supports_ranges: boolean;
  etag: string | null;
  last_modified: string | null;
  content_type: string | null;
  state: DownloadState;
  category_id: CategoryId | null;
  queue_id: QueueId;
  priority: Priority;
  speed_limit: number | null;
  connections: number;
  checksum_expected: Checksum | null;
  checksum_actual: Checksum | null;
  error: string | null;
  added_at: number;
  started_at: number | null;
  completed_at: number | null;
}

/** What a probe learned. */
export interface ProbeResult {
  final_url: string;
  filename: string;
  size: number | null;
  supports_ranges: boolean;
  content_type: string | null;
}

/** Engine events, delivered after `subscribe` (CLI only; listed for completeness). */
export type Event =
  | { type: 'added'; id: DownloadId }
  | {
      type: 'probed';
      id: DownloadId;
      size: number | null;
      supports_ranges: boolean;
      filename: string;
      content_type: string | null;
    }
  | { type: 'state_changed'; id: DownloadId; from: DownloadState; to: DownloadState }
  | {
      type: 'progress';
      id: DownloadId;
      downloaded: number;
      total: number | null;
      speed_bps: number;
      eta_secs: number | null;
      connections: number;
    }
  | { type: 'segment_progress'; id: DownloadId; index: number; written: number; len: number }
  | { type: 'completed'; id: DownloadId; path: string }
  | { type: 'failed'; id: DownloadId; error: string; retryable: boolean }
  | { type: 'removed'; id: DownloadId };

// ---------------------------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------------------------

/**
 * Request payloads. The host forwards only `ping`, `probe`, `add`, `refresh` and the read-only
 * `list` from the browser (hard rule 5: the browser can only ask). The rest exist for the CLI;
 * the extension must still cope with an `error` answer to `list` (an older host).
 */
export type RequestBody =
  | { type: 'ping' }
  | {
      type: 'probe';
      url: string;
      referrer?: string | null;
      user_agent?: string | null;
      secrets?: Secrets | null;
    }
  | { type: 'add'; request: DownloadRequest }
  | {
      type: 'refresh';
      download_id: DownloadId;
      url: string;
      referrer?: string | null;
      secrets?: Secrets | null;
    }
  | { type: 'list' }
  | { type: 'get'; download_id: DownloadId }
  | { type: 'pause'; download_id: DownloadId }
  | { type: 'resume'; download_id: DownloadId }
  | { type: 'cancel'; download_id: DownloadId }
  | { type: 'remove'; download_id: DownloadId; delete_file?: boolean }
  | { type: 'subscribe' }
  | { type: 'shutdown' };

/** Request type tags, in the order of the Rust enum. */
export const REQUEST_TYPES = [
  'ping',
  'probe',
  'add',
  'refresh',
  'list',
  'get',
  'pause',
  'resume',
  'cancel',
  'remove',
  'subscribe',
  'shutdown',
] as const;

/** The request types a browser is allowed to send through the Native Messaging host. */
export const BROWSER_REQUEST_TYPES = ['ping', 'probe', 'add', 'refresh', 'list'] as const;

/** Client → server, with the correlation id flattened next to the body. */
export type Request = { id: number } & RequestBody;

// ---------------------------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------------------------

/** Error classes a client can act on. */
export type ErrorCode = 'bad_request' | 'not_found' | 'illegal_transition' | 'unsupported' | 'internal';

/** Every error code, for validation. */
export const ERROR_CODES: readonly ErrorCode[] = [
  'bad_request',
  'not_found',
  'illegal_transition',
  'unsupported',
  'internal',
];

/** Response payloads. */
export type ResponseBody =
  | { type: 'pong'; version: string; protocol: number }
  | { type: 'probed'; result: ProbeResult }
  | { type: 'added'; download_id: DownloadId }
  | { type: 'downloads'; downloads: Download[] }
  | { type: 'download'; download: Download }
  | { type: 'ok' }
  | { type: 'event'; event: Event }
  | { type: 'error'; code: ErrorCode; message: string };

/** Response type tags, in the order of the Rust enum. */
export const RESPONSE_TYPES = [
  'pong',
  'probed',
  'added',
  'downloads',
  'download',
  'ok',
  'event',
  'error',
] as const;

/** Server → client; `id` is `0` for unsolicited frames (events). */
export type Response = { id: number } & ResponseBody;

/** A frame that did not match the schema. */
export class ProtocolError extends Error {
  override readonly name = 'ProtocolError';
}

// ---------------------------------------------------------------------------------------------
// Encode / decode
// ---------------------------------------------------------------------------------------------

/** Build a request object with the id first, exactly as serde flattens it. */
export function makeRequest(id: number, body: RequestBody): Request {
  if (!Number.isInteger(id) || id < 0) {
    throw new ProtocolError(`request id must be a non-negative integer, got ${String(id)}`);
  }
  return { id, ...body };
}

/** JSON text of a request (same key order as the Rust side: `id`, `type`, fields). */
export function encodeRequest(id: number, body: RequestBody): string {
  return JSON.stringify(makeRequest(id, body));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isResponseType(value: unknown): value is ResponseBody['type'] {
  return typeof value === 'string' && (RESPONSE_TYPES as readonly string[]).includes(value);
}

function isErrorCode(value: unknown): value is ErrorCode {
  return typeof value === 'string' && (ERROR_CODES as readonly string[]).includes(value);
}

function isDownloadState(value: unknown): value is DownloadState {
  if (!isRecord(value) || typeof value['state'] !== 'string') return false;
  if (!(DOWNLOAD_STATES as readonly string[]).includes(value['state'])) return false;
  return value['state'] !== 'failed' || typeof value['retryable'] === 'boolean';
}

function isDownload(value: unknown): value is Download {
  return (
    isRecord(value) &&
    typeof value['id'] === 'number' &&
    typeof value['url'] === 'string' &&
    typeof value['filename'] === 'string' &&
    isDownloadState(value['state'])
  );
}

/**
 * Validate a decoded frame from the host. Unknown fields are tolerated (like the Rust side);
 * an unknown `type`, a missing `id`, or a malformed payload throws [`ProtocolError`].
 */
export function parseResponse(raw: unknown): Response {
  if (!isRecord(raw)) throw new ProtocolError('response is not an object');
  const id = raw['id'];
  if (typeof id !== 'number' || !Number.isInteger(id) || id < 0) {
    throw new ProtocolError('response has no integer id');
  }
  const type = raw['type'];
  if (!isResponseType(type)) throw new ProtocolError(`unknown response type ${JSON.stringify(type)}`);

  switch (type) {
    case 'pong': {
      const version = raw['version'];
      const protocol = raw['protocol'];
      if (typeof version !== 'string' || typeof protocol !== 'number') {
        throw new ProtocolError('malformed pong');
      }
      return { id, type, version, protocol };
    }
    case 'probed': {
      const result = raw['result'];
      if (
        !isRecord(result) ||
        typeof result['final_url'] !== 'string' ||
        typeof result['filename'] !== 'string' ||
        typeof result['supports_ranges'] !== 'boolean'
      ) {
        throw new ProtocolError('malformed probed');
      }
      return {
        id,
        type,
        result: {
          final_url: result['final_url'],
          filename: result['filename'],
          size: typeof result['size'] === 'number' ? result['size'] : null,
          supports_ranges: result['supports_ranges'],
          content_type: typeof result['content_type'] === 'string' ? result['content_type'] : null,
        },
      };
    }
    case 'added': {
      const downloadId = raw['download_id'];
      if (typeof downloadId !== 'number') throw new ProtocolError('malformed added');
      return { id, type, download_id: downloadId };
    }
    case 'downloads': {
      const downloads = raw['downloads'];
      if (!Array.isArray(downloads) || !downloads.every(isDownload)) {
        throw new ProtocolError('malformed downloads');
      }
      return { id, type, downloads };
    }
    case 'download': {
      const download = raw['download'];
      if (!isDownload(download)) throw new ProtocolError('malformed download');
      return { id, type, download };
    }
    case 'ok':
      return { id, type };
    case 'event': {
      const event = raw['event'];
      if (!isRecord(event) || typeof event['type'] !== 'string' || typeof event['id'] !== 'number') {
        throw new ProtocolError('malformed event');
      }
      return { id, type, event: event as unknown as Event };
    }
    case 'error': {
      const code = raw['code'];
      const message = raw['message'];
      if (!isErrorCode(code) || typeof message !== 'string') throw new ProtocolError('malformed error');
      return { id, type, code, message };
    }
  }
}

/** `parseResponse` for a JSON string. */
export function decodeResponse(json: string): Response {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch (e) {
    throw new ProtocolError(`response is not JSON: ${e instanceof Error ? e.message : String(e)}`);
  }
  return parseResponse(raw);
}
