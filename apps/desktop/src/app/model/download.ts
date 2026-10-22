import type { TorrentView } from './torrent';

/**
 * UI-side model of a download. Mirrors `riptide_core::Download` / `Segment` (camelCase here,
 * snake_case on the wire); the IPC mapping lives in data/bridge-mapping.ts. Until the engine is wired the
 * `MockEngineService` produces these.
 */

/** Lifecycle state, identical to the Rust enum names. */
export type DownloadState =
  | 'queued'
  | 'probing'
  | 'downloading'
  | 'paused'
  | 'verifying'
  | 'completed'
  | 'failed'
  | 'cancelled';

/** Every state, in the order the sidebar and filters use. */
export const DOWNLOAD_STATES: readonly DownloadState[] = [
  'queued',
  'probing',
  'downloading',
  'paused',
  'verifying',
  'completed',
  'failed',
  'cancelled',
];

/** Lifecycle of one segment. */
export type SegmentState = 'pending' | 'active' | 'done' | 'failed';

/** One byte range `[start, end)` of a download and how much of it is on disk. */
export interface Segment {
  readonly index: number;
  readonly start: number;
  readonly end: number;
  readonly written: number;
  readonly state: SegmentState;
}

/** Category keys match the `categories.key` column; the UI translates them. */
export type CategoryKey =
  | 'compressed'
  | 'documents'
  | 'music'
  | 'video'
  | 'programs'
  | 'images'
  | 'other';

/** Category keys in sidebar order. */
export const CATEGORY_KEYS: readonly CategoryKey[] = [
  'compressed',
  'documents',
  'music',
  'video',
  'programs',
  'images',
  'other',
];

/** Scheduling priority inside a queue. */
export type Priority = 'low' | 'normal' | 'high';

/** Why a download failed, for the status text and the retry affordance. */
export type FailureKind = 'network' | 'checksum' | 'expired' | 'disk' | 'server' | 'login' | 'refused' | 'auth' | 'denied' | 'notfound';

/** What the user can do about a failed download: a retry, a trip back to the add dialog, or nothing. */
export interface FailureFix {
  readonly action: 'retry' | 'refreshLink';
  /** The i18n key of the button label. */
  readonly labelKey: string;
}

/**
 * The fix offered for a failure kind. A missing file (404/410) is permanent, so nothing is
 * offered; a rejected login or a refused request goes back to the add dialog to enter
 * credentials; an expired signed link asks for a new link; everything else is worth a retry.
 */
export function failureFix(failure: FailureKind | null): FailureFix | null {
  switch (failure) {
    case 'notfound':
      return null;
    case 'expired':
      return { action: 'refreshLink', labelKey: 'action.refreshLink' };
    case 'auth':
    case 'denied':
      return { action: 'refreshLink', labelKey: 'action.enterCredentials' };
    default:
      return { action: 'retry', labelKey: 'action.retry' };
  }
}

/** One download as the UI sees it. */
export interface Download {
  readonly id: number;
  readonly url: string;
  readonly filename: string;
  readonly dir: string;
  /** Total bytes, `null` while unknown (no `Content-Length`). */
  readonly size: number | null;
  readonly supportsRanges: boolean;
  readonly state: DownloadState;
  readonly category: CategoryKey;
  readonly queueId: number;
  /** Place in the queue's manual order (smaller runs first); absent = unknown. */
  readonly queuePosition?: number;
  readonly priority: Priority;
  /** Simultaneous connections the engine may open. */
  readonly connections: number;
  readonly segments: readonly Segment[];
  /** Smoothed bytes per second; `0` when idle. */
  readonly speedBps: number;
  /** Last 60 speed samples (one per second), newest last. */
  readonly speedHistory: readonly number[];
  readonly addedAt: number;
  readonly startedAt: number | null;
  readonly completedAt: number | null;
  /**
   * Milliseconds spent downloading (paused, queued and failed time excluded), up to the last
   * checkpoint. `null` or absent: never recorded (added before the engine tracked it).
   */
  readonly activeMs?: number | null;
  /** Start of the running span while `downloading`; the live time adds `now - activeSince`. */
  readonly activeSince?: number | null;
  readonly failure: FailureKind | null;
  /** For a refused video: yt-dlp's own error line (plain text, at most 300 characters), shown behind "Details". */
  readonly errorDetail?: string | null;
  /** This download's own cap in bytes per second; `null` or absent = unlimited. */
  readonly speedLimitBps?: number | null;
  /** Expected digest, if the user gave one. */
  readonly checksum: { readonly algo: 'md5' | 'sha1' | 'sha256'; readonly hex: string } | null;
  /** `media` = a video page fetched by yt-dlp (no segments); `torrent` = BitTorrent; absent or `http` = a plain file. */
  readonly kind?: DownloadKind;
  /** The torrent's files, peers and ratio (only for `kind: 'torrent'`). */
  readonly torrent?: TorrentView;
}

/** Which engine moves the bytes. */
export type DownloadKind = 'http' | 'media' | 'torrent';

