use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::limits::ErrorKind;

use super::state_util::*;
use super::*;

impl StateDb {
    pub fn record_success(&self, input: RecordSuccessInput<'_>) -> Result<MetricEvent> {
        let RecordSuccessInput {
            request_id,
            phase,
            model_id,
            provider,
            latency_ms,
            winner_model_id,
            usage,
            meta,
            agent,
        } = input;
        let now = now_unix();
        let usage = UsageTotals::from_usage(usage);
        self.upsert_metric_model(model_id, provider)?;
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            r#"
      UPDATE model_state
      SET status = 'healthy',
          failure_count = CASE WHEN failure_count > 0 THEN failure_count - 1 ELSE 0 END,
          success_count = success_count + 1,
          last_latency_ms = ?1,
          last_error_kind = NULL,
          last_error_message = NULL,
          disabled_until = NULL,
          updated_at = ?2
      WHERE model_id = ?3
      "#,
            params![latency_ms as i64, now, model_id],
        )?;
        conn.execute(
            r#"
      UPDATE model_metrics
      SET call_count = MAX(call_count, success_count + failure_count + 1),
          success_count = success_count + 1,
          prompt_tokens = prompt_tokens + ?1,
          completion_tokens = completion_tokens + ?2,
          total_tokens = total_tokens + ?3,
          latency_count = latency_count + 1,
          latency_total_ms = latency_total_ms + ?4,
          latency_min_ms = CASE WHEN latency_min_ms IS NULL THEN ?4 ELSE MIN(latency_min_ms, ?4) END,
          latency_max_ms = CASE WHEN latency_max_ms IS NULL THEN ?4 ELSE MAX(latency_max_ms, ?4) END,
          last_latency_ms = ?4,
          last_error_kind = NULL,
          last_error_message = NULL,
          updated_at = ?5
      WHERE model_id = ?6
      "#,
            params![
                usage.prompt_tokens as i64,
                usage.completion_tokens as i64,
                usage.total_tokens as i64,
                latency_ms as i64,
                now,
                model_id
            ],
        )?;
        conn.execute(
            r#"
      INSERT INTO request_trace (
        request_id, phase, model_id, provider, status, latency_ms, winner_model_id,
        route_mode, backup_rank, complexity_tier, capacity_known, created_at
      )
      VALUES (?1, ?2, ?3, ?4, 'success', ?5, ?6, ?7, ?8, ?9, ?10, ?11)
      "#,
            params![
                request_id,
                phase,
                model_id,
                provider,
                latency_ms as i64,
                winner_model_id,
                &meta.route_mode,
                meta.backup_rank.map(|value| value as i64),
                &meta.complexity_tier,
                option_bool_to_i64(meta.capacity_known),
                now
            ],
        )?;
        let event = MetricEvent {
            id: 0,
            request_id: request_id.to_string(),
            phase: phase.to_string(),
            model_id: model_id.to_string(),
            provider: provider.to_string(),
            status: "success".to_string(),
            error_kind: None,
            latency_ms: Some(latency_ms),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            route_mode: meta.route_mode.clone(),
            backup_rank: meta.backup_rank,
            complexity_tier: meta.complexity_tier.clone(),
            sampled: meta.sampled,
            winner_model_id: winner_model_id.map(str::to_string),
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
            0,
            1,
            0,
            0,
            &usage,
            Some(latency_ms),
        )?;
        prune_metric_events(&conn, self.event_retention_rows)?;
        Ok(MetricEvent { id, ..event })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_failure(
        &self,
        request_id: &str,
        phase: &str,
        model_id: &str,
        provider: &str,
        kind: &ErrorKind,
        latency_ms: u64,
        cooldown_until: Option<i64>,
        message: Option<&str>,
        meta: &RouteEventMeta,
        agent: Option<&AgentSource>,
    ) -> Result<MetricEvent> {
        let now = now_unix();
        let kind_text = format!("{kind:?}");
        self.upsert_metric_model(model_id, provider)?;
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            r#"
      UPDATE model_state
      SET status = ?1,
          failure_count = failure_count + 1,
          last_latency_ms = ?2,
          disabled_until = COALESCE(?3, disabled_until),
          last_error_kind = ?4,
          last_error_message = ?5,
          updated_at = ?6
      WHERE model_id = ?7
      "#,
            params![
                kind_to_status(kind),
                latency_ms as i64,
                cooldown_until,
                kind_text,
                message,
                now,
                model_id
            ],
        )?;
        conn.execute(
            r#"
      UPDATE model_metrics
      SET call_count = MAX(call_count, success_count + failure_count + 1),
          failure_count = failure_count + 1,
          latency_count = latency_count + 1,
          latency_total_ms = latency_total_ms + ?1,
          latency_min_ms = CASE WHEN latency_min_ms IS NULL THEN ?1 ELSE MIN(latency_min_ms, ?1) END,
          latency_max_ms = CASE WHEN latency_max_ms IS NULL THEN ?1 ELSE MAX(latency_max_ms, ?1) END,
          last_latency_ms = ?1,
          last_error_kind = ?2,
          last_error_message = ?3,
          updated_at = ?4
      WHERE model_id = ?5
      "#,
            params![latency_ms as i64, &kind_text, message, now, model_id],
        )?;
        conn.execute(
            r#"
      INSERT INTO request_trace (
        request_id, phase, model_id, provider, status, error_kind, latency_ms, cooldown_until,
        route_mode, backup_rank, complexity_tier, capacity_known, created_at
      )
      VALUES (?1, ?2, ?3, ?4, 'failure', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
      "#,
            params![
                request_id,
                phase,
                model_id,
                provider,
                &kind_text,
                latency_ms as i64,
                cooldown_until,
                &meta.route_mode,
                meta.backup_rank.map(|value| value as i64),
                &meta.complexity_tier,
                option_bool_to_i64(meta.capacity_known),
                now
            ],
        )?;
        let event = MetricEvent {
            id: 0,
            request_id: request_id.to_string(),
            phase: phase.to_string(),
            model_id: model_id.to_string(),
            provider: provider.to_string(),
            status: "failure".to_string(),
            error_kind: Some(format!("{kind:?}")),
            latency_ms: Some(latency_ms),
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
            0,
            0,
            1,
            0,
            &UsageTotals::zero(),
            Some(latency_ms),
        )?;
        prune_metric_events(&conn, self.event_retention_rows)?;
        Ok(MetricEvent { id, ..event })
    }

