//! Cwd-independent discovery of the `jnoccio-fusion/` directory root.
//!
//! Jekko must boot the gateway correctly no matter which folder the user
//! launches it from, so the resolution here is deliberately anchored to stable
//! locations (an explicit env override, the running executable, the installed
//! bundle) and only falls back to a `$PWD`-relative walk as a last resort.
//!
//! This is kept separate from [`crate::unlock`] (which detects the *developer
//! unlock* signal) because the two answer different questions: "is this machine
//! allowed to run local Jnoccio Fusion?" versus "where does the fusion checkout
//! live?".

use std::fs;
use std::path::{Path, PathBuf};

use crate::unlock::{find_repo_root_from, xdg_config_home};

/// Find the `jnoccio-fusion/` directory root.
///
/// Resolution is deliberately **cwd-independent** so jekko boots the gateway
/// correctly no matter which folder the user launches it from. The returned
/// path is canonicalized when possible. Order (first hit wins):
///
/// 1. `JEKKO_FUSION_ROOT` env override — either the `jnoccio-fusion/` dir
///    itself or a repo root that contains one.
/// 2. Walk up from the **running executable** (`current_exe`) for a
///    `jnoccio-fusion/` subdir — the dev case, e.g. `<repo>/target/release/jekko`
///    resolves to `<repo>/jnoccio-fusion` regardless of `$PWD`.
/// 3. `$XDG_CONFIG_HOME/jekko/jnoccio-fusion` (default
///    `$HOME/.config/jekko/jnoccio-fusion`) — the installed bundle layout.
/// 4. Walk up from `$PWD` (legacy dev convenience) — kept last so it never
///    shadows the stable anchors above.
///
/// Returns `None` only when every source is missing.
pub fn find_jnoccio_fusion_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    find_jnoccio_fusion_root_from(&cwd)
}

/// Find the `jnoccio-fusion/` directory root, using `start` as the final
/// (legacy) walk-up base instead of `$PWD`. The stable anchors (env var,
/// executable location, installed bundle) still take precedence over `start`
/// so explicit-base callers (e.g. the runtime auto-boot path) are equally
/// robust to the launch directory.
pub fn find_jnoccio_fusion_root_from(start: &Path) -> Option<PathBuf> {
    // 1. Explicit override.
    if let Some(root) = fusion_root_from_env() {
        tracing::debug!(root = %root.display(), "jnoccio-fusion root from JEKKO_FUSION_ROOT");
        return Some(root);
    }
    // 2. The jekko binary's own location (independent of cwd).
    if let Some(root) = fusion_root_from_exe() {
        tracing::debug!(root = %root.display(), "jnoccio-fusion root from executable path");
        return Some(root);
    }
    // 3. Installed bundle layout.
    if let Some(root) = installed_bundle_root() {
        tracing::debug!(root = %root.display(), "jnoccio-fusion root from installed bundle");
        return Some(root);
    }
    // 4. Legacy: walk up from the provided start path.
    if let Some(root) = fusion_root_in_ancestors(start) {
        tracing::debug!(root = %root.display(), "jnoccio-fusion root from ancestor walk");
        return Some(root);
    }
    None
}

/// Resolve `JEKKO_FUSION_ROOT`, accepting either the `jnoccio-fusion/` dir
/// itself or a parent directory that contains one.
fn fusion_root_from_env() -> Option<PathBuf> {
    let raw = std::env::var_os("JEKKO_FUSION_ROOT")?;
    if raw.is_empty() {
        return None;
    }
    fusion_dir_from_candidate(&PathBuf::from(raw))
}

/// Walk up from the running executable looking for a sibling `jnoccio-fusion/`.
fn fusion_root_from_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let start = exe.parent()?;
    fusion_root_in_ancestors(start)
}

/// The installed bundle root (`$XDG_CONFIG_HOME/jekko/jnoccio-fusion`), if it
/// exists as a directory.
fn installed_bundle_root() -> Option<PathBuf> {
    let bundle = xdg_config_home()?.join("jekko").join("jnoccio-fusion");
    bundle.is_dir().then(|| canonicalize_or(&bundle))
}

