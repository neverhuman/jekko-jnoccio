//! Identity and validation surface for the jekko-jnoccio split-family repository.

use std::fmt;

/// Canonical identity for this split-family checkout.
pub const REPOSITORY: &str = "jekko-jnoccio";

/// Role recorded in the split-family manifest.
pub const ROLE: &str = "router";

/// Profile recorded in the split-family manifest.
pub const PROFILE: &str = "rust-router";

/// Error type used by smoke tests and future repo-local validators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitRepoError {
    /// The compiled identity constants drifted from the expected values.
    InvalidIdentity,
}

impl fmt::Display for SplitRepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity => write!(f, "split-family identity drifted"),
        }
    }
}

impl std::error::Error for SplitRepoError {}

/// Return the repo identity tuple used by CI and integration tests.
pub fn identity() -> (&'static str, &'static str, &'static str) {
    (REPOSITORY, ROLE, PROFILE)
}

/// Validate the identity tuple against the manifest values compiled here.
pub fn validate_identity() -> Result<(), SplitRepoError> {
    if identity() == ("jekko-jnoccio", "router", "rust-router") {
        Ok(())
    } else {
        Err(SplitRepoError::InvalidIdentity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable() {
        assert_eq!(identity(), (REPOSITORY, ROLE, PROFILE));
        validate_identity().expect("identity constants match manifest values");
    }
}