/** One entry of a video page's quality menu. Mirrors `riptide_core::MediaChoice`. */
export interface MediaChoice {
  /** Sent back as `mediaFormat`: `best`, `h720`, `audio-m4a`, `audio-mp3`. */
  readonly key: string;
  readonly kind: 'best' | 'video' | 'audio';
  readonly height: number | null;
  readonly ext: string;
  readonly approxSize: number | null;
}

/** What a video page offers. */
export interface MediaProbe {
  readonly title: string;
  readonly uploader: string | null;
  readonly thumbnail: string | null;
  readonly duration: number | null;
  readonly choices: readonly MediaChoice[];
}

/**
 * Sites the add dialog treats as video pages straight away. Must match `MEDIA_HOSTS` in
 * `crates/riptide-core/src/media.rs`; other pages still get a media probe when the plain probe
 * finds HTML.
 */
export const MEDIA_HOSTS: readonly string[] = [
  'youtube.com',
  'youtu.be',
  'youtube-nocookie.com',
  'vimeo.com',
  'dailymotion.com',
  'dai.ly',
  'twitch.tv',
  'soundcloud.com',
  'bilibili.com',
  'streamable.com',
  'ted.com',
  'tiktok.com',
  'instagram.com',
  'facebook.com',
  'fb.watch',
  'x.com',
  'twitter.com',
  'reddit.com',
  'rumble.com',
  'odysee.com',
];

/** `true` when `url` is on one of the [`MEDIA_HOSTS`] (or a subdomain). */
export function isMediaUrl(url: string): boolean {
  try {
    const host = new URL(url.trim()).hostname.toLowerCase().replace(/\.$/, '');
    return MEDIA_HOSTS.some((h) => host === h || host.endsWith(`.${h}`));
  } catch {
    return false;
  }
}

/** Bytes on disk across all segments. */
export function downloadedBytes(d: Pick<Download, 'segments'>): number {
  let total = 0;
  for (const s of d.segments) {
    total += s.written;
  }
  return total;
}

/** 0..1, or `null` when the size is unknown. */
export function progressOf(d: Pick<Download, 'segments' | 'size' | 'state'>): number | null {
  if (d.state === 'completed') {
    return 1;
  }
  if (d.size === null || d.size === 0) {
    return null;
  }
  return Math.min(1, downloadedBytes(d) / d.size);
}

/** Seconds left at the current speed, `null` when it cannot be estimated. */
/** Time spent downloading as of `now`, or `null` when it was never recorded. */
export function activeMillis(d: Pick<Download, 'activeMs' | 'activeSince' | 'state'>, now: number): number | null {
  if (d.activeMs === null || d.activeMs === undefined) {
    return null;
  }
  const running = d.state === 'downloading' && typeof d.activeSince === 'number' ? Math.max(0, now - d.activeSince) : 0;
  return d.activeMs + running;
}

/**
 * Average speed over the time spent downloading, in bytes per second: `null` while the time is
 * unknown or under a second (an average over a few milliseconds says nothing). Distinct from
 * `speedBps`, which is the current, smoothed speed.
 */
export function averageSpeed(downloaded: number, activeMs: number | null): number | null {
  if (activeMs === null || activeMs < 1000 || downloaded <= 0) {
    return null;
  }
  return (downloaded * 1000) / activeMs;
}

export function etaSeconds(d: Pick<Download, 'segments' | 'size' | 'state' | 'speedBps'>): number | null {
  if (d.state !== 'downloading' || d.size === null || d.speedBps <= 0) {
    return null;
  }
  const remaining = d.size - downloadedBytes(d);
  return Math.max(0, Math.ceil(remaining / d.speedBps));
}

/** Whether the item is consuming a queue slot. */
export function isActive(state: DownloadState): boolean {
  return state === 'probing' || state === 'downloading' || state === 'verifying';
}

/** Whether "pause" applies. */
export function canPause(state: DownloadState): boolean {
  return state === 'queued' || state === 'probing' || state === 'downloading';
}

/** Whether "resume" / "retry" applies. */
export function canResume(state: DownloadState): boolean {
  return state === 'paused' || state === 'failed';
}

/** Terminal states never change again. */
export function isTerminal(state: DownloadState): boolean {
  return state === 'completed' || state === 'cancelled';
}

/** What the user asked to add. Mirrors `riptide_core::DownloadRequest`. */
export interface DownloadRequest {
  readonly url: string;
  readonly filename?: string;
  readonly dir?: string;
  readonly category?: CategoryKey;
  readonly connections?: number;
  readonly startPaused?: boolean;
  readonly checksum?: Download['checksum'];
  /** `media` for a video page; `mediaFormat` is then a [`MediaChoice`] key. */
  readonly kind?: DownloadKind;
  readonly mediaFormat?: string;
  /** Add even if the "skip duplicates" option is on and this was downloaded before. */
  readonly allowDuplicate?: boolean;
}

/** Options shared by every item of a batch. Mirrors `riptide_core::BatchOptions`. */
export interface BatchOptions {
  readonly dir?: string;
  readonly connections?: number;
  readonly startPaused?: boolean;
}

