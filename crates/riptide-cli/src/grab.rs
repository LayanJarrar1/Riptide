//! `riptide grab`: crawl a site for files and (unless `--list`) download them all.
//!
//! The crawl and the downloads run on the same in-process engine the desktop app uses
//! ([`riptide_engine::Engine::crawl`], then `Engine::add`); the engine's database is a temporary
//! one, so a running Riptide app and the CLI never fight over the same records. Files that
//! already exist in the output folder are skipped, so running the same command again only
//! fetches what is new.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context};
use riptide_core::{Category, Credentials, DownloadId, DownloadState, Event, Secrets};
use riptide_crawl::{CrawlEvent, CrawlSpec, Scope, SkipReason, StopReason};
use riptide_engine::{Engine, EngineConfig, EngineSettings};
use riptide_net::redirect::strip_query;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::fetch::{human_bytes, human_duration};
use crate::i18n::{t, tf, Lang};

/// Everything `riptide grab` takes from the command line.
#[derive(Debug, Clone)]
pub struct GrabOptions {
    /// Start page.
    pub url: String,
    /// Link depth (`0` = the start page only).
    pub depth: u32,
    /// File types to keep (empty = all).
    pub types: Vec<Category>,
    /// Walk into the whole registrable domain.
    pub same_domain: bool,
    /// Walk anywhere (bounded by depth and caps).
    pub any_site: bool,
    /// Include globs.
    pub include: Vec<String>,
    /// Exclude globs.
    pub exclude: Vec<String>,
    /// Smallest file.
    pub min_size: Option<u64>,
    /// Largest file.
    pub max_size: Option<u64>,
    /// Page cap.
    pub max_pages: usize,
    /// File cap.
    pub max_files: usize,
    /// Time budget of the crawl in seconds.
    pub time_limit: u64,
    /// Ignore `robots.txt`.
    pub no_robots: bool,
    /// Only print what was found.
    pub list: bool,
    /// Print the result as JSON.
    pub json: bool,
    /// Where files go.
    pub output: PathBuf,
    /// Extra headers (sent to the start origin only).
    pub headers: Vec<(String, String)>,
    /// `Cookie` header value (start origin only).
    pub cookie: Option<String>,
    /// Basic / Digest credentials (start origin only).
    pub credentials: Option<Credentials>,
    /// User agent.
    pub user_agent: Option<String>,
    /// Less output.
    pub quiet: bool,
}

/// Parses `video,documents` into categories. Accepts the keys (`compressed`, `documents`,
/// `music`, `video`, `programs`, `images`, `other`) and a few natural aliases.
pub fn parse_types(raw: &str) -> anyhow::Result<Vec<Category>> {
    let mut out = Vec::new();
    for part in raw.split(',').map(|p| p.trim().to_ascii_lowercase()) {
        if part.is_empty() {
            continue;
        }
        let key = match part.as_str() {
            "archives" | "archive" | "zip" => "compressed",
            "docs" | "document" => "documents",
            "audio" | "mp3" => "music",
            "videos" | "movies" => "video",
            "software" | "apps" | "program" => "programs",
            "image" | "pictures" | "photos" => "images",
            other => other,
        };
        let category = Category::from_key(key).with_context(|| {
            format!("unknown file type {part:?}; use compressed, documents, music, video, programs, images or other")
        })?;
        if !out.contains(&category) {
            out.push(category);
        }
    }
    Ok(out)
}

