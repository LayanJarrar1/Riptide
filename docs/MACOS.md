# Riptide on macOS — plan and status

Riptide was built for Windows first. The engine and the UI are portable (Rust and Angular in a
Tauri v2 window), and the code already compiled on Linux; what macOS needed was packaging and
the platform integrations. This file is the plan, what is done, and how to build and check it.

## Status

| Area | Windows | macOS | Notes |
| --- | --- | --- | --- |
| Compiles (engine, AI, files, desktop app) | yes | **yes** | Checked with `cargo clippy --target aarch64-apple-darwin -D warnings` (cross-checked from Linux, see below), and in the `macos` workflow on a real Mac runner. |
| App bundle | NSIS installer | **`.app` + `.dmg`** | `tauri.macos.conf.json` (merged automatically on macOS) and `npm run bundle:mac`. Minimum macOS 11. |
| Window frame | custom title bar and buttons | **native traffic lights** | `titleBarStyle: Overlay`; the in-window menu bar stays, the drawn window buttons are hidden, 76 px left for the traffic lights. |
| Shortcut labels | `Ctrl+N` | **`⌘N`** | `core/platform.ts`; every handler already accepted ⌘ (`metaKey`). |
| AI and search API keys | Credential Manager | **Keychain** | `KeychainStore` (generic passwords, service `Riptide/ai`). Before: kept in memory only. |
| "From the internet" mark | `Zone.Identifier` stream | **`com.apple.quarantine`** | Gatekeeper checks a downloaded app before it first opens. No URL is stored. |
| `magnet:` links, `.torrent` files | per-user registry, opt-in | **declared in Info.plist** | Delivered as an "open" event (`RunEvent::Opened`), checked like a command line. Making Riptide the *default* handler is the user's choice (browser / Finder "Open with"); the Settings switch still says "not supported" on macOS. |
| After-queue actions | sleep, hibernate, shut down | **sleep, shut down** | `pmset sleepnow`; System Events "shut down" (macOS asks once to allow it). Hibernate is not offered. |
| File manager | Recycle Bin with restore | **Trash, no restore** | macOS has no API to put items back; the UI says "use Finder's Put Back" and offers no Undo for a delete. Drives = `/` and `/Volumes/*`. Option-drag copies. Finder wording. |
| Video pages (yt-dlp) | yt-dlp + ffmpeg | **yt-dlp only** | yt-dlp's universal macOS build, pinned by SHA-256. No ffmpeg yet: formats that need merging fall back to single-file ones. |
| Browser extension (Chrome, Edge, Firefox) | registry keys to the manifests | **links in `~/Library/Application Support/<browser>/NativeMessagingHosts/`** | Settings → Integration works the same: each browser's `app.riptide.host.json` is a symbolic link to the manifest in Riptide's data folder, pointing at `riptide-host` inside the app. Only our own links are ever removed. |
| Wording | Windows, Credential Manager | **macOS, Keychain, Open at login** | A `<key>Mac` string replaces `<key>` on a Mac when it exists (`I18nService.t`). |
| Menu bar | in-window menu | **app menu (About, Hide, Quit ⌘Q), Edit, Window** + the in-window menu | `mac_menu.rs`. Tauri's default Quit ended the process on the spot; ours asks while downloads run (like closing the window) and stops them at a checkpoint. Labels are English for now. |
| Dock icon | – | **reopens the window** | A click on the Dock icon while the window sits in the menu bar shows it (`RunEvent::Reopen`). |
| Release and updates | NSIS + `windows-x86_64` feed entry | **opt-in `darwin-aarch64`** | With the repository variable `MACOS_RELEASE=true`, `release.yml` also builds `Riptide_<version>_aarch64.dmg` and the signed `.app.tar.gz`, uploads both and adds `darwin-aarch64` to `latest-beta.json` (and `latest.json` for stable tags) and `SHA256SUMS.txt`. It only patches a feed of the same version. |
| Notifications, autostart, tray, updater check | yes | yes (plugins) | Autostart uses a LaunchAgent. |

