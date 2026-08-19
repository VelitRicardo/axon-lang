//! v1.19.0 — Build-infrastructure probe (Rust shim).
//!
//! Mirrors `c-src/probe/probe.c`. The probe surface is intentionally tiny:
//! it exists to verify that the whole pipeline (cc-rs → C23 compilation →
//! static link → FFI from Rust) works end-to-end across the CI matrix.
//! Subsequent steps (25.c onward) reuse the same plumbing for real
//! kernels.
//!
//! The compile-time feature bitmask returned by [`probe_features`] is
//! consumed by tests and by downstream kernels that gate code paths on
//! C23 features (e.g. computed gotos in 25.e). Runtime CPU feature
//! detection (AVX-2, NEON, etc.) is a separate mechanism handled inside
//! each kernel — this bitmask is purely a *compile-time* report.

extern "C" {
    fn axon_csys_probe_version() -> u32;
    fn axon_csys_probe_c_standard() -> u32;
    fn axon_csys_probe_features() -> u32;
    fn axon_csys_probe_add(a: i32, b: i32) -> i32;
    fn axon_csys_probe_cacheline_alignment() -> usize;
    fn axon_csys_probe_cacheline_marker(marker: u64) -> u64;
    fn axon_csys_probe_cacheline_size() -> usize;
}

/// Decoded ABI version returned by [`probe_version`].
///
/// Bumped only when the FFI surface shape changes; kernel-internal
/// behaviour changes do not bump this version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxonCsysVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl AxonCsysVersion {
    /// Re-encode back to the wire u32 layout `(major << 16) | (minor << 8) | patch`.
    /// Used by the round-trip drift gate.
    pub const fn raw(self) -> u32 {
        ((self.major as u32) << 16) | ((self.minor as u32) << 8) | (self.patch as u32)
    }
}

/// Bitmask of C23 features available at the C kernel's compile site.
///
/// These flags reflect what the *building* toolchain provides — they do
/// not change at runtime. Tests use them to skip branches the local
/// toolchain cannot exercise (e.g. [`Self::COMPUTED_GOTO`] is absent on
/// MSVC). Bit positions are kept synchronised with the
/// `AXON_CSYS_FEATURE_*` macros in `c-src/probe/probe.c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxonCsysFeatures(u32);

impl AxonCsysFeatures {
    pub const C23: Self = Self(1 << 0);
    pub const EMBED: Self = Self(1 << 1);
    pub const BITINT: Self = Self(1 << 2);
    pub const UNSEQUENCED: Self = Self(1 << 3);
    pub const NULLPTR: Self = Self(1 << 4);
    pub const ALIGNAS_64: Self = Self(1 << 5);
    pub const COMPUTED_GOTO: Self = Self(1 << 6);

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Returns the probe ABI version (currently 0.1.0).
pub fn probe_version() -> AxonCsysVersion {
    let raw = unsafe { axon_csys_probe_version() };
    AxonCsysVersion {
        major: ((raw >> 16) & 0xFF) as u8,
        minor: ((raw >> 8) & 0xFF) as u8,
        patch: (raw & 0xFF) as u8,
    }
}

/// Returns the realised `__STDC_VERSION__` at the C kernel's compile site.
///
/// Expected values: `201112L` (C11), `201710L` (C17), `202000L` (C2x),
/// `202311L` (C23). The build script targets C23 with C2x fallback per
/// founder ratification D2.
pub fn probe_c_standard() -> u32 {
    unsafe { axon_csys_probe_c_standard() }
}

/// Returns the compile-time feature bitmask. See [`AxonCsysFeatures`].
pub fn probe_features() -> AxonCsysFeatures {
    AxonCsysFeatures(unsafe { axon_csys_probe_features() })
}

/// Sanity FFI call — `a + b`. Used by tests to confirm the most basic
/// round-trip works.
pub fn probe_add(a: i32, b: i32) -> i32 {
    unsafe { axon_csys_probe_add(a, b) }
}

/// Returns the alignment of the cache-line canary struct. MUST be 64.
pub fn probe_cacheline_alignment() -> usize {
    unsafe { axon_csys_probe_cacheline_alignment() }
}

/// Returns the size of the cache-line canary struct. MUST be a multiple of 64.
pub fn probe_cacheline_size() -> usize {
    unsafe { axon_csys_probe_cacheline_size() }
}

/// Round-trips a 64-bit marker through a cache-line aligned C struct.
/// Returns the marker exactly as written. Drift gate for FFI layout.
pub fn probe_cacheline_marker(marker: u64) -> u64 {
    unsafe { axon_csys_probe_cacheline_marker(marker) }
}
