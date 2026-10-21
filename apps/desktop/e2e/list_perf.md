# Downloads list performance (alpha.5 build)

Measured 2026-10-20 with `python e2e/list_perf.py` on the production build, Chromium 155.0.4283.62 (headless, software rendering), viewport 1280x780, mock engine. Each row is the median of 3 run(s) by frame pressure (first render: the best run). Frame time = rAF delta (16.7 ms at 60 Hz); dropped = a delta above 25 ms. Busy % = main-thread TaskDuration (script, style, layout and paint work) over wall time.

### Render, memory, keyboard

| N | first render, best of runs (ms) | DOM rows | heap (MB) | keyboard: cursor on screen after Down / End / PgUp / Home / PgDn |
| --- | --- | --- | --- | --- |
| 10 | 224 | 10 (max 10) | 5.9 | yes / yes / yes / yes / yes |
| 100 | 235 | 19 (max 25) | 6.3 | yes / yes / yes / yes / yes |
| 1,000 | 251 | 19 (max 26) | 6.9 | yes / yes / yes / yes / yes |
| 10,000 | 300 | 19 (max 26) | 12.5 | yes / yes / yes / yes / yes |
| 50,000 | 509 | 19 (max 26) | 36.3 | yes / yes / yes / yes / yes |

### Scrolling (5 s each; wheel = 2 400 px/s, flick = 12 000 px/s; 46 px rows)

| N | wheel p50 / p95 / max (ms) | wheel dropped % | wheel busy % | flick p50 / p95 / max (ms) | flick dropped % | flick busy % |
| --- | --- | --- | --- | --- | --- | --- |
| 10 | 16.7 / 16.7 / 16.8 | 0.0 | 20.2 | 16.7 / 16.7 / 16.8 | 0.0 | 24.5 |
| 100 | 16.7 / 16.7 / 33.4 | 0.3 | 37.4 | 16.7 / 16.8 / 16.8 | 0.0 | 49.7 |
| 1,000 | 16.7 / 16.8 / 33.3 | 0.3 | 41.8 | 16.7 / 16.8 / 33.4 | 0.3 | 57.9 |
| 10,000 | 16.7 / 16.7 / 33.3 | 0.3 | 42.4 | 16.7 / 16.8 / 33.4 | 0.3 | 56.9 |
| 50,000 | 16.7 / 16.8 / 33.4 | 0.3 | 47.7 | 16.7 / 16.8 / 33.3 | 0.3 | 61.8 |

### 100 active downloads updating at 10 Hz

| N | active rows | 10 Hz updates: main thread busy % (script %) | DOM rows | wheel scroll p95 while live (ms) | dropped % |
| --- | --- | --- | --- | --- | --- |
| 10 | 10 | 22.5 (3.1) | 10 | 16.8 | 0.0 |
| 100 | 100 | 23.3 (2.6) | 19 | 16.7 | 0.3 |
| 1,000 | 100 | 35.2 (4.3) | 19 | 16.8 | 0.3 |
| 10,000 | 100 | 39.4 (4.8) | 19 | 16.7 | 0.3 |
| 50,000 | 100 | 40.7 (9.6) | 19 | 16.7 | 0.3 |

### Sort and filter latency (ms, click or input until the second following frame)

| N | sort name | sort size | sort status | sort added | filter narrow | filter broad |
| --- | --- | --- | --- | --- | --- | --- |
| 10 | 36 | 24 | 29 | 28 | 28 | 33 |
| 100 | 33 | 28 | 27 | 27 | 26 | 32 |
| 1,000 | 39 | 32 | 29 | 27 | 31 | 26 |
| 10,000 | 53 | 68 | 30 | 25 | 20 | 30 |
| 50,000 | 92 | 117 | 95 | 32 | 30 | 25 |

