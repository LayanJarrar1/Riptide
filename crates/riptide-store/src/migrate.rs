//! Schema migrations.
//!
//! Each migration is one SQL script applied inside a single `BEGIN IMMEDIATE … COMMIT`
//! transaction together with its `schema_version` row. Either the whole script lands or none of
//! it does, so a crash (or a bad statement) between two statements leaves the previous version
//! intact — see the `failed_migration_rolls_back_entirely` test.

use rusqlite::{params, Connection, TransactionBehavior};

use crate::{Error, Result};

/// One versioned SQL script.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Sequential version this script brings the database to (1, 2, 3, …).
    pub version: u32,
    /// Short name, stored next to the version for forensics.
    pub name: &'static str,
    /// The script. Must not contain `BEGIN` / `COMMIT`.
    pub sql: &'static str,
}

/// All migrations, in order. Append; never edit a released entry.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("sql/v1.sql"),
    },
    Migration {
        version: 2,
        name: "media_kind",
        sql: include_str!("sql/v2.sql"),
    },
    Migration {
        version: 3,
        name: "queue_schedule_and_order",
        sql: include_str!("sql/v3.sql"),
    },
    Migration {
        version: 4,
        name: "torrents",
        sql: include_str!("sql/v4.sql"),
    },
    Migration {
        version: 5,
        name: "seen_index",
        sql: include_str!("sql/v5.sql"),
    },
];

/// The version a freshly migrated database has.
pub const SCHEMA_VERSION: u32 = 5;

const CREATE_VERSION_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    applied_at INTEGER NOT NULL
)";

/// Version currently in the file (`0` for an empty database). Creates the `schema_version`
/// table if it is missing.
pub fn current_version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(CREATE_VERSION_TABLE)?;
    let v: Option<i64> =
        conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
    let v = v.unwrap_or(0);
    u32::try_from(v).map_err(|_| Error::Corrupt(format!("schema_version holds {v}")))
}

/// Brings `conn` up to [`SCHEMA_VERSION`] using [`MIGRATIONS`]. Returns the resulting version.
pub fn run(conn: &mut Connection) -> Result<u32> {
    run_with(conn, MIGRATIONS)
}

/// Like [`run`] with an explicit migration list (tests use this to inject a failing script).
///
/// Errors: [`Error::TooNew`] if the file is ahead of `migrations`; [`Error::Migration`] if a
/// script fails (nothing from that script is kept); [`Error::Corrupt`] if the versions are not
/// consecutive.
pub fn run_with(conn: &mut Connection, migrations: &[Migration]) -> Result<u32> {
    let mut current = current_version(conn)?;
    let latest = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    if current > latest {
        return Err(Error::TooNew {
            found: current,
            supported: latest,
        });
    }

    let start = current;
    prune_old_backups(conn);
    if start > 0 && latest > start {
        backup_before_upgrade(conn, start);
    }
    // Table rebuilds (v4) need foreign keys off, and the pragma cannot change inside a
    // transaction. Off for the whole run, restored (and checked) at the end; no migration relies
    // on enforcement, and the check below reports any dangling reference.
    let fk_was_on: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk_was_on && latest > start {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
    }
    let outcome = apply(conn, migrations, start, &mut current);
    if fk_was_on {
        conn.pragma_update(None, "foreign_keys", "ON")?;
    }
    outcome?;
    if latest > start {
        let dangling: i64 =
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if dangling > 0 {
            // the data is intact and usable; refusing to start would only lock the user out
            tracing::error!(dangling, "dangling foreign keys after migrating");
        }
    }
    Ok(current)
}

/// Keeps a copy of a file database as it was before an upgrade: `<file>.v<N>.bak` for schema
/// `N`, written once and never overwritten. An older build refuses a newer schema and cannot
/// open the upgraded file; this copy is what a user who goes back to it can use. A failed backup
/// is logged and does not stop the upgrade.
fn backup_before_upgrade(conn: &Connection, version: u32) {
    let Some(path) = conn.path().filter(|p| !p.is_empty()) else {
        return; // in memory
    };
    let backup = std::path::PathBuf::from(format!("{path}.v{version}.bak"));
    if backup.exists() {
        return; // a good older copy is never replaced
    }
    match write_backup(conn, &backup) {
        Ok(()) => tracing::info!(version, "kept a copy of the database before the upgrade"),
        Err(e) => {
            tracing::warn!(error = %e, "could not keep a copy of the database before the upgrade");
        }
    }
}