/// `500`, `64k`, `5M`, `1.5G`: bytes with an optional binary suffix.
pub fn parse_size(raw: &str) -> anyhow::Result<u64> {
    let s = raw.trim().to_ascii_lowercase();
    let s = s.strip_suffix('b').unwrap_or(&s);
    let (number, factor) = match s.chars().last() {
        Some('k') => (&s[..s.len() - 1], 1u64 << 10),
        Some('m') => (&s[..s.len() - 1], 1 << 20),
        Some('g') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    let value: f64 = number
        .trim()
        .parse()
        .with_context(|| format!("{raw:?} is not a size (try 500, 64k, 5M, 1.5G)"))?;
    if !value.is_finite() || value < 0.0 {
        bail!("{raw:?} is not a size");
    }
    Ok((value * factor as f64) as u64)
}

/// The crawl request for `opts`.
pub fn build_spec(opts: &GrabOptions) -> CrawlSpec {
    let mut spec = CrawlSpec::new(opts.url.clone());
    spec.depth = opts.depth;
    spec.scope = if opts.any_site {
        Scope::Any
    } else if opts.same_domain {
        Scope::SameDomain
    } else {
        Scope::SameHost
    };
    spec.categories = opts.types.clone();
    spec.include = opts.include.clone();
    spec.exclude = opts.exclude.clone();
    spec.min_size = opts.min_size;
    spec.max_size = opts.max_size;
    // sizes make a listing far more useful and cost one HEAD per file
    spec.probe_files = opts.list;
    spec.max_pages = opts.max_pages;
    spec.max_files = opts.max_files;
    spec.time_budget = Duration::from_secs(opts.time_limit);
    spec.respect_robots = !opts.no_robots;
    let secrets = secrets_of(opts);
    spec.secrets = (!secrets.is_empty()).then_some(secrets);
    spec
}

fn secrets_of(opts: &GrabOptions) -> Secrets {
    Secrets {
        cookies: opts.cookie.clone(),
        headers: opts.headers.clone(),
        auth: opts.credentials.clone(),
    }
}

/// Runs `riptide grab`.
pub async fn grab(opts: GrabOptions, lang: Lang) -> anyhow::Result<()> {
    let tmp = tempfile::tempdir().context("creating a temporary folder")?;
    std::fs::create_dir_all(&opts.output)
        .with_context(|| format!("creating {}", opts.output.display()))?;
    let output = std::fs::canonicalize(&opts.output)?;
    let engine = Engine::open(EngineConfig {
        db_path: tmp.path().join("grab.sqlite"),
        default_dir: output.clone(),
        organize_new_install: false,
    })
    .await?;
    let mut settings = EngineSettings::with_default_dir(&output);
    settings.user_agent = opts.user_agent.clone();
    engine.set_settings(settings).await?;

    let cancel = CancellationToken::new();
    let ctrl_c = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel.cancel();
            }
        }
    });

    // --- crawl ---------------------------------------------------------------------------------
    if !opts.quiet {
        eprintln!(
            "{}",
            tf(
                lang,
                "grab.crawling",
                &[
                    ("url", &strip_query(&opts.url)),
                    ("depth", &opts.depth.to_string())
                ]
            )
        );
    }
    let (tx, mut rx) = mpsc::channel(64);
    let quiet = opts.quiet;
    let watcher = tokio::spawn(async move {
        let mut robots = 0u64;
        while let Some(event) = rx.recv().await {
            match event {
                CrawlEvent::PageVisited { url, depth, .. } if !quiet => {
                    eprintln!("  [{depth}] {}", strip_query(&url));
                }
                CrawlEvent::Skipped {
                    reason: SkipReason::Robots,
                    ..
                } => robots += 1,
                _ => {}
            }
        }
        robots
    });
    let spec = build_spec(&opts);
    let secrets = spec.secrets.clone();
    let result = engine.crawl(spec, Some(tx), cancel.clone()).await;
    let robots_skipped = watcher.await.unwrap_or(0);
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            ctrl_c.abort();
            return Err(e.into());
        }
    };

    if !opts.quiet {
        eprintln!(
            "{}",
            tf(
                lang,
                "grab.found",
                &[
                    ("files", &result.files.len().to_string()),
                    ("pages", &result.stats.pages_visited.to_string()),
                    (
                        "time",
                        &human_duration(Duration::from_millis(result.stats.elapsed_ms))
                    ),
                ]
            )
        );
        if result.page_cap_hit {
            eprintln!("{}", t(lang, "grab.cap_pages"));
        }
        if result.file_cap_hit {
            eprintln!("{}", t(lang, "grab.cap_files"));
        }
        match result.stop {
            StopReason::TimeBudget => eprintln!("{}", t(lang, "grab.stop_time")),
            StopReason::Cancelled => eprintln!("{}", t(lang, "grab.stop_cancelled")),
            StopReason::Finished => {}
        }
        if robots_skipped > 0 {
            eprintln!(
                "{}",
                tf(lang, "grab.robots", &[("n", &robots_skipped.to_string())])
            );
        }
    }

    // --- list ----------------------------------------------------------------------------------
    if opts.list {
        ctrl_c.abort();
        if opts.json {
            let out = serde_json::json!({
                "start": result.start,
                "stop": result.stop,
                "stats": result.stats,
                "files": result.files,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else if result.files.is_empty() {
            eprintln!("{}", t(lang, "grab.none"));
        } else {
            for f in &result.files {
                let size = f.size.map_or_else(|| "-".to_string(), human_bytes);
                println!("{}\t{}\t{}\t{}", f.category, size, f.guessed_name, f.url);
            }
        }
        engine.shutdown().await;
        return Ok(());
    }
    if result.stop == StopReason::Cancelled {
        engine.shutdown().await;
        bail!("{}", t(lang, "grab.stop_cancelled"));
    }

    // --- download ------------------------------------------------------------------------------
    let mut events = engine.subscribe();
    let mut pending: BTreeSet<DownloadId> = BTreeSet::new();
    let mut names = std::collections::HashMap::new();
    let mut existing = 0usize;
    for (file, request) in result
        .files
        .iter()
        .zip(result.download_requests(secrets.as_ref(), Some(&output)))
    {
        if output.join(&file.guessed_name).exists() {
            existing += 1;
            continue;
        }
        let id = engine.add(request).await?;
        names.insert(id, file.guessed_name.clone());
        pending.insert(id);
    }
    if !opts.quiet {
        if existing > 0 {
            eprintln!(
                "{}",
                tf(
                    lang,
                    "grab.existing",
                    &[
                        ("n", &existing.to_string()),
                        ("dir", &output.display().to_string())
                    ]
                )
            );
        }
        if !pending.is_empty() {
            eprintln!(
                "{}",
                tf(
                    lang,
                    "grab.downloading",
                    &[
                        ("n", &pending.len().to_string()),
                        ("dir", &output.display().to_string())
                    ]
                )
            );
        }
    }

    let (mut ok, mut failed) = (0usize, 0usize);
    while !pending.is_empty() {
        tokio::select! {
            () = cancel.cancelled() => {
                for id in &pending {
                    let _ = engine.cancel(*id).await;
                }
                engine.shutdown().await;
                bail!("{}", t(lang, "grab.stop_cancelled"));
            }
            event = events.recv() => match event {
                Ok(Event::Completed { id, .. }) if pending.remove(&id) => {
                    ok += 1;
                    if !opts.quiet {
                        eprintln!("{}", tf(lang, "grab.file_done", &[("name", &names[&id])]));
                    }
                }
                Ok(Event::Failed { id, error, .. }) if pending.remove(&id) => {
                    failed += 1;
                    eprintln!(
                        "{}",
                        tf(lang, "grab.file_failed", &[("name", &names[&id]), ("error", &error)])
                    );
                }
                Ok(Event::Removed { id } | Event::StateChanged { id, to: DownloadState::Cancelled, .. })
                    if pending.remove(&id) =>
                {
                    failed += 1;
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
    ctrl_c.abort();
    engine.shutdown().await;
    println!(
        "{}",
        tf(
            lang,
            "grab.summary",
            &[("ok", &ok.to_string()), ("failed", &failed.to_string())]
        )
    );
    if failed > 0 {
        bail!("{}", t(lang, "grab.some_failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_and_sizes_parse() {
        assert_eq!(
            parse_types("video, Documents,archives,video").unwrap(),
            vec![Category::Video, Category::Documents, Category::Compressed]
        );
        assert!(parse_types("").unwrap().is_empty());
        assert!(parse_types("movies,nonsense").is_err());
        assert_eq!(parse_size("500").unwrap(), 500);
        assert_eq!(parse_size("64k").unwrap(), 64 * 1024);
        assert_eq!(parse_size("5MB").unwrap(), 5 << 20);
        assert_eq!(parse_size("1.5G").unwrap(), 3 << 29);
        assert!(parse_size("big").is_err());
        assert!(parse_size("-1").is_err());
    }

    #[test]
    fn options_become_a_spec() {
        let opts = GrabOptions {
            url: "https://example.com/a".into(),
            depth: 2,
            types: vec![Category::Video],
            same_domain: true,
            any_site: false,
            include: vec!["*.mp4".into()],
            exclude: vec!["/ads/**".into()],
            min_size: Some(10),
            max_size: None,
            max_pages: 7,
            max_files: 9,
            time_limit: 30,
            no_robots: false,
            list: true,
            json: false,
            output: ".".into(),
            headers: vec![],
            cookie: Some("a=b".into()),
            credentials: None,
            user_agent: None,
            quiet: true,
        };
        let spec = build_spec(&opts);
        assert_eq!(spec.scope, Scope::SameDomain);
        assert_eq!(spec.depth, 2);
        assert_eq!(spec.max_pages, 7);
        assert_eq!(spec.time_budget, Duration::from_secs(30));
        assert!(spec.respect_robots && spec.probe_files);
        assert_eq!(spec.secrets.unwrap().cookies.as_deref(), Some("a=b"));
        let any = build_spec(&GrabOptions {
            any_site: true,
            no_robots: true,
            cookie: None,
            ..opts
        });
        assert_eq!(any.scope, Scope::Any);
        assert!(!any.respect_robots && any.secrets.is_none());
    }
}
