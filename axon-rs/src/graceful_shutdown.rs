//! Graceful Shutdown — signal handling and orderly server termination.
//!
//! Provides:
//!   - OS signal detection (Ctrl+C / SIGTERM on Unix, Ctrl+C on Windows)
//!   - Programmatic shutdown trigger (via `/v1/shutdown` endpoint)
//!   - `ShutdownCoordinator` — shared notify channel between signal handler, API, and server
//!   - Pre-shutdown hooks: auto-save config, audit log recording
//!
//! The shutdown signal is a tokio::sync::Notify that `axum::serve` awaits
//! via `with_graceful_shutdown`. When triggered, axum stops accepting new
//! connections and drains in-flight requests before returning.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

// ── Shutdown reason ─────────────────────────────────────────────────────

/// Why the server is shutting down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    /// OS signal (Ctrl+C, SIGTERM).
    Signal,
    /// Programmatic shutdown via API endpoint.
    Api,
}

impl ShutdownReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            ShutdownReason::Signal => "signal",
            ShutdownReason::Api => "api",
        }
    }
}

// ── Shutdown coordinator ────────────────────────────────────────────────

/// Coordinates graceful shutdown across signal handlers, API, and server.
///
/// Shared via `Arc` between the signal listener, the `/v1/shutdown` handler,
/// and the `axum::serve(...).with_graceful_shutdown(...)` future.
pub struct ShutdownCoordinator {
    notify: Notify,
    triggered: AtomicBool,
    started_at: Instant,
}

impl ShutdownCoordinator {
    /// Create a new coordinator.
    pub fn new(started_at: Instant) -> Self {
        ShutdownCoordinator {
            notify: Notify::new(),
            triggered: AtomicBool::new(false),
            started_at,
        }
    }

