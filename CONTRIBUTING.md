# Contributing to Riptide

Thanks for looking under the hood. Riptide is in early development, so the most useful
contributions right now are bug reports with reproductions, fixes with tests, and translation
review. This page explains how the project is built, what "done" means here, and how a change
gets in.

## Table of contents

- [Prerequisites](#prerequisites)
- [Getting the code running](#getting-the-code-running)
- [The gate: `scripts/check.ps1`](#the-gate-scriptscheckps1)
- [Project layout](#project-layout)
- [Conventions](#conventions)
- [Translations](#translations)
- [Branches, commits and pull requests](#branches-commits-and-pull-requests)
- [Reporting bugs](#reporting-bugs)

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | stable, via [`rustup`](https://rustup.rs) | `rust-toolchain.toml` pins the channel and adds `rustfmt` + `clippy` |
| Node.js | 20 or newer | for the Angular app and the website build script |
| Visual Studio Build Tools | 2022, "Desktop development with C++" | the MSVC linker, needed by Rust on Windows |
| WebView2 | ships with Windows 10 / 11 | the desktop app's web view |
| Git | any recent | |

No admin rights are needed for anything in this repository, and nothing it builds asks for them.

## Getting the code running

```powershell
git clone https://github.com/LayanJarrar1/Riptide.git
cd Riptide

# 1. Build everything once (the first build downloads and compiles dependencies)
cargo build

# 2. Start the local test server (a fake origin with every failure mode the engine handles)
cargo run -p riptide-testserver
# it listens on http://127.0.0.1:8080 ; try http://127.0.0.1:8080/ok?size=104857600

# 3. Download something with the command-line client (no app needed)
cargo run -p riptide-cli -- fetch "http://127.0.0.1:8080/ok?size=104857600" -o C:\tmp\test.bin
# kill it mid-way and run the same command again: it resumes where it stopped

# 4. Run the desktop app in development mode
cd apps\desktop
npm install
npm run tauri dev
```

`npm run tauri dev` starts the Angular dev server on port 4200 and opens the Tauri window with
the real engine. Opening <http://localhost:4200> in an ordinary browser also works: there the UI
runs on a built-in simulation of the engine, which is handy for UI work and is what the unit
tests use.

The app keeps its data in `%LOCALAPPDATA%\Riptide` (database, logs). If a file called
`portable.txt` sits next to the executable, the app runs in portable mode and keeps everything in
a `data` folder next to it instead.

## The gate: `scripts/check.ps1`

Every change must leave the gate green. Run it from the repository root:

```powershell
.\scripts\check.ps1
```

It runs, in order:

1. `npm run i18n:lint` – every UI string exists in both `ar.json` and `en.json`
2. `npm run lint` – ESLint on the Angular app (strict TypeScript, no `any`)
3. `npm run build` – production build of the app
4. `npm test` – Karma unit tests in headless Chrome
5. `cargo fmt --all -- --check`
6. `cargo build --release`
7. `cargo clippy --all-targets -- -D warnings`
8. `cargo test` for the workspace and the desktop crate
9. a check that the generated TypeScript bindings in `apps/desktop/src/app/tauri/bridge/`
   match the Rust types (regenerate with the command the script prints if they do not)
10. `tsc --noEmit` and `eslint` for the browser extension in `apps/extension`
11. `node scripts/check-links.mjs` (relative links and anchors in the Markdown files) and the tests
    of the release scripts (`node --test scripts/set-release-version.test.mjs`)

The engine tests spawn `riptide-testserver` in-process; nothing in the test suite touches the
network.

## Project layout

```
crates/
  riptide-core/        download model, state machine, segment planner, filename sanitiser (pure)
  riptide-net/         HTTP engine: probe, ranges, multi-connection transfer, retry, auth
  riptide-store/       SQLite persistence (WAL) and migrations
  riptide-engine/      the runtime: queues, scheduler, batch, crash recovery, event stream
  riptide-media/       video pages through the bundled yt-dlp
  riptide-crawl/       the site grabber
  riptide-host/        Native Messaging host for the browser extension
  riptide-ipc/         local pipe transport and message schema
  riptide-log/         logging with secret redaction, installed and portable paths
  riptide-cli/         `riptide fetch | batch | grab | bench | ping | paths`
  riptide-testserver/  local HTTP server with every failure mode the tests need (dev only)
apps/
  desktop/src-tauri/   Tauri shell: commands, event bridge, engine lifetime
  desktop/src/         Angular UI
  extension/           the browser extension (Manifest V3)
  website/             the public site (static, bilingual)
scripts/               check.ps1 (the gate), release.ps1, publish-release.ps1, verify-update-feed.ps1,
                       set-release-version.mjs, check-links.mjs
```

[`ARCHITECTURE.md`](ARCHITECTURE.md) explains how the pieces fit and which guarantees the engine
makes.

## Conventions

**Rust**

- Edition 2021, stable toolchain, `tokio`. Libraries use `thiserror`; binaries use `anyhow`.
- `unwrap()`/`expect()` are denied outside tests (`clippy::unwrap_used`). Return errors.
- No blocking IO on the async runtime: hashing, directory scans and anything disk-bound go
  through `spawn_blocking`.
- `tracing`, never `println!`, and nothing secret in a log line (cookies, headers, credentials,
  query strings).
- Every public type and function is documented. `cargo doc` warnings are treated as bugs.
- Platform-specific code lives in small `cfg(windows)` modules; the engine crates must stay
  portable.

**Angular**

- Standalone components, signals, `OnPush`, strict TypeScript, no `any`, no third-party component
  libraries. Our own components sit on the tokens in `src/styles/tokens.css`.
- Every list is virtual-scrolled. Every screen works in both text directions.
- Components talk to the engine through the abstract `DownloadEngine` only.

**Tests**

- A bug fix comes with the test that would have caught it.
- Engine behaviour is tested against `riptide-testserver` (`/drop`, `/flaky`, `/etag-changes`,
  `/wrong-length`, `/corrupt`, …). Anything that touches resume compares the SHA-256 of the result
  with the source.
- Property tests (`proptest`) cover the segment planner and the filename sanitiser.

## Translations

Arabic and English are both first-class. Every user-facing string in the app lives in
`apps/desktop/src/i18n/ar.json` and `en.json` under the same key; `npm run i18n:lint` fails
when a key is missing on either side. The website keeps its strings in
`apps/website/src/i18n/`. When you add a string, add it to both languages in the same change,
and keep the Arabic natural rather than literal.

## Branches, commits and pull requests

- `main` is always green and always buildable. Work happens on short-lived branches named
  `area/short-description`, for example `net/digest-auth`, `desktop/details-drawer`,
  `site/faq`.
- Commit messages follow `area(scope): what changed`, in the imperative, under 72 characters:
  `net(transfer): split the largest remaining segment when a connection finishes`. Short
  area tags such as `desktop(ui): …` are fine too.
- Open a pull request against `main`. The template asks for three things: what changed, why,
  and how you verified it (the gate result, and a manual test note when a screen changed).
  Screenshots in both languages are expected for UI changes.
- Keep pull requests focused. A refactor and a behaviour change are two pull requests.
- Design decisions that change an established choice (storage, HTTP stack, UI framework,
  install model) need a short written rationale in the pull request so the reasoning survives.

## Publishing an update

Maintainers only. Every number below is a command you can paste. The app's update path is
`apps/desktop/src-tauri/src/updater.rs`: stable reads `releases/latest/download/latest.json`, beta
reads `releases/download/channel-beta/latest-beta.json`. Every beta is a pre-release, so while we
are in beta only the beta channel sees new versions, and beta is the app's default channel.

1. **Bump and build.** From a clean `main`:

   ```powershell
   git switch main; git pull
   git switch -c release/<version>
   .\scripts\release.ps1 -Version <version> -Prerelease -Notes "One line for Settings > About"
   ```

   This bumps the workspace `Cargo.toml`, `Cargo.lock` and `apps/desktop/package.json`, builds the
   signed NSIS installer, its `.sig` and the portable zip, and writes `latest-beta.json` (plus
   `latest.json` for a stable version) and `SHA256SUMS.txt` into
   `target\release-artifacts\<version>\`. It publishes nothing. Add the release to
   `CHANGELOG.md` and write the release notes (a Markdown file outside the repository).
2. **Test the portable build.** Unzip `Riptide-<version>-portable.zip` somewhere else, start
   it, add a download, pause and resume it, and check *Settings → About* shows the new version.
   Also install the installer over the previous release once and confirm the downloads list survived.
3. **Merge.** Commit the version bump and `CHANGELOG.md`, open a PR, wait for the `ci` check, merge
   it, then `git switch main; git pull`.
4. **Publish.** The script refuses a dirty tree, a HEAD that is not on `origin/main` and artifacts
   that do not match `SHA256SUMS.txt`. Without `-Publish` it is a dry run:

   ```powershell
   .\scripts\publish-release.ps1 -Version <version> -NotesFile C:\path\to\notes.md
   .\scripts\publish-release.ps1 -Version <version> -NotesFile C:\path\to\notes.md -Publish
   ```

   It creates the release `v<version>` (a pre-release because the version has a `-`) with the
   installer, `.sig`, portable zip, `SHA256SUMS.txt`, `latest-beta.json` (and `latest.json` for
   stable), uploads `latest-beta.json` to the fixed pre-release `channel-beta` with `--clobber`,
   and then runs the next step. Pushing a `v*` tag instead lets
   `.github/workflows/release.yml` do the same on GitHub's runners.
5. **Verify the feed.** This is the proof that people on an older version will be notified. It
   fetches the public feed without any token, validates it, checks the installer URL answers 200
   and that the feed announces the new version:

   ```powershell
   .\scripts\verify-update-feed.ps1 -ExpectVersion <version>
   ```

   Add `-Stable` for a stable release. A non-zero exit means nobody will be told about the update
   until you fix it.
6. **Site and README.** Point the download links at the new version, then review and merge:

   ```powershell
   node scripts/set-release-version.mjs <version>
   git switch -c site/<version>
   git add README.md apps/website/build.mjs
   git commit -m "docs: download links for <version>"
   ```

   `node scripts/set-release-version.mjs <version> --check` exits 1 when they still differ.
7. **Back up the signing key.** Keep a copy of `%USERPROFILE%\.riptide-keys` (the key and its
   password file) in a password manager or offline storage, never in the repository. Losing the
   key means installed copies can no longer update themselves.
8. **How users are told.** Riptide checks the beta feed at most once a day (and on *Check for
   updates*), shows a system notification once per version and a badge in *Settings → About*, and
   installs only after a click. While we are in beta the default channel is beta. At the first
   stable release, publish it without `-Prerelease` (so `latest.json` exists), run
   `verify-update-feed.ps1 -Stable`, and switch the default channel to stable in the app.

**If an update is bad.** Mark the release as a draft on GitHub (*Edit → Set as a draft*, or
`gh release edit v<version> --draft`) so the installer link stops working, then restore the
feed of the previous good version: download the previous release's `latest-beta.json` from
`target\release-artifacts\<previous>\` (or re-create it with `release.ps1`'s output) and run

```powershell
gh release upload channel-beta target\release-artifacts\<previous-version>\latest-beta.json --clobber --repo LayanJarrar1/Riptide
.\scripts\verify-update-feed.ps1 -ExpectVersion <previous-version>
```

Never reuse a version number: publish the fix as the next version. Users who already installed the
bad build get the fix through the normal update.

Other release facts:

- **Signing key.** The updater key pair lives outside the repository
  (`%USERPROFILE%\.riptide-keys\riptide-updater.key`, its password in `riptide-updater.password`
  next to it); only the public key is in `tauri.conf.json`. CI reads it from the
  `TAURI_SIGNING_PRIVATE_KEY` secret.
- **Privacy.** The update check is the only network call that is not a download. It runs when the
  user clicks *Check for updates*, or at most once a day while *Check for updates automatically*
  is on (Settings → About), and installing always waits for a click.
- **Uninstall.** `src-tauri/windows/hooks.nsh` removes the Native Messaging registry keys and
  manifests and the autostart value, and asks whether to keep the download history and settings
  (silent uninstalls keep them). Downloaded files are never touched.

## Reporting bugs

Use the bug report template. The most helpful reports include the URL (or a description of the
server when the URL is private), what you expected, what happened, and the relevant lines from
`%LOCALAPPDATA%\Riptide\logs`. Logs never contain cookies, credentials or query strings, but
read them before pasting anyway.

Security issues go through [`SECURITY.md`](SECURITY.md), not the public tracker.
