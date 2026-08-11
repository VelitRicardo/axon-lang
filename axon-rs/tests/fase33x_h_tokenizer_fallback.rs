//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 33.x.h — Tokenizer-aware fallback chunking (D9, opt-in).
//!
//! D9 contract: when an adopter opts in via
//! `crate::runtime_flags::set_tokenizer_fallback(true)`, the SSE
//! LEGACY path's per-step chunking switches from synthetic 3-word
//! groups to BPE-tokenized chunks via
//! `axon_csys::tokens::cl100k_base()`. The flag defaults OFF so
//! v1.24.0 wire byte-compat is preserved for adopters who don't
//! opt in.
//!
//! # What this file proves at the integration layer
//!
//! - ASYNC path is UNAFFECTED by the flag (D4 byte-compat: the
//!   flag opts INTO a LEGACY-path-only behavior).
//! - The flag CONTROLS the chunking decision via guard semantics
//!   (set/restore on drop).
//! - The chunker round-trip preserves content byte-for-byte under
//!   typical English prose.
//!
//! # What's covered at the module-test layer (lib unit tests)
//!
//! - Flag default OFF + getter/setter + guard restoration
//! - BPE chunker over empty + short + long English text
//! - BPE produces strictly finer granularity than 3-word grouping
//!   for non-trivial text
//! - Joined chunks reproduce the original text
//!
//! # What's deferred to 33.x.j
//!
//! HTTP-level verification of the LEGACY chunking path with the
//! flag ON requires a flow shape that BOTH deploys cleanly AND
//! survives dynamic-route registration AND triggers
//! `PlanFallback::UnsupportedNode`. The route-registration filters
//! that mediate this combination vary across adopter source
//! shapes — the 33.x.j real-provider lane is where the full HTTP
//! integration runs with vetted adopter sources. The trait-layer
//! BPE chunker correctness is asserted exhaustively by the
//! module-test layer.

#![allow(clippy::needless_return)]

use std::sync::Mutex;

