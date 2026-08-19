//! v1.24.0 — Mono-file `crate::backend` retirement drift gate.
//!
//! D7 contract: the canonical 7-provider backend set lives in a
//! SINGLE source of truth — `crate::backends::CANONICAL_PROVIDERS`.
//! The legacy `crate::backend::SUPPORTED_BACKENDS` constant is a
//! `pub use` re-export, byte-identical by-construction. The
//! `crate::backend::get_api_key` function is a thin shim that
//! delegates to `crate::backends::get_api_key` (the consolidated
//! source of truth) and wraps the result in the legacy
//! `BackendError` shape so existing callers don't need to change.
//!
//! # Closed-catalog discipline
//!
//! Adding a new provider requires updating:
//!   1. `crate::backends::CANONICAL_PROVIDERS` (the single source)
//!   2. `crate::backends::STREAMING_BACKEND_NAMES` (add `"stub"`)
//!   3. The per-provider module + factory in `crate::backends::*`
//! 4. The Python `BACKEND_REGISTRY` mirror (v1.18.0 drift gate
//!      cross-stack)
//!   5. This drift gate's count assertions
//!
//! # Honest scope (per plan vivo section 2.7)
//!
//! 33.x.i SHIPS the consolidation + deprecation markers + drift
//! gates. The DEEPER sync→async migration of the 4 caller files
//! (runner.rs / axon_server.rs legacy JSON path / resilient_backend.rs
//! / tenant_secrets.rs) ships as a separate followup step
//! v1.24.0 — it requires converting synchronous CLI/server
//! paths to async tokio runtime, which is a multi-thousand-LOC
//! refactor on the existing call chains and is independent of
//! the 33.x cycle's wire-activation deliverables.
//!
//! The deprecation markers + `#![allow(deprecated)]` on the 4
//! caller files make the legacy surface OBSERVABLE so any new
//! code that tries to add a fifth caller hits the warning and
//! routes to the async `Backend` trait instead.

use axon::backend::SUPPORTED_BACKENDS;
use axon::backends::{
    get_api_key as canonical_get_api_key, CANONICAL_PROVIDERS, STREAMING_BACKEND_NAMES,
};

// ─── section 1 — Single source of truth: legacy ≡ canonical ──────────────

#[test]
fn legacy_supported_backends_is_reexport_of_canonical_providers() {
    // The pub-use re-export makes byte-equality by-construction;
    // this test pins the invariant + catches accidental re-
    // introduction of a separate const.
    assert_eq!(SUPPORTED_BACKENDS, CANONICAL_PROVIDERS);
}

#[test]
fn canonical_providers_pins_the_seven_canonical_names() {
    // The 7-name set is closed. Adding an 8th canonical provider
    // requires updating this test + all the points in the
    // closed-catalog discipline doc above.
    assert_eq!(CANONICAL_PROVIDERS.len(), 7);
    let mut sorted: Vec<&str> = CANONICAL_PROVIDERS.to_vec();
    sorted.sort();
    assert_eq!(
        sorted,
        vec![
            "anthropic",
            "gemini",
            "glm",
            "kimi",
            "ollama",
            "openai",
            "openrouter",
        ]
    );
}

#[test]
fn streaming_set_is_canonical_plus_stub() {
    // v1.24.0: streaming dispatch includes the test/internal
    // `stub` backend as the 8th entry. The canonical adopter-
    // facing set excludes `stub`.
    assert_eq!(STREAMING_BACKEND_NAMES.len(), 8);
    let mut streaming_sorted: Vec<&str> = STREAMING_BACKEND_NAMES.to_vec();
    streaming_sorted.sort();
    let mut canonical_plus_stub: Vec<&str> = CANONICAL_PROVIDERS.to_vec();
    canonical_plus_stub.push("stub");
    canonical_plus_stub.sort();
    assert_eq!(streaming_sorted, canonical_plus_stub);
}

#[test]
fn legacy_supported_backends_does_not_include_stub() {
    // Adopter-facing constant: `stub` is internal and MUST NOT
    // surface here.
    assert!(!SUPPORTED_BACKENDS.contains(&"stub"));
}

// ─── section 2 — get_api_key: legacy shim ≡ canonical (by delegation) ────

#[test]
fn canonical_get_api_key_rejects_unknown_provider() {
    let err = canonical_get_api_key("does-not-exist").unwrap_err();
    assert!(err.contains("Unknown backend"));
    assert!(err.contains("Supported:"));
    // Hint references the canonical 7 names (NOT 8 — stub stays
    // internal).
    for name in CANONICAL_PROVIDERS {
        assert!(
            err.contains(name),
            "error hint MUST mention canonical provider {name:?}: {err}"
        );
    }
}

