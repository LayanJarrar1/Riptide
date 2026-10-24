//! Query understanding, deterministic and offline: the user's words are normalised (Unicode
//! NFKC, Arabic letter folding, Arabic-Indic digits) and split into a [`QueryIntent`]: exact
//! phrases, exclusions, episode / season, year, version, file type, language hint, and the
//! remaining words (the series or title).
//!
//! The same extractors run over a result's title and file name ([`Facts::of`]) so a result can
//! be checked against what was asked (see [`crate::relevance`]).
//!
//! Folding is for *comparison only*: nothing here changes what the user sees or what is sent to
//! a source (sources receive the user's own text; see [`crate::queries`]).

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

/// File extensions (and aliases) recognised as a file-type constraint.
const FILE_TYPES: &[&str] = &[
    "pdf", "epub", "mobi", "azw3", "djvu", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt",
    "rtf", "odt", "csv", "mp4", "mkv", "avi", "mov", "webm", "mp3", "flac", "wav", "ogg", "m4a",
    "zip", "rar", "7z", "tar", "gz", "iso", "exe", "msi", "dmg", "deb", "rpm", "apk", "appimage",
    "jpg", "jpeg", "png", "gif", "svg", "webp", "tiff", "html",
];

/// Words that carry no meaning for matching (English and Arabic).
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "and", "or", "in", "on", "to", "for", "with", "by", "at", "is", "are",
    "from", "as", "its", "it", "on", "و", "في", "من", "على", "الى", "عن", "مع", "هو", "هي",
];

/// Words people add to a search that no result title needs to contain.
const NOISE: &[&str] = &[
    "download",
    "free",
    "watch",
    "full",
    "online",
    "hd",
    "fhd",
    "uhd",
    "1080p",
    "720p",
    "480p",
    "4k",
    "latest",
    "new",
    "official",
    "file",
    "pdf",
    "episode",
    "season",
    "version",
    "تحميل",
    "مشاهدة",
    "مشاهده",
    "تنزيل",
    "مجانا",
    "كامل",
    "كاملة",
    "كامله",
    "اون",
    "لاين",
    "مباشر",
    // what kind of thing it is: titles rarely repeat it
    "series",
    "movie",
    "film",
    "anime",
    "مسلسل",
    "فيلم",
    "انمي",
    "كتاب",
];

/// Everything understood about a query.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryIntent {
    /// The query as typed.
    pub raw: String,
    /// The query after [`normalize`].
    pub normalized: String,
    /// Exact phrases (in quotes), normalised.
    pub phrases: Vec<String>,
    /// Words (`-word`) a result must not contain, normalised.
    pub exclusions: Vec<String>,
    /// The words left once constraints are removed: the series or title. Normalised, without
    /// stop words and filler such as "download" or "watch".
    pub title_terms: Vec<String>,
    /// Episode number.
    pub episode: Option<u32>,
    /// Season number.
    pub season: Option<u32>,
    /// Year the user asked for (only when written like a year constraint, not as part of a
    /// title such as "1984 orwell").
    pub year: Option<u16>,
    /// Version (`1.2.3`), without a leading `v`.
    pub version: Option<String>,
    /// File type (`pdf`, `mp4`...), lower case.
    pub file_type: Option<String>,
    /// Language the user asked for (`ar`, `en`, `fr`, `de`, `es`, `tr`...), when they said so.
    pub language: Option<String>,
    /// Main script of the query: `arabic`, `latin`, `mixed` or `other`.
    pub script: String,
}

impl QueryIntent {
    /// Understands `query`.
    pub fn parse(query: &str) -> QueryIntent {
        parse(query)
    }

    /// Does the query constrain anything beyond words?
    pub fn has_constraints(&self) -> bool {
        self.episode.is_some()
            || self.season.is_some()
            || self.year.is_some()
            || self.version.is_some()
            || self.file_type.is_some()
            || !self.phrases.is_empty()
            || !self.exclusions.is_empty()
    }
}

// ---------------------------------------------------------------- normalisation

