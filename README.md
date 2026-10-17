<p align="center">
  <img src="assets/readme/banner.png" alt="Riptide: a free, open-source download manager for Windows" width="100%">
</p>

<p align="center">
  <a href="https://github.com/LayanJarrar1/Riptide/releases"><img alt="Latest pre-release" src="https://img.shields.io/github/v/release/LayanJarrar1/Riptide?include_prereleases&style=flat-square&label=pre-release&color=19c6b5"></a>
  <a href="https://github.com/LayanJarrar1/Riptide/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/LayanJarrar1/Riptide/total?style=flat-square&color=0b1c1a"></a>
  <a href="https://github.com/LayanJarrar1/Riptide/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/LayanJarrar1/Riptide?style=flat-square&color=19c6b5"></a>
  <img alt="Platform: Windows 10 / 11" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4?style=flat-square">
  <img alt="Built with Rust, Tauri and Angular" src="https://img.shields.io/badge/built%20with-Rust%20%C2%B7%20Tauri%20%C2%B7%20Angular-dea584?style=flat-square">
  <img alt="Arabic and English" src="https://img.shields.io/badge/%D8%B9%D8%B1%D8%A8%D9%8A%20%2B%20English-RTL%20ready-0b1c1a?style=flat-square">
  <a href="#license"><img alt="License: GPL-3.0 app, MIT or Apache-2.0 engine" src="https://img.shields.io/badge/license-GPL--3.0%20%C2%B7%20MIT%20%2F%20Apache--2.0-6b8683?style=flat-square"></a>
</p>

<h3 align="center">The download manager that pulls harder. Free, open source, no ads, no admin rights.</h3>

<p align="center">
  <a href="#download"><b>Download</b></a> ·
  <a href="#browser-extension">Browser extension</a> ·
  <a href="#features">Features</a> ·
  <a href="#riptide-vs-idm">Riptide vs IDM</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#documentation">Docs</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href="#بالعربي">بالعربي</a>
</p>

> [!NOTE]
> **Riptide is in alpha.** It works well enough to try and to report bugs, but it is not finished. Keep
> important downloads in your current tool for now, and expect changes between releases.

---

Riptide is a **free, open-source download manager for Windows**: the open alternative to Internet
Download Manager. It splits every file across many connections, resumes exactly where it stopped, survives
crashes and power cuts, catches downloads straight from your browser, and speaks **Arabic and English**
with a fully mirrored right-to-left interface.

No license fee. No ads, nag screens or trial timers. No account. No telemetry. No administrator rights.

## Download

| | File | Notes |
|---|---|---|
| **Installer** | [`Riptide_0.1.0-alpha.2_x64-setup.exe`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.2/Riptide_0.1.0-alpha.2_x64-setup.exe) | Per-user install, no admin prompt |
| **Portable** | [`riptide-0.1.0-alpha.2-windows-x64-portable.zip`](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.2/riptide-0.1.0-alpha.2-windows-x64-portable.zip) | Unzip anywhere; data stays next to the app |