    /// Trigger shutdown. Idempotent — second call is a no-op.
    /// Returns true if this call was the one that triggered shutdown.
    pub fn trigger(&self) -> bool {
        let was_triggered = self.triggered.swap(true, Ordering::SeqCst);
        if !was_triggered {
            self.notify.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Whether shutdown has been triggered.
    pub fn is_triggered(&self) -> bool {
        self.triggered.load(Ordering::SeqCst)
    }

    /// Wait for shutdown to be triggered. Resolves immediately if already triggered.
    pub async fn wait(&self) {
        if self.is_triggered() {
            return;
        }
        self.notify.notified().await;
    }

    /// Server uptime at the moment of query.
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

// ── Shutdown status ─────────────────────────────────────────────────────

/// Status report returned by the shutdown endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ShutdownStatus {
    pub initiated: bool,
    pub reason: ShutdownReason,
    pub uptime_secs: u64,
    pub message: String,
}

// ── Signal listener ─────────────────────────────────────────────────────

/// Listen for OS shutdown signals and trigger the coordinator.
///
/// On Unix: listens for SIGTERM and SIGINT (Ctrl+C).
/// On Windows: listens for Ctrl+C.
///
/// This function is designed to be spawned as a tokio task:
/// ```ignore
/// tokio::spawn(listen_signals(coordinator.clone()));
/// ```
pub async fn listen_signals(coordinator: Arc<ShutdownCoordinator>) {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c => {
                eprintln!("\n  Received Ctrl+C, initiating graceful shutdown...");
            }
            _ = sigterm.recv() => {
                eprintln!("\n  Received SIGTERM, initiating graceful shutdown...");
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
        eprintln!("\n  Received Ctrl+C, initiating graceful shutdown...");
    }

    coordinator.trigger();
}

// ── Pre-shutdown hooks ──────────────────────────────────────────────────

/// Actions to perform before the server fully stops.
///
/// - Auto-save config to disk (if config persistence is active).
/// - Record shutdown in audit trail.
/// - Emit shutdown event on bus.
///
/// This is called from the server launcher after axum::serve returns.
///
/// §Fase 118.b.2 — the ONLY item in this module that needs the `server` feature,
/// and honestly so: it takes `&mut ServerState`, so it is server code by its own
/// signature rather than by association. The coordinator itself
/// ([`ShutdownCoordinator`], [`ShutdownReason`],
/// [`ShutdownStatus`], [`listen_signals`]) is a `Notify` and two atomics — it
/// links nothing and stays in every build, so a lean binary can still install a
/// signal handler and drain.
#[cfg(feature = "server")]
pub fn run_pre_shutdown_hooks(
    state: &mut crate::axon_server::ServerState,
    reason: ShutdownReason,
    use_color: bool,
) {
    // Record in audit trail
    state.audit_log.record(
        "system",
        crate::audit_trail::AuditAction::ServerShutdown,
        "server",
        serde_json::json!({ "reason": reason.as_str(), "uptime_secs": state.started_at.elapsed().as_secs() }),
        true,
    );

    // Emit shutdown event
    state.event_bus.publish(
        "server.shutdown",
        serde_json::json!({ "reason": reason.as_str() }),
        "system",
    );

    // Auto-save config if persistence path is configured
    let config_path = crate::config_persistence::resolve_path(state.config.config_path.as_deref());
    if crate::config_persistence::exists(&config_path) || state.config.config_path.is_some() {
        let snap = crate::server_config::snapshot(
            &state.rate_limiter,
            &state.request_logger,
            &state.api_keys,
        );
        let result = crate::config_persistence::save(&snap, &config_path, crate::runner::AXON_VERSION);
        if result.success {
            if use_color {
                eprintln!("\x1b[2;36m  Config auto-saved to {}\x1b[0m", result.path);
            } else {
                eprintln!("  Config auto-saved to {}", result.path);
            }
        }
    }

    if use_color {
        eprintln!("\x1b[1;33m  AxonServer stopped ({})\x1b[0m", reason.as_str());
    } else {
        eprintln!("  AxonServer stopped ({})", reason.as_str());
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_trigger_idempotent() {
        let coord = ShutdownCoordinator::new(Instant::now());
        assert!(!coord.is_triggered());

        let first = coord.trigger();
        assert!(first);
        assert!(coord.is_triggered());

        let second = coord.trigger();
        assert!(!second); // idempotent
        assert!(coord.is_triggered());
    }

    #[test]
    fn coordinator_uptime() {
        let coord = ShutdownCoordinator::new(Instant::now());
        // Just check it doesn't panic and returns a reasonable value
        assert!(coord.uptime_secs() < 5);
    }

    #[test]
    fn shutdown_reason_serialization() {
        let signal_json = serde_json::to_value(ShutdownReason::Signal).unwrap();
        assert_eq!(signal_json, "signal");

        let api_json = serde_json::to_value(ShutdownReason::Api).unwrap();
        assert_eq!(api_json, "api");
    }

    #[test]
    fn shutdown_reason_as_str() {
        assert_eq!(ShutdownReason::Signal.as_str(), "signal");
        assert_eq!(ShutdownReason::Api.as_str(), "api");
    }

    #[test]
    fn shutdown_status_serializable() {
        let status = ShutdownStatus {
            initiated: true,
            reason: ShutdownReason::Api,
            uptime_secs: 3600,
            message: "shutting down".to_string(),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["initiated"], true);
        assert_eq!(json["reason"], "api");
        assert_eq!(json["uptime_secs"], 3600);
        assert_eq!(json["message"], "shutting down");
    }

    #[tokio::test]
    async fn coordinator_wait_resolves_when_triggered() {
        let coord = Arc::new(ShutdownCoordinator::new(Instant::now()));
        let coord2 = coord.clone();

        // Trigger from another task
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            coord2.trigger();
        });

        // Should resolve without hanging
        tokio::time::timeout(std::time::Duration::from_secs(2), coord.wait())
            .await
            .expect("wait should resolve within timeout");

        assert!(coord.is_triggered());
    }

    #[tokio::test]
    async fn coordinator_wait_resolves_immediately_if_already_triggered() {
        let coord = ShutdownCoordinator::new(Instant::now());
        coord.trigger();

        // Should resolve immediately
        tokio::time::timeout(std::time::Duration::from_millis(50), coord.wait())
            .await
            .expect("wait should resolve immediately when already triggered");
    }
}
