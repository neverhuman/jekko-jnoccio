use anyhow::{Context, Result, bail};
use axum::{Json as AxumJson, Router as AxumRouter, routing::get};
use jnoccio_fusion::{config::load_app_config, fusion::Gateway, router};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};

static SPAWN_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn http_mcp_initialize_tools_chat_spawn_and_stop() -> Result<()> {
    let _spawn_lock = SPAWN_TEST_LOCK.lock().await;
    let interim = TempDir::new().context("create tempdir")?;
    let upstream = start_upstream("upstream answer").await?;
    let bind = free_bind().await?;
    let config_path = write_config(interim.path(), &bind, &upstream).await?;
    let (base_url, server) = start_gateway(&config_path).await?;
    let client = reqwest::Client::new();

    let initialize = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": { "name": "test", "version": "1.0.0" },
                "capabilities": {}
            }
        }),
    )
    .await?;
    assert_eq!(initialize["result"]["serverInfo"]["name"], "jnoccio");

    let list = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    )
    .await?;
    let tool_names = list["result"]["tools"]
        .as_array()
        .context("tools list should be an array")?
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(tool_names.contains(&"jnoccio_status"));
    assert!(tool_names.contains(&"jnoccio_chat"));
    assert!(tool_names.contains(&"jnoccio_spawn_instance"));
    assert!(tool_names.contains(&"jnoccio_search"));
    assert!(tool_names.contains(&"jnoccio_research"));
    assert!(tool_names.contains(&"jnoccio_extract"));

    let chat = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_chat",
                "arguments": {
                    "prompt": "Say hello in one short sentence.",
                    "max_tokens": 32
                }
            }
        }),
    )
    .await?;
    assert_eq!(
        chat["result"]["structuredContent"]["answer"],
        "upstream answer"
    );
    assert_eq!(
        chat["result"]["structuredContent"]["route"]["winner_model_id"],
        "local/local-model"
    );

    let spawn = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_spawn_instance",
                "arguments": {}
            }
        }),
    )
    .await?;
    let bind = spawn["result"]["structuredContent"]["instance"]["bind"]
        .as_str()
        .context("spawned instance bind missing")?
        .to_string();
    assert_eq!(spawn["result"]["structuredContent"]["instance_count"], 2);
    assert_eq!(spawn["result"]["structuredContent"]["max_instances"], 10);
    assert_eq!(
        spawn["result"]["structuredContent"]["available_instance_slots"],
        8
    );
    assert_eq!(spawn["result"]["structuredContent"]["role"], "main");
    let child_health = wait_for_http(&format!("http://{bind}/health")).await?;
    assert_eq!(child_health["provider"], "jnoccio");
    let child_status = wait_for_http(&format!("http://{bind}/v1/jnoccio/status")).await?;
    assert_eq!(
        child_status["database"],
        serde_json::Value::from(
            interim
                .path()
                .join("state/jnoccio.sqlite")
                .display()
                .to_string()
        )
    );

    let child_spawn_refusal = post_mcp(
        &client,
        &format!("http://{bind}"),
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_spawn_instance",
                "arguments": {}
            }
        }),
    )
    .await?;
    assert!(
        error_message(&child_spawn_refusal)
            .is_some_and(|message| message.contains("main Jnoccio instance")),
        "unexpected child spawn refusal: {child_spawn_refusal}"
    );

    let instances = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_instances",
                "arguments": {}
            }
        }),
    )
    .await?;
    assert!(
        instances["result"]["structuredContent"]["instances"]
            .as_array()
            .context("instances should be an array")?
            .iter()
            .any(|instance| instance["bind"].as_str() == Some(bind.as_str()))
    );
    assert_eq!(
        instances["result"]["structuredContent"]["instance_count"],
        2
    );
    assert_eq!(
        instances["result"]["structuredContent"]["max_instances"],
        10
    );
    assert_eq!(
        instances["result"]["structuredContent"]["available_instance_slots"],
        8
    );

    let instance_id = spawn["result"]["structuredContent"]["instance"]["id"]
        .as_str()
        .context("spawned instance id missing")?
        .to_string();
    let stopped = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_stop_instance",
                "arguments": {
                    "instance_id": instance_id
                }
            }
        }),
    )
    .await?;
    assert!(
        stopped["result"]["structuredContent"]["stopped"]["stopped"]
            .as_bool()
            .unwrap_or(false),
        "unexpected stop response: {stopped}"
    );

    let get = client.get(format!("{base_url}/mcp")).send().await?;
    assert_eq!(get.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);

    server.abort();
    Ok(())
}

