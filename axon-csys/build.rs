//! §Fase 25.b — axon-csys build orchestration.
//!
//! Compiles the C23 metal-bound kernels into a static archive
//! (`libaxon_csys.a` on Unix, `axon_csys.lib` on MSVC) that the Rust crate
//! links against. The `cc` crate handles cross-platform compiler dispatch
//! (gcc / clang on Unix, MSVC / clang-cl on Windows); we drive it with
//! C23-first flags and a graceful fallback chain per founder ratification
//! D2 (2026-05-08): C23 floor, C2x fallback for clang ≤17 / gcc ≤13, no
//! C17 path.
//!
//! New kernels (25.c onward) append their `.c` files to the source list
//! near the bottom of `main()`. The flag chain stays the same — C23 +
//! strict diagnostics + cargo's own optimisation level.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// §Fase 25.g — probe whether the host C toolchain supports the C23
/// `#embed` preprocessor directive. Drives the conditional compilation
/// of `bpe.c`'s `#embed` block: when supported, the merges tables are
/// embedded directly via `#embed`; otherwise the Rust shim's
/// `include_bytes!` is the sole binding path.
///
/// The probe is a tiny self-contained C source file that uses `#embed`
/// to bake a 3-byte data file. If the compiler accepts it, the host
/// supports the directive. Failing to compile the probe is NOT a build
/// error — it just leaves `AXON_CSYS_BPE_HAS_C23_EMBED` undefined and
/// the C source skips the embed block.
fn probe_c23_embed(c_src: &Path, out_dir: &Path) -> bool {
    let probe_dir = out_dir.join("embed_probe");
    if let Err(e) = fs::create_dir_all(&probe_dir) {
        eprintln!("axon-csys: embed probe dir create failed ({e}); assuming no #embed support");
        return false;
    }
    // Probe data file — 3 bytes that compile to a valid C array initialiser
    // when expanded by `#embed` (each byte becomes a comma-separated int
    // literal).
    let probe_data_path = probe_dir.join("probe_embed_data.bin");
    if let Err(e) = fs::write(&probe_data_path, b"abc") {
        eprintln!("axon-csys: embed probe data write failed ({e}); assuming no #embed support");
        return false;
    }
    let probe_c_path = probe_dir.join("probe_embed.c");
    let probe_c = format!(
        "#include <stddef.h>\n\
         static const unsigned char probe_embed_data[] = {{\n\
         #embed \"{}\"\n\
         }};\n\
         int axon_csys_embed_probe_size(void) {{ return (int) sizeof(probe_embed_data); }}\n",
        probe_data_path.display().to_string().replace('\\', "/")
    );
    if let Err(e) = fs::write(&probe_c_path, probe_c) {
        eprintln!("axon-csys: embed probe write failed ({e}); assuming no #embed support");
        return false;
    }
    let mut try_build = cc::Build::new();
    try_build
        .file(&probe_c_path)
        .include(c_src)
        .out_dir(probe_dir.join("out"))
        .cargo_metadata(false)
        .warnings(false)
        .opt_level(0);
    if cfg!(target_env = "msvc") {
        try_build.flag_if_supported("/std:clatest");
    } else {
        try_build.flag_if_supported("-std=c23");
        try_build.flag_if_supported("-std=c2x");
    }
    try_build.try_compile("axon_csys_embed_probe").is_ok()
}

