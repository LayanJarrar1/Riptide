//! Download matrix: file names, Content-Types and categories.
//!
//! The engine must not behave differently per extension: every case here downloads real bytes
//! from the in-process test server and asserts the sanitised final name, the category folder the
//! engine picked, and byte-exactness (size + SHA-256 + no partial file left).
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use riptide_core::DownloadRequest;
use riptide_testserver::DEFAULT_SEED;

/// One row of the table. `url` is the part after the server origin; `want` the sanitised name;
/// `cat` the category key the engine must pick.
struct Case {
    url: String,
    want: String,
    cat: &'static str,
}

fn case(url: impl Into<String>, want: impl Into<String>, cat: &'static str) -> Case {
    Case {
        url: url.into(),
        want: want.into(),
        cat,
    }
}

/// A `Content-Disposition` request: `/ok?cd=<percent-encoded header value>`.
fn cd(header: &str) -> String {
    format!("/ok?cd={}", pct(header))
}

fn ct_for(ext: &str) -> &'static str {
    match ext {
        "zip" => "application/zip",
        "rar" => "application/vnd.rar",
        "7z" => "application/x-7z-compressed",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "exe" => "application/vnd.microsoft.portable-executable",
        "msi" => "application/x-msi",
        "msix" => "application/msix",
        "iso" => "application/x-iso9660-image",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "epub" => "application/epub+zip",
        _ => "application/octet-stream",
    }
}

fn cat_for(ext: &str) -> &'static str {
    match ext {
        "zip" | "rar" | "7z" | "tar" | "gz" | "iso" => "compressed",
        "exe" | "msi" | "msix" => "programs",
        "pdf" | "docx" | "xlsx" | "pptx" | "txt" | "csv" | "epub" => "documents",
        "jpg" | "png" | "webp" | "gif" | "svg" => "images",
        "mp3" | "wav" => "music",
        "mp4" | "mkv" | "webm" => "video",
        _ => "other",
    }
}

