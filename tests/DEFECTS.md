# Defect log: download compatibility and integrity matrix

Severity: P0 = silent wrong bytes or data loss, P1 = secrets / security rule broken or a failure
with no way out, P2 = wrong state or poor behaviour that has a workaround.
Method: for each failure the cause was found first (reproduced, traced to the line, fixed once at
the root, then the targeted test and the whole affected suites re-run), following the
`systematic-debugging` skill. The `security-best-practices` skill covers Python / JS / Go only, so
for the Rust code its principles were applied by hand: no secret in storage or logs (D-01), treat
every server value as hostile (D-04), least surprise on shared state (D-02).

## Product defects (fixed)

| ID | Sev | Symptom | Root cause | Fix commit | Test |
|---|---|---|---|---|---|
| D-01 | P1 | `http://user:password@host/file` stored the password in clear text in `downloads.url` and `final_url` (and in `probe()` output); `strip_query` kept it in error messages. Breaks hard rule 4. | `prepare_add` copied `request.url` verbatim; credentials in the URL were never moved to the `secrets` table. | `a71159f` | `matrix_network::credentials_in_the_url_move_to_secrets_and_never_stay_in_the_record`, `redirect::tests::userinfo_is_split_off_and_redacted_from_logs` |
| D-02 | P0 | Eight downloads that resolve to the same name at the same moment: one `.rtpart` shared by all, the file reported `Completed` held another download's bytes (size 102 000 instead of 100 000, wrong SHA-256). | Name uniqueness (`unique_path`) only looked at the final file, and every probe ran concurrently: all saw "free". | `3ecacc2` | `matrix_names::simultaneous_same_name_downloads_get_distinct_files` (red before the fix) |
| D-03 | P2 | `pause()` then an immediate `resume()` left the download `Paused` forever (resume ignored because the state was still `Downloading`). Showed up as a flaky random pause/resume test. | `pause`/`cancel` only signalled the task and returned. | `3ecacc2` | `matrix_local::pause_returns_paused_and_an_immediate_resume_is_not_lost`, `::pause_and_resume_at_random_offsets_is_byte_exact` |
| D-04 | P0 | jsDelivr (`jquery.min.js`) was saved as 31 402 of 87 533 bytes and reported `Completed`. Found by the first real run of the public-source runner. | Cloudflare answers `Range` with `Content-Range: bytes 0-31401/31402` (the compressed size) but sends the whole 87 533-byte body. The segment task stopped at the announced end and never noticed the extra bytes. | `978df0e` | `matrix_network::server_sending_more_bytes_than_its_content_range_is_downloaded_whole` (red before the fix: 333 333 of 1 000 000 bytes), public source `jsdelivr-jquery` |
| D-05 | P2 | After such a restart, or after any download of unknown length, the record kept the probed size (or none) instead of the file's size. | The engine never reconciled `size` with the finished file. | `978df0e` | `matrix_sizes::tiny_sizes_through_every_server_mode`, `matrix_network::complete` |
| D-06 | P2 | A link without a file extension and with Content-Type `text/csv`, `application/xml`, `image/svg+xml`, `audio/wav`, EPUB, DOCX, XLSX, PPTX or MSIX was saved with no extension, so it landed in the wrong category and could not be opened. | `extension_for` knew only 20 types. | `f3c53aa` | `matrix_names::names_types_and_categories_are_sanitised_and_bytes_exact` (the `noext-*` rows) |
| D-07 | P0 | (Opus review, repro with release binaries) After the Content-Range fallback a pause, crash or kill made the resume declare a short, zero-filled file `Completed` (3 GB source, 1 GB file, "Done in 0.0 s"). Introduced by the D-04 fix. | The fallback kept its decision in memory only; the checkpointed streaming segment `[0, W)` looked complete, and `can_resume` accepted any non-empty table. | next commit "Content-Range fallback survives pause/kill" | `transfer::tests::only_a_table_covering_exactly_the_size_is_resumable`, `::streaming_progress_is_never_persisted`, `matrix_network::pause_and_resume_around_the_content_range_fallback_is_byte_exact` (red on the old code: 3 145 728 of 8 388 608 bytes), `kill9::killed_during_the_content_range_fallback_restarts_clean` |
| D-08 | P1 | (Opus review) An excess smaller than the last chunk (1 byte to ~64 KiB, e.g. lie=999900) was sliced away silently and the file finished truncated. | `pump` cut the chunk to the announced end without treating it as an overrun. | same commit | `matrix_network::server_sending_more_bytes_than_its_content_range_is_downloaded_whole` now covers excesses 1, 2, 100, 4096, 65 535, 65 537, ... (red on the old code: 999 999 of 1 000 000) |
| D-09 | P2 | (Opus review) `pause`/`cancel` removed the task from `running` before it stopped: `refresh_url`/`remove` during a pause raced the old task, `pause_all` waited for each task so freed slots started (probed) queued items, and the wait had no timeout. | D-03's fix used `take_running` + `await`. | next commit "pause/cancel keep the running entry" | `matrix_local::pause_all_with_a_long_queue_never_probes_queued_items` (red before), `::refresh_during_pause_leaves_a_consistent_state`, `::remove_while_pause_in_flight_then_same_name_add_is_byte_exact` (these two pass on the old code too in this environment: the race window is too narrow to hit reliably; kept as regression guards) |
| D-10 | P3 | (Opus review) Name claims compared folders textually (case, separators); a name ending in `.rtpart` could equal another download's part file; CLI `fetch` stored `user:password@` in the shared table. | see review P2-2 / P2-6 | same commit | `matrix_local::rtpart_names_are_reserved_and_folder_spelling_does_not_split_claims` (red before), `kill9::fetch_with_userinfo_never_stores_the_password` |