#[tokio::test]
async fn http_mcp_search_research_and_extract_tools() -> Result<()> {
    let _spawn_lock = SPAWN_TEST_LOCK.lock().await;
    let interim = TempDir::new().context("create tempdir")?;
    let upstream = start_upstream("search answer").await?;
    let provider = start_search_fixture("Fixture search result").await?;
    let bind = free_bind().await?;
    let config_path = write_config(interim.path(), &bind, &upstream).await?;
    fs::write(
        interim.path().join(".env.jnoccio"),
        format!(
            "LOCAL_API_KEY=test\nSEARXNG_BASE_URL={}\nAGENT_SEARCH_ALLOW=searxng\n",
            provider
        ),
    )
    .context("write search env")?;
    let (base_url, server) = start_gateway(&config_path).await?;
    let client = reqwest::Client::new();

    let search = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_search",
                "arguments": {
                    "query": "rust search",
                    "mode": "web",
                    "max_parallel": 1,
                    "timeout_seconds": 5
                }
            }
        }),
    )
    .await?;
    assert_eq!(search["result"]["structuredContent"]["hit_count"], 1);
    assert_eq!(
        search["result"]["structuredContent"]["hits"][0]["title"],
        "Fixture search result"
    );

    let research = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_research",
                "arguments": {
                    "query": "rust evidence",
                    "mode": "academic",
                    "max_parallel": 1,
                    "timeout_seconds": 5
                }
            }
        }),
    )
    .await?;
    assert_eq!(research["result"]["structuredContent"]["hit_count"], 1);
    assert_eq!(research["result"]["structuredContent"]["evidence_count"], 1);

    let extract = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_extract",
                "arguments": {
                    "url": "http://127.0.0.1:9/private"
                }
            }
        }),
    )
    .await?;
    assert!(
        error_message(&extract).is_some_and(|message| message.contains("blocked")),
        "unexpected extract response: {extract}"
    );

    server.abort();
    Ok(())
}

