<p align="center">
  <img src="assets/readme/banner-light.webp" alt="Riptide: a free, open-source download manager for Windows" width="100%">
</p>

<p align="center">
  <img alt="Platform: Windows 10 / 11" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4?style=flat-square">
  <img alt="Built with Rust, Tauri and Angular" src="https://img.shields.io/badge/built%20with-Rust%20%C2%B7%20Tauri%20%C2%B7%20Angular-dea584?style=flat-square">
  <img alt="Arabic and English" src="https://img.shields.io/badge/%D8%B9%D8%B1%D8%A8%D9%8A%20%2B%20English-RTL%20ready-071a26?style=flat-square">
  <a href="#license"><img alt="License: GPL-3.0 app, MIT or Apache-2.0 engine" src="https://img.shields.io/badge/license-GPL--3.0%20%C2%B7%20MIT%20%2F%20Apache--2.0-6b8683?style=flat-square"></a>
</p>

<h3 align="center">Download faster. Every byte verified.<br>Free, open source, no ads, no admin rights.</h3>

<p align="center">
  <a href="#download"><b>Download</b></a> ·
  <a href="https://layanjarrar1.github.io/Riptide/">Website</a> ·
  <a href="#whats-new-in-alpha-5">What's new</a> ·
  <a href="#features">Features</a> ·
  <a href="#gallery">Gallery</a> ·
  <a href="#browser-extension">Browser extension</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href="#بالعربي">بالعربي</a>
</p>

<p align="center">
  <img src="assets/readme/demo.gif" alt="Riptide downloading several files at once: segmented progress bars fill, speeds change, a completion notice appears, and the details drawer shows the segment map and speed chart" width="900">
</p>

> [!NOTE]
> Riptide is in alpha: good enough to try and to report bugs against, not finished. Keep important downloads in your current tool for now.

## What's new in alpha 5

- **BitTorrent.** Magnet links and `.torrent` files, with a file choice before anything starts, per-file progress, pause and resume, and resume after a crash. Seeding is off unless you turn it on.
- **Research.** Search open, legal sources (papers, books, music, video, games, software, images, datasets, web) and add what you tick. Results stream in per source and carry a licence badge.
- **Optional AI.** Bring your own key; it ranks results and says why. Off until you set it up, and your key stays in Windows Credential Manager.
- **Title bar with menus.** File, Downloads, Tools, View, Window and Help, usable from the keyboard and mirrored in Arabic.
- **Smoother lists.** Tens of thousands of downloads scroll without stutter.

The full list, the upgrade note and the known limits are in the [changelog](CHANGELOG.md).

## Download

| | File | Notes |
|---|---|---|
| **Installer** | [`Riptide_0.1.0-alpha.5_x64-setup.exe`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.5/Riptide_0.1.0-alpha.5_x64-setup.exe) | Per-user install, no admin prompt |
| **Portable** | [`Riptide-0.1.0-alpha.5-portable.zip`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.5/Riptide-0.1.0-alpha.5-portable.zip) | Unzip anywhere; data stays next to the app |

