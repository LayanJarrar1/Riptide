//! BitTorrent for Riptide: magnet links and `.torrent` files on top of
//! [librqbit](https://github.com/ikatson/rqbit) (pure Rust, Apache-2.0; see decision D-torrent in
//! `docs/decisions.md`).
//!
//! The crate knows nothing about Tauri, Angular or the UI: [`TorrentEngine`] is a plain async API
//! that reports [`TorrentEvent`]s. What it adds on top of the library:
//!
//! * **Hostile input is the default assumption.** A `.torrent` or magnet link is vetted before
//!   anything touches the disk ([`meta`], [`magnet`], [`limits`]): absurd piece/file counts and
//!   sizes, nesting and malformed bencode are rejected, and every file name goes through the
//!   same sanitiser as HTTP downloads ([`paths`]). The library's own file storage is replaced by
//!   [`storage::SafeStorageFactory`], which writes only to paths planned by that sanitiser and
//!   confined to the folder the user chose.
//! * **No silent seeding.** When the download is complete and verified the torrent stops all
//!   network activity, unless the user turned seeding on in the settings; even then it stops at
//!   the configured ratio or time ([`SeedingPolicy`]).
//! * **Crash safety.** The library persists torrents and pause state, Riptide's [`registry`]
//!   keeps the folder, the planned layout and the selection; on start every piece found on disk
//!   is re-hashed, so a killed process resumes without trusting partial data.
//! * **Privacy.** Magnet trackers (which may carry a passkey) and query strings never reach the
//!   logs ([`magnet::Magnet::redacted`], [`magnet::redact_url`]).
// `deny`, not `forbid`: `storage::sparse` on Windows is one documented DeviceIoControl call.
#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod engine;
pub mod error;
pub mod limits;
pub mod magnet;
pub mod meta;
pub mod offline;
pub mod paths;
pub mod registry;
pub mod storage;
pub mod types;

pub use engine::TorrentEngine;
pub use error::{Error, Result};
pub use magnet::{parse_magnet, redact_url, Magnet};
pub use meta::{inspect_torrent, inspect_torrent_file, FileEntry, TorrentPreview};
pub use types::{
    AddOptions, SeedingPolicy, TorrentEvent, TorrentSettings, TorrentSnapshot, TorrentSource,
    TorrentState,
};