/// Walk `start` and its ancestors (≤10 levels) for a `jnoccio-fusion/` subdir,
/// returning that subdir (the fusion root), canonicalized when possible.
fn fusion_root_in_ancestors(start: &Path) -> Option<PathBuf> {
    let repo = find_repo_root_from(start)?;
    let candidate = repo.join("jnoccio-fusion");
    candidate.is_dir().then(|| canonicalize_or(&candidate))
}

/// Map a `JEKKO_FUSION_ROOT` candidate to a concrete `jnoccio-fusion/` dir:
/// accept the directory directly when it is already named `jnoccio-fusion`, or
/// descend into a `jnoccio-fusion/` child when the candidate is a repo root.
fn fusion_dir_from_candidate(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    if path.file_name().and_then(|name| name.to_str()) == Some("jnoccio-fusion") {
        return Some(canonicalize_or(path));
    }
    let nested = path.join("jnoccio-fusion");
    nested.is_dir().then(|| canonicalize_or(&nested))
}

/// Canonicalize a path, falling back to the input on error so callers always
/// get a usable (if non-canonical) path.
fn canonicalize_or(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unlock::ENV_LOCK;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn candidate_accepts_direct_fusion_dir() {
        let tmp = TempDir::new().unwrap();
        let fusion = tmp.path().join("jnoccio-fusion");
        fs::create_dir_all(&fusion).unwrap();
        let got = fusion_dir_from_candidate(&fusion).unwrap();
        assert_eq!(got, fs::canonicalize(&fusion).unwrap());
    }

    #[test]
    fn candidate_descends_into_fusion_child() {
        let tmp = TempDir::new().unwrap();
        let fusion = tmp.path().join("jnoccio-fusion");
        fs::create_dir_all(&fusion).unwrap();
        let got = fusion_dir_from_candidate(tmp.path()).unwrap();
        assert_eq!(got, fs::canonicalize(&fusion).unwrap());
    }

    #[test]
    fn candidate_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(fusion_dir_from_candidate(tmp.path()).is_none());
    }

    #[test]
    fn ancestors_walk_finds_fusion_root() {
        let tmp = TempDir::new().unwrap();
        let fusion = tmp.path().join("jnoccio-fusion");
        let nested = tmp.path().join("a").join("b").join("c");
        fs::create_dir_all(&fusion).unwrap();
        fs::create_dir_all(&nested).unwrap();
        let got = fusion_root_in_ancestors(&nested).unwrap();
        assert_eq!(got, fs::canonicalize(&fusion).unwrap());
    }

    #[test]
    fn env_override_resolves_repo_root_and_fusion_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let fusion = tmp.path().join("jnoccio-fusion");
        fs::create_dir_all(&fusion).unwrap();
        let prev = std::env::var_os("JEKKO_FUSION_ROOT");

        // Point at the repo root (parent of jnoccio-fusion/).
        std::env::set_var("JEKKO_FUSION_ROOT", tmp.path());
        let from_repo = fusion_root_from_env();
        // Point directly at the jnoccio-fusion/ dir.
        std::env::set_var("JEKKO_FUSION_ROOT", &fusion);
        let from_dir = fusion_root_from_env();

        match prev {
            Some(v) => std::env::set_var("JEKKO_FUSION_ROOT", v),
            None => std::env::remove_var("JEKKO_FUSION_ROOT"),
        }

        let expected = fs::canonicalize(&fusion).unwrap();
        assert_eq!(from_repo.unwrap(), expected);
        assert_eq!(from_dir.unwrap(), expected);
    }

    #[test]
    fn installed_bundle_resolves_under_xdg_default() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = TempDir::new().unwrap();
        let bundle = home
            .path()
            .join(".config")
            .join("jekko")
            .join("jnoccio-fusion");
        fs::create_dir_all(&bundle).unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("HOME", home.path());
        std::env::remove_var("XDG_CONFIG_HOME");

        let got = installed_bundle_root();

        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }

        assert_eq!(got.unwrap(), fs::canonicalize(&bundle).unwrap());
    }
}
