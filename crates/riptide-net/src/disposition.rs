//! `Content-Disposition` parsing (RFC 6266 with RFC 5987/8187 `filename*`).
//!
//! Returns the *raw* name the server suggested; callers must run it through
//! `riptide_core::sanitize_filename` before touching the file system.

use percent_encoding::percent_decode_str;

/// Extracts the suggested file name from a `Content-Disposition` header value.
///
/// Preference order: `filename*` (UTF-8 or ISO-8859-1, percent-encoded, with an optional
/// language tag) → `filename="quoted"` (with `\"` escapes) → `filename=token`. Returns `None`
/// when the header carries no name or the name decodes to nothing.
pub fn filename_from_disposition(header: &str) -> Option<String> {
    let mut plain: Option<String> = None;
    let mut extended: Option<String> = None;

    for param in split_params(header) {
        let Some((name, value)) = param.split_once('=') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        match name.as_str() {
            "filename*" if extended.is_none() => extended = decode_ext_value(value),
            "filename" if plain.is_none() => plain = Some(unquote(value)),
            _ => {}
        }
    }

    extended
        .or(plain)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Splits on `;` outside quoted strings.
fn split_params(header: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    for (i, c) in header.char_indices() {
        match c {
            '\\' if in_quotes => escaped = !escaped,
            '"' if !escaped => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                parts.push(&header[start..i]);
                start = i + 1;
            }
            _ => escaped = false,
        }
        if c != '\\' {
            escaped = false;
        }
    }
    parts.push(&header[start..]);
    parts
}

/// `"a \"quoted\" name"` → `a "quoted" name`; a bare token is returned as-is.
fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        v.to_string()
    }
}

/// RFC 8187 `ext-value`: `charset'language'percent-encoded`.
fn decode_ext_value(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"');
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _language = parts.next()?;
    let encoded = parts.next()?;
    let bytes = percent_decode_str(encoded).collect::<Vec<u8>>();
    let decoded = match charset.as_str() {
        "utf-8" | "utf8" => String::from_utf8(bytes).ok()?,
        "iso-8859-1" | "latin1" | "us-ascii" => bytes.iter().map(|&b| b as char).collect(),
        _ => String::from_utf8(bytes).ok()?,
    };
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc5987_arabic_wins_over_plain() {
        let h = "attachment; filename=\"fallback.zip\"; filename*=UTF-8''%D9%85%D9%84%D9%81%20%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A.zip";
        assert_eq!(
            filename_from_disposition(h).as_deref(),
            Some("ملف تجريبي.zip")
        );
        let only_ext = "attachment; filename*=UTF-8''%D9%85%D9%84%D9%81%20%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A.zip";
        assert_eq!(
            filename_from_disposition(only_ext).as_deref(),
            Some("ملف تجريبي.zip")
        );
    }

    #[test]
    fn quoted_bare_and_escaped_names() {
        assert_eq!(
            filename_from_disposition("attachment; filename=\"report 2025.pdf\"").as_deref(),
            Some("report 2025.pdf")
        );
        assert_eq!(
            filename_from_disposition("inline; filename=plain.txt").as_deref(),
            Some("plain.txt")
        );
        assert_eq!(
            filename_from_disposition("attachment; filename=\"a \\\"b\\\" c.txt\"").as_deref(),
            Some("a \"b\" c.txt")
        );
        assert_eq!(
            filename_from_disposition("attachment; filename=\"semi;colon.txt\"").as_deref(),
            Some("semi;colon.txt")
        );
        assert_eq!(
            filename_from_disposition("ATTACHMENT; FILENAME=\"Upper.ZIP\"").as_deref(),
            Some("Upper.ZIP")
        );
    }

    #[test]
    fn evil_name_is_returned_raw_for_the_sanitizer() {
        let h = "attachment; filename=\"../../CON%01.txt. \"; filename*=UTF-8''..%2F..%2FCON%01.txt.%20";
        let raw = filename_from_disposition(h).unwrap();
        assert_eq!(raw, "../../CON\u{1}.txt.");
        assert_eq!(riptide_core::sanitize_filename(&raw), "_CON.txt");
    }

    #[test]
    fn latin1_and_language_tags() {
        assert_eq!(
            filename_from_disposition("attachment; filename*=iso-8859-1'en'caf%E9.txt").as_deref(),
            Some("café.txt")
        );
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8'ar'%D8%A3.txt").as_deref(),
            Some("أ.txt")
        );
    }

    #[test]
    fn missing_or_empty_names() {
        assert_eq!(filename_from_disposition("attachment"), None);
        assert_eq!(filename_from_disposition("attachment; filename=\"\""), None);
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8''"),
            None
        );
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8''%FF%FE; filename=ok.bin")
                .as_deref(),
            Some("ok.bin"),
            "bad UTF-8 falls back"
        );
    }
}