Windows 10 or 11, 64-bit. Checksums and older versions are on the [Releases page](https://github.com/LayanJarrar1/Riptide/releases).
Updating from alpha 4 works in place, but the database moves to a new format that alpha 4 cannot open: back up `%LOCALAPPDATA%\Riptide\riptide.sqlite` first if you may want to go back.
The alpha is not code-signed yet, so SmartScreen may warn the first time: choose **More info → Run anyway**.

When a new version upgrades your database, Riptide keeps a copy of the old one next to it (`riptide.sqlite.v3.bak`, without any saved cookies or passwords) in case you go back to an older version. It is deleted automatically after 30 days; you can delete it yourself whenever you like.

## Features

- **Fast:** up to 32 connections per file, and a connection that finishes early takes half of the slowest part left.
- **Exact:** resumes from the exact byte after a crash or a power cut, and checks MD5, SHA-1 or SHA-256 when you give one.
- **BitTorrent:** magnet links (several pasted at once) and `.torrent` files, choose files before you start, per-file progress, pause, resume, resume after a crash, "Check files again", and web seeds offered as ordinary downloads. Seeding is off by default, with ratio, time and upload limits when you turn it on. Opening `magnet:` links and `.torrent` files with Riptide is opt-in (Settings → Integration, current user only).
- **Research:** search open and public sources by category, with a licence badge on every result and an "Already downloaded" reminder. Optional AI (bring your own key from OpenAI, Anthropic, Gemini, DeepSeek, OpenRouter, DeepInfra, Mistral, Groq, Together or Fireworks) ranks results and explains why. It is off until you set it up, the key stays in Windows Credential Manager, and your download history is not shared unless you turn that on. Riptide searches open and public sources; it does not search piracy sites or torrent indexes.
- **Video:** YouTube, Vimeo and more, with a quality picker down to 144p or audio only.
- **Queues:** schedules, "only at night", and what to do when they finish: exit, sleep, hibernate, shut down or run a program.
- **Batch and site grabber:** patterns like `file[001-120].zip`, and a crawler that respects `robots.txt`.
- **Browser extension:** Chrome and Edge take downloads straight into Riptide, with a "Download this video" button.
- **Arabic and English:** a fully mirrored right-to-left interface, light and dark themes, and a title bar with menus that works from the keyboard.
- **Private:** no account, no telemetry, no ads, no administrator rights. It tells you when a new version exists (one check a day, you can turn it off) and updates in one click.

## Gallery

### New in alpha 5

<table>
  <tr>
    <td width="50%"><img src="assets/readme/v6-research-stream-en.webp" alt="Research in the light theme: a status chip for each source while results arrive, the searches the assistant made, and result cards with source and licence badges"></td>
    <td width="50%"><img src="assets/readme/v6-research-stream-ar.webp" alt="The same Research page in Arabic and the dark theme while sources are still answering, with Creative Commons licence badges"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Research streams.</b> Each source reports as it answers; you can cancel or retry one.</sub></td>
    <td align="center"><sub><b>Arabic and dark.</b> Same page, mirrored.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/v6-research-ranked-en.webp" alt="Research results ranked by the optional AI, each with a match score and one line saying why it matches"></td>
    <td width="50%"><img src="assets/readme/v6-research-ranked-ar.webp" alt="Ranked Research results in Arabic with a match percentage and the reason for each"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Ranked, with a reason.</b> Only if you set up AI.</sub></td>
    <td align="center"><sub><b>Ranked, in Arabic.</b></sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/v6-ai-privacy-en.webp" alt="Settings, AI privacy panel: sharing your download history and suggesting names are both off, and a What will be sent section"></td>
    <td width="50%"><img src="assets/readme/v6-ai-privacy-ar.webp" alt="The AI privacy panel in Arabic and the dark theme, with the history switch off"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>AI privacy.</b> Nothing is shared until you switch it on.</sub></td>
    <td align="center"><sub><b>الخصوصية.</b> متوقف افتراضيًا.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/v6-ai-key-en.webp" alt="Settings, AI assistant: provider DeepInfra and a saved API key, blurred here; the key itself is stored in Windows Credential Manager"></td>
    <td width="50%"><img src="assets/readme/v6-ai-key-ar.webp" alt="The AI assistant settings in Arabic and the dark theme with the saved key blurred"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Your key, masked.</b> Kept in Windows Credential Manager, never shown again.</sub></td>
    <td align="center"><sub><b>المفتاح مخفي.</b></sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/v6-torrent-files-en.webp" alt="Add torrent dialog listing three files with sizes; two are ticked and one is not"></td>
    <td width="50%"><img src="assets/readme/v6-torrent-files-ar.webp" alt="The Add torrent dialog in Arabic with a note that the torrent is already in the list"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Choose files first.</b> Untick what you do not want.</sub></td>
    <td align="center"><sub><b>اختر الملفات.</b> وينبّهك إن كان التورنت في قائمتك.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/v6-torrent-details-en.webp" alt="Details drawer of a completed torrent with a progress bar for each file and one file marked not selected"></td>
    <td width="50%"><img src="assets/readme/v6-torrent-details-ar.webp" alt="The torrent details drawer in Arabic and the dark theme with per-file progress"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Per-file progress.</b> A file you skipped stays skipped.</sub></td>
    <td align="center"><sub><b>تقدّم كل ملف.</b></sub></td>
  </tr>
  <tr>
    <td><img src="assets/readme/v6-add-magnet-en.webp" alt="Add download dialog with a magnet link pasted: Riptide says it is a torrent link and offers Choose files"></td>
    <td><img src="assets/readme/v6-torrent-list-en.webp" alt="The downloads list with a 3.8 GB torrent at 44 percent, its speed, time left and the number of connected peers"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Paste a magnet link.</b> Several at once also work.</sub></td>
    <td align="center"><sub><b>Downloading.</b> A real 3.8 GB torrent, with peers.</sub></td>
  </tr>
  <tr>
    <td><img src="assets/readme/v6-menu-file-en.webp" alt="The integrated title bar with the File menu open: Add download, Batch download, Site grabber, Import link list and Open torrent file"></td>
    <td><img src="assets/readme/v6-menu-file-ar.webp" alt="The File menu in Arabic, mirrored, with the window controls kept at the right"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Menus in the title bar.</b> Alt or F10 opens them from the keyboard.</sub></td>
    <td align="center"><sub><b>القوائم.</b> تنعكس في العربية.</sub></td>
  </tr>
</table>

<sub>Screenshots are from real runs of the alpha 5 build; the save location and the end of the saved key are blurred. Not shown: the Arabic Add dialog with a magnet link pasted.</sub>

### Everything else

<table>
  <tr>
    <td width="50%"><img src="assets/readme/v4-list-en.webp" alt="The Riptide downloads list in the light theme: segmented progress bars, speed, time left and status"></td>
    <td width="50%"><img src="assets/readme/v4-list-ar-dark.webp" alt="The same list in Arabic and the dark theme, mirrored right to left"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Light.</b> Every stripe in a progress bar is one connection.</sub></td>
    <td align="center"><sub><b>Arabic and dark.</b> The whole layout mirrors.</sub></td>
  </tr>
  <tr>
    <td><img src="assets/readme/v5-details.webp" alt="Details drawer with the segment map and a live speed chart"></td>
    <td><img src="assets/readme/v4-video.webp" alt="Add dialog showing a video page with a quality picker from Best down to audio only"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Details.</b> Segment map and speed chart.</sub></td>
    <td align="center"><sub><b>Video.</b> Pick a quality, including audio only.</sub></td>
  </tr>
  <tr>
    <td><img src="assets/readme/v4-batch.webp" alt="Batch dialog previewing the forty addresses a pattern makes"></td>
    <td><img src="assets/readme/v5-queue.webp" alt="New queue dialog with a schedule, days of the week and an action for when the queue finishes"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Batch.</b> A pattern becomes a checked list of links.</sub></td>
    <td align="center"><sub><b>Queues.</b> Schedule them, then choose what happens next.</sub></td>
  </tr>
  <tr>
    <td><img src="assets/readme/v4-grabber.webp" alt="Site grabber results grouped by file type with check boxes"></td>
    <td><img src="assets/readme/v5-ext-video.webp" alt="The Download this video button and quality popover on a video page"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Site grabber.</b> Files found on a site, by type.</sub></td>
    <td align="center"><sub><b>In the browser.</b> One click on a video page.</sub></td>
  </tr>
</table>

## Browser extension

Chrome and Edge load the extension from the folder Riptide ships, so there is nothing else to download:

1. In Riptide open **Settings → Integration** and press **Open extension folder**.
2. In the browser open `chrome://extensions` (`edge://extensions` in Edge) and turn on **Developer mode**.
3. Press **Load unpacked** and pick that folder.

It takes downloads automatically (with rules for size, file types and sites; hold **Alt** to skip), adds **Download with Riptide** to the right-click menu, and can grab all links or all images on a page. If Riptide is closed it starts it, and if anything fails the browser downloads the file itself. Firefox and the permissions are covered in [`apps/extension/README.md`](apps/extension/README.md).

<p align="center">
  <img src="assets/readme/v5-integration.webp" alt="Settings, Integration: the three install steps and Bridge ready for Chrome and Edge" width="900">
</p>

## Build from source

You need stable Rust, Node.js 20+ and Visual Studio Build Tools with the C++ workload. No admin rights are needed.

```powershell
git clone https://github.com/LayanJarrar1/Riptide.git
cd Riptide\apps\desktop
npm install
node scripts\fetch-media-tools.mjs   # video support: pinned yt-dlp and FFmpeg, SHA-256 checked
npm run tauri dev

# installer and portable zip
npm run bundle
npm run bundle:portable

# the quality gate (from the repository root)
.\scripts\check.ps1
```

More: [User guide](guide/en.md) · [Architecture](ARCHITECTURE.md) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Changelog](CHANGELOG.md).

