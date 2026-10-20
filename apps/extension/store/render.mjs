// Renders the store listing images from the real built extension pages.
//
//   npm run build                                   (dist/chrome must exist)
//   PLAYWRIGHT_CORE=<path to playwright-core/index.mjs> node store/render.mjs
//   (or run it where `playwright-core` resolves; Chromium must be installed for that version)
//
// The popup, options page and content script are the built bundles from dist/chrome, with
// store/src/stub.js standing in for the extension APIs (status "connected", a few recent
// hand-offs). Output:
//   store/screenshots/*.png   1280x800
//   store/promo/promo-440x280.png, store/promo/marquee-1400x560.png
//   store/icon-128.png        (copy of the built toolbar icon)
/* global window, document -- used inside page.evaluate callbacks, which run in the browser */
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const store = dirname(fileURLToPath(import.meta.url));
const ext = dirname(store);
const built = join(ext, 'dist', 'chrome');
const fonts = join(ext, '..', 'website', 'src', 'assets', 'fonts');
const tmp = join(ext, 'dist', 'store-tmp');
// Brand-kit logos, unchanged: light variant on light backgrounds, dark variant on dark ones.
const LOGO_LIGHT = pathToFileURL(join(store, 'src', 'brand', 'riptide-logo-light.svg')).href;
const LOGO_DARK = pathToFileURL(join(store, 'src', 'brand', 'riptide-logo-dark.svg')).href;
const shots = join(store, 'screenshots');
const promo = join(store, 'promo');
for (const d of [tmp, shots, promo]) mkdirSync(d, { recursive: true });

const pw = await import(process.env.PLAYWRIGHT_CORE ? pathToFileURL(process.env.PLAYWRIGHT_CORE).href : 'playwright-core');
const { chromium } = pw.default ?? pw;
// PW_CHANNEL=msedge (or chrome) drives an installed browser when Playwright's own Chromium is absent.
const browser = await chromium.launch(process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {});
const stub = readFileSync(join(store, 'src', 'stub.js'), 'utf8');
const url = (p) => pathToFileURL(p).href;

async function page(viewport, lang, scale = 1) {
  const ctx = await browser.newContext({ viewport, deviceScaleFactor: scale, colorScheme: 'light', locale: lang });
  await ctx.addInitScript(`window.__RT = ${JSON.stringify({ lang })};\n${stub}`);
  return ctx.newPage();
}

// ---- raw captures of the real pages --------------------------------------------------------

async function popup(lang) {
  const p = await page({ width: 340, height: 600 }, lang, 2);
  await p.goto(url(join(built, 'popup', 'popup.html')));
  await p.waitForSelector('#recent-list li');
  const out = join(tmp, `popup-${lang}.png`);
  await p.locator('body').screenshot({ path: out });
  await p.context().close();
  return out;
}

async function options(lang, file) {
  const p = await page({ width: 1280, height: 800 }, lang);
  await p.goto(url(join(built, 'options', 'options.html')));
  await p.waitForTimeout(300);
  await p.screenshot({ path: join(shots, file) });
  await p.context().close();
}

async function demo(lang, file, action) {
  const p = await page({ width: 1280, height: 800 }, lang);
  await p.goto(url(join(store, 'src', 'demo.html')));
  await p.addScriptTag({ path: join(built, 'content', 'content.js') });
  await p.waitForTimeout(200);
  await action(p);
  await p.waitForTimeout(400);
  await p.screenshot({ path: join(shots, file) });
  await p.context().close();
}

// ---- composed images -----------------------------------------------------------------------

const FONT_CSS = `
@font-face { font-family: Inter; src: url('${url(join(fonts, 'InterVariable.woff2'))}') format('woff2'); font-weight: 100 900; }
@font-face { font-family: Plex; src: url('${url(join(fonts, 'IBMPlexSansArabic-Regular.woff2'))}') format('woff2'); font-weight: 400; }
@font-face { font-family: Plex; src: url('${url(join(fonts, 'IBMPlexSansArabic-SemiBold.woff2'))}') format('woff2'); font-weight: 600; }
@font-face { font-family: Plex; src: url('${url(join(fonts, 'IBMPlexSansArabic-Bold.woff2'))}') format('woff2'); font-weight: 700; }
* { box-sizing: border-box; margin: 0; }
body { font-family: Inter, Plex, sans-serif; color: #0f1d24; background: #fff; overflow: hidden; }
:root[lang="ar"] body { font-family: Plex, Inter, sans-serif; }
`;

