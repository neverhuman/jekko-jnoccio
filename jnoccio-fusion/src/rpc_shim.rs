//! JSON-RPC 2.0 compatibility shim for the OpenHuman desktop app.
//!
//! OpenHuman's cloud-mode BootCheckGate expects a core that speaks JSON-RPC 2.0
//! on `POST /rpc` with `Authorization: Bearer <token>`.  This module translates
//! that protocol into jnoccio-fusion's native gateway calls so the OpenHuman
//! desktop app can connect directly to the fusion server.
//!
//! Supported methods:
//!
//! | Method                        | Behaviour                                |
//! |-------------------------------|------------------------------------------|
//! | `core.ping`                   | Returns `{"ok": true}`                   |
//! | `core.version`                | Returns `{"version": "<cargo version>"}` |
//! | `openhuman.ping`              | Alias for `core.ping`                    |
//! | `openhuman.update_version`    | Wrapped version for boot-check compat    |
//! | `openhuman.service_status`    | No legacy daemon detected                |
//! | `jnoccio.list_models`         | Lists available models from the registry |
//! | `jnoccio.chat_completion`     | Proxies to `Gateway::complete()`         |
//! | Any other method              | JSON-RPC -32601 "Method not found"       |

use crate::fusion::Gateway;
use crate::openai::ChatCompletionRequest;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Lightweight session store (in-memory + file-backed)
// ---------------------------------------------------------------------------

/// Simple session state persisted to `~/.openhuman/jnoccio-session.json`.
static SESSION: OnceLock<Mutex<SessionStore>> = OnceLock::new();

#[derive(Default, Clone, Serialize, Deserialize)]
struct SessionStore {
    token: Option<String>,
}

fn session() -> &'static Mutex<SessionStore> {
    SESSION.get_or_init(|| {
        let store = load_session_from_disk().unwrap_or_default();
        Mutex::new(store)
    })
}

fn session_file_path() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    std::path::Path::new(&home)
        .join(".openhuman")
        .join("jnoccio-session.json")
}

fn load_session_from_disk() -> Option<SessionStore> {
    let path = session_file_path();
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_session_to_disk(store: &SessionStore) {
    let path = session_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(&path, json);
    }
}

/// Crate version baked in at compile time.
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the version string to report for OpenHuman boot-check compatibility.
///
/// The boot gate does a strict `coreVersion === APP_VERSION` comparison.
/// When `JNOCCIO_COMPAT_VERSION` is set (e.g. to `0.53.26`), we echo that
/// value so the gate always passes.  Without it, we fall back to the crate
/// version.
fn compat_version() -> String {
    std::env::var("JNOCCIO_COMPAT_VERSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| CRATE_VERSION.to_string())
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    #[serde(default = "default_id")]
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

fn default_jsonrpc() -> String {
    "2.0".to_string()
}

fn default_id() -> Value {
    Value::Null
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    fn method_not_found(id: Value, method: &str) -> Self {
        Self::err(id, -32601, format!("Method not found: {method}"))
    }
}

// ---------------------------------------------------------------------------
// Bearer token validation
// ---------------------------------------------------------------------------

/// Validates the `Authorization: Bearer <token>` header against the configured
/// core token.  Returns `None` when auth passes, or an HTTP 401 response when
/// it fails.
fn check_bearer_auth(headers: &HeaderMap, expected: Option<&str>) -> Option<Response> {
    let Some(expected_token) = expected else {
        // No token configured — auth is disabled, allow the request.
        return None;
    };

    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if bearer
        .strip_prefix("Bearer ")
        .is_some_and(|token| token == expected_token)
    {
        None
    } else {
        tracing::warn!("[rpc] unauthorized request — missing or wrong bearer token");
        Some(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32000,
                        "message": "Missing or invalid Authorization header. Supply 'Authorization: Bearer <token>'."
                    }
                })),
            )
                .into_response(),
        )
    }
}

// ---------------------------------------------------------------------------
// RPC handler
// ---------------------------------------------------------------------------