## بالعربي

<div dir="rtl">

**ريبتايد** مدير تحميل مجاني ومفتوح المصدر لويندوز، والبديل المفتوح لبرنامج IDM. يقسم كل ملف على اتصالات متعددة، ويكمل من البايت الذي توقف عنده بالضبط، ويصمد أمام انطفاء الجهاز، ويلتقط التحميلات من المتصفح، ويحمّل الفيديو، ويشغّل الطوابير حسب الجدول، ويدعم التورنت وروابط المغناطيس، ويبحث في مصادر مفتوحة، وواجهته عربية معكوسة بالكامل.

- **مجاني بالكامل:** بدون إعلانات، بدون حساب، بدون تتبّع، وبدون صلاحيات مسؤول.
- **دقيق حتى آخر بايت:** لا يعتبر التحميل مكتملًا إلا إذا وصلت كل القطع، وطابق الملف بصمته إن أعطيته واحدة.
- **جديد في ألفا 5:**
  - **التورنت:** روابط المغناطيس (عدة روابط معًا) وملفات `.torrent`، مع اختيار الملفات قبل البدء، وتقدّم كل ملف، والإيقاف المؤقت والاستئناف، والاستئناف بعد انقطاع مفاجئ. المشاركة (البذر) متوقفة افتراضيًا. فتح روابط `magnet:` وملفات `.torrent` بريبتايد اختياري (الإعدادات ← التكامل، للمستخدم الحالي فقط).
  - **البحث:** مصادر مفتوحة وقانونية حسب الفئة (أوراق، كتب، موسيقى، فيديو، ألعاب، برامج، صور، بيانات، ويب) وشارة ترخيص على كل نتيجة، والنتائج تصل مصدرًا بعد مصدر. ريبتايد يبحث في مصادر مفتوحة وعامة ولا يبحث في مواقع القرصنة ولا في فهارس التورنت.
  - **الذكاء الاصطناعي (اختياري):** تستخدم مفتاحك الخاص (OpenAI وAnthropic وGemini وDeepSeek وOpenRouter وDeepInfra وMistral وGroq وTogether وFireworks) ليرتّب النتائج ويشرح السبب. متوقف حتى تُعدّه، والمفتاح يبقى في مدير بيانات اعتماد ويندوز، وسجل تحميلاتك لا يُشارك إلا إذا فعّلت ذلك.
  - **شريط عنوان بقوائم:** ملف، التنزيلات، أدوات، عرض، نافذة، مساعدة، تعمل بلوحة المفاتيح وتنعكس في العربية.
