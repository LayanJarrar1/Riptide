//! File name sanitising: nothing a server sends is ever used as a path.
//!
//! [`sanitize_filename`] turns any string into a name that is safe on Windows, macOS and Linux:
//! last path component only, no control characters or reserved punctuation, no reserved device
//! names, no leading/trailing dots or spaces, NFC-normalised, at most [`MAX_FILENAME_BYTES`].
//! It is idempotent and never returns an empty string.

use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

/// Upper bound on the UTF-8 length of a sanitised name.
pub const MAX_FILENAME_BYTES: usize = 200;

/// Name used when nothing usable is left.
pub const FALLBACK_FILENAME: &str = "download";

/// Characters Windows refuses in file names (plus `/`, which every OS treats as a separator).
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Device names Windows reserves regardless of extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Makes `raw` safe to use as a file name inside a chosen folder.
///
/// ```
/// use riptide_core::filename::sanitize_filename;
/// assert_eq!(sanitize_filename("../../CON\u{1}.txt. "), "_CON.txt");
/// assert_eq!(sanitize_filename("ملف تجريبي.zip"), "ملف تجريبي.zip");
/// assert_eq!(sanitize_filename("   "), "download");
/// ```
pub fn sanitize_filename(raw: &str) -> String {
    // 1. NFC so visually identical names compare equal and composed Arabic/Latin forms are used
    let normalized: String = raw.nfc().collect();

    // 2. last path component: a server-supplied "../../x" or "C:\x" can only ever mean "x"
    let last = normalized
        .rsplit(['/', '\\'])
        .find(|part| !part.trim().is_empty())
        .unwrap_or("");

    // 3. drop control characters and reserved punctuation
    let mut cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !FORBIDDEN.contains(c))
        .collect();

    // 4. no leading/trailing dots or spaces (Windows strips them silently, which hides ".." tricks)
    cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();

    if cleaned.is_empty() {
        return FALLBACK_FILENAME.to_string();
    }

    // 5. reserved device names: "CON", "con.txt", "LPT1.tar.gz" are all the device on Windows
    let stem_upper = cleaned
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        cleaned.insert(0, '_');
    }

    // 6. length cap on a char boundary, keeping a short extension intact
    let capped = truncate_keeping_extension(&cleaned, MAX_FILENAME_BYTES);

    // 7. NFC once more: removing a character can put two combining marks next to each other,
    //    and canonical ordering may then swap them. Normalising the final string keeps the
    //    function idempotent (and composition never makes a string longer in bytes).
    capped.nfc().collect()
}

/// Splits `name` into stem and extension (without the dot). A leading dot (`.bashrc`) or no dot
/// means no extension.
pub fn split_extension(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], Some(&name[i + 1..])),
        _ => (name, None),
    }
}

/// Shortens `name` to at most `max` bytes, cutting the stem on a char boundary and keeping an
/// extension of up to 16 bytes.
fn truncate_keeping_extension(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = split_extension(name);
    let ext = ext.filter(|e| e.len() <= 16);
    let budget = ext.map_or(max, |e| max.saturating_sub(e.len() + 1));
    let mut cut = budget.min(stem.len());
    while cut > 0 && !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    let stem = stem[..cut].trim_end_matches(['.', ' ']);
    let out = match ext {
        Some(e) if !stem.is_empty() => format!("{stem}.{e}"),
        Some(e) => format!("{FALLBACK_FILENAME}.{e}"),
        None if stem.is_empty() => FALLBACK_FILENAME.to_string(),
        None => stem.to_string(),
    };
    // the fallback plus a long extension could still exceed `max`; a final hard cut is safe
    // because the fallback is ASCII and the extension was capped at 16 bytes
    if out.len() > max {
        let mut cut = max;
        while cut > 0 && !out.is_char_boundary(cut) {
            cut -= 1;
        }
        return out[..cut].to_string();
    }
    out
}

