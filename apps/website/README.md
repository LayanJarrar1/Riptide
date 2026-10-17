# Riptide website

The public product site: one static, bilingual page (English at `/`, Arabic at `/ar/`) plus a
bilingual `404.html`. Plain HTML, CSS and vanilla JavaScript. No framework, no npm packages, no
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

The build fails (exit code 1) when a page still contains a `{{ }}` token, a local `href`/`src`
does not resolve to a file, an `<img>` is missing `alt`, `width` or `height`, a font referenced
from the stylesheet is missing, or a structural tag is left unclosed.

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
    css/site.css             tokens (light first, dark override), layout, step animations
    js/site.js               theme, mobile nav, hero canvas, in-view animations, lightbox
    i18n/en.json             every English string
    i18n/ar.json             every Arabic string (same keys, same shape)
    icons/*.svg              inline icons, inserted with {{{svg name}}}
    assets/fonts/            Inter (variable) and IBM Plex Sans Arabic, OFL
    assets/screenshots/      copies of assets/readme/shot-*.png
    assets/favicon.svg       the typographic mark
    assets/og-en.png, og-ar.png, apple-touch-icon.png
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

## Rules the site follows

- Nothing invented: no user counts, download counts, testimonials, ratings or logos. The
  badges in the "Open source" section read live data from shields.io, which is the only
  third-party request on the page.
- The build on offer is an alpha preview. The hero, the Download section, the FAQ and the
  footer say so. The release tag and file names live at the top of `build.mjs`.
- Light theme by default, flat surfaces, no gradients. Dark is available through the toggle
  or when the system prefers dark.
- The browser-extension buttons point at the extension's folder on GitHub until store
  listings exist; no store badges are shown.
- `prefers-reduced-motion` stops the canvas, the step animations and the entrance.
- Feature copy comes from the repository `README.md`. If the README changes, change the site.

## Deploy

`.github/workflows/pages.yml` builds the site on every push to `main` that touches
`apps/website/` and publishes `apps/website/dist` to GitHub Pages. Enable Pages with
"GitHub Actions" as the source in the repository settings once. If Actions are disabled on the
account, build locally and publish `dist/` by hand to any static host.
