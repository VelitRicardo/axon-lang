//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.x.e — Cancellation observed INSIDE the reqwest body.
//!
//! D3 in motion: every per-provider `Backend::stream()` impl wraps
//! its returned chunk stream with `sse_streaming::cancel_aware`,
//! threaded via the new `ChatRequest.cancel` field. The wrapper
//! races each next-chunk poll against `cancel.cancelled()` so the
//! moment any clone of the flag fires `cancel.cancel()`, the
//! consumer's `next().await` returns `None` within ≤100ms p95.
//! The dropped wrapper releases the inner reqwest body which
//! aborts the upstream HTTP request mid-stream — no further bytes
//! consumed, no further token quota spent.
//!
//! # The measurable invariant
//!
//! **p95 latency from `flag.cancel()` to consumer's next `None`
//! observation MUST be ≤ 100ms** under a local-loopback HTTP
//! server emitting one SSE chunk per second (so the consumer is
//! blocked on the inner body when cancel fires). The test below
//! asserts a 30-trial p95 against a 100ms budget.
//!
//! # Why local-loopback (not real provider) in this lane
//!
//! Real-provider HTTP roundtrips against Anthropic / OpenAI live
//! in `fase33x_real_provider.yml` (Fase 33.x.j, opt-in via
//! `AXON_RUN_REAL_PROVIDER_TEST` secret). The cancel-inside-body
//! invariant is a runtime / wire concern; the slow-drip mock
//! captures the same code path (`cancel_aware` wrapping a
//! `reqwest::Response::bytes_stream`) deterministically + without
//! API-key flakiness.

#![allow(clippy::needless_return)]

use std::convert::Infallible;
use std::time::{Duration, Instant};

use axon::backends::{Backend, ChatRequest, Message, OpenAIBackend, StubBackend};
use axon::cancel_token::CancellationFlag;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, Response, StatusCode};
use axum::routing::post;
use axum::Router;
use futures::StreamExt;
use tokio::net::TcpListener;

// ── Slow-drip mock server ──────────────────────────────────────────

/// Spawn an axum server bound to `127.0.0.1:0`. Returns the
/// `http://addr` base URL.
async fn spawn_test_server(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    format!("http://{addr}")
}

/// SSE handler that drips one chunk every 1 second up to N chunks.
/// The consumer blocks on the next chunk arrival between drips —
/// firing cancel during a drip is the test case for the
/// cancel-inside-body invariant.
async fn slow_drip_openai_handler(_req: Request) -> Result<Response<Body>, Infallible> {
    use futures::stream;
    use tokio::time::sleep;

    let total_chunks = 30usize; // 30 seconds of slow drip if uncancelled.
    let drip = stream::unfold(0usize, move |i| async move {
        if i >= total_chunks {
            return None;
        }
        if i == 0 {
            // First chunk arrives immediately so the consumer can
            // observe + then trigger cancel. Subsequent chunks
            // drip slowly.
            let data = format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"c{i}\"}},\"finish_reason\":null}}]}}\n\n"
            );
            Some((Ok::<_, Infallible>(axum::body::Bytes::from(data)), i + 1))
        } else {
            sleep(Duration::from_secs(1)).await;
            let data = format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"c{i}\"}},\"finish_reason\":null}}]}}\n\n"
            );
            Some((Ok::<_, Infallible>(axum::body::Bytes::from(data)), i + 1))
        }
    });
    let body = Body::from_stream(drip);
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .expect("response builder"))
}

async fn spawn_slow_drip_openai_mock() -> String {
    let router = Router::new().route("/v1/chat/completions", post(slow_drip_openai_handler));
    spawn_test_server(router).await
}

fn make_openai_backend(base: &str) -> OpenAIBackend {
    OpenAIBackend::with_api_key(Some("test-key".into())).with_base_url(base.to_string())
}

fn make_request(cancel: CancellationFlag) -> ChatRequest {
    ChatRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![Message::user("hi")],
        stream: true,
        cancel,
        ..Default::default()
    }
}

// ── §1 — Already-cancelled flag → stream yields None immediately ──