#[tokio::test]
async fn http_mcp_spawn_parallel_uses_unique_binds_and_enforces_total_cap() -> Result<()> {
    let _spawn_lock = SPAWN_TEST_LOCK.lock().await;
    let interim = TempDir::new().context("create tempdir")?;
    let upstream = start_upstream("parallel answer").await?;
    let bind = free_bind().await?;
    let config_path = write_config_with_scaling(interim.path(), &bind, &upstream, 4, 10).await?;
    let (base_url, server) = start_gateway(&config_path).await?;
    let client = reqwest::Client::new();

    let parallel = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_spawn_parallel",
                "arguments": { "count": 3 }
            }
        }),
    )
    .await?;
    let spawned = parallel["result"]["structuredContent"]["spawned"]
        .as_array()
        .context("spawned should be an array")?;
    let ids = spawned
        .iter()
        .map(|instance| {
            instance["id"]
                .as_str()
                .context("spawned instance id missing")
                .map(str::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let binds = spawned
        .iter()
        .map(|instance| {
            instance["bind"]
                .as_str()
                .context("spawned instance bind missing")
                .map(str::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let unique_binds = binds.iter().collect::<HashSet<_>>();

    let status = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_status",
                "arguments": {}
            }
        }),
    )
    .await?;

    let cap_refusal = post_mcp(
        &client,
        &base_url,
        json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "jnoccio_spawn_instance",
                "arguments": {}
            }
        }),
    )
    .await?;

    for id in &ids {
        let _ = post_mcp(
            &client,
            &base_url,
            json!({
                "jsonrpc": "2.0",
                "id": 14,
                "method": "tools/call",
                "params": {
                    "name": "jnoccio_stop_instance",
                    "arguments": {
                        "instance_id": id
                    }
                }
            }),
        )
        .await;
    }
    server.abort();

    assert_eq!(
        spawned.len(),
        3,
        "unexpected parallel spawn response: {parallel}"
    );
    assert_eq!(
        unique_binds.len(),
        binds.len(),
        "duplicate binds: {binds:?}"
    );
    assert_eq!(parallel["result"]["structuredContent"]["instance_count"], 4);
    assert_eq!(parallel["result"]["structuredContent"]["max_instances"], 4);
    assert_eq!(
        parallel["result"]["structuredContent"]["available_instance_slots"],
        0
    );
    assert_eq!(status["result"]["structuredContent"]["instance_count"], 4);
    assert_eq!(status["result"]["structuredContent"]["max_instances"], 4);
    assert_eq!(
        status["result"]["structuredContent"]["available_instance_slots"],
        0
    );
    assert_eq!(status["result"]["structuredContent"]["role"], "main");
    let worker_threads = status["result"]["structuredContent"]["worker_threads"]
        .as_u64()
        .context("worker_threads should be present")?;
    assert!(worker_threads > 0);
    assert!(
        error_message(&cap_refusal).is_some_and(|message| message.contains("instance cap reached")),
        "unexpected cap refusal: {cap_refusal}"
    );
    Ok(())
}

#[test]
fn launcher_bootstraps_and_proxies_stdio() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("create runtime")?;
    rt.block_on(async {
        let interim = TempDir::new().context("create tempdir")?;
        let upstream = start_upstream("launcher answer").await?;
        let bind = free_bind().await?;
        let config_path = write_config(interim.path(), &bind, &upstream).await?;

        let mut command = Command::new(env!("CARGO_BIN_EXE_jnoccio-mcp"));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .arg("--config")
            .arg(&config_path)
            .arg("--ensure-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("spawn launcher")?;

        let mut stdin = child.stdin.take().context("launcher stdin")?;
        let stdout = child.stdout.take().context("launcher stdout")?;
        let mut stdout = BufReader::new(stdout);

        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "clientInfo": { "name": "test", "version": "1.0.0" },
                    "capabilities": {}
                }
            })
        )
        .context("write initialize")?;
        stdin.flush().context("flush initialize")?;
        let initialize = read_json_line(&mut stdout).context("read initialize")?;
        assert_eq!(initialize["result"]["serverInfo"]["name"], "jnoccio");

        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            })
        )
        .context("write tools/list")?;
        stdin.flush().context("flush tools/list")?;
        let list = read_json_line(&mut stdout).context("read tools/list")?;
        let tool_names = list["result"]["tools"]
            .as_array()
            .context("tools list should be an array")?
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(tool_names.contains(&"jnoccio_chat"));
        assert!(tool_names.contains(&"jnoccio_spawn_instance"));

        drop(stdin);
        let pid = child.id() as i32;
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{pid}"))
            .status();
        let _ = child.wait();
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}

#[test]
fn launcher_refuses_non_jnoccio_process() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("create runtime")?;
    rt.block_on(async {
        let interim = TempDir::new().context("create tempdir")?;
        let upstream = start_upstream("not used").await?;
        let bind = free_bind().await?;
        let config_path = write_config(interim.path(), &bind, &upstream).await?;

        let dummy_listener = TcpListener::bind(&bind).await?;
        let dummy_addr = dummy_listener.local_addr()?;
        let mock = tokio::spawn(async move {
            axum::serve(
                dummy_listener,
                axum::Router::new().route(
                    "/health",
                    axum::routing::get(|| async {
                        axum::Json(json!({
                            "provider": "someone-else",
                            "visible_model": "wrong/model"
                        }))
                    }),
                ),
            )
            .await
            .ok();
        });

        let output = Command::new(env!("CARGO_BIN_EXE_jnoccio-mcp"))
            .arg("--config")
            .arg(&config_path)
            .arg("--ensure-server")
            .output()
            .context("run launcher")?;
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("occupied by a non-Jnoccio process"));

        mock.abort();
        let _ = dummy_addr;
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}

