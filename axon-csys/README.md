# axon-csys

C23 metal-bound kernels for [axon-lang](https://github.com/VelitRicardo/axon-lang).

This crate is the Rust shim around a small set of carefully chosen C23
kernels. The C side handles what C uniquely does best: cache-line layout, bit
twiddling, hardware intrinsics, FSM dispatch with computed gotos, and the
vendored reference implementations of the signing boundary. The Rust side
handles correctness, ownership, and async glue, and exposes a safe API that
adopters consume without ever writing `unsafe` themselves.

## Layout

| Path | Kernel | Status |
|---|---|---|
| `c-src/probe/`   | build-infrastructure probe (C23 / `#embed` detection)                        | ✅ shipped |
| `c-src/audio/`   | G.711 mulaw + linear PCM resample (SIMD)                                     | ✅ shipped |
| `c-src/buffer/`  | cache-line slab allocator + huge-pages                                       | ✅ shipped |
| `c-src/effects/` | algebraic effects FSM (computed gotos) + the epistemic envelope (Theorem 5.1) | ✅ shipped |
| `c-src/tokens/`  | BPE tokenizer (`#embed` merges table) + SIMD UTF-8                           | ✅ shipped |
| `c-src/crypto/`  | SHA-256 + HMAC-SHA256 + base64url + continuity token (FIPS-friendly)         | ✅ shipped |
| `c-src/vendor/`  | FIPS 202 SHAKE, ML-DSA-65 (FIPS 204) and Ed25519 — vendored reference code, provenance-pinned per file in `PROVENANCE.toml` | ✅ shipped |

Every kernel ships with a byte-identical drift gate against a Rust reference
implementation, and the signature kernels carry known-answer tests (RFC 8032
vectors, FIPS 204 KATs) plus cross-implementation interop tests.

## Build

```sh
cargo build
cargo test
```

The C toolchain is **opt-in** behind the `native` feature (on by default for
direct dependents). With `native` off, the same public API is served by
pure-Rust twins and `cargo test --no-default-features` runs the same gates
against them.

Build orchestration is handled entirely by [`build.rs`](build.rs) via the
[`cc`](https://docs.rs/cc) crate. C standard floor is C23 (`-std=c23` /
`/std:clatest`); falls back to C2x (`-std=c2x`) on toolchains predating
the rename (clang ≤17, gcc ≤13). No C17 path is offered.

## Toolchain matrix (CI)

| OS              | Compilers              | C standard target |
|-----------------|------------------------|-------------------|
| Ubuntu (latest) | gcc 13+, clang 17+     | C23 (fallback C2x)|
| macOS (latest)  | Apple clang 15+        | C23 (fallback C2x)|
| Windows         | MSVC 19.41+, clang-cl  | `/std:clatest`    |

## License

AGPL-3.0-or-later — same as `axon-lang`. See `../LICENSE`.
