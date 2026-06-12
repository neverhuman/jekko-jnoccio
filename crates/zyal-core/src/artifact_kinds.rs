//! Canonical artifact kind enum.
//!
//! Extends the legacy `jankurai_runner::reasoning::ReasoningArtifactKind` with
//! the variants the super-agent kernel needs (`MacroPlan`, `PhaseDag`,
//! `FunctionGraph`, `ParityCase`, `PerfGap`, `SignoffReceipt`, `Contradiction`,
//! `ReducerDecision`). Phase B2.4 migrates the runner to use this enum.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    // --- Legacy ReasoningArtifactKind variants (preserved for migration parity) ---
    /// Crystallized request contract.
    TaskContract,
    /// Retrieved evidence/context pack.
    ContextPack,
    /// Candidate stage plan.
    StageProposal,
    /// Critique or objection set.
    Critique,
    /// Final per-cycle master plan.
    MasterPlan,
    /// Phase-level plan.
    PhasePlan,
    /// Worker build receipt.
    BuildReceipt,
    /// Verification receipt.
    VerificationReceipt,
    /// Parity gap report.
    ParityGap,
    /// Baseline-vs-tournament reasoning benchmark.
    ReasoningBenchmark,
    /// Durable memory capsule.
    MemoryCapsule,

    // --- Super-agent kernel additions ---
    /// Cross-cycle 12-stage macro plan (target_contract → … → release_readiness).
    MacroPlan,
    /// Topologically-ordered phase dependency graph.
    PhaseDag,
    /// Repo function/call graph slice for a phase.
    FunctionGraph,
    /// One parity test case (input + reference output + adapter binding).
    ParityCase,
    /// Performance budget gap.
    PerfGap,
    /// Phase signoff receipt (gates passed / failed).
    SignoffReceipt,
    /// Recorded contradiction between two artifacts or claims.
    Contradiction,
    /// Reducer's synthesized decision from blind-lane proposals.
    ReducerDecision,
}
