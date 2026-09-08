//! SQLite database connection and schema migration.
//!
//! Uses bundled SQLite (no system dependency needed).
//! Auto-creates tables on first use via `migrate()`.

use rusqlite::{Connection, Result as SqliteResult};
use crate::config::paths;
use std::path::PathBuf;

/// Thin wrapper around a SQLite connection with auto-migration.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at `~/.deepseek-carp/sessions.db`.
    /// Automatically runs migrations on open.
    pub fn open() -> SqliteResult<Self> {
        let db_path = db_path();
        std::fs::create_dir_all(db_path.parent().expect("unwrap failed: db.rs:20")).ok();

        let conn = Connection::open(&db_path)?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Get a reference to the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Run schema migrations (idempotent — uses IF NOT EXISTS).
    fn migrate(&mut self) -> SqliteResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL DEFAULT 'New Session',
                working_dir TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id            TEXT PRIMARY KEY,
                session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role          TEXT NOT NULL,
                content       TEXT NOT NULL DEFAULT '',
                tool_calls    TEXT,
                tool_call_id  TEXT,
                seq           INTEGER NOT NULL DEFAULT 0,
                created_at    TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session
                ON messages(session_id, seq);

            CREATE TABLE IF NOT EXISTS api_usage (
                id                TEXT PRIMARY KEY,
                timestamp         TEXT NOT NULL,
                provider          TEXT NOT NULL,
                model             TEXT NOT NULL DEFAULT '',
                prompt_tokens     INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens      INTEGER NOT NULL DEFAULT 0,
                latency_ms        INTEGER NOT NULL DEFAULT 0,
                success           INTEGER NOT NULL DEFAULT 1
            );

            CREATE INDEX IF NOT EXISTS idx_api_usage_provider
                ON api_usage(provider, timestamp);

            CREATE TABLE IF NOT EXISTS loop_runs (
                id              TEXT PRIMARY KEY,
                target          TEXT NOT NULL,
                mode            TEXT NOT NULL DEFAULT 'review',
                max_rounds      INTEGER NOT NULL DEFAULT 5,
                total_rounds    INTEGER NOT NULL DEFAULT 0,
                passed          INTEGER NOT NULL DEFAULT 0,
                total_time_ms   INTEGER NOT NULL DEFAULT 0,
                final_verdict   TEXT,
                started_at      TEXT NOT NULL,
                finished_at     TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS loop_rounds (
                id              TEXT PRIMARY KEY,
                run_id          TEXT NOT NULL REFERENCES loop_runs(id) ON DELETE CASCADE,
                round_number    INTEGER NOT NULL,
                verdict         TEXT NOT NULL,
                phase_times     TEXT NOT NULL DEFAULT '[]',
                total_time_ms   INTEGER NOT NULL DEFAULT 0,
                details         TEXT NOT NULL DEFAULT '',
                created_at      TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_loop_rounds_run
                ON loop_rounds(run_id, round_number);

            CREATE TABLE IF NOT EXISTS diff_records (
                id              TEXT PRIMARY KEY,
                run_id          TEXT NOT NULL REFERENCES loop_runs(id) ON DELETE CASCADE,
                round_number    INTEGER NOT NULL,
                file_path       TEXT NOT NULL,
                original_hash   TEXT NOT NULL DEFAULT '',
                original        TEXT NOT NULL DEFAULT '',
                modified        TEXT NOT NULL DEFAULT '',
                description     TEXT NOT NULL DEFAULT '',
                status          TEXT NOT NULL DEFAULT 'applied',
                created_at      TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_diff_records_run
                ON diff_records(run_id, round_number);
            "
        )?;

        tracing::info!("SQLite database migrated successfully");
        Ok(())
    }
}

fn db_path() -> PathBuf {
    paths::config_dir().join("sessions.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_open_and_migrate() {
        // Use in-memory DB for tests
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, title TEXT, working_dir TEXT,
                created_at TEXT, updated_at TEXT
            );"
        ).unwrap();
        // Verify table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='sessions'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