- **محدَّث دائمًا:** يُشعرك حين يتوفر إصدار جديد، وبنقرة واحدة يثبّته (فحص واحد في اليوم، ويمكن إيقافه).

**التحميل:**
[المثبّت](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.5/Riptide_0.1.0-alpha.5_x64-setup.exe) ·
[النسخة المحمولة](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.5/Riptide-0.1.0-alpha.5-portable.zip) ·
[دليل الاستخدام](guide/ar.md)

**تثبيت الإضافة في كروم أو إيدج:** ١) في ريبتايد: **الإعدادات ← التكامل ← فتح مجلد الإضافة**. ٢) في المتصفح افتح `chrome://extensions` وفعّل **وضع المطوّر**. ٣) اضغط **تحميل إضافة غير مضغوطة** واختر المجلد.

> هذه نسخة تجريبية (Alpha) للتجربة والإبلاغ عن الأخطاء، وليست نهائية بعد.
>
> **ملاحظة للترقية من ألفا 4:** التحديث يتم في مكانه، لكن قاعدة البيانات تتحول إلى صيغة جديدة لا يستطيع ألفا 4 فتحها. انسخ `%LOCALAPPDATA%\Riptide\riptide.sqlite` احتياطيًا إن كنت قد تعود للإصدار السابق.

</div>

<table dir="rtl">
  <tr>
    <td width="50%"><img src="assets/readme/v6-research-stream-ar.webp" alt="صفحة البحث بالعربية في الوضع الداكن: حالة كل مصدر وشارات التراخيص"></td>
    <td width="50%"><img src="assets/readme/v6-torrent-files-ar.webp" alt="نافذة إضافة تورنت بالعربية: اختيار الملفات قبل البدء"></td>
  </tr>
  <tr>
    <td align="center"><sub>البحث: النتائج تصل مصدرًا بعد مصدر</sub></td>
    <td align="center"><sub>التورنت: اختر الملفات قبل أن يبدأ التحميل</sub></td>
  </tr>
</table>

<p align="center">
  <img src="assets/readme/v5-integration-ar.webp" alt="الإعدادات ← التكامل بالعربية: خطوات تثبيت الإضافة وحالة «الجسر جاهز»" width="900">
</p>

## Contributing

Bug reports with steps to reproduce, fixes with tests and translation review are the most useful contributions right now. Start with [CONTRIBUTING.md](CONTRIBUTING.md). Security issues go through [SECURITY.md](SECURITY.md), not the public tracker.

## License

The desktop app, CLI, IPC crate, native host and browser extension are licensed under **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). The engine crates (`riptide-core`, `riptide-net`, `riptide-store`, `riptide-engine`, `riptide-media`, `riptide-crawl`, `riptide-log`, `riptide-testserver`) are dual-licensed **MIT OR Apache-2.0** ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)).

Built on [Tauri](https://tauri.app), [Angular](https://angular.dev), [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest) and [rusqlite](https://github.com/rusqlite/rusqlite). Ships [IBM Plex Sans Arabic](https://github.com/IBM/plex) and [Inter](https://rsms.me/inter/) under the SIL Open Font License. Video support bundles [yt-dlp](https://github.com/yt-dlp/yt-dlp) (Unlicense) and an [FFmpeg](https://ffmpeg.org) LGPL build as separate programs; their licenses are in the `licenses` folder next to the app. Torrent support uses [librqbit](https://github.com/ikatson/rqbit) (Apache-2.0). Only download what you have the right to download.