fn fold_char(c: char) -> Option<char> {
    match c {
        // diacritics (tashkeel), superscript alef, tatweel
        '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}' => None,
        // alef variants
        '\u{0623}' | '\u{0625}' | '\u{0622}' | '\u{0671}' => Some('\u{0627}'),
        // taa marbuta -> haa
        '\u{0629}' => Some('\u{0647}'),
        // alef maqsura -> yaa
        '\u{0649}' => Some('\u{064A}'),
        // Arabic-Indic and Persian digits
        '\u{0660}'..='\u{0669}' => char::from_digit(u32::from(c) - 0x0660, 10),
        '\u{06F0}'..='\u{06F9}' => char::from_digit(u32::from(c) - 0x06F0, 10),
        // curly and guillemet quotes
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{00AB}' | '\u{00BB}' | '\u{201F}' => Some('"'),
        '\u{2018}' | '\u{2019}' => Some('\''),
        // Arabic comma, question mark, dashes
        '\u{060C}' => Some(','),
        '\u{061F}' => Some('?'),
        '\u{2010}'..='\u{2015}' => Some('-'),
        other => Some(other),
    }
}

/// NFKC, lower case, Arabic folding (alef variants to bare alef, taa marbuta to haa, alef
/// maqsura to yaa, diacritics and tatweel removed), Arabic-Indic digits to ASCII, curly quotes
/// to ASCII, whitespace collapsed.
pub fn normalize(text: &str) -> String {
    let folded: String = text
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter_map(fold_char)
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Splits normalised text into words (letters and digits; marks stay with their letter).
pub fn tokens(normalized: &str) -> Vec<String> {
    normalized
        .split(|c: char| !(c.is_alphanumeric() || is_mark(c)))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_mark(c: char) -> bool {
    // combining marks that survived NFKC (Hindi, Thai...) belong to their word
    matches!(c as u32, 0x0300..=0x036F | 0x0900..=0x097F)
}

/// Is `word` something a title does not need to contain?
pub(crate) fn is_filler(word: &str) -> bool {
    STOPWORDS.contains(&word) || NOISE.contains(&word)
}

// ---------------------------------------------------------------- patterns

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static regex"))
}

fn re_sxe() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\bs(\d{1,2})\s*[ ._-]?e(?:p)?(\d{1,4})\b")
}

fn re_episode() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // "episode 22", "ep 22", "ep.22", "e22", "episodes 21-24", Arabic "الحلقه 22", "حلقه 22",
    // "ح22"
    re(
        &R,
        r"\b(?:(?:episodes?|eps?|الحلقه|حلقه)\s*[.#:]?\s*|e|ح)(\d{1,4})(?:\s*[-~]\s*(\d{1,4}))?\b",
    )
}

fn re_season() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(
        &R,
        r"\b(?:seasons?|الموسم|موسم|الجزء|جزء)\s*[.#:]?\s*(\d{1,2})(?:\s*[-~]\s*(\d{1,2}))?\b",
    )
}

fn re_season_word() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // "season two", Arabic "الموسم الثاني" / "الجزء الثالث" (ordinals, folded)
    re(
        &R,
        r"\b(?:seasons?|الموسم|موسم|الجزء|جزء)\s+(one|two|three|four|five|six|seven|eight|nine|ten|الاول|الاولى|الثاني|الثانيه|الثالث|الثالثه|الرابع|الرابعه|الخامس|الخامسه|السادس|السادسه|السابع|السابعه|الثامن|الثامنه|التاسع|التاسعه|العاشر|العاشره)\b",
    )
}

/// The number a season word stands for.
fn word_number(w: &str) -> Option<u32> {
    Some(match w {
        "one" | "الاول" | "الاولى" => 1,
        "two" | "الثاني" | "الثانيه" => 2,
        "three" | "الثالث" | "الثالثه" => 3,
        "four" | "الرابع" | "الرابعه" => 4,
        "five" | "الخامس" | "الخامسه" => 5,
        "six" | "السادس" | "السادسه" => 6,
        "seven" | "السابع" | "السابعه" => 7,
        "eight" | "الثامن" | "الثامنه" => 8,
        "nine" | "التاسع" | "التاسعه" => 9,
        "ten" | "العاشر" | "العاشره" => 10,
        _ => return None,
    })
}

fn re_short_season() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\bs(\d{1,2})\b")
}

fn re_version() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // "v1.2.3", "version 2", "ver. 3.1", or a bare dotted number with two or more parts
    re(
        &R,
        r"\b(?:(?:v|ver|version|الاصدار|اصدار|نسخه)\s*\.?\s*(\d+(?:\.\d+){0,3})|(\d+(?:\.\d+){1,3}))\b",
    )
}

