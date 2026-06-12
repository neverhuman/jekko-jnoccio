//! Phase E2 contract test for the `/v1/embeddings` route.
//!
//! When jnoccio-fusion is configured with no embedding-capable model
//! (the common case — most deployments only wire chat-completion models),
//! POSTing to `/v1/embeddings` must still return a usable response so
//! cold-start runs and the jankurai-runner memory module work. The
//! deterministic sha256-derived fake satisfies that contract.

use anyhow::{Context, Result, bail};
use jnoccio_fusion::{config::load_app_config, fusion::Gateway, router};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn embeddings_endpoint_returns_deterministic_fake_when_no_embedder_configured() -> Result<()>
{
    let interim = TempDir::new().context("create tempdir")?;
    let bind = free_bind().await?;
    let config_path = write_config(interim.path(), &bind).await?;
    let base_url = start_gateway(&config_path).await?;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/embeddings"))
        .json(&json!({
            "model": "text-embedding-3-small",
            "input": "hello world"
        }))
        .send()
        .await
        .context("post /v1/embeddings")?;
    assert_eq!(response.status().as_u16(), 200);
    let body: Value = response.json().await.context("decode response")?;

    assert_eq!(body["object"], "list");
    assert_eq!(body["model"], "jnoccio/fake-embeddings");
    let data = body["data"]
        .as_array()
        .context("data field should be an array")?;
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["object"], "embedding");
    assert_eq!(data[0]["index"].as_u64(), Some(0));
    let embedding = data[0]["embedding"]
        .as_array()
        .context("embedding field should be an array")?;
    assert_eq!(embedding.len(), 1536);
    for value in embedding {
        let f = value
            .as_f64()
            .context("embedding value should be a number")?;
        assert!(
            (-1.0..=1.0).contains(&f),
            "embedding value out of range: {f}"
        );
    }
    assert!(body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) >= 1);
    Ok(())
}

#[tokio::test]
async fn embeddings_endpoint_is_deterministic_for_identical_input() -> Result<()> {
    let interim = TempDir::new().context("create tempdir")?;
    let bind = free_bind().await?;
    let config_path = write_config(interim.path(), &bind).await?;
    let base_url = start_gateway(&config_path).await?;
    let client = reqwest::Client::new();

    let response_a = client
        .post(format!("{base_url}/v1/embeddings"))
        .json(&json!({ "model": "any", "input": "deterministic check" }))
        .send()
        .await?;
    let response_b = client
        .post(format!("{base_url}/v1/embeddings"))
        .json(&json!({ "model": "any", "input": "deterministic check" }))
        .send()
        .await?;
    let body_a: Value = response_a.json().await?;
    let body_b: Value = response_b.json().await?;
    assert_eq!(
        body_a["data"][0]["embedding"],
        body_b["data"][0]["embedding"]
    );
    Ok(())
}

#[tokio::test]
async fn embeddings_endpoint_accepts_batch_input() -> Result<()> {
    let interim = TempDir::new().context("create tempdir")?;
    let bind = free_bind().await?;
    let config_path = write_config(interim.path(), &bind).await?;
    let base_url = start_gateway(&config_path).await?;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base_url}/v1/embeddings"))
        .json(&json!({
            "model": "any",
            "input": ["first", "second", "third"]
        }))
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 200);
    let body: Value = response.json().await?;
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    for (idx, item) in data.iter().enumerate() {
        assert_eq!(item["index"].as_u64(), Some(idx as u64));
    }
    Ok(())
}

async fn start_gateway(config_path: &Path) -> Result<String> {
    let config = load_app_config(config_path, None)?;
    let gateway = Arc::new(Gateway::new(config)?);
    let listener = TcpListener::bind(&gateway.config.bind).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, router::router(gateway)).await;
    });
    wait_for_http(&format!("http://{addr}/health")).await?;
    Ok(format!("http://{addr}"))
}

async fn wait_for_http(url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    for _ in 0..80 {
        if client.get(url).send().await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
    }
    bail!("timed out waiting for {url}")
}

async fn free_bind() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok(addr.to_string())
}

async fn write_config(root: &Path, bind: &str) -> Result<PathBuf> {
    fs::create_dir_all(root.join("config"))?;
    fs::create_dir_all(root.join("state"))?;
    fs::create_dir_all(root.join("receipts"))?;

    fs::write(
        root.join("config/models.json"),
        json!({
            "schema_version": 1,
            "models": [{
                "id": "local-model",
                "provider": "local",
                "model": "local-model",
                "display_name": "Local Model",
                "api": {
                    "style": "openai_chat",
                    "base_url": "http://127.0.0.1:1"
                },
                "env": { "api_key": "LOCAL_API_KEY" },
                "signup_url": "https://example.com",
                "limits": {
                    "rpm": null,
                    "rpd": null,
                    "rpd_after_10_usd_credits": null,
                    "source_url": null
                },
                "context_window": 8192,
                "max_output_tokens": 1024,
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
                    "roles": ["draft", "fusion"],
                    "exploration_floor": 0.1,
                    "cooldown_seconds": 1
                }
            }]
        })
        .to_string(),
    )?;

    fs::write(root.join(".env.jnoccio"), "LOCAL_API_KEY=test\n")?;

    let config_path = root.join("config/server.json");
    fs::write(
        &config_path,
        json!({
            "bind": bind,
            "database": "state/jnoccio.sqlite",
            "env_file": ".env.jnoccio",
            "models_file": "config/models.json",
            "receipts_dir": "receipts",
            "model": "jnoccio/jnoccio-fusion",
            "provider": "jnoccio",
            "routing": {
                "fusion_sample_rate": 0.0,
                "fast_backup_count": 1,
                "event_retention_rows": 1000,
                "minute_bucket_retention_days": 7
            },
            "scaling": {
                "max_instances": 10,
                "spawn_batch_limit": 5
            }
        })
        .to_string(),
    )?;

    Ok(config_path)
}
