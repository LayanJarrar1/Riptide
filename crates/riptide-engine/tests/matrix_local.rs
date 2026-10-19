//! Download matrix: local failure modes (permissions, locks, long paths, vanishing folders,
//! cancel at every state, pause/resume at random offsets).
//!
//! Disk-full is not simulated here: there is no write-injection seam in the engine and creating
//! a size-limited volume needs administrator rights (see `tests/MATRIX.md`).
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use riptide_core::{DownloadRequest, DownloadState, Event};
use riptide_testserver::{splitmix64, DEFAULT_SEED};

const T: Duration = Duration::from_secs(120);

/// Makes `dir` refuse new files for the current user; restores it on drop.
struct ReadOnlyDir(PathBuf);

impl ReadOnlyDir {
    #[cfg(windows)]
    fn new(dir: &Path) -> Self {
        std::fs::create_dir_all(dir).unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let st = std::process::Command::new("icacls")
            .arg(dir)
            .args(["/deny", &format!("{user}:(OI)(CI)(WD,AD)")])
            .output()
            .unwrap();
        assert!(st.status.success(), "icacls: {st:?}");
        ReadOnlyDir(dir.to_path_buf())
    }

    #[cfg(unix)]
    fn new(dir: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        ReadOnlyDir(dir.to_path_buf())
    }
}

impl Drop for ReadOnlyDir {
    #[cfg(windows)]
    fn drop(&mut self) {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let _ = std::process::Command::new("icacls")
            .arg(&self.0)
            .args(["/remove:d", &user])
            .output();
    }

    #[cfg(unix)]
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_only_destination_fails_cleanly_then_works_once_writable() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let locked = w.dir.path().join("locked");
    let size = 200_000;
    let id;
    {
        let _guard = ReadOnlyDir::new(&locked);
        id = engine
            .add(DownloadRequest {
                dir: Some(locked.clone()),
                filename: Some("a.bin".into()),
                ..w.request(&format!("/ok?size={size}"))
            })
            .await
            .unwrap();
        wait_failed(&mut rx, id, T).await;
        assert!(!locked.join("a.bin").exists());
        let rec = engine.get(id).await.unwrap().unwrap();
        assert!(
            rec.error.is_some(),
            "the user must be told why: {:?}",
            rec.error
        );
    } // permissions restored
    engine.resume(id).await.unwrap();
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(&engine, id, &p, &locked, DEFAULT_SEED, size).await;
    engine.shutdown().await;
}

/// A file the user's antivirus/another program holds open without sharing: the part file is
/// locked before the download starts.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn destination_locked_by_another_handle_fails_or_avoids_it_then_recovers() {
    use std::os::windows::fs::OpenOptionsExt;

    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    std::fs::create_dir_all(w.downloads()).unwrap();
    let part = w.downloads().join("locked.bin.rtpart");
    let fin = w.downloads().join("locked.bin");
    let size = 300_000;

    // exclusive handle on the part file name (share mode 0)
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .share_mode(0)
        .open(&part)
        .unwrap();
    let id = engine
        .add(DownloadRequest {
            filename: Some("locked.bin".into()),
            ..w.request(&format!("/ok?size={size}"))
        })
        .await
        .unwrap();
    // either the engine picks another name, or it fails with an error: never a hang, never a
    // Completed with wrong bytes
    let ev = wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Completed { id: i, .. } | Event::Failed { id: i, .. } if *i == id),
    )
    .await;
    match ev {
        Event::Completed { path, .. } => {
            eprintln!(
                "line {}: Completed as {}",
                line!(),
                path.file_name().unwrap().to_string_lossy()
            );
            assert_ne!(path, fin, "must not have used the locked name");
            assert_complete(&engine, id, &path, &w.downloads(), DEFAULT_SEED, size).await;
        }
        Event::Failed { .. } => {
            eprintln!(
                "line {}: Failed: {:?}",
                line!(),
                engine.get(id).await.unwrap().unwrap().error
            );
            assert!(!fin.exists());
            drop(held);
            engine.resume(id).await.unwrap();
            let p = wait_completed_in(&mut rx, id, T).await;
            std::fs::remove_file(&part).ok();
            assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
        }
        other => panic!("{other:?}"),
    }
    engine.shutdown().await;
}

