//! `riptide` — the command-line face of the engine.
//!
//! `fetch` and `bench` run the standalone engine; `paths` and `ping` are utilities.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![forbid(unsafe_code)]

mod fetch;
mod i18n;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use i18n::{t, tf, Lang};
use riptide_core::{Checksum, Credentials};
use riptide_ipc::{Request, RequestBody, Response, ResponseBody};
use riptide_log::{LogConfig, Paths};
use riptide_store::Store;

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
    /// Download a URL with the standalone engine.
    Fetch {
        /// URL.
        url: String,
        /// Output path.
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
        /// Connections.
        #[arg(short, long, default_value_t = riptide_core::DEFAULT_CONNECTIONS, value_parser = clap::value_parser!(u32).range(1..=riptide_core::MAX_CONNECTIONS as i64))]
        connections: u32,
        /// Expected checksum.
        #[arg(long, value_name = "HEX")]
        checksum: Option<String>,
        /// Extra header.
        #[arg(short = 'H', long = "header", value_name = "NAME: VALUE")]
        headers: Vec<String>,
        /// Referer.
        #[arg(long, value_name = "URL")]
        referer: Option<String>,
        /// User agent.
        #[arg(long, value_name = "UA")]
        user_agent: Option<String>,
        /// Username.
        #[arg(long, value_name = "USER", requires = "password")]
        user: Option<String>,
        /// Password.
        #[arg(long, value_name = "PASS", requires = "user")]
        password: Option<String>,
        /// Ignore certificate errors.
        #[arg(long)]
        insecure: bool,
        /// Overwrite an existing file.
        #[arg(long)]
        overwrite: bool,
        /// No progress output.
        #[arg(short, long)]
        quiet: bool,
        /// Start from zero.
        #[arg(long)]
        no_resume: bool,
    },
    /// Benchmark connection counts.
    Bench {
        /// URL.
        url: String,
        /// Connection counts.
        #[arg(short, long, default_value = "1,8,32", value_name = "LIST")]
        connections: String,
        /// JSON output.
        #[arg(long)]
        json: bool,
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
                .mut_arg("checksum", |a| a.help(about("arg.checksum")))
                .mut_arg("headers", |a| a.help(about("arg.header")))
                .mut_arg("referer", |a| a.help(about("arg.referer")))
                .mut_arg("user_agent", |a| a.help(about("arg.user_agent")))
                .mut_arg("user", |a| a.help(about("arg.user")))
                .mut_arg("password", |a| a.help(about("arg.password")))
                .mut_arg("insecure", |a| a.help(about("arg.insecure")))
                .mut_arg("overwrite", |a| a.help(about("arg.overwrite")))
                .mut_arg("quiet", |a| a.help(about("arg.quiet")))
                .mut_arg("no_resume", |a| a.help(about("arg.no_resume")))
        })
        .mut_subcommand("bench", |s| {
            s.about(about("cmd.bench"))
                .mut_arg("url", |a| a.help(about("arg.url")))
                .mut_arg("connections", |a| a.help(about("arg.bench_connections")))
                .mut_arg("json", |a| a.help(about("arg.json")))
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

/// `Name: value` → `("Name", "value")`.
fn parse_header(raw: &str) -> anyhow::Result<(String, String)> {
    let (name, value) = raw
        .split_once(':')
        .with_context(|| format!("header {raw:?} is not `Name: value`"))?;
    let name = name.trim();
    if name.is_empty() {
        bail!("header {raw:?} has an empty name");
    }
    Ok((name.to_string(), value.trim().to_string()))
}

/// `1,8,32` → `[1, 8, 32]`, each within 1..=32.
fn parse_connection_list(raw: &str) -> anyhow::Result<Vec<u32>> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let n: u32 = part
            .trim()
            .parse()
            .with_context(|| format!("{part:?} is not a number"))?;
        if !(1..=riptide_core::MAX_CONNECTIONS).contains(&n) {
            bail!("{n} is outside 1..={}", riptide_core::MAX_CONNECTIONS);
        }
        out.push(n);
    }
    if out.is_empty() {
        bail!("no connection counts given");
    }
    Ok(out)
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
            .with_stderr(false),
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
        Command::Fetch {
            url,
            output,
            connections,
            checksum,
            headers,
            referer,
            user_agent,
            user,
            password,
            insecure,
            overwrite,
            quiet,
            no_resume,
        } => {
            let checksum = match checksum {
                Some(hex) => Some(Checksum::guess(&hex).with_context(|| {
                    format!("{hex:?} is not an MD5, SHA-1 or SHA-256 hex digest")
                })?),
                None => None,
            };
            let headers = headers
                .iter()
                .map(|h| parse_header(h))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let credentials = match (user, password) {
                (Some(username), Some(password)) => Some(Credentials { username, password }),
                _ => None,
            };
            let opts = fetch::FetchOptions {
                url,
                output,
                connections,
                checksum,
                headers,
                referrer: referer,
                user_agent,
                credentials,
                insecure,
                overwrite,
                quiet,
                no_resume,
            };
            let store = Arc::new(Mutex::new(
                Store::open(&paths.db_path).context("opening the database")?,
            ));
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(fetch::fetch(opts, store, lang)).map(|_| ())
        }
        Command::Bench {
            url,
            connections,
            json,
        } => {
            let list = parse_connection_list(&connections)?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(fetch::bench(url, list, json, lang))
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
                "-H",
                "X-A: 1",
                "--user",
                "u",
                "--password",
                "p",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(cli.log_level, "debug");
        assert_eq!(lang, Lang::Ar);
        assert!(
            matches!(cli.command, Command::Fetch { ref url, connections: 4, ref headers, .. } if url == "https://x/y" && headers == &["X-A: 1".to_string()])
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
            parse(["riptide", "fetch", "u", "--user", "x"].map(String::from)).is_err(),
            "user needs password"
        );
        assert!(
            parse(["riptide"].map(String::from)).is_err(),
            "a subcommand is required"
        );
        let (cli, _) = parse(["riptide", "bench", "u", "-c", "1,4"].map(String::from)).unwrap();
        assert!(
            matches!(cli.command, Command::Bench { ref connections, json: false, .. } if connections == "1,4")
        );
    }

    #[test]
    fn header_and_list_parsing() {
        assert_eq!(
            parse_header("X-Token: abc: def").unwrap(),
            ("X-Token".into(), "abc: def".into())
        );
        assert!(parse_header("nocolon").is_err());
        assert!(parse_header(": v").is_err());
        assert_eq!(parse_connection_list("1, 8 ,32").unwrap(), vec![1, 8, 32]);
        assert!(parse_connection_list("0").is_err());
        assert!(parse_connection_list("x").is_err());
    }

    #[test]
    fn help_is_localized() {
        let en = localized_command(Lang::En).render_long_help().to_string();
        let ar = localized_command(Lang::Ar).render_long_help().to_string();
        assert!(en.contains("free, open-source download manager"), "{en}");
        assert!(ar.contains("مدير تحميل"), "{ar}");
        assert!(ar.contains("--log-level"), "flag names stay the same: {ar}");
        let fetch_ar = localized_command(Lang::Ar)
            .find_subcommand_mut("fetch")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(fetch_ar.contains("بصمة الملف"), "{fetch_ar}");
    }
}
