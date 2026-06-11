//! Agent-readable domain exception surface for split-family child repos.

use std::fmt;

/// Typed domain repair categories used by local proof lanes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// The split-family manifest identity does not match this checkout.
    IdentityDrift,
}

impl DomainError {
    /// Purpose of this exception class for agent-readable repair routing.
    pub fn purpose(&self) -> &'static str {
        "keep split-family repository identity aligned with the manifest"
    }

    /// Reason this exception exists instead of a generic string error.
    pub fn reason(&self) -> &'static str {
        "agents need a stable repair category when manifest identity drifts"
    }

    /// Human-readable repair hint for agent reruns.
    pub fn repair_hint(&self) -> &'static str {
        match self {
            Self::IdentityDrift => "rerun the split-family manifest check and restore identity constants",
        }
    }

    /// Common fixes a repair agent should try first.
    pub fn common_fixes(&self) -> &'static [&'static str] {
        match self {
            Self::IdentityDrift => &[
                "compare Cargo.toml package name with repos.manifest.toml",
                "restore REPOSITORY, ROLE, and PROFILE constants",
                "rerun just test and bash ops/ci/jankurai.sh",
            ],
        }
    }

    /// Local docs route for this error class.
    pub fn docs_url(&self) -> &'static str {
        "docs/architecture.md#identity-contract"
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityDrift => write!(f, "split-family identity drifted"),
        }
    }
}

impl std::error::Error for DomainError {}
