# Serving the site under load

The site is a folder of static files. There is no server code, no database and no API, so
"handling load" means choosing where the folder is served from and keeping it small.

## Where traffic goes today

| Traffic | Served by | Touches the site? |
|---|---|---|
| Home page, Arabic page, privacy page, CSS, JS, fonts, screenshots | GitHub Pages (a static CDN, Fastly in front) | yes |
| Installer, portable zip, `SHA256SUMS.txt` | GitHub Releases (`github.com/.../releases/download/...`) | no |
| Bug reports, reviews, feature requests, security reports | GitHub Issues / private advisories | no |
| Update check from the app | GitHub Releases | no |

Installers are the heavy part of any launch, and they never pass through the site. A launch
spike therefore costs the site about one page and its assets per visitor.

## What a visit costs

`node apps/website/build.mjs` enforces these budgets (uncompressed) and fails the build when one is
exceeded:

| Item | Budget | Today (approx.) |
|---|---|---|
| Each HTML page | 60 KiB | 40 KiB |
| `css/site.css` | 64 KiB | 47 KiB |
| `js/site.js` | 40 KiB | 18 KiB |
| Any image or SVG | 120 KiB | under 100 KiB |
| Any font file | 110 KiB | 100 KiB (Inter, Latin subset), about 60 KiB per Arabic weight |
| Whole `dist/` | 2 MiB | about 1.2 MiB |

A first visit to the English page transfers roughly 150 to 200 KiB after gzip or brotli (HTML, CSS,
JS, one font, the hero), and screenshots load only when scrolled into view. The Arabic page loads
the two Arabic weights it preloads. What keeps it cheap:

- Screenshots are WebP, with a 720 px and a 1440 px variant chosen through `srcset`.
- Fonts are subset to Latin (Inter) and to Arabic plus Latin (IBM Plex Sans Arabic), WOFF2,
  `font-display: swap`, and the first-paint weights are preloaded. The unused italic file is gone.
- Logos, icons and the favicon are SVG. The hero animation is a canvas that draws no images.
- No third-party request on page load: no CDN fonts, no badge service, no analytics. The build
  fails if a page references an `http(s)` image, script, stylesheet or frame.
- `css/` and `js/` URLs carry a content hash (`?v=`), so they can be cached for a year.
- No framework, no bundler, one small script loaded with `defer`.

### Cache headers

GitHub Pages sends `Cache-Control: max-age=600` and offers no way to change it. Repeat visitors
revalidate every ten minutes (a cheap `304`). The build also writes `dist/_headers`, which Cloudflare
Pages and Netlify read, with `max-age=31536000, immutable` for `assets/`, `css/` and `js/`. GitHub
Pages ignores that file. If you front the site with Cloudflare, set the same rule as a Cache Rule
(see option 1).

## GitHub Pages limits (documented soft limits)

From GitHub's "About GitHub Pages" limits page:

- Published site size: 1 GB. The site is about 1.2 MB.
- Bandwidth: a soft limit of 100 GB per month.
- Builds: a soft limit of 10 per hour. The deploy workflow builds only on pushes to `main` that
  touch `apps/website/`, so this only matters if you push in a loop.
- Deployments time out after 10 minutes.

They are soft: GitHub may email a request to reduce load or ask you to move to another host.
They are not a hard cut-off, and Pages is not meant for commercial or high-volume use.

How far 100 GB goes: at about 200 KiB per first visit that is on the order of 500,000 first visits a
month; with browser caching and repeat visits, more. Fonts and screenshots are the bulk of it.

## If traffic outgrows Pages

Do these in order; each step is small and reversible.

### Option 1: Cloudflare (free plan) in front of Pages

Best first move: nothing is rebuilt, and Cloudflare absorbs repeat requests so GitHub sees only
cache misses.

1. Register a domain (for example `riptide.example`) and add it to a free Cloudflare account;
   point the registrar's name servers at Cloudflare.
2. In the repository settings, Pages, set the custom domain, tick "Enforce HTTPS", and add the
   `CNAME` file Pages asks for (or let the Pages UI create it).
3. In Cloudflare DNS add a proxied (orange cloud) `CNAME` for `www` or an apex `CNAME`
   (flattened) to `layanjarrar1.github.io`. Set SSL/TLS mode to "Full (strict)".
4. Cache Rules: match `/assets/*`, `/css/*`, `/js/*`; Edge TTL 1 year, Browser TTL 1 year. For
   HTML use "Respect origin" or 5 minutes so a deploy shows up quickly.
5. Turn on Auto Minify off (the files are already small), Brotli on, "Always Use HTTPS" on.
6. Build with the new origin so canonical, hreflang and Open Graph URLs are right:
   `SITE_URL=https://riptide.example/ node apps/website/build.mjs`. Set `SITE_URL` as a repository
   variable used by `.github/workflows/pages.yml`.
7. Check the Cloudflare analytics tab for the cache hit ratio. It should be above 90 %.

Cloudflare's privacy settings do not add scripts to the page. Leave "Web Analytics" and "Rocket
Loader" off; the privacy policy says the site loads nothing from third parties.

### Option 2: Move the same `dist/` to Cloudflare Pages or Netlify

The output is plain files, so any static host works unchanged.

Cloudflare Pages:
1. Create a Pages project from the GitHub repository (or use `wrangler pages deploy`).
2. Build command `node apps/website/build.mjs`, output directory `apps/website/dist`, root
   directory empty, environment variable `SITE_URL` set to the new origin.
3. `_headers` is picked up automatically, so assets get the long cache and `nosniff`.
4. Add the custom domain, then switch DNS. Keep the Pages deployment on GitHub as a fallback until
   DNS has settled, then disable the Pages workflow.

Netlify:
1. New site from Git, build command `node apps/website/build.mjs`, publish directory
   `apps/website/dist`, `SITE_URL` as an environment variable.
2. `_headers` is honoured. Add a custom 404 only if you want one; `404.html` is served already.

Both have free tiers with far higher bandwidth ceilings than Pages' soft limit, and neither
changes the repository layout.

### Option 3: Split the heavy assets

If a single asset ever dominates (for example a video), host it on its own cheap bucket or
YouTube embed with a click-to-load poster, never as an autoplaying file. The brand ad
(`riptide-ad-16x9.mp4`) is 53 MB and is deliberately not in this repository or on the site.

## What to monitor

- GitHub: Settings, Pages, for deployment status and any notice from GitHub Support. Watch the
  email address on the account: soft-limit warnings arrive by mail.
- GitHub Actions: the "Pages" workflow run time and failures. A failing verify step means a
  budget or link check tripped.
- Cloudflare (if used): cache hit ratio, bandwidth, and the 4xx/5xx rate.
- Release downloads: `gh api repos/LayanJarrar1/Riptide/releases` shows `download_count` per
  asset, which is the real measure of interest, and it is not tied to the site's bandwidth.
- Page weight: the build prints the total size and fails past the budgets. Re-run Lighthouse on
  `/` and `/ar/` after any change to images or fonts (mobile profile; the targets are 95 or more
  in all four categories).
- Dead links after a release: bump `RELEASE_TAG`, `INSTALLER_FILE` and `PORTABLE_FILE` at the top
  of `build.mjs` and rebuild; the verify step cannot check remote URLs, so open the Download
  buttons once after deploying.

## Not needed

No server-side rendering, no queue, no database, no autoscaling, no analytics service. If a
feature ever seems to need one, it belongs in the app or on GitHub, not on this site.
