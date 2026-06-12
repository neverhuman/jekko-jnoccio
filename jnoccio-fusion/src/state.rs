use crate::openai::ChatUsage;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;

const AGENT_ACTIVITY_TTL_SECONDS: i64 = 90;

#[derive(Clone, Debug, Serialize)]
pub struct ModelState {
    pub model_id: String,
    pub provider: String,
    pub status: String,
    pub failure_count: u64,
    pub success_count: u64,
    pub win_count: u64,
    pub last_latency_ms: Option<u64>,
    pub disabled_until: Option<i64>,
    pub last_error_kind: Option<String>,
    pub last_error_message: Option<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestTrace {
    pub request_id: String,
    pub phase: String,
    pub model_id: String,
    pub provider: String,
    pub status: String,
    pub error_kind: Option<String>,
    pub latency_ms: Option<u64>,
    pub cooldown_until: Option<i64>,
    pub winner_model_id: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UsageTotals {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TokenRateEstimate {
    pub median_m_tokens_per_24h: f64,
    pub max_m_tokens_per_24h: f64,
    pub sample_minutes: u64,
    pub window_minutes: u64,
    pub smoothing_minutes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AgentSource {
    pub id: String,
    pub client: Option<String>,
    pub session_id: Option<String>,
    pub agent_role: Option<String>,
    pub zyal_run_id: Option<String>,
    pub zyal_lane_id: Option<String>,
    /// User id from `x-jekko-credential-user-id` header — which user's key
    /// slot was selected for this request when `UsersPool` routing is active.
    pub credential_user_id: Option<String>,
    /// Reported policy from `x-jekko-credential-policy` header (e.g.
    /// `"users-only"`). Mirrors `JEKKO_KEY_SOURCE_POLICY` from the runner.
    pub credential_policy: Option<String>,
    pub process_role: Option<String>,
    pub pid: Option<i64>,
    pub user_agent: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentActivity {
    pub agent_id: String,
    pub agent_client: Option<String>,
    pub agent_session_id: Option<String>,
    pub process_role: Option<String>,
    pub pid: Option<i64>,
    pub version: Option<String>,
    pub user_agent: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub request_count: u64,
}

impl UsageTotals {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn from_usage(usage: Option<&ChatUsage>) -> Self {
        Self {
            prompt_tokens: usage.and_then(|item| item.prompt_tokens).unwrap_or(0),
            completion_tokens: usage.and_then(|item| item.completion_tokens).unwrap_or(0),
            total_tokens: usage.and_then(|item| item.total_tokens).unwrap_or(0),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelMetric {
    pub model_id: String,
    pub provider: String,
    pub call_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub win_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub latency_count: u64,
    pub latency_total_ms: u64,
    pub latency_min_ms: Option<u64>,
    pub latency_max_ms: Option<u64>,
    pub last_latency_ms: Option<u64>,
    pub last_error_kind: Option<String>,
    pub last_error_message: Option<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MetricEvent {
    pub id: i64,
    pub request_id: String,
    pub phase: String,
    pub model_id: String,
    pub provider: String,
    pub status: String,
    pub error_kind: Option<String>,
    pub latency_ms: Option<u64>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub route_mode: Option<String>,
    pub backup_rank: Option<u64>,
    pub complexity_tier: Option<String>,
    pub sampled: Option<bool>,
    pub winner_model_id: Option<String>,
    pub capacity_known: Option<bool>,
    pub agent_id: Option<String>,
    pub agent_client: Option<String>,
    pub agent_session_id: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelLimitEstimate {
    pub model_id: String,
    pub provider: String,
    pub configured_context_window: u64,
    pub learned_context_window: Option<u64>,
    pub learned_request_token_limit: Option<u64>,
    pub learned_tpm_limit: Option<u64>,
    pub safe_context_window: u64,
    pub largest_success_prompt_tokens: u64,
    pub largest_success_total_tokens: u64,
    pub smallest_overrun_requested_tokens: Option<u64>,
    pub context_overrun_count: u64,
    pub rate_limit_count: u64,
    pub last_limit_error_kind: Option<String>,
    pub last_limit_error_message: Option<String>,
    pub last_limit_error_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelContextEvent {
    pub request_id: String,
    pub phase: String,
    pub model_id: String,
    pub provider: String,
    pub status: String,
    pub approx_prompt_tokens: u64,
    pub requested_output_tokens: u64,
    pub estimated_total_tokens: u64,
    pub observed_prompt_tokens: Option<u64>,
    pub observed_total_tokens: Option<u64>,
    pub learned_limit: Option<u64>,
    pub overrun_requested_tokens: Option<u64>,
    pub error_kind: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextHistogramBucket {
    pub bucket_start: u64,
    pub bucket_end: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub overrun_count: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RouteEventMeta {
    pub route_mode: Option<String>,
    pub backup_rank: Option<u64>,
    pub complexity_tier: Option<String>,
    pub sampled: Option<bool>,
    pub capacity_known: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct RecordSuccessInput<'a> {
    pub request_id: &'a str,
    pub phase: &'a str,
    pub model_id: &'a str,
    pub provider: &'a str,
    pub latency_ms: u64,
    pub winner_model_id: Option<&'a str>,
    pub usage: Option<&'a ChatUsage>,
    pub meta: &'a RouteEventMeta,
    pub agent: Option<&'a AgentSource>,
}

#[derive(Clone, Debug)]
pub struct RequestRoute {
    pub request_id: String,
    pub route_mode: String,
    pub sampled: bool,
    pub complexity_tier: String,
    pub complexity_score: u64,
    pub primary_model_id: Option<String>,
    pub backup_model_ids: Vec<String>,
    pub fusion_model_id: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default)]
pub struct ModelUsageWindow {
    pub model_id: String,
    pub provider: String,
    pub attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub wins: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub latency_count: u64,
    pub latency_total_ms: u64,
}

pub struct StateDb {
    conn: Mutex<Connection>,
    event_retention_rows: usize,
}

mod state_core;
mod state_limits;
mod state_outcomes;
mod state_recording;
#[cfg(test)]
mod state_tests;
mod state_util;
