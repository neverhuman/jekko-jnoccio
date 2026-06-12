use crate::config::ModelLimits;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct CapacityModel {
    pub id: String,
    pub provider: String,
    pub display_name: String,
    pub status: String,
    pub limits: ModelLimits,
}

#[derive(Clone, Debug, Default)]
pub struct CapacityUsage {
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

#[derive(Clone, Debug, Serialize)]
pub struct CapacitySummary {
    pub known_limit_per_hour: u64,
    pub known_used: u64,
    pub known_remaining: u64,
    pub percent_used: f64,
    pub models: Vec<ModelCapacitySummary>,
    pub unknown_models: Vec<UnknownCapacitySummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelCapacitySummary {
    pub model_id: String,
    pub provider: String,
    pub display_name: String,
    pub status: String,
    pub limit_per_hour: u64,
    pub used: u64,
    pub remaining: u64,
    pub percent_used: f64,
    pub limit_kind: String,
    pub credit_tier: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnknownCapacitySummary {
    pub model_id: String,
    pub provider: String,
    pub display_name: String,
    pub status: String,
    pub used: u64,
    pub successes: u64,
    pub failures: u64,
    pub wins: u64,
    pub average_latency_ms: Option<f64>,
}

pub fn capacity_summary(
    models: &[CapacityModel],
    usage: &HashMap<String, CapacityUsage>,
) -> CapacitySummary {
    let (known, unknown) = split_known_unknown(models, usage);
    summary_from_split(known, unknown)
}

pub fn split_known_unknown(
    models: &[CapacityModel],
    usage: &HashMap<String, CapacityUsage>,
) -> (Vec<ModelCapacitySummary>, Vec<UnknownCapacitySummary>) {
    let known = models
        .iter()
        .filter_map(|model| {
            hourly_capacity(&model.limits).map(|limit| {
                let usage = match usage.get(&model.id) {
                    Some(usage) => usage.clone(),
                    None => CapacityUsage::default(),
                };
                ModelCapacitySummary {
                    model_id: model.id.clone(),
                    provider: model.provider.clone(),
                    display_name: model.display_name.clone(),
                    status: model.status.clone(),
                    limit_per_hour: limit.capacity,
                    used: usage.attempts,
                    remaining: limit.capacity.saturating_sub(usage.attempts),
                    percent_used: ratio(usage.attempts, limit.capacity),
                    limit_kind: limit.kind,
                    credit_tier: limit.credit_tier,
                }
            })
        })
        .collect::<Vec<_>>();
    let unknown = models
        .iter()
        .filter(|model| hourly_capacity(&model.limits).is_none())
        .map(|model| {
            let usage = match usage.get(&model.id) {
                Some(usage) => usage.clone(),
                None => CapacityUsage::default(),
            };
            UnknownCapacitySummary {
                model_id: model.id.clone(),
                provider: model.provider.clone(),
                display_name: model.display_name.clone(),
                status: model.status.clone(),
                used: usage.attempts,
                successes: usage.successes,
                failures: usage.failures,
                wins: usage.wins,
                average_latency_ms: if usage.latency_count == 0 {
                    None
                } else {
                    Some(usage.latency_total_ms as f64 / usage.latency_count as f64)
                },
            }
        })
        .collect::<Vec<_>>();
    (known, unknown)
}

pub fn summary_from_split(
    models: Vec<ModelCapacitySummary>,
    unknown_models: Vec<UnknownCapacitySummary>,
) -> CapacitySummary {
    let known_limit_per_hour = models.iter().map(|model| model.limit_per_hour).sum();
    let known_used = models.iter().map(|model| model.used).sum();
    CapacitySummary {
        known_limit_per_hour,
        known_used,
        known_remaining: known_limit_per_hour.saturating_sub(known_used),
        percent_used: ratio(known_used, known_limit_per_hour),
        models,
        unknown_models,
    }
}

pub fn hourly_capacity(limits: &ModelLimits) -> Option<HourlyCapacity> {
    let daily = limits.rpd.or(limits.rpd_after_10_usd_credits);
    let credit_tier = limits.rpd.is_none() && limits.rpd_after_10_usd_credits.is_some();
    match (limits.rpm, daily) {
        (Some(rpm), Some(rpd)) => Some(HourlyCapacity {
            capacity: (rpm * 60).min((rpd / 24).max(1)),
            kind: if credit_tier {
                "rpm_and_credit_rpd".to_string()
            } else {
                "rpm_and_rpd".to_string()
            },
            credit_tier,
        }),
        (Some(rpm), None) => Some(HourlyCapacity {
            capacity: rpm * 60,
            kind: "rpm".to_string(),
            credit_tier: false,
        }),
        (None, Some(rpd)) => Some(HourlyCapacity {
            capacity: (rpd / 24).max(1),
            kind: if credit_tier { "credit_rpd" } else { "rpd" }.to_string(),
            credit_tier,
        }),
        (None, None) => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HourlyCapacity {
    pub capacity: u64,
    pub kind: String,
    pub credit_tier: bool,
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_math_handles_limit_shapes() {
        assert_eq!(
            hourly_capacity(&limits(Some(20), None, None))
                .unwrap()
                .capacity,
            1200
        );
        assert_eq!(
            hourly_capacity(&limits(None, Some(240), None))
                .unwrap()
                .capacity,
            10
        );
        assert_eq!(
            hourly_capacity(&limits(Some(20), Some(240), None))
                .unwrap()
                .capacity,
            10
        );
        let credit = hourly_capacity(&limits(None, None, Some(240))).unwrap();
        assert_eq!(credit.capacity, 10);
        assert!(credit.credit_tier);
        assert!(hourly_capacity(&limits(None, None, None)).is_none());
    }

    #[test]
    fn known_capacity_excludes_unknown_denominator() {
        let models = vec![
            CapacityModel {
                id: "known".to_string(),
                provider: "a".to_string(),
                display_name: "Known".to_string(),
                status: "ready".to_string(),
                limits: limits(Some(1), None, None),
            },
            CapacityModel {
                id: "unknown".to_string(),
                provider: "b".to_string(),
                display_name: "Unknown".to_string(),
                status: "ready".to_string(),
                limits: limits(None, None, None),
            },
        ];
        let mut usage = HashMap::new();
        usage.insert(
            "known".to_string(),
            CapacityUsage {
                attempts: 70,
                ..Default::default()
            },
        );
        usage.insert(
            "unknown".to_string(),
            CapacityUsage {
                attempts: 9,
                ..Default::default()
            },
        );
        let (known, unknown) = split_known_unknown(&models, &usage);
        let summary = summary_from_split(known, unknown);
        assert_eq!(summary.known_limit_per_hour, 60);
        assert_eq!(summary.known_used, 70);
        assert_eq!(summary.known_remaining, 0);
        assert_eq!(summary.unknown_models[0].used, 9);
    }

    fn limits(rpm: Option<u64>, rpd: Option<u64>, credit: Option<u64>) -> ModelLimits {
        ModelLimits {
            rpm,
            rpd,
            rpd_after_10_usd_credits: credit,
            source_url: None,
        }
    }
}
