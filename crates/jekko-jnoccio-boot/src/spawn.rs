//! Detached jnoccio-fusion server spawn.
//!
//! Mirrors `spawnServer()` + `findBinary()` from
//! `packages/jekko/src/util/jnoccio-server.ts`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Result};

/// Log level passed via `RUST_LOG` when the caller has not set the variable.
/// `info` matches the level used by the original TS implementation.
const DEFAULT_RUST_LOG: &str = "info";

/// Locate the `jnoccio-fusion` binary.
///
/// Search order (matches TS reference):
/// 1. `<repo>/jnoccio-fusion/target/release/jnoccio-fusion` (local release build)
/// 2. `<repo>/jnoccio-fusion/target/debug/jnoccio-fusion`   (local debug build)
/// 3. `~/.config/jekko/jnoccio-fusion/jnoccio-fusion`       (installed bundle)
pub fn find_binary(fusion_root: &Path) -> Option<PathBuf> {
    let candidates = [
        fusion_root
            .join("target")
            .join("release")
            .join("jnoccio-fusion"),
        fusion_root
            .join("target")
            .join("debug")
            .join("jnoccio-fusion"),
    ];
    for c in &candidates {
        if c.is_file() {
            return Some(c.clone());
        }
    }

    // Installed bundle path (~/.config/jekko/jnoccio-fusion/).
    if let Some(installed) = installed_bundle_binary() {
        if installed.is_file() {
            return Some(installed);
        }
    }

    None
}

/// Resolve `$XDG_CONFIG_HOME`, defaulting to `$HOME/.config` when unset (the
/// XDG base-dir spec default). Returned as an explicit branch so the
/// resolution is a typed decision rather than a chained `unwrap_or_else`.
fn xdg_config_home() -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(value) => PathBuf::from(value),
        Err(_) => {
            #[allow(clippy::manual_unwrap_or_default)]
            let home = match std::env::var("HOME") {
                Ok(value) => value,
                Err(_) => String::new(),
            };
            PathBuf::from(home).join(".config")
        }
    }
}

fn installed_bundle_binary() -> Option<PathBuf> {
    Some(
        xdg_config_home()
            .join("jekko")
            .join("jnoccio-fusion")
            .join("jnoccio-fusion"),
    )
}

/// Resolve the server config path. The installed bundle config is preferred
/// when present; the in-repo `config/server.json` is the source of truth on
/// developer machines.
pub fn find_config(fusion_root: &Path) -> PathBuf {
    // Installed bundle config.
    let installed = xdg_config_home()
        .join("jekko")
        .join("jnoccio-fusion")
        .join("server.json");
    if installed.exists() {
        return installed;
    }
    let installed_jsonc = installed.with_extension("jsonc");
    if installed_jsonc.exists() {
        return installed_jsonc;
    }

    // In-repo config.
    fusion_root.join("config").join("server.json")
}

/// Spawn the jnoccio-fusion server as a detached background process.
///
/// The process survives jekko exit (stdout/stderr redirected to a log file,
/// `setsid` / `process_group` used on Unix to detach from our process group).
pub fn spawn_server(binary: &Path, fusion_root: &Path, config: &Path) -> Result<()> {
    // Ensure the log directory exists.
    let state_dir = fusion_root.join("state");
    fs::create_dir_all(&state_dir)?;
    let log_file = state_dir.join("server.log");

    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let stderr = stdout.try_clone()?;

    tracing::info!(
        binary = %binary.display(),
        config = %config.display(),
        log = %log_file.display(),
        "spawning jnoccio-fusion server"
    );

    // Build env — pass through the current env plus RUST_LOG.
    let rust_log: String = match std::env::var("RUST_LOG") {
        Ok(value) => value,
        Err(_) => DEFAULT_RUST_LOG.to_string(),
    };

    let mut cmd = Command::new(binary);
    cmd.arg("--config")
        .arg(config)
        .current_dir(fusion_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("RUST_LOG", rust_log);

    // On Unix, detach the child from our process group so it survives
    // when the TUI exits.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // process_group(0) puts the child in its own process group.
        cmd.process_group(0);
    }

    let child = cmd.spawn()?;
    tracing::info!(pid = child.id(), "jnoccio-fusion server spawned");

    // Deliberately leak the Child so it is not waited on / killed on drop.
    std::mem::forget(child);

    Ok(())
}

/// Combined helper: find binary + config, then spawn.
/// Returns `Err` only if the binary is missing (not found = user needs to build it).
///
/// `fusion_root` is canonicalized first so the binary, config, state dir, and
/// (transitively) the server's `models.json` / `.env.jnoccio` all resolve to
/// absolute paths. This keeps the spawned server independent of the directory
/// jekko happened to be launched from.
pub fn ensure_and_spawn(fusion_root: &Path) -> Result<()> {
    let fusion_root = fs::canonicalize(fusion_root).unwrap_or_else(|_| fusion_root.to_path_buf());
    let fusion_root = fusion_root.as_path();
    let binary = match find_binary(fusion_root) {
        Some(path) => path,
        None => bail!(
            "jnoccio-fusion binary not found in {:?}. \
             Run `cargo build --release` inside jnoccio-fusion/ to build it.",
            fusion_root
        ),
    };
    let config = find_config(fusion_root);
    if !config.exists() {
        bail!("jnoccio-fusion config not found at {:?}", config);
    }
    spawn_server(&binary, fusion_root, &config)
}
