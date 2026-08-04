//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 114 (owed debt) — **`scrape.concurrency` is a REAL bound: the crawl runs
//! up to N fetches in flight, not one page at a time.**
//!
//! Before this, the crawl loop fetched a single page, awaited it, then the next —
//! fully sequential — while `scrape.concurrency` was parsed, type-checked, AND
//! documented as honored ("bounded by max_pages / max_depth / **concurrency**").
//! A documented behavior the code did not have (the §112.f / §111 pattern). The
//! crawl is now a bounded-concurrent BFS.
//!
//! This gate proves it behaviorally against a local server that COUNTS concurrent
//! in-flight fetches: `concurrency: 4` must drive more than one fetch at once (peak
//! > 1) but never more than four (peak ≤ 4); `concurrency: 1` must stay strictly
//! sequential (peak == 1). Deterministic: each leaf handler holds its slot for a
//! fixed sleep, so a batch of N concurrent fetches reliably overlaps.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axon::cancel_token::CancellationFlag;
use axon::scrape_tool::ScrapeStreamingTool;
use axon::tool_registry::{ScrapeConfig, ToolEntry, ToolSource};
use axon::tool_trait::{Tool, ToolContext};

use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use futures::StreamExt;
use tokio::net::TcpListener;

#[derive(Default)]
struct Counters {
    in_flight: AtomicUsize,
    peak: AtomicUsize,
    leaves_served: AtomicUsize,
}

/// The seed page links to eight leaves via ROOT-RELATIVE hrefs (the crawler's
/// `join_url` resolves them against the seed's host). The seed itself is NOT
/// counted, so `peak` reflects only leaf concurrency.
async fn seed() -> Html<String> {
    let links: String = (0..8)
        .map(|i| format!("<a href=\"/p/{i}\">p{i}</a>"))
        .collect();
    Html(format!("<html><body>{links}</body></html>"))
}

/// Each leaf holds its slot for a fixed window, so N concurrent fetches overlap
/// observably. Records the peak simultaneous count.
async fn leaf(State(c): State<Arc<Counters>>) -> Html<&'static str> {
    let now = c.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    c.peak.fetch_max(now, Ordering::SeqCst);
    c.leaves_served.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(80)).await;
    c.in_flight.fetch_sub(1, Ordering::SeqCst);
    Html("<html><body>leaf</body></html>")
}

async fn spawn_server() -> (String, Arc<Counters>) {
    let counters = Arc::new(Counters::default());
    let router = Router::new()
        .route("/", get(seed))
        .route("/p/{id}", get(leaf))
        .with_state(counters.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    (format!("http://{addr}/"), counters)
}

fn crawl_entry(concurrency: i64) -> ToolEntry {
    let cfg = ScrapeConfig {
        // `follow` non-empty enables anchor expansion; max_depth 1 expands the
        // seed's links but not the leaves'; max_pages generous so the success
        // budget never masks the concurrency bound; robots off so no /robots.txt
        // fetch muddies the in-flight count.
        follow: "a".to_string(),
        max_depth: 1,
        max_pages: 50,
        concurrency,
        respect_robots: false,
        ..Default::default()
    };
    ToolEntry {
        name: "Crawl".to_string(),
        provider: "scrape_crawl".to_string(),
        timeout: "10s".to_string(),
        runtime: String::new(),
        resource_ref: String::new(),
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: vec!["network".to_string(), "web".to_string()],
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming: true,
        scrape: Some(cfg),
    }
}

/// Drive the crawl to completion, returning the number of page chunks emitted.
async fn run_crawl(base: &str, concurrency: i64) -> usize {
    let tool = ScrapeStreamingTool::from_entry(&crawl_entry(concurrency));
    let ctx = ToolContext {
        cancel: CancellationFlag::new(),
        trace_id: 0,
    };
    let mut stream = tool.stream(format!("{{\"url\":\"{base}\"}}"), ctx).await;
    let mut pages = 0usize;
    while let Some(chunk) = stream.next().await {
        if chunk.is_terminator() {
            break;
        }
        pages += 1;
    }
    pages
}

/// 🎯 **`concurrency: 4` drives more than one fetch at once, and never more than
/// four.** The peak simultaneous leaf count observed by the server is the proof:
/// > 1 (the crawl is genuinely concurrent, not the old sequential loop) and ≤ 4
/// (the declared bound is real, not unbounded fan-out).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_four_runs_up_to_four_fetches_at_once() {
    let (base, counters) = spawn_server().await;
    let pages = run_crawl(&base, 4).await;

    let peak = counters.peak.load(Ordering::SeqCst);
    assert!(
        peak > 1,
        "concurrency: 4 must run more than one fetch at once — the crawl is bounded-CONCURRENT, \
         not the old sequential loop. Peak observed: {peak}"
    );
    assert!(
        peak <= 4,
        "concurrency: 4 must NEVER run more than four fetches at once — the declared bound is \
         real. Peak observed: {peak}"
    );
    // All eight leaves (plus the seed) were crawled — the bound throttles, never drops.
    assert_eq!(counters.leaves_served.load(Ordering::SeqCst), 8);
    assert_eq!(pages, 9, "seed + eight leaves = nine page chunks");
}

/// **`concurrency: 1` stays strictly sequential** — the tightest bound reproduces
/// the pre-§114 crawl exactly (never accidental concurrency).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_one_is_strictly_sequential() {
    let (base, counters) = spawn_server().await;
    let pages = run_crawl(&base, 1).await;

    assert_eq!(
        counters.peak.load(Ordering::SeqCst),
        1,
        "concurrency: 1 must fetch one page at a time — no fetch overlaps another"
    );
    assert_eq!(pages, 9, "seed + eight leaves = nine page chunks");
}
