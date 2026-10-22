# Implementation progress — desktop experience upgrade

Checkpoint for the multi-phase upgrade (downloads table, folders, add dialog, file manager, AI
research). Update this file at the end of every working session.

Branch: `claude/clever-lovelace-dvz0r8`

## Phase 1 — main download experience

| # | Feature | State | Notes |
| --- | --- | --- | --- |
| 1 | True virtual scrolling | Verified, regression-tested | `main` already renders only the visible rows plus 6 overscan on each side (measured: 19–26 DOM rows at 100–50,000 downloads, `apps/desktop/e2e/list_perf.md`). `downloads-page.layout.spec.ts` now asserts the bound at 100 / 1k / 10k / 50k while scrolling, sorting, searching and with details open. |
| 2 | Resizable columns | Done | `data/column-widths.ts` (tracks, limits, persistence under `riptide.columnWidths`), grips in the page header, keyboard resizing, per-column and global reset, sideways scrolling with a synced header. |
| 3 | Duration and average speed | Not started | Needs `active_ms` accumulated by the engine (exclude paused time), a store migration, bridge type, and two new columns. Old rows show "—" (no fabricated history). |
| 4 | Details panel responsiveness | Done | Docks when the list keeps ≥ 600 px, otherwise overlays with a scrim. |
| 5 | Global search | Improved | Search box already in the top bar; now matches status/category names (en/ar), folds Arabic, debounced for ≥ 5,000 rows. |

## Phases 2–4

Not started: categorized folders, Add Download redesign / quality dropdown, file manager and
dual pane, AI research chat (Angular 20 app — Astryx targets React 19, so its patterns would be
reproduced in Angular rather than installed).

## Test evidence

- `npm test` (Karma, ChromeHeadless, Linux): 540 passed, 1 failed. The failure is
  `BatchDownloadDialogComponent scrolling moves the window and keeps the spacer math right`, which
  fails the same way on unmodified `main` in this Linux container (layout-dependent).
- `npm run lint`, `npm run i18n:lint`: clean.
- CI (`check.ps1` on windows-latest): see the pull request.

## Next action

Implement the active-duration and average-speed metadata (Phase 1, item 3), starting from
`crates/riptide-core/src/download.rs` (`Download`) and `crates/riptide-store/src/store.rs`.
