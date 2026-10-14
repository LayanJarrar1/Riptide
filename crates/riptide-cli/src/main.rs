//! `riptide` — the command-line face of the engine.
//!
//! M0: `--log-level`, `--data-dir`, `--lang`, `paths` and `ping`. `fetch` and `bench` are M1.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

mod i18n;

use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use i18n::{t, tf, Lang};
use riptide_ipc::{Request, RequestBody, Response, ResponseBody};
use riptide_log::{LogConfig, Paths};

#[derive(Debug, Parser)]
#[command(name = "riptide", version, disable_help_subcommand = true)]
struct Cli {
    /// Log filter (see `arg.log_level`).
    #[arg(long, global = true, env = "RUST_LOG", default_value = LogConfig::DEFAULT_FILTER, value_name = "FILTER")]
    log_level: String,

    /// Data directory override.
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Message language.
    #[arg(long, global = true, default_value = "auto", value_parser = ["ar", "en", "auto"])]
    lang: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// M1.
    Fetch {
        /// URL.
        url: String,
        /// Output path.
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
        /// Connections.
        #[arg(short, long, default_value_t = riptide_core::DEFAULT_CONNECTIONS, value_parser = clap::value_parser!(u32).range(1..=riptide_core::MAX_CONNECTIONS as i64))]
        connections: u32,
    },
    /// M1.
    Bench {
        /// URL.
        url: String,
    },
    /// Ping the running app.
    Ping,
    /// Show resolved paths.
    Paths,
}

/// The clap command with every `about`/`help` string taken from the i18n table for `lang`.
fn localized_command(lang: Lang) -> clap::Command {
    let about = |key: &str| t(lang, key).to_string();
    let mut cmd = Cli::command().about(about("about"));
    cmd = cmd
        .mut_arg("log_level", |a| a.help(about("arg.log_level")))
        .mut_arg("data_dir", |a| a.help(about("arg.data_dir")))
        .mut_arg("lang", |a| a.help(about("arg.lang")));
    cmd = cmd
        .mut_subcommand("fetch", |s| {
            s.about(about("cmd.fetch"))
                .mut_arg("url", |a| a.help(about("arg.url")))
                .mut_arg("output", |a| a.help(about("arg.output")))
                .mut_arg("connections", |a| a.help(about("arg.connections")))
        })
        .mut_subcommand("bench", |s| {
            s.about(about("cmd.bench"))
                .mut_arg("url", |a| a.help(about("arg.url")))
        })
        .mut_subcommand("ping", |s| s.about(about("cmd.ping")))
        .mut_subcommand("paths", |s| s.about(about("cmd.paths")));
    cmd
}

/// Parses `args` in two passes: the first only to learn `--lang`, the second with localized help.
fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<(Cli, Lang)> {
    let args: Vec<String> = args.into_iter().collect();
    let explicit = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix("--lang=").map(str::to_owned))
        });
    let lang = Lang::detect(explicit.as_deref());
    let matches = localized_command(lang).try_get_matches_from(&args)?;
    let cli = Cli::from_arg_matches(&matches)?;
    Ok((cli, lang))
}

fn main() -> anyhow::Result<()> {
    let (cli, lang) = match parse(std::env::args()) {
        Ok(v) => v,
        // a clap error (bad flag, --help, --version) prints itself and exits with the right code
        Err(e) => e.downcast::<clap::Error>()?.exit(),
    };

    let paths = match &cli.data_dir {
        Some(dir) => Paths::in_dir(dir, false),
        None => Paths::resolve().context("resolving the data directory")?,
    };
    paths.ensure_dirs().context("creating the data directory")?;
    let _log_guard = riptide_log::init(
        &LogConfig::new(&paths.logs_dir)
            .with_filter(&cli.log_level)
            .with_stderr(true),
    )
    .context("starting the logger")?;

    match cli.command {
        Command::Paths => {
            let mode = if paths.portable {
                "msg.paths.mode.portable"
            } else {
                "msg.paths.mode.installed"
            };
            println!(
                "{}: {}",
                t(lang, "msg.paths.data"),
                paths.data_dir.display()
            );
            println!(
                "{}: {}",
                t(lang, "msg.paths.logs"),
                paths.logs_dir.display()
            );
            println!("{}: {}", t(lang, "msg.paths.db"), paths.db_path.display());
            println!("{}: {}", t(lang, "msg.paths.mode"), t(lang, mode));
            println!(
                "{}: {}",
                t(lang, "msg.paths.endpoint"),
                riptide_ipc::default_endpoint()?
            );
            Ok(())
        }
        Command::Ping => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(ping(lang))
        }
        Command::Fetch { .. } | Command::Bench { .. } => {
            bail!(
                "{}",
                tf(lang, "msg.not_implemented", &[("milestone", "M1")])
            )
        }
    }
}

async fn ping(lang: Lang) -> anyhow::Result<()> {
    let endpoint = riptide_ipc::default_endpoint()?;
    let mut conn = match riptide_ipc::connect(&endpoint).await {
        Ok(c) => c,
        Err(riptide_ipc::Error::NotRunning(ep)) => {
            bail!("{}", tf(lang, "msg.not_running", &[("endpoint", &ep)]))
        }
        Err(e) => return Err(e.into()),
    };
    conn.send(&Request::new(1, RequestBody::Ping)).await?;
    match conn.recv::<Response>().await? {
        Some(Response {
            body: ResponseBody::Pong { version, protocol },
            ..
        }) => {
            println!(
                "{}",
                tf(
                    lang,
                    "msg.pong",
                    &[("version", &version), ("protocol", &protocol.to_string())]
                )
            );
            Ok(())
        }
        _ => bail!("{}", t(lang, "msg.unexpected")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_definition_is_valid_in_both_languages() {
        localized_command(Lang::En).debug_assert();
        localized_command(Lang::Ar).debug_assert();
    }

    #[test]
    fn parses_global_flags_and_subcommands() {
        let (cli, lang) = parse(
            [
                "riptide",
                "--log-level",
                "debug",
                "--lang",
                "ar",
                "fetch",
                "https://x/y",
                "-c",
                "4",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(cli.log_level, "debug");
        assert_eq!(lang, Lang::Ar);
        assert!(
            matches!(cli.command, Command::Fetch { ref url, connections: 4, .. } if url == "https://x/y")
        );

        let (cli, _) =
            parse(["riptide", "paths", "--data-dir", "C:/tmp/r"].map(String::from)).unwrap();
        assert_eq!(
            cli.data_dir.as_deref(),
            Some(std::path::Path::new("C:/tmp/r"))
        );
        assert!(matches!(cli.command, Command::Paths));

        assert!(
            parse(["riptide", "fetch", "u", "-c", "99"].map(String::from)).is_err(),
            "max 32"
        );
        assert!(
            parse(["riptide"].map(String::from)).is_err(),
            "a subcommand is required"
        );
    }

    #[test]
    fn help_is_localized() {
        let en = localized_command(Lang::En).render_long_help().to_string();
        let ar = localized_command(Lang::Ar).render_long_help().to_string();
        assert!(en.contains("free, open-source download manager"), "{en}");
        assert!(ar.contains("مدير تحميل"), "{ar}");
        assert!(ar.contains("--log-level"), "flag names stay the same: {ar}");
    }
}
