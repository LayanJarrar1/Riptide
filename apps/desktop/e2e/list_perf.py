"""List performance harness for the downloads page.

Serves the built app (`npm run build` -> dist/desktop/browser, or the folder in $PERF_DIST) and
drives it in Chromium through Playwright. The mock engine is seeded with N downloads through
`?mockCount=N` (`MOCK_LIST_SIZE` in mock-engine.service.ts; the simulated engine is a lazy chunk
that the Tauri build never loads, so the flag cannot steer the desktop app).

Per N it measures
  * DOM row count (must stay about constant), JS heap after a forced GC, time to the first row;
  * scroll frame times: rAF deltas while the list is scrolled programmatically for 5 s at two
    constant speeds ("wheel" ~ one row per frame, "flick" ~ four rows per frame): p50 / p95 / max
    and the share of dropped frames (a delta above 1.5 frames), plus main-thread busy time;
  * main-thread time (CDP Performance.getMetrics: TaskDuration, ScriptDuration) with 100 active
    downloads updating at 10 Hz, idle and while scrolling;
  * the latency of sorting and filtering (action until the second following frame);
  * keyboard: End / Home / PageDown keep the cursor row on screen and the DOM bounded.

    python e2e/list_perf.py [--counts 10,100,1000,10000,50000] [--out e2e/list_perf.md] [--quick]

Edge or Chrome is used if Playwright's own Chromium is not installed (set CHROME_BIN).
"""

from __future__ import annotations

import argparse
import functools
import http.server
import json
import os
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent
DIST = Path(os.environ["PERF_DIST"]) if os.environ.get("PERF_DIST") else ROOT / "dist" / "desktop" / "browser"
VIEWPORT = {"width": 1280, "height": 780}
FRAME_MS = 1000 / 60
ROW_PX = 46
SPEEDS = {"wheel": 2400, "flick": 12000}  # px per second

# in-page helpers: installed on every page before the app starts
HELPERS = r"""
window.__perf = { firstRow: null };
new MutationObserver((_, obs) => {
  if (window.__perf.firstRow === null && document.querySelector('app-download-row')) {
    window.__perf.firstRow = performance.now();
    obs.disconnect();
  }
}).observe(document, { childList: true, subtree: true });

// scroll at a constant speed (px/s) for `seconds`, starting mid-list and bouncing off the ends;
// resolves with the rAF deltas
window.__scrollRun = (seconds, speed) => new Promise((resolve) => {
  const vp = document.querySelector('.viewport');
  const max = vp.scrollHeight - vp.clientHeight;
  const deltas = [];
  let pos = max / 2, dir = 1;
  let last = performance.now();
  const start = last;
  const step = (now) => {
    deltas.push(now - last);
    const dt = (now - last) / 1000;
    last = now;
    if (now - start >= seconds * 1000) { resolve(deltas); return; }
    pos += dir * speed * dt;
    if (pos >= max) { pos = max; dir = -1; } else if (pos <= 0) { pos = 0; dir = 1; }
    vp.scrollTop = pos;
    requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
});

window.__nextPaint = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(performance.now()))));

window.__timeAction = async (fn) => {
  const t0 = performance.now();
  fn();
  await window.__nextPaint();
  return performance.now() - t0;
};

window.__rowsInDom = () => document.querySelectorAll('app-download-row').length;
"""


class _Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args: object) -> None:
        return


def serve() -> tuple[http.server.ThreadingHTTPServer, int]:
    handler = functools.partial(_Quiet, directory=str(DIST))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


def pct(values: list[float], p: float) -> float:
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round(p / 100 * (len(ordered) - 1))))
    return ordered[index]


def metrics(cdp) -> dict[str, float]:
    return {m["name"]: m["value"] for m in cdp.send("Performance.getMetrics")["metrics"]}


def heap_mb(cdp) -> float:
    cdp.send("HeapProfiler.collectGarbage")
    return cdp.send("Runtime.getHeapUsage")["usedSize"] / 1048576


def launch(p):
    exe = os.environ.get("CHROME_BIN")
    if exe and Path(exe).exists():
        return p.chromium.launch(executable_path=exe, headless=True)
    try:
        return p.chromium.launch(headless=True)
    except Exception:
        return p.chromium.launch(channel="msedge", headless=True)


def open_page(browser, url: str, attempts: int = 3):
    """Opens the app and waits for its first row; a page that never paints (the machine is shared) is retried."""
    for attempt in range(attempts):
        ctx = browser.new_context(viewport=VIEWPORT)
        page = ctx.new_page()
        page.add_init_script(HELPERS)
        cdp = ctx.new_cdp_session(page)
        cdp.send("Performance.enable")
        page.goto(url, wait_until="commit")
        try:
            page.wait_for_selector("app-download-row", timeout=40_000)
        except Exception:
            ctx.close()
            if attempt == attempts - 1:
                raise
            continue
        return ctx, page, cdp, page.evaluate("window.__perf.firstRow")
    raise RuntimeError("unreachable")


