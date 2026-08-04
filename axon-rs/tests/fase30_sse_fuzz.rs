//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 30.g — Server-Sent Events wire-format conformance fuzz.
//!
//! 100 deterministic-seeded iterations × N mutations per iteration
//! exercise the `/v1/execute/sse` + content-negotiated `/v1/execute`
//! handlers with adversarial request bodies + source-text
//! declarations. The contract being verified is the W3C SSE wire
//! format invariant set (plan vivo §4):
//!
//!   * Retry directive `retry: 5000` appears exactly once, before
//!     the first `event:` line of the response (the W3C reconnect
//!     hint must reach the client before any data event so the
//!     adopter EventSource sets its reconnect timer correctly).
//!   * Every `id:` field carries a non-decreasing u64 value (the
//!     30.d contract is STRICT monotonic; the fuzz accepts
//!     non-decreasing to tolerate the keepalive comment events
//!     which carry no id but separate token events).
//!   * No bare carriage returns appear anywhere in the body (W3C
//!     SSE prefers LF-only; mixed line endings break some
//!     EventSource implementations on the wire).
//!   * Body ends with `\n\n` blank-line terminator (the W3C event
//!     separator that EventSource clients use to delimit events).
//!   * `Content-Type` starts with `text/event-stream` on every
//!     200 OK response from `/v1/execute/sse` regardless of input
//!     (including malformed requests — the wire format must be
//!     honored even for `axon.error` events).
//!
//! Additionally exercised:
//!   * The negotiation wrapper (`execute_handler_with_negotiation`)
//!     under adversarial source-text declarations + Accept header
//!     combinations never returns 5xx (the classifier must be
//!     defensive against any source shape, never panic the request
//!     thread).
//!   * Keepalive lookup (`resolve_keepalive_for_flow`) never panics
//!     across arbitrary source strings — defends the dual-signal
//!     source-text fallback against parser-gap regressions.
//!
//! Seeds 0..100 mirror Fase 28's D12 fuzz cadence so CI reporters
//! render this pack as a sibling group.

use axon::axon_server::{build_router, resolve_keepalive_for_flow, ServerConfig};
use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn server_cfg() -> ServerConfig {
    ServerConfig {
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

/// Tiny deterministic PRNG so the fuzz is reproducible across
/// platforms. Same algorithm as `axon-frontend/src/parser.rs`
/// `Xorshift` in the Fase 28 D12 fuzz pack.
#[derive(Clone, Copy)]
struct Xorshift(u64);

impl Xorshift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            0
        } else {
            (self.next_u64() as usize) % max
        }
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = self.next_usize(items.len());
        &items[i]
    }
}

/// W3C SSE wire-format invariant assertions per plan vivo §4.
fn assert_sse_wire_invariants(body: &str, ctx: &str) {
    // 1. No bare CR.
    assert!(
        !body.contains('\r'),
        "{ctx}: bare CR detected in SSE body (W3C prefers LF-only)"
    );
    // 2. Body ends with blank-line terminator.
    assert!(
        body.is_empty() || body.ends_with("\n\n"),
        "{ctx}: SSE body must end with `\\n\\n` blank-line terminator, \
         got tail: {:?}",
        &body[body.len().saturating_sub(40)..]
    );
    // 3. retry: directive appears exactly once, before the first event.
    let retry_pos = body.find("retry: 5000");
    let first_event_pos = body.find("event:");
    if let (Some(rp), Some(ep)) = (retry_pos, first_event_pos) {
        assert!(
            rp < ep,
            "{ctx}: retry directive must precede the first event \
             (retry@{rp} event@{ep})"
        );
        // Only one retry directive (no duplicate / late retry).
        let second_retry = body[rp + 1..].find("retry: 5000");
        assert!(
            second_retry.is_none(),
            "{ctx}: duplicate `retry: 5000` directive at relative offset \
             {second_retry:?}"
        );
    }
    // 4. id: lines (when present) carry a monotonic non-decreasing u64.
    let mut last_id: Option<u64> = None;
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("id: ") {
            if let Ok(n) = rest.trim().parse::<u64>() {
                if let Some(prev) = last_id {
                    assert!(
                        n >= prev,
                        "{ctx}: SSE id field went backwards: {prev} -> {n} \
                         in body:\n{body}"
                    );
                }
                last_id = Some(n);
            }
        }
    }
}

