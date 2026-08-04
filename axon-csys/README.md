# axon-csys

C23 metal-bound kernels for [axon-lang](https://github.com/VelitRicardo/axon-lang).

Fase 25 — *Silicon + Cognition (sesión 1)*. This crate is the Rust shim
around a small set of carefully chosen C23 kernels. The C side handles
what C uniquely does best: cache-line layout, bit twiddling, hardware
intrinsics, FSM dispatch with computed gotos. The Rust side handles
correctness, ownership, and async glue, and exposes a safe API that
adopters consume without ever writing `unsafe` themselves.

## Layout

| Path | Sub-fase | Status |
|---|---|---|
| `c-src/probe/`   | 25.b — build infra probe                          | ✅ shipped |
| `c-src/audio/`   | 25.c — G.711 mulaw + linear PCM resample (SIMD)   | ✅ shipped |
| `c-src/buffer/`  | 25.d — cache-line slab allocator + huge-pages     | ✅ shipped |
| `c-src/effects/` | 25.e — algebraic effects FSM (computed gotos)     | ✅ shipped |
| `c-src/tokens/`  | 25.g — BPE tokenizer (`#embed` merges table) + SIMD UTF-8 | ✅ shipped |
| `c-src/crypto/`  | 25.h — SHA-256 + HMAC-SHA256 + base64url + continuity token (FIPS-friendly) | ✅ shipped |

## Build

```sh
cargo build
cargo test
```

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

MIT — same as `axon-lang`. See `../LICENSE`.

## Provenance

Plan vivo: [`../docs/fase/fase_25_silicon_cognition.md`](../docs/fase/fase_25_silicon_cognition.md).
Founder ratification of decisions D1–D12: 2026-05-08.
