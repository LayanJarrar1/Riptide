//! Queue commands: thin wrappers over the engine's queue API, plus the post-queue action.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `list_queues` | – | every `Queue` in display order |
//! | `queue_items` | `id` | the queue's `Download`s in their manual order |
//! | `create_queue` | `spec: QueueSpec` | new queue id |
//! | `update_queue` | `id`, `spec` | the stored `Queue` |
//! | `delete_queue` | `id` | – (its items move to Main) |
//! | `move_to_queue` | `id`, `queue` | – |
//! | `start_queue` / `stop_queue` | `id` | – |
//! | `reorder_queue_items` | `queue`, `order` (download ids) | – |
//! | `reorder_queues` | `order` (queue ids) | – |
//! | `validate_post_action` | `action` | – / why it cannot run |
//! | `run_post_action` | `queueId` | – (performs the queue's saved action, see [`crate::power`]) |
//!
//! Errors are plain strings, already redacted by the engine. The engine's events
//! (`queue_created`, `queue_updated`, ...) reach the webview through the same
//! `riptide://event` stream as every other event.

use riptide_core::{Download, DownloadId, PostAction, Queue, QueueId, QueueSpec};
use riptide_engine::Engine;
use tauri::State;

use crate::power::{self, SystemPower};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn queue_ids(ids: Vec<i64>) -> Vec<QueueId> {
    ids.into_iter().map(QueueId).collect()
}

fn download_ids(ids: Vec<i64>) -> Vec<DownloadId> {
    ids.into_iter().map(DownloadId).collect()
}

/// Every queue, in display order (Main first unless the user moved it).
#[tauri::command]
pub async fn list_queues(engine: State<'_, Engine>) -> CmdResult<Vec<Queue>> {
    engine.queues().await.map_err(err)
}

/// The downloads of one queue in their manual order.
#[tauri::command]
pub async fn queue_items(engine: State<'_, Engine>, id: i64) -> CmdResult<Vec<Download>> {
    engine.queue_items(QueueId(id)).await.map_err(err)
}

/// Creates a started queue; returns its id.
#[tauri::command]
pub async fn create_queue(engine: State<'_, Engine>, spec: QueueSpec) -> CmdResult<i64> {
    power::validate_spec(&spec)?;
    engine.create_queue(spec).await.map(|id| id.0).map_err(err)
}

/// Replaces a queue's name, limit, schedule and post action.
#[tauri::command]
pub async fn update_queue(engine: State<'_, Engine>, id: i64, spec: QueueSpec) -> CmdResult<Queue> {
    power::validate_spec(&spec)?;
    engine.update_queue(QueueId(id), spec).await.map_err(err)
}

/// Deletes a queue; its downloads go to the end of Main.
#[tauri::command]
pub async fn delete_queue(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.delete_queue(QueueId(id)).await.map_err(err)
}

/// Moves a download to the end of another queue.
#[tauri::command]
pub async fn move_to_queue(engine: State<'_, Engine>, id: i64, queue: i64) -> CmdResult<()> {
    engine
        .move_to_queue(DownloadId(id), QueueId(queue))
        .await
        .map_err(err)
}

/// Starts a queue.
#[tauri::command]
pub async fn start_queue(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.start_queue(QueueId(id)).await.map_err(err)
}

/// Stops a queue (running items go back to waiting).
#[tauri::command]
pub async fn stop_queue(engine: State<'_, Engine>, id: i64) -> CmdResult<()> {
    engine.stop_queue(QueueId(id)).await.map_err(err)
}

/// Puts a queue's downloads in the given order.
#[tauri::command]
pub async fn reorder_queue_items(
    engine: State<'_, Engine>,
    queue: i64,
    order: Vec<i64>,
) -> CmdResult<()> {
    engine
        .reorder(QueueId(queue), &download_ids(order))
        .await
        .map_err(err)
}

/// Puts the queues in the given order.
#[tauri::command]
pub async fn reorder_queues(engine: State<'_, Engine>, order: Vec<i64>) -> CmdResult<()> {
    engine.reorder_queues(&queue_ids(order)).await.map_err(err)
}

/// Checks that a post-queue action could run (a "Run a command" program must exist).
#[tauri::command]
pub fn validate_post_action(action: PostAction) -> CmdResult<()> {
    power::validate(&action)
}

/// Performs the post-queue action **saved on the queue** once the webview's countdown ended.
/// The action is read from the engine, not taken from the caller. Exit stops the transfers at
/// a checkpoint first (they resume on the next start).
#[tauri::command]
pub async fn run_post_action(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    queue_id: i64,
) -> CmdResult<()> {
    let queue = engine
        .queue(QueueId(queue_id))
        .await
        .map_err(err)?
        .ok_or_else(|| "the queue no longer exists".to_string())?;
    let action = queue
        .on_done
        .ok_or_else(|| "the queue has no action set".to_string())?;
    tracing::info!(
        queue = queue_id,
        action = action_name(&action),
        "performing the post-queue action"
    );
    if power::stops_engine(&action) {
        engine.inner().clone().shutdown().await;
    }
    tauri::async_runtime::spawn_blocking(move || power::perform(&SystemPower::new(app), &action))
        .await
        .map_err(err)?
}

/// The action's kind for the log: the command line itself may hold a path the user typed, so
/// it is never logged.
fn action_name(action: &PostAction) -> &'static str {
    match action {
        PostAction::Shutdown => "shutdown",
        PostAction::Sleep => "sleep",
        PostAction::Hibernate => "hibernate",
        PostAction::Exit => "exit",
        PostAction::RunCommand { .. } => "run_command",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_wrapped_in_order() {
        assert_eq!(
            queue_ids(vec![3, 1, 2]),
            [QueueId(3), QueueId(1), QueueId(2)]
        );
        assert_eq!(download_ids(vec![9, 4]), [DownloadId(9), DownloadId(4)]);
    }

    #[test]
    fn the_log_never_sees_the_command_line() {
        let action = PostAction::RunCommand {
            command: "C:\\secret\\tool.exe".into(),
        };
        assert_eq!(action_name(&action), "run_command");
    }
}
