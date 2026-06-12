//! Jnoccio Fusion server auto-boot for the Rust TUI.
//!
//! Mirrors `packages/jekko/src/cli/cmd/tui/context/jnoccio-boot.ts` and
//! `packages/jekko/src/util/jnoccio-server.ts` exactly, adapted to std-only
//! Rust (no async runtime, no TUI dependency).
//!
//! # Boot sequence
//!
//! 1. Check `JEKKO_DISABLE_JNOCCIO_BOOT=1` — if set, emit [`BootStatus::Unavailable`] and exit.
//! 2. Health-probe `127.0.0.1:4317/health` — if reachable, emit `Ready` and start background
//!    re-poll (every 5 s).
//! 3. If not reachable: run unlock detection. If not unlocked, emit `Unavailable` + re-poll.
//! 4. If unlocked: find the `jnoccio-fusion` binary (repo-local build or installed bundle).
//!    If binary missing, emit `Unavailable`.
//! 5. Spawn the server as a detached background process.
//! 6. Wait up to 8 s (6 × 1.3 s retries) for the server to come up.
//! 7. Emit `Ready { model_count }` or `Failed`.
//! 8. Start 5 s re-poll loop regardless.

pub mod fusion_root;
pub mod health;
pub mod poller;
pub mod secret_unlock;
pub mod spawn;
pub mod unlock;

pub use poller::{spawn_boot_thread, BootEvent, BootStatus};