use axon::axon_server::{build_router, ServerConfig};
use axon::runtime_flags::{
    bpe_chunk_text, set_tokenizer_fallback, tokenizer_fallback_enabled,
    TokenizerFallbackGuard,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Serialize all tests that mutate the process-wide flag. The
/// flag is OFF by default; tests that flip it acquire this Mutex
/// for their entire body so parallel test runs don't observe
/// each other's mutations.
static FLAG_TEST_LOCK: Mutex<()> = Mutex::new(());

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

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn fetch_sse_body(app: axum::Router, path: &str, body: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

fn count_axon_tokens(body: &str) -> usize {
    body.lines()
        .filter(|l| *l == "event: axon.token")
        .count()
}

const SIMPLE_STREAM_FLOW: &str =
    "flow Chat() -> Unit {\n\
        step Generate { ask: \"hi\" output: Stream<Token> }\n\
     }\n\
     axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";

// ─── §1 — D4: ASYNC path is unaffected by the flag ─────────────────

#[tokio::test]
async fn d4_async_path_byte_compat_with_flag_off() {
    let _serial = FLAG_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    set_tokenizer_fallback(false);
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM_FLOW).await;
    let body = fetch_sse_body(app, "/chat", "{}").await;
    // Stub backend on ASYNC path → exactly 1 axon.token with
    // delta "(stub)". The flag OFF is the v1.24.0 baseline.
    assert_eq!(count_axon_tokens(&body), 1);
    assert!(body.contains("\"token\":\"(stub)\""));
}

#[tokio::test]
async fn d4_async_path_byte_compat_with_flag_on() {
    let _serial = FLAG_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _g = TokenizerFallbackGuard::set(true);
    let app = build_router(server_cfg());
    deploy(app.clone(), SIMPLE_STREAM_FLOW).await;
    let body = fetch_sse_body(app, "/chat", "{}").await;
    // D9 invariant: the flag opts INTO a LEGACY-path-only behavior.
    // The ASYNC path's wire shape is UNCHANGED — 1 axon.token,
    // content "(stub)" — regardless of the flag value.
    assert_eq!(
        count_axon_tokens(&body),
        1,
        "ASYNC path MUST be byte-compatible regardless of tokenizer_fallback flag"
    );
    assert!(body.contains("\"token\":\"(stub)\""));
}

// ─── §2 — Flag CONTROL semantics ───────────────────────────────────

#[tokio::test]
async fn flag_guard_restores_on_drop() {
    let _serial = FLAG_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    set_tokenizer_fallback(false);
    assert!(!tokenizer_fallback_enabled());
    {
        let _g = TokenizerFallbackGuard::set(true);
        assert!(tokenizer_fallback_enabled());
    }
    assert!(!tokenizer_fallback_enabled());
}

#[tokio::test]
async fn flag_default_is_off() {
    let _serial = FLAG_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    // Reset to default in case prior test leaked.
    set_tokenizer_fallback(false);
    assert!(
        !tokenizer_fallback_enabled(),
        "D9: flag MUST default OFF for v1.24.0 wire byte-compat"
    );
}

// ─── §3 — Chunker correctness over canonical inputs ───────────────

// §Fase 120.f — the two assertions below need the C TOKENISER KERNEL, and say
// so instead of failing as if the chunker were broken.
//
// `bpe_chunk_text` returns `Vec::new()` when `axon_csys::tokens::cl100k_base()`
// is unavailable, and §118.c made `axon-csys`'s native kernels opt-in
// (`--features csys-native`) because `cc` panicked without a C compiler. From
// that moment these two failed with *"BPE chunks (0) MUST be strictly finer
// than 3-word groups"* — a message that reads like a broken chunker and is
// actually a missing dependency. Measured: with `--features csys-native` this
// file is **10/10**; the engine is fine.
//
// ⚠️ The silent empty is the part worth naming. A caller cannot tell "no text"
// from "no kernel" — the §112 kernel defect shape (*if the evidence is missing,
// substitute the belief*). It is left alone deliberately: §119 recorded
// `runtime_flags` as NOT WIRED (nothing in the runtime calls `bpe_chunk_text`),
// and changing the signature of a function with no reader is motion. When it
// gains one, that return type is the first thing to fix.
#[cfg(feature = "csys-native")]
#[tokio::test]
async fn bpe_chunker_round_trips_english_prose() {
    // Multi-paragraph English: round-trip must reconstruct.
    let text = "axon is a deterministic language for AI flows. It honors its own declarations across the wire and the audit row alike.";
    let chunks = bpe_chunk_text(text);
    assert!(!chunks.is_empty());
    let joined: String = chunks.join("");
    assert_eq!(joined, text);
}

#[cfg(feature = "csys-native")]
#[tokio::test]
async fn bpe_chunker_finer_than_3_word_groups() {
    let text = "The quick brown fox jumps over the lazy dog repeatedly.";
    let word_chunks = text.split_whitespace().count().div_ceil(3);
    let bpe_chunks = bpe_chunk_text(text).len();
    assert!(
        bpe_chunks > word_chunks,
        "BPE chunks ({bpe_chunks}) MUST be strictly finer than 3-word groups ({word_chunks}) for D9 adopter value"
    );
}

#[tokio::test]
async fn bpe_chunker_empty_text_yields_empty_vec() {
    assert!(bpe_chunk_text("").is_empty());
    // No panic, no infinite loop, no allocation surprise.
}

// ─── §4 — Tokenizer is reachable via the `axon` public re-export ──

#[tokio::test]
async fn axon_crate_publicly_exposes_runtime_flags_module() {
    // The crate-level re-export pattern lets adopters call the
    // setter from their `main.rs` to opt in at process startup.
    // This test pins the public-API surface as part of the D9
    // contract.
    set_tokenizer_fallback(false);
    let _g = TokenizerFallbackGuard::set(true);
    assert!(tokenizer_fallback_enabled());
}

// ─── §5 — Repeated flag flips don't leak state ─────────────────────

#[tokio::test]
async fn flag_flips_repeated_within_one_test_do_not_leak() {
    let _serial = FLAG_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    set_tokenizer_fallback(false);
    for cycle in 0..10 {
        let _g = TokenizerFallbackGuard::set(true);
        assert!(tokenizer_fallback_enabled(), "cycle {cycle}: on");
        drop(_g);
        assert!(!tokenizer_fallback_enabled(), "cycle {cycle}: restored off");
    }
    assert!(!tokenizer_fallback_enabled(), "final state: off");
}

// ─── §6 — Concurrent reads with serialized writes ─────────────────

#[tokio::test]
async fn flag_reads_are_consistent_under_serialized_writes() {
    let _serial = FLAG_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    set_tokenizer_fallback(false);
    let mut samples_off = 0;
    for _ in 0..1000 {
        if !tokenizer_fallback_enabled() {
            samples_off += 1;
        }
    }
    assert_eq!(samples_off, 1000, "consistent reads when no writer races");
    set_tokenizer_fallback(true);
    let mut samples_on = 0;
    for _ in 0..1000 {
        if tokenizer_fallback_enabled() {
            samples_on += 1;
        }
    }
    assert_eq!(samples_on, 1000);
    // Cleanup.
    set_tokenizer_fallback(false);
}
