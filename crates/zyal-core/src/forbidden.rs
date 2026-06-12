//! Canonical forbidden-content patterns.
//!
//! Two semantic groups:
//!
//! 1. [`FORBIDDEN_ARTIFACT_SHAPE_PATTERNS`] — markers a producer publishes in
//!    its `ArtifactContract.forbidden_content` to declare what its outputs
//!    MUST NOT contain. These are *intent labels* ("raw_chain_of_thought") —
//!    they appear as literal strings inside the published contract JSON, so
//!    the auditor MUST NOT scan packet files for them (it would self-match).
//!
//! 2. [`FORBIDDEN_CREDENTIAL_PATTERNS`] — real credential-leakage markers
//!    (API key prefixes, GitHub tokens). The auditor scans EVERY artifact for
//!    any of these; the packet files don't contain them by design, so no
//!    self-match risk.
//!
//! [`FORBIDDEN_PATTERNS`] is the union; use it when scanning non-packet
//! artifacts where both classes should fire.

/// Producer-published artifact-shape markers (5 items). Safe to publish in
/// `ArtifactContract.forbidden_content`; NOT safe to scan against packet files.
pub const FORBIDDEN_ARTIFACT_SHAPE_PATTERNS: &[&str] = &[
    "raw_chain_of_thought",
    "fixture_target_values_in_model_visible_artifacts",
    "process_env_credentials",
    ".env.jnoccio_credentials",
    "jnoccio-local",
];

/// Credential-leakage markers (13 items). Safe to scan in every artifact,
/// including packet/reviewer files.
pub const FORBIDDEN_CREDENTIAL_PATTERNS: &[&str] = &[
    "OPENAI_API_KEY=",
    "ANTHROPIC_API_KEY=",
    "GEMINI_API_KEY=",
    "OPENROUTER_API_KEY=",
    "MISTRAL_API_KEY=",
    "GROQ_API_KEY=",
    "FIREWORKS_API_KEY=",
    "SAMBANOVA_API_KEY=",
    "CEREBRAS_API_KEY=",
    "sk-",
    "sk-or-",
    "gsk_",
    "ghp_",
];

/// Union of both groups (18 items). Use for general artifact scans where the
/// self-match risk is irrelevant (i.e., scans of ledgers, logs, receipts).
pub const FORBIDDEN_PATTERNS: &[&str] = &[
    "raw_chain_of_thought",
    "fixture_target_values_in_model_visible_artifacts",
    "process_env_credentials",
    ".env.jnoccio_credentials",
    "jnoccio-local",
    "OPENAI_API_KEY=",
    "ANTHROPIC_API_KEY=",
    "GEMINI_API_KEY=",
    "OPENROUTER_API_KEY=",
    "MISTRAL_API_KEY=",
    "GROQ_API_KEY=",
    "FIREWORKS_API_KEY=",
    "SAMBANOVA_API_KEY=",
    "CEREBRAS_API_KEY=",
    "sk-",
    "sk-or-",
    "gsk_",
    "ghp_",
];

/// Return the first forbidden pattern present in `text`, or `None`. Scans
/// against [`FORBIDDEN_PATTERNS`] (the union).
pub fn contains_any_forbidden(text: &str) -> Option<&'static str> {
    FORBIDDEN_PATTERNS
        .iter()
        .copied()
        .find(|pattern| text.contains(pattern))
}

/// Return the first credential-leakage pattern present in `text`, or `None`.
/// Use for packet/reviewer files where shape markers would self-match.
pub fn contains_any_credential(text: &str) -> Option<&'static str> {
    FORBIDDEN_CREDENTIAL_PATTERNS
        .iter()
        .copied()
        .find(|pattern| text.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_counts_match_design() {
        assert_eq!(FORBIDDEN_ARTIFACT_SHAPE_PATTERNS.len(), 5);
        assert_eq!(FORBIDDEN_CREDENTIAL_PATTERNS.len(), 13);
        assert_eq!(FORBIDDEN_PATTERNS.len(), 18);
        // Union covers both subsets.
        for shape in FORBIDDEN_ARTIFACT_SHAPE_PATTERNS {
            assert!(FORBIDDEN_PATTERNS.contains(shape));
        }
        for cred in FORBIDDEN_CREDENTIAL_PATTERNS {
            assert!(FORBIDDEN_PATTERNS.contains(cred));
        }
    }

    #[test]
    fn detects_artifact_shape_marker() {
        assert_eq!(
            contains_any_forbidden("the artifact contains raw_chain_of_thought here"),
            Some("raw_chain_of_thought")
        );
    }

    #[test]
    fn detects_credential_leakage() {
        assert_eq!(
            contains_any_forbidden("OPENAI_API_KEY=sk-abc123"),
            Some("OPENAI_API_KEY=")
        );
        assert_eq!(
            contains_any_credential("token: ghp_redactedabc"),
            Some("ghp_")
        );
    }

    #[test]
    fn credential_scan_ignores_shape_markers() {
        // Shape markers appear in published packet contracts as literal JSON
        // strings; the credential-only scan must skip them.
        assert_eq!(
            contains_any_credential("forbidden_content includes raw_chain_of_thought"),
            None
        );
    }

    #[test]
    fn clean_text_returns_none() {
        assert_eq!(
            contains_any_forbidden("a perfectly fine artifact summary"),
            None
        );
        assert_eq!(
            contains_any_credential("a perfectly fine artifact summary"),
            None
        );
    }
}
