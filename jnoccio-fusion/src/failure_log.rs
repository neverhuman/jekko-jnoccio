use crate::limits::{ParsedLimitSignal, parse_limit_signal};
use crate::providers::openai_compatible::ProviderError;
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
pub struct FailureLogEntry {
    pub request_id: String,
    pub phase: String,
    pub provider: String,
    pub visible_id: String,
    pub upstream_model: String,
    pub api_style: String,
    pub base_url: String,
    pub status_code: Option<u16>,
    pub status_text: Option<String>,
    pub error_kind: String,
    pub latency_ms: u64,
    pub cooldown_seconds: u64,
    pub cooldown_policy: CooldownPolicy,
    pub response_headers: Vec<HeaderLine>,
    pub upstream_error_body: String,
    pub parsed_limit_signal: Option<ParsedLimitSignal>,
    pub request_message_count: usize,
    pub request_tool_count: usize,
    pub request_stream: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct HeaderLine {
    pub name: String,
    pub value: HeaderValue,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HeaderValue {
    Utf8 { value: String },
    NonUtf8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CooldownPolicy {
    Present { seconds: u64 },
    Missing,
}

pub fn write_failure_log(
    receipts_dir: &Path,
    visible_id: &str,
    request_id: &str,
    phase: &str,
    error: &ProviderError,
    entry: FailureLogEntry,
) -> Result<PathBuf> {
    let dir = receipts_dir
        .join("failures")
        .join(&error.provider)
        .join(sanitize_component(visible_id));
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!(
        "{}_{}_{}.json",
        entry.created_at,
        sanitize_component(request_id),
        sanitize_component(phase)
    ));
    let path = with_unique_suffix(&path);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&entry).context("serialize failure log")?,
    )
    .with_context(|| format!("write {}", path.display()))?;
    rotate_failure_logs(&dir)?;
    Ok(path)
}

fn rotate_failure_logs(dir: &Path) -> Result<()> {
    let mut files = fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    if files.len() <= 20 {
        return Ok(());
    }
    let remove_count = files.len() - 20;
    for path in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}

fn with_unique_suffix(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("failure");
    let ext = path.extension().and_then(|value| value.to_str());
    for index in 1..1000 {
        let candidate = path.with_file_name(match ext {
            Some(ext) => format!("{stem}-{index}.{ext}"),
            None => format!("{stem}-{index}"),
        });
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[allow(clippy::too_many_arguments)]
pub fn build_failure_log_entry(
    request_id: &str,
    phase: &str,
    visible_id: &str,
    upstream_model: &str,
    api_style: &str,
    base_url: &str,
    error: &ProviderError,
    latency_ms: u64,
    cooldown: Duration,
    message_count: usize,
    tool_count: usize,
    stream: bool,
) -> FailureLogEntry {
    FailureLogEntry {
        request_id: request_id.to_string(),
        phase: phase.to_string(),
        provider: error.provider.clone(),
        visible_id: visible_id.to_string(),
        upstream_model: upstream_model.to_string(),
        api_style: api_style.to_string(),
        base_url: base_url.to_string(),
        status_code: error.status_code,
        status_text: error.status_text.clone(),
        error_kind: format!("{:?}", error.kind),
        latency_ms,
        cooldown_seconds: cooldown.as_secs(),
        cooldown_policy: match error.cooldown_delay() {
            Some(value) => CooldownPolicy::Present {
                seconds: value.as_secs(),
            },
            None => CooldownPolicy::Missing,
        },
        response_headers: sanitize_headers(&error.headers),
        parsed_limit_signal: parse_limit_signal(&error.body),
        upstream_error_body: sanitize_body(&error.body),
        request_message_count: message_count,
        request_tool_count: tool_count,
        request_stream: stream,
        created_at: chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
    }
}

fn sanitize_headers(headers: &reqwest::header::HeaderMap) -> Vec<HeaderLine> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let lower = name.as_str().to_ascii_lowercase();
            if lower.contains("authorization")
                || lower.contains("cookie")
                || lower.contains("token")
                || lower.contains("api-key")
                || lower.contains("set-cookie")
            {
                return None;
            }
            Some(HeaderLine {
                name: lower,
                value: value
                    .to_str()
                    .ok()
                    .map(|value| HeaderValue::Utf8 {
                        value: sanitize_value(value),
                    })
                    .unwrap_or(HeaderValue::NonUtf8),
            })
        })
        .collect()
}

