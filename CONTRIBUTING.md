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

The app keeps its data in `%LOCALAPPDATA%\Riptide` (database, logs). If a folder called `data`
exists next to the executable, the app runs in portable mode and uses that instead.

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

The engine tests spawn `riptide-testserver` in-process; nothing in the test suite touches the
network.

## Project layout

```
crates/
  riptide-core/        download model, state machine, segment planner, filename sanitiser (pure)
  riptide-net/         HTTP engine: probe, ranges, multi-connection transfer, retry, auth
  riptide-store/       SQLite persistence (WAL) and migrations
  riptide-engine/      the runtime: queue, scheduler, crash recovery, event stream
  riptide-ipc/         local pipe transport and message schema
  riptide-log/         logging with secret redaction, installed and portable paths
  riptide-cli/         `riptide fetch | bench | ping | paths`
  riptide-testserver/  local HTTP server with every failure mode the tests need (dev only)
apps/
  desktop/src-tauri/   Tauri shell: commands, event bridge, engine lifetime
  apps/desktop/src/    Angular UI
  website/             the public site (static, bilingual)
scripts/check.ps1      the gate
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
  `net(transfer): split the largest remaining segment when a connection finishes`. The
  maintainers' own commits carry an internal lane tag instead of an area (`m1b(desktop): …`);
  either form is fine.
- Open a pull request against `main`. The template asks for three things: what changed, why,
  and how you verified it (the gate result, and a manual test note when a screen changed).
  Screenshots in both languages are expected for UI changes.
- Keep pull requests focused. A refactor and a behaviour change are two pull requests.
- Design decisions that change an established choice (storage, HTTP stack, UI framework,
  install model) need a short written rationale in the pull request so the reasoning survives.

## Reporting bugs

Use the bug report template. The most helpful reports include the URL (or a description of the
server when the URL is private), what you expected, what happened, and the relevant lines from
`%LOCALAPPDATA%\Riptide\logs`. Logs never contain cookies, credentials or query strings, but
read them before pasting anyway.

Security issues go through [`SECURITY.md`](SECURITY.md), not the public tracker.
