/**
 * UI-side model of BitTorrent downloads. Mirrors the generated `TorrentInfo`, `TorrentLive`,
 * `TorrentPreviewInfo` and `TorrentAdd` (camelCase here, snake_case on the wire; the conversions
 * are in `data/bridge-mapping.ts`). Pure: no Angular, no IO.
 */

/** One file of a torrent (path is relative to the download folder, `/`-separated, already sanitised). */
export interface TorrentFile {
  readonly index: number;
  readonly path: string;
  readonly len: number;
}

/** What a torrent row shows beyond an ordinary download. */
export interface TorrentView {
  readonly infoHash: string;
  readonly files: readonly TorrentFile[];
  /** Indices the user chose; `null` = every file. */
  readonly selected: readonly number[] | null;
  readonly sequential: boolean;
  /** Bytes of the chosen files verified so far. */
  readonly downloaded: number;
  /** Complete and uploading by the user's choice (Settings, Torrents). */
  readonly seeding: boolean;
  readonly peers: number;
  readonly peersSeen: number;
  readonly upBps: number;
  /** Bytes uploaded over the torrent's whole life. */
  readonly uploaded: number;
  /** Uploaded divided by the selected size. */
  readonly ratio: number;
}

/** One file in the add dialog's list. */
export interface TorrentPreviewFile extends TorrentFile {
  /** The path already exists in the default folder. */
  readonly exists: boolean;
}

/** What a magnet link or `.torrent` file contains, before anything is added. */
export interface TorrentPreview {
  /** Hand back in [`TorrentAddRequest.token`]. */
  readonly token: string;
  readonly infoHash: string;
  readonly name: string;
  readonly files: readonly TorrentPreviewFile[];
  readonly totalBytes: number;
  /** The `private` flag: no DHT or peer exchange for this torrent. */
  readonly isPrivate: boolean;
  /** Id of the download that already holds this torrent, if any. */
  readonly existing: number | null;
}

/** Where a torrent comes from. */
export type TorrentSource = { readonly kind: 'magnet'; readonly uri: string } | { readonly kind: 'file'; readonly path: string };

/** The add dialog's final request. */
export interface TorrentAddRequest {
  readonly token: string;
  readonly dir?: string;
  /** `null` = every file. */
  readonly onlyFiles: readonly number[] | null;
  readonly sequential: boolean;
  readonly startPaused: boolean;
}

/** Live numbers for the details drawer. */
export interface TorrentLiveView {
  readonly files: readonly TorrentFile[];
  /** Verified bytes per file, torrent order (empty when the engine has no data yet). */
  readonly fileProgress: readonly number[];
  readonly selected: readonly number[] | null;
  readonly peers: number;
  readonly peersSeen: number;
  readonly downBps: number;
  readonly upBps: number;
  readonly ratio: number;
  readonly uploaded: number;
}

/** `true` for a `magnet:?` link (any case, surrounding space ignored). */
export function isMagnetLink(text: string): boolean {
  return /^magnet:\?/i.test(text.trim());
}

/** The info-hash a magnet link names (hex or base32 `urn:btih:`), lower-case hex when it is hex; `null` otherwise. */
export function magnetHash(text: string): string | null {
  const m = /[?&]xt=urn:btih:([a-z0-9]+)/i.exec(text.trim());
  if (m === null) {
    return null;
  }
  const h = m[1];
  return /^[0-9a-f]{40}$/i.test(h) ? h.toLowerCase() : /^[a-z2-7]{32}$/i.test(h) ? h.toUpperCase() : null;
}

/** Total size of the chosen files (`null` = all of them). */
export function selectedBytes(files: readonly { readonly index: number; readonly len: number }[], selected: readonly number[] | null): number {
  const chosen = selected === null ? null : new Set(selected);
  let total = 0;
  for (const f of files) {
    if (chosen === null || chosen.has(f.index)) {
      total += f.len;
    }
  }
  return total;
}

/** Progress of one file, 0..1 (an empty file counts as done). */
export function fileFraction(len: number, done: number | undefined): number {
  if (len <= 0) {
    return 1;
  }
  return Math.min(1, Math.max(0, (done ?? 0) / len));
}

/** `3.20` for a ratio of 3.2; always two decimals, `0.00` for nothing uploaded. */
export function formatRatio(ratio: number): string {
  return Number.isFinite(ratio) && ratio > 0 ? ratio.toFixed(2) : '0.00';
}
