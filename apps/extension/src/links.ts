/**
 * Pure helpers for the "download all / selected links / images" checklist: classification,
 * de-duplication, filtering and the same-origin cookie rule. No DOM, no browser APIs; the
 * content script feeds in raw `{url, text}` pairs. Unit-tested in `links.test.ts`.
 */
import { extensionOf, filenameFromUrl, hostOf } from './rules';

/** Most candidates the checklist will hold (the page can have far more links than that). */
export const MAX_CANDIDATES = 5_000;
/** Most URLs one batch sends to the app. */
export const MAX_BATCH = 2_000;
/** Longest URL accepted. Longer ones are dropped (the host refuses them anyway). */
export const MAX_LINK_URL_LEN = 8_192;

/** The checklist's type filter groups, in display order. */
export type LinkGroup = 'archives' | 'video' | 'audio' | 'documents' | 'images' | 'programs' | 'other';

/** Every group, in display order. */
export const LINK_GROUPS: readonly LinkGroup[] = [
  'archives',
  'video',
  'audio',
  'documents',
  'images',
  'programs',
  'other',
];

/**
 * Extensions per group. Must stay identical to `Category::extensions` in
 * `crates/riptide-core/src/category.rs` and `CATEGORY_EXTENSIONS` in
 * `apps/desktop/src/app/model/download.ts` (`links.test.ts` compares with the Rust table when
 * it can read it). `other` has none: it is whatever is left.
 */
export const LINK_EXTENSIONS: Record<Exclude<LinkGroup, 'other'>, readonly string[]> = {
  archives: ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst', 'iso', 'cab'],
  documents: [
    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp', 'txt', 'rtf', 'epub', 'mobi', 'csv',
  ],
  audio: ['mp3', 'flac', 'wav', 'aac', 'm4a', 'ogg', 'opus', 'wma', 'aiff'],
  video: ['mp4', 'mkv', 'avi', 'mov', 'wmv', 'webm', 'm4v', 'ts', 'flv', 'mpg', 'mpeg'],
  programs: ['exe', 'msi', 'msix', 'appx', 'apk', 'dmg', 'pkg', 'deb', 'rpm', 'appimage'],
  images: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg', 'tif', 'tiff', 'heic', 'avif', 'psd'],
};

/** Group of an extension (`other` when unknown or empty). */
export function groupOf(ext: string): LinkGroup {
  const e = ext.toLowerCase();
  if (!e) return 'other';
  for (const key of Object.keys(LINK_EXTENSIONS) as Exclude<LinkGroup, 'other'>[]) {
    if (LINK_EXTENSIONS[key].includes(e)) return key;
  }
  return 'other';
}

/** What the content script reads from the page. */
export interface RawLink {
  /** Already resolved against the document base (`a.href`, `img.currentSrc`, ...). */
  url: string;
  /** Visible text, `alt`, or `download` attribute; may be empty. */
  text?: string;
  /** Where it came from. */
  source: 'link' | 'image';
}

/** One checklist row. */
export interface LinkItem {
  url: string;
  /** Label shown in the list. */
  name: string;
  ext: string;
  group: LinkGroup;
  source: 'link' | 'image';
}

/** Strip the fragment; drop anything that is not a plain http(s) URL of sane length. */
export function cleanUrl(url: string): string | null {
  if (url.length > MAX_LINK_URL_LEN || hostOf(url) === null) return null;
  try {
    const u = new URL(url);
    if (u.username || u.password) return null;
    u.hash = '';
    return u.href;
  } catch {
    return null;
  }
}

/** Collapse whitespace and cap the length of a label. */
export function tidyText(text: string | undefined): string {
  return (text ?? '').replace(/\s+/g, ' ').trim().slice(0, 200);
}

/**
 * Turn raw page links into checklist items: http(s) only, fragment stripped, de-duplicated
 * (first one wins, but a later non-empty label fills an empty one), at most `MAX_CANDIDATES`.
 * Unlabelled items are named after the file name in the URL, then the URL itself.
 */
export function buildItems(raw: readonly RawLink[]): LinkItem[] {
  const byUrl = new Map<string, LinkItem>();
  const labelled = new Set<string>();
  for (const r of raw) {
    const url = cleanUrl(r.url);
    if (url === null) continue;
    const text = tidyText(r.text);
    const existing = byUrl.get(url);
    if (existing) {
      if (text && !labelled.has(url)) {
        existing.name = text;
        labelled.add(url);
      }
      continue;
    }
    if (byUrl.size >= MAX_CANDIDATES) break;
    const guessed = filenameFromUrl(url);
    const ext = extensionOf(guessed);
    byUrl.set(url, { url, name: text || guessed || url, ext, group: groupOf(ext), source: r.source });
    if (text) labelled.add(url);
  }
  return [...byUrl.values()];
}

