<p align="center">
  <img src="assets/readme/banner.png" alt="Riptide: a free, open-source download manager for Windows" width="100%">
</p>

<p align="center">
  <img alt="Status: early development" src="https://img.shields.io/badge/status-early%20development-f59e0b?style=flat-square">
  <img alt="Platform: Windows 10 / 11" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4?style=flat-square">
  <img alt="Engine: Rust" src="https://img.shields.io/badge/engine-Rust-dea584?style=flat-square">
  <img alt="App: Tauri v2 + Angular" src="https://img.shields.io/badge/app-Tauri%20v2%20%2B%20Angular-19c6b5?style=flat-square">
  <img alt="Languages: Arabic and English" src="https://img.shields.io/badge/languages-%D8%B9%D8%B1%D8%A8%D9%8A%20%2B%20English-0b1c1a?style=flat-square">
  <img alt="License: GPL-3.0 app, MIT or Apache-2.0 engine" src="https://img.shields.io/badge/license-GPL--3.0%20%C2%B7%20MIT%20%2F%20Apache--2.0-6b8683?style=flat-square">
</p>

> [!WARNING]
> **Riptide is in early development and is not ready to use yet.**
> There are no releases, no installer and no promises about stability. Everything on this page comes from a
> work-in-progress build that changes daily. Star or watch the repository if you want to follow along.

<p align="center">
  <a href="#what-it-looks-like">Screenshots</a> ·
  <a href="#why-riptide">Why Riptide</a> ·
  <a href="#under-the-hood">Under the hood</a> ·
  <a href="#build-from-source">Build from source</a> ·
  <a href="#بالعربي">بالعربي</a>
</p>

---

Riptide is a **free, open-source download manager** that aims to be faster and more capable than the paid
ones, without the ads, the nag screens or the price tag. It is built on a Rust engine that treats every byte as
sacred and a desktop app that is **Arabic and English from day one**, right-to-left included.

Windows comes first. The engine is portable, so other platforms can follow.

## What it looks like

Screenshots are from the current development build in the light theme. The dark theme is the default.

<p align="center">
  <img src="assets/readme/shot-list-en.png" alt="The downloads list: segmented progress bars, speed, time left and status for every file" width="100%">
</p>
<p align="center"><sub>The list. Every stripe in a progress bar is one connection. Red stripes are a file that failed its checksum.</sub></p>

<br>

<p align="center">
  <img src="assets/readme/shot-details-en.png" alt="Details drawer with stat tiles, a segment map, a live speed chart and the source and destination" width="100%">
</p>
<p align="center"><sub>Details. Eight connections, a live segment map, the last minute of speed, the checksum the file must match.</sub></p>

<br>

<p align="center">
  <img src="assets/readme/shot-add-en.png" alt="Add download dialog: the link is probed as you type, showing size and whether it can resume" width="100%">
</p>
<p align="center"><sub>Add a download. Riptide probes the link while you type: size, file name, category and whether the server allows resuming.</sub></p>

<br>

<p align="center">
  <img src="assets/readme/shot-details-ar.png" alt="The same details view in Arabic, mirrored right-to-left" width="100%">
</p>
<p align="center"><sub>The same screen in Arabic. The whole layout mirrors, numbers and file names stay readable, and the switch is instant.</sub></p>

<br>

<p align="center">
  <img src="assets/readme/shot-settings-en.png" alt="Settings page: language, appearance, default folder and a folder per category" width="100%">
</p>
<p align="center"><sub>Settings. Changes apply immediately. No wizard, no restart.</sub></p>

## Why Riptide

- **Pulls harder.** A file is split into segments and fetched over many connections at once. When one
  connection finishes early it steals work from the slowest one, so the link stays saturated to the last byte.
- **Byte-exact or nothing.** A download counts as complete only when every segment is on disk, the server still
  serves the same version of the file, and the checksum you gave it matches. Riptide never splices bytes from
  two versions of a file.
- **Survives anything.** Progress is written to a local SQLite database continuously. Kill the process, pull the
  plug, crash the machine: on the next start every download resumes exactly where it stopped.
- **Resumes properly.** Pause and resume are instant and cheap. If the server changed the file in the
  meantime, Riptide notices and starts over instead of handing you a corrupt file.
- **Arabic and English, for real.** Both languages everywhere, auto-detected, switchable live, with a fully
  mirrored right-to-left layout. Arabic file names from servers land on disk with the right name.
