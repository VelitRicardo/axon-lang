//! AXON compiler frontend.
//!
//! Pure frontend of the AXON language: lexer, parser, AST, epistemic
//! type primitives, type checker, IR generator, and the top-level
//! compile-time checker that glues them together.
//!
//! # Design contract
//!
//! This crate has **zero runtime dependencies**. The only allowed
//! external dep is `serde` (plus its proc-macro chain). Any addition
//! of a runtime dep (tokio, axum, sqlx, reqwest, aws-*, jsonwebtoken,
//! …) is rejected at CI time.
//!
//! # Consumers
//!
//! - `axon` crate (the AXON runtime in `../axon-rs/`) re-exports these
//!   modules so existing callers keep working.
//! - `axon-lsp` (Language Server, separate repo) consumes the frontend
//!   directly without dragging runtime deps.
//!
//! # Byte-identical parity
//!
//! Outputs must match the Python reference implementation
//! (`../axon/`) on the golden-file test corpus. Divergences are
//! release blockers.

pub mod ast;
pub mod checker;
/// v4.0.0 — the closed regulatory vocabulary Κ, in the crate that
/// type-checks it. The rich per-class metadata stays in
/// `axon-rs::esk::compliance`, which now derives its membership from here.
pub mod compliance;
/// v2.87.0 — the closed catalog of declared `effect`s + the design decision's bare-name
/// resolution. ONE derivation, shared by the IR generator and the type-checker,
/// so the two can never disagree about which effect owns an operation.
pub mod effect_catalog;
/// v2.87.0 — the static effect discipline: D9 exhaustiveness (interprocedural,
/// over the flow call graph), the design decision's resolution diagnostics, D10's structural
/// one-shot law, and the clause-scope law for `resume`/`abort`/`forward`.
pub mod effect_check;
pub mod cron;
pub mod epistemic;
pub mod ir_generator;
pub mod ir_nodes;
pub mod lexer;
pub mod parser;
pub mod smart_suggest;
pub mod store_column_proof;
pub mod store_introspect;
/// v2.65.0 — the symbolic differentiator + simplifier over the
/// closed `Expr` (the proof-carrying derivative).
pub mod expr_diff;
pub mod store_schema;
pub mod store_schema_manifest;
pub mod tokens;
pub mod type_checker;

// v1.4.0 — compile-time catalogs used by the type checker.
// `refinement` declares the closed Trust<T> catalog; `stream_effect`
// declares the closed backpressure policy catalog. Both are pure
// enum-like definitions with `std::fmt` only — no runtime deps.
// The matching runtime implementations (`trust_verifiers`,
// `stream_runtime`) live in the `axon` runtime crate.
pub mod refinement;
/// v2.83.0 — the `mandate` stability judgment (`D < |Kp+Ki+Kd| < 1/L`),
/// shared verbatim with the runtime controller in axon-rs.
pub mod stability;
/// v2.83.0 — the `fabric` substrate judgment (provider ↔ region ↔
/// jurisdiction), shared verbatim with the runtime.
pub mod substrate;
pub mod stream_effect;

// v1.4.0 — closed catalogue of regulatory authorisations
// (GDPR/CCPA/SOX/HIPAA/GLBA/PCI-DSS) used by the type checker to
// enforce `@legal_basis` annotations. Pure catalog, no runtime deps.
pub mod legal_basis;

// v1.4.0 — OTS (Ontological Tool Synthesis) compile-time slug
// catalogs. Runtime pipeline execution lives in `axon::ots` and
// re-exports these for backward compatibility.
pub mod ots_catalog;

// v1.6.0 — LSP-facing analysis primitives for typed channels.
// Pure AST helpers consumed by `axon-lsp` (sibling repo) to implement
// hover, completion, go-to-definition and find-references. Zero
// runtime deps — stays inside the v1.4.2 contract.
pub mod channel_analysis;

