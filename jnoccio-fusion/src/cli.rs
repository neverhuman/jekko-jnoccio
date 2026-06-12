use std::path::PathBuf;

pub struct ConfigEnvArgs {
    pub config_path: PathBuf,
    pub env_path: Option<PathBuf>,
    pub remaining: Vec<String>,
}

pub fn parse_config_env_args(args: Vec<String>, help: &str) -> ConfigEnvArgs {
    let mut config = PathBuf::from("config/server.json");
    let mut env_file = None;
    let mut remaining = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                if let Some(value) = iter.next() {
                    config = PathBuf::from(value);
                }
            }
            "--env-file" => {
                if let Some(value) = iter.next() {
                    env_file = Some(PathBuf::from(value));
                }
            }
            "--help" | "-h" => {
                println!("{help}");
                std::process::exit(0);
            }
            _ => remaining.push(arg),
        }
    }
    ConfigEnvArgs {
        config_path: config,
        env_path: env_file,
        remaining,
    }
}
