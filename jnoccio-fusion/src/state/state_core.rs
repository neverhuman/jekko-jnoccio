use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

use super::state_util::*;
use super::*;
impl StateDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_retention(path, 50_000)
    }

    pub fn open_with_retention(
        path: impl AsRef<Path>,
        event_retention_rows: usize,
    ) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let conn = Connection::open(path.as_ref())
            .with_context(|| format!("open {}", path.as_ref().display()))?;
        let db = Self {
            conn: Mutex::new(conn),
            event_retention_rows: event_retention_rows.max(1),
        };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute_batch(
            r#"
      PRAGMA journal_mode = WAL;
      PRAGMA busy_timeout = 5000;
      PRAGMA synchronous = NORMAL;
      CREATE TABLE IF NOT EXISTS model_state (
        model_id TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        status TEXT NOT NULL,
        failure_count INTEGER NOT NULL DEFAULT 0,
        success_count INTEGER NOT NULL DEFAULT 0,
        win_count INTEGER NOT NULL DEFAULT 0,
        last_latency_ms INTEGER,
        disabled_until INTEGER,
        last_error_kind TEXT,
        last_error_message TEXT,
        updated_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS request_trace (
        request_id TEXT NOT NULL,
        phase TEXT NOT NULL,
        model_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        status TEXT NOT NULL,
        error_kind TEXT,
        latency_ms INTEGER,
        cooldown_until INTEGER,
        winner_model_id TEXT,
        route_mode TEXT,
        backup_rank INTEGER,
        complexity_tier TEXT,
        capacity_known INTEGER,
        created_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS request_route (
        request_id TEXT PRIMARY KEY,
        route_mode TEXT NOT NULL,
        sampled INTEGER NOT NULL,
        complexity_tier TEXT NOT NULL,
        complexity_score INTEGER NOT NULL,
        primary_model_id TEXT,
        backup_model_ids TEXT NOT NULL,
        fusion_model_id TEXT,
        created_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS fusion_score (
        model_id TEXT PRIMARY KEY,
        attempts INTEGER NOT NULL DEFAULT 0,
        wins INTEGER NOT NULL DEFAULT 0,
        last_won_at INTEGER
      );
      CREATE TABLE IF NOT EXISTS provider_quota (
        model_id TEXT PRIMARY KEY,
        requests_today INTEGER NOT NULL DEFAULT 0,
        window_started_at INTEGER NOT NULL DEFAULT 0,
        disabled_until INTEGER
      );
      CREATE TABLE IF NOT EXISTS model_metrics (
        model_id TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        call_count INTEGER NOT NULL DEFAULT 0,
        success_count INTEGER NOT NULL DEFAULT 0,
        failure_count INTEGER NOT NULL DEFAULT 0,
        win_count INTEGER NOT NULL DEFAULT 0,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        total_tokens INTEGER NOT NULL DEFAULT 0,
        latency_count INTEGER NOT NULL DEFAULT 0,
        latency_total_ms INTEGER NOT NULL DEFAULT 0,
        latency_min_ms INTEGER,
        latency_max_ms INTEGER,
        last_latency_ms INTEGER,
        last_error_kind TEXT,
        last_error_message TEXT,
        updated_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS model_metric_event (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        request_id TEXT NOT NULL,
        phase TEXT NOT NULL,
        model_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        status TEXT NOT NULL,
        error_kind TEXT,
        latency_ms INTEGER,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        total_tokens INTEGER NOT NULL DEFAULT 0,
        route_mode TEXT,
        backup_rank INTEGER,
        complexity_tier TEXT,
        sampled INTEGER,
        winner_model_id TEXT,
        capacity_known INTEGER,
        agent_id TEXT,
        agent_client TEXT,
        agent_session_id TEXT,
        created_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS agent_activity (
        agent_id TEXT PRIMARY KEY,
        agent_client TEXT,
        agent_session_id TEXT,
        process_role TEXT,
        pid INTEGER,
        version TEXT,
        user_agent TEXT,
        first_seen INTEGER NOT NULL,
        last_seen INTEGER NOT NULL,
        request_count INTEGER NOT NULL DEFAULT 1
      );
      CREATE INDEX IF NOT EXISTS idx_agent_activity_last_seen
        ON agent_activity (last_seen DESC);
      CREATE TABLE IF NOT EXISTS model_usage_minute (
        model_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        minute_ts INTEGER NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0,
        successes INTEGER NOT NULL DEFAULT 0,
        failures INTEGER NOT NULL DEFAULT 0,
        wins INTEGER NOT NULL DEFAULT 0,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        total_tokens INTEGER NOT NULL DEFAULT 0,
        latency_count INTEGER NOT NULL DEFAULT 0,
        latency_total_ms INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (model_id, minute_ts)
      );
      CREATE TABLE IF NOT EXISTS model_limit_estimate (
        model_id TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        configured_context_window INTEGER NOT NULL DEFAULT 0,
        learned_context_window INTEGER,
        learned_request_token_limit INTEGER,
        learned_tpm_limit INTEGER,
        safe_context_window INTEGER NOT NULL DEFAULT 0,
        largest_success_prompt_tokens INTEGER NOT NULL DEFAULT 0,
        largest_success_total_tokens INTEGER NOT NULL DEFAULT 0,
        smallest_overrun_requested_tokens INTEGER,
        context_overrun_count INTEGER NOT NULL DEFAULT 0,
        rate_limit_count INTEGER NOT NULL DEFAULT 0,
        last_limit_error_kind TEXT,
        last_limit_error_message TEXT,
        last_limit_error_at INTEGER,
        updated_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS model_context_event (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        request_id TEXT NOT NULL,
        phase TEXT NOT NULL,
        model_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        status TEXT NOT NULL,
        approx_prompt_tokens INTEGER NOT NULL,
        requested_output_tokens INTEGER NOT NULL DEFAULT 0,
        estimated_total_tokens INTEGER NOT NULL,
        observed_prompt_tokens INTEGER,
        observed_total_tokens INTEGER,
        learned_limit INTEGER,
        overrun_requested_tokens INTEGER,
        error_kind TEXT,
        created_at INTEGER NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_model_metric_event_created_at
        ON model_metric_event (created_at DESC, id DESC);
      CREATE INDEX IF NOT EXISTS idx_model_metric_event_model_id
        ON model_metric_event (model_id, created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_model_usage_minute_minute_ts
        ON model_usage_minute (minute_ts DESC);
      CREATE INDEX IF NOT EXISTS idx_request_route_created_at
        ON request_route (created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_model_context_event_model_id
        ON model_context_event (model_id, created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_model_context_event_created_at
        ON model_context_event (created_at DESC);
      "#,
        )?;
        ensure_column(&conn, "request_trace", "route_mode", "TEXT")?;
        ensure_column(&conn, "request_trace", "backup_rank", "INTEGER")?;
        ensure_column(&conn, "request_trace", "complexity_tier", "TEXT")?;
        ensure_column(&conn, "request_trace", "capacity_known", "INTEGER")?;
        ensure_column(&conn, "model_metric_event", "route_mode", "TEXT")?;
        ensure_column(&conn, "model_metric_event", "backup_rank", "INTEGER")?;
        ensure_column(&conn, "model_metric_event", "complexity_tier", "TEXT")?;
        ensure_column(&conn, "model_metric_event", "sampled", "INTEGER")?;
        ensure_column(&conn, "model_metric_event", "winner_model_id", "TEXT")?;
        ensure_column(&conn, "model_metric_event", "capacity_known", "INTEGER")?;
        ensure_column(&conn, "model_metric_event", "agent_id", "TEXT")?;
        ensure_column(&conn, "model_metric_event", "agent_client", "TEXT")?;
        ensure_column(&conn, "model_metric_event", "agent_session_id", "TEXT")?;
        ensure_column(&conn, "agent_activity", "agent_session_id", "TEXT")?;
        ensure_column(&conn, "agent_activity", "process_role", "TEXT")?;
        ensure_column(&conn, "agent_activity", "pid", "INTEGER")?;
        ensure_column(&conn, "agent_activity", "version", "TEXT")?;
        ensure_column(&conn, "agent_activity", "user_agent", "TEXT")?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<ModelState>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
      r#"
      SELECT model_id, provider, status, failure_count, success_count, win_count, last_latency_ms, disabled_until, last_error_kind, last_error_message, updated_at
      FROM model_state
      ORDER BY model_id
      "#,
    )?;
        let rows = stmt.query_map([], |row| {
            Ok(ModelState {
                model_id: row.get(0)?,
                provider: row.get(1)?,
                status: row.get(2)?,
                failure_count: row.get::<_, i64>(3)? as u64,
                success_count: row.get::<_, i64>(4)? as u64,
                win_count: row.get::<_, i64>(5)? as u64,
                last_latency_ms: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
                disabled_until: row.get(7)?,
                last_error_kind: row.get(8)?,
                last_error_message: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn state_for(&self, model_id: &str) -> Result<Option<ModelState>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
      r#"
      SELECT model_id, provider, status, failure_count, success_count, win_count, last_latency_ms, disabled_until, last_error_kind, last_error_message, updated_at
      FROM model_state
      WHERE model_id = ?1
      "#,
    )?;
        stmt.query_row([model_id], |row| {
            Ok(ModelState {
                model_id: row.get(0)?,
                provider: row.get(1)?,
                status: row.get(2)?,
                failure_count: row.get::<_, i64>(3)? as u64,
                success_count: row.get::<_, i64>(4)? as u64,
                win_count: row.get::<_, i64>(5)? as u64,
                last_latency_ms: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
                disabled_until: row.get(7)?,
                last_error_kind: row.get(8)?,
                last_error_message: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn upsert_model(&self, model_id: &str, provider: &str, status: &str) -> Result<()> {
        let now = now_unix();
        let existing = self.state_for(model_id)?;
        if let Some(existing) = existing {
            let readiness_changed = matches!(status, "missing_key" | "incomplete_env");
            if readiness_changed {
                let conn = self.conn.lock().expect("sqlite mutex poisoned");
                conn.execute(
                    r#"
          UPDATE model_state
          SET provider = ?1,
              status = ?2,
              disabled_until = NULL,
              last_error_kind = NULL,
              last_error_message = NULL,
              updated_at = ?3
          WHERE model_id = ?4
          "#,
                    params![provider, status, now, model_id],
                )?;
                drop(conn);
                self.upsert_metric_model(model_id, provider)?;
                return Ok(());
            }
            if preserves_startup_status(&existing) {
                let conn = self.conn.lock().expect("sqlite mutex poisoned");
                conn.execute(
                    r#"
          UPDATE model_state
          SET provider = ?1,
              updated_at = ?2
          WHERE model_id = ?3
          "#,
                    params![provider, now, model_id],
                )?;
                drop(conn);
                self.upsert_metric_model(model_id, provider)?;
                return Ok(());
            }
            let conn = self.conn.lock().expect("sqlite mutex poisoned");
            conn.execute(
                r#"
        UPDATE model_state
        SET provider = ?1,
            status = ?2,
            updated_at = ?3
        WHERE model_id = ?4
        "#,
                params![provider, status, now, model_id],
            )?;
            drop(conn);
            self.upsert_metric_model(model_id, provider)?;
            return Ok(());
        }
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
      r#"
      INSERT INTO model_state (model_id, provider, status, failure_count, success_count, win_count, updated_at)
      VALUES (?1, ?2, ?3, 0, 0, 0, ?4)
      "#,
      params![model_id, provider, status, now],
    )?;
        drop(conn);
        self.upsert_metric_model(model_id, provider)?;
        Ok(())
    }

    pub fn record_agent_activity(&self, agent: &AgentSource) -> Result<()> {
        let now = now_unix();
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            r#"
      INSERT INTO agent_activity (
        agent_id, agent_client, agent_session_id, process_role, pid, version, user_agent,
        first_seen, last_seen, request_count
      )
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)
      ON CONFLICT(agent_id) DO UPDATE SET
        agent_client = excluded.agent_client,
        agent_session_id = excluded.agent_session_id,
        process_role = excluded.process_role,
        pid = excluded.pid,
        version = excluded.version,
        user_agent = excluded.user_agent,
        last_seen = excluded.last_seen,
        request_count = request_count + 1
      "#,
            params![
                &agent.id,
                agent.client,
                agent.session_id,
                agent.process_role,
                agent.pid,
                agent.version,
                agent.user_agent,
                now
            ],
        )?;
        Ok(())
    }

    pub fn active_agents(&self, now: i64, ttl_seconds: i64) -> Result<Vec<AgentActivity>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT agent_id, agent_client, agent_session_id, process_role, pid, version, user_agent,
             first_seen, last_seen, request_count
      FROM agent_activity
      WHERE last_seen >= ?1
      ORDER BY last_seen DESC
      "#,
        )?;
        let rows = stmt.query_map([now - ttl_seconds], |row| {
            Ok(AgentActivity {
                agent_id: row.get(0)?,
                agent_client: row.get(1)?,
                agent_session_id: row.get(2)?,
                process_role: row.get(3)?,
                pid: row.get(4)?,
                version: row.get(5)?,
                user_agent: row.get(6)?,
                first_seen: row.get(7)?,
                last_seen: row.get(8)?,
                request_count: row.get::<_, i64>(9)? as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn active_agents_live(&self, now: i64) -> Result<Vec<AgentActivity>> {
        self.active_agents(now, AGENT_ACTIVITY_TTL_SECONDS)
    }

    pub fn prune_expired_agent_activity(&self, ttl_seconds: i64, now: i64) -> Result<usize> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let rows_deleted = conn.execute(
            "DELETE FROM agent_activity WHERE last_seen < ?1",
            [now - ttl_seconds],
        )?;
        Ok(rows_deleted)
    }
}
