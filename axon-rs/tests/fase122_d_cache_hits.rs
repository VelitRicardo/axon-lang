//! §Fase 122.d — **the `cache` primitive memoises something.**
//!
//! # What was wrong
//!
//! §85 shipped `cache` complete on paper: the grammar, four type-checker laws
//! proving what is safe to memoise (`axon-T863`–`T867`), a production-hardened
//! runtime with content-addressed keys, single-flight, TTL jitter and LRU
//! eviction — with its own passing tests — and the audit vocabulary. Then it
//! deferred the last step (§85.h, *"deferred, not built"*: wire
//! `CacheRuntime::dispatch` into the dispatch path).
//!
//! §122.a measured what that left. `CacheRuntime::dispatch` had **zero
//! production callers** — only its own tests. `CacheBackend` had no enterprise
//! implementor. So `ttl:`, `key_params:`, `invalidate_on:` and
//! `default_policy:` were inert in both flavours, and `backend: redis` was not
//! a downgrade to in-process: **there was no cache**. It shipped as `Real` in
//! 2.88.0, which is the one row in the ledger with public exposure.
//!
//! # What this gate asserts, and why through the HTTP door
//!
//! Every assertion below drives `.axon` SOURCE through the real `/v1/deploy` +
//! `/v1/execute` handlers, and counts **hits on a real upstream**. That is the
//! §119.n bar: a gate is proof of a PATH only when its input is source and its
//! evidence is what production actually did. A test that called
//! `CacheRuntime::probe` directly would prove the engine — which was never in
//! doubt — and prove nothing about the cable.
//!
//! `a_second_tenant_does_not_read_the_first_tenants_cached_result` is the one
//! that matters most, and it is why §122.d.1 had to land first. The tenant is a
//! component of every cache key (D85.7/D85.11) precisely so that a
//! mis-namespacing backend still cannot leak. The tenant reaching the key was
//! not a given: `/v1/execute` wraps its whole execution in `spawn_blocking`,
//! and the verified tenant was read from a `tokio::task_local!` INSIDE that
//! blocking task, where it is gone. Measured before the fix:
//!
//! ```text
//! async="acme"   spawn_blocking="default"
//! ```
//!
//! So every request keyed under `"default"`, and mounting a cache on top of
//! that would have turned a scoping bug into a cross-tenant read. That test
//! fails on the pre-§122.d.1 runtime for exactly that reason.

#![cfg(feature = "server")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use tower::ServiceExt;

/// Hit counters for the fake upstream, one per tool slug, created on demand.
///
/// # Why a map and not a field per scenario
///
/// The first draft gave `Upstream` a named field per slug and had two pairs of
/// tests share one. Every assertion here is a DELTA on a counter, and cargo runs
/// these in parallel — so two tests on one slug interleave their deltas and both
/// fail, which is exactly what happened. Worse, the slug is part of the tool's
/// declaration fingerprint (D85.7) and therefore part of the cache key, so a
/// shared slug also means a shared cache entry across scenarios.
///
/// A map keyed by slug makes the isolation structural: a new scenario picks a
/// new slug and gets its own counter and its own key by construction, instead of
/// by someone remembering to add a field.
#[derive(Default)]
struct Upstream {
    hits: std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicUsize>>>,
}

impl Upstream {
    fn counter(&self, slug: &str) -> Arc<AtomicUsize> {
        self.hits
            .lock()
            .unwrap()
            .entry(slug.to_string())
            .or_default()
            .clone()
    }

    fn count(&self, slug: &str) -> usize {
        self.counter(slug).load(Ordering::SeqCst)
    }
}

static UPSTREAM: OnceLock<Arc<Upstream>> = OnceLock::new();

/// A local HTTP upstream that COUNTS the calls it receives, started once for
/// the whole binary.
///
/// One server and one `AXON_TOOL_BASE_URL` for every test here, because the env
/// var is process-global and cargo runs these in parallel threads — a per-test
/// server would have them overwrite each other's base URL and fail at random.
/// The scenarios stay independent by using a distinct tool slug (and so a
/// distinct counter) each.
///
/// It runs on its OWN dedicated OS thread with its own runtime, not on the
/// test's. Each `tokio::test` builds a runtime and drops it when that test
/// ends, so a server spawned onto the first test's runtime dies with that test
/// and every later test sees `connection failed`.
///
/// Worth recording how that surfaced, because it is the failure mode this file
/// exists to catch, aimed at the file itself: with the upstream unreachable,
/// `a_failed_call_is_not_memoised` PASSED — two refused connections are also
/// two calls. A counting assertion cannot tell "the vendor was called twice"
/// from "the vendor was unreachable twice" unless the test also insists the
/// calls SUCCEEDED. It does now.
fn upstream() -> Arc<Upstream> {
    UPSTREAM.get_or_init(build_upstream).clone()
}