    pub fn record_winner(&self, model_id: &str) -> Result<MetricEvent> {
        self.record_winner_for_request(
            &uuid::Uuid::new_v4().to_string(),
            model_id,
            &RouteEventMeta::default(),
            None,
        )
    }

    pub fn record_winner_for_request(
        &self,
        request_id: &str,
        model_id: &str,
        meta: &RouteEventMeta,
        agent: Option<&AgentSource>,
    ) -> Result<MetricEvent> {
        let now = now_unix();
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let provider_from_db: Option<String> = conn
            .query_row(
                "SELECT provider FROM model_metrics WHERE model_id = ?1",
                [model_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let provider = match provider_from_db {
            Some(p) => p,
            None => match model_id.split('/').next() {
                Some(prefix) => prefix.to_string(),
                None => "unknown".to_string(),
            },
        };
        conn.execute(
            r#"
      INSERT INTO fusion_score (model_id, attempts, wins, last_won_at)
      VALUES (?1, 1, 1, ?2)
      ON CONFLICT(model_id) DO UPDATE SET
        attempts = attempts + 1,
        wins = wins + 1,
        last_won_at = excluded.last_won_at
      "#,
            params![model_id, now],
        )?;
        conn.execute(
            r#"
      UPDATE model_state
      SET win_count = win_count + 1,
          updated_at = ?1
      WHERE model_id = ?2
            "#,
            params![now, model_id],
        )?;
        conn.execute(
            r#"
      UPDATE model_metrics
      SET win_count = win_count + 1,
          updated_at = ?1
      WHERE model_id = ?2
      "#,
            params![now, model_id],
        )?;
        let event = MetricEvent {
            id: 0,
            request_id: request_id.to_string(),
            phase: "winner".to_string(),
            model_id: model_id.to_string(),
            provider,
            status: "winner".to_string(),
            error_kind: None,
            latency_ms: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            route_mode: meta.route_mode.clone(),
            backup_rank: meta.backup_rank,
            complexity_tier: meta.complexity_tier.clone(),
            sampled: meta.sampled,
            winner_model_id: Some(model_id.to_string()),
            capacity_known: meta.capacity_known,
            agent_id: agent.map(|value| value.id.clone()),
            agent_client: agent.and_then(|value| value.client.clone()),
            agent_session_id: agent.and_then(|value| value.session_id.clone()),
            created_at: now,
        };
        upsert_usage_minute(
            &conn,
            now,
            model_id,
            &event.provider,
            0,
            0,
            0,
            1,
            &UsageTotals::zero(),
            None,
        )?;
        let id = insert_metric_event(&conn, &event)?;
        prune_metric_events(&conn, self.event_retention_rows)?;
        Ok(MetricEvent { id, ..event })
    }

    pub fn metric_snapshot(&self) -> Result<Vec<ModelMetric>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT model_id, provider, call_count, success_count, failure_count, win_count,
             prompt_tokens, completion_tokens, total_tokens,
             latency_count, latency_total_ms, latency_min_ms, latency_max_ms, last_latency_ms,
             last_error_kind, last_error_message, updated_at
      FROM model_metrics
      ORDER BY model_id
      "#,
        )?;
        let rows = stmt.query_map([], model_metric_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn metric_for(&self, model_id: &str) -> Result<Option<ModelMetric>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT model_id, provider, call_count, success_count, failure_count, win_count,
             prompt_tokens, completion_tokens, total_tokens,
             latency_count, latency_total_ms, latency_min_ms, latency_max_ms, last_latency_ms,
             last_error_kind, last_error_message, updated_at
      FROM model_metrics
      WHERE model_id = ?1
      "#,
        )?;
        stmt.query_row([model_id], model_metric_from_row)
            .optional()
            .map_err(Into::into)
    }

    pub fn recent_metric_events(&self, limit: usize) -> Result<Vec<MetricEvent>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT id, request_id, phase, model_id, provider, status, error_kind, latency_ms,
             prompt_tokens, completion_tokens, total_tokens,
             route_mode, backup_rank, complexity_tier, sampled, winner_model_id,
             capacity_known, agent_id, agent_client, agent_session_id, created_at
      FROM model_metric_event
      ORDER BY created_at DESC, id DESC
      LIMIT ?1
      "#,
        )?;
        let rows = stmt.query_map([limit as i64], metric_event_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn recent_metric_events_after(
        &self,
        after_id: i64,
        limit: usize,
    ) -> Result<Vec<MetricEvent>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT id, request_id, phase, model_id, provider, status, error_kind, latency_ms,
             prompt_tokens, completion_tokens, total_tokens,
             route_mode, backup_rank, complexity_tier, sampled, winner_model_id,
             capacity_known, agent_id, agent_client, agent_session_id, created_at
      FROM model_metric_event
      WHERE id > ?1
      ORDER BY id ASC
      LIMIT ?2
      "#,
        )?;
        let rows = stmt.query_map([after_id, limit as i64], metric_event_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}
