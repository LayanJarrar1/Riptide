//! `riptide torrent ...`: BitTorrent through `riptide-torrent` (hidden from `--help` until the
//! feature ships).
//!
//! `add` and `resume` run the engine in the foreground until the download is complete and every
//! piece is verified; Ctrl-C stops them and keeps the state, so running `resume` again continues.
//! `list`, `pause` and `remove` only read and edit the state files: no sockets are opened.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use riptide_torrent::{
    offline, AddOptions, SeedingPolicy, TorrentEngine, TorrentSettings, TorrentSnapshot,
    TorrentSource, TorrentState,
};

use crate::fetch::{human_bytes, human_speed};
use crate::i18n::{t, tf, Lang};

/// `riptide torrent <command>`.
#[derive(Debug, clap::Subcommand)]
pub enum TorrentCmd {
    /// Add a magnet link or a .torrent file and download it.
    Add {
        /// A magnet link or the path of a .torrent file.
        source: String,
        /// Folder to download into (default: the current folder).
        #[arg(short, long, value_name = "DIR")]
        dir: Option<PathBuf>,
        /// File numbers to download, e.g. 0,2 (see --list).
        #[arg(long, value_delimiter = ',', value_name = "N,N")]
        select: Vec<usize>,
        /// Only show the files in the torrent.
        #[arg(long)]
        list: bool,
        /// Download the selected files one after the other.
        #[arg(long)]
        sequential: bool,
        /// Keep seeding after the download until the ratio 1.0 (off by default).
        #[arg(long)]
        seed: bool,
        /// Upload limit in kB/s.
        #[arg(long, value_name = "KBPS")]
        upload_limit: Option<u32>,
        /// A peer to try first (ip:port); repeatable.
        #[arg(long, value_name = "ADDR")]
        peer: Vec<SocketAddr>,
    },
    /// Show the torrents.
    List,
    /// Mark a torrent paused (it will not start with the next run).
    Pause {
        /// Torrent id (the start of the info-hash is enough).
        id: String,
    },
    /// Continue a torrent until it is complete.
    Resume {
        /// Torrent id (the start of the info-hash is enough).
        id: String,
        /// Upload limit in kB/s.
        #[arg(long, value_name = "KBPS")]
        upload_limit: Option<u32>,
    },
    /// Forget a torrent.
    Remove {
        /// Torrent id (the start of the info-hash is enough).
        id: String,
        /// Also delete the downloaded files.
        #[arg(long)]
        delete_files: bool,
    },
}

fn settings(state_dir: &Path, dir: &Path) -> TorrentSettings {
    TorrentSettings::new(state_dir.to_path_buf(), dir.to_path_buf())
}

fn source_of(raw: &str) -> TorrentSource {
    if raw.trim_start().to_ascii_lowercase().starts_with("magnet:") {
        TorrentSource::Magnet(raw.trim().to_string())
    } else {
        TorrentSource::File(PathBuf::from(raw))
    }
}