/// Build it EXACTLY once. `get_or_init` is load-bearing, not tidiness.
///
/// The first version was a `get()`-then-`set()` pair, which is not atomic: seven
/// parallel tests each raced through the gap, each spawned its own server, and
/// each overwrote `AXON_TOOL_BASE_URL`. Only one set of counters won the
/// `OnceLock`, so the runtime dispatched to the LAST server to write the env var
/// while the assertions read the FIRST server's counters. Every test failed, and
/// every one passed in isolation — the signature of a broken instrument rather
/// than a broken subject.
///
/// Worth stating plainly because it is the same trap this file exists to close,
/// turned on the file itself: for two rounds the measurements here were reporting
/// on something other than what they claimed to measure.
fn build_upstream() -> Arc<Upstream> {
    let counters = Arc::new(Upstream::default());

    // ONE catch-all route: any `/{slug}` counts under that slug. A new scenario
    // needs no new route and no new field — it picks a slug and is isolated.
    //
    // `/flaky` is the single exception, and it is a behaviour rather than a
    // counter: it fails the FIRST time and succeeds afterwards, which is what
    // the D85.10 "errors are never memoised" assertion needs.
    let for_route = counters.clone();
    let app = axum::Router::new().route(
        "/{slug}",
        axum::routing::post(
            move |axum::extract::Path(slug): axum::extract::Path<String>, _b: String| {
                let c = for_route.clone();
                async move {
                    let n = c.counter(&slug).fetch_add(1, Ordering::SeqCst) + 1;
                    if slug == "flaky" && n == 1 {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "upstream is having a bad minute".to_string(),
                        )
                    } else if slug == "flaky" {
                        (StatusCode::OK, "recovered".to_string())
                    } else {
                        (StatusCode::OK, format!("{slug}#{n}"))
                    }
                }
            },
        ),
    );

    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            addr_tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    let addr = addr_rx.recv().expect("upstream bound");
    std::env::set_var("AXON_TOOL_BASE_URL", format!("http://{addr}"));
    counters
}

