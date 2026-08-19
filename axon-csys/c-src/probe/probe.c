/*
 * v1.19.0 — axon-csys build-infrastructure probe kernel.
 *
 * The probe is the canary that proves the entire C build pipeline works
 * end-to-end (cc-rs orchestration → C23 compilation → static link → FFI
 * invocation from Rust). Subsequent steps (25.c onward) reuse the same
 * plumbing for real kernels. The probe reports compile-time C-standard
 * realisation + feature availability so tests + downstream kernels can
 * degrade gracefully on toolchains that pre-date individual C23 features.
 *
 * Layout discipline:
 *   • PUBLIC ABI is everything prefixed `axon_csys_probe_*`. Order of
 *     functions in this file is the order in which Rust's extern block
 *     declares them (src/probe.rs).
 *   • All public functions are [[nodiscard]] — there is no fire-and-forget
 *     entry on this surface.
 *   • Feature-detection macros use the standards-mandated
 *     `__has_c_attribute` / `__has_embed` / `__BITINT_MAXWIDTH__` probes;
 *     anything not detectable through standard macros is gated by a
 *     compiler-vendor #ifdef (only for the goto-labels-as-values extension).
 */

#include <stdint.h>
#include <stddef.h>

/* axon-csys requires at least C11 for _Alignas / _Alignof. C23 is the
 * preferred floor (D2). We accept C17 here so that stragglers in CI matrix
 * (e.g. older Apple clang on macOS-12 runners) still build the probe — but
 * only the probe; future kernels (25.c onward) tighten the floor to C23. */
#if !defined(__STDC_VERSION__) || __STDC_VERSION__ < 201112L
#  error "axon-csys requires at least C11 (preferably C23). Upgrade your toolchain."
#endif

/* ---------------------------------------------------------------------------
 * Feature detection — bitmask reported back to Rust via probe_features().
 * Bit positions MUST stay synchronised with AxonCsysFeatures in src/probe.rs;
 * tests in tests/probe.rs assert the round-trip.
 * ------------------------------------------------------------------------- */

#define AXON_CSYS_FEATURE_C23           (1u << 0)
#define AXON_CSYS_FEATURE_EMBED         (1u << 1)
#define AXON_CSYS_FEATURE_BITINT        (1u << 2)
#define AXON_CSYS_FEATURE_UNSEQUENCED   (1u << 3)
#define AXON_CSYS_FEATURE_NULLPTR       (1u << 4)
#define AXON_CSYS_FEATURE_ALIGNAS_64    (1u << 5)
#define AXON_CSYS_FEATURE_COMPUTED_GOTO (1u << 6)

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 202311L
#  define AXON_CSYS_HAS_C23 1
#else
#  define AXON_CSYS_HAS_C23 0
#endif

/* C23 section 6.10.3.5 mandates __STDC_EMBED_NOT_FOUND__ / __STDC_EMBED_FOUND__ /
 * __STDC_EMBED_EMPTY__ predefined macros once the implementation supports
 * #embed. Their presence is the standard-blessed way to detect support. */
#if defined(__STDC_EMBED_NOT_FOUND__)
#  define AXON_CSYS_HAS_EMBED 1
#else
#  define AXON_CSYS_HAS_EMBED 0
#endif

/* _BitInt(N) availability is signalled by __BITINT_MAXWIDTH__; clang ≥16
 * + gcc ≥14 define it. MSVC does not yet ship _BitInt. */
#if defined(__BITINT_MAXWIDTH__) && __BITINT_MAXWIDTH__ >= 8
#  define AXON_CSYS_HAS_BITINT 1
#else
#  define AXON_CSYS_HAS_BITINT 0
#endif

#if defined(__has_c_attribute)
#  if __has_c_attribute(unsequenced)
#    define AXON_CSYS_HAS_UNSEQUENCED 1
#  else
#    define AXON_CSYS_HAS_UNSEQUENCED 0
#  endif
#else
#  define AXON_CSYS_HAS_UNSEQUENCED 0
#endif

/* `nullptr` constant: C23 keyword. */
#if AXON_CSYS_HAS_C23
#  define AXON_CSYS_HAS_NULLPTR 1
#else
#  define AXON_CSYS_HAS_NULLPTR 0
#endif

/* Computed gotos (labels-as-values): GCC + clang extension; not standard
 * C; MSVC has no equivalent. Used by the FSM dispatcher in 25.e. */
#if defined(__GNUC__) || defined(__clang__)
#  define AXON_CSYS_HAS_COMPUTED_GOTO 1
#else
#  define AXON_CSYS_HAS_COMPUTED_GOTO 0
#endif

/* Cache-line alignment via _Alignas (C11 syntax — works on C11/C17/C23
 * everywhere we care about). C23's `alignas` keyword is just a spelling
 * change. */
#define AXON_CSYS_HAS_ALIGNAS_64 1

