# Release process

One source of truth: the GitHub release. Everything the public sees (README download links, the website,
the "New in Riptide" pill) is derived from it, and scripts fail when they drift apart.

```
GitHub release (assets + SHA256SUMS.txt)
        |  node scripts/gen-release-manifest.mjs
        v
apps/website/release.json  --->  website build (buttons, badge, notes links, pill)
        |
        +--->  scripts/check-doc-sync.mjs  <---  README, CHANGELOG, Cargo.toml, tauri.conf.json, package.json
        +--->  scripts/check-release-links.mjs   (every download/notes link answers 200)
```

`release.json` records, per release: tag, version, channel (`alpha`, `beta`, `stable`), publish date, notes URL,
installer and portable name, URL, size and SHA-256, and which tag is the latest stable and latest pre-release.
The generator accepts only `v<x.y.z>` / `-alpha.N` / `-beta.N` tags, only asset URLs under
`https://github.com/LayanJarrar1/Riptide/releases/download/<tag>/`, only `Riptide_<version>_x64-setup.exe` and
`Riptide-<version>-portable.zip`, and a SHA-256 that agrees between `SHA256SUMS.txt` and GitHub's own digest.
Anything else makes it exit with an error and write nothing. The site reads the file at build time only: no
runtime API calls, nothing taken from the query string. Releases older than `0.1.0-alpha.3` predate the
checksum file and are not listed.

## Testing the self-update before a release

`scripts/test-update-e2e.ps1` builds two disposable signed installers (`0.1.0-beta.901` and `.902`) with the
real updater key from `%USERPROFILE%\.riptide-keys`, installs the old one into a temp folder in portable mode,
serves a signed manifest from `http://127.0.0.1`, and checks two things: a package with one flipped byte is
refused with a signature error and nothing changes, and the genuine package upgrades the app (new version
installed and started by the installer). Registry keys, shortcuts and a running installed Riptide are restored
exactly (`-StopInstalledApp` closes it first and starts it again). The test builds use the `update-e2e` cargo
feature; release builds never do (see decision `D-updater-2`). Run it before tagging when the updater, the
installer hooks or the Tauri/plugin versions changed. `scripts/verify-update-feed.ps1 -RunningVersion <v>` then
proves the published feed offers the new version to people on `<v>`.

## Checklist (all seven, in this order)

1. **Confirm the implementation.** The feature works in the built installer, not only in a dev build.
2. **Confirm the tests.** `scripts/check.ps1` is green on the release commit; manual-test notes for what tests cannot cover.
3. **Update the docs.** README, `guide/` and the website strings describe what this release really does. A feature
   that is not verified and released stays out of README, site and notes (see `apps/website/src/features.json`).
4. **Update the CHANGELOG.** Move `[Unreleased]` under a new `## [<version>] - <date>` heading.
5. **Update the release metadata.** Bump `version` in `Cargo.toml` (`[workspace.package]`) and
   `apps/desktop/package.json` (`tauri.conf.json` has no version of its own; it inherits Cargo's). Tag, build and
   publish the release with the installer, the portable zip and `SHA256SUMS.txt`. Then:

   ```powershell
   node scripts/gen-release-manifest.mjs            # refreshes apps/website/release.json
   node scripts/set-release-version.mjs <version>   # README links + RELEASE_VERSION in build.mjs
   node scripts/check-doc-sync.mjs                  # must end with "doc sync: ok"
   ```

6. **Update the marketing content.** Edit `download.new.items` in `apps/website/src/i18n/en.json` and `ar.json`
   (the "What's new" list) and flip any verified feature in `features.json` (below). Then build and run the e2e check:

   ```powershell
   node apps/website/build.mjs
   node apps/website/build.mjs --check --serve 8391     # keep running, then in another terminal:
   python apps/website/e2e/check.py http://127.0.0.1:8391/
   ```

7. **Verify the links.** `node scripts/check-release-links.mjs` fetches every download and notes link and expects
   200. While the repository is private it prints a NOTICE and skips (unauthenticated requests get 404), so it is a
   real check only once the repository is public. Run it by hand then, or let the daily `site` workflow do it.

## Preparing a release in a pull request (documents ahead of release.json)

`release.json` describes what is published, so it cannot name a release that does not exist yet, and nobody edits it
by hand. The pull request that prepares a release therefore has the version bumped everywhere (README, CHANGELOG,
Cargo, package.json, `RELEASE_VERSION`, `features.json` entries flipped) while `release.json` still lists the previous
release. Two explicit switches make that state legal, and only that state:

- `RIPTIDE_RELEASE_AHEAD=1` (or `--release-ahead`) for `node apps/website/build.mjs`: when `RELEASE_VERSION` is newer
  than the featured release in `release.json`, the site builds for the release that is about to be published (download
  names and links follow the release contract; there is no size or checksum yet) and the `features.json` entries that
  name that version are rendered. Without the switch the build keeps failing in CI on a mismatch.
- `RIPTIDE_RELEASE_AHEAD=1` (or `--allow-unpublished`) for `node scripts/check-doc-sync.mjs`: `release.json` may be older
  than the version all the documents agree on. The documents must still agree with each other, and `release.json` may
  never be newer than they are, so the check keeps protecting every other case. Unit tests: `scripts/check-doc-sync.test.mjs`.

The `site` workflow sets the variable for `pull_request` runs only. The run after the merge, the daily run and the
`release: published` run are strict: after the merge they fail until you have published the release and run
`node scripts/gen-release-manifest.mjs`, which is the intended reminder. Do not commit a hand-made or fixture
`release.json`: it is regenerated from the published release (step 5 above).

## Switching on a verified feature

`apps/website/src/features.json` holds the EN/AR copy for features that are not released yet, each with
`"verified": false, "release": null`. Once the owner has verified a feature in a published release, change that
entry to `"verified": true, "release": "<version>"` (no leading `v`; the version must exist in `release.json`) and
rebuild. Both fields move together or the build fails. The entry then appears in the "What's new" list of that
release, in both languages. Never flip it before the release exists.

## What runs automatically

- `.github/workflows/site.yml` on push to `main` (site files), pull requests, `release: published`, manual dispatch
  and daily at 05:17 UTC: regenerates the manifest, runs the script tests, the doc-sync check, the site build, the
  e2e check and the link check. It deploys to GitHub Pages (official `upload-pages-artifact` and `deploy-pages`,
  pinned by commit) only when the repository variable `SITE_DEPLOY` is `true`; until then it is build and test only.
- A published release that README, CHANGELOG or `RELEASE_VERSION` do not yet match makes the run fail on purpose:
  that is the reminder to finish steps 4 to 6. The previous site keeps serving.
- `scripts/check.ps1` runs the script unit tests. The doc-sync check is not in `check.ps1` because a release
  candidate (Cargo already bumped, release not yet published) would fail it by design.
