# Riptide browser extension

Manifest V3 extension for Chrome, Edge and Firefox that hands downloads to the Riptide desktop
app. One TypeScript codebase, three builds (`dist/chrome`, `dist/edge`, `dist/firefox`).

The extension can only **ask**: it sends `ping`, `probe`, `probe_media`, `add`, `refresh` and the
read-only `list` to the Native Messaging host `app.riptide.host` (`crates/riptide-host`), which forwards
them to the app over its local pipe.
Nothing coming from a page is ever executed, shelled or used as a path.

## What it does

- **Automatic capture.** On Chrome/Edge `downloads.onDeterminingFilename` sees every browser
  download; on Firefox `webRequest.onHeadersReceived` catches `Content-Disposition: attachment`
  and non-renderable MIME types before the download starts, with `downloads.onCreated` as the
  fallback. If the rules say "take it", the browser download is paused/cancelled and an `add`
  is sent with the URL, suggested filename, referrer, user agent and the cookies of that URL
  only. If anything fails (host missing, app not running, host error) the browser download
  **proceeds** and a notification offers *Open Riptide* / *Never ask for this site*.
- **Rules** (options page): on/off, minimum size (default 1 MB), file-type groups (archives,
  video, audio, programs, documents, images) plus custom extensions and a "never take these"
  extension list, site allow/deny lists, hold **Alt** to bypass, per-site "never ask again".
  The engine is pure (`src/rules.ts`, `src/bypass.ts`) and unit-tested.
- **Visible everywhere.** Toolbar badge (count of hand-offs this session; grey when the app is
  not reachable), popup with status (connected / app not running / host not installed), the
  last 10 hand-offs with live state when the host allows `list`, a pause-capture toggle, *Open
  Riptide* and *Options*. Context menu *Download with Riptide* on links, images, video and
  audio; *Download all links on this page*, *Download selected links* and *Download all images*
  open an in-page checklist (see below). A content script shows a small *Download with Riptide* badge when hovering a
  direct file link (shadow DOM, no layout shift, no remote code).
- **Bilingual.** English and Arabic (RTL) for the manifest (`_locales/`), popup, options,
  notifications and the in-page panel; follows the browser UI language or a fixed choice.

## Build, check, test

```
npm ci
npm run build      # dist/chrome, dist/edge, dist/firefox (+ the brand-kit icons copied from brand/)
npm run check      # tsc --noEmit && eslint .
npm test           # node:test over src/**/*.test.ts (bundled with esbuild)
npm run package    # build + dist/riptide-{chrome,edge,firefox,source}-<version>.zip for the stores
```

The store zips drop the manifest `key` (the Chrome Web Store refuses it on a new item) and
contain no source maps; `dist/<target>/` keeps the key for unpacked builds. Listing texts,
images, permission justifications and the step-by-step for each store are in `store/`
(`store/SUBMISSION.md`).

`scripts/check.ps1` at the repository root runs `npm ci`, `tsc --noEmit` and `eslint .` here.
`npm run build -- --watch` rebuilds on change. `npm run build` ends with
`scripts/verify-dist.mjs`, which checks every manifest is valid MV3 JSON, every referenced file
exists, the locales share one key set and both extension ids are documented here.

## Install in Chrome (three steps) / التثبيت في كروم

Branded Chrome (137+) ignores `--load-extension`, and a silent install needs admin policies, so
Riptide ships the extension as a folder you load once. No store account, no admin rights.

**Easiest:** run `scripts\install-extension.ps1` (repository) or `install-extension.ps1` in the
portable folder. It copies the extension to `%LOCALAPPDATA%\Riptide\extension\chrome`, repairs the
browser bridge registration if needed, copies that path to the clipboard and opens
`chrome://extensions`. `-Browser edge` does the same for Edge, `-Uninstall` undoes it, and running
it again after an update refreshes the files. The app also has the same helper in
*Settings → Integration → Install the extension in Chrome or Edge*.

