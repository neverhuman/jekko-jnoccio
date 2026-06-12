//! Unlock detection for Jnoccio Fusion.
//!
//! Mirrors the logic in `packages/jekko/src/util/jnoccio-unlock.ts`:
//! `isJnoccioFusionUnlocked` / `isJnoccioFusionConfigured` / `hasPlaintextSignals`.
//!
//! # Unlock hierarchy (left-to-right, fast exit)
//!
//! 1. `JNOCCIO_DEVELOPER_KEY` env var is set and non-empty → unlocked.
//! 2. `~/.env.jnoccio` contains a non-empty `JNOCCIO_DEVELOPER_KEY=...` → unlocked.
//!
//! Plaintext `jnoccio-fusion/` files remain a diagnostic/configuration signal,
//! but plaintext alone never unlocks developer-only runtime paths.
//!
//! Note: checks are read-only and cheap. No crypto is performed here.

use std::fs;
use std::path::{Path, PathBuf};

/// Returns `true` if the developer unlock signal is present, meaning the
/// current machine has explicit access to run local Jnoccio Fusion paths.
pub fn is_unlocked() -> bool {
    developer_key().is_some()
}

/// Returns the developer key when it is present in process env or
/// `~/.env.jnoccio`. This is intentionally the only unlock source.
pub fn developer_key() -> Option<String> {
    if let Ok(value) = std::env::var("JNOCCIO_DEVELOPER_KEY") {
        if !value.trim().is_empty() {
            tracing::debug!("jnoccio unlocked via JNOCCIO_DEVELOPER_KEY env var");
            return Some(value);
        }
    }

    let home = home_dir()?;
    let env_file = home.join(".env.jnoccio");
    let text = fs::read_to_string(&env_file).ok()?;
    let key = developer_key_from_env_text(&text)?;
    tracing::debug!("jnoccio unlocked via {}", env_file.display());
    Some(key)
}

fn developer_key_from_env_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed).trim();
        let (name, value) = trimmed.split_once('=')?;
        if name.trim() != "JNOCCIO_DEVELOPER_KEY" {
            return None;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Returns `true` if the encrypted subtree is readable as plaintext. This is
/// useful for diagnostics and setup status, but it does not unlock runtime
/// developer behavior.
pub fn has_plaintext_checkout() -> bool {
    if let Some(root) = find_repo_root() {
        return has_plaintext_signals(&root);
    }
    false
}

/// Resolve `$XDG_CONFIG_HOME`, defaulting to `$HOME/.config` (XDG base-dir
/// spec default). Returns `None` only if neither variable is set.
///
/// Shared with [`crate::fusion_root`] for installed-bundle discovery.
pub(crate) fn xdg_config_home() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("XDG_CONFIG_HOME") {
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    home_dir().map(|h| h.join(".config"))
}

/// Walk `$PWD` (then its ancestors) looking for a `jnoccio-fusion/` subdirectory.
/// Returns the **repo root** (parent of `jnoccio-fusion/`), not the subtree itself.
pub fn find_repo_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    find_repo_root_from(&cwd)
}