/// First of `name`, `name (1).ext`, `name (2).ext`, … for which `exists` is false.
pub fn unique_path(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = split_extension(name);
    for n in 1..10_000 {
        let candidate = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let path = dir.join(candidate);
        if !exists(&path) {
            return path;
        }
    }
    dir.join(format!("{stem}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn evil_names_become_harmless() {
        assert_eq!(sanitize_filename("../../CON\u{1}.txt. "), "_CON.txt");
        assert_eq!(
            sanitize_filename("..\\..\\windows\\system32\\drivers\\etc\\hosts"),
            "hosts"
        );
        assert_eq!(sanitize_filename("C:\\Users\\me\\x.exe"), "x.exe");
        assert_eq!(sanitize_filename("/etc/passwd"), "passwd");
        // separators split the name: only the last component survives, then punctuation goes
        assert_eq!(sanitize_filename("a<b>c:d\"e/f\\g|h?i*j.txt"), "ghij.txt");
        assert_eq!(sanitize_filename("a<b>c:d\"e|f?g*h.txt"), "abcdefgh.txt");
        assert_eq!(sanitize_filename("nul"), "_nul");
        assert_eq!(sanitize_filename("LPT1.tar.gz"), "_LPT1.tar.gz");
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(
            sanitize_filename("console.txt"),
            "console.txt",
            "only exact device names"
        );
        assert_eq!(sanitize_filename("..."), "download");
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("\u{0}\u{1f}\u{7f}"), "download");
        assert_eq!(sanitize_filename("  spaced.pdf  "), "spaced.pdf");
        assert_eq!(sanitize_filename("trailing.dots..."), "trailing.dots");
    }

    #[test]
    fn arabic_and_unicode_survive_and_are_nfc() {
        assert_eq!(sanitize_filename("ملف تجريبي.zip"), "ملف تجريبي.zip");
        assert_eq!(
            sanitize_filename("التقرير-السنوي-2025.pdf"),
            "التقرير-السنوي-2025.pdf"
        );
        // decomposed e + combining acute becomes the composed form
        assert_eq!(sanitize_filename("cafe\u{301}.txt"), "café.txt");
    }

    #[test]
    fn long_names_keep_their_extension() {
        let long = format!("{}.tar.gz", "ع".repeat(300));
        let out = sanitize_filename(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(out.ends_with(".gz"));
        let ascii = format!("{}.mkv", "a".repeat(500));
        let out = sanitize_filename(&ascii);
        assert_eq!(out.len(), MAX_FILENAME_BYTES);
        assert!(out.ends_with(".mkv"));
        let huge_ext = format!("x.{}", "e".repeat(300));
        assert!(sanitize_filename(&huge_ext).len() <= MAX_FILENAME_BYTES);
    }

    #[test]
    fn split_and_unique() {
        assert_eq!(split_extension("a.tar.gz"), ("a.tar", Some("gz")));
        assert_eq!(split_extension(".bashrc"), (".bashrc", None));
        assert_eq!(split_extension("noext"), ("noext", None));
        assert_eq!(split_extension("dot."), ("dot.", None));

        let taken = ["x.zip", "x (1).zip", "x (2).zip"].map(|n| Path::new("/d").join(n));
        let path = unique_path(Path::new("/d"), "x.zip", |p| {
            taken.contains(&p.to_path_buf())
        });
        assert_eq!(path, Path::new("/d").join("x (3).zip"));
        let free = unique_path(Path::new("/d"), "y", |_| false);
        assert_eq!(free, Path::new("/d").join("y"));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1500))]

        #[test]
        fn output_is_always_safe(raw in "\\PC{0,260}") {
            let out = sanitize_filename(&raw);
            prop_assert!(!out.is_empty());
            prop_assert!(out.len() <= MAX_FILENAME_BYTES, "{} bytes", out.len());
            prop_assert!(!out.chars().any(|c| c.is_control() || FORBIDDEN.contains(&c)), "{out:?}");
            prop_assert!(!out.starts_with(['.', ' ']) && !out.ends_with(['.', ' ']), "{out:?}");
            let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
            prop_assert!(!RESERVED.contains(&stem.as_str()), "{out:?}");
            prop_assert!(!out.contains('/') && !out.contains('\\'));
            // a path built from it never escapes the directory
            let joined = Path::new("/base").join(&out);
            prop_assert_eq!(joined.parent(), Some(Path::new("/base")));
        }

        #[test]
        fn is_idempotent(raw in "\\PC{0,120}") {
            let once = sanitize_filename(&raw);
            prop_assert_eq!(sanitize_filename(&once), once.clone());
        }

        #[test]
        fn plain_names_pass_through(stem in "[a-zA-Z0-9_-]{1,40}", ext in "[a-z0-9]{1,5}") {
            let name = format!("{stem}.{ext}");
            let upper = stem.to_ascii_uppercase();
            prop_assume!(!RESERVED.contains(&upper.as_str()));
            prop_assert_eq!(sanitize_filename(&name), name.clone());
        }
    }
}
