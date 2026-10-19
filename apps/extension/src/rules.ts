/**
 * The capture rules engine. Pure: no browser APIs, no IO, fully unit-tested in `rules.test.ts`.
 *
 * Given the user's rules and a candidate download (URL, suggested filename, MIME, size, whether
 * the server said `Content-Disposition: attachment`, whether Alt was held), decide whether the
 * extension should take the download away from the browser and hand it to Riptide.
 */

/** Built-in file-type groups shown as checkboxes in the options page. */
export type FileTypeKey = 'archives' | 'video' | 'audio' | 'programs' | 'documents' | 'images';

/** Every group, in display order. */
export const FILE_TYPE_KEYS: readonly FileTypeKey[] = [
  'archives',
  'video',
  'audio',
  'programs',
  'documents',
  'images',
];

/** Extensions (lower-case, no dot) per group. */
export const FILE_TYPES: Record<FileTypeKey, readonly string[]> = {
  archives: ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'tbz', 'xz', 'txz', 'zst', 'iso', 'img', 'cab', 'dmg', 'wim'],
  video: ['mp4', 'mkv', 'avi', 'mov', 'wmv', 'webm', 'flv', 'm4v', 'mpg', 'mpeg', 'ts', 'm2ts', '3gp', 'vob'],
  audio: ['mp3', 'flac', 'wav', 'aac', 'ogg', 'oga', 'm4a', 'wma', 'opus', 'aiff', 'aif', 'ape', 'alac'],
  programs: ['exe', 'msi', 'msix', 'msixbundle', 'appx', 'appinstaller', 'apk', 'deb', 'rpm', 'appimage', 'jar', 'pkg'],
  documents: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp', 'epub', 'mobi', 'rtf', 'csv', 'djvu'],
  images: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'avif', 'bmp', 'tiff', 'tif', 'heic', 'psd', 'raw', 'cr2', 'nef', 'dng'],
};

/** User rules, as stored in `storage.local` under `settings.rules`. */
export interface Rules {
  /** Master switch: when off, the extension never intercepts a browser download. */
  enabled: boolean;
  /** Minimum size in bytes; downloads with a known size below this stay in the browser. */
  minSize: number;
  /** Which built-in groups to capture. */
  types: Record<FileTypeKey, boolean>;
  /** Extra extensions (lower-case, no dot) to capture. */
  customExtensions: string[];
  /** Extensions (lower-case, no dot) that always stay in the browser, even if their group is on. */
  ignoreExtensions: string[];
  /** If non-empty, capture only on these sites (host patterns, see `siteMatches`). */
  allowSites: string[];
  /** Never capture on these sites. */
  denySites: string[];
  /** Holding Alt while clicking a link leaves that download to the browser. */
  altBypass: boolean;
  /** Sites where the user chose "never ask again": no capture, no notification. */
  neverAskSites: string[];
}

/** One mebibyte. */
export const MIB = 1024 * 1024;

/** Defaults for a fresh install. */
export const DEFAULT_RULES: Readonly<Rules> = Object.freeze({
  enabled: true,
  minSize: MIB,
  types: { archives: true, video: true, audio: true, programs: true, documents: true, images: false },
  customExtensions: [],
  ignoreExtensions: [],
  allowSites: [],
  denySites: [],
  altBypass: true,
  neverAskSites: [],
});

/** What the engine knows about a download before deciding. Every field but `url` is optional. */
export interface Candidate {
  url: string;
  /** Filename the browser or the server suggested. */
  filename?: string;
  /** `Content-Type` without parameters. */
  mime?: string;
  /** Size in bytes; `undefined` or a negative number means unknown. */
  size?: number;
  referrer?: string;
  /** The server sent `Content-Disposition: attachment`. */
  attachment?: boolean;
  /** Alt was held when the link was clicked. */
  altHeld?: boolean;
}

/** Why a download was left to the browser. */
export type SkipReason =
  | 'not_http'
  | 'disabled'
  | 'paused'
  | 'never_ask'
  | 'alt_bypass'
  | 'site_denied'
  | 'site_not_allowed'
  | 'type'
  | 'ignored_type'
  | 'too_small';

/** Why a download was taken. */
export type TakeReason = 'extension' | 'attachment' | 'mime';

/** The engine's verdict. */
export type Decision = { take: true; reason: TakeReason } | { take: false; reason: SkipReason };

// ---------------------------------------------------------------------------------------------
// URL / filename helpers
// ---------------------------------------------------------------------------------------------

/** Lower-case host of an http(s) URL, or `null` if the URL is not http(s). */
export function hostOf(url: string): string | null {
  try {
    const u = new URL(url);
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
    return u.hostname.toLowerCase().replace(/\.$/, '');
  } catch {
    return null;
  }
}

/** Filename part of a URL path (percent-decoded, no query/hash); `''` if there is none. */
export function filenameFromUrl(url: string): string {
  try {
    const u = new URL(url);
    const last = u.pathname.split('/').filter(Boolean).pop() ?? '';
    try {
      return decodeURIComponent(last);
    } catch {
      return last;
    }
  } catch {
    return '';
  }
}

