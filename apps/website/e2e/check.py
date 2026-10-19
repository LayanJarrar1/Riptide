#!/usr/bin/env python3
"""Smoke and layout checks for the Riptide website. Builds nothing; run it against a served site.

    python apps/website/e2e/check.py http://127.0.0.1:8124/

Needs Python Playwright (`pip install playwright`). Uses the installed Microsoft Edge by default
(RIPTIDE_E2E_CHANNEL=chrome for Chrome, or empty for Playwright's own Chromium).
Exit code 0 when every check passes, 1 otherwise.
"""
import os
import sys
from urllib.parse import urljoin, urlparse

from playwright.sync_api import sync_playwright

BASE = (sys.argv[1] if len(sys.argv) > 1 else 'http://127.0.0.1:8124/').rstrip('/') + '/'
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
        check(f'{tag} extension block says coming soon', page.locator('.store').count() >= 2)
        check(f'{tag} FAQ has at most 5 entries', 1 <= page.locator('details.faq-item').count() <= 5)

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

    # Reduced motion: no pinning, everything visible, simulation shows its final state
    for lang, url in PAGES:
        ctx, page = new_page(browser, 1440, 900, reduced=True)
        page.goto(url, wait_until='load')
        page.wait_for_timeout(600)
        tag = f'[{lang} reduced-motion]'
        check(f'{tag} no anim class', not page.evaluate("document.documentElement.classList.contains('anim')"))
        sticky = page.evaluate("Array.from(document.querySelectorAll('.pin')).filter(e => getComputedStyle(e).position === 'sticky').length")
        check(f'{tag} nothing pinned', sticky == 0, str(sticky))
        hidden = page.evaluate("Array.from(document.querySelectorAll('.rv, .hero-copy .rise, .hero-copy .ln > span')).filter(e => { const c = getComputedStyle(e); return c.opacity === '0' || c.visibility === 'hidden'; }).length")
        check(f'{tag} all content visible without animation', hidden == 0, str(hidden))
        anims = page.evaluate("document.getAnimations().filter(a => { const t = a.effect.getTiming(); return !(t.duration <= 200 && t.iterations === 1); }).length")
        check(f'{tag} no running animations (only <=200 ms fades allowed)', anims == 0, str(anims))
        check(f'{tag} data-motion is reduced by default', page.evaluate("document.documentElement.dataset.motion") == 'reduced')
        check(f'{tag} hero shows the completed simulation and verified chip', page.is_visible('.sha') and page.evaluate("getComputedStyle(document.querySelector('.sha')).opacity") == '1')
        check(f'{tag} CTA visible and clickable', page.is_visible('.hero .btn-primary'))
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

    # JavaScript disabled: content and CTA still there
    ctx = browser.new_context(viewport={'width': 1440, 'height': 900}, java_script_enabled=False)
    page = ctx.new_page()
    page.goto(BASE, wait_until='load')
    check('[no-js] headline and CTA visible', page.is_visible('h1') and page.is_visible('.hero .btn-primary'))
    check('[no-js] all chapters present', page.locator('main > section').count() >= 9)
    ctx.close()

    browser.close()

failed = [r for r in results if not r[1]]
print(f'\n{len(results) - len(failed)}/{len(results)} checks passed')
sys.exit(1 if failed else 0)
