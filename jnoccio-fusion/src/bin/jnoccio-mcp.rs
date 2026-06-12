use anyhow::{Context, Result, bail};
use fs2::FileExt;
use jnoccio_fusion::cli::parse_config_env_args;
use jnoccio_fusion::config::load_app_config;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() -> Result<()> {
    let parsed = parse_config_env_args(
        std::env::args().skip(1).collect(),
        "jnoccio-mcp --config <path> --env-file <path> --ensure-server",
    );
    let ensure_server = parsed.remaining.iter().any(|arg| arg == "--ensure-server");
    let config = load_app_config(&parsed.config_path, parsed.env_path.as_deref())?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("build http client")?;
    let base_url = format!("http://{}", config.bind);
    if ensure_server {
        ensure_main_server(&config, &client, &base_url)?;
    }
    proxy_stdio(&client, &format!("{base_url}/mcp"))?;
    Ok(())
}

fn ensure_main_server(
    config: &jnoccio_fusion::AppConfig,
    client: &reqwest::blocking::Client,
    base_url: &str,
) -> Result<()> {
    match probe_health(client, base_url) {
        HealthProbe::Healthy(health) if is_jnoccio_health(&health, config) => return Ok(()),
        HealthProbe::Occupied(diagnostic) => {
            bail!(
                "port {} is occupied by a non-Jnoccio process: {diagnostic}",
                config.bind
            )
        }
        HealthProbe::Healthy(_) => {
            bail!("port {} is occupied by a non-Jnoccio process", config.bind)
        }
        HealthProbe::Missing => {}
    }

    let lock_path = config.root.join("state/jnoccio-mcp.lock");
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("open {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("lock {}", lock_path.display()))?;

    match probe_health(client, base_url) {
        HealthProbe::Healthy(health) if is_jnoccio_health(&health, config) => return Ok(()),
        HealthProbe::Occupied(diagnostic) => {
            bail!(
                "port {} is occupied by a non-Jnoccio process: {diagnostic}",
                config.bind
            )
        }
        HealthProbe::Healthy(_) => {
            bail!("port {} is occupied by a non-Jnoccio process", config.bind)
        }
        HealthProbe::Missing => {}
    }

    let mut child = spawn_main_server(config)?;
    if wait_for_health(client, base_url, config) {
        return Ok(());
    }
    let _ = child.kill();
    let _ = child.wait();
    bail!(
        "main Jnoccio server did not become healthy on {}",
        config.bind
    )
}

fn spawn_main_server(config: &jnoccio_fusion::AppConfig) -> Result<std::process::Child> {
    let binary = main_binary_path(config)?;
    Command::new(binary)
        .arg("--config")
        .arg(&config.config_path)
        .arg("--env-file")
        .arg(&config.env_path)
        .arg("--bind")
        .arg(&config.bind)
        .arg("--instance-role")
        .arg("main")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {}", config.bind))
}

fn main_binary_path(config: &jnoccio_fusion::AppConfig) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("JNOCCIO_FUSION_BINARY") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe().context("current exe")?;
    if current.file_name().and_then(|name| name.to_str()) == Some("jnoccio-fusion") {
        return Ok(current);
    }
    if let Some(parent) = current.parent() {
        let candidate = parent.join("jnoccio-fusion");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let candidate = config.root.join("target/debug/jnoccio-fusion");
    if candidate.exists() {
        return Ok(candidate);
    }
    Ok(current.with_file_name("jnoccio-fusion"))
}

enum HealthProbe {
    Healthy(Value),
    Occupied(String),
    Missing,
}

fn probe_health(client: &reqwest::blocking::Client, base_url: &str) -> HealthProbe {
    let response = match client.get(format!("{base_url}/health")).send() {
        Ok(response) => response,
        Err(err) => {
            if err.is_connect() || err.is_timeout() {
                return HealthProbe::Missing;
            }
            return HealthProbe::Occupied(err.to_string());
        }
    };

    let status = response.status();
    let text = match response.text() {
        Ok(text) => text,
        Err(err) => {
            return HealthProbe::Occupied(format!("failed reading health response body: {err}"));
        }
    };
    let parsed = serde_json::from_str::<Value>(&text).ok();
    if let Some(value) = parsed {
        return HealthProbe::Healthy(value);
    }
    HealthProbe::Occupied(format!("unexpected health response {status}: {text}"))
}

fn is_jnoccio_health(value: &Value, config: &jnoccio_fusion::AppConfig) -> bool {
    value.get("provider").and_then(Value::as_str) == Some(config.provider_id.as_str())
        && value.get("visible_model").and_then(Value::as_str)
            == Some(config.visible_model_id.as_str())
}

fn wait_for_health(
    client: &reqwest::blocking::Client,
    base_url: &str,
    config: &jnoccio_fusion::AppConfig,
) -> bool {
    for _ in 0..80 {
        if let HealthProbe::Healthy(health) = probe_health(client, base_url)
            && is_jnoccio_health(&health, config)
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn proxy_stdio(client: &reqwest::blocking::Client, url: &str) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let mut protocol_version = String::from("2025-11-25");
    for line in BufReader::new(stdin.lock()).lines() {
        let line = line.context("read stdio")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line)
            && let Some(version) = value
                .get("params")
                .and_then(|params| params.get("protocolVersion"))
                .and_then(Value::as_str)
        {
            protocol_version = version.to_string();
        }
        let method = method_from_line(&line);
        let name = name_from_line(&line);
        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", &protocol_version);
        if let Some(method) = method.as_deref() {
            request = request.header("MCP-Method", method);
        }
        if let Some(name) = name.as_deref() {
            request = request.header("MCP-Name", name);
        }
        let response = request
            .body(line.clone())
            .send()
            .context("proxy mcp request")?;
        if response.status() == reqwest::StatusCode::ACCEPTED {
            continue;
        }
        let text = response.text().context("read mcp response")?;
        if text.trim().is_empty() {
            continue;
        }
        stdout
            .write_all(text.as_bytes())
            .context("write mcp response")?;
        if !text.ends_with('\n') {
            stdout.write_all(b"\n").context("write newline")?;
        }
        stdout.flush().context("flush mcp response")?;
    }
    Ok(())
}

fn method_from_line(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line).ok().and_then(|value| {
        value
            .get("method")
            .and_then(Value::as_str)
            .map(|text| text.to_string())
    })
}

fn name_from_line(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    if value.get("method").and_then(Value::as_str) == Some("tools/call") {
        return value
            .get("params")
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .map(|text| text.to_string());
    }
    if value.get("method").and_then(Value::as_str) == Some("resources/read") {
        return value
            .get("params")
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
            .map(|text| text.to_string());
    }
    if value.get("method").and_then(Value::as_str) == Some("prompts/get") {
        return value
            .get("params")
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .map(|text| text.to_string());
    }
    None
}