/// Writes the copy to `<backup>.tmp`, strips the secrets (cookies, passwords and headers of
/// downloads must not outlive the downloads in a file nothing else cleans up), flushes it to disk
/// and only then renames it into place: a failure (disk full) leaves no partial `.bak`, which would
/// never be retried.
fn write_backup(conn: &Connection, backup: &std::path::Path) -> std::result::Result<(), String> {
    let mut tmp = backup.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let _ = std::fs::remove_file(&tmp);
    let result = (|| -> std::result::Result<(), String> {
        conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])
            .map_err(|e| e.to_string())?;
        {
            let copy = Connection::open(&tmp).map_err(|e| e.to_string())?;
            // a v1 file has the table too; tolerate a database that does not
            let _ = copy.execute("DELETE FROM secrets", []);
            copy.execute_batch("VACUUM").map_err(|e| e.to_string())?;
        }
        std::fs::File::options()
            .write(true)
            .open(&tmp)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, backup).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Days a pre-upgrade copy is kept; a later start deletes it.
pub const BACKUP_KEEP_DAYS: u64 = 30;

/// Deletes `<file>.v<N>.bak` copies (and leftover `.tmp` files) older than
/// [`BACKUP_KEEP_DAYS`].
fn prune_old_backups(conn: &Connection) {
    let Some(path) = conn.path().filter(|p| !p.is_empty()) else {
        return;
    };
    let path = std::path::Path::new(path);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let prefix = format!("{name}.v");
    let max_age = std::time::Duration::from_secs(BACKUP_KEEP_DAYS * 24 * 3600);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let fname = e.file_name().to_string_lossy().into_owned();
        if !fname.starts_with(&prefix) || !(fname.ends_with(".bak") || fname.ends_with(".bak.tmp"))
        {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if old && std::fs::remove_file(e.path()).is_ok() {
            tracing::info!(file = %fname, "deleted an old pre-upgrade copy of the database");
        }
    }
}

fn apply(
    conn: &mut Connection,
    migrations: &[Migration],
    start: u32,
    current: &mut u32,
) -> Result<()> {
    for m in migrations.iter().filter(|m| m.version > start) {
        if m.version != *current + 1 {
            return Err(Error::Corrupt(format!(
                "migration {} ({}) follows version {}; versions must be consecutive",
                m.version, m.name, *current
            )));
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(m.sql).map_err(|source| Error::Migration {
            version: m.version,
            name: m.name.to_string(),
            source,
        })?;
        tx.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![m.version, m.name, now_millis()],
        )?;
        tx.commit()?;
        tracing::info!(
            version = m.version,
            name = m.name,
            "applied schema migration"
        );
        *current = m.version;
    }
    Ok(())
}

/// Milliseconds since the Unix epoch; `0` if the clock is before 1970.
pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    #[test]
    fn empty_database_migrates_to_latest() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert_eq!(run(&mut conn).unwrap(), SCHEMA_VERSION);
        for t in [
            "downloads",
            "segments",
            "secrets",
            "queues",
            "categories",
            "settings",
            "schema_version",
        ] {
            assert!(table_exists(&conn, t), "{t} missing");
        }
        // idempotent
        assert_eq!(run(&mut conn).unwrap(), SCHEMA_VERSION);
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, i64::from(SCHEMA_VERSION));
    }

    #[test]
    fn failed_migration_rolls_back_entirely() {
        let mut conn = Connection::open_in_memory().unwrap();
        // a database at the last released schema (v3); the broken script plays the next one
        run_with(&mut conn, &MIGRATIONS[..3]).unwrap();

        let mut broken = MIGRATIONS[..3].to_vec();
        broken.push(Migration {
            version: 4,
            name: "broken",
            // the third statement fails: nothing from the first two may survive
            sql: "CREATE TABLE will_vanish_a (x);
                  CREATE TABLE will_vanish_b (y);
                  INSERT INTO no_such_table VALUES (1);",
        });
        let err = run_with(&mut conn, &broken).unwrap_err();
        assert!(matches!(err, Error::Migration { version: 4, .. }), "{err}");

        assert_eq!(
            current_version(&conn).unwrap(),
            3,
            "version must not advance"
        );
        assert!(!table_exists(&conn, "will_vanish_a"));
        assert!(!table_exists(&conn, "will_vanish_b"));
        // the database is still fully usable
        let ok: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ok, "ok");
        assert_eq!(run(&mut conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn newer_file_is_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (99, 'future', 0)",
            [],
        )
        .unwrap();
        let err = run(&mut conn).unwrap_err();
        assert!(
            matches!(
                err,
                Error::TooNew {
                    found: 99,
                    supported: SCHEMA_VERSION
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn non_consecutive_versions_are_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        let gap = [
            MIGRATIONS[0],
            Migration {
                version: 3,
                name: "gap",
                sql: "SELECT 1;",
            },
        ];
        let err = run_with(&mut conn, &gap).unwrap_err();
        assert!(matches!(err, Error::Corrupt(_)), "{err}");
        assert_eq!(
            current_version(&conn).unwrap(),
            1,
            "v1 was applied before the gap"
        );
    }
}