#[test]
fn legacy_get_api_key_delegates_to_canonical_with_error_shape_wrap() {
    // The legacy shim wraps the canonical error string in
    // `BackendError { message }` so existing callers' error-
    // handling code compiles unchanged. Verify the error message
    // round-trips.
    let canonical_err = canonical_get_api_key("does-not-exist").unwrap_err();
    let legacy_err = axon::backend::get_api_key("does-not-exist").unwrap_err();
    assert_eq!(legacy_err.message, canonical_err);
}

#[test]
fn canonical_get_api_key_ollama_permits_missing_key() {
    // Local daemon — missing key allowed.
    let prev = std::env::var("OLLAMA_API_KEY").ok();
    std::env::remove_var("OLLAMA_API_KEY");
    let result = canonical_get_api_key("ollama");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "");
    if let Some(v) = prev {
        std::env::set_var("OLLAMA_API_KEY", v);
    }
}

// ─── section 3 — Drift gates against backends/ filesystem layout ─────────

#[test]
fn each_canonical_provider_has_per_provider_module() {
    // Sanity check via the public re-export pattern: each
    // CANONICAL_PROVIDERS entry has a corresponding
    // `crate::backends::*Backend` type. Captured via the trait-
    // surface drift gate in `crate::backends::resolver_tests`;
    // this test pins the count invariant separately so future
    // additions to either side are caught.
    let expected_count = 7;
    assert_eq!(CANONICAL_PROVIDERS.len(), expected_count);
    // The streaming-resolver dispatch table has 8 entries
    // (canonical 7 + stub) — pinned by the resolver_tests in
    // crate::backends.
    assert_eq!(STREAMING_BACKEND_NAMES.len(), expected_count + 1);
}

// ─── section 4 — Deprecation markers exist (compile-level check) ─────────
//
// The `#[deprecated]` attributes on `crate::backend::call` /
// `call_multi` / `call_stream` / `call_multi_stream` are
// compile-time only. Verifying their presence at runtime would
// require macro introspection; instead we rely on the
// `#![allow(deprecated)]` pattern on the 4 caller files to make
// the deprecation surface OBSERVABLE in CI:
//   - Without `allow`, every caller of these fns shows a
//     compiler warning.
//   - WITH `allow`, the legacy surface compiles cleanly + new
//     code that tries to call these fns from a non-allow'd file
//     hits the warning.
//
// This test documents the deprecation discipline + ensures the
// re-export shim continues working (i.e. SUPPORTED_BACKENDS
// + get_api_key still compile + behave per spec).

#[test]
fn deprecated_surface_continues_to_function_for_legacy_callers() {
    // SUPPORTED_BACKENDS re-export still accessible.
    assert_eq!(SUPPORTED_BACKENDS.len(), 7);
    // get_api_key shim still callable.
    let err = axon::backend::get_api_key("nonexistent").unwrap_err();
    assert!(!err.message.is_empty());
}

// ─── section 5 — Followup-step tracker (33.x.i.2) ────────────────────
//
// 33.x.i is the consolidation + deprecation + drift gate. The
// deeper sync→async migration of the 4 callers ships as cycle
// 33.x.i.2:
//
//   - runner.rs CLI sync path → async via tokio runtime +
//     Backend trait
//   - axon_server.rs legacy JSON /v1/execute path → async via
//     run_streaming_async_path-style dispatcher (extends 33.x.b)
//   - resilient_backend.rs sync wrapper → async with circuit-
//     breaker + retry layered on Backend::complete()
//   - tenant_secrets.rs env-var fallback → already delegates to
//     `crate::backends::get_api_key` via the shim; full
//     async-aware tenant-secret resolution is part of 33.x.i.2.
//
// When 33.x.i.2 ships, this test's `deprecated_surface_continues_to_function_for_legacy_callers`
// test inverts to assert the absence of the deprecated symbols
// (the `pub use` re-export stays for backwards-compat with
// external adopters who may import `axon::backend::*`).

#[test]
fn followup_sub_fase_33x_i_2_tracker_documents_async_migration_scope() {
    // No-op runtime test; the rustdoc above is the scope
    // statement. This test slot exists so a grep for
    // "33.x.i.2" lands in this file + adopters can see the
    // migration roadmap.
    let _ = "v1.24.0 — sync→async migration of crate::backend's 4 callers";
}