#[tokio::test]
async fn pre_cancelled_flag_terminates_stream_immediately() {
    let cancel = CancellationFlag::new();
    cancel.cancel(); // pre-fire BEFORE stream construction

    let base = spawn_slow_drip_openai_mock().await;
    let backend = make_openai_backend(&base);
    let stream = backend
        .stream(make_request(cancel.clone()))
        .await
        .expect("stream construction succeeds even if pre-cancelled");

    let start = Instant::now();
    let mut delivered = 0usize;
    let mut stream = stream;
    while let Some(_item) = stream.next().await {
        delivered += 1;
    }
    let elapsed = start.elapsed();

    assert_eq!(delivered, 0, "pre-cancelled flag MUST deliver zero chunks");
    assert!(
        elapsed < Duration::from_millis(100),
        "pre-cancelled stream MUST terminate within 100ms, got {:?}",
        elapsed
    );
}

// ── §2 — D3 measurable invariant: p95 cancel-to-None ≤ 100ms ──────

#[tokio::test]
async fn d3_p95_cancel_to_none_within_100ms_30_trials() {
    let base = spawn_slow_drip_openai_mock().await;
    let mut latencies: Vec<Duration> = Vec::with_capacity(30);

    for _ in 0..30 {
        let backend = make_openai_backend(&base);
        let cancel = CancellationFlag::new();
        let mut stream = backend
            .stream(make_request(cancel.clone()))
            .await
            .expect("stream construction");

        // Consume the first chunk (arrives immediately).
        let first = stream.next().await.expect("first chunk arrives");
        let _ = first.expect("ok chunk");

        // The next chunk is 1s away. Fire cancel + measure how
        // long the next .next().await takes to return None.
        let cancel_at = Instant::now();
        cancel.cancel();
        // The first await after cancel: should return None within
        // the 100ms budget (the wrapper's biased select wakes on
        // the cancel Notify ahead of the inner body).
        let next = stream.next().await;
        let latency = cancel_at.elapsed();
        latencies.push(latency);

        assert!(
            next.is_none(),
            "cancel MUST drive the next() to None (got Some after cancel)"
        );
    }

    // p95 over 30 samples = 28th-percentile element (0-indexed:
    // `latencies.sort(); latencies[28]`).
    latencies.sort();
    let p50 = latencies[15];
    let p95 = latencies[28];
    let max = *latencies.last().unwrap();
    eprintln!("cancel→None latencies (30 trials): p50={p50:?} p95={p95:?} max={max:?}");

    assert!(
        p95 < Duration::from_millis(100),
        "D3 invariant violated: p95 cancel→None = {p95:?} (budget 100ms). \
         Full distribution: p50={p50:?} max={max:?}",
        p95 = p95,
        p50 = p50,
        max = max
    );
}

// ── §3 — No cancel → all chunks delivered (no regression) ─────────

#[tokio::test]
async fn no_cancel_delivers_chunks_normally() {
    let base = spawn_slow_drip_openai_mock().await;
    let backend = make_openai_backend(&base);
    let cancel = CancellationFlag::new();
    let mut stream = backend
        .stream(make_request(cancel.clone()))
        .await
        .expect("stream construction");

    // Consume the first 2 chunks (immediate + 1s drip) to prove
    // the cancel-aware wrapper doesn't break the happy path.
    let first = stream.next().await.expect("first chunk");
    assert!(first.is_ok());
    let second = stream.next().await.expect("second chunk after 1s drip");
    assert!(second.is_ok());
    // Now cancel to terminate so the test finishes quickly.
    cancel.cancel();
    let _ = stream.next().await; // None
}

// ── §4 — Cancel via cloned flag from independent task ─────────────

#[tokio::test]
async fn cancel_propagates_from_independent_task_clone() {
    let base = spawn_slow_drip_openai_mock().await;
    let backend = make_openai_backend(&base);
    let cancel = CancellationFlag::new();
    let cancel_for_remote = cancel.clone();

    // Independent task that fires cancel after 200ms.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel_for_remote.cancel();
    });

    let mut stream = backend
        .stream(make_request(cancel.clone()))
        .await
        .expect("stream construction");
    let start = Instant::now();
    // Walk to terminal None.
    while let Some(_item) = stream.next().await {}
    let elapsed = start.elapsed();
    // Remote cancel fires at 200ms; consumer should see None
    // within 200ms + 100ms budget = 300ms total wall-clock.
    assert!(
        elapsed < Duration::from_millis(400),
        "remote-cancel propagation slow: {elapsed:?}"
    );
}

// ── §5 — Stub backend honors the cancel field uniformly ───────────

