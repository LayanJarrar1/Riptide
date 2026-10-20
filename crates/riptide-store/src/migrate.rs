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
