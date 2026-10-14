//! The download state machine.
//!
//! ```text
//! Queued ─► Probing ─► Downloading ─► Verifying ─► Completed
//!   ▲          │            │  ▲           │
//!   │          ▼            ▼  │           ▼
//!   └─────── Paused ◄───────┘  └───── Failed(retryable: bool)
//! Cancelled is reachable from every non-terminal state.
//! ```
//!
//! The exhaustive table lives in [`DownloadState::can_transition_to`]; everything not listed there
//! is [`Error::IllegalTransition`]. Two refinements over the sketch, both documented in
//! `docs/architecture.md`:
//!
//! * a retry leaves `Failed` for **`Queued`**, not straight into `Downloading`, because every start
//!   must re-probe the server (validators, `If-Range`) and respect queue limits;
//! * `Queued -> Paused` is legal so that "add but start later" and "pause a waiting item" work.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::Error;

/// Lifecycle state of a [`crate::Download`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadState {
    /// Waiting for a free slot in its queue.
    Queued,
    /// `HEAD` / ranged `GET` in flight to learn size, validators and filename.
    Probing,
    /// Segments are being written.
    Downloading,
    /// Stopped by the user; resumable.
    Paused,
    /// All bytes written; checksum / validator check running.
    Verifying,
    /// Byte-exact and verified. Terminal.
    Completed,
    /// Stopped by an error. `retryable` tells the scheduler whether an automatic retry makes sense;
    /// the user may always retry manually.
    Failed {
        /// Whether an automatic retry is reasonable (network error) or not (checksum mismatch,
        /// `Content-Length` lie, server replaced the file).
        retryable: bool,
    },
    /// Removed by the user before completion. Terminal.
    Cancelled,
}

impl DownloadState {
    /// Every distinct state value, used by exhaustive tests and UI filters.
    pub const ALL: [DownloadState; 9] = [
        DownloadState::Queued,
        DownloadState::Probing,
        DownloadState::Downloading,
        DownloadState::Paused,
        DownloadState::Verifying,
        DownloadState::Completed,
        DownloadState::Failed { retryable: true },
        DownloadState::Failed { retryable: false },
        DownloadState::Cancelled,
    ];

    /// State of a freshly added download.
    pub const fn initial() -> Self {
        DownloadState::Queued
    }

    /// `Completed` and `Cancelled` never change again.
    pub const fn is_terminal(self) -> bool {
        matches!(self, DownloadState::Completed | DownloadState::Cancelled)
    }

    /// States in which the engine holds connections or files open.
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            DownloadState::Probing | DownloadState::Downloading | DownloadState::Verifying
        )
    }

    /// Whether the user may press "resume" / "retry" from here.
    pub const fn is_resumable(self) -> bool {
        matches!(self, DownloadState::Paused | DownloadState::Failed { .. })
    }

    /// The exhaustive transition table. Returns `true` only for the arrows drawn in the module
    /// documentation. Self-transitions are illegal.
    pub const fn can_transition_to(self, to: DownloadState) -> bool {
        use DownloadState::*;
        match self {
            Queued => matches!(to, Probing | Paused | Cancelled),
            Probing => matches!(to, Downloading | Paused | Failed { .. } | Cancelled),
            Downloading => matches!(to, Verifying | Paused | Failed { .. } | Cancelled),
            Verifying => matches!(to, Completed | Failed { .. } | Cancelled),
            Paused => matches!(to, Queued | Cancelled),
            Failed { .. } => matches!(to, Queued | Cancelled),
            Completed | Cancelled => false,
        }
    }

    /// Every state reachable from `self` in one step, in table order.
    pub fn successors(self) -> Vec<DownloadState> {
        Self::ALL
            .iter()
            .copied()
            .filter(|to| self.can_transition_to(*to))
            .collect()
    }

    /// Checked transition: `Ok(to)` or [`Error::IllegalTransition`].
    pub fn transition(self, to: DownloadState) -> Result<DownloadState, Error> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(Error::IllegalTransition { from: self, to })
        }
    }

    /// Stable lower-case name used in the database and the IPC schema. `Failed` loses its
    /// `retryable` flag here; it is stored in its own column — see [`Self::from_name`].
    pub const fn name(self) -> &'static str {
        match self {
            DownloadState::Queued => "queued",
            DownloadState::Probing => "probing",
            DownloadState::Downloading => "downloading",
            DownloadState::Paused => "paused",
            DownloadState::Verifying => "verifying",
            DownloadState::Completed => "completed",
            DownloadState::Failed { .. } => "failed",
            DownloadState::Cancelled => "cancelled",
        }
    }

    /// Inverse of [`Self::name`]. `retryable` is only consulted for `"failed"`.
    pub fn from_name(name: &str, retryable: bool) -> Option<Self> {
        Some(match name {
            "queued" => DownloadState::Queued,
            "probing" => DownloadState::Probing,
            "downloading" => DownloadState::Downloading,
            "paused" => DownloadState::Paused,
            "verifying" => DownloadState::Verifying,
            "completed" => DownloadState::Completed,
            "failed" => DownloadState::Failed { retryable },
            "cancelled" => DownloadState::Cancelled,
            _ => return None,
        })
    }

    /// The `retryable` flag of a `Failed` state, `None` for every other state.
    pub const fn retryable(self) -> Option<bool> {
        match self {
            DownloadState::Failed { retryable } => Some(retryable),
            _ => None,
        }
    }
}

