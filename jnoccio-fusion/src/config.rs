use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const MAX_RUNTIME_WORKER_THREADS: usize = 32;
pub const MAX_MANAGED_INSTANCES: usize = 20;
pub const DEFAULT_SPAWN_BATCH_LIMIT: usize = 5;

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    pub bind: Option<String>,
    pub database: Option<String>,
    pub env_file: Option<String>,
    pub models_file: Option<String>,
    pub receipts_dir: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub core_token: Option<String>,
    pub routing: Option<ServerRoutingConfig>,
    pub runtime: Option<ServerRuntimeConfig>,
    pub scaling: Option<ServerScalingConfig>,
    pub upstream_key_source: Option<UpstreamKeySource>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerRoutingConfig {
    pub fusion_sample_rate: Option<f64>,
    pub fast_backup_count: Option<usize>,
    pub event_retention_rows: Option<usize>,
    pub minute_bucket_retention_days: Option<u64>,
    pub proof_profile: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerRuntimeConfig {
    pub worker_threads: Option<usize>,
    pub spawned_worker_threads: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerScalingConfig {
    pub max_instances: Option<usize>,
    pub spawn_batch_limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct RoutingDefaults {
    pub fusion_sample_rate: f64,
    pub fast_backup_count: usize,
    pub event_retention_rows: usize,
    pub minute_bucket_retention_days: u64,
    /// When `true`, routing prefers a stricter "proof" profile (deterministic
    /// model selection, conservative backups). Auto-enabled when
    /// `AppConfig.upstream_key_source.users_only()` returns true.
    pub proof_profile: bool,
}

#[derive(Clone, Debug)]
pub struct RuntimeSettings {
    pub worker_threads: usize,
    pub spawned_worker_threads: usize,
}

#[derive(Clone, Debug)]
pub struct ScalingSettings {
    pub max_instances: usize,
    pub spawn_batch_limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceRole {
    Main,
    Spawned,
}

impl InstanceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Spawned => "spawned",
        }
    }
}

impl FromStr for InstanceRole {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "main" => Ok(Self::Main),
            "spawned" => Ok(Self::Spawned),
            _ => bail!("invalid instance role {value:?}; expected main or spawned"),
        }
    }
}

/// How jnoccio-fusion sources upstream provider API keys.
///
/// `ConfigEnv` is the legacy single-pool path: keys come from `.env.jnoccio` /
/// process env, one slot per model. `UsersPool` is the multi-tenant path: each
/// `~/.jekko/users/<id>/llm.env` becomes an independent slot, and a model entry
/// fans out into one routable slot per user that has the provider key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamKeySource {
    #[default]
    ConfigEnv,
    UsersPool,
}

impl UpstreamKeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfigEnv => "config_env",
            Self::UsersPool => "users_pool",
        }
    }

    pub fn users_only(self) -> bool {
        matches!(self, Self::UsersPool)
    }
}

