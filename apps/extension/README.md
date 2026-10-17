# Riptide browser extension

Manifest V3 extension for Chrome, Edge and Firefox that hands downloads to the Riptide desktop
app. One TypeScript codebase, three builds (`dist/chrome`, `dist/edge`, `dist/firefox`).

The extension can only **ask**: it sends `ping`, `probe`, `add` and `refresh` messages to the
Native Messaging host `app.riptide.host`, which forwards them to the app over its local pipe.
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
  video, audio, programs, documents, images) plus custom extensions, site allow/deny lists,
  hold **Alt** to bypass, per-site "never ask again". The engine is pure (`src/rules.ts`) and
  unit-tested.
- **Visible everywhere.** Toolbar badge (count of hand-offs this session; grey when the app is
  not reachable), popup with status (connected / app not running / host not installed), the
  last 10 hand-offs with live state when the host allows `list`, a pause-capture toggle, *Open
  Riptide* and *Options*. Context menu *Download with Riptide* on links, images, video and
  audio; *Download all links on this page* and *Download selected links* open an in-page
  checklist. A content script shows a small *Download with Riptide* badge when hovering a
  direct file link (shadow DOM, no layout shift, no remote code).
- **Bilingual.** English and Arabic (RTL) for the manifest (`_locales/`), popup, options,
  notifications and the in-page panel; follows the browser UI language or a fixed choice.

## Build, check, test

```
npm ci
npm run build      # dist/chrome, dist/edge, dist/firefox (+ icons rendered from icons/riptide.svg)
npm run check      # tsc --noEmit && eslint .
npm test           # node:test over src/**/*.test.ts (bundled with esbuild)
```

`scripts/check.ps1` at the repository root runs `npm ci`, `tsc --noEmit` and `eslint .` here.
`npm run build -- --watch` rebuilds on change. `npm run build` ends with
`scripts/verify-dist.mjs`, which checks every manifest is valid MV3 JSON, every referenced file
exists, the locales share one key set and both extension ids are documented here.

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
same key is uploaded; the Edge Add-ons store assigns its own). *Settings → Integration* in the
app should therefore accept extra ids.

## Native Messaging host registration

The desktop app registers the host from *Settings → Integration*, per user, no elevation:

| Browser | Registry value (`HKCU`) | Manifest file it points to |
|---|---|---|
| Chrome | `HKCU\Software\Google\Chrome\NativeMessagingHosts\app.riptide.host` (default value = path) | `%LOCALAPPDATA%\Riptide\host\app.riptide.host.chromium.json` |
| Edge | `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\app.riptide.host` | same Chromium manifest |
| Firefox | `HKCU\Software\Mozilla\NativeMessagingHosts\app.riptide.host` | `%LOCALAPPDATA%\Riptide\host\app.riptide.host.firefox.json` |

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

*Open Riptide* (popup, notification button) sends a plain `ping`; whether to launch the app on
it is the host's decision. The popup tries `list` to show live states; if the host answers with
an `error` (it forwards only `ping`/`probe`/`add`/`refresh`), the popup shows "sent" and says
live state is unavailable.

## Permissions

| Permission | Why |
|---|---|
| `downloads` | see browser downloads as they start, pause/cancel the ones Riptide takes, re-issue a download when Riptide is unavailable |
| `nativeMessaging` | talk to `app.riptide.host` |
| `contextMenus` | *Download with Riptide* / *Download all links* / *Download selected links* |
| `storage` | rules and language (`local`), session counters (`session`) |
| `notifications` | "Riptide not running — open it?" and "Sent to Riptide" |
| `cookies` | send the cookies of the downloaded URL (only that URL) so logged-in downloads work in Riptide; stored in the app's `secrets` table and wiped on completion |
| `webRequest`, `webRequestBlocking` (Firefox only) | Firefox has no `onDeterminingFilename`; the response headers are the only way to catch a download before it starts |
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
icons/           riptide.svg (PNGs are rendered at build time by scripts/make-icons.mjs)
src/
  protocol.ts    IPC schema mirror (+ fixtures and test)
  rules.ts       capture rules engine (+ test)
  settings.ts    settings shape and normalisation; storage.ts does the IO
  i18n.ts        runtime strings (src/i18n/en.json, ar.json) (+ parity test)
  status.ts      host status + disconnect classification
  host.ts        Native Messaging client
  background.ts  capture, context menus, badge, notifications, message bus
  content/       hover badge, Alt tracking, link panel (shadow DOM)
  popup/         toolbar popup
  options/       options page
scripts/         build.mjs, verify-dist.mjs, test.mjs, make-icons.mjs, gen-key.mjs
```