1. Open `chrome://extensions` and turn on **Developer mode** (top right).
2. Press **Load unpacked**.
3. Pick the `extension\chrome` folder (installer: `%LOCALAPPDATA%\Riptide\extension\chrome`;
   portable zip: the `extension\chrome` folder next to `riptide-desktop.exe`; repository:
   `apps/extension/dist/chrome`).

The card must show the id `dcoeaodjhaibfchhcbfdkekpcacofnhm`; that is the id the bridge trusts.
Pin the Riptide icon from the puzzle-piece menu. After updating Riptide, press the reload arrow on
the card. If Chrome says "Developer mode extensions" on start, choose *Keep*.

**بالعربية:**

لا يسمح كروم (الإصدار 137 فأحدث) بتثبيت إضافة غير منشورة تلقائيًا، لذلك يوفّر ريبتايد الإضافة كمجلد
تحمّله مرة واحدة، دون حساب متجر ودون صلاحيات مدير.

**الأسهل:** شغّل `scripts\install-extension.ps1` (من المستودع) أو `install-extension.ps1` من مجلد النسخة المحمولة.
ينسخ الإضافة إلى `%LOCALAPPDATA%\Riptide\extension\chrome`، ويُصلح تسجيل الجسر عند الحاجة، وينسخ المسار إلى الحافظة،
ويفتح `chrome://extensions`. نفس المساعد موجود في التطبيق: *الإعدادات ← التكامل ← تثبيت الإضافة في كروم أو إيدج*.

1. افتح `chrome://extensions` وفعّل **وضع المطوّر (Developer mode)** أعلى الصفحة.
2. اضغط **تحميل إضافة غير مضغوطة (Load unpacked)**.
3. اختر مجلد `extension\chrome` (النسخة المثبّتة: `%LOCALAPPDATA%\Riptide\extension\chrome`،
   النسخة المحمولة: المجلد `extension\chrome` بجانب `riptide-desktop.exe`).

يجب أن تعرض البطاقة المعرّف `dcoeaodjhaibfchhcbfdkekpcacofnhm`. ثبّت أيقونة ريبتايد من قائمة الإضافات (قطعة الأحجية).
بعد تحديث ريبتايد اضغط سهم إعادة التحميل على البطاقة.

## Load unpacked

| Browser | Steps |
|---|---|
| Chrome | `chrome://extensions` → *Developer mode* → *Load unpacked* → `apps/extension/dist/chrome` |
| Edge | `edge://extensions` → *Developer mode* → *Load unpacked* → `apps/extension/dist/edge` |
| Firefox | `about:debugging#/runtime/this-firefox` → *Load Temporary Add-on…* → `apps/extension/dist/firefox/manifest.json`. Then open the popup and press *Allow access to all sites* (Firefox MV3 makes host permissions optional). |

## Extension ids (the host's allow-list needs them)

| Browser | Id | Where it comes from |
|---|---|---|
| Chrome, Edge (unpacked and self-hosted) | `dcoeaodjhaibfchhcbfdkekpcacofnhm` | `key` in `manifest/chrome.json` / `manifest/edge.json` (public key; `npm run key` prints a new pair, `node scripts/gen-key.mjs --id` prints the id of the current key) |
| Firefox | `riptide@riptide.app` | `browser_specific_settings.gecko.id` in `manifest/firefox.json` |

Store-published builds get ids from the stores (Chrome Web Store keeps this one only if the
same key is uploaded; the Edge Add-ons store assigns its own). Extra ids go into
`extra_extension_ids` of `%LOCALAPPDATA%\Riptide\integration.json`; the app adds them to the
manifests on its next registration and the host accepts them (there is no UI for this yet).

## Native Messaging host

The host is `riptide-host.exe` (`crates/riptide-host`), shipped next to `riptide-desktop.exe`
in the installer and the portable zip. The browser starts it per port; it

- checks the caller: Chrome/Edge pass `chrome-extension://<id>/` as the first argument,
  Firefox passes the manifest path and then the extension id; only the ids above (plus the
  extras in `integration.json`) are served, anyone else gets one `error` frame and the host
  exits;
