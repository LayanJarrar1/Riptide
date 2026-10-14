//! Error type shared by the core model.

use crate::state::DownloadState;

/// Errors produced by the pure model. Engine and store crates wrap this in their own error types.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A state transition that the table in [`DownloadState::can_transition_to`] does not allow.
    #[error("illegal download state transition: {from} -> {to}")]
    IllegalTransition {
        /// State the download was in.
        from: DownloadState,
        /// State that was requested.
        to: DownloadState,
    },
    /// A value that violates a model invariant (for example a segment whose `end < start`).
    #[error("invalid value: {0}")]
    Invalid(String),
}
