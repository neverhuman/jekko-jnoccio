use crate::capacity::CapacitySummary;
use crate::state::{
    AgentActivity, ContextHistogramBucket, MetricEvent, ModelLimitEstimate, ModelMetric,
    TokenRateEstimate,
};
use serde::Serialize;

#[derive(Clone, Debug, serde::Serialize)]
pub struct DashboardSnapshot {
    pub totals: DashboardTotals,
    pub token_rate: TokenRateEstimate,
    pub capacity: CapacitySummary,
    pub context: ContextDashboard,
    pub models: Vec<DashboardModel>,
    pub recent_events: Vec<MetricEvent>,
    pub agent_count: usize,
    pub max_agents: usize,
    pub active_agents: Vec<AgentActivity>,
    pub instance_count: usize,
    pub max_instances: usize,
    pub available_instance_slots: usize,
    pub instance_role: String,
    pub worker_threads: usize,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ContextDashboard {
    pub estimates: Vec<ModelLimitEstimate>,
    pub histogram: Vec<ContextHistogramBucket>,
    pub recent_events: Vec<crate::state::ModelContextEvent>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct DashboardTotals {
    pub total_models: u64,
    pub enabled_models: u64,
    pub calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub wins: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub average_latency_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DashboardModel {
    pub id: String,
    pub provider: String,
    pub display_name: String,
    pub upstream_model: String,
    pub roles: Vec<String>,
    pub enabled: bool,
    pub status: String,
    pub cooldown_until: Option<i64>,
    pub capacity_known: bool,
    pub hourly_capacity: Option<u64>,
    pub hourly_used: u64,
    pub configured_context_window: u64,
    pub safe_context_window: u64,
    pub learned_context_window: Option<u64>,
    pub learned_request_token_limit: Option<u64>,
    pub context_overrun_count: u64,
    pub smallest_overrun_requested_tokens: Option<u64>,
    pub call_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub win_count: u64,
    pub win_rate: f64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub avg_latency_ms: Option<f64>,
    pub last_latency_ms: Option<u64>,
    pub min_latency_ms: Option<u64>,
    pub max_latency_ms: Option<u64>,
    pub last_error_kind: Option<String>,
    pub last_error_message: Option<String>,
    pub updated_at: i64,
}

pub fn dashboard_totals(models: &[DashboardModel]) -> DashboardTotals {
    let latency = models
        .iter()
        .filter_map(|model| {
            model
                .avg_latency_ms
                .map(|value| (value, model.call_count.max(1)))
        })
        .fold((0.0, 0u64), |acc, item| {
            (acc.0 + item.0 * item.1 as f64, acc.1 + item.1)
        });
    DashboardTotals {
        total_models: models.len() as u64,
        enabled_models: models.iter().filter(|model| model.enabled).count() as u64,
        calls: models.iter().map(|model| model.call_count).sum(),
        successes: models.iter().map(|model| model.success_count).sum(),
        failures: models.iter().map(|model| model.failure_count).sum(),
        wins: models.iter().map(|model| model.win_count).sum(),
        prompt_tokens: models.iter().map(|model| model.prompt_tokens).sum(),
        completion_tokens: models.iter().map(|model| model.completion_tokens).sum(),
        total_tokens: models.iter().map(|model| model.total_tokens).sum(),
        average_latency_ms: if latency.1 == 0 {
            None
        } else {
            Some(latency.0 / latency.1 as f64)
        },
    }
}

pub fn metric_average(metric: &ModelMetric) -> Option<f64> {
    average(metric.latency_total_ms, metric.latency_count)
}

pub fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64
}

pub fn average(total: u64, count: u64) -> Option<f64> {
    if count == 0 {
        return None;
    }
    Some(total as f64 / count as f64)
}

/// Render a [`DashboardSnapshot`] as Prometheus 0.0.4 text format. Emits
/// per-model counters (requests/successes/failures/wins/tokens) and gauges
/// (avg/last latency, hourly_used). Wired to the `/metrics` route in
/// `router.rs` for operators that scrape Prometheus.
///
/// Label values are escaped per the Prometheus spec: backslash, double-quote,
/// and newline are backslash-escaped; nothing else needs special handling for
/// the model/provider id space we control.
pub fn render_prometheus(snapshot: &DashboardSnapshot) -> String {
    let mut out = String::with_capacity(2048);

    // Top-level totals (gateway-wide).
    push_help(
        &mut out,
        "fusion_models_total",
        "Total registered models",
        "gauge",
    );
    push_metric(
        &mut out,
        "fusion_models_total",
        &[],
        snapshot.totals.total_models as f64,
    );
    push_help(
        &mut out,
        "fusion_models_enabled",
        "Enabled (routable) models",
        "gauge",
    );
    push_metric(
        &mut out,
        "fusion_models_enabled",
        &[],
        snapshot.totals.enabled_models as f64,
    );
    push_help(
        &mut out,
        "fusion_requests_total",
        "Total upstream requests across all models",
        "counter",
    );
    push_metric(
        &mut out,
        "fusion_requests_total",
        &[],
        snapshot.totals.calls as f64,
    );
    push_help(
        &mut out,
        "fusion_success_total",
        "Total successful upstream requests",
        "counter",
    );
    push_metric(
        &mut out,
        "fusion_success_total",
        &[],
        snapshot.totals.successes as f64,
    );
    push_help(
        &mut out,
        "fusion_failure_total",
        "Total failed upstream requests",
        "counter",
    );
    push_metric(
        &mut out,
        "fusion_failure_total",
        &[],
        snapshot.totals.failures as f64,
    );
    push_help(
        &mut out,
        "fusion_prompt_tokens_total",
        "Total prompt tokens billed",
        "counter",
    );
    push_metric(
        &mut out,
        "fusion_prompt_tokens_total",
        &[],
        snapshot.totals.prompt_tokens as f64,
    );
    push_help(
        &mut out,
        "fusion_completion_tokens_total",
        "Total completion tokens billed",
        "counter",
    );
    push_metric(
        &mut out,
        "fusion_completion_tokens_total",
        &[],
        snapshot.totals.completion_tokens as f64,
    );

    if let Some(avg) = snapshot.totals.average_latency_ms {
        push_help(
            &mut out,
            "fusion_latency_avg_ms",
            "Call-count-weighted average latency",
            "gauge",
        );
        push_metric(&mut out, "fusion_latency_avg_ms", &[], avg);
    }

    push_help(
        &mut out,
        "fusion_agents_active",
        "Active agent count (last heartbeat window)",
        "gauge",
    );
    push_metric(
        &mut out,
        "fusion_agents_active",
        &[],
        snapshot.agent_count as f64,
    );
    push_help(
        &mut out,
        "fusion_instances_total",
        "Managed fusion instances (main + spawned)",
        "gauge",
    );
    push_metric(
        &mut out,
        "fusion_instances_total",
        &[],
        snapshot.instance_count as f64,
    );

    // Per-model series. Counters carry both `model` and `provider` labels;
    // gauges carry only `model` so a multi-provider model id (rare) doesn't
    // produce divergent samples.
    push_help(
        &mut out,
        "fusion_model_requests_total",
        "Per-model upstream request count",
        "counter",
    );
    push_help(
        &mut out,
        "fusion_model_success_total",
        "Per-model successful upstream count",
        "counter",
    );
    push_help(
        &mut out,
        "fusion_model_failure_total",
        "Per-model failed upstream count",
        "counter",
    );
    push_help(
        &mut out,
        "fusion_model_win_total",
        "Per-model fusion-sample win count",
        "counter",
    );
    push_help(
        &mut out,
        "fusion_model_prompt_tokens_total",
        "Per-model prompt tokens billed",
        "counter",
    );
    push_help(
        &mut out,
        "fusion_model_completion_tokens_total",
        "Per-model completion tokens billed",
        "counter",
    );
    push_help(
        &mut out,
        "fusion_model_latency_avg_ms",
        "Per-model average latency",
        "gauge",
    );
    push_help(
        &mut out,
        "fusion_model_latency_last_ms",
        "Per-model latency of the most recent call",
        "gauge",
    );
    push_help(
        &mut out,
        "fusion_model_hourly_used",
        "Per-model rolling 1h request count",
        "gauge",
    );
    push_help(
        &mut out,
        "fusion_model_enabled",
        "Per-model enabled flag (1 = enabled)",
        "gauge",
    );

    for model in &snapshot.models {
        let labels = [
            ("model", model.id.as_str()),
            ("provider", model.provider.as_str()),
        ];
        let model_only = [("model", model.id.as_str())];
        push_metric(
            &mut out,
            "fusion_model_requests_total",
            &labels,
            model.call_count as f64,
        );
        push_metric(
            &mut out,
            "fusion_model_success_total",
            &labels,
            model.success_count as f64,
        );
        push_metric(
            &mut out,
            "fusion_model_failure_total",
            &labels,
            model.failure_count as f64,
        );
        push_metric(
            &mut out,
            "fusion_model_win_total",
            &labels,
            model.win_count as f64,
        );
        push_metric(
            &mut out,
            "fusion_model_prompt_tokens_total",
            &labels,
            model.prompt_tokens as f64,
        );
        push_metric(
            &mut out,
            "fusion_model_completion_tokens_total",
            &labels,
            model.completion_tokens as f64,
        );
        if let Some(avg) = model.avg_latency_ms {
            push_metric(&mut out, "fusion_model_latency_avg_ms", &model_only, avg);
        }
        if let Some(last) = model.last_latency_ms {
            push_metric(
                &mut out,
                "fusion_model_latency_last_ms",
                &model_only,
                last as f64,
            );
        }
        push_metric(
            &mut out,
            "fusion_model_hourly_used",
            &model_only,
            model.hourly_used as f64,
        );
        push_metric(
            &mut out,
            "fusion_model_enabled",
            &model_only,
            if model.enabled { 1.0 } else { 0.0 },
        );
    }

    out
}

fn push_help(out: &mut String, name: &str, help: &str, kind: &str) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push(' ');
    out.push_str(kind);
    out.push('\n');
}

fn push_metric(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (i, (k, v)) in labels.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(k);
            out.push_str("=\"");
            push_escaped_label(out, v);
            out.push('"');
        }
        out.push('}');
    }
    out.push(' ');
    // Render NaN / inf as 0 so scrapers don't choke; real values use Display.
    if value.is_finite() {
        out.push_str(&format!("{value}"));
    } else {
        out.push('0');
    }
    out.push('\n');
}

