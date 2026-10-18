//! The site grabber: starting from one page, find the downloadable files a site links to.
//!
//! [`crawl`] walks HTML pages breadth-first to a link depth (`0` = the start page only), lists
//! the files it finds as [`FoundFile`]s (name, category, size when probed, the page it was found
//! on) and reports progress as [`CrawlEvent`]s. It does **not** download anything: the caller
//! turns the result into downloads (the engine's `add`), after the user has previewed it.
//! `riptide_engine::Engine::crawl` is the entry point the app and the CLI use.
//!
//! # What is followed
//!
//! * Only `http` and `https`. Fragments are removed, user info is dropped, and every URL is
//!   visited once ([`scope::normalize_url`]), so link loops end.
//! * A [`Scope`] decides which pages are walked into (same host, same domain, anywhere). Files
//!   linked from an in-scope page are listed even when they live elsewhere
//!   ([`CrawlSpec::offsite_files`]); an off-site link of unknown type is fetched once, to see
//!   whether it is a file, and an off-site *page* is never read.
//! * Links come from the HTML only ([`html`]): no JavaScript runs.
//! * [`CrawlSpec::include`] / [`CrawlSpec::exclude`] globs, file-type [`Category`] filter and a
//!   size window narrow what is listed.
//!
//! # How politely
//!
//! * `robots.txt` is fetched once per origin and obeyed ([`robots`]): `User-agent: riptide` or
//!   `*`, `Allow`/`Disallow` with `*` and `$`, `Crawl-delay`. A missing file means "no rules"; an
//!   unreachable one means "no access" (RFC 9309).
//! * At most [`CrawlSpec::per_host`] (2) requests in flight per host, at least
//!   [`CrawlSpec::delay`] or the host's `Crawl-delay` apart ([`gate`]).
//! * Hard caps: pages (500), files (5 000), bytes per HTML page (5 MiB) and a time budget; a
//!   [`tokio_util::sync::CancellationToken`] stops it at once.
//!
//! # What it never sends where
//!
//! Cookies, headers and credentials in [`CrawlSpec::secrets`] go to the **start page's origin
//! only** (`riptide_net::RequestContext` enforces it per hop, redirects included). The `Referer`
//! of a request to another origin is the page's origin only. `robots.txt` is fetched without any
//! secrets. Redirects are followed by hand, at most 20, as everywhere in the engine.
//!
//! # Sync mode
//!
//! Pass the previous [`CrawlResult::known_files`] as [`CrawlSpec::previous`]: every file is
//! probed and [`FoundFile::changed`] says whether its `ETag` (else `Last-Modified`) differs.
//! The check is a `HEAD` (or a one-byte ranged `GET`) compared locally, not a conditional
//! request, so servers that ignore `If-None-Match` work too.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

mod crawler;
pub mod gate;
pub mod glob;
pub mod html;
pub mod robots;
pub mod scope;
mod types;

pub use crawler::crawl;
pub use riptide_core::Category;
pub use scope::{normalize_url, Scope};
pub use types::{
    CrawlEvent, CrawlResult, CrawlSpec, CrawlStats, Error, FoundFile, KnownFile, SkipReason,
    StopReason, VisitedPage, DEFAULT_MAX_FILES, DEFAULT_MAX_PAGES, DEFAULT_MAX_PAGE_BYTES,
    MAX_LINKS_PER_PAGE,
};