/// Axum handler for `POST /rpc`.
///
/// Accepts a JSON-RPC 2.0 request body, authenticates via bearer token (when
/// configured), and dispatches to the appropriate method handler.
pub async fn rpc_handler(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // --- Auth check --------------------------------------------------------
    if let Some(rejection) = check_bearer_auth(&headers, gateway.config.core_token.as_deref()) {
        return rejection;
    }

    // --- Parse request -----------------------------------------------------
    let request: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(err) => {
            return Json(JsonRpcResponse::err(
                Value::Null,
                -32700,
                format!("Parse error: {err}"),
            ))
            .into_response();
        }
    };

    let id = request.id.clone();
    let method = request.method.as_str();

    tracing::debug!("[rpc] method={method}");

    // --- Method dispatch ---------------------------------------------------
    let response = match method {
        "core.ping" | "openhuman.ping" => JsonRpcResponse::ok(id, json!({ "ok": true })),

        "core.version" => JsonRpcResponse::ok(id, json!({ "version": CRATE_VERSION })),

        // OpenHuman boot-check calls this and expects the version wrapped in
        // an `RpcOutcome` envelope: `{ result: { version: "..." } }`.
        //
        // The boot gate does a strict `coreVersion === APP_VERSION` check.
        // Since we are a compatibility shim (not the real openhuman-core),
        // we use `compat_version()` which reads `JNOCCIO_COMPAT_VERSION`
        // env var (set to the OpenHuman app version) so the gate passes.
        "openhuman.update_version" => {
            let version = compat_version();
            JsonRpcResponse::ok(
                id,
                json!({
                    "result": { "version": version }
                }),
            )
        }

        // Cloud update trigger — the boot gate calls this when versions
        // mismatch.  Since we echo the caller's version above this should
        // rarely fire, but if it does, return success so the gate re-checks.
        "openhuman.update_run" => JsonRpcResponse::ok(id, json!({ "ok": true })),

        // Legacy daemon detection — we're not a daemon, report clean.
        "openhuman.service_status" => {
            JsonRpcResponse::ok(id, json!({ "installed": false, "running": false }))
        }

        // Daemon lifecycle stubs — silently succeed.
        "openhuman.service_stop" | "openhuman.service_uninstall" => {
            JsonRpcResponse::ok(id, json!({ "ok": true }))
        }

        // Backend API URL — the OAuth flow calls this to discover the
        // TinyHumans API endpoint for Google/GitHub/Twitter sign-in.
        "openhuman.config_resolve_api_url" => {
            let api_url = std::env::var("OPENHUMAN_API_URL")
                .unwrap_or_else(|_| "https://api.tinyhumans.ai".to_string());
            JsonRpcResponse::ok(id, json!({ "api_url": api_url }))
        }

        // Login token exchange — the deep link callback passes a one-time
        // loginToken that must be exchanged with the TinyHumans backend for
        // a JWT session token.  We proxy this request server-side.
        //
        // Note: the frontend normalises `openhuman.auth.X` → `openhuman.auth_X`
        // before sending over the wire (see rpcMethods.ts:39-41).
        "openhuman.auth_consume_login_token" | "openhuman.auth.consume_login_token" => {
            dispatch_consume_login_token(id, request.params).await
        }

        // Auth session management — persist the JWT so the app stays
        // logged in across restarts.
        "openhuman.auth_store_session" => {
            let token = request
                .params
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Ok(mut s) = session().lock() {
                s.token = if token.is_empty() { None } else { Some(token) };
                save_session_to_disk(&s);
            }
            JsonRpcResponse::ok(id, json!({ "result": { "ok": true } }))
        }
        "openhuman.auth_get_state" => {
            let has_token = session()
                .lock()
                .ok()
                .and_then(|s| s.token.as_ref().map(|_| true))
                .unwrap_or(false);
            JsonRpcResponse::ok(
                id,
                json!({ "result": { "isAuthenticated": has_token, "user": null } }),
            )
        }
        "openhuman.auth_get_session_token" => {
            let token = session().lock().ok().and_then(|s| s.token.clone());
            JsonRpcResponse::ok(id, json!({ "result": { "token": token } }))
        }
        "openhuman.auth_clear_session" => {
            if let Ok(mut s) = session().lock() {
                s.token = None;
                save_session_to_disk(&s);
            }
            JsonRpcResponse::ok(id, json!({ "result": { "ok": true } }))
        }

        // Full app state snapshot — reads the persisted session so the
        // app stays logged in across restarts.
        "openhuman.app_state_snapshot" => {
            let stored = session().lock().ok().and_then(|s| s.token.clone());
            let is_authed = stored.is_some();
            JsonRpcResponse::ok(
                id,
                json!({
                    "result": {
                        "auth": {
                            "isAuthenticated": is_authed,
                            "userId": null,
                            "user": null,
                            "profileId": null
                        },
                        "sessionToken": stored,
                        "currentUser": null,
                        "onboardingCompleted": false,
                        "chatOnboardingCompleted": false,
                        "analyticsEnabled": false,
                        "meetAutoOrchestratorHandoff": false,
                        "localState": {},
                        "runtime": {
                            "screenIntelligence": { "available": false, "enabled": false },
                            "localAi": { "available": false, "running": false },
                            "autocomplete": { "available": false, "enabled": false },
                            "service": { "installed": false, "running": false }
                        }
                    }
                }),
            )
        }

        // Config — return minimal defaults so config reads don't crash.
        "openhuman.config_get" => JsonRpcResponse::ok(id, json!({ "result": {} })),

        // Model listing — returns the resolved model registry.
        "jnoccio.list_models" => JsonRpcResponse::ok(id, gateway.model_list()),

        // Chat completion — translate JSON-RPC params into a chat request and
        // proxy through the gateway.
        "jnoccio.chat_completion" => {
            dispatch_chat_completion(id, request.params, &gateway, &headers).await
        }

        // Gateway health / status
        "jnoccio.health" => JsonRpcResponse::ok(id, json!(gateway.health())),
        "jnoccio.status" => JsonRpcResponse::ok(id, gateway.status()),

        // Catch-all for openhuman.* methods we haven't explicitly handled.
        // The app makes many RPC calls during onboarding and normal operation
        // (config, state, integrations, etc.).  Returning a generic empty
        // success lets the app proceed without breaking.
        _ if method.starts_with("openhuman.") => {
            tracing::info!("[rpc] unhandled openhuman method (returning empty success): {method}");
            JsonRpcResponse::ok(id, json!({ "result": {} }))
        }

        _ => JsonRpcResponse::method_not_found(id, method),
    };

    Json(response).into_response()
}

