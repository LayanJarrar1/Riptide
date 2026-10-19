# Privacy practices (Chrome Web Store "Privacy" tab; reused for Edge and AMO)

Every field below is plain text to paste. The wording matches what the code does
(`README.md` → Permissions, Privacy).

## Single purpose

Riptide hands downloads from the browser to the Riptide download manager app installed on the same computer, either automatically by the user's rules or when the user chooses "Download with Riptide".

## Permission justifications

**downloads**
Needed to see a download as it starts so it can be handed to the Riptide app when it matches the user's rules, to pause or cancel the browser's copy of a download that Riptide took over, and to let the browser continue the download when the Riptide app is not available.

**nativeMessaging**
The extension's only connection: it sends the download request (address, suggested file name, referrer) to the Riptide desktop app on the same computer through the Native Messaging host "app.riptide.host". The extension has no other network access.

**contextMenus**
Adds "Download with Riptide" to the right-click menu on links, images, video and audio, and "Download all links on this page" / "Download selected links" on pages and selections.

**storage**
Saves the user's capture rules (file types, minimum size, site lists) and language choice locally, and keeps a per-session counter and the list of recent hand-offs shown in the popup. Nothing is synced or sent anywhere.

**notifications**
Tells the user when a download was sent to Riptide, and when the Riptide app is not running, with a button to open it.

**cookies**
Many downloads only work for signed-in users. For a download the user hands to Riptide, the extension reads the cookies for that download's address only and passes them to the local Riptide app, which sends them only to that same site for that one download and deletes them when the download completes. The same applies to videos: only the cookies of the video page's own site are read, and only when the user picks a quality. Cookies are never sent to the developer or any third party.

**Host permission (<all_urls>)**
Downloads can come from any website, so the extension needs access on all sites to: read the cookies of the address being downloaded (above), run the content script that shows the "Download with Riptide" button on hover and the in-page link picker, and (Firefox) read response headers to recognise a download before it starts. The content script only reads link addresses on the page; it also measures the size of video elements to decide whether to show the "Download this video" button; it does not read page content, form fields or browsing history, and sends nothing off the device.

**webRequest, webRequestBlocking (Firefox only)**
Firefox has no downloads.onDeterminingFilename event; inspecting the response headers (Content-Disposition, Content-Type, Content-Length) of top-level responses is the only way to recognise a download before it starts so it can be handed to Riptide instead. It is also used, without blocking, to notice that a page is playing video or audio (only the address and Content-Type of the response are looked at, nothing is stored or sent) so the toolbar badge and the "Download this video" button can appear.

## Remote code

No, I am not using remote code. All JavaScript is bundled in the package; the extension loads no external scripts, uses no eval, and has no network access of its own.

## Data usage (Chrome Web Store checkboxes)

What user data do you plan to collect from users now or in the future? **Tick nothing.**

Reason: the extension does not collect or transmit user data to the developer or any third party. Download addresses and the cookies of the downloaded address are passed only to the Riptide app on the user's own computer to perform the download the user asked for, and are deleted when it completes.

If a reviewer insists that passing cookies to the local app counts as handling, tick **Authentication information** and **Website content** only, and keep all three certifications below.

Certifications (tick all three):
- I do not sell or transfer user data to third parties, outside of the approved use cases.
- I do not use or transfer user data for purposes that are unrelated to my item's single purpose.
- I do not use or transfer user data to determine creditworthiness or for lending purposes.

Privacy policy URL: https://layanjarrar1.github.io/Riptide/privacy/

## Firefox data collection

`manifest/firefox.json` declares `browser_specific_settings.gecko.data_collection_permissions.required: ["none"]`. On AMO answer that the add-on does not collect or transmit data.

## Edge

Edge asks only for the privacy policy URL and whether the product accesses personal information: answer **Yes** (it reads cookies for the downloaded address and passes them to the local app) and give the URL above, so the listing and the policy agree.
