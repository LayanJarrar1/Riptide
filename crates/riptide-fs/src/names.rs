//! Names the user types for a new folder or a rename.

use crate::{Error, Result};

/// Device names Windows reserves in every folder, with or without an extension.
const RESERVED: [&str; 24] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "CONIN$",
    "CONOUT$",
];

/// Longest name in UTF-16 units that Windows file systems accept.
const MAX_NAME_UNITS: usize = 255;

/// Bidi controls that can make a name display differently from what it is (`gpj.exe` shown as
/// `exe.jpg`). LRM and RLM (U+200E, U+200F) are not listed: Arabic and mixed-direction names
/// legitimately use them.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `COM¹`, `LPT²` and so on: Windows treats the superscript digits as device numbers too.
fn is_superscript_device(stem: &str) -> bool {
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
        && matches!(chars.next(), Some('\u{00B9}' | '\u{00B2}' | '\u{00B3}'))
        && chars.next().is_none()
}

/// Checks the name of an item that already exists (a rename source, a transfer source). Windows may
/// hold names it would not accept today (`report.`, `aux`), and the user must be able to repair or
/// move them, so only an NTFS alternate data stream marker (`file.txt:stream`) is refused.
pub(crate) fn validate_existing_name(name: &str) -> Result<()> {
    if name.contains(':') {
        return Err(Error::BadName(
            "a name cannot contain : (that would address an alternate data stream)".to_owned(),
        ));
    }
    Ok(())
}

/// Checks a single file or folder name. The rules are Windows' (the strictest of the platforms
/// Riptide runs on), applied everywhere so a name never works on one machine and not another.
pub fn validate_name(name: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::BadName(why.to_owned()));
    if name.trim().is_empty() {
        return bad("a name cannot be empty");
    }
    if name == "." || name == ".." {
        return bad("\".\" and \"..\" are not names");
    }
    if let Some(c) = name.chars().find(|c| {
        matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control()
    }) {
        return Err(Error::BadName(if c.is_control() {
            "a name cannot contain control characters".to_owned()
        } else {
            format!("a name cannot contain {c}")
        }));
    }
    if name.chars().any(is_bidi_control) {
        return bad("a name cannot contain text-direction control characters");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return bad("a name cannot end with a dot or a space");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) || is_superscript_device(stem) {
        return Err(Error::BadName(format!("\"{stem}\" is reserved by Windows")));
    }
    if name.encode_utf16().count() > MAX_NAME_UNITS {
        return bad("the name is too long");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_and_unicode_names() {
        for ok in [
            "report.pdf",
            "Q3 numbers",
            ".gitignore",
            "مشروع التخرج",
            "日本語.txt",
            "a.b.c",
        ] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn refuses_what_windows_refuses_or_misreads() {
        for bad in [
            "",
            "   ",
            ".",
            "..",
            "a/b",
            "a\\b",
            "..\\x",
            "what?",
            "x*",
            "pipe|",
            "q\"",
            "c:",
            "trailing.",
            "trailing ",
            "CON",
            "con.txt",
            "Com1",
            "lpt9.log",
            "tab\tname",
            "conin$",
            "CONOUT$.txt",
            "com\u{00B9}",
            "LPT\u{00B2}.log",
            "COM\u{00B3}.txt",
            "evil\u{202E}gpj.exe",
            "a\u{2066}b",
            "a\u{202A}b",
        ] {
            assert!(
                matches!(validate_name(bad), Err(Error::BadName(_))),
                "{bad:?}"
            );
        }
        // LRM and RLM are legitimate in Arabic and mixed-direction names
        assert!(validate_name("مشروع\u{200F} (2024)\u{200E}.txt").is_ok());
        assert!(validate_name("a\u{200F}b").is_ok());
        assert!(validate_name(&"a".repeat(255)).is_ok());
        assert!(validate_name(&"a".repeat(256)).is_err());
        // CONSOLE is not CON
        assert!(validate_name("console.log").is_ok());
    }
}