- **Nothing to trust.** File names from servers are sanitised, redirects are capped, credentials never leak to
  another site, TLS verification is on. Secrets live in their own table and are wiped when a download ends.
- **No admin, no accounts, no ads, no telemetry.** Per-user install, your data stays on your machine, and the
  only network traffic is your downloads.
- **Fast UI.** A dense, keyboard-friendly list that stays smooth with a thousand items. Dark by default, light
  if you prefer, and it respects reduced-motion settings.

More is on the way. This page grows with the product.

## Under the hood

| Layer | Choice |
|---|---|
| Engine | Rust 2021 on `tokio`; HTTP through `reqwest` with `rustls` |
| Persistence | SQLite via `rusqlite` in WAL mode, transactional progress checkpoints |
| Desktop app | Tauri v2 with an Angular front end (standalone components, signals, strict TypeScript) |
| Types across the bridge | Generated from the Rust types with `ts-rs`, so the engine and the UI share one schema |
| Fonts | IBM Plex Sans Arabic and Inter, bundled locally |
| Platform | Windows 10 / 11, per-user install, no elevation anywhere |

The engine knows nothing about the UI. The same crates power the desktop app and a small command-line
client, and the whole engine is exercised against a local test server that fakes every way a download can go
wrong: dropped connections, flaky servers, changed files, wrong lengths, expired links, hostile file names.

## Build from source

This is for the curious. Nothing here is packaged for end users yet.

Prerequisites: stable Rust (`rustup`), Node 20+, Visual Studio Build Tools with the C++ workload, and WebView2
(already part of Windows 10 / 11).

```powershell
# everything: lint, build, tests (this is the gate every change must pass)
.\scripts\check.ps1

# the desktop app in development mode
cd apps\desktop
npm install
npm run tauri dev

# the local test server the engine tests run against
cargo run -p riptide-testserver
# then try http://127.0.0.1:8080/ok?size=104857600

# fetch one file from the command line
cargo run -p riptide-cli -- fetch http://127.0.0.1:8080/ok?size=104857600 -o C:\tmp\file.bin
```

## Repository layout

```
crates/
  riptide-core/        download model, state machine, segment planner, file-name sanitiser (pure, no IO)
  riptide-net/         the HTTP engine: probing, ranges, multi-connection transfer, retry, auth, redirects
  riptide-store/       SQLite persistence and migrations
  riptide-engine/      the runtime: queue, scheduler, crash recovery, event stream
  riptide-ipc/         local pipe transport and message schema
  riptide-log/         logging with secret redaction, portable and installed paths
  riptide-cli/         command-line client
  riptide-testserver/  local HTTP server with every failure mode the tests need (dev only)
apps/
  desktop/             Tauri v2 shell (src-tauri) and the Angular UI (src)
scripts/check.ps1      the gate: build, clippy, tests, lint
```

## بالعربي

**ريبتايد** مدير تحميل مجاني ومفتوح المصدر لويندوز، هدفه يكون أسرع وأقدر من البرامج المدفوعة، بدون إعلانات
وبدون حسابات وبدون تتبّع. المحرّك مكتوب بلغة Rust ويتعامل مع كل بايت على أنه لا يُمَسّ: التحميل لا يُعتبر
مكتملًا إلا إذا وصلت كل القطع كاملة، وبقي الملف على الخادم نفس النسخة، وطابق بصمة الملف إن أعطيته واحدة.

يتحمّل أي شيء: اقتل العملية، افصل الكهرباء، أعد التشغيل، وستجد كل التحميلات تكمل من حيث توقفت بالضبط.

الواجهة **عربية وإنجليزية من أول يوم**، بتخطيط معكوس كامل من اليمين لليسار، وتبديل فوري بين اللغتين، وأسماء
الملفات العربية القادمة من الخوادم تصل إلى القرص بالاسم الصحيح.

> **تنبيه:** المشروع ما زال في مرحلة تطوير مبكرة وغير جاهز للاستخدام بعد. لا توجد إصدارات ولا مثبّت حاليًا.
> كل ما تراه في هذه الصفحة من نسخة قيد العمل تتغيّر يوميًا.

## License

The desktop app, CLI and IPC crate are licensed under **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). The engine
crates (`riptide-core`, `riptide-net`, `riptide-store`, `riptide-engine`, `riptide-log`, `riptide-testserver`)
are dual-licensed **MIT OR Apache-2.0** ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)).