// ---------------------------------------------------------------------------
// Chat completion bridge
// ---------------------------------------------------------------------------

async fn dispatch_chat_completion(
    id: Value,
    params: Value,
    gateway: &Gateway,
    headers: &HeaderMap,
) -> JsonRpcResponse {
    let chat_request: ChatCompletionRequest = match serde_json::from_value(params) {
        Ok(req) => req,
        Err(err) => {
            return JsonRpcResponse::err(
                id,
                -32602,
                format!("Invalid chat completion params: {err}"),
            );
        }
    };

    let agent = agent_source_from_headers(headers);
    match gateway.complete(chat_request, agent.as_ref()).await {
        Ok(result) => JsonRpcResponse::ok(id, json!(result.response)),
        Err(err) => JsonRpcResponse::err(id, -32000, err.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Login token exchange proxy
// ---------------------------------------------------------------------------

/// Proxy the one-time login token → JWT exchange to the TinyHumans backend.
///
/// The frontend sends `{ loginToken: "..." }` via RPC; we POST to
/// `POST /telegram/login-tokens/<token>/consume` and return the JWT
/// wrapped in the `{ result: { jwtToken } }` envelope the frontend expects.
async fn dispatch_consume_login_token(id: Value, params: Value) -> JsonRpcResponse {
    let login_token = params
        .get("loginToken")
        .or_else(|| params.get("login_token"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if login_token.is_empty() {
        return JsonRpcResponse::err(id, -32602, "Missing loginToken parameter");
    }

    let api_url = std::env::var("OPENHUMAN_API_URL")
        .unwrap_or_else(|_| "https://api.tinyhumans.ai".to_string());

    let url = format!(
        "{}/telegram/login-tokens/{}/consume",
        api_url.trim_end_matches('/'),
        &login_token // Login tokens from the backend are already URL-safe hex/base64url
    );

    tracing::debug!("[rpc] consuming login token via backend");

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return JsonRpcResponse::err(id, -32000, format!("HTTP client error: {e}")),
    };

    let resp = match client.post(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("[rpc] login token consume request failed: {e}");
            return JsonRpcResponse::err(id, -32000, format!("Backend request failed: {e}"));
        }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        tracing::error!("[rpc] login token consume failed ({status}): {body}");
        return JsonRpcResponse::err(
            id,
            -32000,
            format!("Login token exchange failed ({status})"),
        );
    }

    // Backend returns: { success: true, data: { jwtToken: "..." } }
    let parsed: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return JsonRpcResponse::err(
                id,
                -32000,
                format!("Failed to parse backend response: {e}"),
            );
        }
    };

    let jwt_token = parsed
        .get("data")
        .and_then(|d| d.get("jwtToken"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    if jwt_token.is_empty() {
        return JsonRpcResponse::err(id, -32000, "Login token invalid or expired");
    }

    // The frontend expects: { result: { jwtToken: "..." } }
    JsonRpcResponse::ok(
        id,
        json!({
            "result": { "jwtToken": jwt_token }
        }),
    )
}

fn agent_source_from_headers(headers: &HeaderMap) -> Option<crate::state::AgentSource> {
    let id = header_string_alternate(headers, "x-jekko-run-id", "x-opencode-run-id")
        .or_else(|| header_string(headers, "x-openhuman-run-id"))?;
    Some(crate::state::AgentSource {
        id,
        client: header_string_alternate(headers, "x-jekko-client", "x-opencode-client")
            .or_else(|| header_string(headers, "x-openhuman-client")),
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
    header_string(headers, preferred_key).or_else(|| header_string(headers, alternate_key))
}

fn header_string(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn ok_response_shape() {
        let resp = JsonRpcResponse::ok(json!(1), json!({"ok": true}));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert_eq!(json["result"]["ok"], true);
        assert!(json.get("error").is_none());
    }

    #[test]
    fn error_response_shape() {
        let resp = JsonRpcResponse::err(json!(42), -32601, "Method not found: foo.bar");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 42);
        assert!(json.get("result").is_none());
        assert_eq!(json["error"]["code"], -32601);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap()
                .contains("foo.bar")
        );
    }

    #[test]
    fn method_not_found_uses_minus_32601() {
        let resp = JsonRpcResponse::method_not_found(json!(1), "unknown.method");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["error"]["code"], -32601);
    }

    #[test]
    fn bearer_auth_passes_when_token_matches() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer secret-token-123"),
        );
        assert!(check_bearer_auth(&headers, Some("secret-token-123")).is_none());
    }

    #[test]
    fn bearer_auth_rejects_wrong_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer wrong-token"),
        );
        assert!(check_bearer_auth(&headers, Some("secret-token-123")).is_some());
    }

    #[test]
    fn bearer_auth_rejects_missing_header() {
        let headers = HeaderMap::new();
        assert!(check_bearer_auth(&headers, Some("secret-token-123")).is_some());
    }

    #[test]
    fn bearer_auth_passes_when_no_token_configured() {
        let headers = HeaderMap::new();
        assert!(check_bearer_auth(&headers, None).is_none());
    }

    #[test]
    fn version_response_is_not_empty() {
        assert!(!CRATE_VERSION.is_empty());
    }
}
