//! Per-user credential pool, round-robin balancer cursor, and stub budget
//! policy for ZYAL multi-tenant routing.
//!
//! This crate is the single canonical home for the three concerns that today
//! are scattered across `jekko-provider/src/key_pool/`,
//! `jekko-cli/src/cmd/keys/`, and `jekko-runtime/src/key_balancer/`. Both
//! `jnoccio-fusion` (standalone package) and the jekko workspace crates
//! depend on it, so credential resolution flows through one code path.
//!
//! Layout:
//! - [`pool`] enumerates per-user `llm.env` slots on disk.
//! - [`balancer`] persists the global round-robin cursor in
//!   `~/.jekko/users/.balancer.sqlite` (REUSED, not recreated).
//! - [`budget`] declares the [`budget::PolicyHook`] trait plus a stub
//!   [`budget::AlwaysAllow`] implementation; real enforcement is a follow-up.

pub mod balancer;
pub mod budget;
pub mod pool;

pub use balancer::RoundRobinCursor;
pub use budget::{AlwaysAllow, PolicyHook, UserBudget};
pub use pool::{KeyPool, UserKeySlot};
