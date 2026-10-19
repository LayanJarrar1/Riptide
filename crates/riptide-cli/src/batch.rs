//! `riptide batch "<pattern>"`: expand a URL pattern (`file[001-120].zip`, `img_[a-z].jpg`,
//! `[0-100:5]`, several ranges) and either print the list (`--list`) or download every item,
//! one after the other, into a folder with the standalone engine.

use std::path::PathBuf;

use riptide_core::BatchError;

use crate::fetch::{self, FetchOptions, SharedStore};
use crate::i18n::{t, tf, Lang};

/// What to do with the expansion.
#[derive(Debug, Clone)]
pub struct BatchArgs {
    /// The pattern.
    pub pattern: String,
    /// Destination folder (created if missing); `None` = the current directory.
    pub dir: Option<PathBuf>,
    /// Print the expansion and stop.
    pub list: bool,
    /// Connections per file.
    pub connections: u32,
    /// No progress output.
    pub quiet: bool,
}

/// The error in the user's language.
pub fn describe(lang: Lang, error: &BatchError) -> String {
    let key = match error {
        BatchError::Empty => "batch.error.empty",
        BatchError::NotHttp => "batch.error.not_http",
        BatchError::InvalidChar { .. } => "batch.error.invalid_char",
        BatchError::Unclosed { .. } => "batch.error.unclosed",
        BatchError::InvalidRange { .. } => "batch.error.invalid_range",
        BatchError::ZeroStep { .. } => "batch.error.zero_step",
        BatchError::NumberTooLarge { .. } => "batch.error.number_too_large",
        BatchError::TooMany { .. } => "batch.error.too_many",
    };
    let limit = riptide_core::MAX_EXPANSION.to_string();
    tf(lang, key, &[("limit", &limit)])
}

/// Expands the pattern; with `--list` prints it, otherwise downloads every item. Items that fail
/// do not stop the batch; the exit status is an error when any failed.
pub async fn run(args: BatchArgs, store: Option<SharedStore>, lang: Lang) -> anyhow::Result<()> {
    let urls = riptide_core::batch::expand(&args.pattern)
        .map_err(|e| anyhow::anyhow!("{}", describe(lang, &e)))?;
    if args.list {
        print!("{}", riptide_core::export_url_list(&urls));
        if !args.quiet {
            eprintln!(
                "{}",
                tf(lang, "batch.count", &[("n", &urls.len().to_string())])
            );
        }
        return Ok(());
    }
    let Some(store) = store else {
        anyhow::bail!("{}", t(lang, "msg.unexpected"));
    };
    if let Some(dir) = &args.dir {
        std::fs::create_dir_all(dir)?;
    }
    let total = urls.len();
    let mut failed = 0usize;
    for (i, url) in urls.into_iter().enumerate() {
        if !args.quiet {
            eprintln!(
                "{}",
                tf(
                    lang,
                    "batch.item",
                    &[
                        ("i", &(i + 1).to_string()),
                        ("n", &total.to_string()),
                        ("url", &url)
                    ]
                )
            );
        }
        let opts = FetchOptions {
            url: url.clone(),
            output: Some(args.dir.clone().unwrap_or_else(|| PathBuf::from("."))),
            connections: args.connections,
            checksum: None,
            headers: Vec::new(),
            referrer: None,
            user_agent: None,
            credentials: None,
            insecure: false,
            overwrite: false,
            quiet: args.quiet,
            no_resume: false,
        };
        if let Err(e) = fetch::fetch(opts, store.clone(), lang).await {
            failed += 1;
            eprintln!("{}: {e:#}", t(lang, "fetch.failed"));
        }
    }
    let summary = tf(
        lang,
        "batch.summary",
        &[
            ("ok", &(total - failed).to_string()),
            ("n", &total.to_string()),
            ("failed", &failed.to_string()),
        ],
    );
    if failed > 0 {
        anyhow::bail!("{summary}");
    }
    if !args.quiet {
        eprintln!("{summary}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_a_message_in_both_languages() {
        let errors = [
            BatchError::Empty,
            BatchError::NotHttp,
            BatchError::InvalidChar { at: 1 },
            BatchError::Unclosed { at: 1 },
            BatchError::InvalidRange { at: 1 },
            BatchError::ZeroStep { at: 1 },
            BatchError::NumberTooLarge { at: 1 },
            BatchError::TooMany { limit: 10 },
        ];
        for e in &errors {
            for lang in [Lang::Ar, Lang::En] {
                let text = describe(lang, e);
                assert!(!text.is_empty() && !text.contains('{'), "{e:?} {text}");
            }
        }
        assert!(describe(Lang::En, &BatchError::TooMany { limit: 10 }).contains("10000"));
    }
}