/** What an extension may look like: 1–12 alphanumerics (`appinstaller` is the longest built-in). */
const EXTENSION_RE = /^[a-z0-9]{1,12}$/;

/** Normalise one user-entered extension token (`.ZIP` -> `zip`); `''` when it is not one. */
export function normaliseExtension(token: string): string {
  const e = token.trim().toLowerCase().replace(/^\*?\./, '');
  return EXTENSION_RE.test(e) ? e : '';
}

/**
 * Lower-case extension (no dot) of a filename or URL; `''` when there is none or it does not
 * look like an extension.
 */
export function extensionOf(nameOrUrl: string): string {
  const name = /^[a-z][a-z0-9+.-]*:\/\//i.test(nameOrUrl) ? filenameFromUrl(nameOrUrl) : nameOrUrl;
  const base = name.split(/[/\\]/).pop() ?? '';
  const dot = base.lastIndexOf('.');
  if (dot <= 0) return '';
  const ext = base.slice(dot + 1).toLowerCase();
  return EXTENSION_RE.test(ext) ? ext : '';
}

/** The built-in group an extension belongs to, or `null`. */
export function groupOfExtension(ext: string): FileTypeKey | null {
  for (const key of FILE_TYPE_KEYS) {
    if (FILE_TYPES[key].includes(ext)) return key;
  }
  return null;
}

/** Whether the rules select this extension (built-in group on, or custom). */
export function wantsExtension(rules: Rules, ext: string): boolean {
  if (!ext) return false;
  if (rules.customExtensions.includes(ext)) return true;
  const group = groupOfExtension(ext);
  return group !== null && rules.types[group];
}

/** Quick check for the content script: does this link point straight at a file we would take? */
export function isDirectFileLink(href: string, rules: Rules): boolean {
  if (hostOf(href) === null) return false;
  return wantsExtension(rules, extensionOf(href));
}

// ---------------------------------------------------------------------------------------------
// MIME helpers (Firefox header path)
// ---------------------------------------------------------------------------------------------

const RENDERABLE_MIME_PREFIXES = ['text/', 'image/', 'video/', 'audio/', 'font/'];
const RENDERABLE_MIME_EXACT = new Set([
  'application/xhtml+xml',
  'application/xml',
  'application/json',
  'application/ld+json',
  'application/javascript',
  'application/x-javascript',
  'application/ecmascript',
  'application/pdf',
  'application/rss+xml',
  'application/atom+xml',
  'application/manifest+json',
  'application/wasm',
  'application/x-mpegurl',
  'application/vnd.apple.mpegurl',
  'application/dash+xml',
  'multipart/x-mixed-replace',
]);

/** Normalise a `Content-Type` header value to its bare media type. */
export function mediaType(contentType: string | undefined | null): string {
  if (!contentType) return '';
  return (contentType.split(';')[0] ?? '').trim().toLowerCase();
}

/** Whether a browser would render this media type inline (so it is not a download). */
export function isRenderableMime(mime: string): boolean {
  const m = mediaType(mime);
  if (!m) return true; // unknown: let the browser decide
  if (RENDERABLE_MIME_EXACT.has(m)) return true;
  if (m.endsWith('+xml') || m.endsWith('+json')) return true;
  return RENDERABLE_MIME_PREFIXES.some((p) => m.startsWith(p));
}

/** Whether this media type means "a file to save" (`application/octet-stream`, zip, ...). */
export function isDownloadMime(mime: string | undefined | null): boolean {
  const m = mediaType(mime);
  return m !== '' && !isRenderableMime(m);
}