/// The final name is taken by a locked file at the moment of the rename: the download must not
/// lose its bytes or report Completed for a file that is not there.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn final_name_appearing_locked_before_the_rename_never_corrupts_or_lies() {
    use std::os::windows::fs::OpenOptionsExt;

    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 3 * MIB;
    let fin = w.downloads().join("race.bin");
    let id = engine
        .add(DownloadRequest {
            filename: Some("race.bin".into()),
            ..w.request(&format!("/slow?bps={}&size={size}", MIB))
        })
        .await
        .unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0),
    )
    .await;
    // someone creates the final name and holds it exclusively
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .share_mode(0)
        .open(&fin)
        .unwrap();
    let ev = wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Completed { id: i, .. } | Event::Failed { id: i, .. } if *i == id),
    )
    .await;
    match ev {
        Event::Completed { path, .. } => {
            eprintln!(
                "line {}: Completed as {}",
                line!(),
                path.file_name().unwrap().to_string_lossy()
            );
            // acceptable only if the engine moved to a different name with the right bytes
            assert_ne!(path, fin);
            assert_complete(&engine, id, &path, &w.downloads(), DEFAULT_SEED, size).await;
        }
        Event::Failed { .. } => {
            eprintln!(
                "line {}: Failed: {:?}",
                line!(),
                engine.get(id).await.unwrap().unwrap().error
            );
            // the bytes are not lost: after the other program lets go and removes its file,
            // resume finishes byte-exact
            drop(held);
            std::fs::remove_file(&fin).unwrap();
            engine.resume(id).await.unwrap();
            let p = wait_completed_in(&mut rx, id, T).await;
            assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
        }
        other => panic!("{other:?}"),
    }
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn very_long_paths_complete_or_fail_cleanly() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    // 20 nested folders of 20 chars: > 400 chars in total, beyond the classic 260 limit
    let mut deep = w.dir.path().to_path_buf();
    for i in 0..20 {
        deep.push(format!("folder-number-{i:05}"));
    }
    let size = 50_000;
    let id = engine
        .add(DownloadRequest {
            dir: Some(deep.clone()),
            filename: Some(format!("{}.bin", "n".repeat(190))),
            ..w.request(&format!("/ok?size={size}"))
        })
        .await
        .unwrap();
    let ev = wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Completed { id: i, .. } | Event::Failed { id: i, .. } if *i == id),
    )
    .await;
    match ev {
        Event::Completed { path, .. } => {
            eprintln!(
                "line {}: Completed as {}",
                line!(),
                path.file_name().unwrap().to_string_lossy()
            );
            assert_complete(&engine, id, &path, &deep, DEFAULT_SEED, size).await;
        }
        Event::Failed { .. } => {
            eprintln!(
                "line {}: Failed: {:?}",
                line!(),
                engine.get(id).await.unwrap().unwrap().error
            );
            let rec = engine.get(id).await.unwrap().unwrap();
            assert!(rec.error.is_some());
            assert!(!rec.final_path().exists());
        }
        other => panic!("{other:?}"),
    }
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_removed_mid_download_never_completes_wrongly_and_recovers() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 4 * MIB;
    let id = engine
        .add(w.request(&format!("/slow?bps={}&size={size}", MIB)))
        .await
        .unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 0),
    )
    .await;
    // the folder vanishes while the transfer runs (user deletes it, drive unplugged)
    let removed = std::fs::remove_dir_all(w.downloads());
    let ev = wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Completed { id: i, .. } | Event::Failed { id: i, .. } if *i == id),
    )
    .await;
    match ev {
        Event::Completed { path, .. } => {
            eprintln!(
                "line {}: Completed as {}",
                line!(),
                path.file_name().unwrap().to_string_lossy()
            );
            // only legitimate if the file really is there and correct
            assert!(
                path.exists(),
                "Completed for a path that does not exist (folder removal: {removed:?})"
            );
            assert_complete(&engine, id, &path, &w.downloads(), DEFAULT_SEED, size).await;
        }
        Event::Failed { .. } => {
            eprintln!(
                "line {}: Failed: {:?}",
                line!(),
                engine.get(id).await.unwrap().unwrap().error
            );
            // restart from zero into the recreated folder
            std::fs::create_dir_all(w.downloads()).unwrap();
            engine.restart(id).await.unwrap();
            let p = wait_completed_in(&mut rx, id, T).await;
            assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
        }
        other => panic!("{other:?}"),
    }
    engine.shutdown().await;
}

