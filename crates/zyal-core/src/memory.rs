//! Memory subfamilies + promotion lifecycle for the cross-run capsule store.
//!
//! Capsules are written by the Memory Curator at phase end and gated by the
//! Verifier or Reducer. Retrieval scope follows `MemoryPromotionStatus`: a new
//! run only sees `ProjectOnly` / `Global` capsules by default; `RunOnly`
//! capsules are visible only within the writing run.

use serde::{Deserialize, Serialize};

/// Memory subfamilies. Different retrieval policies apply per kind (e.g.
/// `Negative` is retrieved aggressively during gap-closure phases).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// Specific event memory ("this run did X and it failed for reason Y").
    Episodic,
    /// General fact / claim ("RESP3 framing always uses a single CRLF").
    #[default]
    Semantic,
    /// How-to memory ("to bind to a port use `tokio::net::TcpListener::bind`").
    Procedural,
    /// Falsified-assumption memory ("approach Z does NOT work because W").
    /// Treated with higher retrieval weight during gap-closure / debugging.
    Negative,
}

/// Promotion lifecycle for memory capsules. A capsule starts as `Scratch` (not
/// retrieved cross-run) and advances only after explicit verifier signoff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPromotionStatus {
    /// Working memory — discarded at run end unless promoted.
    #[default]
    Scratch,
    /// Survives within the same run (e.g. across stages of one mega-run) but
    /// not visible to other runs.
    RunOnly,
    /// Visible to other runs operating on the same project / workspace.
    ProjectOnly,
    /// Globally retrievable. Promotion requires reuse across 2+ phases.
    Global,
}
