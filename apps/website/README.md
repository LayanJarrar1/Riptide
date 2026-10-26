# Riptide website

The public product site: one static, bilingual page (English at `/`, Arabic at `/ar/`), a privacy
policy and a bilingual `404.html`. Plain HTML, CSS and vanilla JavaScript. No framework, no npm packages, no
CDN, no analytics, fonts bundled locally.

## Build and preview

Node 20 or newer is the only requirement.

```powershell
node apps/website/build.mjs            # renders and copies everything into apps/website/dist, then verifies it
node apps/website/build.mjs --serve    # same, then serves dist on http://127.0.0.1:8123
node apps/website/build.mjs --check    # verifies an existing dist without rebuilding
```

Any static server works for previews, for example `python -m http.server 8123` from inside
`dist`. `dist/` is build output and is ignored by git.

The build also writes `robots.txt`, `sitemap.xml` and a `_headers` file (long cache for hashed
assets, read by Cloudflare Pages and Netlify; GitHub Pages ignores it). See `SCALING.md` for
serving the site under load.

The build fails (exit code 1) when a page still contains a `{{ }}` token, a local `href`/`src`
does not resolve to a file, an `<img>` is missing `alt`, `width` or `height`, a font referenced
from the stylesheet is missing, a structural tag is left unclosed, a page loads an image, script,
stylesheet or frame from another origin, or a size budget is exceeded (HTML 100 KiB, CSS 96 KiB, JS
40 KiB, images 260 KiB, fonts 110 KiB, whole `dist/` 4 MiB).

`SITE_URL` sets the public origin used for canonical, `hreflang` and Open Graph URLs (default:
the GitHub Pages URL of the repository). Every in-page link is relative, so the site also works
from a subfolder or `file://`.

## Structure

```
apps/website/
  build.mjs                  the whole build: template engine, release manifest, copy, verify, preview server
  release.json               generated release manifest (scripts/gen-release-manifest.mjs); the single source for links and badges
  src/
    index.template.html      the page, rendered once per language
    404.template.html        bilingual not-found page
    privacy.template.html    privacy policy, rendered to privacy/ and ar/privacy/ (strings under `privacy`)
    css/site.css             tokens (light first, dark override), mobile-first layout, entrances and the hero simulation
    js/site.js               theme, mobile nav, reveals, simulation pause, current-section highlight (no dependencies, ~4 KB)
    i18n/en.json             every English string
    i18n/ar.json             every Arabic string (same keys, same shape)
    features.json            EN/AR copy for features not released yet, switched on per entry (verified + release)
    icons/*.svg              inline icons, inserted with {{{svg name}}}
    assets/fonts/            Inter (variable, Latin subset) and IBM Plex Sans Arabic (Arabic + Latin subset), OFL
    assets/screenshots/      app-<lang>-<theme>-<width>.webp: the app list captured from the desktop build at a 1040x660 viewport at device scales 0.54 to 2 (560 to 2080 px wide, so the browser rarely resamples), plus crop-drawer / crop-add panels cut from the details and add-download captures
    assets/riptide-logo-light.svg, riptide-logo-dark.svg   brand kit logos, cropped to their artwork
    assets/favicon.svg       the brand kit app icon
    assets/og-en.png, og-ar.png, apple-touch-icon.png   rendered from og/og.html
  og/og.html                 source of the Open Graph cards and touch icon (render at 1200x630 with
                             any headless browser; `?lang=ar`, `?icon=1`)
  SCALING.md                 limits, budgets and what to do if traffic outgrows GitHub Pages
```

## Page structure and motion (v5, compact)

One short page, nine sections, nothing pinned: hero (promise, installer and portable zip, four trust chips, the real app
window and a labelled download simulation) → proof strip (four facts the product backs up) → `#how` (speed and integrity
as two cards) → `#features` (three screenshot cards, then eight small cards: video, file manager, BitTorrent, queues,
batch, site grabber, no admin, no junk) → `#research` (with the optional-AI note) → `#browser` (extension) → `#privacy`
(privacy points and open source in one band) → `#faq` (folded) → `#download` (two download cards; what's new, extension
install steps and the SmartScreen note are folded `<details>`). The hero's "See what's new" link opens the release fold.

Layout is mobile first: one column on phones, two from 960 px, small cards two-up on phones; type and spacing use
`clamp()`. The e2e check asserts no horizontal overflow and nothing cut at the edges from 320 to 1920 px, in both languages
and both themes, plus readable text (12 px minimum) and 40 px touch targets on phones.

Motion:

- Hero entrance: CSS only (`.in-1` to `.in-5`, 650 ms, 60 ms apart).
- Reveals: `.rv` elements get `.in` once from an IntersectionObserver; they fade and rise 18 px with a 70 ms stagger.
- `#how`: the eight lanes fill once (all on the compositor), the resume and SHA-256 rows appear one after another.
- Hero simulation: pure CSS on a 7 s loop. Eight lanes fill with different curves and finish together (as work stealing
  does), an `@property` integer counts to 100 %, then "SHA-256 verified" shows. `js/site.js` pauses it off-screen and in a
  hidden tab.
- Only `transform` and `opacity` animate (plus that counter); the check asserts it and that scrolling causes no long tasks.
- `prefers-reduced-motion: reduce` shows every end state at once and runs nothing. There is no separate Animations switch.

Hidden start states only exist under `html.js-ready`, which `site.js` adds after a successful init; any error in it, or
the script not arriving within 3 s, forces everything visible (`html.rv-all`). Without JavaScript the page is complete.

## Releasing