fn dir_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_none())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_in_every_state_leaves_no_files() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut s = engine.settings();
    s.max_active = 1;
    engine.set_settings(s).await.unwrap();
    let mut rx = engine.subscribe();
    let size = 6 * MIB;

    // Downloading
    let running = engine
        .add(DownloadRequest {
            filename: Some("running.bin".into()),
            ..w.request(&format!("/slow?bps={}&size={size}", MIB))
        })
        .await
        .unwrap();
    wait_for(&mut rx, T, |e| {
        matches!(e, Event::Progress { id: i, downloaded, .. } if *i == running && *downloaded > 0)
    })
    .await;
    // Queued (max_active = 1 keeps it waiting)
    let queued = engine
        .add(DownloadRequest {
            filename: Some("queued.bin".into()),
            ..w.request("/ok?size=1000")
        })
        .await
        .unwrap();
    assert_eq!(
        engine.get(queued).await.unwrap().unwrap().state,
        DownloadState::Queued
    );
    engine.cancel(queued).await.unwrap();
    assert_eq!(
        engine.get(queued).await.unwrap().unwrap().state,
        DownloadState::Cancelled
    );
    // Paused
    engine.pause(running).await.unwrap();
    wait_state(&mut rx, running, DownloadState::Paused, T).await;
    assert!(w.downloads().join("running.bin.rtpart").exists());
    engine.cancel(running).await.unwrap();
    wait_state(&mut rx, running, DownloadState::Cancelled, T).await;
    // Downloading again, then cancelled while running
    let again = engine
        .add(DownloadRequest {
            filename: Some("again.bin".into()),
            ..w.request(&format!("/slow?bps={}&size={size}", MIB))
        })
        .await
        .unwrap();
    wait_for(&mut rx, T, |e| {
        matches!(e, Event::Progress { id: i, downloaded, .. } if *i == again && *downloaded > 0)
    })
    .await;
    engine.cancel(again).await.unwrap();
    wait_state(&mut rx, again, DownloadState::Cancelled, T).await;
    // Failed
    let failed = engine
        .add(DownloadRequest {
            filename: Some("failed.bin".into()),
            ..w.request("/wrong-length?delta=100&size=100000")
        })
        .await
        .unwrap();
    wait_failed(&mut rx, failed, Duration::from_secs(180)).await;
    engine.cancel(failed).await.unwrap();
    wait_state(&mut rx, failed, DownloadState::Cancelled, T).await;
    // Completed: cancel is a no-op, the file stays
    let done = engine
        .add(DownloadRequest {
            filename: Some("done.bin".into()),
            ..w.request("/ok?size=5000")
        })
        .await
        .unwrap();
    let p = wait_completed_in(&mut rx, done, T).await;
    engine.cancel(done).await.unwrap();
    assert_eq!(
        engine.get(done).await.unwrap().unwrap().state,
        DownloadState::Completed
    );
    assert!(p.exists());
    // only the finished file remains
    let left: Vec<_> = std::fs::read_dir(w.downloads())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["done.bin"], "leftovers: {left:?}");
    let _ = dir_is_empty;
    engine.shutdown().await;
}

/// Property-style test: pause and resume at pseudo-random moments, many times, on a download
/// whose bytes are checked at the end. The seed is printed; set `RIPTIDE_SEED` to replay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_at_random_offsets_is_byte_exact() {
    let seed: u64 = std::env::var("RIPTIDE_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        });
    eprintln!("pause/resume seed = {seed} (RIPTIDE_SEED={seed} to replay)");
    let mut rng = seed;
    let mut next = move |bound: u64| {
        rng = splitmix64(rng);
        rng % bound
    };
    for round in 0..4 {
        let w = World::new().await;
        let engine = w.engine().await;
        let mut rx = engine.subscribe();
        let size = 3 * MIB + next(MIB);
        let connections = 1 + next(6) as u32;
        let id = engine
            .add(DownloadRequest {
                connections: Some(connections),
                ..w.request(&format!("/slow?bps={}&size={size}", 3 * MIB))
            })
            .await
            .unwrap();
        for _ in 0..(2 + next(4)) {
            tokio::time::sleep(Duration::from_millis(30 + next(400))).await;
            engine.pause(id).await.unwrap();
            // the pause may race with completion
            let rec = engine.get(id).await.unwrap().unwrap();
            if rec.state == DownloadState::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(next(100))).await;
            engine.resume(id).await.unwrap();
        }
        let p = wait_completed_in(&mut rx, id, T).await;
        assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
        eprintln!("round {round}: {size} bytes, {connections} connections ok");
        engine.shutdown().await;
    }
}

/// `pause` returns when the transfer has really stopped: the record is `Paused` at once and an
/// immediate `resume` is never swallowed (it used to be ignored while the state was still
/// `Downloading`, leaving the download paused for good).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_returns_paused_and_an_immediate_resume_is_not_lost() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 4 * MIB;
    let id = engine
        .add(w.request(&format!("/slow?bps={MIB}&size={size}")))
        .await
        .unwrap();
    for _ in 0..10 {
        wait_for(
            &mut rx,
            T,
            |e| matches!(e, Event::Progress { id: i, .. } if *i == id),
        )
        .await;
        engine.pause(id).await.unwrap();
        let state = engine.get(id).await.unwrap().unwrap().state;
        if state == DownloadState::Completed {
            break; // finished before this pause: nothing left to test
        }
        assert_eq!(state, DownloadState::Paused);
        engine.resume(id).await.unwrap();
    }
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(&engine, id, &p, &w.downloads(), DEFAULT_SEED, size).await;
    engine.shutdown().await;
}

