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

## D-torrent-2: Torrents in the app: the same list, their own engine, schema v4

Status: accepted (branch `feat/alpha5-preview`).

**Rows, not a second list.** A torrent is a `downloads` row with `kind = 'torrent'`, so the list,
filters, notifications and events work unchanged. The segment scheduler never starts one
(`start_next` skips the kind; crash recovery leaves it to `Engine::enable_torrents`); the torrent
engine (`riptide-torrent`) moves the bytes and `riptide-engine::torrent` bridges the two: every
torrent event or start-up snapshot becomes legal `DownloadState` transitions (the table is walked,
never bypassed; the one exception is a completed row that a re-check finds damaged, which is
reopened in place). Torrents do not count against the queues' concurrency limit and ignore the
per-download speed limit; the upload cap and seeding rules are in Settings, Torrents.

**Schema v4.** The `kind` CHECK had to allow `'torrent'`, which SQLite cannot alter in place, so
the migration rebuilds `downloads` (new table, copy, drop, rename) with foreign keys switched off
around the whole run and `foreign_key_check` afterwards; ids, segments and secrets are kept (test:
`v3_database_migrates_to_v4_keeping_segments_secrets_and_ids`). New table `torrents` (info-hash,
planned files, selection, seeding flag, lifetime upload and verified-download totals). An older
build cannot open a v4 file; the preview notes say so.

**Two-step add.** `torrent_preview` resolves a magnet link or reads a `.torrent` and returns the
files plus a token (the metadata stays in memory for 30 minutes, so a magnet link is not resolved
twice); `add_torrent` takes the token and the selection. The list stores the info-hash link only,
never the trackers of the magnet (they can carry a private passkey).

**The torrent engine starts lazily.** At launch only when the list already holds a torrent, else on
the first preview. An app that never touches a torrent opens no listening port and contacts no
tracker or DHT node.

**Sparse files.** Torrent pieces do not respect file boundaries, so choosing a tiny file can write
a few KB at the end of a neighbour you did not choose. Without the NTFS sparse flag that reserved
the neighbour's whole length (found with a real torrent: 263 MB of disk for a 303 KB choice). Files
we create are marked sparse (`FSCTL_SET_SPARSE`, one documented `unsafe` call in
`riptide-torrent::storage`, so the crate is `deny(unsafe_code)` with that single exception).

**Magnet and `.torrent` handlers.** Registered only when the user presses the button in Settings,
Integration, under `HKCU\Software\Classes` (`RIPTIDE_REGISTRY_ROOT` moves it for tests); `.torrent`
becomes the default only when the user has no per-user default yet; unregistering removes only what
points at this executable. A launch with a magnet link or an existing `.torrent` path (first launch
or a second one handed over by the single-instance plugin) opens the file-choice dialog; nothing is
added without the user pressing Download.


## D-research-1: Research searches open and public sources only

**Decision.** The Research page searches only sources whose purpose is open or public content,
and shows the licence the source reports. The following are out of scope and will not be added,
whatever the request: torrent indexes and trackers' search pages; piracy, warez and "abandonware"
sites; paywall-bypass and Sci-Hub-like sources; any source whose purpose is copyrighted content
without a licence. A source qualifies when its API or terms say the items are public domain, openly
licensed or open source, or when it is a public archive that marks restricted items (those are
skipped: lending-library scans, access-restricted Internet Archive items).

**Why.** Riptide is a download manager that is free for everyone; it must not become a way to find
infringing copies, and the app (and its maintainer) should be defensible in every store and every
country. Showing the licence next to each result lets people check what they may do with a file.

**What it means in the code.**
- Each adapter in `riptide-research` has a recorded fixture, a mock test and one live test, and
  names the licence field it reads (`Rights::classify` turns it into a badge: public domain, CC,
  open source, unknown). A result without a direct http(s) file link is dropped (web search
  results are marked as pages and only open the site grabber, which obeys `robots.txt`).
- General web search is a backend the user chooses and configures (Brave Search API token or a
  SearXNG instance, validated base URL); it is off by default and nothing is sent to it otherwise.
- Everything returned by a source is untrusted: escaped in the UI, never executed, http/https only
  (no `javascript:`, `file:` or `data:`), and the AI can only rank results that the sources
  returned (ids, not links; every entry is validated as it streams in).
