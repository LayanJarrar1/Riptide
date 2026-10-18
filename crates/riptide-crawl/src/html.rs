//! Link extraction from HTML.
//!
//! Only the *tokenizer* of `html5ever` is used (no DOM is ever built): it implements the WHATWG
//! tokenisation rules exactly as browsers do (unquoted attributes, character references,
//! comments, `<script>`/`<style>` raw text), which hand-rolled scanning gets wrong in exactly the
//! places hostile pages exploit. `html5ever` is the Servo project's parser, actively maintained,
//! and already in the build through the Tauri webview stack, so it adds no new code to ship.
//! `scraper` / `kuchikiki` were rejected because they build a full tree plus a CSS selector
//! engine for what is a linear scan; `tl` is not spec-conformant.
//!
//! No JavaScript is executed or screened: links that only exist after scripts run are not found.

use std::cell::{Cell, RefCell};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use url::Url;

/// What a link probably points at, judged from its URL alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// A web page: no extension and a trailing `/`, or `.html`, `.php`, ...
    Page,
    /// Unknown: no recognisable extension (`/download?id=3`). Treated as a page, but fetched
    /// just far enough to see whether it is really a file.
    Maybe,
    /// A file: a media element's source, `<a download>`, or a known non-page extension.
    File,
}

/// One link found on a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Resolved against the page URL (or its `<base href>`), fragment not yet removed, scheme not
    /// yet checked.
    pub url: Url,
    /// Page or file.
    pub kind: LinkKind,
}

/// Extensions that mean "this is a page to read".
const PAGE_EXTENSIONS: &[&str] = &[
    "html", "htm", "xhtml", "shtml", "php", "asp", "aspx", "jsp", "jspx", "cfm", "cgi", "pl",
];

/// Classifies `url` by its path.
#[must_use]
pub fn classify(url: &Url) -> LinkKind {
    let last = url.path().rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        return LinkKind::Page;
    }
    let Some((_, ext)) = last.rsplit_once('.') else {
        return LinkKind::Maybe;
    };
    let ext = ext.to_ascii_lowercase();
    if PAGE_EXTENSIONS.contains(&ext.as_str()) {
        return LinkKind::Page;
    }
    // "v1.2", "1.5.3": not an extension
    let plausible = (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic());
    if plausible {
        LinkKind::File
    } else {
        LinkKind::Maybe
    }
}

/// `rel` values whose `<link>` points at another document worth following.
const DOCUMENT_RELS: &[&str] = &[
    "alternate",
    "next",
    "prev",
    "previous",
    "help",
    "author",
    "license",
    "archives",
    "bookmark",
    "enclosure",
    "index",
    "start",
    "up",
    "contents",
];

/// `rel` values that make a `<link>` a resource of the page, never a document.
const RESOURCE_RELS: &[&str] = &[
    "stylesheet",
    "icon",
    "shortcut",
    "apple-touch-icon",
    "preload",
    "prefetch",
    "preconnect",
    "dns-prefetch",
    "modulepreload",
    "manifest",
    "mask-icon",
    "pingback",
];

/// Extracts the links of `html` (a page fetched from `page_url`), at most `max_links`.
///
/// Elements read: `a`/`area[href]`, `iframe`/`frame[src]`, `img`/`source[src|srcset]` (also the
/// lazy-loading `data-src`/`data-srcset`), `video[src|poster]`, `audio`/`track`/`embed[src]`,
/// `object[data]`, `link[href]` for document relations (not stylesheets or icons), and
/// `meta http-equiv=refresh`. Text inside `<script>`, `<style>`, `<title>`, `<textarea>` and
/// comments is never searched.
#[must_use]
pub fn extract_links(html: &str, page_url: &Url, max_links: usize) -> Vec<Link> {
    let sink = LinkSink {
        base: RefCell::new(page_url.clone()),
        base_seen: Cell::new(false),
        links: RefCell::new(Vec::new()),
        max_links,
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.links.take()
}

struct LinkSink {
    base: RefCell<Url>,
    base_seen: Cell<bool>,
    links: RefCell<Vec<Link>>,
    max_links: usize,
}

fn attr<'a>(tag: &'a Tag, name: &str) -> Option<&'a str> {
    tag.attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| &*a.value)
}

impl LinkSink {
    fn push(&self, raw: &str, kind: Option<LinkKind>) {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with('#') {
            return;
        }
        let mut links = self.links.borrow_mut();
        if links.len() >= self.max_links {
            return;
        }
        if let Ok(url) = self.base.borrow().join(raw) {
            let kind = kind.unwrap_or_else(|| classify(&url));
            links.push(Link { url, kind });
        }
    }

