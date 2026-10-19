# Riptide architecture

This document describes how Riptide is built today: the crates, how a download flows through
them, the guarantees the engine makes, and how the desktop app talks to it. It is written for
contributors; the user-facing explanation lives in the [user guide](guide/en.md).

## Table of contents

- [The one-paragraph version](#the-one-paragraph-version)
- [Crates and apps](#crates-and-apps)
- [Life of a download](#life-of-a-download)
- [The state machine](#the-state-machine)
- [Segmentation and work stealing](#segmentation-and-work-stealing)
- [Guarantees](#guarantees)
- [Files on disk](#files-on-disk)
- [Persistence](#persistence)
- [The runtime](#the-runtime)
- [The desktop app](#the-desktop-app)
- [Logging](#logging)
- [Testing](#testing)

## The one-paragraph version

A pure Rust **model** (`riptide-core`) defines what a download is and which state changes are
legal. An HTTP **engine** (`riptide-net`) moves bytes for one download at a time: it probes the
server, splits the file into segments, fetches them over several connections, writes each byte at
its final offset, and resumes safely after any interruption. A **store** (`riptide-store`)
persists every record and every segment's progress in SQLite. A **runtime** (`riptide-engine`)
owns the store, runs a queue of downloads, recovers after crashes and broadcasts events. The
**desktop app** (Tauri + Angular) and the **CLI** are two thin clients of that runtime; neither
contains download logic.

## Crates and apps

| Crate | Responsibility | Depends on |
|---|---|---|
| `riptide-core` | `Download`, `Segment`, `DownloadState` and its transitions, events, the segment planner, the filename sanitiser, categories. No IO. | – |
| `riptide-net` | Probe, redirects, auth, ranges, multi-connection transfer, retry policy, part files, checksum verification. One download at a time. | core |
| `riptide-store` | SQLite (WAL), schema migrations, records, segments, settings, the separate `secrets` table. | core |
| `riptide-engine` | The runtime: scheduler, queues (a limit, a schedule and a finish action per queue, host caps learned from `429` / `503`), batch adds in one transaction, video downloads, crash recovery, event broadcast, settings, `crawl`. The API the app and CLI use. | core, net, store, media, crawl |
| `riptide-media` | Video pages (YouTube, Vimeo and others) through the bundled yt-dlp sidecar: probe into a quality menu, download with progress, pause and resume, cookies in a private temporary file. Refuses live streams and DRM. | core |
| `riptide-crawl` | The site grabber: crawl to depth N, scope, `robots.txt`, per-host politeness, filters, caps; lists files, downloads nothing. | core, net |
| `riptide-ipc` | Length-prefixed JSON over a per-user named pipe (Windows) or Unix socket; message schema. | – |
| `riptide-log` | `tracing` setup, daily rotating files, redaction layer, installed vs portable paths. | – |
| `riptide-cli` | `riptide fetch`, `batch`, `grab`, `bench`, `ping`, `paths`. Bilingual help. | engine, crawl, net, log |
| `riptide-host` | Native Messaging host `app.riptide.host` (`riptide-host.exe`): checks the calling extension id, accepts only `ping`/`probe`/`probe_media`/`add`/`refresh`/`list`, forwards them to the app's pipe, starts the app on `ping`. | ipc, log |
| `riptide-testserver` | An `axum` server with deterministic bytes and every failure mode the tests need. Dev only. | – |
| `apps/desktop/src-tauri` | Tauri shell: hosts the engine, exposes commands, forwards events, serves the IPC pipe, registers the browser host under `HKCU`. | engine, ipc, host, log |
| `apps/desktop/src` | Angular UI. Knows nothing about HTTP or SQLite. | the Tauri bridge |
| `apps/website` | The public site. Static, bilingual, built by a dependency-free Node script. | – |

The engine crates never import Tauri, Angular or Windows-only APIs outside small `cfg` modules,
so the same code can later run on other platforms.

## Life of a download

1. **Add.** A client (the app's dialog, the CLI) builds a `DownloadRequest` (URL, optional
   folder, name, connections, checksum, start paused, referrer, headers, credentials). The
   runtime sanitises the filename, stores the record as `Queued`, puts secrets in their own table,
   and emits `added`.
2. **Schedule.** The scheduler runs at most `max_active` downloads at once, oldest first within
   the same priority. When a slot frees up it starts the next queued download.
3. **Probe.** `HEAD`, falling back to a ranged `GET` when the server does not answer `HEAD`
   properly. Redirects are followed manually (at most 20; cookies and `Authorization` are dropped
   when the origin changes). The probe captures the final URL, the size, whether the server honours
   `Range`, `ETag` / `Last-Modified`, the content type and the suggested filename
   (`Content-Disposition`, RFC 5987 included). The record moves to `Probing` → `Downloading` and
   emits `probed`.
4. **Plan.** With a known size and range support the planner splits the file into
   `min(connections, size / 1 MiB)` equal segments. Without ranges or without a size there is one
   streaming segment.
5. **Transfer.** Each segment is one HTTP connection sending `Range: bytes=start+written-end` and
   `If-Range: <validator>`. Bytes go through a writer thread that coalesces contiguous chunks and
   writes them at their final offset into the `.rtpart` file. Progress events are coalesced to at
   most ten per second per download; a checkpoint (all segments' `written`) is persisted at least
   once a second and on every state change.
6. **Steal.** When a connection finishes, the largest remaining segment is split in two if the
   remainder is at least 2 MiB, and the idle connection takes the second half.
7. **Verify.** When every segment is complete the record moves to `Verifying`: the file size is
   checked against the declared length, and if the user gave a checksum (MD5, SHA-1, SHA-256) the
   file is read back on a blocking thread and compared.
8. **Finish.** The part file is renamed atomically to its final name (`name (1).ext` if the name
   is taken), the record moves to `Completed`, secrets are deleted, and `completed` is emitted.

Pause cancels the connections and keeps the checkpoint; resume starts again at step 3, and the
`If-Range` validator decides whether the bytes on disk are still valid.

## The state machine

```
Queued ─► Probing ─► Downloading ─► Verifying ─► Completed
  ▲          │            │  ▲           │
  │          ▼            ▼  │           ▼
  └─────── Paused ◄───────┘  └───── Failed(retryable)
Cancelled is reachable from every non-terminal state.
```

Every other transition is `Error::IllegalTransition`, enforced in `riptide-core` and tested
exhaustively, so no component can put a download into a state the others do not expect.

## Segmentation and work stealing

The planner (`riptide-core::segment`) is a pure function from (size, connections, minimum
segment size) to a list of byte ranges, with property tests asserting that the ranges are
contiguous, non-overlapping and cover exactly `[0, size)`. Work stealing happens in the transfer
coordinator: a finishing connection asks for the largest segment with at least 2 MiB left, the
coordinator shrinks that segment's `end`, and a new segment with the second half is appended and
persisted before the new connection starts. A per-host limiter caps simultaneous connections to
one server across all downloads.

Because every segment writes at its own absolute offset, segments can finish in any order and the
file on disk is correct the moment the last byte lands.

## Guarantees

**Byte-exact or nothing.** A download is `Completed` only when every segment is written, the
server's validator still matches, and the expected checksum, if given, matches. On resume the
engine sends `If-Range`; if the server answers `200` instead of `206`, or the validator changed,
the download restarts from zero. Bytes from two versions of a file are never spliced. Every resume
path has a test that compares the SHA-256 of the result with the source.

**Crash-safe at every instant.** All state lives in SQLite in WAL mode and changes
transactionally; segment progress is flushed at least once a second and on every state change.
Killing the process at any moment leaves a resumable download: on the next start, records found in
`Downloading`/`Probing`/`Verifying` are brought back to `Queued` with their checkpoints intact.

**Never trust the server.** Filenames are sanitised (no directories, no `..`, no control
characters, no Windows reserved names, no trailing dots or spaces, NFC-normalised, at most 200
bytes) and the engine refuses to write outside the chosen folder. Redirects are capped. TLS
verification is on; the per-download "ignore certificate errors" switch is never a default.
`Accept-Encoding: identity` is always sent so sizes and ranges mean bytes on disk; a server that
compresses anyway is detected and handled as a stream.

**Secrets stay secret.** Cookies, headers and credentials live in the `secrets` table, are deleted
when the download completes or is removed, are only sent to the origin they were captured for,
and never reach a log line (the redaction layer strips them, and URLs are logged without query
strings).

## Files on disk

| What | Where |
|---|---|
| Partial file | `<folder>\<name>.rtpart`, preallocated to the final size, sparse on NTFS so out-of-order writes do not trigger zero-filling |
| Finished file | `<folder>\<name>` (atomic rename; `name (1).ext` on conflict) |
| Database | `%LOCALAPPDATA%\Riptide\riptide.sqlite` (+ `-wal`, `-shm`) |
| Logs | `%LOCALAPPDATA%\Riptide\logs\riptide.<date>.log`, rotated daily |
| Portable mode | a `portable.txt` file next to the executable switches everything to `data\` next to it |

## Persistence

`riptide-store` opens SQLite with WAL and a busy timeout, runs numbered migrations on open, and
exposes typed operations: insert/update/list downloads, replace a download's segments in one
transaction, read and write settings as JSON values, and the secrets table. The store is wrapped
in a mutex inside the runtime and every call runs on a blocking thread so the async runtime never
waits on disk.

## The runtime

`riptide_engine::Engine` is a cheap-to-clone handle. `Engine::open` opens the store, recovers
crash leftovers, builds the HTTP client and the per-host limiter, and starts the scheduler. The
API is small: `add`, `probe`, `pause`, `resume`, `cancel`, `remove`, `pause_all`, `resume_all`,
`list`, `get`, `segments`, `settings` / `set_settings`, `subscribe`, `shutdown`.

Events (`added`, `probed`, `state_changed`, `progress`, `segment_progress`, `completed`,
`failed`, `removed`) are broadcast on a `tokio::sync::broadcast` channel; the Tauri shell and the
CLI subscribe and render them. Engine settings (default folder, connections per download,
connections per host, maximum active downloads, user agent) are persisted as one JSON document in
the store's `settings` table.

`shutdown` stops every running download gracefully (checkpoint, then close), so closing the app
window never loses more than a second of progress.

## The desktop app

The Rust side (`apps/desktop/src-tauri`) opens the engine when the app starts, keeps it as Tauri
managed state, exposes one `#[tauri::command]` per engine operation (plus `open_file`,
`show_in_folder`, `pick_folder`, `app_info`), forwards every engine event to the web view as the
`riptide://event` Tauri event, and calls `shutdown` when the window closes.

The TypeScript types for every record and event are **generated from the Rust types** with
`ts-rs` into `apps/desktop/src/app/tauri/bridge/`; the gate fails if they are stale. There is one
schema, defined once.

The Angular side injects an abstract `DownloadEngine` (signals for the list, the active set and
speeds; promise-returning commands). Inside Tauri the provider resolves it to
`TauriEngineService`, which loads the list once and then keeps it current from the event stream.
In a plain browser (and in Karma) it resolves to `MockEngineService`, a simulation with the same
API. Components cannot tell the difference, which is what makes the UI testable without a
runtime.

Language (`ar`/`en`, auto-detected, switchable live, `html[dir]` mirrored) and theme
(system/dark/light) are UI preferences and live in `localStorage`; everything the engine needs
lives in the engine's settings and is synced through `get_engine_settings` /
`set_engine_settings`.

## Logging

`riptide-log` installs `tracing` with a daily rotating file appender and a redaction layer. Log
lines never contain cookies, authorization headers, credentials or query strings. The CLI and the
app accept `--log-level` (`error`…`trace`, or a directive such as `riptide_net=debug,info`).

## Testing

- **Unit tests** in every crate; the state machine's transition table is tested exhaustively;
  property tests cover the segment planner and the filename sanitiser.
- **Engine tests** run against `riptide-testserver` spawned in-process. Its endpoints model the
  real world: `/ok`, `/noranges`, `/nolength`, `/slow`, `/drop`, `/flaky`, `/retry-after`,
  `/limit`, `/redirect`, `/etag-changes`, `/wrong-length`, `/expires`, `/needs-cookie`,
  `/needs-referer`, `/needs-auth/basic`, `/needs-auth/digest`, `/gzip`, `/cd/arabic`,
  `/cd/evil`, `/corrupt`. Every endpoint serves deterministic bytes so a test can compare hashes.
- **Kill tests** start a real download in a child process, kill it at a random moment, run it
  again and assert the SHA-256 of the result equals the source.
- **Runtime tests** exercise the scheduler, pause/resume/cancel/remove, queue limits and
  crash recovery across two `Engine::open` calls on the same database.
- **UI tests** (Karma, headless Chrome) cover the stores, the mapping between bindings and the
  UI model, the Tauri engine service against a fake IPC, and the settings sync.
- **Benchmarks**: `riptide bench` measures 1 vs 8 vs 32 connections against the local server.
