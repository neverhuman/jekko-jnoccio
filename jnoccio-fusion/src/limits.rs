use axum::http::{HeaderMap, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    AuthFailed,
    RateLimited,
    Timeout,
    ServerError,
    InvalidResponse,
    ContextOverflow,
    CustomerVerificationRequired,
    NoAccess,
    UnsupportedApi,
    ModelUnavailable,
    QuotaExhausted,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ParsedLimitSignal {
    pub learned_context_window: Option<u64>,
    pub learned_request_token_limit: Option<u64>,
    pub learned_tpm_limit: Option<u64>,
    pub requested_tokens: Option<u64>,
    pub prompt_tokens: Option<u64>,
    pub tool_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub message_tokens: Option<u64>,
    pub kind: String,
}

pub fn classify_status(status: StatusCode, body: &str) -> ErrorKind {
    match status.as_u16() {
        413 => ErrorKind::ContextOverflow,
        _ if is_context_overflow(body) => ErrorKind::ContextOverflow,
        401 => ErrorKind::AuthFailed,
        402 => ErrorKind::CustomerVerificationRequired,
        403 if body_contains(body, &["card", "billing", "verify", "payment"]) => {
            ErrorKind::CustomerVerificationRequired
        }
        403 if body_contains(
            body,
            &[
                "access denied",
                "no access",
                "not authorized",
                "service tier",
            ],
        ) =>
        {
            ErrorKind::NoAccess
        }
        403 => ErrorKind::AuthFailed,
        408 => ErrorKind::Timeout,
        404 if body_contains(
            body,
            &[
                "model not found",
                "model unavailable",
                "does not exist",
                "discontinued",
                "unknown model",
                "unknown_model",
            ],
        ) =>
        {
            ErrorKind::ModelUnavailable
        }
        404 if body_contains(body, &["unsupported", "not supported", "chat/completions"]) => {
            ErrorKind::UnsupportedApi
        }
        429 if body_contains(
            body,
            &[
                "daily quota",
                "free tier",
                "quota exceeded",
                "billing details",
                "payment method",
            ],
        ) =>
        {
            ErrorKind::QuotaExhausted
        }
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::ServerError,
        _ if is_payload_incompatibility(body) => ErrorKind::UnsupportedApi,
        _ if body_contains(body, &["unsupported", "not supported", "unsupported api"]) => {
            ErrorKind::UnsupportedApi
        }
        _ if body_contains(
            body,
            &[
                "model not found",
                "model unavailable",
                "does not exist",
                "discontinued",
                "unknown model",
                "unknown_model",
            ],
        ) =>
        {
            ErrorKind::ModelUnavailable
        }
        _ if body_contains(
            body,
            &["quota exceeded", "free tier", "daily quota", "billing"],
        ) =>
        {
            ErrorKind::QuotaExhausted
        }
        _ => ErrorKind::Unknown,
    }
}

pub fn is_context_overflow(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("context length")
        || body.contains("maximum context")
        || body.contains("context window limit")
        || body.contains("prompt is too long")
        || body.contains("token limit")
        || body.contains("tokens_limit_reached")
        || body.contains("request too large")
        || body.contains("max size:")
        || body.contains("messages resulted in")
        || body.contains("max_tokens must be less than")
        || body.contains("max tokens must be less than")
        || body.contains("max_completion_tokens must be less than")
        || parse_limit_signal(&body).is_some()
}

pub fn is_payload_incompatibility(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    (body.contains("reasoning_content")
        || body.contains("reasoning_text")
        || body.contains("reasoning_opaque"))
        && (body.contains("unsupported")
            || body.contains("unknown")
            || body.contains("invalid")
            || body.contains("not permitted")
            || body.contains("extra"))
        || body.contains("unsupported message field")
        || body.contains("unsupported field")
        || body.contains("unknown field")
        || body.contains("extra fields not permitted")
        || body.contains("unrecognized request argument")
        || body.contains("stream_options can only be set when stream is true")
        || body.contains("stream_options is only allowed when stream is true")
        || (body.contains("stream_options") && body.contains("stream") && body.contains("false"))
        || (body.contains("tool_choice") && body.contains("tools") && body.contains("without"))
        || (body.contains("tool_choice") && body.contains("no tools"))
}

pub fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after_value)
}

pub fn parse_reset_headers(headers: &HeaderMap) -> Option<Duration> {
    headers.iter().find_map(|(name, value)| {
        let key = name.as_str().to_ascii_lowercase();
        if !key.starts_with("x-ratelimit-reset") {
            return None;
        }
        value.to_str().ok().and_then(parse_reset_value)
    })
}