impl FromStr for UpstreamKeySource {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "config_env" => Ok(Self::ConfigEnv),
            "users_pool" => Ok(Self::UsersPool),
            _ => bail!("invalid upstream_key_source {value:?}; expected config_env or users_pool"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Registry {
    pub schema_version: u64,
    pub models: Vec<ModelEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub display_name: String,
    pub api: ModelApi,
    pub env: ModelEnv,
    pub signup_url: String,
    pub limits: ModelLimits,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub capabilities: ModelCapabilities,
    pub score: ModelScore,
    pub routing: ModelRouting,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelApi {
    pub style: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_param: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelEnv {
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub struct EnvResolution {
    pub value: String,
    pub missing_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelLimits {
    pub rpm: Option<u64>,
    pub rpd: Option<u64>,
    pub rpd_after_10_usd_credits: Option<u64>,
    pub source_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub tools: bool,
    pub reasoning: bool,
    pub openai_compatible: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelScore {
    pub power: u64,
    pub free_quota: u64,
    pub reliability: u64,
    pub integration: u64,
    pub latency: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelRouting {
    pub enabled: bool,
    pub roles: Vec<String>,
    pub exploration_floor: f64,
    pub cooldown_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ResolvedModel {
    pub entry: ModelEntry,
    pub visible_id: String,
    /// Routing slot identifier. `"{provider}/{id}"` for ConfigEnv; gains a
    /// `"@{user_id}"` suffix when fanned out for `UsersPool`.
    pub route_slot_id: String,
    /// Upstream model id sent to the provider (e.g. `"gpt-4o"`). Distinct from
    /// `visible_id`, which is the router-facing slot id.
    pub upstream_model_id: String,
    /// User id this slot is bound to under the multi-user pool. `None` for
    /// `ConfigEnv` (single global pool).
    pub credential_user_id: Option<String>,
    /// Env var name the slot's API key was read from (e.g.
    /// `"OPENROUTER_API_KEY"`).
    pub credential_env_name: String,
    pub key_source: UpstreamKeySource,
    pub api_key: Option<String>,
    pub key_present: bool,
    pub base_url: String,
    pub base_url_missing_keys: Vec<String>,
}

impl ResolvedModel {
    pub fn readiness_status(&self) -> &'static str {
        if !self.key_present {
            return "missing_key";
        }
        if !self.base_url_missing_keys.is_empty() || self.base_url.trim().is_empty() {
            return "incomplete_env";
        }
        "ready"
    }

    pub fn is_ready(&self) -> bool {
        self.readiness_status() == "ready"
    }
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub config_path: PathBuf,
    pub env_path: PathBuf,
    pub root: PathBuf,
    pub server: ServerConfig,
    pub registry: Registry,
    pub env: HashMap<String, String>,
    pub bind: String,
    pub database: PathBuf,
    pub receipts_dir: PathBuf,
    pub visible_model_id: String,
    pub provider_id: String,
    pub routing: RoutingDefaults,
    pub runtime: RuntimeSettings,
    pub scaling: ScalingSettings,
    pub instance_role: InstanceRole,
    pub worker_threads: usize,
    /// Where upstream API keys come from. Defaults to `ConfigEnv` (legacy
    /// single-pool). When set to `UsersPool`, `resolve_models` fans out one
    /// slot per `~/.jekko/users/<id>/llm.env` that has the provider key.
    pub upstream_key_source: UpstreamKeySource,
    /// Optional bearer token for the `/rpc` JSON-RPC endpoint.
    /// Resolved from `core_token` in server.json or `JNOCCIO_CORE_TOKEN` env var.
    pub core_token: Option<String>,
}

pub fn load_app_config(
    config_path: impl AsRef<Path>,
    env_override: Option<&Path>,
) -> Result<AppConfig> {
    let config_path = canonicalize_path(config_path.as_ref())?;
    let root = resolve_config_root(&config_path)?;
    let text = fs::read_to_string(&config_path)
        .with_context(|| format!("read {}", config_path.display()))?;
    let server: ServerConfig = serde_json::from_str(&strip_jsonc(&text))
        .with_context(|| format!("parse {}", config_path.display()))?;
    let env_path = resolve_configured_path(
        &root,
        env_override,
        server.env_file.as_deref(),
        ".env.jnoccio",
    );
    let mut env = std::env::vars().collect::<HashMap<_, _>>();
    if env_path.exists() {
        env.extend(parse_env_file(
            &fs::read_to_string(&env_path)
                .with_context(|| format!("read {}", env_path.display()))?,
        ));
    }

    let models_path =
        resolve_configured_path(&root, None, server.models_file.as_deref(), "models.json");
    let registry_text = fs::read_to_string(&models_path)
        .with_context(|| format!("read {}", models_path.display()))?;
    let registry: Registry = serde_json::from_str(&strip_jsonc(&registry_text))
        .with_context(|| format!("parse {}", models_path.display()))?;
    validate_registry(&registry)?;

    let visible_model_id =
        resolve_configured_string(server.model.as_deref(), "jnoccio/jnoccio-fusion");
    let provider_id = resolve_configured_string(server.provider.as_deref(), "jnoccio");
    let bind = resolve_configured_string(server.bind.as_deref(), "127.0.0.1:4317");
    let database = resolve_configured_path(
        &root,
        None,
        server.database.as_deref(),
        "state/jnoccio.sqlite",
    );
    let receipts_dir =
        resolve_configured_path(&root, None, server.receipts_dir.as_deref(), "receipts");
    let routing = RoutingDefaults::from_config(server.routing.as_ref());
    let runtime = RuntimeSettings::from_config(server.runtime.as_ref())?;
    let scaling = ScalingSettings::from_config(server.scaling.as_ref())?;
    let worker_threads = runtime.worker_threads;

    // Resolve the RPC bearer token: env var takes precedence over config file.
    let core_token = std::env::var("JNOCCIO_CORE_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| server.core_token.clone().filter(|s| !s.trim().is_empty()));

    // Upstream key source: env var > server.json > default ConfigEnv.
    let upstream_key_source = match std::env::var("JNOCCIO_UPSTREAM_KEY_SOURCE").ok() {
        Some(value) if !value.trim().is_empty() => UpstreamKeySource::from_str(value.trim())?,
        _ => server.upstream_key_source.unwrap_or_default(),
    };

    Ok(AppConfig {
        config_path,
        env_path,
        root,
        server,
        registry,
        env,
        bind,
        database,
        receipts_dir,
        visible_model_id,
        provider_id,
        routing,
        runtime,
        scaling,
        instance_role: InstanceRole::Main,
        worker_threads,
        upstream_key_source,
        core_token,
    })
}

impl RoutingDefaults {
    pub fn from_config(config: Option<&ServerRoutingConfig>) -> Self {
        Self {
            fusion_sample_rate: config
                .and_then(|config| config.fusion_sample_rate)
                .unwrap_or(0.10)
                .clamp(0.0, 1.0),
            fast_backup_count: config
                .and_then(|config| config.fast_backup_count)
                .unwrap_or(2),
            event_retention_rows: config
                .and_then(|config| config.event_retention_rows)
                .unwrap_or(50_000),
            minute_bucket_retention_days: config
                .and_then(|config| config.minute_bucket_retention_days)
                .unwrap_or(30),
            proof_profile: config
                .and_then(|config| config.proof_profile)
                .unwrap_or(false),
        }
    }
}

impl RuntimeSettings {
    pub fn from_config(config: Option<&ServerRuntimeConfig>) -> Result<Self> {
        let worker_threads = bounded_usize(
            config.and_then(|config| config.worker_threads),
            "runtime.worker_threads",
            MAX_RUNTIME_WORKER_THREADS,
            default_worker_threads(),
        )?;
        let spawned_worker_threads = bounded_usize(
            config.and_then(|config| config.spawned_worker_threads),
            "runtime.spawned_worker_threads",
            MAX_RUNTIME_WORKER_THREADS,
            2,
        )?;
        Ok(Self {
            worker_threads,
            spawned_worker_threads,
        })
    }

    pub fn worker_threads_for_role(&self, role: InstanceRole) -> usize {
        match role {
            InstanceRole::Main => self.worker_threads,
            InstanceRole::Spawned => self.spawned_worker_threads,
        }
    }
}

impl ScalingSettings {
    pub fn from_config(config: Option<&ServerScalingConfig>) -> Result<Self> {
        let max_instances = bounded_usize(
            config.and_then(|config| config.max_instances),
            "scaling.max_instances",
            MAX_MANAGED_INSTANCES,
            MAX_MANAGED_INSTANCES,
        )?;
        let spawn_batch_limit = bounded_usize(
            config.and_then(|config| config.spawn_batch_limit),
            "scaling.spawn_batch_limit",
            MAX_MANAGED_INSTANCES,
            DEFAULT_SPAWN_BATCH_LIMIT,
        )?;
        Ok(Self {
            max_instances,
            spawn_batch_limit,
        })
    }
}

pub fn resolve_models(config: &AppConfig) -> Result<Vec<ResolvedModel>> {
    match config.upstream_key_source {
        UpstreamKeySource::ConfigEnv => resolve_models_config_env(config),
        UpstreamKeySource::UsersPool => resolve_models_users_pool(config),
    }
}

fn resolve_models_config_env(config: &AppConfig) -> Result<Vec<ResolvedModel>> {
    config
        .registry
        .models
        .iter()
        .map(|entry| {
            let api_key = config
                .env
                .get(&entry.env.api_key)
                .cloned()
                .filter(|value| !value.trim().is_empty());
            let key_present = api_key.is_some();
            let base_url = substitute_env_report(&entry.api.base_url, &config.env);
            let visible_id = format!("{}/{}", entry.provider, entry.id);
            Ok(ResolvedModel {
                visible_id: visible_id.clone(),
                // ConfigEnv path: one slot per model entry, no user binding.
                route_slot_id: visible_id,
                upstream_model_id: entry.model.clone(),
                credential_user_id: None,
                credential_env_name: entry.env.api_key.clone(),
                key_source: UpstreamKeySource::ConfigEnv,
                entry: entry.clone(),
                api_key,
                key_present,
                base_url: base_url.value,
                base_url_missing_keys: base_url.missing_keys,
            })
        })
        .collect()
}

/// Fan one [`ResolvedModel`] per [`zyal_key_pool::UserKeySlot`] that matches
/// the model entry's provider key env name. Multiple users with the same
/// provider become independent routing slots; unmatched entries still emit a
/// single slot (with no key) so downstream readiness reporting still surfaces
/// missing-key state.
fn resolve_models_users_pool(config: &AppConfig) -> Result<Vec<ResolvedModel>> {
    let users_root = resolve_users_root();
    let slots = zyal_key_pool::KeyPool::scan(&users_root)?;
    let mut resolved = Vec::with_capacity(config.registry.models.len());
    for entry in &config.registry.models {
        let base_url = substitute_env_report(&entry.api.base_url, &config.env);
        let visible_id = format!("{}/{}", entry.provider, entry.id);
        let matching: Vec<&zyal_key_pool::UserKeySlot> = slots
            .iter()
            .filter(|slot| slot.env_name == entry.env.api_key)
            .collect();
        if matching.is_empty() {
            // Surface as a single missing-key slot so health/diagnostics stay
            // honest even when no user has a key for the provider yet.
            resolved.push(ResolvedModel {
                visible_id: visible_id.clone(),
                route_slot_id: visible_id,
                upstream_model_id: entry.model.clone(),
                credential_user_id: None,
                credential_env_name: entry.env.api_key.clone(),
                key_source: UpstreamKeySource::UsersPool,
                entry: entry.clone(),
                api_key: None,
                key_present: false,
                base_url: base_url.value.clone(),
                base_url_missing_keys: base_url.missing_keys.clone(),
            });
            continue;
        }
        for slot in matching {
            let route_slot_id = format!("{}/{}@{}", entry.provider, entry.id, slot.user_id);
            let value_present = !slot.value.trim().is_empty();
            resolved.push(ResolvedModel {
                visible_id: visible_id.clone(),
                route_slot_id,
                upstream_model_id: entry.model.clone(),
                credential_user_id: Some(slot.user_id.clone()),
                credential_env_name: slot.env_name.clone(),
                key_source: UpstreamKeySource::UsersPool,
                entry: entry.clone(),
                api_key: if value_present {
                    Some(slot.value.clone())
                } else {
                    None
                },
                key_present: value_present,
                base_url: base_url.value.clone(),
                base_url_missing_keys: base_url.missing_keys.clone(),
            });
        }
    }
    Ok(resolved)
}

/// Resolve the canonical `~/.jekko/users/` root for the multi-user pool.
/// Honors `JEKKO_HOME` (for tests + isolated installs); otherwise falls back
/// to `$HOME/.jekko/users/`. When neither is available, returns an empty path
/// so `KeyPool::scan` reports "no slots" without panicking.
fn resolve_users_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("JEKKO_HOME") {
        let path = PathBuf::from(custom);
        if !path.as_os_str().is_empty() {
            return path.join("users");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".jekko").join("users");
    }
    PathBuf::new()
}

fn validate_registry(registry: &Registry) -> Result<()> {
    if registry.schema_version != 1 {
        bail!(
            "unsupported models schema version {}",
            registry.schema_version
        )
    }
    if registry.models.is_empty() {
        bail!("registry has no models")
    }
    let mut ids = HashMap::new();
    for model in &registry.models {
        if model.id.trim().is_empty() {
            bail!("registry contains model with empty id")
        }
        if model.provider.trim().is_empty() {
            bail!("registry contains model {} with empty provider", model.id)
        }
        if model.model.trim().is_empty() {
            bail!(
                "registry contains model {} with empty upstream model id",
                model.id
            )
        }
        if ids.insert(model.id.clone(), true).is_some() {
            bail!("registry contains duplicate model id {}", model.id)
        }
    }
    Ok(())
}

fn resolve_relative(root: &Path, value: &str) -> PathBuf {
    resolve_path(root, Path::new(value))
}

fn resolve_configured_path(
    root: &Path,
    override_path: Option<&Path>,
    configured: Option<&str>,
    default: &str,
) -> PathBuf {
    if let Some(path) = override_path {
        return resolve_path(root, path);
    }
    resolve_relative(root, resolve_or_default(configured, default))
}

fn resolve_configured_string(value: Option<&str>, default: &'static str) -> String {
    resolve_or_default(value, default).to_string()
}

fn resolve_config_root(config_path: &Path) -> Result<PathBuf> {
    match config_path.parent().and_then(Path::parent) {
        Some(root) => Ok(root.to_path_buf()),
        None => bail!("config path must be nested under a directory"),
    }
}

fn canonicalize_path(path: &Path) -> Result<PathBuf> {
    Ok(resolve_path(&std::env::current_dir()?, path))
}

fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut string_quote = '\0';
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == string_quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = true;
                string_quote = ch;
                out.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                let _ = chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                    } else if next == '\r' {
                        out.push('\r');
                    }
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

fn resolve_path(base: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        return value.to_path_buf();
    }
    base.join(value)
}

fn resolve_or_default<T>(value: Option<T>, default: T) -> T {
    value.unwrap_or(default)
}

fn bounded_usize(value: Option<usize>, label: &str, cap: usize, default: usize) -> Result<usize> {
    match value {
        Some(0) => bail!("{label} must be at least 1"),
        Some(value) => Ok(value.min(cap)),
        None => Ok(default),
    }
}

fn parse_env_file(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            let value = strip_quotes(value.trim());
            Some((key.to_string(), value))
        })
        .collect()
}

fn strip_quotes(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes.first() == Some(&b'"') && bytes.last() == Some(&b'"'))
            || (bytes.first() == Some(&b'\'') && bytes.last() == Some(&b'\''))
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

pub fn substitute_env(input: &str, env: &HashMap<String, String>) -> String {
    substitute_env_report(input, env).value
}

pub fn substitute_env_report(input: &str, env: &HashMap<String, String>) -> EnvResolution {
    let mut out = String::with_capacity(input.len());
    let mut missing_keys = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'{') {
            let _ = chars.next();
            let mut key = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                key.push(next);
            }
            if let Some(value) = env.get(&key).filter(|value| !value.trim().is_empty()) {
                out.push_str(value);
            } else {
                push_missing_key(&mut missing_keys, &key);
            }
            continue;
        }
        let mut key = String::new();
        while let Some(next) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || next == '_' {
                key.push(next);
                let _ = chars.next();
            } else {
                break;
            }
        }
        if key.is_empty() {
            out.push('$');
            continue;
        }
        if let Some(value) = env.get(&key).filter(|value| !value.trim().is_empty()) {
            out.push_str(value);
        } else {
            push_missing_key(&mut missing_keys, &key);
        }
    }
    EnvResolution {
        value: out,
        missing_keys,
    }
}

