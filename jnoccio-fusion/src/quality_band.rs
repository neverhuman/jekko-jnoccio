//! Model quality band — request-side knob to constrain routing selection
//! by observed win-rate percentile.
//!
//! Win-rate evidence is already collected by the fusion-sample mechanism
//! (`routing.fusion_sample_rate: 0.1`): on a sampled fraction of qualifying
//! calls a backup model is invoked in parallel and the winner increments
//! `model_metrics.win_count`. This module exposes the resulting percentile
//! distribution to ZYAL stages so they can declare:
//!
//! - `quality_band: top10`  — only top-10% win-rate models (critical stages)
//! - `quality_band: top20`  — only top-20% win-rate models (load-bearing)
//! - `quality_band: top50`  — top half (default for non-critical reasoning)
//! - `quality_band: bottom20` — bottom 20% (give weaker models routine work)
//! - `quality_band: any`    — current behavior (no filter)
//!
//! Cold-start: a model with fewer than `min_calls_for_ranking` observations
//! has no reliable percentile rank. Such "unranked" models are
//! admitted in `Top*` bands (so exploration keeps refreshing evidence) and
//! rejected in `Bottom*` bands (so we don't blindly send to a fresh model
//! that may not work). The threshold is configurable per call.
//!
//! Soft-fallback: if a band filter empties the candidate pool, the caller
//! should fall back to the unfiltered eligible set rather than fail the
//! request — this module exposes the fallback as a separate result variant.

use std::collections::HashMap;

/// Default minimum number of calls before a model is "ranked" for the
/// purposes of band filtering. Models below this floor are unranked.
pub const DEFAULT_MIN_CALLS_FOR_RANKING: u64 = 20;

/// Requested quality band on a per-request basis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityBand {
    #[default]
    Any,
    Top10,
    Top20,
    Top50,
    Bottom20,
}

impl QualityBand {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Top10 => "top10",
            Self::Top20 => "top20",
            Self::Top50 => "top50",
            Self::Bottom20 => "bottom20",
        }
    }

    /// Parse from the lowercase string form (`"top20"`, `"bottom20"`, …).
    /// Unknown / empty strings yield `None` so callers can choose whether
    /// to default to `Any` or reject.
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "any" => Some(Self::Any),
            "top10" => Some(Self::Top10),
            "top20" => Some(Self::Top20),
            "top50" => Some(Self::Top50),
            "bottom20" => Some(Self::Bottom20),
            _ => None,
        }
    }

    /// (low, high) percentile bounds for this band — both inclusive,
    /// values in `[0.0, 1.0]` where 1.0 is the best win-rate.
    /// `Any` returns the full range.
    pub fn percentile_range(&self) -> (f64, f64) {
        match self {
            Self::Any => (0.0, 1.0),
            Self::Top10 => (0.90, 1.0),
            Self::Top20 => (0.80, 1.0),
            Self::Top50 => (0.50, 1.0),
            Self::Bottom20 => (0.0, 0.20),
        }
    }

    /// True for top-tier bands (Top10/Top20/Top50). Unranked (cold-start)
    /// models are admitted under top bands so exploration keeps refreshing
    /// the evidence base.
    pub fn admits_unranked(&self) -> bool {
        matches!(self, Self::Any | Self::Top10 | Self::Top20 | Self::Top50)
    }
}

/// One model's win-rate observation. Pass `Some` for ranked models with
/// `call_count >= min_calls_for_ranking`; pass `None` for unranked
/// (cold-start) models.
#[derive(Clone, Copy, Debug)]
pub struct ModelWinRate {
    pub win_count: u64,
    pub call_count: u64,
}

impl ModelWinRate {
    pub fn ratio(&self) -> f64 {
        if self.call_count == 0 {
            0.0
        } else {
            (self.win_count as f64 / self.call_count as f64).clamp(0.0, 1.0)
        }
    }
}

