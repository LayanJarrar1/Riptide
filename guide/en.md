# Riptide user guide

[العربية](ar.md)

Riptide is a free, open-source download manager for Windows. This guide covers the current
development build. Get the alpha preview (installer or portable zip) from
[Releases](https://github.com/LayanJarrar1/Riptide/releases), or build it from source (see [CONTRIBUTING.md](../CONTRIBUTING.md)).

## Table of contents

- [The window](#the-window)
- [Adding a download](#adding-a-download)
- [While it downloads](#while-it-downloads)
- [Pause, resume, retry](#pause-resume-retry)
- [Finished files](#finished-files)
- [Removing downloads](#removing-downloads)
- [Details](#details)
- [Video, batches and the site grabber](#video-batches-and-the-site-grabber)
- [Research: open and public sources](#research-open-and-public-sources)
- [Magnet links](#magnet-links)
- [Queues and schedules](#queues-and-schedules)
- [Browser extension](#browser-extension)
- [Updates](#updates)
- [Settings](#settings)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Language and theme](#language-and-theme)
- [Where things are stored](#where-things-are-stored)
- [Portable mode](#portable-mode)
- [Command line](#command-line)
- [Troubleshooting](#troubleshooting)

## The window

Four areas, under a **title bar** that holds the menus (File, Downloads, Tools, View, Window,
Help) and the window buttons (minimize, maximize or restore, close). The window buttons stay at the
right edge in Arabic too. *View* switches theme, language, display density (comfortable or compact)
and shows or hides the sidebar. Press `Alt` or `F10` to reach the menus from the keyboard; the
arrow keys move, `Esc` leaves, and `Alt` plus the underlined letter opens a menu.

- **Sidebar** (start side): views (All, Active, Waiting, Completed, Needs attention), categories
  (Archives, Documents, Music, Video, Programs, Images, Other) and queues, each with a live count.
  Settings sits at the bottom.
- **Header**: search, the language switch and **Add download**.
- **List**: one row per download with name and server, size, a segmented progress bar, speed,
  time left, status and when it was added. Click a column header to sort; click again to flip the
  direction.
- **Status bar**: total speed with a small chart, how many downloads are active or waiting, and
  free space on the download drive.

## Adding a download

Click **Add download** or press `Ctrl+N`, then paste a link. Riptide checks the link while you
type and shows the file name, the size and whether the server allows resuming
("resumable, multiple connections"). Then:

- **Save as**: the file name, pre-filled from the server. Change it if you like.
- **Category**: picked from the extension; you can override it.
- **Save to**: the folder. **Browse** opens the Windows folder picker.
- **Connections**: how many parallel connections to use (1, 4, 8, 16 or 32). 8 suits most
  servers; some servers limit you to fewer.
- **Start**: **Now** starts immediately; **Later** adds it paused so you can start it when you
  want.
- **Checksum, login and headers**: an expected MD5, SHA-1 or SHA-256 checksum (the download is
  marked failed if the finished file does not match), a username and password for servers that
  ask for one, and extra request headers, one per line as `Name: value`.

Press **Download**. The item appears at the top of the list.

If a link is not resumable, Riptide uses a single connection and tells you so in the dialog.

## While it downloads

The progress bar is made of **stripes**: each stripe is one connection working on one part of
the file. Stripes fill independently and the bar turns solid green when every part is in. The
speed column shows a smoothed speed; time left is estimated from it.

Riptide saves its progress every second. If the app is closed, the computer loses power or the
process is killed, the download simply continues from where it was the next time the app starts.

## Pause, resume, retry

- **Pause** stops the connections and keeps everything on disk. **Resume** continues from the
  same point, as long as the server still serves the same file. If the file changed on the server,
  Riptide starts over rather than mixing two versions.
- **Pause all** and **Resume all** in the toolbar act on every download.
- A download that failed shows why in its status (connection lost, link expired, checksum
  mismatch, server error, disk full). **Retry** tries again from the saved progress.
- **Cancel** stops a download and marks it cancelled; the partial file is removed.

## Finished files

A finished download shows **Completed**. Hover a row for **Show in folder**; the row menu also
has **Open file** and **Copy link**. If a file with the same name already exists in the folder,
the new one is saved as `name (1).ext`.

When you gave a checksum, Riptide reads the finished file back and compares it; a mismatch shows
as **Checksum mismatch** and the file is not treated as complete.

## Removing downloads

**Remove** (from the row menu, or `Delete` with rows selected) takes a download out of the list.
The confirmation offers **Also delete the files from disk**; leave it unchecked to keep the file.

Select several rows with `Ctrl`+click or `Shift`+click to act on them together.

## Details

Press `Enter` on a row or choose **Details** to open the drawer. It shows:

- **Stat tiles**: bytes done of total, speed, time left, active connections.
- **Segments**: one block per part of the file, filling as it arrives.
- **Speed, last minute**: a live chart with the peak.
- **Source and destination**: the link (with a copy button), the server, the folder, the expected
  checksum, when it was added and completed.
- **Timeline**: what happened to this download, in order.

The drawer's own buttons pause, cancel or remove the download. `Esc` closes it.

## Video, batches and the site grabber

- **Video pages.** Paste the address of a YouTube, Vimeo or other page that yt-dlp supports. Riptide
  lists the qualities from the best available down to 144p, or audio only as M4A or MP3. yt-dlp and
  ffmpeg ship with the app; **Settings → About → Update video support** fetches the newest yt-dlp when
  you ask.
- **Batch downloads.** Use a pattern such as `file[001-120].zip` or `img_[a-z].jpg` and check the
  preview of every address, or paste a list of links. Lists can be imported and exported.
- **Site grabber.** Crawl a page to a depth you choose, then tick the files you want. It respects
  `robots.txt`.

## Research: open and public sources

*Research* (sidebar, or **Tools → Research**) searches open and public sources and lists files with a direct download link. Pick a tab: **All, Papers, Books, Music, Video, Games, Software, Images, Datasets, Web**.

- **Results appear as each source answers.** Every source has a chip (searching, how many results, timed out or failed with a retry button). You can keep using the page, filter by a source by clicking its chip, and press **Cancel** at any time.
- **With an AI provider** (Settings → AI) the question is expanded into extra searches, and then the results are ranked, one line of "why it matches" per result, arriving one by one while the list re-sorts in place. Without one, your own words are sent to the sources as they are.
- **Licence badge.** Each result shows *public domain*, a Creative Commons licence, *open source* with the licence name, or *licence unknown* when the source does not say. The badge repeats what the source reports; it is not legal advice.
- **Sources.** Papers: OpenAlex, Crossref, arXiv, Europe PMC. Books: Project Gutenberg, Open Library (fully open scans), LibriVox audiobooks, Internet Archive. Music: Internet Archive audio, Jamendo (needs your free client id, Settings → Research sources), Openverse, Wikimedia Commons, OpenGameArt. Video: Internet Archive public-domain films, Wikimedia Commons, NASA, Blender open movies. Games: open-source games on GitHub (OSI licence required), OpenGameArt. Software: GitHub releases. Images: Openverse, Wikimedia Commons, NASA. Datasets: Data.gov.
- **Web.** Choose a backend in Settings → Research sources: the Brave Search API (your own token, kept in the Windows Credential Manager) or a SearXNG instance, or leave it off. Web results are pages, not files: **Find files on this page** opens the site grabber on that page (robots.txt respected) so you can tick what to download.
- **What is not here, on purpose.** Riptide does not search torrent indexes, piracy or "abandonware" sites, paywall-bypass or Sci-Hub-like sources, or anything whose purpose is copyrighted content without a licence. Only download what you have the right to download. See decision D-research-1.
- Everything a source returns is treated as untrusted text: it is shown as plain text, only http and https links are used, and the AI can only rank what the sources returned.

## Magnet links

Paste a magnet link in **Add download** (or drop it on the window, copy it with the clipboard monitor on, or use **File → Open magnet link from clipboard**) and Riptide shows the files so you can choose. Paste several links, one per line, to get a list: choose the files of each in turn, or **Add all (every file)**. Tracker addresses are never shown in lists or written to the log (private trackers keep a passkey there).

## Queues and schedules

Make as many queues as you like, each with its own limit on simultaneous downloads. A queue can run
only at night, on chosen days or at a set time. When it finishes you can have Riptide exit, sleep,
hibernate, shut down or run a program, with a 30 second countdown you can cancel. A sleeping PC is
not woken by a schedule, and "run a program" takes no arguments.

## Browser extension

The extension adds a **Download this video** button with a quality picker, can download all or
selected links and all images on a page, and has capture rules. It is not in the Chrome Web Store
or Edge Add-ons yet: open **Settings → Integration** and use the install helper for Chrome and Edge
(**Open extension folder**, then Chrome's **Developer mode → Load unpacked**).

## Updates

Riptide checks for a new version once a day. When one exists you get a system notification (once per
version) and a badge in **Settings → About**, where you can install it. After an update, a
**What's new** link opens the release notes. The check can be turned off, and **Update channel** in
the same place chooses between stable and beta releases; while Riptide is in alpha, beta is the default.

## Settings

Open with the sidebar's **Settings** or `Ctrl+,`. Changes apply immediately.

- **Appearance**: language (Arabic / English) and theme (follow Windows, dark, light).
- **Downloads**: the default folder, a folder per category (empty = default), connections per
  download, connections per server (a cap across all downloads to the same host), and a speed
  limit.
- **Behaviour**: notify when a download finishes, ask before closing while downloads are active,
  keep running in the tray, start with Windows, clipboard watching.
- **Integration**: browser extension and proxy settings.
- **About**: update channel, check for updates, reset settings.

Some switches describe behaviour that is still being wired to the engine in this development
build; the interface marks them as such where it matters.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| `Ctrl+N` | Add download |
| `Ctrl+F` | Search |
| `Ctrl+Shift+N` | Batch download |
| `Ctrl+,` | Settings |
| `Ctrl+B` | Show or hide the sidebar |
| `Alt` or `F10` | Go to the menu bar |
| `↑` / `↓` | Move the selection (with `Shift`, extend it) |
| `Home` / `End` | First or last download |
| `Page Up` / `Page Down` | Move a page at a time |
| `Ctrl+A` | Select every download in the list |
| `Enter` | Open details |
| `Space` | Pause or resume the selected download |
| `Delete` | Remove the selected downloads |
| `Esc` | Clear the selection, close a dialog or the details drawer |

## Language and theme

The interface follows Windows on first start and can be switched at any time from the header or
Settings. Switching to Arabic mirrors the whole layout right-to-left, including icons that imply
a direction. The dark theme is the default; **Light** and **System** are in Settings.

## Where things are stored

| What | Where |
|---|---|
| Downloads | the folder you chose (default: your Downloads folder) |
| Partial files | `<name>.rtpart` next to the final file until it completes |
| Database (the list and progress) | `%LOCALAPPDATA%\Riptide\riptide.sqlite` |
| Logs | `%LOCALAPPDATA%\Riptide\logs` (daily files; no cookies, credentials or query strings) |

Riptide needs no administrator rights for anything and writes nothing outside these places.

## Portable mode

The portable zip comes ready: it contains a file named `portable.txt` next to `Riptide.exe`.
While that file is there, Riptide keeps the database and logs in a `data` folder next to the
executable instead of `%LOCALAPPDATA%`, so the whole thing can live on a USB drive. Delete
`portable.txt` to switch back to the installed location.

## Command line

The engine also runs on its own:

```powershell
riptide fetch https://example.com/file.zip -o D:\Downloads
riptide fetch https://example.com/file.zip --connections 16 --checksum <sha256>
riptide bench https://example.com/big.bin      # 1 vs 8 vs 32 connections
riptide paths                                   # where data and logs live
riptide --lang ar fetch --help                  # help in Arabic
```

Run the same `fetch` again to resume an interrupted download.

## Troubleshooting

- **"single connection, the server does not support ranges"**: the server ignores range
  requests, so there is nothing to parallelise and a resume starts from the beginning. Nothing is
  wrong on your side.
- **The download restarted from zero after a resume**: the file on the server changed (its
  validator no longer matches). Riptide refuses to combine bytes from two versions.
- **Link expired**: the server's signed link stopped working (common with file hosts). Get a
  fresh link from the page and add it again; **Get a new link** in the row menu opens the add
  dialog pre-filled.
- **Checksum mismatch**: the finished file does not match the checksum you entered. Check the
  checksum you were given; if it is right, the server sent a corrupted file.
- **Too many connections**: some servers answer `429` when you open too many connections. Lower
  **Connections per server** in Settings.
- **Still stuck**: look at today's file in `%LOCALAPPDATA%\Riptide\logs` and open a bug report
  with the relevant lines.
