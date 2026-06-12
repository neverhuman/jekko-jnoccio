use crate::config::{ModelLimits, ModelRouting, ModelScore};
use crate::openai::ChatCompletionRequest;
use crate::quality_band::{
    compute_percentiles, passes_band, ModelWinRate, QualityBand, DEFAULT_MIN_CALLS_FOR_RANKING,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityTier {
    Light,
    Standard,
    Heavy,
}

impl ComplexityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Standard => "standard",
            Self::Heavy => "heavy",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    Fast,
    Backup,
    FusionSample,
}

impl RouteMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Backup => "backup",
            Self::FusionSample => "fusion_sample",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RoutingConfig {
    pub fusion_sample_rate: f64,
    pub fast_backup_count: usize,
    /// Stricter deterministic-routing profile. Carried through plan_route as
    /// a hint; reserved for the proof-profile draft expansion (see
    /// `AppConfig.upstream_key_source.users_only()`).
    pub proof_profile: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestProfile {
    pub approx_prompt_tokens: u64,
    pub message_count: u64,
    pub code_block_count: u64,
    pub path_ref_count: u64,
    pub has_json_or_schema: bool,
    pub has_tools: bool,
    pub has_diff_or_error_log: bool,
    pub imperative_score: u64,
    pub requested_output_tokens: Option<u64>,
    pub complexity_score: u64,
    pub complexity_tier: ComplexityTier,
    /// Optional ZYAL-declared model-quality band. Defaults to `Any`
    /// (no filter). Read from `request.extra["quality_band"]`.
    pub quality_band: QualityBand,
}

impl RequestProfile {
    pub fn from_request(request: &ChatCompletionRequest) -> Self {
        let text = request
            .messages
            .iter()
            .map(message_text)
            .collect::<Vec<_>>()
            .join("\n");
        let lower = text.to_lowercase();
        let approx_prompt_tokens = (text.chars().count() as u64 / 4).max(1);
        let code_block_count = lower.matches("```").count() as u64 / 2;
        let path_ref_count = lower
            .split_whitespace()
            .filter(|word| looks_like_path_ref(word))
            .count() as u64;
        let has_json_or_schema = request.response_format.is_some()
            || request.tools.is_some()
            || lower.contains("json")
            || lower.contains("schema")
            || lower.contains("\"type\"")
            || lower.contains("zod");
        let has_diff_or_error_log = lower.contains("diff --git")
            || lower.contains("@@")
            || lower.contains("stack trace")
            || lower.contains("traceback")
            || lower.contains("error:")
            || lower.contains("exception");
        let imperative_score = [
            "implement",
            "fix",
            "debug",
            "refactor",
            "migrate",
            "test",
            "write",
            "create",
            "update",
            "plan",
            "analyze",
        ]
        .iter()
        .filter(|verb| lower.contains(**verb))
        .count() as u64;
        let requested_output_tokens = request.max_completion_tokens.or(request.max_tokens);
        let complexity_score = approx_prompt_tokens / 450
            + request.messages.len() as u64
            + code_block_count * 5
            + path_ref_count.min(12)
            + if has_json_or_schema { 4 } else { 0 }
            + if request.tools.is_some() { 5 } else { 0 }
            + if has_diff_or_error_log { 7 } else { 0 }
            + imperative_score * 2
            + requested_output_tokens
                .map(|value| value / 1500)
                .unwrap_or(0);
        let complexity_tier = if complexity_score >= 18 || approx_prompt_tokens >= 6000 {
            ComplexityTier::Heavy
        } else if complexity_score >= 7 || approx_prompt_tokens >= 1200 {
            ComplexityTier::Standard
        } else {
            ComplexityTier::Light
        };
        // Honor ZYAL-declared model-quality band via `request.extra`.
        // Unknown / missing → Any (current behavior, backwards-compatible).
        let quality_band = request
            .extra
            .get("quality_band")
            .and_then(Value::as_str)
            .and_then(QualityBand::from_str)
            .unwrap_or_default();
        Self {
            approx_prompt_tokens,
            message_count: request.messages.len() as u64,
            code_block_count,
            path_ref_count,
            has_json_or_schema,
            has_tools: request.tools.is_some(),
            has_diff_or_error_log,
            imperative_score,
            requested_output_tokens,
            complexity_score,
            complexity_tier,
            quality_band,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RoutingModelInput {
    pub id: String,
    pub provider: String,
    /// User id this routing slot is bound to under `UsersPool`. `None` for
    /// single-pool ConfigEnv routing.
    pub user_id: Option<String>,
    /// Env var name the key was sourced from (e.g. `"OPENROUTER_API_KEY"`).
    /// `None` when the slot has no key yet.
    pub credential_env_name: Option<String>,
    /// Upstream model id sent to the provider. Distinct from `id`, which is
    /// the router-facing visible id.
    pub upstream_model_id: String,
    pub ready: bool,
    pub status: String,
    pub failure_count: u64,
    pub disabled_reason: Option<String>,
    pub cooldown_until: Option<i64>,
    pub roles: Vec<String>,
    pub routing: ModelRouting,
    pub score: ModelScore,
    pub limits: ModelLimits,
    pub context_window: u64,
    pub configured_context_window: u64,
    pub safe_context_window: u64,
    pub learned_context_window: Option<u64>,
    pub learned_request_token_limit: Option<u64>,
    pub learned_tpm_limit: Option<u64>,
    pub recent_context_overrun_count: u64,
    pub max_output_tokens: u64,
    pub last_latency_ms: Option<u64>,
    /// Lifetime fusion-sample win count for this model. Loaded from the
    /// persistent `model_metrics` table. Used by the `quality_band` filter
    /// to compute win-rate percentiles. Defaults to 0 (treated as unranked).
    pub win_count: u64,
    /// Lifetime call count for this model (success + failure). Loaded from
    /// `model_metrics.call_count`. Combined with `win_count` for win-rate.
    pub call_count: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RoutingUsage {
    pub one_hour_attempts: u64,
    pub provider_one_hour_attempts: u64,
}

#[derive(Clone, Debug)]
pub struct RoutePlan {
    pub mode: RouteMode,
    pub sampled: bool,
    pub complexity_score: u64,
    pub complexity_tier: ComplexityTier,
    pub primary_model_id: Option<String>,
    pub backup_model_ids: Vec<String>,
    pub draft_model_ids: Vec<String>,
    pub fusion_model_id: Option<String>,
}

pub fn required_total_tokens(model: &RoutingModelInput, profile: &RequestProfile) -> u64 {
    let output_tokens = match profile.requested_output_tokens {
        Some(t) => t,
        None => model.max_output_tokens.min(8_192),
    };
    profile.approx_prompt_tokens.saturating_add(output_tokens)
}

pub fn should_sample(fusion_sample_rate: f64, draw: f64) -> bool {
    draw < fusion_sample_rate.clamp(0.0, 1.0)
}

pub fn plan_route(
    models: &[RoutingModelInput],
    usage: &HashMap<String, RoutingUsage>,
    profile: &RequestProfile,
    config: &RoutingConfig,
    now: i64,
    sample_draw: f64,
) -> RoutePlan {
    if should_sample(config.fusion_sample_rate, sample_draw) {
        let draft_model_ids = select_without_replacement(
            models,
            usage,
            profile,
            now,
            2,
            Some("draft"),
            &HashSet::new(),
            true,
        );
        let excluded = draft_model_ids.iter().cloned().collect::<HashSet<_>>();
        let fusion_model_id = select_without_replacement(
            models,
            usage,
            profile,
            now,
            1,
            Some("fusion"),
            &excluded,
            false,
        )
        .into_iter()
        .next();
        return RoutePlan {
            mode: RouteMode::FusionSample,
            sampled: true,
            complexity_score: profile.complexity_score,
            complexity_tier: profile.complexity_tier.clone(),
            primary_model_id: draft_model_ids.first().cloned(),
            backup_model_ids: Vec::new(),
            draft_model_ids,
            fusion_model_id,
        };
    }

    let primary_model_id =
        select_without_replacement(models, usage, profile, now, 1, None, &HashSet::new(), false)
            .into_iter()
            .next();
    let excluded = primary_model_id.iter().cloned().collect::<HashSet<_>>();
    let backup_model_ids = select_without_replacement(
        models,
        usage,
        profile,
        now,
        config.fast_backup_count,
        None,
        &excluded,
        true,
    );
    RoutePlan {
        mode: RouteMode::Fast,
        sampled: false,
        complexity_score: profile.complexity_score,
        complexity_tier: profile.complexity_tier.clone(),
        primary_model_id,
        backup_model_ids,
        draft_model_ids: Vec::new(),
        fusion_model_id: None,
    }
}

pub fn roulette_weight(
    model: &RoutingModelInput,
    usage: &RoutingUsage,
    profile: &RequestProfile,
    now: i64,
) -> f64 {
    if !is_eligible(model, profile, now) {
        return 0.0;
    }
    let quality = quality_score(&model.score, &profile.complexity_tier);
    let health = health_score(&model.status);
    let complexity = complexity_affinity(model, profile);
    let capacity = capacity_headroom(&model.limits, usage.one_hour_attempts);
    let load = load_balance_factor(usage.one_hour_attempts, usage.provider_one_hour_attempts);
    let latency = latency_factor(model.last_latency_ms, &model.score);
    let context = context_fit_factor(model, profile);
    let failure = failure_penalty(model.failure_count, &model.status);
    let learned_rate = learned_rate_factor(model, profile);
    let overruns = context_overrun_factor(model.recent_context_overrun_count);
    (quality
        * health
        * complexity
        * capacity
        * load
        * latency
        * context
        * failure
        * learned_rate
        * overruns
        * model.routing.exploration_floor.max(0.03))
    .max(if health > 0.0 { 0.0001 } else { 0.0 })
}

pub fn is_eligible(model: &RoutingModelInput, profile: &RequestProfile, now: i64) -> bool {
    model.routing.enabled
        && model.ready
        && model.disabled_reason.is_none()
        && model
            .cooldown_until
            .map(|until| until <= now)
            .unwrap_or(true)
        && !matches!(
            model.status.as_str(),
            "missing_key"
                | "incomplete_env"
                | "auth_failed"
                | "customer_verification_required"
                | "no_access"
                | "model_unavailable"
        )
        && model.safe_context_window >= required_total_tokens(model, profile)
}

#[allow(clippy::too_many_arguments)]
fn select_without_replacement(
    models: &[RoutingModelInput],
    usage: &HashMap<String, RoutingUsage>,
    profile: &RequestProfile,
    now: i64,
    count: usize,
    role: Option<&str>,
    excluded: &HashSet<String>,
    prefer_distinct_provider: bool,
) -> Vec<String> {
    // Quality-band filter — applied AFTER eligibility but BEFORE scoring.
    // When the band is `Any` this is effectively a no-op. When the
    // band-filtered pool is empty, soft-fallback to the full eligible set
    // and continue (the band is a hint, not a hard gate).
    let eligible_for_ranking: Vec<&RoutingModelInput> = models
        .iter()
        .filter(|model| !excluded.contains(&model.id))
        .filter(|model| {
            role.map(|role| model.roles.iter().any(|item| item == role))
                .unwrap_or(true)
        })
        .filter(|model| is_eligible(model, profile, now))
        .collect();
    let percentiles = if profile.quality_band == QualityBand::Any {
        HashMap::new()
    } else {
        compute_percentiles(
            eligible_for_ranking.iter().map(|m| {
                (
                    m.id.as_str(),
                    ModelWinRate {
                        win_count: m.win_count,
                        call_count: m.call_count,
                    },
                )
            }),
            DEFAULT_MIN_CALLS_FOR_RANKING,
        )
    };
    let mut pool = models
        .iter()
        .filter(|model| !excluded.contains(&model.id))
        .filter(|model| {
            role.map(|role| model.roles.iter().any(|item| item == role))
                .unwrap_or(true)
        })
        .filter(|model| is_eligible(model, profile, now))
        .filter(|model| {
            if profile.quality_band == QualityBand::Any {
                return true;
            }
            let percentile = percentiles.get(&model.id).copied().flatten();
            passes_band(percentile, profile.quality_band)
        })
        .cloned()
        .collect::<Vec<_>>();
    // Soft-fallback: if the band filter emptied the candidate set, log
    // and fall back to the unfiltered eligible pool. The band is a hint;
    // refusing the request would be worse than serving a weaker model.
    if pool.is_empty() && profile.quality_band != QualityBand::Any {
        tracing::warn!(
            target: "jnoccio_fusion::quality_band",
            band = profile.quality_band.as_str(),
            "quality_band yielded zero candidates; falling back to full eligible set"
        );
        pool = eligible_for_ranking.into_iter().cloned().collect();
    }
    let mut selected = Vec::new();
    let mut selected_providers = HashSet::new();
    while selected.len() < count && !pool.is_empty() {
        let available = if prefer_distinct_provider {
            let distinct = pool
                .iter()
                .enumerate()
                .filter(|(_, model)| !selected_providers.contains(&model.provider))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if distinct.is_empty() {
                (0..pool.len()).collect::<Vec<_>>()
            } else {
                distinct
            }
        } else {
            (0..pool.len()).collect::<Vec<_>>()
        };
        let Some(relative_index) = pick_weighted_index(
            &available
                .iter()
                .map(|index| {
                    let model_usage: RoutingUsage =
                        usage.get(&pool[*index].id).cloned().unwrap_or_default();
                    roulette_weight(&pool[*index], &model_usage, profile, now)
                })
                .collect::<Vec<_>>(),
        ) else {
            break;
        };
        let picked = pool.remove(available[relative_index]);
        selected_providers.insert(picked.provider.clone());
        selected.push(picked.id);
    }
    selected
}

fn pick_weighted_index(weights: &[f64]) -> Option<usize> {
    let total = weights.iter().sum::<f64>();
    if total <= 0.0 {
        return None;
    }
    let mut draw = rand::rng().random_range(0.0..total);
    weights.iter().enumerate().find_map(|(index, weight)| {
        if draw < *weight {
            return Some(index);
        }
        draw -= *weight;
        None
    })
}

fn quality_score(score: &ModelScore, tier: &ComplexityTier) -> f64 {
    let base = match tier {
        ComplexityTier::Light => {
            score.latency as f64 * 0.34
                + score.free_quota as f64 * 0.24
                + score.reliability as f64 * 0.22
                + score.integration as f64 * 0.12
                + score.power as f64 * 0.08
        }
        ComplexityTier::Standard => {
            score.power as f64 * 0.26
                + score.reliability as f64 * 0.26
                + score.integration as f64 * 0.20
                + score.latency as f64 * 0.18
                + score.free_quota as f64 * 0.10
        }
        ComplexityTier::Heavy => {
            score.power as f64 * 0.38
                + score.reliability as f64 * 0.28
                + score.integration as f64 * 0.18
                + score.context_latency() * 0.08
                + score.free_quota as f64 * 0.08
        }
    };
    (base / 100.0).clamp(0.05, 1.5)
}

fn health_score(status: &str) -> f64 {
    match status {
        "ready" | "healthy" => 1.0,
        "rate_limited" => 0.25,
        "quota_exhausted" => 0.08,
        "timeout" | "invalid_response" => 0.65,
        "server_error" => 0.45,
        "unsupported_api" => 0.03,
        "context_overflow" => 0.15,
        "unhealthy" => 0.25,
        _ => 0.8,
    }
}

fn failure_penalty(failure_count: u64, status: &str) -> f64 {
    if failure_count == 0 {
        return 1.0;
    }
    let base = match status {
        "rate_limited" => 0.22,
        "quota_exhausted" => 0.05,
        "timeout" => 0.28,
        "server_error" => 0.18,
        "invalid_response" => 0.12,
        "context_overflow" => 0.10,
        "unhealthy" => 0.08,
        _ => 0.20,
    };
    let penalty = 1.0 / (1.0 + failure_count as f64 * 6.0);
    (base * penalty).clamp(0.01, 1.0)
}

fn complexity_affinity(model: &RoutingModelInput, profile: &RequestProfile) -> f64 {
    match profile.complexity_tier {
        ComplexityTier::Light => {
            let power_penalty = if model.score.power >= 90 { 0.58 } else { 1.0 };
            let context_penalty = if model.safe_context_window >= 100_000 {
                0.82
            } else {
                1.0
            };
            power_penalty * context_penalty * (1.0 + model.score.latency as f64 / 500.0)
        }
        ComplexityTier::Standard => 0.85 + model.score.reliability as f64 / 650.0,
        ComplexityTier::Heavy => {
            let context = if model.safe_context_window >= profile.approx_prompt_tokens * 3 {
                1.15
            } else {
                0.35
            };
            context * (0.75 + model.score.power as f64 / 250.0)
        }
    }
}

fn context_fit_factor(model: &RoutingModelInput, profile: &RequestProfile) -> f64 {
    let required = required_total_tokens(model, profile);
    let target = context_band(required);
    let actual = context_band(model.safe_context_window);
    let extra_bands = actual.saturating_sub(target) as f64;
    let small_prompt_penalty = if required < 32_000 { 0.38 } else { 0.16 };
    let headroom = model.safe_context_window as f64 / required.max(1) as f64;
    (1.2 / (1.0 + extra_bands * small_prompt_penalty) * headroom.min(2.0).sqrt()).clamp(0.15, 1.45)
}

fn learned_rate_factor(model: &RoutingModelInput, profile: &RequestProfile) -> f64 {
    let required = required_total_tokens(model, profile);
    let learned_cap = model
        .learned_request_token_limit
        .or(model.learned_tpm_limit);
    let Some(learned_cap) = learned_cap else {
        return 1.0;
    };
    if learned_cap == 0 {
        return 0.05;
    }
    let used = required as f64 / learned_cap as f64;
    if used >= 0.90 {
        return 0.18;
    }
    if used >= 0.75 {
        return 0.45;
    }
    1.0
}

fn context_overrun_factor(count: u64) -> f64 {
    (1.0 / (1.0 + count as f64 / 3.0)).clamp(0.18, 1.0)
}

fn context_band(tokens: u64) -> u8 {
    if tokens < 8_000 {
        return 0;
    }
    if tokens < 32_000 {
        return 1;
    }
    if tokens < 128_000 {
        return 2;
    }
    if tokens < 256_000 {
        return 3;
    }
    4
}

fn capacity_headroom(limits: &ModelLimits, one_hour_attempts: u64) -> f64 {
    let Some(limit) = hourly_capacity(limits) else {
        return 1.0;
    };
    if limit == 0 {
        return 0.05;
    }
    let remaining = limit.saturating_sub(one_hour_attempts) as f64 / limit as f64;
    (0.25 + remaining * 0.75).clamp(0.05, 1.0)
}

fn hourly_capacity(limits: &ModelLimits) -> Option<u64> {
    match (limits.rpm, limits.rpd.or(limits.rpd_after_10_usd_credits)) {
        (Some(rpm), Some(rpd)) => Some((rpm * 60).min((rpd / 24).max(1))),
        (Some(rpm), None) => Some(rpm * 60),
        (None, Some(rpd)) => Some((rpd / 24).max(1)),
        (None, None) => None,
    }
}

fn load_balance_factor(model_attempts: u64, provider_attempts: u64) -> f64 {
    let model = 1.0 / (1.0 + model_attempts as f64 / 20.0);
    let provider = 1.0 / (1.0 + provider_attempts as f64 / 80.0);
    (model * 0.75 + provider * 0.25).clamp(0.18, 1.35)
}

fn latency_factor(last_latency_ms: Option<u64>, score: &ModelScore) -> f64 {
    let observed = last_latency_ms
        .map(|ms| (1_500.0 / ms.max(100) as f64).clamp(0.25, 1.5))
        .unwrap_or(1.0);
    observed * (0.75 + score.latency as f64 / 400.0)
}

fn message_text(message: &Value) -> String {
    let Some(content) = message.get("content") else {
        return message.to_string();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => content.to_string(),
    }
}

fn looks_like_path_ref(word: &str) -> bool {
    let trimmed = word.trim_matches(|ch: char| {
        !ch.is_ascii_alphanumeric() && ch != '/' && ch != '.' && ch != '_' && ch != '-'
    });
    trimmed.contains('/')
        || trimmed.ends_with(".rs")
        || trimmed.ends_with(".ts")
        || trimmed.ends_with(".tsx")
        || trimmed.ends_with(".js")
        || trimmed.ends_with(".json")
        || trimmed.ends_with(".md")
}

trait ScoreExt {
    fn context_latency(&self) -> f64;
}

impl ScoreExt for ModelScore {
    fn context_latency(&self) -> f64 {
        self.latency as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn sample_draw_below_rate_samples() {
        assert!(should_sample(0.10, 0.099));
        assert!(!should_sample(0.10, 0.10));
    }

    #[test]
    fn classifies_simple_greeting_as_light() {
        let profile = RequestProfile::from_request(&request("hi", None, None));
        assert_eq!(profile.complexity_tier, ComplexityTier::Light);
    }

    #[test]
    fn classifies_code_debug_task_as_heavy() {
        let profile = RequestProfile::from_request(&request(
            "Implement a fix across src/a.rs and src/b.ts. ```rust\nfn main(){}\n``` error: stack trace traceback diff --git schema json tools",
            Some(serde_json::json!([{"type":"function"}])),
            Some(6000),
        ));
        assert_eq!(profile.complexity_tier, ComplexityTier::Heavy);
    }

    #[test]
    fn light_prompts_favor_latency_over_power() {
        let profile = RequestProfile::from_request(&request("hello", None, None));
        let light = model("fast", 45, 95, None, 0, "ready", "a");
        let heavy = model("heavy", 98, 50, None, 0, "ready", "b");
        assert!(
            roulette_weight(&light, &RoutingUsage::default(), &profile, 0)
                > roulette_weight(&heavy, &RoutingUsage::default(), &profile, 0)
        );
    }

    #[test]
    fn heavy_prompts_favor_power() {
        let profile = RequestProfile::from_request(&request(
            "Implement debug refactor migrate test src/a.rs src/b.rs src/c.rs ```rust\nx\n``` error: traceback schema json",
            Some(serde_json::json!([{"type":"function"}])),
            Some(6000),
        ));
        let light = model("fast", 45, 95, None, 0, "ready", "a");
        let heavy = model("heavy", 98, 50, None, 0, "ready", "b");
        assert!(
            roulette_weight(&heavy, &RoutingUsage::default(), &profile, 0)
                > roulette_weight(&light, &RoutingUsage::default(), &profile, 0)
        );
    }

    #[test]
    fn overused_model_loses_weight() {
        let profile = RequestProfile::from_request(&request("hello", None, None));
        let item = model("fast", 60, 80, Some(1200), 0, "ready", "a");
        assert!(
            roulette_weight(&item, &RoutingUsage::default(), &profile, 0)
                > roulette_weight(
                    &item,
                    &RoutingUsage {
                        one_hour_attempts: 1200,
                        provider_one_hour_attempts: 1200
                    },
                    &profile,
                    0
                )
        );
    }

    #[test]
    fn failure_count_drops_weight_sharply() {
        let profile = RequestProfile::from_request(&request("hello", None, None));
        let healthy = model("fast", 60, 80, None, 0, "ready", "a");
        let failed = RoutingModelInput {
            failure_count: 1,
            ..model("failed", 60, 80, None, 0, "unhealthy", "a")
        };
        assert!(
            roulette_weight(&healthy, &RoutingUsage::default(), &profile, 0)
                > roulette_weight(&failed, &RoutingUsage::default(), &profile, 0) * 8.0
        );
    }

    #[test]
    fn route_excludes_models_below_required_context() {
        let profile =
            RequestProfile::from_request(&request(&"x".repeat(40_000), None, Some(8_000)));
        let small = RoutingModelInput {
            safe_context_window: 12_000,
            ..model("small", 70, 70, None, 0, "ready", "a")
        };
        let large = RoutingModelInput {
            safe_context_window: 64_000,
            ..model("large", 70, 70, None, 0, "ready", "b")
        };
        let route = plan_route(
            &[small, large],
            &HashMap::new(),
            &profile,
            &RoutingConfig {
                fusion_sample_rate: 0.0,
                fast_backup_count: 0,
                proof_profile: false,
            },
            0,
            1.0,
        );
        assert_eq!(route.primary_model_id.as_deref(), Some("large"));
    }

    #[test]
    fn no_eligible_context_band_returns_no_model() {
        let profile =
            RequestProfile::from_request(&request(&"x".repeat(80_000), None, Some(8_000)));
        let small = RoutingModelInput {
            safe_context_window: 16_000,
            ..model("small", 70, 70, None, 0, "ready", "a")
        };
        let route = plan_route(
            &[small],
            &HashMap::new(),
            &profile,
            &RoutingConfig {
                fusion_sample_rate: 0.0,
                fast_backup_count: 0,
                proof_profile: false,
            },
            0,
            1.0,
        );
        assert!(route.primary_model_id.is_none());
    }

    #[test]
    fn light_prompts_prefer_smaller_safe_context_when_quality_is_close() {
        let profile = RequestProfile::from_request(&request("hello", None, None));
        let small = RoutingModelInput {
            safe_context_window: 16_000,
            context_window: 16_000,
            configured_context_window: 16_000,
            ..model("small", 70, 80, None, 0, "ready", "a")
        };
        let huge = RoutingModelInput {
            safe_context_window: 256_000,
            context_window: 256_000,
            configured_context_window: 256_000,
            ..model("huge", 70, 80, None, 0, "ready", "b")
        };
        assert!(
            roulette_weight(&small, &RoutingUsage::default(), &profile, 0)
                > roulette_weight(&huge, &RoutingUsage::default(), &profile, 0)
        );
    }

    #[test]
    fn backups_exclude_primary_and_prefer_provider_diversity() {
        let profile = RequestProfile::from_request(&request("hello", None, None));
        let models = vec![
            model("a/one", 60, 80, None, 0, "ready", "a"),
            model("a/two", 60, 80, None, 0, "ready", "a"),
            model("b/one", 60, 80, None, 0, "ready", "b"),
        ];
        let route = plan_route(
            &models,
            &HashMap::new(),
            &profile,
            &RoutingConfig {
                fusion_sample_rate: 0.0,
                fast_backup_count: 2,
                proof_profile: false,
            },
            0,
            1.0,
        );
        assert!(
            !route
                .backup_model_ids
                .contains(route.primary_model_id.as_ref().unwrap())
        );
        let primary_provider = route
            .primary_model_id
            .as_deref()
            .and_then(|id| id.split('/').next())
            .unwrap();
        assert!(route.backup_model_ids.iter().any(|id| {
            id.split('/')
                .next()
                .map(|provider| provider != primary_provider)
                .unwrap_or(false)
        }));
    }

    #[test]
    fn request_profile_reads_quality_band_from_extra() {
        let mut req = request("plan a release", None, None);
        req.extra
            .insert("quality_band".into(), serde_json::json!("top20"));
        let profile = RequestProfile::from_request(&req);
        assert_eq!(profile.quality_band, QualityBand::Top20);

        // Unknown values fall back to Any (backwards-compatible default).
        let mut req2 = request("plan a release", None, None);
        req2.extra
            .insert("quality_band".into(), serde_json::json!("nonsense"));
        assert_eq!(
            RequestProfile::from_request(&req2).quality_band,
            QualityBand::Any
        );

        // Absent → Any.
        assert_eq!(
            RequestProfile::from_request(&request("hi", None, None)).quality_band,
            QualityBand::Any
        );
    }

    #[test]
    fn quality_band_top20_filters_to_top_tier_only() {
        // 5 models, all eligible, all with ≥20 calls but very different win
        // rates. Top20 band must restrict selection to the top-tier model.
        let mut req = request("plan a release", None, None);
        req.extra
            .insert("quality_band".into(), serde_json::json!("top20"));
        let profile = RequestProfile::from_request(&req);
        let mk = |id: &str, win: u64| {
            let mut m = model(id, 60, 80, None, 0, "ready", id.split('/').next().unwrap());
            m.win_count = win;
            m.call_count = 30; // ranked
            m
        };
        let models = vec![
            mk("a/loser", 3),
            mk("b/mid_lo", 9),
            mk("c/mid_hi", 18),
            mk("d/almost", 24),
            mk("e/winner", 29),
        ];
        let usage = HashMap::new();
        let excluded = HashSet::new();
        let picks =
            select_without_replacement(&models, &usage, &profile, 0, 1, None, &excluded, false);
        assert_eq!(picks.len(), 1);
        // Only e/winner (percentile 1.0) is in Top20.
        assert_eq!(picks[0], "e/winner");
    }

    #[test]
    fn quality_band_soft_fallback_when_empty() {
        // Same models but request Top10. Set all win counts to 0 (unranked
        // due to call_count below floor) — Top10 admits unranked per
        // cold-start policy, so we should still get a pick.
        let mut req = request("plan a release", None, None);
        req.extra
            .insert("quality_band".into(), serde_json::json!("top10"));
        let profile = RequestProfile::from_request(&req);
        // call_count below DEFAULT_MIN_CALLS_FOR_RANKING (20) → unranked.
        let mut a = model("a/x", 60, 80, None, 0, "ready", "a");
        a.win_count = 0;
        a.call_count = 5;
        let models = vec![a];
        let usage = HashMap::new();
        let excluded = HashSet::new();
        let picks =
            select_without_replacement(&models, &usage, &profile, 0, 1, None, &excluded, false);
        // Cold-start unranked admitted in Top10 — pick is returned.
        assert_eq!(picks.len(), 1);
        assert_eq!(picks[0], "a/x");
    }

    fn request(text: &str, tools: Option<Value>, output: Option<u64>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "jnoccio/jnoccio-fusion".to_string(),
            messages: vec![serde_json::json!({"role":"user","content":text})],
            stream: Some(false),
            temperature: None,
            top_p: None,
            max_tokens: None,
            max_completion_tokens: output,
            tools,
            tool_choice: None,
            reasoning_effort: None,
            response_format: None,
            stream_options: None,
            extra: Map::new(),
        }
    }

    fn model(
        id: &str,
        power: u64,
        latency: u64,
        hourly_capacity: Option<u64>,
        cooldown_until: i64,
        status: &str,
        provider: &str,
    ) -> RoutingModelInput {
        RoutingModelInput {
            id: id.to_string(),
            provider: provider.to_string(),
            user_id: None,
            credential_env_name: None,
            upstream_model_id: id.to_string(),
            ready: true,
            status: status.to_string(),
            failure_count: 0,
            disabled_reason: None,
            cooldown_until: if cooldown_until == 0 {
                None
            } else {
                Some(cooldown_until)
            },
            roles: vec!["draft".to_string(), "fusion".to_string()],
            routing: ModelRouting {
                enabled: true,
                roles: vec!["draft".to_string(), "fusion".to_string()],
                exploration_floor: 1.0,
                cooldown_seconds: 1,
                disabled_reason: None,
            },
            score: ModelScore {
                power,
                free_quota: 70,
                reliability: 80,
                integration: 80,
                latency,
            },
            limits: ModelLimits {
                rpm: hourly_capacity.map(|value| value / 60),
                rpd: None,
                rpd_after_10_usd_credits: None,
                source_url: None,
            },
            context_window: 128_000,
            configured_context_window: 128_000,
            safe_context_window: 128_000,
            learned_context_window: None,
            learned_request_token_limit: None,
            learned_tpm_limit: None,
            recent_context_overrun_count: 0,
            max_output_tokens: 8_000,
            last_latency_ms: None,
            win_count: 0,
            call_count: 0,
        }
    }
}