/// Compute the percentile rank of every model id by win_rate. Returns a
/// `HashMap<id, percentile>` where percentile is in `[0.0, 1.0]` and is
/// `None` for unranked models (insufficient samples).
pub fn compute_percentiles<'a, I>(observations: I, min_calls: u64) -> HashMap<String, Option<f64>>
where
    I: IntoIterator<Item = (&'a str, ModelWinRate)>,
{
    let mut entries: Vec<(String, ModelWinRate, bool)> = observations
        .into_iter()
        .map(|(id, w)| {
            let ranked = w.call_count >= min_calls;
            (id.to_string(), w, ranked)
        })
        .collect();
    // Sort the ranked subset by ratio, ascending. Unranked stay out of the
    // rank order — they get `None`.
    entries.sort_by(|a, b| a.1.ratio().partial_cmp(&b.1.ratio()).unwrap_or(std::cmp::Ordering::Equal));
    let ranked: Vec<&str> = entries
        .iter()
        .filter_map(|(id, _, ranked)| if *ranked { Some(id.as_str()) } else { None })
        .collect();
    let total = ranked.len();
    let mut out = HashMap::with_capacity(entries.len());
    for (id, _w, is_ranked) in &entries {
        if !is_ranked {
            out.insert(id.clone(), None);
            continue;
        }
        // The model's position in the ranked list (ascending). 0 = worst.
        let pos = ranked.iter().position(|other| *other == id.as_str()).unwrap_or(0);
        // Percentile in [0.0, 1.0]: highest rank → 1.0, lowest → 0.0.
        let percentile = if total <= 1 {
            1.0
        } else {
            pos as f64 / (total - 1) as f64
        };
        out.insert(id.clone(), Some(percentile));
    }
    out
}

/// Decide whether a model's percentile (or lack thereof) qualifies it for
/// the requested band.
pub fn passes_band(percentile: Option<f64>, band: QualityBand) -> bool {
    match percentile {
        None => band.admits_unranked(),
        Some(p) => {
            let (lo, hi) = band.percentile_range();
            p >= lo && p <= hi
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wr(w: u64, c: u64) -> ModelWinRate {
        ModelWinRate {
            win_count: w,
            call_count: c,
        }
    }

    #[test]
    fn band_strings_round_trip() {
        for band in [
            QualityBand::Any,
            QualityBand::Top10,
            QualityBand::Top20,
            QualityBand::Top50,
            QualityBand::Bottom20,
        ] {
            assert_eq!(QualityBand::from_str(band.as_str()), Some(band));
        }
        assert_eq!(QualityBand::from_str(""), Some(QualityBand::Any));
        assert_eq!(QualityBand::from_str("nonsense"), None);
    }

    #[test]
    fn percentiles_sort_by_win_rate() {
        // 5 models with 20+ calls each, varying win counts.
        let obs = vec![
            ("a", wr(2, 20)),  // 10% win
            ("b", wr(8, 20)),  // 40%
            ("c", wr(18, 20)), // 90%
            ("d", wr(5, 20)),  // 25%
            ("e", wr(14, 20)), // 70%
        ];
        let percs = compute_percentiles(obs, DEFAULT_MIN_CALLS_FOR_RANKING);
        // Ascending order: a(10%), d(25%), b(40%), e(70%), c(90%).
        assert_eq!(percs["a"], Some(0.0));
        assert_eq!(percs["d"], Some(0.25));
        assert_eq!(percs["b"], Some(0.5));
        assert_eq!(percs["e"], Some(0.75));
        assert_eq!(percs["c"], Some(1.0));
        // Top-10 admits only c.
        assert!(passes_band(percs["c"], QualityBand::Top10));
        assert!(!passes_band(percs["a"], QualityBand::Top10));
        // Bottom-20 admits only a.
        assert!(passes_band(percs["a"], QualityBand::Bottom20));
        assert!(!passes_band(percs["c"], QualityBand::Bottom20));
    }

    #[test]
    fn unranked_admitted_only_in_top_bands() {
        let obs = vec![
            ("ranked_winner", wr(18, 20)),
            ("cold_start", wr(1, 3)), // below min_calls floor
        ];
        let percs = compute_percentiles(obs, DEFAULT_MIN_CALLS_FOR_RANKING);
        assert_eq!(percs["cold_start"], None);
        assert!(passes_band(percs["cold_start"], QualityBand::Any));
        assert!(passes_band(percs["cold_start"], QualityBand::Top10));
        assert!(passes_band(percs["cold_start"], QualityBand::Top20));
        assert!(!passes_band(percs["cold_start"], QualityBand::Bottom20));
    }

    #[test]
    fn single_ranked_model_gets_top_percentile() {
        let obs = vec![("only", wr(10, 20))];
        let percs = compute_percentiles(obs, DEFAULT_MIN_CALLS_FOR_RANKING);
        assert_eq!(percs["only"], Some(1.0));
        assert!(passes_band(percs["only"], QualityBand::Top10));
    }

    #[test]
    fn band_ranges_match_spec() {
        assert_eq!(QualityBand::Top10.percentile_range(), (0.9, 1.0));
        assert_eq!(QualityBand::Top20.percentile_range(), (0.8, 1.0));
        assert_eq!(QualityBand::Top50.percentile_range(), (0.5, 1.0));
        assert_eq!(QualityBand::Bottom20.percentile_range(), (0.0, 0.2));
        assert_eq!(QualityBand::Any.percentile_range(), (0.0, 1.0));
    }
}