fn sanitize_body(body: &str) -> String {
    // This function redacts potential secrets from log output to prevent secret sprawl
    // It checks for common API key/token patterns and replaces them with [redacted]
    let mut redacted = Vec::new();
    let mut redact_next = false;
    for token in body.replace('\r', " ").split_whitespace() {
        if redact_next {
            redacted.push("[redacted]".to_string());
            redact_next = false;
            continue;
        }
        if token.eq_ignore_ascii_case("bearer") {
            redacted.push("[redacted]".to_string());
            redact_next = true;
            continue;
        }
        // Detect and redact common API key patterns to prevent secret leakage in logs
        let is_secret = token.starts_with("sk") && token.chars().nth(2) == Some('-')
            || token.starts_with("ghp") && token.chars().nth(3) == Some('_')
            || token.starts_with("rk") && token.chars().nth(2) == Some('-');
        if is_secret {
            redacted.push("[redacted]".to_string());
            continue;
        }
        redacted.push(token.to_string());
    }
    sanitize_value(&redacted.join(" "))
}

fn sanitize_value(value: &str) -> String {
    value.chars().take(2000).collect()
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | ' ' => '_',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::ErrorKind;
    use crate::providers::openai_compatible::ProviderError;
    use axum::http::HeaderMap;
    use proptest::prelude::*;

    fn error(body: &str, cooldown_delay: Option<Duration>) -> ProviderError {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        ProviderError::test_fixture(
            "kilo",
            "openai_responses",
            "/responses",
            Some(429),
            Some("Too Many Requests".to_string()),
            headers,
            body.to_string(),
            ErrorKind::RateLimited,
            cooldown_delay,
        )
    }

    fn error_without_delay(body: &str) -> ProviderError {
        error(body, None)
    }

    proptest! {
        #[test]
        fn sanitize_component_strips_path_separators_and_spaces(chars in prop::collection::vec(any::<char>(), 0..64)) {
            let value: String = chars.into_iter().collect();
            let sanitized = sanitize_component(&value);

            prop_assert_eq!(sanitized.chars().count(), value.chars().count());
            prop_assert!(!sanitized.contains('/'));
            prop_assert!(!sanitized.contains('\\'));
            prop_assert!(!sanitized.contains(':'));
            prop_assert!(!sanitized.contains(' '));
        }

        #[test]
        fn sanitize_body_truncates_and_removes_carriage_returns(chars in prop::collection::vec(any::<char>(), 0..4096)) {
            let value: String = chars.into_iter().collect();
            let sanitized = sanitize_body(&value);

            prop_assert!(sanitized.chars().count() <= 2000);
            prop_assert!(!sanitized.contains('\r'));
        }
    }

    #[test]
    fn does_not_change_non_secret_body() {
        let entry = build_failure_log_entry(
            "request-1",
            "probe",
            "provider/model",
            "upstream",
            "openai_responses",
            "https://example.com",
            &error("quota exceeded\nplease wait", Some(Duration::from_secs(17))),
            12,
            Duration::from_secs(17),
            1,
            0,
            false,
        );
        assert_eq!(entry.upstream_error_body, "quota exceeded please wait");
    }

    #[test]
    fn redacts_bearer_token() {
        let entry = build_failure_log_entry(
            "request-1",
            "probe",
            "provider/model",
            "upstream",
            "openai_responses",
            "https://example.com",
            &error("Bearer fake-token", Some(Duration::from_secs(17))),
            12,
            Duration::from_secs(17),
            1,
            0,
            false,
        );
        assert_eq!(entry.upstream_error_body, "[redacted] [redacted]");
    }

    #[test]
    fn marks_missing_cooldown_explicitly() {
        let entry = build_failure_log_entry(
            "request-1",
            "probe",
            "provider/model",
            "upstream",
            "openai_responses",
            "https://example.com",
            &error_without_delay("quota exceeded"),
            12,
            Duration::from_secs(17),
            1,
            0,
            false,
        );
        assert!(matches!(entry.cooldown_policy, CooldownPolicy::Missing));
    }

    #[test]
    fn redacts_sk_token() {
        let token = ["sk", "TEST", "FAKE", "TOKEN"].join("-");
        let entry = build_failure_log_entry(
            "request-1",
            "probe",
            "provider/model",
            "upstream",
            "openai_responses",
            "https://example.com",
            &error(&token, Some(Duration::from_secs(17))),
            12,
            Duration::from_secs(17),
            1,
            0,
            false,
        );
        assert_eq!(entry.upstream_error_body, "[redacted]");
    }

    #[test]
    fn rotates_to_twenty_files() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..25 {
            let entry = build_failure_log_entry(
                &format!("request-{index}"),
                "probe",
                "provider/model",
                "upstream",
                "openai_responses",
                "https://example.com",
                &error("quota", Some(Duration::from_secs(17))),
                12,
                Duration::from_secs(17),
                1,
                0,
                false,
            );
            let request_id = entry.request_id.clone();
            let phase = entry.phase.clone();
            let _ = write_failure_log(
                dir.path(),
                "provider/model",
                &request_id,
                &phase,
                &error("quota", Some(Duration::from_secs(17))),
                entry,
            )
            .unwrap();
        }
        let count = fs::read_dir(
            dir.path()
                .join("failures")
                .join("kilo")
                .join("provider_model"),
        )
        .unwrap()
        .count();
        assert_eq!(count, 20);
    }
}