def frame_stats(deltas: list[float]) -> dict[str, float]:
    deltas = deltas[1:]
    return {
        "p50": round(pct(deltas, 50), 1),
        "p95": round(pct(deltas, 95), 1),
        "max": round(max(deltas), 1),
        "dropped_pct": round(100 * sum(1 for d in deltas if d > FRAME_MS * 1.5) / len(deltas), 1),
    }


def busy(before: dict[str, float], after: dict[str, float], wall: float, name: str) -> float:
    return round(100 * (after[name] - before[name]) / wall, 1)


def measure(browser, port: int, count: int, quick: bool) -> dict[str, object]:
    base = f"http://127.0.0.1:{port}/"
    seconds = 2.0 if quick else 5.0
    out: dict[str, object] = {"n": count}

    # ---- A: the list as a user finds it (the engine's default few active downloads) ---------
    ctx, page, cdp, first_row = open_page(browser, f"{base}?mockCount={count}&mockActive=4#/")
    out["first_render_ms"] = round(first_row)
    page.wait_for_timeout(1500)  # the first-paint orchestration is over
    out["dom_rows"] = page.evaluate("window.__rowsInDom()")
    out["heap_mb"] = round(heap_mb(cdp), 1)

    for name, speed in SPEEDS.items():
        a, t0 = metrics(cdp), time.perf_counter()
        deltas = page.evaluate(f"window.__scrollRun({seconds}, {speed})")
        b, wall = metrics(cdp), time.perf_counter() - t0
        stats = frame_stats(deltas)
        stats["busy_pct"] = busy(a, b, wall, "TaskDuration")
        stats["script_pct"] = busy(a, b, wall, "ScriptDuration")
        out[f"scroll_{name}"] = stats
        out[f"dom_rows_after_{name}"] = page.evaluate("window.__rowsInDom()")
        page.wait_for_timeout(300)

    # sort and filter latency
    page.evaluate("document.querySelector('.viewport').scrollTop = 0")
    page.wait_for_timeout(300)
    out["sort_ms"] = {}
    for key in ("name", "size", "status", "added"):
        out["sort_ms"][key] = round(page.evaluate(f"window.__timeAction(() => document.querySelector('.th-{key}').click())"))
        page.wait_for_timeout(200)
    out["filter_ms"] = {}
    for label, text in (("narrow", "00042"), ("broad", "iso")):
        out["filter_ms"][label] = round(
            page.evaluate(
                "async (t) => window.__timeAction(() => {"
                " const i = document.querySelector('input.search-input'); i.value = t;"
                " i.dispatchEvent(new Event('input', {bubbles: true})); })",
                text,
            )
        )
        page.wait_for_timeout(200)
    page.locator("input.search-input").fill("")
    page.wait_for_timeout(300)

    # keyboard at depth: the cursor row stays on screen, the DOM stays small
    page.locator(".grid").focus()
    visible = (
        "(() => { const s = document.querySelector('app-download-row.is-active');"
        " const v = document.querySelector('.viewport').getBoundingClientRect();"
        " if (!s) return false; const r = s.getBoundingClientRect();"
        " return r.top >= v.top - 1 && r.bottom <= v.bottom + 1; })()"
    )
    keys = {}
    for key in ("ArrowDown", "End", "PageUp", "Home", "PageDown"):
        page.keyboard.press(key)
        page.wait_for_timeout(120)
        keys[key] = bool(page.evaluate(visible))
    out["keyboard_cursor_visible"] = keys
    out["keyboard_dom_rows"] = page.evaluate("window.__rowsInDom()")
    ctx.close()

    # ---- B: 100 active downloads updating at 10 Hz ------------------------------------------
    active = min(100, count)
    ctx, page, cdp, _ = open_page(browser, f"{base}?mockCount={count}&mockActive={active}&mockTickMs=100#/")
    page.wait_for_timeout(1500)
    a, t0 = metrics(cdp), time.perf_counter()
    page.wait_for_timeout(int(seconds * 1000))
    b, wall = metrics(cdp), time.perf_counter() - t0
    out["live_active"] = active
    out["live_idle_busy_pct"] = busy(a, b, wall, "TaskDuration")
    out["live_idle_script_pct"] = busy(a, b, wall, "ScriptDuration")
    out["live_dom_rows"] = page.evaluate("window.__rowsInDom()")
    a, t0 = metrics(cdp), time.perf_counter()
    deltas = page.evaluate(f"window.__scrollRun({seconds}, {SPEEDS['wheel']})")
    b, wall = metrics(cdp), time.perf_counter() - t0
    live = frame_stats(deltas)
    live["busy_pct"] = busy(a, b, wall, "TaskDuration")
    out["live_scroll"] = live
    ctx.close()
    return out


