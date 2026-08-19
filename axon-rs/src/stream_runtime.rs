//! Runtime implementation of `Stream<T>` with the four closed
//! backpressure policies from [`crate::stream_effect`].
//!
//! v1.4.0 — Temporal Algebraic Effects.
//!
//! A [`Stream<T>`] is a bounded async channel with an explicit
//! backpressure handler. Every producer push goes through the
//! policy: the policy is not a fallback but the primary control
//! point, which is why the compiler makes it mandatory.
//!
//! Naming is intentionally generic. The existing [`crate::runtime`]
//! tree keeps LLM-token streaming (semantic streaming with
//! epistemic gradient) separate — that's higher-level, cognitive.
//! `Stream<T>` is the byte/frame/event-level primitive that
//! websocket ingress, microphone taps, and file uploads land into.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

use crate::stream_effect::{BackpressureAnnotation, BackpressurePolicy};

// ── Policy-specific inputs ───────────────────────────────────────────

/// Pure (effect-free) degradation function the runtime applies when
/// the [`BackpressurePolicy::DegradeQuality`] policy fires. The
/// function MUST be deterministic — the checker rejects impure
/// degraders at compile time.
pub type DegradationFn<T> = Arc<dyn Fn(T) -> T + Send + Sync>;

/// Error surfaced to a producer whose push was dropped by [`Fail`]
/// or which timed out waiting for [`PauseUpstream`]. Also the error
/// a consumer sees when the stream was cancelled.
#[derive(Debug)]
pub enum StreamError {
    /// [`BackpressurePolicy::Fail`] triggered — buffer was full.
    Overflow {
        policy: BackpressurePolicy,
        buffer_capacity: usize,
    },
    /// The stream was cancelled (producer dropped, consumer gave up).
    Cancelled,
    /// Policy is `DegradeQuality` but no degrader was attached.
    MissingDegrader,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Overflow {
                policy,
                buffer_capacity,
            } => write!(
                f,
                "stream overflow under policy {policy} (capacity={buffer_capacity})"
            ),
            Self::Cancelled => write!(f, "stream cancelled"),
            Self::MissingDegrader => write!(
                f,
                "DegradeQuality policy requires a degrader function; none attached"
            ),
        }
    }
}

impl std::error::Error for StreamError {}

// ── Metrics (counters only — tenant_id tags are out of scope for
//                  the primitive; adopters wrap and tag per-tenant) ──

#[derive(Debug, Default)]
pub struct StreamMetrics {
    pub items_pushed: AtomicU64,
    pub items_delivered: AtomicU64,
    pub drop_oldest_hits: AtomicU64,
    pub degrade_quality_hits: AtomicU64,
    pub pause_upstream_blocks: AtomicU64,
    pub fail_overflows: AtomicU64,
}

