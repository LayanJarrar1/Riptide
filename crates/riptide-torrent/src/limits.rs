//! Hard limits applied to every torrent before a single byte is written.
//!
//! A `.torrent` file or a magnet link is attacker-controlled input. These numbers are far above
//! anything legitimate (the largest public torrents have a few hundred thousand pieces) and far
//! below what it takes to exhaust memory or the disk with a hostile description.

/// Largest `.torrent` file (or in-memory buffer) we parse: 8 MiB.
pub const MAX_TORRENT_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Largest magnet link: 8 KiB.
pub const MAX_MAGNET_BYTES: usize = 8 * 1024;
/// Most trackers kept from a magnet link.
pub const MAX_TRACKERS: usize = 64;
/// Most files in one torrent.
pub const MAX_FILES: usize = 50_000;
/// Most pieces in one torrent (20 bytes of SHA-1 each: 80 MB of hashes at the limit).
pub const MAX_PIECES: u64 = 4_000_000;
/// Largest piece length: 256 MiB.
pub const MAX_PIECE_LENGTH: u32 = 256 * 1024 * 1024;
/// Smallest piece length: 1 KiB (a zero piece length would divide by zero downstream).
pub const MIN_PIECE_LENGTH: u32 = 1024;
/// Largest total payload: 1 PiB (also guards against `u64` overflow when summing file lengths).
pub const MAX_TOTAL_BYTES: u64 = 1 << 50;
/// Deepest folder nesting of a file inside a torrent.
pub const MAX_PATH_DEPTH: usize = 32;
/// Longest relative path (all components and separators) inside the download folder, in bytes.
pub const MAX_RELATIVE_PATH_BYTES: usize = 1024;
/// How long a magnet link may take to deliver its metadata before a preview gives up.
pub const METADATA_TIMEOUT_SECS: u64 = 120;
