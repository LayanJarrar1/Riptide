/**
 * Video-page detection, pure and unit-tested in `media.test.ts` (no browser APIs).
 *
 * Mirrors `MEDIA_HOSTS`, `is_media_host` and `MediaSelection::parse` from
 * `crates/riptide-core/src/media.rs`; keep them in step. The host re-validates everything, this
 * module only decides when to offer the button and what to put in the quality menu.
 */
import type { MediaChoice } from './protocol';
import { hostOf } from './rules';

/** Sites the app's video support knows (`MEDIA_HOSTS` in riptide-core). */
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

/** Largest resolution an `h<N>` key may name (`MAX_MEDIA_HEIGHT`). */
export const MAX_MEDIA_HEIGHT = 8640;

/** `true` when `host` is one of [`MEDIA_HOSTS`] or a subdomain of one. */
export function isMediaHost(host: string): boolean {
  const h = host.toLowerCase().replace(/\.$/, '');
  return MEDIA_HOSTS.some((m) => h === m || h.endsWith(`.${m}`));
}

/** `true` for a quality key the host accepts: `best`, `h<1..8640>`, `audio-m4a`, `audio-mp3`. */
export function isMediaKey(key: string): boolean {
  if (key === 'best' || key === 'audio-m4a' || key === 'audio-mp3') return true;
  const m = /^h(\d{1,4})$/.exec(key);
  if (!m) return false;
  const h = Number(m[1]);
  return h >= 1 && h <= MAX_MEDIA_HEIGHT;
}

/**
 * Whether `url` is a page of a known video site that can hold a video (not the home page, not a
 * channel list). The app decides for real when it probes; this only gates the button.
 */
export function isVideoPageUrl(url: string): boolean {
  const host = hostOf(url);
  if (host === null || !isMediaHost(host)) return false;
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  const path = u.pathname.replace(/\/+$/, '');
  if (path === '') return false;
  if (host === 'youtu.be' || host.endsWith('.youtu.be')) return true;
  if (/(^|\.)youtube(-nocookie)?\.com$/.test(host)) {
    if (path === '/watch') return u.searchParams.has('v');
    return /^\/(shorts|live|embed|clip|v)\//.test(path);
  }
  return true;
}

/** Kinds of media response worth a badge. */
export type MediaResponseKind = 'hls' | 'dash' | 'progressive';

const HLS_MIMES = new Set(['application/vnd.apple.mpegurl', 'application/x-mpegurl', 'audio/mpegurl', 'audio/x-mpegurl']);
const AMBIGUOUS_MIMES = new Set(['', 'application/octet-stream', 'binary/octet-stream', 'text/plain']);

function pathExtension(url: string): string {
  try {
    const name = new URL(url).pathname.split('/').pop() ?? '';
    const dot = name.lastIndexOf('.');
    return dot >= 0 ? name.slice(dot + 1).toLowerCase() : '';
  } catch {
    return '';
  }
}

/**
 * Classifies a response seen on the wire: a video/audio MIME, an HLS or DASH manifest, or a
 * progressive `mp4`/`webm` served with a vague MIME. MPEG-TS segments (`video/mp2t`, `.ts`)
 * are ignored: they are pieces, the manifest is the video.
 */
export function classifyMediaResponse(url: string, contentType: string | undefined | null): MediaResponseKind | null {
  const mime = (contentType ?? '').split(';')[0]?.trim().toLowerCase() ?? '';
  const ext = pathExtension(url);
  if (HLS_MIMES.has(mime) || ext === 'm3u8') return 'hls';
  if (mime === 'application/dash+xml' || ext === 'mpd') return 'dash';
  if (mime === 'video/mp2t' || ext === 'ts') return null;
  if (mime.startsWith('video/') || mime.startsWith('audio/')) return 'progressive';
  if (AMBIGUOUS_MIMES.has(mime) && ['mp4', 'webm', 'm4v', 'm4a', 'mp3'].includes(ext)) return 'progressive';
  return null;
}

/** A `<video>` big enough to be "the" video of the page rather than an animated icon. */
export function isSubstantialVideo(width: number, height: number): boolean {
  return width >= 200 && height >= 112;
}

/** The origin (`scheme://host[:port]`) cookies are scoped to, or `null` for a non-http(s) URL. */
export function cookieOriginOf(url: string): string | null {
  if (hostOf(url) === null) return null;
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/** One row of the quality menu, ready for display. */
export interface MenuRow {
  key: string;
  /** `best`, a resolution, or audio only. */
  kind: MediaChoice['kind'];
  height: number | null;
  ext: string;
  approxSize: number | null;
}

/** The menu: video rows (best first) and audio-only rows, unknown keys dropped. */
export interface QualityMenu {
  video: MenuRow[];
  audio: MenuRow[];
}

/** Splits the app's `choices` into video and audio rows and drops keys the host would refuse. */
export function buildMenu(choices: readonly MediaChoice[]): QualityMenu {
  const menu: QualityMenu = { video: [], audio: [] };
  const seen = new Set<string>();
  for (const c of choices) {
    if (!isMediaKey(c.key) || seen.has(c.key)) continue;
    seen.add(c.key);
    const row: MenuRow = { key: c.key, kind: c.kind, height: c.height, ext: c.ext, approxSize: c.approx_size };
    (c.kind === 'audio' ? menu.audio : menu.video).push(row);
  }
  return menu;
}

/** List update for "never on this site": adds the site once. */
export function withSite(list: readonly string[], site: string): string[] {
  return list.includes(site) ? [...list] : [...list, site];
}

/** The row shown the moment the popover opens, before the quality list arrives. */
export const BEST_ROW: MenuRow = { key: 'best', kind: 'best', height: null, ext: '', approxSize: null };

/**
 * Whether to warm the app's probe cache for `url`: only a page the extension already treats as
 * a video page, and only once per URL (`already` is the last URL prefetched on this page).
 */
export function shouldPrefetch(url: string, already: string): boolean {
  return url !== already && isVideoPageUrl(url);
}
