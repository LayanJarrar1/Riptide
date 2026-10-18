//! Which queued download starts next: pure, so the limits can be property-tested.
//!
//! Three limits apply at once: the global `max_active`, each queue's `max_concurrent`, and the
//! queue's started/stopped switch. Within a queue the order is priority (high first), then the
//! manual position, then the id. Queues are visited in their own order (`Queue::position`), so
//! when slots are scarce the first queue in the list gets them first.

use std::collections::HashMap;

use crate::download::{Download, DownloadId, QueueId};
use crate::queue::Queue;
use crate::state::DownloadState;

/// The `Queued` download that should start now, or `None` if nothing may start.
///
/// * `queues` — all queues in display order.
/// * `queued` — candidates; anything not `Queued` or already counted as running is ignored by
///   the caller before the call (this function trusts `state == Queued` only for sanity).
/// * `running_per_queue` — how many downloads of each queue are running right now (Probing,
///   Downloading or Verifying tasks, whatever the caller counts as occupying a slot).
/// * `max_active` — the global cap across all queues.
pub fn pick_next(
    queues: &[Queue],
    queued: &[Download],
    running_per_queue: &HashMap<QueueId, usize>,
    max_active: usize,
) -> Option<DownloadId> {
    let total: usize = running_per_queue.values().sum();
    if total >= max_active {
        return None;
    }
    for q in queues {
        if !q.enabled {
            continue;
        }
        let running = running_per_queue.get(&q.id).copied().unwrap_or(0);
        if running >= q.max_concurrent as usize {
            continue;
        }
        let best = queued
            .iter()
            .filter(|d| d.queue_id == q.id && d.state == DownloadState::Queued)
            .max_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then(b.queue_position.cmp(&a.queue_position))
                    .then(b.id.cmp(&a.id))
            });
        if let Some(d) = best {
            return Some(d.id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::Priority;
    use crate::schedule::Schedule;
    use proptest::prelude::*;

    fn queue(id: i64, max: u32, enabled: bool) -> Queue {
        Queue {
            id: QueueId(id),
            name: format!("q{id}"),
            max_concurrent: max,
            position: id,
            enabled,
            schedule: Schedule::default(),
            on_done: None,
        }
    }

    fn dl(id: i64, queue: i64, pos: i64, priority: Priority) -> Download {
        let mut d = Download::new(DownloadId(id), "https://x/f", "f", ".", id);
        d.queue_id = QueueId(queue);
        d.queue_position = pos;
        d.priority = priority;
        d
    }

    #[test]
    fn order_is_priority_then_position_then_id() {
        let qs = [queue(1, 3, true)];
        let ds = [
            dl(1, 1, 5, Priority::Normal),
            dl(2, 1, 2, Priority::Normal),
            dl(3, 1, 9, Priority::High),
            dl(4, 1, 1, Priority::Low),
        ];
        let none = HashMap::new();
        assert_eq!(pick_next(&qs, &ds, &none, 5), Some(DownloadId(3)));
        assert_eq!(pick_next(&qs, &ds[..2], &none, 5), Some(DownloadId(2)));
        assert_eq!(pick_next(&qs, &ds[3..], &none, 5), Some(DownloadId(4)));
    }

    #[test]
    fn stopped_and_full_queues_are_skipped() {
        let qs = [queue(1, 1, true), queue(2, 2, false), queue(3, 2, true)];
        let ds = [
            dl(1, 1, 1, Priority::Normal),
            dl(2, 2, 1, Priority::Normal),
            dl(3, 3, 1, Priority::Normal),
        ];
        let mut running = HashMap::new();
        assert_eq!(pick_next(&qs, &ds, &running, 9), Some(DownloadId(1)));
        running.insert(QueueId(1), 1);
        // queue 1 is full, queue 2 is stopped: queue 3 gets the slot
        assert_eq!(pick_next(&qs, &ds, &running, 9), Some(DownloadId(3)));
        // global cap reached
        assert_eq!(pick_next(&qs, &ds, &running, 1), None);
    }

    proptest! {
        /// Simulate a scheduler: start whatever `pick_next` offers, finish random runners, and
        /// check the limits after every step.
        #[test]
        fn limits_hold_through_any_run(
            queue_specs in proptest::collection::vec((1u32..5, any::<bool>()), 1..5),
            items in proptest::collection::vec((0usize..5, 0u8..3), 0..40),
            max_active in 1usize..8,
            finishes in proptest::collection::vec(any::<u16>(), 0..80),
        ) {
            let queues: Vec<Queue> = queue_specs
                .iter()
                .enumerate()
                .map(|(i, (max, on))| queue(i as i64 + 1, *max, *on))
                .collect();
            let mut waiting: Vec<Download> = items
                .iter()
                .enumerate()
                .map(|(i, (q, p))| {
                    let prio = match p { 0 => Priority::Low, 1 => Priority::Normal, _ => Priority::High };
                    dl(i as i64 + 1, (q % queues.len()) as i64 + 1, i as i64, prio)
                })
                .collect();
            let mut running: Vec<(DownloadId, QueueId)> = Vec::new();
            let mut finishes = finishes.into_iter();
            loop {
                // fill every free slot
                loop {
                    let mut per: HashMap<QueueId, usize> = HashMap::new();
                    for (_, q) in &running { *per.entry(*q).or_default() += 1; }
                    let Some(id) = pick_next(&queues, &waiting, &per, max_active) else { break };
                    let pos = waiting.iter().position(|d| d.id == id).unwrap();
                    let d = waiting.remove(pos);
                    let q = queues.iter().find(|q| q.id == d.queue_id).unwrap();
                    prop_assert!(q.enabled, "started from a stopped queue");
                    // the chosen one is the best of its queue
                    for other in waiting.iter().filter(|o| o.queue_id == d.queue_id) {
                        prop_assert!(
                            (d.priority, std::cmp::Reverse(d.queue_position))
                                >= (other.priority, std::cmp::Reverse(other.queue_position))
                        );
                    }
                    running.push((d.id, d.queue_id));
                    prop_assert!(running.len() <= max_active, "global max_active exceeded");
                    let in_queue = running.iter().filter(|(_, qq)| *qq == q.id).count();
                    prop_assert!(in_queue <= q.max_concurrent as usize, "queue limit exceeded");
                }
                if running.is_empty() {
                    break;
                }
                let k = finishes.next().unwrap_or(0) as usize % running.len();
                running.remove(k);
            }
            // nothing can start any more: only stopped queues still hold work
            for d in &waiting {
                let q = queues.iter().find(|q| q.id == d.queue_id).unwrap();
                prop_assert!(!q.enabled, "a runnable item was left waiting");
            }
        }
    }
}
