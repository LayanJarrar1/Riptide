# Riptide v0.1.0-alpha.4 release notes

> **Draft — update after merging; items from PRs not merged by release time must be removed.**
> Each item lists its PR. "(open)" means the PR was not merged when this draft was written. Strike any item whose PR is not merged at release time.

---

## English

### New
- Video pages: download from YouTube, Vimeo and other video sites through the bundled yt-dlp and ffmpeg. Pick a quality (best, 2160p down to 144p, or audio as M4A or MP3). Settings → About → Video support has an update button. (#21, merged)
- Download queues: create queues with their own concurrency limit, start and stop them, and schedule them (a daily window, "only at night", chosen days, or a one-time start). When a queue finishes you can shut down, sleep, hibernate, exit, or run a program. (#22 engine, merged; #36 queue screens, open)
- Batch download: expand patterns such as `file[001-120].zip` or `img_[a-z].jpg`, or paste a list of links. Import and export URL lists. (#31, open)
- Browser: a "Download this video" button and quality picker on video pages in Chrome, Edge and Firefox. (#37, open)
- Browser: download all links, selected links or all images on a page, with type filters and per-site rules. (#38, open)
- Command line: `riptide grab <url>` crawls a site to a set depth and lists or downloads the files it finds. It follows robots.txt unless you pass `--no-robots`. (#39, open)
- Update notifications: a system notification when a new version is out, a badge in Settings, and a "What's new" link after updating. (#32, open)

### Improved
- Servers that answer 429 or 503 make Riptide use fewer connections to that host, and it remembers the lower limit after a restart. (#22, merged)
- Smaller startup bundle: the app loads 617 kB of JavaScript down to 484 kB, and the Arabic strings and dialogs load on demand. (#40, open)
- New app icons and README banner from the brand kit. (#34, open)

### Fixed
- Video downloads could end with the last progress update missing, so the bar stopped short of 100%. (#23, merged)

### Known limitations
- This is an alpha. Expect rough edges and keep copies of important files.
- The browser extension is not in the Chrome Web Store or Edge Add-ons yet.
- A sleeping PC is not woken by schedules. The PC has to be awake when a scheduled queue starts.
- "Run a program" takes no arguments.

### Download
https://github.com/LayanJarrar1/Riptide/releases/tag/v0.1.0-alpha.4

---

## العربية

### الجديد
- صفحات الفيديو: نزّل من يوتيوب وفيميو ومواقع فيديو أخرى عبر yt-dlp وffmpeg المرفقين بالتطبيق. اختر الجودة: أفضل جودة، أو من 2160p حتى 144p، أو صوتاً بصيغة M4A أو MP3. في الإعدادات ← حول ← دعم الفيديو زر للتحديث. (#21، مدموج)
- قوائم التنزيل: أنشئ قوائم، ولكل قائمة حد خاص لعدد التنزيلات المتزامنة. شغّلها وأوقفها، وجدولها: نافذة يومية، أو "ليلاً فقط"، أو أيام محددة، أو بدء لمرة واحدة. عند انتهاء القائمة يمكنك إيقاف التشغيل أو السكون أو السبات أو إغلاق التطبيق، أو تشغيل برنامج. (#22 المحرك، مدموج؛ #36 شاشات القوائم، مفتوح)
- التنزيل الدفعي: وسّع أنماطاً مثل `file[001-120].zip` أو `img_[a-z].jpg`، أو الصق قائمة روابط. ويمكنك استيراد قوائم الروابط وتصديرها. (#31، مفتوح)
- المتصفح: زر "نزّل هذا الفيديو" واختيار الجودة في صفحات الفيديو، على Chrome وEdge وFirefox. (#37، مفتوح)
- المتصفح: نزّل كل الروابط أو الروابط المحددة أو كل الصور في الصفحة، مع فلاتر حسب النوع وقواعد لكل موقع. (#38، مفتوح)
- سطر الأوامر: الأمر `riptide grab <url>` يمسح موقعاً حتى عمق تحدده، ويعرض الملفات التي يجدها أو ينزلها. يلتزم بملف robots.txt إلا إذا أضفت `--no-robots`. (#39، مفتوح)
- إشعارات التحديث: إشعار في النظام عند توفر إصدار جديد، وشارة في الإعدادات، ورابط "ما الجديد" بعد التحديث. (#32، مفتوح)

### المحسّن
- إذا ردّ الخادم برمز 429 أو 503، يقلّل Riptide عدد الاتصالات المتجهة إلى ذلك الخادم، ويتذكّر هذا الحد بعد إعادة التشغيل. (#22، مدموج)
- حزمة بدء أصغر: كان التطبيق يحمّل 617 كيلوبايت من JavaScript، والآن 484 كيلوبايت. الترجمة العربية والنوافذ الحوارية تُحمَّل عند الحاجة. (#40، مفتوح)
- أيقونات التطبيق الجديدة وبانر صفحة README من هوية Riptide البصرية. (#34، مفتوح)

### الإصلاحات
- كان تنزيل الفيديو قد ينتهي دون آخر تحديث للتقدم، فتتوقف شريطة التقدم دون 100%. (#23، مدموج)

### القيود المعروفة
- هذه نسخة ألفا، وقد تجد عيوباً صغيرة. احتفظ بنسخ من ملفاتك المهمة.
- إضافة المتصفح ليست في المتاجر بعد: لا في Chrome Web Store ولا في Edge Add-ons.
- الجدولة لا توقظ جهاز الكمبيوتر النائم. يجب أن يكون الجهاز مستيقظاً حين تبدأ القائمة المجدولة.
- الأمر "تشغيل برنامج" لا يقبل وسائط إضافية.

### التنزيل
https://github.com/LayanJarrar1/Riptide/releases/tag/v0.1.0-alpha.4