- reads 4-byte little-endian length + UTF-8 JSON frames (1 MB limit; a larger one gets an
  `error` and ends the session), accepts only `ping`, `probe`, `add`, `refresh` and `list`
  that parse against the schema, answers everything else with `error` (`bad_request`), and
  forwards the rest to the app's pipe `\\.\pipe\riptide.<user-sid>` (current user only);
- answers with an `error` (`internal`, "the Riptide app is not running") when the pipe is
  down and keeps the session open. On a `ping` it first starts `riptide-desktop.exe` from its
  own folder (or `%LOCALAPPDATA%\Riptide`) and waits up to 10 s for the pipe, so *Open Riptide*
  works; a second app launch only focuses the running window.

Nothing coming from the browser is executed or used as a path. The app answers `probe`, `add`
(the new download appears in the list and the window comes to the front), `list`, `get` and
`refresh` (new URL and secrets for an existing download; `not_found` otherwise).

### Registration

The host is registered per user, no elevation, at three moments (all idempotent, all the same
Rust code, so the paths and extension ids can not drift):

1. **At install time.** The NSIS hooks (`apps/desktop/src-tauri/windows/hooks.nsh`) run
   `riptide-desktop.exe --register-host` after copying the files, on a fresh install and on every
   upgrade. The extension therefore works before the app was ever opened. A silent install
   (`/S`) does the same.
2. **On every app start** (it also repairs the paths after the app moved) until the user presses
   *Turn off* in *Settings → Integration → Browser extension*, and again with *Set up*. The choice
   is stored as `browser_host` in `integration.json`; an upgrade never turns it back on.
3. **On uninstall** `--unregister-host` removes only the keys and manifests that point at *this*
   install's `riptide-host.exe`. A second copy (portable folder, another install dir) keeps its
   registration; settings and downloads are untouched.

`NativeMessagingHosts` is per Windows user, not per browser profile: one registration serves every
Chrome profile (and every Edge profile) of that user. What is per profile is the extension itself:
each profile loads it once (*Developer mode → Load unpacked*, or from a store later). Riptide never
sideloads the extension, never writes browser policy or `Preferences` files, and the browser's own
consent screens stay in charge.

The extension folders (`extension\chrome`, `extension\edge`) are installed next to the app, which is
`%LOCALAPPDATA%\Riptide` by default, and survive upgrades; *Settings → Integration → Open extension
folder* and `install-extension.ps1` point at that copy.

**Open Riptide** (popup) asks the host for a `ping`; when the app's pipe is down the host starts
`riptide-desktop.exe` (next to the host, else `%LOCALAPPDATA%\Riptide`) detached and waits up to
10 s. If the app is already running nothing is started (the single-instance lock would focus it
anyway). The popup shows a "Starting Riptide…" state, then connected, or "did not start" with the
next step. With no registration the popup says *host not installed* and **Website and help** opens
the project site (a fixed https URL, opened by the extension itself).

Test knobs (never needed in normal use): `RIPTIDE_REGISTRY_ROOT` moves the keys under
`HKCU\<value>`, `RIPTIDE_INTEGRATION_DIR` moves the manifests and `integration.json`. The installer
test `scripts/test-installer.ps1` and the popup E2E `apps/extension/e2e/popup_states.py` use them.

| Browser | Registry value (`HKCU`) | Manifest file it points to |
|---|---|---|
| Chrome | `HKCU\Software\Google\Chrome\NativeMessagingHosts\app.riptide.host` (default value = path) | `%LOCALAPPDATA%\Riptide\native-messaging\app.riptide.host.chromium.json` |
| Edge | `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\app.riptide.host` | same Chromium manifest |
| Firefox | `HKCU\Software\Mozilla\NativeMessagingHosts\app.riptide.host` | `%LOCALAPPDATA%\Riptide\native-messaging\app.riptide.host.firefox.json` |

