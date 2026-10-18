# LinkedIn launch posts

Publish date: **Tuesday 2026-10-20** (or Monday evening once the checklist steps 1–4 are done). Post the English and the Arabic version as two separate posts, a few hours apart (see tips). Each is under 1,300 characters and has no link in the body, because LinkedIn lowers reach for posts with outbound links. The links go in the first comment.

Before posting, confirm the facts still hold on the day: the latest release tag, whether video-page support (yt-dlp + ffmpeg) shipped in alpha.4, and whether queues and batch downloads are merged. The posts only claim what is on `main` or list under "Next".

## English post (1,244 characters)

```text
I wrote a download manager because I was tired of the paid ones.

Riptide is free and open source, for Windows. It is an alpha, and I would like you to break it.

Why: the paid managers want money, many want admin rights, and Arabic and RTL are usually an afterthought. I wanted one with no accounts, no ads, no telemetry, and a UI that is Arabic and English from day one.

What a developer might care about:
- Segmented multi-connection downloads with work stealing
- Resume uses If-Range. If the server answers 200 or the ETag changed, it restarts instead of splicing two versions of a file. Tests compare SHA-256 with the source.
- State lives in SQLite (WAL), checkpointed every second. Kill the process and the download is still resumable.
- The partial file is preallocated as .rtpart and renamed atomically at the end, so there is no "assembling" wait.
- Server filenames are sanitised: no .., no CON/NUL, 200-byte cap.

Also there: Chrome, Edge and Firefox extension, speed limiter, proxy, tray, per-user install.

Next: queues with schedules, batch downloads, store listings for the extension.

Try the alpha, open an issue for whatever breaks, and star the repo if it is useful. Links in the first comment.

#opensource #rust #windows
```

### First comment (English)

```text
Site (download, screenshots): https://layanjarrar1.github.io/Riptide/
Source, releases, issues: https://github.com/LayanJarrar1/Riptide
Latest alpha: https://github.com/LayanJarrar1/Riptide/releases

The extension is not in the browser stores yet (review takes days), so for now it installs from the repo. Tell me which browser you use.
```

## Arabic post (1,121 characters)

Written in Arabic, not translated from the English: same points, different phrasing. LinkedIn sets the direction from the first strong character, so the post renders right to left. Technical names (If-Range, SQLite, WAL, ETag, SHA-256, .rtpart) stay in Latin letters. Check the preview before posting.

```text
تعبت من برامج التحميل المدفوعة، فكتبت واحدًا.

اسمه Riptide: مجاني ومفتوح المصدر لويندوز. ما زال في مرحلة ألفا، وأريدك أن تحاول كسره.

لماذا؟ البرامج المدفوعة تطلب مالًا، وكثير منها يطلب صلاحيات المدير، والعربية واتجاه RTL فيها يأتيان غالبًا في آخر القائمة. أردت برنامجًا بلا حسابات ولا إعلانات ولا تتبّع، وواجهته عربية وإنجليزية منذ اليوم الأول.

ما قد يهم المطوّر:
- تحميل مقسّم على عدة اتصالات، وعند انتهاء اتصال يأخذ جزءًا من أكبر قطعة متبقية
- الاستئناف بـ If-Range. إذا ردّ الخادم بـ 200 أو تغيّر ETag يبدأ من الصفر ولا يخلط نسختين من الملف. الاختبارات تقارن SHA-256 بالمصدر.
- الحالة في SQLite بوضع WAL وتُحفظ كل ثانية، فإذا أُغلقت العملية فجأة يبقى التحميل قابلًا للاستئناف
- الملف الجزئي .rtpart بحجمه النهائي ثم إعادة تسمية ذرية، فلا انتظار لمرحلة «تجميع»
- أسماء الملفات من الخادم تُنقّى: لا .. ولا CON ولا NUL، وحدّ 200 بايت

وفيه أيضًا إضافة لكروم وإيدج وفايرفوكس، ومحدّد سرعة، وبروكسي، وتثبيت بلا صلاحيات مدير.

القادم: طوابير بجداول زمنية، وتحميل دفعات، وإضافة المتصفح في المتاجر الرسمية.

جرّب النسخة، وافتح issue لأي شيء ينكسر، وضع نجمة للمستودع إن أفادك. الروابط في أول تعليق.

#مصدر_مفتوح #Rust #ويندوز
```

### First comment (Arabic)

```text
الموقع (التحميل ولقطات الشاشة): https://layanjarrar1.github.io/Riptide/ar/
الكود والإصدارات والمشاكل: https://github.com/LayanJarrar1/Riptide
آخر نسخة ألفا: https://github.com/LayanJarrar1/Riptide/releases

الإضافة ليست في المتاجر بعد (المراجعة تستغرق أيامًا)، فتُثبَّت الآن من المستودع. أخبروني أي متصفح تستخدمون.
```

## Media

- Video, uploaded natively (LinkedIn plays it inline): `C:\Users\Administrator\Desktop\riptide-brand-kit\07-ads\riptide-ad-16x9.mp4`. The same ad is `apps/website/src/assets/video/riptide-ad.mp4` on the `site/launch` branch (PR #33). 30 seconds, 16:9.
- Screenshot: `assets/readme/shot-list-en.png` (English) and `assets/readme/shot-list-ar.png` (Arabic). LinkedIn allows one media type per post, so with the video attached, put the screenshot in the first comment thread or a reply.
- Add captions to the video; most people watch muted. Upload the file instead of pasting a YouTube link.

## Posting tips

- Timing: Tuesday to Thursday is usually best on LinkedIn, but the launch date is fixed on a Monday, so just post early. For readers in the Middle East plus global tech, 08:00 to 09:00 Gulf time (04:00 to 05:00 UTC) suits the Arabic post; the English post fits about 15:00 Gulf time (11:00 UTC), when it is morning in the US East and midday in Europe. These are defaults, not data. Check your own post analytics afterwards and adjust.
- Stay online for the first hour and reply to every comment with substance, not "thanks!". Technical questions (why If-Range, how resume survives `kill -9`) are the best material.
- Answer "another download manager" and "IDM works fine" calmly and with specifics. Do not edit the post to sound defensive.
- Do not ask friends to leave bulk comments. A few real questions beat twenty empty ones.
- Pin the post to Featured on your profile once it is live.
- Do not tag the products you compare against; name them only if asked.
- Keep to 3 or 4 hashtags, as above.