fn re_year() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\b((?:19|20)\d\d)\b")
}

fn re_quotes() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r#""([^"]+)""#)
}

fn re_filetype_kw() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(
        &R,
        r"(?:^|\s)(?:filetype|ext|format|type):\s*\.?([a-z0-9]{2,8})(?:\s|$)",
    )
}

fn re_dot_ext() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?:^|\s)\.([a-z0-9]{2,8})(?:\s|$)")
}

/// The canonical spelling of a file type (`jpeg` is `jpg`).
pub(crate) fn canonical_type(ext: &str) -> String {
    match ext {
        "jpeg" => "jpg",
        "htm" => "html",
        "tif" => "tiff",
        "tgz" => "gz",
        other => other,
    }
    .to_string()
}

/// Do these two file types count as the same family (`doc`/`docx`, `xls`/`xlsx`...)?
pub(crate) fn same_type_family(a: &str, b: &str) -> bool {
    let (a, b) = (canonical_type(a), canonical_type(b));
    if a == b {
        return true;
    }
    let family = |t: &str| match t {
        "doc" | "docx" | "odt" | "rtf" => 1,
        "xls" | "xlsx" | "csv" => 2,
        "ppt" | "pptx" => 3,
        "epub" | "mobi" | "azw3" => 4,
        "tar" | "gz" => 5,
        _ => 0,
    };
    family(&a) != 0 && family(&a) == family(&b)
}

pub(crate) fn is_file_type(word: &str) -> bool {
    FILE_TYPES.contains(&canonical_type(word).as_str()) || FILE_TYPES.contains(&word)
}

/// Replaces `range` of `text` with a space (keeps positions of the rest meaningful enough for
/// the next extractor).
fn blank(text: &mut String, range: std::ops::Range<usize>) {
    text.replace_range(range, " ");
}

fn parse_u32(s: &str) -> Option<u32> {
    s.parse::<u32>().ok().filter(|n| *n > 0 && *n < 10_000)
}

fn language_hint(words: &[String]) -> Option<String> {
    const HINTS: &[(&str, &str)] = &[
        ("arabic", "ar"),
        ("عربي", "ar"),
        ("بالعربي", "ar"),
        ("بالعربيه", "ar"),
        ("العربيه", "ar"),
        ("english", "en"),
        ("انجليزي", "en"),
        ("بالانجليزي", "en"),
        ("بالانجليزيه", "en"),
        ("french", "fr"),
        ("فرنسي", "fr"),
        ("german", "de"),
        ("الماني", "de"),
        ("spanish", "es"),
        ("اسباني", "es"),
        ("turkish", "tr"),
        ("تركي", "tr"),
    ];
    words.iter().find_map(|w| {
        HINTS
            .iter()
            .find(|(k, _)| k == w)
            .map(|(_, code)| (*code).to_string())
    })
}

fn script_of(text: &str) -> &'static str {
    let (mut ar, mut lat, mut other) = (0usize, 0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        match c as u32 {
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => ar += 1,
            0x0041..=0x024F => lat += 1,
            _ => other += 1,
        }
    }
    match (ar > 0, lat > 0, other > 0) {
        (true, false, false) => "arabic",
        (false, true, false) => "latin",
        (false, false, false) => "latin",
        (true, true, _) | (true, _, true) | (_, true, true) => "mixed",
        _ => "other",
    }
}

