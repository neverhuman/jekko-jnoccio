// jankurai:allow HLT-000-SCORE-DIMENSION reason=completion-common-is-shared-provider-abstraction-not-duplication expires=2027-01-01
use crate::limits::{
    ErrorKind, classify_status, parse_reset_headers, parse_retry_after, retry_after_from_body,
};
use crate::openai::{ChatChoiceMessage, ChatUsage, ToolCall};
use axum::http::HeaderValue;
use axum::http::{HeaderMap, StatusCode};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Map, Value};
use std::fmt;
use std::time::Duration;
use std::time::Instant;
use tracing::{debug, info, warn};

#[derive(Clone, Debug)]
pub struct ProviderError {
    pub provider: String,
    pub api_style: String,
    pub endpoint: String,
    pub status_code: Option<u16>,
    pub status_text: Option<String>,
    pub headers: HeaderMap,
    pub body: String,
    pub kind: ErrorKind,
    pub retry_after: Option<Duration>,
}

impl ProviderError {
    pub fn cooldown_delay(&self) -> Option<Duration> {
        self.retry_after
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn test_fixture(
        provider: &str,
        api_style: &str,
        endpoint: &str,
        status_code: Option<u16>,
        status_text: Option<String>,
        headers: HeaderMap,
        body: String,
        kind: ErrorKind,
        cooldown_delay: Option<Duration>,
    ) -> Self {
        Self {
            provider: provider.to_string(),
            api_style: api_style.to_string(),
            endpoint: endpoint.to_string(),
            status_code,
            status_text,
            headers,
            body,
            kind,
            retry_after: cooldown_delay,
        }
    }

    pub fn transport(
        provider: &str,
        api_style: &str,
        endpoint: &str,
        error: &reqwest::Error,
    ) -> Self {
        let kind = if error.is_timeout() {
            ErrorKind::Timeout
        } else {
            ErrorKind::ServerError
        };
        Self {
            provider: provider.to_string(),
            api_style: api_style.to_string(),
            endpoint: endpoint.to_string(),
            status_code: None,
            status_text: Some(error.to_string()),
            headers: HeaderMap::new(),
            body: error.to_string(),
            kind,
            retry_after: None,
        }
    }

    pub fn read_failure(
        provider: &str,
        api_style: &str,
        endpoint: &str,
        status: StatusCode,
        headers: HeaderMap,
        error: &reqwest::Error,
    ) -> Self {
        let kind = if error.is_timeout() {
            ErrorKind::Timeout
        } else {
            ErrorKind::InvalidResponse
        };
        Self {
            provider: provider.to_string(),
            api_style: api_style.to_string(),
            endpoint: endpoint.to_string(),
            status_code: Some(status.as_u16()),
            status_text: status.canonical_reason().map(str::to_string),
            headers,
            body: error.to_string(),
            kind,
            retry_after: None,
        }
    }

    pub fn response(
        provider: &str,
        api_style: &str,
        endpoint: &str,
        status: StatusCode,
        headers: HeaderMap,
        body: String,
    ) -> Self {
        let retry_after = if let Some(t) = parse_retry_after(&headers) {
            Some(t)
        } else if let Some(t) = parse_reset_headers(&headers) {
            Some(t)
        } else {
            retry_after_from_body(&body)
        };
        let kind = classify_status(status, &body);
        Self {
            provider: provider.to_string(),
            api_style: api_style.to_string(),
            endpoint: endpoint.to_string(),
            status_code: Some(status.as_u16()),
            status_text: status.canonical_reason().map(str::to_string),
            headers,
            body,
            kind,
            retry_after,
        }
    }

    pub fn parse_failure(
        provider: &str,
        api_style: &str,
        endpoint: &str,
        status: StatusCode,
        headers: HeaderMap,
        body: String,
        error: &dyn fmt::Display,
    ) -> Self {
        Self {
            provider: provider.to_string(),
            api_style: api_style.to_string(),
            endpoint: endpoint.to_string(),
            status_code: Some(status.as_u16()),
            status_text: Some(format!("invalid json: {error}")),
            headers,
            body,
            kind: ErrorKind::InvalidResponse,
            retry_after: None,
        }
    }

    pub fn summary(&self) -> String {
        let status = match self.status_code {
            Some(code) => code.to_string(),
            None => "transport".to_string(),
        };
        format!(
            "{} {} {} {} {:?}",
            self.provider, self.api_style, self.endpoint, status, self.kind
        )
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.summary(), self.kind)
    }
}

#[derive(Clone, Debug)]
pub struct UpstreamCompletion {
    pub message: ChatChoiceMessage,
    pub usage: Option<ChatUsage>,
    pub finish_reason: Option<String>,
    pub raw: Value,
}

pub struct ParsedJsonResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub text: String,
    pub raw: Value,
}

#[derive(Clone)]
pub struct CompletionTransport {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    provider: String,
    api_style: String,
}