fn push_missing_key(missing_keys: &mut Vec<String>, key: &str) {
    if missing_keys.iter().any(|missing| missing == key) {
        return;
    }
    missing_keys.push(key.to_string());
}

fn default_worker_threads() -> usize {
    let available = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(2);
    if available == 1 {
        1
    } else {
        available.clamp(2, MAX_RUNTIME_WORKER_THREADS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cloudflare_model_entry(disabled_reason: Option<&str>) -> ModelEntry {
        serde_json::from_value(json!({
            "id": "test",
            "provider": "cloudflare",
            "model": "@cf/test/model",
            "display_name": "Test",
            "api": {
                "style": "cloudflare_openai",
                "base_url": "https://api.cloudflare.com/client/v4/accounts/${CLOUDFLARE_ACCOUNT_ID}/ai/v1"
            },
            "env": {
                "api_key": "CLOUDFLARE_API_TOKEN"
            },
            "signup_url": "https://example.com",
            "limits": {
                "rpm": null,
                "rpd": null,
                "rpd_after_10_usd_credits": null,
                "source_url": null
            },
            "context_window": 1024,
            "max_output_tokens": 128,
            "capabilities": {
                "streaming": true,
                "tools": true,
                "reasoning": false,
                "openai_compatible": true
            },
            "score": {
                "power": 1,
                "free_quota": 1,
                "reliability": 1,
                "integration": 1,
                "latency": 1
            },
            "routing": {
                "enabled": true,
                "roles": ["draft"],
                "exploration_floor": 0.1,
                "cooldown_seconds": 1,
                "disabled_reason": disabled_reason
            }
        }))
        .unwrap()
    }

    #[test]
    fn substitutes_env_variables() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        assert_eq!(substitute_env("x-${FOO}-y", &env), "x-bar-y");
    }

    #[test]
    fn reports_missing_env_variables() {
        let env = HashMap::new();
        let resolution = substitute_env_report("x-${FOO}-y-$BAR", &env);
        assert_eq!(resolution.value, "x--y-");
        assert_eq!(
            resolution.missing_keys,
            vec!["FOO".to_string(), "BAR".to_string()]
        );
    }

    #[test]
    fn parses_env_file() {
        let env = parse_env_file("A=1\n# comment\nexport B='two'\n");
        assert_eq!(env.get("A").map(String::as_str), Some("1"));
        assert_eq!(env.get("B").map(String::as_str), Some("two"));
    }

    #[test]
    fn strips_jsonc_comments_without_touching_string_literals() {
        let input = r#"
        {
          // inline comment
          "url": "https://example.com/path//value",
          /* block comment */
          "pattern": "keep /* and // inside string",
          "nested": {
            "text": "quote with \\\" // preserved"
          }
        }
        "#;
        let _: ServerConfig = serde_json::from_str(&strip_jsonc(input)).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&strip_jsonc(input)).unwrap();
        assert_eq!(
            value["url"].as_str(),
            Some("https://example.com/path//value")
        );
        assert_eq!(
            value["pattern"].as_str(),
            Some("keep /* and // inside string")
        );
        assert_eq!(
            value["nested"]["text"].as_str(),
            Some("quote with \\\" // preserved")
        );
    }

    #[test]
    fn load_app_config_accepts_jsonc_bundle_defaults_and_clamps_limits() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let bundle_dir = root.join("jnoccio-fusion");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("config/models.json"),
            bundle_dir.join("models.json"),
        )
        .unwrap();
        std::fs::write(
            bundle_dir.join("server.jsonc"),
            r#"
            {
              "bind": "127.0.0.1:4317",
              "env_file": ".env.jnoccio",
              "models_file": "jnoccio-fusion/models.json",
              "routing": {
                "fast_backup_count": 2,
                "event_retention_rows": 50000,
                "minute_bucket_retention_days": 30
              },
              "runtime": {
                "spawned_worker_threads": 64
              },
              "scaling": {
                "max_instances": 64,
                "spawn_batch_limit": 64
              }
            }
            "#,
        )
        .unwrap();

        let config = load_app_config(bundle_dir.join("server.jsonc"), None).unwrap();
        assert_eq!(config.routing.fusion_sample_rate, 0.10);
        assert_eq!(config.routing.fast_backup_count, 2);
        assert_eq!(config.routing.event_retention_rows, 50000);
        assert_eq!(config.routing.minute_bucket_retention_days, 30);
        assert_eq!(
            config.runtime.spawned_worker_threads,
            MAX_RUNTIME_WORKER_THREADS
        );
        assert_eq!(config.scaling.max_instances, MAX_MANAGED_INSTANCES);
        assert_eq!(config.scaling.spawn_batch_limit, MAX_MANAGED_INSTANCES);
    }

    #[test]
    fn runtime_and_scaling_defaults_are_bounded() {
        let runtime = RuntimeSettings::from_config(None).unwrap();
        assert_eq!(runtime.worker_threads, default_worker_threads());
        assert_eq!(runtime.spawned_worker_threads, 2);
        assert!((1..=MAX_RUNTIME_WORKER_THREADS).contains(&runtime.worker_threads));
        assert!((1..=MAX_RUNTIME_WORKER_THREADS).contains(&runtime.spawned_worker_threads));

        let scaling = ScalingSettings::from_config(None).unwrap();
        assert_eq!(scaling.max_instances, MAX_MANAGED_INSTANCES);
        assert_eq!(scaling.spawn_batch_limit, DEFAULT_SPAWN_BATCH_LIMIT);
    }

    #[test]
    fn runtime_and_scaling_reject_zero_values() {
        assert!(
            RuntimeSettings::from_config(Some(&ServerRuntimeConfig {
                worker_threads: Some(0),
                spawned_worker_threads: None,
            }))
            .is_err()
        );
        assert!(
            RuntimeSettings::from_config(Some(&ServerRuntimeConfig {
                worker_threads: None,
                spawned_worker_threads: Some(0),
            }))
            .is_err()
        );
        assert!(
            ScalingSettings::from_config(Some(&ServerScalingConfig {
                max_instances: Some(0),
                spawn_batch_limit: None,
            }))
            .is_err()
        );
        assert!(
            ScalingSettings::from_config(Some(&ServerScalingConfig {
                max_instances: None,
                spawn_batch_limit: Some(0),
            }))
            .is_err()
        );
    }

    #[test]
    fn runtime_and_scaling_clamp_values_above_hard_cap() {
        let runtime = RuntimeSettings::from_config(Some(&ServerRuntimeConfig {
            worker_threads: Some(64),
            spawned_worker_threads: Some(64),
        }))
        .unwrap();
        assert_eq!(runtime.worker_threads, MAX_RUNTIME_WORKER_THREADS);
        assert_eq!(runtime.spawned_worker_threads, MAX_RUNTIME_WORKER_THREADS);

        let scaling = ScalingSettings::from_config(Some(&ServerScalingConfig {
            max_instances: Some(64),
            spawn_batch_limit: Some(64),
        }))
        .unwrap();
        assert_eq!(scaling.max_instances, MAX_MANAGED_INSTANCES);
        assert_eq!(scaling.spawn_batch_limit, MAX_MANAGED_INSTANCES);
    }

    #[test]
    fn readiness_status_marks_incomplete_env() {
        let entry = cloudflare_model_entry(None);
        let model = ResolvedModel {
            entry,
            visible_id: "cloudflare/test".to_string(),
            route_slot_id: "cloudflare/test".to_string(),
            upstream_model_id: "cloudflare/test".to_string(),
            credential_user_id: None,
            credential_env_name: "CLOUDFLARE_API_TOKEN".to_string(),
            key_source: UpstreamKeySource::ConfigEnv,
            api_key: Some("token".to_string()),
            key_present: true,
            base_url: "https://api.cloudflare.com/client/v4/accounts//ai/v1".to_string(),
            base_url_missing_keys: vec!["CLOUDFLARE_ACCOUNT_ID".to_string()],
        };

        assert_eq!(model.readiness_status(), "incomplete_env");
        assert!(!model.is_ready());
    }

    #[test]
    fn parses_disabled_reason() {
        let entry = cloudflare_model_entry(Some("billing required"));

        assert_eq!(
            entry.routing.disabled_reason.as_deref(),
            Some("billing required")
        );
    }

    #[test]
    fn resolve_config_root_requires_two_parent_directories() {
        let nested = Path::new("/tmp/workspace/project/config.json");
        let root = resolve_config_root(nested).unwrap();
        assert_eq!(root, PathBuf::from("/tmp/workspace"));

        let shallow = Path::new("/config.json");
        assert!(resolve_config_root(shallow).is_err());
    }

    #[test]
    fn resolve_configured_path_prefers_override_and_falls_back_to_config() {
        let root = Path::new("/tmp/workspace");
        let override_path = Path::new("/var/tmp/env.jnoccio");
        assert_eq!(
            resolve_configured_path(
                root,
                Some(override_path),
                Some("ignored.env"),
                ".env.jnoccio"
            ),
            PathBuf::from("/var/tmp/env.jnoccio"),
        );
        assert_eq!(
            resolve_configured_path(root, None, Some("configs/models.json"), "models.json"),
            PathBuf::from("/tmp/workspace/configs/models.json"),
        );
        assert_eq!(
            resolve_configured_path(root, None, None, "models.json"),
            PathBuf::from("/tmp/workspace/models.json"),
        );
    }

    #[test]
    fn resolve_configured_string_uses_default_when_missing() {
        assert_eq!(
            resolve_configured_string(Some("custom"), "default"),
            "custom",
        );
        assert_eq!(resolve_configured_string(None, "default"), "default");
    }
}
