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
stylesheet or frame from another origin, or a size budget is exceeded (HTML 80 KiB, CSS 76 KiB, JS
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
    assets/screenshots/      app-<lang>-<theme>-<width>.webp: the app list captured from the desktop build at a 1040x660 viewport at device scales 0.54 to 2 (560 to 2080 px wide, so the browser rarely resamples), plus crop-drawer / crop-add panels cut from the details and add-download captures
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

Two chapters are `position: sticky` stages, and only on desktop (900px and wider) with full motion: `#streams` (about 100 vh of
scroll) and `#app` (about 60 vh). The scroll loop turns the scroll position into a progress value per stage (negative while it
scrolls in, 0 to 1 while pinned, reaching 1 at 85 % of the stage so there is a resting moment before the next section) and
writes it as `--p`. The value is eased toward the scroll position (a light lerp, factor 0.15 per frame, snapping when the delta
is tiny); only the value is smoothed, scrolling itself stays native. One `requestAnimationFrame` loop runs while something
moves and sleeps otherwise. CSS derives everything else with `calc()`/`clamp()`; only `transform`, `opacity`,
`stroke-dashoffset` and `clip-path` change.

`#speed`, `#integrity`, and every chapter on phones or in calm motion are not pinned: each plays once, by time (2 to 6 s),
when it comes into view (an IntersectionObserver), and is then left at its end state.

- `#streams`: the three streams draw the brand logo (masks, original fills), then the same three lanes continue out of the
  R's stem and merge into one file chip. Reading order: streams in, one exact file out.
- `#app`: the real window flattens from a tilt while two plain-dot annotations arrive (positions live in `i18n/*.json`
  under `app.callouts`, per language, because the Arabic screenshot is mirrored).
- `#speed`: eight connections with work stealing, a deterministic simulation.
- `#integrity`: power cut and exact resume, then the SHA-256 digest resolves.

Scrolling itself is native: no smooth-scroll library, no scroll-jacking, no custom cursor. Motion tokens (`--ease-*`, `--dur-*`,
`--stagger`) are at the top of `site.css`; hover and focus feedback is at most 150 ms, every one-shot animation at most 700 ms (the
hero ribbon draw is the one decorative exception), the rest is scroll-scrubbed or an ambient loop that pauses off-screen.

### Motion modes and levels

The head script resolves `html[data-motion="full|reduced"]` before first paint: `?motion=on|off` (stored, then removed from the URL) wins, then the saved
`riptide-motion` choice from the header's Animations button (`aria-pressed`), then the OS `prefers-reduced-motion`. Full sets `html.anim`, reduced sets `html.calm`.

- Full is the whole story (parallax, 3D tilt, pinned scrubbed stages, slides).
- Calm ("reduce, don't remove") keeps everything non-spatial: opacity reveals with their stagger, the app preview's progress bars
  filling, counters, the SHA-256 digest resolving, stroke draws, the hero simulation. It drops parallax, the tilt, large slides,
  scrubbed pinned stages and endless loops. When the OS asks for reduced motion and the visitor has not chosen yet, a small
  dismissible hint under the header offers "Turn them on?" (`riptide-motion-hint` remembers a dismissal).
- `html[data-level="lite"]` (separate from the mode) drops parallax, the tilt, ambient loops and the third ribbon lane and shortens the
  simulations. It starts on `hardwareConcurrency <= 2`, `deviceMemory <= 2` or `saveData`, and a frame-time monitor in the engine loop
  switches to it (and remembers it for the session in `sessionStorage`) when the median frame stays above 32 ms while moving.

Robustness: the engine re-measures on resize, orientation change, font load and image load, restores on bfcache `pageshow`, hash jumps
and back/forward, pauses everything in a hidden tab, and clamps every progress value. Hidden start states in the CSS only exist under
`html.js-ready`, which `site.js` adds after a successful init (any error in `site.js` removes it and forces every reveal visible), so
a script failure never hides content; without JS the CSS shows every end state.

The header is the top layer (`z-index: 100`, near-opaque tone background plus blur) and the hero cards sit inside the hero stage; the e2e check asserts both.

## Releasing

The page links to one release (currently v0.1.0-alpha.4). To point it at a new build, change the one constant in `build.mjs`:

```js
const RELEASE_VERSION = '0.1.0-alpha.4';   // next build: '0.1.0-alpha.5'
```

The hero badge ("Alpha {n}"), the download section badge, the installer and portable file names, the tag in every download URL
the SHA256SUMS link, the "All releases" link (`releases/tag/v<version>`), the version shown on the buttons, in "What's new in …" and in the footer all derive from it. The "What's new" list itself (`download.new.items` in both i18n files) is edited by hand for each release. Do this only after the assets exist on the GitHub release (installer
`Riptide_<version>_x64-setup.exe`, portable zip `Riptide-<version>-portable.zip`, `SHA256SUMS.txt`), then run `node apps/website/build.mjs`
and the e2e check.

Deploy: GitHub Pages is published by `.github/workflows/pages.yml` on every push to `main` that touches `apps/website`. It needs the
repository to be public (or a plan that includes Pages for private repositories) and Settings, Pages, Source set to "GitHub Actions".
Merge the site PR only after the release exists, so the download links are live the moment the page is.

## Checks

`apps/website/e2e/check.py <base-url>` (Python Playwright, Edge by default) runs against a served build: nav and download
links, theme persistence, language switch, mobile menu, console errors, third-party requests, no video, no filter or blend on
the logo, native scroll, horizontal overflow at 1920x1080, 1440x900, 768x1024 and 390x844 in both languages and themes,
calm-motion and no-JS states, the first-visit hint, the engine (progress follows scroll, reload, back/forward, anchor jumps, resize 1440 to 390 to 1440, hidden tab, CPU throttling to lite, no IntersectionObserver, script failure, layout shift), pinned stage budgets, no numbered labels or "Next release" tags, the header/hero no-overlap assertion at seven viewports, CTAs above the fold at 1366x768 and 1350x530, and the Animations override (`?motion=on` animates under an OS reduce setting and persists).

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