**Requirements:** Windows 10 or 11, 64-bit. WebView2 is already part of Windows.
All builds and checksums are on the [Releases page](https://github.com/LayanJarrar1/Riptide/releases).

> [!TIP]
> The alpha is not code-signed yet, so Windows SmartScreen may warn the first time. Choose **More info** →
> **Run anyway**. Every file's SHA-256 is listed in its release notes.

## Browser extension

The extension catches downloads in **Chrome, Edge and Firefox** and hands them to Riptide, with the page's
cookies and referrer, so links that need a login keep working. If Riptide is closed, the extension starts it.
If anything goes wrong, the browser simply downloads the file itself; nothing is lost.

- **Automatic capture**, with rules: minimum size, file types, sites to always or never use, hold **Alt** to skip.
- **Right-click** *Download with Riptide* on any link, image, video or audio, or grab *all links* on a page.
- **A small Riptide button** on direct file links, and a toolbar popup with the latest downloads.

**Install (store listings are coming):**

1. Install Riptide (above) and start it once. It registers itself with your browsers, without admin rights.
2. Build the extension: `cd apps/extension && npm ci && npm run build`.
3. Chrome / Edge: open `chrome://extensions` (or `edge://extensions`), turn on **Developer mode**, click
   **Load unpacked** and pick `apps/extension/dist/chrome` (or `dist/edge`).
   Firefox: `about:debugging` → *This Firefox* → *Load Temporary Add-on* → `apps/extension/dist/firefox/manifest.json`.

Details, permissions and privacy: [`apps/extension/README.md`](apps/extension/README.md).

## Screenshots

<p align="center">
  <img src="assets/readme/shot-list-en.png" alt="The downloads list with segmented progress bars, speed, time left and status" width="100%">
</p>
<p align="center"><sub>Every stripe in a progress bar is one connection. Red stripes mark a file that failed its checksum.</sub></p>

<table>
  <tr>
    <td width="50%"><img src="assets/readme/shot-details-en.png" alt="Details drawer with segment map and live speed chart"></td>
    <td width="50%"><img src="assets/readme/shot-add-en.png" alt="Add dialog probing the link as you type"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Details:</b> live segment map, speed chart, checksum.</sub></td>
    <td align="center"><sub><b>Add:</b> size, name and resumability checked as you type.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="assets/readme/shot-details-ar.png" alt="The details view in Arabic, mirrored right to left"></td>
    <td width="50%"><img src="assets/readme/shot-settings-en.png" alt="Settings: language, theme, folders per category"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Arabic:</b> the whole layout mirrors, instantly.</sub></td>
    <td align="center"><sub><b>Settings:</b> changes apply immediately.</sub></td>
  </tr>
</table>

## Features

**Speed**
- Multi-connection downloads (1 to 32) with **work stealing**: when a connection finishes early it takes half
  of the slowest remaining part, so the link stays saturated to the last byte.
- A per-server connection cap so hosts don't block you; automatic retries with back-off and `Retry-After`.

**Reliability**
- **Byte-exact resume.** Pause any time; resume continues from the exact byte, and only if the server still
  serves the same file. If it changed, Riptide starts over instead of mixing two versions.
- **Crash-proof.** Progress is saved to a local database every second. Kill the app or lose power, and every
  download continues on the next start.
- **Checksum verification** (MD5, SHA-1, SHA-256), and *Restart from the beginning* when you need a clean copy.

**Organised**
- Categories by file type, each with its own folder. Duplicate detection before adding. Real free-space
  checks, with a warning when a file won't fit.
- Search, sort, filter, multi-select, a details drawer, and full keyboard control (press `?` for shortcuts).

**Safe and private**
- File names from servers are sanitised and can never escape your folder. Redirects are capped. Cookies and
  passwords are only sent to the site they belong to, never logged, and deleted when the download ends.
- No admin rights, no account, no telemetry. The only network traffic is your downloads.

**Yours**
- Arabic and English everywhere, auto-detected and switchable live, with a mirrored RTL layout and
  correct Arabic file names. Light and dark themes. Respects reduced-motion settings.
- A portable mode for USB drives, and a command-line tool (`riptide fetch`) for scripts.

## Riptide vs IDM

| | **Riptide** | IDM |
|---|---|---|
| Price | **Free, forever** | Paid license per PC |
| Source code | **Open** (GPL-3.0 app, MIT / Apache-2.0 engine) | Closed |
| Ads, trial reminders, activation | **None** | Trial reminders, serial activation |
| Administrator rights | **Never needed** | Standard installer |
| Arabic interface | **Fully mirrored RTL** | Translated strings |
| Multi-connection downloads | Yes, with work stealing | Yes |
| Browser integration | Chrome, Edge, Firefox | Chrome, Edge, Firefox and more |
| Resume after crash or power loss | Database checkpoints every second | Resume support |
| Built-in checksum verification | **Yes** (MD5, SHA-1, SHA-256) | No |
| Maturity | Alpha | Shipping for many years |

<sub>IDM is a trademark of Tonec Inc. The comparison reflects publicly documented behaviour at the time of
writing. If a row is wrong, please open an issue.</sub>

## How it works

1. **Probe.** Riptide asks the server for the file's size, final address, name, and whether it supports
   partial downloads.
2. **Split.** The file is cut into equal parts, one per connection, and space for the whole file is reserved on
   disk.
3. **Pull.** Each connection fetches its part and writes it straight into place. Idle connections take work
   from slow ones.
4. **Checkpoint.** Every second, each part's progress is written to a local SQLite database in one transaction.
5. **Verify and finish.** The size and optional checksum are checked, then the file gets its final name in one
   atomic step.

The browser extension talks to a tiny local helper (`riptide-host`) that forwards only a short list of safe
requests to the app over a private, per-user channel. The full picture is in
[ARCHITECTURE.md](ARCHITECTURE.md).

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
npm run tauri dev

# build the installer and the portable zip
npm run bundle
npm run bundle:portable

# the quality gate: lint, build and every test (run from the repository root)
.\scripts\check.ps1
```

Engine tests run against a local test server that fakes every way a download can go wrong (dropped
connections, flaky servers, files that change mid-download, wrong lengths, expired links, hostile file names),
and every resume test compares the SHA-256 of the result with the source. More in
[CONTRIBUTING.md](CONTRIBUTING.md).

<details>
<summary><b>Repository layout</b></summary>

```
crates/
  riptide-core/        download model, state machine, segment planner, categories, file-name sanitiser
  riptide-net/         HTTP engine: probe, ranges, multi-connection transfer, retry, auth, redirects
  riptide-store/       SQLite persistence and migrations
  riptide-engine/      runtime: queue, scheduler, crash recovery, events, disk space
  riptide-ipc/         private per-user pipe and message schema
  riptide-host/        browser Native Messaging host
  riptide-log/         logging with secret redaction, installed and portable paths
  riptide-cli/         command-line client (riptide fetch / bench)
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

**التحميل:** [المثبّت](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.2/Riptide_0.1.0-alpha.2_x64-setup.exe) ·
[النسخة المحمولة](https://github.com/LayanJarrar1/Riptide/releases/download/v0.1.0-alpha.2/riptide-0.1.0-alpha.2-windows-x64-portable.zip) ·
[دليل الاستخدام بالعربية](guide/ar.md)

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
Open Font License.
