//! Queues as data: name, concurrency limit, order, schedule and what to do when one finishes.

use serde::{Deserialize, Serialize};

use crate::download::QueueId;
use crate::error::Error;
use crate::schedule::Schedule;

/// Most downloads a single queue may run at once.
pub const MAX_QUEUE_CONCURRENT: u32 = 32;
/// Longest queue name, in characters.
pub const MAX_QUEUE_NAME_CHARS: usize = 80;

/// What to do when the last unfinished item of a queue completes.
///
/// The engine only *announces* this ([`crate::Event::QueueFinished`]); the app layer performs
/// it (platform code, with the countdown dialog). `RunCommand` carries a command line the
/// *user* configured in the app's settings; nothing from the network ever reaches it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PostAction {
    /// Shut the computer down.
    Shutdown,
    /// Suspend to RAM.
    Sleep,
    /// Suspend to disk.
    Hibernate,
    /// Quit Riptide.
    Exit,
    /// Run a user-configured command line.
    RunCommand {
        /// The command line, as the user typed it.
        command: String,
    },
}

/// The editable part of a queue (everything but id, position and the on/off switch).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSpec {
    /// Display name, unique among queues.
    pub name: String,
    /// Downloads running at once in this queue (`1..=32`).
    pub max_concurrent: u32,
    /// When the queue may run.
    #[serde(default)]
    pub schedule: Schedule,
    /// What to announce when the queue drains.
    #[serde(default)]
    pub on_done: Option<PostAction>,
}

impl QueueSpec {
    /// A spec with no schedule and no post action.
    pub fn new(name: impl Into<String>, max_concurrent: u32) -> Self {
        QueueSpec {
            name: name.into(),
            max_concurrent,
            schedule: Schedule::default(),
            on_done: None,
        }
    }

    /// Trims the name and checks every field.
    pub fn validated(mut self) -> Result<Self, Error> {
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            return Err(Error::Invalid("queue name is empty".into()));
        }
        if self.name.chars().count() > MAX_QUEUE_NAME_CHARS
            || self.name.chars().any(char::is_control)
        {
            return Err(Error::Invalid(
                "queue name is too long or has control characters".into(),
            ));
        }
        if !(1..=MAX_QUEUE_CONCURRENT).contains(&self.max_concurrent) {
            return Err(Error::Invalid(format!(
                "max_concurrent must be 1..={MAX_QUEUE_CONCURRENT}"
            )));
        }
        self.schedule.validate()?;
        if let Some(PostAction::RunCommand { command }) = &mut self.on_done {
            *command = command.trim().to_string();
            if command.is_empty() {
                return Err(Error::Invalid("run_command needs a command".into()));
            }
        }
        Ok(self)
    }
}

/// A queue row.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    /// Row id; `1` is the built-in *Main* queue.
    pub id: QueueId,
    /// Display name.
    pub name: String,
    /// Maximum simultaneously active downloads.
    pub max_concurrent: u32,
    /// Sort order among queues.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub position: i64,
    /// Whether the queue is started. A stopped queue keeps its items `Queued` but starts none.
    pub enabled: bool,
    /// When the queue may run.
    pub schedule: Schedule,
    /// What to announce when the queue drains.
    pub on_done: Option<PostAction>,
}

impl Queue {
    /// The editable part.
    pub fn spec(&self) -> QueueSpec {
        QueueSpec {
            name: self.name.clone(),
            max_concurrent: self.max_concurrent,
            schedule: self.schedule,
            on_done: self.on_done.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{DaySet, Window};

    #[test]
    fn spec_validation() {
        assert_eq!(
            QueueSpec::new("  Night  ", 2).validated().unwrap().name,
            "Night"
        );
        assert!(QueueSpec::new("", 2).validated().is_err());
        assert!(QueueSpec::new("a\nb", 2).validated().is_err());
        assert!(QueueSpec::new("x".repeat(81), 2).validated().is_err());
        assert!(QueueSpec::new("q", 0).validated().is_err());
        assert!(QueueSpec::new("q", 33).validated().is_err());
        let mut bad = QueueSpec::new("q", 1);
        bad.schedule = Schedule {
            window: Some(Window {
                start_min: 5,
                stop_min: 5,
            }),
            days: DaySet::ALL,
            start_at: None,
        };
        assert!(bad.validated().is_err());
        let mut cmd = QueueSpec::new("q", 1);
        cmd.on_done = Some(PostAction::RunCommand {
            command: "  ".into(),
        });
        assert!(cmd.validated().is_err());
    }

    #[test]
    fn post_action_json_is_tagged() {
        let json = serde_json::to_value(PostAction::Shutdown).unwrap();
        assert_eq!(json["action"], "shutdown");
        let run = PostAction::RunCommand {
            command: "notify.exe".into(),
        };
        let back: PostAction = serde_json::from_value(serde_json::to_value(&run).unwrap()).unwrap();
        assert_eq!(back, run);
    }
}
