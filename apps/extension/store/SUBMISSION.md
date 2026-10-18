# Submitting the Riptide extension

Everything for the three stores is in this folder; the packages come from `npm run package`.

| File | Use |
|---|---|
| `listing-en.md`, `listing-ar.md` | name, short and long descriptions, links, screenshot captions |
| `privacy-practices.md` | single purpose, permission justifications, data-use answers |
| `screenshots/1…5-*.png` | 1280×800, in this order (5 is Arabic) |
| `promo/promo-440x280.png` | CWS small promo tile, Edge small tile |
| `promo/marquee-1400x560.png` | CWS marquee, Edge large tile |
| `icon-128.png` | store icon (same as the toolbar icon) |

## 0. Before any store

1. Bump `version` in `package.json` **and** `manifest/base.json` (they must match; packaging fails
   otherwise).
2. In `apps/extension`: `npm ci`, `npm run check`, `npm test`, `npm run package`. Output in `dist/`:
   `riptide-chrome-<ver>.zip`, `riptide-edge-<ver>.zip`, `riptide-firefox-<ver>.zip`,
   `riptide-source-<ver>.zip`.
3. The privacy policy must be live: build and publish the website, then open
   https://layanjarrar1.github.io/Riptide/privacy/ and `/ar/privacy/`. (GitHub Pages on a private
   repository needs a paid plan; otherwise publish `apps/website/dist` on any static host and use
   that URL everywhere below.)
4. A Riptide desktop release must be downloadable from GitHub Releases: reviewers need it.
5. Re-render the images only if the UI changed:
   `npm run build`, then `PLAYWRIGHT_CORE=<…>/playwright-core/index.mjs node store/render.mjs`.

### Why the store zips have no `key`