// v2.3.0 — session types: the pure algebra of typed bidirectional
// dialogue (WebSocket as a cognitive primitive). The session-type
// grammar + the duality involution `(·)⊥` + regular-coinductive
// equality for `μ`-types + the connection law (`peer ≡ self⊥`).
// Grounded in Caires–Pfenning (session types = intuitionistic linear
// propositions). Pure — no runtime deps; the `socket` surface (41.b),
// credit-refined backpressure (41.c) and the typed-WS runtime (41.d,
// in the `axon` crate) build on this. See
// docs/paper_websocket_cognitive_primitive.md.
pub mod session;
// v2.3.0 — multiparty session types (Honda–Yoshida–Carbone). A
// `GlobalType` declares an n-party protocol; projection `G⌐r` extracts
// each role's binary `SessionType` (the v2.3.0 algebra). The safe-
// realizability gate is `project_all`: a `Result::Ok` is the structural
// certificate that independent per-role runtimes faithfully realise `G`.
pub mod multiparty;

// v1.2.0 — the closed registry of every primitive AXON exposes as
// a named language construct. Single source of truth for the ℰMCP
// coverage gate + scaffold CLI + future LSP completions / docs-site
// generators. Pure const data, no runtime deps. See the module-level
// docs for the discipline (registry + corpus = atomic addition).
/// v2.67.0 — the anti-drift gate. The public README is parsed at test time and
/// every primitive it advertises must carry a human-attested statement of what
/// its runtime actually does. A presence-only gate would not have caught a single
/// v2.67.0 defect (`warden` and `quant` had a badge, a registry entry, a parser
/// production AND a dispatch arm — and were no-ops), so this one forces the
/// question no linter can decide.
pub mod advertised;
pub mod primitive_registry;

// v2.76.0 — the Epistemic Module System, rebuilt natively in Rust
// (papers/paper_ems_axon.md). The retired Python EMS (v0.23.0,
// gone since v2.0.0) advertised separate compilation the Rust toolchain
// never had: `import` parsed, lowered, and resolved NOTHING. v2.76.0 makes
// it real — and goes one phase further: the LINKER exists, so a
// multi-module program executes. Pure modules, zero new deps (SHA-256 is
// the v1.31.0 hand-rolled `sha256_hex`); in-memory-first so the LSP and the
// enterprise bundle loader resolve without a filesystem.
pub mod compilation_cache;
pub mod ems;
pub mod epistemic_compat;
pub mod module_interface;
pub mod module_linker;
pub mod module_resolver;
pub use primitive_registry::{
    by_category, coverage_summary, find as find_primitive, with_status, CoverageSummary,
    DocStatus, PrimitiveInfo, PRIMITIVE_REGISTRY,
};

// v2.37.0 — the blessed upstream preset catalog (versioned, forkable,
// ordinary `.axon` source per the design decision) + the `from Preset@vN` expansion the
// parser runs before type-check. Pure const data + a pure AST pass.
pub mod upstream_presets;

// v2.37.0 — `voice` macro-expansion to source text (the `axon desugar`
// payload). Pure AST pass run by the parser before preset expansion.
pub mod voice_desugar;

// v2.39.0 — Remote Hands: the pure, shared argv-template classifier + risk
// catalog used by BOTH the type-checker and the runtime dispatcher.
pub mod technician;

/// v2.46.0 — convert a duration literal (the lexer's `Duration` token
/// shape: digits + one of `s`/`ms`/`m`/`h`/`d`) into whole seconds. Pure,
/// total over the token grammar; `None` for anything else (a malformed
/// literal is `axon-T894` at the type-check layer). `ms` floors to whole
/// seconds — a sub-second credential TTL is `0` and rejected by the same
/// law. Shared by the IR lowering and the type checker so the two can
/// never disagree about what a `ttl:` means.
pub fn duration_literal_to_secs(literal: &str) -> Option<u64> {
    let t = literal.trim();
    let split = t.find(|c: char| !c.is_ascii_digit())?;
    let (digits, suffix) = t.split_at(split);
    let n: u64 = digits.parse().ok()?;
    match suffix {
        "s" => Some(n),
        "ms" => Some(n / 1000),
        "m" => n.checked_mul(60),
        "h" => n.checked_mul(3600),
        "d" => n.checked_mul(86_400),
        _ => None,
    }
}
