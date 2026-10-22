# Changelog

All notable changes to Riptide are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

Pre-releases are on [GitHub Releases](https://github.com/LayanJarrar1/Riptide/releases).

## [Unreleased]

### Downloads list

- Columns can be resized: drag the grip on a header's edge, or focus it and use the arrow keys
  (Shift for bigger steps). Double-click a grip, or press Enter on it, to restore that column;
  right-click the header to reset every column. Widths are kept across restarts, and a list wider
  than the window scrolls sideways with the header following.
- The details drawer docks beside the list only when the list keeps at least 600 px; in a narrower
  window it slides over the list instead, so the table keeps its columns, scroll position and
  selection. Click outside it or press Escape to close it.
- Search also finds downloads by status and category name (in English or Arabic, e.g. "paused",
  "مستندات"), ignores Arabic diacritics, tatweel and alef/yeh variants, and waits for a pause in
  typing on histories of 5,000 or more downloads.
- New tests hold the list to a bounded number of rendered rows at 100, 1,000, 10,000 and 50,000
  downloads, while scrolling, sorting, searching and with the details drawer open.

## [0.1.0-alpha.5] - 2026-10-21

BitTorrent, Research with an optional AI assistant, and an integrated title bar with menus. An alpha for
Windows 10 and 11. See the [release](https://github.com/LayanJarrar1/Riptide/releases/tag/v0.1.0-alpha.5).

### Security and hardening

- Updated TLS trust roots: `rustls-webpki` 0.103.16 and `webpki-roots` 1.0.10 stop trusting the
  distrusted emSign intermediates (RUSTSEC-2026-0381, -0382, -0383). Four remaining advisories
  (`ringbuf` in the torrent transport, `zip` in the updater plugin) are not reachable and are
  documented with an expiry in `docs/decisions.md` (D-advisories-1).
- Hardening from the independent security review of this release:
  - Two torrents that name the same file no longer share it, and removing or cancelling a torrent
    deletes only files it created itself; names are planned around every other torrent, queued
    downloads and `.rtpart` files.
  - A torrent removed or cancelled while the torrent engine was not running no longer comes back
    and downloads with no row in the list.
  - Pause all pauses HTTP downloads even when the torrent engine is not running.
  - Files of a torrent get the Mark of the Web as soon as each one completes, not at the end.
  - The torrent engine (listening port, DHT, uTP and peer connections) starts only when a torrent
    is downloading, checking or seeding, and is shut down completely five seconds after the last
    one stops (a finished torrent with seeding off, a paused one, a seeding limit reached); it
    starts again when you add, resume or re-check a torrent. Nothing stays open after a finished
    download.
  - Files you did not select no longer stay behind as full-size empty files: the bytes of a piece
    shared with a selected file are dropped when the engine stops, and the file is created again
    if you select it later. Files with real data, files you added yourself and files of other
    torrents are never touched.
  - Answers from AI providers and research sources are cut off at 8 MiB while they arrive.
  - An API key is sent only to the address it was saved for; changing the endpoint asks for the
    key again. The model list is always fetched from the saved endpoint.
  - The `magnet:` handler passes the link as data (`--open-link -- "%1"`); a link that breaks out
    of its quotes can no longer add flags such as `--unregister-host` or `--log-level`.
  - Web seed downloads and search results that point at the local network, localhost or a
    metadata address are refused; web seed addresses use the names written in the `.torrent`.
  - Smaller: the upgrade keeps the old database as `riptide.sqlite.v3.bak` (written to a temporary
    file, stripped of the saved cookies, headers and passwords, flushed and only then renamed; a
    failed attempt leaves nothing behind and a good older copy is never replaced; deleted
    automatically after 30 days, and safe to delete by hand); ids of removed
    downloads are not reused after the upgrade; folders are never created through a junction;
    the server-sent-events parser is linear; research hints are deleted; magnet links are
    de-duplicated by info-hash; file names with `&`, `=` or `+` keep their extension in name
    suggestions; no `Referer` on redirects; more Windows device names are defused; a cancelled
    AI search still counts against the daily budget; unregistering restores another app's
    `magnet:` handler.

### Added

- **BitTorrent.** Paste a magnet link in *Add download* (or choose a `.torrent` file, or use *File,
  Open torrent file...*), see the files with their sizes, tick the ones you want, optionally download
  them one after another, and start. Several magnet links pasted at once (one per line), dropped on the
  window, or read from the clipboard (*File, Open magnet link from clipboard*) open the file choice one
  after the other; `.torrent` files dropped on the window open it too. Torrent rows show peers and the
  share ratio, the details drawer shows per-file progress, and pause, resume, remove and "Check files
  again" work. A torrent survives a restart or a killed process (pieces are re-checked on start, never
  trusted). File names from a torrent go through the same sanitising as HTTP downloads and never leave
  the chosen folder. Verified on real torrents (Ubuntu 24.04.5, 3.8 GB, and Debian 13.7.0: SHA-256 equal
  to the official `SHA256SUMS`).
- **Seeding is off by default.** A finished torrent stops all network activity unless *Settings,
  Torrents* turns seeding on, with a ratio limit, a time limit and an upload cap.
- **Web seeds.** Torrents that list web seeds (HTTP mirrors, BEP 19), such as Internet Archive item
  torrents, can be downloaded from the web seed as ordinary downloads when few peers are online. The
  file choice says so, and that those downloads are not checked against the torrent piece hashes.
- **Opt-in handlers.** In *Settings, Integration*: open `magnet:` links and `.torrent` files with
  Riptide (registered for your user only, removable). The clipboard monitor also offers magnet links.
- **Research** page (Tools menu and sidebar). Searches open, legal sources for directly downloadable
  files and adds the ones you tick to your downloads. Tabs for Papers, Books, Music, Video, Games,
  Software, Images, Datasets and Web. Sources: OpenAlex, Crossref, arXiv, Europe PMC, Internet Archive
  (including audio and public-domain films), Wikimedia Commons, GitHub releases and open-source games,
  Data.gov, Project Gutenberg, Open Library, LibriVox, Jamendo, Openverse, Blender open movies,
  OpenGameArt and NASA. Results stream in: every source's results appear as soon as that source answers,
  with per-source status, retry and Cancel. Every result has a licence badge. Optional web search
  through Brave or SearXNG, with "Find files on this page". No torrent indexes and no piracy sources
  (decision D-research-1).
- **Optional AI assistant** in *Settings, AI* (off until you set it up). Ten providers (OpenAI,
  Anthropic, Gemini, DeepSeek, OpenRouter, DeepInfra, Mistral, Groq, Together, Fireworks), your own API
  key kept only in the Windows Credential Manager (the page shows `••••abcd` and never gets the key
  back), a model list from the provider plus a free-text model, an optional compatible endpoint, a
  test button, a daily token and request budget, and a privacy panel (download history for the AI off
  by default, a live "what will be sent" view, "Forget everything"). With a provider set up, the
  question is expanded, results are ranked with a one-line reason as they stream in, and every link the
  model cites is checked against the real results. Without one, Research is a plain keyword search.
- **"You already downloaded this."** Riptide remembers finished downloads by normalised address, DOI,
  arXiv id, PubMed id, file name and size and SHA-256, warns in the Add dialog and on Research cards
  (Open file, Open folder, Download again), notices when the file was deleted, and can optionally skip
  repeats (*Settings, AI, Download memory*).
- **Suggested names** in the Add dialog (off by default): only the host, the file name and the size are
  sent; you accept or dismiss the suggestion.
- **An integrated title bar with menus** (File, Downloads, Tools, View, Window, Help), keyboard
  accessible (Alt or F10 to enter, arrows, mnemonics) and mirrored in Arabic; the window controls stay
  at the right. *View* also gets Sidebar (Ctrl+B) and Display density (comfortable or compact).
- Home, End, Page Up, Page Down, Shift+arrows and Ctrl+A in the downloads list.
- Magnet links and tracker announce addresses are masked in the log.
- New crates `riptide-torrent` and `riptide-research`; database schema version 5 (torrents, then the
  download memory).

### Changed

- The downloads list stays smooth at tens of thousands of items (measured at 50 000): progress updates
  touch only the rows on screen, and sorting and filtering reuse the previous work.
- The English strings are now a lazily loaded chunk like the Arabic ones, which keeps the initial
  bundle under its 500 kB budget as the app grows.

### Upgrading

- alpha.4 updates in place. The database is upgraded to schema version 5 on first start; **alpha.4
  cannot open it afterwards**. Back up `%LOCALAPPDATA%\Riptide\riptide.sqlite` (or the `data` folder of
  the portable build) if you may want to go back.

### Known limits

- This is an alpha. Keep copies of important files. Windows only.
- Seeding upload and the ratio stop were checked by engine tests but not seen live against a real peer.
- Brave Search and Jamendo were not called live (they need your own token or client id); their results
  are built from the documented response shape. Public services are sometimes slow or down, and the
  Research page says so and offers a retry. Most public SearXNG instances refuse JSON; use your own.
- Web-seed downloads are not checked against the torrent's piece hashes.
- The native file pickers and a real drag from Explorer were not driven in testing; the hand-over by
  second launch and the window's drop event were.
- The alpha is not code-signed, so SmartScreen may warn on first run. SHA-256 checksums are in
  `SHA256SUMS.txt`.
- Do not go back to alpha.4 after upgrading: it cannot open schema version 5. The
  upgrade keeps `riptide.sqlite.v3.bak` next to the database for that case (without saved
  cookies or passwords; deleted automatically after 30 days).
- While a torrent is downloading or seeding, its listening port is open on all interfaces (Windows
  may ask about the firewall; allowing it needs no admin rights, declining only stops incoming
  peers). It closes five seconds after the last torrent stops.
- The private-network check for web seeds and search results resolves a host name once when you
  click and again when the download connects; a hostile DNS server could answer differently the
  second time. Redirects from a public address to a private one are refused as for every download.
- If a torrent needs a folder named like a file another torrent holds, Riptide refuses it with an
  error instead of renaming the folder.
- A search that is cancelled while the model is still answering is charged for its request and
  prompt only (the reply tokens already streamed are not counted).
- Marking each finished torrent file as downloaded from the internet relies on engine events; if
  the app is so busy that events are dropped, the remaining files are marked when the torrent
  completes.

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
- Downloads that start together with the same file name no longer share one partial file (the
  finished file could hold another download's bytes).
- A server that sends more bytes than its own `Content-Range` announces (jsDelivr) no longer
  yields a truncated file marked Completed; Riptide falls back to one plain connection.
- `http://user:password@host/...` links keep the password out of the downloads list; it is stored
  with the other secrets and removed with them.
- Pause followed at once by Resume no longer leaves the download paused.
- Links without a file extension get one for CSV, XML, SVG, WAV, EPUB and Office types.

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
