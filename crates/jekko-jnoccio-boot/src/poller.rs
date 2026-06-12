//! Boot thread + background re-poll loop.
//!
//! Mirrors `bootJnoccioFusion()` + `startBackgroundRepoll()` from
//! `packages/jekko/src/cli/cmd/tui/context/jnoccio-boot.ts`.
//!
//! The thread sends [`BootEvent`]s over an `mpsc::Sender`. The TUI's action
//! loop receives them and converts them to `Action::JnoccioBootUpdate`.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::fusion_root::find_jnoccio_fusion_root;
use crate::health::{probe_health_combined, HealthResult};
use crate::spawn::ensure_and_spawn;
use crate::unlock::is_unlocked;

/// How often the background re-poll fires (matches TS: 5 000 ms).
const REPOLL_INTERVAL: Duration = Duration::from_secs(5);

/// Retries waiting for the server after spawn, matching TS POST_SPAWN_HEALTH_RETRIES × delay.
const SPAWN_RETRIES: u32 = 6;
const SPAWN_RETRY_DELAY: Duration = Duration::from_millis(1350);

/// Current status of the Jnoccio boot lifecycle.
///
/// Mirrors `JnoccioBootStatus` from `jnoccio-boot.ts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootStatus {
    /// Haven't probed yet.
    Idle,
    /// Health-probing `127.0.0.1:4317`.
    Checking,
    /// Spawning the server process.
    Starting,
    /// Server is up and reachable.
    Ready {
        /// Models with valid API keys (the routable set).
        enabled_models: u32,
        /// Total registered models.
        total_models: u32,
    },
    /// Server not running and not unlocked / configured.
    Unavailable,
    /// Spawn attempted but server didn't come up in time.
    Failed,
}

/// Events sent from the boot thread to the TUI's action channel.
#[derive(Clone, Debug)]
pub enum BootEvent {
    /// Boot status changed.
    StatusChanged(BootStatus),
}

/// Spawn the Jnoccio boot thread.
///
/// The thread sends [`BootEvent`]s over `tx` then transitions to a 5 s
/// re-poll loop. The `tx` is cloned internally — the caller may drop their
/// end once [`run_loop`] has obtained its own copy.
///
/// Respects `JEKKO_DISABLE_JNOCCIO_BOOT=1` (sends `Unavailable` and exits).
pub fn spawn_boot_thread(tx: mpsc::Sender<BootEvent>) {
    thread::Builder::new()
        .name("jnoccio-boot".into())
        .spawn(move || run_boot(tx))
        .expect("failed to spawn jnoccio-boot thread");
}

fn send(tx: &mpsc::Sender<BootEvent>, status: BootStatus) {
    tracing::debug!(?status, "jnoccio boot status");
    // Ignore send errors — TUI may have exited.
    let _ = tx.send(BootEvent::StatusChanged(status));
}

fn health_to_ready(h: &HealthResult) -> BootStatus {
    BootStatus::Ready {
        enabled_models: h.enabled_models,
        total_models: h.total_models,
    }
}

/// Read `JNOCCIO_EXTRA_PORT` env var. When set to a valid u16, the poller
/// probes that port as a second jnoccio-fusion instance and sums model counts.
fn extra_port() -> Option<u16> {
    std::env::var("JNOCCIO_EXTRA_PORT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
}

fn run_boot(tx: mpsc::Sender<BootEvent>) {
    // ── Escape hatch for PTY / smoke tests ──────────────────────────────
    if std::env::var("JEKKO_DISABLE_JNOCCIO_BOOT").as_deref() == Ok("1") {
        tracing::debug!("JEKKO_DISABLE_JNOCCIO_BOOT=1 — skipping boot");
        send(&tx, BootStatus::Unavailable);
        return;
    }

    let xport = extra_port();
    if let Some(p) = xport {
        tracing::info!(
            extra_port = p,
            "dual-instance mode: aggregating model counts from port {p}"
        );
    }

    send(&tx, BootStatus::Checking);

    // ── Step 1: Is the server already running? ──────────────────────────
    let initial = probe_health_combined(xport);
    if initial.reachable {
        tracing::info!(
            enabled = initial.enabled_models,
            total = initial.total_models,
            "jnoccio server already running"
        );
        send(&tx, health_to_ready(&initial));
        run_repoll(tx);
        return;
    }

    // ── Step 2: Unlock check ─────────────────────────────────────────────
    if !is_unlocked() {
        tracing::debug!("jnoccio not unlocked on this machine — staying unavailable");
        send(&tx, BootStatus::Unavailable);
        run_repoll(tx);
        return;
    }

    // ── Step 3: Find the jnoccio-fusion directory ────────────────────────
    let Some(fusion_root) = find_jnoccio_fusion_root() else {
        tracing::debug!("jnoccio-fusion/ directory not found in repo tree");
        send(&tx, BootStatus::Unavailable);
        run_repoll(tx);
        return;
    };

    // ── Step 4: Spawn the server ─────────────────────────────────────────
    send(&tx, BootStatus::Starting);
    match ensure_and_spawn(&fusion_root) {
        Ok(()) => {
            tracing::info!("jnoccio-fusion server spawned, waiting for health...");
        }
        Err(err) => {
            tracing::warn!(%err, "failed to spawn jnoccio-fusion server");
            send(&tx, BootStatus::Unavailable);
            run_repoll(tx);
            return;
        }
    }

    // ── Step 5: Wait for the server to become reachable ──────────────────
    let mut post_spawn = HealthResult::default();
    for attempt in 0..SPAWN_RETRIES {
        thread::sleep(SPAWN_RETRY_DELAY);
        post_spawn = probe_health_combined(xport);
        if post_spawn.reachable {
            tracing::info!(attempt, "jnoccio server reachable after spawn");
            break;
        }
        tracing::debug!(attempt, "jnoccio server not yet reachable after spawn");
    }

    if post_spawn.reachable {
        send(&tx, health_to_ready(&post_spawn));
    } else {
        tracing::warn!("jnoccio server did not come up within timeout");
        send(&tx, BootStatus::Failed);
    }

    // ── Step 6: Background re-poll (always) ─────────────────────────────
    run_repoll(tx);
}

/// Continuously re-probes health every 5 s. When the `tx` receiver is dropped
/// (TUI exited), `send` silently ignores the error and the thread exits on the
/// next iteration.
fn run_repoll(tx: mpsc::Sender<BootEvent>) {
    let xport = extra_port();
    let mut last_reachable: Option<bool> = None;

    loop {
        thread::sleep(REPOLL_INTERVAL);
        let result = probe_health_combined(xport);

        let changed = last_reachable != Some(result.reachable);
        if changed {
            if result.reachable {
                send(&tx, health_to_ready(&result));
            } else {
                send(&tx, BootStatus::Unavailable);
            }
        } else if result.reachable {
            // Always forward model count updates even when still reachable.
            send(&tx, health_to_ready(&result));
        }

        last_reachable = Some(result.reachable);

        // If the receiver has gone away, exit.
        if tx.send(BootEvent::StatusChanged(BootStatus::Idle)).is_err() {
            tracing::debug!("jnoccio repoll: receiver gone, exiting thread");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_status_eq() {
        assert_eq!(BootStatus::Unavailable, BootStatus::Unavailable);
        assert_ne!(
            BootStatus::Ready {
                enabled_models: 1,
                total_models: 2
            },
            BootStatus::Ready {
                enabled_models: 3,
                total_models: 2
            },
        );
    }
}
