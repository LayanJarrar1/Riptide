//! Arabic / English strings for the CLI. Two tables with identical keys; a test
//! keeps them in sync.

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// العربية
    Ar,
    /// English
    En,
}

impl Lang {
    /// Picks the language: an explicit `--lang`, else `RIPTIDE_LANG`, else `LANG` / `LC_ALL`,
    /// else English.
    pub fn detect(explicit: Option<&str>) -> Lang {
        let from_env = || {
            ["RIPTIDE_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .filter_map(|k| std::env::var(k).ok())
                .find(|v| !v.is_empty())
        };
        let pick = match explicit {
            Some("auto") | None => from_env(),
            Some(other) => Some(other.to_string()),
        };
        match pick.as_deref().map(|s| s.to_ascii_lowercase()) {
            Some(s) if s.starts_with("ar") => Lang::Ar,
            _ => Lang::En,
        }
    }
}

/// One table entry: key, Arabic, English.
type Entry = (&'static str, &'static str, &'static str);

/// All strings. Keep alphabetical by key (a test checks).
pub const STRINGS: &[Entry] = &[
    ("about", "ريبتايد — مدير تحميل مجاني ومفتوح المصدر", "Riptide — a free, open-source download manager"),
    ("arg.batch_dir", "مجلد الحفظ (يُنشأ إن لم يكن موجودًا)؛ الافتراضي المجلد الحالي", "Folder to save into (created if missing); default: the current folder"),
    ("arg.batch_list", "طباعة الروابط الناتجة عن النمط دون تحميل", "Print the addresses the pattern makes, without downloading"),
    ("arg.bench_connections", "قائمة عدد الاتصالات لكل جولة، مثل 1,8,32", "Connection counts to try, e.g. 1,8,32"),
    ("arg.checksum", "بصمة الملف المتوقعة (MD5 أو SHA-1 أو SHA-256 بالست عشري)؛ يفشل التحميل إن لم تتطابق", "Expected checksum (MD5, SHA-1 or SHA-256 hex); the download fails if it does not match"),
    ("arg.connections", "عدد الاتصالات المتزامنة (1–32)", "Number of simultaneous connections (1–32)"),
    ("arg.data_dir", "مجلد البيانات (الافتراضي: %LOCALAPPDATA%\\Riptide أو data/ بجانب البرنامج في الوضع المحمول)", "Data directory (default: %LOCALAPPDATA%\\Riptide, or data/ next to the exe in portable mode)"),
    ("arg.grab_any_site", "السماح بالتنقل بين أي مواقع (يحدّها العمق والحدود)", "Walk into any site (bounded by depth and the limits)"),
    ("arg.grab_cookie", "قيمة ترويسة Cookie؛ تُرسل إلى نطاق صفحة البداية فقط", "Cookie header value; sent to the start page's origin only"),
    ("arg.grab_depth", "عمق الروابط: 0 للصفحة الأولى فقط", "Link depth: 0 reads only the start page"),
    ("arg.grab_exclude", "نمط glob لمسار الرابط يُستبعد (يمكن تكراره)، مثل **/ads/**", "Glob on the URL path to skip (repeatable), e.g. **/ads/**"),
    ("arg.grab_include", "نمط glob لمسار الملفات المطلوبة (يمكن تكراره)، مثل *.pdf", "Glob on the URL path of files to keep (repeatable), e.g. *.pdf"),
    ("arg.grab_list", "اعرض الملفات التي وُجدت فقط دون تحميلها", "Only list the files that were found; download nothing"),
    ("arg.grab_max_files", "أقصى عدد للملفات (الافتراضي 5000)", "Most files to list (default 5000)"),
    ("arg.grab_max_pages", "أقصى عدد للصفحات المقروءة (الافتراضي 500)", "Most pages to read (default 500)"),
    ("arg.grab_max_size", "أكبر حجم للملف، مثل 500 أو 64k أو 5M", "Largest file size, e.g. 500, 64k, 5M"),
    ("arg.grab_min_size", "أصغر حجم للملف، مثل 500 أو 64k أو 5M", "Smallest file size, e.g. 500, 64k, 5M"),
    ("arg.grab_no_robots", "تجاهل robots.txt (لمواقعك أنت فقط)", "Ignore robots.txt (for your own sites only)"),
    ("arg.grab_output", "المجلد الذي تُحفظ فيه الملفات (الافتراضي: المجلد الحالي)", "Folder to save the files in (default: the current folder)"),
    ("arg.grab_same_domain", "البقاء في النطاق نفسه (مثل example.com وكل نطاقاته الفرعية) بدل المضيف نفسه", "Stay on the same domain (example.com and its subdomains) instead of the same host"),
    ("arg.grab_time_limit", "أقصى مدة للزحف بالثواني (الافتراضي 600)", "Longest crawl in seconds (default 600)"),
    ("arg.grab_types", "أنواع الملفات، مثل video,documents (compressed, documents, music, video, programs, images, other)", "File types, e.g. video,documents (compressed, documents, music, video, programs, images, other)"),
    ("arg.grab_url", "صفحة البداية", "The page to start from"),
    ("arg.header", "ترويسة إضافية بصيغة Name: value (يمكن تكرارها)؛ تُرسل إلى نطاق الرابط فقط", "Extra header as Name: value (repeatable); sent to the link's origin only"),
    ("arg.insecure", "تجاهل أخطاء شهادة TLS لهذا التحميل (غير آمن)", "Ignore TLS certificate errors for this download (unsafe)"),
    ("arg.json", "طباعة النتائج بصيغة JSON", "Print the results as JSON"),
    ("arg.lang", "لغة الرسائل: ar أو en أو auto", "Message language: ar, en or auto"),
    ("arg.log_level", "مستوى السجل: error | warn | info | debug | trace أو عبارة تصفية مثل riptide_net=debug,info", "Log filter: error | warn | info | debug | trace, or a directive like riptide_net=debug,info"),
    ("arg.no_resume", "تجاهل أي ملف جزئي والبدء من الصفر", "Ignore any partial file and start from zero"),
    ("arg.output", "مسار الحفظ (ملف أو مجلد)", "Where to save (file or folder)"),
    ("arg.overwrite", "استبدال الملف إن كان موجودًا بدل تسميته name (1)", "Replace an existing file instead of naming the new one name (1)"),
    ("arg.password", "كلمة المرور لمصادقة Basic/Digest", "Password for Basic/Digest authentication"),
    ("arg.pattern", "نمط الرابط، مثل https://example.com/file[001-120].zip أو img_[a-z].jpg أو [0-100:5]", "URL pattern, e.g. https://example.com/file[001-120].zip, img_[a-z].jpg or [0-100:5]"),
    ("arg.quiet", "بدون شريط تقدّم", "No progress output"),
    ("arg.referer", "قيمة Referer (صفحة الرابط)", "Referer value (the page the link came from)"),
    ("arg.url", "الرابط المراد تحميله", "The URL to download"),
    ("arg.user", "اسم المستخدم لمصادقة Basic/Digest", "Username for Basic/Digest authentication"),
    ("arg.user_agent", "قيمة User-Agent تحاكي متصفحًا", "User-Agent value to mimic a browser"),
    ("batch.count", "{n} رابط", "{n} addresses"),
    ("batch.error.empty", "اكتب نمط الرابط أولًا.", "Give an address pattern first."),
    ("batch.error.invalid_char", "الرابط يحتوي على مسافة أو رمز غير مسموح.", "The address contains a space or a character that is not allowed."),
    ("batch.error.invalid_range", "النطاق غير صحيح. أمثلة: [1-10] أو [001-120] أو [a-z] أو [0-100:5].", "That range is not valid. Examples: [1-10], [001-120], [a-z], [0-100:5]."),
    ("batch.error.not_http", "يجب أن يبدأ الرابط بـ http:// أو https://.", "The address must start with http:// or https://."),
    ("batch.error.number_too_large", "أحد الأرقام في النطاق كبير جدًا.", "A number in the range is too large."),
    ("batch.error.too_many", "هذا النمط ينتج أكثر من {limit} رابط. صغّر النطاق.", "This pattern makes more than {limit} addresses. Make the range smaller."),
    ("batch.error.unclosed", "هناك قوس [ بلا قوس إغلاق ]. اكتب \\[ للحصول على قوس عادي.", "A [ has no closing ]. Write \\[ for a plain bracket."),
    ("batch.error.zero_step", "يجب ألا تقل الخطوة عن 1.", "The step must be at least 1."),
    ("batch.item", "الملف {i} من {n}: {url}", "File {i} of {n}: {url}"),
    ("batch.summary", "تم تحميل {ok} من {n}، وفشل {failed}", "{ok} of {n} downloaded, {failed} failed"),
    ("bench.col.avg", "متوسط السرعة", "avg speed"),
    ("bench.col.conn", "الاتصالات", "connections"),
    ("bench.col.peak", "الذروة", "peak"),
    ("bench.col.retries", "إعادات", "retries"),
    ("bench.col.time", "الوقت", "time"),
    ("bench.running", "جولة بـ {n} اتصال…", "Run with {n} connection(s)…"),
    ("bench.summary", "الأفضل: {n} اتصال، أسرع بـ {x}× من اتصال واحد", "Best: {n} connections, {x}× faster than one"),
    ("cmd.batch", "تحميل مجموعة روابط من نمط مثل file[001-120].zip (مع --list لعرضها فقط)", "Download many files from a pattern like file[001-120].zip (use --list to only print them)"),
    ("cmd.bench", "قياس السرعة: اتصال واحد مقابل 8 مقابل 32", "Benchmark 1 vs 8 vs 32 connections"),
    ("cmd.fetch", "تحميل رابط بالمحرك المستقل دون الحاجة للتطبيق؛ تكرار الأمر يستأنف", "Download a URL with the standalone engine, no app needed; run it again to resume"),
    ("cmd.grab", "زحف موقع واستخراج ملفاته ثم تحميلها (أو عرضها مع --list)؛ يحترم robots.txt", "Crawl a site for its files and download them (or just list them with --list); obeys robots.txt"),
    ("cmd.paths", "عرض مجلد البيانات والسجلات ووضع التشغيل", "Show the data directory, logs directory and run mode"),
    ("cmd.ping", "التحقق من أن تطبيق ريبتايد يعمل", "Check whether the Riptide app is running"),
    ("fetch.changed", "تغيّر الملف على الخادم منذ آخر مرة؛ البدء من الصفر.", "The file on the server changed since last time; starting from zero."),
    ("fetch.conn", "اتصال", "conn"),
    ("fetch.done", "تم: {path} ({bytes} في {time}، {speed})", "Done: {path} ({bytes} in {time}, {speed})"),
    ("fetch.failed", "فشل التحميل", "Download failed"),
    ("fetch.insecure", "تحذير: التحقق من شهادة TLS معطّل لهذا التحميل.", "Warning: TLS certificate verification is off for this download."),
    ("fetch.left", "متبقٍ", "left"),
    ("fetch.paused", "توقّف. {bytes} على القرص؛ كرّر الأمر نفسه للاستئناف.", "Paused. {bytes} on disk; run the same command to resume."),
    ("fetch.probing", "فحص {url}…", "Probing {url}…"),
    ("fetch.restarted", "إعادة البدء من الصفر: {reason}", "Restarting from zero: {reason}"),
    ("fetch.resuming", "استئناف: {bytes} موجودة على القرص.", "Resuming: {bytes} already on disk."),
    ("fetch.retrying", "المقطع {n}: إعادة المحاولة {attempt} بعد {secs} ث ({error})", "Segment {n}: retry {attempt} in {secs} s ({error})"),
    ("fetch.verifying", "جارٍ التحقق من بصمة الملف…", "Verifying the checksum…"),
    ("grab.cap_files", "توقّف الزحف عند حد الملفات؛ ارفع --max-files للمزيد.", "Stopped at the file limit; raise --max-files for more."),
    ("grab.cap_pages", "توقّف الزحف عند حد الصفحات؛ ارفع --max-pages للمزيد.", "Stopped at the page limit; raise --max-pages for more."),
    ("grab.crawling", "زحف {url} (العمق {depth})…", "Crawling {url} (depth {depth})…"),
    ("grab.downloading", "تحميل {n} ملف إلى {dir}…", "Downloading {n} file(s) to {dir}…"),
    ("grab.existing", "تخطي {n} ملف موجود مسبقًا في {dir}.", "Skipping {n} file(s) that already exist in {dir}."),
    ("grab.file_done", "تم: {name}", "Done: {name}"),
    ("grab.file_failed", "فشل: {name} ({error})", "Failed: {name} ({error})"),
    ("grab.found", "وُجد {files} ملف في {pages} صفحة خلال {time}.", "Found {files} file(s) on {pages} page(s) in {time}."),
    ("grab.none", "لم يُعثر على ملفات.", "No files found."),
    ("grab.robots", "تخطّي {n} رابط بسبب robots.txt.", "Skipped {n} link(s) because of robots.txt."),
    ("grab.some_failed", "فشل تحميل بعض الملفات", "Some files failed to download"),
    ("grab.stop_cancelled", "أُلغي الزحف.", "The crawl was cancelled."),
    ("grab.stop_time", "انتهت مهلة الزحف؛ النتيجة جزئية. ارفع --time-limit للمزيد.", "The crawl ran out of time; the result is partial. Raise --time-limit for more."),
    ("grab.summary", "تم تحميل {ok} ملف، وفشل {failed}.", "{ok} downloaded, {failed} failed."),
    ("msg.not_running", "تطبيق ريبتايد لا يعمل ({endpoint})", "The Riptide app is not running ({endpoint})"),
    ("msg.paths.data", "مجلد البيانات", "Data directory"),
    ("msg.paths.db", "قاعدة البيانات", "Database"),
    ("msg.paths.endpoint", "قناة IPC", "IPC endpoint"),
    ("msg.paths.logs", "مجلد السجلات", "Logs directory"),
    ("msg.paths.mode", "الوضع", "Mode"),
    ("msg.paths.mode.installed", "مثبّت", "installed"),
    ("msg.paths.mode.portable", "محمول (portable.txt موجود)", "portable (portable.txt present)"),
    ("msg.pong", "التطبيق يعمل — الإصدار {version}، البروتوكول {protocol}", "App is running — version {version}, protocol {protocol}"),
    ("msg.unexpected", "رد غير متوقع من التطبيق", "Unexpected reply from the app"),
];

/// Looks up `key` in `lang`; a missing key logs a warning and yields an empty string.
pub fn t(lang: Lang, key: &str) -> &'static str {
    match STRINGS.iter().find(|(k, _, _)| *k == key) {
        Some((_, ar, en)) => match lang {
            Lang::Ar => ar,
            Lang::En => en,
        },
        None => {
            // A missing key is a bug caught by `tables_are_complete`; never panic at runtime.
            tracing::warn!(key, "missing CLI translation");
            ""
        }
    }
}

/// `t()` with `{name}` placeholders substituted.
pub fn tf(lang: Lang, key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = t(lang, key).to_string();
    for (name, value) in vars {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_complete_sorted_and_non_empty() {
        let keys: Vec<&str> = STRINGS.iter().map(|(k, _, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(keys, sorted, "keys must be unique and alphabetical");
        for (k, ar, en) in STRINGS {
            assert!(!ar.trim().is_empty(), "{k}: Arabic missing");
            assert!(!en.trim().is_empty(), "{k}: English missing");
            assert!(
                ar.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)),
                "{k}: Arabic text expected"
            );
            // placeholders must match between the two languages
            assert_eq!(
                placeholders(ar),
                placeholders(en),
                "{k}: placeholders differ"
            );
        }
    }

    /// `{name}` tokens in `s`, sorted.
    fn placeholders(s: &str) -> Vec<String> {
        let mut out: Vec<String> = s
            .match_indices('{')
            .filter_map(|(i, _)| s[i..].find('}').map(|j| s[i..=i + j].to_string()))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn lookup_and_formatting() {
        assert_eq!(t(Lang::En, "msg.paths.mode"), "Mode");
        assert_eq!(t(Lang::Ar, "msg.paths.mode"), "الوضع");
        assert_eq!(
            tf(
                Lang::En,
                "msg.pong",
                &[("version", "0.1.0"), ("protocol", "1")]
            ),
            "App is running — version 0.1.0, protocol 1"
        );
        assert_eq!(t(Lang::En, "no.such.key"), "");
    }

    #[test]
    fn detect_prefers_explicit_then_env() {
        assert_eq!(Lang::detect(Some("ar")), Lang::Ar);
        assert_eq!(Lang::detect(Some("en")), Lang::En);
        assert_eq!(Lang::detect(Some("AR-SA")), Lang::Ar);
        // "auto" with no Arabic in the environment of a CI machine falls back to English unless
        // the machine itself is Arabic; only assert the explicit cases to stay deterministic.
    }
}