/** Why the engine left a batch item out. */
export type BatchSkipReason = 'duplicate' | 'invalid';

/** A batch item the engine did not add. */
export interface BatchSkipped {
  readonly url: string;
  readonly reason: BatchSkipReason;
}

/** What `addBatch` did: the new ids and what was skipped. */
export interface BatchResult {
  readonly added: readonly number[];
  readonly skipped: readonly BatchSkipped[];
}

/** Result of probing a URL before adding it. */
export interface ProbeResult {
  readonly finalUrl: string;
  readonly filename: string;
  readonly size: number | null;
  readonly supportsRanges: boolean;
  readonly contentType: string | null;
  readonly category: CategoryKey;
}

/**
 * Extension → category. Must stay identical to `Category::extensions()` in
 * `crates/riptide-core/src/category.rs` (the engine classifies with that table and its
 * `extension_table_is_pinned` test fails when it changes); `category.spec.ts` pins this copy.
 */
export const CATEGORY_EXTENSIONS: Readonly<Record<CategoryKey, readonly string[]>> = {
  compressed: ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst', 'iso', 'cab'],
  documents: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp', 'txt', 'rtf', 'epub', 'mobi', 'csv'],
  music: ['mp3', 'flac', 'wav', 'aac', 'm4a', 'ogg', 'opus', 'wma', 'aiff'],
  video: ['mp4', 'mkv', 'avi', 'mov', 'wmv', 'webm', 'm4v', 'ts', 'flv', 'mpg', 'mpeg'],
  programs: ['exe', 'msi', 'msix', 'appx', 'apk', 'dmg', 'pkg', 'deb', 'rpm', 'appimage'],
  images: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg', 'tif', 'tiff', 'heic', 'avif', 'psd'],
  other: [],
};

/** Category for a file name, by extension. */
export function categoryOf(filename: string): CategoryKey {
  const dot = filename.lastIndexOf('.');
  if (dot < 0) {
    return 'other';
  }
  const ext = filename.slice(dot + 1).toLowerCase();
  for (const key of CATEGORY_KEYS) {
    if (CATEGORY_EXTENSIONS[key].includes(ext)) {
      return key;
    }
  }
  return 'other';
}

/** Category of a `Content-Type`; `null` when it does not tell. Mirrors `Category::from_mime`. */
export function categoryOfMime(contentType: string | null | undefined): CategoryKey | null {
  const mime = (contentType ?? '').split(';')[0].trim().toLowerCase();
  const slash = mime.indexOf('/');
  if (slash < 0) {
    return null;
  }
  const kind = mime.slice(0, slash);
  const sub = mime.slice(slash + 1);
  if (kind === 'video') {
    return 'video';
  }
  if (kind === 'audio') {
    return 'music';
  }
  if (kind === 'image') {
    return 'images';
  }
  if (kind === 'text') {
    return ['plain', 'csv', 'rtf'].includes(sub) ? 'documents' : null;
  }
  if (kind !== 'application') {
    return null;
  }
  if (
    ['pdf', 'msword', 'rtf', 'epub+zip', 'vnd.ms-excel', 'vnd.ms-powerpoint'].includes(sub) ||
    sub.startsWith('vnd.openxmlformats-officedocument.') ||
    sub.startsWith('vnd.oasis.opendocument.')
  ) {
    return 'documents';
  }
  if (['zip', 'x-zip-compressed', 'x-7z-compressed', 'x-rar-compressed', 'vnd.rar', 'gzip', 'x-gzip', 'x-tar', 'x-bzip2', 'x-xz', 'zstd', 'x-iso9660-image'].includes(sub)) {
    return 'compressed';
  }
  if (
    ['x-msdownload', 'x-msi', 'x-ms-installer', 'vnd.microsoft.portable-executable', 'x-dosexec', 'vnd.android.package-archive', 'x-apple-diskimage', 'vnd.debian.binary-package', 'x-rpm', 'msix', 'appx'].includes(sub)
  ) {
    return 'programs';
  }
  return null;
}

/** The name's extension decides; only an unknown one lets the server's type speak. Mirrors `Category::detect`. */
export function detectCategory(filename: string, contentType: string | null | undefined): CategoryKey {
  const byName = categoryOf(filename);
  return byName === 'other' ? (categoryOfMime(contentType) ?? 'other') : byName;
}

/** File name from a URL path, URL-decoded; `download` when the path has none. */
export function filenameFromUrl(url: string): string {
  try {
    const path = new URL(url).pathname;
    const last = path.split('/').filter((p) => p.length > 0).pop() ?? '';
    const decoded = decodeURIComponent(last);
    return decoded.length > 0 ? decoded : 'download';
  } catch {
    return 'download';
  }
}

/** `true` for http(s) URLs with a host. */
export function isHttpUrl(value: string): boolean {
  try {
    const u = new URL(value.trim());
    return (u.protocol === 'http:' || u.protocol === 'https:') && u.hostname.length > 0;
  } catch {
    return false;
  }
}