/// `refresh_url` racing a `pause` must never leave the record `Downloading` with no task.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_during_pause_leaves_a_consistent_state() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 4 * MIB;
    for round in 0..4 {
        let mut rx = engine.subscribe();
        let path = format!("/slow?bps={MIB}&size={size}&seed={}", 300 + round);
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("refresh{round}.bin")),
                ..w.request(&path)
            })
            .await
            .unwrap();
        wait_for(
            &mut rx,
            T,
            |e| matches!(e, Event::Progress { id: i, .. } if *i == id),
        )
        .await;
        let fresh_url = w.server.url(&path);
        let (a, b) = tokio::join!(
            engine.pause(id),
            engine.refresh_url(id, &fresh_url, None, None)
        );
        a.unwrap();
        b.unwrap();
        // consistent: either a live task owns a "Downloading" record, or nothing claims to run
        tokio::time::sleep(Duration::from_millis(300)).await;
        let state = engine.get(id).await.unwrap().unwrap().state;
        let has_task = engine.running_ids().contains(&id);
        assert!(
            !(matches!(state, DownloadState::Downloading | DownloadState::Probing) && !has_task),
            "round {round}: {state:?} with no task"
        );
        if state == DownloadState::Paused {
            engine.resume(id).await.unwrap();
        }
        let p = wait_completed_in(&mut rx, id, T).await;
        assert_complete(&engine, id, &p, &w.downloads(), 300 + round, size).await;
    }
    engine.shutdown().await;
}

/// `remove` while a pause is in flight waits for the old task; a same-name download added at
/// once afterwards is byte-exact and does not share a part file with the removed one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_while_pause_in_flight_then_same_name_add_is_byte_exact() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 3 * MIB;
    let mut rx = engine.subscribe();
    let first = engine
        .add(DownloadRequest {
            filename: Some("same.bin".into()),
            ..w.request(&format!("/slow?bps={MIB}&size={size}&seed=11"))
        })
        .await
        .unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Progress { id: i, .. } if *i == first),
    )
    .await;
    let (a, b) = tokio::join!(engine.pause(first), engine.remove(first, true));
    a.unwrap();
    b.unwrap();
    assert!(!engine.running_ids().contains(&first));
    let second = engine
        .add(DownloadRequest {
            filename: Some("same.bin".into()),
            ..w.request(&format!("/ok?size={size}&seed=22"))
        })
        .await
        .unwrap();
    let p = wait_completed_in(&mut rx, second, T).await;
    assert_eq!(p.file_name().unwrap(), "same.bin");
    assert_complete(&engine, second, &p, &w.downloads(), 22, size).await;
    engine.shutdown().await;
}

/// `pause_all` with a long queue pauses the queued items without ever starting (probing) one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_all_with_a_long_queue_never_probes_queued_items() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let mut ids = Vec::new();
    for i in 0..40 {
        ids.push(
            engine
                .add(DownloadRequest {
                    filename: Some(format!("q{i}.bin")),
                    ..w.request(&format!("/slow?bps={MIB}&size={}&seed={i}", 8 * MIB))
                })
                .await
                .unwrap(),
        );
    }
    wait_for(&mut rx, T, |e| matches!(e, Event::Progress { .. })).await;
    let before: std::collections::HashSet<_> = engine.running_ids().into_iter().collect();
    assert!(!before.is_empty() && before.len() < 40);
    let mut late = engine.subscribe();
    engine.pause_all().await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let mut started_later = 0;
    while let Ok(ev) = late.try_recv() {
        if let Event::Probed { id, .. } = ev {
            if !before.contains(&id) {
                started_later += 1;
            }
        }
    }
    assert_eq!(
        started_later, 0,
        "queued items were probed during pause_all"
    );
    assert!(engine.running_ids().is_empty());
    for id in ids {
        assert_eq!(
            engine.get(id).await.unwrap().unwrap().state,
            DownloadState::Paused
        );
    }
    engine.shutdown().await;
}