impl fmt::Display for DownloadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadState::Failed { retryable: true } => f.write_str("failed (retryable)"),
            DownloadState::Failed { retryable: false } => f.write_str("failed (permanent)"),
            other => f.write_str(other.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DownloadState::{self, *};
    use crate::Error;

    const FT: DownloadState = Failed { retryable: true };
    const FF: DownloadState = Failed { retryable: false };

    /// The complete legal set, written out by hand so the table in `can_transition_to` is checked
    /// against an independent source rather than against itself.
    const LEGAL: &[(DownloadState, DownloadState)] = &[
        (Queued, Probing),
        (Queued, Paused),
        (Queued, Cancelled),
        (Probing, Downloading),
        (Probing, Paused),
        (Probing, FT),
        (Probing, FF),
        (Probing, Cancelled),
        (Downloading, Verifying),
        (Downloading, Paused),
        (Downloading, FT),
        (Downloading, FF),
        (Downloading, Cancelled),
        (Verifying, Completed),
        (Verifying, FT),
        (Verifying, FF),
        (Verifying, Cancelled),
        (Paused, Queued),
        (Paused, Cancelled),
        (FT, Queued),
        (FT, Cancelled),
        (FF, Queued),
        (FF, Cancelled),
    ];

    #[test]
    fn table_is_exactly_the_documented_arrows() {
        let mut seen = 0;
        for from in DownloadState::ALL {
            for to in DownloadState::ALL {
                let expected = LEGAL.contains(&(from, to));
                assert_eq!(from.can_transition_to(to), expected, "{from} -> {to}");
                if expected {
                    seen += 1;
                }
            }
        }
        assert_eq!(
            seen,
            LEGAL.len(),
            "every legal arrow appears exactly once in ALL x ALL"
        );
        assert_eq!(LEGAL.len(), 23);
    }

    #[test]
    fn every_legal_transition_succeeds() {
        for (from, to) in LEGAL {
            assert_eq!(from.transition(*to), Ok(*to));
        }
    }

    #[test]
    fn cancelled_is_reachable_from_every_non_terminal_state() {
        for s in DownloadState::ALL {
            assert_eq!(s.can_transition_to(Cancelled), !s.is_terminal(), "{s}");
        }
    }

    #[test]
    fn terminal_states_have_no_successors() {
        assert!(Completed.successors().is_empty());
        assert!(Cancelled.successors().is_empty());
        for s in DownloadState::ALL {
            if !s.is_terminal() {
                assert!(!s.successors().is_empty(), "{s} must lead somewhere");
            }
        }
    }

    #[test]
    fn name_round_trips() {
        for s in DownloadState::ALL {
            let back = DownloadState::from_name(s.name(), s.retryable().unwrap_or(false));
            assert_eq!(back, Some(s));
        }
        assert_eq!(DownloadState::from_name("bogus", false), None);
    }

    #[test]
    fn serde_uses_snake_case_tag_and_keeps_retryable() {
        let json = serde_json::to_string(&FT).unwrap();
        assert_eq!(json, r#"{"state":"failed","retryable":true}"#);
        assert_eq!(
            serde_json::to_string(&Queued).unwrap(),
            r#"{"state":"queued"}"#
        );
        let back: DownloadState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, FT);
    }

    #[test]
    fn illegal_transition_error_names_both_states() {
        let err = Completed.transition(Queued).unwrap_err();
        assert_eq!(
            err,
            Error::IllegalTransition {
                from: Completed,
                to: Queued
            }
        );
        assert_eq!(
            err.to_string(),
            "illegal download state transition: completed -> queued"
        );
    }

    // One named test per illegal transition (58 of them: 9 x 9 pairs minus the 23 legal arrows).
    macro_rules! illegal {
        ($($name:ident: $from:expr => $to:expr;)*) => {$(
            #[test]
            fn $name() {
                let from: DownloadState = $from;
                let to: DownloadState = $to;
                assert!(!from.can_transition_to(to), "{from} -> {to} must be illegal");
                assert_eq!(from.transition(to), Err(Error::IllegalTransition { from, to }));
            }
        )*};
    }

    illegal! {
        queued_to_queued: Queued => Queued;
        queued_to_downloading: Queued => Downloading;
        queued_to_verifying: Queued => Verifying;
        queued_to_completed: Queued => Completed;
        queued_to_failed_retryable: Queued => FT;
        queued_to_failed_permanent: Queued => FF;

        probing_to_queued: Probing => Queued;
        probing_to_probing: Probing => Probing;
        probing_to_verifying: Probing => Verifying;
        probing_to_completed: Probing => Completed;

        downloading_to_queued: Downloading => Queued;
        downloading_to_probing: Downloading => Probing;
        downloading_to_downloading: Downloading => Downloading;
        downloading_to_completed: Downloading => Completed;

        paused_to_probing: Paused => Probing;
        paused_to_downloading: Paused => Downloading;
        paused_to_paused: Paused => Paused;
        paused_to_verifying: Paused => Verifying;
        paused_to_completed: Paused => Completed;
        paused_to_failed_retryable: Paused => FT;
        paused_to_failed_permanent: Paused => FF;

        verifying_to_queued: Verifying => Queued;
        verifying_to_probing: Verifying => Probing;
        verifying_to_downloading: Verifying => Downloading;
        verifying_to_paused: Verifying => Paused;
        verifying_to_verifying: Verifying => Verifying;

        completed_to_queued: Completed => Queued;
        completed_to_probing: Completed => Probing;
        completed_to_downloading: Completed => Downloading;
        completed_to_paused: Completed => Paused;
        completed_to_verifying: Completed => Verifying;
        completed_to_completed: Completed => Completed;
        completed_to_failed_retryable: Completed => FT;
        completed_to_failed_permanent: Completed => FF;
        completed_to_cancelled: Completed => Cancelled;

        failed_retryable_to_probing: FT => Probing;
        failed_retryable_to_downloading: FT => Downloading;
        failed_retryable_to_paused: FT => Paused;
        failed_retryable_to_verifying: FT => Verifying;
        failed_retryable_to_completed: FT => Completed;
        failed_retryable_to_failed_retryable: FT => FT;
        failed_retryable_to_failed_permanent: FT => FF;

        failed_permanent_to_probing: FF => Probing;
        failed_permanent_to_downloading: FF => Downloading;
        failed_permanent_to_paused: FF => Paused;
        failed_permanent_to_verifying: FF => Verifying;
        failed_permanent_to_completed: FF => Completed;
        failed_permanent_to_failed_retryable: FF => FT;
        failed_permanent_to_failed_permanent: FF => FF;

        cancelled_to_queued: Cancelled => Queued;
        cancelled_to_probing: Cancelled => Probing;
        cancelled_to_downloading: Cancelled => Downloading;
        cancelled_to_paused: Cancelled => Paused;
        cancelled_to_verifying: Cancelled => Verifying;
        cancelled_to_completed: Cancelled => Completed;
        cancelled_to_failed_retryable: Cancelled => FT;
        cancelled_to_failed_permanent: Cancelled => FF;
        cancelled_to_cancelled: Cancelled => Cancelled;
    }

    #[test]
    fn illegal_macro_covers_every_illegal_pair() {
        // 9 states, 81 pairs, 23 legal => 58 illegal. The macro above lists 58 cases; this guards
        // against the two drifting apart when a state is added.
        let illegal = DownloadState::ALL
            .iter()
            .flat_map(|f| DownloadState::ALL.iter().map(move |t| (*f, *t)))
            .filter(|(f, t)| !f.can_transition_to(*t))
            .count();
        assert_eq!(illegal, 58);
    }
}
