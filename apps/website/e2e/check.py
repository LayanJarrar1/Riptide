#!/usr/bin/env python3
"""Smoke, layout and motion checks for the Riptide website. Builds nothing; run it against a served site.

    python apps/website/e2e/check.py http://127.0.0.1:8124/

Needs Python Playwright (`pip install playwright`). Uses the installed Microsoft Edge by default
(RIPTIDE_E2E_CHANNEL=chrome for Chrome, or empty for Playwright's own Chromium).
Exit code 0 when every check passes, 1 otherwise.
"""
import json
import os
import re
import sys
from pathlib import Path
from urllib.parse import urlparse

from playwright.sync_api import sync_playwright

BASE = (sys.argv[1] if len(sys.argv) > 1 else 'http://127.0.0.1:8124/').rstrip('/') + '/'

# The featured release comes from release.json (what the site is built from), so this check
# follows each release instead of naming one. RIPTIDE_RELEASE_AHEAD runs use RELEASE_VERSION in build.mjs.
_SITE = Path(__file__).resolve().parents[1]
def _featured_version() -> str:
    if os.environ.get('RIPTIDE_RELEASE_AHEAD') == '1':
        m = re.search(r"RELEASE_VERSION\s*=\s*'([^']+)'", (_SITE / 'build.mjs').read_text(encoding='utf8'))
        if m:
            return m.group(1)
    latest = json.loads((_SITE / 'release.json').read_text(encoding='utf8'))['latest']
    return (latest.get('prerelease') or latest.get('stable') or '').lstrip('v')
VER = _featured_version()
_N = re.search(r'-(?:alpha|beta|rc)\.(\d+)$', VER)
STAGE_EN = f"Beta {_N.group(1)}" if _N and '-beta.' in VER else VER
STAGE_AR = f"بيتا {_N.group(1)}" if _N and '-beta.' in VER else VER
CHANNEL = os.environ.get('RIPTIDE_E2E_CHANNEL', 'msedge') or None
VIEWPORTS = [(1920, 1080), (1440, 900), (1366, 768), (1280, 720), (1024, 768), (768, 1024), (600, 960), (390, 844), (360, 740), (320, 640)]
PAGES = [('en', BASE), ('ar', BASE + 'ar/')]
REL = 'https://github.com/LayanJarrar1/Riptide/releases'
FEATURES = json.loads((_SITE / 'src' / 'features.json').read_text(encoding='utf8'))['features']

results = []


def check(name, ok, detail=''):
    results.append((name, bool(ok), detail))
    print(('PASS ' if ok else 'FAIL ') + name + (f'  [{detail}]' if detail and not ok else ''))


def new_page(browser, w=1440, h=900, reduced=False, theme=None, js=True):
    ctx = browser.new_context(viewport={'width': w, 'height': h}, reduced_motion='reduce' if reduced else 'no-preference', java_script_enabled=js)
    if theme:
        ctx.add_init_script(f"try{{localStorage.setItem('riptide-theme','{theme}')}}catch(e){{}}")
    page = ctx.new_page()
    page.errors = []
    page.external = []
    page.on('console', lambda m: page.errors.append(m.text) if m.type == 'error' else None)
    page.on('pageerror', lambda e: page.errors.append(str(e)))
    origin = urlparse(BASE).netloc
    page.on('request', lambda r: page.external.append(r.url) if urlparse(r.url).netloc not in (origin, '') and not r.url.startswith('data:') else None)
    return ctx, page


def scroll_through(page, step=600, pause=60):
    total = page.evaluate('document.documentElement.scrollHeight')
    for y in range(0, total, step):
        page.evaluate(f'window.scrollTo(0, {y})')
        page.wait_for_timeout(pause)


HIDDEN = "Array.from(document.querySelectorAll('main *')).filter(e => e.getClientRects().length && parseFloat(getComputedStyle(e).opacity) < 0.05 && !e.closest('.sim-foot, details:not([open]), .shot-over')).map(e => e.tagName + '.' + e.className).slice(0, 6)"

