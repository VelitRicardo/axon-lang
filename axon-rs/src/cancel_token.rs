//! v1.24.0 — Cancellation primitives (D6 cancel-safety).
//!
//! Provides the building blocks for **cooperative** cancellation of
//! long-running streaming executions when the SSE client disconnects.
//!
//! ## Why hand-rolled instead of `tokio_util::sync::CancellationToken`
//!
//! `tokio-util` is in the dependency graph transitively (via reqwest +
//! axum) but is not a direct dependency. Pulling it in just to get
//! `CancellationToken` would mean tracking a new direct-dep version
//! across releases for ~30 LOC of functionality. The primitives below
//! are byte-equivalent in semantics (atomic flag + async wait) and
//! pinned by this module's tests against the catalog of usage shapes
//! the SSE handler needs.
//!
//! ## Surface
//!
//! - [`CancellationFlag`] — clone-able handle. Producer + consumer +
//!   drop guards all hold their own clone of the same inner state.
//!     * [`Self::cancel`] — non-async; idempotent. Once called, any
//!       subsequent [`Self::is_cancelled`] returns `true`.
//!     * [`Self::is_cancelled`] — non-async polling check. Producers
//!       use this between event emissions to detect cancellation
//!       deterministically.
//!     * [`Self::cancelled`] — async; returns a future that resolves
//!       the first time `cancel()` is called on any clone. Consumers
//!       can `tokio::select!` between this and their normal pipeline
//!       to react to cancellation without polling.
//!
//! - [`CancelOnDrop`] — RAII guard that calls `cancel()` on the
//!   wrapped flag in its `Drop` impl. Install at the top of any
//!   scope that "owns" cancellation responsibility (typically the
//!   spawned SSE consumer task); the guard fires automatically on
//!   panic, on early return via `?`, or on task abort.
//!
//! ## Pillar trace (D6 + D10)
//!
//! - **MATHEMATICS** — cancellation is monotone: once cancelled, the
//!   flag never returns to non-cancelled. The semantics are a closed
//!   2-state machine ({Running, Cancelled}).
//! - **LOGIC** — the drop-guard invariant is precise: if the guard
//!   exists, cancellation MUST fire by the time `Drop::drop` runs.
//!   Compiler enforces drop ordering.
//! - **PHILOSOPHY** — declared intent ("this scope cancels on exit")
//!   IS the runtime behavior. No runtime configuration needed.
//! - **COMPUTING** — atomic + Notify; no spinning, no allocation per
//!   check. `is_cancelled` is a single `Acquire` load.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Inner state shared by every clone of a [`CancellationFlag`].
struct Inner {
    /// `true` iff `cancel()` has been called on any clone.
    cancelled: AtomicBool,
    /// Wake-up handle for async `cancelled()` awaits. `notify_waiters`
    /// fires on cancel; cheap when there are no waiters.
    notify: Notify,
}