Raw: `[{"n": 10, "first_render_ms": 224, "dom_rows": 10, "heap_mb": 5.9, "scroll_wheel": {"p50": 16.7, "p95": 16.7, "max": 16.8, "dropped_pct": 0.0, "busy_pct": 20.2, "script_pct": 1.4}, "dom_rows_after_wheel": 10, "scroll_flick": {"p50": 16.7, "p95": 16.7, "max": 16.8, "dropped_pct": 0.0, "busy_pct": 24.5, "script_pct": 1.8}, "dom_rows_after_flick": 10, "sort_ms": {"name": 36, "size": 24, "status": 29, "added": 28}, "filter_ms": {"narrow": 28, "broad": 33}, "keyboard_cursor_visible": {"ArrowDown": true, "End": true, "PageUp": true, "Home": true, "PageDown": true}, "keyboard_dom_rows": 10, "live_active": 10, "live_idle_busy_pct": 22.5, "live_idle_script_pct": 3.1, "live_dom_rows": 10, "live_scroll": {"p50": 16.7, "p95": 16.8, "max": 16.8, "dropped_pct": 0.0, "busy_pct": 27.5}, "runs": 3, "first_render_runs_ms": [224, 227, 268]}, {"n": 100, "first_render_ms": 235, "dom_rows": 19, "heap_mb": 6.3, "scroll_wheel": {"p50": 16.7, "p95": 16.7, "max": 33.4, "dropped_pct": 0.3, "busy_pct": 37.4, "script_pct": 9.2}, "dom_rows_after_wheel": 25, "scroll_flick": {"p50": 16.7, "p95": 16.8, "max": 16.8, "dropped_pct": 0.0, "busy_pct": 49.7, "script_pct": 16.2}, "dom_rows_after_flick": 25, "sort_ms": {"name": 33, "size": 28, "status": 27, "added": 27}, "filter_ms": {"narrow": 26, "broad": 32}, "keyboard_cursor_visible": {"ArrowDown": true, "End": true, "PageUp": true, "Home": true, "PageDown": true}, "keyboard_dom_rows": 19, "live_active": 100, "live_idle_busy_pct": 23.3, "live_idle_script_pct": 2.6, "live_dom_rows": 19, "live_scroll": {"p50": 16.7, "p95": 16.7, "max": 33.2, "dropped_pct": 0.3, "busy_pct": 53.8}, "runs": 3, "first_render_runs_ms": [235, 241, 250]}, {"n": 1000, "first_render_ms": 251, "dom_rows": 19, "heap_mb": 6.9, "scroll_wheel": {"p50": 16.7, "p95": 16.8, "max": 33.3, "dropped_pct": 0.3, "busy_pct": 41.8, "script_pct": 10.2}, "dom_rows_after_wheel": 25, "scroll_flick": {"p50": 16.7, "p95": 16.8, "max": 33.4, "dropped_pct": 0.3, "busy_pct": 57.9, "script_pct": 18.9}, "dom_rows_after_flick": 26, "sort_ms": {"name": 39, "size": 32, "status": 29, "added": 27}, "filter_ms": {"narrow": 31, "broad": 26}, "keyboard_cursor_visible": {"ArrowDown": true, "End": true, "PageUp": true, "Home": true, "PageDown": true}, "keyboard_dom_rows": 19, "live_active": 100, "live_idle_busy_pct": 35.2, "live_idle_script_pct": 4.3, "live_dom_rows": 19, "live_scroll": {"p50": 16.7, "p95": 16.8, "max": 50, "dropped_pct": 0.3, "busy_pct": 49.9}, "runs": 3, "first_render_runs_ms": [251, 251, 267]}, {"n": 10000, "first_render_ms": 300, "dom_rows": 19, "heap_mb": 12.5, "scroll_wheel": {"p50": 16.7, "p95": 16.7, "max": 33.3, "dropped_pct": 0.3, "busy_pct": 42.4, "script_pct": 10.9}, "dom_rows_after_wheel": 26, "scroll_flick": {"p50": 16.7, "p95": 16.8, "max": 33.4, "dropped_pct": 0.3, "busy_pct": 56.9, "script_pct": 19.1}, "dom_rows_after_flick": 25, "sort_ms": {"name": 53, "size": 68, "status": 30, "added": 25}, "filter_ms": {"narrow": 20, "broad": 30}, "keyboard_cursor_visible": {"ArrowDown": true, "End": true, "PageUp": true, "Home": true, "PageDown": true}, "keyboard_dom_rows": 19, "live_active": 100, "live_idle_busy_pct": 39.4, "live_idle_script_pct": 4.8, "live_dom_rows": 19, "live_scroll": {"p50": 16.7, "p95": 16.7, "max": 33.4, "dropped_pct": 0.3, "busy_pct": 44.2}, "runs": 3, "first_render_runs_ms": [300, 305, 323]}, {"n": 50000, "first_render_ms": 509, "dom_rows": 19, "heap_mb": 36.3, "scroll_wheel": {"p50": 16.7, "p95": 16.8, "max": 33.4, "dropped_pct": 0.3, "busy_pct": 47.7, "script_pct": 13.9}, "dom_rows_after_wheel": 25, "scroll_flick": {"p50": 16.7, "p95": 16.8, "max": 33.3, "dropped_pct": 0.3, "busy_pct": 61.8, "script_pct": 22.5}, "dom_rows_after_flick": 26, "sort_ms": {"name": 92, "size": 117, "status": 95, "added": 32}, "filter_ms": {"narrow": 30, "broad": 25}, "keyboard_cursor_visible": {"ArrowDown": true, "End": true, "PageUp": true, "Home": true, "PageDown": true}, "keyboard_dom_rows": 19, "live_active": 100, "live_idle_busy_pct": 40.7, "live_idle_script_pct": 9.6, "live_dom_rows": 19, "live_scroll": {"p50": 16.7, "p95": 16.7, "max": 33.4, "dropped_pct": 0.3, "busy_pct": 45.3}, "runs": 3, "first_render_runs_ms": [509, 511, 518]}]`
