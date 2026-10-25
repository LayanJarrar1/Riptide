#!/usr/bin/env python3
"""Smoke and layout checks for the Riptide website. Builds nothing; run it against a served site.

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
from urllib.parse import urljoin, urlparse

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
VIEWPORTS = [(1920, 1080), (1440, 900), (1366, 768), (1280, 720), (1350, 530), (768, 1024), (390, 844)]
PAGES = [('en', BASE), ('ar', BASE + 'ar/')]

results = []


def check(name, ok, detail=''):
    results.append((name, bool(ok), detail))
    print(('PASS ' if ok else 'FAIL ') + name + (f'  [{detail}]' if detail and not ok else ''))


def new_page(browser, w=1440, h=900, reduced=False, theme=None):
    ctx = browser.new_context(viewport={'width': w, 'height': h}, reduced_motion='reduce' if reduced else 'no-preference')
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


with sync_playwright() as p:
    browser = p.chromium.launch(channel=CHANNEL, headless=True)

    for lang, url in PAGES:
        ctx, page = new_page(browser)
        page.goto(url, wait_until='load')
        page.wait_for_timeout(800)
        tag = f'[{lang}]'

        # Document basics
        check(f'{tag} html lang/dir', page.evaluate("[document.documentElement.lang, document.documentElement.dir]") == [lang, 'rtl' if lang == 'ar' else 'ltr'])
        check(f'{tag} default theme is light', page.evaluate("document.documentElement.dataset.theme") == 'light')
        check(f'{tag} no video element anywhere', page.locator('video').count() == 0)
        check(f'{tag} no console errors', not page.errors, '; '.join(page.errors)[:200])
        check(f'{tag} no third-party requests', not page.external, ', '.join(page.external)[:200])

        # Nav links resolve to sections that exist
        hrefs = page.eval_on_selector_all('.nav-links a', 'els => els.map(e => e.getAttribute("href"))')
        missing = [h for h in hrefs if not page.evaluate("id => !!document.getElementById(id)", h.lstrip('#'))]
        check(f'{tag} nav links resolve ({len(hrefs)})', hrefs and not missing, str(missing))

        # Download links point at release assets
        dl = page.eval_on_selector_all('a[href*="/releases/download/"]', 'els => els.map(e => e.href)')
        ok = len(dl) >= 4 and all(('/releases/download/v' in h) for h in dl)
        check(f'{tag} download links point to release assets ({len(dl)})', ok, str(dl[:3]))
        hero_dl = page.eval_on_selector('.hero .btn-primary', 'e => e.href')
        check(f'{tag} hero CTA is the installer', hero_dl.endswith('x64-setup.exe'), hero_dl)
        REL = 'https://github.com/LayanJarrar1/Riptide/releases'
        check(f'{tag} hero CTA is the {VER} installer', hero_dl == f'{REL}/download/v{VER}/Riptide_{VER}_x64-setup.exe', hero_dl)
        links = page.eval_on_selector_all('a[href]', 'els => els.map(e => e.href)')
        for want in (f'{REL}/download/v{VER}/Riptide-{VER}-portable.zip', f'{REL}/download/v{VER}/SHA256SUMS.txt', f'{REL}/tag/v{VER}'):
            check(f'{tag} links to {want.split("/")[-1]}', want in links)
        EXACT = {
            'installer': f'{REL}/download/v{VER}/Riptide_{VER}_x64-setup.exe',
            'portable': f'{REL}/download/v{VER}/Riptide-{VER}-portable.zip',
            'sums': f'{REL}/download/v{VER}/SHA256SUMS.txt',
        }
        rel_hrefs = page.eval_on_selector_all('a[href*="/releases"]', 'els => els.map(e => e.href)')
        allowed = set(EXACT.values()) | {REL, f'{REL}/tag/v{VER}'}
        check(f'{tag} every releases href is exactly an expected release URL', rel_hrefs and all(h in allowed for h in rel_hrefs), str([h for h in rel_hrefs if h not in allowed]))
        main_dl = page.eval_on_selector('#download .dl-main', 'e => e.href'); port = page.eval_on_selector('#download .dl:not(.dl-main)', 'e => e.href')
        check(f'{tag} band installer and portable are the exact asset URLs', main_dl == EXACT['installer'] and port == EXACT['portable'], f'{main_dl} {port}')
        check(f'{tag} header Download button is the installer', page.eval_on_selector('.nav-actions .btn-primary', 'e => e.href') == EXACT['installer'])
        body = page.content()
        check(f'{tag} no leftover alpha 3 or 4', not re.search(r'alpha[ .\-]?[34]|ألفا [34]', body, re.I))
        check(f'{tag} badges say {STAGE_EN}', (STAGE_EN in body) if lang == 'en' else (STAGE_AR in body))
        check(f'{tag} no store tiles and no soon/coming text', page.locator('.stores, .store').count() == 0 and not re.search(r'coming soon|\bsoon\b|قريبًا|قريباً', page.content(), re.I))
        check(f'{tag} hero CTA shows the version', VER in page.text_content('.hero .btn-primary'))
        check(f'{tag} footer shows the version', VER in page.text_content('.footer-meta'))
        check(f'{tag} download band has the version', VER in page.text_content('#download .dl-main'))
        wn = page.locator('.whats-new li').count()
        check(f'{tag} whats-new list has 5-9 lines', 5 <= wn <= 9, str(wn))
        check(f'{tag} whats-new heading names the version', VER in page.text_content('.whats-new h3'))
        check(f'{tag} always-up-to-date block present', page.locator('.always-up p').text_content().count('GitHub') == 1)
        check(f'{tag} 3 install steps for Chrome', page.locator('.steps li').count() == 3 and 'chrome://extensions' in page.text_content('.steps'))
        check(f'{tag} feature row has queues, batch, grabber', page.locator('.more li').count() == 3)
        txt = page.text_content('main')
        for word in ('yt-dlp', '144p', 'robots.txt', 'file[001-120].zip'):
            check(f'{tag} copy mentions {word}', word in txt)
        check(f'{tag} FAQ has at most 8 entries', 1 <= page.locator('details.faq-item').count() <= 8)
        new_imgs = page.eval_on_selector_all('#new img', 'els => els.map(e => [e.getAttribute("src"), e.getAttribute("alt"), e.getAttribute("width"), e.getAttribute("height"), e.getAttribute("loading")])')
        check(f'{tag} new-in-beta-1 block has 4 lazy images with alt, width and height', len(new_imgs) == 4 and all(a and b and c and d and e == 'lazy' for a, b, c, d, e in new_imgs), str(new_imgs)[:200])
        check(f'{tag} new-in-beta-1 images are the {lang} ones', all(f'-{lang}.webp' in a for a, *_ in new_imgs))
        check(f'{tag} open-sources policy note is shown', page.locator('#new .policy').count() == 1 and (('piracy' in page.text_content('#new .policy')) if lang == 'en' else ('القرصنة' in page.text_content('#new .policy'))))
        check(f'{tag} new block title names the release', (STAGE_EN if lang == 'en' else STAGE_AR) in page.text_content('#new .new-title'))
        check(f'{tag} no social-network mentions', not any(w in page.content().lower() for w in ('link' + 'edin', 'twitter.com', 'facebook.com')))
        wn_text = page.text_content('.whats-new')
        # every verified feature of the featured release is listed, by its own title
        feats = json.loads((_SITE / 'src' / 'features.json').read_text(encoding='utf8'))['features']
        titles = [f['title_' + lang] for f in feats if f.get('verified') and f.get('release') == VER]
        check(f'{tag} whats-new lists the features of {VER} ({len(titles)})', bool(titles) and all(t in wn_text for t in titles), str([t for t in titles if t not in wn_text]))

        # Logo is the unmodified brand-kit file, and no filter or blend is applied to it
        srcs = page.eval_on_selector_all('.brand-logo, .footer-logo', 'els => els.map(e => e.getAttribute("src"))')
        check(f'{tag} logos are the brand-kit SVGs', all(s.endswith(('riptide-logo-light.svg', 'riptide-logo-dark.svg')) for s in srcs), str(srcs))
        bad = page.evaluate("""() => Array.from(document.querySelectorAll('.brand-logo, .footer-logo, .hl-svg, .hl-svg *')).filter(e => { const c = getComputedStyle(e); return (c.filter && c.filter !== 'none') || (c.mixBlendMode && c.mixBlendMode !== 'normal'); }).length""")
        check(f'{tag} no filter or blend mode on the logo', bad == 0, str(bad))

        # Scroll behaviour untouched: no smooth-scroll, default cursor
        sb = page.evaluate("[getComputedStyle(document.documentElement).scrollBehavior, getComputedStyle(document.body).cursor]")
        check(f'{tag} native scroll and cursor', sb == ['auto', 'auto'], str(sb))
        ctx.close()

    # Theme toggle persists across a reload; language switch works both ways
    ctx, page = new_page(browser)
    page.goto(BASE, wait_until='load')
    page.click('.theme-toggle')
    check('theme toggle switches to dark', page.evaluate("document.documentElement.dataset.theme") == 'dark')
    page.reload(wait_until='load')
    check('theme choice persists after reload', page.evaluate("document.documentElement.dataset.theme") == 'dark')
    page.click('.theme-toggle')
    check('theme toggle switches back to light', page.evaluate("document.documentElement.dataset.theme") == 'light')
    page.click('.nav-lang')
    page.wait_for_load_state('load')
    check('language switch goes to Arabic (rtl)', page.url.rstrip('/').endswith('/ar') and page.evaluate("document.documentElement.dir") == 'rtl', page.url)
    page.click('.nav-lang')
    page.wait_for_load_state('load')
    check('language switch goes back to English (ltr)', page.evaluate("document.documentElement.lang") == 'en', page.url)
    ctx.close()

    # Mobile menu opens and closes
    ctx, page = new_page(browser, 390, 844)
    page.goto(BASE, wait_until='load')
    page.click('.nav-toggle')
    check('mobile menu opens', page.get_attribute('.nav-toggle', 'aria-expanded') == 'true' and page.is_visible('.nav-menu'))
    page.keyboard.press('Escape')
    check('mobile menu closes on Escape', page.get_attribute('.nav-toggle', 'aria-expanded') == 'false')
    ctx.close()

    # No horizontal overflow at the four viewports, both languages, both themes
    for w, h in VIEWPORTS:
        for lang, url in PAGES:
            for theme in ('light', 'dark'):
                ctx, page = new_page(browser, w, h, theme=theme)
                page.goto(url, wait_until='load')
                page.wait_for_timeout(500)
                ov = page.evaluate("document.documentElement.scrollWidth - document.documentElement.clientWidth")
                # also after scrolling through the page once (lazy content, pinned stages)
                total = page.evaluate("document.documentElement.scrollHeight")
                for y in range(0, total, max(300, h)):
                    page.evaluate(f"window.scrollTo(0, {y})")
                    page.wait_for_timeout(40)
                ov2 = page.evaluate("document.documentElement.scrollWidth - document.documentElement.clientWidth")
                check(f'no horizontal overflow {w}x{h} {lang} {theme}', ov <= 0 and ov2 <= 0, f'{ov}/{ov2}')
                check(f'no console errors {w}x{h} {lang} {theme}', not page.errors, '; '.join(page.errors)[:160])
                ctx.close()

    # ---- Calm motion (the OS asks for less): alive, but no parallax, no pinning, no endless loops ----
    for lang, url in PAGES:
        ctx, page = new_page(browser, 1440, 900, reduced=True)
        page.goto(url, wait_until='load')
        page.wait_for_timeout(1800)
        tag = f'[{lang} calm]'
        check(f'{tag} calm class, no anim class', page.evaluate("[document.documentElement.classList.contains('calm'), document.documentElement.classList.contains('anim')]") == [True, False])
        check(f'{tag} js-ready and fonts-ready set', page.evaluate("[document.documentElement.classList.contains('js-ready'), document.documentElement.classList.contains('ready')]") == [True, True])
        sticky = page.evaluate("Array.from(document.querySelectorAll('.pin')).filter(e => getComputedStyle(e).position === 'sticky').length")
        check(f'{tag} nothing pinned', sticky == 0, str(sticky))
        check(f'{tag} CTA and headline visible without scrolling', page.is_visible('.hero .btn-primary') and page.is_visible('h1'))
        # alive: the hero simulation counts, and fades are running or done
        a = page.inner_text('[data-o="pct"]') + page.inner_text('[data-o="speed"]')
        page.wait_for_timeout(1500)
        b = page.inner_text('[data-o="pct"]') + page.inner_text('[data-o="speed"]')
        check(f'{tag} hero simulation counts', a != b, f'{a} -> {b}')
        check(f'{tag} app preview progress bars animate (fill)', page.evaluate("document.getAnimations().some(a => a.animationName === 'rt-fill')") or page.evaluate("getComputedStyle(document.querySelector('.rt-d .rt-pg b')).transform") != 'none')
        # no spatial or endless motion
        page.evaluate("window.scrollTo(0, 400)")
        page.wait_for_timeout(500)
        hp = page.evaluate("document.getElementById('hero').style.getPropertyValue('--hp')")
        par = page.evaluate("getComputedStyle(document.querySelector('.hero .par')).transform")
        check(f'{tag} no hero parallax', hp in ('', '0') and par == 'none', f'{hp!r} {par!r}')
        loops = page.evaluate("document.getAnimations().filter(a => a.effect && a.effect.getTiming().iterations === Infinity && a.playState === 'running').map(a => a.animationName)")
        check(f'{tag} no endless loops running', not loops, str(loops))
        # reveals are opacity fades: scrolling through shows everything, nothing translated
        total = page.evaluate("document.documentElement.scrollHeight")
        for y in range(0, total, 450):
            page.evaluate(f"window.scrollTo(0, {y})")
            page.wait_for_timeout(120)
        page.wait_for_timeout(1500)
        hidden = page.evaluate("Array.from(document.querySelectorAll('.rv, .hero-copy .rise')).filter(e => getComputedStyle(e).opacity === '0').length")
        check(f'{tag} every reveal ends visible after scrolling through', hidden == 0, str(hidden))
        moved = page.evaluate("Array.from(document.querySelectorAll('.rv.in')).filter(e => { const t = getComputedStyle(e).transform; return t !== 'none' && t !== 'matrix(1, 0, 0, 1, 0, 0)'; }).length")
        check(f'{tag} reveals use no translation', moved == 0, str(moved))
        # one-shot chapters finished (SHA digest resolved, race at 100 %)
        try:
            page.wait_for_function("document.querySelector('#speed .pin').style.getPropertyValue('--p') === '1' && document.querySelector('#integrity .pin').style.getPropertyValue('--p') === '1'", timeout=9000)
        except Exception:
            pass
        fin = page.evaluate("[document.querySelector('#speed [data-o=pct]').textContent, document.querySelector('#integrity [data-o=ok]').textContent.length > 20, document.querySelector('#speed .pin').style.getPropertyValue('--p')]")
        check(f'{tag} one-shot chapters reach their end state', fin[0] == '100%' and fin[1] and fin[2] == '1', str(fin))
        check(f'{tag} no console errors', not page.errors, '; '.join(page.errors)[:200])
        ctx.close()

    # ---- First-visit hint when the OS reduces motion ----
    for lang, url in PAGES:
        ctx, page = new_page(browser, 1440, 900, reduced=True)
        page.goto(url, wait_until='load')
        page.wait_for_timeout(500)
        tag = f'[{lang} hint]'
        check(f'{tag} shown on first visit', page.is_visible('.mo-hint'))
        box = page.evaluate("(() => { const r = document.querySelector('.mo-hint').getBoundingClientRect(); const h = document.querySelector('.site-header').getBoundingClientRect(); return [r.top >= h.bottom - 1, r.right <= innerWidth && r.left >= 0]; })()")
        check(f'{tag} sits under the header inside the viewport', box == [True, True], str(box))
        shift = page.evaluate("document.querySelector('h1').getBoundingClientRect().top")
        page.click('.mo-x')
        check(f'{tag} dismiss hides it', not page.is_visible('.mo-hint'))
        check(f'{tag} dismiss causes no layout shift', abs(page.evaluate("document.querySelector('h1').getBoundingClientRect().top") - shift) < 0.5)
        page.reload(wait_until='load')
        page.wait_for_timeout(300)
        check(f'{tag} dismissal persists', not page.is_visible('.mo-hint'))
        ctx.close()
        ctx, page = new_page(browser, 1440, 900, reduced=True)
        page.goto(url, wait_until='load')
        page.click('.mo-on')
        page.wait_for_load_state('load')
        page.wait_for_timeout(400)
        check(f'{tag} "On" turns full motion on', page.evaluate("[document.documentElement.dataset.motion, localStorage.getItem('riptide-motion')]") == ['full', 'on'])
        check(f'{tag} not shown once a choice exists', not page.is_visible('.mo-hint'))
        ctx.close()
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE, wait_until='load')
    check('[hint] never shown without an OS reduce setting', not page.is_visible('.mo-hint'))
    ctx.close()

    # ---- Pinned stage budget and numbered labels ----
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(500)
    ch = page.evaluate("Array.from(document.querySelectorAll('main > section.ch')).map(s => [s.id, getComputedStyle(s.querySelector('.pin')).position, (s.offsetHeight - innerHeight) / innerHeight])")
    pinned = {c[0]: c[2] for c in ch if c[1] == 'sticky'}
    check('[stages] only the logo and app chapters are pinned', set(pinned) == {'streams', 'app'}, str(ch))
    check('[stages] streams stage scrolls <= 1.3 viewport heights', 0.8 <= pinned.get('streams', 9) <= 1.3, str(pinned))
    check('[stages] app stage scrolls <= 0.8 viewport heights', pinned.get('app', 9) <= 0.8, str(pinned))
    ctx.close()
    ctx, page = new_page(browser, 390, 844)
    page.goto(BASE + '?motion=on', wait_until='load')
    sticky = page.evaluate("Array.from(document.querySelectorAll('.pin')).filter(e => getComputedStyle(e).position === 'sticky').length")
    check('[stages] nothing is pinned on phones, even with full motion', sticky == 0, str(sticky))
    ctx.close()
    for lang, url in PAGES:
        for path, label in (('', 'home'), ('privacy/', 'privacy'), ('404.html', '404')):
            ctx, page = new_page(browser)
            page.goto((url if path == '' else url + path) if path != '404.html' else BASE + '404.html', wait_until='load')
            html = page.content()
            texts = page.evaluate("Array.from(document.querySelectorAll('.eyebrow, h1, h2, h3, .h2, .h3, .panel-tag')).map(e => e.textContent.trim())")
            bad = [t for t in texts if re.match(r'^\d{1,2}\s*/\s*\S', t)]
            check(f'[{lang} {label}] no "NN /" numbered labels', not bad, str(bad))
            nxt = re.findall(r'Next release|الإصدار القادم', html)
            check(f'[{lang} {label}] no "Next release" tags', not nxt, str(nxt))
            if label == 'home':
                check(f'[{lang} home] app images carry no marker dots or legend', page.locator('.dot, .legend, .lg').count() == 0)
                eb = page.locator('.eyebrow').count()
                check(f'[{lang} home] only the status badges remain as labels ({eb})', eb == 2)
            ctx.close()

    # ---- Download band: the stream line runs under the cards (z-order), never over them, and is gone on narrow screens ----
    for lang, url in PAGES:
        for w, h_ in ((1920, 1080), (1440, 900), (1366, 768), (1280, 720), (768, 1024), (390, 844)):
            ctx, page = new_page(browser, w, h_)
            page.goto(url, wait_until='load')
            page.evaluate("window.scrollTo(0, document.querySelector('.dl-grid').getBoundingClientRect().top + scrollY - 160)")  # the band is taller than a viewport now: aim at the cards
            page.wait_for_timeout(1200)
            if w >= 1100:
                res = page.evaluate("""() => Array.from(document.querySelectorAll('.dl')).map((e) => {
                  const b = e.getBoundingClientRect(); const rtl = document.dir === 'rtl';
                  return [rtl ? b.left + 3 : b.right - 3, rtl ? b.right - 3 : b.left + 3].map((x) => { const t = document.elementFromPoint(x, b.top + b.height / 2); return !!t && e.contains(t); });
                })""")
                z = page.evaluate("(() => { const l = getComputedStyle(document.querySelector('.cta-lines')); const g = getComputedStyle(document.querySelector('.dl-grid')); return [l.zIndex, g.zIndex]; })()")
                check(f'[{lang} band {w}] stream line is under both cards at both edges (elementFromPoint)', all(all(r) for r in res) and len(res) == 2, str(res))
                check(f'[{lang} band {w}] cards stack above the stream', int(z[1]) > int(z[0]), str(z))
                ov = page.evaluate("document.documentElement.scrollWidth - document.documentElement.clientWidth")
                check(f'[{lang} band {w}] no horizontal overflow', ov <= 0, str(ov))
            else:
                shown = page.evaluate("getComputedStyle(document.querySelector('.cta-lines')).display")
                check(f'[{lang} band {w}] stream is hidden on narrow screens', shown == 'none', shown)
            ctx.close()

    # ---- Reliability of the scroll engine (full motion) ----
    def p_of(page, sel='#streams .pin'):
        return page.evaluate(f"parseFloat(document.querySelector('{sel}').style.getPropertyValue('--p'))")

    def expected(page, sel='#streams'):
        return page.evaluate(f"""(() => {{ const s = document.querySelector('{sel}'); const top = s.getBoundingClientRect().top + scrollY; const len = s.offsetHeight - innerHeight; const raw = scrollY < top ? (scrollY - top) / innerHeight : Math.min(1, (scrollY - top) / len); return raw < 0 ? raw : Math.min(1, raw / 0.85); }})()""")

    def stage_y(page, frac, sel='#streams'):
        y = page.evaluate(f"(() => {{ const s = document.querySelector('{sel}'); return s.getBoundingClientRect().top + scrollY + (s.offsetHeight - innerHeight) * {frac}; }})()")
        page.evaluate(f"window.scrollTo(0, {y})")
        page.wait_for_timeout(900)

    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(600)
    stage_y(page, 0.5)
    p1, e1 = p_of(page), expected(page)
    check('[engine] progress follows the scroll position (eased value settles)', abs(p1 - e1) < 0.02, f'{p1} vs {e1}')
    stage_y(page, 0.95)
    p2 = p_of(page)
    check('[engine] stage reaches its end state before the pin releases (>= 95 % of the stage)', p2 == 1.0, str(p2))
    stage_y(page, 0.0)
    page.evaluate("window.scrollTo(0, 0)")
    page.wait_for_timeout(900)
    check('[engine] scrolling back up reverses to the start', p_of(page) <= 0, str(p_of(page)))
    # smoothing is on the value only: after a jump the value is still moving a frame later, native scroll is instant
    page.evaluate("window.scrollTo(0, 0)")
    y = page.evaluate("(() => { const s = document.querySelector('#streams'); return s.getBoundingClientRect().top + scrollY + (s.offsetHeight - innerHeight) * 0.5; })()")
    page.evaluate(f"window.scrollTo(0, {y})")
    check('[engine] native scroll position is exact immediately (no scroll-jacking)', abs(page.evaluate("scrollY") - y) < 1)
    # mid-page reload
    stage_y(page, 0.5)
    before = (p_of(page), page.evaluate("getComputedStyle(document.querySelector('#streams .pin')).getPropertyValue('--ms')"))
    page.reload(wait_until='load')
    page.wait_for_timeout(1500)
    after = (p_of(page), page.evaluate("getComputedStyle(document.querySelector('#streams .pin')).getPropertyValue('--ms')"))
    check('[engine] mid-page reload restores the stage state', abs(before[0] - after[0]) < 0.03 and after[0] == after[0], f'{before} -> {after}')
    # back/forward (navigate away and back)
    page.goto(BASE + 'privacy/', wait_until='load')
    page.go_back(wait_until='load')
    page.wait_for_timeout(1500)
    for _ in range(20):  # the restored state settles after the scroll position does; poll instead of a fixed wait
        pb = p_of(page)
        if pb == pb and abs(pb - expected(page)) < 0.03:
            break
        page.wait_for_timeout(500)
    check('[engine] back/forward restores the stage state', abs(pb - expected(page)) < 0.03 and pb == pb, f'{pb} vs {expected(page)}')
    # anchor jump
    page.evaluate("window.scrollTo(0, 0)")
    page.wait_for_timeout(300)
    page.click('.nav-links a[href="#integrity"]')
    page.wait_for_timeout(5500)
    cur = page.evaluate("[document.querySelector('#integrity [data-o=ok]').textContent.length > 20, document.querySelector('#integrity .pin').style.getPropertyValue('--p')]")
    check('[engine] anchor jump lands on a finished chapter', cur[0] and cur[1] == '1', str(cur))
    # resize 1440 -> 390 -> 1440
    stage_y(page, 0.5)
    page.set_viewport_size({'width': 390, 'height': 844})
    page.wait_for_timeout(1200)
    mid = page.evaluate("[document.querySelector('#streams').dataset.mode, getComputedStyle(document.querySelector('#streams .pin')).position]")
    check('[engine] phone width unpins the stage', mid[1] != 'sticky', str(mid))
    ov = page.evaluate("document.documentElement.scrollWidth - document.documentElement.clientWidth")
    check('[engine] no overflow after shrinking', ov <= 0, str(ov))
    page.set_viewport_size({'width': 1440, 'height': 900})
    page.wait_for_timeout(1200)
    stage_y(page, 0.5)
    p3, e3 = p_of(page), expected(page)
    check('[engine] stage progress is right again after 1440 -> 390 -> 1440', abs(p3 - e3) < 0.03 and p3 == p3, f'{p3} vs {e3}')
    clamped = page.evaluate("Array.from(document.querySelectorAll('.pin')).every(p => { const v = parseFloat(p.style.getPropertyValue('--p')); return !(v > 1.0001) && !(v < -1.0001); })")
    check('[engine] every progress value stays in [-1, 1]', clamped)
    check('[engine] no console errors across the sequence', not page.errors, '; '.join(page.errors)[:200])
    ctx.close()

    # hidden tab pauses the simulation and resumes cleanly
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(2500)
    page.evaluate("Object.defineProperty(document, 'hidden', { configurable: true, get: () => true }); document.dispatchEvent(new Event('visibilitychange'));")
    page.wait_for_timeout(300)
    a = page.inner_text('[data-o="pct"]') + page.inner_text('[data-o="speed"]')
    page.wait_for_timeout(1500)
    b = page.inner_text('[data-o="pct"]') + page.inner_text('[data-o="speed"]')
    check('[tab] simulation pauses while the tab is hidden', a == b and page.evaluate("document.documentElement.classList.contains('tab-hidden')"), f'{a} {b}')
    page.evaluate("Object.defineProperty(document, 'hidden', { configurable: true, get: () => false }); document.dispatchEvent(new Event('visibilitychange'));")
    page.wait_for_timeout(1500)
    c = page.inner_text('[data-o="pct"]') + page.inner_text('[data-o="speed"]')
    check('[tab] simulation resumes when the tab is visible', c != b and not page.evaluate("document.documentElement.classList.contains('tab-hidden')"), f'{b} {c}')
    check('[tab] no console errors', not page.errors)
    ctx.close()

    # ---- Adaptive performance ----
    for label, script in (('hardwareConcurrency <= 2', "Object.defineProperty(navigator, 'hardwareConcurrency', { get: () => 2 });"),
                          ('deviceMemory <= 2', "Object.defineProperty(navigator, 'deviceMemory', { get: () => 2 });"),
                          ('saveData', "Object.defineProperty(navigator, 'connection', { get: () => ({ saveData: true }) });")):
        ctx = browser.new_context(viewport={'width': 1440, 'height': 900})
        ctx.add_init_script(script)
        page = ctx.new_page()
        page.goto(BASE + '?motion=on', wait_until='load')
        check(f'[lite] starts in lite on {label}', page.evaluate("document.documentElement.dataset.level") == 'lite')
        ctx.close()
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(500)
    check('[lite] normal device starts at the full level', page.evaluate("document.documentElement.dataset.level") == 'full')
    cdp = ctx.new_cdp_session(page)
    cdp.send('Emulation.setCPUThrottlingRate', {'rate': 20})
    # keep scrolling with some main-thread load so frames are slow
    for i in range(70):
        page.evaluate(f"window.scrollBy(0, 40); (() => {{ const t = performance.now(); while (performance.now() - t < 12); }})()")
        page.wait_for_timeout(20)
    page.wait_for_timeout(500)
    lvl = page.evaluate("[document.documentElement.dataset.level, sessionStorage.getItem('riptide-lite')]")
    check('[lite] sustained slow frames switch to lite and remember it', lvl == ['lite', '1'], str(lvl))
    cdp.send('Emulation.setCPUThrottlingRate', {'rate': 1})
    shown = page.evaluate("Array.from(document.querySelectorAll('h1, h2')).every(e => getComputedStyle(e).display !== 'none')")
    check('[lite] content stays visible and usable', shown and page.is_visible('.site-header'))
    page.reload(wait_until='load')
    check('[lite] lite is remembered for the session', page.evaluate("document.documentElement.dataset.level") == 'lite')
    check('[lite] no console errors', not page.errors, '; '.join(page.errors)[:200])
    ctx.close()

    # ---- Robustness: no layout shift, script failure, missing IntersectionObserver ----
    ctx, page = new_page(browser, 1440, 900)
    page.add_init_script("window.__cls = 0; new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: 'layout-shift', buffered: true });")
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(1500)
    total = page.evaluate("document.documentElement.scrollHeight")
    for y in range(0, total, 500):
        page.evaluate(f"window.scrollTo(0, {y})")
        page.wait_for_timeout(100)
    page.wait_for_timeout(500)
    cls = page.evaluate("window.__cls")
    check('[load] cumulative layout shift stays under 0.02', cls < 0.02, str(cls))
    ctx.close()
    ctx, page = new_page(browser, 1440, 900)
    page.route('**/js/site.js*', lambda route: route.abort())
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(4600)
    vis = page.evaluate("[document.documentElement.classList.contains('js-ready'), Array.from(document.querySelectorAll('.rv')).filter(e => getComputedStyle(e).opacity === '0').length]")
    check('[failsafe] script missing: nothing is hidden (reveals forced visible)', vis == [False, 0] and page.is_visible('h1') and page.is_visible('.hero .btn-primary'), str(vis))
    ctx.close()
    ctx = browser.new_context(viewport={'width': 1440, 'height': 900})
    ctx.add_init_script("delete window.IntersectionObserver;")
    page = ctx.new_page()
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(500)
    page.evaluate("window.scrollTo(0, 3000)")
    page.wait_for_timeout(800)
    fin = page.evaluate("[Array.from(document.querySelectorAll('.rv')).filter(e => getComputedStyle(e).opacity === '0').length, document.documentElement.classList.contains('rv-all')]")
    check('[fallback] without IntersectionObserver every final state is visible', fin == [0, True], str(fin))
    ctx.close()

    # Header never overlaps the hero, and the first screen fits short viewports
    for w, h in VIEWPORTS:
        for lang, url in PAGES:
            ctx, page = new_page(browser, w, h)
            page.goto(url, wait_until='load')
            page.wait_for_timeout(2800)
            data = page.evaluate("""() => {
              const box = (e) => { const r = e.getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom]; };
              const vis = (e) => { const c = getComputedStyle(e); return c.display !== 'none' && c.visibility !== 'hidden' && e.getBoundingClientRect().width > 0; };
              const hd = box(document.querySelector('.site-header'));
              const hero = box(document.querySelector('.hero'));
              const hit = (a, b) => a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];
              const bad = [];
              const parts = Array.from(document.querySelectorAll('.hero-copy, .win-hero, .fl .sim, .fl .spd, .fl .sha')).filter(vis);
              parts.forEach((e) => { if (hit(box(e), hd)) bad.push('header overlaps ' + e.className); });
              Array.from(document.querySelectorAll('.fl .sim, .fl .spd, .fl .sha')).filter(vis).forEach((e) => {
                const b = box(e);
                if (b[0] < hero[0] - 1 || b[2] > hero[2] + 1 || b[1] < hero[1] - 1 || b[3] > hero[3] + 1) bad.push('card outside hero ' + e.className);
              });
              const q = (s) => { const e = document.querySelector(s); return e ? box(e) : null; };
              return { bad, header: hd, cta: q('.hero .btn-primary'), cta2: q('.hero .btn-ghost'), chips: q('.hero .chips'), vh: innerHeight, vw: innerWidth };
            }""")
            check(f'header does not overlap hero elements {w}x{h} {lang}', not data['bad'], '; '.join(data['bad']))
            if w >= 900:
                ok = all(data[k] and data[k][3] <= data['vh'] for k in ('cta', 'cta2'))
                check(f'both hero CTAs above the fold {w}x{h} {lang}', ok, str((data['cta'], data['cta2'], data['vh'])))
            else:
                check(f'primary hero CTA above the fold {w}x{h} {lang}', data['cta'][3] <= data['vh'], str(data['cta']))
            ctx.close()

    # Header stays on top while scrolling and has a solid background
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(1500)
    page.evaluate("window.scrollTo(0, 260)")
    page.wait_for_timeout(300)
    top = page.evaluate("""() => { const h = document.querySelector('.site-header'); const r = h.getBoundingClientRect(); const e = document.elementFromPoint(r.left + r.width / 2, r.top + r.height - 4); return { onTop: !!e && h.contains(e), alpha: getComputedStyle(h).backgroundColor, z: getComputedStyle(h).zIndex, height: r.height }; }""")
    check('header is the top layer while scrolling', top['onTop'], str(top))
    check('header is compact (<= 60 px)', top['height'] <= 60, str(top['height']))
    ctx.close()

    # Motion control: OS reduce => static by default; ?motion=on forces the full experience; the choice persists
    ctx, page = new_page(browser, 1440, 900, reduced=True)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(500)
    check('[motion] OS reduce: default is static', page.evaluate("document.documentElement.dataset.motion") == 'reduced' and page.get_attribute('.motion-toggle', 'aria-pressed') == 'false')
    page.goto(BASE + '?motion=on', wait_until='load')
    page.wait_for_timeout(600)
    check('[motion] ?motion=on overrides OS reduce', page.evaluate("[document.documentElement.dataset.motion, document.documentElement.classList.contains('anim')]") == ['full', True])
    check('[motion] ?motion=on is removed from the URL and stored', 'motion=' not in page.url and page.evaluate("localStorage.getItem('riptide-motion')") == 'on', page.url)
    check('[motion] toggle shows pressed', page.get_attribute('.motion-toggle', 'aria-pressed') == 'true')
    sticky = page.evaluate("Array.from(document.querySelectorAll('.pin')).filter(e => getComputedStyle(e).position === 'sticky').length")
    check('[motion] chapters are pinned when forced on', sticky >= 2, str(sticky))
    page.reload(wait_until='load')
    page.wait_for_timeout(500)
    check('[motion] choice persists across reload', page.evaluate("document.documentElement.dataset.motion") == 'full')
    # scroll-scrubbed logo: mid-draw (not final) inside the first chapter, different at two scroll positions
    pos = page.evaluate("(() => { const s = document.getElementById('streams'); const r = s.getBoundingClientRect(); return [r.top + scrollY, s.offsetHeight, innerHeight]; })()")
    def prog(frac):
        y = pos[0] + (pos[1] - pos[2]) * frac
        page.evaluate(f"window.scrollTo(0, {y})")
        page.wait_for_timeout(250)
        return page.evaluate("(() => { const p = document.querySelector('#streams .pin'); return [parseFloat(p.style.getPropertyValue('--p')), parseFloat(p.style.getPropertyValue('--ms'))]; })()")
    a, b2, c3 = prog(0.02), prog(0.3), prog(0.9)
    check('[motion] pinned stage progress changes with scroll', a[0] != b2[0] and b2[0] != c3[0], str((a, b2, c3)))
    check('[motion] logo is mid-draw (not final) at the chapter start', b2[1] > 1.01, str(b2))
    page.click('.motion-toggle')
    page.wait_for_load_state('load')
    page.wait_for_timeout(400)
    check('[motion] toggle switches off and persists', page.evaluate("[document.documentElement.dataset.motion, localStorage.getItem('riptide-motion')]") == ['reduced', 'off'])
    ctx.close()
    ctx, page = new_page(browser, 1440, 900)
    page.goto(BASE + '?motion=off', wait_until='load')
    check('[motion] ?motion=off overrides a normal OS setting', page.evaluate("document.documentElement.dataset.motion") == 'reduced')
    ctx.close()

    # ---- Release manifest and the "New in Riptide" pill ----
    ctx, page = new_page(browser, 1440, 900)
    resp = page.request.get(BASE + 'release.json')
    manifest = resp.json() if resp.ok else {}
    feat = next((r for r in manifest.get('releases', []) if r['tag'] == (manifest.get('latest', {}).get('stable') or manifest.get('latest', {}).get('prerelease'))), {})
    check('[release] release.json is served and has a featured release', bool(feat), str(resp.status))
    ctx.close()
    REL_BASE = 'https://github.com/LayanJarrar1/Riptide/releases'
    AHEAD_OK = os.environ.get('RIPTIDE_RELEASE_AHEAD') == '1'
    FEATURES = json.loads((Path(__file__).resolve().parent.parent / 'src' / 'features.json').read_text(encoding='utf8'))['features']

    def numbers(v):
        core, _, pre = v.partition('-')
        rank = {'alpha': 0, 'beta': 1, '': 2}.get(re.sub(r'[^a-z]', '', pre), 0)
        return [int(n) for n in core.split('.')] + [rank] + [int(n) for n in re.findall(r'\d+', pre)]

    def expected_release(page_version):
        # Normal run: the page shows the release in release.json. Release preparation (RIPTIDE_RELEASE_AHEAD=1): the page may
        # show a newer version than release.json, and then the links must follow the release contract for that version.
        if feat and feat['version'] == page_version:
            return feat
        ok = AHEAD_OK and feat and numbers(page_version) > numbers(feat['version'])
        check(f'[release] page version {page_version} is the manifest version or, in release preparation, newer than it', ok, f"manifest {feat.get('version') if feat else None}")
        return {
            'version': page_version,
            'installer': {'url': f'{REL_BASE}/download/v{page_version}/Riptide_{page_version}_x64-setup.exe'},
            'portable': {'url': f'{REL_BASE}/download/v{page_version}/Riptide-{page_version}-portable.zip'},
            'sums': {'url': f'{REL_BASE}/download/v{page_version}/SHA256SUMS.txt'},
            'notesUrl': f'{REL_BASE}/tag/v{page_version}',
        }
    for lang, url in PAGES:
        tag = f'[release {lang}]'
        ctx, page = new_page(browser, 1440, 900)
        page.add_init_script("window.__cls = 0; new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: 'layout-shift', buffered: true });")
        page.goto(url, wait_until='load')
        page.wait_for_timeout(900)
        hrefs = page.evaluate("[...document.querySelectorAll('a[href*=\"releases/\"]')].map(a => a.href)")
        feat = expected_release(page.get_attribute('.rel-pill', 'data-version'))
        check(f'{tag} download buttons come from the manifest', feat and all(h in (feat['installer']['url'], feat['portable']['url'], feat['sums']['url'], feat['notesUrl'], 'https://github.com/LayanJarrar1/Riptide/releases') for h in hrefs) and feat['installer']['url'] in hrefs and feat['portable']['url'] in hrefs, str(hrefs))
        body = page.text_content('body')
        for f in FEATURES:
            title = f[f'title_{lang}']
            shown = feat['version'] == f['release'] and f['verified']
            # compare whole item titles (<b>Title.</b>), so 'Research' is not found inside 'Research by category'
            listed = page.eval_on_selector_all('.whats-new li > b', 'els => els.map(e => e.textContent.trim().replace(/\.$/, ""))')
            in_list = title in listed
            check(f'{tag} features.json "{f["id"]}" is listed exactly when verified for {feat["version"]}', in_list == shown, f'{title!r} listed={in_list} expected={shown}')
        check(f'{tag} channel badge text', ('Beta' in body) if lang == 'en' else ('بيتا' in body))
        pill = page.locator('.rel-pill')
        check(f'{tag} pill is visible on first visit', pill.is_visible())
        text = pill.inner_text()
        check(f'{tag} pill names the version and points at the notes', feat and feat['version'] in text and pill.locator('a').get_attribute('href') == feat['notesUrl'], text)
        check(f'{tag} pill wording', ("is out" in text and "what's new" in text) if lang == 'en' else ('متاح' in text and 'الجديد' in text), text)
        check(f'{tag} pill sits inside the viewport', page.evaluate("(() => { const r = document.querySelector('.rel-pill').getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth; })()"))
        check(f'{tag} showing the pill causes no layout shift', page.evaluate("window.__cls") < 0.001, str(page.evaluate("window.__cls")))
        top = page.evaluate("document.querySelector('h1').getBoundingClientRect().top")
        page.click('.rel-x')
        page.wait_for_timeout(150)
        check(f'{tag} dismiss hides it without moving the page', (not pill.is_visible()) and abs(page.evaluate("document.querySelector('h1').getBoundingClientRect().top") - top) < 0.5)
        check(f'{tag} dismissal is stored per version', page.evaluate("localStorage.getItem('riptide-release-seen')") == feat['version'])
        page.reload(wait_until='load')
        page.wait_for_timeout(400)
        check(f'{tag} stays dismissed after reload', not page.locator('.rel-pill').is_visible())
        page.evaluate("localStorage.setItem('riptide-release-seen', '0.0.1')")
        page.reload(wait_until='load')
        page.wait_for_timeout(400)
        check(f'{tag} a newer version shows it again', page.locator('.rel-pill').is_visible())
        ctx.close()
    # storage blocked: the page still works and the pill shows
    ctx, page = new_page(browser, 1440, 900)
    page.add_init_script("Object.defineProperty(window, 'localStorage', { get() { throw new Error('blocked'); } });")
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(500)
    check('[release] works with storage blocked', page.locator('.rel-pill').is_visible() and not page.errors, '; '.join(page.errors)[:160])
    ctx.close()
    # reduced motion: no animation on the pill
    ctx, page = new_page(browser, 1440, 900, reduced=True)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(500)
    check('[release] reduced motion: pill does not animate', page.locator('.rel-pill').is_visible() and page.evaluate("getComputedStyle(document.querySelector('.rel-pill')).animationName") == 'none')
    ctx.close()
    # the pill and the keyboard
    ctx, page = new_page(browser, 390, 844)
    page.goto(BASE, wait_until='load')
    page.wait_for_timeout(500)
    check('[release] mobile: pill fits and no horizontal overflow', page.evaluate("document.documentElement.scrollWidth <= innerWidth") and page.evaluate("(() => { const r = document.querySelector('.rel-pill').getBoundingClientRect(); return r.right <= innerWidth && r.left >= 0; })()"))
    page.focus('.rel-link')
    page.keyboard.press('Escape')
    check('[release] Escape dismisses it', not page.locator('.rel-pill').is_visible())
    ctx.close()

    # JavaScript disabled: content and CTA still there
    ctx = browser.new_context(viewport={'width': 1440, 'height': 900}, java_script_enabled=False)
    page = ctx.new_page()
    page.goto(BASE, wait_until='load')
    check('[no-js] headline and CTA visible', page.is_visible('h1') and page.is_visible('.hero .btn-primary'))
    check('[no-js] download link points at the release asset', (page.get_attribute('.hero .btn-primary', 'href') or '').startswith('https://github.com/LayanJarrar1/Riptide/releases/download/'))
    check('[no-js] release pill stays hidden', not page.locator('.rel-pill').is_visible())
    check('[no-js] all chapters present', page.locator('main > section').count() >= 9)
    ctx.close()

    browser.close()

failed = [r for r in results if not r[1]]
print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
sys.exit(1 if failed else 0)