    fn push_srcset(&self, srcset: &str) {
        // "url1 1x, url2 2x": a candidate is a URL (no spaces), optional descriptor, comma
        let mut rest = srcset.trim_start();
        while !rest.is_empty() {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (candidate, after) = rest.split_at(end);
            // a URL may carry the separating comma itself ("a.jpg,b.jpg 2x" style is invalid, but
            // "a.jpg, b.jpg" is common)
            let candidate_url = candidate.trim_end_matches(',');
            self.push(candidate_url, Some(LinkKind::File));
            rest = if candidate.ends_with(',') {
                after.trim_start()
            } else {
                // skip the descriptor up to the next comma
                match after.find(',') {
                    Some(i) => after[i + 1..].trim_start(),
                    None => "",
                }
            };
        }
    }

    fn handle(&self, tag: &Tag) -> TokenSinkResult<()> {
        let name: &str = &tag.name;
        match name {
            "base" => {
                if !self.base_seen.replace(true) {
                    if let Some(href) = attr(tag, "href") {
                        let joined = self.base.borrow().join(href.trim());
                        if let Ok(base) = joined {
                            if matches!(base.scheme(), "http" | "https") {
                                *self.base.borrow_mut() = base;
                            }
                        }
                    }
                }
            }
            "a" | "area" => {
                if let Some(href) = attr(tag, "href") {
                    // a download attribute says it is a file whatever the URL looks like
                    let kind = attr(tag, "download").map(|_| LinkKind::File);
                    self.push(href, kind);
                }
            }
            "iframe" | "frame" => {
                if let Some(src) = attr(tag, "src") {
                    self.push(src, None);
                }
            }
            "img" | "source" => {
                for key in ["src", "data-src"] {
                    if let Some(src) = attr(tag, key) {
                        self.push(src, Some(LinkKind::File));
                    }
                }
                for key in ["srcset", "data-srcset"] {
                    if let Some(set) = attr(tag, key) {
                        self.push_srcset(set);
                    }
                }
            }
            "video" => {
                for key in ["src", "poster"] {
                    if let Some(src) = attr(tag, key) {
                        self.push(src, Some(LinkKind::File));
                    }
                }
            }
            "audio" | "track" | "embed" => {
                if let Some(src) = attr(tag, "src") {
                    self.push(src, Some(LinkKind::File));
                }
            }
            "object" => {
                if let Some(data) = attr(tag, "data") {
                    self.push(data, Some(LinkKind::File));
                }
            }
            "link" => {
                let rel = attr(tag, "rel").unwrap_or("").to_ascii_lowercase();
                let rels: Vec<&str> = rel.split_whitespace().collect();
                let is_resource = rels.iter().any(|r| RESOURCE_RELS.contains(r));
                let is_document = rels.iter().any(|r| DOCUMENT_RELS.contains(r));
                if is_document && !is_resource {
                    if let Some(href) = attr(tag, "href") {
                        self.push(href, None);
                    }
                }
            }
            "meta" => {
                let refresh = attr(tag, "http-equiv")
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("refresh"));
                if refresh {
                    if let Some(content) = attr(tag, "content") {
                        if let Some(pos) = content.to_ascii_lowercase().find("url=") {
                            let target = content[pos + 4..].trim().trim_matches(['\'', '"']);
                            self.push(target, None);
                        }
                    }
                }
            }
            // text inside these must not be tokenised as markup
            "script" => return TokenSinkResult::RawData(RawKind::ScriptData),
            "style" | "xmp" => return TokenSinkResult::RawData(RawKind::Rawtext),
            "title" | "textarea" => return TokenSinkResult::RawData(RawKind::Rcdata),
            "plaintext" => return TokenSinkResult::Plaintext,
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

impl TokenSink for LinkSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => self.handle(&tag),
            _ => TokenSinkResult::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(html: &str) -> Vec<(String, LinkKind)> {
        let base = Url::parse("http://h.test/dir/page.html").unwrap();
        extract_links(html, &base, 1000)
            .into_iter()
            .map(|l| (l.url.to_string(), l.kind))
            .collect()
    }

    fn urls(html: &str) -> Vec<String> {
        links(html).into_iter().map(|(u, _)| u).collect()
    }

    #[test]
    fn anchors_resolve_and_classify() {
        let got = links(
            r##"<a href="a.html">1</a><a href=b.pdf>2</a><a href='/c/'>3</a><a href="d?id=1">4</a>
               <a href="http://other.test/e.zip">5</a><a href="v1.2">6</a><a href="#top">7</a><a>8</a>"##,
        );
        assert_eq!(
            got,
            vec![
                ("http://h.test/dir/a.html".into(), LinkKind::Page),
                ("http://h.test/dir/b.pdf".into(), LinkKind::File),
                ("http://h.test/c/".into(), LinkKind::Page),
                ("http://h.test/dir/d?id=1".into(), LinkKind::Maybe),
                ("http://other.test/e.zip".into(), LinkKind::File),
                ("http://h.test/dir/v1.2".into(), LinkKind::Maybe),
            ]
        );
    }

    #[test]
    fn media_elements_and_srcset() {
        let got = urls(
            r#"<img src="i.jpg" srcset="i1.jpg 1x, i2.jpg 2x,i3.jpg 480w">
               <video src="v.mp4" poster="p.png"><source src="v.webm"><track src="t.vtt"></video>
               <audio src=a.mp3></audio><embed src="e.swf"><object data="o.pdf"></object>
               <picture><source srcset="s1.avif, s2.avif 2x"></picture>
               <img data-src="lazy.jpg">"#,
        );
        for want in [
            "i.jpg", "i1.jpg", "i2.jpg", "i3.jpg", "v.mp4", "p.png", "v.webm", "t.vtt", "a.mp3",
            "e.swf", "o.pdf", "s1.avif", "s2.avif", "lazy.jpg",
        ] {
            assert!(
                got.contains(&format!("http://h.test/dir/{want}")),
                "{want} missing in {got:?}"
            );
        }
        assert!(links(r#"<img src="noext">"#)
            .iter()
            .all(|(_, k)| *k == LinkKind::File));
    }

    #[test]
    fn link_rel_picks_documents_not_resources() {
        let got = urls(
            r#"<link rel="stylesheet" href="s.css"><link rel="icon" href="f.ico">
               <link rel="alternate" type="application/pdf" href="print.pdf">
               <link rel="next" href="page2.html"><link rel="alternate stylesheet" href="x.css">
               <link rel="preload" href="font.woff2"><link href="norel.html">"#,
        );
        assert_eq!(
            got,
            vec![
                "http://h.test/dir/print.pdf".to_string(),
                "http://h.test/dir/page2.html".to_string()
            ]
        );
    }

    #[test]
    fn base_href_and_download_attribute_and_refresh() {
        let got = links(
            r#"<base href="/other/"><base href="/ignored/"><a href="x.html">1</a>
               <a href="getit" download>2</a><meta http-equiv="Refresh" content="0; URL='/moved.html'">"#,
        );
        assert_eq!(
            got,
            vec![
                ("http://h.test/other/x.html".into(), LinkKind::Page),
                ("http://h.test/other/getit".into(), LinkKind::File),
                ("http://h.test/moved.html".into(), LinkKind::Page),
            ]
        );
        // a base that is not http(s) is ignored
        assert_eq!(
            urls(r#"<base href="javascript:x"><a href="y.html">"#),
            vec!["http://h.test/dir/y.html"]
        );
    }

    #[test]
    fn scripts_styles_comments_and_titles_are_not_markup() {
        let got = urls(
            r#"<title><a href="t.html"></title>
               <script>document.write('<a href="s.html">')</script>
               <style>a[href="c.css"] { background: url(x.png) }</style>
               <!-- <a href="comment.html"> -->
               <textarea><a href="ta.html"></textarea>
               <a href="real.html">"#,
        );
        assert_eq!(got, vec!["http://h.test/dir/real.html"]);
    }

    #[test]
    fn malformed_html_is_survived() {
        let got = urls("<a href=\"unterminated.html><a href=ok.zip>text<a href=");
        // the unterminated attribute swallows the rest, like a browser; nothing panics
        assert!(got.len() <= 1, "{got:?}");
        assert!(urls("").is_empty());
        assert!(urls("\u{0}<<<>>>&amp;&#x41;<a href=&amp;x.zip>").len() <= 1);
        assert_eq!(
            urls(r#"<A HREF="Up.PDF" Title=x>"#),
            vec!["http://h.test/dir/Up.PDF"]
        );
    }

    #[test]
    fn the_link_cap_is_enforced() {
        let html = "<a href=x.zip>".repeat(100);
        let base = Url::parse("http://h.test/").unwrap();
        assert_eq!(extract_links(&html, &base, 10).len(), 10);
    }

    #[test]
    fn entities_in_attributes_are_decoded() {
        assert_eq!(
            urls(r#"<a href="f.php?a=1&amp;b=2">"#),
            vec!["http://h.test/dir/f.php?a=1&b=2"]
        );
    }
}