The page links to one release, read at build time from `apps/website/release.json`. That file is generated by
`scripts/gen-release-manifest.mjs` from the GitHub Releases API and validated again by `build.mjs`; the download
buttons, the Alpha/Beta/Stable badge (EN and AR), the version, the notes links and the "What's new" list all come from
it. The site makes no API call at runtime. The full procedure, with the seven-step checklist, is in
[`guide/release-process.md`](../../guide/release-process.md).

```powershell
node scripts/gen-release-manifest.mjs            # after the GitHub release exists
node scripts/set-release-version.mjs <version>   # README links and the RELEASE_VERSION guard in build.mjs
node scripts/check-doc-sync.mjs
node apps/website/build.mjs
```

`RELEASE_VERSION` in `build.mjs` is only the fallback when `release.json` is missing. When both exist and disagree (and the switch below is not set) the build
warns, and fails when `CI` is set. The featured release is the newest stable one, else the newest pre-release.

While a release is being prepared in a pull request, `RELEASE_VERSION` is ahead of `release.json` (which only ever lists
published releases and is never edited by hand). `RIPTIDE_RELEASE_AHEAD=1` (or `--release-ahead`) lets the build run for
that case: it uses the new version for the buttons, names and the `features.json` entries, with no size or checksum. The
`site` workflow sets it for pull requests only; see "Preparing a release in a pull request" in
[`guide/release-process.md`](../../guide/release-process.md).

The hero eyebrow names the featured version's channel and links to the "What's new" fold in the download section.

Features that are not released yet live in `src/features.json` with `verified: false`. Only entries that are verified and
released in the featured version are rendered (in the "What's new" list). To switch one on, set `verified: true` and
`release: "<version>"` in its entry; see the release process doc.

Deploy: `.github/workflows/site.yml` builds, tests and link-checks the site on pushes to `main`, pull requests, published
releases and daily. It publishes to GitHub Pages only when the repository variable `SITE_DEPLOY` is `true` and Pages has
"GitHub Actions" as its source, so it does nothing until the owner turns that on. `_headers` (Cloudflare Pages and Netlify)
gives hashed assets a year, HTML and `release.json` five minutes; GitHub Pages ignores it.

## Checks

`apps/website/e2e/check.py <base-url>` (Python Playwright, Edge by default) runs against a served build: document basics,
console errors, third-party requests, nav and in-page links, every release link against the featured version, the nine
sections and their cards, alt text and sizes, lazy screenshots, the "What's new" list against `features.json`, overflow and
edge clipping at ten widths (320 to 1920) in both languages and themes, phone compactness, text size and touch targets,
header clear of the hero and the primary CTA above the fold at every width, the motion rules (simulation runs and pauses
off-screen, only transform/opacity animate, no long tasks while scrolling, reduced motion runs nothing), layout shift,
missing script, missing IntersectionObserver, JavaScript off, header, menu, theme persistence, blocked storage, and the
privacy and 404 pages.

## Template syntax

`{{path.to.key}}` inserts an HTML-escaped string from the language table; `{{{path}}}` inserts it
raw (only for trusted markup); `{{{svg name}}}` inlines `src/icons/name.svg`; `{{#each list}} …
{{/each}}` repeats a block with `{{@index}}`, `{{@num}}`, `{{@first}}`, `{{@last}}` and `{{.}}`
available inside; `{{#if key}} … {{/if}}` renders when the value is truthy. Globals available on
every page: `root` (`""` or `../`), `v` (cache-busting hash), `repo`, `repoSlug`, `siteUrl`,
`pageUrl`, `altHref`, `altLang`, `isEn`, `isAr`.

A missing key is a build error, so a string can never silently fall back to English.

## Adding or changing a string

1. Add the key to **both** `src/i18n/en.json` and `src/i18n/ar.json` with the same path.
2. Reference it from `src/index.template.html` (`{{section.key}}`).
3. Run `node apps/website/build.mjs`; it fails if either table is missing the key.
4. Open both `dist/index.html` and `dist/ar/index.html` and check the Arabic page mirrors
   correctly (logical CSS properties only: `margin-inline`, `inset-inline-start`, …).

Keep the Arabic natural Modern Standard Arabic, not a word-for-word translation. Product names,
protocol words (`ETag`, `If-Range`) and file names stay Latin and are wrapped in `direction: ltr`
where they appear inside Arabic text.

## Brand

Colours and logos come from the Riptide brand kit: Abyss `#071A26`, Deep Teal `#075C69`, Riptide
Cyan `#00D5E8`, Ocean Blue `#168BFF`, Soft Ice `#D9F8FA`. The site is light by default (an inline head script sets the theme before paint, whatever the OS prefers) with Abyss dark
behind the toggle; the choice is stored in `localStorage` only. The header and footer use the
light or dark logo according to the theme (`.logo-light` / `.logo-dark` in `site.css`).

## Rules the site follows

- Nothing invented: no user counts, download counts, testimonials, ratings or logos. The star and
  licence links live only in the "Open source" section and are plain links (no badge service), so
  the page makes no third-party request on load.
- The build on offer is whatever `release.json` features (currently an alpha preview, and the hero, Download section and footer say so).
- Only verified, released features are advertised; the rest wait in `features.json`. Simulations (hero card, speed, resume) are labelled "Simulation"; there are no benchmark numbers.
- Feedback goes to GitHub issue forms (`.github/ISSUE_TEMPLATE/`); the site has no form, no
  analytics and no tracking.
- `prefers-reduced-motion` (and a browser without JavaScript) gets every end state and no animation.
- The logo is never recoloured, filtered or animated: the header and footer show the brand-kit SVGs as delivered (light
  logo on light, dark logo on dark).
- Screenshots are real captures with sample data only; the extension captures were taken from the built `apps/extension` in Edge with the native host stubbed. Anything personal is absent or blurred. Research copy keeps the "open sources only" policy, which must stay true of the product.
- Feature copy comes from the repository `README.md`. If the README changes, change the site.
