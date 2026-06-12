use crate::capacity::{CapacityModel, CapacityUsage, capacity_summary};
use crate::config::{AppConfig, ModelEntry, UpstreamKeySource};
use crate::failure_log::{build_failure_log_entry, write_failure_log};
use crate::limits::{ErrorKind, cooldown_for, parse_limit_signal};
use crate::mcp::McpState;
use crate::metrics::{
    ContextDashboard, DashboardModel, DashboardSnapshot, dashboard_totals, metric_average, ratio,
};
use crate::openai::{
    ChatCompletionRequest, ChatCompletionResponse, EmbeddingObject, EmbeddingsRequest,
    EmbeddingsResponse, EmbeddingsUsage, clamp_output_tokens,
};
use crate::providers::ProviderClient;
use crate::providers::client as provider_client;
use crate::providers::openai_compatible::{UpstreamCompletion, build_body};
use crate::routing::{
    RequestProfile, RouteMode, RoutePlan, RoutingConfig, RoutingModelInput, RoutingUsage,
    plan_route,
};
use crate::state::{
    AgentSource, MetricEvent, ModelLimitEstimate, ModelMetric, ModelState, RecordSuccessInput,
    RequestRoute, RouteEventMeta, StateDb,
};
use anyhow::{Context, Result};
use rand::Rng;
use serde::Deserialize as _;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::{info, warn};

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("no upstream keys available")]
    NoAvailableModels,
    #[error(
        "no context-safe model available: estimated {required_total_tokens} tokens, largest safe window {largest_safe_context_window} tokens"
    )]
    NoContextSafeModel {
        required_total_tokens: u64,
        largest_safe_context_window: u64,
    },
    #[error(
        "no API keys configured: {keyed_count}/{total_count} models have valid keys. Add provider API keys to {env_path} and restart jnoccio-fusion"
    )]
    NoKeysConfigured {
        keyed_count: usize,
        total_count: usize,
        env_path: String,
    },
    #[error(
        "all {total_count} models are unavailable: {summary}. Check provider status or API key validity in {env_path}"
    )]
    AllModelsUnavailable {
        total_count: usize,
        summary: String,
        env_path: String,
    },
    #[error(
        "all eligible models are in cooldown ({cooldown_count}/{total_count}). Retry after rate limits expire"
    )]
    AllModelsInCooldown {
        cooldown_count: usize,
        total_count: usize,
    },
    #[error("upstream request failed: {0}")]
    Upstream(String),
    #[error("invalid upstream response: {0}")]
    InvalidResponse(String),
    #[error("per-user budget exceeded for user `{user_id}`: {reason}")]
    BudgetExceeded { user_id: String, reason: String },
}

impl GatewayError {
    fn classification(&self) -> (axum::http::StatusCode, &'static str) {
        match self {
            GatewayError::ModelNotFound(_) => {
                (axum::http::StatusCode::NOT_FOUND, "model_not_found")
            }
            GatewayError::NoAvailableModels => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "no_available_models",
            ),
            GatewayError::NoContextSafeModel { .. } => (
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                "no_context_safe_model",
            ),
            GatewayError::NoKeysConfigured { .. } => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "no_keys_configured",
            ),
            GatewayError::AllModelsUnavailable { .. } => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "all_models_unavailable",
            ),
            GatewayError::AllModelsInCooldown { .. } => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "all_models_in_cooldown",
            ),
            GatewayError::Upstream(_) => (axum::http::StatusCode::BAD_GATEWAY, "upstream_error"),
            GatewayError::InvalidResponse(_) => {
                (axum::http::StatusCode::BAD_GATEWAY, "invalid_response")
            }
            GatewayError::BudgetExceeded { .. } => {
                (axum::http::StatusCode::TOO_MANY_REQUESTS, "budget_exceeded")
            }
        }
    }

    pub fn status_code(&self) -> axum::http::StatusCode {
        self.classification().0
    }

    pub fn kind(&self) -> &'static str {
        self.classification().1
    }
}

fn upstream_err(e: impl std::fmt::Display) -> GatewayError {
    GatewayError::Upstream(e.to_string())
}

#[derive(Clone)]
pub struct Gateway {
    pub config: AppConfig,
    pub state: Arc<StateDb>,
    pub mcp: Arc<McpState>,
    events: broadcast::Sender<DashboardMessage>,
    http: reqwest::Client,
    /// Per-user budget gate. Defaults to [`zyal_key_pool::AlwaysAllow`] so
    /// unconfigured deployments pass through. Phase C2 wires the call into
    /// [`Gateway::complete`]; a real `EnforceDailyCaps` impl is a follow-up.
    /// Stored as `Arc` rather than `Box` so the derived `Clone` continues to
    /// work for tests that hand-construct `Gateway`.
    policy_hook: Arc<dyn zyal_key_pool::PolicyHook>,
}

#[derive(Clone, Debug)]
struct RuntimeModel {
    entry: ModelEntry,
    visible_id: String,
    route_slot_id: String,
    upstream_model_id: String,
    credential_user_id: Option<String>,
    credential_env_name: String,
    key_source: UpstreamKeySource,
    api_key: Option<String>,
    key_present: bool,
    base_url: String,
    base_url_missing_keys: Vec<String>,
    state: Option<ModelState>,
}

#[derive(Clone, Debug)]
pub struct GatewayResult {
    pub response: ChatCompletionResponse,
    pub receipts: Vec<String>,
    pub winner_model_id: String,
    pub confidence: f64,
}