The portable build keeps the manifests in `data\native-messaging\` next to the executable (the
registry values are per user either way).

Chromium manifest (`allowed_origins` is required; the trailing slash matters):

```json
{
  "name": "app.riptide.host",
  "description": "Riptide download manager — browser bridge",
  "path": "C:\\Users\\<user>\\AppData\\Local\\Riptide\\riptide-host.exe",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://dcoeaodjhaibfchhcbfdkekpcacofnhm/"]
}
```

Firefox manifest (`allowed_extensions` instead of `allowed_origins`):

```json
{
  "name": "app.riptide.host",
  "description": "Riptide download manager — browser bridge",
  "path": "C:\\Users\\<user>\\AppData\\Local\\Riptide\\riptide-host.exe",
  "type": "stdio",
  "allowed_extensions": ["riptide@riptide.app"]
}
```

### Protocol and status contract

Frames are the JSON in `crates/riptide-ipc/src/message.rs`, mirrored in `src/protocol.ts` and
pinned by `src/protocol.fixtures.json` (the browser adds the 4-byte length prefix). The
extension derives its status from what happens:

| Observation | Status shown |
|---|---|
| `pong` | connected |
| `error` answer to `ping` (host up, app's pipe down) | app not running |
| port disconnect "host not found / forbidden / no such native application" | host not installed |
| any other disconnect or timeout | app not running |

*Open Riptide* (popup, notification button) sends a plain `ping`; the host starts the app when
it is not running (see above). Because the extension also pings on browser start-up, the app
starts with the browser while the integration is on. The popup sends `list` to show live
states; if the host answers with an `error` (an older host forwarded only
`ping`/`probe`/`add`/`refresh`), the popup shows "sent" and says live state is unavailable.

## Video pages ("Download this video")

- **Detection** (`src/media.ts`, pure, unit-tested). A page counts as a video page when it is a
  watch page of a site in `MEDIA_HOSTS` (mirrors `riptide-core`), or when it holds a `<video>`
  of at least 200x112 px. On Firefox the same `webRequest` permission the capture already uses
  also *observes* (never blocks) responses with a `video/*` or `audio/*` MIME, an HLS/DASH
  manifest (`.m3u8`, `.mpd`) or a progressive `mp4`/`webm`. Chrome and Edge have no such
  permission in this extension, so they rely on the DOM check and the known hosts; adding
  `webRequest` there would be a new permission and is listed as a follow-up. A detected video
  puts a counter on the toolbar badge of that tab (cyan `#00D5E8`).
- **Floating button.** A small pill in the content script's closed shadow root, at the corner
  of the video (or the viewport corner), RTL-aware, reachable with the keyboard (`Tab`, arrow
  keys in the menu, `Esc` closes). `x` hides it until the next page; *Never show on this site*
  stores the host in `settings.videoNeverSites` (editable in Options, switch
  `settings.videoButton`).
- **Quality menu.** Clicking asks the app through the host (`probe_media`) and lists the
  qualities it returns (best, `1080p`, ..., audio-only M4A/MP3) with approximate sizes.
  Choosing one sends `add` with `kind: "media"` and `media_format` set to the menu's key
  (`best`, `h<N>`, `audio-m4a`, `audio-mp3`). The host refuses any other value. If the app is
  not running or video support is missing, the message the app/host returned is shown.
- **Context menu.** *Download this video with Riptide* on pages and links that belong to a
  known video site (`documentUrlPatterns` / `targetUrlPatterns` built from `MEDIA_HOSTS`); it
  opens the same menu on the page.
- **Cookies** for a video are the cookies of the page's own origin only, sent together with a
  `cookie_origin` claim that `riptide-host` checks against the URL and drops on mismatch. Extra
  headers and credentials never travel with a media request. Nothing is logged.

## Download all links / selected links / images

The three context-menu items ask the content script to collect candidates and show a checklist
in a closed shadow root (RTL-aware, keyboard accessible: arrows/Home/End move, Space/Enter
toggles, Ctrl+A selects all, Tab stays inside, Escape closes). Candidates are the page's
resolved `<a href>` values (or, for images, `img` `currentSrc`/`srcset` (largest), `<picture>`
sources, `input[type=image]` and links straight to pictures), `http(s)` only, no credentials,
fragment stripped, de-duplicated, at most 5000. File names are guessed from the URL. The list
filters by type (archives, video, audio, documents, images, programs, other; the same table as
`riptide-core` `category.rs`, checked by a unit test) and by text, shows a selected count,
select all/none, and renders only the visible rows above 200 items.

**Sending.** `riptide-host` has no batch message yet, so *Download N* is one `handoff-many`
message to the background, which sends sequential-with-cap (4 in flight) `add` messages, at
most 2000, and shows one notification for the batch. Cookies are attached only to URLs on the
page's own origin. The host validates each `add` (http(s) only, no control
characters, at most 8192 bytes).

**Alt bypass limits.** Chromium's `onDeterminingFilename` carries no modifier keys, so the
content script tracks Alt (keydown/keyup/blur and `click.altKey`) and, on a click, tells the
background to leave that URL (4 s) and any download within 1.5 s to the browser. It cannot work
on pages where content scripts do not run (`chrome://`, the add-on store, the PDF viewer, other
extensions' pages) or for downloads not started by a click (keyboard activation, scripted
navigation).

## Permissions

| Permission | Why |
|---|---|
| `downloads` | see browser downloads as they start, pause/cancel the ones Riptide takes, re-issue a download when Riptide is unavailable |
| `nativeMessaging` | talk to `app.riptide.host` |
| `contextMenus` | *Download with Riptide* / *Download all links* / *Download selected links* |
| `storage` | rules and language (`local`), session counters (`session`) |
| `notifications` | "Riptide not running — open it?" and "Sent to Riptide" |
| `cookies` | send the cookies of the downloaded URL (only that URL) so logged-in downloads work in Riptide; stored in the app's `secrets` table and wiped on completion |
| `webRequest`, `webRequestBlocking` (Firefox only) | Firefox has no `onDeterminingFilename`; the response headers are the only way to catch a download before it starts. Also used, without blocking, to notice media responses for the video badge (only the `Content-Type` and URL are looked at, nothing is stored) |
| *(no new permission for link/image checklists or capture rules)* | the checklist is the content script already listed; the extra menu item needs only `contextMenus` |
| *(no new permission for video support)* | the floating button needs only the content script and `cookies` already listed; the extra context-menu items need only `contextMenus` |
| host permissions `<all_urls>` | the content script (hover badge, link panel), reading cookies/referrers and, on Firefox, inspecting response headers on any site the user downloads from. Firefox treats it as optional and the popup asks for it. |

## Privacy

The extension collects nothing. It has no analytics, no remote code and no network access of
its own; its only connection is the Native Messaging host on this computer. Cookies travel
only for the URL being downloaded and only to the local app. Rules and the language choice stay
in the browser's extension storage.

## Layout

```
manifest/        base.json + chrome.json / edge.json / firefox.json overlays
_locales/        manifest strings (en, ar)
icons/           riptide.svg (brand-kit app icon; PNGs are copied from brand/ at build time by scripts/make-icons.mjs)
src/
  protocol.ts    IPC schema mirror (+ fixtures and test)
  rules.ts       capture rules engine (+ test)
  settings.ts    settings shape and normalisation; storage.ts does the IO
  i18n.ts        runtime strings (src/i18n/en.json, ar.json) (+ parity test)
  status.ts      host status + disconnect classification
  host.ts        Native Messaging client
  background.ts  capture, context menus, badge, notifications, message bus
  content/       hover badge, Alt tracking, link checklist (links.ts), video button (shadow DOM)
  popup/         toolbar popup
  options/       options page
scripts/         build.mjs, verify-dist.mjs, test.mjs, make-icons.mjs, gen-key.mjs
```
