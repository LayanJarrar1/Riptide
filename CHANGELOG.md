# Changelog

All notable changes to Riptide are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

Pre-releases are on [GitHub Releases](https://github.com/LayanJarrar1/Riptide/releases).

## [Unreleased]

### Fixed

- Downloads that start together with the same file name no longer share one partial file (the
  finished file could hold another download's bytes).
- A server that sends more bytes than its own `Content-Range` announces (jsDelivr) no longer
  yields a truncated file marked Completed; Riptide falls back to one plain connection.
- `http://user:password@host/...` links keep the password out of the downloads list; it is stored
  with the other secrets and removed with them.
- Pause followed at once by Resume no longer leaves the download paused.
- Links without a file extension get one for CSV, XML, SVG, WAV, EPUB and Office types.

### Added

- An integrated title bar with menus (File, Downloads, Tools, View, Window, Help), keyboard
  accessible (Alt or F10 to enter, arrows, mnemonics) and mirrored in Arabic; the window controls
  stay at the right. *View* also gets Sidebar (Ctrl+B) and Display density (comfortable or compact).
- Home, End, Page Up, Page Down, Shift+arrows and Ctrl+A in the downloads list.
- BitTorrent. Paste a magnet link in *Add download* (or choose a `.torrent` file, or use
  *File, Open torrent file...*), see the files with their sizes, tick the ones you want, optionally
  download them one after another, and start. Torrent rows show peers and the share ratio, the
  details drawer shows per-file progress, pause, resume, remove and "Check files again" work, and a
  torrent survives a restart or a killed process (pieces are re-checked on start, never trusted).
  Seeding is off by default: a finished torrent stops all network activity unless *Settings,
  Torrents* turns it on, with a ratio limit, a time limit and an upload cap. File names from a
  torrent go through the same sanitising as HTTP downloads and never leave the chosen folder.
  Opt-in in *Settings, Integration*: open `magnet:` links and `.torrent` files with Riptide
  (registered for your user only, removable). The database moves to schema version 5; an
  alpha.4 build cannot open it afterwards.
- **Settings -> AI** (off until you set it up): ten providers, your own API key kept only in the
  Windows Credential Manager (the page shows `••••abcd` and never gets the key back), model list
  from the provider plus a free-text model, optional compatible endpoint, a test button that
  reports latency and provider-reported tokens, a daily token/request budget, a privacy panel
  (download history for the AI off by default, a live "what will be sent" view, "Forget
  everything") and no telemetry.
- **Research** page (Tools menu and sidebar): searches OpenAlex, Crossref, arXiv, Europe PMC,
  Internet Archive, Wikimedia Commons, GitHub releases and Data.gov for open, directly
  downloadable files and adds the ones you tick to your downloads. Without an AI provider it is a
  keyword search; with one, the question is expanded, results are ranked with a one-line reason
  and every link the model cites is checked against the real results.
- "You already downloaded this": Riptide remembers finished downloads by normalised address,
  DOI, arXiv id, PubMed id, file name and size and SHA-256, warns in the Add dialog and on
  Research cards (Open file / Open folder / Download again), notices when the file was deleted,
  and can optionally skip repeats (Settings -> AI -> Download memory).
- Add dialog: optional "Suggest name and category" (only host, file name and size are sent; you
  accept or dismiss).
- New crate `riptide-research`; new database migration 5 (`seen_downloads`, `seen_keys`, after the torrents migration 4). Version 5
  is not readable by alpha.4: back up `%LOCALAPPDATA%\Riptide\riptide.sqlite` before trying the
  preview on your real data, or use the portable build.

### Changed

- The clipboard monitor also offers magnet links.
- The English strings are now a lazily loaded chunk like the Arabic ones, which keeps the initial
  bundle under its 500 kB budget as the app grows.
- The downloads list stays smooth at tens of thousands of items (measured at 50 000): progress
  updates touch only the rows on screen, and sorting and filtering reuse the previous work.


## [0.1.0-alpha.4] - 2026-10-19

An alpha for Windows 10 and 11. See the [release](https://github.com/LayanJarrar1/Riptide/releases/tag/v0.1.0-alpha.4).

### Added

- Video pages: download from YouTube, Vimeo and other sites yt-dlp supports, with a quality picker
  from the best available down to 144p, or audio only as M4A or MP3. yt-dlp and ffmpeg are bundled;
  *Settings → About → Update video support* fetches the newest yt-dlp on request.
- Queues with schedules: several queues, each with its own limit; run one only at night, on chosen
  days or at a set time. When it finishes: exit, sleep, hibernate, shut down or run a program, with
  a 30 second countdown to cancel.
- Batch downloads: patterns such as `file[001-120].zip` or `img_[a-z].jpg` with a preview of every
  address, or a pasted list of links; import and export of lists.
- Site grabber: crawl a page to a chosen depth and tick the files you want; respects `robots.txt`.
- Browser extension: a "Download this video" button with a quality picker, download all or selected
  links and all images, capture rules, and a one-click install helper for Chrome and Edge in
  *Settings → Integration*.
- Update notifications: a system notification when a new version exists (once per version), a badge
  in Settings, and a "What's new" link after updating.
- The Riptide brand: new logo, icons and website.

### Changed

- A server that answers 429 or 503 now gets fewer connections from Riptide, and the lower limit is
  remembered after a restart.
- Faster start: the app loads about 130 kB less before the first paint.

### Fixed

- A video download could finish with its progress bar stopped short of 100%.

### Known limits

- This is an alpha. Keep copies of important files.
- The browser extension is not in the Chrome Web Store or Edge Add-ons yet; load it once from
  *Settings → Integration → Open extension folder* and Chrome's *Developer mode → Load unpacked*.
- A sleeping PC is not woken by schedules. "Run a program" takes no arguments.
- Windows SmartScreen may warn on first run because the alpha is not code-signed yet: choose
  *More info → Run anyway*. SHA-256 checksums are in `SHA256SUMS.txt`.

## Earlier development

The detailed record of what was built up to and including 0.1.0-alpha.4.

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
- Logging with daily rotation and secret redaction; portable mode (`portable.txt` next to the executable keeps data in `data\`).

**Site grabber** (`riptide-crawl`, `Engine::crawl`)

- Crawl from a start page to a depth (0 = that page only) over same host, same domain or any
  site; files linked from off-site are listed, off-site pages are never read.
- HTML only (the `html5ever` tokenizer): `a`, `area`, `img`/`source` (+ `srcset`), `video`,
  `audio`, `track`, `embed`, `object`, `iframe`, `link rel=alternate|next|...`, `meta refresh`,
  `<base href>`. No JavaScript.
- `robots.txt` obeyed (`riptide` and `*` groups, `Allow`/`Disallow` with `*` and `$`,
  `Crawl-delay`), at most 2 requests in flight per host with a minimum gap, hard caps on pages
  (500), files (5 000), bytes per page (5 MiB) and time (10 min), cancellation.
- Include/exclude globs, file-type and size filters, loop-safe de-duplication, sync mode
  (`ETag`/`Last-Modified` against a previous result). Cookies and credentials go to the start
  origin only.
- `riptide-testserver` serves a crawlable `/site/`, `/robots.txt` and keeps a request log.

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

- Batch download (Ctrl+Shift+N): patterns such as `file[001-120].zip`, `img_[a-z].jpg`,
  `[0-100:5]` and several ranges in one address, a live preview with a check box per link, or a
  pasted / loaded list of links; copy or export the checked links; duplicates are skipped and
  reported. The engine adds the whole batch in one transaction.

- Queues in the sidebar: create, rename and delete (the downloads move to Main), start and stop,
  a limit per queue, drag or Alt + arrow reordering of queues and of the downloads inside one,
  drag a row onto a queue (or use Move to queue). A schedule per queue (daily hours with an
  "Only at night" preset, days of the week, a one-time start; a sleeping PC is not woken) and an
  action for when it finishes: exit, sleep, hibernate, shut down or run a program, after a
  30-second countdown you can cancel.

**Command line**

- `riptide batch "<pattern>"` downloads every address of a pattern (`--list` only prints them).
- `riptide fetch` (standalone download with resume), `riptide bench` (1 vs 8 vs 32 connections),
  `riptide ping`, `riptide paths`; help in Arabic and English.
- `riptide grab <url> --depth N --types video,documents --same-domain [--list]`: crawl a site,
  list or download the files it links to; help in Arabic and English.

**Project**

- A local test server with every failure mode the engine handles, used by all engine tests.
- `scripts/check.ps1`: the gate (lint, build, tests, clippy, bindings check).
- Public documentation: README, architecture, contributing guide, security policy, user guide.
