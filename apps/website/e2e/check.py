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
VIEWPORTS = [(1920, 1080), (1440, 900), (768, 1024), (390, 844)]
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
        anims = page.evaluate("document.getAnimations().length")
        check(f'{tag} no running animations', anims == 0, str(anims))
        check(f'{tag} hero shows the completed simulation and verified chip', page.is_visible('.sha') and page.evaluate("getComputedStyle(document.querySelector('.sha')).opacity") == '1')
        check(f'{tag} CTA visible and clickable', page.is_visible('.hero .btn-primary'))
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
