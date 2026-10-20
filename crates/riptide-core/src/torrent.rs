//! Plain data for BitTorrent downloads: what the store keeps per torrent, what the add dialog
//! shows before anything is written, and the live numbers the details drawer polls.
//!
//! No IO and no BitTorrent code lives here (that is `riptide-torrent`); these are the shapes the
//! store, the engine, the IPC layer and the UI share.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::download::{DownloadId, QueueId};

/// One file of a torrent as the engine planned it (sanitised, relative to the download folder).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentFileInfo {
    /// Index in torrent order (the number the selection uses).
    pub index: u32,
    /// Sanitised path relative to the download folder.
    pub path: String,
    /// Size in bytes.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub len: u64,
}

/// What the store keeps for a download of kind `torrent` (table `torrents`).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentInfo {
    /// The download this belongs to.
    pub download_id: DownloadId,
    /// Info-hash, 40 lowercase hex digits.
    pub info_hash: String,
    /// Sanitised torrent name.
    pub name: String,
    /// Every file of the torrent (padding files left out).
    pub files: Vec<TorrentFileInfo>,
    /// Indices of the files the user chose; `None` = all.
    pub selected: Option<Vec<u32>>,
    /// Files are fetched one after the other, first file first.
    pub sequential: bool,
    /// Complete and uploading by the user's choice (Settings -> Torrents).
    pub seeding: bool,
    /// Bytes uploaded over the torrent's whole life.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub uploaded: u64,
    /// Bytes of selected data verified so far (survives restarts, so the list is right before the
    /// torrent engine has re-checked anything).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub downloaded: u64,
    /// Peers connected at the last update (`0` while stopped).
    pub peers: u32,
}

impl TorrentInfo {
    /// Bytes the user asked for: the sum of the selected files (all files when `selected` is
    /// `None`).
    pub fn selected_bytes(&self) -> u64 {
        self.files
            .iter()
            .filter(|f| self.selected.as_ref().is_none_or(|s| s.contains(&f.index)))
            .map(|f| f.len)
            .sum()
    }

    /// Uploaded divided by the selected size (`0.0` for an empty selection).
    pub fn ratio(&self) -> f32 {
        let size = self.selected_bytes();
        if size == 0 {
            0.0
        } else {
            (self.uploaded as f64 / size as f64) as f32
        }
    }
}

/// Where a torrent comes from, as the add dialog hands it over.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TorrentSourceRequest {
    /// A `magnet:` link.
    Magnet {
        /// The link as pasted.
        uri: String,
    },
    /// A `.torrent` file the user picked in the app's own file dialog.
    File {
        /// Path of the file.
        path: PathBuf,
    },
}

/// One file in the add dialog's list.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentPreviewFile {
    /// Index to send back in [`TorrentAdd::only_files`].
    pub index: u32,
    /// Sanitised relative path.
    pub path: String,
    /// Size in bytes.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub len: u64,
    /// The path is already taken in the default folder.
    pub exists: bool,
}

/// What a torrent contains, before anything is added.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentPreviewInfo {
    /// Opaque handle to hand back in [`TorrentAdd::token`]; the engine keeps the metadata for a
    /// while so a magnet link is not resolved twice.
    pub token: String,
    /// Info-hash (40 hex digits).
    pub info_hash: String,
    /// Sanitised name.
    pub name: String,
    /// Files without padding, in torrent order.
    pub files: Vec<TorrentPreviewFile>,
    /// Sum of all file sizes.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub total_bytes: u64,
    /// `private` flag: no DHT / PEX for this torrent.
    pub private: bool,
    /// The torrent is already in the list (same info-hash).
    pub existing: Option<DownloadId>,
}

/// The add dialog's final request for a torrent.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentAdd {
    /// From [`TorrentPreviewInfo::token`].
    pub token: String,
    /// Destination folder; `None` = the default folder.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// File indices to download; `None` = all.
    #[serde(default)]
    pub only_files: Option<Vec<u32>>,
    /// One file after the other.
    #[serde(default)]
    pub sequential: bool,
    /// Add without starting.
    #[serde(default)]
    pub start_paused: bool,
    /// Queue to list it under.
    #[serde(default)]
    pub queue_id: Option<QueueId>,
    /// Peers to try first (`ip:port`); the add dialog leaves this empty, tests and the CLI use it.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub peers: Vec<std::net::SocketAddr>,
}

/// Live numbers of one torrent, polled by the details drawer.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TorrentLive {
    /// Stored facts (files, selection, totals).
    pub info: TorrentInfo,
    /// Verified bytes per file, in torrent order (empty while the torrent engine has no data).
    #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
    pub file_progress: Vec<u64>,
    /// Peers connected.
    pub peers: u32,
    /// Peers seen.
    pub peers_seen: u32,
    /// Download speed, bytes per second.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub down_bps: u64,
    /// Upload speed, bytes per second.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub up_bps: u64,
    /// Uploaded over the whole life divided by the selected size.
    pub ratio: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(selected: Option<Vec<u32>>) -> TorrentInfo {
        TorrentInfo {
            download_id: DownloadId(1),
            info_hash: "0".repeat(40),
            name: "t".into(),
            files: vec![
                TorrentFileInfo {
                    index: 0,
                    path: "a".into(),
                    len: 100,
                },
                TorrentFileInfo {
                    index: 1,
                    path: "b".into(),
                    len: 300,
                },
            ],
            selected,
            sequential: false,
            seeding: false,
            uploaded: 200,
            downloaded: 0,
            peers: 0,
        }
    }

    #[test]
    fn selected_bytes_and_ratio() {
        assert_eq!(info(None).selected_bytes(), 400);
        assert_eq!(info(Some(vec![1])).selected_bytes(), 300);
        assert!((info(None).ratio() - 0.5).abs() < 1e-6);
        assert_eq!(info(Some(vec![])).ratio(), 0.0);
    }

    #[test]
    fn source_request_json_is_tagged() {
        let m = serde_json::to_value(TorrentSourceRequest::Magnet {
            uri: "magnet:?x".into(),
        })
        .unwrap();
        assert_eq!(m["kind"], "magnet");
        let back: TorrentSourceRequest = serde_json::from_value(m).unwrap();
        assert!(matches!(back, TorrentSourceRequest::Magnet { .. }));
    }
}
