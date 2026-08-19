//! v1.31.0 (D2) — cross-stack drift gate for the `axonendpoint
//! backend:` closed catalog.
//!
//! The catalog the `axon-frontend` parser + type-checker validate
//! against (`AXONENDPOINT_BACKEND_VALUES`) is, by D2,
//! `CANONICAL_PROVIDERS ∪ {auto, stub}`. `axon-frontend` carries zero
//! runtime deps and therefore cannot `use axon::backends::
//! CANONICAL_PROVIDERS` — the frontend list is a hand-maintained
//! mirror.
//!
//! This gate is the safety net: it lives in `axon-rs`, which sees
//! BOTH crates, and asserts the two catalogs stay byte-identical.
//! Adding a provider to `CANONICAL_PROVIDERS` without mirroring it
//! into `AXONENDPOINT_BACKEND_VALUES` (or vice versa) fails CI here —
//! the frontend would otherwise reject a backend the runtime can
//! resolve, or accept one it cannot.

use axon::backends::CANONICAL_PROVIDERS;
use axon::parser::AXONENDPOINT_BACKEND_VALUES;
use std::collections::HashSet;

/// The two non-provider catalog entries (D2): `auto` is the
/// transparent ladder-fallthrough, `stub` the explicit-opt-in no-op.
const NON_PROVIDER_BACKENDS: &[&str] = &["auto", "stub"];

#[test]
fn frontend_catalog_is_canonical_providers_plus_auto_and_stub() {
    let mut expected: HashSet<&str> = CANONICAL_PROVIDERS.iter().copied().collect();
    expected.extend(NON_PROVIDER_BACKENDS.iter().copied());

    let actual: HashSet<&str> = AXONENDPOINT_BACKEND_VALUES.iter().copied().collect();

    assert_eq!(
        actual, expected,
        "36.d D2 cross-stack drift: `axon-frontend`'s \
         AXONENDPOINT_BACKEND_VALUES must equal `CANONICAL_PROVIDERS ∪ \
         {{auto, stub}}`. The frontend list is a hand-maintained mirror \
         of the runtime's CANONICAL_PROVIDERS — update both together."
    );
}

#[test]
fn frontend_catalog_cardinality_tracks_canonical_providers() {
    assert_eq!(
        AXONENDPOINT_BACKEND_VALUES.len(),
        CANONICAL_PROVIDERS.len() + NON_PROVIDER_BACKENDS.len(),
        "36.d: the frontend catalog is exactly the {} canonical \
         providers plus {{auto, stub}}",
        CANONICAL_PROVIDERS.len()
    );
}

#[test]
fn every_canonical_provider_is_a_declarable_backend() {
    let frontend: HashSet<&str> = AXONENDPOINT_BACKEND_VALUES.iter().copied().collect();
    for &provider in CANONICAL_PROVIDERS {
        assert!(
            frontend.contains(provider),
            "36.d: canonical provider `{provider}` is not declarable \
             as an `axonendpoint backend:` — the frontend would reject \
             a backend the runtime can resolve"
        );
    }
}

#[test]
fn frontend_catalog_has_no_duplicates() {
    let unique: HashSet<&str> = AXONENDPOINT_BACKEND_VALUES.iter().copied().collect();
    assert_eq!(
        unique.len(),
        AXONENDPOINT_BACKEND_VALUES.len(),
        "36.d: the backend catalog must have no duplicate entries"
    );
}

#[test]
fn stub_is_in_the_catalog_explicit_optin_only() {
    // D5 — `stub` is a *declarable* backend (an operator may opt in
    // explicitly) but never an `auto` / silent fallback. Its presence
    // in the *declaration* catalog is correct; the resolver
    // (`backend_resolution.rs`) is what guarantees it never resolves
    // silently.
    assert!(
        AXONENDPOINT_BACKEND_VALUES.contains(&"stub"),
        "36.d D5: `stub` must be declarable — D5 forbids a SILENT \
         stub, not an explicit written opt-in"
    );
    assert!(
        !CANONICAL_PROVIDERS.contains(&"stub"),
        "36.d D5: `stub` must NOT be a canonical provider — the `auto` \
         rungs filter it so auto-resolution can never land on it"
    );
}
