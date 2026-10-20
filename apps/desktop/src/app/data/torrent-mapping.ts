/**
 * Conversions for BitTorrent between the generated IPC bindings (snake_case) and the UI model
 * (`model/torrent.ts`). Its own file so the first paint does not carry it: the engine service
 * imports it the first time a torrent is shown, previewed, added or polled.
 */

import type { TorrentAddRequest, TorrentLiveView, TorrentPreview, TorrentSource, TorrentView } from '../model/torrent';
import { selectedBytes } from '../model/torrent';
import type { TorrentAdd } from '../tauri/bridge/TorrentAdd';
import type { TorrentInfo } from '../tauri/bridge/TorrentInfo';
import type { TorrentLive } from '../tauri/bridge/TorrentLive';
import type { TorrentPreviewInfo } from '../tauri/bridge/TorrentPreviewInfo';
import type { TorrentSourceRequest } from '../tauri/bridge/TorrentSourceRequest';

/** The stored torrent facts → what the row shows; live numbers (`peers`, speeds) carry over from `previous`. */
export function toUiTorrent(info: TorrentInfo, previous?: TorrentView): TorrentView {
  const size = selectedBytes(info.files, info.selected);
  return {
    infoHash: info.info_hash,
    files: info.files.map((f) => ({ index: f.index, path: f.path, len: f.len })),
    selected: info.selected,
    sequential: info.sequential,
    seeding: info.seeding,
    downloaded: info.downloaded,
    peers: previous?.peers ?? info.peers,
    peersSeen: previous?.peersSeen ?? 0,
    upBps: previous?.upBps ?? 0,
    uploaded: Math.max(info.uploaded, previous?.uploaded ?? 0),
    ratio: size > 0 ? Math.max(info.uploaded, previous?.uploaded ?? 0) / size : 0,
  };
}

/** What the add dialog sends to preview: a magnet link or a `.torrent` path. */
export function toBridgeSource(s: TorrentSource): TorrentSourceRequest {
  return s.kind === 'magnet' ? { kind: 'magnet', uri: s.uri } : { kind: 'file', path: s.path };
}

/** The engine's preview → what the add dialog lists. */
export function toUiPreview(p: TorrentPreviewInfo): TorrentPreview {
  return {
    token: p.token,
    infoHash: p.info_hash,
    name: p.name,
    files: p.files.map((f) => ({ index: f.index, path: f.path, len: f.len, exists: f.exists })),
    totalBytes: p.total_bytes,
    isPrivate: p.private,
    existing: p.existing,
  };
}

/** The add dialog's choice → the engine's request (`null` file list = every file). */
export function toBridgeTorrentAdd(r: TorrentAddRequest): TorrentAdd {
  return {
    token: r.token,
    dir: r.dir ?? null,
    only_files: r.onlyFiles === null ? null : [...r.onlyFiles],
    sequential: r.sequential,
    start_paused: r.startPaused,
    queue_id: null,
    peers: [],
  };
}

/** Live numbers for the details drawer. */
export function toUiTorrentLive(l: TorrentLive): TorrentLiveView {
  return {
    files: l.info.files.map((f) => ({ index: f.index, path: f.path, len: f.len })),
    fileProgress: l.file_progress,
    selected: l.info.selected,
    peers: l.peers,
    peersSeen: l.peers_seen,
    downBps: l.down_bps,
    upBps: l.up_bps,
    ratio: l.ratio,
    uploaded: l.info.uploaded,
  };
}