fn error_message(response: &Value) -> Option<&str> {
    response.get("error")?.get("message")?.as_str()
}

async fn start_upstream(answer: &'static str) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let router = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move || {
                let answer = answer.to_string();
                async move {
                    axum::Json(json!({
                        "id": "chatcmpl-test",
                        "object": "chat.completion",
                        "created": 1,
                        "model": "local-model",
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": answer
                            },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1,
                            "total_tokens": 2
                        }
                    }))
                }
            }),
        );
        let _ = axum::serve(listener, router).await;
    });
    Ok(format!("http://{addr}"))
}

async fn write_config(root: &Path, bind: &str, upstream_url: &str) -> Result<PathBuf> {
    write_config_with_scaling(root, bind, upstream_url, 10, 5).await
}

async fn write_config_with_scaling(
    root: &Path,
    bind: &str,
    upstream_url: &str,
    max_instances: usize,
    spawn_batch_limit: usize,
) -> Result<PathBuf> {
    fs::create_dir_all(root.join("config")).context("create config dir")?;
    fs::create_dir_all(root.join("state")).context("create state dir")?;
    fs::create_dir_all(root.join("receipts")).context("create receipts dir")?;

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
                    "base_url": upstream_url
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
    )
    .context("write models")?;

    fs::write(root.join(".env.jnoccio"), "LOCAL_API_KEY=test\n").context("write env")?;

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
                "max_instances": max_instances,
                "spawn_batch_limit": spawn_batch_limit
            }
        })
        .to_string(),
    )
    .context("write server config")?;

    Ok(config_path)
}

async fn start_gateway(config_path: &Path) -> Result<(String, JoinHandle<()>)> {
    let config = load_app_config(config_path, None)?;
    let gateway = Arc::new(Gateway::new(config)?);
    let listener = TcpListener::bind(&gateway.config.bind).await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router::router(gateway)).await;
    });
    wait_for_http(&format!("http://{addr}/health")).await?;
    Ok((format!("http://{addr}"), handle))
}

async fn wait_for_http(url: &str) -> Result<Value> {
    let client = reqwest::Client::new();
    for _ in 0..80 {
        if let Ok(response) = client.get(url).send().await
            && let Ok(json) = response.json::<Value>().await
        {
            return Ok(json);
        }
        sleep(Duration::from_millis(250)).await;
    }
    bail!("timed out waiting for {url}")
}

async fn post_mcp(client: &reqwest::Client, base_url: &str, body: Value) -> Result<Value> {
    let response = client
        .post(format!("{base_url}/mcp"))
        .json(&body)
        .send()
        .await
        .context("post mcp")?;
    response
        .json::<Value>()
        .await
        .context("decode mcp response")
}

async fn free_bind() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok(addr.to_string())
}

async fn start_search_fixture(title: &'static str) -> Result<String> {
    let app = AxumRouter::new().route(
        "/search",
        get(move || async move {
            AxumJson(json!({
                "results": [{
                    "title": title,
                    "url": "https://example.com/fixture",
                    "content": "fixture snippet"
                }]
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    wait_for_http(&format!("http://{addr}/search")).await?;
    Ok(format!("http://{addr}"))
}

fn read_json_line(reader: &mut BufReader<std::process::ChildStdout>) -> Result<Value> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("read launcher response")?;
    if line.trim().is_empty() {
        bail!("launcher response was empty");
    }
    serde_json::from_str(line.trim()).context("parse launcher response")
}