fn push_escaped_label(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod prometheus_tests {
    use super::*;

    fn empty_snapshot() -> DashboardSnapshot {
        DashboardSnapshot {
            totals: DashboardTotals::default(),
            token_rate: TokenRateEstimate::default(),
            capacity: CapacitySummary {
                known_limit_per_hour: 0,
                known_used: 0,
                known_remaining: 0,
                percent_used: 0.0,
                models: Vec::new(),
                unknown_models: Vec::new(),
            },
            context: ContextDashboard::default(),
            models: Vec::new(),
            recent_events: Vec::new(),
            agent_count: 0,
            max_agents: 0,
            active_agents: Vec::new(),
            instance_count: 1,
            max_instances: 20,
            available_instance_slots: 19,
            instance_role: "main".to_string(),
            worker_threads: 4,
        }
    }

    #[test]
    fn renders_help_and_type_lines() {
        let out = render_prometheus(&empty_snapshot());
        assert!(out.contains("# HELP fusion_requests_total"));
        assert!(out.contains("# TYPE fusion_requests_total counter"));
        assert!(out.contains("# HELP fusion_model_requests_total"));
        // fusion_latency_avg_ms is only emitted when average_latency_ms is
        // Some(_) — verified in renders_per_model_lines_with_labels.
    }

    #[test]
    fn renders_per_model_lines_with_labels() {
        let mut snap = empty_snapshot();
        snap.models.push(DashboardModel {
            id: "openrouter/gpt-4".to_string(),
            provider: "openrouter".to_string(),
            display_name: "GPT-4".to_string(),
            upstream_model: "gpt-4".to_string(),
            roles: vec!["fast".to_string()],
            enabled: true,
            status: "ready".to_string(),
            cooldown_until: None,
            capacity_known: true,
            hourly_capacity: Some(60),
            hourly_used: 12,
            configured_context_window: 128000,
            safe_context_window: 64000,
            learned_context_window: None,
            learned_request_token_limit: None,
            context_overrun_count: 0,
            smallest_overrun_requested_tokens: None,
            call_count: 42,
            success_count: 40,
            failure_count: 2,
            win_count: 8,
            win_rate: 0.19,
            prompt_tokens: 12345,
            completion_tokens: 6789,
            total_tokens: 19134,
            avg_latency_ms: Some(1234.5),
            last_latency_ms: Some(900),
            min_latency_ms: Some(400),
            max_latency_ms: Some(2200),
            last_error_kind: None,
            last_error_message: None,
            updated_at: 0,
        });
        let out = render_prometheus(&snap);
        assert!(out.contains(
            r#"fusion_model_requests_total{model="openrouter/gpt-4",provider="openrouter"} 42"#
        ));
        assert!(out.contains(r#"fusion_model_enabled{model="openrouter/gpt-4"} 1"#));
        assert!(out.contains(r#"fusion_model_latency_avg_ms{model="openrouter/gpt-4"} 1234.5"#));
    }

    #[test]
    fn label_quoting_escapes_special_chars() {
        let mut out = String::new();
        push_escaped_label(&mut out, r#"a"b\c"#);
        assert_eq!(out, r#"a\"b\\c"#);
    }

    #[test]
    fn non_finite_values_render_as_zero() {
        let mut out = String::new();
        push_metric(&mut out, "x", &[], f64::NAN);
        assert!(out.trim_end().ends_with(" 0"));
    }
}