## Remaining phases

1. **Intel Macs and signing.** Releases are Apple Silicon only (`darwin-aarch64`); an Intel or
   universal build needs `riptide-host` staged for `x86_64-apple-darwin` / `universal-apple-darwin`.
   Signed and notarized releases need an Apple Developer account (the secrets listed in
   `.github/workflows/macos.yml`).
2. **Default handler switch.** Settings → Integration could set Riptide as the default `magnet:`
   handler through LaunchServices (`LSSetDefaultHandlerForURLScheme`), opt-in like on Windows.
3. **ffmpeg for video.** Bundle an LGPL-only macOS ffmpeg, or let the user point Riptide at one
   (Homebrew), so best-quality video and audio can be merged.
4. **Full native menu bar.** The app, Edit and Window menus are native now; File / Downloads /
   Tools / View / Help could move there too (and the in-window menu go away on macOS), with the
   labels following the app language.
5. **Linux.** The same work for Secret Service (keys), `xdg` handlers and packaging (AppImage /
   .deb), if wanted.

## Building on a Mac

```sh
# once: Rust (rustup), Node 20, Xcode command line tools
cd apps/desktop
npm ci
npm run tauri dev          # development window
npm run bundle:mac         # Riptide.app and Riptide_<version>_aarch64.dmg in target/release/bundle
```

Without signing secrets the app is ad-hoc signed. On first launch macOS refuses it as from an
unidentified developer: System Settings → Privacy & Security → Open Anyway (or right-click → Open).

## Checking macOS code from Linux

The macOS code paths can be compile-checked without a Mac: add the target and use zig as the C
compiler for the few C dependencies (an Objective-C source that needs Apple's SDK is replaced by
an empty object; nothing is linked, so the check stays valid):

```sh
rustup target add aarch64-apple-darwin
pip install ziglang
# CC_aarch64_apple_darwin / AR_aarch64_apple_darwin -> small wrappers around `python3 -m ziglang cc/ar`
cargo clippy --target aarch64-apple-darwin -p riptide-desktop --all-targets -- -D warnings
```

## Manual checks on a Mac (not covered by automated tests)

- [ ] The `.dmg` opens, drag to Applications works, the app starts (after Open Anyway when unsigned).
- [ ] Traffic lights sit in the 76 px gap of the title bar, in English and Arabic; dragging the bar moves the window.
- [ ] ⌘N, ⌘F, ⌘, and ⌘B work; ⌘C / ⌘V / ⌘A work in text fields.
- [ ] ⌘Q with a download running asks first (when "Confirm on exit" is on); after Quit, the download resumes on the next start. With the window hidden in the menu bar, clicking the Dock icon shows it.
- [ ] A download finishes and Finder shows "downloaded from the internet" for a `.dmg` or `.app` inside it.
- [ ] An AI key entered in Settings is still there after quitting and reopening (Keychain Access shows `Riptide/ai`).
- [ ] Clicking a `magnet:` link in Safari (after choosing Riptide) and double-clicking a `.torrent` in Finder open the torrent dialog, with the app running and not running.
- [ ] A queue set to "Sleep" puts the Mac to sleep; "Shut down" asks once for permission, then shuts down.
- [ ] Files: browse `/Volumes`, copy between two volumes, delete to the Trash, Option-drag copies.
- [ ] Settings → Integration → Set up: Chrome, Edge and Firefox each get `app.riptide.host.json` under `~/Library/Application Support/…/NativeMessagingHosts/`, and a download clicked in the browser arrives in Riptide (with the app running and not running). Turn off removes the links.
- [ ] After a release with `MACOS_RELEASE=true`: an older Mac install finds the update in Settings → About, installs it and restarts.
- [ ] A YouTube page downloads with yt-dlp (single-file formats without ffmpeg).
