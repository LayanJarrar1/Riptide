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

## Download

| | File | Notes |
|---|---|---|
| **Installer** | [`Riptide_0.1.0-alpha.4_x64-setup.exe`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide_0.1.0-alpha.4_x64-setup.exe) | Per-user install, no admin prompt |
| **Portable** | [`Riptide-0.1.0-alpha.4-portable.zip`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide-0.1.0-alpha.4-portable.zip) | Unzip anywhere; data stays next to the app |

Windows 10 or 11, 64-bit. Checksums and older versions are on the [Releases page](https://github.com/LayanJarrar1/Riptide/releases).
The alpha is not code-signed yet, so SmartScreen may warn the first time: choose **More info → Run anyway**.

## Features

- **Fast:** up to 32 connections per file, and a connection that finishes early takes half of the slowest part left.
- **Exact:** resumes from the exact byte after a crash or a power cut, and checks MD5, SHA-1 or SHA-256 when you give one.
- **Video:** YouTube, Vimeo and more, with a quality picker down to 144p or audio only.
- **Queues:** schedules, "only at night", and what to do when they finish: exit, sleep, hibernate, shut down or run a program.
- **Batch and site grabber:** patterns like `file[001-120].zip`, and a crawler that respects `robots.txt`.
- **Browser extension:** Chrome and Edge take downloads straight into Riptide, with a "Download this video" button.
- **Arabic and English:** a fully mirrored right-to-left interface, light and dark themes.
- **Private:** no account, no telemetry, no ads, no administrator rights. It tells you when a new version exists (one check a day, you can turn it off) and updates in one click.

## Gallery

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

**ريبتايد** مدير تحميل مجاني ومفتوح المصدر لويندوز، والبديل المفتوح لبرنامج IDM. يقسم كل ملف على اتصالات متعددة، ويكمل من البايت الذي توقف عنده بالضبط، ويصمد أمام انطفاء الجهاز، ويلتقط التحميلات من المتصفح، ويحمّل الفيديو، ويشغّل الطوابير حسب الجدول، وواجهته عربية معكوسة بالكامل.

- **مجاني بالكامل:** بدون إعلانات، بدون حساب، بدون تتبّع، وبدون صلاحيات مسؤول.
- **دقيق حتى آخر بايت:** لا يعتبر التحميل مكتملًا إلا إذا وصلت كل القطع، وطابق الملف بصمته إن أعطيته واحدة.
- **محدَّث دائمًا:** يُشعرك حين يتوفر إصدار جديد، وبنقرة واحدة يثبّته (فحص واحد في اليوم، ويمكن إيقافه).

**التحميل:**
[المثبّت](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide_0.1.0-alpha.4_x64-setup.exe) ·
[النسخة المحمولة](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.4/Riptide-0.1.0-alpha.4-portable.zip) ·
[دليل الاستخدام](guide/ar.md)

**تثبيت الإضافة في كروم أو إيدج:** ١) في ريبتايد: **الإعدادات ← التكامل ← فتح مجلد الإضافة**. ٢) في المتصفح افتح `chrome://extensions` وفعّل **وضع المطوّر**. ٣) اضغط **تحميل إضافة غير مضغوطة** واختر المجلد.

> هذه نسخة تجريبية (Alpha) للتجربة والإبلاغ عن الأخطاء، وليست نهائية بعد.

</div>

<p align="center">
  <img src="assets/readme/v5-integration-ar.webp" alt="الإعدادات ← التكامل بالعربية: خطوات تثبيت الإضافة وحالة «الجسر جاهز»" width="900">
</p>

## Contributing

Bug reports with steps to reproduce, fixes with tests and translation review are the most useful contributions right now. Start with [CONTRIBUTING.md](CONTRIBUTING.md). Security issues go through [SECURITY.md](SECURITY.md), not the public tracker.

## License

The desktop app, CLI, IPC crate, native host and browser extension are licensed under **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). The engine crates (`riptide-core`, `riptide-net`, `riptide-store`, `riptide-engine`, `riptide-media`, `riptide-crawl`, `riptide-log`, `riptide-testserver`) are dual-licensed **MIT OR Apache-2.0** ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)).

Built on [Tauri](https://tauri.app), [Angular](https://angular.dev), [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest) and [rusqlite](https://github.com/rusqlite/rusqlite). Ships [IBM Plex Sans Arabic](https://github.com/IBM/plex) and [Inter](https://rsms.me/inter/) under the SIL Open Font License. Video support bundles [yt-dlp](https://github.com/yt-dlp/yt-dlp) (Unlicense) and an [FFmpeg](https://ffmpeg.org) LGPL build as separate programs; their licenses are in the `licenses` folder next to the app. Only download what you have the right to download.
