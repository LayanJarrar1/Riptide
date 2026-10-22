# Implementation progress — desktop experience upgrade

Checkpoint for the multi-phase upgrade (downloads table, folders, add dialog, file manager, AI
research). Update this file at the end of every working session.

Branch: `claude/clever-lovelace-dvz0r8` (restarted from `main` after each merge)

## Phase 1 — main download experience

| # | Feature | State | Notes |
| --- | --- | --- | --- |
| 1 | True virtual scrolling | Verified, regression-tested | `main` already renders only the visible rows plus 6 overscan on each side (measured: 19–26 DOM rows at 100–50,000 downloads, `apps/desktop/e2e/list_perf.md`). `downloads-page.layout.spec.ts` now asserts the bound at 100 / 1k / 10k / 50k while scrolling, sorting, searching and with details open. |
| 2 | Resizable columns | Done | `data/column-widths.ts` (tracks, limits, persistence under `riptide.columnWidths`), grips in the page header, keyboard resizing, per-column and global reset, sideways scrolling with a synced header. |
| 3 | Duration and average speed | Done | Schema v6 (`active_ms`, `active_since`); spans opened/closed in `Store::set_state`, checkpointed with segment saves, closed on shutdown, abandoned after a crash. Optional columns + details drawer. Old rows stay `NULL` → "—". Limitation: a torrent still downloading when the app quits resumes without an open span until its next state change. |
| 4 | Details panel responsiveness | Done | Docks when the list keeps ≥ 600 px, otherwise overlays with a scrim. |
| 5 | Global search | Improved | Search box already in the top bar; now matches status/category names (en/ar), folds Arabic, debounced for ≥ 5,000 rows. |

Phase 1 merged in PR #66.

## Phase 2 — download management

| # | Feature | State | Notes |
| --- | --- | --- | --- |
| 6 | Category folders | Done | `EngineSettings.organize` / `organize_root`, `dir_for` / `torrent_dir`, `"torrents"` key in `category_dirs`; first-run folder creation behind `EngineConfig::organize_new_install` (desktop only); `Category::detect` adds the `Content-Type` fallback. Settings UI with restore defaults. |
| 7 | Add Download redesign | Done | Standard-width dialog, compact video card, "More options" with a summary of non-default choices. |
| 8 | Best quality by default | Done | Quality dropdown of the engine's detected choices; Best preferred explicitly. |

## Phases 3–4

Not started: file manager and dual pane, AI research chat (Angular 20 app — Astryx targets
React 19, so its patterns would be reproduced in Angular rather than installed).

## Test evidence

- `npm test` (Karma, ChromeHeadless, Linux): 548 passed, 1 failed. The failure is
  `BatchDownloadDialogComponent scrolling moves the window and keeps the spacer math right`, which
  fails the same way on unmodified `main` in this Linux container (layout-dependent).
- `npm run lint`, `npm run i18n:lint`: clean.
- CI (`check.ps1` on windows-latest): see the pull request.

Rust (Linux container): `cargo fmt --check` and clippy `-D warnings` on core/store/engine clean;
`riptide-store` and `riptide-core` tests pass. `riptide-engine`: one failure,
`read_only_destination_fails_cleanly_then_works_once_writable`, because the container runs as root
(a read-only directory does not stop root); Windows CI is the reference.

## Next action

Phase 3, item 9: file manager basics. Needs new Tauri commands for listing, rename, copy, move,
delete-to-Recycle-Bin and properties (path-checked against traversal), then an Angular page.
