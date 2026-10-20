# Download compatibility and integrity matrix

Maps every requirement of the matrix to the test that proves it. "Existing" tests were already in
the repository; "new" tests were added with the matrix. Everything runs against the in-process
`riptide-testserver` (no network in CI) and compares the SHA-256 of the result with the server's
deterministic bytes, unless stated otherwise.

Paths: `E` = `crates/riptide-engine/tests`, `N` = `crates/riptide-net/tests`,
`C` = `crates/riptide-cli/tests`. Counts and the latest results are in the PR description.

Every completed download in the new engine tests goes through `assert_complete`
(`E/common/mod.rs`): size, SHA-256, final path inside the chosen folder, state `Completed`, every
segment done, no `*.rtpart` left in the folder.

## 1. File names, types, categories

| Requirement | Test | Status |
|---|---|---|
| zip rar 7z tar gz exe msi msix iso pdf docx xlsx pptx txt csv json xml jpg png webp gif svg mp3 wav mp4 mkv webm epub: final name, category folder, bytes | `E/matrix_names.rs::names_types_and_categories_are_sanitised_and_bytes_exact` (92 rows, table driven) | new |
| Hostile Content-Type must not change behaviour (`text/html` on an `.mkv`, empty type) | same table | new |
| Extensionless names, extension added from Content-Type (24 types) | same table | new (mapping extended: csv xml svg wav epub doc docx xlsx pptx msix) |
| Spaces `& # % ; ' ( ) [ ] { } @ + = , ! ^ ~` in the URL path and in Content-Disposition | same table | new |
| Forbidden characters (`" < > \| ? *`) dropped, quoted-string escapes | same table | new |
| Unicode / Arabic via `filename*=UTF-8''`, via the URL path, CJK, emoji, ISO-8859-1 `filename*`, NFD to NFC | same table | new; Arabic CD also in `C/kill9.rs::arabic_name_from_content_disposition_lands_on_disk`, `E/runtime.rs::category_folders_decide_where_a_request_without_a_dir_lands` |
| Names > 200 bytes (ASCII and Arabic) | same table | new |
| Hostile: `..`, `/`, `\`, `C:\`, `CON`, `NUL.tar.gz`, `COM1`, `lpt9`, `AUX .txt`, trailing dots/spaces, control chars | same table; `E/runtime.rs::evil_content_disposition_lands_with_a_safe_name_inside_the_folder`; proptests in `riptide-core` (`filename.rs`) | existing + new |
| Request-supplied name override is sanitised too | `E/matrix_names.rs::a_request_name_override_is_sanitised_too` | new |
| Same name requested by 8 downloads at once: distinct files, right bytes | `E/matrix_names.rs::simultaneous_same_name_downloads_get_distinct_files` | new (found D-02) |
| Engine does not behave differently per extension | all rows share one code path and one assertion set | new |

Note on `\` in a quoted `filename`: RFC 6266 treats it as a quote escape, so `"..\..\evil.exe"`
becomes `Windowsevil.exe` style names, always inside the folder. Documented in the table.

## 2. Sizes

| Requirement | Test | Status |
|---|---|---|
| 0 B, 1 B, 2/7/8/9 B, 1 KiB, 1 KiB+1, 64 KiB, 1 MiB-1, 1 MiB, 1 MiB+1 through `/ok`, `/noranges`, `/nolength` | `E/matrix_sizes.rs::tiny_sizes_through_every_server_mode` | new |
| 10 MiB (ranges and no ranges), 100 MiB | `E/matrix_sizes.rs::ten_and_hundred_mebibytes_are_byte_exact` | new |
| 1 GiB (`RIPTIDE_BIG_TESTS=1`, streamed both ends) | `E/matrix_sizes.rs::one_gibibyte_is_byte_exact` | new, opt-in |
| 4 GiB + 1 B, a segment ends beyond `u32::MAX` (`RIPTIDE_BIG_TESTS=1`) | `E/matrix_sizes.rs::more_than_four_gibibytes_proves_u64_offsets` | new, opt-in |
| Record size equals the file on disk afterwards | `E/matrix_sizes.rs`, `E/matrix_network.rs::complete` | new (found D-05) |

## 3. Network behaviour

| Requirement | Test | Status |
|---|---|---|
| Single and multiple redirects, all of 301 302 303 307 308, 20 hops OK, 21 fail without retry | `E/matrix_network.rs::every_redirect_status_and_long_chains_complete`, `::more_than_twenty_redirects_fail_and_leave_nothing`; `N/engine.rs::redirect_chain_is_followed_and_credentials_do_not_cross_origins` | existing + new |
| Cross-origin redirect completes; Authorization / Cookie / custom secret headers never reach the other origin | `E/matrix_network.rs::cross_origin_redirect_completes_and_never_leaks_credentials` (server request log), `N/engine.rs` (echo) | existing + new |
| Range ignored (200), no `Accept-Ranges`, chunked / unknown length | `E/matrix_network.rs::range_ignored_and_no_length_are_streamed_whole`; `N/engine.rs::servers_without_ranges_or_length_get_one_streaming_connection` | existing + new |
| HEAD refused (405) | `E/matrix_network.rs::head_refused_and_http_error_statuses` | new |
| HTTP errors 404 / 403 (cookie, referer needed): Failed, not retryable, no file | same | new |
| Content-Disposition variants | section 1 | new |
| Signed / expiring URL then `refresh_url` | `E/matrix_network.rs::expired_signed_url_is_finished_after_refresh`; `E/runtime.rs::refresh_url_swaps_the_link_of_a_running_download_and_finishes_byte_exact` | existing + new |
| Query strings and percent-encoding (not in the name, not lost on the wire) | `E/matrix_network.rs::query_strings_and_percent_encoding_do_not_leak_into_the_name` | new |
| `gzip` although `identity` was requested | `N/engine.rs::gzip_despite_identity_is_saved_as_sent` | existing |
| Server sends more bytes than its own `Content-Range` (CDN compression) | `E/matrix_network.rs::server_sending_more_bytes_than_its_content_range_is_downloaded_whole` (`/range-lie`) | new (found D-04) |
| Slow link | `E/matrix_network.rs::slow_link_and_connection_resets_resume_from_the_offset` | new |
| Connection resets mid-segment (`/drop`, `/flaky`) | same; `N/engine.rs::dropped_connections_are_retried_from_the_offset`, `::flaky_server_eventually_completes` | existing + new |
| Timeouts: stalled body hits the 60 s read timeout | `N/stall.rs::a_stalled_body_times_out_instead_of_hanging` | new, `#[ignore]` (65 s), run manually |
| Retry back-off and `Retry-After` | `N/engine.rs::retry_after_is_honoured`; `E/matrix_network.rs::retry_after_and_host_caps_are_learned_and_obeyed`; unit tests in `retry.rs` | existing + new |
| 429 / 503 learned host caps | `N/engine.rs::throttled_host_gets_a_lower_cap_and_the_download_still_completes`, `::per_host_cap_keeps_a_limited_server_happy`; `N/limits.rs` | existing |
| Basic / digest auth, cookies, referer | `N/engine.rs::cookies_referer_basic_and_digest_auth`; `E/matrix_network.rs::auth_challenges_succeed_with_credentials_and_fail_without` (wrong password: Failed, not retryable) | existing + new |
| Credentials inside the URL never stored in the record | `E/matrix_network.rs::credentials_in_the_url_move_to_secrets_and_never_stay_in_the_record` | new (found D-01) |
| Validator change mid-download: restart from zero, never a splice | `N/engine.rs::a_changed_file_restarts_from_zero_with_the_new_bytes`; `E/matrix_network.rs::validator_change_mid_download_never_splices_versions` (5 timings, file is v1 or v2, at least one v2) | existing + new |
| Wrong Content-Length, both directions, 4 deltas | `N/engine.rs::wrong_content_length_fails_without_retry`; `E/matrix_network.rs::wrong_content_length_never_completes_in_either_direction` | existing + new |
| Corrupt bytes + checksum: Failed (not retryable), never Completed, no final file; good bytes pass | `E/matrix_network.rs::corrupt_bytes_with_a_checksum_end_failed_and_leave_no_final_file`; `E/runtime.rs::checksum_mismatch_fails_permanently_and_never_touches_a_good_file`; `N/engine.rs::checksum_mismatch_is_reported_and_match_passes` | existing + new |
| 24 simultaneous downloads with caps; 25 downloads under a queue limit of 3 | `E/matrix_network.rs::many_simultaneous_downloads_respect_caps`, `::queue_limit_holds_with_twenty_five_downloads` | new |

