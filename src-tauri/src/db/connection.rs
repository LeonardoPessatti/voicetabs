use std::{path::Path, sync::Arc};

use parking_lot::Mutex;
use rusqlite::Connection;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/001_initial_schema.sql")),
];

#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn with<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&mut Connection) -> T,
    {
        let mut guard = self.inner.lock();
        f(&mut guard)
    }

    /// Build a `Db` from an existing `rusqlite::Connection`. Intended for
    /// unit + integration tests that want an in-memory database; not used by
    /// production code, but kept on the public API so external test files
    /// (which compile as separate crates) can call it.
    pub fn from_connection(conn: Connection) -> Self {
        Db { inner: Arc::new(Mutex::new(conn)) }
    }
}

pub fn open(path: &Path) -> anyhow::Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    apply_migrations(&conn)?;
    Ok(Db { inner: Arc::new(Mutex::new(conn)) })
}

fn apply_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
        [],
    )?;
    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    for (version, sql) in MIGRATIONS {
        if *version > current {
            tracing::info!("applying migration v{version}");
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO schema_version (version) VALUES (?)", [version])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_migrations(&conn).unwrap();
        Db::from_connection(conn)
    }

    #[test]
    fn migrations_apply_idempotently() {
        let db = mem_db();
        // Applying twice should not fail.
        db.with(|c| apply_migrations(c)).unwrap();
        let version: i64 = db.with(|c| {
            c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
                .unwrap()
        });
        assert_eq!(version, 1);
    }

    #[test]
    fn tables_exist() {
        let db = mem_db();
        let count: i64 = db.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('tabs','segments','settings')",
                [],
                |r| r.get(0),
            )
            .unwrap()
        });
        assert_eq!(count, 3);
    }
}