#[tokio::test]
async fn stub_backend_honors_cancel_field() {
    // Stub emits 1 chunk immediately; the cancel-aware wrap should
    // still terminate immediately if pre-cancelled.
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let backend = StubBackend::new();
    let mut stream = backend
        .stream(make_request(cancel.clone()))
        .await
        .expect("stub stream");
    let start = Instant::now();
    let mut count = 0;
    while let Some(_c) = stream.next().await {
        count += 1;
    }
    assert!(start.elapsed() < Duration::from_millis(50));
    assert_eq!(count, 0, "pre-cancelled stub MUST yield zero chunks");
}

#[tokio::test]
async fn stub_backend_without_cancel_yields_one_chunk() {
    // Sanity check that the cancel-aware wrap doesn't degrade the
    // happy path for stub backend (used by 33.x.b's wire byte-compat
    // tests).
    let backend = StubBackend::new();
    let cancel = CancellationFlag::new(); // uncancelled
    let mut stream = backend
        .stream(make_request(cancel))
        .await
        .expect("stub stream");
    let mut count = 0;
    while let Some(_c) = stream.next().await {
        count += 1;
    }
    assert_eq!(count, 1);
}

// ── §6 — cancel_aware wrapper direct (no backend) ─────────────────

#[tokio::test]
async fn cancel_aware_adapter_direct_terminates_promptly() {
    use axon::backends::sse_streaming::cancel_aware;
    use std::pin::Pin;

    // Build a synthetic stream that ticks every 200ms forever.
    let infinite: Pin<Box<dyn futures::Stream<Item = u64> + Send>> = Box::pin(
        futures::stream::unfold(0u64, |i| async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Some((i, i + 1))
        }),
    );
    let cancel = CancellationFlag::new();
    let mut wrapped = cancel_aware(infinite, cancel.clone());

    // Pop one item to confirm the wrapper passes through.
    let v0 = wrapped.next().await.expect("first tick");
    assert_eq!(v0, 0);

    // Fire cancel + assert next() resolves to None within 100ms
    // (the inner is sleeping for 200ms; without cancel-awareness
    // we'd see the second tick or wait the full sleep).
    let cancel_at = Instant::now();
    cancel.cancel();
    let next = wrapped.next().await;
    let latency = cancel_at.elapsed();
    assert!(next.is_none());
    assert!(
        latency < Duration::from_millis(100),
        "cancel→None on synthetic stream: {latency:?} (budget 100ms)"
    );
}

#[tokio::test]
async fn cancel_aware_already_cancelled_returns_none_without_polling_inner() {
    use axon::backends::sse_streaming::cancel_aware;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_for_inner = polls.clone();
    let inner: Pin<Box<dyn futures::Stream<Item = u64> + Send>> = Box::pin(
        futures::stream::unfold((), move |_| {
            let polls = polls_for_inner.clone();
            async move {
                polls.fetch_add(1, Ordering::SeqCst);
                Some((42, ()))
            }
        }),
    );
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let mut wrapped = cancel_aware(inner, cancel);
    let first = wrapped.next().await;
    assert!(first.is_none());
    // The fast path in `cancel_aware` returns None WITHOUT polling
    // the inner stream when cancel is already set.
    assert_eq!(
        polls.load(Ordering::SeqCst),
        0,
        "inner stream MUST NOT be polled when cancel is already set"
    );
}

// ── §7 — Multi-step flow: cancel applies per-step uniformly ───────
//
// The HTTP-level path (`execute_sse_handler` → `server_execute_streaming`
// → `run_streaming_async_path`) installs the cancel flag on every
// step's ChatRequest. Closing the SSE response fires `CancelOnDrop`
// → `cancel.cancel()` → the in-flight step's stream wrapper sees
// the cancel + returns None → the per-step loop breaks. This is
// asserted at the HTTP layer by Fase 33.f's cancel-safety suite
// (which 33.x.b/c/d/e preserve). This test asserts the trait-layer
// invariant that the cancel field on ChatRequest propagates
// correctly into the returned stream — i.e. the wrap is wired.

#[tokio::test]
async fn chat_request_carries_cancel_field_observable_on_returned_stream() {
    let backend = StubBackend::new();
    let cancel = CancellationFlag::new();
    let req = make_request(cancel.clone());
    let mut stream = backend.stream(req).await.expect("stub stream");
    // Fire cancel BEFORE consuming. Stub emits 1 chunk; if the
    // cancel wrap is wired, the consumer sees zero chunks.
    cancel.cancel();
    let mut count = 0;
    while let Some(_c) = stream.next().await {
        count += 1;
    }
    assert_eq!(count, 0);
}