fn server_cfg() -> axon::axon_server::ServerConfig {
    axon::axon_server::ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn post(
    app: &axum::Router,
    uri: &str,
    body: serde_json::Value,
    tenant: Option<&str>,
) -> serde_json::Value {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = tenant {
        req = req.header("X-Tenant-ID", t);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

async fn deploy(app: &axum::Router, src: &str) -> serde_json::Value {
    post(
        app,
        "/v1/deploy",
        serde_json::json!({ "source": src, "source_file": "cache.axon", "backend": "stub" }),
        None,
    )
    .await
}

async fn execute(app: &axum::Router, flow: &str, tenant: Option<&str>) -> serde_json::Value {
    post(
        app,
        "/v1/execute",
        serde_json::json!({ "flow": flow, "backend": "stub" }),
        tenant,
    )
    .await
}

/// Assert the run actually executed, and did not merely return.
///
/// `/v1/execute` answers with a flow envelope — `result`, `step_audit`,
/// `execution_metrics` — and **no `success` field**; asserting on one silently
/// compares `Null` to `true` and fails for the wrong reason, which is how the
/// first draft of this file read.
///
/// The `connection failed` check is the one that earns its place: every
/// assertion here counts upstream calls, and a refused connection counts too.
/// Without this, "the vendor was called twice" and "the vendor was unreachable
/// twice" are the same measurement.
fn assert_ran(out: &serde_json::Value) {
    let result = out["result"].as_str().unwrap_or_default();
    assert!(
        !result.contains("connection failed"),
        "the upstream must be reachable, or every call-counting assertion below is \
         measuring failed connections instead of vendor hits: {out}"
    );
    assert_eq!(
        out["step_audit"]["errors"], 0,
        "the flow must complete without step errors: {out}"
    );
}

/// Drive a deployed flow through the SSE door, with a tenant header.
async fn execute_sse(app: &axum::Router, flow: &str, tenant: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .header("X-Tenant-ID", tenant)
        .body(Body::from(
            serde_json::json!({ "flow_name": flow, "backend": "stub" }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    let text = String::from_utf8(bytes).expect("utf-8 sse body");
    assert!(
        !text.contains("connection failed"),
        "the upstream must be reachable on the SSE path too: {text}"
    );
    text
}

/// A tool + a default cache + a flow that calls the tool twice with the SAME
/// arguments. `slug` selects which upstream counter it hits.
fn program(slug: &str, cache_block: &str, flow_body: &str) -> String {
    // The cache NAME is per-scenario too, and this one is not harness hygiene —
    // it is the runtime behaving correctly.
    //
    // A cache's name IS its backend namespace, so `invalidate` flushes exactly
    // that cache's entries (`cache C { invalidate_on: [Ch] }` must not empty
    // some other cache). Every scenario here originally declared `cache Memo`,
    // which made them ONE namespace — so `an_emit_on_the_declared_channel_…`
    // flushed the entries the other tests were about to assert hits on, and the
    // two tests that expect a hit ACROSS requests failed in parallel while
    // passing alone.
    //
    // The cache was right and the fixtures were wrong. Naming them apart is the
    // fix; suppressing the parallelism would have hidden a real property.
    // (`default: true` means the tool never names its cache, so renaming is
    // invisible to the rest of the program.)
    let cache_block = cache_block.replace("Memo", &format!("Memo_{slug}"));
    format!(
        "tool T {{\n\
         \x20   provider: http\n\
         \x20   runtime: {slug}\n\
         \x20   effects: <pure>\n\
         \x20   output_type: String\n\
         \x20   parameters: {{ id: String }}\n\
         }}\n\
         {cache_block}\n\
         flow Run() -> Unit {{\n{flow_body}\n}}\n"
    )
}

// ─────────────────────────────────────────────────────────────────────────────

/// 🎯 The whole sub-fase in one assertion: a `pure` tool called twice with the
/// same arguments hits the vendor ONCE.
#[tokio::test(flavor = "multi_thread")]
async fn a_pure_tool_called_twice_is_computed_once() {
    let up = upstream();
    const SLUG: &str = "enrich";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    // Read from DISK, not built in Rust. `advertised.rs` cites this file as the
    // `cache` row's fixture, and its Law requires the citation to reach a real
    // `.axon` that still compiles — a gate whose input is a Rust string proves
    // the handler, never the path (§119.n).
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fase122_d_cache/memoised_pure_tool.axon"),
    )
    .expect("the fixture `advertised.rs` cites must exist");
    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "deploy must succeed: {d}");

    let before = up.count(SLUG);
    let out = execute(&app, "Run", Some("acme")).await;
    assert_ran(&out);
    let calls = up.count(SLUG) - before;

    assert_eq!(
        calls, 1,
        "two identical calls to a `pure` tool under a default cache must reach the vendor \
         ONCE. {calls} calls means the declaration is still inert — which is exactly what \
         §122.a measured and what shipped as `Real` in 2.88.0"
    );
}

/// The negative that gives the test above its meaning: `cache: none` opts the
/// tool out, and BOTH calls reach the vendor.
///
/// Without this, `a_pure_tool_called_twice_is_computed_once` would pass just as
/// well for a runtime that had quietly stopped dispatching the second call for
/// some unrelated reason.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_program_with_cache_none_computes_twice() {
    let up = upstream();
    const SLUG: &str = "cache_none";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    let src = program(
        SLUG,
        "cache Memo { default: true }",
        "    use T(id = \"ada\")\n    use T(id = \"ada\")",
    )
    .replace("provider: http", "provider: http\n    cache: none");

    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "deploy must succeed: {d}");

    let before = up.count(SLUG);
    let out = execute(&app, "Run", Some("acme")).await;
    assert_ran(&out);
    let calls = up.count(SLUG) - before;

    assert_eq!(
        calls, 2,
        "`cache: none` must opt out; {calls} calls means the runtime memoises regardless of \
         the declaration, which is a different lie from the one §122.d fixed"
    );
}

/// 🎯 **D85.10 — an error is never memoised.**
///
/// The upstream fails once and then recovers. If the failure were cached, the
/// second run would serve it forever (up to `ttl:`) and never retry — one bad
/// minute at a vendor turning into a flow that cannot succeed, silently,
/// because from the flow's side a cached error is indistinguishable from a
/// fresh one.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_call_is_not_memoised() {
    let up = upstream();
    const SLUG: &str = "flaky";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    let src = program(
        SLUG,
        "cache Memo { default: true }",
        "    use T(id = \"ada\")",
    );
    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "{d}");

    let before = up.count(SLUG);
    let first = execute(&app, "Run", Some("acme")).await; // upstream 500s
    let second = execute(&app, "Run", Some("acme")).await; // must RETRY, not serve the 500
    let calls = up.count(SLUG) - before;

    assert_eq!(
        calls, 2,
        "the failed first call must not have been stored: a second run has to reach the \
         vendor again. {calls} call(s) means a transient failure got memoised"
    );
    // The counter alone cannot tell a retried failure from an unreachable
    // upstream — both are "two calls". The recovery is what distinguishes them.
    assert!(
        second["result"]
            .as_str()
            .unwrap_or_default()
            .contains("recovered"),
        "the second run must carry the RECOVERED body, proving the retry reached a live \
         upstream rather than failing to connect twice.\nfirst: {first}\nsecond: {second}"
    );
}