pub fn cooldown_for(kind: &ErrorKind, retry_after: Option<Duration>, failures: u64) -> Duration {
    if let Some(retry_after) = retry_after {
        return retry_after;
    }
    match kind {
        ErrorKind::AuthFailed
        | ErrorKind::CustomerVerificationRequired
        | ErrorKind::NoAccess
        | ErrorKind::UnsupportedApi
        | ErrorKind::ModelUnavailable => Duration::from_secs(24 * 60 * 60),
        ErrorKind::QuotaExhausted => Duration::from_secs(24 * 60 * 60),
        ErrorKind::RateLimited => {
            Duration::from_secs((15 * 2u64.saturating_pow(failures.min(5) as u32)).min(900))
        }
        ErrorKind::Timeout => {
            Duration::from_secs((10 * 2u64.saturating_pow(failures.min(4) as u32)).min(300))
        }
        ErrorKind::ServerError => {
            Duration::from_secs((10 * 2u64.saturating_pow(failures.min(5) as u32)).min(600))
        }
        ErrorKind::InvalidResponse => Duration::from_secs(60),
        ErrorKind::ContextOverflow => Duration::from_secs(0),
        ErrorKind::Unknown => {
            Duration::from_secs((5 * 2u64.saturating_pow(failures.min(5) as u32)).min(300))
        }
    }
}

pub fn retry_after_from_body(body: &str) -> Option<Duration> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    find_retry_info(&value).and_then(|delay| parse_retry_after_value(&delay))
}

pub fn parse_limit_signal(body: &str) -> Option<ParsedLimitSignal> {
    let lower = body.to_ascii_lowercase();
    let mut signal = ParsedLimitSignal::default();

    if let Some(limit) = number_after(&lower, "maximum context length is") {
        signal.learned_context_window = Some(limit);
        signal.kind = "context_window".to_string();
    }
    if let Some(limit) = number_between(&lower, "context window limit (", ")") {
        signal.learned_context_window = Some(limit);
        signal.kind = "context_window".to_string();
    }
    if let Some(limit) = number_after(&lower, "max size:") {
        signal.learned_request_token_limit = Some(limit);
        signal.kind = "request_token_limit".to_string();
    }
    if let Some(limit) = number_after(&lower, "limit ")
        && (lower.contains("request too large") || lower.contains("requested "))
    {
        signal.learned_request_token_limit = Some(limit);
        signal.kind = "request_token_limit".to_string();
        if lower.contains("tpm")
            || lower.contains("tokens per minute")
            || lower.contains("rate limit")
        {
            signal.learned_tpm_limit = Some(limit);
        }
    }
    if lower.contains("tokens_limit_reached") && signal.kind.is_empty() {
        signal.kind = "request_token_limit".to_string();
    }
    if let Some(limit) = number_after(&lower, "max_tokens must be less than or equal to") {
        signal.learned_request_token_limit = Some(limit);
        signal.output_tokens = Some(limit);
        signal.kind = "request_token_limit".to_string();
    }
    if let Some(limit) = number_after(&lower, "max_tokens must be less than") {
        signal.learned_request_token_limit = Some(limit);
        signal.output_tokens = Some(limit);
        signal.kind = "request_token_limit".to_string();
    }
    if let Some(limit) = number_after(&lower, "max_completion_tokens must be less than") {
        signal.learned_request_token_limit = Some(limit);
        signal.output_tokens = Some(limit);
        signal.kind = "request_token_limit".to_string();
    }

    signal.requested_tokens = if let Some(value) = number_after(&lower, "requested about") {
        Some(value)
    } else if let Some(value) = number_after(&lower, "requested ") {
        Some(value)
    } else {
        number_after(&lower, "requested:")
    };
    signal.message_tokens =
        if let Some(value) = number_between(&lower, "messages resulted in", "tokens") {
            Some(value)
        } else {
            number_after(&lower, "messages resulted in")
        };
    signal.prompt_tokens = number_before(&lower, "of text input");
    signal.tool_tokens = number_before(&lower, "of tool input");
    signal.output_tokens = if let Some(value) = signal.output_tokens {
        Some(value)
    } else {
        number_before(&lower, "in the output")
    };

    if signal.kind.is_empty() && has_any_limit_number(&signal) {
        signal.kind = "context_overflow".to_string();
    }
    if has_any_limit_number(&signal) {
        return Some(signal);
    }
    None
}