fn parse(query: &str) -> QueryIntent {
    let normalized = normalize(query);
    let mut rest = normalized.clone();
    let mut intent = QueryIntent {
        raw: query.trim().to_string(),
        normalized: normalized.clone(),
        script: script_of(&normalized).to_string(),
        ..QueryIntent::default()
    };

    // exact phrases
    while let Some(m) = re_quotes().captures(&rest) {
        let whole = m.get(0).map(|w| w.range());
        let phrase = m.get(1).map(|p| p.as_str().trim().to_string());
        if let Some(p) = phrase.filter(|p| !p.is_empty()) {
            intent.phrases.push(p);
        }
        match whole {
            Some(r) => blank(&mut rest, r),
            None => break,
        }
    }
    // an unmatched quote is just punctuation
    rest = rest.replace('"', " ");

    // exclusions: `-word` at the start of a word
    let mut kept = Vec::new();
    for word in rest.split_whitespace() {
        match word.strip_prefix('-') {
            Some(w) if w.chars().count() >= 2 && w.chars().any(char::is_alphabetic) => {
                intent.exclusions.push(w.trim_matches('-').to_string());
            }
            _ => kept.push(word),
        }
    }
    rest = kept.join(" ");

    // season + episode together (S02E05)
    if let Some(m) = re_sxe().captures(&rest) {
        intent.season = m.get(1).and_then(|s| parse_u32(s.as_str()));
        intent.episode = m.get(2).and_then(|s| parse_u32(s.as_str()));
        if let Some(r) = m.get(0).map(|w| w.range()) {
            blank(&mut rest, r);
        }
    }
    // episode
    if intent.episode.is_none() {
        if let Some(m) = re_episode().captures(&rest) {
            // a range ("episodes 1-10") names no single episode
            if m.get(2).is_none() {
                intent.episode = m.get(1).and_then(|s| parse_u32(s.as_str()));
            }
            if let Some(r) = m.get(0).map(|w| w.range()) {
                blank(&mut rest, r);
            }
        }
    }
    // season
    if intent.season.is_none() {
        if let Some(m) = re_season().captures(&rest) {
            if m.get(2).is_none() {
                intent.season = m.get(1).and_then(|s| parse_u32(s.as_str()));
            }
            if let Some(r) = m.get(0).map(|w| w.range()) {
                blank(&mut rest, r);
            }
        } else if let Some(m) = re_season_word().captures(&rest) {
            intent.season = m.get(1).and_then(|w| word_number(w.as_str()));
            if let Some(r) = m.get(0).map(|w| w.range()) {
                blank(&mut rest, r);
            }
        } else if let Some(m) = re_short_season().captures(&rest) {
            let digits = m.get(1).map_or("", |d| d.as_str());
            // `s3` alone is too often something else (S3 storage); `s03` or next to an episode
            if digits.len() == 2 || intent.episode.is_some() {
                intent.season = parse_u32(digits);
                if let Some(r) = m.get(0).map(|w| w.range()) {
                    blank(&mut rest, r);
                }
            }
        }
    }

    // file type: explicit forms anywhere, a bare extension word only as the last word (or just
    // before a quality tag), so "pdf reader" stays a title
    if let Some(m) = re_filetype_kw().captures(&rest) {
        let t = m.get(1).map_or("", |t| t.as_str()).to_string();
        if is_file_type(&t) {
            intent.file_type = Some(canonical_type(&t));
            if let Some(r) = m.get(0).map(|w| w.range()) {
                blank(&mut rest, r);
            }
        }
    }
    if intent.file_type.is_none() {
        if let Some(m) = re_dot_ext().captures(&rest) {
            let t = m.get(1).map_or("", |t| t.as_str()).to_string();
            if is_file_type(&t) {
                intent.file_type = Some(canonical_type(&t));
                if let Some(r) = m.get(0).map(|w| w.range()) {
                    blank(&mut rest, r);
                }
            }
        }
    }
    if intent.file_type.is_none() {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let quality = |w: &str| matches!(w, "1080p" | "720p" | "480p" | "2160p" | "4k" | "hd");
        let idx = match words.last() {
            Some(l) if quality(l) && words.len() >= 2 => Some(words.len() - 2),
            Some(_) if words.len() >= 2 => Some(words.len() - 1),
            _ => None,
        };
        if let Some(i) = idx {
            if is_file_type(words[i]) {
                intent.file_type = Some(canonical_type(words[i]));
                let mut w2 = words.clone();
                w2.remove(i);
                rest = w2.join(" ");
            }
        }
    }

    // version
    if let Some(m) = re_version().captures(&rest) {
        let v = m
            .get(1)
            .or_else(|| m.get(2))
            .map(|v| v.as_str().to_string());
        if let (Some(v), Some(r)) = (v, m.get(0).map(|w| w.range())) {
            // `2021.5` style dotted numbers are versions too; a bare integer is not
            intent.version = Some(v);
            blank(&mut rest, r);
        }
    }

    // year, only when it reads as a constraint: not the first word, and something else is left
    let year_hit = re_year().captures(&rest).and_then(|m| {
        let y = m.get(1)?;
        let whole = m.get(0)?.range();
        let first = rest[..y.start()].trim().is_empty();
        let parenthesised = rest[..y.start()].trim_end().ends_with('(');
        let others = rest
            .split_whitespace()
            .filter(|w| !w.contains(y.as_str()))
            .count();
        let value = y.as_str().parse::<u16>().ok().filter(|v| *v <= 2035)?;
        ((!first || parenthesised) && others >= 1).then_some((value, whole))
    });
    if let Some((value, whole)) = year_hit {
        intent.year = Some(value);
        blank(&mut rest, whole);
    }

    // what remains: language hint, then the title words
    let words = tokens(&rest);
    intent.language = language_hint(&words);
    intent.title_terms = words
        .into_iter()
        .filter(|w| !is_filler(w))
        .filter(|w| !intent.exclusions.contains(w))
        .collect();
    intent
}