/// 🎯 **The cross-tenant assertion — and the reason §122.d.1 had to land first.**
///
/// Two tenants run the same program with the same arguments. The tenant is a
/// component of the cache key (D85.7/D85.11), so the second tenant MUST miss
/// and reach the vendor itself.
///
/// This fails on the pre-§122.d.1 runtime, and not because of the cache: the
/// `/v1/execute` handler wraps execution in `spawn_blocking`, the verified
/// tenant was read from a `tokio::task_local!` inside that blocking task, and
/// task-locals do not cross to the blocking pool. Every request keyed under the
/// `"default"` fallback, so tenant B would have been served tenant A's result.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_tenant_does_not_read_the_first_tenants_cached_result() {
    let up = upstream();
    const SLUG: &str = "tenant_iso";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    let src = program(
        SLUG,
        "cache Memo { default: true }",
        "    use T(id = \"shared\")",
    );
    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "{d}");

    let before = up.count(SLUG);

    let a1 = execute(&app, "Run", Some("acme")).await;
    assert_ran(&a1);
    let after_a1 = up.count(SLUG);
    assert_eq!(after_a1 - before, 1, "tenant acme's first call computes");

    // Same tenant, same args — a hit, so the vendor is untouched.
    let a2 = execute(&app, "Run", Some("acme")).await;
    assert_ran(&a2);
    assert_eq!(
        up.count(SLUG),
        after_a1,
        "the same tenant repeating the same call must HIT — otherwise this test could not \
         distinguish tenant isolation from a cache that never hits at all"
    );

    // A DIFFERENT tenant. Must miss.
    let b1 = execute(&app, "Run", Some("globex")).await;
    assert_ran(&b1);
    assert_eq!(
        up.count(SLUG) - after_a1,
        1,
        "tenant globex must reach the vendor itself. Serving it acme's memoised result is a \
         cross-tenant read — the exact failure D85.11 puts the tenant in the key to prevent, \
         and the one that was reachable while `/v1/execute` lost the tenant across \
         `spawn_blocking`"
    );
}

// ── §Fase 120's two-doors rule, applied ─────────────────────────────────────

