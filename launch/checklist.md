# Launch checklist

Launch day: **Monday 2026-10-19** (LinkedIn). Work backwards from it; dates in brackets are suggestions. Tick a box only when it is done.

## 1. Merge what ships (by Fri 2026-10-16)

- [ ] Review and merge #31 (batch downloads)
- [ ] Review and merge #32 (update notifications)
- [ ] Review and merge #33 (website; adds the launch video and `STORE_LINKS`)
- [ ] Review and merge #34 (icons)
- [ ] Review and merge the queues UI PR (branch `m3b/queues-ui`) once it is open
- [ ] `scripts/check.ps1` is green on `main` after the last merge

## 2. Website

- [ ] The Pages workflow ran on `main`; `SITE_URL` in `.github/workflows/pages.yml` resolves to https://layanjarrar1.github.io/Riptide/
- [ ] Open the live site on a phone (English and `/ar/`): layout, RTL, video, download buttons, privacy page
- [ ] The download buttons point at the new release, not alpha.3

## 3. Release v0.1.0-alpha.4 (by Sat 2026-10-17)

Alpha.3 does not contain video-page support or the new features; alpha.4 does.

- [ ] The updater signing key file is on the release machine. `scripts/release.ps1` reads it from `-KeyPath`, never from the repo; without it the installer has no valid `.sig` and self-update breaks for everyone.
- [ ] Run `scripts/release.ps1 -Version 0.1.0-alpha.4 -Prerelease` and follow the `gh` commands it prints (it does not publish, tag or push by itself)
- [ ] Check `SHA256SUMS.txt`, the `.sig`, the portable zip and `latest-beta.json` (re-uploaded to `channel-beta`)
- [ ] From an installed alpha.3, confirm the app offers the update on the beta channel and installs it
- [ ] Release notes: say it is an alpha, list what is new, list known rough edges

## 4. Verify on a clean machine (Sun 2026-10-18)

- [ ] New Windows user account (not an admin): install with the NSIS installer, no UAC prompt
- [ ] First run, Arabic and English, RTL screens
- [ ] Download a large file, kill the process mid-way, reopen: the download resumes and the SHA-256 matches
- [ ] A video page (needs the bundled yt-dlp and ffmpeg)
- [ ] Settings, Integration: native messaging manifests register
- [ ] Portable zip behaves the same
- [ ] Uninstall leaves nothing behind

## 5. Extension stores

- [ ] Submit to Chrome Web Store, Edge Add-ons and AMO: steps in [`extension-stores.md`](extension-stores.md) (issue #29)
- [ ] Reviews can take days and will not finish before 10-19. The posts say so; the extension installs from the repo until then.

## 6. Launch day (Mon 2026-10-19)

- [ ] Re-read both posts in [`linkedin-post.md`](linkedin-post.md) against the live release
- [ ] Post the Arabic version in the morning, the English version in the afternoon
- [ ] Add the first comment with the links to each, right after posting
- [ ] Pin the post to Featured; pin the repo on the GitHub profile
- [ ] Reply to every comment in the first hour

## 7. First 48 hours (until Wed 2026-10-21)

- [ ] Check GitHub issues, the repo's Discussions if enabled, LinkedIn comments and messages twice a day
- [ ] Triage each new issue within 24 h with a label: `bug` (reproducible), `needs-info` (ask for the log and Windows version), `feature` (acknowledge, do not promise), `duplicate`
- [ ] Fix order: data loss or wrong bytes, then crashes and install failures, then UI problems, then everything else
- [ ] Anything that touches resume or checksum correctness stops the line: investigate before shipping more
- [ ] Write a short "what we fixed" reply in the thread when something reported there is fixed

## 8. If something breaks

- [ ] Mark the release as a draft (hides it) or keep it a pre-release and edit the notes with a warning. Do not delete the tag.
- [ ] Update the site's download links to the previous version (v0.1.0-alpha.3) and redeploy Pages
- [ ] Check what `latest-beta.json` on `channel-beta` points to; re-upload the previous version's file so self-update does not offer the broken build
- [ ] Say so in the LinkedIn thread and on the issue, plainly, with the next step
- [ ] Ship alpha.5 with the fix, then restore the links