struct GatewayResultInput<'a> {
    request_id: &'a str,
    response: ChatCompletionResponse,
    receipts: Vec<String>,
    winner_model_id: &'a str,
    confidence: f64,
    prompt_hash: &'a str,
    context_hash: &'a str,
    selected_model: &'a RuntimeModel,
    selected_latency_ms: u64,
    draft_results: Option<&'a [DraftResult]>,
    route: &'a RoutePlan,
    agent: Option<&'a AgentSource>,
    structured_output: Option<StructuredOutputReceipt>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct StructuredOutputReceipt {
    raw_hash: String,
    normalized_hash: Option<String>,
    repair_attempts: u64,
    schema_status: String,
    error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct ModelDecisionRecord {
    model_id: String,
    route_slot_id: String,
    upstream_model_id: String,
    credential_user_id: Option<String>,
    credential_env_name: String,
    configured_score: f64,
    selection_score: f64,
    latency_ms: u64,
    status: String,
    output_hash: Option<String>,
    selected: bool,
    token_usage: TokenUsageRecord,
}

#[derive(Clone, Debug, serde::Serialize)]
struct TokenUsageRecord {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct HealthInfo {
    pub ok: bool,
    pub visible_model: String,
    pub provider: String,
    pub upstream_key_source: String,
    pub user_count: usize,
    pub eligible_slot_count: usize,
    pub per_user_slot_counts: BTreeMap<String, usize>,
    pub per_provider_slot_counts: BTreeMap<String, usize>,
    pub available_models: usize,
    pub keyed_models: usize,
    pub missing_keys: Vec<String>,
    pub incomplete_env: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelStatusView {
    pub id: String,
    pub provider: String,
    pub route_slot_id: String,
    pub upstream_model_id: String,
    pub credential_user_id: Option<String>,
    pub credential_env_name: String,
    pub key_source: String,
    pub display_name: String,
    pub upstream_model: String,
    pub visible_id: String,
    pub api_style: String,
    pub base_url: String,
    pub signup_url: String,
    pub key_present: bool,
    pub enabled: bool,
    pub status: String,
    pub disabled_reason: Option<String>,
    pub cooldown_until: Option<i64>,
    pub roles: Vec<String>,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub limits: serde_json::Value,
    pub score: serde_json::Value,
    pub state: Option<ModelState>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DashboardMessage {
    Snapshot { snapshot: DashboardSnapshot },
    ModelUpdated { model: DashboardModel },
    RequestEvent { event: MetricEvent },
    Heartbeat { timestamp: i64 },
}

impl Gateway {
    pub fn new(config: AppConfig) -> Result<Self> {
        let state = Arc::new(StateDb::open_with_retention(
            &config.database,
            config.routing.event_retention_rows,
        )?);
        let (events, _) = broadcast::channel(512);
        let mcp = Arc::new(McpState::new(config.instance_role, config.scaling.clone()));
        let gateway = Self {
            config,
            state,
            mcp,
            events,
            http: reqwest::Client::builder()
                .timeout(upstream_attempt_timeout())
                .build()
                .context("build http client")?,
            policy_hook: Arc::new(zyal_key_pool::AlwaysAllow),
        };
        gateway.seed_model_state()?;
        gateway
            .state
            .prune_minute_buckets(gateway.config.routing.minute_bucket_retention_days)?;
        Ok(gateway)
    }

    fn seed_model_state(&self) -> Result<()> {
        for model in self.runtime_models()? {
            let status = model.readiness_status();
            self.state
                .upsert_model(&model.visible_id, &model.entry.provider, status)?;
            self.state
                .upsert_metric_model(&model.visible_id, &model.entry.provider)?;
            self.state.upsert_limit_model(
                &model.visible_id,
                &model.entry.provider,
                model.entry.context_window,
            )?;
        }
        Ok(())
    }

    fn runtime_models(&self) -> Result<Vec<RuntimeModel>> {
        let states = self
            .state
            .snapshot()?
            .into_iter()
            .map(|state| (state.model_id.clone(), state))
            .collect::<HashMap<_, _>>();
        Ok(crate::config::resolve_models(&self.config)?
            .into_iter()
            .map(|model| RuntimeModel {
                visible_id: model.visible_id.clone(),
                route_slot_id: model.route_slot_id,
                upstream_model_id: model.upstream_model_id,
                credential_user_id: model.credential_user_id,
                credential_env_name: model.credential_env_name,
                key_source: model.key_source,
                api_key: model.api_key,
                key_present: model.key_present,
                base_url: model.base_url,
                base_url_missing_keys: model.base_url_missing_keys,
                entry: model.entry,
                state: states.get(&model.visible_id).cloned(),
            })
            .collect())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DashboardMessage> {
        self.events.subscribe()
    }

    pub fn heartbeat_message() -> DashboardMessage {
        DashboardMessage::Heartbeat {
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    pub fn dashboard_snapshot(&self) -> Result<DashboardSnapshot> {
        let now = chrono::Utc::now().timestamp();
        let metric_rows = self
            .state
            .metric_snapshot()?
            .into_iter()
            .map(|metric| (metric.model_id.clone(), metric))
            .collect::<HashMap<_, _>>();
        let usage_rows = self.usage_last_hour()?;
        let limit_rows = self
            .state
            .limit_estimates()?
            .into_iter()
            .map(|estimate| (estimate.model_id.clone(), estimate))
            .collect::<HashMap<_, _>>();
        let models = self
            .runtime_models()?
            .iter()
            .map(|model| {
                self.dashboard_model(
                    model,
                    metric_rows.get(&model.visible_id),
                    limit_rows.get(&model.visible_id),
                    usage_rows
                        .get(&model.visible_id)
                        .map(|usage| usage.attempts)
                        .unwrap_or(0),
                )
            })
            .collect::<Vec<_>>();
        let capacity = capacity_summary(
            &self
                .runtime_models()?
                .iter()
                .map(|model| self.capacity_model(model))
                .collect::<Vec<_>>(),
            &usage_rows,
        );
        let totals = dashboard_totals(&models);
        let token_rate = self.state.token_rate_estimate(now, 24 * 60, 10)?;
        let active_agents = self.state.active_agents_live(now)?;
        Ok(DashboardSnapshot {
            totals,
            token_rate,
            capacity,
            context: ContextDashboard {
                estimates: limit_rows.into_values().collect(),
                histogram: self
                    .state
                    .context_histogram(None, 8_000, now - 86_400 * 30)?,
                recent_events: self.state.context_events(200)?,
            },
            models,
            recent_events: self.state.recent_metric_events(100)?,
            agent_count: active_agents.len(),
            max_agents: self.mcp.max_instances(),
            active_agents,
            instance_count: self.mcp.instance_count(),
            max_instances: self.mcp.max_instances(),
            available_instance_slots: self.mcp.available_instance_slots(),
            instance_role: self.config.instance_role.as_str().to_string(),
            worker_threads: self.config.worker_threads,
        })
    }

    fn emit_model_metric(&self, model_id: &str) {
        let metric = self.state.metric_for(model_id).ok().flatten();
        let Ok(limit_estimates) = self.state.limit_estimates() else {
            return;
        };
        let limit_rows = limit_estimates
            .into_iter()
            .map(|estimate| (estimate.model_id.clone(), estimate))
            .collect::<HashMap<_, _>>();
        let Ok(models) = self.runtime_models() else {
            return;
        };
        if let Some(model) = models
            .iter()
            .find(|model| model.visible_id == model_id)
            .map(|model| {
                self.dashboard_model(
                    model,
                    metric.as_ref(),
                    limit_rows.get(&model.visible_id),
                    self.usage_last_hour()
                        .ok()
                        .and_then(|items| items.get(&model.visible_id).map(|usage| usage.attempts))
                        .unwrap_or(0),
                )
            })
        {
            let _ = self.events.send(DashboardMessage::ModelUpdated { model });
        }
    }

    fn emit_metric_event(&self, event: MetricEvent) {
        let _ = self.events.send(DashboardMessage::RequestEvent { event });
    }

    fn record_winner(
        &self,
        request_id: &str,
        model_id: &str,
        meta: &RouteEventMeta,
        agent: Option<&AgentSource>,
    ) {
        if let Ok(event) = self
            .state
            .record_winner_for_request(request_id, model_id, meta, agent)
        {
            self.emit_metric_event(event);
            self.emit_model_metric(model_id);
            info!(
                request_id = %request_id,
                model_id = %model_id,
                route_mode = meta.route_mode.as_deref().unwrap_or(""),
                backup_rank = meta.backup_rank.unwrap_or(0),
                sampled = meta.sampled.unwrap_or(false),
                "winner recorded"
            );
        }
    }

    pub fn health(&self) -> HealthInfo {
        let Ok(models) = self.runtime_models() else {
            return HealthInfo {
                ok: false,
                visible_model: self.config.visible_model_id.clone(),
                provider: self.config.provider_id.clone(),
                upstream_key_source: self.config.upstream_key_source.as_str().to_string(),
                user_count: 0,
                eligible_slot_count: 0,
                per_user_slot_counts: BTreeMap::new(),
                per_provider_slot_counts: BTreeMap::new(),
                available_models: 0,
                keyed_models: 0,
                missing_keys: Vec::new(),
                incomplete_env: Vec::new(),
            };
        };
        let keyed_models = models.iter().filter(|model| model.is_ready()).count();
        let eligible_slot_count = models
            .iter()
            .filter(|model| model.is_routable_now(chrono::Utc::now().timestamp()))
            .count();
        let per_user_slot_counts = count_ready_slots_by_user(&models);
        let per_provider_slot_counts = count_ready_slots_by_provider(&models);
        let missing_keys = models
            .iter()
            .filter(|model| !model.has_key())
            .map(|model| model.visible_id.clone())
            .collect::<Vec<_>>();
        let incomplete_env = models
            .iter()
            .filter(|model| model.has_key() && !model.base_url_missing_keys.is_empty())
            .map(|model| model.visible_id.clone())
            .collect::<Vec<_>>();
        HealthInfo {
            ok: keyed_models > 0,
            visible_model: self.config.visible_model_id.clone(),
            provider: self.config.provider_id.clone(),
            upstream_key_source: self.config.upstream_key_source.as_str().to_string(),
            user_count: per_user_slot_counts.len(),
            eligible_slot_count,
            per_user_slot_counts,
            per_provider_slot_counts,
            available_models: models.len(),
            keyed_models,
            missing_keys,
            incomplete_env,
        }
    }

    pub fn status(&self) -> serde_json::Value {
        let models = self.runtime_models().unwrap_or_default();
        let states = self.state.snapshot().unwrap_or_default();
        let health = self.health();
        json!({
          "ok": health.ok,
          "health": health,
          "visible_model": self.config.visible_model_id,
          "provider": self.config.provider_id,
          "upstream_key_source": self.config.upstream_key_source.as_str(),
          "bind": self.config.bind,
          "database": self.config.database,
          "receipts_dir": self.config.receipts_dir,
          "instance_count": self.mcp.instance_count(),
          "max_instances": self.mcp.max_instances(),
          "available_instance_slots": self.mcp.available_instance_slots(),
          "instance_role": self.config.instance_role.as_str(),
          "worker_threads": self.config.worker_threads,
          "models": models.iter().map(|model| self.model_status_view(model)).collect::<Vec<_>>(),
          "state_rows": states,
        })
    }

    pub fn model_list(&self) -> serde_json::Value {
        json!({
          "object": "list",
          "data": [{
            "id": self.config.visible_model_id,
            "object": "model",
            "created": 0,
            "owned_by": "jnoccio",
            "permission": [],
            "root": self.config.visible_model_id,
            "parent": null
          }]
        })
    }

    pub async fn complete(
        &self,
        request: ChatCompletionRequest,
        agent: Option<&AgentSource>,
    ) -> Result<GatewayResult, GatewayError> {
        if !model_matches_visible(&self.config.visible_model_id, &request.model) {
            return Err(GatewayError::ModelNotFound(request.model));
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let (prompt_hash, context_hash) = request_hashes(&request);
        let models = self.runtime_models().map_err(upstream_err)?;
        let now = chrono::Utc::now().timestamp();
        let profile = RequestProfile::from_request(&request);
        let routing_inputs = self.routing_inputs(&models, now);
        let routing_usage = self.routing_usage().map_err(upstream_err)?;
        let route = plan_route(
            &routing_inputs,
            &routing_usage,
            &profile,
            &RoutingConfig {
                fusion_sample_rate: self.config.routing.fusion_sample_rate,
                fast_backup_count: self.config.routing.fast_backup_count,
                proof_profile: self.config.routing.proof_profile
                    || self.config.upstream_key_source.users_only(),
            },
            now,
            rand::rng().random_range(0.0..1.0),
        );
        if route.primary_model_id.is_none()
            && route.draft_model_ids.is_empty()
            && route.fusion_model_id.is_none()
        {
            warn!(
                request_id = %request_id,
                model = %request.model,
                approx_prompt_tokens = profile.approx_prompt_tokens,
                requested_output_tokens = profile.requested_output_tokens.unwrap_or(0),
                available_models = models.len(),
                "no eligible route planned"
            );
            return Err(self.diagnose_no_eligible_models(&routing_inputs, &models, &profile, now));
        }
        info!(
            request_id = %request_id,
            model = %request.model,
            stream = request.stream.unwrap_or(false),
            approx_prompt_tokens = profile.approx_prompt_tokens,
            requested_output_tokens = profile.requested_output_tokens.unwrap_or(0),
            route_mode = route.mode.as_str(),
            sampled = route.sampled,
            complexity_tier = route.complexity_tier.as_str(),
            primary_model_id = route.primary_model_id.as_deref().unwrap_or("none"),
            backup_model_ids = %route.backup_model_ids.join(","),
            fusion_model_id = route.fusion_model_id.as_deref().unwrap_or("none"),
            "route planned"
        );
        self.state
            .record_route(&RequestRoute {
                request_id: request_id.clone(),
                route_mode: route.mode.as_str().to_string(),
                sampled: route.sampled,
                complexity_tier: route.complexity_tier.as_str().to_string(),
                complexity_score: route.complexity_score,
                primary_model_id: route.primary_model_id.clone(),
                backup_model_ids: route.backup_model_ids.clone(),
                fusion_model_id: route.fusion_model_id.clone(),
                created_at: now,
            })
            .map_err(upstream_err)?;

        // Phase C2 budget gate: check the policy hook against the selected
        // slot before any upstream dispatch. The default `AlwaysAllow` impl
        // is a no-op so existing flows keep working; a future
        // `EnforceDailyCaps` impl will read per-user caps from
        // `~/.jekko/users/<id>/state.sqlite` and refuse when over.
        // `estimated_tokens = 0` for now — Phase E adds real token accounting.
        if let Some(selected) = budget_gate_target(&route, &models) {
            let user_id = selected
                .credential_user_id
                .as_deref()
                .unwrap_or("default")
                .to_string();
            let provider_id = selected.entry.provider.clone();
            let decision = self
                .policy_hook
                .check_and_reserve(&user_id, &provider_id, 0)
                .map_err(upstream_err)?;
            if let zyal_key_pool::budget::BudgetDecision::Refuse { reason } = decision {
                return Err(GatewayError::BudgetExceeded {
                    user_id,
                    reason: format!("{reason:?}"),
                });
            }
        }

        if let Some(agent) = agent {
            let _ = self.state.record_agent_activity(agent);
        }

        if route.sampled {
            return self
                .complete_fusion_sample(&request_id, &request, &models, &route, agent)
                .await;
        }

        let Some(primary) = route
            .primary_model_id
            .as_deref()
            .and_then(|id| model_by_id(&models, id))
        else {
            return Err(GatewayError::NoAvailableModels);
        };
        let mut receipts = self.build_route_receipts(&request_id, &route);
        match self
            .run_phase(
                &request_id,
                "fast",
                &primary,
                &request,
                true,
                None,
                &self.route_meta(&route, &primary, None),
                agent,
            )
            .await
        {
            Ok(outcome) => {
                let winner_model_id = primary.visible_id.clone();
                let (response, structured_output) = if structured_json_schema(&request).is_some() {
                    let (response, receipt) = self
                        .enforce_structured_output(
                            &request_id,
                            &request,
                            outcome.response,
                            &route,
                            &models,
                            &primary,
                            agent,
                        )
                        .await?;
                    (response, Some(receipt))
                } else {
                    (outcome.response, None)
                };
                info!(
                    request_id = %request_id,
                    winner_model_id = %winner_model_id,
                    phase = "fast",
                    "request completed on primary model"
                );
                return Ok(self.build_gateway_result(GatewayResultInput {
                    request_id: &request_id,
                    response,
                    receipts,
                    winner_model_id: &winner_model_id,
                    confidence: 0.65,
                    prompt_hash: prompt_hash.as_str(),
                    context_hash: context_hash.as_str(),
                    selected_model: &primary,
                    selected_latency_ms: outcome.latency_ms,
                    draft_results: None,
                    route: &route,
                    agent,
                    structured_output,
                }));
            }
            Err(err) => receipts.push(format!("{} -> error: {err}", primary.visible_id)),
        }

        for (index, backup_id) in route.backup_model_ids.iter().enumerate() {
            let Some(backup) = model_by_id(&models, backup_id) else {
                continue;
            };
            match self
                .run_phase(
                    &request_id,
                    "backup",
                    &backup,
                    &request,
                    true,
                    None,
                    &self.route_meta(&route, &backup, Some(index as u64 + 1)),
                    agent,
                )
                .await
            {
                Ok(outcome) => {
                    let winner_model_id = backup.visible_id.clone();
                    let (response, structured_output) =
                        if structured_json_schema(&request).is_some() {
                            let (response, receipt) = self
                                .enforce_structured_output(
                                    &request_id,
                                    &request,
                                    outcome.response,
                                    &route,
                                    &models,
                                    &backup,
                                    agent,
                                )
                                .await?;
                            (response, Some(receipt))
                        } else {
                            (outcome.response, None)
                        };
                    receipts.push(format!("backup_rank={} succeeded", index + 1));
                    info!(
                        request_id = %request_id,
                        winner_model_id = %winner_model_id,
                        backup_rank = index + 1,
                        "request completed on backup model"
                    );
                    return Ok(self.build_gateway_result(GatewayResultInput {
                        request_id: &request_id,
                        response,
                        receipts,
                        winner_model_id: &winner_model_id,
                        confidence: 0.55,
                        prompt_hash: prompt_hash.as_str(),
                        context_hash: context_hash.as_str(),
                        selected_model: &backup,
                        selected_latency_ms: outcome.latency_ms,
                        draft_results: None,
                        route: &route,
                        agent,
                        structured_output,
                    }));
                }
                Err(err) => receipts.push(format!("{} -> error: {err}", backup.visible_id)),
            }
        }

        let mut excluded_ids = Vec::new();
        if let Some(primary_id) = route.primary_model_id.as_ref() {
            excluded_ids.push(primary_id.clone());
        }
        excluded_ids.extend(route.backup_model_ids.iter().cloned());
        for (index, fallback) in self
            .fallback_candidates(&models, &excluded_ids)
            .into_iter()
            .enumerate()
        {
            let backup_rank = route.backup_model_ids.len() as u64 + index as u64 + 1;
            match self
                .run_phase(
                    &request_id,
                    "backup",
                    &fallback,
                    &request,
                    true,
                    None,
                    &self.route_meta(&route, &fallback, Some(backup_rank)),
                    agent,
                )
                .await
            {
                Ok(outcome) => {
                    let winner_model_id = fallback.visible_id.clone();
                    let (response, structured_output) =
                        if structured_json_schema(&request).is_some() {
                            let (response, receipt) = self
                                .enforce_structured_output(
                                    &request_id,
                                    &request,
                                    outcome.response,
                                    &route,
                                    &models,
                                    &fallback,
                                    agent,
                                )
                                .await?;
                            (response, Some(receipt))
                        } else {
                            (outcome.response, None)
                        };
                    receipts.push(format!("fallback_rank={} succeeded", backup_rank));
                    info!(
                        request_id = %request_id,
                        winner_model_id = %winner_model_id,
                        backup_rank,
                        "request completed on fallback model"
                    );
                    return Ok(self.build_gateway_result(GatewayResultInput {
                        request_id: &request_id,
                        response,
                        receipts,
                        winner_model_id: &winner_model_id,
                        confidence: 0.5,
                        prompt_hash: prompt_hash.as_str(),
                        context_hash: context_hash.as_str(),
                        selected_model: &fallback,
                        selected_latency_ms: outcome.latency_ms,
                        draft_results: None,
                        route: &route,
                        agent,
                        structured_output,
                    }));
                }
                Err(err) => receipts.push(format!("{} -> error: {err}", fallback.visible_id)),
            }
        }

        Err(GatewayError::Upstream(receipts.join("; ")))
    }

    async fn complete_fusion_sample(
        &self,
        request_id: &str,
        request: &ChatCompletionRequest,
        models: &[RuntimeModel],
        route: &RoutePlan,
        agent: Option<&AgentSource>,
    ) -> Result<GatewayResult, GatewayError> {
        let (prompt_hash, context_hash) = request_hashes(request);
        let draft_candidates = route
            .draft_model_ids
            .iter()
            .filter_map(|id| model_by_id(models, id))
            .collect::<Vec<_>>();
        let fusion_candidates = route
            .fusion_model_id
            .as_deref()
            .and_then(|id| model_by_id(models, id))
            .into_iter()
            .collect::<Vec<_>>();

        if draft_candidates.is_empty() && fusion_candidates.is_empty() {
            return Err(GatewayError::NoAvailableModels);
        }

        let draft_results = self
            .run_drafts(request_id, request, &draft_candidates, route, agent)
            .await;
        let mut receipts = self.build_receipts(
            request_id,
            &draft_candidates,
            &draft_results,
            &fusion_candidates,
        );
        let successful_drafts = draft_results.iter().any(|item| item.output.is_some());

        if !successful_drafts {
            let alternative_path = if let Some(model) = fusion_candidates.first().cloned() {
                model
            } else if let Some(model) = draft_candidates.first().cloned() {
                model
            } else {
                return Err(GatewayError::NoAvailableModels);
            };
            let outcome = self
                .run_phase(
                    request_id,
                    "fusion_sample",
                    &alternative_path,
                    request,
                    true,
                    None,
                    &self.route_meta(route, &alternative_path, None),
                    agent,
                )
                .await?;
            let winner_model_id = alternative_path.visible_id.clone();
            let (response, structured_output) = if structured_json_schema(request).is_some() {
                let (response, receipt) = self
                    .enforce_structured_output(
                        request_id,
                        request,
                        outcome.response,
                        route,
                        models,
                        &alternative_path,
                        agent,
                    )
                    .await?;
                (response, Some(receipt))
            } else {
                (outcome.response, None)
            };
            let result = self.build_gateway_result(GatewayResultInput {
                request_id,
                response,
                receipts,
                winner_model_id: &winner_model_id,
                confidence: 0.45,
                prompt_hash: prompt_hash.as_str(),
                context_hash: context_hash.as_str(),
                selected_model: &alternative_path,
                selected_latency_ms: outcome.latency_ms,
                draft_results: None,
                route,
                agent,
                structured_output,
            });
            self.record_winner(
                request_id,
                &winner_model_id,
                &self.route_meta(route, &alternative_path, None),
                agent,
            );
            info!(
                request_id = %request_id,
                winner_model_id = %winner_model_id,
                "fusion sample fell back to alternative path"
            );
            return Ok(result);
        }

        let fusion_model = if let Some(model) = fusion_candidates.first().cloned() {
            model
        } else if let Some(draft) = self.pick_best_draft(&draft_results) {
            draft.model.clone()
        } else {
            return Err(GatewayError::NoAvailableModels);
        };
        let fusion_messages = self.build_fusion_messages(request, &draft_results, &receipts);
        let fusion_request = ChatCompletionRequest {
            messages: fusion_messages.clone(),
            tools: request.tools.clone(),
            tool_choice: request.tool_choice.clone(),
            stream: Some(false),
            ..request.clone()
        };
        let fusion_result = self
            .run_phase(
                request_id,
                "fusion",
                &fusion_model,
                &fusion_request,
                true,
                Some(fusion_messages.clone()),
                &self.route_meta(route, &fusion_model, None),
                agent,
            )
            .await;

        match fusion_result {
            Ok(outcome) => {
                let winner_model_id = fusion_model.visible_id.clone();
                let confidence = self.confidence(&draft_results, true);
                let (response, structured_output) = if structured_json_schema(request).is_some() {
                    let (response, receipt) = self
                        .enforce_structured_output(
                            request_id,
                            request,
                            outcome.response,
                            route,
                            models,
                            &fusion_model,
                            agent,
                        )
                        .await?;
                    (response, Some(receipt))
                } else {
                    (outcome.response, None)
                };
                let result = self.build_gateway_result(GatewayResultInput {
                    request_id,
                    response,
                    receipts,
                    winner_model_id: &winner_model_id,
                    confidence,
                    prompt_hash: prompt_hash.as_str(),
                    context_hash: context_hash.as_str(),
                    selected_model: &fusion_model,
                    selected_latency_ms: outcome.latency_ms,
                    draft_results: Some(&draft_results),
                    route,
                    agent,
                    structured_output,
                });
                self.record_winner(
                    request_id,
                    &winner_model_id,
                    &self.route_meta(route, &fusion_model, None),
                    agent,
                );
                info!(
                    request_id = %request_id,
                    winner_model_id = %winner_model_id,
                    confidence,
                    "fusion sample completed"
                );
                Ok(result)
            }
            Err(_) if request.tools.is_some() => {
                let mut excluded_ids = draft_candidates
                    .iter()
                    .map(|model| model.visible_id.clone())
                    .collect::<Vec<_>>();
                excluded_ids.extend(
                    fusion_candidates
                        .iter()
                        .map(|model| model.visible_id.clone()),
                );
                let candidates = self.fallback_candidates(models, &excluded_ids);
                if candidates.is_empty() {
                    return Err(GatewayError::NoAvailableModels);
                }
                let mut last_error = None;
                for (index, alternative_path) in candidates.into_iter().enumerate() {
                    let backup_rank = index as u64 + 1;
                    match self
                        .run_phase(
                            request_id,
                            "backup",
                            &alternative_path,
                            request,
                            true,
                            None,
                            &self.route_meta(route, &alternative_path, Some(backup_rank)),
                            agent,
                        )
                        .await
                    {
                        Ok(outcome) => {
                            let winner_model_id = alternative_path.visible_id.clone();
                            let (response, structured_output) =
                                if structured_json_schema(request).is_some() {
                                    let (response, receipt) = self
                                        .enforce_structured_output(
                                            request_id,
                                            request,
                                            outcome.response,
                                            route,
                                            models,
                                            &alternative_path,
                                            agent,
                                        )
                                        .await?;
                                    (response, Some(receipt))
                                } else {
                                    (outcome.response, None)
                                };
                            let result = self.build_gateway_result(GatewayResultInput {
                                request_id,
                                response,
                                receipts,
                                winner_model_id: &winner_model_id,
                                confidence: 0.5,
                                prompt_hash: prompt_hash.as_str(),
                                context_hash: context_hash.as_str(),
                                selected_model: &alternative_path,
                                selected_latency_ms: outcome.latency_ms,
                                draft_results: None,
                                route,
                                agent,
                                structured_output,
                            });
                            self.record_winner(
                                request_id,
                                &winner_model_id,
                                &self.route_meta(route, &alternative_path, None),
                                agent,
                            );
                            info!(
                                request_id = %request_id,
                                winner_model_id = %winner_model_id,
                                backup_rank,
                                "fusion sample fell back after fusion failure"
                            );
                            return Ok(result);
                        }
                        Err(err) => {
                            receipts
                                .push(format!("{} -> error: {err}", alternative_path.visible_id));
                            last_error = Some(err);
                        }
                    }
                }
                Err(last_error.unwrap_or(GatewayError::NoAvailableModels))
            }
            Err(err) => {
                if let Some(best) = self.pick_best_draft(&draft_results) {
                    let winner_model_id = best.model.visible_id.clone();
                    let confidence = self.confidence(&draft_results, false);
                    let best_response = best
                        .output
                        .clone()
                        .unwrap()
                        .into_response(&best.model.entry.model);
                    let (response, structured_output) = if structured_json_schema(request).is_some()
                    {
                        let (response, receipt) = self
                            .enforce_structured_output(
                                request_id,
                                request,
                                best_response,
                                route,
                                models,
                                &best.model,
                                agent,
                            )
                            .await?;
                        (response, Some(receipt))
                    } else {
                        (best_response, None)
                    };
                    let result = self.build_gateway_result(GatewayResultInput {
                        request_id,
                        response,
                        receipts,
                        winner_model_id: &winner_model_id,
                        confidence,
                        prompt_hash: prompt_hash.as_str(),
                        context_hash: context_hash.as_str(),
                        selected_model: &best.model,
                        selected_latency_ms: best.latency_ms,
                        draft_results: Some(&draft_results),
                        route,
                        agent,
                        structured_output,
                    });
                    self.record_winner(
                        request_id,
                        &winner_model_id,
                        &self.route_meta(route, &best.model, None),
                        agent,
                    );
                    info!(
                        request_id = %request_id,
                        winner_model_id = %winner_model_id,
                        "fusion sample selected best draft"
                    );
                    return Ok(result);
                }
                let mut excluded_ids = draft_candidates
                    .iter()
                    .map(|model| model.visible_id.clone())
                    .collect::<Vec<_>>();
                excluded_ids.extend(
                    fusion_candidates
                        .iter()
                        .map(|model| model.visible_id.clone()),
                );
                let candidates = self.fallback_candidates(models, &excluded_ids);
                for (index, alternative_path) in candidates.into_iter().enumerate() {
                    let backup_rank = index as u64 + 1;
                    match self
                        .run_phase(
                            request_id,
                            "backup",
                            &alternative_path,
                            request,
                            true,
                            None,
                            &self.route_meta(route, &alternative_path, Some(backup_rank)),
                            agent,
                        )
                        .await
                    {
                        Ok(outcome) => {
                            let winner_model_id = alternative_path.visible_id.clone();
                            let (response, structured_output) =
                                if structured_json_schema(request).is_some() {
                                    let (response, receipt) = self
                                        .enforce_structured_output(
                                            request_id,
                                            request,
                                            outcome.response,
                                            route,
                                            models,
                                            &alternative_path,
                                            agent,
                                        )
                                        .await?;
                                    (response, Some(receipt))
                                } else {
                                    (outcome.response, None)
                                };
                            let result = self.build_gateway_result(GatewayResultInput {
                                request_id,
                                response,
                                receipts,
                                winner_model_id: &winner_model_id,
                                confidence: 0.5,
                                prompt_hash: prompt_hash.as_str(),
                                context_hash: context_hash.as_str(),
                                selected_model: &alternative_path,
                                selected_latency_ms: outcome.latency_ms,
                                draft_results: None,
                                route,
                                agent,
                                structured_output,
                            });
                            self.record_winner(
                                request_id,
                                &winner_model_id,
                                &self.route_meta(route, &alternative_path, None),
                                agent,
                            );
                            info!(
                                request_id = %request_id,
                                winner_model_id = %winner_model_id,
                                backup_rank,
                                "fusion sample fell back after fusion error"
                            );
                            return Ok(result);
                        }
                        Err(inner_err) => receipts.push(format!(
                            "{} -> error: {inner_err}",
                            alternative_path.visible_id
                        )),
                    }
                }
                Err(err)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finalize_response(
        &self,
        request_id: &str,
        mut response: ChatCompletionResponse,
        receipts: Vec<String>,
        winner_model_id: Option<&str>,
        confidence: f64,
        prompt_hash: &str,
        context_hash: &str,
        draft_results: Option<&[DraftResult]>,
        route: &RoutePlan,
        selected_model: &RuntimeModel,
        selected_latency_ms: u64,
        agent: Option<&AgentSource>,
        structured_output: Option<StructuredOutputReceipt>,
    ) -> ChatCompletionResponse {
        let model_decisions = self.build_model_decisions(
            response.usage.as_ref(),
            winner_model_id,
            selected_model,
            selected_latency_ms,
            draft_results,
            route,
            &response,
            confidence,
        );
        let model_decisions_hash = hash_json(&model_decisions);
        let receipts_hash = hash_json(&receipts);
        let token_usage = response.usage.as_ref().map(|usage| {
            json!({
                "prompt_tokens": usage.prompt_tokens.unwrap_or(0),
                "completion_tokens": usage.completion_tokens.unwrap_or(0),
                "total_tokens": usage.total_tokens.unwrap_or(0),
            })
        });
        let mut metadata = json!({
          "request_id": request_id,
          "provider": "jnoccio",
          "model": self.config.visible_model_id,
          "route_mode": route.mode.as_str(),
          "sampled": route.sampled,
          "complexity_tier": route.complexity_tier.as_str(),
          "primary_model_id": &route.primary_model_id,
          "backup_model_ids": &route.backup_model_ids,
          "fusion_model_id": &route.fusion_model_id,
          "winner_model_id": winner_model_id,
          "winner_route_slot_id": selected_model.route_slot_id.clone(),
          "winner_upstream_model_id": selected_model.upstream_model_id.clone(),
          "credential_user_id": selected_model.credential_user_id.clone(),
          "credential_env_name": selected_model.credential_env_name.clone(),
          "upstream_key_source": selected_model.key_source.as_str(),
          "caller": match agent {
            Some(agent) => json!({
                "agent_id": agent.id.clone(),
                "agent_client": agent.client.clone(),
                "agent_session_id": agent.session_id.clone(),
                "agent_role": agent.agent_role.clone(),
                "zyal_run_id": agent.zyal_run_id.clone(),
                "zyal_lane_id": agent.zyal_lane_id.clone(),
                "credential_user_id": agent.credential_user_id.clone(),
                "credential_policy": agent.credential_policy.clone(),
            }),
            None => Value::Null,
          },
          "confidence": confidence,
          "route_confidence": confidence,
          "prompt_hash": prompt_hash,
          "context_hash": context_hash,
          "receipts_hash": receipts_hash,
          "token_usage": token_usage,
          "model_decisions_hash": model_decisions_hash,
          "model_decisions": model_decisions,
          "receipts": receipts,
          "drafts": match draft_results {
            Some(items) => json!(items.iter().map(|item| item.summary()).collect::<Vec<_>>()),
            None => Value::Null,
          },
        });
        if let Some(receipt) = structured_output
            && let Some(map) = metadata.as_object_mut()
        {
            map.insert("structured_raw_hash".to_string(), json!(receipt.raw_hash));
            map.insert(
                "structured_normalized_hash".to_string(),
                receipt
                    .normalized_hash
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            map.insert(
                "structured_repair_attempts".to_string(),
                json!(receipt.repair_attempts),
            );
            map.insert(
                "structured_schema_status".to_string(),
                json!(receipt.schema_status),
            );
            map.insert(
                "structured_error".to_string(),
                receipt.error.map(Value::String).unwrap_or(Value::Null),
            );
        }
        response.extra.insert("jnoccio".to_string(), metadata);
        response
    }

    fn build_gateway_result(&self, input: GatewayResultInput<'_>) -> GatewayResult {
        let response = self.finalize_response(
            input.request_id,
            input.response,
            input.receipts.clone(),
            Some(input.winner_model_id),
            input.confidence,
            input.prompt_hash,
            input.context_hash,
            input.draft_results,
            input.route,
            input.selected_model,
            input.selected_latency_ms,
            input.agent,
            input.structured_output,
        );
        GatewayResult {
            response,
            receipts: input.receipts,
            winner_model_id: input.winner_model_id.to_string(),
            confidence: input.confidence,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_model_decisions(
        &self,
        usage: Option<&crate::openai::ChatUsage>,
        winner_model_id: Option<&str>,
        selected_model: &RuntimeModel,
        selected_latency_ms: u64,
        draft_results: Option<&[DraftResult]>,
        route: &RoutePlan,
        response: &ChatCompletionResponse,
        confidence: f64,
    ) -> Vec<ModelDecisionRecord> {
        let mut decisions = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(items) = draft_results {
            for draft in items {
                if let Some(output) = draft.output.as_ref() {
                    if winner_model_id == Some(draft.model.visible_id.as_str()) {
                        continue;
                    }
                    if seen.insert(draft.model.visible_id.clone()) {
                        decisions.push(ModelDecisionRecord {
                            model_id: draft.model.visible_id.clone(),
                            route_slot_id: draft.model.route_slot_id.clone(),
                            upstream_model_id: draft.model.upstream_model_id.clone(),
                            credential_user_id: draft.model.credential_user_id.clone(),
                            credential_env_name: draft.model.credential_env_name.clone(),
                            configured_score: normalized_score(&draft.model.entry.score),
                            selection_score: draft_selection_score(&draft.model, route),
                            latency_ms: draft.latency_ms,
                            status: "success".to_string(),
                            output_hash: draft.output_hash.clone(),
                            selected: false,
                            token_usage: token_usage_record(output.usage.as_ref()),
                        });
                    }
                }
            }
        }
        if seen.insert(selected_model.visible_id.clone()) {
            decisions.push(ModelDecisionRecord {
                model_id: selected_model.visible_id.clone(),
                route_slot_id: selected_model.route_slot_id.clone(),
                upstream_model_id: selected_model.upstream_model_id.clone(),
                credential_user_id: selected_model.credential_user_id.clone(),
                credential_env_name: selected_model.credential_env_name.clone(),
                configured_score: normalized_score(&selected_model.entry.score),
                selection_score: confidence.clamp(0.0, 1.0),
                latency_ms: selected_latency_ms,
                status: "selected".to_string(),
                output_hash: Some(hash_json(&response.choices[0].message)),
                selected: true,
                token_usage: token_usage_record(usage),
            });
        } else if let Some(existing) = decisions
            .iter_mut()
            .find(|decision| decision.model_id == selected_model.visible_id)
        {
            existing.selected = true;
        }
        decisions
    }

    fn confidence(&self, draft_results: &[DraftResult], fusion_success: bool) -> f64 {
        let successes = draft_results
            .iter()
            .filter(|item| item.output.is_some())
            .count() as f64;
        let base = if fusion_success { 0.7 } else { 0.5 };
        (base + successes * 0.1).min(0.99)
    }

    fn build_receipts(
        &self,
        request_id: &str,
        draft_candidates: &[RuntimeModel],
        draft_results: &[DraftResult],
        fusion_candidates: &[RuntimeModel],
    ) -> Vec<String> {
        let mut receipts = vec![format!("request_id={request_id}")];
        receipts.push(format!(
            "draft_models={}",
            draft_candidates
                .iter()
                .map(|model| model.visible_id.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for result in draft_results {
            receipts.push(result.summary());
        }
        if let Some(model) = fusion_candidates.first() {
            receipts.push(format!("fusion_model={}", model.visible_id));
        } else {
            receipts.push("fusion_model=none".to_string());
        }
        receipts
    }

    fn build_route_receipts(&self, request_id: &str, route: &RoutePlan) -> Vec<String> {
        vec![
            format!("request_id={request_id}"),
            format!("route_mode={}", route.mode.as_str()),
            format!("sampled={}", route.sampled),
            format!("complexity_tier={}", route.complexity_tier.as_str()),
            format!(
                "primary_model={}",
                route.primary_model_id.as_deref().unwrap_or("none")
            ),
            format!("backup_models={}", route.backup_model_ids.join(", ")),
        ]
    }

    fn build_fusion_messages(
        &self,
        request: &ChatCompletionRequest,
        draft_results: &[DraftResult],
        receipts: &[String],
    ) -> Vec<Value> {
        let mut messages = request.messages.clone();
        let draft_text = draft_results
            .iter()
            .map(|item| item.summary())
            .collect::<Vec<_>>()
            .join("\n");
        messages.push(json!({
      "role": "system",
      "content": format!(
        "Gateway receipts:\n{}\n\nDraft summaries:\n{}\n\nProduce the best final answer. Keep tool calls valid if needed.",
        receipts.join("\n"),
        draft_text
      )
    }));
        messages
    }

    fn weight_for_model(&self, model: &RuntimeModel) -> f64 {
        if !model.is_routable_now(chrono::Utc::now().timestamp()) {
            return 0.0;
        }
        let base = (model.entry.score.power
            + model.entry.score.reliability
            + model.entry.score.integration
            + model.entry.score.latency
            + model.entry.score.free_quota) as f64
            / 5.0;
        let health = match model.state.as_ref() {
            Some(state) if state.status == "healthy" || state.status == "ready" => 1.0,
            Some(state) if state.status == "missing_key" => 0.0,
            Some(state) if state.status == "incomplete_env" => 0.0,
            Some(state) if state.status == "auth_failed" => 0.0,
            Some(state) if state.status == "customer_verification_required" => 0.0,
            Some(state) if state.status == "no_access" => 0.0,
            Some(state) if state.status == "unsupported_api" => 0.05,
            Some(state) if state.status == "model_unavailable" => 0.0,
            Some(state) if state.status == "quota_exhausted" => 0.1,
            Some(state) if state.status == "rate_limited" => 0.3,
            Some(state) if state.status == "timeout" => 0.7,
            Some(state) if state.status == "server_error" => 0.5,
            Some(state) if state.status == "invalid_response" => 0.7,
            Some(_) => 0.8,
            None => 1.0,
        };
        let latency = model
            .state
            .as_ref()
            .and_then(|state| state.last_latency_ms)
            .map(|ms| (1_500.0 / ms.max(100) as f64).clamp(0.25, 1.5))
            .unwrap_or(1.0);
        let learned = 1.0 + self.state.learned_boost(&model.visible_id).unwrap_or(1.0) - 1.0;
        (base / 100.0) * health * latency * learned
    }

    fn fallback_candidates(
        &self,
        models: &[RuntimeModel],
        excluded_ids: &[String],
    ) -> Vec<RuntimeModel> {
        let now = chrono::Utc::now().timestamp();
        let exclude = excluded_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let mut candidates = models
            .iter()
            .filter(|model| model.entry.routing.enabled)
            .filter(|model| model.is_selectable(now))
            .filter(|model| !exclude.contains(&model.visible_id))
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates = models
                .iter()
                .filter(|model| model.entry.routing.enabled)
                .filter(|model| model.is_selectable(now))
                .cloned()
                .collect::<Vec<_>>();
        }
        candidates.sort_by(|a, b| {
            self.weight_for_model(b)
                .partial_cmp(&self.weight_for_model(a))
                .unwrap_or(Ordering::Equal)
        });
        candidates
    }

    fn pick_best_draft<'a>(&self, drafts: &'a [DraftResult]) -> Option<&'a DraftResult> {
        drafts
            .iter()
            .filter_map(|draft| draft.output.as_ref().map(|output| (draft, output)))
            .max_by(|(left_result, left_output), (right_result, right_output)| {
                let left_score = left_result.model.entry.score.power as usize
                    + left_output.message.content.as_deref().unwrap_or("").len();
                let right_score = right_result.model.entry.score.power as usize
                    + right_output.message.content.as_deref().unwrap_or("").len();
                left_score.cmp(&right_score)
            })
            .map(|(draft, _)| draft)
    }

    async fn run_drafts(
        &self,
        request_id: &str,
        request: &ChatCompletionRequest,
        drafts: &[RuntimeModel],
        route: &RoutePlan,
        agent: Option<&AgentSource>,
    ) -> Vec<DraftResult> {
        let futures = drafts.iter().cloned().map(|model| async move {
            let meta = self.route_meta(route, &model, None);
            info!(
                request_id = %request_id,
                phase = "draft",
                model_id = %model.visible_id,
                provider = %model.entry.provider,
                "draft phase scheduled"
            );
            match self
                .run_phase(
                    request_id, "draft", &model, request, false, None, &meta, agent,
                )
                .await
            {
                Ok(outcome) => {
                    DraftResult::from_output(model.clone(), outcome.output, outcome.latency_ms)
                }
                Err(err) => DraftResult::from_error(model.clone(), err),
            }
        });
        futures::future::join_all(futures).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_phase(
        &self,
        request_id: &str,
        phase: &str,
        model: &RuntimeModel,
        request: &ChatCompletionRequest,
        include_tools: bool,
        messages_override: Option<Vec<Value>>,
        meta: &RouteEventMeta,
        agent: Option<&AgentSource>,
    ) -> Result<PhaseOutcome, GatewayError> {
        let attempt_event = self
            .state
            .record_attempt(
                request_id,
                phase,
                &model.visible_id,
                &model.entry.provider,
                meta,
                agent,
            )
            .map_err(upstream_err)?;
        self.emit_metric_event(attempt_event);
        self.emit_model_metric(&model.visible_id);

        let started = Instant::now();
        let request = clamp_output_tokens(request, model.entry.max_output_tokens);
        let mut messages = if let Some(messages) = messages_override {
            messages
        } else {
            request.messages.clone()
        };
        if phase == "draft" {
            messages.push(json!({
        "role": "system",
        "content": "You are a draft model. Give a concise strategic answer, avoid tool calls, and focus on approach, edge cases, and likely edits."
      }));
        }
        let tools = if include_tools {
            request.tools.clone()
        } else {
            None
        };
        let phase_request = ChatCompletionRequest {
            messages: messages.clone(),
            tools: tools.clone(),
            ..request.clone()
        };
        let phase_profile = RequestProfile::from_request(&phase_request);
        let requested_output_tokens = if let Some(tokens) = phase_profile.requested_output_tokens {
            tokens
        } else {
            model.entry.max_output_tokens.min(8_192)
        };
        let message_count = messages.len();
        let tool_count = tools
            .as_ref()
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0);
        let upstream_stream = false;
        let body = build_body(
            &phase_request,
            &model.entry.model,
            upstream_stream,
            tools,
            messages,
            model.entry.api.completion_tokens_param.as_deref(),
            &model.entry.api.style,
        );
        let body_bytes = serde_json::to_vec(&body)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        let client = self.client_for(model)?;
        info!(
            request_id = %request_id,
            phase = phase,
            model_id = %model.visible_id,
            provider = %model.entry.provider,
            api_style = %model.entry.api.style,
            message_count,
            tool_count,
            stream = upstream_stream,
            approx_prompt_tokens = phase_profile.approx_prompt_tokens,
            requested_output_tokens = requested_output_tokens,
            body_bytes,
            "upstream request started"
        );
        match client.complete(&phase_request, body).await {
            Ok(output) => {
                let latency_ms = started.elapsed().as_millis() as u64;
                let success_event = self
                    .state
                    .record_success(RecordSuccessInput {
                        request_id,
                        phase,
                        model_id: &model.visible_id,
                        provider: &model.entry.provider,
                        latency_ms,
                        winner_model_id: None,
                        usage: output.usage.as_ref(),
                        meta,
                        agent,
                    })
                    .map_err(upstream_err)?;
                self.state
                    .record_context_success(
                        request_id,
                        phase,
                        &model.visible_id,
                        &model.entry.provider,
                        phase_profile.approx_prompt_tokens,
                        requested_output_tokens,
                        output.usage.as_ref(),
                    )
                    .map_err(upstream_err)?;
                self.emit_metric_event(success_event);
                self.emit_model_metric(&model.visible_id);
                self.state.increment_quota(&model.visible_id).ok();
                info!(
                    request_id = %request_id,
                    phase = phase,
                    model_id = %model.visible_id,
                    provider = %model.entry.provider,
                    latency_ms = latency_ms,
                    prompt_tokens = output.usage.as_ref().and_then(|usage| usage.prompt_tokens).unwrap_or(0),
                    completion_tokens = output.usage.as_ref().and_then(|usage| usage.completion_tokens).unwrap_or(0),
                    total_tokens = output.usage.as_ref().and_then(|usage| usage.total_tokens).unwrap_or(0),
                    "upstream success"
                );
                Ok(PhaseOutcome {
                    response: output.clone().into_response(&model.entry.model),
                    output,
                    latency_ms,
                })
            }
            Err(err) => {
                let latency_ms = started.elapsed().as_millis() as u64;
                let text = err.summary();
                let kind = err.kind.clone();
                let parsed_signal = parse_limit_signal(&err.body);
                let retry_after = err.retry_after;
                let cooldown = cooldown_for(
                    &kind,
                    retry_after,
                    model
                        .state
                        .as_ref()
                        .map(|state| state.failure_count)
                        .unwrap_or(0),
                );
                let disabled_until = if cooldown.is_zero() {
                    None
                } else {
                    Some(
                        chrono::Utc::now().timestamp()
                            + i64::try_from(cooldown.as_secs()).unwrap_or(i64::MAX),
                    )
                };
                if matches!(kind, ErrorKind::ContextOverflow) || parsed_signal.is_some() {
                    self.state
                        .record_context_failure(
                            request_id,
                            phase,
                            &model.visible_id,
                            &model.entry.provider,
                            phase_profile.approx_prompt_tokens,
                            requested_output_tokens,
                            parsed_signal.as_ref(),
                            &kind,
                            &err.body,
                        )
                        .map_err(upstream_err)?;
                }
                let failure_event = self
                    .state
                    .record_failure(
                        request_id,
                        phase,
                        &model.visible_id,
                        &model.entry.provider,
                        &kind,
                        latency_ms,
                        disabled_until,
                        Some(&text),
                        meta,
                        agent,
                    )
                    .map_err(upstream_err)?;
                self.emit_metric_event(failure_event);
                self.emit_model_metric(&model.visible_id);
                let _ = write_failure_log(
                    &self.config.receipts_dir,
                    &model.visible_id,
                    request_id,
                    phase,
                    &err,
                    build_failure_log_entry(
                        request_id,
                        phase,
                        &model.visible_id,
                        &model.entry.model,
                        &model.entry.api.style,
                        &model.base_url,
                        &err,
                        latency_ms,
                        cooldown,
                        message_count,
                        tool_count,
                        upstream_stream,
                    ),
                );
                warn!(
                  request_id = %request_id,
                  phase = phase,
                  model_id = %model.visible_id,
                  provider = %model.entry.provider,
                  status_code = err.status_code.unwrap_or_default(),
                  error_kind = ?kind,
                  retry_after_ms = retry_after.map(|value| value.as_millis() as u64).unwrap_or(0),
                  error = %text,
                  "upstream failure"
                );
                Err(GatewayError::Upstream(text))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn enforce_structured_output(
        &self,
        request_id: &str,
        request: &ChatCompletionRequest,
        mut response: ChatCompletionResponse,
        route: &RoutePlan,
        models: &[RuntimeModel],
        selected_model: &RuntimeModel,
        agent: Option<&AgentSource>,
    ) -> Result<(ChatCompletionResponse, StructuredOutputReceipt), GatewayError> {
        let Some(schema) = structured_json_schema(request) else {
            return Ok((
                response,
                StructuredOutputReceipt {
                    raw_hash: String::new(),
                    normalized_hash: None,
                    repair_attempts: 0,
                    schema_status: "not_requested".to_string(),
                    error: None,
                },
            ));
        };

        let raw_text = structured_message_content(&response).unwrap_or_default();
        let raw_hash = hash_str(&raw_text);
        match normalize_structured_response(&mut response, &schema) {
            Ok(normalized_hash) => Ok((
                response,
                StructuredOutputReceipt {
                    raw_hash,
                    normalized_hash: Some(normalized_hash),
                    repair_attempts: 0,
                    schema_status: "valid".to_string(),
                    error: None,
                },
            )),
            Err(first_error) => {
                let repair_model = route
                    .fusion_model_id
                    .as_deref()
                    .and_then(|id| model_by_id(models, id))
                    .unwrap_or_else(|| selected_model.clone());
                let mut last_error = first_error;
                for attempt in 1..=2 {
                    let repair_request =
                        structured_repair_request(request, &raw_text, &last_error, attempt);
                    let outcome = self
                        .run_phase(
                            request_id,
                            "structured_repair",
                            &repair_model,
                            &repair_request,
                            false,
                            Some(repair_request.messages.clone()),
                            &self.route_meta(route, &repair_model, None),
                            agent,
                        )
                        .await?;
                    let mut repaired = outcome.response;
                    match normalize_structured_response(&mut repaired, &schema) {
                        Ok(normalized_hash) => {
                            return Ok((
                                repaired,
                                StructuredOutputReceipt {
                                    raw_hash,
                                    normalized_hash: Some(normalized_hash),
                                    repair_attempts: attempt,
                                    schema_status: "repaired".to_string(),
                                    error: None,
                                },
                            ));
                        }
                        Err(err) => {
                            last_error = err;
                        }
                    }
                }
                Err(GatewayError::InvalidResponse(format!(
                    "structured output failed schema enforcement after repair attempts: {last_error}; raw_hash={raw_hash}"
                )))
            }
        }
    }

    fn client_for(&self, model: &RuntimeModel) -> Result<ProviderClient, GatewayError> {
        let Some(api_key) = model.api_key.clone() else {
            let total_count = match self.runtime_models() {
                Ok(models) => models.len(),
                Err(_) => 0,
            };
            return Err(GatewayError::NoKeysConfigured {
                keyed_count: 0,
                total_count,
                env_path: self.config.env_path.display().to_string(),
            });
        };
        Ok(provider_client(
            self.http.clone(),
            &model.entry.api.style,
            model.base_url.clone(),
            api_key,
            model.entry.provider.clone(),
        ))
    }

    fn routing_inputs(&self, models: &[RuntimeModel], now: i64) -> Vec<RoutingModelInput> {
        let limit_rows = self
            .state
            .limit_estimates()
            .unwrap_or_default()
            .into_iter()
            .map(|estimate| (estimate.model_id.clone(), estimate))
            .collect::<HashMap<_, _>>();
        models
            .iter()
            .map(|model| {
                let limit = limit_rows.get(&model.visible_id);
                RoutingModelInput {
                    // Use `route_slot_id` so per-user `UsersPool` slots route
                    // independently. ConfigEnv keeps the existing
                    // `"{provider}/{id}"` shape because route_slot_id == visible_id
                    // for that path.
                    id: model.route_slot_id.clone(),
                    provider: model.entry.provider.clone(),
                    user_id: model.credential_user_id.clone(),
                    credential_env_name: Some(model.credential_env_name.clone()),
                    upstream_model_id: model.upstream_model_id.clone(),
                    ready: model.is_ready(),
                    status: model.status_label(now),
                    failure_count: model
                        .state
                        .as_ref()
                        .map(|state| state.failure_count)
                        .unwrap_or(0),
                    disabled_reason: model.disabled_reason(),
                    cooldown_until: model.cooldown_until(),
                    roles: model.entry.routing.roles.clone(),
                    routing: model.entry.routing.clone(),
                    score: model.entry.score.clone(),
                    limits: model.entry.limits.clone(),
                    context_window: model.entry.context_window,
                    configured_context_window: limit
                        .map(|limit| limit.configured_context_window)
                        .unwrap_or(model.entry.context_window),
                    safe_context_window: limit
                        .map(|limit| limit.safe_context_window)
                        .unwrap_or(model.entry.context_window),
                    learned_context_window: limit.and_then(|limit| limit.learned_context_window),
                    learned_request_token_limit: limit
                        .and_then(|limit| limit.learned_request_token_limit),
                    learned_tpm_limit: limit.and_then(|limit| limit.learned_tpm_limit),
                    recent_context_overrun_count: limit
                        .map(|limit| limit.context_overrun_count)
                        .unwrap_or(0),
                    max_output_tokens: model.entry.max_output_tokens,
                    last_latency_ms: model.state.as_ref().and_then(|state| state.last_latency_ms),
                    // Surface persistent win-rate evidence for the
                    // quality_band filter in routing.rs. Defaults to 0 for
                    // never-seen models — they're treated as unranked and
                    // admitted under Top* bands per the cold-start policy.
                    win_count: model.state.as_ref().map(|s| s.win_count).unwrap_or(0),
                    call_count: model
                        .state
                        .as_ref()
                        .map(|s| s.success_count + s.failure_count)
                        .unwrap_or(0),
                }
            })
            .collect()
    }

    fn routing_usage(&self) -> Result<HashMap<String, RoutingUsage>> {
        let rows = self
            .state
            .usage_since(chrono::Utc::now().timestamp() - 3600)?;
        let provider_attempts = rows.iter().fold(HashMap::new(), |mut acc, item| {
            *acc.entry(item.provider.clone()).or_insert(0) += item.attempts;
            acc
        });
        Ok(rows
            .into_iter()
            .map(|item| {
                (
                    item.model_id,
                    RoutingUsage {
                        one_hour_attempts: item.attempts,
                        provider_one_hour_attempts: provider_attempts
                            .get(&item.provider)
                            .copied()
                            .unwrap_or(0),
                    },
                )
            })
            .collect())
    }

    fn usage_last_hour(&self) -> Result<HashMap<String, CapacityUsage>> {
        Ok(self
            .state
            .usage_since(chrono::Utc::now().timestamp() - 3600)?
            .into_iter()
            .map(|item| {
                (
                    item.model_id,
                    CapacityUsage {
                        attempts: item.attempts,
                        successes: item.successes,
                        failures: item.failures,
                        wins: item.wins,
                        prompt_tokens: item.prompt_tokens,
                        completion_tokens: item.completion_tokens,
                        total_tokens: item.total_tokens,
                        latency_count: item.latency_count,
                        latency_total_ms: item.latency_total_ms,
                    },
                )
            })
            .collect())
    }

    fn capacity_model(&self, model: &RuntimeModel) -> CapacityModel {
        CapacityModel {
            id: model.visible_id.clone(),
            provider: model.entry.provider.clone(),
            display_name: model.entry.display_name.clone(),
            status: model.status_label(chrono::Utc::now().timestamp()),
            limits: model.entry.limits.clone(),
        }
    }

    fn route_meta(
        &self,
        route: &RoutePlan,
        model: &RuntimeModel,
        backup_rank: Option<u64>,
    ) -> RouteEventMeta {
        RouteEventMeta {
            route_mode: Some(match backup_rank {
                Some(_) if !route.sampled => RouteMode::Backup.as_str().to_string(),
                _ => route.mode.as_str().to_string(),
            }),
            backup_rank,
            complexity_tier: Some(route.complexity_tier.as_str().to_string()),
            sampled: Some(route.sampled),
            capacity_known: Some(crate::capacity::hourly_capacity(&model.entry.limits).is_some()),
        }
    }

    fn model_status_view(&self, model: &RuntimeModel) -> ModelStatusView {
        let now = chrono::Utc::now().timestamp();
        let enabled = model.is_selectable(now);
        ModelStatusView {
            id: model.entry.id.clone(),
            provider: model.entry.provider.clone(),
            route_slot_id: model.route_slot_id.clone(),
            upstream_model_id: model.upstream_model_id.clone(),
            credential_user_id: model.credential_user_id.clone(),
            credential_env_name: model.credential_env_name.clone(),
            key_source: model.key_source.as_str().to_string(),
            display_name: model.entry.display_name.clone(),
            upstream_model: model.entry.model.clone(),
            visible_id: model.visible_id.clone(),
            api_style: model.entry.api.style.clone(),
            base_url: model.entry.api.base_url.clone(),
            signup_url: model.entry.signup_url.clone(),
            key_present: model.has_key(),
            enabled,
            status: model.status_label(now),
            disabled_reason: model.disabled_reason(),
            cooldown_until: model.cooldown_until(),
            roles: model.entry.routing.roles.clone(),
            context_window: model.entry.context_window,
            max_output_tokens: model.entry.max_output_tokens,
            limits: json!(model.entry.limits),
            score: json!(model.entry.score),
            state: model.state.clone(),
        }
    }

    fn dashboard_model(
        &self,
        model: &RuntimeModel,
        metric: Option<&ModelMetric>,
        limit: Option<&ModelLimitEstimate>,
        hourly_used: u64,
    ) -> DashboardModel {
        let now = chrono::Utc::now().timestamp();
        let empty = ModelMetric {
            model_id: model.visible_id.clone(),
            provider: model.entry.provider.clone(),
            call_count: model
                .state
                .as_ref()
                .map(|state| state.success_count + state.failure_count)
                .unwrap_or(0),
            success_count: model
                .state
                .as_ref()
                .map(|state| state.success_count)
                .unwrap_or(0),
            failure_count: model
                .state
                .as_ref()
                .map(|state| state.failure_count)
                .unwrap_or(0),
            win_count: model
                .state
                .as_ref()
                .map(|state| state.win_count)
                .unwrap_or(0),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            latency_count: model
                .state
                .as_ref()
                .and_then(|state| state.last_latency_ms)
                .map(|_| 1)
                .unwrap_or(0),
            latency_total_ms: model
                .state
                .as_ref()
                .and_then(|state| state.last_latency_ms)
                .unwrap_or(0),
            latency_min_ms: model.state.as_ref().and_then(|state| state.last_latency_ms),
            latency_max_ms: model.state.as_ref().and_then(|state| state.last_latency_ms),
            last_latency_ms: model.state.as_ref().and_then(|state| state.last_latency_ms),
            last_error_kind: model
                .state
                .as_ref()
                .and_then(|state| state.last_error_kind.clone()),
            last_error_message: model
                .state
                .as_ref()
                .and_then(|state| state.last_error_message.clone()),
            updated_at: model
                .state
                .as_ref()
                .map(|state| state.updated_at)
                .unwrap_or(now),
        };
        let metric = metric.unwrap_or(&empty);
        DashboardModel {
            id: model.visible_id.clone(),
            provider: model.entry.provider.clone(),
            display_name: model.entry.display_name.clone(),
            upstream_model: model.entry.model.clone(),
            roles: model.entry.routing.roles.clone(),
            enabled: model.is_selectable(now),
            status: model.status_label(now),
            cooldown_until: model.cooldown_until(),
            capacity_known: crate::capacity::hourly_capacity(&model.entry.limits).is_some(),
            hourly_capacity: crate::capacity::hourly_capacity(&model.entry.limits)
                .map(|capacity| capacity.capacity),
            hourly_used,
            configured_context_window: limit
                .map(|limit| limit.configured_context_window)
                .unwrap_or(model.entry.context_window),
            safe_context_window: limit
                .map(|limit| limit.safe_context_window)
                .unwrap_or(model.entry.context_window),
            learned_context_window: limit.and_then(|limit| limit.learned_context_window),
            learned_request_token_limit: limit.and_then(|limit| limit.learned_request_token_limit),
            context_overrun_count: limit.map(|limit| limit.context_overrun_count).unwrap_or(0),
            smallest_overrun_requested_tokens: limit
                .and_then(|limit| limit.smallest_overrun_requested_tokens),
            call_count: metric.call_count,
            success_count: metric.success_count,
            failure_count: metric.failure_count,
            win_count: metric.win_count,
            win_rate: ratio(metric.win_count, metric.call_count),
            prompt_tokens: metric.prompt_tokens,
            completion_tokens: metric.completion_tokens,
            total_tokens: metric.total_tokens,
            avg_latency_ms: metric_average(metric),
            last_latency_ms: metric.last_latency_ms,
            min_latency_ms: metric.latency_min_ms,
            max_latency_ms: metric.latency_max_ms,
            last_error_kind: metric.last_error_kind.clone(),
            last_error_message: metric.last_error_message.clone(),
            updated_at: metric.updated_at,
        }
    }

    /// Inspects every model to determine the real reason no models passed
    /// eligibility. Returns the most specific error variant instead of a
    /// misleading context-window error.
    fn diagnose_no_eligible_models(
        &self,
        routing_inputs: &[RoutingModelInput],
        models: &[RuntimeModel],
        profile: &RequestProfile,
        now: i64,
    ) -> GatewayError {
        let total = models.len();
        if total == 0 {
            return GatewayError::NoAvailableModels;
        }

        let env_path = self.config.env_path.display().to_string();
        let availability = availability_counts(models, now);
        let required_tokens = profile
            .approx_prompt_tokens
            .saturating_add(profile.requested_output_tokens.unwrap_or(8_192));
        let routing = routing_eligibility(routing_inputs, required_tokens, now);

        // Count failure categories across all models
        let missing_key_count = availability.missing_key;
        let incomplete_env_count = availability.incomplete_env;
        let keyed_and_env_ok = total - missing_key_count - incomplete_env_count;

        // If most/all models have no keys, that's the primary problem
        if missing_key_count + incomplete_env_count == total {
            return GatewayError::NoKeysConfigured {
                keyed_count: 0,
                total_count: total,
                env_path,
            };
        }

        // Check models that DO have keys but still aren't eligible
        // If few models have keys relative to total, surface that
        if keyed_and_env_ok < 3 && missing_key_count > 0 {
            return GatewayError::NoKeysConfigured {
                keyed_count: keyed_and_env_ok,
                total_count: total,
                env_path,
            };
        }

        // If keyed models are all in hard-disabled states
        if routing.hard_disabled_count > 0
            && routing.cooldown_count == 0
            && routing.context_too_small_count == 0
        {
            return GatewayError::AllModelsUnavailable {
                total_count: total,
                summary: routing.hard_disabled_summary.clone(),
                env_path,
            };
        }

        // All eligible models in cooldown
        if routing.cooldown_count > 0 && routing.context_too_small_count == 0 {
            return GatewayError::AllModelsInCooldown {
                cooldown_count: routing.cooldown_count,
                total_count: total,
            };
        }

        // Genuine context overflow — all remaining eligible models have
        // context windows too small
        if routing.context_too_small_count > 0 {
            return GatewayError::NoContextSafeModel {
                required_total_tokens: required_tokens,
                largest_safe_context_window: routing.largest_safe_context_window,
            };
        }

        // Mixed / unclear — build a comprehensive summary
        let mut parts = Vec::new();
        if missing_key_count > 0 {
            parts.push(format!("{missing_key_count} missing keys"));
        }
        if incomplete_env_count > 0 {
            parts.push(format!("{incomplete_env_count} incomplete env"));
        }
        if routing.hard_disabled_count > 0 {
            parts.push(format!("{} unavailable", routing.hard_disabled_count));
        }
        if routing.cooldown_count > 0 {
            parts.push(format!("{} in cooldown", routing.cooldown_count));
        }
        if routing.routing_disabled_count > 0 {
            parts.push(format!(
                "{} routing disabled",
                routing.routing_disabled_count
            ));
        }
        let summary = if parts.is_empty() {
            "unknown reason".to_string()
        } else {
            parts.join(", ")
        };
        GatewayError::AllModelsUnavailable {
            total_count: total,
            summary,
            env_path,
        }
    }

    /// OpenAI-compatible `/v1/embeddings`. Routes to the first configured
    /// embedding-capable model when one exists, otherwise returns a
    /// deterministic sha256-derived 1536-dim fake so cold-start runs and
    /// tests work without provider setup.
    ///
    /// The "real" path is best-effort — if the upstream call fails (network,
    /// shape mismatch) we fall back to the deterministic path rather than
    /// surfacing the failure, because Phase E2 retrieval is meant to degrade
    /// gracefully when the embedder is unavailable.
    pub async fn embed(
        &self,
        request: EmbeddingsRequest,
    ) -> Result<EmbeddingsResponse, GatewayError> {
        let inputs: Vec<String> = request.input.clone().into_vec();
        if inputs.is_empty() {
            return Err(GatewayError::InvalidResponse(
                "embeddings request input was empty".to_string(),
            ));
        }

        // Best-effort real upstream: pick the first runtime model whose
        // entry.id or upstream model id contains "embedding" and that has a
        // key configured. Most jnoccio-fusion deployments today only configure
        // chat-completion models, so this commonly returns None and we drop
        // into the deterministic path.
        if let Ok(models) = self.runtime_models()
            && let Some(target) = models.iter().find(|model| {
                model.has_key()
                    && (model.entry.id.contains("embedding")
                        || model.upstream_model_id.contains("embedding"))
            })
            && let Some(response) = self
                .embed_via_provider(target, &request.model, &inputs)
                .await
        {
            return Ok(response);
        }

        Ok(deterministic_fake_embeddings(&inputs))
    }

    async fn embed_via_provider(
        &self,
        model: &RuntimeModel,
        requested_model: &str,
        inputs: &[String],
    ) -> Option<EmbeddingsResponse> {
        let api_key = model.api_key.clone()?;
        let base_url = model.base_url.trim_end_matches('/').to_string();
        let endpoint = format!("{base_url}/embeddings");
        let upstream_model = if !requested_model.is_empty() {
            requested_model.to_string()
        } else {
            model.upstream_model_id.clone()
        };
        let body = json!({
            "model": upstream_model,
            "input": inputs,
        });
        let response = self
            .http
            .post(&endpoint)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let parsed: Value = response.json().await.ok()?;
        parse_embeddings_response(parsed, &model.visible_id)
    }
}

fn parse_embeddings_response(value: Value, fallback_model: &str) -> Option<EmbeddingsResponse> {
    let data = value.get("data")?.as_array()?.clone();
    let mut objects: Vec<EmbeddingObject> = Vec::with_capacity(data.len());
    for (index, item) in data.into_iter().enumerate() {
        let embedding = item
            .get("embedding")?
            .as_array()?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<f32>>();
        if embedding.is_empty() {
            return None;
        }
        let idx = item
            .get("index")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(index);
        objects.push(EmbeddingObject {
            kind: "embedding".to_string(),
            embedding,
            index: idx,
        });
    }
    if objects.is_empty() {
        return None;
    }
    let usage = value
        .get("usage")
        .map(|u| EmbeddingsUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0),
        })
        .unwrap_or_default();
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model)
        .to_string();
    Some(EmbeddingsResponse {
        kind: "list".to_string(),
        data: objects,
        model,
        usage,
    })
}

/// Deterministic sha256-derived 1536-dim fake embedding. For each input,
/// hash with sha256, then read the 32-byte digest as 8 little-endian f32s
/// (8 × 4 = 32) and tile to 1536 dims (8 × 192). Values are clamped to
/// `[-1, 1]` by dividing by the max absolute byte interpretation, so the
/// vector stays bounded and produces stable cosine similarities for
/// identical inputs.
pub(crate) fn deterministic_fake_embeddings(inputs: &[String]) -> EmbeddingsResponse {
    const DIMS: usize = 1536;
    let mut data = Vec::with_capacity(inputs.len());
    let mut total_chars: u64 = 0;
    for (index, text) in inputs.iter().enumerate() {
        total_chars += text.chars().count() as u64;
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let digest = hasher.finalize();
        let mut base = [0f32; 8];
        for (slot, chunk) in base.iter_mut().zip(digest.chunks_exact(4)) {
            let arr: [u8; 4] = chunk.try_into().expect("4-byte chunk");
            // Map the 4-byte chunk into a stable [-1, 1] float by treating
            // it as a little-endian u32, scaling to [0, 1], then shifting
            // to [-1, 1]. f32::from_le_bytes would produce NaNs/Infs for
            // arbitrary hash bytes, so the manual mapping is safer.
            let scaled = (u32::from_le_bytes(arr) as f32) / (u32::MAX as f32);
            *slot = scaled * 2.0 - 1.0;
        }
        let mut embedding = Vec::with_capacity(DIMS);
        while embedding.len() < DIMS {
            for value in base.iter() {
                if embedding.len() >= DIMS {
                    break;
                }
                embedding.push(*value);
            }
        }
        data.push(EmbeddingObject {
            kind: "embedding".to_string(),
            embedding,
            index,
        });
    }
    // Rough token estimate so callers metering usage see a non-zero value.
    let prompt_tokens = (total_chars / 4).max(1);
    EmbeddingsResponse {
        kind: "list".to_string(),
        data,
        model: "jnoccio/fake-embeddings".to_string(),
        usage: EmbeddingsUsage {
            prompt_tokens,
            total_tokens: prompt_tokens,
        },
    }
}

fn model_by_id(models: &[RuntimeModel], id: &str) -> Option<RuntimeModel> {
    // Route plan ids come from `RoutingModelInput.id`, which is now
    // `route_slot_id` (so per-user `UsersPool` slots route independently).
    // ConfigEnv collapses to visible_id == route_slot_id, so the legacy
    // visible_id lookup still works for that path.
    models
        .iter()
        .find(|model| model.route_slot_id == id)
        .cloned()
}

/// Pick the model whose budget should gate this request. The primary route
/// is the natural target; for sampled fusion routes we fall back to the
/// fusion model, then the first draft, so the hook is never called with a
/// stale picker. Returns `None` if no slot was selected — the route plan
/// will already have failed in that case.
fn budget_gate_target(route: &RoutePlan, models: &[RuntimeModel]) -> Option<RuntimeModel> {
    if let Some(id) = route.primary_model_id.as_deref()
        && let Some(model) = model_by_id(models, id)
    {
        return Some(model);
    }
    if let Some(id) = route.fusion_model_id.as_deref()
        && let Some(model) = model_by_id(models, id)
    {
        return Some(model);
    }
    route
        .draft_model_ids
        .iter()
        .find_map(|id| model_by_id(models, id))
}

fn structured_json_schema(request: &ChatCompletionRequest) -> Option<Value> {
    let format = request.response_format.as_ref()?.as_object()?;
    if format.get("type").and_then(Value::as_str) != Some("json_schema") {
        return None;
    }
    let json_schema = format.get("json_schema")?.as_object()?;
    json_schema
        .get("schema")
        .cloned()
        .or_else(|| Some(Value::Object(Map::new())))
}

fn structured_message_content(response: &ChatCompletionResponse) -> Option<String> {
    response
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
}

fn normalize_structured_response(
    response: &mut ChatCompletionResponse,
    schema: &Value,
) -> Result<String, String> {
    let choice = response
        .choices
        .first_mut()
        .ok_or_else(|| "structured response has no choices".to_string())?;
    if choice.finish_reason.as_deref() == Some("length") {
        return Err("structured response was truncated".to_string());
    }
    if choice
        .message
        .reasoning_text
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
        || choice
            .message
            .reasoning_content
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
        || choice
            .message
            .reasoning_opaque
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        return Err("structured response included reasoning outside JSON content".to_string());
    }
    let content = choice
        .message
        .content
        .as_deref()
        .ok_or_else(|| "structured response content is empty".to_string())?;
    let value = parse_exact_json(content)?;
    validate_json_schema(&value, schema, "$")?;
    let canonical = serde_json::to_string(&value)
        .map_err(|err| format!("structured JSON canonicalization failed: {err}"))?;
    let normalized_hash = hash_str(&canonical);
    choice.message.content = Some(canonical);
    choice.message.reasoning_text = None;
    choice.message.reasoning_content = None;
    choice.message.reasoning_opaque = None;
    Ok(normalized_hash)
}

fn parse_exact_json(text: &str) -> Result<Value, String> {
    let mut deserializer = serde_json::Deserializer::from_str(text.trim());
    let value = Value::deserialize(&mut deserializer)
        .map_err(|err| format!("structured response is not exact JSON: {err}"))?;
    deserializer
        .end()
        .map_err(|err| format!("structured response has trailing non-JSON content: {err}"))?;
    Ok(value)
}

fn structured_repair_request(
    request: &ChatCompletionRequest,
    raw_text: &str,
    error: &str,
    attempt: u64,
) -> ChatCompletionRequest {
    let messages = vec![
        json!({
            "role": "system",
            "content": "You repair structured model output. Return one JSON value only. Do not include markdown, prose, comments, reasoning, or multiple JSON values."
        }),
        json!({
            "role": "user",
            "content": format!(
                "Repair attempt {attempt}. The previous response failed validation: {error}\n\nRequired response_format:\n{}\n\nInvalid response:\n{}",
                request
                    .response_format
                    .as_ref()
                    .map(Value::to_string)
                    .unwrap_or_else(|| "{}".to_string()),
                raw_text
            )
        }),
    ];
    ChatCompletionRequest {
        model: request.model.clone(),
        messages,
        stream: Some(false),
        temperature: Some(0.0),
        top_p: request.top_p,
        max_tokens: request.max_tokens,
        max_completion_tokens: request.max_completion_tokens,
        tools: None,
        tool_choice: None,
        reasoning_effort: None,
        response_format: request.response_format.clone(),
        stream_options: None,
        extra: Map::new(),
    }
}

fn validate_json_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if schema.is_null() {
        return Ok(());
    }
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    if let Some(enum_values) = object.get("enum").and_then(Value::as_array)
        && !enum_values.iter().any(|candidate| candidate == value)
    {
        return Err(format!("{path} is not one of the allowed enum values"));
    }
    if let Some(const_value) = object.get("const")
        && const_value != value
    {
        return Err(format!("{path} does not match the required const value"));
    }
    if let Some(all_of) = object.get("allOf").and_then(Value::as_array) {
        for (index, child) in all_of.iter().enumerate() {
            validate_json_schema(value, child, &format!("{path}.allOf[{index}]"))?;
        }
    }
    if let Some(any_of) = object.get("anyOf").and_then(Value::as_array)
        && !any_of
            .iter()
            .any(|child| validate_json_schema(value, child, path).is_ok())
    {
        return Err(format!("{path} did not match any anyOf schema"));
    }
    if let Some(one_of) = object.get("oneOf").and_then(Value::as_array) {
        let matches = one_of
            .iter()
            .filter(|child| validate_json_schema(value, child, path).is_ok())
            .count();
        if matches != 1 {
            return Err(format!(
                "{path} matched {matches} oneOf schemas, expected 1"
            ));
        }
    }
    if let Some(type_spec) = object.get("type") {
        validate_json_schema_type(value, type_spec, path)?;
    }
    match value {
        Value::Object(map) => validate_json_schema_object(map, object, path),
        Value::Array(items) => validate_json_schema_array(items, object, path),
        Value::Number(number) => {
            if object
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|minimum| number.as_f64().unwrap_or(f64::NAN) < minimum)
            {
                return Err(format!("{path} is below minimum"));
            }
            if object
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number.as_f64().unwrap_or(f64::NAN) > maximum)
            {
                return Err(format!("{path} is above maximum"));
            }
            Ok(())
        }
        Value::String(text) => {
            if object
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|minimum| text.chars().count() < minimum as usize)
            {
                return Err(format!("{path} is shorter than minLength"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_json_schema_type(value: &Value, type_spec: &Value, path: &str) -> Result<(), String> {
    let matches_type = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    };
    match type_spec {
        Value::String(kind) if matches_type(kind) => Ok(()),
        Value::String(kind) => Err(format!("{path} expected JSON schema type {kind}")),
        Value::Array(kinds) if kinds.iter().filter_map(Value::as_str).any(matches_type) => Ok(()),
        Value::Array(_) => Err(format!("{path} did not match any allowed JSON schema type")),
        _ => Ok(()),
    }
}

fn validate_json_schema_object(
    map: &Map<String, Value>,
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), String> {
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !map.contains_key(key) {
                return Err(format!("{path}.{key} is required"));
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, child_schema) in properties {
            if let Some(child_value) = map.get(key) {
                validate_json_schema(child_value, child_schema, &format!("{path}.{key}"))?;
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            for key in map.keys() {
                if !properties.contains_key(key) {
                    return Err(format!("{path}.{key} is not allowed by schema"));
                }
            }
        }
    }
    Ok(())
}

fn validate_json_schema_array(
    items: &[Value],
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), String> {
    if schema
        .get("minItems")
        .and_then(Value::as_u64)
        .is_some_and(|minimum| items.len() < minimum as usize)
    {
        return Err(format!("{path} has fewer than minItems"));
    }
    if schema
        .get("maxItems")
        .and_then(Value::as_u64)
        .is_some_and(|maximum| items.len() > maximum as usize)
    {
        return Err(format!("{path} has more than maxItems"));
    }
    if let Some(item_schema) = schema.get("items") {
        for (index, item) in items.iter().enumerate() {
            validate_json_schema(item, item_schema, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AvailabilityCounts {
    missing_key: usize,
    incomplete_env: usize,
    disabled: usize,
    cooldown: usize,
    ready: usize,
}

fn availability_counts(models: &[RuntimeModel], now: i64) -> AvailabilityCounts {
    models
        .iter()
        .fold(AvailabilityCounts::default(), |mut counts, model| {
            match model.availability(now) {
                ModelAvailability::MissingKey => counts.missing_key += 1,
                ModelAvailability::IncompleteEnv => counts.incomplete_env += 1,
                ModelAvailability::Disabled => counts.disabled += 1,
                ModelAvailability::Cooldown => counts.cooldown += 1,
                ModelAvailability::Ready => counts.ready += 1,
            }
            counts
        })
}

fn count_ready_slots_by_user(models: &[RuntimeModel]) -> BTreeMap<String, usize> {
    let now = chrono::Utc::now().timestamp();
    let mut counts = BTreeMap::new();
    for model in models.iter().filter(|model| model.is_routable_now(now)) {
        let Some(user_id) = model.credential_user_id.as_ref() else {
            continue;
        };
        *counts.entry(user_id.clone()).or_insert(0) += 1;
    }
    counts
}

fn count_ready_slots_by_provider(models: &[RuntimeModel]) -> BTreeMap<String, usize> {
    let now = chrono::Utc::now().timestamp();
    let mut counts = BTreeMap::new();
    for model in models.iter().filter(|model| model.is_routable_now(now)) {
        *counts.entry(model.entry.provider.clone()).or_insert(0) += 1;
    }
    counts
}

#[derive(Clone, Debug, Default)]
struct RoutingEligibility {
    hard_disabled_count: usize,
    routing_disabled_count: usize,
    cooldown_count: usize,
    context_too_small_count: usize,
    largest_safe_context_window: u64,
    hard_disabled_summary: String,
}

fn routing_eligibility(
    routing_inputs: &[RoutingModelInput],
    required_tokens: u64,
    now: i64,
) -> RoutingEligibility {
    let hard_disabled_statuses = [
        "missing_key",
        "incomplete_env",
        "auth_failed",
        "customer_verification_required",
        "no_access",
        "unsupported_api",
        "model_unavailable",
        "quota_exhausted",
    ];
    let mut status_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    let mut eligibility = RoutingEligibility::default();

    for model in routing_inputs {
        if !model.routing.enabled || model.disabled_reason.is_some() {
            eligibility.routing_disabled_count += 1;
        }

        let cooldown = model
            .cooldown_until
            .map(|until| until > now)
            .unwrap_or(false);
        let eligible_for_routing =
            model.ready && model.routing.enabled && model.disabled_reason.is_none();

        if eligible_for_routing && hard_disabled_statuses.contains(&model.status.as_str()) {
            eligibility.hard_disabled_count += 1;
            *status_counts.entry(model.status.as_str()).or_default() += 1;
        }

        if eligible_for_routing && cooldown {
            eligibility.cooldown_count += 1;
        }

        if eligible_for_routing
            && model
                .cooldown_until
                .map(|until| until <= now)
                .unwrap_or(true)
            && !hard_disabled_statuses.contains(&model.status.as_str())
        {
            eligibility.largest_safe_context_window = eligibility
                .largest_safe_context_window
                .max(model.safe_context_window);
            if model.safe_context_window < required_tokens {
                eligibility.context_too_small_count += 1;
            }
        }
    }

    eligibility.hard_disabled_summary = status_counts
        .iter()
        .map(|(status, count)| format!("{count} {status}"))
        .collect::<Vec<_>>()
        .join(", ");
    eligibility
}

impl RuntimeModel {
    fn has_key(&self) -> bool {
        self.key_present
    }

    fn is_ready(&self) -> bool {
        self.has_key() && self.base_url_missing_keys.is_empty() && !self.base_url.trim().is_empty()
    }

    fn disabled_reason(&self) -> Option<String> {
        self.entry.routing.disabled_reason.clone()
    }

    fn cooldown_until(&self) -> Option<i64> {
        self.state.as_ref().and_then(|state| state.disabled_until)
    }

    fn is_routable_now(&self, now: i64) -> bool {
        matches!(self.availability(now), ModelAvailability::Ready)
    }

    fn is_selectable(&self, now: i64) -> bool {
        self.is_routable_now(now)
    }

    fn status_label(&self, now: i64) -> String {
        match self.availability(now) {
            ModelAvailability::MissingKey => "missing_key".to_string(),
            ModelAvailability::IncompleteEnv => "incomplete_env".to_string(),
            ModelAvailability::Disabled => "disabled".to_string(),
            ModelAvailability::Cooldown => {
                if let Some(state) = self.state.as_ref() {
                    state.status.clone()
                } else {
                    "cooldown".to_string()
                }
            }
            ModelAvailability::Ready => {
                if let Some(state) = self.state.as_ref() {
                    if is_hard_disabled_status(&state.status)
                        && state
                            .disabled_until
                            .map(|until| until <= now)
                            .unwrap_or(false)
                    {
                        return "ready".to_string();
                    }
                    state.status.clone()
                } else {
                    "ready".to_string()
                }
            }
        }
    }

    fn readiness_status(&self) -> &'static str {
        match self.availability(chrono::Utc::now().timestamp()) {
            ModelAvailability::MissingKey => "missing_key",
            ModelAvailability::IncompleteEnv => "incomplete_env",
            ModelAvailability::Disabled => "disabled",
            ModelAvailability::Cooldown => "cooldown",
            ModelAvailability::Ready => "ready",
        }
    }

    fn availability(&self, now: i64) -> ModelAvailability {
        if !self.has_key() {
            return ModelAvailability::MissingKey;
        }
        if !self.base_url_missing_keys.is_empty() || self.base_url.trim().is_empty() {
            return ModelAvailability::IncompleteEnv;
        }
        if self.disabled_reason().is_some() {
            return ModelAvailability::Disabled;
        }
        if self
            .cooldown_until()
            .map(|until| until > now)
            .unwrap_or(false)
        {
            return ModelAvailability::Cooldown;
        }
        if let Some(state) = self.state.as_ref()
            && is_hard_disabled_status(&state.status)
            && state.disabled_until.is_none()
        {
            return ModelAvailability::Disabled;
        }
        ModelAvailability::Ready
    }
}

enum ModelAvailability {
    MissingKey,
    IncompleteEnv,
    Disabled,
    Cooldown,
    Ready,
}

#[derive(Clone, Debug)]
struct DraftResult {
    model: RuntimeModel,
    output: Option<UpstreamCompletion>,
    error: Option<String>,
    latency_ms: u64,
    // Populated for telemetry parity with single-shot calls but currently
    // unused by the fusion reducer. Keep the field so downstream consumers
    // can pick it up without re-plumbing.
    #[allow(dead_code)]
    token_usage: Option<crate::openai::ChatUsage>,
    output_hash: Option<String>,
}

impl DraftResult {
    fn from_error(model: RuntimeModel, error: GatewayError) -> Self {
        Self {
            model,
            output: None,
            error: Some(error.to_string()),
            latency_ms: 0,
            token_usage: None,
            output_hash: None,
        }
    }

    fn from_output(model: RuntimeModel, output: UpstreamCompletion, latency_ms: u64) -> Self {
        let output_hash = hash_json(&output.message);
        Self {
            model,
            output: Some(output.clone()),
            error: None,
            latency_ms,
            token_usage: output.usage.clone(),
            output_hash: Some(output_hash),
        }
    }

    fn summary(&self) -> String {
        if let Some(output) = &self.output {
            let mut parts = Vec::new();
            if let Some(text) = output
                .message
                .content
                .as_deref()
                .filter(|text| !text.trim().is_empty())
            {
                parts.push(text.trim().chars().take(400).collect::<String>());
            }
            if let Some(tools) = &output.message.tool_calls {
                parts.push(format!("tool_calls: {}", tools.len()));
            }
            if parts.is_empty() {
                parts.push("empty".to_string());
            }
            return format!("{} -> {}", self.model.visible_id, parts.join(" | "));
        }
        let error = if let Some(error) = &self.error {
            error.clone()
        } else {
            "unknown".to_string()
        };
        format!("{} -> error: {}", self.model.visible_id, error)
    }
}

fn hash_json<T: serde::Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

fn hash_str(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn normalized_score(score: &crate::config::ModelScore) -> f64 {
    ((score.power + score.free_quota + score.reliability + score.integration + score.latency)
        as f64
        / 500.0)
        .clamp(0.0, 1.0)
}

fn draft_selection_score(model: &RuntimeModel, route: &RoutePlan) -> f64 {
    let base = normalized_score(&model.entry.score);
    let route_bias = match route.mode.as_str() {
        "fast" => 0.55,
        "backup" => 0.65,
        "fusion_sample" => 0.75,
        _ => 0.50,
    };
    (base * route_bias).clamp(0.0, 1.0)
}

fn token_usage_record(usage: Option<&crate::openai::ChatUsage>) -> TokenUsageRecord {
    TokenUsageRecord {
        prompt_tokens: usage.and_then(|item| item.prompt_tokens).unwrap_or(0),
        completion_tokens: usage.and_then(|item| item.completion_tokens).unwrap_or(0),
        total_tokens: usage.and_then(|item| item.total_tokens).unwrap_or(0),
    }
}

fn request_hashes(request: &ChatCompletionRequest) -> (String, String) {
    let prompt_hash = hash_json(&request.messages);
    let context_hash = hash_json(&json!({
        "messages": request.messages,
        "temperature": request.temperature,
        "top_p": request.top_p,
        "max_tokens": request.max_tokens,
        "max_completion_tokens": request.max_completion_tokens,
        "tools": request.tools,
        "tool_choice": request.tool_choice,
        "reasoning_effort": request.reasoning_effort,
        "response_format": request.response_format,
        "stream_options": request.stream_options,
        "extra": request.extra,
    }));
    (prompt_hash, context_hash)
}

struct PhaseOutcome {
    response: ChatCompletionResponse,
    output: UpstreamCompletion,
    latency_ms: u64,
}

fn is_hard_disabled_status(status: &str) -> bool {
    matches!(
        status,
        "auth_failed"
            | "customer_verification_required"
            | "no_access"
            | "unsupported_api"
            | "model_unavailable"
            | "quota_exhausted"
    )
}

fn model_matches_visible(visible_model_id: &str, request_model: &str) -> bool {
    if canonical_visible_model_alias(visible_model_id)
        == canonical_visible_model_alias(request_model)
    {
        return true;
    }
    if request_model == visible_model_id {
        return true;
    }

    if request_model
        == visible_model_id
            .rsplit('/')
            .next()
            .unwrap_or(visible_model_id)
    {
        return true;
    }

    visible_model_id == request_model.rsplit('/').next().unwrap_or(request_model)
}

fn canonical_visible_model_alias(value: &str) -> &str {
    match value.strip_prefix("jnoccio/").unwrap_or(value) {
        "jnoccio-router" => "jnoccio-fusion",
        other => other,
    }
}

fn upstream_attempt_timeout() -> std::time::Duration {
    let secs = std::env::var("JNOCCIO_UPSTREAM_ATTEMPT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(150)
        .clamp(5, 300);
    std::time::Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AppConfig, InstanceRole, ModelApi, ModelCapabilities, ModelEnv, ModelLimits, ModelRouting,
        ModelScore, Registry, RuntimeSettings, ScalingSettings, ServerConfig,
    };
    use std::collections::HashMap;

    fn test_runtime_model(
        id: &str,
        key_present: bool,
        base_url: &str,
        base_url_missing_keys: Vec<&str>,
        enabled: bool,
        disabled_reason: Option<String>,
    ) -> RuntimeModel {
        RuntimeModel {
            entry: crate::config::ModelEntry {
                id: id.to_string(),
                provider: "provider".to_string(),
                model: format!("provider/{id}"),
                display_name: id.to_string(),
                api: ModelApi {
                    style: "openai_chat".to_string(),
                    base_url: "https://example.com".to_string(),
                    completion_tokens_param: None,
                },
                env: ModelEnv {
                    api_key: "API_KEY".to_string(),
                },
                signup_url: "https://example.com".to_string(),
                limits: ModelLimits {
                    rpm: None,
                    rpd: None,
                    rpd_after_10_usd_credits: None,
                    source_url: None,
                },
                context_window: 1024,
                max_output_tokens: 128,
                capabilities: ModelCapabilities {
                    streaming: true,
                    tools: true,
                    reasoning: false,
                    openai_compatible: true,
                },
                score: ModelScore {
                    power: 1,
                    free_quota: 1,
                    reliability: 1,
                    integration: 1,
                    latency: 1,
                },
                routing: ModelRouting {
                    enabled,
                    roles: vec!["draft".to_string()],
                    exploration_floor: 0.1,
                    cooldown_seconds: 1,
                    disabled_reason,
                },
            },
            visible_id: format!("provider/{id}"),
            route_slot_id: format!("provider/{id}"),
            upstream_model_id: format!("provider/{id}"),
            credential_user_id: None,
            credential_env_name: "API_KEY".to_string(),
            key_source: UpstreamKeySource::ConfigEnv,
            api_key: if key_present {
                Some("test-key".to_string())
            } else {
                None
            },
            key_present,
            base_url: base_url.to_string(),
            base_url_missing_keys: base_url_missing_keys
                .into_iter()
                .map(ToString::to_string)
                .collect(),
            state: None,
        }
    }

    #[test]
    fn status_view_exposes_disabled_reason() {
        let interim = tempfile::tempdir().unwrap();
        let gateway = Gateway {
            config: AppConfig {
                config_path: interim.path().join("config/server.json"),
                env_path: interim.path().join(".env.jnoccio"),
                root: interim.path().to_path_buf(),
                server: ServerConfig {
                    bind: None,
                    database: None,
                    env_file: None,
                    models_file: None,
                    receipts_dir: None,
                    model: None,
                    provider: None,
                    core_token: None,
                    routing: None,
                    runtime: None,
                    scaling: None,
                    upstream_key_source: None,
                },
                registry: Registry {
                    schema_version: 1,
                    models: vec![],
                },
                env: HashMap::new(),
                bind: "127.0.0.1:4317".to_string(),
                database: interim.path().join("state.sqlite"),
                receipts_dir: interim.path().join("receipts"),
                visible_model_id: "jnoccio/jnoccio-fusion".to_string(),
                provider_id: "jnoccio".to_string(),
                upstream_key_source: UpstreamKeySource::ConfigEnv,
                routing: crate::config::RoutingDefaults::from_config(None),
                runtime: RuntimeSettings::from_config(None).unwrap(),
                scaling: ScalingSettings::from_config(None).unwrap(),
                instance_role: InstanceRole::Main,
                worker_threads: RuntimeSettings::from_config(None).unwrap().worker_threads,
                core_token: None,
            },
            state: Arc::new(StateDb::open(interim.path().join("state.sqlite")).unwrap()),
            mcp: Arc::new(McpState::new(
                InstanceRole::Main,
                ScalingSettings::from_config(None).unwrap(),
            )),
            events: broadcast::channel(16).0,
            http: reqwest::Client::new(),
            policy_hook: Arc::new(zyal_key_pool::AlwaysAllow),
        };
        let model = test_runtime_model(
            "test",
            true,
            "https://example.com",
            vec![],
            false,
            Some("billing required".to_string()),
        );

        let status = gateway.model_status_view(&model);
        assert_eq!(status.disabled_reason.as_deref(), Some("billing required"));
        assert!(!status.enabled);
    }

    #[test]
    fn matches_visible_model_accepts_bare_slug() {
        assert!(model_matches_visible(
            "jnoccio/jnoccio-fusion",
            "jnoccio-fusion"
        ));
        assert!(model_matches_visible(
            "jnoccio/jnoccio-fusion",
            "jnoccio/jnoccio-fusion"
        ));
        assert!(model_matches_visible(
            "jnoccio-fusion",
            "jnoccio/jnoccio-fusion"
        ));
        assert!(!model_matches_visible(
            "jnoccio/jnoccio-fusion",
            "other-model"
        ));
    }

    #[test]
    fn matches_visible_model_accepts_router_alias() {
        assert!(model_matches_visible(
            "jnoccio/jnoccio-fusion",
            "jnoccio/jnoccio-router"
        ));
        assert!(model_matches_visible(
            "jnoccio/jnoccio-fusion",
            "jnoccio-router"
        ));
    }

    #[test]
    fn availability_counts_cover_missing_key_and_incomplete_env() {
        let counts = availability_counts(
            &[
                test_runtime_model("missing", false, "https://example.com", vec![], true, None),
                test_runtime_model("incomplete", true, "", vec!["base_url"], true, None),
            ],
            0,
        );

        assert_eq!(counts.missing_key, 1);
        assert_eq!(counts.incomplete_env, 1);
        assert_eq!(counts.disabled, 0);
        assert_eq!(counts.cooldown, 0);
        assert_eq!(counts.ready, 0);
    }

    fn structured_test_response(content: &str, finish_reason: &str) -> ChatCompletionResponse {
        crate::openai::build_response(
            "provider/model",
            crate::openai::ChatChoiceMessage {
                role: "assistant".to_string(),
                content: Some(content.to_string()),
                tool_calls: None,
                reasoning_text: None,
                reasoning_content: None,
                reasoning_opaque: None,
                extra: Map::new(),
            },
            Some(finish_reason.to_string()),
            None,
            None,
        )
    }

    fn structured_test_schema() -> Value {
        json!({
            "type": "object",
            "required": ["answer", "confidence"],
            "additionalProperties": false,
            "properties": {
                "answer": {"type": "string", "minLength": 1},
                "confidence": {"type": "integer", "minimum": 0, "maximum": 100}
            }
        })
    }

    #[test]
    fn structured_output_exact_json_is_canonicalized() {
        let mut response =
            structured_test_response("{\n  \"confidence\": 87,\n  \"answer\": \"yes\"\n}", "stop");

        let normalized_hash =
            normalize_structured_response(&mut response, &structured_test_schema()).unwrap();

        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("{\"answer\":\"yes\",\"confidence\":87}")
        );
        assert_eq!(
            normalized_hash,
            hash_str("{\"answer\":\"yes\",\"confidence\":87}")
        );
    }

    #[test]
    fn structured_output_rejects_prose_and_multiple_json_values() {
        let mut prose = structured_test_response(
            "Here is the JSON: {\"answer\":\"yes\",\"confidence\":87}",
            "stop",
        );
        assert!(normalize_structured_response(&mut prose, &structured_test_schema()).is_err());

        let mut multiple = structured_test_response(
            "{\"answer\":\"yes\",\"confidence\":87}{\"answer\":\"no\",\"confidence\":1}",
            "stop",
        );
        assert!(normalize_structured_response(&mut multiple, &structured_test_schema()).is_err());
    }

    #[test]
    fn structured_output_rejects_schema_mismatch_and_truncation() {
        let mut mismatch = structured_test_response("{\"answer\":\"yes\"}", "stop");
        assert!(normalize_structured_response(&mut mismatch, &structured_test_schema()).is_err());

        let mut truncated =
            structured_test_response("{\"answer\":\"yes\",\"confidence\":87}", "length");
        assert!(normalize_structured_response(&mut truncated, &structured_test_schema()).is_err());
    }
}
