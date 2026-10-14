# Riptide

A download manager for Windows, written in Rust (engine) and Tauri v2 + Angular (desktop app).
Arabic and English. Work in progress.

Crates: `riptide-core` (model), `riptide-net` (HTTP engine), `riptide-store` (SQLite),
`riptide-engine` (queue + runtime shared by the app and the CLI), `riptide-ipc`, `riptide-log`,
`riptide-cli` (`riptide fetch` / `riptide bench`), `riptide-testserver` (dev only).

## Build

Prerequisites: stable Rust (`rustup`), Node 20+, Angular CLI, Visual Studio Build Tools (C++),
WebView2 (ships with Windows 10/11).

```powershell
# the gate — everything must be green before a PR merges
.\scripts\check.ps1

# run the local test server
cargo run -p riptide-testserver
# then open http://127.0.0.1:8080/ok?size=1048576

# run the desktop app in dev mode
cd apps\desktop
npm install
npm run tauri dev
```

## License

The desktop app, CLI, IPC crate and browser extension are licensed under **GPL-3.0-or-later**
([`LICENSE`](LICENSE)). The engine crates (`riptide-core`, `riptide-net`, `riptide-store`,
`riptide-log`, `riptide-testserver`) are dual-licensed **MIT OR Apache-2.0**
([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)).