async function compose(html, size, out) {
  const file = join(tmp, `${out.replace(/[\\/:]/g, '_')}.html`);
  writeFileSync(file, html);
  const p = await page(size, 'en');
  await p.goto(url(file));
  await p.evaluate(() => document.fonts.ready);
  await p.screenshot({ path: out });
  await p.context().close();
}

function popupFrame(lang, img, title, sub) {
  const dir = lang === 'ar' ? 'rtl' : 'ltr';
  return `<!doctype html><html lang="${lang}" dir="${dir}"><head><meta charset="utf-8"><style>${FONT_CSS}
body { width: 1280px; height: 800px; display: grid; grid-template-columns: 1fr 456px; align-items: center; gap: 64px; padding: 0 96px; }
.logo { display: block; height: 64px; width: auto; margin-bottom: 40px; }
:root[dir="rtl"] .logo { margin-inline-end: auto; }
h1 { font-size: 52px; line-height: 1.12; letter-spacing: -0.025em; font-weight: 700; }
:root[lang="ar"] h1 { letter-spacing: 0; line-height: 1.35; }
p { margin-top: 22px; font-size: 21px; line-height: 1.6; color: #4b5b66; max-width: 34ch; }
.bar { width: 64px; height: 4px; background: #00D5E8; border-radius: 2px; margin-top: 36px; }
.shot { justify-self: center; background: #f3f6f7; border-radius: 22px; padding: 28px; }
.shot img { display: block; width: 400px; border-radius: 12px; box-shadow: 0 1px 2px rgb(15 29 36 / 10%), 0 18px 48px rgb(15 29 36 / 16%); }
</style></head><body>
<div><img class="logo" src="${LOGO_LIGHT}" alt="Riptide"><h1>${title}</h1><p>${sub}</p><div class="bar"></div></div>
<div class="shot"><img src="${url(img)}" alt=""></div></body></html>`;
}

function tile(w, h, big) {
  // Abyss background, brand dark logo, short tagline from the store listing, Cyan accent rule.
  const s = big ? { logo: 760, tag: 36, gap: 40, rule: 6 } : { logo: 280, tag: 18, gap: 20, rule: 4 };
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><style>${FONT_CSS}
body { width: ${w}px; height: ${h}px; background: #071A26; color: #D9F8FA; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: ${s.gap}px; text-align: center; }
img { display: block; width: ${s.logo}px; height: auto; }
.rule { width: ${s.logo / 8}px; height: ${s.rule}px; border-radius: 3px; background: #00D5E8; }
.tag { font-size: ${s.tag}px; line-height: 1.4; font-weight: 500; color: #D9F8FA; }
</style></head><body><img src="${LOGO_DARK}" alt="Riptide"><div class="rule"></div>
<div class="tag">${big ? 'Hands your downloads to Riptide, the free download manager,<br>automatically or with one click.' : 'Free, open-source<br>download manager'}</div></body></html>`;
}

// ---- run -----------------------------------------------------------------------------------

const popupEn = await popup('en');
const popupAr = await popup('ar');
await compose(
  popupFrame('en', popupEn, 'Your downloads,<br>handed to Riptide.', 'Big files go to the Riptide desktop app automatically: many connections, pause and resume, checksums.'),
  { width: 1280, height: 800 },
  join(shots, '1-popup-en.png'),
);
await demo('en', '2-hover-badge-en.png', (p) => p.hover('#target'));
await demo('en', '3-link-panel-en.png', (p) =>
  p.evaluate(() => {
    for (const fn of window.__rtMessageListeners) fn({ kind: 'show-panel', selectionOnly: false });
  }),
);
await options('en', '4-options-en.png');
await compose(
  popupFrame('ar', popupAr, 'تنزيلاتك تنتقل<br>إلى Riptide.', 'الملفات الكبيرة تذهب إلى تطبيق Riptide تلقائياً: اتصالات متعددة، إيقاف واستئناف، وتحقق من سلامة الملف.'),
  { width: 1280, height: 800 },
  join(shots, '5-popup-ar.png'),
);
await compose(tile(440, 280, false), { width: 440, height: 280 }, join(promo, 'promo-440x280.png'));
await compose(tile(1400, 560, true), { width: 1400, height: 560 }, join(promo, 'marquee-1400x560.png'));
copyFileSync(join(built, 'icons', 'icon-128.png'), join(store, 'icon-128.png'));

await browser.close();
rmSync(tmp, { recursive: true, force: true });
console.log('store images written to store/screenshots, store/promo, store/icon-128.png');