// ---------------------------------------------------------------- facts stated by a result

/// What a result's own text says about itself, found with the same extractors.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Episodes stated (a range `21-24` lists every number in it, up to 60 of them).
    pub episodes: Vec<u32>,
    /// Seasons stated.
    pub seasons: Vec<u32>,
    /// Years stated (1900-2035).
    pub years: Vec<u16>,
    /// Versions stated (`v` removed).
    pub versions: Vec<String>,
}

fn push_range(out: &mut Vec<u32>, a: Option<u32>, b: Option<u32>) {
    match (a, b) {
        (Some(a), Some(b)) if b >= a && b - a <= 60 => out.extend(a..=b),
        (Some(a), Some(b)) if a > b => out.extend([a, b]),
        (Some(a), _) => out.push(a),
        _ => {}
    }
}

impl Facts {
    /// The facts stated in `text` (title, file name, URL tail...). Every occurrence counts.
    pub fn of(text: &str) -> Facts {
        let n = normalize(text);
        let mut facts = Facts::default();
        for m in re_sxe().captures_iter(&n) {
            push_range(
                &mut facts.seasons,
                m.get(1).and_then(|s| parse_u32(s.as_str())),
                None,
            );
            push_range(
                &mut facts.episodes,
                m.get(2).and_then(|s| parse_u32(s.as_str())),
                None,
            );
        }
        for m in re_episode().captures_iter(&n) {
            push_range(
                &mut facts.episodes,
                m.get(1).and_then(|s| parse_u32(s.as_str())),
                m.get(2).and_then(|s| parse_u32(s.as_str())),
            );
        }
        for m in re_season().captures_iter(&n) {
            push_range(
                &mut facts.seasons,
                m.get(1).and_then(|s| parse_u32(s.as_str())),
                m.get(2).and_then(|s| parse_u32(s.as_str())),
            );
        }
        for m in re_season_word().captures_iter(&n) {
            push_range(
                &mut facts.seasons,
                m.get(1).and_then(|w| word_number(w.as_str())),
                None,
            );
        }
        for m in re_short_season().captures_iter(&n) {
            let d = m.get(1).map_or("", |d| d.as_str());
            if d.len() == 2 {
                push_range(&mut facts.seasons, parse_u32(d), None);
            }
        }
        let without_versions = re_version().replace_all(&n, " ");
        for m in re_year().captures_iter(&without_versions) {
            if let Some(y) = m
                .get(1)
                .and_then(|y| y.as_str().parse::<u16>().ok())
                .filter(|y| *y <= 2035)
            {
                facts.years.push(y);
            }
        }
        for m in re_version().captures_iter(&n) {
            if let Some(v) = m.get(1).or_else(|| m.get(2)) {
                facts.versions.push(v.as_str().to_string());
            }
        }
        facts.episodes.sort_unstable();
        facts.episodes.dedup();
        facts.seasons.sort_unstable();
        facts.seasons.dedup();
        facts.years.sort_unstable();
        facts.years.dedup();
        facts.versions.dedup();
        facts
    }
}