fn parse_retry_after_value(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    if let Some(value) = trimmed.strip_suffix('s') {
        return value.parse::<u64>().ok().map(Duration::from_secs);
    }
    if let Some(value) = trimmed.strip_suffix('m') {
        return value
            .parse::<u64>()
            .ok()
            .map(|minutes| Duration::from_secs(minutes * 60));
    }
    if let Some(value) = trimmed.strip_suffix('h') {
        return value
            .parse::<u64>()
            .ok()
            .map(|hours| Duration::from_secs(hours * 60 * 60));
    }
    None
}

fn parse_reset_value(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(seconds) = trimmed.parse::<u64>() {
        if seconds > 1_000_000_000 {
            let now = chrono::Utc::now().timestamp().max(0) as u64;
            return seconds.checked_sub(now).map(Duration::from_secs);
        }
        return Some(Duration::from_secs(seconds));
    }
    parse_retry_after_value(trimmed)
}

fn find_retry_info(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if map
                .get("@type")
                .and_then(Value::as_str)
                .map(|text| text.ends_with("RetryInfo"))
                .unwrap_or(false)
            {
                return map
                    .get("retryDelay")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            map.values().find_map(find_retry_info)
        }
        Value::Array(items) => items.iter().find_map(find_retry_info),
        _ => None,
    }
}

fn body_contains(body: &str, needles: &[&str]) -> bool {
    let body = body.to_ascii_lowercase();
    needles.iter().any(|needle| body.contains(needle))
}

fn has_any_limit_number(signal: &ParsedLimitSignal) -> bool {
    signal.learned_context_window.is_some()
        || signal.learned_request_token_limit.is_some()
        || signal.learned_tpm_limit.is_some()
        || signal.requested_tokens.is_some()
        || signal.message_tokens.is_some()
}

fn number_after(text: &str, marker: &str) -> Option<u64> {
    let start = text.find(marker)? + marker.len();
    parse_first_number(&text[start..])
}

fn number_between(text: &str, start_marker: &str, end_marker: &str) -> Option<u64> {
    let start = text.find(start_marker)? + start_marker.len();
    let end = text[start..].find(end_marker).map(|index| start + index)?;
    parse_first_number(&text[start..end])
}

fn number_before(text: &str, marker: &str) -> Option<u64> {
    let end = text.find(marker)?;
    parse_last_number(&text[..end])
}

fn parse_first_number(text: &str) -> Option<u64> {
    number_spans(text)
        .into_iter()
        .next()
        .and_then(|number| number.parse::<u64>().ok())
}

fn parse_last_number(text: &str) -> Option<u64> {
    number_spans(text)
        .into_iter()
        .last()
        .and_then(|number| number.parse::<u64>().ok())
}