/// A server-chosen name ending in `.rtpart` never lands on another download's part file, and
/// the same folder spelled with another case (Windows) still counts as the same folder.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rtpart_names_are_reserved_and_folder_spelling_does_not_split_claims() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let a = engine
        .add(DownloadRequest {
            filename: Some("x.zip".into()),
            start_paused: true,
            ..w.request("/ok?size=50000&seed=1")
        })
        .await
        .unwrap();
    let b = engine
        .add(w.request("/ok?size=60000&seed=2"))
        .await
        .unwrap();
    // b's name is chosen by the server/URL; give it the dangerous one via the request
    engine.remove(b, true).await.unwrap();
    let b = engine
        .add(DownloadRequest {
            filename: Some("x.zip.rtpart".into()),
            ..w.request("/ok?size=60000&seed=2")
        })
        .await
        .unwrap();
    let pb = wait_completed_in(&mut rx, b, T).await;
    assert!(!pb.to_string_lossy().to_lowercase().ends_with(".rtpart"));
    assert_complete(&engine, b, &pb, &w.downloads(), 2, 60000).await;
    if cfg!(windows) {
        let upper = PathBuf::from(w.downloads().to_string_lossy().to_uppercase());
        let c = engine
            .add(DownloadRequest {
                filename: Some("x.zip".into()),
                dir: Some(upper),
                ..w.request("/ok?size=70000&seed=3")
            })
            .await
            .unwrap();
        let pc = wait_completed_in(&mut rx, c, T).await;
        assert_ne!(
            pc.file_name().unwrap().to_string_lossy().to_lowercase(),
            "x.zip",
            "the paused download's claim on x.zip was ignored"
        );
    }
    engine.resume(a).await.unwrap();
    let pa = wait_completed_in(&mut rx, a, T).await;
    assert_complete(&engine, a, &pa, &w.downloads(), 1, 50000).await;
    engine.shutdown().await;
}

/// A stop that exceeds the wait must not leave the record `Downloading` with no task: the entry
/// is dropped, the record is `Paused`, the caller gets an error and a retry works.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stuck_stop_times_out_into_a_paused_record_and_recovers() {
    let w = World::new().await;
    let engine = w.engine().await;
    let mut rx = engine.subscribe();
    let size = 4 * MIB;
    let id = engine
        .add(w.request(&format!("/slow?bps={MIB}&size={size}&seed=5")))
        .await
        .unwrap();
    wait_for(
        &mut rx,
        T,
        |e| matches!(e, Event::Progress { id: i, .. } if *i == id),
    )
    .await;
    engine.set_test_stop_knobs(200, 5_000);
    assert!(engine.pause(id).await.is_err(), "the stop should time out");
    assert!(!engine.running_ids().contains(&id));
    assert_eq!(
        engine.get(id).await.unwrap().unwrap().state,
        DownloadState::Paused
    );
    // remove on a stuck task behaves the same way and then works
    engine.set_test_stop_knobs(0, 0);
    engine.resume(id).await.unwrap();
    let p = wait_completed_in(&mut rx, id, T).await;
    assert_complete(&engine, id, &p, &w.downloads(), 5, size).await;
    engine.shutdown().await;
}

/// The `.rtpart` deleted, truncated or extended between a pause and the resume: the engine
/// starts over instead of declaring a zero-filled file complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn damaged_part_file_between_pause_and_resume_restarts_byte_exact() {
    let w = World::new().await;
    let engine = w.engine().await;
    let size = 4 * MIB;
    for (round, what) in ["delete", "truncate", "extend"].into_iter().enumerate() {
        let mut rx = engine.subscribe();
        let id = engine
            .add(DownloadRequest {
                filename: Some(format!("dmg{round}.bin")),
                ..w.request(&format!("/slow?bps={MIB}&size={size}&seed=9"))
            })
            .await
            .unwrap();
        wait_for(&mut rx, T, |e| {
            matches!(e, Event::Progress { id: i, downloaded, .. } if *i == id && *downloaded > 300_000)
        })
        .await;
        engine.pause(id).await.unwrap();
        let rec = engine.get(id).await.unwrap().unwrap();
        let part = riptide_net::part_path_for(&rec.final_path());
        match what {
            "delete" => std::fs::remove_file(&part).unwrap(),
            "truncate" => std::fs::OpenOptions::new()
                .write(true)
                .open(&part)
                .unwrap()
                .set_len(size / 2)
                .unwrap(),
            _ => std::fs::OpenOptions::new()
                .write(true)
                .open(&part)
                .unwrap()
                .set_len(size + 8192)
                .unwrap(),
        }
        engine.resume(id).await.unwrap();
        let p = wait_completed_in(&mut rx, id, T).await;
        assert_complete(&engine, id, &p, &w.downloads(), 9, size).await;
    }
    engine.shutdown().await;
}
