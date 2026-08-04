# axon-lang

**AXON — the formal cognitive language: a deterministic, proof-carrying AI runtime.**

[![crates.io](https://img.shields.io/crates/v/axon-lang.svg)](https://crates.io/crates/axon-lang)
[![docs.rs](https://docs.rs/axon-lang/badge.svg)](https://docs.rs/axon-lang)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](https://github.com/VelitRicardo/axon-lang/blob/master/LICENSE)

AXON is a programming language for AI cognition in which the properties you care
about — *this endpoint may not leak regulated data*, *this agent may not exceed
this budget*, *this credential may not outlive this session* — are **type errors**,
checked before anything runs, rather than conventions enforced by code review.

The crate publishes as `axon-lang`; the library import is `use axon::*`.

> **The language reference, the design papers and the full documentation live in
> the [repository README](https://github.com/VelitRicardo/axon-lang).** This page is
> the crate's landing page: what you get, how to install it, and how to watch it
> refuse something.

## Install

| Requirement | Why |
| ----------- | --- |
| **Rust 1.95+** | the `rust-version` of every crate in the workspace |
| a **C compiler** (MSVC / clang / gcc) | `axon-csys` is a non-optional dependency and builds C23 kernels via `cc` |

```bash
cargo install axon-lang
```

This installs the `axon` binary. If the build stops with a `cc`/linker error
rather than a Rust error, the C toolchain is what is missing.

## See it work

```bash
echo 'type PatientRecord compliance [HIPAA, GDPR] { ssn: String }
shield PHIShield { scan: [pii_leak] on_breach: halt severity: critical
                   compliance: [HIPAA, GDPR] }
axonendpoint Api { method: POST path: "/p" body: PatientRecord
                   execute: F output: FlowEnvelope<PatientRecord> shield: PHIShield
                   backend: anthropic compliance: [HIPAA, GDPR] }
flow F(ssn: String) -> PatientRecord {
  step R { ask: "summarize" output: PatientRecord } }' > app.axon

axon check   app.axon                  # compile-time compliance verification
axon dossier app.axon                  # regulatory posture, as JSON
axon audit   app.axon --framework all  # per-framework gap analysis
```

Now delete `shield: PHIShield` from the endpoint and check again:

```
X app.axon  1 error(s)
  error [line 4]: axon-T957 axonendpoint 'Api' carries regulated data
  (kappa = {GDPR, HIPAA}) across a trust boundary but declares no `shield:`.
  Regulated boundaries require a shield whose `compliance:` covers the type's
  kappa — ESK Fase 6.1 coverage rule. […] Declaring the classes on the
  endpoint's own `compliance:` does NOT cover them: that list is a label,
  the shield is the control that acts on a breach.
```

`axon check` exits `1`. That is the whole idea: the regulatory property rides on
the type system, so it holds at compile time or it does not hold at all.

## What's in the box

One binary, 28 subcommands. The ones you are most likely to reach for:

| Command | Does |
| ------- | ---- |
| `axon check` | lex, parse, type-check — the compliance gate |
| `axon run` | compile and execute (`--tool-mode stub` runs with no API keys) |
| `axon compile` | lower to IR JSON |
| `axon serve` | run the HTTP / SSE / NDJSON / WebSocket server |
| `axon dossier` · `axon audit` · `axon sbom` | regulatory posture, gap analysis, SBOM |
| `axon prove` · `axon verify` | emit and independently check proof objects |
| `axon fmt` · `axon fix` · `axon repl` · `axon inspect` | the usual tooling |

`axon --help` lists all of them.

## As a library

```toml
[dependencies]
axon-lang = "2"
```

```rust
use axon::*;
```

Building tooling rather than running programs? The compiler frontend — lexer,
parser, AST, epistemic type system, type checker, IR generator — is a separate
crate, [`axon-frontend`](https://crates.io/crates/axon-frontend), with **zero
runtime dependencies**. An LSP, a linter or an analyzer should depend on that and
skip the server, database and HTTP stack entirely.

## API keys

Only needed to execute flows against real backends — never for `axon check`,
`axon compile`, `axon dossier`, `axon audit` or `axon run --tool-mode stub`.
Seven backends are supported (Anthropic, OpenAI, Gemini, Kimi, GLM, OpenRouter,
Ollama); see the [repository README](https://github.com/VelitRicardo/axon-lang) for
the environment variables each expects.

## License

AGPL-3.0-or-later — see
[LICENSE](https://github.com/VelitRicardo/axon-lang/blob/master/LICENSE).
Commercial licensing and the enterprise layer are described in the
[repository](https://github.com/VelitRicardo/axon-lang).
