use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use jnoccio_fusion::{
    cli::parse_config_env_args,
    config::{InstanceRole, load_app_config},
    fusion::Gateway,
    router, telemetry,
};

fn main() -> Result<()> {
    let args = parse_args(std::env::args().skip(1).collect())?;
    telemetry::init();
    let mut config = load_app_config(args.config_path, args.env_path.as_deref())?;
    if let Some(bind) = args.bind_override {
        config.bind = bind.clone();
        config.server.bind = Some(bind);
    }
    config.instance_role = args.instance_role;
    config.worker_threads = config.runtime.worker_threads_for_role(args.instance_role);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(config.worker_threads)
        .build()?
        .block_on(run_gateway(config))
}

async fn run_gateway(config: jnoccio_fusion::AppConfig) -> Result<()> {
    let gateway = Arc::new(Gateway::new(config)?);
    let listener = tokio::net::TcpListener::bind(&gateway.config.bind).await?;
    tracing::info!(
        bind = %gateway.config.bind,
        database = %gateway.config.database.display(),
        instance_role = gateway.config.instance_role.as_str(),
        worker_threads = gateway.config.worker_threads,
        max_instances = gateway.config.scaling.max_instances,
        "jnoccio fusion gateway starting"
    );
    axum::serve(listener, router::router(gateway))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

struct Args {
    config_path: PathBuf,
    env_path: Option<PathBuf>,
    bind_override: Option<String>,
    instance_role: InstanceRole,
}

fn parse_args(args: Vec<String>) -> Result<Args> {
    let parsed = parse_config_env_args(
        args,
        "jnoccio-fusion --config <path> --env-file <path> --bind <addr> --instance-role <main|spawned>",
    );
    let config = parsed.config_path;
    let env_file = parsed.env_path;
    let mut bind = None;
    let mut instance_role = InstanceRole::Main;
    let mut iter = parsed.remaining.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bind" => {
                if let Some(value) = iter.next() {
                    bind = Some(value);
                }
            }
            "--instance-role" => {
                if let Some(value) = iter.next() {
                    instance_role = value.parse()?;
                }
            }
            _ => {}
        }
    }
    Ok(Args {
        config_path: config,
        env_path: env_file,
        bind_override: bind,
        instance_role,
    })
}