- Jamendo needs the user's own free client id (Credential Manager); Free Music Archive has no
  keyless API today and is not included; OpenGameArt has no API, so its public search page is read
  with the 10 s crawl delay its `robots.txt` asks for.
- The page says so in one line at the foot and in a standing notice: *Riptide searches open and
  public sources. Only download what you have the right to download.*

**Streaming (same entry).** Sources run concurrently and report as they answer; the AI step runs
in the background and streams (SSE for the eight OpenAI-compatible providers, Anthropic and
Gemini; `Provider::complete_stream` has a non-streaming default). Retries happen only before the
first byte; dropping the future closes the HTTP connection, which is how Cancel works.

## D-review-1: Review round of alpha.5 (names, keys, launch arguments)

*Status: accepted.* An independent review of the preview found two data/network defects and a
handful of hardening gaps. The rules that came out of it:

- **A torrent owns only what it created.** The registry records the files its storage created
  (`created`); removing or cancelling deletes those (plus data the user declared its own with
  "use existing files"), never a name that merely appears in its planned layout. File names are
  planned around every registry layout, queued/running HTTP downloads and `.rtpart` files, and
  adds are serialised so two torrents cannot plan the same free name.
- **No torrent without a row.** Remove and cancel edit the stored torrent state when the torrent
  engine is not running, and the registry is reconciled against the rows before the engine
  restores anything. The engine starts at launch only for torrents that need the network
  (downloading, queued, checking, or seeding by choice) and on demand otherwise.
- **A secret goes only where it was entered for.** An API key is stored with the origin of the
  endpoint in force at that time; if the effective endpoint changes, the key is treated as
  missing. No command takes an address from the webview to send a key to.
- **Launch arguments are data.** The `magnet:` / `.torrent` handler is
  `"exe" --open-link -- "%1"`; an open launch has exactly one data argument and never runs the
  installer hooks or honours `--log-level`.
- **Untrusted addresses are checked.** Web seed addresses (built by the engine from the names in
  the `.torrent`) and search results are refused when they point at a private, loopback or
  link-local address, unless the result comes from the user's own private SearXNG.
- **Bodies are bounded as they arrive**, not after they were buffered.

## D-advisories-1: Dependency advisories accepted for 0.1.0-alpha.5

*Status: accepted, review by 2026-12-31.* `cargo audit` on the alpha.5 lockfile (advisory database of
2026-10-21) reported seven vulnerabilities. Three are fixed by a targeted patch update
(`rustls-webpki` 0.103.16, `webpki-roots` and `webpki-root-certs` 1.0.10: the distrusted emSign
intermediates, RUSTSEC-2026-0381, -0382, -0383). Four are accepted for now because no compatible
fixed release exists, and each is listed in `.cargo/audit.toml` with the same reason and expiry:

| Id | Crate (pulled in by) | Why it is not reachable | What to bump later |
|---|---|---|---|
| RUSTSEC-2026-0293 | `ringbuf` 0.4.8 (`librqbit-utp` 0.7.0, the uTP transport of librqbit) | Undefined behaviour only when an element's `Drop` panics during `Consumer::skip`/`clear`. uTP stores plain bytes, which have no `Drop`, so the condition cannot occur. Fixed upstream in `ringbuf` 0.5.2, which `librqbit-utp` 0.7.0 (the newest release) does not allow. | `librqbit-utp` once it moves to `ringbuf` >= 0.5.2, or a librqbit that does. |
| RUSTSEC-2026-0367, -0368, -0369 | `zip` 4.6.1 (`tauri-plugin-updater` 2.14.0) | All three are about `ZipArchive::extract` / `ZipStreamReader::extract` (SUID bits, symlink targets). Riptide never calls them; on Windows the updater downloads a signed NSIS installer and runs it, and the archive path is used only for other platforms. Fixed in `zip` >= 9.0.0, which the updater plugin does not allow yet. | `tauri-plugin-updater` once it depends on `zip` >= 9. |

The remaining `cargo audit` warnings are unmaintained or unsound crates that only build for Linux
(`proc-macro-error`, `zbus_names`, `zvariant`, `zvariant_derive`, `glib`); the Windows build does not
contain them. They stay visible as warnings. Revisit this entry when `.cargo/audit.toml` expires.