fn number_spans(text: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
            continue;
        }
        if ch == ',' && !current.is_empty() {
            continue;
        }
        if !current.is_empty() {
            spans.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        spans.push(current);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, header};

    #[test]
    fn classifies_verification_required() {
        let kind = classify_status(
            StatusCode::PAYMENT_REQUIRED,
            "please verify your payment method",
        );
        assert_eq!(kind, ErrorKind::CustomerVerificationRequired);
    }

    #[test]
    fn classifies_unsupported_api() {
        let kind = classify_status(
            StatusCode::NOT_FOUND,
            "This endpoint is not supported for chat/completions",
        );
        assert_eq!(kind, ErrorKind::UnsupportedApi);
    }

    #[test]
    fn classifies_model_unavailable() {
        let kind = classify_status(StatusCode::NOT_FOUND, "model not found");
        assert_eq!(kind, ErrorKind::ModelUnavailable);
    }

    #[test]
    fn classifies_unknown_model_as_unavailable() {
        let kind = classify_status(
            StatusCode::BAD_REQUEST,
            r#"{"code":"unknown_model","message":"Unknown model"}"#,
        );
        assert_eq!(kind, ErrorKind::ModelUnavailable);
    }

    #[test]
    fn classifies_discontinued_model() {
        let kind = classify_status(StatusCode::NOT_FOUND, "discontinued model");
        assert_eq!(kind, ErrorKind::ModelUnavailable);
    }

    #[test]
    fn classifies_quota_exhausted() {
        let kind = classify_status(StatusCode::TOO_MANY_REQUESTS, "daily quota exceeded");
        assert_eq!(kind, ErrorKind::QuotaExhausted);
    }

    #[test]
    fn classifies_payload_shape_errors_as_unsupported_api() {
        assert_eq!(
            classify_status(
                StatusCode::BAD_REQUEST,
                "reasoning_content is an unsupported field"
            ),
            ErrorKind::UnsupportedApi
        );
        assert_eq!(
            classify_status(
                StatusCode::BAD_REQUEST,
                "stream_options can only be set when stream is true"
            ),
            ErrorKind::UnsupportedApi
        );
        assert_eq!(
            classify_status(
                StatusCode::BAD_REQUEST,
                "tool_choice without tools is invalid"
            ),
            ErrorKind::UnsupportedApi
        );
    }

    #[test]
    fn classifies_max_token_rejections_as_context_overflow() {
        let kind = classify_status(StatusCode::BAD_REQUEST, "max_tokens must be less than 4096");
        assert_eq!(kind, ErrorKind::ContextOverflow);
    }

    #[test]
    fn classifies_payload_too_large_as_context_overflow() {
        let kind = classify_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "messages resulted in 67328 tokens",
        );
        assert_eq!(kind, ErrorKind::ContextOverflow);
    }

    #[test]
    fn parses_openrouter_context_window_and_request() {
        let signal = parse_limit_signal(
            "This model's maximum context length is 131072 tokens. However, you requested about 138731 tokens (67000 of text input, 328 of tool input, 71340 in the output).",
        )
        .unwrap();
        assert_eq!(signal.learned_context_window, Some(131_072));
        assert_eq!(signal.requested_tokens, Some(138_731));
        assert_eq!(signal.prompt_tokens, Some(67_000));
        assert_eq!(signal.tool_tokens, Some(328));
        assert_eq!(signal.output_tokens, Some(71_340));
    }

    #[test]
    fn parses_github_token_cap() {
        let signal = parse_limit_signal(
            r#"{"error":{"code":"tokens_limit_reached","message":"Max size: 8000 tokens"}}"#,
        )
        .unwrap();
        assert_eq!(signal.learned_request_token_limit, Some(8_000));
        assert_eq!(
            classify_status(
                StatusCode::BAD_REQUEST,
                "tokens_limit_reached Max size: 8000 tokens"
            ),
            ErrorKind::ContextOverflow
        );
    }

    #[test]
    fn parses_groq_request_too_large_without_quota() {
        let body = "Request too large for model. Limit 8000, Requested 91641";
        let signal = parse_limit_signal(body).unwrap();
        assert_eq!(signal.learned_request_token_limit, Some(8_000));
        assert_eq!(signal.requested_tokens, Some(91_641));
        assert_eq!(
            classify_status(StatusCode::TOO_MANY_REQUESTS, body),
            ErrorKind::ContextOverflow
        );
    }

    #[test]
    fn classifies_service_tier_unavailable() {
        let kind = classify_status(StatusCode::FORBIDDEN, "service tier unavailable");
        assert_eq!(kind, ErrorKind::NoAccess);
    }

    #[test]
    fn classifies_timeout() {
        let kind = classify_status(StatusCode::REQUEST_TIMEOUT, "");
        assert_eq!(kind, ErrorKind::Timeout);
    }

    #[test]
    fn classifies_temporary_rate_limit() {
        let kind = classify_status(StatusCode::TOO_MANY_REQUESTS, "please retry in 17s");
        assert_eq!(kind, ErrorKind::RateLimited);
    }

    #[test]
    fn parses_retry_after_from_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("17"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(17)));
    }

    #[test]
    fn parses_retry_info_from_body() {
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"17s"}]}}"#;
        assert_eq!(retry_after_from_body(body), Some(Duration::from_secs(17)));
    }

    #[test]
    fn quota_exhausted_cools_down_for_a_day() {
        assert_eq!(
            cooldown_for(&ErrorKind::QuotaExhausted, None, 0),
            Duration::from_secs(24 * 60 * 60)
        );
    }

    #[test]
    fn rate_limited_cooldown_grows_with_failures() {
        assert!(
            cooldown_for(&ErrorKind::RateLimited, None, 0)
                < cooldown_for(&ErrorKind::RateLimited, None, 3)
        );
    }

    #[test]
    fn retry_after_wins_over_exponential_backoff() {
        assert_eq!(
            cooldown_for(&ErrorKind::RateLimited, Some(Duration::from_secs(17)), 5),
            Duration::from_secs(17)
        );
    }
}