## 4. Local failure modes

| Requirement | Test | Status |
|---|---|---|
| Destination read-only / permission denied (NTFS deny ACE; chmod on Unix), recovery after fixing it | `E/matrix_local.rs::read_only_destination_fails_cleanly_then_works_once_writable` | new |
| Destination file locked by another handle (Windows, share mode 0) before start and at the rename | `E/matrix_local.rs::destination_locked_by_another_handle_fails_or_avoids_it_then_recovers`, `::final_name_appearing_locked_before_the_rename_never_corrupts_or_lies` | new (Windows only) |
| Path longer than 260 characters | `E/matrix_local.rs::very_long_paths_complete_or_fail_cleanly` (completes on this machine) | new |
| Folder removed mid-download | `E/matrix_local.rs::folder_removed_mid_download_never_completes_wrongly_and_recovers` (Failed, restart works) | new |
| Disk full | **not covered**: no write-injection seam in the engine, and a size-limited volume (VHD / quota) needs administrator rights. Free-space reporting is tested (`E/runtime.rs::free_space_...`) | gap, documented |
| Cancel in every state (Queued, Downloading, Paused, Failed, Completed no-op) leaves only finished files | `E/matrix_local.rs::cancel_in_every_state_leaves_no_files`; `E/runtime.rs::pause_resume_cancel_and_remove` | new |
| Pause / resume at random offsets (seeded, `RIPTIDE_SEED` replays), 1-6 connections | `E/matrix_local.rs::pause_and_resume_at_random_offsets_is_byte_exact`, `::pause_returns_paused_and_an_immediate_resume_is_not_lost` | new (found D-03) |
| Process kill at random moments, restart, resume, SHA-256, no `.rtpart` on success | `C/kill9.rs::killed_at_random_offsets_then_resumes_byte_exact` (existing); `::killed_at_any_instant_still_ends_byte_exact_with_no_leftovers`, `::killed_download_from_a_server_without_ranges_restarts_clean` (new) | existing + new |
| App restart during a download | `E/runtime.rs::a_download_survives_the_process_and_resumes_on_the_next_open`, `::restart_from_the_middle_downloads_the_exact_bytes_again`, `::restart_of_a_paused_download_stays_paused_with_nothing_left_on_disk` | existing |
| Queue limits | `E/runtime.rs::queue_limit_and_order_are_respected`, `E/queues.rs` (9 tests), `E/limits.rs` | existing |

## 5. Real public sources (manual, not CI)

`tests/fixtures/public-download-sources.json` + `scripts/test-public-sources.mjs`; results in the
PR description and in each entry's `last_result`.

## How to run

```powershell
cargo test -p riptide-core -p riptide-net -p riptide-store -p riptide-engine -p riptide-testserver -p riptide-cli
$env:RIPTIDE_BIG_TESTS = "1"; cargo test -p riptide-engine --test matrix_sizes -- --test-threads=1   # 1 GiB + 4 GiB, needs ~5 GB free
cargo test -p riptide-net --test stall -- --ignored                                                  # 65 s
$env:RIPTIDE_SEED = "<seed printed by a failing run>"                                                # replay random pause/resume
cargo build --release -p riptide-cli; node scripts/test-public-sources.mjs                          # internet, ~1 min
```
