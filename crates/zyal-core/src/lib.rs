//! Canonical ZYAL backbone types.
//!
//! This crate is dependency-light by design so that both the root workspace and
//! the standalone `jnoccio-fusion` package can depend on it without dragging in
//! heavy transitive deps. Add only `serde` / `serde_json` / `thiserror`-class
//! deps here — runtime, persistence, and network code lives elsewhere.

pub mod artifact_kinds;
pub mod credential_policy;
pub mod forbidden;
pub mod lane;
pub mod memory;
pub mod super_reasoning;

pub use artifact_kinds::ArtifactKind;
pub use credential_policy::CredentialSourcePolicy;
pub use forbidden::{
    contains_any_credential, contains_any_forbidden, FORBIDDEN_ARTIFACT_SHAPE_PATTERNS,
    FORBIDDEN_CREDENTIAL_PATTERNS, FORBIDDEN_PATTERNS,
};
pub use lane::{ArtifactRef, LaneId, RunId};
pub use memory::{MemoryKind, MemoryPromotionStatus};
pub use super_reasoning::{ArtifactContract, PhaseSignoffPolicy, SuperReasoningPacket};
