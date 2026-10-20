//! "You already downloaded this": the engine side of the dedup index.
//!
//! When a download completes, a background task hashes the finished file (SHA-256, on a
//! blocking thread, never on the async runtime) and stores the keys built by
//! [`riptide_core::dedup`]. [`Engine::seen_lookup`] answers "is this URL / DOI / arXiv id /
//! file / hash known?" for the Add dialog, the batch dialog and the Research page, checking the
//! disk so a deleted file is reported as stale rather than as a hit.

use std::io::Read;
use std::path::{Path, PathBuf};

use riptide_core::dedup::{self, Identity};
use riptide_core::{Download, DownloadId};
use riptide_store::SeenRecord;
use serde::{Deserialize, Serialize};

use crate::{db, now_millis, Engine, Error, Result};

/// What the caller knows about something it may download. Everything is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SeenQuery {
    /// Address.
    pub url: Option<String>,
    /// DOI.
    pub doi: Option<String>,
    /// arXiv id.
    pub arxiv: Option<String>,
    /// PubMed id.
    pub pmid: Option<String>,
    /// PubMed Central id.
    pub pmcid: Option<String>,
    /// File name.
    pub filename: Option<String>,
    /// Size in bytes.
    pub size: Option<u64>,
    /// SHA-256 hex.
    pub sha256: Option<String>,
}

impl SeenQuery {
    fn keys(&self) -> Vec<String> {
        dedup::keys(&Identity {
            url: self.url.as_deref(),
            doi: self.doi.as_deref(),
            arxiv: self.arxiv.as_deref(),
            pmid: self.pmid.as_deref(),
            pmcid: self.pmcid.as_deref(),
            filename: self.filename.as_deref(),
            size: self.size,
            sha256: self.sha256.as_deref(),
        })
    }
}

/// Extra identifiers a caller attaches to a download it is adding (the Research page knows the
/// DOI of a paper whose PDF lives at an unrelated address).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SeenHint {
    /// Title to remember it by.
    pub title: Option<String>,
    /// DOI.
    pub doi: Option<String>,
    /// arXiv id.
    pub arxiv: Option<String>,
    /// PubMed id.
    pub pmid: Option<String>,
    /// PubMed Central id.
    pub pmcid: Option<String>,
}

/// A remembered download that matches a query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenMatch {
    /// Id the download had in the list.
    pub download_id: i64,
    /// Title it is remembered by.
    pub title: String,
    /// File name.
    pub filename: String,
    /// Folder.
    pub dir: String,
    /// Full path of the file.
    pub path: String,
    /// When it finished, Unix milliseconds.
    pub completed_at: i64,
    /// The file is no longer on disk (deleted or moved): offer "Download again", not "Open".
    pub stale: bool,
    /// Which kind of key matched: `url`, `doi`, `arxiv`, `pmid`, `pmcid`, `file` or `sha256`.
    pub matched_by: String,
}

fn hint_key(id: DownloadId) -> String {
    format!("seen.hint.{}", id.0)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Whether `path` still holds a file of `size` (when known).
fn file_present(path: &Path, size: Option<u64>) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && size.is_none_or(|s| s == m.len()))
        .unwrap_or(false)
}

impl Engine {
    /// Adds a download and remembers `hint`'s identifiers for when it completes.
    pub async fn add_hinted(
        &self,
        request: riptide_core::DownloadRequest,
        hint: SeenHint,
    ) -> Result<DownloadId> {
        let id = self.add(request).await?;
        if hint != SeenHint::default() {
            if let Ok(json) = serde_json::to_string(&hint) {
                let key = hint_key(id);
                db(&self.inner.store, move |s| s.set_setting(&key, &json)).await?;
            }
        }
        Ok(id)
    }

    /// Indexes a completed download: the keys that need no disk access now, and its SHA-256
    /// as soon as the file is hashed on a blocking thread. Best effort; failures are logged.
    pub(crate) fn spawn_record_seen(&self, id: DownloadId) {
        let engine = self.clone();
        tokio::spawn(async move {
            if let Err(e) = engine.record_seen(id).await {
                tracing::warn!(id = id.0, error = %e, "could not index the finished download");
            }
        });
    }

