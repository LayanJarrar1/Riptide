//! Parse-only checks of magnet links as people actually paste them, including one whose tracker
//! URL has an empty-valued query parameter (`uk=`) and is percent-encoded. NO network: nothing
//! here touches a session, DHT or tracker, and the content behind the hash is never fetched.
#![allow(clippy::unwrap_used, missing_docs)]

use riptide_torrent::{parse_magnet, redact_url, Error};

/// The link the owner pasted: lower-case hex hash, one percent-encoded tracker whose query has an
/// empty value.
const OWNER: &str = "magnet:?xt=urn:btih:428c72ff4aac627830cfacdc8622be0e908c3b65&tr=http%3A%2F%2Frutracker.ru%2Fbt%2Fannounce.php%3Fuk%3D";
const HASH: &str = "428c72ff4aac627830cfacdc8622be0e908c3b65";

#[test]
fn the_pasted_link_parses_into_hash_and_decoded_tracker() {
    let m = parse_magnet(OWNER).unwrap();
    assert_eq!(m.info_hash, HASH);
    assert_eq!(m.display_name, None);
    assert_eq!(m.trackers.len(), 1);
    let t = &m.trackers[0];
    assert_eq!(t.scheme(), "http");
    assert_eq!(t.host_str(), Some("rutracker.ru"));
    assert_eq!(t.path(), "/bt/announce.php");
    // the empty value is kept, not dropped: `?uk=` and not `?`
    assert_eq!(t.query(), Some("uk="));
    assert_eq!(t.as_str(), "http://rutracker.ru/bt/announce.php?uk=");
}

#[test]
fn rebuilding_the_link_keeps_the_empty_valued_parameter() {
    let m = parse_magnet(OWNER).unwrap();
    let uri = m.to_uri();
    // percent-encoded again, so the tracker URL stays one parameter value
    assert!(uri.contains("uk%3D"), "{uri}");
    assert_eq!(uri, OWNER, "the rebuilt link is the pasted one");
    let again = parse_magnet(&uri).unwrap();
    assert_eq!(again, m);
    assert_eq!(again.trackers[0].query(), Some("uk="));
}

#[test]
fn input_noise_around_the_link_is_tolerated_but_not_inside_it() {
    assert!(parse_magnet(&format!("  {OWNER}  ")).is_ok());
    assert!(parse_magnet(&format!("{OWNER}\n")).is_ok());
    assert!(parse_magnet(&format!("{OWNER}\r\n")).is_ok());
    // a line break inside the link is refused (no smuggled second parameter)
    assert!(parse_magnet(&OWNER.replace("&tr=", "\n&tr=")).is_err());
    // the scheme is case-insensitive
    assert!(parse_magnet(&OWNER.replacen("magnet", "MAGNET", 1)).is_ok());
    // the parameter name `xt` is case-sensitive in the spec; an upper-case hash is fine
    let upper = OWNER.replace(HASH, &HASH.to_uppercase());
    assert_eq!(parse_magnet(&upper).unwrap().info_hash, HASH);
}

#[test]
fn base32_and_uppercase_hashes_are_accepted_and_normalised() {
    // base32 of the 20 bytes c12fe1c0...a88a (decoded independently)
    let b32 = "YEX6DQDLXISUVHOJ6UM3GWVHPQJWPKEK";
    let m = parse_magnet(&format!("magnet:?xt=urn:btih:{b32}")).unwrap();
    assert_eq!(m.info_hash, "c12fe1c06bba254a9dc9f519b35aa77c1367a88a");
    let lower = parse_magnet(&format!("magnet:?xt=urn:btih:{}", b32.to_lowercase())).unwrap();
    assert_eq!(lower.info_hash, m.info_hash);
}

#[test]
fn version_two_only_links_get_a_clear_message() {
    let v2 = "magnet:?xt=urn:btmh:1220c12fe1c06bba254a9dc9f519b335aa7c1367a88ac12fe1c06bba254a9dc9f519&dn=x";
    let e = parse_magnet(v2).unwrap_err();
    assert!(matches!(e, Error::InvalidMagnet(_)));
    let text = e.to_string();
    assert!(
        text.contains("BitTorrent v1") && text.contains("urn:btih"),
        "{text}"
    );
    // a hybrid link carries both; the v1 part is what we use
    let hybrid = format!(
        "{OWNER}&xt=urn:btmh:1220c12fe1c06bba254a9dc9f519b335aa7c1367a88ac12fe1c06bba254a9dc9f519"
    );
    assert_eq!(parse_magnet(&hybrid).unwrap().info_hash, HASH);
}