def tables(results: list[dict[str, object]]) -> str:
    def row(cells: list[object]) -> str:
        return "| " + " | ".join(str(c) for c in cells) + " |"

    def block(head: list[str], rows: list[list[object]]) -> str:
        return "\n".join([row(head), row(["---"] * len(head)), *[row(r) for r in rows]]) + "\n"

    t1 = block(
        ["N", "first render, best of runs (ms)", "DOM rows", "heap (MB)", "keyboard: cursor on screen after Down / End / PgUp / Home / PgDn"],
        [
            [
                f"{r['n']:,}",
                r["first_render_ms"],
                f"{r['dom_rows']} (max {max(r['dom_rows_after_wheel'], r['dom_rows_after_flick'], r['keyboard_dom_rows'])})",
                r["heap_mb"],
                " / ".join("yes" if v else "NO" for v in r["keyboard_cursor_visible"].values()),
            ]
            for r in results
        ],
    )
    t2 = block(
        ["N", "wheel p50 / p95 / max (ms)", "wheel dropped %", "wheel busy %", "flick p50 / p95 / max (ms)", "flick dropped %", "flick busy %"],
        [
            [
                f"{r['n']:,}",
                f"{r['scroll_wheel']['p50']} / {r['scroll_wheel']['p95']} / {r['scroll_wheel']['max']}",
                r["scroll_wheel"]["dropped_pct"],
                r["scroll_wheel"]["busy_pct"],
                f"{r['scroll_flick']['p50']} / {r['scroll_flick']['p95']} / {r['scroll_flick']['max']}",
                r["scroll_flick"]["dropped_pct"],
                r["scroll_flick"]["busy_pct"],
            ]
            for r in results
        ],
    )
    t3 = block(
        ["N", "active rows", "10 Hz updates: main thread busy % (script %)", "DOM rows", "wheel scroll p95 while live (ms)", "dropped %"],
        [
            [
                f"{r['n']:,}",
                r["live_active"],
                f"{r['live_idle_busy_pct']} ({r['live_idle_script_pct']})",
                r["live_dom_rows"],
                r["live_scroll"]["p95"],
                r["live_scroll"]["dropped_pct"],
            ]
            for r in results
        ],
    )
    t4 = block(
        ["N", "sort name", "sort size", "sort status", "sort added", "filter narrow", "filter broad"],
        [
            [f"{r['n']:,}", *[r["sort_ms"][k] for k in ("name", "size", "status", "added")], r["filter_ms"]["narrow"], r["filter_ms"]["broad"]]
            for r in results
        ],
    )
    return (
        "### Render, memory, keyboard\n\n" + t1 + "\n### Scrolling (5 s each; wheel = 2 400 px/s, flick = 12 000 px/s; 46 px rows)\n\n" + t2
        + "\n### 100 active downloads updating at 10 Hz\n\n" + t3 + "\n### Sort and filter latency (ms, click or input until the second following frame)\n\n" + t4
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--counts", default="10,100,1000,10000,50000")
    ap.add_argument("--out", default=str(ROOT / "e2e" / "list_perf.md"))
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--label", default="")
    ap.add_argument("--runs", type=int, default=1, help="repeat each N and keep the median run (the machine is shared)")
    args = ap.parse_args()
    if not (DIST / "index.html").exists():
        print(f"build first: npm run build ({DIST})", file=sys.stderr)
        return 2
    server, port = serve()
    results = []
    with sync_playwright() as p:
        browser = launch(p)
        version = browser.version
        for n in (int(c) for c in args.counts.split(",")):
            print(f"measuring N={n} ...", flush=True)
            runs = [measure(browser, port, n, args.quick) for _ in range(args.runs)]
            # the median run by total frame pressure (p95 of both scroll speeds and of the live scroll)
            runs.sort(key=lambda m: m["scroll_wheel"]["p95"] + m["scroll_flick"]["p95"] + m["live_scroll"]["p95"] + m["scroll_wheel"]["busy_pct"] / 100)
            r = runs[len(runs) // 2]
            r["runs"] = args.runs
            # a page that starts while the machine is busy says little about the app: best of the runs
            r["first_render_runs_ms"] = sorted(m["first_render_ms"] for m in runs)
            r["first_render_ms"] = r["first_render_runs_ms"][0]
            print(json.dumps(r), flush=True)
            results.append(r)
        browser.close()
    server.shutdown()
    md = (
        f"# Downloads list performance {args.label}\n\n"
        f"Measured {time.strftime('%Y-%m-%d')} with `python e2e/list_perf.py` on the production build, Chromium {version} "
        f"(headless, software rendering), viewport {VIEWPORT['width']}x{VIEWPORT['height']}, mock engine. "
        f"Each row is the median of {args.runs} run(s) by frame pressure (first render: the best run). Frame time = rAF delta (16.7 ms at 60 Hz); dropped = a delta above 25 ms. Busy % = main-thread "
        f"TaskDuration (script, style, layout and paint work) over wall time.\n\n"
        + tables(results)
        + "\nRaw: `" + json.dumps(results) + "`\n"
    )
    Path(args.out).write_text(md, encoding="utf-8")
    print(md.encode("ascii", "replace").decode())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
