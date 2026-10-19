<p align="center">
  <img src="assets/brand/riptide-readme-banner.svg" alt="Riptide: a free, open-source download manager for Windows" width="100%">
</p>

<p align="center">
  <a href="https://github.com/LayanJarrar1/Riptide/releases"><img alt="Latest pre-release" src="https://img.shields.io/github/v/release/LayanJarrar1/Riptide?include_prereleases&style=flat-square&label=pre-release&color=00d5e8"></a>
  <a href="https://github.com/LayanJarrar1/Riptide/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/LayanJarrar1/Riptide/total?style=flat-square&color=075c69"></a>
  <a href="https://github.com/LayanJarrar1/Riptide/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/LayanJarrar1/Riptide?style=flat-square&color=168bff"></a>
  <img alt="Platform: Windows 10 / 11" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4?style=flat-square">
  <img alt="Built with Rust, Tauri and Angular" src="https://img.shields.io/badge/built%20with-Rust%20%C2%B7%20Tauri%20%C2%B7%20Angular-dea584?style=flat-square">
  <img alt="Arabic and English" src="https://img.shields.io/badge/%D8%B9%D8%B1%D8%A8%D9%8A%20%2B%20English-RTL%20ready-071a26?style=flat-square">
  <a href="#license"><img alt="License: GPL-3.0 app, MIT or Apache-2.0 engine" src="https://img.shields.io/badge/license-GPL--3.0%20%C2%B7%20MIT%20%2F%20Apache--2.0-6b8683?style=flat-square"></a>
</p>

<h3 align="center">Download faster. Every byte, verified.<br>Free, open source, no ads, no admin rights.</h3>

<p align="center">
  <a href="#download"><b>Download</b></a> ·
  <a href="#whats-new-in-alpha-4">What's new</a> ·
  <a href="#browser-extension">Browser extension</a> ·
  <a href="#features">Features</a> ·
  <a href="#riptide-vs-idm">Riptide vs IDM</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href="#بالعربي">بالعربي</a>
</p>

> [!NOTE]
> **Riptide is in alpha.** It is solid enough to try and to report bugs against, but it is not finished.
> Keep important downloads in your current tool for now, and expect changes between releases.

---

Riptide is a **free, open-source download manager for Windows**: the open alternative to Internet
Download Manager. It splits every file across many connections, resumes from the exact byte after a crash or
a power cut, pulls video from video sites, runs queues on a schedule, takes downloads straight from your
browser, and speaks **Arabic and English** with a fully mirrored right-to-left interface.

No license fee. No ads, nag screens or trial timers. No account. No telemetry. No administrator rights.

<p align="center">
  <img src="assets/readme/v4-list-en.webp" alt="The Riptide downloads list: segmented progress bars, speed, time left and status for every download" width="100%">
</p>
<p align="center"><sub>Every stripe in a progress bar is one connection. Red stripes mark a file that failed its checksum.</sub></p>

## What's new in alpha 4

| | |
|---|---|
| **Video pages** | Paste a YouTube or Vimeo link (or any site yt-dlp supports) and pick a quality, down to audio-only M4A or MP3. Pause and resume work like any other download. |
| **Queues and a scheduler** | Make as many queues as you like, each with its own limit. Run one only at night, on certain days, or at a set time, and choose what happens when it finishes: exit, sleep, hibernate, shut down or run a program, with a 30 second countdown to cancel. |
| **Batch downloads** | Type `file[001-120].zip` or `img_[a-z].jpg` and see every address before you add them. Import or export a plain list of links. |
| **Site grabber** | Point it at a page, choose how deep to go and which file types you want, and tick what to download. It respects `robots.txt` and is polite to servers. |
| **"Download this video" in the browser** | A small button appears on video pages. Click it, pick a quality, and Riptide does the rest. Right-click any page to download all links, selected links or all images, with a checklist. |
| **Update notifications** | A system notification when a new version exists (once per version), a badge in Settings, and a "What's new" note after you update. Turn it off any time. |
| **Faster start** | The interface loads about 130 kB less before the first paint. |
| **New look** | The Riptide brand: new logo, icons and website. |

<table>
  <tr>
    <td width="50%"><img src="assets/readme/v4-video.webp" alt="Add dialog showing a video page with a quality picker from Best down to audio-only"></td>
    <td width="50%"><img src="assets/readme/v4-batch.webp" alt="Batch dialog previewing the forty addresses a pattern makes"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Video:</b> pick a quality, including audio-only.</sub></td>
    <td align="center"><sub><b>Batch:</b> a pattern becomes a checked list of links.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/v4-queue.webp" alt="New queue dialog with a schedule, days of the week and an action for when the queue finishes"></td>
    <td width="50%"><img src="assets/readme/v4-grabber.webp" alt="Site grabber results grouped by file type with check boxes"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Queues:</b> schedule them, then choose what happens next.</sub></td>
    <td align="center"><sub><b>Site grabber:</b> files found on a site, grouped by type.</sub></td>
  </tr>
