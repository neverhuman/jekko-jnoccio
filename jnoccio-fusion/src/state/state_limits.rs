use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::state_util::*;
use super::*;

impl StateDb {
    pub fn limit_estimates(&self) -> Result<Vec<ModelLimitEstimate>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT model_id, provider, configured_context_window, learned_context_window,
             learned_request_token_limit, learned_tpm_limit, safe_context_window,
             largest_success_prompt_tokens, largest_success_total_tokens,
             smallest_overrun_requested_tokens, context_overrun_count, rate_limit_count,
             last_limit_error_kind, last_limit_error_message, last_limit_error_at, updated_at
      FROM model_limit_estimate
      ORDER BY model_id
      "#,
        )?;
        let rows = stmt.query_map([], model_limit_estimate_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn context_events(&self, limit: usize) -> Result<Vec<ModelContextEvent>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT request_id, phase, model_id, provider, status, approx_prompt_tokens,
             requested_output_tokens, estimated_total_tokens, observed_prompt_tokens,
             observed_total_tokens, learned_limit, overrun_requested_tokens,
             error_kind, created_at
      FROM model_context_event
      ORDER BY created_at DESC, id DESC
      LIMIT ?1
      "#,
        )?;
        let rows = stmt.query_map([limit as i64], model_context_event_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn context_histogram(
        &self,
        model_id: Option<&str>,
        bucket_size: u64,
        since_ts: i64,
    ) -> Result<Vec<ContextHistogramBucket>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let bucket_size = bucket_size.max(1) as i64;
        let sql = format!(
            r#"
      SELECT (estimated_total_tokens / ?1) * ?2 AS bucket_start,
             SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
             SUM(CASE WHEN status = 'failure' THEN 1 ELSE 0 END),
             SUM(CASE WHEN error_kind = 'ContextOverflow' THEN 1 ELSE 0 END)
      FROM model_context_event
      WHERE created_at >= ?3 {}
      GROUP BY bucket_start
      ORDER BY bucket_start
      "#,
            if model_id.is_some() {
                "AND model_id = ?4"
            } else {
                ""
            }
        );
        let mut stmt = conn.prepare(&sql)?;
        let map_row = |row: &rusqlite::Row<'_>| {
            let bucket_start = row.get::<_, i64>(0)?.max(0) as u64;
            Ok(ContextHistogramBucket {
                bucket_start,
                bucket_end: bucket_start.saturating_add(bucket_size as u64),
                success_count: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                failure_count: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                overrun_count: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
            })
        };
        let rows = if let Some(model_id) = model_id {
            stmt.query_map(
                params![bucket_size, bucket_size, since_ts, model_id],
                map_row,
            )?
        } else {
            stmt.query_map(params![bucket_size, bucket_size, since_ts], map_row)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn record_route(&self, route: &RequestRoute) -> Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            r#"
      INSERT INTO request_route (
        request_id, route_mode, sampled, complexity_tier, complexity_score,
        primary_model_id, backup_model_ids, fusion_model_id, created_at
      )
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
      ON CONFLICT(request_id) DO UPDATE SET
        route_mode = excluded.route_mode,
        sampled = excluded.sampled,
        complexity_tier = excluded.complexity_tier,
        complexity_score = excluded.complexity_score,
        primary_model_id = excluded.primary_model_id,
        backup_model_ids = excluded.backup_model_ids,
        fusion_model_id = excluded.fusion_model_id
      "#,
            params![
                &route.request_id,
                &route.route_mode,
                bool_to_i64(route.sampled),
                &route.complexity_tier,
                route.complexity_score as i64,
                &route.primary_model_id,
                serde_json::to_string(&route.backup_model_ids)?,
                &route.fusion_model_id,
                route.created_at
            ],
        )?;
        Ok(())
    }

    pub fn usage_since(&self, since_ts: i64) -> Result<Vec<ModelUsageWindow>> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT model_id, provider,
             SUM(attempts), SUM(successes), SUM(failures), SUM(wins),
             SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens),
             SUM(latency_count), SUM(latency_total_ms)
      FROM model_usage_minute
      WHERE minute_ts >= ?1
      GROUP BY model_id, provider
      ORDER BY model_id
      "#,
        )?;
        let rows = stmt.query_map([minute_floor(since_ts)], |row| {
            Ok(ModelUsageWindow {
                model_id: row.get(0)?,
                provider: row.get(1)?,
                attempts: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                successes: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
                failures: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                wins: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                prompt_tokens: row.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                completion_tokens: row.get::<_, Option<i64>>(7)?.unwrap_or(0) as u64,
                total_tokens: row.get::<_, Option<i64>>(8)?.unwrap_or(0) as u64,
                latency_count: row.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
                latency_total_ms: row.get::<_, Option<i64>>(10)?.unwrap_or(0) as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn token_rate_estimate(
        &self,
        now_ts: i64,
        window_minutes: u64,
        smoothing_minutes: u64,
    ) -> Result<TokenRateEstimate> {
        let window_minutes = window_minutes.clamp(1, 10_080);
        let smoothing_minutes = smoothing_minutes.clamp(1, window_minutes);
        let current_minute = minute_floor(now_ts);
        let since_minute = current_minute - ((window_minutes as i64 - 1) * 60);
        let mut minute_tokens = vec![0u64; window_minutes as usize];

        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
      SELECT minute_ts, SUM(total_tokens)
      FROM model_usage_minute
      WHERE minute_ts >= ?1 AND minute_ts <= ?2
      GROUP BY minute_ts
      ORDER BY minute_ts
      "#,
        )?;
        let rows = stmt.query_map(params![since_minute, current_minute], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
            ))
        })?;
        for row in rows {
            let (minute_ts, tokens) = row?;
            let offset = (minute_ts - since_minute) / 60;
            if offset >= 0
                && let Some(slot) = minute_tokens.get_mut(offset as usize)
            {
                *slot = tokens;
            }
        }

        let smoothing = smoothing_minutes as usize;
        let mut rolling_tokens = 0u64;
        let mut smoothed_rates = Vec::new();
        for (index, tokens) in minute_tokens.iter().enumerate() {
            rolling_tokens = rolling_tokens.saturating_add(*tokens);
            if index >= smoothing {
                rolling_tokens = rolling_tokens.saturating_sub(minute_tokens[index - smoothing]);
            }
            if rolling_tokens > 0 {
                smoothed_rates.push(rolling_tokens as f64 / smoothing_minutes as f64);
            }
        }

        let sample_minutes = smoothed_rates.len() as u64;
        let max_tokens_per_minute = smoothed_rates.iter().copied().fold(0.0, f64::max);
        let median_tokens_per_minute = median_f64(&mut smoothed_rates);
        Ok(TokenRateEstimate {
            median_m_tokens_per_24h: project_tokens_per_minute_to_day_millions(
                median_tokens_per_minute,
            ),
            max_m_tokens_per_24h: project_tokens_per_minute_to_day_millions(max_tokens_per_minute),
            sample_minutes,
            window_minutes,
            smoothing_minutes,
        })
    }

    pub fn prune_minute_buckets(&self, retention_days: u64) -> Result<()> {
        let cutoff = now_unix() - i64::try_from(retention_days.saturating_mul(86_400)).unwrap_or(0);
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
            "DELETE FROM model_usage_minute WHERE minute_ts < ?1",
            [minute_floor(cutoff)],
        )?;
        Ok(())
    }

    pub fn learned_boost(&self, model_id: &str) -> Result<f64> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let boost = conn
            .query_row(
                "SELECT attempts, wins FROM fusion_score WHERE model_id = ?1",
                [model_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .map(|(attempts, wins)| 1.0 + (wins as f64 / (attempts.max(1) as f64)) * 0.5)
            .unwrap_or(1.0);
        Ok(boost)
    }

    pub fn provider_quota(
        &self,
        model_id: &str,
        window_seconds: i64,
    ) -> Result<(i64, i64, Option<i64>)> {
        let now = now_unix();
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let row = conn
      .query_row(
        "SELECT requests_today, window_started_at, disabled_until FROM provider_quota WHERE model_id = ?1",
        [model_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<i64>>(2)?)),
      )
      .optional()?;
        if let Some((requests, started, disabled_until)) = row {
            if now - started >= window_seconds {
                conn.execute(
                    r#"
          INSERT INTO provider_quota (model_id, requests_today, window_started_at, disabled_until)
          VALUES (?1, 0, ?2, NULL)
          ON CONFLICT(model_id) DO UPDATE SET
            requests_today = 0,
            window_started_at = excluded.window_started_at,
            disabled_until = NULL
          "#,
                    params![model_id, now],
                )?;
                return Ok((0, now, None));
            }
            return Ok((requests, started, disabled_until));
        }
        conn.execute(
            r#"
      INSERT INTO provider_quota (model_id, requests_today, window_started_at, disabled_until)
      VALUES (?1, 0, ?2, NULL)
      "#,
            params![model_id, now],
        )?;
        Ok((0, now, None))
    }

    pub fn increment_quota(&self, model_id: &str) -> Result<()> {
        let now = now_unix();
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.execute(
      r#"
      INSERT INTO provider_quota (model_id, requests_today, window_started_at, disabled_until)
      VALUES (?1, 1, ?2, NULL)
      ON CONFLICT(model_id) DO UPDATE SET
        requests_today = requests_today + 1,
        window_started_at = CASE WHEN window_started_at = 0 THEN excluded.window_started_at ELSE window_started_at END
      "#,
      params![model_id, now],
    )?;
        Ok(())
    }
}