#[test]
fn hostile_variants_are_refused() {
    let other = "0".repeat(40);
    let cases = [
        // wrong or missing xt
        "magnet:?xt=urn:btih:".to_string(),
        "magnet:?xt=urn:btih:zz2c72ff4aac627830cfacdc8622be0e908c3b65".to_string(),
        format!("magnet:?xt=urn:btih:{HASH}0"),
        format!("magnet:?xt=urn:btih:{}", &HASH[..39]),
        format!("magnet:?xt=urn:btih:{HASH}%20"),
        "magnet:?xt=urn:sha1:428c72ff4aac627830cfacdc8622be0e908c3b65".to_string(),
        "magnet:?dn=just-a-name&tr=udp%3A%2F%2Ft.example%3A80".to_string(),
        "magnet:".to_string(),
        "magnet:?".to_string(),
        // two different hashes in one link
        format!("magnet:?xt=urn:btih:{HASH}&xt=urn:btih:{other}"),
        // another scheme in disguise
        format!("javascript:alert(1)//?xt=urn:btih:{HASH}"),
        format!("http://example.com/?xt=urn:btih:{HASH}"),
        format!("file:///c:/x?xt=urn:btih:{HASH}"),
        // control characters, NUL
        format!("magnet:?xt=urn:btih:{HASH}\0"),
        format!("magnet:?xt=urn:btih:{HASH}&dn=a\u{7}b"),
    ];
    for c in &cases {
        assert!(parse_magnet(c).is_err(), "should refuse {c:?}");
    }
}

#[test]
fn only_http_https_and_udp_trackers_survive() {
    let m = parse_magnet(&format!(
        "{OWNER}&tr=ftp%3A%2F%2Fx%2Fa&tr=file%3A%2F%2F%2Fetc%2Fpasswd&tr=javascript%3Aalert(1)&tr=udp%3A%2F%2Ftracker.example%3A1337%2Fannounce&tr=%20&tr=http%3A%2F%2F"
    ))
    .unwrap();
    let urls: Vec<&str> = m.trackers.iter().map(|t| t.as_str()).collect();
    assert_eq!(
        urls,
        [
            "http://rutracker.ru/bt/announce.php?uk=",
            "udp://tracker.example:1337/announce"
        ]
    );
}

#[test]
fn trackers_and_passkeys_never_reach_logs_or_errors() {
    let pass = "PASSKEY0123456789abcdef";
    let link = format!(
        "magnet:?xt=urn:btih:{HASH}&dn=Some+Movie&tr=https%3A%2F%2Fprivate.example%3A2710%2F{pass}%2Fannounce%3Fuk%3D{pass}"
    );
    let m = parse_magnet(&link).unwrap();
    // every form that exists for logs and UI strings
    for shown in [
        m.redacted(),
        format!("{m:?}").replace(&m.trackers[0].to_string(), ""),
    ] {
        assert!(!shown.contains(pass), "{shown}");
        assert!(!shown.contains("private.example"), "{shown}");
    }
    assert_eq!(
        m.redacted(),
        format!("magnet:{HASH} \"Some Movie\" (1 trackers)")
    );
    assert_eq!(
        redact_url(m.trackers[0].as_str()),
        "https://private.example:2710"
    );

    // refusals say what is wrong, not what was pasted
    let broken = [
        format!("magnet:?xt=urn:btih:nothex&tr=https%3A%2F%2Fprivate.example%2F{pass}%2Fannounce"),
        format!("magnet:?dn=x&tr=https%3A%2F%2Fprivate.example%2F{pass}%2Fannounce"),
        format!("magnet:?xt=urn:btmh:1220aa&tr=https%3A%2F%2Fprivate.example%2F{pass}%2Fannounce"),
        format!(
            "magnet:?xt=urn:btih:{HASH}&xt=urn:btih:{}&tr=https%3A%2F%2Fprivate.example%2F{pass}",
            "1".repeat(40)
        ),
        format!("magnet:?xt=urn:btih:{HASH}\n&tr=https%3A%2F%2Fprivate.example%2F{pass}"),
    ];
    for b in &broken {
        let e = parse_magnet(b).unwrap_err();
        for text in [e.to_string(), format!("{e:?}"), format!("{e:#?}")] {
            assert!(!text.contains(pass), "{text}");
            assert!(!text.contains("private.example"), "{text}");
            assert!(!text.contains("tr="), "{text}");
        }
    }
}
