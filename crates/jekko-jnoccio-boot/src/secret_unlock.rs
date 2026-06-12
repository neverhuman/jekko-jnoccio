//! Software-secret unlock support for Jnoccio Fusion.
//!
//! The 128-character `jnoccio-fusion.unlock` file is a local developer secret
//! used to validate access and seed `JNOCCIO_DEVELOPER_KEY`.
//! It no longer decrypts or installs any repository-encryption material because the
//! `jnoccio-fusion/` source tree is tracked in plaintext.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const SECRET_LEN: usize = 128;

/// Result of applying the software unlock to a repository checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretUnlockReport {
    /// Repository root associated with the unlock attempt.
    pub repo_root: PathBuf,
    /// True when plaintext Jnoccio files were readable after the unlock.
    pub plaintext: bool,
}

/// Normalize terminal/paste noise out of a Jnoccio software unlock secret.
pub fn normalize_unlock_secret(input: &str) -> String {
    let mut compact = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            compact.push(ch);
        }
    }
    if compact.len() == SECRET_LEN + 6 && compact.starts_with("200") && compact.ends_with("201") {
        compact[3..compact.len() - 3].to_string()
    } else {
        compact
    }
}

/// Return true when a normalized unlock secret has the expected shape.
pub fn is_valid_unlock_secret(input: &str) -> bool {
    input.len() == SECRET_LEN
        && input
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Read, normalize, and validate a Jnoccio software unlock secret from disk.
pub fn read_unlock_secret(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let secret = normalize_unlock_secret(&text);
    if !is_valid_unlock_secret(&secret) {
        bail!(
            "unlock secret at {} must be exactly 128 ASCII characters from [A-Za-z0-9_-]",
            path.display()
        );
    }
    Ok(secret)
}

/// Unlock a repository checkout using a software secret file.
pub fn unlock_repo_with_secret_file(
    repo_root: &Path,
    secret_path: &Path,
) -> Result<SecretUnlockReport> {
    let secret = read_unlock_secret(secret_path)?;
    unlock_repo_with_secret(repo_root, &secret)
}

/// Unlock a repository checkout using a normalized software secret.
pub fn unlock_repo_with_secret(repo_root: &Path, secret: &str) -> Result<SecretUnlockReport> {
    unlock_repo_with_secret_options(repo_root, secret, false)
}

/// Unlock a repository checkout, optionally refreshing `jnoccio-fusion/` from
/// `HEAD` when a stale checkout needs to be normalized.
pub fn unlock_repo_with_secret_options(
    repo_root: &Path,
    secret: &str,
    force_refresh_checkout: bool,
) -> Result<SecretUnlockReport> {
    if !is_valid_unlock_secret(secret) {
        bail!("unlock secret must be exactly 128 ASCII characters from [A-Za-z0-9_-]");
    }

    let mut plaintext = crate::unlock::has_plaintext_signals(repo_root);
    if !plaintext && force_refresh_checkout {
        refresh_jnoccio_checkout(repo_root)?;
        plaintext = crate::unlock::has_plaintext_signals(repo_root);
    }
    if !plaintext {
        bail!(
            "Jnoccio Fusion files are not readable as plaintext; update the checkout to the plaintext source tree"
        );
    }

    Ok(SecretUnlockReport {
        repo_root: repo_root.to_path_buf(),
        plaintext,
    })
}

/// Refresh tracked Jnoccio files from `HEAD` after a stale checkout is detected.
///
/// This is a plain Git checkout refresh.
pub fn refresh_jnoccio_checkout(repo_root: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("checkout")
        .arg("--force")
        .arg("--")
        .arg("jnoccio-fusion")
        .current_dir(repo_root)
        .status()
        .context("refresh Jnoccio checkout")?;
    if !status.success() {
        bail!("git checkout --force -- jnoccio-fusion failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bracketed_paste_digits() {
        let secret = "A".repeat(SECRET_LEN);
        assert_eq!(normalize_unlock_secret(&format!("200{secret}201")), secret);
    }

    #[test]
    fn strips_escape_and_noise() {
        let secret = "B".repeat(SECRET_LEN);
        assert_eq!(
            normalize_unlock_secret(&format!("\u{1b}[200~\n{secret}\n\u{1b}[201~")),
            secret
        );
    }

    #[test]
    fn validates_secret_shape() {
        assert!(is_valid_unlock_secret(&"a".repeat(SECRET_LEN)));
        assert!(!is_valid_unlock_secret(&"a".repeat(SECRET_LEN - 1)));
        assert!(!is_valid_unlock_secret(&format!(
            "{}!",
            "a".repeat(SECRET_LEN - 1)
        )));
    }
}