The Chrome Web Store rejects a new item whose manifest has a `key` field ("key field is not
allowed in manifest") and assigns its own id; Edge asks for the same on the first upload. So
`scripts/package.mjs` removes `key` from the store zips; `dist/chrome` and `dist/edge` keep the
dev key so unpacked builds keep the id `dcoeaodjhaibfchhcbfdkekpcacofnhm`.

**Consequence (must be done before submitting for review):** each store gives the extension a
new id, and the native host only answers ids it knows. Upload the zip as a draft first, copy the
id the dashboard shows, and add it to the allow-list in `crates/riptide-host/src/lib.rs`
(`CHROMIUM_EXTENSION_ID` is a single id today; it needs a list) and to the host manifest written by
`apps/desktop/src-tauri/src/integration.rs`, then ship that desktop build. Until then a user (or a
reviewer) can add the id to `extra_extension_ids` in `%LOCALAPPDATA%\Riptide\integration.json`
and restart the app. Firefox keeps `riptide@riptide.app`, which is already allowed.

## 1. Chrome Web Store

Dashboard: https://chrome.google.com/webstore/devconsole (one-time USD 5 registration).

1. **New item** → upload `dist/riptide-chrome-<ver>.zip`. Note the item id (see above).
2. **Store listing**
   - Description: long description from `listing-en.md`. Add language **Arabic** and paste
     `listing-ar.md`. (Name and summary come from the manifest locales.)
   - Category: *Tools*. Language: English (+ Arabic).
   - Store icon: `icon-128.png`. Screenshots: `screenshots/1…5`. Small promo tile:
     `promo/promo-440x280.png`. Marquee: `promo/marquee-1400x560.png`.
   - Homepage URL and Support URL: from `listing-en.md` → Links.
3. **Privacy** tab: paste from `privacy-practices.md` — single purpose, one justification per
   permission (downloads, nativeMessaging, contextMenus, storage, notifications, cookies, host
   permission), remote code **No**, data usage none + three certifications, privacy policy URL.
4. **Distribution**: Public, all regions, free.
5. **Test instructions** (Submit → "Test instructions" field or the notes): paste *Review notes*
   below.
6. Submit. `<all_urls>` triggers an in-depth review; expect several days.

## 2. Microsoft Edge Add-ons

Partner Center: https://partner.microsoft.com/dashboard/microsoftedge (free).

1. **Create new extension** → upload `dist/riptide-edge-<ver>.zip`. Note the extension id on the
   overview page (see above).
2. **Availability**: Public, all markets.
3. **Properties**: Category *Productivity*; privacy policy required → **Yes**, URL from
   `privacy-practices.md`; website and support URLs from `listing-en.md`; mature content No.
4. **Store listings**: add **English** and **Arabic**. Description (min 250 characters): long
   description from the matching listing file. Logo `icon-128.png` (Edge wants 300×300 at most;
   128 is accepted), small promotional tile `promo/promo-440x280.png`, large promotional tile
   `promo/marquee-1400x560.png`, screenshots `screenshots/1…5` (1280×800).
5. **Submit** → *Notes for certification*: paste *Review notes* below.

## 3. Firefox Add-ons (AMO)

Developer Hub: https://addons.mozilla.org/developers/ (free).

1. **Submit a New Add-on** → *On this site* → upload `dist/riptide-firefox-<ver>.zip`.
2. "Do you need to submit source code?" → **Yes** (the code is bundled by esbuild) → upload
   `dist/riptide-source-<ver>.zip`. It contains every tracked file of `apps/extension` plus
   `BUILD.md`: `npm ci && npm run package` reproduces `dist/riptide-firefox-<ver>.zip` byte for
   byte (fixed timestamps; tool versions pinned in `package-lock.json`).
3. Describe the add-on: name and summary come from the locales; description from `listing-en.md`
   (add Arabic under *Manage localisations*), category *Download Management*, support site and
   e-mail/URL from `listing-en.md`, license **GPL-3.0-or-later**, privacy policy: paste the text of
   the website page or link it.
4. Data collection: the manifest declares `none`; confirm it.
5. Screenshots `screenshots/1…5`; icon is taken from the package.
6. **Notes to reviewer**: paste *Review notes* below.

Firefox MV3 treats host permissions as optional: after install the popup shows *Allow access to
all sites*; this is expected and is mentioned in the review notes.

## Review notes (paste into all three stores)

```
Riptide is a free, open-source (GPL-3.0) download manager for Windows. This extension is its
browser bridge: it hands downloads from the browser to the Riptide desktop app on the same
computer. Source: https://github.com/LayanJarrar1/Riptide (apps/extension).

HOW TO TEST
1. Install the Riptide desktop app (Windows 10/11, per-user, no admin) from
   https://github.com/LayanJarrar1/Riptide/releases and start it once. It registers the
   Native Messaging host "app.riptide.host" for Chrome, Edge and Firefox under HKCU.
2. Install the extension. The toolbar popup shows "Connected to Riptide".
   (Firefox: press "Allow access to all sites" in the popup first.)
3. Download any file over 1 MB (e.g. a Linux ISO or a large PDF): it appears in Riptide
   instead of the browser's download bar. Right-click a link -> "Download with Riptide" also
   works, and hovering a direct file link shows a small "Download with Riptide" button.
4. Without the desktop app the popup says "Host not installed" and downloads stay in the
   browser: nothing is lost and nothing else happens.

NATIVE MESSAGING
The extension's only connection is runtime.connectNative("app.riptide.host"). It sends a small
JSON schema (ping, probe, add, refresh, list) with the download address, suggested file name,
referrer, user agent and the cookies for that one address. The host checks the calling
extension id, validates every message, and forwards it to the app over a local named pipe
restricted to the current user. Nothing from the browser is executed or used as a file path.
The extension has no network access of its own, no analytics and no remote code.

WHY <all_urls>
Downloads come from any site, so the extension needs host access on all sites to:
- read the cookies of the URL being downloaded (only that URL) so signed-in downloads keep
  working in the app; they are sent only to that same site and deleted when the download ends;
- run the content script for the hover button and the "Download all links" picker (it reads
  link addresses only, in a closed shadow root);
- on Firefox, read response headers (webRequest) to recognise a download before it starts,
  because Firefox has no downloads.onDeterminingFilename.

PRIVACY
Collects nothing. Policy: https://layanjarrar1.github.io/Riptide/privacy/
```