/// Find the repository root from an explicit starting path.
pub fn find_repo_root_from(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors().take(10) {
        if ancestor.join("jnoccio-fusion").is_dir() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Checks whether the `jnoccio-fusion/` subtree inside `repo_root` is readable
/// as plaintext instead of an encrypted blob.
///
/// Mirrors `hasPlaintextSignals` from `jnoccio-unlock.ts`.
pub fn has_plaintext_signals(repo_root: &Path) -> bool {
    let fusion_root = repo_root.join("jnoccio-fusion");

    // Check 1: Cargo.toml must mention the expected package name.
    let cargo_path = fusion_root.join("Cargo.toml");
    let Ok(cargo_text) = fs::read_to_string(&cargo_path) else {
        return false;
    };
    if !cargo_text.contains("[package]") || !cargo_text.contains("name = \"jnoccio-fusion\"") {
        return false;
    }

    // Check 2: config/server.json must have the provider/model fields readable.
    let config_path = fusion_root.join("config").join("server.json");
    let Ok(config_text) = fs::read_to_string(&config_path) else {
        return false;
    };
    config_text.contains("\"jnoccio\"")
        && (config_text.contains("\"jnoccio-fusion\"")
            || config_text.contains("\"jnoccio/jnoccio-fusion\""))
}

/// Returns the `.env.jnoccio` path inside the `jnoccio-fusion/` subtree if it
/// exists (a fully configured + unlocked install also has this file with API
/// keys written by `jekko jnoccio unlock`).
pub fn jnoccio_env_path(repo_root: &Path) -> PathBuf {
    repo_root.join("jnoccio-fusion").join(".env.jnoccio")
}

/// Returns `true` if the repo is readable AND the `.env.jnoccio` file exists
/// (meaning `jekko jnoccio unlock` was previously completed successfully).
pub fn is_configured(repo_root: &Path) -> bool {
    has_plaintext_signals(repo_root) && jnoccio_env_path(repo_root).exists()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// Serializes tests that mutate process-global environment variables
/// (`HOME`, `JNOCCIO_DEVELOPER_KEY`, `XDG_CONFIG_HOME`, `JEKKO_FUSION_ROOT`) and
/// the current directory. Shared with [`crate::fusion_root`] tests so the two
/// modules never race on the same globals under the parallel test runner.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct EnvGuard {
        prev_home: Option<std::ffi::OsString>,
        prev_dev: Option<std::ffi::OsString>,
        prev_cwd: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn install(home: &Path, cwd: Option<&Path>, dev_key: Option<&str>) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_home = std::env::var_os("HOME");
            let prev_dev = std::env::var_os("JNOCCIO_DEVELOPER_KEY");
            let prev_cwd = std::env::current_dir().unwrap();
            std::env::set_var("HOME", home);
            match dev_key {
                Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
                None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
            }
            if let Some(cwd) = cwd {
                std::env::set_current_dir(cwd).unwrap();
            }
            Self {
                prev_home,
                prev_dev,
                prev_cwd,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.prev_cwd).unwrap();
            match &self.prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_dev {
                Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
                None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
            }
        }
    }

    fn make_plaintext_signals(root: &Path) {
        let fusion = root.join("jnoccio-fusion");
        fs::create_dir_all(fusion.join("config")).unwrap();
        fs::write(
            fusion.join("Cargo.toml"),
            "[package]\nname = \"jnoccio-fusion\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            fusion.join("config").join("server.json"),
            r#"{"provider":"jnoccio","model":"jnoccio-fusion"}"#,
        )
        .unwrap();
    }

    #[test]
    fn detects_plaintext_signals() {
        let tmp = TempDir::new().unwrap();
        make_plaintext_signals(tmp.path());
        assert!(has_plaintext_signals(tmp.path()));
    }

    #[test]
    fn detects_namespaced_plaintext_model_signal() {
        let tmp = TempDir::new().unwrap();
        make_plaintext_signals(tmp.path());
        fs::write(
            tmp.path()
                .join("jnoccio-fusion")
                .join("config")
                .join("server.json"),
            r#"{"provider":"jnoccio","model":"jnoccio/jnoccio-fusion"}"#,
        )
        .unwrap();
        assert!(has_plaintext_signals(tmp.path()));
    }

    #[test]
    fn rejects_encrypted_signals() {
        let tmp = TempDir::new().unwrap();
        let fusion = tmp.path().join("jnoccio-fusion");
        fs::create_dir_all(fusion.join("config")).unwrap();
        // Simulated encrypted binary content (not valid UTF-8 / JSON)
        fs::write(fusion.join("Cargo.toml"), b"\x00GITCRYPT\x00\x02encrypted").unwrap();
        assert!(!has_plaintext_signals(tmp.path()));
    }

    #[test]
    fn is_configured_requires_env_file() {
        let tmp = TempDir::new().unwrap();
        make_plaintext_signals(tmp.path());
        // Plaintext but no .env.jnoccio yet
        assert!(!is_configured(tmp.path()));

        // Write the env file
        fs::write(
            tmp.path().join("jnoccio-fusion").join(".env.jnoccio"),
            "JNOCCIO_DEFAULT_API_KEY=test\n",
        )
        .unwrap();
        assert!(is_configured(tmp.path()));
    }

    #[test]
    fn is_unlocked_requires_developer_key_even_with_plaintext_signals() {
        let home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        make_plaintext_signals(repo.path());
        let _guard = EnvGuard::install(home.path(), Some(repo.path()), None);

        assert!(has_plaintext_checkout());
        assert!(!is_unlocked());
    }

    #[test]
    fn env_file_existence_alone_does_not_unlock() {
        let home = TempDir::new().unwrap();
        fs::write(home.path().join(".env.jnoccio"), "# local jnoccio file\n").unwrap();
        let _guard = EnvGuard::install(home.path(), None, None);

        assert!(!is_unlocked());
    }

    #[test]
    fn env_file_with_developer_key_unlocks() {
        let home = TempDir::new().unwrap();
        fs::write(
            home.path().join(".env.jnoccio"),
            "JNOCCIO_DEVELOPER_KEY=file-secret\n",
        )
        .unwrap();
        let _guard = EnvGuard::install(home.path(), None, None);

        assert_eq!(developer_key().as_deref(), Some("file-secret"));
        assert!(is_unlocked());
    }

    #[test]
    fn process_env_developer_key_unlocks() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::install(home.path(), None, Some("process-secret"));

        assert_eq!(developer_key().as_deref(), Some("process-secret"));
        assert!(is_unlocked());
    }
}
