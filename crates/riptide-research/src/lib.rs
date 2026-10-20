//! Open-access research search for Riptide.
//!
//! Eight adapters ([`sources`]) turn a keyword query into [`Hit`]s with a direct, open download
//! link: OpenAlex, Crossref, arXiv, Europe PMC, Internet Archive, Wikimedia Commons, GitHub
//! releases and Data.gov. Nothing paywalled, nothing pirated: a result without an open direct
//! link is dropped, never guessed. [`search_all`] runs them concurrently and merges the answers.
//!
//! The AI layer ([`assist`]) is optional: it plans queries, ranks results with a one-line reason
//! and suggests file names, and everything the model returns is validated against the adapter
//! results. This crate knows nothing about Tauri or any UI.
//!
//! Etiquette: every request carries a polite `User-Agent` without personal data, each service
//! is paced (arXiv: one request per three seconds), timeouts and bounded retries apply, and
//! failures are typed ([`ResearchError`]).

#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod assist;
pub mod error;
pub mod http;
pub mod search;
pub mod sources;
pub mod types;

pub use error::{ResearchError, Result};
pub use search::{
    merge, search_all, search_all_in, search_stream, Dedup, SearchEvent, SearchOutcome,
    SourceFailure,
};
pub use sources::{default_source, SearchSource};
pub use types::{Category, Hit, HitKind, Rights, RightsKind, Source};
