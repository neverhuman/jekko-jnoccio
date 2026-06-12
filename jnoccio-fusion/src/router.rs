use crate::fusion::{DashboardMessage, Gateway, GatewayError};
use crate::mcp;
use crate::openai::{
    ChatChoiceDelta, ChatCompletionRequest, EmbeddingsRequest, build_chunk, error_response,
    sse_data, sse_done, sse_event,
};
use crate::state::AgentSource;
use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{Duration, interval};
use tokio_stream::iter;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

pub fn router(gateway: Arc<Gateway>) -> axum::Router {
    // CORS — required for the Tauri webview transport. It issues preflight
    // OPTIONS requests before cross-origin RPC calls, so the API must allow
    // the methods and headers that the webview sends.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderName::from_static("x-jekko-agent-role"),
            axum::http::HeaderName::from_static("x-jekko-zyal-run-id"),
            axum::http::HeaderName::from_static("x-jekko-zyal-lane-id"),
            axum::http::HeaderName::from_static("x-jekko-credential-user-id"),
            axum::http::HeaderName::from_static("x-jekko-credential-policy"),
        ])
        .max_age(std::time::Duration::from_secs(86400));

    axum::Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/jnoccio/status", get(status))
        .route("/v1/jnoccio/metrics", get(metrics))
        .route("/v1/jnoccio/metrics/ws", get(metrics_ws))
        .route("/metrics", get(metrics_prometheus))
        .route("/v1/jnoccio/agents/heartbeat", post(agent_heartbeat))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/embeddings", post(embeddings))
        .route("/mcp", get(mcp_get).post(mcp_post))
        // JSON-RPC 2.0 shim — lets OpenHuman desktop connect in cloud mode.
        .route("/rpc", post(crate::rpc_shim::rpc_handler))
        .layer(middleware::from_fn(log_http_activity))
        .layer(cors)
        .with_state(gateway)
}

async fn log_http_activity(req: axum::http::Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let query = uri.query().map(str::to_string);
    let headers = req.headers().clone();
    let agent = agent_source_from_headers(&headers);
    let started = Instant::now();

    info!(
        method = %method,
        path = %path,
        query = query.as_deref().unwrap_or(""),
        agent = ?agent,
        user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or(""),
        authorization_present = headers.contains_key(header::AUTHORIZATION),
        "http request started"
    );

    let response = next.run(req).await;
    let status = response.status();
    let latency_ms = started.elapsed().as_millis() as u64;

    if status.is_server_error() {
        warn!(
            method = %method,
            path = %path,
            status = status.as_u16(),
            latency_ms,
            "http request completed"
        );
    } else {
        info!(
            method = %method,
            path = %path,
            status = status.as_u16(),
            latency_ms,
            "http request completed"
        );
    }

    response
}

async fn health(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    Json(json!(gateway.health()))
}

async fn models(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    Json(gateway.model_list())
}

async fn status(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    Json(gateway.status())
}

async fn metrics(State(gateway): State<Arc<Gateway>>) -> Response {
    match gateway.dashboard_snapshot() {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn metrics_ws(State(gateway): State<Arc<Gateway>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| dashboard_socket(socket, gateway))
}

/// Prometheus 0.0.4 text-format scrape endpoint at the canonical `/metrics`
/// path. Mirrors the JSON dashboard data at `/v1/jnoccio/metrics` but in the
/// shape Prometheus + Grafana expect — see
/// `jnoccio_fusion::metrics::render_prometheus` for the metric set.
async fn metrics_prometheus(State(gateway): State<Arc<Gateway>>) -> Response {
    match gateway.dashboard_snapshot() {
        Ok(snapshot) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; version=0.0.4",
            )],
            crate::metrics::render_prometheus(&snapshot),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; version=0.0.4",
            )],
            format!("# error: {err}\n"),
        )
            .into_response(),
    }
}

