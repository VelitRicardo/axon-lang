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

This installs **`axon`** — the compiler and governance CLI. It is what you want
in CI, in a pre-commit hook, and on a developer machine: `axon check` needs no
database, no HTTP server and no API key.

If the build stops with a `cc`/linker error rather than a Rust error, the C
toolchain is what is missing.

### Running governed flows: `axon-server`

`axon` compiles and audits; **`axon-server` executes**. It is a separate binary
behind a feature flag, the way `psql` and `postgres` — or `docker` and `dockerd`
— are separate:

```bash
cargo install axon-lang --features server            # adds `axon-server`
cargo install axon-lang --features server,postgres   # …with the SQL data plane
```

| feature | adds |
| ------- | ---- |
| *(none — the default)* | `axon`: check, compile, run, dossier, audit, sbom, prove, verify, fmt, fix, repl, … |
| `server` | the `axon-server` binary — HTTP / SSE / NDJSON / WebSocket |
| `postgres` | `backend: postgresql` axonstores, `axon store introspect` |
| `documents` | the OOXML surface (synthesis, ingestion, governed egress) and `axon evidence-package` |

**Both binaries are AGPL and both live in this crate.** The split is a *role*
boundary — compiler vs runtime — not a paid one.

Nothing disappears when you install less. **Every subcommand stays in `--help`
under every profile**, and one whose feature is absent refuses in writing with
the exact command that brings it back:

```
$ axon serve
X `axon serve` requires the `server` feature - this build was compiled without it…
  Reinstall with: cargo install axon-lang --features server
  and run `axon-server` (the `axon` you have keeps working for everything else).
```

`axon check` type-checks a program that declares `backend: postgresql` even in a
build with no database driver — the compiler resolves a *declaration*; it opens
no connection.

**What the split costs and buys** (measured 2026-08-06, warm registry, cold
target dir): the default install is **11m09s → 7m05s** and the `axon` binary
**36.1 MiB → 19.1 MiB**.

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
  kappa — the ESK coverage rule. […] Declaring the classes on the
  endpoint's own `compliance:` does NOT cover them: that list is a label,
  the shield is the control that acts on a breach.
```

`axon check` exits `1`. That is the whole idea: the regulatory property rides on
the type system, so it holds at compile time or it does not hold at all.

## What's in the box

**28 subcommands, all of them listed under every profile.** The ones you are most
likely to reach for:

| Command | Does |
| ------- | ---- |
| `axon check` | lex, parse, type-check — the compliance gate |
| `axon run` | compile and execute (`--tool-mode stub` runs with no API keys) |
| `axon compile` | lower to IR JSON |
| `axon serve` | run the HTTP / SSE / NDJSON / WebSocket server — needs `--features server`, and points you at `axon-server` |
| `axon dossier` · `axon audit` · `axon sbom` | regulatory posture, gap analysis, SBOM |
| `axon pcc prove` · `axon pcc verify` | emit and independently check proof objects |
| `axon fmt` · `axon fix` · `axon repl` · `axon inspect` | the usual tooling |

`axon --help` lists all of them.

## As a library

```toml
[dependencies]
axon-lang = "4"
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