fn cases() -> Vec<Case> {
    let mut v = Vec::new();

    // every requested type, name in the URL path, matching Content-Type
    for ext in [
        "zip", "rar", "7z", "tar", "gz", "exe", "msi", "msix", "iso", "pdf", "docx", "xlsx",
        "pptx", "txt", "csv", "json", "xml", "jpg", "png", "webp", "gif", "svg", "mp3", "wav",
        "mp4", "mkv", "webm", "epub",
    ] {
        v.push(case(
            format!("/file/sample.{ext}?ct={}", pct(ct_for(ext))),
            format!("sample.{ext}"),
            cat_for(ext),
        ));
    }
    // the same bytes behind a hostile Content-Type must not change naming or behaviour
    v.push(case("/file/movie.mkv?ct=text/html", "movie.mkv", "video"));
    v.push(case("/file/PHOTO.JPG?ct=image/jpeg", "PHOTO.JPG", "images"));
    v.push(case(
        "/file/backup.tar.gz?ct=application/gzip",
        "backup.tar.gz",
        "compressed",
    ));
    v.push(case("/file/notes.txt.exe", "notes.txt.exe", "programs"));
    // leading dots are trimmed on purpose (a dot-file would be hidden and could hide a traversal)
    v.push(case("/file/.hidden", "hidden", "other"));

    // extensionless names: the Content-Type may add an extension, otherwise the bare name stays
    v.push(case(
        "/file/README?ct=text/plain",
        "README.txt",
        "documents",
    ));
    v.push(case(
        "/file/archive?ct=application/zip",
        "archive.zip",
        "compressed",
    ));
    for ext in [
        "csv", "xml", "svg", "wav", "epub", "docx", "xlsx", "pptx", "msix", "json", "pdf", "mp4",
        "webm", "mkv", "7z", "rar", "gz", "tar", "iso", "jpg", "png", "gif", "webp", "mp3",
    ] {
        v.push(case(
            format!("/file/noext-{ext}?ct={}", pct(ct_for(ext))),
            format!("noext-{ext}.{ext}"),
            cat_for(ext),
        ));
    }
    v.push(case("/file/blob", "blob", "other"));
    v.push(case(
        "/file/blob2?ct=application/octet-stream",
        "blob2",
        "other",
    ));

    // punctuation that is legal in a Windows file name, from the URL path and from the header
    let punct = "my file & more #1 100% a;b 'q' (x) [y] {z} @ + = , ! ^ ~ `.pdf";
    v.push(case(format!("/file/{}", pct(punct)), punct, "documents"));
    v.push(case(
        cd(&format!("attachment; filename=\"x {punct}\"")),
        format!("x {punct}"),
        "documents",
    ));
    // characters Windows forbids are dropped, never turned into a path
    v.push(case(
        cd(r#"attachment; filename="say \"hi\" <a|b>?*.txt""#),
        "say hi ab.txt",
        "documents",
    ));
    v.push(case(
        cd(r#"attachment; filename="a;b\"c.zip""#),
        "a;bc.zip",
        "compressed",
    ));
    v.push(case(
        cd("attachment; filename=plain.mp3; size=5"),
        "plain.mp3",
        "music",
    ));
    v.push(case(
        cd("inline; filename=\"view.png\""),
        "view.png",
        "images",
    ));

    // Unicode and Arabic: RFC 5987 header and the URL path
    v.push(case(
        cd("attachment; filename*=UTF-8''%D8%AA%D9%82%D8%B1%D9%8A%D8%B1%20%D8%B3%D9%86%D9%88%D9%8A.pdf"),
        "تقرير سنوي.pdf",
        "documents",
    ));
    v.push(case(
        format!("/file/{}", pct("ملف تجريبي.mp3")),
        "ملف تجريبي.mp3",
        "music",
    ));
    v.push(case(
        format!("/file/{}", pct("日本語のファイル.zip")),
        "日本語のファイル.zip",
        "compressed",
    ));
    v.push(case(
        format!("/file/{}", pct("naïve café 😀.txt")),
        "naïve café 😀.txt",
        "documents",
    ));
    v.push(case(
        cd("attachment; filename*=ISO-8859-1''caf%E9.txt"),
        "café.txt",
        "documents",
    ));
    // decomposed (NFD) input is stored composed (NFC)
    v.push(case(
        cd("attachment; filename*=UTF-8''e%CC%81.txt"),
        "\u{e9}.txt",
        "documents",
    ));
    // the extended value wins over the plain one
    v.push(case(
        cd("attachment; filename=\"fallback.txt\"; filename*=UTF-8''real.zip"),
        "real.zip",
        "compressed",
    ));

    // very long names: capped at 200 bytes, extension kept
    v.push(case(
        format!("/file/{}.pdf", "a".repeat(300)),
        format!("{}.pdf", "a".repeat(196)),
        "documents",
    ));
    let arabic_long = "ع".repeat(150); // 300 bytes
    v.push(case(
        cd(&format!(
            "attachment; filename*=UTF-8''{}.zip",
            pct(&arabic_long)
        )),
        format!("{}.zip", "ع".repeat(98)), // 196 bytes + ".zip" = 200
        "compressed",
    ));

    // hostile names
    v.push(case(
        cd("attachment; filename=\"../../etc/passwd\""),
        "passwd",
        "other",
    ));
    v.push(case(
        cd("attachment; filename=\"..\\..\\Windows\\evil.exe\""),
        "Windowsevil.exe", // RFC 6266: a backslash in a quoted string is a quote escape
        "programs",
    ));
    v.push(case(
        cd("attachment; filename=\"C:\\Windows\\System32\\x.dll\""),
        "CWindowsSystem32x.dll",
        "other",
    ));
    v.push(case(
        cd("attachment; filename=\"/abs/olute/name.txt\""),
        "name.txt",
        "documents",
    ));
    v.push(case(
        cd("attachment; filename*=UTF-8''..%2F..%2Fup.txt"),
        "up.txt",
        "documents",
    ));
    for (raw, want, cat) in [
        ("CON", "_CON", "other"),
        ("con.txt", "_con.txt", "documents"),
        ("NUL.tar.gz", "_NUL.tar.gz", "compressed"),
        ("COM1.zip", "_COM1.zip", "compressed"),
        ("lpt9", "_lpt9", "other"),
        ("AUX .txt", "_AUX .txt", "documents"),
    ] {
        v.push(case(
            cd(&format!("attachment; filename=\"{raw}\"")),
            want,
            cat,
        ));
    }
    v.push(case(
        cd("attachment; filename=\"trailing... \""),
        "trailing",
        "other",
    ));
    v.push(case(
        cd("attachment; filename=\"report.pdf. . .\""),
        "report.pdf",
        "documents",
    ));
    v.push(case(
        cd("attachment; filename*=UTF-8''a%00b%01c%1Fd%7Fe.txt"),
        "abcde.txt",
        "documents",
    ));
    // a blank header name falls back to the URL name
    v.push(case(cd("attachment; filename=\"   \""), "ok", "other"));
    v.push(case(
        format!("/file/fallback.7z?cd={}", pct("attachment")),
        "fallback.7z",
        "compressed",
    ));
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn names_types_and_categories_are_sanitised_and_bytes_exact() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();

    // one folder per category so the category the engine chose is observable
    let root = w.dir.path().join("cats");
    let mut s = engine.settings();
    s.category_dirs = riptide_core::Category::ALL
        .iter()
        .map(|c| {
            (
                c.key().to_string(),
                root.join(c.key()).to_string_lossy().into_owned(),
            )
        })
        .collect();
    engine.set_settings(s).await.unwrap();

    let mut failures = Vec::new();
    let all = cases();
    let total = all.len();
    eprintln!("{total} name/type/category cases");
    for (n, c) in all.into_iter().enumerate() {
        let size = 2048 + (n as u64 % 7) * 1000;
        let sep = if c.url.contains('?') { '&' } else { '?' };
        let url = format!("{}{sep}size={size}", c.url);
        let id = engine.add(w.request(&url)).await.unwrap();
        let path = wait_completed(&mut rx, id).await;
        let folder = root.join(c.cat);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name != c.want || path.parent() != Some(folder.as_path()) {
            failures.push(format!(
                "{}: want {:?} in {}, got {:?} in {}",
                c.url,
                c.want,
                c.cat,
                name,
                path.parent().unwrap().display()
            ));
            continue;
        }
        assert!(name.len() <= 200, "{name} is longer than 200 bytes");
        assert_complete(&engine, id, &path, &folder, DEFAULT_SEED, size).await;
    }
    assert!(
        failures.is_empty(),
        "{} of {total} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_name_override_is_sanitised_too() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    for (raw, want) in [
        ("../../escape.txt", "escape.txt"),
        ("CON", "_CON"),
        ("ok name.zip", "ok name.zip"),
        ("dots...", "dots"),
    ] {
        let id = engine
            .add(DownloadRequest {
                filename: Some(raw.into()),
                ..w.request("/ok?size=1500")
            })
            .await
            .unwrap();
        let path = wait_completed(&mut rx, id).await;
        assert_eq!(path, w.downloads().join(want), "override {raw:?}");
        assert_complete(&engine, id, &path, &w.downloads(), DEFAULT_SEED, 1500).await;
    }
    engine.shutdown().await;
}

/// Many downloads that resolve to the same file name at the same moment get distinct names and
/// the right bytes each; nothing is overwritten.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_same_name_downloads_get_distinct_files() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let mut ids = Vec::new();
    for i in 0..8u64 {
        let size = 100_000 + i * 1000;
        let id = engine
            .add(w.request(&format!("/file/same.zip?size={size}&seed={}", 500 + i)))
            .await
            .unwrap();
        ids.push((id, size, 500 + i));
    }
    // completions arrive in any order: collect them all, then check each against its own seed
    let mut done = std::collections::HashMap::new();
    while done.len() < ids.len() {
        let ev = wait_for(&mut rx, std::time::Duration::from_secs(120), |e| {
            matches!(e, riptide_core::Event::Completed { .. })
        })
        .await;
        if let riptide_core::Event::Completed { id, path } = ev {
            done.insert(id, path);
        }
    }
    let mut names = std::collections::HashSet::new();
    for (id, size, seed) in ids {
        let p = done.remove(&id).unwrap();
        assert!(names.insert(p.clone()), "{} used twice", p.display());
        assert_complete(&engine, id, &p, &w.downloads(), seed, size).await;
    }
    assert_eq!(names.len(), 8);
    engine.shutdown().await;
}