impl CompletionTransport {
    pub fn new(
        client: reqwest::Client,
        base_url: String,
        api_key: String,
        provider: String,
        api_style: String,
    ) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            provider,
            api_style,
        }
    }

    pub async fn send_json(
        &self,
        endpoint: &str,
        body: Value,
    ) -> Result<reqwest::Response, ProviderError> {
        let started = Instant::now();
        let body_bytes = serde_json::to_vec(&body)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        info!(
            provider = %self.provider,
            api_style = %self.api_style,
            endpoint,
            body_bytes,
            "upstream transport request"
        );
        match self
            .client
            .post(format!("{}{}", self.base_url, endpoint))
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .json(&body)
            .send()
            .await
        {
            Ok(response) => {
                info!(
                    provider = %self.provider,
                    api_style = %self.api_style,
                    endpoint,
                    status = response.status().as_u16(),
                    latency_ms = started.elapsed().as_millis() as u64,
                    "upstream transport response"
                );
                Ok(response)
            }
            Err(err) => {
                warn!(
                    provider = %self.provider,
                    api_style = %self.api_style,
                    endpoint,
                    latency_ms = started.elapsed().as_millis() as u64,
                    error = %err,
                    "upstream transport failure"
                );
                Err(ProviderError::transport(
                    &self.provider,
                    &self.api_style,
                    endpoint,
                    &err,
                ))
            }
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn api_style(&self) -> &str {
        &self.api_style
    }
}

pub(crate) fn build_upstream_completion(
    raw: Value,
    message: ChatChoiceMessage,
    usage: Option<ChatUsage>,
    finish_reason: Option<String>,
) -> UpstreamCompletion {
    UpstreamCompletion {
        message,
        usage,
        finish_reason,
        raw,
    }
}

pub(crate) fn build_tool_call(
    id: Option<&str>,
    kind: Option<&str>,
    name: &str,
    arguments: Option<&str>,
) -> ToolCall {
    let id = match id {
        Some(id) => id.to_string(),
        None => uuid::Uuid::new_v4().to_string(),
    };
    let kind = match kind {
        Some(kind) => kind.to_string(),
        None => "function".to_string(),
    };
    let arguments = match arguments {
        Some(arguments) => arguments.to_string(),
        None => String::new(),
    };
    ToolCall {
        id,
        kind,
        function: crate::openai::ToolCallFunction {
            name: name.to_string(),
            arguments,
        },
    }
}

pub(crate) async fn parse_json_response(
    response: reqwest::Response,
    provider: &str,
    api_style: &str,
    endpoint: &str,
) -> Result<ParsedJsonResponse, ProviderError> {
    let started = Instant::now();
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await.map_err(|err| {
        ProviderError::read_failure(provider, api_style, endpoint, status, headers.clone(), &err)
    })?;
    if !status.is_success() {
        warn!(
            provider,
            api_style,
            endpoint,
            status = status.as_u16(),
            body_bytes = text.len(),
            body_preview = %preview_text(&text, 1024),
            "upstream response rejected"
        );
        return Err(ProviderError::response(
            provider, api_style, endpoint, status, headers, text,
        ));
    }
    let raw = serde_json::from_str(&text).map_err(|err| {
        ProviderError::parse_failure(
            provider,
            api_style,
            endpoint,
            status,
            headers.clone(),
            text.clone(),
            &err,
        )
    })?;
    debug!(
        provider,
        api_style,
        endpoint,
        status = status.as_u16(),
        body_bytes = text.len(),
        latency_ms = started.elapsed().as_millis() as u64,
        "upstream response parsed"
    );
    Ok(ParsedJsonResponse {
        status,
        headers,
        text,
        raw,
    })
}

pub(crate) fn preview_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = collapsed.chars().take(max_chars).collect::<String>();
    if collapsed.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

pub fn parse_raw_completion(raw: Value) -> UpstreamCompletion {
    let text = extract_output_text(&raw).unwrap_or_default();
    let usage = raw
        .get("usage")
        .cloned()
        .and_then(|value| serde_json::from_value::<ChatUsage>(value).ok());
    let reasoning = extract_reasoning(&raw);
    UpstreamCompletion {
        message: ChatChoiceMessage {
            role: "assistant".to_string(),
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls: extract_tool_calls(&raw),
            reasoning_text: reasoning.clone(),
            reasoning_content: reasoning,
            reasoning_opaque: None,
            extra: Map::new(),
        },
        usage,
        finish_reason: raw.get("status").and_then(Value::as_str).map(|value| {
            if value == "incomplete" {
                "length".to_string()
            } else {
                "stop".to_string()
            }
        }),
        raw,
    }
}

fn extract_output_text(raw: &Value) -> Option<String> {
    if let Some(text) = raw.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    let mut parts = Vec::new();
    let output = raw.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for part in content {
                match part.get("type").and_then(Value::as_str) {
                    Some("output_text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            parts.push(text.to_string());
                        }
                    }
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            parts.push(text.to_string());
                        } else if let Some(text) = part.as_str() {
                            parts.push(text.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(""))
}

fn extract_reasoning(raw: &Value) -> Option<String> {
    raw.get("reasoning")
        .and_then(Value::as_object)
        .and_then(|reasoning| reasoning.get("summary"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn extract_tool_calls(raw: &Value) -> Option<Vec<ToolCall>> {
    let output = raw.get("output")?.as_array()?;
    let mut calls = Vec::new();
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(tool_calls) = item.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        for call in tool_calls {
            let function = call.get("function").and_then(Value::as_object)?;
            let name = function.get("name").and_then(Value::as_str)?;
            calls.push(build_tool_call(
                call.get("id").and_then(Value::as_str),
                call.get("type").and_then(Value::as_str),
                name,
                function.get("arguments").and_then(Value::as_str),
            ));
        }
    }
    if calls.is_empty() { None } else { Some(calls) }
}

#[cfg(test)]
mod tests {
    use super::preview_text;

    #[test]
    fn preview_text_collapses_whitespace() {
        assert_eq!(
            preview_text("hello\n   world\tfrom", 64),
            "hello world from"
        );
    }

    #[test]
    fn preview_text_truncates_long_values() {
        assert_eq!(preview_text("0123456789abcdef", 8), "01234567...");
    }
}
