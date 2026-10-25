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
- [BitTorrent and magnet links](#bittorrent-and-magnet-links)
- [Set up AI (optional)](#set-up-ai-optional)
- [AI assistant and your privacy](#ai-assistant-and-your-privacy)
- [Already downloaded](#already-downloaded)
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
- **No setup needed.** Out of the box Research searches the open sources with your own words (keyword mode). AI is an optional upgrade: see [Set up AI (optional)](#set-up-ai-optional).
- **With an AI provider** (Settings → AI) the question is expanded into extra searches, and then the results are ranked, one line of "why it matches" per result, arriving one by one while the list re-sorts in place. Without one, your own words are sent to the sources as they are.
- **Licence badge.** Each result shows *public domain*, a Creative Commons licence, *open source* with the licence name, or *licence unknown* when the source does not say. The badge repeats what the source reports; it is not legal advice.
- **Sources.** Papers: OpenAlex, Crossref, arXiv, Europe PMC. Books: Project Gutenberg, Open Library (fully open scans), LibriVox audiobooks, Internet Archive. Music: Internet Archive audio, Jamendo (needs your free client id, Settings → Research sources), Openverse, Wikimedia Commons, OpenGameArt. Video: Internet Archive public-domain films, Wikimedia Commons, NASA, Blender open movies. Games: open-source games on GitHub (OSI licence required), OpenGameArt. Software: GitHub releases. Images: Openverse, Wikimedia Commons, NASA. Datasets: Data.gov.
- **Web (optional, advanced, off by default).** Search engines charge for their APIs, so Riptide does not ask ordinary users for a key. If you want it, choose a backend under Advanced in Settings → Research sources: the Brave Search API (paid, needs a card, your own token kept in the Windows Credential Manager) or a SearXNG instance (free if you, or someone you trust, runs one). Web results are pages, not files: **Find files on this page** opens the site grabber on that page (robots.txt respected) so you can tick what to download.
- **What is not here, on purpose.** Riptide does not search torrent indexes, piracy or "abandonware" sites, paywall-bypass or Sci-Hub-like sources, or anything whose purpose is copyrighted content without a licence. Only download what you have the right to download. See decision D-research-1.
- Everything a source returns is treated as untrusted text: it is shown as plain text, only http and https links are used, and the AI can only rank what the sources returned.

## BitTorrent and magnet links

**Add a torrent.** Paste a magnet link in **Add download**, choose a `.torrent` file (**Choose .torrent file...** in the dialog, or **File → Open torrent file...**), or drop a `.torrent` file or a magnet link on the window. You can also copy a magnet link with the clipboard monitor on, or use **File → Open magnet link from clipboard**.

**Choose files before anything starts.** A magnet link does not contain the file list, so Riptide asks other peers for it (this can take up to two minutes). You then see every file with its size: tick the ones you want, pick the folder, and optionally turn on **Download one file after another** (first file first, handy when you want to open them while the rest arrives). Press **Download**. A torrent that is already in your list says so and offers **Show it**.

**Several at once.** Paste several magnet links, one per line, and choose the files of each in turn, or press **Add all (every file)**.

**While it downloads.** The row shows the peers (for example "20 of 574 peers") and, once there is something to share, the ratio. The details drawer lists every file with its own progress bar; a file you did not select says *not selected* and is never written. **Pause** and **Resume** work like any other download. If Riptide is closed or the process is killed, the torrent comes back on the next start and its pieces are checked again, never trusted. **Check files again** (row menu) re-verifies everything on disk, keeps the intact pieces and fetches the rest.

**Seeding is off by default.** When a torrent finishes, Riptide stops all network activity for it. To keep sharing, open **Settings → Torrents** and turn on *Keep sharing after a download finishes*; you can stop at a share ratio, stop after some minutes, and set an upload limit (which also applies while downloading). The same page has DHT (peers without a tracker; private torrents never use it) and the port for incoming peers; both apply after a restart.

**Web seeds.** Some torrents, such as Internet Archive items, list ordinary HTTP mirrors. If few peers are online, the file choice offers **Download from the web seed**: the files you ticked become normal downloads. Those downloads are not checked against the torrent's piece hashes, and the dialog says so.

**Open magnet links and .torrent files with Riptide.** This is opt-in: **Settings → Integration** registers the `magnet:` links and the `.torrent` file type for your Windows user only (no administrator rights), and the same page removes them again.

**Safety.** File names inside a torrent go through the same checks as any download (no folders outside the one you chose, no reserved Windows names). Tracker addresses are never shown in lists or written to the log, because private trackers keep a passkey there.

Only download what you have the right to download.

## Set up AI (optional)

Research already works without AI. Adding it makes Research plan extra searches, rank the results and explain each match. It is free to set up with the right provider and takes about a minute.

1. Open **Settings → AI → Quick setup** (or click **Keyword search · Add AI** in Research). Choose where the AI runs: **Google Gemini** (free, recommended), **Groq** (free, very fast), **On this computer** (Ollama or LM Studio: free and private, no key, the app must be installed and running) or **Another provider** (OpenAI, Anthropic, OpenRouter, DeepSeek, Mistral, Together, Fireworks, DeepInfra).
2. **Get your key.** Press **Open the page**, sign in, press the button that creates a key and copy it, then press **Paste** (or Ctrl+V in the field). Riptide recognises a key from its start (for example `AIza` for Gemini, `gsk_` for Groq) and switches provider if you picked the wrong one. The local option has no key, so this step is skipped.
3. **Test.** Riptide saves the key, picks a sensible model, saves your choice and runs a small test. When it says **AI is ready** you are done. If something fails you see why, and you can try again, use another key or remove the key.

What you should know:

- **Cost.** Gemini and Groq have free plans with no card. A paid provider charges you directly: Riptide takes no cut and has no account.
- **Privacy.** Riptide sends only your search words. Your download history is never sent unless you turn that on in Settings → AI. The free plans of cloud providers may use prompts to improve their products; use the local option if that matters to you.
- **Remove it.** Settings → AI → **Remove key** deletes the key from this computer. **Forget everything** also resets the AI settings. Research keeps working in keyword mode.
- **Change the model.** Settings → AI → **Advanced** has the provider, key, model, endpoint and limits.

## AI assistant and your privacy

The AI assistant is optional and **off until you set it up** in **Settings → AI**. Riptide has no accounts and no telemetry; it never sends anything to an AI provider unless you set one up and use a feature that needs it.

- **Provider.** OpenAI, Anthropic, Gemini, DeepSeek, OpenRouter, DeepInfra, Mistral, Groq, Together or Fireworks. You pay the provider directly; Riptide takes no cut. Pick a model from the provider's list or type any model name, and optionally set a compatible endpoint (https, or http for localhost).
- **Your key.** Paste your own API key and press **Save key**. It is stored only in Windows Credential Manager. The page shows it masked (for example `••••abcd`) and cannot read it back; it is never written to Riptide's settings, database, logs or events. **Remove key** deletes it.
- **Test.** The test button reports how long the provider took and how many tokens it says it used.
- **Daily budget.** A limit on tokens and requests per day. Riptide stops calling the provider once either limit is reached; empty means no limit.
- **What the AI does.** In Research it widens your question into extra searches and ranks the results with one line of "why it matches". Every link the model mentions is checked against the real results, so it cannot invent a download. In the **Add dialog**, *Suggest name and category* (off by default) sends only the link's host, the file name without its query string and the size, and nothing is applied until you accept it.
- **Your download history stays yours.** *Let the AI see my download history to avoid duplicates* is off by default. When on, only the titles of up to 25 recent downloads (never folders or paths) are added to Research requests.
- **See exactly what is sent.** **What will be sent** shows the messages of the most recent AI request. They are kept in memory only and disappear when Riptide closes. **Forget everything** (asks you to click twice) deletes every saved API key, today's usage, the last request and the AI settings.

Without an AI provider, Research is a plain keyword search over the sources.

## Already downloaded

Riptide remembers what you finished downloading (the address, DOI, arXiv id, PubMed id, file name and size, and SHA-256) and warns you before you fetch it again: in the Add dialog and on Research cards, with **Open file**, **Open folder** and **Download again**. It notices when the file is no longer on disk and then lets you download it again without a warning. It stays on your computer. *Skip downloads I already have* (**Settings → AI → Download memory**, off by default) refuses repeats unless you choose Download again; **Clear download memory** erases the list.

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

Riptide checks for a new version shortly after it starts and then every 6 hours (**Settings → Updates & About**
lets you choose 1, 6, 12 or 24 hours). When one exists you get a system notification (once per version) and a
badge on that section, where you can install it. The package is verified before it is installed, and Riptide
never installs over running downloads unless you choose **Install now**; **Install when downloads finish** waits. After an update, a
**What's new** link opens the release notes. The check can be turned off, and **Update channel** in
the same place chooses between stable and beta releases; while Riptide is in beta, beta is the default.

## Settings

Open with the sidebar's **Settings** or `Ctrl+,`. Changes apply immediately.

- **Appearance**: language (Arabic / English) and theme (follow Windows, dark, light).
- **Downloads**: the default folder, a folder per category (empty = default), connections per
  download, connections per server (a cap across all downloads to the same host), and a speed
  limit.
- **Behaviour**: notify when a download finishes, ask before closing while downloads are active,
  keep running in the tray, start with Windows, clipboard watching.
- **Integration**: browser extension, proxy settings, and the opt-in `magnet:` and `.torrent` handlers.
- **Torrents**: seeding (off by default), the share ratio and time limits, the upload limit, DHT and the incoming port.
- **AI**: provider, key, model, daily budget, privacy switches and Download memory (see above).
- **Research sources**: which sources to use, the Jamendo client id, and the web search backend (Brave or SearXNG).
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
| Database (the list, progress, torrents and download memory) | `%LOCALAPPDATA%\Riptide\riptide.sqlite` |
| API keys for AI and web search | Windows Credential Manager, never in a Riptide file |
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
- **A torrent shows 0 peers and does not start**: nobody online has the pieces, or the magnet link is wrong. Wait a little, check the link, or, if the file choice offers it, use **Download from the web seed**.
- **A torrent says it is not valid or not safe to use**: the `.torrent` file is damaged or not a torrent. Get it again from the source.
- **Research lists nothing from a source**: the chip says whether it timed out or failed; press its retry button. Public services are sometimes slow or down.
- **Going back to alpha 4**: alpha 5 upgrades the database to a new format that alpha 4 cannot open. Restore a copy of `riptide.sqlite` made before updating.
- **Still stuck**: look at today's file in `%LOCALAPPDATA%\Riptide\logs` and open a bug report
  with the relevant lines.
