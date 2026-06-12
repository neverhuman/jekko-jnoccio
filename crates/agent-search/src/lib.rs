//! # agent-search
//!
//! Privacy-first, offline-testable research/search primitives for Jekko and
//! Jnoccio. The crate keeps provenance, receipts, and deduplication separate
//! from provider transport so callers can keep evidence handling explicit.

pub mod config;
pub mod dedupe;
pub mod extract;
pub mod parallel;
pub mod providers;
pub mod router;
pub mod safety;
pub mod store;
pub mod types;

pub use config::{ProviderEntry, SearchConfig};
pub use dedupe::{dedupe_hits, hash_fingerprint, hash_search_batch, normalize_url};
pub use parallel::search_parallel;
pub use router::{plan_providers, QueryRouter};
pub use safety::{block_internal_url, quarantine_content, sanitize_query};
pub use store::ProvenanceStore;
pub use types::*;
