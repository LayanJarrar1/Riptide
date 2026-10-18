# Getting the extension into the browser stores (issue #29)

## Can users install straight into Chrome without GitHub?

Only through the Chrome Web Store. On Windows and macOS, Chrome blocks extensions installed from outside the store for normal users; an off-store CRX is refused unless an enterprise policy allows it. The only route that gives a one-click "Add to Chrome" button is a Web Store listing. The same applies in spirit to the others, and each has its own store:

| Browser | Store | Cost |
|---|---|---|
| Chrome | Chrome Web Store | one-time US$5 developer registration |
| Edge | Microsoft Edge Add-ons (Partner Center) | free |
| Firefox | addons.mozilla.org (AMO) | free |

Until approval, users load the unpacked extension from the repo (Chrome/Edge developer mode, Firefox temporary add-on), as the README describes.

## Steps for Layan

All packaging and form-filling details are in [`apps/extension/store/SUBMISSION.md`](../apps/extension/store/SUBMISSION.md). Do not copy them here; follow that file.

1. **Create the accounts.** Chrome: https://chrome.google.com/webstore/devconsole (pay the US$5 once). Edge: https://partner.microsoft.com/dashboard/microsoftedge. Firefox: https://addons.mozilla.org/developers/.
2. **Build the packages** (SUBMISSION.md, section 0): bump the version in `package.json` and `manifest/base.json`, then `npm ci`, `npm run check`, `npm test`, `npm run package` in `apps/extension`. This produces the Chrome, Edge and Firefox zips and the source zip AMO asks for.
3. **Make sure the privacy policy page is live** (published by the website, `/privacy/` and `/ar/privacy/`), and that a desktop release is downloadable: reviewers install it to test.
4. **Fix the extension ID first.** Stores assign their own ID, and the native host only answers IDs it knows. Upload a draft, copy the ID the dashboard shows, add it to the host allow-list and the manifest written by the app (details in SUBMISSION.md, "Why the store zips have no `key`"), and ship that desktop build. Skipping this means the store version cannot talk to the app.
5. **Fill the listing** from `listing-en.md` and `listing-ar.md` (name, descriptions, screenshots, promo tiles), and the privacy tab from `privacy-practices.md`. Paste the review notes from SUBMISSION.md into all three stores.
6. **Submit.** Typical review times: Chrome takes several days because the extension asks for `<all_urls>` and gets an in-depth review; Edge and Firefox usually take a few days too, and AMO may ask questions about the build. Plan for a week and do not tie the launch to approval.
7. **After approval**, paste the three store URLs into `STORE_LINKS` in `apps/website/build.mjs` (on `site/launch`, PR #33) so the site shows the install buttons, and into the app's integration settings if it has a place for a store link. Rebuild and redeploy the site, then update the first comment of the LinkedIn posts.

## Rejection

If a store rejects it, the email names the policy. Fix it, bump the version, upload again. Permission justifications are the usual cause; they live in `privacy-practices.md`.
