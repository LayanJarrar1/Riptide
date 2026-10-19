#!/usr/bin/env node
// Riptide website build. Node 20, zero dependencies.
//
//   node apps/website/build.mjs            build dist/ and verify it
//   node apps/website/build.mjs --check    verify an existing dist/ only
//   node apps/website/build.mjs --serve    build, then serve dist/ on http://127.0.0.1:8123
//
// Renders src/index.template.html once per language (src/i18n/*.json) into
// dist/index.html (English) and dist/ar/index.html (Arabic), renders the bilingual
// dist/404.html, copies css/, js/ and assets/, then checks the output: no `{{ }}` left
// behind, every local href/src resolves to a file, every <img> has width and height.

import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { gzipSync } from 'node:zlib';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const SRC = join(here, 'src');
const DIST = join(here, 'dist');

const REPO_URL = 'https://github.com/LayanJarrar1/Riptide';
const REPO_SLUG = 'LayanJarrar1/Riptide';
// The current pre-release. Bump these when a new build is published.
const RELEASE_TAG = 'v0.1.0-alpha.3';
const INSTALLER_FILE = 'Riptide_0.1.0-alpha.3_x64-setup.exe';
const PORTABLE_FILE = 'Riptide-0.1.0-alpha.3-portable.zip';
const RELEASES_URL = `${REPO_URL}/releases`;
const INSTALLER_URL = `${RELEASES_URL}/download/${RELEASE_TAG}/${INSTALLER_FILE}`;
const PORTABLE_URL = `${RELEASES_URL}/download/${RELEASE_TAG}/${PORTABLE_FILE}`;
const SUMS_URL = `${RELEASES_URL}/download/${RELEASE_TAG}/SHA256SUMS.txt`;
const EXTENSION_URL = `${REPO_URL}/tree/main/apps/extension`;
// Browser-store listings. Leave a value empty and the page shows "Coming soon" for that store;
// paste the listing URL here when it goes live and the tile becomes a link. One line each.
const STORE_LINKS = { chrome: '', edge: '' };
// The public URL the site is served from. GitHub Pages for this repository by default;
// override with SITE_URL=https://example.org/ for a custom domain. Used for canonical,
// hreflang and Open Graph URLs only; every in-page link is relative.
const SITE_URL = (process.env.SITE_URL || 'https://layanjarrar1.github.io/Riptide/').replace(/\/*$/, '/');
// Root-absolute prefix for the 404 page, which GitHub Pages serves at any depth ("/Riptide/").
const BASE_PATH = new URL(SITE_URL).pathname;

// Size budgets in bytes (uncompressed). The build fails when something grows past them.
const BUDGET = { html: 80 * 1024, css: 64 * 1024, js: 40 * 1024, image: 120 * 1024, font: 110 * 1024, total: 2 * 1024 * 1024 };

const LANGUAGES = [
  { code: 'en', out: '', root: '' },
  { code: 'ar', out: 'ar', root: '../' },
];

// ---------------------------------------------------------------------------------------
// Template engine: {{path}} (escaped), {{{path}}} (raw), {{{svg name}}} (inline icon),
// {{#each path}}...{{/each}} with {{@index}} / {{@num}} / {{@first}} / {{@last}} / {{.}},
// {{#if path}}...{{/if}}. Paths resolve innermost scope first, then outward. A missing
// key is a build error, never an empty string.
// ---------------------------------------------------------------------------------------

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function lookup(path, scopes) {
  if (path === '.') return scopes[0]?.['.'];
  for (const scope of scopes) {
    let value = scope;
    let found = true;
    for (const part of path.split('.')) {
      if (value === null || typeof value !== 'object' || !(part in value)) { found = false; break; }
      value = value[part];
    }
    if (found && value !== undefined) return value;
  }
  return undefined;
}

function parse(template, file) {
  const re = /\{\{\{\s*(.+?)\s*\}\}\}|\{\{\s*(.+?)\s*\}\}/gs;
  const root = { type: 'root', children: [] };
  const stack = [root];
  let last = 0;
  let match;
  while ((match = re.exec(template))) {
    if (match.index > last) stack.at(-1).children.push({ type: 'text', value: template.slice(last, match.index) });
    last = re.lastIndex;
    if (match[1] !== undefined) { stack.at(-1).children.push({ type: 'raw', expr: match[1] }); continue; }
    const expr = match[2];
    if (expr.startsWith('#each ') || expr.startsWith('#if ')) {
      const [kind, path] = expr.slice(1).split(/\s+/);
      const node = { type: kind, path, children: [] };
      stack.at(-1).children.push(node);
      stack.push(node);
    } else if (expr === '/each' || expr === '/if') {
      const node = stack.pop();
      if (!node || node.type !== expr.slice(1)) throw new Error(`${file}: unbalanced {{${expr}}}`);
    } else {
      stack.at(-1).children.push({ type: 'var', expr });
    }
  }
  if (last < template.length) stack.at(-1).children.push({ type: 'text', value: template.slice(last) });
  if (stack.length !== 1) throw new Error(`${file}: unclosed {{#${stack.at(-1).type}}}`);
  return root;
}

function evaluate(expr, scopes, icons, file) {
  const parts = expr.split(/\s+/);
  if (parts[0] === 'svg') {
    // `{{{svg icon}}}` inside a loop resolves the item's icon name; `{{{svg download}}}` is a literal.
    const resolved = lookup(parts[1], scopes);
    const name = typeof resolved === 'string' && resolved in icons ? resolved : parts[1];
    if (!(name in icons)) throw new Error(`${file}: unknown icon "${name}"`);
    return icons[name];
  }
  const value = lookup(expr, scopes);
  if (value === undefined) throw new Error(`${file}: missing key "${expr}"`);
  return value;
}

function renderNodes(nodes, scopes, icons, file) {
  let out = '';
  for (const node of nodes) {
    switch (node.type) {
      case 'text': out += node.value; break;
      case 'raw': out += String(evaluate(node.expr, scopes, icons, file)); break;
      case 'var': out += escapeHtml(evaluate(node.expr, scopes, icons, file)); break;
      case 'if': {
        const value = lookup(node.path, scopes);
        if (value) out += renderNodes(node.children, scopes, icons, file);
        break;
      }
      case 'each': {
        const list = lookup(node.path, scopes);
        if (!Array.isArray(list)) throw new Error(`${file}: "${node.path}" is not a list`);
        list.forEach((item, index) => {
          const local = typeof item === 'object' && item !== null ? { ...item } : {};
          Object.assign(local, { '.': item, '@index': index, '@num': index + 1, '@first': index === 0, '@last': index === list.length - 1 });
          out += renderNodes(node.children, [local, ...scopes], icons, file);
        });
        break;
      }
      default: break;
    }
  }
  return out;
}

function render(templatePath, scopes, icons) {
  const file = templatePath.slice(SRC.length + 1);
  return renderNodes(parse(readFileSync(templatePath, 'utf8'), file).children, scopes, icons, file);
}

// ---------------------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------------------

function loadIcons() {
  const dir = join(SRC, 'icons');
  const icons = {};
  for (const name of readdirSync(dir)) {
    if (extname(name) !== '.svg') continue;
    icons[name.slice(0, -4)] = readFileSync(join(dir, name), 'utf8').trim();
  }
  return icons;
}

function shortHash(...paths) {
  const h = createHash('sha256');
  for (const p of paths) h.update(readFileSync(p));
  return h.digest('hex').slice(0, 10);
}

function build() {
  rmSync(DIST, { recursive: true, force: true });
  mkdirSync(DIST, { recursive: true });

  cpSync(join(SRC, 'assets'), join(DIST, 'assets'), { recursive: true });
  cpSync(join(SRC, 'css'), join(DIST, 'css'), { recursive: true });
  cpSync(join(SRC, 'js'), join(DIST, 'js'), { recursive: true });
  writeFileSync(join(DIST, '.nojekyll'), '');

  const icons = loadIcons();
  const version = shortHash(join(SRC, 'css', 'site.css'), join(SRC, 'js', 'site.js'));
  const tables = Object.fromEntries(LANGUAGES.map((l) => [l.code, JSON.parse(readFileSync(join(SRC, 'i18n', `${l.code}.json`), 'utf8'))]));

  for (const lang of LANGUAGES) {
    const table = tables[lang.code];
    const other = LANGUAGES.find((l) => l.code !== lang.code);
    const globals = {
      root: lang.root,
      v: version,
      repo: REPO_URL,
      repoSlug: REPO_SLUG,
      releasesUrl: RELEASES_URL,
      installerUrl: INSTALLER_URL,
      blocks24: Array.from({ length: 24 }, (_, i) => i),
      blocks64: Array.from({ length: 64 }, (_, i) => i),
      installerFile: INSTALLER_FILE,
      portableUrl: PORTABLE_URL,
      portableFile: PORTABLE_FILE,
      sumsUrl: SUMS_URL,
      extensionUrl: EXTENSION_URL,
      chromeStoreUrl: STORE_LINKS.chrome,
      chromeStoreSoon: !STORE_LINKS.chrome,
      edgeStoreUrl: STORE_LINKS.edge,
      edgeStoreSoon: !STORE_LINKS.edge,
      siteUrl: SITE_URL,
      pageUrl: SITE_URL + (lang.out ? `${lang.out}/` : ''),
      altHref: lang.root + (other.out ? `${other.out}/` : ''),
      altLang: other.code,
      isEn: lang.code === 'en',
      isAr: lang.code === 'ar',
    };
    const html = render(join(SRC, 'index.template.html'), [globals, table], icons);
    const outDir = join(DIST, lang.out);
    mkdirSync(outDir, { recursive: true });
    writeFileSync(join(outDir, 'index.html'), html);

    // Privacy policy: /privacy/ and /ar/privacy/, one level deeper than the home page.
    const privacyOut = join(lang.out, 'privacy');
    const privacyHtml = render(join(SRC, 'privacy.template.html'), [{
      ...globals,
      root: `../${lang.root}`,
      home: '../',
      pageUrl: `${SITE_URL}${privacyOut.split(/[\\/]/).join('/')}/`,
      altHref: `../${lang.root}${other.out ? `${other.out}/` : ''}privacy/`,
    }, table], icons);
    mkdirSync(join(DIST, privacyOut), { recursive: true });
    writeFileSync(join(DIST, privacyOut, 'index.html'), privacyHtml);
  }

  const notFound = render(join(SRC, '404.template.html'), [{ base: BASE_PATH, v: version, repo: REPO_URL, en: tables.en, ar: tables.ar }], icons);
  writeFileSync(join(DIST, '404.html'), notFound);

  // robots.txt, sitemap.xml (with hreflang alternates) and a _headers file that Cloudflare Pages and
  // Netlify honour (GitHub Pages ignores it and serves its own fixed cache headers).
  const pages = [['', ''], ['privacy/', 'privacy/']];
  const url = (lang, path) => `${SITE_URL}${lang ? 'ar/' : ''}${path}`;
  const sitemap = pages.map(([, path]) => `  <url>
    <loc>${url(false, path)}</loc>
    <xhtml:link rel="alternate" hreflang="en" href="${url(false, path)}"/>
    <xhtml:link rel="alternate" hreflang="ar" href="${url(true, path)}"/>
  </url>
  <url>
    <loc>${url(true, path)}</loc>
    <xhtml:link rel="alternate" hreflang="en" href="${url(false, path)}"/>
    <xhtml:link rel="alternate" hreflang="ar" href="${url(true, path)}"/>
  </url>`).join('\n');
  writeFileSync(join(DIST, 'sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${sitemap}
</urlset>
`);
  writeFileSync(join(DIST, 'robots.txt'), `User-agent: *
Allow: /

Sitemap: ${SITE_URL}sitemap.xml
`);
  writeFileSync(join(DIST, '_headers'), `/assets/*
  Cache-Control: public, max-age=31536000, immutable
/css/*
  Cache-Control: public, max-age=31536000, immutable
/js/*
  Cache-Control: public, max-age=31536000, immutable
/*
  X-Content-Type-Options: nosniff
  Referrer-Policy: strict-origin-when-cross-origin
`);
}

// ---------------------------------------------------------------------------------------
// Verify: the checks a reviewer would run by hand.
// ---------------------------------------------------------------------------------------

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

function verify() {
  const problems = [];
  const pages = walk(DIST).filter((f) => f.endsWith('.html'));
  if (pages.length === 0) problems.push('no HTML pages in dist/');

  for (const page of pages) {
    const html = readFileSync(page, 'utf8');
    const rel = page.slice(DIST.length + 1);
    const leftovers = html.match(/\{\{[^]*?\}\}/g);
    if (leftovers) problems.push(`${rel}: unresolved template tokens: ${leftovers.slice(0, 3).join(', ')}`);
    if (!/^<!doctype html>/i.test(html)) problems.push(`${rel}: missing doctype`);
    if (!/<html[^>]*\slang="/.test(html)) problems.push(`${rel}: <html> has no lang`);

    for (const m of html.matchAll(/<img\b[^>]*>/g)) {
      const tag = m[0];
      if (!/\swidth="\d+"/.test(tag) || !/\sheight="\d+"/.test(tag)) problems.push(`${rel}: <img> without width/height: ${tag.slice(0, 80)}`);
      if (!/\salt="/.test(tag)) problems.push(`${rel}: <img> without alt: ${tag.slice(0, 80)}`);
    }

    // Nothing may load from a third party: only plain <a> links may leave the origin.
    for (const m of html.matchAll(/<(img|script|link|iframe|source|video|audio|embed)\b[^>]*>/g)) {
      if (/\s(?:src|href|srcset|poster)="https?:\/\//.test(m[0]) && !/rel="(?:canonical|alternate|sitemap)"/.test(m[0])) problems.push(`${rel}: third-party resource: ${m[0].slice(0, 90)}`);
    }
    for (const m of html.matchAll(/\ssrcset="([^"]+)"/g)) {
      for (const part of m[1].split(',')) {
        const ref = part.trim().split(/\s+/)[0];
        if (ref && !existsSync(resolve(dirname(page), ref))) problems.push(`${rel}: broken srcset reference "${ref}"`);
      }
    }
    const htmlBytes = Buffer.byteLength(html);
    if (htmlBytes > BUDGET.html) problems.push(`${rel}: ${htmlBytes} bytes of HTML exceeds the ${BUDGET.html} byte budget`);

    for (const m of html.matchAll(/\s(?:src|href)="([^"]+)"/g)) {
      const ref = m[1];
      if (/^(?:https?:|mailto:|#|data:)/.test(ref)) continue;
      const clean = ref.split(/[?#]/)[0];
      if (!clean) continue;
      let target;
      if (clean.startsWith('/')) {
        if (!clean.startsWith(BASE_PATH)) { problems.push(`${rel}: root-absolute reference outside the site base "${ref}"`); continue; }
        target = join(DIST, clean.slice(BASE_PATH.length));
      } else {
        target = resolve(dirname(page), clean);
      }
      let ok = existsSync(target);
      if (ok && statSync(target).isDirectory()) ok = existsSync(join(target, 'index.html'));
      if (!ok) problems.push(`${rel}: broken local reference "${ref}"`);
    }

    // Tag balance for the elements that matter for landmarks and lists.
    for (const tag of ['main', 'header', 'footer', 'nav', 'section', 'ul', 'ol', 'li', 'table', 'details', 'dialog', 'figure', 'button', 'a', 'p', 'h1', 'h2', 'h3']) {
      const open = (html.match(new RegExp(`<${tag}(?:\\s[^>]*)?>`, 'g')) || []).length;
      const close = (html.match(new RegExp(`</${tag}>`, 'g')) || []).length;
      if (open !== close) problems.push(`${rel}: <${tag}> opened ${open} times, closed ${close}`);
    }
  }

  // Fonts referenced from CSS must exist.
  const css = readFileSync(join(DIST, 'css', 'site.css'), 'utf8');
  for (const m of css.matchAll(/url\("?([^")]+)"?\)/g)) {
    const target = normalize(join(DIST, 'css', m[1].split(/[?#]/)[0]));
    if (!existsSync(target)) problems.push(`site.css: missing ${m[1]}`);
  }

  // Size budgets: what a first visit may cost, uncompressed.
  for (const [file, limit] of [['css/site.css', BUDGET.css], ['js/site.js', BUDGET.js]]) {
    const size = statSync(join(DIST, file)).size;
    if (size > limit) problems.push(`${file}: ${size} bytes exceeds the ${limit} byte budget`);
  }
  for (const f of walk(join(DIST, 'assets'))) {
    const size = statSync(f).size;
    const ext = extname(f);
    if (['.png', '.webp', '.jpg', '.svg'].includes(ext) && size > BUDGET.image) problems.push(`${f.slice(DIST.length + 1)}: ${size} bytes exceeds the ${BUDGET.image} byte image budget`);
    if (ext === '.woff2' && size > BUDGET.font) problems.push(`${f.slice(DIST.length + 1)}: ${size} bytes exceeds the ${BUDGET.font} byte font budget`);
  }
  const total = walk(DIST).reduce((sum, f) => sum + statSync(f).size, 0);
  if (total > BUDGET.total) problems.push(`dist is ${total} bytes, over the ${BUDGET.total} byte budget`);

  if (problems.length) {
    console.error(`verify: ${problems.length} problem(s)`);
    for (const p of problems) console.error(`  - ${p}`);
    process.exit(1);
  }
  const bytes = walk(DIST).reduce((sum, f) => sum + statSync(f).size, 0);
  console.log(`verify: ok (${pages.length} pages, ${(bytes / 1024).toFixed(0)} KiB total)`);
}

// ---------------------------------------------------------------------------------------
// Tiny static server for previews (no dependencies, local only).
// ---------------------------------------------------------------------------------------

const MIME = {
  '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg',
  '.woff2': 'font/woff2', '.webp': 'image/webp', '.xml': 'application/xml', '.txt': 'text/plain; charset=utf-8', '.ico': 'image/x-icon', '.webmanifest': 'application/manifest+json',
};

function serve(port) {
  const server = createServer((req, res) => {
    let path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    if (path.endsWith('/')) path += 'index.html';
    let file = normalize(join(DIST, path));
    if (!file.startsWith(DIST + sep) && file !== DIST) { res.writeHead(403); res.end(); return; }
    if (existsSync(file) && statSync(file).isDirectory()) { res.writeHead(301, { Location: `${path}/` }); res.end(); return; }
    if (!existsSync(file)) { file = join(DIST, '404.html'); res.statusCode = 404; }
    res.setHeader('Content-Type', MIME[extname(file)] || 'application/octet-stream');
    res.setHeader('Cache-Control', 'no-store');
    // Compress text like the real hosts do, so a preview (and Lighthouse) sees realistic transfer sizes.
    const body = readFileSync(file);
    if (/(?:text|javascript|json|xml|svg)/.test(res.getHeader('Content-Type')) && /\bgzip\b/.test(req.headers['accept-encoding'] || '')) {
      res.setHeader('Content-Encoding', 'gzip');
      res.setHeader('Vary', 'Accept-Encoding');
      res.end(gzipSync(body));
    } else {
      res.end(body);
    }
  });
  server.listen(port, '127.0.0.1', () => console.log(`serving ${DIST} at http://127.0.0.1:${port}/  (Ctrl+C to stop)`));
}

const args = process.argv.slice(2);
if (!args.includes('--check')) build();
verify();
if (args.includes('--serve')) serve(Number(args[args.indexOf('--serve') + 1]) || 8123);
