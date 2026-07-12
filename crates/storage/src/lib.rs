//! SQLite schema and migration entrypoint.

use rusqlite::Connection;
use thiserror::Error;

pub const MIGRATIONS: &[(&str, &str)] = &[(
    "0001_initial",
    include_str!("../migrations/0001_initial.sql"),
)];

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

pub fn apply_migrations(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );",
    )?;

    let transaction = connection.transaction()?;
    for (name, sql) in MIGRATIONS {
        let already_applied: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE name = ?1)",
            [name],
            |row| row.get(0),
        )?;
        if !already_applied {
            transaction.execute_batch(sql)?;
            transaction.execute("INSERT INTO schema_migrations (name) VALUES (?1)", [name])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_migration_creates_core_tables() {
        let mut connection = Connection::open_in_memory().expect("open sqlite");
        apply_migrations(&mut connection).expect("apply migrations");
        apply_migrations(&mut connection).expect("migrations are idempotent");

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                    'card_definitions',
                    'catalogue_revision',
                    'image_cache',
                    'app_settings',
                    'overlay_geometry',
                    'diagnostic_snapshots',
                    'match_events'
                )",
                [],
                |row| row.get(0),
            )
            .expect("count tables");
        assert_eq!(count, 7);
    }
}
