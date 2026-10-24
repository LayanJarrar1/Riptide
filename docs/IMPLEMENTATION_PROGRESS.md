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

Phase 2 merged in PR #67.

## Phase 3 — file manager

| # | Feature | State | Notes |
| --- | --- | --- | --- |
| 9 | Basic file operations | Done | `crates/riptide-fs` + `src-tauri/src/files.rs` commands + `features/files/`. |
| 10 | Folder colours | Done | `FolderColorsService`, preference `riptide.folderColors`, follows renames and moves. |
| 11 | Dual pane | Done | `FilesPageComponent` hosts two `FilePaneComponent`s; layout in `riptide.files.layout`. |
| 12 | Drag and drop | Done | `application/x-riptide-files` between panes and onto folders, crumbs and places; Ctrl copies. |

Needs a real Windows session to confirm: Recycle Bin restore through the shell, moves across
drives, locked files (another program has them open), OneDrive placeholder files.

## Phase 4 — AI Research workspace

Branch `m6/ai-research-workspace`. Astryx targets React 19, so its patterns are reproduced in Angular
(decision D-research-ws-1).

| # | Feature | State | Notes |
| --- | --- | --- | --- |
| 13 | Conversations | Done | `research-history.ts` (models, caps, defensive parser), `research-history.service.ts` (`PREFERENCE_STORE`, key `riptide.research.history.v1`), `research-sidebar.component.ts` (new, rename, delete, clear all with confirmation). 30 conversations, 40 turns, 60 results; stored without local paths. |
| 14 | Turns and progress steps | Done | `research-turn.ts` derives the five steps from the real events (plan, sources, rank, seen matches, end); a step that never ran is `skipped`. `research-turn.component.ts` renders the turn from a stored `Turn`, so live and restored turns look the same. `ResearchSession` is unchanged except for `watch()`, which the page uses to mirror it into the stored turn. |
| 15 | Result cards | Done | `research-result-card.component.ts`: type, source, description, size, year, licence, why, seen badge, open page (`open_web_url`), download. Text only, `safeWebUrl` for every link. |
| 16 | Turn actions | Done | Download selected / one, select all, copy response, save Markdown, BibTeX and RIS (`research-export.ts`), cancel, retry source, search without AI, try again. |
| 17 | Composer | Done | Enter / Shift+Enter (IME-safe), kind of result, AI switch, provider and model chip linking to Settings → AI. |
| 18 | Token efficiency | Done | A turn sends `{ topic, useAi, lang, category }` only; an identical prompt within 10 minutes reuses the results (no search, no AI call). |
| 19 | Backend | Done | `research_start` no longer clears the result cache; `open_web_url`; `save_text_file(kind)`. |

Known limits: a turn restored after a restart shows its results but cannot be downloaded until it is
searched again (the backend cache is in memory); the downloads of the 60 kept results only.

## Phase 5 — security review and checks

Branch `m6/phase5-security-review` (stacked on Phase 4). Reviewed the Phase 1–4 diffs:

- Secrets and personal paths: no API keys, tokens, e-mail addresses or Windows user-profile paths in the
  files added by PRs #66–#68 or in tracked files (only a fake key inside a unit test). Research
  history is stored in app storage, never in the repository, and without local paths.
- External content in Research: results are text-only (a spec injects `<img onerror>` and `<b>`),
  every link goes through `safeWebUrl`, `open_web_url` accepts http/https only, stored history is
  re-validated on load (bad links and unknown sources are dropped).
- `riptide-fs` / `files.rs` path handling — five findings fixed, each with a test: into-itself check
  on resolved, case-folded paths; same-item test case-insensitive on Windows; a move across drives
  keeps the original when links were not copied; source names validated (alternate data streams);
  bidi override characters, `CONIN$`/`CONOUT$` and superscript `COM`/`LPT` names refused.
- Left as documented limits (crate docs, "Known limits"): the time between an "exists" check and a
  rename; no allowed-roots restriction (a file manager can touch any path the user can).
- README feature list and CHANGELOG updated.

Manual checks still needed on a real Windows session (nothing below was run in the desktop app):
Recycle Bin restore through the shell, moves across drives, files another program has open,
OneDrive placeholder files, the native save dialog for `.md` / `.bib` / `.ris`, `open_web_url`
opening the browser, the Research page in RTL in the running app.

## Test evidence

- `npm test` (Karma, Chrome headless, Windows Server 2022, Phase 4 branch): 599 passed, 0 failed
  (the layout-dependent `BatchDownloadDialogComponent scrolling…` passes on Windows).
- `npm run lint`, `npm run i18n:lint`: clean.
- CI: GitHub Actions still ends every run in `startup_failure` (checked 2026-10-24), so no CI result exists for these PRs. Bundle: 500.33 kB initial (warning, budget 500 kB).

Rust (Windows): `cargo fmt --all -- --check` clean; clippy `-D warnings` and tests pass for
`riptide-desktop` (99 tests) and `riptide-fs` (18 tests). Earlier, on Linux: core/store/engine clean. `riptide-engine`: one failure,
`read_only_destination_fails_cleanly_then_works_once_writable`, because the container runs as root
(a read-only directory does not stop root); Windows CI is the reference.

## Next action

Merge the Phase 4 and Phase 5 pull requests, then walk through the manual checks below on Windows.