/* ---------------------------------------------------------------------------
 * [[nodiscard]] portable guard.
 * `__has_c_attribute(nodiscard)` is C23-mandated; in pre-C23 we compile
 * the attribute away.
 *
 * Pre-C23 GCC (≤ ~10 on aarch64-linux-musl in cross 0.2.5) does NOT define
 * the `__has_c_attribute` builtin and does NOT short-circuit it inside a
 * `#if defined(X) && X(y)` expression — it eagerly expands the second
 * conjunct and emits "missing binary operator before token (". Nested
 * `#ifdef` / `#if` guards avoid the eager-expansion path on those
 * pre-C23 toolchains.
 * ------------------------------------------------------------------------- */

#ifdef __has_c_attribute
#  if __has_c_attribute(nodiscard)
#    define AXON_CSYS_NODISCARD [[nodiscard]]
#  endif
#endif
#ifndef AXON_CSYS_NODISCARD
#  define AXON_CSYS_NODISCARD
#endif

/* ---------------------------------------------------------------------------
 * Cache-line aligned canary struct.
 * Tests assert _Alignof(struct ...) == 64 on every supported platform.
 * ------------------------------------------------------------------------- */

struct axon_csys_cacheline_canary {
    _Alignas(64) uint64_t marker;
    uint8_t  padding[56];
};

/* ---------------------------------------------------------------------------
 * Public ABI — keep stable. New fields go at the END of structs only.
 * The order of declarations here mirrors src/probe.rs's extern block.
 * ------------------------------------------------------------------------- */

/* Returns the axon-csys probe ABI version: (major << 16) | (minor << 8) | patch.
 * Bumped when the FFI surface changes shape, NOT for kernel-internal changes. */
AXON_CSYS_NODISCARD
uint32_t axon_csys_probe_version(void) {
    /* 25.b ships ABI version 0.1.0 — probe kernel only. */
    return (0u << 16) | (1u << 8) | 0u;
}

/* Returns the realised __STDC_VERSION__ at the C kernel's compile site.
 * Tests use this to verify the chosen `-std=` flag actually took effect.
 * Expected values:
 *   201112L = C11
 *   201710L = C17
 *   202000L = C2x (pre-ratification spelling)
 *   202311L = C23 (ratified)
 */
AXON_CSYS_NODISCARD
uint32_t axon_csys_probe_c_standard(void) {
    return (uint32_t)__STDC_VERSION__;
}

/* Returns a bitmask of available C23 features. Tests use this to skip
 * branches that the local toolchain cannot compile. */
AXON_CSYS_NODISCARD
uint32_t axon_csys_probe_features(void) {
    uint32_t mask = 0u;
    if (AXON_CSYS_HAS_C23)           mask |= AXON_CSYS_FEATURE_C23;
    if (AXON_CSYS_HAS_EMBED)         mask |= AXON_CSYS_FEATURE_EMBED;
    if (AXON_CSYS_HAS_BITINT)        mask |= AXON_CSYS_FEATURE_BITINT;
    if (AXON_CSYS_HAS_UNSEQUENCED)   mask |= AXON_CSYS_FEATURE_UNSEQUENCED;
    if (AXON_CSYS_HAS_NULLPTR)       mask |= AXON_CSYS_FEATURE_NULLPTR;
    if (AXON_CSYS_HAS_ALIGNAS_64)    mask |= AXON_CSYS_FEATURE_ALIGNAS_64;
    if (AXON_CSYS_HAS_COMPUTED_GOTO) mask |= AXON_CSYS_FEATURE_COMPUTED_GOTO;
    return mask;
}

/* Sanity FFI call — pure integer arithmetic. Used by tests to confirm
 * that calling C from Rust works at the most basic level. */
AXON_CSYS_NODISCARD
int32_t axon_csys_probe_add(int32_t a, int32_t b) {
    return a + b;
}

/* Returns _Alignof(struct axon_csys_cacheline_canary). MUST be 64 on every
 * platform we ship. Caught at runtime by the Rust drift gate. */
AXON_CSYS_NODISCARD
size_t axon_csys_probe_cacheline_alignment(void) {
    return _Alignof(struct axon_csys_cacheline_canary);
}

/* Round-trips a 64-bit marker through a cache-line aligned struct.
 * Confirms (a) struct lays out correctly across the FFI boundary,
 * (b) auto-storage allocation honours the alignment requirement,
 * (c) reads and writes don't get reordered into adjacent padding. */
AXON_CSYS_NODISCARD
uint64_t axon_csys_probe_cacheline_marker(uint64_t marker) {
    struct axon_csys_cacheline_canary canary;
    canary.marker = marker;
    for (size_t i = 0; i < sizeof canary.padding; ++i) {
        canary.padding[i] = 0u;
    }
    return canary.marker;
}

/* Returns sizeof(struct axon_csys_cacheline_canary). MUST be a multiple of
 * 64 (same reasoning as alignment). Tests assert it. */
AXON_CSYS_NODISCARD
size_t axon_csys_probe_cacheline_size(void) {
    return sizeof(struct axon_csys_cacheline_canary);
}
