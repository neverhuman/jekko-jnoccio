use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::limits::{ErrorKind, ParsedLimitSignal};
use crate::openai::ChatUsage;

use super::state_util::*;
use super::*;

impl StateDb {
    pub fn record_attempt(
        &self,
        request_id: &str,
        phase: &str,
        model_id: &str,
        provider: &str,
        meta: &RouteEventMeta,
        agent: Option<&AgentSource>,
    ) -> Result<MetricEvent> {
        let now = now_unix();
        self.upsert_metric_model(model_id, provider)?;
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            r#"
      INSERT INTO request_trace (
        request_id, phase, model_id, provider, status, route_mode, backup_rank,
        complexity_tier, capacity_known, created_at
      )
      VALUES (?1, ?2, ?3, ?4, 'attempt', ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                request_id,
                phase,
                model_id,
                provider,
                &meta.route_mode,
                meta.backup_rank.map(|value| value as i64),
                &meta.complexity_tier,
                option_bool_to_i64(meta.capacity_known),
                now
            ],
        )?;
        conn.execute(
            r#"
      UPDATE model_metrics
      SET call_count = call_count + 1,
          updated_at = ?1
      WHERE model_id = ?2
      "#,
            params![now, model_id],
        )?;
        let event = MetricEvent {
            id: 0,
            request_id: request_id.to_string(),
            phase: phase.to_string(),
            model_id: model_id.to_string(),
            provider: provider.to_string(),
            status: "attempt".to_string(),
            error_kind: None,
            latency_ms: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            route_mode: meta.route_mode.clone(),
            backup_rank: meta.backup_rank,
            complexity_tier: meta.complexity_tier.clone(),
            sampled: meta.sampled,
            winner_model_id: None,
            capacity_known: meta.capacity_known,
            agent_id: agent.map(|value| value.id.clone()),
            agent_client: agent.and_then(|value| value.client.clone()),
            agent_session_id: agent.and_then(|value| value.session_id.clone()),
            created_at: now,
        };
        let id = insert_metric_event(&conn, &event)?;
        upsert_usage_minute(
            &conn,
            now,
            model_id,
            provider,
            1,
            0,
            0,
            0,
            &UsageTotals::zero(),
            None,
        )?;
        prune_metric_events(&conn, self.event_retention_rows)?;
        Ok(MetricEvent { id, ..event })
    }

    pub fn upsert_metric_model(&self, model_id: &str, provider: &str) -> Result<()> {
        let now = now_unix();
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let existing = conn
            .query_row(
                "SELECT model_id FROM model_metrics WHERE model_id = ?1",
                [model_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if existing.is_some() {
            conn.execute(
                r#"
        UPDATE model_metrics
        SET provider = ?1,
            updated_at = ?2
        WHERE model_id = ?3
        "#,
                params![provider, now, model_id],
            )?;
            return Ok(());
        }
        let historical = conn
            .query_row(
                r#"
        SELECT success_count, failure_count, win_count, last_latency_ms, last_error_kind, last_error_message
        FROM model_state
        WHERE model_id = ?1
        "#,
                [model_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let (
            success_count,
            failure_count,
            win_count,
            last_latency_ms,
            last_error_kind,
            last_error_message,
        ) = historical.unwrap_or((0, 0, 0, None, None, None));
        conn.execute(
            r#"
      INSERT INTO model_metrics (
        model_id, provider, call_count, success_count, failure_count, win_count,
        latency_count, latency_total_ms, latency_min_ms, latency_max_ms, last_latency_ms,
        last_error_kind, last_error_message, updated_at
      )
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
      "#,
            params![
                model_id,
                provider,
                success_count + failure_count,
                success_count,
                failure_count,
                win_count,
                last_latency_ms.map(|_| 1).unwrap_or(0),
                last_latency_ms.unwrap_or(0),
                last_latency_ms,
                last_latency_ms,
                last_latency_ms,
                last_error_kind,
                last_error_message,
                now
            ],
        )?;
        Ok(())
    }

    pub fn upsert_limit_model(
        &self,
        model_id: &str,
        provider: &str,
        configured_context_window: u64,
    ) -> Result<()> {
        let now = now_unix();
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            r#"
      INSERT INTO model_limit_estimate (
        model_id, provider, configured_context_window, safe_context_window, updated_at
      )
      VALUES (?1, ?2, ?3, ?3, ?4)
      ON CONFLICT(model_id) DO UPDATE SET
        provider = excluded.provider,
        configured_context_window = excluded.configured_context_window,
        updated_at = excluded.updated_at
      "#,
            params![model_id, provider, configured_context_window as i64, now],
        )?;
        recompute_limit_safe(&conn, model_id)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_context_success(
        &self,
        request_id: &str,
        phase: &str,
        model_id: &str,
        provider: &str,
        approx_prompt_tokens: u64,
        requested_output_tokens: u64,
        usage: Option<&ChatUsage>,
    ) -> Result<()> {
        let now = now_unix();
        let observed_prompt_tokens = match usage {
            Some(item) => item.prompt_tokens,
            None => None,
        };
        let observed_total_tokens = match usage {
            Some(item) => match item.total_tokens {
                Some(total_tokens) => Some(total_tokens),
                None => item
                    .prompt_tokens
                    .zip(item.completion_tokens)
                    .map(|(prompt_tokens, completion_tokens)| prompt_tokens + completion_tokens),
            },
            None => None,
        };
        let estimated_total_tokens = approx_prompt_tokens.saturating_add(requested_output_tokens);
        let updated_prompt_tokens = match observed_prompt_tokens {
            Some(value) => value,
            None => approx_prompt_tokens,
        };
        let updated_total_tokens = match observed_total_tokens {
            Some(value) => value,
            None => estimated_total_tokens,
        };
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        ensure_limit_row(&conn, model_id, provider, now)?;
        conn.execute(
            r#"
      INSERT INTO model_context_event (
        request_id, phase, model_id, provider, status, approx_prompt_tokens,
        requested_output_tokens, estimated_total_tokens, observed_prompt_tokens,
        observed_total_tokens, created_at
      )
      VALUES (?1, ?2, ?3, ?4, 'success', ?5, ?6, ?7, ?8, ?9, ?10)
      "#,
            params![
                request_id,
                phase,
                model_id,
                provider,
                approx_prompt_tokens as i64,
                requested_output_tokens as i64,
                estimated_total_tokens as i64,
                observed_prompt_tokens.map(|value| value as i64),
                observed_total_tokens.map(|value| value as i64),
                now
            ],
        )?;
        conn.execute(
            r#"
      UPDATE model_limit_estimate
      SET largest_success_prompt_tokens = MAX(largest_success_prompt_tokens, ?1),
          largest_success_total_tokens = MAX(largest_success_total_tokens, ?2),
          updated_at = ?3
      WHERE model_id = ?4
      "#,
            params![
                updated_prompt_tokens as i64,
                updated_total_tokens as i64,
                now,
                model_id
            ],
        )?;
        recompute_limit_safe(&conn, model_id)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_context_failure(
        &self,
        request_id: &str,
        phase: &str,
        model_id: &str,
        provider: &str,
        approx_prompt_tokens: u64,
        requested_output_tokens: u64,
        signal: Option<&ParsedLimitSignal>,
        kind: &ErrorKind,
        message: &str,
    ) -> Result<()> {
        let now = now_unix();
        let estimated_total_tokens = approx_prompt_tokens.saturating_add(requested_output_tokens);
        let learned_limit = match signal {
            Some(signal) => match signal.learned_context_window {
                Some(value) => Some(value),
                None => match signal.learned_request_token_limit {
                    Some(value) => Some(value),
                    None => signal.learned_tpm_limit,
                },
            },
            None => None,
        };
        let overrun_requested_tokens = match signal {
            Some(signal) => match signal.requested_tokens.or(signal.message_tokens) {
                Some(tokens) => Some(tokens),
                None => Some(estimated_total_tokens),
            },
            None => Some(estimated_total_tokens),
        };
        let learned_context_window = match signal {
            Some(signal) => signal.learned_context_window,
            None => None,
        };
        let learned_request_token_limit = match signal {
            Some(signal) => signal.learned_request_token_limit,
            None => None,
        };
        let learned_tpm_limit = match signal {
            Some(signal) => signal.learned_tpm_limit,
            None => None,
        };
        let kind_text = format!("{kind:?}");
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        ensure_limit_row(&conn, model_id, provider, now)?;
        conn.execute(
            r#"
      INSERT INTO model_context_event (
        request_id, phase, model_id, provider, status, approx_prompt_tokens,
        requested_output_tokens, estimated_total_tokens, learned_limit,
        overrun_requested_tokens, error_kind, created_at
      )
      VALUES (?1, ?2, ?3, ?4, 'failure', ?5, ?6, ?7, ?8, ?9, ?10, ?11)
      "#,
            params![
                request_id,
                phase,
                model_id,
                provider,
                approx_prompt_tokens as i64,
                requested_output_tokens as i64,
                estimated_total_tokens as i64,
                learned_limit.map(|value| value as i64),
                overrun_requested_tokens.map(|value| value as i64),
                &kind_text,
                now
            ],
        )?;
        conn.execute(
            r#"
      UPDATE model_limit_estimate
      SET learned_context_window = CASE
            WHEN ?1 IS NULL THEN learned_context_window
            WHEN learned_context_window IS NULL THEN ?1
            ELSE MIN(learned_context_window, ?1)
          END,
          learned_request_token_limit = CASE
            WHEN ?2 IS NULL THEN learned_request_token_limit
            WHEN learned_request_token_limit IS NULL THEN ?2
            ELSE MIN(learned_request_token_limit, ?2)
          END,
          learned_tpm_limit = CASE
            WHEN ?3 IS NULL THEN learned_tpm_limit
            WHEN learned_tpm_limit IS NULL THEN ?3
            ELSE MIN(learned_tpm_limit, ?3)
          END,
          smallest_overrun_requested_tokens = CASE
            WHEN ?4 IS NULL THEN smallest_overrun_requested_tokens
            WHEN smallest_overrun_requested_tokens IS NULL THEN ?4
            ELSE MIN(smallest_overrun_requested_tokens, ?4)
          END,
          context_overrun_count = context_overrun_count + ?5,
          rate_limit_count = rate_limit_count + ?6,
          last_limit_error_kind = ?7,
          last_limit_error_message = ?8,
          last_limit_error_at = ?9,
          updated_at = ?9
      WHERE model_id = ?10
      "#,
            params![
                learned_context_window.map(|value| value as i64),
                learned_request_token_limit.map(|value| value as i64),
                learned_tpm_limit.map(|value| value as i64),
                overrun_requested_tokens.map(|value| value as i64),
                if matches!(kind, ErrorKind::ContextOverflow) || signal.is_some() {
                    1
                } else {
                    0
                },
                if matches!(kind, ErrorKind::RateLimited | ErrorKind::QuotaExhausted) {
                    1
                } else {
                    0
                },
                kind_text,
                message,
                now,
                model_id
            ],
        )?;
        recompute_limit_safe(&conn, model_id)?;
        Ok(())
    }
}
