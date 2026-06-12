use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::limits::ErrorKind;

use super::*;
pub(crate) fn kind_to_status(kind: &ErrorKind) -> &'static str {
    match kind {
        ErrorKind::AuthFailed => "auth_failed",
        ErrorKind::RateLimited => "rate_limited",
        ErrorKind::Timeout => "timeout",
        ErrorKind::ServerError => "server_error",
        ErrorKind::InvalidResponse => "invalid_response",
        ErrorKind::ContextOverflow => "context_overflow",
        ErrorKind::CustomerVerificationRequired => "customer_verification_required",
        ErrorKind::NoAccess => "no_access",
        ErrorKind::UnsupportedApi => "unsupported_api",
        ErrorKind::ModelUnavailable => "model_unavailable",
        ErrorKind::QuotaExhausted => "quota_exhausted",
        ErrorKind::Unknown => "unhealthy",
    }
}

pub(crate) fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let exists = rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == column);
    if exists {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn upsert_usage_minute(
    conn: &Connection,
    created_at: i64,
    model_id: &str,
    provider: &str,
    attempts: u64,
    successes: u64,
    failures: u64,
    wins: u64,
    usage: &UsageTotals,
    latency_ms: Option<u64>,
) -> Result<()> {
    conn.execute(
        r#"
      INSERT INTO model_usage_minute (
        model_id, provider, minute_ts, attempts, successes, failures, wins,
        prompt_tokens, completion_tokens, total_tokens, latency_count, latency_total_ms
      )
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
      ON CONFLICT(model_id, minute_ts) DO UPDATE SET
        provider = excluded.provider,
        attempts = attempts + excluded.attempts,
        successes = successes + excluded.successes,
        failures = failures + excluded.failures,
        wins = wins + excluded.wins,
        prompt_tokens = prompt_tokens + excluded.prompt_tokens,
        completion_tokens = completion_tokens + excluded.completion_tokens,
        total_tokens = total_tokens + excluded.total_tokens,
        latency_count = latency_count + excluded.latency_count,
        latency_total_ms = latency_total_ms + excluded.latency_total_ms
      "#,
        params![
            model_id,
            provider,
            minute_floor(created_at),
            attempts as i64,
            successes as i64,
            failures as i64,
            wins as i64,
            usage.prompt_tokens as i64,
            usage.completion_tokens as i64,
            usage.total_tokens as i64,
            latency_ms.map(|_| 1).unwrap_or(0),
            latency_ms.unwrap_or(0) as i64
        ],
    )?;
    Ok(())
}

pub(crate) fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

pub(crate) fn option_bool_to_i64(value: Option<bool>) -> Option<i64> {
    value.map(bool_to_i64)
}

pub(crate) fn minute_floor(value: i64) -> i64 {
    value - value.rem_euclid(60)
}

pub(crate) fn median_f64(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

pub(crate) fn project_tokens_per_minute_to_day_millions(tokens_per_minute: f64) -> f64 {
    tokens_per_minute * 1_440.0 / 1_000_000.0
}

pub(crate) fn insert_metric_event(conn: &Connection, event: &MetricEvent) -> Result<i64> {
    conn.execute(
        r#"
      INSERT INTO model_metric_event (
        request_id, phase, model_id, provider, status, error_kind, latency_ms,
        prompt_tokens, completion_tokens, total_tokens, route_mode, backup_rank,
        complexity_tier, sampled, winner_model_id, capacity_known, agent_id, agent_client, agent_session_id, created_at
      )
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
      "#,
        params![
            &event.request_id,
            &event.phase,
            &event.model_id,
            &event.provider,
            &event.status,
            &event.error_kind,
            event.latency_ms.map(|value| value as i64),
            event.prompt_tokens as i64,
            event.completion_tokens as i64,
            event.total_tokens as i64,
            &event.route_mode,
            event.backup_rank.map(|value| value as i64),
            &event.complexity_tier,
            option_bool_to_i64(event.sampled),
            &event.winner_model_id,
            option_bool_to_i64(event.capacity_known),
            &event.agent_id,
            &event.agent_client,
            &event.agent_session_id,
            event.created_at
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub(crate) fn prune_metric_events(conn: &Connection, retention_rows: usize) -> Result<()> {
    conn.execute(
        r#"
      DELETE FROM model_metric_event
      WHERE id NOT IN (
        SELECT id
        FROM model_metric_event
        ORDER BY created_at DESC, id DESC
        LIMIT ?1
      )
      "#,
        [retention_rows as i64],
    )?;
    Ok(())
}

pub(crate) fn model_metric_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelMetric> {
    Ok(ModelMetric {
        model_id: row.get(0)?,
        provider: row.get(1)?,
        call_count: row.get::<_, i64>(2)? as u64,
        success_count: row.get::<_, i64>(3)? as u64,
        failure_count: row.get::<_, i64>(4)? as u64,
        win_count: row.get::<_, i64>(5)? as u64,
        prompt_tokens: row.get::<_, i64>(6)? as u64,
        completion_tokens: row.get::<_, i64>(7)? as u64,
        total_tokens: row.get::<_, i64>(8)? as u64,
        latency_count: row.get::<_, i64>(9)? as u64,
        latency_total_ms: row.get::<_, i64>(10)? as u64,
        latency_min_ms: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
        latency_max_ms: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
        last_latency_ms: row.get::<_, Option<i64>>(13)?.map(|value| value as u64),
        last_error_kind: row.get(14)?,
        last_error_message: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

pub(crate) fn metric_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MetricEvent> {
    Ok(MetricEvent {
        id: row.get(0)?,
        request_id: row.get(1)?,
        phase: row.get(2)?,
        model_id: row.get(3)?,
        provider: row.get(4)?,
        status: row.get(5)?,
        error_kind: row.get(6)?,
        latency_ms: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
        prompt_tokens: row.get::<_, i64>(8)? as u64,
        completion_tokens: row.get::<_, i64>(9)? as u64,
        total_tokens: row.get::<_, i64>(10)? as u64,
        route_mode: row.get(11)?,
        backup_rank: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
        complexity_tier: row.get(13)?,
        sampled: row.get::<_, Option<i64>>(14)?.map(|value| value != 0),
        winner_model_id: row.get(15)?,
        capacity_known: row.get::<_, Option<i64>>(16)?.map(|value| value != 0),
        agent_id: row.get(17)?,
        agent_client: row.get(18)?,
        agent_session_id: row.get(19)?,
        created_at: row.get(20)?,
    })
}

pub(crate) fn model_limit_estimate_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ModelLimitEstimate> {
    Ok(ModelLimitEstimate {
        model_id: row.get(0)?,
        provider: row.get(1)?,
        configured_context_window: row.get::<_, i64>(2)? as u64,
        learned_context_window: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
        learned_request_token_limit: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
        learned_tpm_limit: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
        safe_context_window: row.get::<_, i64>(6)? as u64,
        largest_success_prompt_tokens: row.get::<_, i64>(7)? as u64,
        largest_success_total_tokens: row.get::<_, i64>(8)? as u64,
        smallest_overrun_requested_tokens: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
        context_overrun_count: row.get::<_, i64>(10)? as u64,
        rate_limit_count: row.get::<_, i64>(11)? as u64,
        last_limit_error_kind: row.get(12)?,
        last_limit_error_message: row.get(13)?,
        last_limit_error_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

pub(crate) fn model_context_event_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ModelContextEvent> {
    Ok(ModelContextEvent {
        request_id: row.get(0)?,
        phase: row.get(1)?,
        model_id: row.get(2)?,
        provider: row.get(3)?,
        status: row.get(4)?,
        approx_prompt_tokens: row.get::<_, i64>(5)? as u64,
        requested_output_tokens: row.get::<_, i64>(6)? as u64,
        estimated_total_tokens: row.get::<_, i64>(7)? as u64,
        observed_prompt_tokens: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
        observed_total_tokens: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
        learned_limit: row.get::<_, Option<i64>>(10)?.map(|value| value as u64),
        overrun_requested_tokens: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
        error_kind: row.get(12)?,
        created_at: row.get(13)?,
    })
}

pub(crate) fn ensure_limit_row(
    conn: &Connection,
    model_id: &str,
    provider: &str,
    now: i64,
) -> Result<()> {
    conn.execute(
        r#"
      INSERT INTO model_limit_estimate (model_id, provider, safe_context_window, updated_at)
      VALUES (?1, ?2, 0, ?3)
      ON CONFLICT(model_id) DO NOTHING
      "#,
        params![model_id, provider, now],
    )?;
    Ok(())
}

pub(crate) fn recompute_limit_safe(conn: &Connection, model_id: &str) -> Result<()> {
    let estimate = conn
        .query_row(
            r#"
      SELECT model_id, provider, configured_context_window, learned_context_window,
             learned_request_token_limit, learned_tpm_limit, safe_context_window,
             largest_success_prompt_tokens, largest_success_total_tokens,
             smallest_overrun_requested_tokens, context_overrun_count, rate_limit_count,
             last_limit_error_kind, last_limit_error_message, last_limit_error_at, updated_at
      FROM model_limit_estimate
      WHERE model_id = ?1
      "#,
            [model_id],
            model_limit_estimate_from_row,
        )
        .optional()?;
    if let Some(estimate) = estimate {
        conn.execute(
            "UPDATE model_limit_estimate SET safe_context_window = ?1 WHERE model_id = ?2",
            params![compute_safe_context_window(&estimate) as i64, model_id],
        )?;
    }
    Ok(())
}

pub(crate) fn compute_safe_context_window(estimate: &ModelLimitEstimate) -> u64 {
    let learned_caps = [
        estimate
            .learned_context_window
            .map(|value| value.saturating_mul(95) / 100),
        estimate
            .learned_request_token_limit
            .map(|value| value.saturating_mul(90) / 100),
        estimate
            .learned_tpm_limit
            .map(|value| value.saturating_mul(90) / 100),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if learned_caps.is_empty() {
        return estimate
            .configured_context_window
            .max(estimate.largest_success_total_tokens);
    }
    let cap = learned_caps.iter().min().copied().unwrap_or(0);
    if estimate.configured_context_window == 0 {
        return cap;
    }
    cap.min(estimate.configured_context_window)
}

pub(crate) fn preserves_startup_status(state: &ModelState) -> bool {
    if state.disabled_until.is_some() {
        return true;
    }
    !matches!(
        state.status.as_str(),
        "ready" | "healthy" | "missing_key" | "incomplete_env"
    )
}

pub(crate) fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}
