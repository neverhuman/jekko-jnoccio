//! Enumerate per-user credential slots on disk.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Default single-user slot id. Created on first run; always unlocked.
pub const DEFAULT_USER_ID: &str = "user";

/// On-disk filename for per-user `KEY=value` env files under
/// `~/.jekko/users/<user_id>/`.
pub const LLM_ENV_FILENAME: &str = "llm.env";

/// On-disk filename for per-user balancer/health state. (NOT created by this
/// crate — [`crate::balancer::RoundRobinCursor`] only touches the global
/// `.balancer.sqlite` at the users-root level.)
pub const STATE_DB_FILENAME: &str = "state.sqlite";

/// One credential slot: a single (user, provider) binding sourced from one
/// `llm.env` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserKeySlot {
    /// User id (the `~/.jekko/users/<user_id>/` directory name).
    pub user_id: String,
    /// Provider env var name (e.g. `"OPENROUTER_API_KEY"`).
    pub env_name: String,
    /// Credential value (the part after `=`).
    pub value: String,
    /// Absolute path to the `llm.env` file that produced this slot.
    pub source_path: PathBuf,
}

/// Static enumerator over `~/.jekko/users/*/llm.env`.
///
/// This is a thin, allocation-light scanner — no caching, no TTL. Higher
/// layers (e.g. `jnoccio-fusion::config::resolve_models`) call [`Self::scan`]
/// when they need the current slot set; the balancer cursor selects an
/// individual slot per request.
pub struct KeyPool;

impl KeyPool {
    /// Enumerate every readable `llm.env` under `users_root`. Returns slots
    /// in deterministic order: by `user_id` ascending, then by env name.
    ///
    /// `users_root` is typically `~/.jekko/users/`. The function silently
    /// skips entries that aren't a directory, that lack `llm.env`, or whose
    /// `llm.env` can't be read — callers that need stricter validation should
    /// check directory existence first.
    pub fn scan(users_root: &Path) -> Result<Vec<UserKeySlot>> {
        let mut slots = Vec::new();
        if !users_root.is_dir() {
            return Ok(slots);
        }
        let mut user_ids: Vec<(String, PathBuf)> = Vec::new();
        for entry in fs::read_dir(users_root)
            .with_context(|| format!("read_dir {}", users_root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match entry.file_name().to_str() {
                Some(name) if !name.starts_with('.') => name.to_string(),
                _ => continue,
            };
            user_ids.push((name, path));
        }
        user_ids.sort_by(|a, b| a.0.cmp(&b.0));

        for (user_id, dir) in user_ids {
            let llm_env = dir.join(LLM_ENV_FILENAME);
            let Ok(text) = fs::read_to_string(&llm_env) else {
                continue;
            };
            let mut entries: Vec<(String, String)> = parse_env_lines(&text).into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (env_name, value) in entries {
                slots.push(UserKeySlot {
                    user_id: user_id.clone(),
                    env_name,
                    value,
                    source_path: llm_env.clone(),
                });
            }
        }
        Ok(slots)
    }
}

/// Parse a simple `KEY=value` env file. Lines starting with `#` are comments;
/// blank lines are skipped; later entries override earlier ones (same key).
/// Values are trimmed of surrounding whitespace; surrounding single or double
/// quotes are stripped.
pub fn parse_env_lines(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let stripped = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((key, value)) = stripped.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .to_string();
        out.insert(key, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::create_dir_all;
    use tempfile::tempdir;

    #[test]
    fn parse_env_lines_handles_comments_quotes_and_export() {
        let env = "# leading comment\nOPENAI_API_KEY=sk-plain\nexport GROQ_API_KEY=\"gsk_quoted\"\n  # indented comment\nGEMINI_API_KEY='single-quoted'\n";
        let parsed = parse_env_lines(env);
        assert_eq!(
            parsed.get("OPENAI_API_KEY").map(String::as_str),
            Some("sk-plain")
        );
        assert_eq!(
            parsed.get("GROQ_API_KEY").map(String::as_str),
            Some("gsk_quoted")
        );
        assert_eq!(
            parsed.get("GEMINI_API_KEY").map(String::as_str),
            Some("single-quoted")
        );
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn scan_returns_empty_for_missing_users_root() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nonexistent");
        assert_eq!(KeyPool::scan(&missing).unwrap(), Vec::new());
    }

    #[test]
    fn scan_enumerates_two_users_in_sorted_order() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for (uid, contents) in [
            ("user_2", "GROQ_API_KEY=gsk_two\n"),
            ("user_1", "GROQ_API_KEY=gsk_one\nOPENAI_API_KEY=sk-one\n"),
        ] {
            let d = root.join(uid);
            create_dir_all(&d).unwrap();
            fs::write(d.join(LLM_ENV_FILENAME), contents).unwrap();
        }
        let slots = KeyPool::scan(root).unwrap();
        assert_eq!(slots.len(), 3);
        // user_1 first (sorted), with its env vars in env-name order
        assert_eq!(slots[0].user_id, "user_1");
        assert_eq!(slots[0].env_name, "GROQ_API_KEY");
        assert_eq!(slots[0].value, "gsk_one");
        assert_eq!(slots[1].user_id, "user_1");
        assert_eq!(slots[1].env_name, "OPENAI_API_KEY");
        assert_eq!(slots[2].user_id, "user_2");
        assert_eq!(slots[2].env_name, "GROQ_API_KEY");
    }

    #[test]
    fn scan_skips_hidden_dirs_and_missing_llm_env() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        create_dir_all(root.join(".cache")).unwrap();
        create_dir_all(root.join("user_empty")).unwrap();
        let with_keys = root.join("user");
        create_dir_all(&with_keys).unwrap();
        fs::write(with_keys.join(LLM_ENV_FILENAME), "OPENAI_API_KEY=sk\n").unwrap();
        let slots = KeyPool::scan(root).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].user_id, "user");
    }
}