/** Split an `srcset` attribute into candidate parts (commas inside URLs do not split). */
function srcsetParts(srcset: string): string[] {
  return srcset.split(/,\s+|,$/);
}

/** URLs of an `srcset` attribute (descriptors dropped), resolved against `base`. */
export function parseSrcset(srcset: string, base: string): string[] {
  const out: string[] = [];
  for (const part of srcsetParts(srcset)) {
    const first = part.trim().split(/\s+/)[0];
    if (!first) continue;
    try {
      out.push(new URL(first, base).href);
    } catch {
      /* not a URL */
    }
  }
  return out;
}

/** The biggest candidate of an `srcset` (by `w`/`x` descriptor); the last one on a tie. */
export function largestFromSrcset(srcset: string, base: string): string | null {
  let best: { url: string; weight: number } | null = null;
  for (const part of srcsetParts(srcset)) {
    const bits = part.trim().split(/\s+/);
    const first = bits[0];
    if (!first) continue;
    let url: string;
    try {
      url = new URL(first, base).href;
    } catch {
      continue;
    }
    const d = bits[1] ?? '';
    const weight = /^\d+(\.\d+)?[wx]$/.test(d) ? Number.parseFloat(d) : 1;
    if (!best || weight >= best.weight) best = { url, weight };
  }
  return best?.url ?? null;
}

/** Active filter state of the checklist. */
export interface LinkFilter {
  /** Groups that are shown; empty set = all. */
  groups: ReadonlySet<LinkGroup>;
  /** Case-insensitive substring of name or URL. */
  text: string;
}

/** Items that pass the filter, in original order. */
export function filterItems(items: readonly LinkItem[], f: LinkFilter): LinkItem[] {
  const q = f.text.trim().toLowerCase();
  return items.filter((i) => {
    if (f.groups.size > 0 && !f.groups.has(i.group)) return false;
    return !q || i.url.toLowerCase().includes(q) || i.name.toLowerCase().includes(q);
  });
}

/** How many items each group has (for the chip counters). */
export function countByGroup(items: readonly LinkItem[]): Record<LinkGroup, number> {
  const out: Record<LinkGroup, number> = {
    archives: 0,
    video: 0,
    audio: 0,
    documents: 0,
    images: 0,
    programs: 0,
    other: 0,
  };
  for (const i of items) out[i.group] += 1;
  return out;
}

/** Lists with more rows than this are rendered through a window. */
export const VIRTUALISE_ABOVE = 200;

/**
 * Visible window of a virtualised list. `scrollTop` and `viewport` are pixels; rows are a fixed
 * `rowHeight`. Returns the index range `[start, end)` to render, with `overscan` extra rows.
 */
export function visibleRange(
  total: number,
  scrollTop: number,
  viewport: number,
  rowHeight: number,
  overscan = 6,
): { start: number; end: number } {
  if (total <= 0 || rowHeight <= 0) return { start: 0, end: 0 };
  const top = Math.max(0, scrollTop);
  const first = Math.floor(top / rowHeight);
  const last = Math.ceil((top + Math.max(0, viewport)) / rowHeight);
  return { start: Math.max(0, first - overscan), end: Math.min(total, last + overscan) };
}

/** Origin of an http(s) URL, or `null`. */
export function originOf(url: string): string | null {
  if (hostOf(url) === null) return null;
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/** Cookies of the page may only accompany URLs on the page's own origin. */
export function mayCarryCookies(url: string, pageUrl: string | undefined): boolean {
  if (!pageUrl) return false;
  const a = originOf(url);
  return a !== null && a === originOf(pageUrl);
}

/** Valid, de-duplicated http(s) URLs, capped at `MAX_BATCH`. */
export function sanitiseBatch(urls: readonly string[]): string[] {
  const out = new Set<string>();
  for (const u of urls) {
    if (typeof u !== 'string') continue;
    const c = cleanUrl(u);
    if (c !== null) out.add(c);
    if (out.size >= MAX_BATCH) break;
  }
  return [...out];
}

/** Run `worker` over `items` with at most `limit` in flight; results keep input order. */
export async function mapLimit<T, R>(
  items: readonly T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>,
  shouldStop: () => boolean = () => false,
): Promise<(R | undefined)[]> {
  const results = new Array<R | undefined>(items.length).fill(undefined);
  let next = 0;
  const lane = async (): Promise<void> => {
    while (next < items.length && !shouldStop()) {
      const i = next++;
      const item = items[i];
      if (item === undefined) continue;
      results[i] = await worker(item, i);
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, lane));
  return results;
}