fn main() {
    // §Fase 119.h — without the `native` feature this build script does
    // NOTHING. `cc::Build::compile` panics when no compiler is present, so
    // running it unconditionally made a C toolchain a hard install
    // requirement; the pure-Rust path exists precisely so it is not one.
    if std::env::var_os("CARGO_FEATURE_NATIVE").is_none() {
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR is always set by cargo when invoking build scripts"),
    );
    let c_src = manifest_dir.join("c-src");
    let out_dir = PathBuf::from(
        env::var("OUT_DIR").expect("OUT_DIR is always set by cargo when invoking build scripts"),
    );

    // ─── §Fase 25.g — #embed support probe ────────────────────────────────
    //
    // Run BEFORE the main `cc::Build` so the probe doesn't pollute the
    // primary build's source list. Outcome drives a conditional
    // `cc::Build::define` for the main `bpe.c` translation unit.
    let has_c23_embed = probe_c23_embed(&c_src, &out_dir);
    if has_c23_embed {
        println!("cargo:rustc-cfg=axon_csys_c23_embed");
    }
    // The cargo:rustc-cfg above is the canonical signal exposed to
    // downstream Rust code; nothing else gates on it. The cc::Build
    // define below is what the C source consults via #ifdef.

    let mut build = cc::Build::new();
    build.include(&c_src);
    if has_c23_embed {
        build.define("AXON_CSYS_BPE_HAS_C23_EMBED", "1");
    }

    // ─── Feature-test macros for POSIX-only declarations ─────────────────
    //
    // 25.d's `c-src/buffer/pool.c` calls `posix_memalign` on Unix, which
    // glibc gates behind `_POSIX_C_SOURCE >= 200112L` (or `_XOPEN_SOURCE`).
    // In strict C23 mode (`-std=c23`) without an explicit feature-test
    // macro, glibc hides the declaration → `-Werror=implicit-function-
    // declaration` fails the build. macOS exposes posix_memalign
    // unconditionally; MSVC uses `_aligned_malloc` instead and never sees
    // posix_memalign at all. Define `_POSIX_C_SOURCE` only on Linux/BSD.
    //
    // Discovered post-hoc on the v1.19.0 CI run (linux gcc-13 + clang-17
    // + linux/aarch64 + musl all surfaced the implicit-declaration error;
    // local Windows MSVC builds were unaffected and shipped the bug).
    if !cfg!(target_env = "msvc") && !cfg!(target_os = "macos") {
        build.define("_POSIX_C_SOURCE", "200809L");
    }

    // ─── C23-first standard flag chain (D2 ratified 2026-05-08) ────────────
    //
    // `flag_if_supported` silently drops flags the compiler does not
    // recognise. The cc crate appends in order; the LAST recognised flag
    // wins. So writing `-std=c23` then `-std=c2x` means clang 17 takes c2x,
    // clang 18+ takes c23, gcc 13 takes c2x, gcc 14+ takes c23. This is
    // the documented graceful-degrade path.
    if cfg!(target_env = "msvc") {
        // MSVC's "C latest" mode covers what is currently available of
        // C23 in the Microsoft compiler (nullptr, #embed preview, etc.).
        // It does NOT cover labels-as-values or _BitInt — those kernels
        // gate themselves with #ifdef in the C source.
        build.flag_if_supported("/std:clatest");
        // C11 _Atomic on MSVC is gated behind an experimental flag even
        // with /std:clatest. Required by 25.d's pool.c — slab allocator
        // counters use _Atomic uint64_t. Silently ignored by older MSVCs
        // that don't recognise the flag (the build then fails loudly on
        // the #include <stdatomic.h>, which is the right behaviour).
        build.flag_if_supported("/experimental:c11atomics");
    } else {
        build.flag_if_supported("-std=c23");
        build.flag_if_supported("-std=c2x");
    }

    // ─── Diagnostics — strict for kernels ──────────────────────────────────
    //
    // No slop tolerated in the C surface. Warnings are errors so that any
    // implicit conversion / unused-result / signed-comparison warning
    // breaks CI immediately.
    if cfg!(target_env = "msvc") {
        build.flag_if_supported("/W4");
        // /WX makes warnings errors. Match the -Werror posture below.
        build.flag_if_supported("/WX");
    } else {
        build.flag_if_supported("-Wall");
        build.flag_if_supported("-Wextra");
        build.flag_if_supported("-Wpedantic");
        build.flag_if_supported("-Werror");
        // Belt + braces — these are the warnings most likely to mask
        // FFI safety bugs at the C boundary.
        build.flag_if_supported("-Wshadow");
        build.flag_if_supported("-Wcast-align");
        build.flag_if_supported("-Wconversion");
        build.flag_if_supported("-Wstrict-prototypes");
    }

    // ─── Optimisation ──────────────────────────────────────────────────────
    //
    // Cargo respects `OPT_LEVEL` env var that it sets per profile, so we
    // do NOT override `-O3` here — that would defeat `cargo build`
    // (debug profile wants -O0 + debuginfo). `-march=native` is also
    // intentionally NOT added at the global level; baking the build
    // host's instruction set into release tarballs would crash adopter
    // machines lacking AVX-512 / NEON. Per-kernel SIMD opt-in lives in
    // 25.c via `target_feature` cfg gates.

    // ─── Sources ───────────────────────────────────────────────────────────
    //
    // 25.b: build-infra probe.
    // 25.c: G.711 μ-law transcoders + linear PCM16 resampler.
    //       OTS native morphisms — port of axon-rs/src/ots/native/{mulaw,
    //       resample}.rs preserving the categorical structure documented
    //       in docs/ontological_tool_synthesis.md.
    // 25.d: Cache-line-aligned slab allocator with bitmap free-list +
    //       huge-pages opt-in. Port of axon-rs/src/buffer/pool.rs.
    //       Per-tenant accounting stays in the Rust shim (HashMap-of-
    //       Arc<str>) per founder pillar split — C handles slabs;
    //       Rust handles symbolic bookkeeping.
    build.file(c_src.join("probe").join("probe.c"));
    build.file(c_src.join("audio").join("mulaw.c"));
    build.file(c_src.join("audio").join("resample.c"));
    build.file(c_src.join("buffer").join("pool.c"));
    // 25.e: Algebraic effects FSM dispatcher with computed gotos
    //       (gcc/clang) + switch fallback (MSVC). Paper §5 delivery —
    //       "operaciones atómicas de salto en la pila de CPU sin
    //       objetos de control opacos" finally honoured by the
    //       per-opcode label table. Direct port of
    //       axon-rs/src/effects/runtime.rs preserving D2 (one-shot
    //       continuations), D9 (typechecker rejects unhandled effect)
    //       and D10 (typechecker rejects no-discharge / multi-resume)
    //       — the C runtime mirrors the Rust ref in surfacing those
    //       cases as defensive error codes for the unlikely path
    //       where the compiler missed them.
    build.file(c_src.join("effects").join("dispatch.c"));
    // §Fase 39.c.x: Epistemic Envelope kernel — Theorem 5.1
    //       enforcement in silicon for the FlowEnvelope wire
    //       payload (`transport: json`). Pure + total +
    //       deterministic; no allocation. Three functions:
    //         - validate_degradation: ceiling clamp on derived
    //         - theorem_5_1_ceiling:  constant exported for drift
    //         - clamp_ceiling:        belt-and-suspenders unconditional
    //       Called from axon-csys::envelope (Rust shim) which is
    //       in turn called from axon-rs::wire_envelope::FlowEnvelope::seal.
    build.file(c_src.join("effects").join("envelope.c"));
    // 25.g: BPE merge engine with #embed-baked merges tables for
    //       cl100k_base + o200k_base + SIMD UTF-8 codepoint counter.
    //       The C kernel handles byte-level BPE only — pretokenisation
    //       (PCRE-style regex with possessive quantifiers + Unicode
    //       categories) lives in the Rust shim's `tokens.rs` via the
    //       fancy-regex crate. Replaces the tiktoken-rs dep on the
    //       cl100k / o200k hot paths while keeping fancy-regex (which
    //       tiktoken-rs already pulls transitively).
    build.file(c_src.join("tokens").join("bpe.c"));
    // 25.h: HMAC-SHA256 + base64url + hex + constant-time compare +
    //       continuity-token wire format. Pure-C FIPS-friendly crypto
    //       (FIPS 180-4 + FIPS 198-1 algorithmically compliant; not
    //       formally validated). Replaces sha2 + hmac + base64 + subtle
    //       crates as the runtime dependency for the PEM continuity
    //       token; the Rust crates persist only as dev-dependencies in
    //       axon-csys/tests/crypto.rs for cross-validation drift gates.
    build.file(c_src.join("crypto").join("sha256.c"));
    build.file(c_src.join("crypto").join("hmac.c"));
    build.file(c_src.join("crypto").join("util.c"));
    build.file(c_src.join("crypto").join("continuity.c"));

    build.compile("axon_csys");

    // ─── Math library link (resample.c uses floor / round) ────────────────
    //
    // Modern glibc inlines libm functions into libc.so but musl, BSD, and
    // older glibcs require explicit `-lm`. macOS libSystem covers it; MSVC
    // bundles math into the CRT. Link explicitly on Unix to be portable.
    if !cfg!(target_env = "msvc") && !cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=m");
    }

    // ─── Re-build triggers ─────────────────────────────────────────────────
    println!("cargo:rerun-if-changed=c-src");
    println!("cargo:rerun-if-changed=build.rs");
}