    async fn record_seen(&self, id: DownloadId) -> Result<()> {
        let Some(d) = self.get(id).await? else {
            return Ok(());
        };
        let hint_k = hint_key(id);
        let hint: SeenHint = db(&self.inner.store, {
            let k = hint_k.clone();
            move |s| s.setting(&k)
        })
        .await?
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
        let path = d.final_path();
        let size = std::fs::metadata(&path).ok().map(|m| m.len()).or(d.size);
        let url_keys = |d: &Download, hint: &SeenHint, sha: Option<&str>| {
            let mut keys = dedup::keys(&Identity {
                url: Some(&d.url),
                doi: hint.doi.as_deref(),
                arxiv: hint.arxiv.as_deref(),
                pmid: hint.pmid.as_deref(),
                pmcid: hint.pmcid.as_deref(),
                filename: Some(&d.filename),
                size,
                sha256: sha,
            });
            if let Some(final_url) = d.final_url.as_deref() {
                keys.extend(dedup::keys(&Identity {
                    url: Some(final_url),
                    ..Identity::default()
                }));
                keys.sort();
                keys.dedup();
            }
            keys
        };
        let record = SeenRecord {
            download_id: id.0,
            title: hint
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| d.filename.clone()),
            filename: d.filename.clone(),
            dir: d.dir.to_string_lossy().into_owned(),
            size,
            sha256: None,
            completed_at: now_millis(),
            stale: false,
        };
        let keys = url_keys(&d, &hint, None);
        db(&self.inner.store, move |s| s.seen_upsert(&record, &keys)).await?;

        // the hash is disk-bound: a blocking thread, after the record is already usable
        let p: PathBuf = path;
        let sha = tokio::task::spawn_blocking(move || sha256_file(&p))
            .await
            .map_err(|e| Error::Internal(format!("hash task: {e}")))?;
        match sha {
            Ok(h) => {
                let keys = vec![format!("sha256:{h}")];
                db(&self.inner.store, move |s| {
                    s.seen_add_keys(id.0, Some(&h), &keys)?;
                    s.set_setting(&hint_k, "")
                })
                .await?;
            }
            Err(e) => tracing::debug!(id = id.0, error = %e, "file not hashed"),
        }
        Ok(())
    }

    /// For each query, the best remembered download that matches it (newest first), with a
    /// disk check: a file that is gone is returned with `stale = true`, and the flag is saved
    /// (and cleared again if the file comes back).
    pub async fn seen_lookup(&self, queries: Vec<SeenQuery>) -> Result<Vec<Option<SeenMatch>>> {
        let mut out = Vec::with_capacity(queries.len());
        for q in queries {
            let keys = q.keys();
            if keys.is_empty() {
                out.push(None);
                continue;
            }
            let hits = db(&self.inner.store, move |s| s.seen_lookup(&keys)).await?;
            let mut best: Option<SeenMatch> = None;
            for (rec, key) in hits {
                let path = Path::new(&rec.dir).join(&rec.filename);
                let size = rec.size;
                let present = {
                    let p = path.clone();
                    tokio::task::spawn_blocking(move || file_present(&p, size))
                        .await
                        .map_err(|e| Error::Internal(format!("stat task: {e}")))?
                };
                if present == rec.stale {
                    let id = rec.download_id;
                    db(&self.inner.store, move |s| s.seen_set_stale(id, !present)).await?;
                }
                let m = SeenMatch {
                    download_id: rec.download_id,
                    title: rec.title,
                    filename: rec.filename,
                    dir: rec.dir,
                    path: path.to_string_lossy().into_owned(),
                    completed_at: rec.completed_at,
                    stale: !present,
                    matched_by: key.split(':').next().unwrap_or("url").to_string(),
                };
                // a live file beats a stale one; otherwise the newest wins (hits are ordered)
                if best.as_ref().is_none_or(|b| b.stale && !m.stale) {
                    best = Some(m);
                }
            }
            out.push(best);
        }
        Ok(out)
    }

    /// Whether adding something already downloaded (and still on disk) is refused. Off unless
    /// the user turned it on.
    pub async fn skip_duplicates(&self) -> bool {
        db(&self.inner.store, |s| s.setting("seen.skip_duplicates"))
            .await
            .ok()
            .flatten()
            .is_some_and(|v| v == "1")
    }

    /// Turns "skip duplicates" on or off (persisted).
    pub async fn set_skip_duplicates(&self, on: bool) -> Result<()> {
        db(&self.inner.store, move |s| {
            s.set_setting("seen.skip_duplicates", if on { "1" } else { "0" })
        })
        .await
    }

    /// Forgets the whole index (Settings → AI → Forget everything). Returns how many records
    /// were removed. Downloads and files are untouched.
    pub async fn seen_forget(&self) -> Result<usize> {
        db(&self.inner.store, |s| s.seen_clear()).await
    }

    /// Titles and ids (never paths) of what the user has downloaded, newest first, for the AI
    /// context when the user allowed it. At most `limit` entries.
    pub async fn seen_titles(&self, limit: usize) -> Result<Vec<String>> {
        let all = self.list().await?;
        Ok(all
            .into_iter()
            .rev()
            .filter(|d| d.state == riptide_core::DownloadState::Completed)
            .take(limit)
            .map(|d| d.filename)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_checks_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"12345").unwrap();
        assert!(file_present(&p, Some(5)));
        assert!(file_present(&p, None));
        assert!(!file_present(&p, Some(6)));
        std::fs::remove_file(&p).unwrap();
        assert!(!file_present(&p, None));
    }

    #[test]
    fn hashes_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