</table>

<sub>Video support uses [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [FFmpeg](https://ffmpeg.org), shipped as separate programs
next to Riptide. Websites change often, so *Settings → About → Update video support* fetches the newest yt-dlp when you ask.
Live streams, playlists and DRM-protected videos are refused. Only download what you have the right to download.</sub>

## Download

| | File | Notes |
|---|---|---|
| **Installer** | [`Riptide_0.1.0-alpha.4_x64-setup.exe`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide_0.1.0-alpha.4_x64-setup.exe) | Per-user install, no admin prompt |
| **Portable** | [`Riptide-0.1.0-alpha.4-portable.zip`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide-0.1.0-alpha.4-portable.zip) | Unzip anywhere; data stays next to the app |

**Requirements:** Windows 10 or 11, 64-bit. WebView2 is already part of Windows.
All builds and their SHA-256 checksums are on the [Releases page](https://github.com/LayanJarrar1/Riptide/releases).

> [!TIP]
> The alpha is not code-signed yet, so Windows SmartScreen may warn the first time. Choose **More info** →
> **Run anyway**. Every file's SHA-256 is listed in `SHA256SUMS.txt` on the release.

## Browser extension

The extension takes downloads from **Chrome, Edge and Firefox** and hands them to Riptide, with the page's
cookies and referrer, so links that need a login keep working. If Riptide is closed, the extension starts it.
If anything goes wrong, the browser simply downloads the file itself; nothing is lost.

- **Automatic capture**, with rules: minimum size, file types, sites to always or never use, hold **Alt** to skip.
- **Right-click** *Download with Riptide* on any link, image, video or audio, or grab **all links**, **selected links** or **all images** on a page.
- **A video button** on video pages with a quality picker, and a small button on direct file links.

### Install it in Chrome (3 steps)

The extension is not in the Chrome Web Store yet, so Chrome needs it loaded once by hand. It takes under a minute.

1. Install Riptide (above) and start it once. It registers the browser bridge for you, without admin rights.
2. In Riptide open **Settings → Integration** and choose **Open extension folder**. (The folder is `%LOCALAPPDATA%\Riptide\extension\chrome`; from the portable zip it is the `extension\chrome` folder next to the app.)
3. In Chrome open `chrome://extensions`, turn on **Developer mode**, click **Load unpacked**, and pick that folder.

Edge works the same way (`edge://extensions`, folder `extension\edge`). Firefox: `about:debugging` → *This Firefox* →
*Load Temporary Add-on* → `extension\firefox\manifest.json`. Building it yourself: `cd apps/extension && npm ci && npm run build`.
Permissions, privacy and how it talks to the app: [`apps/extension/README.md`](apps/extension/README.md).

## Screenshots

<p align="center">
  <img src="assets/readme/v4-list-ar-dark.webp" alt="The same list in Arabic and the dark theme, mirrored right to left" width="100%">
</p>
<p align="center"><sub>Arabic and the dark theme. The whole layout mirrors, instantly, and you can switch language while downloads run.</sub></p>

## Features

**Speed**
- Multi-connection downloads (1 to 32) with **work stealing**: when a connection finishes early it takes half
  of the slowest remaining part, so the link stays saturated to the last byte.
- A per-server connection cap so hosts don't block you. If a server answers *429* or *503*, Riptide uses fewer
  connections on that host from then on, and retries with back-off and `Retry-After`.
- A speed limit for everything or for a single download, and proxy support.

**Reliability**
- **Byte-exact resume.** Pause any time; resume continues from the exact byte, and only if the server still
  serves the same file. If it changed, Riptide starts over instead of mixing two versions.
- **Crash-proof.** Progress is saved to a local database every second. Kill the app or lose power, and every
  download continues on the next start.
- **Checksum verification** (MD5, SHA-1, SHA-256), and *Restart from the beginning* when you need a clean copy.

**Organised**
- Queues with schedules and finish actions, categories by file type with their own folders, duplicate detection
  before adding, and real free-space checks with a warning when a file won't fit.
- Search, sort, filter, multi-select, a details drawer, and full keyboard control (press `?` for shortcuts).
- Close to the tray, start with Windows, completion notifications, and a clipboard watcher if you want one.

**Safe and private**
- File names from servers are sanitised and can never escape your folder. Redirects are capped. Cookies and
  passwords are only sent to the site they belong to, never logged, and deleted when the download ends.
- The browser can only ask: the extension talks to a small local helper that accepts a short list of message
  types and checks who is calling. Nothing from a web page is ever executed.
- No admin rights, no account, no telemetry. The only network traffic is your downloads and, if you leave it on,
  one update check against GitHub Releases.

**Yours**
- Arabic and English everywhere, auto-detected and switchable live, with a mirrored RTL layout and
  correct Arabic file names. Light and dark themes. Respects reduced-motion settings.
- A portable mode for USB drives, and a command-line tool for scripts: `riptide fetch`, `riptide batch`
  and `riptide grab`.

## Riptide vs IDM

| | **Riptide** | IDM |
|---|---|---|
| Price | **Free, forever** | Paid license per PC |
| Source code | **Open** (GPL-3.0 app, MIT / Apache-2.0 engine) | Closed |
| Ads, trial reminders, activation | **None** | Trial reminders, serial activation |
| Administrator rights | **Never needed** | Standard installer |
| Arabic interface | **Fully mirrored RTL** | Translated strings |
| Multi-connection downloads | Yes, with work stealing | Yes |
| Queues and scheduler | Yes | Yes |
| Batch downloads and site grabber | Yes | Yes |
| Browser integration | Chrome, Edge, Firefox | Chrome, Edge, Firefox and more |
| Resume after crash or power loss | Database checkpoints every second | Resume support |
| Built-in checksum verification | **Yes** (MD5, SHA-1, SHA-256) | No |
| Maturity | Alpha | Shipping for many years |

<sub>IDM is a trademark of Tonec Inc. The comparison reflects publicly documented behaviour at the time of
writing. If a row is wrong, please open an issue.</sub>

## How it works

1. **Probe.** Riptide asks the server for the file's size, final address, name, and whether it supports
   partial downloads. For a video page it asks yt-dlp for the title and the qualities on offer.
2. **Split.** The file is cut into equal parts, one per connection, and space for the whole file is reserved on
   disk as `name.rtpart`.
3. **Pull.** Each connection fetches its part and writes it straight into place. Idle connections take work
   from slow ones.
4. **Checkpoint.** Every second, each part's progress is written to a local SQLite database in one transaction.
5. **Verify and finish.** The size and optional checksum are checked, then the file gets its final name in one
   atomic step. There is no "assembling" wait at the end.

The browser extension talks to a tiny local helper (`riptide-host`) that forwards only a short list of safe
requests to the app over a private, per-user channel. The full picture is in
[ARCHITECTURE.md](ARCHITECTURE.md).

### Measured

On a Windows Server 2022 machine, release build, 32 downloads in the list: the window is ready in **864 ms**
(average of three cold starts), idle CPU is **0 %**, and the app process uses **31 MB** (plus WebView2). A batch of
120 files from the local test server finishes with every SHA-256 matching its source. These are
development-machine numbers, not promises for yours; `riptide bench` measures your own.

## Documentation

| | |
|---|---|
| [User guide (English)](guide/en.md) · [دليل الاستخدام](guide/ar.md) | Adding downloads, details, settings, shortcuts, portable mode, troubleshooting |
| [Browser extension](apps/extension/README.md) | Install, permissions, privacy, how it talks to the app |
| [Architecture](ARCHITECTURE.md) | Crates, life of a download, guarantees, testing |
| [Contributing](CONTRIBUTING.md) | Setup, the quality gate, conventions, pull requests |
| [Security policy](SECURITY.md) | Private reporting and what Riptide guards against |
| [Changelog](CHANGELOG.md) | What changed |

## Build from source

You need stable Rust (`rustup`), Node.js 20+, and Visual Studio Build Tools with the C++ workload. No admin
rights are needed for any of it.

```powershell
git clone https://github.com/LayanJarrar1/Riptide.git
cd Riptide

# run the desktop app in development mode
cd apps\desktop
npm install
node scripts\fetch-media-tools.mjs   # video support: pinned yt-dlp and FFmpeg, SHA-256 checked
npm run tauri dev

# build the installer and the portable zip
npm run bundle
npm run bundle:portable

# the quality gate: lint, build and every test (run from the repository root)
.\scripts\check.ps1
```

Engine tests run against a local test server that fakes every way a download can go wrong (dropped
connections, flaky servers, files that change mid-download, wrong lengths, expired links, hostile file names,
a small crawlable site), and every resume test compares the SHA-256 of the result with the source. More in
[CONTRIBUTING.md](CONTRIBUTING.md).

<details>
<summary><b>Repository layout</b></summary>

```
crates/
  riptide-core/        download model, state machine, segment planner, queues and schedules, batch patterns
  riptide-net/         HTTP engine: probe, ranges, multi-connection transfer, retry, auth, redirects
  riptide-store/       SQLite persistence and migrations
  riptide-engine/      runtime: queues, scheduler, crash recovery, events, disk space
  riptide-media/       video pages through yt-dlp: probe, quality menu, progress, resume
  riptide-crawl/       site grabber: depth, scope, robots.txt, filters
  riptide-ipc/         private per-user pipe and message schema
  riptide-host/        browser Native Messaging host
  riptide-log/         logging with secret redaction, installed and portable paths
  riptide-cli/         command-line client (riptide fetch / batch / grab / bench)
  riptide-testserver/  local server with every failure mode the tests need
apps/
  desktop/             Tauri v2 shell (src-tauri) and the Angular interface (src)
  extension/           Manifest V3 extension for Chrome, Edge and Firefox
  website/             the product website (static, bilingual)
guide/                 user guide in English and Arabic
scripts/check.ps1      the quality gate
```

</details>

## بالعربي

**ريبتايد** مدير تحميل مجاني ومفتوح المصدر لويندوز، والبديل المفتوح لبرنامج IDM. يقسم كل ملف على اتصالات
متعددة، ويكمل من البايت الذي توقف عنده بالضبط، ويصمد أمام انطفاء الجهاز أو تعطّل البرنامج، ويلتقط التحميلات
مباشرة من المتصفح عبر إضافة لكروم وإيدج وفايرفوكس.

- **مجاني بالكامل:** بدون رخصة، بدون إعلانات، بدون حساب، بدون تتبّع، وبدون صلاحيات مسؤول.
- **عربي من أول يوم:** واجهة معكوسة بالكامل من اليمين لليسار، وتبديل فوري بين العربية والإنجليزية، وأسماء الملفات
  العربية تصل إلى القرص صحيحة.
- **دقيق حتى آخر بايت:** لا يعتبر التحميل مكتملًا إلا إذا وصلت كل القطع، وطابق الملف بصمته إن أعطيته واحدة.

**الجديد في النسخة 4:** تحميل الفيديو من يوتيوب وفيميو وغيرهما من المواقع التي يدعمها yt-dlp مع اختيار الجودة (حتى الصوت فقط)، وطوابير بجداول
زمنية مع خيار «عند الانتهاء» (إغلاق أو سكون أو إيقاف التشغيل) بعدّ تنازلي لثلاثين ثانية، وتحميل دفعات بنمط مثل
`file[001-120].zip`، وجامع الملفات الذي يمرّ على موقع كامل ويحترم `robots.txt`، وزرّ «حمّل هذا الفيديو» في المتصفح،
وإشعارات بالتحديثات الجديدة.

**التحميل:** [المثبّت](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide_0.1.0-alpha.4_x64-setup.exe) ·
[النسخة المحمولة](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide-0.1.0-alpha.4-portable.zip) ·
[دليل الاستخدام بالعربية](guide/ar.md)

**تثبيت الإضافة في كروم:** ١) ثبّت ريبتايد وشغّله مرة واحدة. ٢) من **الإعدادات ← التكامل** اختر **فتح مجلد الإضافة**.
٣) في كروم افتح `chrome://extensions` وفعّل **وضع المطوّر** ثم **تحميل إضافة غير مضغوطة** واختر المجلد. الإضافة
ليست في متجر كروم بعد، ولذلك تحتاج هذه الخطوة مرة واحدة فقط.

