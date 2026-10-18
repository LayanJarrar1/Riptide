//! Property tests: URL normalisation / de-duplication, scope, globs, robots and the HTML
//! tokenizer must hold for arbitrary input and never panic.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use riptide_crawl::glob::Glob;
use riptide_crawl::html::extract_links;
use riptide_crawl::normalize_url;
use riptide_crawl::robots::Robots;
use riptide_crawl::scope::{same_host, Scope};
use url::Url;

fn host() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z][a-zA-Z0-9-]{0,8}(\\.[a-zA-Z]{2,5}){1,2}",
        "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}".prop_filter("octets", |h| {
            h.split('.')
                .all(|o| o.parse::<u16>().is_ok_and(|n| n < 256))
        }),
    ]
}

fn segment() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[a-zA-Z0-9%._~-]{0,8}",
        1 => Just("..".to_string()),
        1 => Just(".".to_string()),
    ]
}

/// A syntactically valid http(s) URL with every feature normalisation touches.
fn any_url() -> impl Strategy<Value = Url> {
    (
        prop_oneof![Just("http"), Just("https")],
        proptest::option::of("[a-z]{1,5}(:[a-z]{0,5})?@"),
        host(),
        proptest::option::of(1u16..=65535),
        proptest::collection::vec(segment(), 0..5),
        proptest::option::of("[a-z0-9=&%]{0,10}"),
        proptest::option::of("[a-z0-9]{0,6}"),
    )
        .prop_filter_map(
            "valid url",
            |(scheme, user, host, port, segs, query, frag)| {
                let mut s = format!("{scheme}://{}{host}", user.unwrap_or_default());
                if let Some(p) = port {
                    s.push_str(&format!(":{p}"));
                }
                s.push('/');
                s.push_str(&segs.join("/"));
                if let Some(q) = query {
                    s.push('?');
                    s.push_str(&q);
                }
                if let Some(f) = frag {
                    s.push('#');
                    s.push_str(&f);
                }
                Url::parse(&s).ok()
            },
        )
}

proptest! {
    #[test]
    fn normalisation_is_idempotent_and_clean(url in any_url()) {
        let n = normalize_url(&url).unwrap();
        prop_assert_eq!(normalize_url(&n), Some(n.clone()));
        prop_assert!(n.fragment().is_none());
        prop_assert!(n.username().is_empty() && n.password().is_none());
        prop_assert!(matches!(n.scheme(), "http" | "https"));
        prop_assert!(n.query() != Some(""));
        prop_assert!(!n.path().contains("/../") && !n.path().ends_with("/.."));
    }

    #[test]
    fn urls_that_differ_only_in_fragment_userinfo_or_host_case_dedupe(url in any_url(), frag in "[a-z0-9]{0,6}") {
        let base = normalize_url(&url).unwrap();
        let mut with_frag = url.clone();
        with_frag.set_fragment(Some(&frag));
        prop_assert_eq!(normalize_url(&with_frag).unwrap(), base.clone());

        let upper = url.as_str().replacen(
            url.host_str().unwrap(),
            &url.host_str().unwrap().to_ascii_uppercase(),
            1,
        );
        // upper-casing the host can only change the host's spelling, which `Url` lower-cases
        if let Ok(u) = Url::parse(&upper) {
            prop_assert_eq!(normalize_url(&u).unwrap(), base);
        }
    }

    #[test]
    fn the_default_port_is_the_same_resource(host in host(), path in "[a-z0-9/]{0,12}") {
        let (Ok(a), Ok(b)) = (
            Url::parse(&format!("http://{host}:80/{path}")),
            Url::parse(&format!("http://{host}/{path}")),
        ) else {
            return Ok(()); // an octal-looking IPv4 host such as 08.0.0.0
        };
        prop_assert_eq!(normalize_url(&a), normalize_url(&b));
    }

    #[test]
    fn other_schemes_are_never_followed(scheme in "[a-z][a-z0-9+.-]{0,8}", rest in "[a-z0-9/:.@?=&-]{0,20}") {
        prop_assume!(scheme != "http" && scheme != "https");
        if let Ok(u) = Url::parse(&format!("{scheme}:{rest}")) {
            prop_assert!(normalize_url(&u).is_none());
        }
    }

    #[test]
    fn scopes_are_reflexive_and_nested(a in any_url(), b in any_url()) {
        let (a, b) = (normalize_url(&a).unwrap(), normalize_url(&b).unwrap());
        prop_assert!(Scope::SameHost.contains(&a, &a));
        prop_assert!(Scope::SameDomain.contains(&a, &a));
        prop_assert!(Scope::Any.contains(&a, &b));
        // same host implies same domain
        if same_host(&a, &b) {
            prop_assert!(Scope::SameDomain.contains(&a, &b));
        }
    }

    #[test]
    fn globs_never_panic_and_double_star_matches_everything(pattern in "[a-z0-9*?/.]{0,16}", url in any_url()) {
        let _ = Glob::new(&pattern).matches_url(&url);
        prop_assert!(Glob::new("**").matches_url(&url));
        prop_assert!(Glob::new("/**").matches_url(&url));
    }

    #[test]
    fn robots_parser_never_panics_and_empty_rules_allow(text in "\\PC{0,200}", path in "/[a-z0-9/?=]{0,20}") {
        let r = Robots::parse(&text, "riptide");
        let _ = r.is_allowed(&path);
        let _ = r.crawl_delay();
        prop_assert!(Robots::allow_all().is_allowed(&path));
        prop_assert!(Robots::parse("", "riptide").is_allowed(&path));
    }

    #[test]
    fn html_extraction_never_panics_and_respects_the_cap(html in "\\PC{0,300}", cap in 0usize..5) {
        let base = Url::parse("http://h.test/a/b.html").unwrap();
        prop_assert!(extract_links(&html, &base, cap).len() <= cap);
        let tagged = format!("<a href=\"{html}\">x</a><img src='{html}'>");
        prop_assert!(extract_links(&tagged, &base, cap).len() <= cap);
    }
}
