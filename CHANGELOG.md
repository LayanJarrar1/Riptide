# Changelog

All notable changes to Riptide are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

There are no releases yet. Everything below is on `main` and changes often.

## [Unreleased]

### Added

**Engine**

- Probe with `HEAD` and ranged-`GET` fallback; manual redirects (at most 20, credentials dropped
  across origins); Basic and Digest authentication; cookies, referrer and user agent per request.
- Multi-connection, segmented transfer with work stealing; a per-host connection cap; a retry
  policy with exponential backoff, jitter and `Retry-After`.
- Byte-exact resume with `If-Range`; restart from zero when the server's validator changed;
  detection of wrong `Content-Length` and of servers that compress despite
  `Accept-Encoding: identity`.
- Preallocated, sparse `.rtpart` files written in place, then renamed atomically; `name (1).ext`
  on conflict.
- Checksum verification (MD5, SHA-1, SHA-256) after completion.
- Filename sanitiser with property tests (directories, `..`, control characters, Windows reserved
  names, trailing dots/spaces, NFC, 200-byte cap), RFC 5987 `Content-Disposition` names
  including Arabic.
- SQLite (WAL) persistence with migrations, per-second checkpoints and crash recovery on open.
- A runtime with a queue (`max_active`), scheduler, settings, and an event stream.
- Logging with daily rotation and secret redaction; portable mode (`data` next to the executable).

**Desktop app (Windows)**

- Downloads list with virtual scrolling, segmented progress bars, sort, search, status and
  category filters, multi-select, context menu and full keyboard operation.
- Add dialog with live probe preview (name, size, resumable), folder picker, category,
  connections, start now / later, expected checksum, login and custom headers.
- Details drawer with stat tiles, segment map, live speed chart, source and destination, timeline.
- Pause, resume, cancel, retry, remove (with or without the file), open file, show in folder,
  copy link; pause all / resume all.
- Settings: language, theme, default folder and folder per category, connections per download and
  per server, behaviour toggles.
- Arabic and English with a fully mirrored right-to-left layout, live language switch, dark and
  light themes, reduced-motion support.
- A real engine inside the app: Tauri commands and an event bridge, with TypeScript types
  generated from the Rust types.

**Command line**

- `riptide fetch` (standalone download with resume), `riptide bench` (1 vs 8 vs 32 connections),
  `riptide ping`, `riptide paths`; help in Arabic and English.

**Project**

- A local test server with every failure mode the engine handles, used by all engine tests.
- `scripts/check.ps1`: the gate (lint, build, tests, clippy, bindings check).
- Public documentation: README, architecture, contributing guide, security policy, user guide.
