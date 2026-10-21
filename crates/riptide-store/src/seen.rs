//! The "already downloaded" index: what finished downloads are remembered by. Keys are built by
//! `riptide_core::dedup`; this module only stores and looks them up.

use rusqlite::{params, OptionalExtension};

use crate::{Result, Store};

/// One remembered finished download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenRecord {
    /// Id the download had in the list (may no longer exist there).
    pub download_id: i64,
    /// Human title (the file name unless the source knew a better one).
    pub title: String,
    /// File name on disk.
    pub filename: String,
    /// Folder it was saved to.
    pub dir: String,
    /// Size in bytes.
    pub size: Option<u64>,
    /// SHA-256 of the finished file, once computed.
    pub sha256: Option<String>,
    /// Completion time, Unix milliseconds.
    pub completed_at: i64,
    /// The file was missing from disk when last checked.
    pub stale: bool,
}

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<SeenRecord> {
    Ok(SeenRecord {
        download_id: r.get(0)?,
        title: r.get(1)?,
        filename: r.get(2)?,
        dir: r.get(3)?,
        size: r
            .get::<_, Option<i64>>(4)?
            .and_then(|v| u64::try_from(v).ok()),
        sha256: r.get(5)?,
        completed_at: r.get(6)?,
        stale: r.get::<_, i64>(7)? != 0,
    })
}

const COLS: &str =
    "d.download_id, d.title, d.filename, d.dir, d.size, d.sha256, d.completed_at, d.stale";

impl Store {
    /// Remembers (or refreshes) a finished download under all of `keys`.
    pub fn seen_upsert(&mut self, rec: &SeenRecord, keys: &[String]) -> Result<()> {
        self.transaction(|s| {
            let c = s.connection();
            c.execute(
                "INSERT INTO seen_downloads (download_id, title, filename, dir, size, sha256, completed_at, stale)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(download_id) DO UPDATE SET title = excluded.title, filename = excluded.filename,
                   dir = excluded.dir, size = excluded.size, sha256 = excluded.sha256,
                   completed_at = excluded.completed_at, stale = excluded.stale",
                params![
                    rec.download_id,
                    rec.title,
                    rec.filename,
                    rec.dir,
                    rec.size.and_then(|v| i64::try_from(v).ok()),
                    rec.sha256,
                    rec.completed_at,
                    i64::from(rec.stale)
                ],
            )?;
            for k in keys {
                c.execute(
                    "INSERT OR IGNORE INTO seen_keys (key, download_id) VALUES (?1, ?2)",
                    params![k, rec.download_id],
                )?;
            }
            Ok(())
        })
    }

    /// Adds keys to an existing record (the SHA-256 key arrives after the file was hashed).
    pub fn seen_add_keys(
        &mut self,
        download_id: i64,
        sha256: Option<&str>,
        keys: &[String],
    ) -> Result<()> {
        self.transaction(|s| {
            let c = s.connection();
            if let Some(h) = sha256 {
                c.execute(
                    "UPDATE seen_downloads SET sha256 = ?2 WHERE download_id = ?1",
                    params![download_id, h],
                )?;
            }
            for k in keys {
                c.execute(
                    "INSERT OR IGNORE INTO seen_keys (key, download_id) VALUES (?1, ?2)",
                    params![k, download_id],
                )?;
            }
            Ok(())
        })
    }

    /// Records matching any of `keys`, newest first, each with the first key that matched.
    pub fn seen_lookup(&self, keys: &[String]) -> Result<Vec<(SeenRecord, String)>> {
        let mut out: Vec<(SeenRecord, String)> = Vec::new();
        let mut stmt = self.connection().prepare_cached(&format!(
            "SELECT {COLS} FROM seen_keys k JOIN seen_downloads d ON d.download_id = k.download_id
             WHERE k.key = ?1 ORDER BY d.completed_at DESC"
        ))?;
        for key in keys {
            let rows = stmt.query_map([key], row_to_record)?;
            for r in rows {
                let r = r?;
                if !out.iter().any(|(o, _)| o.download_id == r.download_id) {
                    out.push((r, key.clone()));
                }
            }
        }
        out.sort_by_key(|(r, _)| std::cmp::Reverse(r.completed_at));
        Ok(out)
    }

    /// One record by download id.
    pub fn seen_get(&self, download_id: i64) -> Result<Option<SeenRecord>> {
        Ok(self
            .connection()
            .query_row(
                &format!("SELECT {COLS} FROM seen_downloads d WHERE d.download_id = ?1"),
                [download_id],
                row_to_record,
            )
            .optional()?)
    }

    /// Sets or clears the stale flag.
    pub fn seen_set_stale(&mut self, download_id: i64, stale: bool) -> Result<()> {
        self.connection().execute(
            "UPDATE seen_downloads SET stale = ?2 WHERE download_id = ?1",
            params![download_id, i64::from(stale)],
        )?;
        Ok(())
    }

    /// Forgets everything (the "forget" button); returns the number of records removed.
    pub fn seen_clear(&mut self) -> Result<usize> {
        // identifiers kept for downloads that have not finished (titles, DOIs of research hits)
        // are part of the memory too
        self.connection()
            .execute("DELETE FROM settings WHERE key LIKE 'seen.hint.%'", [])?;
        Ok(self
            .connection()
            .execute("DELETE FROM seen_downloads", [])?)
    }

    /// Number of remembered downloads.
    pub fn seen_count(&self) -> Result<u64> {
        let n: i64 =
            self.connection()
                .query_row("SELECT COUNT(*) FROM seen_downloads", [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: i64, name: &str) -> SeenRecord {
        SeenRecord {
            download_id: id,
            title: name.into(),
            filename: name.into(),
            dir: "C:\\d".into(),
            size: Some(10),
            sha256: None,
            completed_at: id,
            stale: false,
        }
    }

    #[test]
    fn upsert_lookup_stale_and_clear() {
        let mut s = Store::open_in_memory().unwrap();
        s.seen_upsert(
            &rec(1, "a.pdf"),
            &["url:https://h/a".into(), "doi:10.1/x".into()],
        )
        .unwrap();
        s.seen_upsert(&rec(2, "b.pdf"), &["url:https://h/b".into()])
            .unwrap();
        let hit = s
            .seen_lookup(&["doi:10.1/x".into(), "url:https://h/zzz".into()])
            .unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].0.download_id, 1);
        assert_eq!(hit[0].1, "doi:10.1/x");
        s.seen_set_stale(1, true).unwrap();
        assert!(s.seen_get(1).unwrap().unwrap().stale);
        s.seen_add_keys(2, Some("ab"), &["sha256:ab".into()])
            .unwrap();
        assert_eq!(
            s.seen_lookup(&["sha256:ab".into()]).unwrap()[0]
                .0
                .sha256
                .as_deref(),
            Some("ab")
        );
        assert_eq!(s.seen_count().unwrap(), 2);
        assert_eq!(s.seen_clear().unwrap(), 2);
        assert!(s.seen_lookup(&["doi:10.1/x".into()]).unwrap().is_empty());
    }
}
