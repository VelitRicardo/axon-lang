# axon-lang — this package is discontinued

**AXON is a Rust + C23 toolchain. There is nothing to `pip install`.**

```bash
cargo install axon-lang
```

Requires [Rust](https://rustup.rs) **1.95+** and a C compiler (MSVC / clang / gcc)
— `axon-csys` builds C23 kernels via `cc`.

## What happened

The Python interpreter was retired in **Fase 40 (*Pure Silicon*, v2.0.0)**. For a
while afterwards this package was a thin launcher that downloaded and executed the
native binary; that shim was deleted in **Fase 116.c.5**. Everything — lexer,
parser, epistemic type system, IR generator, runtime, HTTP server, CLI — is native
Rust, with the FIPS-routable cryptographic and tokeniser kernels in standalone C23.

The historical Python releases remain on PyPI, unmaintained, for anyone who needs
to reproduce an old build.

## Where things are now

| | |
| --- | --- |
| Crate | [crates.io/crates/axon-lang](https://crates.io/crates/axon-lang) |
| API docs | [docs.rs/axon-lang](https://docs.rs/axon-lang) |
| Source | [github.com/VelitRicardo/axon-lang](https://github.com/VelitRicardo/axon-lang) |
| Compiler frontend only (zero runtime dependencies) | [crates.io/crates/axon-frontend](https://crates.io/crates/axon-frontend) |

If you are building tooling — an LSP, a linter, an analyzer — depend on
`axon-frontend` rather than the full crate: it is the lexer, parser, type checker
and IR generator with no runtime dependencies at all.

## What AXON is

A programming language for AI cognition in which the properties you care about —
*this endpoint may not leak regulated data*, *this agent may not exceed this
budget*, *this credential may not outlive this session* — are **type errors**,
checked before anything runs.

```bash
axon check app.axon    # compile-time compliance verification
```

Licensed AGPL-3.0-or-later.
