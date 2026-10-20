"""
Popup E2E for the Riptide extension, real Chrome + throwaway profile (Playwright for Python).

    python apps/extension/e2e/popup_states.py            # rendering + wiring, touches nothing outside a temp profile
    python apps/extension/e2e/popup_states.py --live     # also drives the real native-messaging path

Needs: `npm run build` in apps/extension (dist/chrome), `pip install playwright`, Google Chrome.

Chrome 137+ branded builds ignore --load-extension, so the extension is loaded through the DevTools
protocol (`Extensions.loadUnpacked`, enabled by --enable-unsafe-extension-debugging). The profile is a
temp folder; the owner's real profile is never opened. This is the test harness, not a way to
install the extension for users: users load it themselves (Developer mode -> Load unpacked) or from a store.

Part 1 (always): the popup page of the real extension is opened in a tab, and `runtime.sendMessage`
is replaced by a scripted background so each state is deterministic: connected, app not running ->
Open Riptide -> connected, app not running -> Open Riptide -> still not running (clear hint), host
missing (clear message, Open Riptide does not hang), Arabic RTL, and the website/help action
(opens the fixed https URL, nothing else).

Part 2 (--live): the real background and the real riptide-host.exe.
  * host missing   : the real Chrome NativeMessagingHosts key is exported, removed, and restored
                     (and compared byte for byte afterwards).
  * app not running, key present: the key points to a throwaway manifest (spaces + non-ASCII path)
                     whose "app" is a stub that records how it was started; Open Riptide must start
                     it, detached, with the app folder as working directory.
Both restore the key in a `finally`. Skipped (not failed) when the host binary is not built.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
import winreg

from playwright.sync_api import sync_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
EXT = os.path.join(REPO, "apps", "extension", "dist", "chrome")
SHOTS = os.environ.get("RIPTIDE_E2E_SHOTS", os.path.join(tempfile.gettempdir(), "riptide-popup-shots"))
HELP_URL = "https://layanjarrar1.github.io/Riptide/"
HOST_KEY = r"Software\Google\Chrome\NativeMessagingHosts\app.riptide.host"
os.makedirs(SHOTS, exist_ok=True)

failures = []
sys.stdout.reconfigure(encoding="utf-8", errors="replace")


def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg, flush=True)
    if not cond:
        failures.append(msg)


def chromium_id():
    src = open(os.path.join(REPO, "crates", "riptide-host", "src", "lib.rs"), encoding="utf-8").read()
    import re
    return re.search(r'CHROMIUM_EXTENSION_ID: &str = "([a-p]{32})"', src).group(1)


EXT_ID = chromium_id()


def launch(p, env=None):
    profile = tempfile.mkdtemp(prefix="rt-e2e-profile-")
    full = dict(os.environ)
    full.update(env or {})
    ctx = p.chromium.launch_persistent_context(
        profile, channel="chrome", headless=True, env=full,
        ignore_default_args=["--disable-extensions"],
        args=["--enable-unsafe-extension-debugging"], viewport={"width": 520, "height": 640})
    session = ctx.browser.new_browser_cdp_session()
    loaded = session.send("Extensions.loadUnpacked", {"path": EXT})
    assert loaded["id"] == EXT_ID, loaded
    time.sleep(2)  # let the extension's service worker start; the first page load is flaky without it
    return ctx, profile


STUB = """
(() => {
  const sc = %s;
  let current = sc.initial;
  const reply = (status) => ({ status, version: status === 'connected' ? '0.1.0' : null, captured: 0,
    recent: [], paused: false, rulesEnabled: true, listAvailable: true });
  window.__calls = [];
  // answers both ways: promise (what the polyfill passes through in MV3) and callback
  chrome.runtime.sendMessage = function (msg, cb) {
    window.__calls.push(msg.kind);
    return new Promise((resolve) => {
      const done = (r) => { if (typeof cb === 'function') cb(r); resolve(r); };
      if (msg.kind === 'open-app') {
        setTimeout(() => { current = sc.afterOpen; done(reply(current)); }, sc.delay);
      } else {
        done(reply(current));
      }
    });
  };
})();
"""


def popup(ctx, scenario, lang=None):
    pg = ctx.new_page()
    pg.add_init_script(STUB % json.dumps(scenario))
    if lang:
        boot = ctx.new_page()
        boot.goto(f"chrome-extension://{EXT_ID}/options/options.html")
        boot.evaluate("(l) => chrome.storage.local.set({ settings: { language: l } })", lang)
        boot.close()
    pg.goto(f"chrome-extension://{EXT_ID}/popup/popup.html")
    pg.wait_for_selector("#status:not([data-status='unknown'])", timeout=5000)
    return pg


def state(pg):
    return pg.evaluate("""() => ({
      status: document.getElementById('status').dataset.status,
      label: document.getElementById('status-label').textContent,
      hint: document.getElementById('status-hint').textContent,
      open: document.getElementById('open').textContent,
      openDisabled: document.getElementById('open').disabled,
      dir: document.documentElement.dir })""")


def part1(ctx):
    print("Part 1: popup states (scripted background)")
    # connected
    pg = popup(ctx, {"initial": "connected", "afterOpen": "connected", "delay": 0})
    s = state(pg)
    check(s["status"] == "connected" and "Connected" in s["label"], f"connected: {s['label']!r}")
    pg.screenshot(path=os.path.join(SHOTS, "popup-connected.png"))
    pg.close()

    # app not running -> Open Riptide -> connected (busy state while the host starts the app)
    pg = popup(ctx, {"initial": "app_not_running", "afterOpen": "connected", "delay": 1200})
    s = state(pg)
    check(s["status"] == "app_not_running" and "not running" in s["label"], f"app not running: {s['label']!r}")
    check("Open Riptide" in s["open"], "the Open Riptide button is offered")
    pg.screenshot(path=os.path.join(SHOTS, "popup-app-not-running.png"))
    pg.click("#open")
    pg.wait_for_timeout(300)
    s = state(pg)
    check(s["openDisabled"] and "Starting" in s["open"], f"busy feedback while starting: {s['open']!r}")
    pg.screenshot(path=os.path.join(SHOTS, "popup-starting.png"))
    pg.wait_for_selector("#status[data-status='connected']", timeout=5000)
    s = state(pg)
    check(not s["openDisabled"] and s["open"] == "Open Riptide", "button restored after the app came up")
    check(pg.evaluate("window.__calls").count("open-app") == 1, "exactly one open-app message was sent")
    pg.close()

    # app not running -> Open Riptide -> still not running: a clear next step, no hang
    pg = popup(ctx, {"initial": "app_not_running", "afterOpen": "app_not_running", "delay": 400})
    pg.click("#open")
    pg.wait_for_selector("#open:not([disabled])", timeout=5000)
    s = state(pg)
    check(s["status"] == "app_not_running" and "did not start" in s["hint"], f"failed start explained: {s['hint']!r}")
    pg.screenshot(path=os.path.join(SHOTS, "popup-start-failed.png"))
    pg.close()

    # host missing: clear message
    pg = popup(ctx, {"initial": "host_missing", "afterOpen": "host_missing", "delay": 200})
    s = state(pg)
    check(s["status"] == "host_missing" and "not installed" in s["label"], f"host missing: {s['label']!r}")
    check("Reinstall Riptide" in s["hint"] and "Integration" in s["hint"], f"host missing hint is actionable: {s['hint']!r}")
    pg.click("#open")
    pg.wait_for_selector("#open:not([disabled])", timeout=5000)
    check(state(pg)["status"] == "host_missing", "Open Riptide with a missing host ends cleanly")
    pg.screenshot(path=os.path.join(SHOTS, "popup-host-missing.png"))

    # website / help: the extension itself opens the fixed https URL
    ctx.route(HELP_URL + "**", lambda route: route.fulfill(status=200, body="<title>help</title>", content_type="text/html"))
    before = set(id(x) for x in ctx.pages)
    pump = ctx.pages[0] if ctx.pages[0] is not pg else ctx.new_page()  # events only flow while a call waits
    pg.click("#help")
    deadline = time.time() + 8
    while time.time() < deadline and not any(x.url.startswith(HELP_URL) for x in ctx.pages):
        pump.wait_for_timeout(100)  # the popup closes itself after opening the tab
    opened = [x for x in ctx.pages if id(x) not in before]
    urls = [x.url for x in ctx.pages]
    check(any(u == HELP_URL for u in urls), f"Website and help opened {HELP_URL} ({urls})")
    for x in opened + [pg]:
        try:
            x.close()
        except Exception:
            pass

    # Arabic, right to left
    pg = popup(ctx, {"initial": "app_not_running", "afterOpen": "connected", "delay": 100}, lang="ar")
    s = state(pg)
    check(s["dir"] == "rtl", "Arabic popup is right-to-left")
    check("\u0627\u0641\u062a\u062d" in s["open"], f"Arabic Open Riptide label: {s['open']!r}")
    pg.screenshot(path=os.path.join(SHOTS, "popup-ar.png"))
    pg.close()


def reg_read():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, HOST_KEY) as k:
            return winreg.QueryValueEx(k, "")[0]
    except OSError:
        return None


def reg_write(value):
    if value is None:
        subprocess.run(["reg", "delete", "HKCU\\" + HOST_KEY, "/f"], capture_output=True)
    else:
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, HOST_KEY) as k:
            winreg.SetValueEx(k, "", 0, winreg.REG_SZ, value)


def csc():
    for c in (r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe", r"C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe"):
        if os.path.isfile(c):
            return c
    return None


def part2(p):
    print("Part 2 (--live): real background + riptide-host.exe")
    host_src = None
    for rel in ("target/debug/riptide-host.exe", "target/release/riptide-host.exe"):
        cand = os.path.join(REPO, *rel.split("/"))
        if os.path.isfile(cand):
            host_src = cand
            break
    if not host_src or not csc():
        print("  SKIPPED: build riptide-host (cargo build -p riptide-host) and have the .NET csc.exe")
        return
    original = reg_read()
    print("  saved HKCU Chrome host key:", original)
    work = tempfile.mkdtemp(prefix="rt-e2e-live-")
    app_dir = os.path.join(work, "Riptide T\u00e4st \u0645\u062c\u0644\u062f")
    os.makedirs(app_dir)
    try:
        # --- host missing: remove the key (restored in finally)
        reg_write(None)
        ctx, profile = launch(p)
        try:
            pg = ctx.new_page()
            pg.goto(f"chrome-extension://{EXT_ID}/popup/popup.html")
            pg.wait_for_selector("#status[data-status='host_missing']", timeout=15000)
            s = state(pg)
            check("not installed" in s["label"], f"live host_missing: {s['label']!r}")
            pg.click("#open")
            pg.wait_for_selector("#open:not([disabled])", timeout=20000)
            check(state(pg)["status"] == "host_missing", "live: Open Riptide with no host key ends in host_missing")
            pg.screenshot(path=os.path.join(SHOTS, "live-host-missing.png"))
        finally:
            ctx.close()

        # --- key present, app closed: stub app records how the host starts it
        marker = os.path.join(work, "launched.txt")
        stub_cs = os.path.join(work, "stub.cs")
        open(stub_cs, "w", encoding="utf-8").write(
            'using System; using System.IO; using System.Threading;\n'
            'class S { static void Main(string[] a) {\n'
            f'  File.AppendAllText(@"{marker}", Environment.CurrentDirectory + "|" + string.Join(" ", a) + "\\r\\n");\n'
            '  Thread.Sleep(3000); } }\n')
        stub_exe = os.path.join(app_dir, "riptide-desktop.exe")
        r = subprocess.run([csc(), "/nologo", "/target:winexe", f"/out:{stub_exe}", stub_cs], capture_output=True, text=True)
        assert r.returncode == 0, r.stdout + r.stderr
        host_exe = os.path.join(app_dir, "riptide-host.exe")
        shutil.copy(host_src, host_exe)
        manifest = os.path.join(work, "app.riptide.host.chromium.json")
        json.dump({"name": "app.riptide.host", "description": "e2e", "path": host_exe, "type": "stdio",
                   "allowed_origins": [f"chrome-extension://{EXT_ID}/"]}, open(manifest, "w", encoding="utf-8"))
        reg_write(manifest)
        env = {"RIPTIDE_IPC_ENDPOINT": r"\\.\pipe\riptide-e2e-" + uuid.uuid4().hex + "-none",
               "RIPTIDE_HOST_DATA_DIR": work}
        ctx, profile = launch(p, env)
        try:
            pg = ctx.new_page()
            pg.goto(f"chrome-extension://{EXT_ID}/popup/popup.html")
            pg.click("#open")
            pg.wait_for_selector("#open:not([disabled])", timeout=40000)
            s = state(pg)
            check(os.path.isfile(marker), "live: the host started the app from a path with spaces and non-ASCII")
            if os.path.isfile(marker):
                line = open(marker, encoding="utf-8", errors="replace").read().splitlines()[0]
                check(os.path.realpath(line.split("|")[0]).lower() == os.path.realpath(app_dir).lower(), f"live: app working directory is its own folder ({line!r})")
            check(s["status"] == "app_not_running", f"live: no pipe came up, status stays app_not_running ({s['status']})")
            check("did not start" in s["hint"], f"live: the failure is explained ({s['hint']!r})")
            pg.screenshot(path=os.path.join(SHOTS, "live-app-not-running.png"))
        finally:
            ctx.close()
    finally:
        reg_write(original)
        check(reg_read() == original, f"live: the real Chrome host key is back exactly as it was ({original!r})")
        shutil.rmtree(work, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--live", action="store_true")
    args = ap.parse_args()
    if not os.path.isfile(os.path.join(EXT, "manifest.json")):
        sys.exit("build the extension first: cd apps/extension && npm run build")
    with sync_playwright() as p:
        ctx, profile = launch(p)
        try:
            part1(ctx)
        finally:
            ctx.close()
            shutil.rmtree(profile, ignore_errors=True)
        if args.live:
            part2(p)
    print("screenshots:", SHOTS)
    if failures:
        print(f"FAILED ({len(failures)}):")
        for f in failures:
            print("  -", f)
        sys.exit(1)
    print("PASSED")


if __name__ == "__main__":
    main()