async fn dashboard_socket(socket: WebSocket, gateway: Arc<Gateway>) {
    struct SocketGuard;
    impl Drop for SocketGuard {
        fn drop(&mut self) {
            info!("dashboard websocket disconnected");
        }
    }

    let _guard = SocketGuard;
    let (mut sender, mut receiver) = socket.split();
    let mut last_event_id = 0i64;
    let mut seen_event_ids = HashSet::new();
    info!("dashboard websocket connected");
    if let Ok(snapshot) = gateway.dashboard_snapshot() {
        last_event_id = snapshot
            .recent_events
            .iter()
            .map(|event| event.id)
            .max()
            .unwrap_or(0);
        seen_event_ids.extend(snapshot.recent_events.iter().map(|event| event.id));
        if sender
            .send(Message::Text(
                serde_json::to_string(&DashboardMessage::Snapshot { snapshot })
                    .expect("dashboard snapshot serializes")
                    .into(),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut updates = gateway.subscribe();
    let mut heartbeat = interval(Duration::from_secs(15));
    let mut poll = interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            item = updates.recv() => {
                match item {
                    Ok(message) => {
                        if let DashboardMessage::RequestEvent { event } = &message {
                            if !seen_event_ids.insert(event.id) {
                                continue;
                            }
                            last_event_id = last_event_id.max(event.id);
                        }
                        if sender
                            .send(Message::Text(serde_json::to_string(&message).expect("dashboard message serializes").into()))
                            .await
                            .is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Ok(snapshot) = gateway.dashboard_snapshot() {
                            last_event_id = snapshot.recent_events.iter().map(|event| event.id).max().unwrap_or(last_event_id);
                            seen_event_ids.extend(snapshot.recent_events.iter().map(|event| event.id));
                            if sender
                                .send(Message::Text(serde_json::to_string(&DashboardMessage::Snapshot { snapshot }).expect("dashboard snapshot serializes").into()))
                                .await
                                .is_err() {
                                return;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            _ = poll.tick() => {
                let mut saw_new_event = false;
                loop {
                    let Ok(events) = gateway.state.recent_metric_events_after(last_event_id, 200) else {
                        break;
                    };
                    if events.is_empty() {
                        break;
                    }
                    let event_count = events.len();
                    for event in events {
                        if !seen_event_ids.insert(event.id) {
                            continue;
                        }
                        last_event_id = last_event_id.max(event.id);
                        saw_new_event = true;
                        if sender
                            .send(Message::Text(serde_json::to_string(&DashboardMessage::RequestEvent { event }).expect("dashboard event serializes").into()))
                            .await
                            .is_err() {
                            return;
                        }
                    }
                    if event_count < 200 {
                        break;
                    }
                }
                if saw_new_event {
                    if let Ok(snapshot) = gateway.dashboard_snapshot()
                        && sender
                            .send(Message::Text(serde_json::to_string(&DashboardMessage::Snapshot { snapshot }).expect("dashboard snapshot serializes").into()))
                            .await
                            .is_err() {
                        return;
                    }
                } else if let Ok(snapshot) = gateway.dashboard_snapshot()
                    && sender
                        .send(Message::Text(serde_json::to_string(&DashboardMessage::Snapshot { snapshot }).expect("dashboard snapshot serializes").into()))
                        .await
                        .is_err() {
                    return;
                }
            }
            _ = heartbeat.tick() => {
                if sender
                    .send(Message::Text(serde_json::to_string(&Gateway::heartbeat_message()).expect("heartbeat serializes").into()))
                    .await
                    .is_err() {
                    return;
                }
            }
            received = receiver.next() => {
                if received.is_none() {
                    return;
                }
            }
        }
    }
}

async fn chat(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> Response {
    let agent = agent_source_from_headers(&headers);
    let tool_count = request
        .tools
        .as_ref()
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    info!(
        model = %request.model,
        stream = request.stream.unwrap_or(false),
        message_count = request.messages.len(),
        tool_count,
        has_response_format = request.response_format.is_some(),
        agent = ?agent,
        "chat request received"
    );
    match gateway.complete(request.clone(), agent.as_ref()).await {
        Ok(result) => {
            let jnoccio_headers = jnoccio_response_headers(&result);
            let usage = result.response.usage.as_ref();
            info!(
                request_id = result
                    .response
                    .extra
                    .get("jnoccio")
                    .and_then(|value| value.get("request_id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                winner_model_id = %result.winner_model_id,
                confidence = result.confidence,
                prompt_tokens = usage.and_then(|item| item.prompt_tokens).unwrap_or(0),
                completion_tokens = usage.and_then(|item| item.completion_tokens).unwrap_or(0),
                total_tokens = usage.and_then(|item| item.total_tokens).unwrap_or(0),
                stream = request.stream.unwrap_or(false),
                "chat request completed"
            );
            if request.stream.unwrap_or(false) {
                stream_response(&result, &jnoccio_headers)
            } else {
                let mut response = Json(result.response).into_response();
                apply_jnoccio_response_headers(response.headers_mut(), &jnoccio_headers);
                response
            }
        }
        Err(err) => {
            warn!(
                model = %request.model,
                status = err.status_code().as_u16(),
                kind = err.kind(),
                "chat request failed"
            );
            error_response_for(err)
        }
    }
}

async fn embeddings(
    State(gateway): State<Arc<Gateway>>,
    Json(request): Json<EmbeddingsRequest>,
) -> Response {
    let input_count = match &request.input {
        crate::openai::EmbeddingsInput::Single(_) => 1,
        crate::openai::EmbeddingsInput::Batch(items) => items.len(),
    };
    info!(
        model = %request.model,
        input_count,
        "embeddings request received"
    );
    match gateway.embed(request).await {
        Ok(response) => {
            info!(
                model = %response.model,
                vectors = response.data.len(),
                "embeddings request completed"
            );
            Json(response).into_response()
        }
        Err(err) => {
            warn!(
                status = err.status_code().as_u16(),
                kind = err.kind(),
                "embeddings request failed"
            );
            error_response_for(err)
        }
    }
}

async fn mcp_get() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, HeaderValue::from_static("POST"))],
        Json(json!({
            "error": {
                "message": "MCP Streamable HTTP GET is not enabled on this server",
                "type": "method_not_allowed",
                "code": "method_not_allowed"
            }
        })),
    )
        .into_response()
}

async fn mcp_post(
    State(gateway): State<Arc<Gateway>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    mcp::handle_http(gateway, body).await
}

async fn agent_heartbeat(State(gateway): State<Arc<Gateway>>, headers: HeaderMap) -> Response {
    if let Some(agent) = agent_source_from_headers(&headers) {
        let _ = gateway.state.record_agent_activity(&agent);
        info!(agent = ?agent, "agent heartbeat recorded");
    }
    StatusCode::NO_CONTENT.into_response()
}

fn stream_response(
    result: &crate::fusion::GatewayResult,
    jnoccio_headers: &[(&'static str, String)],
) -> Response {
    let stream = iter(
        stream_events(result)
            .into_iter()
            .map(|event| Ok::<_, Infallible>(Bytes::from(event))),
    );
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=utf-8"),
        )
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from_stream(stream))
        .expect("sse response");
    apply_jnoccio_response_headers(response.headers_mut(), jnoccio_headers);
    response
}

fn apply_jnoccio_response_headers(
    headers: &mut HeaderMap,
    jnoccio_headers: &[(&'static str, String)],
) {
    for (name, value) in jnoccio_headers {
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(axum::http::HeaderName::from_static(name), value);
        }
    }
}

fn jnoccio_response_headers(result: &crate::fusion::GatewayResult) -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    let Some(meta) = result
        .response
        .extra
        .get("jnoccio")
        .and_then(Value::as_object)
    else {
        return headers;
    };

    push_string_header(&mut headers, "x-jnoccio-request-id", meta.get("request_id"));
    push_string_header(&mut headers, "x-jnoccio-route-mode", meta.get("route_mode"));
    push_string_header(&mut headers, "x-jnoccio-sampled", meta.get("sampled"));
    push_string_header(
        &mut headers,
        "x-jnoccio-complexity-tier",
        meta.get("complexity_tier"),
    );
    push_string_header(
        &mut headers,
        "x-jnoccio-primary-model-id",
        meta.get("primary_model_id"),
    );
    push_json_header(
        &mut headers,
        "x-jnoccio-backup-model-ids",
        meta.get("backup_model_ids"),
    );
    push_string_header(
        &mut headers,
        "x-jnoccio-fusion-model-id",
        meta.get("fusion_model_id"),
    );
    push_string_header(
        &mut headers,
        "x-jnoccio-winner-model-id",
        meta.get("winner_model_id"),
    );
    push_string_header(
        &mut headers,
        "x-jnoccio-winner-route-slot-id",
        meta.get("winner_route_slot_id"),
    );
    push_string_header(
        &mut headers,
        "x-jnoccio-winner-upstream-model-id",
        meta.get("winner_upstream_model_id"),
    );
    push_string_header(
        &mut headers,
        "x-jnoccio-credential-user-id",
        meta.get("credential_user_id"),
    );
    push_string_header(&mut headers, "x-jnoccio-confidence", meta.get("confidence"));
    push_string_header(
        &mut headers,
        "x-jnoccio-model-decisions-hash",
        meta.get("model_decisions_hash"),
    );
    headers
}

fn push_string_header(
    headers: &mut Vec<(&'static str, String)>,
    name: &'static str,
    value: Option<&Value>,
) {
    let Some(value) = value else {
        return;
    };
    let next = match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    };
    if let Some(next) = next {
        headers.push((name, next));
    }
}

fn push_json_header(
    headers: &mut Vec<(&'static str, String)>,
    name: &'static str,
    value: Option<&Value>,
) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() {
        return;
    }
    headers.push((name, value.to_string()));
}

fn stream_events(result: &crate::fusion::GatewayResult) -> Vec<String> {
    let mut parts = Vec::new();
    let choice = &result.response.choices[0];
    let content = choice.message.content.clone().unwrap_or_default();
    let tool_calls = choice.message.tool_calls.clone();
    let pieces = chunk_text(&content, 160);
    for (index, piece) in pieces.into_iter().enumerate() {
        let delta = ChatChoiceDelta {
            role: if index == 0 {
                Some("assistant".to_string())
            } else {
                None
            },
            content: Some(piece),
            reasoning_text: None,
            reasoning_content: None,
            reasoning_opaque: None,
            tool_calls: None,
            extra: Default::default(),
        };
        parts.push(sse_data(&build_chunk(
            &result.response.model,
            delta,
            None,
            None,
        )));
    }

    if let Some(tool_calls) = tool_calls {
        let delta = ChatChoiceDelta {
            role: if content.is_empty() {
                Some("assistant".to_string())
            } else {
                None
            },
            content: None,
            reasoning_text: None,
            reasoning_content: None,
            reasoning_opaque: None,
            tool_calls: Some(
                tool_calls
                    .into_iter()
                    .enumerate()
                    .map(|(index, call)| crate::openai::ToolCallDelta {
                        index: index as u64,
                        id: Some(call.id),
                        r#type: Some(call.kind),
                        function: crate::openai::ToolCallFunctionDelta {
                            name: Some(call.function.name),
                            arguments: Some(call.function.arguments),
                            extra: Default::default(),
                        },
                        extra: Default::default(),
                    })
                    .collect(),
            ),
            extra: Default::default(),
        };
        parts.push(sse_data(&build_chunk(
            &result.response.model,
            delta,
            choice.finish_reason.clone(),
            result.response.usage.clone(),
        )));
    } else {
        let delta = ChatChoiceDelta {
            role: if content.is_empty() {
                Some("assistant".to_string())
            } else {
                None
            },
            content: None,
            reasoning_text: None,
            reasoning_content: None,
            reasoning_opaque: None,
            tool_calls: None,
            extra: Default::default(),
        };
        parts.push(sse_data(&build_chunk(
            &result.response.model,
            delta,
            choice.finish_reason.clone(),
            result.response.usage.clone(),
        )));
    }
    if let Some(meta) = result
        .response
        .extra
        .get("jnoccio")
        .filter(|value| value.is_object())
    {
        parts.push(sse_event("jnoccio-metadata", meta));
    }
    parts.push(sse_done());
    parts
}

fn error_response_for(err: GatewayError) -> Response {
    (
        err.status_code(),
        Json(json!(error_response(err.to_string(), err.kind(), None))),
    )
        .into_response()
}

fn agent_source_from_headers(headers: &HeaderMap) -> Option<AgentSource> {
    let id = header_string_alternate(headers, "x-jekko-run-id", "x-opencode-run-id")?;
    Some(AgentSource {
        id,
        client: header_string_alternate(headers, "x-jekko-client", "x-opencode-client"),
        session_id: header_string_alternate(headers, "x-jekko-session", "x-opencode-session"),
        agent_role: header_string_alternate(headers, "x-jekko-agent-role", "x-opencode-agent-role"),
        zyal_run_id: header_string_alternate(
            headers,
            "x-jekko-zyal-run-id",
            "x-opencode-zyal-run-id",
        ),
        zyal_lane_id: header_string_alternate(
            headers,
            "x-jekko-zyal-lane-id",
            "x-opencode-zyal-lane-id",
        ),
        credential_user_id: header_string_alternate(
            headers,
            "x-jekko-credential-user-id",
            "x-opencode-credential-user-id",
        ),
        credential_policy: header_string_alternate(
            headers,
            "x-jekko-credential-policy",
            "x-opencode-credential-policy",
        ),
        process_role: header_string_alternate(
            headers,
            "x-jekko-process-role",
            "x-opencode-process-role",
        ),
        pid: header_string_alternate(headers, "x-jekko-pid", "x-opencode-pid")
            .and_then(|value| value.parse::<i64>().ok()),
        user_agent: header_string(headers, "user-agent"),
        version: header_string_alternate(headers, "x-jekko-version", "x-opencode-version"),
    })
}

fn header_string_alternate(
    headers: &HeaderMap,
    preferred_key: &str,
    alternate_key: &str,
) -> Option<String> {
    if let Some(value) = header_string(headers, preferred_key) {
        return Some(value);
    }
    header_string(headers, alternate_key)
}

fn header_string(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

fn chunk_text(text: &str, size: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if current.chars().count() >= size {
            out.push(current);
            current = String::new();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::{ChatChoiceMessage, ChatCompletionChoice, ChatCompletionResponse};
    use serde_json::Map;

    #[test]
    fn error_response_uses_gateway_status_code() {
        let response = error_response_for(GatewayError::NoAvailableModels);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn agent_source_accepts_jekko_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-jekko-run-id", HeaderValue::from_static("run-jekko"));
        headers.insert("x-jekko-client", HeaderValue::from_static("codex"));
        headers.insert("x-jekko-session", HeaderValue::from_static("session-jekko"));
        headers.insert("x-jekko-agent-role", HeaderValue::from_static("answerer"));
        headers.insert("x-jekko-zyal-run-id", HeaderValue::from_static("zyal-run"));
        headers.insert("x-jekko-zyal-lane-id", HeaderValue::from_static("lane-a"));
        headers.insert("x-jekko-process-role", HeaderValue::from_static("main"));
        headers.insert("x-jekko-pid", HeaderValue::from_static("123"));
        headers.insert("x-jekko-version", HeaderValue::from_static("1.2.3"));
        headers.insert("user-agent", HeaderValue::from_static("jekko/1.2.3"));

        let source = agent_source_from_headers(&headers).expect("agent source");
        assert_eq!(source.id, "run-jekko");
        assert_eq!(source.client.as_deref(), Some("codex"));
        assert_eq!(source.session_id.as_deref(), Some("session-jekko"));
        assert_eq!(source.agent_role.as_deref(), Some("answerer"));
        assert_eq!(source.zyal_run_id.as_deref(), Some("zyal-run"));
        assert_eq!(source.zyal_lane_id.as_deref(), Some("lane-a"));
        assert_eq!(source.process_role.as_deref(), Some("main"));
        assert_eq!(source.pid, Some(123));
        assert_eq!(source.version.as_deref(), Some("1.2.3"));
        assert_eq!(source.user_agent.as_deref(), Some("jekko/1.2.3"));
    }

    #[test]
    fn agent_source_accepts_opencode_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-opencode-run-id", HeaderValue::from_static("run-alt"));
        headers.insert("x-opencode-client", HeaderValue::from_static("alt-client"));
        headers.insert(
            "x-opencode-session",
            HeaderValue::from_static("session-alt"),
        );
        headers.insert(
            "x-opencode-process-role",
            HeaderValue::from_static("worker"),
        );
        headers.insert("x-opencode-pid", HeaderValue::from_static("456"));
        headers.insert("x-opencode-version", HeaderValue::from_static("0.9.0"));

        let source = agent_source_from_headers(&headers).expect("agent source");
        assert_eq!(source.id, "run-alt");
        assert_eq!(source.client.as_deref(), Some("alt-client"));
        assert_eq!(source.session_id.as_deref(), Some("session-alt"));
        assert_eq!(source.process_role.as_deref(), Some("worker"));
        assert_eq!(source.pid, Some(456));
        assert_eq!(source.version.as_deref(), Some("0.9.0"));
    }

    #[test]
    fn agent_source_prefers_jekko_headers_over_opencode_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-opencode-run-id", HeaderValue::from_static("run-alt"));
        headers.insert("x-jekko-run-id", HeaderValue::from_static("run-jekko"));
        headers.insert("x-opencode-process-role", HeaderValue::from_static("alt"));
        headers.insert("x-jekko-process-role", HeaderValue::from_static("main"));

        let source = agent_source_from_headers(&headers).expect("agent source");
        assert_eq!(source.id, "run-jekko");
        assert_eq!(source.process_role.as_deref(), Some("main"));
    }

    #[test]
    fn stream_events_do_not_expose_internal_receipts_as_reasoning() {
        let result = crate::fusion::GatewayResult {
            response: ChatCompletionResponse {
                id: "chatcmpl-test".to_string(),
                kind: "chat.completion".to_string(),
                created: 1,
                model: "jnoccio/jnoccio-fusion".to_string(),
                choices: vec![ChatCompletionChoice {
                    index: 0,
                    message: ChatChoiceMessage {
                        role: "assistant".to_string(),
                        content: Some("hello".to_string()),
                        tool_calls: None,
                        reasoning_text: None,
                        reasoning_content: None,
                        reasoning_opaque: None,
                        extra: Map::new(),
                    },
                    finish_reason: Some("stop".to_string()),
                    extra: Map::new(),
                }],
                usage: None,
                extra: Map::new(),
            },
            receipts: vec![
                "request_id=secret".to_string(),
                "draft_models=a,b".to_string(),
                "provider failure".to_string(),
            ],
            winner_model_id: "provider/model".to_string(),
            confidence: 0.9,
        };

        let text = stream_events(&result).join("");
        assert!(text.contains("\"content\":\"hello\""));
        assert!(!text.contains("request_id=secret"));
        assert!(!text.contains("draft_models"));
        assert!(!text.contains("provider failure"));
        assert!(!text.contains("reasoning_content"));
    }

    #[test]
    fn stream_events_include_metadata_event_before_done() {
        let mut extra = Map::new();
        extra.insert(
            "jnoccio".to_string(),
            json!({
                "request_id": "req-1",
                "credential_user_id": "user-7",
                "winner_model_id": "provider/model",
            }),
        );
        let result = crate::fusion::GatewayResult {
            response: ChatCompletionResponse {
                id: "chatcmpl-test".to_string(),
                kind: "chat.completion".to_string(),
                created: 1,
                model: "jnoccio/jnoccio-fusion".to_string(),
                choices: vec![ChatCompletionChoice {
                    index: 0,
                    message: ChatChoiceMessage {
                        role: "assistant".to_string(),
                        content: Some("hello".to_string()),
                        tool_calls: None,
                        reasoning_text: None,
                        reasoning_content: None,
                        reasoning_opaque: None,
                        extra: Map::new(),
                    },
                    finish_reason: Some("stop".to_string()),
                    extra: Map::new(),
                }],
                usage: None,
                extra,
            },
            receipts: vec![],
            winner_model_id: "provider/model".to_string(),
            confidence: 0.9,
        };

        let text = stream_events(&result).join("");
        let metadata_pos = text
            .find("event: jnoccio-metadata")
            .expect("metadata event");
        let done_pos = text.find("data: [DONE]").expect("done marker");
        assert!(metadata_pos < done_pos);
        assert!(text.contains("\"credential_user_id\":\"user-7\""));
    }
}
