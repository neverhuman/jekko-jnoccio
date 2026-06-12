//! Canonical super-reasoning packet schema shared by producer and auditor.

use serde::{Deserialize, Serialize};

/// The artifact contract that a producer publishes and an auditor verifies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactContract {
    /// Filenames (relative to the run dir) that MUST exist after a successful run.
    pub required_artifacts: Vec<String>,
    /// Marker strings that MUST NOT appear in any produced artifact byte stream.
    /// Convention: the canonical list lives at [`crate::forbidden::FORBIDDEN_PATTERNS`];
    /// producers should publish that list verbatim here.
    pub forbidden_content: Vec<String>,
    /// Path to the claim ledger JSONL file.
    pub claim_ledger: String,
    /// Path to the unsupported-claims ledger JSONL file.
    pub unsupported_claims_ledger: String,
    /// Path to the negative-memory JSONL file.
    pub negative_memory: String,
}

/// Promotion-gate policy for a single phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseSignoffPolicy {
    /// Proof commands that MUST pass before the phase can be marked complete.
    pub required_proofs: Vec<String>,
    /// Gate names checked by the orchestrator (e.g. `parity_closed`,
    /// `jankurai_no_regression`, `rollback_plan_present`, `worktrees_resolved`).
    pub gates: Vec<String>,
}

/// Minimal canonical shape of a super-reasoning packet. Producers extend with
/// flow-specific fields; the schema here is the cross-crate contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuperReasoningPacket {
    /// Schema version. Bump when shape changes incompatibly.
    pub schema_version: String,
    /// Stable run id.
    pub run_id: String,
    /// What was being attempted.
    pub objective: String,
    /// Artifact contract for this run.
    pub artifact_contract: ArtifactContract,
    /// Per-phase signoff policies, keyed by phase id.
    #[serde(default)]
    pub phase_signoff_policies: Vec<(String, PhaseSignoffPolicy)>,
}