> **ملاحظة:** هذه نسخة تجريبية (Alpha) للتجربة والإبلاغ عن الأخطاء، وليست نهائية بعد.

## Contributing

Bug reports with steps to reproduce, fixes with tests, and translation review are the most useful
contributions right now. Start with [CONTRIBUTING.md](CONTRIBUTING.md). Security issues go through
[SECURITY.md](SECURITY.md), not the public tracker.

## License

The desktop app, CLI, IPC crate, native host and browser extension are licensed under
**GPL-3.0-or-later** ([`LICENSE`](LICENSE)). The engine crates (`riptide-core`, `riptide-net`,
`riptide-store`, `riptide-engine`, `riptide-log`, `riptide-testserver`) are dual-licensed
**MIT OR Apache-2.0** ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)).

Built on [Tauri](https://tauri.app), [Angular](https://angular.dev), [tokio](https://tokio.rs),
[reqwest](https://github.com/seanmonstar/reqwest) and [rusqlite](https://github.com/rusqlite/rusqlite).
Ships [IBM Plex Sans Arabic](https://github.com/IBM/plex) and [Inter](https://rsms.me/inter/) under the SIL
Open Font License. Video support bundles [yt-dlp](https://github.com/yt-dlp/yt-dlp) (Unlicense) and an
[FFmpeg](https://ffmpeg.org) LGPL build as separate programs; their licenses are in the `licenses` folder next to the app.
