//! Per-user budget schema and policy hook.
//!
//! The schema (two idempotent tables) lives in each user's
//! `~/.jekko/users/<user_id>/state.sqlite`. The [`PolicyHook`] trait lets a
//! router-layer caller gate a request by user before dispatch; the stub
//! [`AlwaysAllow`] impl is the default, so unused budgets never block.
//!
//! Real enforcement is a follow-up: a future `EnforceDailyCaps` impl will
//! read `user_budget` + the day's `user_usage_day` row and refuse when over.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;

/// Idempotent schema added to each per-user `state.sqlite`. Safe to apply
/// repeatedly — the existing `key_usage` table from jekko-runtime is
/// untouched.
pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS user_budget (
    user_id          TEXT NOT NULL PRIMARY KEY,
    daily_token_cap   INTEGER,
    daily_request_cap INTEGER,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_usage_day (
    user_id        TEXT NOT NULL,
    ymd            TEXT NOT NULL,
    tokens_used    INTEGER NOT NULL DEFAULT 0,
    requests_used  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, ymd)
);
";

/// Per-user budget caps. Either cap may be `None` to disable that dimension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserBudget {
    pub user_id: String,
    pub daily_token_cap: Option<i64>,
    pub daily_request_cap: Option<i64>,
    /// Epoch seconds when the budget row was last written.
    pub updated_at: i64,
}

/// Outcome of a budget check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetDecision {
    /// Allow the request and reserve the estimated tokens against the day's
    /// usage. Reconcile actual usage post-completion.
    Allow,
    /// Refuse the request because a cap was reached or exceeded.
    Refuse {
        /// Which cap fired.
        reason: BudgetRefusal,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetRefusal {
    DailyTokens,
    DailyRequests,
}

/// Gate a routed request by per-user budget. Implementations are called once
/// per upstream dispatch by jnoccio-fusion's router (Phase C2 wiring).
pub trait PolicyHook: Send + Sync {
    /// Check the budget for `user_id` and reserve `estimated_tokens` if
    /// allowed. `provider_id` is informational; current implementations
    /// don't shard by provider, but future versions may.
    fn check_and_reserve(
        &self,
        user_id: &str,
        provider_id: &str,
        estimated_tokens: i64,
    ) -> Result<BudgetDecision>;
}

/// Stub `PolicyHook` that always allows. Default when no budget enforcement
/// is configured; production deployments can swap in an `EnforceDailyCaps`
/// implementation that reads [`SCHEMA`] tables.
pub struct AlwaysAllow;

impl PolicyHook for AlwaysAllow {
    fn check_and_reserve(
        &self,
        _user_id: &str,
        _provider_id: &str,
        _estimated_tokens: i64,
    ) -> Result<BudgetDecision> {
        Ok(BudgetDecision::Allow)
    }
}

/// Apply [`SCHEMA`] to a per-user `state.sqlite` at `path`. Idempotent.
pub fn ensure_schema(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch(SCHEMA)
        .with_context(|| format!("init budget schema in {}", path.display()))?;
    Ok(())
}

/// Helper: current UTC date as `YYYY-MM-DD`, used as the `ymd` key in
/// `user_usage_day`.
pub fn today_ymd() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn always_allow_returns_allow_for_any_input() {
        let hook = AlwaysAllow;
        let decision = hook.check_and_reserve("user_1", "openai", 1000).unwrap();
        assert_eq!(decision, BudgetDecision::Allow);
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("state.sqlite");
        ensure_schema(&db).unwrap();
        ensure_schema(&db).unwrap();
        let conn = Connection::open(&db).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('user_budget','user_usage_day') ORDER BY name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            names,
            vec!["user_budget".to_string(), "user_usage_day".to_string()]
        );
    }

    #[test]
    fn today_ymd_format_is_iso() {
        let ymd = today_ymd();
        assert_eq!(ymd.len(), 10);
        assert_eq!(ymd.chars().nth(4), Some('-'));
        assert_eq!(ymd.chars().nth(7), Some('-'));
    }
}