with sync_playwright() as p:
    browser = p.chromium.launch(channel=CHANNEL, headless=True)

    # ---- Content, links and release data -----------------------------------------------------
    for lang, url in PAGES:
        ctx, page = new_page(browser)
        page.goto(url, wait_until='load')
        page.wait_for_timeout(600)
        tag = f'[{lang}]'
        check(f'{tag} html lang/dir', page.evaluate('[document.documentElement.lang, document.documentElement.dir]') == [lang, 'rtl' if lang == 'ar' else 'ltr'])
        check(f'{tag} default theme is light', page.evaluate('document.documentElement.dataset.theme') == 'light')
        check(f'{tag} no console errors', not page.errors, '; '.join(page.errors)[:200])
        check(f'{tag} no third-party requests', not page.external, ', '.join(page.external)[:200])
        check(f'{tag} no video element anywhere', page.locator('video').count() == 0)

        hrefs = page.eval_on_selector_all('.nav-links a', 'els => els.map(e => e.getAttribute("href"))')
        missing = [h for h in hrefs if not page.evaluate('id => !!document.getElementById(id)', h.lstrip('#'))]
        check(f'{tag} nav links resolve ({len(hrefs)})', hrefs and not missing, str(missing))
        internal = page.eval_on_selector_all('a[href^="#"]', 'els => els.map(e => e.getAttribute("href")).filter(h => h.length > 1)')
        check(f'{tag} every in-page link has a target', all(page.evaluate('id => !!document.getElementById(id)', h[1:]) for h in internal), str(internal))

        exact = {
            'installer': f'{REL}/download/v{VER}/Riptide_{VER}_x64-setup.exe',
            'portable': f'{REL}/download/v{VER}/Riptide-{VER}-portable.zip',
            'sums': f'{REL}/download/v{VER}/SHA256SUMS.txt',
        }
        rel_hrefs = page.eval_on_selector_all('a[href*="/releases"]', 'els => els.map(e => e.href)')
        allowed = set(exact.values()) | {REL, f'{REL}/tag/v{VER}'}
        check(f'{tag} every releases link is an expected {VER} URL', rel_hrefs and all(h in allowed for h in rel_hrefs), str([h for h in rel_hrefs if h not in allowed]))
        check(f'{tag} hero CTA is the {VER} installer and shows the version', page.eval_on_selector('.hero .btn-primary', 'e => e.href') == exact['installer'] and VER in page.text_content('.hero .btn-primary'))
        check(f'{tag} header Download button is the installer', page.eval_on_selector('.nav-actions .btn-primary', 'e => e.href') == exact['installer'])
        check(f'{tag} download cards are the installer and the portable zip', page.eval_on_selector('#download .dl-main', 'e => e.href') == exact['installer'] and page.eval_on_selector('#download .dl:not(.dl-main)', 'e => e.href') == exact['portable'])
        links = page.eval_on_selector_all('a[href]', 'els => els.map(e => e.href)')
        check(f'{tag} links to the checksums and the release notes', exact['sums'] in links and f'{REL}/tag/v{VER}' in links)
        body = page.content()
        check(f'{tag} badges say {STAGE_EN if lang == "en" else STAGE_AR}', (STAGE_EN in body) if lang == 'en' else (STAGE_AR in body))
        check(f'{tag} no soon/coming text and no social networks', not re.search(r'coming soon|\bsoon\b|قريبًا|قريباً', body, re.I) and not any(w in body.lower() for w in ('link' + 'edin', 'twitter.com', 'facebook.com')))
        check(f'{tag} footer shows the version', VER in page.text_content('.footer-meta'))

        # Compact page: the sections a launch visitor needs, nothing pinned or repeated.
        ids = page.evaluate("Array.from(document.querySelectorAll('main > section')).map(s => s.id || s.className)")
        check(f'{tag} sections: hero, proof, how, features, research, browser, privacy, faq, download ({len(ids)})', ids == ['hero', 'proof', 'how', 'features', 'research', 'browser', 'privacy', 'faq', 'download'], str(ids))
        check(f'{tag} proof strip has 4 facts', page.locator('.proof-list li').count() == 4)
        check(f'{tag} feature cards: 3 large, 8 small (video, files, torrent, queues, batch, grabber, no admin, no junk)', page.locator('.bento > .card').count() == 3 and page.locator('.mini-grid > li').count() == 8)
        txt = page.text_content('main')
        for word in ('yt-dlp', '144p', 'robots.txt', 'file[001-120].zip', '%LOCALAPPDATA%'):
            check(f'{tag} copy mentions {word}', word in txt)
        check(f'{tag} Research mentions the optional AI', page.locator('#research .callout').count() == 1)
        imgs = page.eval_on_selector_all('main img', 'els => els.filter(e => !e.closest(".tb")).map(e => [e.alt.length, !!e.getAttribute("width"), !!e.getAttribute("height")])')
        check(f'{tag} every content image has alt text and a size', imgs and all(a > 10 and w and h for a, w, h in imgs), str(imgs)[:160])
        lazy = page.eval_on_selector_all('#features img, #research img, #browser img', 'els => els.every(e => e.loading === "lazy")')
        check(f'{tag} below-the-fold screenshots are lazy', lazy)
        check(f'{tag} FAQ: 1-10 entries, all folded at load', 1 <= page.locator('details.faq-item').count() <= 10 and page.locator('details.faq-item[open]').count() == 0)
        check(f'{tag} 3 install steps for Chrome', page.locator('.steps li').count() == 3 and 'chrome://extensions' in page.text_content('.steps'))
        check(f'{tag} one JSON-LD SoftwareApplication with a free offer', page.evaluate("(() => { const s = Array.from(document.querySelectorAll('script[type=\"application/ld+json\"]')).map(e => JSON.parse(e.textContent)); return s.length === 1 && s[0]['@type'] === 'SoftwareApplication' && s[0].offers.price === '0'; })()"))

        # What's new (folded): 5-9 lines, every verified feature of the featured release by its own title.
        wn = page.locator('.whats-new li').count()
        check(f'{tag} whats-new list has 5-9 lines', 5 <= wn <= 9, str(wn))
        check(f'{tag} whats-new summary names the version', VER in page.text_content('#whats-new summary'))
        listed = page.eval_on_selector_all('.whats-new li > b', 'els => els.map(e => e.textContent.trim().replace(/\\.$/, ""))')
        for f in FEATURES:
            shown = f['release'] == VER and f['verified']
            check(f'{tag} features.json "{f["id"]}" listed exactly when verified for {VER}', (f[f'title_{lang}'] in listed) == shown)
        page.click('.hero .eyebrow a')
        page.wait_for_timeout(700)
        check(f'{tag} "See what\'s new" opens the release fold', page.evaluate("document.getElementById('whats-new').open") or page.evaluate("location.hash") == '#whats-new')
        ctx.close()

    # ---- Responsive: no horizontal overflow, nothing cut, at every width, both languages and themes ----
    OVER = """() => {
      const vw = document.documentElement.clientWidth, bad = [];
      document.querySelectorAll('main *, .site-header *, .site-footer *').forEach((e) => {
        if (!e.getClientRects().length || e.closest('.nav-menu') && innerWidth < 960) return;
        const r = e.getBoundingClientRect();
        if (r.width && (r.right > vw + 1 || r.left < -1)) bad.push(e.tagName.toLowerCase() + '.' + String(e.className).split(' ')[0] + ' ' + Math.round(r.left) + '..' + Math.round(r.right));
      });
      return { sw: document.documentElement.scrollWidth, vw, bad: bad.slice(0, 6) };
    }"""
    for lang, url in PAGES:
        for theme in ('light', 'dark'):
            for w, h in VIEWPORTS:
                ctx, page = new_page(browser, w, h, theme=theme)
                page.goto(url, wait_until='load')
                scroll_through(page, 900, 30)
                r = page.evaluate(OVER)
                check(f'[{lang} {theme} {w}] no horizontal overflow and nothing cut at the edges', r['sw'] <= r['vw'] and not r['bad'], str(r))
                ctx.close()

    # Phones: compact (no section taller than needed), readable text, big enough targets.
    for lang, url in PAGES:
        ctx, page = new_page(browser, 390, 844)
        page.goto(url, wait_until='load')
        total = page.evaluate('document.documentElement.scrollHeight')
        check(f'[{lang} 390] page stays compact (< 11000 px)', total < 11000, str(total))
        small = page.evaluate("Array.from(document.querySelectorAll('main p, main li, main span')).filter(e => e.getClientRects().length && e.textContent.trim().length > 20 && parseFloat(getComputedStyle(e).fontSize) < 12).length")
        check(f'[{lang} 390] no body text below 12 px', small == 0, str(small))
        tiny = page.evaluate("Array.from(document.querySelectorAll('.btn, .nav-toggle, .icon-btn, summary')).filter(e => e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden').map(e => e.getBoundingClientRect()).filter(r => r.height < 40).length")
        check(f'[{lang} 390] buttons and folds are at least 40 px tall', tiny == 0, str(tiny))
        ctx.close()

    # Header never overlaps the hero copy; the primary CTA is above the fold.
    for w, h in VIEWPORTS:
        for lang, url in PAGES:
            ctx, page = new_page(browser, w, h)
            page.goto(url, wait_until='load')
            page.wait_for_timeout(900)
            d = page.evaluate("""() => { const b = (s) => document.querySelector(s).getBoundingClientRect();
              const hd = b('.site-header'), c = b('.hero-copy'), cta = b('.hero .btn-primary');
              return { overlap: c.top < hd.bottom, cta: cta.bottom, vh: innerHeight }; }""")
            check(f'[{lang} {w}x{h}] header clear of the hero copy, primary CTA above the fold', not d['overlap'] and d['cta'] <= d['vh'], str(d))
            ctx.close()

    # ---- Motion ---------------------------------------------------------------------------------
    # Full motion: reveals finish after scrolling, nothing stays hidden, the simulation runs and pauses off-screen.
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(400)
    check('[motion] the hero simulation animates', page.evaluate("getComputedStyle(document.querySelector('.sim-pct')).animationName") == 'pct')
    scroll_through(page, 400, 120)
    page.wait_for_timeout(1500)
    hidden = page.evaluate(HIDDEN)
    check('[motion] after scrolling, every reveal is visible', not hidden, str(hidden))
    check('[motion] the simulation pauses off-screen', page.evaluate("document.querySelector('.sim').classList.contains('paused')"))
    page.evaluate('window.scrollTo(0, 0)')
    page.wait_for_timeout(400)
    check('[motion] and resumes when back in view', not page.evaluate("document.querySelector('.sim').classList.contains('paused')"))
    anim = page.evaluate("Array.from(document.querySelectorAll('*')).flatMap(e => e.getAnimations ? e.getAnimations() : []).map(a => a.effect && a.effect.getKeyframes()).flat().filter(Boolean).flatMap(k => Object.keys(k)).filter(k => !['offset', 'easing', 'composite', 'computedOffset', 'transform', 'opacity', '--pct'].includes(k))")
    check('[motion] only transform, opacity and the counter animate', not anim, str(sorted(set(anim)))[:160])
    page.evaluate("window.__lt = 0; new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__lt += e.duration; }).observe({ type: 'longtask', buffered: false });")
    for y in range(0, 4000, 200):
        page.mouse.wheel(0, 200)
        page.wait_for_timeout(30)
    page.wait_for_timeout(300)
    check('[motion] scrolling causes no long main-thread tasks (< 100 ms total)', page.evaluate('window.__lt') < 100, str(page.evaluate('window.__lt')))
    ctx.close()

    # Reduced motion: no animation runs, nothing starts hidden.
    ctx, page = new_page(browser, 1440, 900, reduced=True)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(300)
    running = page.evaluate("document.getAnimations().filter(a => a.playState === 'running' && a.effect && a.effect.getComputedTiming().duration > 5).length")
    check('[reduced] no animation runs', running == 0, str(running))
    page.evaluate('window.scrollTo(0, 1800)')
    page.wait_for_timeout(200)
    check('[reduced] reveals are visible without waiting', not page.evaluate(HIDDEN), str(page.evaluate(HIDDEN)))
    check('[reduced] the simulation shows its end state (verified, not animated)', page.evaluate("[getComputedStyle(document.querySelector('.sim-ok')).opacity, getComputedStyle(document.querySelector('.sim-pct')).animationName, getComputedStyle(document.querySelector('.sim-run')).display]") == ['1', 'none', 'none'])
    ctx.close()

    # Robustness: layout shift, missing script, missing IntersectionObserver, JavaScript off.
    ctx, page = new_page(browser, 1440, 900)
    page.add_init_script("window.__cls = 0; new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: 'layout-shift', buffered: true });")
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(800)
    scroll_through(page, 500, 100)
    check('[load] cumulative layout shift stays under 0.02', page.evaluate('window.__cls') < 0.02, str(page.evaluate('window.__cls')))
    ctx.close()
    ctx, page = new_page(browser, 1440, 900)
    page.route('**/js/site.js*', lambda route: route.abort())
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(3400)
    check('[failsafe] script missing: nothing is hidden', page.evaluate("document.documentElement.classList.contains('rv-all')") and not page.evaluate(HIDDEN))
    ctx.close()
    ctx = browser.new_context(viewport={'width': 1440, 'height': 900})
    ctx.add_init_script('delete window.IntersectionObserver;')
    page = ctx.new_page()
    page.goto(BASE, wait_until='load')
    page.evaluate('window.scrollTo(0, 3000)')
    page.wait_for_timeout(800)
    check('[fallback] without IntersectionObserver every reveal is visible', not page.evaluate(HIDDEN))
    ctx.close()
    ctx, page = new_page(browser, 1440, 900, js=False)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(1000)
    check('[no-js] headline, CTA and every section visible', page.is_visible('h1') and page.is_visible('.hero .btn-primary') and page.locator('main > section').count() == 9 and not page.evaluate(HIDDEN))
    ctx.close()

    # ---- Header, menu and theme -------------------------------------------------------------------
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE, wait_until='load')
    page.evaluate('window.scrollTo(0, 2600)')
    page.wait_for_timeout(400)
    top = page.evaluate("(() => { const h = document.querySelector('.site-header'); const r = h.getBoundingClientRect(); const e = document.elementFromPoint(r.left + r.width / 2, r.top + r.height - 4); return { onTop: !!e && h.contains(e), height: r.height, solid: h.classList.contains('is-solid') }; })()")
    check('[header] stays on top, compact (<= 60 px) and gets its hairline after scrolling', top['onTop'] and top['height'] <= 60 and top['solid'], str(top))
    check('[header] the menu marks the section in view', page.locator('.nav-links a[aria-current="true"]').count() == 1)
    page.click('.theme-toggle')
    page.wait_for_timeout(200)
    check('[theme] toggle switches to dark and stores it', page.evaluate("[document.documentElement.dataset.theme, localStorage.getItem('riptide-theme')]") == ['dark', 'dark'])
    page.reload(wait_until='load')
    check('[theme] the choice persists', page.evaluate('document.documentElement.dataset.theme') == 'dark')
    ctx.close()
    ctx, page = new_page(browser, 390, 844)
    page.goto(BASE, wait_until='load')
    check('[menu] closed on phones at load', not page.is_visible('.nav-links'))
    page.click('.nav-toggle')
    page.wait_for_timeout(400)
    check('[menu] opens with the links and the download button', page.is_visible('.nav-links') and page.is_visible('.nav-actions .btn-primary') and page.get_attribute('.nav-toggle', 'aria-expanded') == 'true')
    page.keyboard.press('Escape')
    page.wait_for_timeout(400)
    check('[menu] Escape closes it', not page.is_visible('.nav-links') and page.get_attribute('.nav-toggle', 'aria-expanded') == 'false')
    page.click('.nav-toggle')
    page.wait_for_timeout(300)
    page.click('.nav-links a[href="#faq"]')
    page.wait_for_timeout(900)
    check('[menu] a link closes the menu and lands on its section', not page.is_visible('.nav-links') and abs(page.evaluate("document.getElementById('faq').getBoundingClientRect().top") - 68) < 40)
    ctx.close()
    ctx, page = new_page(browser, 1440, 900)
    page.add_init_script("Object.defineProperty(window, 'localStorage', { get() { throw new Error('blocked'); } });")
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(400)
    page.click('.theme-toggle')
    check('[storage blocked] the page works and the theme still switches', not page.errors and page.evaluate('document.documentElement.dataset.theme') == 'dark', '; '.join(page.errors)[:160])
    ctx.close()

    # ---- Other pages --------------------------------------------------------------------------------
    for lang, url in PAGES:
        for path, label in (('privacy/', 'privacy'), ('404.html', '404')):
            for w in (1440, 390):
                ctx, page = new_page(browser, w, 900)
                page.goto(url + path if label == 'privacy' else BASE + path, wait_until='load')
                r = page.evaluate(OVER)
                # The 404 page uses the deployed base path for its assets, so locally only its layout is checked.
                errs = [e for e in page.errors if label == 'privacy']
                check(f'[{lang} {label} {w}] loads without errors or overflow', not errs and r['sw'] <= r['vw'], str(r) + '; '.join(errs)[:100])
                ctx.close()
    ctx, page = new_page(browser)
    resp = page.request.get(BASE + 'release.json')
    check('[release] release.json is served', resp.ok, str(resp.status))
    ctx.close()

    browser.close()

failed = [r for r in results if not r[1]]
print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
sys.exit(1 if failed else 0)
