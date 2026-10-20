# Design decisions

Short records of choices that are expensive to reverse. Add a new entry instead of silently
changing an old one; mark the old one "superseded by" the new one.

## D-torrent-1: BitTorrent through librqbit 9.0.1 (pinned), behind our own safe storage

Status: accepted (branch `feat/torrent`, planned for 0.1.0-alpha.5; not part of alpha.4).

**Decision.** `riptide-torrent` wraps [`librqbit`](https://github.com/ikatson/rqbit), pinned with
`=9.0.1` (exact version, in `crates/riptide-torrent/Cargo.toml`), using the `rust-tls` feature set
(no OpenSSL, `ring` for SHA-1) and none of its HTTP API, web UI or Postgres features.

**Why librqbit.** Pure Rust (no C toolchain, no `libtorrent` binary to ship, builds on every
target we support), async on tokio like the rest of the engine, has DHT, PEX, UDP/HTTP trackers,
uTP, magnet metadata fetch, per-torrent file selection, bandwidth limits and a pluggable storage
trait. The alternatives (`libtorrent` through FFI, `transmission` as a sidecar) bring a C++
dependency or a second process to secure and ship.

**License.** librqbit and its `librqbit-*` sub-crates are Apache-2.0. Apache-2.0 is compatible
with our `MIT OR Apache-2.0` library crates and, in a combined work, with the GPL-3.0-or-later
CLI and desktop binaries (Apache-2.0 code may be included in GPLv3 works). The transitive
dependency licenses (344 packages reachable from `riptide-torrent`, normal dependencies only) were
checked with `cargo metadata`: MIT, Apache-2.0, ISC, BSD, Zlib, Unicode-3.0, CDLA-Permissive-2.0
and dual licenses that offer one of those. Two to know about: `option-ext` is MPL-2.0 (file-level
copyleft, already in the lock file through `dirs`) and `aws-lc-sys` (BoringSSL-derived C code,
ISC/OpenSSL-style licenses) is **new** to the lock file: librqbit's `rust-tls` feature pulls
`reqwest 0.13` with the `aws-lc-rs` crypto provider, so a C compiler is now needed to build the
workspace. It builds with MSVC on this machine; the CI runners must confirm it. The notices
of the new dependencies must be added to `THIRD-PARTY-NOTICES.txt` when the crate is wired into a
shipped binary (not in this change: no binary links it yet except the optional CLI hook).

**What we do not trust in it, and how we compensate.**

* *File naming.* librqbit rejects `..` and separators inside a path component but still accepts
  drive prefixes, device names (`CON`, `NUL`), trailing dots, bidirectional overrides, control
  characters, 4 KB names and names that collide after normalisation. We replace its file storage
  (`SafeStorageFactory`): the on-disk path of every file comes from `plan_layout`, which applies
  `riptide_core::filename::sanitize_filename` to each component and is confined to the folder the
  user chose (lexical check plus symlink/junction check). Hostile names are covered by tests that
  fail if the storage is swapped back to the stock one.
* *Resource limits.* Before the library sees a `.torrent` we bound the file size, bencode
  nesting depth, file count, piece count, piece length and total size with overflow-checked
  arithmetic (`crate::limits`), because the library divides by the piece length and sums file
  lengths with unchecked `u64` arithmetic. Metadata fetched through a magnet link is parsed by
  the library first and is re-checked afterwards; a hostile magnet-resolved torrent can therefore
  still trip an upstream assertion in a library task. This is a known limitation.
* *Seeding.* librqbit keeps seeding forever after completion. We pause the torrent as soon as
  every selected piece is verified unless the user enabled seeding, and then stop at the
  configured ratio or time.
* *Sequential mode.* librqbit 9.0.1 has no piece-level sequential picker. "Sequential" in
  Riptide means one file after the other, in order (first selected file first).
* *Logging.* Our code never logs a magnet link or a tracker URL (they may embed a passkey); it
  logs the info-hash, the display name and the tracker count. librqbit itself may print tracker
  URLs at `debug`/`trace`, so its targets must stay at `info` or above in every default filter.

**Files are written in place.** CLAUDE.md rule 2 (`<name>.rtpart`, renamed on completion) does
not fit a torrent: pieces arrive in random order across many files, and a seeding torrent must
keep its final names. Completion is recorded in Riptide's registry only after every selected
piece passed its SHA-1 check (`TorrentEngine::verify` re-hashes everything on demand), and a
restart re-hashes what is on disk before trusting it.

**Not done on purpose.** No `DownloadKind::Torrent` in `riptide-core` / `riptide-store` and no
desktop wiring yet: that touches the database schema and every exhaustive match on the kind, and
belongs to its own change. `TorrentSnapshot::download_state()` already maps onto the existing
state machine so the integration is mechanical.
