//! Names the user types for a new folder or a rename.

use crate::{Error, Result};

/// Device names Windows reserves in every folder, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Longest name in UTF-16 units that Windows file systems accept.
const MAX_NAME_UNITS: usize = 255;

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
    if name.ends_with('.') || name.ends_with(' ') {
        return bad("a name cannot end with a dot or a space");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
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
        ] {
            assert!(
                matches!(validate_name(bad), Err(Error::BadName(_))),
                "{bad:?}"
            );
        }
        assert!(validate_name(&"a".repeat(255)).is_ok());
        assert!(validate_name(&"a".repeat(256)).is_err());
        // CONSOLE is not CON
        assert!(validate_name("console.log").is_ok());
    }
}