| D-11 | P1 | (Opus re-review R-1) A kill between the fallback restart (which deletes the part file) and the streaming run's first save left the old, complete-looking table: the resume declared a short or zero-filled file Completed (524 288 of 1 000 000 bytes). | The reset was only in memory; nothing invalidated the stored table before the file was deleted. | this commit | `fallback_kill::kill_right_at_the_fallback_restart_then_resume_is_byte_exact` (red on the old code: 524 288 bytes) |
| D-12 | P1 | (R-2, pre-existing) A deleted, truncated or extended `.rtpart` was silently re-created and a "Done" file with a zero-filled head was reported. | `PartFile::open` creates a missing file and `can_resume` never looked at it. | this commit | `fallback_kill::deleted_truncated_or_extended_part_file_restarts_instead_of_completing` (red on old: wrong SHA-256), `matrix_local::damaged_part_file_between_pause_and_resume_restarts_byte_exact` |
| D-13 | P2 | (R-5) A stop that outlasted the 10 s wait returned an error but left the record Downloading with no task for remove/restart. | The timeout path changed nothing. | this commit | `matrix_local::a_stuck_stop_times_out_into_a_paused_record_and_recovers` (test knobs; not run against the old code, which has no knob) |
| D-14 | P3 | (R-6) `pause_all` paused queued items from a stale list. | | this commit | re-read right before the change; covered by `pause_all_with_a_long_queue_never_probes_queued_items` |

Behaviour change worth a reviewer's eye (D-02): a download that is queued, paused or failed now
*claims* its file name. A newcomer with the same name yields and gets `name (1).ext`, so the first
download keeps `name.ext`. `runtime::find_duplicate_matches_the_url_without_fragment_or_the_name_in_the_folder`
was adjusted to this (before, the paused one yielded when the newcomer finished first).

## Test-environment and external issues (not product defects)

| ID | What happened | Resolution |
|---|---|---|
| T-01 | First runs of the new tables failed on wrong expectations of mine: a leading dot is trimmed on purpose (`.hidden` becomes `hidden`); in a quoted `filename` a backslash is a quote escape (RFC 6266), so `..\..\x.exe` becomes `Windowsx.exe`, always inside the folder; two table rows resolved to the same name (`blob`, the punctuation row) and the engine correctly added ` (1)`; `/redirect?n=20` is 21 redirect responses (the last hop points at the file) so the limit is `n=19`; a checksum failure keeps its `.rtpart` for inspection (documented design). | Expectations corrected, rationale in comments. |
| T-02 | `/expires` does not cut running bodies, so a plain slow download never expired. | The test pauses, waits past the expiry and resumes; the test server got an optional `?bps=`. |
| T-03 | The machine ran out of disk twice while building test binaries (LNK1318 / LNK1140, debug PDBs of 20 test executables). | Not product related. Later builds used `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0`. |
| T-04 | `more_than_four_gibibytes` first failed on my assertion (a segment *starting* beyond 2^32; the planner's last segment starts before it and *ends* beyond it). The download itself was byte-exact. | Assertion changed to "a segment ends beyond `u32::MAX`". |
| T-05 | jsDelivr's compressed-size `Content-Range` is an external-server quirk, but the product must not trust it, hence D-04. | Fixed in the product; the quirk is emulated by `/range-lie`. |

## Observations (no change made)

- Error text for local failures is raw OS text prefixed with `io:` (for example `io: The system
  cannot find the path specified. (os error 3)`). Accurate, not friendly; a UI mapping would help.
- When the destination `.rtpart` is locked by another program the download fails (retryable) and
  resumes cleanly once the lock is gone; it does not pick another name.
- Disk full could not be simulated (no write-injection seam, VHD needs administrator rights).
- A one-byte file served with an honest `Content-Range` cannot be told apart from a lying server
  (nothing follows the byte); the guard is for ranges longer than one byte.

## Flakiness

No test is quarantined. One flake appeared (random pause/resume, about 1 run in 4 before the fix)
and was traced to D-03; after the fix 20+ consecutive runs of `matrix_local` passed. The random
test prints its seed and accepts `RIPTIDE_SEED` to replay the input sequence (timings still vary).
