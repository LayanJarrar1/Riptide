//! SQLite persistence for Riptide.
//!
//! One file, WAL journal, `busy_timeout`, foreign keys on. Every write is a transaction, so killing
//! the process at any instant leaves a consistent database. The schema is versioned
//! and migrated forward by [`migrate`]; secrets live in their own table and are wiped when a
//! download reaches a terminal state or is removed.
//!
//! The crate is synchronous on purpose: callers on an async runtime wrap calls in
//! `spawn_blocking` or own a dedicated thread.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

mod error;
pub mod migrate;
mod store;

pub use error::Error;
pub use migrate::{Migration, MIGRATIONS, SCHEMA_VERSION};
pub use store::{Category, Queue, SegmentProgress, Store, BUSY_TIMEOUT_MS};

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