/// Runs a torrent command. `data_dir` is the Riptide data directory.
pub async fn run(cmd: TorrentCmd, data_dir: &Path, lang: Lang) -> anyhow::Result<()> {
    let state_dir = data_dir.join("torrents-state");
    match cmd {
        TorrentCmd::List => {
            let all = offline::list(&state_dir)?;
            if all.is_empty() {
                println!("{}", t(lang, "torrent.none"));
            }
            for s in all {
                println!(
                    "{}  {:<10} {:>3}%  {:>10}  {}",
                    &s.id[..12],
                    state_name(&s.state),
                    (s.fraction() * 100.0) as u32,
                    human_bytes(s.total_bytes),
                    s.name
                );
            }
            Ok(())
        }
        TorrentCmd::Pause { id } => {
            let id = offline::resolve_id(&state_dir, &id)?;
            offline::set_paused(&state_dir, &id, true)?;
            println!("{}", t(lang, "torrent.paused"));
            Ok(())
        }
        TorrentCmd::Remove { id, delete_files } => {
            let id = offline::resolve_id(&state_dir, &id)?;
            offline::remove(&state_dir, &id, delete_files)?;
            println!("{}", t(lang, "torrent.removed"));
            Ok(())
        }
        TorrentCmd::Add {
            source,
            dir,
            select,
            list,
            sequential,
            seed,
            upload_limit,
            peer,
        } => {
            let dir = match dir {
                Some(d) => d,
                None => std::env::current_dir()?,
            };
            let mut s = settings(&state_dir, &dir);
            s.upload_limit_bps = upload_limit.map(|k| k.saturating_mul(1000));
            if seed {
                s.seeding = SeedingPolicy {
                    enabled: true,
                    ..Default::default()
                };
            }
            let engine = TorrentEngine::start(s)
                .await
                .context("starting the engine")?;
            let opts = AddOptions {
                dir: Some(dir),
                only_files: (!select.is_empty()).then_some(select),
                sequential,
                peers: peer,
                ..Default::default()
            };
            let src = source_of(&source);
            if list {
                let p = engine.preview(&src, &opts).await?;
                println!("{}  {}", p.info_hash, p.name);
                for f in p.files.iter().filter(|f| !f.padding) {
                    println!(
                        "{:>5}  {:>10}  {}",
                        f.index,
                        human_bytes(f.len),
                        f.path.display()
                    );
                }
                engine.shutdown().await;
                return Ok(());
            }
            let id = engine.add(src, opts).await?;
            follow(&engine, &id, seed, lang).await
        }
        TorrentCmd::Resume { id, upload_limit } => {
            let id = offline::resolve_id(&state_dir, &id)?;
            let mut s = settings(&state_dir, &std::env::current_dir()?);
            s.upload_limit_bps = upload_limit.map(|k| k.saturating_mul(1000));
            let engine = TorrentEngine::start(s)
                .await
                .context("starting the engine")?;
            engine.resume(&id).await?;
            follow(&engine, &id, false, lang).await
        }
    }
}

fn state_name(s: &TorrentState) -> &'static str {
    match s {
        TorrentState::Checking => "checking",
        TorrentState::Downloading => "downloading",
        TorrentState::Seeding => "seeding",
        TorrentState::Paused => "paused",
        TorrentState::Completed => "completed",
        TorrentState::Failed { .. } => "failed",
    }
}

fn line(s: &TorrentSnapshot, lang: Lang) -> String {
    tf(
        lang,
        "torrent.progress",
        &[
            ("state", state_name(&s.state)),
            ("percent", &((s.fraction() * 100.0) as u32).to_string()),
            ("done", &human_bytes(s.downloaded_bytes)),
            ("total", &human_bytes(s.total_bytes)),
            ("speed", &human_speed(s.down_bps)),
            ("peers", &s.peers_connected.to_string()),
        ],
    )
}

/// Prints progress until the torrent is complete (and, with `seed`, until seeding ends).
async fn follow(
    engine: &Arc<TorrentEngine>,
    id: &str,
    seed: bool,
    lang: Lang,
) -> anyhow::Result<()> {
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
    let mut announced_seeding = false;
    loop {
        let snap = engine.snapshot(id)?;
        match &snap.state {
            TorrentState::Failed { error } => {
                engine.shutdown().await;
                bail!("{error}");
            }
            TorrentState::Completed => {
                println!("{}", tf(lang, "torrent.done", &[("name", &snap.name)]));
                engine.shutdown().await;
                return Ok(());
            }
            TorrentState::Seeding if seed => {
                if !announced_seeding {
                    println!("{}", tf(lang, "torrent.done", &[("name", &snap.name)]));
                    println!("{}", t(lang, "torrent.seeding"));
                    announced_seeding = true;
                }
            }
            _ => println!("{}", line(&snap, lang)),
        }
        tokio::select! {
            _ = &mut ctrl_c => {
                engine.shutdown().await;
                println!("{}", tf(lang, "torrent.interrupted", &[("id", &id[..12])]));
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
    }
}
