"""Screenshots of the title bar (EN / AR, light / dark, menus open, narrow window).

Serves the built app and drives it with Playwright. The window controls only exist inside the
Tauri webview, so a second set of shots injects a stub of the Tauri bridge (enough for the bar to
believe it has a native window); nothing here proves behaviour in a real window.

    python e2e/titlebar_shots.py OUT_DIR
"""

from __future__ import annotations

import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

sys.path.insert(0, str(Path(__file__).resolve().parent))
import list_perf as L  # noqa: E402  (serve / launch helpers)

TAURI_STUB = r"""
(() => {
  let maximized = false;
  const handlers = new Map();
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback(cb) { const id = Math.floor(Math.random() * 1e9); handlers.set(id, cb); return id; },
    unregisterCallback() {},
    convertFileSrc: (p) => p,
    async invoke(cmd, args) {
      if (cmd === 'plugin:window|is_maximized') return maximized;
      if (cmd === 'plugin:window|maximize') { maximized = true; window.dispatchEvent(new Event('resize')); return null; }
      if (cmd === 'plugin:window|unmaximize') { maximized = false; window.dispatchEvent(new Event('resize')); return null; }
      if (cmd === 'plugin:event|listen') return 1;
      if (cmd === 'list_downloads') return [];
      if (cmd === 'list_queues') return [];
      if (cmd === 'app_info') return { name: 'Riptide', version: '0.1.0-alpha.5', build: 'preview' };
      if (cmd === 'autostart_enabled') return false;
      if (cmd === 'disk_free') return 400_000_000_000;
      return null;
    },
  };
})();
"""


def prefs(lang: str, theme: str) -> str:
    return f"localStorage.setItem('riptide.lang', '{lang}'); localStorage.setItem('riptide.theme', '{theme}');"


def shot(browser, port: int, out: Path, name: str, lang: str, theme: str, width: int, height: int, menu: str | None = None, stub: bool = False) -> None:
    ctx = browser.new_context(viewport={"width": width, "height": height}, device_scale_factor=1)
    page = ctx.new_page()
    page.add_init_script(prefs(lang, theme))
    if stub:
        page.add_init_script(TAURI_STUB)
    page.goto(f"http://127.0.0.1:{port}/?mockCount=40&mockActive=4#/")
    page.wait_for_selector("app-title-bar .top-button", timeout=60_000)
    page.wait_for_timeout(1500)
    if menu:
        page.locator(f"#menu-button-{menu}").click()
        page.wait_for_selector("[role=menu] .item", timeout=20_000)
        page.wait_for_timeout(400)
    # the bar and the strip below it are what is on show; the full window is saved as well
    bar = page.locator("app-title-bar")
    box = bar.bounding_box()
    page.screenshot(path=str(out / f"{name}.png"), full_page=False)
    clip_h = 420 if menu else 90
    page.screenshot(path=str(out / f"{name}-bar.png"), clip={"x": 0, "y": 0, "width": width, "height": min(height, clip_h)})
    assert box is not None
    ctx.close()


def main() -> int:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    server, port = L.serve()
    with sync_playwright() as p:
        browser = L.launch(p)
        for lang in ("en", "ar"):
            for theme in ("dark", "light"):
                shot(browser, port, out, f"{lang}-{theme}-closed", lang, theme, 1280, 780)
        shot(browser, port, out, "en-dark-file-menu", "en", "dark", 1280, 780, "file")
        shot(browser, port, out, "en-light-view-menu", "en", "light", 1280, 780, "view")
        shot(browser, port, out, "ar-dark-downloads-menu", "ar", "dark", 1280, 780, "downloads")
        shot(browser, port, out, "ar-light-view-menu", "ar", "light", 1280, 780, "view")
        shot(browser, port, out, "en-dark-help-menu", "en", "dark", 1280, 780, "help")
        shot(browser, port, out, "narrow-en-dark", "en", "dark", 760, 600)
        shot(browser, port, out, "narrow-en-light-menu", "en", "light", 760, 600, "tools")
        shot(browser, port, out, "narrow-ar-dark-menu", "ar", "dark", 760, 600, "view")
        # with a stub of the Tauri bridge: window controls and the Window menu
        shot(browser, port, out, "tauri-en-dark", "en", "dark", 1280, 780, None, True)
        shot(browser, port, out, "tauri-ar-light", "ar", "light", 1280, 780, None, True)
        shot(browser, port, out, "tauri-ar-dark-window-menu", "ar", "dark", 1280, 780, "window", True)
        shot(browser, port, out, "tauri-en-light-window-menu", "en", "light", 1280, 780, "window", True)
        browser.close()
    server.shutdown()
    print("saved to", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