/** Filename from a `Content-Disposition` header (RFC 6266 / 5987), or `''`. */
export function filenameFromDisposition(value: string | undefined | null): string {
  if (!value) return '';
  const ext = /filename\*\s*=\s*([^']*)'[^']*'([^;]+)/i.exec(value);
  if (ext?.[2]) {
    try {
      return decodeURIComponent(ext[2].trim()).trim();
    } catch {
      /* fall through to the plain parameter */
    }
  }
  const plain = /filename\s*=\s*("((?:\\"|[^"])*)"|([^;]+))/i.exec(value);
  const raw = plain?.[2] ?? plain?.[3] ?? '';
  return raw.replace(/\\"/g, '"').trim();
}

/** `true` when a `Content-Disposition` value asks for a download. */
export function isAttachmentDisposition(value: string | undefined | null): boolean {
  return /^\s*attachment\b/i.test(value ?? '');
}

// ---------------------------------------------------------------------------------------------
// Site lists
// ---------------------------------------------------------------------------------------------

/** Normalise a user-entered site pattern: lower-case host, no scheme/path/port, no leading `*.`. */
export function normaliseSite(pattern: string): string {
  let p = pattern.trim().toLowerCase();
  if (!p) return '';
  p = p.replace(/^[a-z][a-z0-9+.-]*:\/\//, '');
  p = p.split(/[/?#]/)[0] ?? '';
  p = p.replace(/:\d+$/, '');
  p = p.replace(/^\*\./, '').replace(/^\./, '').replace(/\.$/, '');
  return /^[a-z0-9.-]+$/.test(p) ? p : '';
}

/** `example.com` matches `example.com` and every subdomain. */
export function siteMatches(pattern: string, host: string): boolean {
  const p = normaliseSite(pattern);
  if (!p) return false;
  const h = host.toLowerCase();
  return h === p || h.endsWith('.' + p);
}

/** Whether any pattern in the list matches the host. */
export function listMatches(list: readonly string[], host: string): boolean {
  return list.some((p) => siteMatches(p, host));
}

/** Parse a textarea / comma-separated site list. */
export function parseSiteList(text: string): string[] {
  const out = new Set<string>();
  for (const part of text.split(/[\s,;]+/)) {
    const s = normaliseSite(part);
    if (s) out.add(s);
  }
  return [...out];
}

/** Parse a textarea / comma-separated extension list (`.ZIP, rar` -> `['zip', 'rar']`). */
export function parseExtensionList(text: string): string[] {
  const out = new Set<string>();
  for (const part of text.split(/[\s,;]+/)) {
    const e = normaliseExtension(part);
    if (e) out.add(e);
  }
  return [...out];
}

// ---------------------------------------------------------------------------------------------
// Normalisation (storage may hold anything)
// ---------------------------------------------------------------------------------------------

function stringList(value: unknown, norm: (s: string) => string): string[] {
  if (!Array.isArray(value)) return [];
  const out = new Set<string>();
  for (const v of value) {
    if (typeof v === 'string') {
      const s = norm(v);
      if (s) out.add(s);
    }
  }
  return [...out];
}

/** Merge whatever is in storage with the defaults, dropping invalid values. */
export function normaliseRules(input: unknown): Rules {
  const raw = (typeof input === 'object' && input !== null ? input : {}) as Record<string, unknown>;
  const rawTypes = (typeof raw['types'] === 'object' && raw['types'] !== null ? raw['types'] : {}) as Record<
    string,
    unknown
  >;
  const types = {} as Record<FileTypeKey, boolean>;
  for (const key of FILE_TYPE_KEYS) {
    const v = rawTypes[key];
    types[key] = typeof v === 'boolean' ? v : DEFAULT_RULES.types[key];
  }
  const minSize = raw['minSize'];
  return {
    enabled: typeof raw['enabled'] === 'boolean' ? raw['enabled'] : DEFAULT_RULES.enabled,
    minSize: typeof minSize === 'number' && Number.isFinite(minSize) && minSize >= 0 ? Math.floor(minSize) : DEFAULT_RULES.minSize,
    types,
    customExtensions: stringList(raw['customExtensions'], normaliseExtension),
    ignoreExtensions: stringList(raw['ignoreExtensions'], normaliseExtension),
    allowSites: stringList(raw['allowSites'], normaliseSite),
    denySites: stringList(raw['denySites'], normaliseSite),
    altBypass: typeof raw['altBypass'] === 'boolean' ? raw['altBypass'] : DEFAULT_RULES.altBypass,
    neverAskSites: stringList(raw['neverAskSites'], normaliseSite),
  };
}

// ---------------------------------------------------------------------------------------------
// The decision
// ---------------------------------------------------------------------------------------------

/**
 * Decide whether to capture. `paused` is the session-level "pause capture" toggle in the popup.
 *
 * Order: scheme, master switch, pause, never-ask sites, Alt bypass, deny list, allow list,
 * ignored extensions, file type, size. Explicit user actions (context menu, hover badge) do not go through here.
 */
export function decide(rules: Rules, c: Candidate, paused = false): Decision {
  const host = hostOf(c.url);
  if (host === null) return { take: false, reason: 'not_http' };
  if (!rules.enabled) return { take: false, reason: 'disabled' };
  if (paused) return { take: false, reason: 'paused' };
  if (listMatches(rules.neverAskSites, host)) return { take: false, reason: 'never_ask' };
  if (rules.altBypass && c.altHeld) return { take: false, reason: 'alt_bypass' };
  if (listMatches(rules.denySites, host)) return { take: false, reason: 'site_denied' };
  if (rules.allowSites.length > 0 && !listMatches(rules.allowSites, host)) {
    return { take: false, reason: 'site_not_allowed' };
  }

  const ext = extensionOf(c.filename || filenameFromUrl(c.url));
  if (ext && rules.ignoreExtensions.includes(ext)) return { take: false, reason: 'ignored_type' };
  let reason: TakeReason;
  if (wantsExtension(rules, ext)) {
    reason = 'extension';
  } else if (ext && groupOfExtension(ext) !== null) {
    // A known type the user switched off (e.g. images): respect that even for attachments.
    return { take: false, reason: 'type' };
  } else if (c.attachment) {
    reason = 'attachment';
  } else if (isDownloadMime(c.mime)) {
    reason = 'mime';
  } else {
    return { take: false, reason: 'type' };
  }

  if (typeof c.size === 'number' && c.size >= 0 && c.size < rules.minSize) {
    return { take: false, reason: 'too_small' };
  }
  return { take: true, reason };
}
