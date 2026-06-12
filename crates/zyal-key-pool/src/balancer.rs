//! Round-robin cursor over per-user credential slots.
//!
//! Persists cursor state in the existing `~/.jekko/users/.balancer.sqlite`
//! file (REUSED, never recreated) under the `round_robin_cursor` table:
//!
//! ```sql
//! CREATE TABLE round_robin_cursor (
//!   provider TEXT NOT NULL,
//!   model    TEXT NOT NULL,
//!   cursor   INTEGER NOT NULL DEFAULT 0,
//!   PRIMARY KEY (provider, model)
//! );
//! ```
//!
//! Keyed by `(provider, model)` so different models on the same provider
//! advance independently. The cursor is monotonic modulo `candidates_len`;
//! callers that change the candidate ordering should reset the cursor.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

/// Filename for the global cursor database under `~/.jekko/users/`.
pub const BALANCER_DB_FILENAME: &str = ".balancer.sqlite";

/// Idempotent schema. Safe to run on every `open()` — the existing jekko
/// runtime balancer creates the same table, so this is a no-op if the file
/// was created by jekko-runtime first.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS round_robin_cursor (
    provider TEXT NOT NULL,
    model    TEXT NOT NULL,
    cursor   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider, model)
);
";

/// Round-robin cursor backed by `~/.jekko/users/.balancer.sqlite`.
///
/// Open one cursor per process (the connection is mutexed by SQLite at the
/// file level — concurrent processes are fine, concurrent connections within
/// a process are fine but pointless).
pub struct RoundRobinCursor {
    conn: Connection,
    db_path: PathBuf,
}

impl RoundRobinCursor {
    /// Open the global cursor DB under `users_root` (typically
    /// `~/.jekko/users/`). Creates the file + schema if missing.
    pub fn open(users_root: &Path) -> Result<Self> {
        let db_path = users_root.join(BALANCER_DB_FILENAME);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        let conn =
            Connection::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;
        conn.execute_batch(SCHEMA)
            .with_context(|| format!("init schema in {}", db_path.display()))?;
        Ok(Self { conn, db_path })
    }

    /// Path to the underlying database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Return the next candidate index for `(provider, model)`, advancing
    /// the cursor by one. Returns 0 when `candidates_len == 0` (caller should
    /// treat as "no eligible slot").
    pub fn next_index(&self, provider: &str, model: &str, candidates_len: usize) -> Result<usize> {
        if candidates_len == 0 {
            return Ok(0);
        }
        let current: Option<i64> = self
            .conn
            .query_row(
                "SELECT cursor FROM round_robin_cursor WHERE provider = ?1 AND model = ?2",
                params![provider, model],
                |row| row.get(0),
            )
            .ok();
        let current = current.unwrap_or(0).max(0) as usize;
        let idx = current % candidates_len;
        let next = ((idx + 1) % candidates_len) as i64;
        self.conn.execute(
            "INSERT INTO round_robin_cursor (provider, model, cursor) VALUES (?1, ?2, ?3)
             ON CONFLICT(provider, model) DO UPDATE SET cursor = excluded.cursor",
            params![provider, model, next],
        )?;
        Ok(idx)
    }

    /// Reset the cursor for `(provider, model)`. Useful after the eligible
    /// candidate set is reshuffled (e.g. a slot was disabled).
    pub fn reset(&self, provider: &str, model: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM round_robin_cursor WHERE provider = ?1 AND model = ?2",
            params![provider, model],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn next_index_cycles_through_candidates() {
        let dir = tempdir().unwrap();
        let cursor = RoundRobinCursor::open(dir.path()).unwrap();
        assert_eq!(cursor.next_index("openai", "gpt-4", 3).unwrap(), 0);
        assert_eq!(cursor.next_index("openai", "gpt-4", 3).unwrap(), 1);
        assert_eq!(cursor.next_index("openai", "gpt-4", 3).unwrap(), 2);
        assert_eq!(cursor.next_index("openai", "gpt-4", 3).unwrap(), 0);
    }

    #[test]
    fn per_provider_model_cursors_are_independent() {
        let dir = tempdir().unwrap();
        let cursor = RoundRobinCursor::open(dir.path()).unwrap();
        cursor.next_index("openai", "gpt-4", 5).unwrap();
        cursor.next_index("openai", "gpt-4", 5).unwrap();
        assert_eq!(cursor.next_index("openai", "gpt-3.5", 5).unwrap(), 0);
        assert_eq!(cursor.next_index("groq", "gpt-4", 5).unwrap(), 0);
        assert_eq!(cursor.next_index("openai", "gpt-4", 5).unwrap(), 2);
    }

    #[test]
    fn empty_candidates_returns_zero_without_advancing() {
        let dir = tempdir().unwrap();
        let cursor = RoundRobinCursor::open(dir.path()).unwrap();
        assert_eq!(cursor.next_index("openai", "gpt-4", 0).unwrap(), 0);
        assert_eq!(cursor.next_index("openai", "gpt-4", 3).unwrap(), 0);
    }

    #[test]
    fn reset_zeroes_cursor() {
        let dir = tempdir().unwrap();
        let cursor = RoundRobinCursor::open(dir.path()).unwrap();
        cursor.next_index("openai", "gpt-4", 3).unwrap();
        cursor.next_index("openai", "gpt-4", 3).unwrap();
        cursor.reset("openai", "gpt-4").unwrap();
        assert_eq!(cursor.next_index("openai", "gpt-4", 3).unwrap(), 0);
    }
}
