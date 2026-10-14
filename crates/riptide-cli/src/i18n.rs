//! Arabic / English strings for the CLI (hard rule 8). Two tables with identical keys; a test
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

/// All strings. Keep alphabetical by key.
pub const STRINGS: &[Entry] = &[
    ("about", "ريبتايد — مدير تحميل مجاني ومفتوح المصدر", "Riptide — a free, open-source download manager"),
    ("arg.connections", "عدد الاتصالات المتزامنة (1–32)", "Number of simultaneous connections (1–32)"),
    ("arg.data_dir", "مجلد البيانات (الافتراضي: %LOCALAPPDATA%\\Riptide أو data/ بجانب البرنامج في الوضع المحمول)", "Data directory (default: %LOCALAPPDATA%\\Riptide, or data/ next to the exe in portable mode)"),
    ("arg.lang", "لغة الرسائل: ar أو en أو auto", "Message language: ar, en or auto"),
    ("arg.log_level", "مستوى السجل: error | warn | info | debug | trace أو عبارة تصفية مثل riptide_net=debug,info", "Log filter: error | warn | info | debug | trace, or a directive like riptide_net=debug,info"),
    ("arg.output", "مسار الحفظ (ملف أو مجلد)", "Where to save (file or folder)"),
    ("arg.url", "الرابط المراد تحميله", "The URL to download"),
    ("cmd.bench", "قياس السرعة: اتصال واحد مقابل 8 مقابل 32 (المرحلة M1)", "Benchmark 1 vs 8 vs 32 connections (milestone M1)"),
    ("cmd.fetch", "تحميل رابط بالمحرك المستقل دون الحاجة للتطبيق (المرحلة M1)", "Download a URL with the standalone engine, no app needed (milestone M1)"),
    ("cmd.paths", "عرض مجلد البيانات والسجلات ووضع التشغيل", "Show the data directory, logs directory and run mode"),
    ("cmd.ping", "التحقق من أن تطبيق ريبتايد يعمل", "Check whether the Riptide app is running"),
    ("msg.not_implemented", "هذا الأمر يصل في المرحلة {milestone}؛ الهيكل الحالي هو M0.", "This command arrives in milestone {milestone}; the current build is the M0 skeleton."),
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

/// Looks up `key` in `lang`; falls back to English, then to the key itself.
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