impl StreamMetrics {
    pub fn snapshot(&self) -> StreamMetricsSnapshot {
        StreamMetricsSnapshot {
            items_pushed: self.items_pushed.load(Ordering::Relaxed),
            items_delivered: self.items_delivered.load(Ordering::Relaxed),
            drop_oldest_hits: self.drop_oldest_hits.load(Ordering::Relaxed),
            degrade_quality_hits: self
                .degrade_quality_hits
                .load(Ordering::Relaxed),
            pause_upstream_blocks: self
                .pause_upstream_blocks
                .load(Ordering::Relaxed),
            fail_overflows: self.fail_overflows.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamMetricsSnapshot {
    pub items_pushed: u64,
    pub items_delivered: u64,
    pub drop_oldest_hits: u64,
    pub degrade_quality_hits: u64,
    pub pause_upstream_blocks: u64,
    pub fail_overflows: u64,
}

// ── The stream itself ────────────────────────────────────────────────

struct Inner<T> {
    buffer: VecDeque<T>,
    capacity: usize,
    closed: bool,
}

/// Bounded async stream with a declared backpressure policy.
/// Clone-able handle — producer and consumer hold the same `Arc`.
pub struct Stream<T> {
    inner: Arc<Mutex<Inner<T>>>,
    not_empty: Arc<Notify>,
    not_full: Arc<Notify>,
    policy: BackpressurePolicy,
    annotation: BackpressureAnnotation,
    degrader: Option<DegradationFn<T>>,
    pub metrics: Arc<StreamMetrics>,
}

impl<T> Clone for Stream<T> {
    fn clone(&self) -> Self {
        Stream {
            inner: Arc::clone(&self.inner),
            not_empty: Arc::clone(&self.not_empty),
            not_full: Arc::clone(&self.not_full),
            policy: self.policy,
            annotation: self.annotation.clone(),
            degrader: self.degrader.clone(),
            metrics: Arc::clone(&self.metrics),
        }
    }
}

impl<T: Send + 'static> Stream<T> {
    /// Build a stream whose policy is `DropOldest` / `PauseUpstream` /
    /// `Fail`. Use [`Stream::with_degrader`] for `DegradeQuality`.
    pub fn new(capacity: usize, annotation: BackpressureAnnotation) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buffer: VecDeque::with_capacity(capacity),
                capacity,
                closed: false,
            })),
            not_empty: Arc::new(Notify::new()),
            not_full: Arc::new(Notify::new()),
            policy: annotation.policy,
            annotation,
            degrader: None,
            metrics: Arc::new(StreamMetrics::default()),
        }
    }

    /// Build a `DegradeQuality` stream with the mandatory degrader.
    pub fn with_degrader(
        capacity: usize,
        annotation: BackpressureAnnotation,
        degrader: DegradationFn<T>,
    ) -> Self {
        let mut s = Self::new(capacity, annotation);
        s.degrader = Some(degrader);
        s
    }

    pub fn policy(&self) -> BackpressurePolicy {
        self.policy
    }

    pub fn annotation(&self) -> &BackpressureAnnotation {
        &self.annotation
    }

    /// Push an item. Dispatches to the declared policy.
    pub async fn push(&self, item: T) -> Result<(), StreamError> {
        self.metrics.items_pushed.fetch_add(1, Ordering::Relaxed);
        match self.policy {
            BackpressurePolicy::DropOldest => self.push_drop_oldest(item).await,
            BackpressurePolicy::DegradeQuality => {
                self.push_degrade_quality(item).await
            }
            BackpressurePolicy::PauseUpstream => {
                self.push_pause_upstream(item).await
            }
            BackpressurePolicy::Fail => self.push_fail(item).await,
        }
    }

    async fn push_drop_oldest(&self, item: T) -> Result<(), StreamError> {
        let mut g = self.inner.lock().await;
        if g.closed {
            return Err(StreamError::Cancelled);
        }
        if g.buffer.len() >= g.capacity {
            g.buffer.pop_front();
            self.metrics
                .drop_oldest_hits
                .fetch_add(1, Ordering::Relaxed);
        }
        g.buffer.push_back(item);
        self.not_empty.notify_one();
        Ok(())
    }

    async fn push_degrade_quality(&self, item: T) -> Result<(), StreamError> {
        let mut g = self.inner.lock().await;
        if g.closed {
            return Err(StreamError::Cancelled);
        }
        let value = if g.buffer.len() >= g.capacity {
            // v1.4.2 — the degrader lookup belongs in the overflow
            // branch, not at function entry. A `Stream::new` built
            // without `with_degrader` must still accept pushes while
            // the buffer has room; only when we actually need to
            // degrade an incoming item is the absence of a degrader
            // a policy violation. Failing closed early prevented
            // even the first push from succeeding.
            let degrader = self
                .degrader
                .as_ref()
                .ok_or(StreamError::MissingDegrader)?
                .clone();
            self.metrics
                .degrade_quality_hits
                .fetch_add(1, Ordering::Relaxed);
            // Drop oldest degraded + push new item at degraded
            // quality — caller decides whether "degraded" means
            // lower bitrate, coarser resolution, etc.
            g.buffer.pop_front();
            degrader(item)
        } else {
            item
        };
        g.buffer.push_back(value);
        self.not_empty.notify_one();
        Ok(())
    }

    async fn push_pause_upstream(&self, item: T) -> Result<(), StreamError> {
        loop {
            {
                let mut g = self.inner.lock().await;
                if g.closed {
                    return Err(StreamError::Cancelled);
                }
                if g.buffer.len() < g.capacity {
                    g.buffer.push_back(item);
                    self.not_empty.notify_one();
                    return Ok(());
                }
            }
            self.metrics
                .pause_upstream_blocks
                .fetch_add(1, Ordering::Relaxed);
            self.not_full.notified().await;
        }
    }

    async fn push_fail(&self, item: T) -> Result<(), StreamError> {
        let mut g = self.inner.lock().await;
        if g.closed {
            return Err(StreamError::Cancelled);
        }
        if g.buffer.len() >= g.capacity {
            self.metrics
                .fail_overflows
                .fetch_add(1, Ordering::Relaxed);
            return Err(StreamError::Overflow {
                policy: BackpressurePolicy::Fail,
                buffer_capacity: g.capacity,
            });
        }
        g.buffer.push_back(item);
        self.not_empty.notify_one();
        Ok(())
    }

    /// Pull the next item. Returns `None` when the stream is closed
    /// AND the buffer drained — analogous to a closed channel.
    pub async fn pop(&self) -> Option<T> {
        loop {
            {
                let mut g = self.inner.lock().await;
                if let Some(item) = g.buffer.pop_front() {
                    self.not_full.notify_one();
                    self.metrics
                        .items_delivered
                        .fetch_add(1, Ordering::Relaxed);
                    return Some(item);
                }
                if g.closed {
                    return None;
                }
            }
            self.not_empty.notified().await;
        }
    }

    /// Signal end-of-stream. Wakes pending consumers; idempotent.
    pub async fn close(&self) {
        let mut g = self.inner.lock().await;
        g.closed = true;
        drop(g);
        self.not_empty.notify_waiters();
        self.not_full.notify_waiters();
    }

    /// Current buffer depth. Snapshot only — use for dashboards,
    /// never for flow-control (race with any concurrent push).
    pub async fn depth(&self) -> usize {
        self.inner.lock().await.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream_effect::parse_backpressure_annotation;

    fn annotation(slug: &str) -> BackpressureAnnotation {
        parse_backpressure_annotation(slug).expect("valid slug")
    }

    #[tokio::test]
    async fn drop_oldest_replaces_oldest_under_pressure() {
        let s: Stream<i32> = Stream::new(2, annotation("drop_oldest"));
        s.push(1).await.unwrap();
        s.push(2).await.unwrap();
        s.push(3).await.unwrap(); // triggers drop_oldest -> removes 1
        assert_eq!(s.pop().await, Some(2));
        assert_eq!(s.pop().await, Some(3));
        let m = s.metrics.snapshot();
        assert_eq!(m.drop_oldest_hits, 1);
        assert_eq!(m.items_pushed, 3);
        assert_eq!(m.items_delivered, 2);
    }

    #[tokio::test]
    async fn degrade_quality_applies_degrader() {
        let degrader: DegradationFn<i32> = Arc::new(|x| x / 2);
        let s: Stream<i32> = Stream::with_degrader(
            2,
            annotation("degrade_quality"),
            degrader,
        );
        s.push(100).await.unwrap();
        s.push(200).await.unwrap();
        s.push(300).await.unwrap(); // overflow → 300/2 pushed
        assert_eq!(s.pop().await, Some(200));
        assert_eq!(s.pop().await, Some(150));
        let m = s.metrics.snapshot();
        assert_eq!(m.degrade_quality_hits, 1);
    }

    #[tokio::test]
    async fn degrade_quality_without_degrader_errors() {
        let s: Stream<i32> = Stream::new(1, annotation("degrade_quality"));
        s.push(1).await.unwrap();
        // Second push overflows; without a degrader the policy fails
        // closed with MissingDegrader.
        let err = s.push(2).await.unwrap_err();
        matches!(err, StreamError::MissingDegrader);
    }

    #[tokio::test]
    async fn pause_upstream_blocks_until_consumer_drains() {
        let s: Stream<i32> = Stream::new(1, annotation("pause_upstream"));
        s.push(1).await.unwrap();

        // Spawn a consumer that pops after a short delay.
        let consumer = {
            let s = s.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                s.pop().await
            })
        };

        // Producer push blocks until consumer drains.
        s.push(2).await.unwrap();
        assert_eq!(consumer.await.unwrap(), Some(1));
        assert_eq!(s.pop().await, Some(2));
        let m = s.metrics.snapshot();
        assert!(m.pause_upstream_blocks >= 1);
    }

    #[tokio::test]
    async fn fail_policy_errors_on_overflow() {
        let s: Stream<i32> = Stream::new(1, annotation("fail"));
        s.push(1).await.unwrap();
        let err = s.push(2).await.unwrap_err();
        match err {
            StreamError::Overflow {
                policy,
                buffer_capacity,
            } => {
                assert_eq!(policy, BackpressurePolicy::Fail);
                assert_eq!(buffer_capacity, 1);
            }
            other => panic!("expected Overflow, got {other:?}"),
        }
        let m = s.metrics.snapshot();
        assert_eq!(m.fail_overflows, 1);
    }

    #[tokio::test]
    async fn close_drains_buffer_then_signals_end() {
        let s: Stream<i32> = Stream::new(4, annotation("fail"));
        s.push(1).await.unwrap();
        s.push(2).await.unwrap();
        s.close().await;
        assert_eq!(s.pop().await, Some(1));
        assert_eq!(s.pop().await, Some(2));
        assert_eq!(s.pop().await, None); // closed + drained
    }

    #[tokio::test]
    async fn push_after_close_errors() {
        let s: Stream<i32> = Stream::new(4, annotation("fail"));
        s.close().await;
        let err = s.push(99).await.unwrap_err();
        matches!(err, StreamError::Cancelled);
    }
}
