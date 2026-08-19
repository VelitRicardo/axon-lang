//! v1.19.0 — End-to-end build-infrastructure probe tests.
//!
//! These tests exercise the entire cc-rs → C23 → static link → FFI
//! pipeline. If they pass on Linux/macOS/Windows × clang/gcc/msvc, the
//! foundation for 25.c onward is sound.

use axon_csys::{
    probe_add, probe_c_standard, probe_cacheline_alignment, probe_cacheline_marker,
    probe_cacheline_size, probe_features, probe_version, AxonCsysFeatures, AxonCsysVersion,
};

// ────────────────────────────────────────────────────────────────────────
// Basic FFI sanity
// ────────────────────────────────────────────────────────────────────────

#[test]
fn probe_version_is_zero_one_zero() {
    let v = probe_version();
    assert_eq!(
        v,
        AxonCsysVersion {
            major: 0,
            minor: 1,
            patch: 0
        }
    );
    assert_eq!(v.raw(), 0x000100, "wire encoding must match probe.c");
}

#[test]
fn probe_add_arithmetic_is_correct() {
    assert_eq!(probe_add(0, 0), 0);
    assert_eq!(probe_add(1, 2), 3);
    assert_eq!(probe_add(-5, 5), 0);
    assert_eq!(probe_add(i32::MAX - 1, 1), i32::MAX);
    // Commutativity is a free property of integer addition; assert it
    // crosses the FFI boundary intact.
    assert_eq!(probe_add(7, 11), probe_add(11, 7));
}

// ────────────────────────────────────────────────────────────────────────
// C standard realisation — proves the -std= flag chain in build.rs
// actually took effect.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn probe_c_standard_meets_c11_floor() {
    let std = probe_c_standard();
    // axon-csys requires at least C11 (probe.c #error guards this).
    assert!(
        std >= 201112,
        "C standard {std} below C11 floor — toolchain too old for axon-csys",
    );
}

#[test]
fn probe_c_standard_is_a_known_value() {
    let std = probe_c_standard();
    // Sanity: should be one of the published __STDC_VERSION__ values.
    // MSVC quirk: `/std:clatest` reports 202312L instead of the ratified
    // 202311L — observed empirically on MSVC 19.41+. The implementation
    // honours C23 semantics so we accept it as the C23 floor.
    let known = [
        201112u32, // C11
        201710,    // C17
        202000,    // C2x (pre-ratification spelling)
        202311,    // C23 (ratified — clang ≥18, gcc ≥14)
        202312,    // C23 (MSVC `/std:clatest` quirk — see comment above)
    ];
    assert!(
        known.contains(&std),
        "unexpected __STDC_VERSION__ value {std}; expected one of {known:?} (C11/C17/C2x/C23)",
    );
}

// ────────────────────────────────────────────────────────────────────────
// Feature bitmask round-trip + cross-checks against the C standard.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn probe_features_reports_at_least_alignas() {
    // alignas_64 is the one feature we mandate at the C11 floor — every
    // supported toolchain must report it.
    let features = probe_features();
    assert!(
        features.contains(AxonCsysFeatures::ALIGNAS_64),
        "every supported toolchain MUST honour _Alignas(64); got features={:#x}",
        features.raw(),
    );
}

#[test]
fn probe_features_c23_implies_nullptr() {
    // Logical invariant: C23 ⇒ nullptr. If the C kernel reports C23 but
    // not nullptr, the probe macros are inconsistent.
    let features = probe_features();
    if features.contains(AxonCsysFeatures::C23) {
        assert!(
            features.contains(AxonCsysFeatures::NULLPTR),
            "C23 must imply nullptr availability; got features={:#x}",
            features.raw(),
        );
    }
}

#[test]
fn probe_features_c23_flag_matches_stdc_version() {
    // Cross-check: AXON_CSYS_FEATURE_C23 bit is set iff __STDC_VERSION__
    // is ≥202311L.
    let features = probe_features();
    let std = probe_c_standard();
    assert_eq!(
        features.contains(AxonCsysFeatures::C23),
        std >= 202311,
        "C23 feature bit + __STDC_VERSION__ disagree (features={:#x}, std={})",
        features.raw(),
        std,
    );
}

#[test]
fn probe_features_round_trip_through_raw() {
    let features = probe_features();
    let raw = features.raw();
    let reconstructed = AxonCsysFeatures::from_raw(raw);
    assert_eq!(features, reconstructed);
}

// ────────────────────────────────────────────────────────────────────────
// Cache-line layout — proves _Alignas(64) survives the FFI boundary
// and matches Rust's layout expectations.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn probe_cacheline_alignment_is_sixty_four() {
    assert_eq!(
        probe_cacheline_alignment(),
        64,
        "_Alignof(struct axon_csys_cacheline_canary) must be 64",
    );
}

#[test]
fn probe_cacheline_size_is_multiple_of_alignment() {
    let size = probe_cacheline_size();
    let align = probe_cacheline_alignment();
    assert_eq!(
        size % align,
        0,
        "struct size {size} must be a multiple of alignment {align}",
    );
    // The canary is u64 (8) + u8[56] = 64 bytes total — exactly one cache line.
    assert_eq!(size, 64, "canary struct should be exactly one cache line");
}

#[test]
fn probe_cacheline_marker_round_trips() {
    for marker in [
        0u64,
        1,
        u64::MAX,
        0xDEAD_BEEF_CAFE_BABE,
        0x0123_4567_89AB_CDEF,
        u64::MAX / 2,
    ] {
        assert_eq!(
            probe_cacheline_marker(marker),
            marker,
            "marker {marker:#x} did not survive struct round-trip",
        );
    }
}

// ────────────────────────────────────────────────────────────────────────
// Concurrency — confirms the probe is reentrant + thread-safe (it should
// be: all probe functions are pure / stateless).
// ────────────────────────────────────────────────────────────────────────

#[test]
fn probe_is_thread_safe() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    let mismatches = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for thread_idx in 0i32..8 {
        let mismatches = Arc::clone(&mismatches);
        handles.push(thread::spawn(move || {
            for i in 0i32..1_000 {
                let a = thread_idx * 1_000 + i;
                let b = i % 17;
                if probe_add(a, b) != a + b {
                    mismatches.fetch_add(1, Ordering::Relaxed);
                }
                let marker = ((thread_idx as u64) << 32) | (i as u64);
                if probe_cacheline_marker(marker) != marker {
                    mismatches.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("worker thread panicked");
    }

    assert_eq!(
        mismatches.load(Ordering::Relaxed),
        0,
        "probe must be deterministic + thread-safe under concurrent load",
    );
}