/// The file extension of a file name or URL path (`report.final.PDF` is `pdf`), canonical.
pub fn extension_of(name: &str) -> Option<String> {
    let path;
    let name = match url::Url::parse(name) {
        Ok(u) if u.has_host() => {
            path = u.path().to_string();
            path.as_str()
        }
        _ => name.split(['?', '#']).next().unwrap_or(name),
    };
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let ext = last.rsplit_once('.')?.1.to_lowercase();
    is_file_type(&ext).then(|| canonical_type(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(q: &str) -> QueryIntent {
        QueryIntent::parse(q)
    }

    #[test]
    fn arabic_text_is_folded_for_comparison() {
        assert_eq!(normalize("أحمد إبراهيم آمنة"), "احمد ابراهيم امنه");
        assert_eq!(normalize("مَدْرَسَة"), "مدرسه");
        assert_eq!(normalize("الحلقة ٢٢"), "الحلقه 22");
        assert_eq!(normalize("۱۲۳ ٠٩"), "123 09");
        assert_eq!(normalize("ＦＵＬＬ  Width"), "full width");
        assert_eq!(normalize("“quoted”"), "\"quoted\"");
        assert_eq!(normalize("كـــتاب"), "كتاب");
    }

    #[test]
    fn english_episode_forms() {
        for q in [
            "naruto shippuden episode 22",
            "naruto shippuden ep 22",
            "naruto shippuden ep.22",
            "naruto shippuden e22",
            "Naruto Shippuden EPISODE 022",
        ] {
            let i = p(q);
            assert_eq!(i.episode, Some(22), "{q}");
            assert_eq!(i.title_terms, ["naruto", "shippuden"], "{q}");
        }
    }

    #[test]
    fn arabic_episode_and_season_forms() {
        let i = p("ناروتو شيبودن الحلقة 22");
        assert_eq!(i.episode, Some(22));
        assert_eq!(i.title_terms, ["ناروتو", "شيبودن"]);
        let i = p("مسلسل قيامة عثمان حلقة ٢٢ الموسم ٣");
        assert_eq!(i.episode, Some(22));
        assert_eq!(i.season, Some(3));
        assert_eq!(i.title_terms, ["قيامه", "عثمان"]);
        let i = p("الحلقه ٥ من مسلسل");
        assert_eq!(i.episode, Some(5));
        // Arabic series often number seasons as "parts"
        let i = p("باب الحارة الجزء 3 الحلقة 5");
        assert_eq!((i.season, i.episode), (Some(3), Some(5)));
        assert_eq!(i.title_terms, ["باب", "الحاره"]);
        // spelled-out parts and seasons
        assert_eq!(p("باب الحارة الجزء الثالث").season, Some(3));
        assert_eq!(p("قيامة عثمان الموسم الثانية").season, Some(2));
        assert_eq!(p("the show season two episode 3").season, Some(2));
        assert_eq!(
            Facts::of("باب الحارة - الجزء الثاني - الحلقة 5").seasons,
            [2]
        );
    }

    #[test]
    fn season_and_episode_together() {
        let i = p("breaking bad s02e05");
        assert_eq!((i.season, i.episode), (Some(2), Some(5)));
        assert_eq!(i.title_terms, ["breaking", "bad"]);
        let i = p("the office season 4 episode 12");
        assert_eq!((i.season, i.episode), (Some(4), Some(12)));
        let i = p("the office s04");
        assert_eq!((i.season, i.episode), (Some(4), None));
        // a lone `s3` is not a season
        assert_eq!(p("aws s3 sync").season, None);
    }

    #[test]
    fn ranges_name_no_single_episode() {
        let i = p("one piece episodes 1-10");
        assert_eq!(i.episode, None);
        assert_eq!(
            Facts::of("One Piece Episodes 1-10 Complete").episodes.len(),
            10
        );
        let f = Facts::of("Naruto Shippuden Episode 21 and 22");
        assert!(f.episodes.contains(&21));
    }

    #[test]
    fn quoted_phrases_and_exclusions() {
        let i = p(r#""attention is all you need" transformer -survey -review"#);
        assert_eq!(i.phrases, ["attention is all you need"]);
        assert_eq!(i.exclusions, ["survey", "review"]);
        assert_eq!(i.title_terms, ["transformer"]);
        let i = p("“الحلقة الأخيرة” مسلسل");
        assert_eq!(i.phrases, ["الحلقه الاخيره"]);
        // a hyphen inside a word is not an exclusion
        let i = p("covid-19 e-book");
        assert!(i.exclusions.is_empty());
    }

    #[test]
    fn year_is_a_constraint_only_when_it_reads_like_one() {
        assert_eq!(p("dune 2021").year, Some(2021));
        assert_eq!(p("dune (2021)").year, Some(2021));
        assert_eq!(p("climate report 2019 pdf").year, Some(2019));
        // a title that starts with a number-like word keeps it as a word
        let i = p("1984 george orwell");
        assert_eq!(i.year, None);
        assert!(i.title_terms.contains(&"1984".to_string()));
        assert_eq!(p("2021").year, None);
    }

    #[test]
    fn versions() {
        assert_eq!(p("vlc v3.0.20").version.as_deref(), Some("3.0.20"));
        assert_eq!(p("python 3.12.4").version.as_deref(), Some("3.12.4"));
        assert_eq!(p("godot version 4").version.as_deref(), Some("4"));
        assert_eq!(p("blender 2.93").version.as_deref(), Some("2.93"));
        assert_eq!(p("nodejs v20").version.as_deref(), Some("20"));
        assert_eq!(p("blender").version, None);
        assert_eq!(p("vlc v3.0.20").title_terms, ["vlc"]);
        // arabic wording
        assert_eq!(p("برنامج فوتوشوب الاصدار 24").version.as_deref(), Some("24"));
    }

    #[test]
    fn file_types() {
        assert_eq!(p("rust programming pdf").file_type.as_deref(), Some("pdf"));
        assert_eq!(p("rust book .epub").file_type.as_deref(), Some("epub"));
        assert_eq!(p("filetype:pdf quantum").file_type.as_deref(), Some("pdf"));
        assert_eq!(p("trailer mp4 1080p").file_type.as_deref(), Some("mp4"));
        assert_eq!(p("photo jpeg").file_type.as_deref(), Some("jpg"));
        assert_eq!(p("vlc installer exe").file_type.as_deref(), Some("exe"));
        // a bare extension word at the start is a title word ("pdf reader")
        let i = p("pdf reader");
        assert_eq!(i.file_type, None);
        assert_eq!(i.title_terms, ["reader"]);
        assert_eq!(p("كتاب البرمجة pdf").file_type.as_deref(), Some("pdf"));
    }

    #[test]
    fn language_and_script() {
        assert_eq!(p("grammar book english").language.as_deref(), Some("en"));
        assert_eq!(p("قواعد اللغة بالعربي").language.as_deref(), Some("ar"));
        assert_eq!(p("hello").language, None);
        assert_eq!(p("ناروتو").script, "arabic");
        assert_eq!(p("naruto").script, "latin");
        assert_eq!(p("naruto ناروتو").script, "mixed");
    }

    #[test]
    fn filler_words_do_not_become_title_terms() {
        let i = p("download the lord of the rings free");
        assert_eq!(i.title_terms, ["lord", "rings"]);
        assert_eq!(p("تحميل برنامج vlc مجانا").title_terms, ["برنامج", "vlc"]);
    }

    #[test]
    fn facts_come_from_the_same_extractors() {
        let f = Facts::of("Naruto.Shippuden.S01E22.1080p.mkv");
        assert_eq!(f.seasons, [1]);
        assert_eq!(f.episodes, [22]);
        let f = Facts::of("VLC media player 3.0.18 (2022)");
        assert_eq!(f.versions, ["3.0.18"]);
        assert_eq!(f.years, [2022]);
        let f = Facts::of("ناروتو الحلقة ٢١");
        assert_eq!(f.episodes, [21]);
        assert_eq!(
            extension_of("https://x.org/a/Report.Final.PDF?x=1"),
            Some("pdf".into())
        );
        assert_eq!(extension_of("https://x.org/watch"), None);
        assert_eq!(extension_of("photo.JPEG"), Some("jpg".into()));
    }

    #[test]
    fn empty_and_odd_input_never_panics() {
        for q in [
            "",
            "   ",
            "\"",
            "\"\"",
            "-",
            "- -",
            "e",
            "s",
            "v",
            "٠",
            "🙂 🙂",
            "ep",
            "e0",
            "episode 0",
        ] {
            let i = p(q);
            let _ = serde_json::to_string(&i).unwrap();
        }
        assert_eq!(p("episode 0").episode, None);
    }
}
