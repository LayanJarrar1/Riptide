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
stylesheet or frame from another origin, or a size budget is exceeded (HTML 60 KiB, CSS 64 KiB, JS
40 KiB, images 120 KiB, fonts 110 KiB, whole `dist/` 2 MiB).

`SITE_URL` sets the public origin used for canonical, `hreflang` and Open Graph URLs (default:
the GitHub Pages URL of the repository). Every in-page link is relative, so the site also works
from a subfolder or `file://`.

## Structure

```
apps/website/
  build.mjs                  the whole build: template engine, copy, verify, preview server
  src/
    index.template.html      the page, rendered once per language
    404.template.html        bilingual not-found page
    privacy.template.html    privacy policy, rendered to privacy/ and ar/privacy/ (strings under `privacy`)
    css/site.css             tokens (light first, dark override), layout, step animations
    js/site.js               theme, mobile nav, hero canvas, in-view animations, lightbox
    i18n/en.json             every English string
    i18n/ar.json             every Arabic string (same keys, same shape)
    icons/*.svg              inline icons, inserted with {{{svg name}}}
    assets/fonts/            Inter (variable, Latin subset) and IBM Plex Sans Arabic (Arabic + Latin subset), OFL
    assets/screenshots/      WebP copies of assets/readme/shot-*.png, 1440 px and a 720 px `-720` variant
    assets/riptide-logo-light.svg, riptide-logo-dark.svg   brand kit logos, cropped to their artwork
    assets/favicon.svg       the brand kit app icon
    assets/og-en.png, og-ar.png, apple-touch-icon.png   rendered from og/og.html
  og/og.html                 source of the Open Graph cards and touch icon (render at 1200x630 with
                             any headless browser; `?lang=ar`, `?icon=1`)
  SCALING.md                 limits, budgets and what to do if traffic outgrows GitHub Pages
```

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
Cyan `#00D5E8`, Ocean Blue `#168BFF`, Soft Ice `#D9F8FA`. The site is dark first (Abyss) with a light
theme behind the toggle; the choice is stored in `localStorage` only. The header and footer use the
light or dark logo according to the theme (`.logo-light` / `.logo-dark` in `site.css`).

## Browser-store links

`STORE_LINKS` at the top of `build.mjs` holds the Chrome Web Store and Edge Add-ons URLs. While a
value is empty the Extension section shows "Coming soon" for that store; paste the URL and rebuild
to turn the tile into a link. Nothing else needs to change.

## Rules the site follows

- Nothing invented: no user counts, download counts, testimonials, ratings or logos. The star and
  licence links live only in the "Open source" section and are plain links (no badge service), so
  the page makes no third-party request on load.
- The build on offer is an alpha preview. The hero, the Download section, the FAQ and the
  footer say so. The release tag and file names live at the top of `build.mjs`.
- Features that are not in a release say so: video pages ("Next alpha") and queues with schedules
  ("Coming soon").
- Feedback goes to GitHub issue forms (`.github/ISSUE_TEMPLATE/`); the site has no form, no
  analytics and no tracking.
- `prefers-reduced-motion` stops the canvas, the step animations and the entrance.
- Feature copy comes from the repository `README.md`. If the README changes, change the site.

## Deploy

`.github/workflows/pages.yml` builds the site on every push to `main` that touches
`apps/website/` and publishes `apps/website/dist` to GitHub Pages. Enable Pages with
"GitHub Actions" as the source in the repository settings once. If Actions are disabled on the
account, build locally and publish `dist/` by hand to any static host.
