use tracing_subscriber::{EnvFilter, fmt, fmt::format::FmtSpan};

const DEFAULT_ENV_FILTER: &str = "info";

pub fn init() {
    let filter = build_env_filter();
    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_file(true)
        .with_line_number(true)
        .json();
    let _ = tracing::subscriber::set_global_default(subscriber.finish());
}

fn build_env_filter() -> EnvFilter {
    if std::env::var_os("RUST_LOG").is_none() {
        return EnvFilter::new(DEFAULT_ENV_FILTER);
    }
    match EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(err) => {
            eprintln!(
                "invalid tracing filter in environment: {err}; using default filter {DEFAULT_ENV_FILTER}"
            );
            EnvFilter::new(DEFAULT_ENV_FILTER)
        }
    }
}