/// 🎯 **The SSE door memoises too — and carries its own tenant.**
///
/// Everything above drives `/v1/execute`. A subsystem wired on one entry point
/// and dead on the other is the defect §111 named ("real-on-one-path,
/// dead-on-the-other"), §114 found twice more (`budget`, then `capacity:`/
/// `lease` inert on SSE), and §120 generalised into the rule that a subsystem's
/// doors must be audited as a pair. `cache` had two doors from the start.
///
/// The tenant half of this assertion is why §122.d.1 exists. This path had NO
/// tenant at all — `DispatchCtx::with_tenant_id` had exactly one caller in the
/// whole OSS tree, the synchronous runner — so mounting a tenant-keyed cache
/// here first would have put every SSE caller in one namespace.
#[tokio::test(flavor = "multi_thread")]
async fn the_sse_door_memoises_and_keys_by_tenant_too() {
    let up = upstream();
    const SLUG: &str = "sse_door";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    let src = program(
        SLUG,
        "cache Memo { default: true }",
        "    use T(id = \"carol\")",
    );
    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "deploy must succeed: {d}");

    let before = up.count(SLUG);

    execute_sse(&app, "Run", "acme").await;
    let after_first = up.count(SLUG);
    assert_eq!(after_first - before, 1, "the first SSE run computes");

    execute_sse(&app, "Run", "acme").await;
    assert_eq!(
        up.count(SLUG),
        after_first,
        "the second SSE run with the same tenant and arguments must HIT. A miss here means \
         the cache is mounted on the sync runner only — `cache` wired on one door and dead \
         on the other, which is the shape §111/§114/§120 each found and named"
    );

    execute_sse(&app, "Run", "globex").await;
    assert_eq!(
        up.count(SLUG) - after_first,
        1,
        "a different tenant on the SSE path must MISS. Before §122.d.1 this path never \
         called `with_tenant_id`, so `ctx.tenant_id` was the empty string for everyone and \
         every SSE caller would have shared one cache namespace"
    );
}

/// 🎯 **One tier, not one per door.**
///
/// A value computed through `/v1/execute` is served to `/v1/execute/sse` for the
/// same tenant. This is the assertion that separates a cache from a decoration,
/// and it is the §114.a lesson transposed: a `BudgetGate` built per request
/// starts full every time and can never deny; a `CacheRuntime` built per run
/// starts empty every time and can never hit.
///
/// `CacheRuntime::process_local` is what makes this pass — the BACKEND is
/// process-wide so entries outlive a run, while the TENANT comes from the run
/// so D85.11 isolation still holds. Sharing the whole runtime instead would
/// have passed this test and failed the two above.
#[tokio::test(flavor = "multi_thread")]
async fn a_value_computed_on_one_door_is_served_on_the_other() {
    let up = upstream();
    const SLUG: &str = "shared_tier";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    let src = program(
        SLUG,
        "cache Memo { default: true }",
        "    use T(id = \"dave\")",
    );
    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "deploy must succeed: {d}");

    let before = up.count(SLUG);
    let sync_run = execute(&app, "Run", Some("acme")).await;
    assert_ran(&sync_run);
    assert_eq!(
        up.count(SLUG) - before,
        1,
        "the sync run computes"
    );

    execute_sse(&app, "Run", "acme").await;
    assert_eq!(
        up.count(SLUG) - before,
        1,
        "the SSE run must be served the value the SYNC run computed. A second vendor call \
         means each door holds its own tier, so nothing survives a request — memoisation \
         that only works within a single run is the cache equivalent of a token bucket \
         rebuilt per request (§114.a), and every test of it would pass"
    );
}

/// 🎯 **`invalidate_on:` stops being inert.**
///
/// An `emit` on a channel the cache names flushes its namespace, so the next
/// call recomputes. §85 typed the reference (`axon-T864` proves the channel
/// exists) and flushed nothing — the worst of the three inert fields for a
/// regulated adopter, who reads a declared invalidation channel as "stale data
/// is interruptible" when it was neither bounded nor interruptible.
#[tokio::test(flavor = "multi_thread")]
async fn an_emit_on_the_declared_channel_flushes_the_cache() {
    let up = upstream();
    const SLUG: &str = "invalidation";
    let (app, _s) = axon::axon_server::build_router_with_state(server_cfg());

    let src = format!(
        "channel Orders {{ message: String }}\n{}",
        program(
            SLUG,
            "cache Memo { default: true  invalidate_on: [Orders] }",
            "    use T(id = \"bob\")\n    \
             let payload = \"changed\"\n    \
             emit Orders(payload)\n    \
             use T(id = \"bob\")",
        )
    );
    let d = deploy(&app, &src).await;
    assert_eq!(d["success"], true, "deploy must succeed: {d}");

    let before = up.count(SLUG);
    let out = execute(&app, "Run", Some("acme")).await;
    assert_ran(&out);
    let calls = up.count(SLUG) - before;

    assert_eq!(
        calls, 2,
        "the `emit Orders(…)` between the two identical calls must have flushed the \
         namespace, so the second call recomputes. {calls} call(s) means `invalidate_on:` is \
         still a typed reference to nothing"
    );
}
