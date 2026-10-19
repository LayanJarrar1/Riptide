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
stylesheet or frame from another origin, or a size budget is exceeded (HTML 80 KiB, CSS 64 KiB, JS
40 KiB, images 260 KiB, fonts 110 KiB, whole `dist/` 3 MiB).

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
    css/site.css             tokens and section tones (light first, dark override), layout, every scroll-linked state
    js/site.js               theme, mobile nav, the scroll loop and its chapters, the hero simulation, reveals (no dependencies)
    i18n/en.json             every English string
    i18n/ar.json             every Arabic string (same keys, same shape)
    icons/*.svg              inline icons, inserted with {{{svg name}}}; hero-logo.svg is the brand-kit logo with reveal masks
    assets/fonts/            Inter (variable, Latin subset) and IBM Plex Sans Arabic (Arabic + Latin subset), OFL
    assets/screenshots/      shot-list-<lang>-<theme>.webp captured from the desktop build at 1440x900 (+ `-960`, `-720` variants), and crop-drawer / crop-add panels cut from the details and add-download captures
    assets/riptide-logo-light.svg, riptide-logo-dark.svg   brand kit logos, cropped to their artwork
    assets/favicon.svg       the brand kit app icon
    assets/og-en.png, og-ar.png, apple-touch-icon.png   rendered from og/og.html
  og/og.html                 source of the Open Graph cards and touch icon (render at 1200x630 with
                             any headless browser; `?lang=ar`, `?icon=1`)
  SCALING.md                 limits, budgets and what to do if traffic outgrows GitHub Pages
```

## How the page moves (v4)

The hero opens with a CSS-only choreography (brand mark wipes in, headline lines rise, lead, buttons and chips follow, the
app window rises, then the floating simulation card, speed readout and SHA-256 chip). Nothing about the message or the
buttons depends on it finishing. The only script in the hero is the labelled download simulation (`js/site.js`, section 4),
which pauses when the hero is off-screen.

Four chapters are `position: sticky` stages (`#streams`, `#app`, `#speed`, `#integrity`): each `section.ch` is taller than the
viewport and holds a `.pin` stage. One passive scroll listener schedules at most one `requestAnimationFrame`; the frame turns
the scroll position into a progress value per chapter (negative while it scrolls in, 0 to 1 while pinned) and writes it as
`--p` on the stage. CSS derives everything else with `calc()`/`clamp()`; only `transform`, `opacity`, `stroke-dashoffset` and
`clip-path` change. The first measurement waits for the first scroll (or 4 s of idle time), so loading never forces layout.

- `#streams`: the three streams draw the brand logo (masks, original fills), then the same three lanes continue out of the
  R's stem and merge into one file chip. Reading order: streams in, one exact file out.
- `#app`: the real window flattens from a tilt while four numbered annotations arrive (positions live in `i18n/*.json`
  under `app.callouts`, per language, because the Arabic screenshot is mirrored).
- `#speed`: eight connections with work stealing, a deterministic simulation scrubbed by scroll.
- `#integrity`: power cut and exact resume, then the SHA-256 digest resolves.

On phones (`max-width: 899px`) `#app` and `#integrity` are not pinned and show their end state; `prefers-reduced-motion` and
no-JS show every end state with nothing pinned. Scrolling itself is native: no smooth-scroll library, no scroll-jacking, no
custom cursor. Motion tokens (`--ease-*`, `--dur-*`, `--stagger`) are at the top of `site.css`.

Chapter lengths live in `site.css` (`#speed { --len: 1.8; }` and so on, in viewport heights of pinned scroll).

## Checks

`apps/website/e2e/check.py <base-url>` (Python Playwright, Edge by default) runs against a served build: nav and download
links, theme persistence, language switch, mobile menu, console errors, third-party requests, no video, no filter or blend on
the logo, native scroll, horizontal overflow at 1920x1080, 1440x900, 768x1024 and 390x844 in both languages and themes,
reduced-motion and no-JS states.

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

## Browser-store links

`STORE_LINKS` at the top of `build.mjs` holds the Chrome Web Store and Edge Add-ons URLs. While a
value is empty the Extension section shows "Coming soon" for that store; paste the URL and rebuild
to turn the tile into a link. Nothing else needs to change.

## Rules the site follows

- Nothing invented: no user counts, download counts, testimonials, ratings or logos. The star and
  licence links live only in the "Open source" section and are plain links (no badge service), so
  the page makes no third-party request on load.
- The build on offer is an alpha preview. The hero, the Download section and the footer say so. The release tag and file names live at the top of `build.mjs`.
- Features that are not in a release say so: batch download, site grabber, queues and video pages carry a "Next release" tag.
  Simulations (hero card, speed, resume) are labelled "Simulation"; there are no benchmark numbers. The comparison names no product.
- Feedback goes to GitHub issue forms (`.github/ISSUE_TEMPLATE/`); the site has no form, no
  analytics and no tracking.
- `prefers-reduced-motion` (and a browser without JavaScript) gets no pinning and no scroll animation: the inline head
  script only adds `html.anim` when motion is allowed, and without it every chapter shows its end state.
- The logo is never recoloured. `icons/hero-logo.svg` holds the brand-kit mark and wordmark paths; the scroll animation
  reveals them through masks whose strokes follow the centre line of each stream, so what shows is always the original
  artwork in its original fill (light-logo colours on light, dark-logo colours on dark). Light effects belong to the
  background flow lines, never to the logo.
- Feature copy comes from the repository `README.md`. If the README changes, change the site.

## Deploy

`.github/workflows/pages.yml` builds the site on every push to `main` that touches
`apps/website/` and publishes `apps/website/dist` to GitHub Pages. Enable Pages with
"GitHub Actions" as the source in the repository settings once. If Actions are disabled on the
account, build locally and publish `dist/` by hand to any static host.