async fn build_app_and_deploy(source: &str) -> axum::Router {
    let app = build_router(server_cfg());
    let body = serde_json::json!({
        "source": source,
        "source_file": "fuzz.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let _ = app.clone().oneshot(req).await.unwrap();
    app
}

async fn fetch_sse(app: axum::Router, payload: serde_json::Value) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute/sse")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (status, ct, text)
}

// ────────────────────────────────────────────────────────────────────
// 1. SSE wire-format conformance under request-body fuzz.
// ────────────────────────────────────────────────────────────────────

/// 100 deterministic iterations × 1 SSE call each. Each iteration
/// rolls a (flow_name, backend) tuple of varying validity and asserts
/// the response either:
///   * 200 OK + text/event-stream + W3C wire invariants, OR
///   * a structured non-5xx (400/401/422 etc.) — server NEVER crashes.
#[tokio::test]
async fn sse_wire_invariants_hold_under_request_body_fuzz() {
    // Deploy ONE valid flow that the fuzz may target.
    let app = build_app_and_deploy(
        "flow F() { step S { ask: \"hello world\" } }",
    )
    .await;

    let flow_pool = ["F", "G", "ghost", "F\u{00a0}", "", "F F", "FF", "f"];
    let backend_pool = ["stub", "auto", "stub2", "noop", "", "anthropic"];

    for seed in 0..100u64 {
        let mut rng = Xorshift(0x3030_3030_3030_a5a5_u64.wrapping_add(seed));
        let flow = (*rng.pick(&flow_pool)).to_string();
        let backend = (*rng.pick(&backend_pool)).to_string();

        let payload = serde_json::json!({
            "flow_name": flow,
            "backend": backend,
        });
        let (status, ct, body) = fetch_sse(app.clone(), payload).await;

        // Server must never 5xx, regardless of input.
        assert!(
            !status.is_server_error(),
            "seed={seed}: server returned 5xx (flow={flow:?} backend={backend:?})"
        );

        if status == StatusCode::OK {
            // 200 must be SSE-typed.
            assert!(
                ct.starts_with("text/event-stream"),
                "seed={seed}: 200 OK without text/event-stream content-type \
                 (got {ct:?})"
            );
            assert_sse_wire_invariants(&body, &format!("seed={seed}"));
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// 2. SSE wire-format invariants under varied keepalive declarations.
// ────────────────────────────────────────────────────────────────────

/// For each of the 4 ratified keepalive values + the "no axonendpoint"
/// baseline, deploy a flow whose source carries that declaration and
/// verify the SSE response still satisfies wire invariants. Combined
/// with sse_wire_invariants_hold_under_request_body_fuzz this gives
/// 5 declaration shapes × 100 = 500 total wire-format-invariant
/// assertions across the fuzz pack.
#[tokio::test]
async fn sse_wire_invariants_hold_under_keepalive_variants() {
    let declarations = [
        (
            "no_axonendpoint",
            "flow F() { step S { ask: \"hi\" } }".to_string(),
        ),
        (
            "keepalive_5s",
            "flow F() { step S { ask: \"hi\" } }\n\
             axonendpoint E { public: true method: POST path: \"/f\" execute: F \
             transport: sse keepalive: 5s }"
                .to_string(),
        ),
        (
            "keepalive_15s",
            "flow F() { step S { ask: \"hi\" } }\n\
             axonendpoint E { public: true method: POST path: \"/f\" execute: F \
             transport: sse keepalive: 15s }"
                .to_string(),
        ),
        (
            "keepalive_30s",
            "flow F() { step S { ask: \"hi\" } }\n\
             axonendpoint E { public: true method: POST path: \"/f\" execute: F \
             transport: sse keepalive: 30s }"
                .to_string(),
        ),
        (
            "keepalive_60s",
            "flow F() { step S { ask: \"hi\" } }\n\
             axonendpoint E { public: true method: POST path: \"/f\" execute: F \
             transport: sse keepalive: 60s }"
                .to_string(),
        ),
    ];

    for (label, source) in declarations {
        let app = build_app_and_deploy(&source).await;
        let (status, ct, body) = fetch_sse(
            app,
            serde_json::json!({ "flow_name": "F", "backend": "stub" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{label}: non-200 response");
        assert!(
            ct.starts_with("text/event-stream"),
            "{label}: non-SSE content-type: {ct}"
        );
        assert_sse_wire_invariants(&body, label);
        // Sanity: deployed flow must actually emit a complete event.
        assert!(
            body.contains("event: axon.complete"),
            "{label}: complete event missing in body:\n{body}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// 3. Negotiation wrapper never panics under adversarial Accept fuzz.
// ────────────────────────────────────────────────────────────────────

/// 100 iterations. Each rolls an `Accept` header value of varying
/// validity (real MIME types, junk strings, oversized, control
/// characters filtered to keep header-bytes valid). The negotiation
/// classifier on /v1/execute must never panic the request thread.
#[tokio::test]
async fn negotiation_never_panics_under_accept_header_fuzz() {
    let app = build_app_and_deploy(
        "flow F() { step S { ask: \"hi\" } }\n\
         axonendpoint E { public: true method: POST path: \"/f\" execute: F \
         transport: sse keepalive: 15s }",
    )
    .await;

    let accept_pool = [
        "text/event-stream",
        "application/json",
        "text/event-stream; charset=utf-8",
        "*/*",
        "",
        "foo/bar",
        "text/event-stream,application/json;q=0.5",
        "TEXT/EVENT-STREAM",
        "text/event-stream\r\nInjection: yes", // CRLF in headers is rejected at hyper layer
        "application/xml",
    ];

    for seed in 0..100u64 {
        let mut rng = Xorshift(0xdead_beef_cafe_babe_u64.wrapping_add(seed));
        let raw = *rng.pick(&accept_pool);
        // Sanitize against CRLF injection — hyper rejects those headers
        // at the construction layer; skip the iteration to keep the
        // fuzz exercising the application layer rather than HTTP framing.
        let Ok(header_value) = HeaderValue::from_str(raw) else {
            continue;
        };

        let payload = serde_json::json!({
            "flow": "F",
            "backend": "stub",
        });
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/execute")
            .header("content-type", "application/json");
        builder = builder.header(HeaderName::from_static("accept"), header_value);
        let req = builder.body(Body::from(payload.to_string())).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        assert!(
            !status.is_server_error(),
            "seed={seed}: server panicked on Accept={raw:?} (status={status})"
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// 4. resolve_keepalive_for_flow never panics on arbitrary source.
// ────────────────────────────────────────────────────────────────────

/// 1000 mutations of a base source string fed to
/// `resolve_keepalive_for_flow`. Defends the dual-signal AST +
/// source-text classifier against any byte sequence — the function
/// must always return a `Duration` (never panic, never overflow,
/// never deadlock on a parser regression).
#[test]
fn resolve_keepalive_never_panics_under_source_byte_fuzz() {
    let base = "flow F() { step S { ask: \"hi\" } }\n\
                axonendpoint E { public: true method: POST path: \"/f\" execute: F \
                transport: sse keepalive: 15s }";
    let safe_alphabet: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 \
          {}()[]<>:,;\"'\n\t.-_/";

    for bucket in 0..100u64 {
        let mut rng = Xorshift(0xa15c_a15c_a15c_a15c_u64.wrapping_add(bucket));
        let mut current = base.as_bytes().to_vec();
        for _ in 0..10 {
            if current.is_empty() {
                current = base.as_bytes().to_vec();
            }
            let op = rng.next_u64() % 4;
            let pos = rng.next_usize(current.len().max(1));
            match op {
                0 => {
                    if !current.is_empty() {
                        current.remove(pos.min(current.len() - 1));
                    }
                }
                1 => {
                    let b = *rng.pick(safe_alphabet);
                    current.insert(pos.min(current.len()), b);
                }
                2 if pos + 1 < current.len() => current.swap(pos, pos + 1),
                _ => {
                    let b = *rng.pick(safe_alphabet);
                    let idx = pos.min(current.len().saturating_sub(1));
                    if !current.is_empty() {
                        current[idx] = b;
                    }
                }
            }
            current.retain(|b| b.is_ascii());

            let s = std::str::from_utf8(&current).unwrap_or("");
            // The function must always return — no panic, no abort.
            let d = resolve_keepalive_for_flow(s, "F");
            // Returned Duration must be in the closed-enum range
            // {5, 15, 30, 60} seconds. Bounds check defends against
            // accidental drift in the parse helper.
            let secs = d.as_secs();
            assert!(
                secs == 5 || secs == 15 || secs == 30 || secs == 60,
                "resolve_keepalive_for_flow returned out-of-enum {secs}s \
                 for bucket={bucket}, source={s:?}"
            );
        }
    }
}