/// A clone-able cancellation handle. Internally `Arc`; cloning is cheap.
///
/// Construct with [`Self::new`]; cancel any clone with [`Self::cancel`].
/// Multiple consumers can await on [`Self::cancelled`] concurrently.
#[derive(Clone)]
pub struct CancellationFlag {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for CancellationFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // v1.24.0 — Required so `ChatRequest` (which derives
        // `Debug`) can carry a CancellationFlag field. We surface
        // only the cancelled state, NOT the Notify internals (which
        // don't impl Debug and aren't meaningful to log anyway).
        f.debug_struct("CancellationFlag")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl Default for CancellationFlag {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationFlag {
    /// Construct a fresh flag in the `Running` state.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    /// Mark the flag as cancelled and wake any pending
    /// [`Self::cancelled`] awaits. Idempotent — multiple calls are
    /// safe.
    ///
    /// `Release` ordering pairs with the `Acquire` load in
    /// [`Self::is_cancelled`] so any state the cancelling thread
    /// established before the call is visible to subsequent
    /// `is_cancelled` observers.
    pub fn cancel(&self) {
        // `swap` returns the previous value; we use it to skip the
        // notify when the flag was already cancelled (cheap idempotency).
        let was = self.inner.cancelled.swap(true, Ordering::Release);
        if !was {
            self.inner.notify.notify_waiters();
        }
    }

    /// Non-async polling check. Returns `true` iff any clone has called
    /// `cancel()`.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Future that resolves the first time any clone calls `cancel()`.
    /// If the flag is already cancelled by the time `cancelled()` is
    /// called, the returned future resolves immediately on first poll.
    ///
    /// Compose with `tokio::select!` to interrupt long-running awaits.
    pub async fn cancelled(&self) {
        // Fast path: already cancelled.
        if self.is_cancelled() {
            return;
        }
        // Register notify wait BEFORE re-checking the atomic so we
        // don't miss a cancellation that fires between the check and
        // the await (standard Notify pattern per tokio docs).
        let notified = self.inner.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// RAII drop guard. When the guard is dropped, the wrapped flag is
/// cancelled. Use to bind cancellation to a scope's lifetime: any
/// path out of the scope (normal return, early `?`-return, panic,
/// task abort) fires the cancellation.
///
/// The guard holds a clone of the flag, so the original flag and any
/// sibling clones remain usable after the guard drops.
pub struct CancelOnDrop {
    flag: CancellationFlag,
}

impl CancelOnDrop {
    pub fn new(flag: CancellationFlag) -> Self {
        Self { flag }
    }

    /// Borrow the wrapped flag — useful when the same scope needs to
    /// observe cancellation in addition to firing it on drop.
    pub fn flag(&self) -> &CancellationFlag {
        &self.flag
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.flag.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn flag_starts_not_cancelled() {
        let f = CancellationFlag::new();
        assert!(!f.is_cancelled());
    }

    #[test]
    fn cancel_marks_flag_cancelled() {
        let f = CancellationFlag::new();
        f.cancel();
        assert!(f.is_cancelled());
    }

    #[test]
    fn cancel_is_idempotent() {
        let f = CancellationFlag::new();
        f.cancel();
        f.cancel();
        f.cancel();
        assert!(f.is_cancelled());
    }

    #[test]
    fn clones_share_state() {
        let f = CancellationFlag::new();
        let f2 = f.clone();
        let f3 = f.clone();
        assert!(!f2.is_cancelled());
        assert!(!f3.is_cancelled());
        f.cancel();
        assert!(f2.is_cancelled());
        assert!(f3.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_future_resolves_on_cancel() {
        let f = CancellationFlag::new();
        let f2 = f.clone();
        let h = tokio::spawn(async move {
            f2.cancelled().await;
            true
        });
        // Give the spawn a tick to start awaiting.
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(!h.is_finished());
        f.cancel();
        let v = h.await.unwrap();
        assert!(v);
    }

    #[tokio::test]
    async fn cancelled_future_resolves_immediately_when_already_cancelled() {
        let f = CancellationFlag::new();
        f.cancel();
        // Should NOT block — completes within the timeout.
        tokio::time::timeout(Duration::from_millis(50), f.cancelled())
            .await
            .expect("cancelled() future must resolve immediately");
    }

    #[tokio::test]
    async fn multiple_cancelled_awaits_all_wake_on_cancel() {
        let f = CancellationFlag::new();
        let n = 4;
        let mut handles = Vec::new();
        for _ in 0..n {
            let f2 = f.clone();
            handles.push(tokio::spawn(async move {
                f2.cancelled().await;
            }));
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
        f.cancel();
        // All N handles should complete promptly.
        for h in handles {
            tokio::time::timeout(Duration::from_millis(100), h)
                .await
                .expect("await completes within budget")
                .expect("join ok");
        }
    }

    #[test]
    fn cancel_on_drop_fires_on_scope_exit() {
        let f = CancellationFlag::new();
        {
            let _guard = CancelOnDrop::new(f.clone());
            assert!(!f.is_cancelled());
        }
        // Guard dropped → cancellation fired.
        assert!(f.is_cancelled());
    }

    #[test]
    fn cancel_on_drop_fires_on_panic() {
        let f = CancellationFlag::new();
        let f2 = f.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = CancelOnDrop::new(f2);
            panic!("synthetic panic");
        }));
        // Despite the panic, the guard ran Drop → cancellation fired.
        assert!(f.is_cancelled());
    }

    #[test]
    fn cancel_on_drop_explicit_drop_fires() {
        let f = CancellationFlag::new();
        let guard = CancelOnDrop::new(f.clone());
        assert!(!f.is_cancelled());
        drop(guard);
        assert!(f.is_cancelled());
    }

    #[test]
    fn cancel_on_drop_flag_borrowable_during_guard_lifetime() {
        let f = CancellationFlag::new();
        let guard = CancelOnDrop::new(f.clone());
        // Borrow the flag through the guard.
        assert!(!guard.flag().is_cancelled());
        // Or fire cancellation manually before guard drops.
        guard.flag().cancel();
        assert!(f.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_on_drop_async_consumer_sees_cancellation() {
        let f = CancellationFlag::new();
        let f2 = f.clone();
        let consumer = tokio::spawn(async move {
            f2.cancelled().await;
            "cancelled"
        });
        // Spawn a producer scope that drops the guard.
        let producer = tokio::spawn(async move {
            let _guard = CancelOnDrop::new(f);
            tokio::time::sleep(Duration::from_millis(10)).await;
            // _guard drops here on task return
        });
        producer.await.unwrap();
        let result =
            tokio::time::timeout(Duration::from_millis(100), consumer)
                .await
                .expect("consumer completes after cancel")
                .expect("join ok");
        assert_eq!(result, "cancelled");
    }
}
