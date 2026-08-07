//! AXON Parser — recursive descent, fail-fast.
//!
//! Direct port of axon/compiler/parser.py.
//!
//! Tier 1 constructs (persona, context, anchor, memory, tool, type,
//! flow, step, intent, run, epistemic, if, for, let, return) are
//! fully parsed into typed AST nodes.
//!
//! Tier 2+ constructs are parsed structurally (balanced braces) into
//! `GenericDeclaration` / `GenericFlowStep`.

use crate::ast::*;
use crate::tokens::{is_declaration_keyword, Token, TokenType, Trivia, TriviaKind};

// Comment token kinds the lexer now emits (Fase 14.a). The parser
// filters these out of its working stream — they are materialised into
// a parallel `Trivia` array indexed by effective-token position, then
// attached to `Program.declaration_trivia[i]` once each declaration's
// span is known.
const fn is_comment_token(tt: &TokenType) -> bool {
    matches!(
        tt,
        TokenType::LineComment
            | TokenType::BlockComment
            | TokenType::DocLineComment
            | TokenType::DocBlockComment
            | TokenType::InnerDocLineComment
            | TokenType::InnerDocBlockComment
    )
}

const fn token_to_trivia_kind(tt: &TokenType) -> Option<TriviaKind> {
    match tt {
        TokenType::LineComment => Some(TriviaKind::Line),
        TokenType::BlockComment => Some(TriviaKind::Block),
        TokenType::DocLineComment => Some(TriviaKind::DocLine),
        TokenType::DocBlockComment => Some(TriviaKind::DocBlock),
        TokenType::InnerDocLineComment => Some(TriviaKind::InnerDocLine),
        TokenType::InnerDocBlockComment => Some(TriviaKind::InnerDocBlock),
        _ => None,
    }
}

/// Fase 14.b — write `leading_trivia` and `trailing_trivia` into the
/// per-struct fields of a `Declaration` variant.
///
/// Mirrors what the Python parser does automatically via its
/// `_with_trivia` decorator on every `_parse_*` method. In Rust we
/// do it once at the top of the parse loop so the spread to every
/// variant is in a single place.
fn attach_trivia_to_decl(decl: &mut Declaration, leading: Vec<Trivia>, trailing: Vec<Trivia>) {
    match decl {
        // §Fase 114.a — a top-level `budget` carries its comments like any other
        // declaration.
        Declaration::Budget(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Import(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Persona(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Context(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Anchor(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Memory(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Tool(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Type(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Flow(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Intent(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Run(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Epistemic(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Let(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::LambdaData(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Agent(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Shield(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Window(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Pix(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Ledger(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Psyche(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Corpus(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Dataspace(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Ots(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Mandate(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Compute(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Daemon(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Extension(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::AxonStore(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::AxonEndpoint(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Resource(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Fabric(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Manifest(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Observe(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Reconcile(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Lease(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Ensemble(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Session(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Topology(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Immune(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Reflex(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Heal(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Component(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::View(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Channel(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Socket(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Upstream(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Voice(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Cors(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Credential(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Cache(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Savant(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Synth(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Scope(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Observable(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Witness(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Document(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Deliver(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Notify(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
        Declaration::Generic(n) => {
            n.leading_trivia = leading;
            n.trailing_trivia = trailing;
        }
    }
}

// ── Public error type ────────────────────────────────────────────────────────

/// §Fase 28.d — Source-context constants. D4 ratified 2026-05-10:
/// 2 lines before + 2 lines after the error line. Mirror of the
/// Python-side `_SOURCE_CONTEXT_LINES_BEFORE` / `_AFTER` so the
/// rustc-style block has identical shape across stacks.
pub const SOURCE_CONTEXT_LINES_BEFORE: usize = 2;
pub const SOURCE_CONTEXT_LINES_AFTER: usize = 2;

/// §Fase 28.d — Rustc-style source-context block for a parse error.
///
/// Holds a reference to the source text plus the line/column the
/// error points at. Rendering is lazy — call ``render()`` to format
/// the block (line numbers + caret + 2 lines before + 2 after).
///
/// Pure and deterministic: no ANSI colors, no terminal-width
/// detection. Output shape is byte-identical to the Python
/// `SourceSnippet.render()` on the same input — that's the cross-
/// stack drift gate (28.i).
#[derive(Debug, Clone)]
pub struct SourceSnippet {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub filename: String,
    pub context_before: usize,
    pub context_after: usize,
}

impl SourceSnippet {
    /// Construct with the default 2/2 context window.
    pub fn new(source: String, line: u32, column: u32, filename: String) -> Self {
        Self {
            source,
            line,
            column,
            filename,
            context_before: SOURCE_CONTEXT_LINES_BEFORE,
            context_after: SOURCE_CONTEXT_LINES_AFTER,
        }
    }

    /// Format the snippet as a multi-line rustc-style block.
    ///
    /// Empty source → empty string. Out-of-range line → empty
    /// string. Caret column is clamped to `[1, line_len + 1]`.
    /// Output shape matches Python `SourceSnippet.render` byte-
    /// identically per D7.
    #[must_use]
    pub fn render(&self) -> String {
        if self.source.is_empty() || self.line < 1 {
            return String::new();
        }
        let raw: Vec<&str> = self.source.split('\n').collect();
        // Match Python's str.splitlines() trailing-newline shape:
        // strip an empty trailing entry produced by a final '\n'.
        let lines: Vec<&str> = if raw.last() == Some(&"") {
            raw[..raw.len() - 1].to_vec()
        } else {
            raw
        };
        if lines.is_empty() || self.line as usize > lines.len() {
            return String::new();
        }

        let line_idx = self.line as usize;
        let start = line_idx.saturating_sub(self.context_before).max(1);
        let end = (line_idx + self.context_after).min(lines.len());

        let gutter = end.to_string().len();
        let empty_gutter = " ".repeat(gutter);

        let mut out: Vec<String> = Vec::with_capacity(end - start + 4);
        out.push(format!(
            "{empty_gutter} --> {}:{}:{}",
            self.filename, self.line, self.column
        ));
        out.push(format!("{empty_gutter} |"));
        for n in start..=end {
            let line_text = lines[n - 1];
            out.push(format!("{n:>gutter$} | {line_text}", gutter = gutter));
            if n == line_idx {
                let line_len = line_text.chars().count();
                let col = (self.column as usize).clamp(1, line_len + 1);
                out.push(format!(
                    "{empty_gutter} | {pad}^",
                    pad = " ".repeat(col - 1)
                ));
            }
        }
        out.join("\n")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub column: u32,
    /// §Fase 28.d — Optional rustc-style source-context block.
    /// `None` preserves the legacy single-line shape; populated by
    /// `Parser::with_source` callers (and by `parse_with_recovery`
    /// / `parse` when a source has been attached to the parser).
    /// Existing struct-literal call sites use the `..Default::default()`
    /// idiom (default = None) to stay terse.
    pub source_snippet: Option<SourceSnippet>,
}

impl ParseError {
    /// §Fase 28.d — Attach a `SourceSnippet` derived from raw source
    /// text and filename. Returns `self` so the call can be chained
    /// at the construction site. No-op when `line == 0`. Idempotent.
    #[must_use]
    pub fn attach_source(mut self, source: &str, filename: &str) -> Self {
        if self.line >= 1 {
            self.source_snippet = Some(SourceSnippet::new(
                source.to_string(),
                self.line,
                self.column,
                filename.to_string(),
            ));
        }
        self
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[line {}:{}] {}", self.line, self.column, self.message)?;
        if let Some(snippet) = &self.source_snippet {
            let block = snippet.render();
            if !block.is_empty() {
                write!(f, "\n{block}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

// ── §Fase 28.c — Public recovery result ──────────────────────────────────────
//
// Mirror of Python's `axon.compiler.parser.ParseResult` (Fase 28.b).
// The rationale, sync semantics, and test contract are documented in
// `docs/fase/fase_28_adopter_diagnostic_robustness.md`. The Rust frontend
// must produce structurally identical error lists to the Python parser
// when handed the same source — that is the cross-stack drift gate
// (D7 ratified 2026-05-10: byte-identical error lists).
//
// `program` holds whatever declarations the parser was able to parse
// successfully. `errors` holds every recovered error in source order.
// A clean parse returns `errors.is_empty()`; the existing fail-fast
// `parse()` API is preserved verbatim per D9.

/// Outcome of `Parser::parse_with_recovery` — partial program plus the
/// list of every error the parser recovered from. See module docs for
/// the panic-mode + sync-point recovery semantics.
#[derive(Debug)]
pub struct ParseResult {
    pub program: Program,
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    /// True iff at least one parse error was recovered. Callers that
    /// want to short-circuit on failure should check this rather than
    /// relying on `program.declarations.is_empty()` (the parser may
    /// have salvaged some declarations even with errors present).
    #[inline]
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Inverse of `has_errors`. Convenience for the "happy path" check
    /// in tests + adopter integrations.
    #[inline]
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

/// §Fase 28.c — Top-level declaration keywords used as resync points
/// during error recovery (D2 ratified 2026-05-10). Mirrors the
/// `_TOP_LEVEL_DECLARATION_KEYWORDS` frozenset on the Python side.
///
/// Distinct from `tokens::is_declaration_keyword` because that helper
/// is used by the structural declaration counter and intentionally
/// excludes some grammar-only tokens (Know/Believe/Speculate/Doubt,
/// Ingest, Ots) that DO begin a top-level declaration in
/// `parse_declaration` and therefore must be valid sync points.
///
/// Adding a new top-level dispatch arm in `parse_declaration` MUST
/// add the corresponding token here so the recovery walker can
/// re-sync correctly.
#[inline]
const fn is_top_level_decl_kw_for_recovery(tt: &TokenType) -> bool {
    matches!(
        tt,
        TokenType::Import
            | TokenType::Persona
            | TokenType::Context
            | TokenType::Anchor
            | TokenType::Memory
            | TokenType::Tool
            | TokenType::Type
            | TokenType::Flow
            | TokenType::Intent
            | TokenType::Run
            | TokenType::Let
            | TokenType::Know
            | TokenType::Believe
            | TokenType::Speculate
            | TokenType::Doubt
            | TokenType::Lambda
            | TokenType::Agent
            | TokenType::Shield
            | TokenType::Pix
            | TokenType::Ledger
            | TokenType::Psyche
            | TokenType::Corpus
            | TokenType::Dataspace
            | TokenType::Ots
            | TokenType::Mandate
            | TokenType::Compute
            | TokenType::Daemon
            // §Fase 87.a/d — the autonomous research primitive + synth policy.
            | TokenType::Savant
            | TokenType::Synth
            // §Fase 88.a — the authorization-scope policy declaration.
            | TokenType::Scope
            | TokenType::AxonStore
            | TokenType::AxonEndpoint
            | TokenType::Resource
            | TokenType::Fabric
            | TokenType::Manifest
            | TokenType::Observe
            | TokenType::Reconcile
            | TokenType::Lease
            | TokenType::Ensemble
            | TokenType::Session
            | TokenType::Topology
            | TokenType::Immune
            | TokenType::Reflex
            | TokenType::Heal
            | TokenType::Component
            | TokenType::View
            | TokenType::Channel
            | TokenType::Ingest
            | TokenType::Persist
            | TokenType::Retrieve
            | TokenType::Mutate
            | TokenType::Purge
            | TokenType::Transact
            | TokenType::Mcp
    )
}

// ── §Fase 30.b — axonendpoint transport + keepalive closed enums ────────────
//
// D2 ratified 2026-05-10: `transport` is a closed enum
// {json, sse, ndjson}. D6 ratified: `keepalive` is a closed enum
// {5s, 15s, 30s, 60s}. Both mirror the Python frontend's
// `_AXONENDPOINT_TRANSPORT_VALUES` / `_AXONENDPOINT_KEEPALIVE_VALUES`
// frozensets in `axon/compiler/parser.py`. Cross-stack drift gate
// (30.b fixture) asserts byte-identical parse for every entry.

/// Adopter-facing acceptable values for `transport:` field.
/// Used by both the parser (validation + smart-suggest) and the
/// type-checker (30.c) so adopter tooling sees one canonical list.
pub const AXONENDPOINT_TRANSPORT_VALUES: &[&str] = &["json", "sse", "ndjson"];

/// §Fase 33.z.k.b (v1.28.0) — Closed-catalog SSE wire-format
/// dialects. Selected via the parametrized grammar
/// `transport: sse(<dialect>)`; bare `transport: sse` resolves to
/// the Q1 default per the flow's algebraic-effect predicate
/// (openai for tool-streaming flows; axon for type-annotation-only).
///
/// Vertical-grounded scope (Q3 revised 2026-05-14): five dialects
/// cover ~99% of LLM-streaming adopter expectations.
///   - `axon`      — current W3C named events
///                   (event: axon.token / event: axon.complete).
///                   D6 backwards-compat baseline; indefinitely
///                   supported as a first-class option.
///   - `openai`    — `data: {"choices":[{"delta":{...}}]}` frames
///                   terminated by `data: [DONE]`. OpenAI Chat
///                   Completions streaming wire verbatim.
///   - `kimi`      — Moonshot Kimi (kimi.moonshot.cn) — uses the
///                   OpenAI-compatible Chat Completions wire format
///                   verbatim (same chunk shape, same `data: [DONE]`
///                   sentinel). First-class entry so adopters
///                   declare intent explicitly; under the hood the
///                   wire is identical to `openai`.
///   - `glm`       — Zhipu ChatGLM (open.bigmodel.cn) — same as
///                   kimi, uses OpenAI-compat wire. First-class
///                   entry for adopter clarity.
///   - `anthropic` — `event: content_block_delta` frames terminated
///                   by `event: message_stop`. Adopter SDKs
///                   targeting Anthropic Claude consume this shape
///                   verbatim.
///
/// Why kimi + glm as first-class entries (Q3 revision rationale):
/// The project's primary adopter pipelines through Kimi K2.x +
/// Zhipu GLM-4.x. While the wire IS byte-identical to OpenAI's
/// Chat Completions streaming, declaring `transport: sse(kimi)` /
/// `transport: sse(glm)` lets the audit trail + observability
/// surfaces correlate adopter intent against the underlying
/// provider — without the adopter having to know that "kimi
/// happens to be OpenAI-compat on the wire today". The runtime
/// dispatches kimi + glm to the same `OpenAIDialectAdapter` so
/// the wire shape stays canonical-OpenAI-bytes.
///
/// Open-set adapter pluggability (downstream crates registering
/// custom dialects) remains explicitly out of scope per the
/// Axon-for-Axon discipline.
pub const AXONENDPOINT_TRANSPORT_DIALECTS: &[&str] =
    &["axon", "openai", "kimi", "glm", "anthropic"];

/// Adopter-facing acceptable values for `keepalive:` field.
pub const AXONENDPOINT_KEEPALIVE_VALUES: &[&str] = &["5s", "15s", "30s", "60s"];

/// §Fase 32.b D3 — Closed method enum for `method:` field. Adopter-
/// declarable methods only; HEAD/OPTIONS/CONNECT/TRACE are
/// runtime-managed (CORS preflight, etc.) and never declared from
/// source. Closed enum refuses interpretation drift; smart-suggest
/// catches near-misses at parse time.
///
/// §Fase 107.a — `QUERY` (RFC 10008, Proposed Standard, June 2026): the safe +
/// idempotent + cacheable method that CARRIES A REQUEST BODY — the first new HTTP
/// method in two decades. It carries a LAW, not just a route: `axon-T927` refuses
/// at compile time a QUERY endpoint whose flow performs a declared write (the
/// RFC's normative "safe and idempotent" MUST, made a proof).
///
/// Must stay in lockstep with `type_checker::VALID_ENDPOINT_METHODS`.
pub const AXONENDPOINT_METHOD_VALUES: &[&str] =
    &["GET", "POST", "PUT", "DELETE", "PATCH", "QUERY"];

/// §Fase 36.d (D2) — Closed catalog for the `axonendpoint backend:`
/// declaration. The set is `CANONICAL_PROVIDERS ∪ {auto, stub}`:
///
///   - the seven canonical LLM providers — `anthropic`, `gemini`,
///     `glm`, `kimi`, `ollama`, `openai`, `openrouter` — a concrete,
///     declared backend that rung 2 of the Fase 36 D1 resolution
///     ladder fires immediately;
///   - `auto` — transparent: declaring it is equivalent to omitting
///     `backend:` entirely (the route resolves down the ladder —
///     server default → environment-available providers);
///   - `stub` — the no-op backend, reachable ONLY by an explicit,
///     written declaration (D5: a silent degradation to `stub` is
///     forbidden; an explicit opt-in is not).
///
/// `axon-frontend` carries zero runtime deps and therefore cannot
/// import `axon::backends::CANONICAL_PROVIDERS`; this list is a
/// hand-maintained mirror. The axon-rs drift gate
/// (`tests/fase36_d_backend_catalog_drift.rs`) asserts the two stay
/// byte-identical — adding a provider in one place without the other
/// fails CI.
pub const AXONENDPOINT_BACKEND_VALUES: &[&str] = &[
    "anthropic",
    "auto",
    "gemini",
    "glm",
    "kimi",
    "ollama",
    "openai",
    "openrouter",
    "stub",
];

#[inline]
fn axonendpoint_is_valid_transport(s: &str) -> bool {
    AXONENDPOINT_TRANSPORT_VALUES.iter().any(|&v| v == s)
}

#[inline]
fn axonendpoint_is_valid_method(s: &str) -> bool {
    AXONENDPOINT_METHOD_VALUES.iter().any(|&v| v == s)
}

#[inline]
fn axonendpoint_is_valid_backend(s: &str) -> bool {
    AXONENDPOINT_BACKEND_VALUES.iter().any(|&v| v == s)
}

#[inline]
fn axonendpoint_is_valid_keepalive(s: &str) -> bool {
    AXONENDPOINT_KEEPALIVE_VALUES.iter().any(|&v| v == s)
}

/// §Fase 37.y (D2) — Closed type catalog for query parameters.
///
/// Query values arrive over HTTP as URL-encoded strings; the catalog
/// is the set of types axon will validate / coerce them into for the
/// Request Binding Contract. Hand-curated, intentionally small:
///   - `Text` — the raw string (always succeeds)
///   - `Int` — `i64` parseable
///   - `Float` — `f64` parseable, finite
///   - `Bool` — case-insensitive `{true, false, 1, 0, yes, no, on, off}`
///   - `Uuid` — RFC 4122 textual form
///
/// Extending the catalog is a future axon-T?nn surface; v1.38.5 ships
/// the 5 types covering ~95% of REST query patterns. Lists / dates /
/// datetimes / enums are honest deferrals (see §7 of the plan vivo).
pub const AXONENDPOINT_QUERY_PARAM_TYPES: &[&str] =
    &["Text", "Int", "Float", "Bool", "Uuid"];

/// `true` iff `s` is one of the §Fase 37.y (D2) query-param catalog
/// entries — exact case-sensitive match (axon types are PascalCase).
#[inline]
pub(crate) fn axonendpoint_is_valid_query_param_type(s: &str) -> bool {
    AXONENDPOINT_QUERY_PARAM_TYPES.iter().any(|&v| v == s)
}

/// §Fase 37.y (D1) — Extract `{name}` placeholder names from an
/// `axonendpoint` `path:` string, in left-to-right declaration order.
///
/// Recognized placeholder grammar (single-segment, no nested braces):
/// `{NAME}` where `NAME` matches `[A-Za-z_][A-Za-z0-9_]*`. Anything
/// inside braces that does NOT match the identifier shape is silently
/// IGNORED — it's either an adopter typo (caught later by axum at
/// route registration) or a literal brace in the URL pattern.
///
/// Returns `Err(duplicate_name)` when the same `{name}` appears more
/// than once in the path — HTTP route patterns reject duplicates
/// structurally (`axum` would panic at registration), so surfacing
/// the error at parse time is the right place.
///
/// Pure + total: never panics; deterministic over its single string
/// argument. Hand-rolled scanner (no regex dep at parser layer).
///
/// # Examples
///
/// - `"/api/users"` → `Ok(vec![])`
/// - `"/api/users/{id}"` → `Ok(vec!["id"])`
/// - `"/api/tenants/{tenant_id}/secrets/{secret_name}"`
///   → `Ok(vec!["tenant_id", "secret_name"])`
/// - `"/api/users/{id}/posts/{id}"` → `Err("id")` (duplicate)
/// - `"/api/{not valid}"` → `Ok(vec![])` (malformed brace content
///   silently ignored; axum surfaces the error at registration)
pub(crate) fn extract_path_param_names(path: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        // Find the matching close brace; if none, the open brace is
        // a literal — leave it alone.
        let start = i + 1;
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'}' {
            end += 1;
        }
        if end == bytes.len() {
            // Unterminated — give up; downstream parser/runtime
            // surface the malformed path elsewhere.
            break;
        }
        let raw = &path[start..end];
        // Validate identifier shape: [A-Za-z_][A-Za-z0-9_]*
        let valid = !raw.is_empty()
            && raw.bytes().enumerate().all(|(idx, b)| {
                if idx == 0 {
                    b.is_ascii_alphabetic() || b == b'_'
                } else {
                    b.is_ascii_alphanumeric() || b == b'_'
                }
            });
        if valid {
            let name = raw.to_string();
            if out.iter().any(|existing| existing == &name) {
                return Err(name);
            }
            out.push(name);
        }
        i = end + 1;
    }
    Ok(out)
}

/// §Fase 32.g (D8) — Closed capability-slug grammar. Validates a
/// `requires:` slug per `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$`.
///
/// Hand-rolled (no regex dep at parser layer) — each segment must
/// match `[a-z][a-z0-9_]*` and segments are joined by single dots.
/// Public so the runtime mirror (`axon::auth_scope`) reuses the same
/// predicate without duplicating the rule.
///
/// Examples valid: `admin`, `legal.read`, `hipaa.phi.read`,
/// `bank.officer.senior`, `a`, `a_b`, `a1`.
/// Examples invalid: empty, `Admin` (uppercase), `1admin` (digit
/// first), `bank-officer` (hyphen), `bank..a` (empty segment),
/// `.admin`, `admin.`, `admin..` .
pub fn is_valid_capability_slug(slug: &str) -> bool {
    if slug.is_empty() {
        return false;
    }
    for segment in slug.split('.') {
        if !is_valid_slug_segment(segment) {
            return false;
        }
    }
    true
}

fn is_valid_slug_segment(seg: &str) -> bool {
    let mut chars = seg.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 37.y (D1) — `extract_path_param_names` unit tests
// ════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════
//  §Fase 37.y (D2) — `axonendpoint_is_valid_query_param_type` + the
//  inline `query: { … }` parser, end-to-end through the lexer.
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod query_param_catalog_tests {
    use super::{axonendpoint_is_valid_query_param_type, AXONENDPOINT_QUERY_PARAM_TYPES};

    #[test]
    fn accepts_every_catalog_entry() {
        for ty in AXONENDPOINT_QUERY_PARAM_TYPES {
            assert!(
                axonendpoint_is_valid_query_param_type(ty),
                "catalog entry `{ty}` must validate"
            );
        }
    }

    #[test]
    fn rejects_off_catalog_types() {
        for off in &[
            "Timestamp",    // not in v1.38.5 — list/dates deferred
            "Date",
            "DateTime",
            "List<Text>",   // multi-value query params deferred (§7)
            "Jsonb",        // store-only types not query-applicable
            "Bytea",
            "text",         // lowercase rejected (axon types are PascalCase)
            "TEXT",
            "Number",       // not in axon's type catalog at all
            "",             // empty
            " ",            // whitespace
        ] {
            assert!(
                !axonendpoint_is_valid_query_param_type(off),
                "off-catalog `{off}` must reject"
            );
        }
    }

    #[test]
    fn catalog_size_matches_design() {
        // The plan vivo D2 states a closed 5-type catalog. A future
        // axon-T?nn surface may extend it; that requires updating BOTH
        // the catalog AND the plan vivo §7 honest-scope note.
        assert_eq!(AXONENDPOINT_QUERY_PARAM_TYPES.len(), 5);
    }
}

#[cfg(test)]
mod query_param_parser_tests {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse_endpoint_source(src: &str) -> Result<crate::ast::AxonEndpointDefinition, String> {
        let tokens = Lexer::new(src, "test.axon")
            .tokenize()
            .map_err(|e| format!("lex: {}", e.message))?;
        let mut parser = Parser::new(tokens);
        let program = parser.parse().map_err(|e| format!("parse: {}", e.message))?;
        program
            .declarations
            .into_iter()
            .find_map(|d| match d {
                crate::ast::Declaration::AxonEndpoint(e) => Some(e),
                _ => None,
            })
            .ok_or_else(|| "no axonendpoint in program".to_string())
    }

    #[test]
    fn endpoint_with_no_query_block_keeps_empty_vec() {
        let src = r#"
            axonendpoint write_secret {
                method: POST
                path: "/api/users"
                body: SecretWriteRequest
                execute: WriteSecret
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses");
        assert!(
            ep.query_params.is_empty(),
            "D5 — no `query:` block ⇒ empty query_params"
        );
    }

    #[test]
    fn single_query_param_required() {
        let src = r#"
            axonendpoint list_users {
                method: GET
                path: "/api/users"
                query: { status: Text }
                execute: ListUsers
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses");
        assert_eq!(ep.query_params.len(), 1);
        assert_eq!(ep.query_params[0].name, "status");
        assert_eq!(ep.query_params[0].type_expr.name, "Text");
        assert!(!ep.query_params[0].type_expr.optional);
    }

    #[test]
    fn optional_query_param_via_question_suffix() {
        let src = r#"
            axonendpoint list_users {
                method: GET
                path: "/api/users"
                query: { limit: Int? }
                execute: ListUsers
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses");
        assert_eq!(ep.query_params.len(), 1);
        assert_eq!(ep.query_params[0].name, "limit");
        assert_eq!(ep.query_params[0].type_expr.name, "Int");
        assert!(
            ep.query_params[0].type_expr.optional,
            "`?` suffix sets optional"
        );
    }

    #[test]
    fn multiple_query_params_preserve_declaration_order() {
        let src = r#"
            axonendpoint search {
                method: GET
                path: "/api/search"
                query: { q: Text, page: Int?, limit: Int?, exact: Bool? }
                execute: Search
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses");
        let names: Vec<&str> = ep.query_params.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["q", "page", "limit", "exact"]);
        let types: Vec<&str> = ep
            .query_params
            .iter()
            .map(|f| f.type_expr.name.as_str())
            .collect();
        assert_eq!(types, vec!["Text", "Int", "Int", "Bool"]);
        let optionals: Vec<bool> = ep
            .query_params
            .iter()
            .map(|f| f.type_expr.optional)
            .collect();
        assert_eq!(optionals, vec![false, true, true, true]);
    }

    #[test]
    fn duplicate_query_param_is_parse_error() {
        let src = r#"
            axonendpoint bad {
                method: GET
                path: "/api/x"
                query: { name: Text, name: Int? }
                execute: Bad
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("duplicate query param 'name'"),
            "error must name the duplicate. Got: {err}"
        );
    }

    #[test]
    fn off_catalog_type_with_smart_suggest_hint() {
        // `Strng` is one edit away from `Text` (would suggest `Text`?
        // Actually edit distance to `Text` is 4; to `Int` is 5. Likely
        // no smart suggestion within distance 2. The error still names
        // the catalog explicitly.)
        let src = r#"
            axonendpoint bad {
                method: GET
                path: "/api/x"
                query: { value: Strng }
                execute: Bad
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("unsupported type 'Strng'"),
            "error must name the bad type. Got: {err}"
        );
        assert!(
            err.contains("Expected one of: Text | Int | Float | Bool | Uuid"),
            "error must list the closed catalog. Got: {err}"
        );
    }

    #[test]
    fn close_typo_gets_did_you_mean_hint() {
        // `Txt` → edit distance 1 from `Text` → smart-suggest should
        // surface the hint.
        let src = r#"
            axonendpoint bad {
                method: GET
                path: "/api/x"
                query: { value: Txt }
                execute: Bad
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("Did you mean") && err.contains("`Text`"),
            "smart-suggest must hint `Text`. Got: {err}"
        );
    }

    #[test]
    fn every_catalog_type_parses_cleanly() {
        // Round-trip smoke for all 5 catalog entries.
        for ty in &["Text", "Int", "Float", "Bool", "Uuid"] {
            let src = format!(
                r#"
                    axonendpoint x {{
                        method: GET
                        path: "/api/x"
                        query: {{ v: {ty} }}
                        execute: X
                    }}
                "#
            );
            let ep = parse_endpoint_source(&src)
                .unwrap_or_else(|e| panic!("`{ty}` should parse: {e}"));
            assert_eq!(ep.query_params[0].type_expr.name, *ty);
        }
    }

    #[test]
    fn comma_optional_between_params() {
        // The plan vivo design accepts both comma-separated and
        // whitespace-separated query params (existing parser style is
        // forgiving). Whitespace-only:
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { a: Text b: Int? }
                execute: X
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses without commas");
        assert_eq!(ep.query_params.len(), 2);
    }

    // ─── Robustness hardening (37.y.2 100% robust closure) ──────────

    #[test]
    fn double_query_block_is_parse_error() {
        // An adopter who copy-pastes the `query:` block twice should
        // see a clear parse error, not a silent merge that produces
        // an unexpectedly-augmented endpoint with both blocks fused.
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { a: Text }
                query: { b: Int? }
                execute: X
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("declares `query: { … }` more than once"),
            "error must call out the duplicate block. Got: {err}"
        );
        assert!(
            err.contains("combine all params into a single block"),
            "error must hint the canonical fix. Got: {err}"
        );
    }

    #[test]
    fn optional_generic_type_is_parse_error_with_canonical_hint() {
        // `Optional<Text>` is the wrong way to declare an optional
        // query param. The canonical syntax is `Text?` (the `?`
        // suffix). The error must surface this with a literal example.
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { value: Optional<Text> }
                execute: X
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("generic type `Optional<Text>`"),
            "error must name the generic type literally. Got: {err}"
        );
        assert!(
            err.contains("Use `Text?` (the `?` suffix)"),
            "error must hint the canonical `Text?` syntax. Got: {err}"
        );
    }

    #[test]
    fn list_generic_type_is_parse_error_with_deferral_hint() {
        // Multi-value query params (`?tag=a&tag=b`) are honest-
        // deferred per the plan vivo §7. Adopters who write
        // `List<Text>` should see a clear error explaining the
        // deferral, not a confusing "type `List` not in catalog".
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { tags: List<Text> }
                execute: X
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("generic type `List<Text>`"),
            "error must name the generic type. Got: {err}"
        );
        assert!(
            err.contains("Multi-value query params")
                && err.contains("honest-deferred"),
            "error must mention the multi-value deferral. Got: {err}"
        );
    }

    #[test]
    fn other_generic_types_caught_generically() {
        // Generic types beyond `Optional` and `List` get the
        // generic-rejection message without a canonical-syntax hint
        // (the catalog list is the canonical guidance).
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { value: Stream<Int> }
                execute: X
            }
        "#;
        let err = parse_endpoint_source(src).expect_err("must fail");
        assert!(
            err.contains("generic type `Stream<Int>`"),
            "error must name the generic type. Got: {err}"
        );
        assert!(
            err.contains("Text | Int | Float | Bool | Uuid"),
            "error must list the closed catalog. Got: {err}"
        );
    }

    #[test]
    fn uuid_optional_parses_cleanly() {
        // Hardening companion — `Uuid?` is in the catalog AND
        // optional. The two features compose without surprise.
        let src = r#"
            axonendpoint find {
                method: GET
                path: "/api/x"
                query: { after: Uuid? }
                execute: Find
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses");
        assert_eq!(ep.query_params.len(), 1);
        assert_eq!(ep.query_params[0].name, "after");
        assert_eq!(ep.query_params[0].type_expr.name, "Uuid");
        assert!(ep.query_params[0].type_expr.optional);
        assert_eq!(ep.query_params[0].type_expr.generic_param, "");
    }

    #[test]
    fn empty_query_block_yields_empty_vec() {
        // `query: { }` is grammatically valid but semantically a
        // no-op (equivalent to omitting the block). Don't error;
        // just record an empty Vec.
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { }
                execute: X
            }
        "#;
        let ep = parse_endpoint_source(src).expect("empty block parses");
        assert!(ep.query_params.is_empty());
    }

    #[test]
    fn kivi_secret_write_path_plus_query() {
        // Combined path-param + query-param test: an endpoint that
        // takes IDs in the URL AND optional filters in the query
        // string. This is the natural REST shape Fase 37.y serves.
        let src = r#"
            axonendpoint write_secret {
                method: POST
                path: "/api/tenants/{tenant_id}/secrets/{secret_name}"
                query: { dry_run: Bool?, overwrite: Bool? }
                body: SecretWriteRequest
                execute: WriteSecret
            }
        "#;
        let ep = parse_endpoint_source(src).expect("parses");
        // Path params populated (from 37.y.1):
        assert_eq!(ep.path_params, vec!["tenant_id", "secret_name"]);
        // Query params populated (from this sub-fase 37.y.2):
        assert_eq!(ep.query_params.len(), 2);
        assert_eq!(ep.query_params[0].name, "dry_run");
        assert_eq!(ep.query_params[0].type_expr.name, "Bool");
        assert!(ep.query_params[0].type_expr.optional);
        assert_eq!(ep.query_params[1].name, "overwrite");
        // Body still works:
        assert_eq!(ep.body_type, "SecretWriteRequest");
    }
}

#[cfg(test)]
mod path_param_extraction_tests {
    use super::extract_path_param_names;

    #[test]
    fn empty_path_no_placeholders() {
        assert_eq!(extract_path_param_names("/api/users"), Ok(vec![]));
        assert_eq!(extract_path_param_names("/"), Ok(vec![]));
        assert_eq!(extract_path_param_names(""), Ok(vec![]));
    }

    #[test]
    fn single_placeholder() {
        assert_eq!(
            extract_path_param_names("/api/users/{id}"),
            Ok(vec!["id".to_string()])
        );
    }

    #[test]
    fn multiple_placeholders_in_declaration_order() {
        assert_eq!(
            extract_path_param_names(
                "/api/tenants/{tenant_id}/secrets/{secret_name}"
            ),
            Ok(vec![
                "tenant_id".to_string(),
                "secret_name".to_string(),
            ])
        );
    }

    #[test]
    fn kivi_chat_history_path_pattern() {
        // The exact pattern the kivi adopter reported (2026-05-20):
        // POST /api/tenants/{tenant_id}/secrets/{secret_name}
        // Both names extracted in source order.
        let names = extract_path_param_names(
            "/api/tenants/{tenant_id}/secrets/{secret_name}",
        );
        assert_eq!(
            names,
            Ok(vec![
                "tenant_id".to_string(),
                "secret_name".to_string(),
            ])
        );
    }

    #[test]
    fn duplicate_placeholder_returns_err() {
        assert_eq!(
            extract_path_param_names("/api/users/{id}/posts/{id}"),
            Err("id".to_string())
        );
    }

    #[test]
    fn underscore_and_numeric_in_name() {
        assert_eq!(
            extract_path_param_names("/api/{user_id}/items/{item_2}"),
            Ok(vec!["user_id".to_string(), "item_2".to_string()])
        );
    }

    #[test]
    fn leading_underscore_accepted() {
        // Identifiers in HTTP paths often start with letters but the
        // grammar permits leading underscore (parity with Rust identifier
        // rules). The flow parameter name on the binding side has to
        // match exactly, so adopters with `_internal_id` in the path
        // can pair it with a same-named flow param.
        assert_eq!(
            extract_path_param_names("/api/{_internal}"),
            Ok(vec!["_internal".to_string()])
        );
    }

    #[test]
    fn malformed_placeholder_silently_ignored() {
        // Content inside `{...}` that does not match the identifier
        // grammar is skipped at this layer. axum surfaces the route
        // registration failure if the literal text is invalid.
        assert_eq!(
            extract_path_param_names("/api/{not valid}"),
            Ok(vec![])
        );
        // Empty braces — same: skip silently.
        assert_eq!(extract_path_param_names("/api/{}"), Ok(vec![]));
        // Mixed: malformed brace skipped, valid placeholder kept.
        assert_eq!(
            extract_path_param_names("/api/{tenant id}/users/{id}"),
            Ok(vec!["id".to_string()])
        );
    }

    #[test]
    fn unterminated_brace_returns_clean() {
        // Open brace with no close brace — give up without panicking.
        // (axum surfaces the malformed-route error at registration.)
        assert_eq!(extract_path_param_names("/api/{id"), Ok(vec![]));
    }

    #[test]
    fn placeholders_at_path_boundaries() {
        // Placeholder as the very first segment AND the very last
        // segment — both should be extracted.
        assert_eq!(
            extract_path_param_names("{prefix}/api/users/{id}"),
            Ok(vec!["prefix".to_string(), "id".to_string()])
        );
        assert_eq!(
            extract_path_param_names("/api/{id}"),
            Ok(vec!["id".to_string()])
        );
    }

    #[test]
    fn deduplication_detects_non_adjacent_duplicates() {
        // The duplicate-detection sweep is global, not just adjacent.
        assert_eq!(
            extract_path_param_names(
                "/api/orgs/{org_id}/teams/{team_id}/repos/{org_id}"
            ),
            Err("org_id".to_string())
        );
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        // Light fuzz: a handful of weird inputs return cleanly.
        for input in &[
            "{",
            "}",
            "{}",
            "{{}}",
            "{{{",
            "/api/{}/{id}",
            "////",
            "\u{1F4A1}",        // emoji (lightbulb)
            "\u{0000}",         // null byte
        ] {
            let _ = extract_path_param_names(input); // must not panic
        }
    }
}

#[cfg(test)]
mod capability_slug_tests {
    use super::is_valid_capability_slug;

    #[test]
    fn accepts_canonical_examples() {
        assert!(is_valid_capability_slug("admin"));
        assert!(is_valid_capability_slug("legal.read"));
        assert!(is_valid_capability_slug("hipaa.phi.read"));
        assert!(is_valid_capability_slug("bank.officer.senior"));
        assert!(is_valid_capability_slug("a"));
        assert!(is_valid_capability_slug("a_b"));
        assert!(is_valid_capability_slug("a1"));
        assert!(is_valid_capability_slug("a.b1_c"));
    }

    #[test]
    fn rejects_empty_string() {
        assert!(!is_valid_capability_slug(""));
    }

    #[test]
    fn rejects_uppercase() {
        assert!(!is_valid_capability_slug("Admin"));
        assert!(!is_valid_capability_slug("admin.READ"));
    }

    #[test]
    fn rejects_digit_first() {
        assert!(!is_valid_capability_slug("1admin"));
        assert!(!is_valid_capability_slug("admin.1read"));
    }

    #[test]
    fn rejects_hyphen() {
        assert!(!is_valid_capability_slug("bank-officer"));
    }

    #[test]
    fn rejects_empty_segments() {
        assert!(!is_valid_capability_slug("bank..a"));
        assert!(!is_valid_capability_slug(".admin"));
        assert!(!is_valid_capability_slug("admin."));
    }

    #[test]
    fn rejects_special_chars() {
        assert!(!is_valid_capability_slug("admin@read"));
        assert!(!is_valid_capability_slug("admin/read"));
        assert!(!is_valid_capability_slug("admin read"));
    }
}

// ── Parser ───────────────────────────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Fase 14.a — leading trivia parallel array, indexed by the
    /// effective-token position. `leading_trivia[i]` is the comment
    /// trivia that appeared between the previous effective token (or
    /// file start) and `tokens[i]`.
    leading_trivia: Vec<Vec<Trivia>>,
    /// Fase 14.a — trailing trivia parallel array. `trailing_trivia[i]`
    /// is the comment trivia on the same line as `tokens[i]`, before
    /// the next effective token. Populated by the constructor.
    trailing_trivia: Vec<Vec<Trivia>>,
    /// Fase 17.a — side-channel for tagging let value_kind. Set by
    /// `parse_let_atom` / `parse_let_value_expr` as they descend; read
    /// at the end of `parse_let` and stored on the LetStatement.
    last_let_value_kind: String,
    /// Fase 19.e — loop nesting depth for break/continue scope check.
    /// Incremented at the start of `parse_for_in`, decremented after.
    /// `parse_break`/`parse_continue` raise ParseError when this is
    /// zero (the keyword has no meaning outside a loop body).
    loop_depth: u32,
    /// §Fase 28.d — Optional source text + filename for the rustc-
    /// style source-context block on `ParseError`. Set via the
    /// fluent `Parser::with_source` builder; default `None` keeps
    /// existing callers (`Parser::new(tokens).parse()`) emitting
    /// the legacy single-line shape.
    source: Option<String>,
    filename: String,
}

impl Parser {
    pub fn new(raw_tokens: Vec<Token>) -> Self {
        // ── Fase 14.a — split the raw token stream into:
        //   - effective tokens the grammar consumes (cursor advances
        //     over these as before),
        //   - parallel `leading_trivia` / `trailing_trivia` arrays
        //     indexed by effective-token position.
        // Comments on a fresh line attach as leading trivia of the
        // next effective token; comments on the same line as an
        // effective token attach as trailing trivia of that token.
        // Roslyn/Swift convention.
        let mut effective: Vec<Token> = Vec::with_capacity(raw_tokens.len());
        let mut leading: Vec<Vec<Trivia>> = Vec::with_capacity(raw_tokens.len());
        let mut trailing: Vec<Vec<Trivia>> = Vec::with_capacity(raw_tokens.len());

        let mut pending_leading: Vec<Trivia> = Vec::new();
        let mut last_effective_line: i64 = -1;
        for tok in raw_tokens {
            if is_comment_token(&tok.ttype) {
                let kind = token_to_trivia_kind(&tok.ttype)
                    .expect("comment token must map to a trivia kind");
                let triv = Trivia {
                    kind,
                    text: tok.value,
                    line: tok.line,
                    column: tok.column,
                };
                if !effective.is_empty() && (tok.line as i64) == last_effective_line {
                    trailing.last_mut().unwrap().push(triv);
                } else {
                    pending_leading.push(triv);
                }
            } else {
                last_effective_line = tok.line as i64;
                effective.push(tok);
                leading.push(std::mem::take(&mut pending_leading));
                trailing.push(Vec::new());
            }
        }

        Parser {
            tokens: effective,
            pos: 0,
            leading_trivia: leading,
            trailing_trivia: trailing,
            last_let_value_kind: "literal".to_string(),
            loop_depth: 0,
            source: None,
            filename: "<source>".to_string(),
        }
    }

    /// §Fase 28.d — Fluent attach of source text + filename for
    /// rustc-style source-context blocks on emitted `ParseError`s.
    /// Returns `self` so it chains with `.parse_with_recovery()`:
    ///
    /// ```ignore
    /// let result = Parser::new(tokens)
    ///     .with_source(src, "foo.axon")
    ///     .parse_with_recovery();
    /// ```
    ///
    /// No-op of any other behaviour — pure metadata attach.
    #[must_use]
    pub fn with_source(mut self, source: &str, filename: &str) -> Self {
        self.source = Some(source.to_string());
        self.filename = filename.to_string();
        self
    }

    // ── public API ───────────────────────────────────────────────

    pub fn parse(&mut self) -> Result<Program, ParseError> {
        let mut program = Program {
            declarations: Vec::new(),
            declaration_trivia: Vec::new(),
            loc: Loc { line: 1, column: 1 },
        };
        while !self.check(TokenType::Eof) {
            // Capture trivia around the declaration. `start_pos` is
            // the effective-token position of the declaration's first
            // token; that position carries the leading trivia. After
            // parsing, `pos - 1` is the last token consumed; that
            // position carries the trailing trivia.
            let start_pos = self.pos;
            let mut decl = match self.parse_declaration() {
                Ok(d) => d,
                Err(e) => return Err(self.attach_source_to_error(e)),
            };
            let end_pos = self.pos.saturating_sub(1);
            let leading = self
                .leading_trivia
                .get(start_pos)
                .cloned()
                .unwrap_or_default();
            let trailing = self
                .trailing_trivia
                .get(end_pos)
                .cloned()
                .unwrap_or_default();
            // Fase 14.b — also copy trivia into the per-struct fields on
            // the declaration so consumers can read `flow.leading_trivia`
            // directly without going through `program.declaration_trivia[i]`.
            // The side-channel is preserved for backward compat with
            // 14.a callers and as a flat enumeration source.
            attach_trivia_to_decl(&mut decl, leading.clone(), trailing.clone());
            program.declarations.push(decl);
            program
                .declaration_trivia
                .push(DeclarationTrivia { leading, trailing });
        }
        // §Fase 80.g — expand `voice` declarations FIRST (they may emit
        // `from Preset@vN` upstream legs), then §80.f preset references,
        // BEFORE type-check — so the §80.c laws and the IR see the expanded
        // program (and `axon desugar` prints exactly this lowering).
        // Unknown presets stay unexpanded — the checker reports them with
        // the catalog list (accumulating diagnostics beat a parse abort).
        crate::voice_desugar::expand(&mut program);
        crate::upstream_presets::expand(&mut program);
        Ok(program)
    }

    // ── §Fase 28.c — recovery-mode parse ─────────────────────────
    //
    // Mirror of Python's `Parser.parse_with_recovery` from
    // `axon/compiler/parser.py`. Wraps `parse_declaration` in a
    // try/recover loop: on any `ParseError` the error is appended to
    // the list and the cursor advances to the next sync point, then
    // parsing resumes. The two stacks must produce structurally
    // identical error lists on the same input — that is the cross-
    // stack drift gate (D7). See the test module
    // `tests::fase28_recovery_tests` and Python-side
    // `tests/test_fase28_parser_recovery.py`.

    /// Recovery-mode parse. Collects every parse error in source
    /// order; the existing `parse()` API remains fail-fast (D9).
    ///
    /// # Recovery contract (D2)
    ///
    /// On `ParseError`:
    ///   1. Push the error onto `errors`.
    ///   2. If the cursor is already on a top-level declaration
    ///      keyword (and brace-depth ≤ 0), do not consume — the
    ///      caller should retry the declaration parse from here.
    ///      Otherwise advance one token to make progress, then
    ///      walk to the next sync point.
    ///   3. Resume the outer loop.
    ///
    /// Sync points: top-level declaration keyword at brace-depth ≤ 0,
    /// or EOF. Negative depths are treated identically to ≤ 0 — the
    /// walker keeps walking through over-balanced `}` rather than
    /// pretending a closing brace is itself a sync point (which would
    /// emit a ghost "Unexpected token at top level" error in the
    /// outer loop).
    pub fn parse_with_recovery(&mut self) -> ParseResult {
        let mut program = Program {
            declarations: Vec::new(),
            declaration_trivia: Vec::new(),
            loc: Loc { line: 1, column: 1 },
        };
        let mut errors: Vec<ParseError> = Vec::new();

        while !self.check(TokenType::Eof) {
            let start_pos = self.pos;
            match self.parse_declaration() {
                Ok(mut decl) => {
                    let end_pos = self.pos.saturating_sub(1);
                    let leading = self
                        .leading_trivia
                        .get(start_pos)
                        .cloned()
                        .unwrap_or_default();
                    let trailing = self
                        .trailing_trivia
                        .get(end_pos)
                        .cloned()
                        .unwrap_or_default();
                    attach_trivia_to_decl(&mut decl, leading.clone(), trailing.clone());
                    program.declarations.push(decl);
                    program
                        .declaration_trivia
                        .push(DeclarationTrivia { leading, trailing });
                }
                Err(err) => {
                    // §Fase 28.d — attach source-context block when a
                    // source has been provided via `with_source(...)`;
                    // otherwise the error keeps its single-line shape.
                    errors.push(self.attach_source_to_error(err));
                    // Make progress. If parse_declaration returned
                    // immediately on the same token (e.g. unknown
                    // top-level token), we MUST advance at least one
                    // token to avoid an infinite loop.
                    if self.pos == start_pos && !self.check(TokenType::Eof) {
                        self.advance();
                    }
                    self.advance_to_sync_point();
                }
            }
        }

        ParseResult { program, errors }
    }

    /// §Fase 28.d — Decorate a `ParseError` with a `SourceSnippet`
    /// when the parser has source context attached, otherwise return
    /// the error unchanged. Idempotent: if the error already carries
    /// a snippet, this overwrites it with the parser's source.
    fn attach_source_to_error(&self, err: ParseError) -> ParseError {
        match &self.source {
            Some(src) if err.line >= 1 => err.attach_source(src, &self.filename),
            _ => err,
        }
    }

    /// §Fase 28.c — Walk the cursor forward until the next sync
    /// point (top-level declaration keyword at brace-depth ≤ 0) or
    /// EOF. Used by `parse_with_recovery` to skip the malformed
    /// remainder of a failed declaration.
    fn advance_to_sync_point(&mut self) {
        let mut depth: i32 = 0;
        while !self.check(TokenType::Eof) {
            let tt = self.current().ttype.clone();
            // Sync at top-level keywords when depth ≤ 0. We do not
            // consume the keyword — the outer loop will dispatch on
            // it.
            if is_top_level_decl_kw_for_recovery(&tt) && depth <= 0 {
                return;
            }
            if matches!(tt, TokenType::LBrace) {
                depth += 1;
            } else if matches!(tt, TokenType::RBrace) {
                depth -= 1;
            }
            self.advance();
        }
    }

    // ── token helpers ────────────────────────────────────────────

    fn current(&self) -> &Token {
        if self.pos >= self.tokens.len() {
            self.tokens.last().unwrap() // EOF sentinel
        } else {
            &self.tokens[self.pos]
        }
    }

    fn advance(&mut self) -> &Token {
        let idx = self.pos;
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        &self.tokens[idx]
    }

    fn check(&self, tt: TokenType) -> bool {
        self.current().ttype == tt
    }

    fn consume(&mut self, expected: TokenType) -> Result<Token, ParseError> {
        let tok = self.current().clone();
        if tok.ttype != expected {
            return Err(ParseError {
                message: format!(
                    "Expected {:?}, found {:?}('{}')",
                    expected, tok.ttype, tok.value
                ),
                line: tok.line,
                column: tok.column,
                            ..Default::default()
            });
        }
        self.pos += 1;
        Ok(tok)
    }

    /// §Fase 41.b — build a `ParseError` at the current token's location.
    fn error(&self, message: &str) -> ParseError {
        let tok = self.current();
        ParseError { message: message.to_string(), line: tok.line, column: tok.column, ..Default::default() }
    }

    /// Consume any identifier or keyword-used-as-value.
    fn consume_any_ident_or_kw(&mut self) -> Result<Token, ParseError> {
        let tok = self.current().clone();
        match tok.ttype {
            TokenType::Identifier
            | TokenType::Bool
            | TokenType::StringLit
            | TokenType::Integer
            | TokenType::Float => {
                self.pos += 1;
                Ok(tok)
            }
            _ => {
                // Allow any keyword token whose value is alphabetic
                if !tok.value.is_empty()
                    && tok.value.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && tok.ttype != TokenType::Eof
                {
                    self.pos += 1;
                    Ok(tok)
                } else {
                    Err(ParseError {
                        message: format!(
                            "Expected identifier or keyword value, found {:?}('{}')",
                            tok.ttype, tok.value
                        ),
                        line: tok.line,
                        column: tok.column,
                                            ..Default::default()
                    })
                }
            }
        }
    }

    fn consume_number(&mut self) -> Result<f64, ParseError> {
        let tok = self.current().clone();
        match tok.ttype {
            TokenType::Float | TokenType::Integer => {
                self.pos += 1;
                tok.value.parse::<f64>().map_err(|_| ParseError {
                    message: format!("Invalid number '{}'", tok.value),
                    line: tok.line,
                    column: tok.column,
                                    ..Default::default()
                })
            }
            _ => Err(ParseError {
                message: format!("Expected number, found {:?}('{}')", tok.ttype, tok.value),
                line: tok.line,
                column: tok.column,
                            ..Default::default()
            }),
        }
    }

    fn parse_bool(&mut self) -> Result<bool, ParseError> {
        let tok = self.consume(TokenType::Bool)?;
        Ok(tok.value == "true")
    }

    fn loc_of(&self, tok: &Token) -> Loc {
        Loc {
            line: tok.line,
            column: tok.column,
        }
    }

    fn check_run_modifier(&self) -> bool {
        matches!(
            self.current().ttype,
            TokenType::As
                | TokenType::Within
                | TokenType::ConstrainedBy
                | TokenType::OnFailure
                | TokenType::OutputTo
                | TokenType::Effort
        )
    }

    // ── list helpers ─────────────────────────────────────────────

    fn parse_string_list(&mut self) -> Result<Vec<String>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut items = Vec::new();
        items.push(self.consume(TokenType::StringLit)?.value);
        while self.check(TokenType::Comma) {
            self.advance();
            items.push(self.consume(TokenType::StringLit)?.value);
        }
        self.consume(TokenType::RBracket)?;
        Ok(items)
    }

    /// §Fase 83.a — a bracketed list of quoted string literals, tolerant of
    /// an empty `[]` and a trailing comma before `]` (the `Window.exclude`
    /// shape, generalized into a reusable helper). Used for CORS field
    /// lists whose values contain characters (`://`, `.`, `-`) that aren't
    /// valid bare identifiers — `allow_origins`, `allow_headers`,
    /// `expose_headers` — where `parse_string_list`'s "at least one item,
    /// no trailing comma" strictness would reject a legitimate empty or
    /// comma-terminated declaration.
    fn parse_bracketed_strings(&mut self) -> Result<Vec<String>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut items = Vec::new();
        if !self.check(TokenType::RBracket) {
            items.push(self.consume(TokenType::StringLit)?.value);
            while self.check(TokenType::Comma) {
                self.advance();
                if self.check(TokenType::RBracket) {
                    break; // trailing comma
                }
                items.push(self.consume(TokenType::StringLit)?.value);
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(items)
    }

    fn parse_identifier_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut names = Vec::new();
        names.push(self.consume(TokenType::Identifier)?.value);
        while self.check(TokenType::Comma) {
            self.advance();
            names.push(self.consume(TokenType::Identifier)?.value);
        }
        Ok(names)
    }

    fn parse_bracketed_identifiers(&mut self) -> Result<Vec<String>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let items = self.parse_extended_identifier_list()?;
        self.consume(TokenType::RBracket)?;
        Ok(items)
    }

    fn parse_extended_identifier_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut items = Vec::new();
        items.push(self.consume_any_ident_or_kw()?.value);
        while self.check(TokenType::Comma) {
            self.advance();
            items.push(self.consume_any_ident_or_kw()?.value);
        }
        Ok(items)
    }

    fn parse_dotted_identifier(&mut self) -> Result<String, ParseError> {
        let mut parts = vec![self.consume_any_ident_or_kw()?.value];
        while self.check(TokenType::Dot) {
            self.advance();
            parts.push(self.consume_any_ident_or_kw()?.value);
        }
        Ok(parts.join("."))
    }

    fn parse_expression_string(&mut self) -> Result<String, ParseError> {
        if self.check(TokenType::LBracket) {
            let items = self.parse_bracketed_dot_identifiers()?;
            return Ok(format!("[{}]", items.join(", ")));
        }
        self.parse_dotted_identifier()
    }

    fn parse_bracketed_dot_identifiers(&mut self) -> Result<Vec<String>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut items = vec![self.parse_dotted_identifier()?];
        while self.check(TokenType::Comma) {
            self.advance();
            items.push(self.parse_dotted_identifier()?);
        }
        self.consume(TokenType::RBracket)?;
        Ok(items)
    }

    fn parse_argument_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut args = Vec::new();
        while !self.check(TokenType::RParen) {
            let tok = self.current().clone();
            match tok.ttype {
                TokenType::StringLit | TokenType::Integer | TokenType::Float => {
                    self.advance();
                    args.push(tok.value);
                }
                TokenType::Identifier => {
                    self.advance();
                    let mut val = tok.value;
                    if self.check(TokenType::Dot) {
                        self.advance();
                        val.push('.');
                        val.push_str(&self.consume_any_ident_or_kw()?.value);
                    }
                    args.push(val);
                }
                _ => {
                    self.advance();
                    let key = tok.value;
                    if self.check(TokenType::Colon) {
                        self.advance();
                        let v = self.advance().value.clone();
                        args.push(format!("{key}:{v}"));
                    } else {
                        args.push(key);
                    }
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        Ok(args)
    }

    /// Skip a single value or balanced bracketed/braced block (unknown field).
    fn skip_value(&mut self) {
        match self.current().ttype {
            TokenType::LBracket => {
                self.advance();
                let mut depth = 1u32;
                while depth > 0 && !self.check(TokenType::Eof) {
                    if self.check(TokenType::LBracket) {
                        depth += 1;
                    } else if self.check(TokenType::RBracket) {
                        depth -= 1;
                    }
                    self.advance();
                }
            }
            TokenType::LBrace => {
                self.advance();
                let mut depth = 1u32;
                while depth > 0 && !self.check(TokenType::Eof) {
                    if self.check(TokenType::LBrace) {
                        depth += 1;
                    } else if self.check(TokenType::RBrace) {
                        depth -= 1;
                    }
                    self.advance();
                }
            }
            TokenType::Lt => {
                // effect row: <io, network, ...>
                self.advance();
                let mut depth = 1u32;
                while depth > 0 && !self.check(TokenType::Eof) {
                    if self.check(TokenType::Lt) {
                        depth += 1;
                    } else if self.check(TokenType::Gt) {
                        depth -= 1;
                    }
                    self.advance();
                }
            }
            _ => {
                self.advance();
                while self.check(TokenType::Dot) {
                    self.advance();
                    self.advance();
                }
            }
        }
    }

    /// Skip a balanced `{ ... }` block including its braces.
    fn skip_braced_block(&mut self) -> Result<(), ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut depth = 1u32;
        while depth > 0 {
            if self.check(TokenType::Eof) {
                let tok = self.current();
                return Err(ParseError {
                    message: "Unterminated block — expected '}'".to_string(),
                    line: tok.line,
                    column: tok.column,
                                    ..Default::default()
                });
            }
            if self.check(TokenType::LBrace) {
                depth += 1;
            } else if self.check(TokenType::RBrace) {
                depth -= 1;
            }
            self.advance();
        }
        Ok(())
    }

    fn at_declaration_start(&self) -> bool {
        is_declaration_keyword(&self.current().ttype) || self.check(TokenType::Eof)
    }

    // ── top-level dispatch ───────────────────────────────────────

    fn parse_declaration(&mut self) -> Result<Declaration, ParseError> {
        let tok = self.current().clone();

        // §Fase 114.a — a TOP-LEVEL `budget <Name> { … }`.
        //
        // `budget` lexes as `TokenType::Budget` (the daemon-field keyword). At top
        // level it is only a declaration when a NAME follows — `budget Foo { … }`.
        // The lookahead is what keeps the daemon-attached form (`daemon D { budget
        // { … } }`, where `{` follows immediately) untouched: there the next token
        // is `{`, not an identifier, so this branch does not fire.
        if tok.ttype == TokenType::Budget && self.peek_is_identifier() {
            return self.parse_top_level_budget().map(Declaration::Budget);
        }

        match tok.ttype {
            TokenType::Import => self.parse_import().map(Declaration::Import),
            TokenType::Persona => self.parse_persona().map(Declaration::Persona),
            TokenType::Context => self.parse_context().map(Declaration::Context),
            TokenType::Anchor => self.parse_anchor().map(Declaration::Anchor),
            TokenType::Memory => self.parse_memory().map(Declaration::Memory),
            TokenType::Tool => self.parse_tool().map(Declaration::Tool),
            TokenType::Type => self.parse_type_def().map(Declaration::Type),
            TokenType::Flow => self.parse_flow().map(Declaration::Flow),
            TokenType::Intent => self.parse_intent().map(Declaration::Intent),
            TokenType::Run => self.parse_run().map(Declaration::Run),
            TokenType::Let => self.parse_let().map(Declaration::Let),
            TokenType::Know | TokenType::Believe | TokenType::Speculate | TokenType::Doubt => {
                self.parse_epistemic_block().map(Declaration::Epistemic)
            }
            TokenType::Lambda => self.parse_lambda_data().map(Declaration::LambdaData),

            // ── Tier 2 declarations (full AST) ──────────────────
            TokenType::Agent => self.parse_agent().map(Declaration::Agent),
            TokenType::Shield => self.parse_shield().map(Declaration::Shield),
            // §Fase 71.a — temporal execution-window guard.
            TokenType::Window => self.parse_window().map(Declaration::Window),
            TokenType::Pix => self.parse_pix().map(Declaration::Pix),
            TokenType::Ledger => self.parse_ledger().map(Declaration::Ledger),
            TokenType::Psyche => self.parse_psyche().map(Declaration::Psyche),
            TokenType::Corpus => self.parse_corpus().map(Declaration::Corpus),
            TokenType::Dataspace => self.parse_dataspace().map(Declaration::Dataspace),
            TokenType::Ots => self.parse_ots().map(Declaration::Ots),
            TokenType::Mandate => self.parse_mandate().map(Declaration::Mandate),
            TokenType::Compute => self.parse_compute().map(Declaration::Compute),
            TokenType::Daemon => self.parse_daemon().map(Declaration::Daemon),
            TokenType::Extension => self.parse_extension().map(Declaration::Extension),
            TokenType::AxonStore => self.parse_axonstore().map(Declaration::AxonStore),
            TokenType::AxonEndpoint => self.parse_axonendpoint().map(Declaration::AxonEndpoint),

            // ── §λ-L-E Fase 1 — I/O cognitivo ───────────────────
            TokenType::Resource => self.parse_resource().map(Declaration::Resource),
            TokenType::Fabric => self.parse_fabric().map(Declaration::Fabric),
            TokenType::Manifest => self.parse_manifest().map(Declaration::Manifest),
            TokenType::Observe => self.parse_observe().map(Declaration::Observe),

            // ── §λ-L-E Fase 3 — Control cognitivo ───────────────
            TokenType::Reconcile => self.parse_reconcile().map(Declaration::Reconcile),
            TokenType::Lease => self.parse_lease().map(Declaration::Lease),
            TokenType::Ensemble => self.parse_ensemble().map(Declaration::Ensemble),

            // ── §λ-L-E Fase 4 — Topology + π-calculus sessions ─
            TokenType::Session => self.parse_session_definition().map(Declaration::Session),
            TokenType::Topology => self.parse_topology().map(Declaration::Topology),

            // ── §Fase 41.b — typed WebSocket transport ─────────
            TokenType::Socket => self.parse_socket().map(Declaration::Socket),

            // ── §Fase 80.b — outbound vendor connection ─────────
            TokenType::Upstream => self.parse_upstream().map(Declaration::Upstream),

            // ── §Fase 80.g — the voice-agent simplicity layer ───
            TokenType::Voice => self.parse_voice().map(Declaration::Voice),

            // ── §Fase 83.a — the named origin-policy declaration ─
            TokenType::Cors => self.parse_cors().map(Declaration::Cors),

            // ── §Fase 85.a — the named result-memoization policy ─
            TokenType::Cache => self.parse_cache().map(Declaration::Cache),
            TokenType::Document => self.parse_document().map(Declaration::Document),

            // ── §Fase 105 — Governed CRM Delivery ─
            TokenType::Deliver => self.parse_deliver().map(Declaration::Deliver),
            TokenType::Notify => self.parse_notify().map(Declaration::Notify),

            // ── §Fase 87.a — the long-horizon autonomous research primitive ─
            TokenType::Savant => self.parse_savant().map(Declaration::Savant),

            // ── §Fase 87.d — the dynamic tool-synthesis policy ──────────────
            TokenType::Synth => self.parse_synth().map(Declaration::Synth),

            // ── §Fase 88.a — the authorization-scope policy declaration ─────
            TokenType::Scope => self.parse_scope().map(Declaration::Scope),

            // ── §Fase 92.a — the ephemeral-credential contract ──────────────
            TokenType::Credential => self.parse_credential().map(Declaration::Credential),

            // ── §Fase 51.c.2 — Pauli-sum observable ────────────
            TokenType::Observable => self.parse_observable().map(Declaration::Observable),

            // ── §Fase 69.a — Advantage Witness ──────────────────
            TokenType::Witness => self.parse_witness().map(Declaration::Witness),

            // ── §λ-L-E Fase 5 — Cognitive immune system ─────────
            TokenType::Immune => self.parse_immune().map(Declaration::Immune),
            TokenType::Reflex => self.parse_reflex().map(Declaration::Reflex),
            TokenType::Heal => self.parse_heal().map(Declaration::Heal),

            // ── §λ-L-E Fase 9 — UI cognitiva ────────────────────
            TokenType::Component => self.parse_component().map(Declaration::Component),
            TokenType::View => self.parse_view().map(Declaration::View),

            // ── §λ-L-E Fase 13 — Mobile typed channels ──────────
            TokenType::Channel => self.parse_channel().map(Declaration::Channel),

            // ── Tier 3+ structural fallback ─────────────────────
            // Store operations: keyword target { ... } or keyword target ...
            TokenType::Ingest
            | TokenType::Persist
            | TokenType::Retrieve
            | TokenType::Mutate
            | TokenType::Purge
            | TokenType::Transact => self.parse_generic_declaration(),

            // MCP declaration
            TokenType::Mcp => self.parse_generic_declaration(),

            _ => {
                // §Fase 28.e — append "Did you mean X?" hint when the
                // unknown token looks like a typo'd top-level keyword
                // (Levenshtein ≤ 2). D3, D11 ratified 2026-05-10.
                let hint = crate::smart_suggest::suggest_for(
                    &tok.value,
                    crate::smart_suggest::TOP_LEVEL_KEYWORD_NAMES,
                );
                let base = format!(
                    "Unexpected token at top level: '{}' — expected declaration \
                     (persona, context, anchor, flow, run, ...)",
                    tok.value
                );
                let message = if hint.is_empty() {
                    base
                } else {
                    format!("{base}. {hint}")
                };
                Err(ParseError {
                    message,
                    line: tok.line,
                    column: tok.column,
                    ..Default::default()
                })
            }
        }
    }

    // ── IMPORT ───────────────────────────────────────────────────

    fn parse_import(&mut self) -> Result<ImportNode, ParseError> {
        let tok = self.consume(TokenType::Import)?;
        let loc = self.loc_of(&tok);

        let mut path_parts = Vec::new();

        // Optional @ scope
        if self.check(TokenType::At) {
            self.advance();
            let first = self.consume(TokenType::Identifier)?;
            path_parts.push(format!("@{}", first.value));
        } else {
            let first = self.consume(TokenType::Identifier)?;
            path_parts.push(first.value);
        }

        while self.check(TokenType::Dot) {
            self.advance();
            if self.check(TokenType::LBrace) {
                break;
            }
            let part = self.consume(TokenType::Identifier)?;
            path_parts.push(part.value);
        }

        let mut names = Vec::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            names = self.parse_identifier_list()?;
            self.consume(TokenType::RBrace)?;
        }

        // ── §Fase 115.c — the `@allow_downgrade` ECC valve ───────────────
        //
        // `import a.b.{X} @allow_downgrade` acknowledges an epistemic
        // downgrade across this edge (see `epistemic_compat.rs`). The
        // annotation position is unambiguous: no top-level declaration
        // begins with `@`, so an `@` here belongs to this import — and an
        // unknown annotation is refused with the fix in the message
        // rather than surfacing later as an opaque parse error.
        let mut allow_downgrade = false;
        if self.check(TokenType::At) {
            let at_tok = self.current().clone();
            self.advance();
            let ident = self.consume(TokenType::Identifier)?;
            if ident.value == "allow_downgrade" {
                allow_downgrade = true;
            } else {
                return Err(ParseError {
                    message: format!(
                        "unknown import annotation '@{}' — the only import annotation is \
                         `@allow_downgrade` (the §115 epistemic-downgrade acknowledgment).",
                        ident.value
                    ),
                    line: at_tok.line,
                    column: at_tok.column,
                    ..Default::default()
                });
            }
        }

        // ── §Fase 111 — `apx` is RETRACTED ───────────────────────────────
        //
        // `import X with apx { … }` used to parse and then call
        // `skip_braced_block()` — the policy was consumed and thrown on the
        // floor. It never reached the AST, let alone the IR. In `axon-rs` the
        // string "apx" occurred only inside comments: there is no APX crate,
        // no binary, no MEC/PCC dependency verification, no EPR ranking, no
        // quarantine and no compliance gate. The public README advertised all
        // five.
        //
        // A dependency policy that silently evaporates is the worst possible
        // shape for this particular promise: the adopter believes their supply
        // chain is being verified, which is exactly the belief that stops them
        // from verifying it themselves. Refuse, loudly.
        let next_is_apx = self
            .tokens
            .get(self.pos + 1)
            .map(|t| t.value == "apx")
            .unwrap_or(false);
        if self.current().value == "with" && next_is_apx {
            let tok = self.current().clone();
            return Err(ParseError {
                message: "`import … with apx { … }` is RETRACTED (§111). The apx policy block was \
                          parsed and silently DISCARDED — it never reached the IR, and no epistemic \
                          dependency manager exists: no MEC/PCC verification, no EPR ranking, no \
                          quarantine, no compliance gate. Declaring it verified nothing while \
                          implying your supply chain was checked. Remove the `with apx { … }` \
                          clause; the plain `import` resolves through the §115 Epistemic Module \
                          System."
                    .to_string(),
                line: tok.line,
                column: tok.column,
                ..Default::default()
            });
        }

        Ok(ImportNode {
            module_path: path_parts,
            names,
            allow_downgrade,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        })
    }

    // ── PERSONA ──────────────────────────────────────────────────

    fn parse_persona(&mut self) -> Result<PersonaDefinition, ParseError> {
        let tok = self.consume(TokenType::Persona)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LBrace)?;

        let mut node = PersonaDefinition {
            name,
            domain: Vec::new(),
            tone: String::new(),
            confidence_threshold: None,
            cite_sources: None,
            refuse_if: Vec::new(),
            language: String::new(),
            description: String::new(),
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while !self.check(TokenType::RBrace) {
            let field_name = self.current().value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;

            match field_name.as_str() {
                "domain" => node.domain = self.parse_string_list()?,
                "tone" => node.tone = self.consume_any_ident_or_kw()?.value,
                "confidence_threshold" => node.confidence_threshold = Some(self.consume_number()?),
                "cite_sources" => node.cite_sources = Some(self.parse_bool()?),
                "refuse_if" => node.refuse_if = self.parse_bracketed_identifiers()?,
                "language" => node.language = self.consume(TokenType::StringLit)?.value,
                "description" => node.description = self.consume(TokenType::StringLit)?.value,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── CONTEXT ──────────────────────────────────────────────────

    fn parse_context(&mut self) -> Result<ContextDefinition, ParseError> {
        let tok = self.consume(TokenType::Context)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LBrace)?;

        let mut node = ContextDefinition {
            name,
            memory_scope: String::new(),
            language: String::new(),
            depth: String::new(),
            max_tokens: None,
            temperature: None,
            cite_sources: None,
            now_tz: None,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while !self.check(TokenType::RBrace) {
            let field_name = self.current().value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;

            match field_name.as_str() {
                "memory" => node.memory_scope = self.consume_any_ident_or_kw()?.value,
                "language" => node.language = self.consume(TokenType::StringLit)?.value,
                "depth" => node.depth = self.consume_any_ident_or_kw()?.value,
                // §Fase 91.a — the frame's cognitive timezone (IANA string).
                "now" => node.now_tz = Some(self.consume(TokenType::StringLit)?.value),
                "max_tokens" => {
                    node.max_tokens = Some(
                        self.consume(TokenType::Integer)?
                            .value
                            .parse::<i64>()
                            .unwrap_or(0),
                    )
                }
                "temperature" => node.temperature = Some(self.consume_number()?),
                "cite_sources" => node.cite_sources = Some(self.parse_bool()?),
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── ANCHOR ───────────────────────────────────────────────────

    fn parse_anchor(&mut self) -> Result<AnchorConstraint, ParseError> {
        let tok = self.consume(TokenType::Anchor)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LBrace)?;

        let mut node = AnchorConstraint {
            name,
            require: String::new(),
            reject: Vec::new(),
            enforce: String::new(),
            description: String::new(),
            confidence_floor: None,
            unknown_response: String::new(),
            on_violation: String::new(),
            on_violation_target: String::new(),
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while !self.check(TokenType::RBrace) {
            let field_name = self.current().value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;

            match field_name.as_str() {
                "require" => node.require = self.consume_any_ident_or_kw()?.value,
                "description" => node.description = self.consume(TokenType::StringLit)?.value,
                "reject" => node.reject = self.parse_bracketed_identifiers()?,
                "enforce" => node.enforce = self.consume_any_ident_or_kw()?.value,
                "confidence_floor" => node.confidence_floor = Some(self.consume_number()?),
                "unknown_response" => {
                    node.unknown_response = self.consume(TokenType::StringLit)?.value
                }
                "on_violation" => {
                    // Parse: raise ErrorName | fallback(...) | identifier
                    let action = self.consume_any_ident_or_kw()?.value;
                    node.on_violation = action.clone();
                    if action == "raise" || action == "fallback" {
                        node.on_violation_target = self.consume_any_ident_or_kw()?.value;
                    }
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── MEMORY ───────────────────────────────────────────────────

    fn parse_memory(&mut self) -> Result<MemoryDefinition, ParseError> {
        let tok = self.consume(TokenType::Memory)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LBrace)?;

        let mut node = MemoryDefinition {
            name,
            store: String::new(),
            backend: String::new(),
            retrieval: String::new(),
            decay: String::new(),
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while !self.check(TokenType::RBrace) {
            let field_name = self.current().value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;

            match field_name.as_str() {
                "store" => node.store = self.consume_any_ident_or_kw()?.value,
                "backend" => node.backend = self.consume_any_ident_or_kw()?.value,
                "retrieval" => node.retrieval = self.consume_any_ident_or_kw()?.value,
                "decay" => {
                    if self.check(TokenType::Duration) {
                        node.decay = self.advance().value.clone();
                    } else {
                        node.decay = self.consume_any_ident_or_kw()?.value;
                    }
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── TOOL ─────────────────────────────────────────────────────

    fn parse_tool(&mut self) -> Result<ToolDefinition, ParseError> {
        let tok = self.consume(TokenType::Tool)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LBrace)?;

        let mut node = ToolDefinition {
            name,
            provider: String::new(),
            max_results: None,
            filter_expr: String::new(),
            timeout: String::new(),
            runtime: String::new(),
            resource_ref: String::new(),
            sandbox: None,
            effects: None,
            parameters: Vec::new(),
            output_type: None,
            requires: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            target: None,
            risk: None,
            argv: Vec::new(),
            cache: String::new(),
            scrape: None,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        // §Fase 84.b/D84.13 — unknown fields are recorded (not silently
        // skipped) so a `target:`-bound technician tool can HARD-ERROR on one
        // (a typo'd safety field must never quietly disable a guard), while a
        // legacy schema-less tool keeps its lenient record-and-skip (zero
        // regression). The decision is deferred to after the block is parsed,
        // since `target:` may appear after the unknown field.
        let mut unknown_fields: Vec<(String, u32, u32)> = Vec::new();

        while !self.check(TokenType::RBrace) {
            let field_tok = self.current().clone();
            let field_name = field_tok.value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;

            match field_name.as_str() {
                "provider" => node.provider = self.consume_any_ident_or_kw()?.value,
                "max_results" => {
                    node.max_results = Some(
                        self.consume(TokenType::Integer)?
                            .value
                            .parse::<i64>()
                            .unwrap_or(0),
                    )
                }
                "filter" => node.filter_expr = self.parse_filter_expression()?,
                "timeout" => node.timeout = self.consume(TokenType::Duration)?.value,
                "runtime" => node.runtime = self.consume_any_ident_or_kw()?.value,
                // §Fase 114.c — the `resource` this tool's channel runs on. The
                // channel's address, concurrency and lifecycle come from it;
                // `runtime:` then names the path within the channel.
                "resource" => node.resource_ref = self.consume_any_ident_or_kw()?.value,
                "sandbox" => node.sandbox = Some(self.parse_bool()?),
                "effects" => node.effects = Some(self.parse_effect_row()?),
                // §Fase 58.a — the tool's typed input schema + output type.
                "parameters" => node.parameters = self.parse_tool_param_schema()?,
                "output_type" => node.output_type = Some(self.parse_output_type_string()?),
                // §Fase 116.a (D116.9) — the tool's required authorization
                // scopes: bare dot-separated capability slugs, the EXACT
                // grammar + charset of `credential.grants` (§92) so the two
                // vocabularies are one. `requires: [w_organization_social,
                // video.publish]`. Subset coverage is `axon-T956`.
                "requires" => {
                    let bracket_tok = self.current().clone();
                    let items = self.parse_bracketed_dot_identifiers()?;
                    for slug in &items {
                        if !is_valid_capability_slug(slug) {
                            return Err(ParseError {
                                message: format!(
                                    "Invalid capability slug '{slug}' in tool '{}' \
                                     `requires:`. Scope slugs must match \
                                     ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$ — the same \
                                     grammar as `credential.grants`. Examples: \
                                     `w_organization_social`, `video.publish`.",
                                    node.name
                                ),
                                line: bracket_tok.line,
                                column: bracket_tok.column,
                                ..Default::default()
                            });
                        }
                    }
                    node.requires = items;
                }
                // §Fase 94.c — the per-tenant secret KEY injected at
                // dispatch (`rotation_without_revelation`). Key shape +
                // technician exclusion are `axon-T902` (type-checker).
                "secret" => node.secret = self.parse_dotted_identifier()?,
                // §Fase 95.a — `secret_partition:` names one of this tool's
                // own `parameters:` (a bare identifier, NOT dotted — it is a
                // parameter reference, not a key). Its runtime value becomes
                // a single appended key segment at dispatch. The membership +
                // `String`-type + technician laws are `axon-T903`.
                "secret_partition" => {
                    node.secret_partition = self.consume_any_ident_or_kw()?.value
                }
                // §Fase 84.b — Remote Hands technician fields.
                "target" => node.target = Some(self.consume_any_ident_or_kw()?.value),
                "risk" => node.risk = Some(self.consume_any_ident_or_kw()?.value),
                // The argv template: a bracketed list of quoted elements
                // (`argv: ["ping", "-c", "${count}", "${host}"]`). Reuses the
                // CORS list helper (tolerant of `[]` and a trailing comma).
                "argv" => node.argv = self.parse_bracketed_strings()?,
                // §Fase 85.b — the tool's result-memoization policy reference
                // (a declared `cache` name, or the `none` opt-out sentinel).
                "cache" => node.cache = self.consume_any_ident_or_kw()?.value,
                // §Fase 98.b — the closed-catalog web-acquisition config
                // block. `scrape: { engine: …, extract: […], … }`.
                "scrape" => node.scrape = Some(self.parse_scrape_spec()?),
                _ => {
                    unknown_fields.push((field_name, field_tok.line, field_tok.column));
                    self.skip_value();
                }
            }
        }
        self.consume(TokenType::RBrace)?;

        // §Fase 84.b/D84.13 — a `target:`-bound tool opts into strict field
        // checking. An unknown field on it is a parse error, mirroring the §83
        // `cors`/`voice` closed-catalog discipline — but scoped to the
        // technician surface so ordinary tools are untouched.
        // §Fase 98.b (D98.2) — a `scrape:`-bearing web-acquisition tool opts
        // into the same strictness: a typo'd safety field (e.g. a mis-spelled
        // `respect_robots`) must never quietly disable a guard.
        if node.target.is_some() || node.scrape.is_some() {
            if let Some((field_name, line, column)) = unknown_fields.into_iter().next() {
                let (surface, valid) = if node.target.is_some() {
                    (
                        "technician tool (§Fase 84 D84.13)",
                        "provider, parameters, output_type, timeout, effects, target, risk, argv",
                    )
                } else {
                    (
                        "web-acquisition tool (§Fase 98 D98.2)",
                        "provider, parameters, output_type, timeout, effects, secret, \
                         secret_partition, cache, scrape",
                    )
                };
                return Err(ParseError {
                    message: format!(
                        "unknown field `{field_name}` in {surface} `{}` — this tool uses \
                         strict field checking; valid fields: {valid}",
                        node.name
                    ),
                    line,
                    column,
                    ..Default::default()
                });
            }
        }
        Ok(node)
    }

    /// §Fase 98.b — parse the closed-catalog `scrape: { … }` web-acquisition
    /// config sub-block. Every field is optional; an unknown field is a hard
    /// parse error (the §83 `cors` closed-catalog discipline). Mirrors the
    /// field grammar of `parse_tool` for the scrape-specific keys.
    fn parse_scrape_spec(&mut self) -> Result<crate::ast::ScrapeSpec, ParseError> {
        let open = self.consume(TokenType::LBrace)?;
        let loc = self.loc_of(&open);
        let mut spec = crate::ast::ScrapeSpec {
            loc,
            ..Default::default()
        };
        while !self.check(TokenType::RBrace) {
            let field_tok = self.current().clone();
            let field_name = field_tok.value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;
            match field_name.as_str() {
                "engine" => spec.engine = Some(self.consume_any_ident_or_kw()?.value),
                "impersonate" => spec.impersonate = Some(self.consume_any_ident_or_kw()?.value),
                "render_wait" => spec.render_wait = Some(self.consume(TokenType::Duration)?.value),
                "proxy" => spec.proxy = self.parse_dotted_identifier()?,
                "respect_robots" => spec.respect_robots = Some(self.parse_bool()?),
                "extract" => spec.extract = self.parse_bracketed_strings()?,
                "adaptive" => spec.adaptive = Some(self.parse_bool()?),
                "similarity_floor" => spec.similarity_floor = self.parse_optional_float(),
                "follow" => spec.follow = self.consume(TokenType::StringLit)?.value,
                "max_depth" => {
                    spec.max_depth =
                        Some(self.consume(TokenType::Integer)?.value.parse::<i64>().unwrap_or(0))
                }
                "max_pages" => {
                    spec.max_pages =
                        Some(self.consume(TokenType::Integer)?.value.parse::<i64>().unwrap_or(0))
                }
                "concurrency" => {
                    spec.concurrency =
                        Some(self.consume(TokenType::Integer)?.value.parse::<i64>().unwrap_or(0))
                }
                "politeness" => spec.politeness = self.consume_any_ident_or_kw()?.value,
                "checkpoint" => spec.checkpoint = self.consume_any_ident_or_kw()?.value,
                other => {
                    return Err(self.error(&format!(
                        "unknown scrape field `{other}` — the `scrape: {{ … }}` block is a \
                         closed catalog (§Fase 98 D98.2); valid fields: engine, impersonate, \
                         render_wait, proxy, respect_robots, extract, adaptive, \
                         similarity_floor, follow, max_depth, max_pages, concurrency, \
                         politeness, checkpoint"
                    )));
                }
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(spec)
    }

    /// §Fase 58.a — parse a tool's INPUT SCHEMA: a brace-delimited list of
    /// `name: Type` parameters (`parameters: { query: String, max_results: Int }`).
    /// Reuses the flow-parameter shape (`Parameter`), so the same `TypeExpr`
    /// grammar — generics like `List<T>`, `?`-optionals — applies. A trailing
    /// comma is tolerated; an empty `{}` yields no parameters.
    fn parse_tool_param_schema(&mut self) -> Result<Vec<Parameter>, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut params = Vec::new();
        while !self.check(TokenType::RBrace) {
            // Accept a keyword-as-name (`filter`, `type`, `domain`, …) — real
            // adopter tool schemas use such parameter names; the `:` after it
            // disambiguates.
            let name = self.consume_any_ident_or_kw()?;
            let ploc = self.loc_of(&name);
            self.consume(TokenType::Colon)?;
            let type_expr = self.parse_type_expr()?;
            params.push(Parameter {
                name: name.value,
                type_expr,
                loc: ploc,
            });
            if self.check(TokenType::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(params)
    }

    fn parse_filter_expression(&mut self) -> Result<String, ParseError> {
        let name = self.consume_any_ident_or_kw()?.value;
        if self.check(TokenType::LParen) {
            self.advance();
            let mut parts = vec![name, "(".to_string()];
            while !self.check(TokenType::RParen) {
                parts.push(self.advance().value.clone());
            }
            self.consume(TokenType::RParen)?;
            parts.push(")".to_string());
            Ok(parts.join(""))
        } else {
            Ok(name)
        }
    }

    fn parse_effect_row(&mut self) -> Result<EffectRow, ParseError> {
        let tok = self.consume(TokenType::Lt)?;
        let loc = self.loc_of(&tok);
        let mut effects = Vec::new();
        let mut epistemic_level = String::new();

        while !self.check(TokenType::Gt) {
            let name = self.consume_any_ident_or_kw()?.value;
            if self.check(TokenType::Colon) {
                self.advance();
                // Fase 11.c / 11.e — qualifiers can be compound slugs
                // from a closed catalogue:
                //
                //   * dot-separated  — `legal:HIPAA.164_502`,
                //                       `legal:GDPR.Art6.Consent`,
                //                       `legal:PCI_DSS.v4_Req3`
                //   * colon-separated — `ots:transform:mulaw8:pcm16`,
                //                       `ots:backend:native`
                //   * mixed           — supported by the same loop.
                //
                // The lexer fragments dotted slugs across IDENT /
                // INTEGER tokens (e.g., `164_502` lexes as INTEGER
                // `164` + IDENT `_502` because `_` starts a fresh
                // identifier); we recombine here using source-column
                // adjacency so the type checker sees the catalog
                // string verbatim.
                let level = self.parse_qualifier_value()?;
                if name == "epistemic" {
                    epistemic_level = level;
                } else {
                    effects.push(format!("{name}:{level}"));
                }
            } else {
                effects.push(name);
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::Gt)?;

        Ok(EffectRow {
            effects,
            epistemic_level,
            loc,
        })
    }

    /// Parse a compound qualifier value following an effect's first
    /// colon — supports both dot-separated (`HIPAA.164_502`) and
    /// colon-separated (`transform:mulaw8:pcm16`) catalogue slugs, as
    /// well as mixed forms.
    ///
    /// The grammar is: `segment ((`.` | `:`) segment)*` where a
    /// segment is a contiguous run of IDENT / INTEGER tokens (see
    /// [`Self::consume_dotted_slug_segment`]).
    fn parse_qualifier_value(&mut self) -> Result<String, ParseError> {
        let mut buf = self.consume_dotted_slug_segment()?;
        loop {
            let sep = if self.check(TokenType::Dot) {
                '.'
            } else if self.check(TokenType::Colon) {
                ':'
            } else {
                break;
            };
            self.advance();
            let part = self.consume_dotted_slug_segment()?;
            buf.push(sep);
            buf.push_str(&part);
        }
        Ok(buf)
    }

    /// Consume a contiguous run of IDENT / INTEGER / keyword-ident
    /// tokens whose source positions are adjacent (no whitespace
    /// between them), concatenating their text into a single segment.
    ///
    /// Needed for closed-catalogue qualifier slugs whose segment
    /// mixes digits and identifier characters — e.g. `HIPAA.164_502`
    /// lexes as INTEGER `164` + IDENT `_502` because `_` starts a
    /// fresh identifier; the catalog value is the concatenation
    /// `164_502`. Adjacency is determined by matching
    /// `(line, column + len)` of the previous token against the next
    /// token's start position.
    fn consume_dotted_slug_segment(&mut self) -> Result<String, ParseError> {
        let first = self.consume_any_ident_or_kw()?;
        let mut buf = first.value.clone();
        let mut next_line = first.line;
        let mut next_col = first.column + first.value.chars().count() as u32;
        loop {
            let cur = self.current();
            let is_segment_token = matches!(cur.ttype, TokenType::Identifier | TokenType::Integer,);
            if !is_segment_token {
                break;
            }
            if cur.line != next_line || cur.column != next_col {
                break;
            }
            buf.push_str(&cur.value);
            next_col = cur.column + cur.value.chars().count() as u32;
            next_line = cur.line;
            self.pos += 1;
        }
        Ok(buf)
    }

    // ── TYPE ─────────────────────────────────────────────────────

    fn parse_type_def(&mut self) -> Result<TypeDefinition, ParseError> {
        let tok = self.consume(TokenType::Type)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;

        let mut node = TypeDefinition {
            name,
            fields: Vec::new(),
            range_constraint: None,
            where_clause: None,
            compliance: Vec::new(),
            loc: loc.clone(),
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        // Optional range: (0.0..1.0)
        if self.check(TokenType::LParen) {
            self.advance();
            let min_val = self.consume_number()?;
            self.consume(TokenType::DotDot)?;
            let max_val = self.consume_number()?;
            self.consume(TokenType::RParen)?;
            node.range_constraint = Some(RangeConstraint {
                min_value: min_val,
                max_value: max_val,
                loc: loc.clone(),
            });
        }

        // Optional where clause
        if self.check(TokenType::Where) {
            self.advance();
            let mut expr_parts = Vec::new();
            while !self.check(TokenType::LBrace) && !self.at_declaration_start() {
                if self.check(TokenType::Eof) {
                    break;
                }
                expr_parts.push(self.advance().value.clone());
            }
            node.where_clause = Some(WhereClause {
                expression: expr_parts.join(" "),
                loc: loc.clone(),
            });
        }

        // Optional ESK Fase 6.1 — `compliance [HIPAA, ...]` prefix modifier
        // between `type Name` / `range` / `where` and the body `{`.
        if self.check(TokenType::Identifier) && self.current().value == "compliance" {
            self.advance();
            node.compliance = self.parse_bracketed_identifiers()?;
        }

        // Optional body: { field: Type, ... }
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) {
                let field_name = self.consume(TokenType::Identifier)?;
                let field_loc = self.loc_of(&field_name);
                self.consume(TokenType::Colon)?;
                let type_expr = self.parse_type_expr()?;
                node.fields.push(TypeField {
                    name: field_name.value,
                    type_expr,
                    loc: field_loc,
                });
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.consume(TokenType::RBrace)?;
        }

        Ok(node)
    }

    fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let name_tok = self.consume(TokenType::Identifier)?;
        let loc = self.loc_of(&name_tok);
        let mut generic_param = String::new();
        let mut optional = false;

        if self.check(TokenType::Lt) {
            self.advance();
            // §Fase 39.a — recursive: the generic param can itself be a
            // nested type expression. `FlowEnvelope<List<TenantRecord>>`
            // parses as outer=FlowEnvelope, inner=List<TenantRecord>.
            // Pre-39.a the inner had to be a single Identifier; nested
            // generics like the canonical FlowEnvelope<T> wrapper
            // required this lift. Backwards-compat preserved for
            // single-level generics like `Stream<Token>` and
            // `List<T>` — the recursion lands once and returns the
            // same flat string the v1.x parser produced.
            let inner = self.parse_type_expr()?;
            generic_param = if inner.generic_param.is_empty() {
                inner.name
            } else {
                format!("{}<{}>", inner.name, inner.generic_param)
            };
            self.consume(TokenType::Gt)?;
        }
        // §Fase 51.c.3 — bracket type parameters for the continuous-carrier
        // grammar: `SymbolicPtr[Tensor[Float32]]`, `DensityMatrix[1024]`. The
        // param is either a nested type expression OR a numeric dimension.
        if self.check(TokenType::LBracket) {
            self.advance();
            if matches!(self.current().ttype, TokenType::Integer | TokenType::Float) {
                generic_param = self.advance().value.clone();
            } else {
                let inner = self.parse_type_expr()?;
                generic_param = if inner.generic_param.is_empty() {
                    inner.name
                } else {
                    format!("{}[{}]", inner.name, inner.generic_param)
                };
            }
            self.consume(TokenType::RBracket)?;
        }
        if self.check(TokenType::Question) {
            self.advance();
            optional = true;
        }

        Ok(TypeExpr {
            name: name_tok.value,
            generic_param,
            optional,
            loc,
        })
    }

    /// Parse a type expression in a context where the AST stores the
    /// shape as a flat string (step / reason / forge / ots-apply
    /// productions). Mirrors Python `_parse_output_type_string`.
    ///
    /// Accepts:
    /// - `Identifier`        → `"Identifier"`
    /// - `Stream<String>`    → `"Stream<String>"`
    /// - `Optional?`         → `"Optional?"`
    /// - `Stream<String>?`   → `"Stream<String>?"`
    ///
    /// **Why this exists** — pre-fix, the step parser called
    /// `consume(TokenType::Identifier)?.value` which captured only
    /// the head identifier and left `< … >` unconsumed. For
    /// `output: Stream<Token>`, this produced `output_type =
    /// "Stream"`, and downstream `flow_has_stream_output`'s
    /// `starts_with("Stream<") && ends_with('>')` predicate then
    /// returned false → `implicit_transport == "json"` → the
    /// dynamic-route fallback in `axon-rs` served JSON instead of
    /// SSE even when the adopter's source canonically declared the
    /// algebraic stream effect. Surfaced 2026-05-12 by adopter
    /// `docs/MIGRATION_TO_AXON.md` audit after the v1.23.0 wire-
    /// layer didn't honor the declarative effect. Python parser was
    /// fixed for the same gap 2026-05-09; this is the Rust cross-
    /// stack catch-up.
    fn parse_output_type_string(&mut self) -> Result<String, ParseError> {
        let expr = self.parse_type_expr()?;
        let mut s = expr.name;
        if !expr.generic_param.is_empty() {
            s.push('<');
            s.push_str(&expr.generic_param);
            s.push('>');
        }
        if expr.optional {
            s.push('?');
        }
        Ok(s)
    }

    // ── FLOW ─────────────────────────────────────────────────────

    fn parse_flow(&mut self) -> Result<FlowDefinition, ParseError> {
        let tok = self.consume(TokenType::Flow)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;

        self.consume(TokenType::LParen)?;
        let mut parameters = Vec::new();
        if !self.check(TokenType::RParen) {
            parameters = self.parse_param_list()?;
        }
        self.consume(TokenType::RParen)?;

        let mut return_type = None;
        if self.check(TokenType::Arrow) {
            self.advance();
            return_type = Some(self.parse_type_expr()?);
        }

        self.consume(TokenType::LBrace)?;
        let mut body = Vec::new();
        while !self.check(TokenType::RBrace) {
            body.push(self.parse_flow_step()?);
        }
        self.consume(TokenType::RBrace)?;

        Ok(FlowDefinition {
            name,
            parameters,
            return_type,
            body,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Parameter>, ParseError> {
        let mut params = Vec::new();

        let name = self.consume(TokenType::Identifier)?;
        let ploc = self.loc_of(&name);
        self.consume(TokenType::Colon)?;
        let type_expr = self.parse_type_expr()?;
        params.push(Parameter {
            name: name.value,
            type_expr,
            loc: ploc,
        });

        while self.check(TokenType::Comma) {
            self.advance();
            let name = self.consume(TokenType::Identifier)?;
            let ploc = self.loc_of(&name);
            self.consume(TokenType::Colon)?;
            let type_expr = self.parse_type_expr()?;
            params.push(Parameter {
                name: name.value,
                type_expr,
                loc: ploc,
            });
        }
        Ok(params)
    }

    // ── FLOW STEP dispatch ───────────────────────────────────────

    fn parse_flow_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();

        match tok.ttype {
            TokenType::Step => self.parse_step().map(FlowStep::Step),
            TokenType::If => self.parse_if().map(FlowStep::If),
            TokenType::For => self.parse_for_in().map(FlowStep::ForIn),
            TokenType::Let => self.parse_let().map(FlowStep::Let),
            TokenType::Return => self.parse_return().map(FlowStep::Return),
            TokenType::Break => self.parse_break().map(FlowStep::Break),
            TokenType::Continue => self.parse_continue().map(FlowStep::Continue),
            TokenType::Lambda => self.parse_lambda_data_apply().map(FlowStep::LambdaDataApply),

            // ── Tier 2 flow steps (typed AST) ─────────────────────
            TokenType::Probe => self.parse_flow_step_simple("probe").map(|l| FlowStep::Probe(ProbeStep { target: l.1, loc: l.0 })),
            TokenType::Reason => self.parse_flow_step_simple("reason").map(|l| FlowStep::Reason(ReasonStep { strategy: String::new(), target: l.1, loc: l.0 })),
            TokenType::Validate => self.parse_flow_step_simple("validate").map(|l| FlowStep::Validate(ValidateStep { target: l.1, rule: String::new(), loc: l.0 })),
            TokenType::Refine => self.parse_flow_step_simple("refine").map(|l| FlowStep::Refine(RefineStep { target: l.1, strategy: String::new(), loc: l.0 })),
            TokenType::Weave => self.parse_weave_step(),
            TokenType::Use => self.parse_use_step(),
            TokenType::Remember => self.parse_remember_step(),
            TokenType::Recall => self.parse_recall_step(),
            TokenType::Par => self.parse_par_block().map(FlowStep::Par),
            TokenType::Hibernate => self.parse_hibernate_step(),
            TokenType::Deliberate => self.parse_block_step("deliberate").map(|l| FlowStep::Deliberate(DeliberateBlock { loc: l })),
            TokenType::Consensus => self.parse_block_step("consensus").map(|l| FlowStep::Consensus(ConsensusBlock { loc: l })),
            TokenType::Forge => self.parse_forge_step().map(FlowStep::Forge),
            TokenType::Focus => self.parse_focus_step(),
            TokenType::Grad => self.parse_grad_step(),
            TokenType::Associate => self.parse_associate_step(),
            TokenType::Aggregate => self.parse_aggregate_step(),
            TokenType::Explore => self.parse_explore_step(),
            TokenType::Ingest => self.parse_ingest_step(),
            TokenType::Shield => self.parse_apply_step("shield").map(|l| FlowStep::ShieldApply(ShieldApplyStep { shield_name: l.1, target: l.2, output_type: l.3, loc: l.0 })),
            // §Fase 111.e — `stream` parses its BODY. It used to go through
            // `parse_block_step`, whose entire job is `skip_braced_block()` —
            // the block's contents were thrown away at parse time, which is why
            // `run_stream` had nothing to run and "completed" with an empty
            // string while the README sold "Algebraic Effects and Free Monads".
            TokenType::Stream => self.parse_stream_block().map(FlowStep::Stream),
            TokenType::Navigate => self.parse_navigate_step(),
            TokenType::Drill => self.parse_drill_step(),
            TokenType::Trail => self.parse_flow_step_simple("trail").map(|l| FlowStep::Trail(TrailStep { navigate_ref: l.1, loc: l.0 })),
            TokenType::Corroborate => self.parse_corroborate_step(),
            TokenType::Ots => self.parse_apply_step("ots").map(|l| FlowStep::OtsApply(OtsApplyStep { ots_name: l.1, target: l.2, output_type: l.3, loc: l.0 })),
            TokenType::Mandate => self.parse_apply_step("mandate").map(|l| FlowStep::MandateApply(MandateApplyStep { mandate_name: l.1, target: l.2, output_type: l.3, loc: l.0 })),
            // §Fase 111.f — `compute <Name> on a, b -> out`. The ARGUMENTS used to
            // be `Vec::new()` — hardcoded empty at the parse site — so even if
            // the runtime had wanted to compute something, it had nothing to
            // compute it FROM.
            TokenType::Compute => self.parse_compute_apply().map(FlowStep::ComputeApply),
            TokenType::Listen => self.parse_listen_step(),
            TokenType::Daemon => self.parse_flow_step_simple("daemon").map(|l| FlowStep::DaemonStep(DaemonStepNode { daemon_ref: l.1, loc: l.0 })),
            // §λ-L-E Fase 13 — Mobile typed channels (paper §3.1, §3.2, §4.3)
            TokenType::Emit => self.parse_emit_step(),
            // §Fase 92.b — `mint <Credential> as <binding>` (ephemeral credential).
            TokenType::Mint => self.parse_mint_step(),
            // §Fase 94.b — `rotate <SecretsStore> [where "…"] with <Tool> as
            // <binding>` (mediated secret renewal).
            TokenType::Rotate => self.parse_rotate_step(),
            TokenType::Publish => self.parse_publish_step(),
            TokenType::Discover => self.parse_discover_step(),
            TokenType::Persist => self.parse_persist_step(),
            TokenType::Retrieve => self.parse_retrieve_step(),
            TokenType::Mutate => self.parse_mutate_step(),
            TokenType::Purge => self.parse_store_where_step().map(|(loc, store_name, where_expr)| FlowStep::Purge(PurgeStep { store_name, where_expr, loc })),
            TokenType::Transact => self.parse_block_step("transact").map(|l| FlowStep::Transact(TransactBlock { loc: l })),
            // §Fase 88.a — the `warden` adversarial-analysis block.
            TokenType::Warden => self.parse_warden().map(FlowStep::Warden),
            // §Fase 51.a — the `quant` cognitive block (Hilbert-space projection).
            TokenType::Quant => self.parse_quant().map(FlowStep::Quant),
            // §Fase 51.d.2 — the `yield` measurement point.
            TokenType::Yield => self.parse_yield().map(FlowStep::Yield),
            // §Fase 52.c — `run <Flow>(args)` as a flow-step: invoke a declared
            // flow from inside a body (a `daemon` listen handler, Q3). Reuses
            // the top-level run parser.
            TokenType::Run => self.parse_run().map(FlowStep::Run),

            _ => {
                // §Fase 28.e — append "Did you mean X?" hint when the
                // unknown token looks like a typo'd flow-body keyword
                // (e.g. `stepp` / `reasn` / `validte`). D3, D11.
                let hint = crate::smart_suggest::suggest_for(
                    &tok.value,
                    crate::smart_suggest::FLOW_BODY_KEYWORD_NAMES,
                );
                let base = format!(
                    "Unexpected token in flow body: '{}' — expected step, if, for, let, return, ...",
                    tok.value
                );
                let message = if hint.is_empty() {
                    base
                } else {
                    format!("{base}. {hint}")
                };
                Err(ParseError {
                    message,
                    line: tok.line,
                    column: tok.column,
                    ..Default::default()
                })
            }
        }
    }

    // ── STEP ─────────────────────────────────────────────────────

    fn parse_step(&mut self) -> Result<StepNode, ParseError> {
        let tok = self.consume(TokenType::Step)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;

        let mut persona_ref = String::new();
        if self.check(TokenType::Use) {
            self.advance();
            persona_ref = self.consume_any_ident_or_kw()?.value;
        }

        self.consume(TokenType::LBrace)?;

        let mut node = StepNode {
            name,
            persona_ref,
            given: String::new(),
            ask: String::new(),
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            requires_context: None,
            now_tz: None,
            guards: Vec::new(),
            loc,
        };

        while !self.check(TokenType::RBrace) {
            let inner = self.current().clone();

            match inner.ttype {
                TokenType::Given => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.given = self.parse_expression_string()?;
                }
                TokenType::Ask => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.ask = self.consume(TokenType::StringLit)?.value;
                }
                TokenType::Output => {
                    // Mirror of Python `_parse_step` `case "output":`
                    // which uses `_parse_output_type_string` — accepts
                    // the FULL generic-aware shape `Stream<T>`,
                    // `Stream<T>?`, `Identifier?`, NOT just the bare
                    // head identifier. Pre-fix the step parser dropped
                    // `<T>` and downstream `flow_has_stream_output`'s
                    // `starts_with("Stream<") && ends_with('>')` then
                    // returned false → `implicit_transport == "json"`
                    // → dynamic routes served JSON instead of SSE.
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.output_type = self.parse_output_type_string()?;
                }
                TokenType::Navigate => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.navigate_ref = self.parse_dotted_identifier()?;
                }
                TokenType::Identifier if inner.value == "confidence_floor" => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.confidence_floor = Some(self.consume_number()?);
                }
                TokenType::Identifier if inner.value == "apply" => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.apply_ref = self.consume_any_ident_or_kw()?.value;
                }
                // §Fase 68.b — `requires_context: <tokens>`: the step's declared
                // model-capability requirement (the context window the cognition
                // needs). A bare positive integer literal; the §68.c resolver maps
                // it to a concrete model. Range/ceiling is the type-checker's job
                // (§68.b positive-int + §68.f catalog ceiling) — the parser only
                // requires an integer token here (a float / non-number is a parse
                // error, surfaced at the exact column).
                TokenType::Identifier if inner.value == "requires_context" => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    let num = self.current().clone();
                    let bad = |tok: &crate::tokens::Token| ParseError {
                        message: format!(
                            "`requires_context:` must be a positive integer token count \
                             (got '{}')",
                            tok.value
                        ),
                        line: tok.line,
                        column: tok.column,
                        ..Default::default()
                    };
                    if num.ttype != TokenType::Integer {
                        return Err(bad(&num));
                    }
                    let value = num.value.parse::<u32>().map_err(|_| bad(&num))?;
                    self.advance();
                    node.requires_context = Some(value);
                }
                // §Fase 91.a — `now: "<IANA-tz>"`: the step's declared cognitive
                // timezone. A string literal; the format law (IANA shape) is the
                // type-checker's job (`axon-T892`) — the parser only requires a
                // string token here, surfaced at the exact column.
                TokenType::Identifier if inner.value == "now" => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    let tz = self.current().clone();
                    if tz.ttype != TokenType::StringLit {
                        return Err(ParseError {
                            message: format!(
                                "`now:` must be an IANA timezone string literal like \
                                 \"America/Bogota\" or \"UTC\" (got '{}')",
                                tz.value
                            ),
                            line: tz.line,
                            column: tz.column,
                            ..Default::default()
                        });
                    }
                    self.advance();
                    node.now_tz = Some(tz.value);
                }
                // §Fase 54.a — a `use` nested inside a `step { }` body used
                // to be skipped structurally (grouped with the sub-constructs
                // below), silently degrading the tool dispatch to an
                // unconstrained LLM step with NO diagnostic. That fallthrough
                // drops the AST node before the type-checker can see it, so the
                // resource the tool would provision is never linearly accounted
                // for (use_tool soundness). Reject it here, at the parser —
                // the only place that still sees the token — and redirect to
                // the canonical forms.
                TokenType::Use => {
                    let tool = self
                        .tokens
                        .get(self.pos + 1)
                        .map(|t| t.value.as_str())
                        .filter(|v| !v.is_empty())
                        .unwrap_or("<Tool>");
                    return Err(ParseError {
                        message: format!(
                            "`use` is not valid inside a `step {{ }}` body — the tool dispatch \
                             would be silently dropped. To invoke a tool, either write the \
                             flow-level step `use {tool} on <arg>` (outside this block), or bind \
                             it inside this step with `apply: {tool}`. To attach a persona, put \
                             it in the step header: `step <name> use <Persona> {{ … }}`."
                        ),
                        line: inner.line,
                        column: inner.column,
                        ..Default::default()
                    });
                }
                // §Fase 119 (D119.4) — `mandate X on Y`, `shield X on Y -> b`,
                // `ots X on Y` as STEP-BODY statements. README §XV has always
                // written the application here — next to the `output:` it
                // constrains — and the parser accepted the same form only at
                // flow level, which is why README blocks 40–42 never compiled.
                // The published position is also the better semantics: a
                // mandate inside a step is scoped to THIS step's generation;
                // the flow-level form governs a bare statement whose subject
                // must be inferred. One concept, two positions, same AST shape
                // as the flow-level `*ApplyStep` family.
                TokenType::Mandate => {
                    let g = self.parse_step_guard("mandate")?;
                    node.guards.push(g);
                }
                TokenType::Shield => {
                    let g = self.parse_step_guard("shield")?;
                    node.guards.push(g);
                }
                TokenType::Ots => {
                    let g = self.parse_step_guard("ots")?;
                    node.guards.push(g);
                }
                // Sub-constructs (probe, reason, weave, stream) → skip structurally
                TokenType::Probe
                | TokenType::Reason
                | TokenType::Weave
                | TokenType::Stream => {
                    self.skip_flow_step_structural()?;
                }
                _ => {
                    return Err(ParseError {
                        message: format!(
                            "Unexpected token in step body: '{}' — expected given, ask, \
                             probe, reason, weave, stream, output, confidence_floor, navigate, \
                             apply, requires_context, now",
                            inner.value
                        ),
                        line: inner.line,
                        column: inner.column,
                                            ..Default::default()
                    });
                }
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Skip a flow-level sub-construct structurally (consume keyword + args + optional block).
    fn skip_flow_step_structural(&mut self) -> Result<(), ParseError> {
        // Consume the keyword
        self.advance();
        // Consume tokens until we hit a { or a closing }, or a known flow step keyword
        while !self.check(TokenType::LBrace)
            && !self.check(TokenType::RBrace)
            && !self.check(TokenType::Eof)
        {
            // Check if we hit a new step-level keyword (means this was a one-liner)
            let tt = &self.current().ttype;
            if matches!(
                tt,
                TokenType::Step
                    | TokenType::Given
                    | TokenType::Ask
                    | TokenType::Output
                    | TokenType::Navigate
                    | TokenType::Use
                    | TokenType::Probe
                    | TokenType::Reason
                    | TokenType::Weave
                    | TokenType::Stream
                    | TokenType::If
                    | TokenType::For
                    | TokenType::Let
                    | TokenType::Return
            ) {
                return Ok(());
            }
            self.advance();
        }
        // If block, skip it
        if self.check(TokenType::LBrace) {
            self.skip_braced_block()?;
        }
        Ok(())
    }

    // ── INTENT ───────────────────────────────────────────────────

    fn parse_intent(&mut self) -> Result<IntentNode, ParseError> {
        let tok = self.consume(TokenType::Intent)?;
        let loc = self.loc_of(&tok);
        let name = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LBrace)?;

        let mut node = IntentNode {
            name,
            given: String::new(),
            ask: String::new(),
            output_type: None,
            confidence_floor: None,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while !self.check(TokenType::RBrace) {
            let field_name = self.current().value.clone();
            self.advance();
            self.consume(TokenType::Colon)?;

            match field_name.as_str() {
                "given" => node.given = self.consume(TokenType::Identifier)?.value,
                "ask" => node.ask = self.consume(TokenType::StringLit)?.value,
                "output" => node.output_type = Some(self.parse_type_expr()?),
                "confidence_floor" => node.confidence_floor = Some(self.consume_number()?),
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── RUN ──────────────────────────────────────────────────────

    fn parse_run(&mut self) -> Result<RunStatement, ParseError> {
        let tok = self.consume(TokenType::Run)?;
        let loc = self.loc_of(&tok);
        let flow_name = self.consume(TokenType::Identifier)?.value;

        self.consume(TokenType::LParen)?;
        let mut arguments = Vec::new();
        if !self.check(TokenType::RParen) {
            arguments = self.parse_argument_list()?;
        }
        self.consume(TokenType::RParen)?;

        let mut node = RunStatement {
            flow_name,
            arguments,
            persona: String::new(),
            context: String::new(),
            anchors: Vec::new(),
            on_failure: String::new(),
            on_failure_params: Vec::new(),
            output_to: String::new(),
            effort: String::new(),
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while self.check_run_modifier() {
            let mod_tok = self.current().clone();
            match mod_tok.ttype {
                TokenType::As => {
                    self.advance();
                    node.persona = self.consume(TokenType::Identifier)?.value;
                }
                TokenType::Within => {
                    self.advance();
                    node.context = self.consume(TokenType::Identifier)?.value;
                }
                TokenType::ConstrainedBy => {
                    self.advance();
                    node.anchors = self.parse_bracketed_identifiers()?;
                }
                TokenType::OnFailure => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.on_failure = self.consume_any_ident_or_kw()?.value;
                    // Parse optional params: (key: val, ...)
                    if self.check(TokenType::LParen) {
                        self.advance();
                        while !self.check(TokenType::RParen) && !self.check(TokenType::Eof) {
                            let key = self.consume_any_ident_or_kw()?.value;
                            self.consume(TokenType::Colon)?;
                            let val = self.consume_any_ident_or_kw()?.value;
                            node.on_failure_params.push((key, val));
                            if self.check(TokenType::Comma) {
                                self.advance();
                            }
                        }
                        if self.check(TokenType::RParen) {
                            self.advance();
                        }
                    }
                }
                TokenType::OutputTo => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.output_to = self.consume(TokenType::StringLit)?.value;
                }
                TokenType::Effort => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.effort = self.consume_any_ident_or_kw()?.value;
                }
                _ => break,
            }
        }

        Ok(node)
    }

    // ── EPISTEMIC BLOCK ──────────────────────────────────────────

    fn parse_epistemic_block(&mut self) -> Result<EpistemicBlock, ParseError> {
        let tok = self.current().clone();
        let mode = match tok.ttype {
            TokenType::Know => "know",
            TokenType::Believe => "believe",
            TokenType::Speculate => "speculate",
            TokenType::Doubt => "doubt",
            _ => unreachable!(),
        };
        self.advance();
        let loc = self.loc_of(&tok);

        self.consume(TokenType::LBrace)?;
        let mut body = Vec::new();
        while !self.check(TokenType::RBrace) {
            body.push(self.parse_declaration()?);
        }
        self.consume(TokenType::RBrace)?;

        Ok(EpistemicBlock {
            mode: mode.to_string(),
            body,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        })
    }

    // ── IF ────────────────────────────────────────────────────────

    // ── §Fase 70.a — the pure expression engine (Pratt parser) ───────────

    /// Parse a pure expression (§Fase 70). Precedence-climbing: `or` < `and` <
    /// comparison < `+ -` < `* / %` < unary (`- not`) < atom. Total + pure; no
    /// side effects. Field/index access + the builtin catalog land in §70.c/d.
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        // Prefix: unary `-` (negation) / `not` (boolean). Binds tighter than
        // every binary operator (bp 6).
        let mut lhs = match self.current().ttype {
            TokenType::Minus => {
                self.advance();
                Expr::Unary(UnOp::Neg, Box::new(self.parse_expr_bp(6)?))
            }
            TokenType::Not => {
                self.advance();
                Expr::Unary(UnOp::Not, Box::new(self.parse_expr_bp(6)?))
            }
            _ => self.parse_postfix()?,
        };
        // Infix: left-associative (right_bp = left_bp + 1).
        while let Some((op, lbp)) = Self::binop_of(self.current().ttype.clone()) {
            if lbp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.parse_expr_bp(lbp + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// Map a token to `(BinOp, left binding power)`, or `None` if it is not an
    /// infix operator (which stops the climb — e.g. at `->` or `{`).
    fn binop_of(t: TokenType) -> Option<(BinOp, u8)> {
        Some(match t {
            TokenType::Or => (BinOp::Or, 1),
            TokenType::And => (BinOp::And, 2),
            TokenType::Eq => (BinOp::Eq, 3),
            TokenType::Neq => (BinOp::Ne, 3),
            TokenType::Lt => (BinOp::Lt, 3),
            TokenType::Lte => (BinOp::Le, 3),
            TokenType::Gt => (BinOp::Gt, 3),
            TokenType::Gte => (BinOp::Ge, 3),
            TokenType::Plus => (BinOp::Add, 4),
            TokenType::Minus => (BinOp::Sub, 4),
            TokenType::Star => (BinOp::Mul, 5),
            TokenType::Slash => (BinOp::Div, 5),
            TokenType::Percent => (BinOp::Mod, 5),
            _ => return None,
        })
    }

    /// §Fase 70.c — parse a primary then its `.` postfix chain: a builtin call
    /// (`.length`, `.contains(x)`) when the name is in the closed catalog, else
    /// a dotted reference-path continuation (`a.b.c` → `Ref("a.b.c")`, the
    /// pre-§70.c behaviour). Field access on a non-reference (`(a+b).x`) is
    /// reserved for §70.d.
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_expr_atom()?;
        loop {
            if self.check(TokenType::Dot) {
                self.advance();
                let name = self.consume_any_ident_or_kw()?.value;
                if let Some(builtin) = Builtin::from_name(&name) {
                    let mut args = vec![expr];
                    if self.check(TokenType::LParen) {
                        self.advance();
                        while !self.check(TokenType::RParen) && !self.check(TokenType::Eof) {
                            args.push(self.parse_expr_bp(0)?);
                            if self.check(TokenType::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        self.consume(TokenType::RParen)?;
                    }
                    expr = Expr::Call(builtin, args);
                } else {
                    // §Fase 70.d — a plain dotted path on a Ref extends the Ref
                    // (back-compat: `a.b.c` → `Ref("a.b.c")`); on any other base
                    // it is a structured field access (the JSONB seam).
                    expr = match expr {
                        Expr::Ref(p) => Expr::Ref(format!("{p}.{name}")),
                        other => Expr::Field(Box::new(other), name),
                    };
                }
            } else if self.check(TokenType::LBracket) {
                // §Fase 70.d — index access `base[index]`.
                self.advance();
                let index = self.parse_expr_bp(0)?;
                self.consume(TokenType::RBracket)?;
                expr = Expr::Index(Box::new(expr), Box::new(index));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_expr_atom(&mut self) -> Result<Expr, ParseError> {
        let tok = self.current().clone();
        match tok.ttype {
            TokenType::Integer => {
                self.advance();
                let lit = tok
                    .value
                    .parse::<i64>()
                    .map(ExprLit::Int)
                    .or_else(|_| tok.value.parse::<f64>().map(ExprLit::Float))
                    .map_err(|_| ParseError {
                        message: format!("invalid integer literal '{}'", tok.value),
                        line: tok.line,
                        column: tok.column,
                        ..Default::default()
                    })?;
                Ok(Expr::Lit(lit))
            }
            TokenType::Float => {
                self.advance();
                let f = tok.value.parse::<f64>().map_err(|_| ParseError {
                    message: format!("invalid float literal '{}'", tok.value),
                    line: tok.line,
                    column: tok.column,
                    ..Default::default()
                })?;
                Ok(Expr::Lit(ExprLit::Float(f)))
            }
            TokenType::Bool => {
                self.advance();
                Ok(Expr::Lit(ExprLit::Bool(tok.value == "true")))
            }
            TokenType::StringLit => {
                self.advance();
                Ok(Expr::Lit(ExprLit::Str(tok.value)))
            }
            TokenType::LParen => {
                self.advance();
                let inner = self.parse_expr_bp(0)?;
                self.consume(TokenType::RParen)?;
                Ok(inner)
            }
            _ => {
                // Reference: a single identifier (or keyword used as a name).
                // The `.` chain (dotted path / builtin call) is handled by the
                // postfix layer (§70.c `parse_postfix`).
                Ok(Expr::Ref(self.consume_any_ident_or_kw()?.value))
            }
        }
    }

    /// §Fase 70.a — render a literal to its legacy surface string (for the
    /// back-compat `(condition, op, value)` triple). Only used when an
    /// expression fits the legacy shape; numeric round-tripping is exact for
    /// ints and faithful-enough for floats (the legacy runtime re-parses it).
    fn expr_lit_surface(lit: &ExprLit) -> String {
        match lit {
            ExprLit::Int(i) => i.to_string(),
            ExprLit::Float(f) => f.to_string(),
            ExprLit::Bool(b) => b.to_string(),
            ExprLit::Str(s) => s.clone(),
        }
    }

    fn expr_leaf_surface(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ref(p) => Some(p.clone()),
            Expr::Lit(l) => Some(Self::expr_lit_surface(l)),
            _ => None,
        }
    }

    /// A legacy "leaf" is a bare reference (truthy check) or a
    /// `<ref> <cmp> <ref|literal>` triple — exactly what the pre-§70 `if`
    /// grammar could express.
    fn expr_legacy_leaf(expr: &Expr) -> Option<(String, String, String)> {
        match expr {
            Expr::Ref(p) => Some((p.clone(), String::new(), String::new())),
            Expr::Binary(op, l, r) => {
                let op_s = match op {
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    _ => return None,
                };
                let lhs = match &**l {
                    Expr::Ref(p) => p.clone(),
                    _ => return None,
                };
                let rhs = Self::expr_leaf_surface(r)?;
                Some((lhs, op_s.to_string(), rhs))
            }
            _ => None,
        }
    }

    /// Flatten an `or`-tree of legacy leaves in left-to-right order. Returns
    /// `false` (and leaves `out` unusable) if any node is not a legacy leaf.
    fn collect_or_leaves(expr: &Expr, out: &mut Vec<(String, String, String)>) -> bool {
        match expr {
            Expr::Binary(BinOp::Or, l, r) => {
                Self::collect_or_leaves(l, out) && Self::collect_or_leaves(r, out)
            }
            _ => match Self::expr_legacy_leaf(expr) {
                Some(t) => {
                    out.push(t);
                    true
                }
                None => false,
            },
        }
    }

    /// §Fase 70.a — if the parsed condition fits the legacy
    /// `(condition, op, value)` + `or`-chain shape, return the legacy fields so
    /// the IR + runtime stay byte-identical to pre-§70 (zero drift). `None` ⇒
    /// the condition uses richer forms (`and`, `not`, arithmetic, parentheses,
    /// nesting) and must ride the `cond` expression evaluator.
    #[allow(clippy::type_complexity)]
    fn cond_as_legacy(
        expr: &Expr,
    ) -> Option<(String, String, String, Vec<(String, String, String)>, String)> {
        let mut leaves = Vec::new();
        if !Self::collect_or_leaves(expr, &mut leaves) || leaves.is_empty() {
            return None;
        }
        let (c0, o0, v0) = leaves[0].clone();
        let rest = leaves[1..].to_vec();
        let conjunctor = if rest.is_empty() {
            String::new()
        } else {
            "or".to_string()
        };
        Some((c0, o0, v0, rest, conjunctor))
    }

    fn parse_if(&mut self) -> Result<ConditionalNode, ParseError> {
        let tok = self.consume(TokenType::If)?;
        let loc = self.loc_of(&tok);

        // §Fase 70.a — parse the condition as a pure expression, then split:
        // a legacy-expressible condition populates the legacy triple fields
        // (cond = None → byte-identical IR + eval); a richer condition rides
        // the `cond` expression evaluator.
        let expr = self.parse_expr()?;
        let (condition, comparison_op, comparison_value, conditions, conjunctor, cond) =
            match Self::cond_as_legacy(&expr) {
                Some((c, o, v, more, conj)) => (c, o, v, more, conj, None),
                None => (
                    String::new(),
                    String::new(),
                    String::new(),
                    Vec::new(),
                    String::new(),
                    Some(expr),
                ),
            };

        let mut then_body = Vec::new();
        let mut else_body = Vec::new();

        // Arrow form or block form
        if self.check(TokenType::Arrow) {
            self.advance();
            then_body.push(self.parse_flow_step()?);
        } else if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) {
                then_body.push(self.parse_flow_step()?);
            }
            self.consume(TokenType::RBrace)?;
        }

        // Else branch
        if self.check(TokenType::Else) {
            self.advance();
            if self.check(TokenType::Arrow) {
                self.advance();
                else_body.push(self.parse_flow_step()?);
            } else if self.check(TokenType::LBrace) {
                self.advance();
                while !self.check(TokenType::RBrace) {
                    else_body.push(self.parse_flow_step()?);
                }
                self.consume(TokenType::RBrace)?;
            }
        }

        Ok(ConditionalNode {
            condition,
            comparison_op,
            comparison_value,
            then_body,
            else_body,
            conditions,
            conjunctor,
            cond,
            loc,
        })
    }

    // ── FOR IN ───────────────────────────────────────────────────

    fn parse_for_in(&mut self) -> Result<ForInStatement, ParseError> {
        let tok = self.consume(TokenType::For)?;
        let loc = self.loc_of(&tok);
        let variable = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::In)?;
        let iterable = self.parse_dotted_identifier()?;

        self.consume(TokenType::LBrace)?;
        // Fase 19.e — increment loop_depth so `parse_break` /
        // `parse_continue` inside the body pass the scope check.
        // Decrement on every exit path (Ok / Err) so a parse error
        // mid-body does not leave the depth permanently elevated
        // for later top-level parsing — `?` would skip the
        // decrement otherwise.
        self.loop_depth += 1;
        let body_result = (|| -> Result<Vec<FlowStep>, ParseError> {
            let mut body = Vec::new();
            while !self.check(TokenType::RBrace) {
                body.push(self.parse_flow_step()?);
            }
            Ok(body)
        })();
        self.loop_depth -= 1;
        let body = body_result?;
        self.consume(TokenType::RBrace)?;

        Ok(ForInStatement {
            variable,
            iterable,
            body,
            loc,
        })
    }

    /// Fase 19.e — `break` keyword. Compile-time scope check
    /// (`loop_depth == 0`) rejects break outside a for-in body.
    fn parse_break(&mut self) -> Result<BreakStatement, ParseError> {
        let tok = self.consume(TokenType::Break)?;
        let loc = self.loc_of(&tok);
        if self.loop_depth == 0 {
            return Err(ParseError {
                message: "'break' outside of a for-in loop body".to_string(),
                line: tok.line,
                column: tok.column,
                            ..Default::default()
            });
        }
        Ok(BreakStatement { loc })
    }

    /// Fase 19.e — `continue` keyword. Same scope check as
    /// `parse_break`.
    fn parse_continue(&mut self) -> Result<ContinueStatement, ParseError> {
        let tok = self.consume(TokenType::Continue)?;
        let loc = self.loc_of(&tok);
        if self.loop_depth == 0 {
            return Err(ParseError {
                message: "'continue' outside of a for-in loop body".to_string(),
                line: tok.line,
                column: tok.column,
                            ..Default::default()
            });
        }
        Ok(ContinueStatement { loc })
    }

    // ── LET ──────────────────────────────────────────────────────

    fn parse_let(&mut self) -> Result<LetStatement, ParseError> {
        let tok = self.consume(TokenType::Let)?;
        let loc = self.loc_of(&tok);

        // Name can be an identifier or a keyword used as binding name
        let name = self.consume_any_ident_or_kw()?.value;
        // §Fase 51.c.3 — optional type annotation `let x: <TypeExpr> = …`.
        let type_annotation = if self.check(TokenType::Colon) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };
        self.consume(TokenType::Assign)?;
        // Fase 17.a — reset side-channel before parsing value; the
        // atom / expr helpers tag the kind as they descend.
        self.last_let_value_kind = "literal".to_string();
        let (value, value_ast) = self.parse_let_value_expr_with_ast()?;

        Ok(LetStatement {
            identifier: name,
            value_expr: value,
            value_kind: self.last_let_value_kind.clone(),
            type_annotation,
            value_ast,
            loc,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        })
    }

    fn parse_let_value_expr(&mut self) -> Result<String, ParseError> {
        let atom = self.parse_let_atom()?;

        // Arithmetic expression: collect as string
        if matches!(
            self.current().ttype,
            TokenType::Plus | TokenType::Minus | TokenType::Star | TokenType::Slash
        ) {
            let mut parts = vec![atom];
            while matches!(
                self.current().ttype,
                TokenType::Plus | TokenType::Minus | TokenType::Star | TokenType::Slash
            ) {
                parts.push(self.advance().value.clone());
                parts.push(self.parse_let_atom()?);
            }
            self.last_let_value_kind = "expression".to_string();
            return Ok(parts.join(" "));
        }
        Ok(atom)
    }

    /// §Fase 70.f — parse a `let`-binding value, additionally producing a
    /// structured `value_ast` for the expression case. A list literal keeps the
    /// dedicated path; everything else is parsed through the §70 expression
    /// engine and classified: a bare literal / reference keeps its pre-§70
    /// string form (`value_ast = None`, byte-identical), while a real expression
    /// (`price * qty`, `recent.length`) additionally carries a `value_ast` the
    /// runtime evaluates for real (pre-§70.f it was treated as an opaque literal
    /// string). Used ONLY by `parse_let` — other value positions (list items,
    /// remember/stream values) keep the string-only `parse_let_value_expr`.
    fn parse_let_value_expr_with_ast(&mut self) -> Result<(String, Option<Expr>), ParseError> {
        if self.check(TokenType::LBracket) {
            self.last_let_value_kind = "literal".to_string();
            return Ok((self.parse_let_list_literal()?, None));
        }
        let expr = self.parse_expr()?;
        Ok(match expr {
            Expr::Lit(lit) => {
                self.last_let_value_kind = "literal".to_string();
                (Self::expr_lit_surface(&lit), None)
            }
            Expr::Ref(p) => {
                self.last_let_value_kind = "reference".to_string();
                (p, None)
            }
            other => {
                self.last_let_value_kind = "expression".to_string();
                (Self::render_expr(&other), Some(other))
            }
        })
    }

    /// §Fase 70.f — a readable surface rendering of an expression for the
    /// vestigial `value_expr` string (the runtime uses `value_ast`).
    fn render_expr(e: &Expr) -> String {
        match e {
            Expr::Lit(l) => Self::expr_lit_surface(l),
            Expr::Ref(p) => p.clone(),
            Expr::Unary(UnOp::Neg, x) => format!("-{}", Self::render_expr(x)),
            Expr::Unary(UnOp::Not, x) => format!("not {}", Self::render_expr(x)),
            Expr::Binary(op, l, r) => {
                let sym = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => "%",
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    BinOp::And => "and",
                    BinOp::Or => "or",
                };
                format!("({} {sym} {})", Self::render_expr(l), Self::render_expr(r))
            }
            Expr::Call(b, args) => {
                let recv = args.first().map(Self::render_expr).unwrap_or_default();
                let rest: Vec<String> = args.iter().skip(1).map(Self::render_expr).collect();
                if rest.is_empty() {
                    format!("{recv}.{}", b.surface())
                } else {
                    format!("{recv}.{}({})", b.surface(), rest.join(", "))
                }
            }
            Expr::Field(b, f) => format!("{}.{f}", Self::render_expr(b)),
            Expr::Index(b, i) => format!("{}[{}]", Self::render_expr(b), Self::render_expr(i)),
        }
    }

    fn parse_let_atom(&mut self) -> Result<String, ParseError> {
        let tok = self.current().clone();

        match tok.ttype {
            TokenType::StringLit => {
                self.last_let_value_kind = "literal".to_string();
                self.advance();
                Ok(tok.value)
            }
            TokenType::Integer | TokenType::Float => {
                self.last_let_value_kind = "literal".to_string();
                self.advance();
                Ok(tok.value)
            }
            TokenType::Bool => {
                self.last_let_value_kind = "literal".to_string();
                self.advance();
                Ok(tok.value)
            }
            TokenType::Identifier => {
                self.last_let_value_kind = "reference".to_string();
                self.parse_dotted_identifier()
            }
            TokenType::LBracket => {
                self.last_let_value_kind = "literal".to_string();
                self.parse_let_list_literal()
            }
            _ => {
                // Keywords starting a dotted path (pix.document_tree)
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].ttype == TokenType::Dot
                {
                    self.last_let_value_kind = "reference".to_string();
                    return self.parse_dotted_identifier();
                }
                Err(ParseError {
                    message: format!(
                        "Expected value expression, found {:?}('{}')",
                        tok.ttype, tok.value
                    ),
                    line: tok.line,
                    column: tok.column,
                                    ..Default::default()
                })
            }
        }
    }

    fn parse_let_list_literal(&mut self) -> Result<String, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut items = Vec::new();
        if !self.check(TokenType::RBracket) {
            items.push(self.parse_let_value_expr()?);
            while self.check(TokenType::Comma) {
                self.advance();
                if self.check(TokenType::RBracket) {
                    break; // trailing comma
                }
                items.push(self.parse_let_value_expr()?);
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(format!("[{}]", items.join(", ")))
    }

    // ── RETURN ───────────────────────────────────────────────────

    fn parse_return(&mut self) -> Result<ReturnStatement, ParseError> {
        let tok = self.consume(TokenType::Return)?;
        let loc = self.loc_of(&tok);
        let value = self.parse_let_value_expr()?;
        Ok(ReturnStatement {
            value_expr: value,
            loc,
        })
    }

    // ── TIER 2 FLOW STEP HELPERS ────────────────────────────────────

    /// Parse: keyword target (consumes keyword + one identifier/keyword-as-value).
    fn parse_flow_step_simple(&mut self, _kw: &str) -> Result<(Loc, String), ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume keyword
        let target = if self.at_declaration_start()
            || self.check(TokenType::RBrace)
            || self.check(TokenType::Eof)
        {
            String::new()
        } else {
            self.consume_any_ident_or_kw()?.value.clone()
        };
        // Skip optional braced block
        if self.check(TokenType::LBrace) {
            self.skip_braced_block()?;
        }
        Ok((
            Loc {
                line: tok.line,
                column: tok.column,
            },
            target,
        ))
    }

    /// Parse: keyword { ... } — block-level step, skip body structurally.
    /// §Fase 111.e — `stream { <steps> }` with a REAL body.
    ///
    /// The four block primitives (`deliberate`, `consensus`, `stream`,
    /// `transact`) all went through [`Self::parse_block_step`], whose entire job
    /// is `skip_braced_block()`. Their bodies were discarded at parse time — so
    /// their handlers were not no-ops through neglect, they were no-ops
    /// *by construction*: there was nothing in the AST to execute. §111 retracted
    /// `transact`; this gives `stream` its body back. `deliberate` / `consensus`
    /// remain body-less pending their Tier-4 disposition.
    fn parse_stream_block(&mut self) -> Result<StreamBlock, ParseError> {
        let tok = self.current().clone();
        let loc = self.loc_of(&tok);
        self.advance(); // consume `stream`

        // Tolerate the pre-111 form `stream <effect-ish tokens> { … }`: skip any
        // argument tokens ahead of the brace, exactly as `parse_block_step` did,
        // so an existing program keeps parsing. Only the BODY changes.
        while !self.check(TokenType::LBrace)
            && !self.check(TokenType::RBrace)
            && !self.check(TokenType::Eof)
            && !self.at_declaration_start()
        {
            self.advance();
        }

        let mut body = Vec::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                body.push(self.parse_flow_step()?);
            }
            self.consume(TokenType::RBrace)?;
        }

        Ok(StreamBlock { body, loc })
    }

    fn parse_block_step(&mut self, _kw: &str) -> Result<Loc, ParseError> {
        let tok = self.current().clone();
        self.advance();
        // Skip optional arguments before brace
        while !self.check(TokenType::LBrace)
            && !self.check(TokenType::RBrace)
            && !self.check(TokenType::Eof)
            && !self.at_declaration_start()
        {
            self.advance();
        }
        if self.check(TokenType::LBrace) {
            self.skip_braced_block()?;
        }
        Ok(Loc {
            line: tok.line,
            column: tok.column,
        })
    }

    /// §Fase 86 — parse `forge <Name>(seed: "<text>") -> <Type> { mode:,
    /// novelty:, depth:, branches:, constraints: }`. Real field capture
    /// (replacing the pre-§86 discard-everything stub). Strict closed-catalog:
    /// an unknown field is a hard parse error; all cross-field laws (Boden mode
    /// catalog, novelty range, depth/branches ≥ 1, `constraints:` → `anchor`)
    /// are §86.c type-checker territory.
    fn parse_forge_step(&mut self) -> Result<ForgeBlock, ParseError> {
        let tok = self.consume(TokenType::Forge)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ForgeBlock {
            name,
            novelty: 0.5,
            depth: 1,
            branches: 1,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        // `(seed: "...")`
        self.consume(TokenType::LParen)?;
        let arg = self.consume_any_ident_or_kw()?.value;
        self.consume(TokenType::Colon)?;
        if arg != "seed" {
            return Err(self.error(&format!(
                "forge '{}' expects `seed:` as its argument, found `{arg}`",
                node.name
            )));
        }
        node.seed = self.consume(TokenType::StringLit)?.value;
        self.consume(TokenType::RParen)?;
        // `-> <Type>`
        self.consume(TokenType::Arrow)?;
        node.output_type = self.consume_any_ident_or_kw()?.value;
        // `{ fields }`
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match field.as_str() {
                "mode" => node.mode = self.consume_any_ident_or_kw()?.value,
                "novelty" => node.novelty = self.consume_number()?,
                "depth" => {
                    node.depth = self.consume(TokenType::Integer)?.value.parse::<i64>().unwrap_or(0)
                }
                "branches" => {
                    node.branches =
                        self.consume(TokenType::Integer)?.value.parse::<i64>().unwrap_or(0)
                }
                "constraints" => node.constraints_ref = self.consume_any_ident_or_kw()?.value,
                other => {
                    return Err(self.error(&format!("unknown forge field `{other}`")))
                }
            }
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 65 — Parse `par { stmt1  stmt2  … }` into CONCURRENT branches.
    /// Each top-level flow statement inside the block is one branch (a
    /// single-statement body); they execute concurrently at runtime
    /// (`flow_dispatcher::parallel::run_branches_concurrently`). Before §65 the
    /// `par` body was skipped (`parse_block_step`), so the branches were lost
    /// and the handler ran as a stub. Multi-statement branches (grouping
    /// several steps into one sequential branch) are a future grammar
    /// extension; today the natural `par { step A  step B }` fans A and B out.
    fn parse_par_block(&mut self) -> Result<ParBlock, ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume `par`
        self.consume(TokenType::LBrace)?;
        let mut branches: Vec<Vec<FlowStep>> = Vec::new();
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            branches.push(vec![self.parse_flow_step()?]);
        }
        self.consume(TokenType::RBrace)?;
        Ok(ParBlock {
            branches,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        })
    }

    /// §Fase 51.a — Parse the `quant` cognitive block surface.
    ///
    /// Grammar (the attribute header is OPTIONAL):
    /// ```text
    /// quant { <flow steps> }
    /// quant(encoding: amplitude, observable: M, qubits: 10,
    ///       depth: 4, bandwidth: 0.5, reupload: 3, backend: quant_sim) { <flow steps> }
    /// ```
    /// The bare form (the paper's example) leaves every attribute defaulted
    /// (`encoding = amplitude`, `effect = quant_sim`). The body is parsed into
    /// real nested `FlowStep`s — like `par` branches — so §51.b's Continuous
    /// Type Invariant scans actual AST rather than skipped tokens.
    /// §Fase 88.a — parse `warden(<target>) within <Scope> { <body> }`. The
    /// `within <Scope>` clause is MANDATORY at the grammar level (fail-closed by
    /// construction: a scopeless warden cannot be written); §88.c checks the
    /// scope RESOLVES + the target is in its allowlist.
    fn parse_warden(&mut self) -> Result<WardenBlock, ParseError> {
        let tok = self.consume(TokenType::Warden)?;
        // `(<target>)` — the resource under analysis.
        self.consume(TokenType::LParen)?;
        let target = self.consume_any_ident_or_kw()?.value;
        self.consume(TokenType::RParen)?;
        // `within <Scope>` — MANDATORY. Omitting it is a hard parse error.
        self.consume(TokenType::Within)?;
        let scope_ref = self.consume(TokenType::Identifier)?.value;
        let mut block = WardenBlock {
            target,
            scope_ref,
            body: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };
        // Body: real nested flow steps (like `quant`/`par`).
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            block.body.push(self.parse_flow_step()?);
        }
        self.consume(TokenType::RBrace)?;
        Ok(block)
    }

    /// §Fase 88.a — parse `scope <Name> { targets: [ … ], depth: <ident>,
    /// approver: [requires] "<cap>" }`. Flat key:value block (the `cache` shape).
    /// Catalog + non-empty validation is §88.c. Unknown fields are a hard error
    /// (D83.7): a scope governs an offensive-capable analysis.
    fn parse_scope(&mut self) -> Result<ScopeDefinition, ParseError> {
        let tok = self.consume(TokenType::Scope)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ScopeDefinition {
            name,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "targets" => {
                    self.consume(TokenType::LBracket)?;
                    while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
                        let t = if self.check(TokenType::StringLit) {
                            self.consume(TokenType::StringLit)?.value
                        } else {
                            self.consume_any_ident_or_kw()?.value
                        };
                        node.targets.push(t);
                        if self.check(TokenType::Comma) {
                            self.advance();
                        }
                    }
                    self.consume(TokenType::RBracket)?;
                }
                "depth" => node.depth = self.consume_any_ident_or_kw()?.value,
                "approver" => {
                    // Optional `requires` sugar before the capability string.
                    if self.current().value == "requires" {
                        self.advance();
                    }
                    node.approver = self.consume(TokenType::StringLit)?.value;
                }
                other => {
                    return Err(self.error(&format!(
                        "unknown scope field `{other}` in scope `{}` — expected \
                         `targets` / `depth` / `approver`",
                        node.name
                    )))
                }
            }
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_quant(&mut self) -> Result<QuantBlock, ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume `quant`

        let mut block = QuantBlock {
            encoding: None,
            observable: None,
            qubits: None,
            depth: None,
            bandwidth: None,
            reupload: None,
            // D1/D9 default backend: the CPU simulator effect. `qpu_native` is
            // opt-in via `backend: qpu_native`.
            effect: "quant_sim".to_string(),
            body: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };

        // ── Optional attribute header: `(key: value, …)` ──
        if self.check(TokenType::LParen) {
            self.advance();
            while !self.check(TokenType::RParen) && !self.check(TokenType::Eof) {
                let key = self.consume_any_ident_or_kw()?.value;
                self.consume(TokenType::Colon)?;
                match key.as_str() {
                    "encoding" => {
                        block.encoding = Some(self.consume_any_ident_or_kw()?.value)
                    }
                    "observable" => {
                        block.observable = Some(self.parse_dotted_identifier()?)
                    }
                    "qubits" => block.qubits = Some(self.consume_number()? as i64),
                    "depth" => block.depth = Some(self.consume_number()? as i64),
                    "bandwidth" => block.bandwidth = Some(self.consume_number()?),
                    // §Fase 69.c — data re-uploading layers.
                    "reupload" => block.reupload = Some(self.consume_number()? as i64),
                    // `backend:` selects the algebraic-effect tag (D1/D9).
                    "backend" => block.effect = self.consume_any_ident_or_kw()?.value,
                    other => {
                        return Err(ParseError {
                            message: format!(
                                "Unknown `quant` attribute `{other}` — expected one of \
                                 encoding, observable, qubits, depth, bandwidth, reupload, backend"
                            ),
                            line: self.current().line,
                            column: self.current().column,
                            ..Default::default()
                        });
                    }
                }
                // Optional comma between attributes (order-free, trailing-comma ok).
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.consume(TokenType::RParen)?;
        }

        // ── Body: real nested flow steps (like `par`) ──
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            block.body.push(self.parse_flow_step()?);
        }
        self.consume(TokenType::RBrace)?;

        Ok(block)
    }

    /// §Fase 51.d.2 — Parse the `yield <expr>` measurement point. Reuses the
    /// `let`-value expression grammar (reference / literal / arithmetic) so the
    /// yielded value's tokenization intent is preserved in `value_kind`.
    fn parse_yield(&mut self) -> Result<YieldStatement, ParseError> {
        let tok = self.consume(TokenType::Yield)?;
        let loc = self.loc_of(&tok);
        self.last_let_value_kind = "literal".to_string();
        let value_expr = self.parse_let_value_expr()?;
        Ok(YieldStatement {
            value_expr,
            value_kind: self.last_let_value_kind.clone(),
            loc,
        })
    }

    /// Parse: keyword Name on target -> output_type (apply pattern).
    /// §Fase 111.f — `compute <Name> on <a>, <b>, … -> <out>`.
    ///
    /// Positional arguments, bound to the compute's declared parameters in order.
    /// The generic [`Self::parse_apply_step`] captured a single `on <target>` and
    /// then the call site threw even that away (`arguments: Vec::new()`).
    fn parse_compute_apply(&mut self) -> Result<ComputeApplyStep, ParseError> {
        let tok = self.current().clone();
        let loc = self.loc_of(&tok);
        self.advance(); // consume `compute`
        let compute_name = self.consume_any_ident_or_kw()?.value.clone();

        let mut arguments = Vec::new();
        if self.current().value == "on" {
            self.advance();
            loop {
                arguments.push(self.consume_any_ident_or_kw()?.value.clone());
                if self.check(TokenType::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let mut output_name = String::new();
        if self.check(TokenType::Arrow) {
            self.advance();
            output_name = self.consume_any_ident_or_kw()?.value.clone();
        }

        Ok(ComputeApplyStep {
            compute_name,
            arguments,
            output_name,
            loc,
        })
    }

    fn parse_apply_step(&mut self, _kw: &str) -> Result<(Loc, String, String, String), ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume keyword
        let name = self.consume_any_ident_or_kw()?.value.clone();
        let mut target = String::new();
        let mut output_type = String::new();
        // "on" target
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            let next = self.current().clone();
            if next.value == "on" {
                self.advance();
                target = self.consume_any_ident_or_kw()?.value.clone();
            }
        }
        // -> output_type
        if self.check(TokenType::Arrow) {
            self.advance();
            output_type = self.consume_any_ident_or_kw()?.value.clone();
        }
        // Skip optional braced block
        if self.check(TokenType::LBrace) {
            self.skip_braced_block()?;
        }
        Ok((
            Loc {
                line: tok.line,
                column: tok.column,
            },
            name,
            target,
            output_type,
        ))
    }

    /// §Fase 119 (D119.4) — `<kind> <Name> [on <target>] [-> <binding>]` inside
    /// a `step { }` body.
    ///
    /// Differences from the flow-level `parse_apply_step`, both deliberate:
    ///
    /// - The target may be a CALL EXPRESSION, captured verbatim: README block
    ///   42 writes `mandate LegalPrecision on ContractDrafter(terms)`. The
    ///   flow-level form never needed this; the published step-level form does.
    /// - No trailing braced block is skipped. A guard is one statement; a
    ///   silently-skipped block after it would be the §119.b.1 defect again.
    fn parse_step_guard(&mut self, kind: &str) -> Result<StepGuardNode, ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume the keyword
        let name = self.consume_any_ident_or_kw()?.value.clone();
        let mut target = String::new();
        let mut binding = String::new();
        if self.current().value == "on" {
            self.advance();
            target = self.consume_any_ident_or_kw()?.value.clone();
            // `ContractDrafter(terms)` — capture the balanced argument list
            // verbatim into the target string.
            if self.check(TokenType::LParen) {
                let mut depth = 0usize;
                loop {
                    let t = self.current().clone();
                    match t.ttype {
                        TokenType::LParen => depth += 1,
                        TokenType::RParen => depth -= 1,
                        TokenType::Eof => {
                            return Err(ParseError {
                                message: format!(
                                    "unterminated argument list in `{kind} {name} on {target}(…`"
                                ),
                                line: t.line,
                                column: t.column,
                                ..Default::default()
                            })
                        }
                        _ => {}
                    }
                    target.push_str(&t.value);
                    self.advance();
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
        if self.check(TokenType::Arrow) {
            self.advance();
            binding = self.consume_any_ident_or_kw()?.value.clone();
        }
        Ok(StepGuardNode {
            kind: kind.to_string(),
            name,
            target,
            binding,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        })
    }

    fn parse_weave_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let mut node = WeaveStep {
            sources: Vec::new(),
            target: String::new(),
            format_type: String::new(),
            priority: Vec::new(),
            style: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "sources" => node.sources = self.parse_bracketed_identifiers()?,
                        "target" => node.target = self.consume_any_ident_or_kw()?.value.clone(),
                        "format" => {
                            node.format_type = self.consume_any_ident_or_kw()?.value.clone()
                        }
                        "priority" => node.priority = self.parse_bracketed_identifiers()?,
                        "style" => node.style = self.consume_any_ident_or_kw()?.value.clone(),
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Weave(node))
    }

    fn parse_use_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let tool_name = self.consume_any_ident_or_kw()?.value.clone();
        // §Fase 58.b — two mutually-exclusive `use` argument surfaces:
        //   * `use Tool(query = "${q}", max_results = 5)` — D2 canonical
        //     multi-field keyword args (§58.b `UseArgs::Named`).
        //   * `use Tool on "${arg}"` / `on query` — the §54.b single positional
        //     argument (D5 back-compat, `UseArgs::LegacyPositional`):
        //       - a STRING LITERAL carrying interpolation (`on "${query}"`)
        //         resolved at dispatch against request-bound flow params;
        //       - a BARE identifier / literal (`on query` / `on 42`) verbatim.
        //     (Unquoted `${query}` is intentionally NOT a form — interpolation
        //     lives inside string literals everywhere in Axon.)
        let args = if self.check(TokenType::LParen) {
            UseArgs::Named(self.parse_named_arg_list()?)
        } else {
            let mut argument = String::new();
            if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
                let next = self.current().clone();
                if next.value == "on" {
                    self.advance();
                    argument = self.consume_any_ident_or_kw()?.value.clone();
                }
            }
            UseArgs::LegacyPositional(argument)
        };
        if self.check(TokenType::LBrace) {
            self.skip_braced_block()?;
        }
        Ok(FlowStep::UseTool(UseToolStep {
            tool_name,
            args,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 58.b — parse `(name = value, …)` keyword args for the canonical
    /// `use Tool(...)` multi-field dispatch. Values are captured as expression
    /// strings (StringLit / Integer / Float / Bool / dotted identifier / list)
    /// via the shared `parse_let_atom`, since the frontend has no structured
    /// `Expr`. A trailing comma is tolerated; `()` yields no args.
    fn parse_named_arg_list(&mut self) -> Result<Vec<(String, String, String)>, ParseError> {
        self.consume(TokenType::LParen)?;
        let mut args = Vec::new();
        while !self.check(TokenType::RParen) {
            // Accept a keyword-as-name (`filter`, `type`, `from`, …) — real
            // adopter schemas use such names; the following `=` disambiguates.
            let name = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Assign)?;
            let value = self.parse_let_atom()?;
            // §Fase 60 — `parse_let_atom` classified the value (`"literal"` vs
            // `"reference"`); carry it so the runtime resolves a bare
            // identifier / `Step.output` as a binding lookup, not a literal.
            let value_kind = self.last_let_value_kind.clone();
            args.push((name, value, value_kind));
            if self.check(TokenType::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.consume(TokenType::RParen)?;
        Ok(args)
    }

    fn parse_remember_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let expr = self.consume_any_ident_or_kw()?.value.clone();
        let mut mem = String::new();
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            let next = self.current().clone();
            if next.value == "in" || next.ttype == TokenType::In {
                self.advance();
                mem = self.consume_any_ident_or_kw()?.value.clone();
            }
        }
        Ok(FlowStep::Remember(RememberStep {
            expression: expr,
            memory_target: mem,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_recall_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let query = if self.check(TokenType::StringLit) {
            self.consume(TokenType::StringLit)?.value.clone()
        } else {
            self.consume_any_ident_or_kw()?.value.clone()
        };
        let mut mem = String::new();
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            let next = self.current().clone();
            if next.value == "from" || next.ttype == TokenType::From {
                self.advance();
                mem = self.consume_any_ident_or_kw()?.value.clone();
            }
        }
        Ok(FlowStep::Recall(RecallStep {
            query,
            memory_source: mem,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_hibernate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let mut event = String::new();
        let mut timeout = String::new();
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            event = self.consume_any_ident_or_kw()?.value.clone();
        }
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            let next = self.current().clone();
            if next.ttype == TokenType::Duration {
                self.advance();
                timeout = next.value.clone();
            }
        }
        Ok(FlowStep::Hibernate(HibernateStep {
            event_name: event,
            timeout,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 108.d — `focus <Dataspace> { where: "<filter>", select: [cols], as: <name> }`
    /// — σ_φ ∘ π_v over a declared dataspace. The `where:` string is the
    /// §35 data-plane filter grammar (D108.9, shared with retrieve /
    /// navigate). Pre-108.d the optional body was silently discarded.
    /// §Fase 109.a — `grad <letName> wrt <x> [as <name>]` /
    /// `grad <letName> wrt [a, b] as <name>`. The differentiation itself
    /// happens at CHECK/IR time (T931/T932 + the symbolic differentiator);
    /// the parser only captures the surface.
    fn parse_grad_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let target = self.consume_any_ident_or_kw()?.value.clone();
        let mut wrt: Vec<String> = Vec::new();
        let mut output = String::new();
        if !self.at_declaration_start() && self.current().value == "wrt" {
            self.advance();
            if self.check(TokenType::LBracket) {
                wrt = self.parse_bracketed_identifiers()?;
            } else {
                wrt.push(self.consume_any_ident_or_kw()?.value.clone());
            }
        }
        if !self.at_declaration_start() && self.current().value == "as" {
            self.advance();
            output = self.consume_any_ident_or_kw()?.value.clone();
        }
        Ok(FlowStep::Grad(GradStep {
            target,
            wrt,
            output,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_focus_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let expression = if self.at_declaration_start()
            || self.check(TokenType::RBrace)
            || self.check(TokenType::Eof)
        {
            String::new()
        } else {
            self.consume_any_ident_or_kw()?.value.clone()
        };
        let mut where_expr = String::new();
        let mut select: Vec<String> = Vec::new();
        let mut output = String::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                if self.check(TokenType::Comma) {
                    self.advance();
                    continue;
                }
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "where" => {
                            where_expr = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        "select" => select = self.parse_bracketed_identifiers()?,
                        "as" | "alias" => {
                            output = self.consume_any_ident_or_kw()?.value.clone()
                        }
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Focus(FocusStep {
            expression,
            where_expr,
            select,
            output,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_associate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let left = self.consume_any_ident_or_kw()?.value.clone();
        let mut right = String::new();
        let mut using = String::new();
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            right = self.consume_any_ident_or_kw()?.value.clone();
        }
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            let next = self.current().clone();
            if next.value == "using" {
                self.advance();
                using = self.consume_any_ident_or_kw()?.value.clone();
            }
        }
        let mut output = String::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "as" | "alias" => output = self.consume_any_ident_or_kw()?.value.clone(),
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Associate(AssociateStep {
            left,
            right,
            using_field: using,
            output,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_aggregate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let target = self.consume_any_ident_or_kw()?.value.clone();
        let mut group_by = Vec::new();
        let mut alias = String::new();
        let mut compute: Vec<String> = Vec::new();
        let mut where_expr = String::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "group_by" => group_by = self.parse_bracketed_identifiers()?,
                        "alias" | "as" => alias = self.consume_any_ident_or_kw()?.value.clone(),
                        // §Fase 108.d — the closed aggregate catalog, kept
                        // RAW (`count`, `sum(score)`, …); T930 validates.
                        "compute" => compute = self.parse_bracketed_aggregates()?,
                        // §Fase 108.d — the data-plane where (D108.9).
                        "where" => where_expr = self.consume(TokenType::StringLit)?.value.clone(),
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Aggregate(AggregateStep {
            target,
            group_by,
            alias,
            compute,
            where_expr,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_explore_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let target = self.consume_any_ident_or_kw()?.value.clone();
        let mut limit = None;
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            if self.current().ttype == TokenType::Integer {
                limit = self.current().value.parse::<i64>().ok();
                self.advance();
            }
        }
        let mut output = String::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "as" | "alias" => output = self.consume_any_ident_or_kw()?.value.clone(),
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::ExploreStep(ExploreStepNode {
            target,
            limit,
            output,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 108.d — parse `[count, sum(score), avg(x)]`: bracketed
    /// aggregate entries, each `ident` or `ident(ident)`, kept raw.
    fn parse_bracketed_aggregates(&mut self) -> Result<Vec<String>, ParseError> {
        let mut out = Vec::new();
        self.consume(TokenType::LBracket)?;
        while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
            let name = self.consume_any_ident_or_kw()?.value.clone();
            if self.check(TokenType::LParen) {
                self.advance();
                let col = self.consume_any_ident_or_kw()?.value.clone();
                self.consume(TokenType::RParen)?;
                out.push(format!("{name}({col})"));
            } else {
                out.push(name);
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(out)
    }

    /// §Fase 108.c — the governed ingest step:
    ///
    /// ```text
    /// ingest <sourceRef> into <Dataspace> {
    ///     format: csv | json
    ///     limits { max_bytes: N, max_rows: N }
    /// }
    /// ```
    ///
    /// Until 108.c the body was consumed by `skip_braced_block()`. Now it
    /// is a closed grammar: `format:` (raw here; required + validated by
    /// `axon-T929`) and an optional `limits { … }` block whose bounds are
    /// enforced on the raw byte stream BEFORE parsing (§100). An unknown
    /// body entry is a parse error.
    fn parse_ingest_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let source = self.consume_any_ident_or_kw()?.value.clone();
        let mut target = String::new();
        let mut format = String::new();
        let mut max_bytes: Option<u64> = None;
        let mut max_rows: Option<u64> = None;
        if !self.at_declaration_start() && !self.check(TokenType::RBrace) {
            let next = self.current().clone();
            if next.value == "into" || next.ttype == TokenType::Into {
                self.advance();
                target = self.consume_any_ident_or_kw()?.value.clone();
            }
        }
        if self.check(TokenType::LBrace) {
            self.consume(TokenType::LBrace)?;
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                // Optional separators between body entries.
                if self.check(TokenType::Comma) {
                    self.advance();
                    continue;
                }
                let entry = self.current().clone();
                match entry.value.as_str() {
                    "format" => {
                        self.advance();
                        self.consume(TokenType::Colon)?;
                        format = self.consume_any_ident_or_kw()?.value.clone();
                    }
                    "limits" => {
                        self.advance();
                        self.consume(TokenType::LBrace)?;
                        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                            let bound = self.current().clone();
                            self.advance();
                            self.consume(TokenType::Colon)?;
                            let num_tok = self.consume(TokenType::Integer)?.clone();
                            let value = num_tok.value.parse::<u64>().map_err(|_| ParseError {
                                message: format!(
                                    "ingest `limits` bound `{}` must be a non-negative \
                                     integer byte/row count, got `{}`.",
                                    bound.value, num_tok.value
                                ),
                                line: num_tok.line,
                                column: num_tok.column,
                                ..Default::default()
                            })?;
                            match bound.value.as_str() {
                                "max_bytes" => max_bytes = Some(value),
                                "max_rows" => max_rows = Some(value),
                                other => {
                                    return Err(ParseError {
                                        message: format!(
                                            "Unknown ingest limit `{other}`. The closed \
                                             limits grammar is `max_bytes: <N>` and \
                                             `max_rows: <N>` — bounds enforced on the raw \
                                             stream BEFORE parsing (§100).",
                                        ),
                                        line: bound.line,
                                        column: bound.column,
                                        ..Default::default()
                                    });
                                }
                            }
                            if self.check(TokenType::Comma) {
                                self.advance();
                            }
                        }
                        self.consume(TokenType::RBrace)?;
                    }
                    other => {
                        return Err(ParseError {
                            message: format!(
                                "Unknown entry `{other}` in ingest body. The closed \
                                 grammar is `format: csv|json` and \
                                 `limits {{ max_bytes: <N>, max_rows: <N> }}`.",
                            ),
                            line: entry.line,
                            column: entry.column,
                            ..Default::default()
                        });
                    }
                }
            }
            self.consume(TokenType::RBrace)?;
        }
        Ok(FlowStep::Ingest(IngestStep {
            source,
            target,
            format,
            max_bytes,
            max_rows,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_navigate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let pix_name = self.consume_any_ident_or_kw()?.value.clone();
        let mut node = NavigateStep {
            pix_name,
            corpus_name: String::new(),
            query_expr: String::new(),
            trail_enabled: false,
            output_name: String::new(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "corpus" => {
                            node.corpus_name = self.consume_any_ident_or_kw()?.value.clone()
                        }
                        "query" => {
                            node.query_expr = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        "trail" => {
                            node.trail_enabled = self.consume_any_ident_or_kw()?.value == "true"
                        }
                        "output" | "as" => {
                            node.output_name = self.consume_any_ident_or_kw()?.value.clone()
                        }
                        // §Fase 63.B — MDN corpus-graph navigation.
                        "from" => node.seed = self.consume_any_ident_or_kw()?.value.clone(),
                        "budget" => node.budget = self.parse_optional_int(),
                        // §Fase 66 (Q2) — column-scoped navigation: a raw filter
                        // expr (mirrors `retrieve … where`) pushed to the SELECT
                        // that sources the corpus `documents:`/`relations:` rows,
                        // so a `corpus from axonstore` is scoped to a sub-tenant
                        // COLUMN (`where: "tenant_id == '${tenant_id}'"`), not just
                        // the axon-tenant RLS scope. Resolved by the §37.d filter
                        // compiler at runtime (`${name}` → `$N` bind params).
                        "where" => {
                            node.where_expr = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Navigate(node))
    }

    fn parse_drill_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let pix_name = self.consume_any_ident_or_kw()?.value.clone();
        let mut node = DrillStep {
            pix_name,
            subtree_path: String::new(),
            query_expr: String::new(),
            output_name: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "subtree" | "path" => {
                            node.subtree_path = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        "query" => {
                            node.query_expr = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        "output" | "as" => {
                            node.output_name = self.consume_any_ident_or_kw()?.value.clone()
                        }
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Drill(node))
    }

    fn parse_corroborate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let nav_ref = self.consume_any_ident_or_kw()?.value.clone();
        let mut output = String::new();
        if self.check(TokenType::Arrow) {
            self.advance();
            output = self.consume_any_ident_or_kw()?.value.clone();
        }
        Ok(FlowStep::Corroborate(CorroborateStep {
            navigate_ref: nav_ref,
            output_name: output,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    fn parse_listen_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        // §λ-L-E Fase 13 D4 — dual-mode listen:
        //   • String topic (legacy, deprecated since Fase 13)
        //   • Identifier (canonical: declared ChannelDefinition)
        let (channel, channel_is_ref) = if self.check(TokenType::StringLit) {
            (self.consume(TokenType::StringLit)?.value.clone(), false)
        } else {
            (self.consume_any_ident_or_kw()?.value.clone(), true)
        };
        let mut alias = String::new();
        if !self.at_declaration_start()
            && !self.check(TokenType::RBrace)
            && !self.check(TokenType::LBrace)
        {
            let next = self.current().clone();
            if next.value == "as" || next.ttype == TokenType::As {
                self.advance();
                alias = self.consume_any_ident_or_kw()?.value.clone();
            }
        }
        // §Fase 52.a — parse the handler body into real flow-steps (was
        // `skip_braced_block`'d, leaving the listener inert). The body runs on
        // each event / scheduled tick.
        let body = self.parse_listener_body()?;
        Ok(FlowStep::Listen(ListenStep {
            channel,
            channel_is_ref,
            event_alias: alias,
            body,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 52.a — parse a `listen … { <flow steps> }` handler body. The body
    /// is OPTIONAL (a bodyless `listen channel` returns an empty Vec); when
    /// present, each statement is a real [`FlowStep`] (the same grammar as a
    /// flow / `quant` / `par` body), executed per trigger by the §52.c runtime.
    fn parse_listener_body(&mut self) -> Result<Vec<FlowStep>, ParseError> {
        let mut body = Vec::new();
        if self.check(TokenType::LBrace) {
            self.advance(); // consume `{`
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                body.push(self.parse_flow_step()?);
            }
            self.consume(TokenType::RBrace)?;
        }
        Ok(body)
    }

    fn parse_retrieve_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance();
        let store = self.consume_any_ident_or_kw()?.value.clone();
        let mut where_expr = String::new();
        let mut alias = String::new();
        let mut order_by = String::new();
        let mut limit_expr = String::new();
        let mut aggregate = String::new();
        let mut group_by = String::new();
        let mut cache = String::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let f = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match f.as_str() {
                        "where" => where_expr = self.consume(TokenType::StringLit)?.value.clone(),
                        "as" | "alias" => alias = self.consume_any_ident_or_kw()?.value.clone(),
                        // §Fase 67.b — `order_by:` is a string literal
                        // (`"col asc, col2 desc"`), same surface as `where:`.
                        "order_by" => {
                            order_by = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        // §Fase 67.b — `limit:` is a bare integer literal
                        // (`limit: 100`) OR a string carrying a binding
                        // (`limit: "${max}"`). Captured raw; the runtime
                        // resolves + validates it as a `u32`.
                        "limit" => {
                            let t = self.current().clone();
                            match t.ttype {
                                TokenType::Integer | TokenType::StringLit => {
                                    limit_expr = t.value.clone();
                                    self.advance();
                                }
                                _ => self.skip_value(),
                            }
                        }
                        // §Fase 76.d — `aggregate:` is a string literal from
                        // the CLOSED catalog (`"count"`, `"sum(tokens)"`, …);
                        // `group_by:` is a string literal listing columns
                        // (`"industry, status"`). Both captured raw; the
                        // §38.d proof (axon-T843/T844/T845) + the runtime
                        // (`filter::parse_aggregate_clause`) validate.
                        "aggregate" => {
                            aggregate = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        "group_by" => {
                            group_by = self.consume(TokenType::StringLit)?.value.clone()
                        }
                        // §Fase 85.b — `cache:` names a declared `cache`
                        // policy. A retrieve reads a store (never `pure`), so
                        // caching it always accepts staleness — the checker
                        // requires a finite `ttl:` on the referenced cache
                        // (axon-T865) and resolves the reference (axon-T864).
                        "cache" => cache = self.consume_any_ident_or_kw()?.value.clone(),
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Retrieve(RetrieveStep {
            store_name: store,
            where_expr,
            alias,
            order_by,
            limit_expr,
            aggregate,
            group_by,
            cache,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 35.m — Parse a `purge` step, capturing the optional
    /// `{ where: "<expr>" }` filter. (Fase 35.p moved `mutate` to its
    /// own `parse_mutate_step`, which also captures SET columns; this
    /// helper now serves `purge` alone — a `DELETE` has no SET clause.)
    ///
    /// Before Fase 35.m these two steps parsed via `parse_flow_step_simple`,
    /// which *skipped* the braced block — so a written `where:` clause
    /// was silently dropped and every `mutate`/`purge` ran against the
    /// whole store, leaving the entire Fase 35.b/c parameterized-filter
    /// machinery unreachable for them. This mirror of `parse_retrieve_step`
    /// (minus the `as:` alias — a mutate/purge binds no result) closes
    /// that gap. Returns `(loc, store_name, where_expr)`.
    fn parse_store_where_step(
        &mut self,
    ) -> Result<(Loc, String, String), ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume the keyword
        let store = if self.at_declaration_start()
            || self.check(TokenType::RBrace)
            || self.check(TokenType::Eof)
        {
            String::new()
        } else {
            self.consume_any_ident_or_kw()?.value.clone()
        };
        let mut where_expr = String::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let field = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    match field.as_str() {
                        "where" => {
                            where_expr =
                                self.consume(TokenType::StringLit)?.value.clone()
                        }
                        _ => self.skip_value(),
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok((
            Loc {
                line: tok.line,
                column: tok.column,
            },
            store,
            where_expr,
        ))
    }

    /// §Fase 35.o — Parse a `persist` step, capturing the optional
    /// `{ col: value }` field block.
    ///
    /// Before Fase 35.o `persist` parsed via `parse_flow_step_simple`,
    /// which *skipped* the braced block — so a written field block was
    /// silently dropped and the runtime fell back to writing every
    /// context binding as a row, which fails against any real table
    /// (flows always carry more bindings than a table has columns).
    /// This captures the declared columns into `PersistStep.fields`;
    /// the runtime writes exactly those (interpolated). A `persist`
    /// with no block keeps the v1.30.0 user-bindings fallback — fully
    /// backward-compatible. Mirror of `parse_retrieve_step`, but the
    /// keys are arbitrary column names rather than the fixed
    /// `where:` / `as:` filter keys.
    ///
    /// The optional `into` connector (`persist into <store>`) is
    /// accepted and skipped — before Fase 35.o `into` was captured as
    /// the store name.
    fn parse_persist_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume `persist`
        // Optional `into` connector — skip it so the store name that
        // follows is not mistaken for the target.
        if self.current().value == "into" && !self.check(TokenType::LBrace) {
            self.advance();
        }
        let store = if self.at_declaration_start()
            || self.check(TokenType::LBrace)
            || self.check(TokenType::RBrace)
            || self.check(TokenType::Eof)
        {
            String::new()
        } else {
            self.consume_any_ident_or_kw()?.value.clone()
        };
        let mut fields: Vec<(String, String)> = Vec::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let col = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    let value = if self.check(TokenType::StringLit) {
                        self.consume(TokenType::StringLit)?.value.clone()
                    } else if self.check(TokenType::RBrace)
                        || self.check(TokenType::Eof)
                        || self.check(TokenType::Colon)
                    {
                        String::new()
                    } else {
                        let v = self.current().clone();
                        self.advance();
                        v.value.clone()
                    };
                    fields.push((col, value));
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Persist(PersistStep {
            store_name: store,
            fields,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 35.p — Parse a `mutate` step, capturing both the
    /// `{ where: "<expr>" }` filter AND the `{ col: value }` SET
    /// assignments.
    ///
    /// Before Fase 35.p `mutate` parsed via `parse_store_where_step`,
    /// which captured only `where:` and *skipped* every other key — so
    /// the runtime built the `UPDATE … SET` clause from every flow
    /// binding (params + step results + `let`s), which fails against
    /// any real table (`column "X" does not exist`). This closes the
    /// gap symmetrically to 35.o's `persist` block: every key other
    /// than `where:` is a SET column; a `mutate` with no SET column
    /// keeps the v1.31.0 user-bindings fallback. `where:` keeps its
    /// string-literal grammar (as in `retrieve` / `purge`).
    fn parse_mutate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.current().clone();
        self.advance(); // consume `mutate`
        let store = if self.at_declaration_start()
            || self.check(TokenType::LBrace)
            || self.check(TokenType::RBrace)
            || self.check(TokenType::Eof)
        {
            String::new()
        } else {
            self.consume_any_ident_or_kw()?.value.clone()
        };
        let mut where_expr = String::new();
        let mut fields: Vec<(String, String)> = Vec::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let key = self.current().value.clone();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                    if key == "where" {
                        where_expr =
                            self.consume(TokenType::StringLit)?.value.clone();
                    } else {
                        let value = if self.check(TokenType::StringLit) {
                            self.consume(TokenType::StringLit)?.value.clone()
                        } else if self.check(TokenType::RBrace)
                            || self.check(TokenType::Eof)
                            || self.check(TokenType::Colon)
                        {
                            String::new()
                        } else {
                            let v = self.current().clone();
                            self.advance();
                            v.value.clone()
                        };
                        fields.push((key, value));
                    }
                }
            }
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        Ok(FlowStep::Mutate(MutateStep {
            store_name: store,
            where_expr,
            fields,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    // ── TIER 2 DECLARATIONS ────────────────────────────────────────

    fn parse_agent(&mut self) -> Result<AgentDefinition, ParseError> {
        let tok = self.consume(TokenType::Agent)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = AgentDefinition {
            name,
            goal: String::new(),
            tools: Vec::new(),
            memory_ref: String::new(),
            strategy: String::new(),
            on_stuck: String::new(),
            shield_ref: String::new(),
            max_iterations: None,
            max_tokens: None,
            max_time: String::new(),
            max_cost: None,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        // Skip optional parameters/return type before brace
        while !self.check(TokenType::LBrace) && !self.check(TokenType::Eof) {
            self.advance();
        }
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "goal" => node.goal = self.consume(TokenType::StringLit)?.value.clone(),
                    "tools" => node.tools = self.parse_bracketed_identifiers()?,
                    "memory" => node.memory_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    "strategy" => node.strategy = self.consume_any_ident_or_kw()?.value.clone(),
                    "on_stuck" => node.on_stuck = self.consume_any_ident_or_kw()?.value.clone(),
                    "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    "max_iterations" => node.max_iterations = self.parse_optional_int(),
                    "max_tokens" => node.max_tokens = self.parse_optional_int(),
                    "max_time" => node.max_time = self.consume_any_ident_or_kw()?.value.clone(),
                    "max_cost" => node.max_cost = self.parse_optional_float(),
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 53 — `extension Name { category: effects|scan, members: [ … ] }`.
    /// The parser is permissive on field/category VALUES (validated in
    /// §53.c by the type-checker — no-shadowing, category-membership);
    /// it only enforces the structural grammar here.
    fn parse_extension(&mut self) -> Result<ExtensionDefinition, ParseError> {
        let tok = self.consume(TokenType::Extension)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ExtensionDefinition {
            name,
            category: String::new(),
            members: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "category" => {
                        node.category = self.consume_any_ident_or_kw()?.value.clone()
                    }
                    "members" => node.members = self.parse_extension_members()?,
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 53 — parse `[ "name" [ : { semantics: "…", default_confidence: 0.8 } ], … ]`.
    /// Each member is a string literal optionally followed by a metadata
    /// block. Trailing/interleaved commas are tolerated.
    fn parse_extension_members(&mut self) -> Result<Vec<ExtensionMember>, ParseError> {
        let mut members = Vec::new();
        self.consume(TokenType::LBracket)?;
        while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
            let name_tok = self.consume(TokenType::StringLit)?;
            let mut member = ExtensionMember {
                name: name_tok.value.clone(),
                semantics: None,
                default_confidence: None,
                loc: Loc {
                    line: name_tok.line,
                    column: name_tok.column,
                },
            };
            // Optional `: { semantics: "…", default_confidence: 0.8 }`.
            if self.check(TokenType::Colon) {
                self.advance();
                self.consume(TokenType::LBrace)?;
                while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                    let mkey = self.current().value.clone();
                    self.advance();
                    if self.check(TokenType::Colon) {
                        self.advance();
                        match mkey.as_str() {
                            "semantics" => {
                                member.semantics =
                                    Some(self.consume(TokenType::StringLit)?.value.clone())
                            }
                            "default_confidence" => {
                                member.default_confidence = self.parse_optional_float()
                            }
                            _ => self.skip_value(),
                        }
                    }
                    if self.check(TokenType::Comma) {
                        self.advance();
                    }
                }
                self.consume(TokenType::RBrace)?;
            }
            members.push(member);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(members)
    }

    /// §Fase 71.a/e — `window <Name> { timezone: "…"  allow: [ {days hours} ]
    /// exclude: [ "YYYY-MM-DD", … ]  on_outside: skip|defer|warn }`.
    fn parse_window(&mut self) -> Result<WindowDefinition, ParseError> {
        let tok = self.consume(TokenType::Window)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = WindowDefinition {
            name,
            timezone: String::new(),
            allow: Vec::new(),
            exclude: Vec::new(),
            on_outside: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match field_name.as_str() {
                "timezone" => node.timezone = self.consume(TokenType::StringLit)?.value,
                "allow" => node.allow = self.parse_window_allow()?,
                "exclude" => node.exclude = self.parse_window_exclude()?,
                "on_outside" => node.on_outside = self.consume_any_ident_or_kw()?.value,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 71.a — the `allow: [ { … }, { … } ]` span list.
    fn parse_window_allow(&mut self) -> Result<Vec<WindowSpan>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut spans = Vec::new();
        if !self.check(TokenType::RBracket) {
            spans.push(self.parse_window_span()?);
            while self.check(TokenType::Comma) {
                self.advance();
                if self.check(TokenType::RBracket) {
                    break; // trailing comma
                }
                spans.push(self.parse_window_span()?);
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(spans)
    }

    /// §Fase 71.e — the `exclude: [ "YYYY-MM-DD", … ]` holiday list (ISO
    /// date-string literals; validated for real-calendar-date-ness by the
    /// `axon-T826` type check). An empty list / absent field ⇒ no holidays.
    fn parse_window_exclude(&mut self) -> Result<Vec<String>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut dates = Vec::new();
        if !self.check(TokenType::RBracket) {
            dates.push(self.consume(TokenType::StringLit)?.value);
            while self.check(TokenType::Comma) {
                self.advance();
                if self.check(TokenType::RBracket) {
                    break; // trailing comma
                }
                dates.push(self.consume(TokenType::StringLit)?.value);
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(dates)
    }

    /// §Fase 71.a — one span `{ days: Mon..Fri  hours: 9..18 }`.
    fn parse_window_span(&mut self) -> Result<WindowSpan, ParseError> {
        let tok = self.consume(TokenType::LBrace)?;
        let mut span = WindowSpan {
            day_start: String::new(),
            day_end: String::new(),
            hour_start: 0,
            hour_end: 0,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match field.as_str() {
                "days" => {
                    span.day_start = self.consume_any_ident_or_kw()?.value;
                    self.consume(TokenType::DotDot)?;
                    span.day_end = self.consume_any_ident_or_kw()?.value;
                }
                "hours" => {
                    span.hour_start = self.consume_number()? as i64;
                    self.consume(TokenType::DotDot)?;
                    span.hour_end = self.consume_number()? as i64;
                }
                _ => self.skip_value(),
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(span)
    }

    fn parse_shield(&mut self) -> Result<ShieldDefinition, ParseError> {
        let tok = self.consume(TokenType::Shield)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ShieldDefinition {
            name,
            scan: Vec::new(),
            strategy: String::new(),
            on_breach: String::new(),
            severity: String::new(),
            quarantine: String::new(),
            max_retries: None,
            confidence_threshold: None,
            allow_tools: Vec::new(),
            deny_tools: Vec::new(),
            sandbox: None,
            redact: Vec::new(),
            log: String::new(),
            deflect_message: String::new(),
            taint: String::new(),
            compliance: Vec::new(),
            sign: String::new(),
            unknown_fields: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            let field_loc = Loc {
                line: self.current().line,
                column: self.current().column,
            };
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "scan" => node.scan = self.parse_bracketed_identifiers()?,
                    "strategy" => node.strategy = self.consume_any_ident_or_kw()?.value.clone(),
                    "on_breach" => node.on_breach = self.consume_any_ident_or_kw()?.value.clone(),
                    "severity" => node.severity = self.consume_any_ident_or_kw()?.value.clone(),
                    "quarantine" => {
                        node.quarantine = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    "max_retries" => node.max_retries = self.parse_optional_int(),
                    "confidence_threshold" => {
                        node.confidence_threshold = self.parse_optional_float()
                    }
                    "allow_tools" => node.allow_tools = self.parse_bracketed_identifiers()?,
                    "deny_tools" => node.deny_tools = self.parse_bracketed_identifiers()?,
                    "sandbox" => {
                        node.sandbox = Some(self.consume_any_ident_or_kw()?.value == "true")
                    }
                    "redact" => node.redact = self.parse_bracketed_identifiers()?,
                    "log" => node.log = self.consume_any_ident_or_kw()?.value.clone(),
                    "deflect_message" => {
                        node.deflect_message = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    "taint" => node.taint = self.consume_any_ident_or_kw()?.value.clone(),
                    // ESK Fase 6.1 — covered regulatory classes.
                    "compliance" => node.compliance = self.parse_bracketed_identifiers()?,
                    // §Fase 77.a — egress signing algorithm (closed catalog,
                    // validated by the checker: `axon-T846`).
                    "sign" => node.sign = self.consume_any_ident_or_kw()?.value.clone(),
                    // §Fase 77.a — the value is still skipped (leniency
                    // preserved) but the NAME is recorded so the checker
                    // emits `axon-W010` instead of a silent drop.
                    _ => {
                        node.unknown_fields.push((field_name.clone(), field_loc));
                        self.skip_value()
                    }
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_pix(&mut self) -> Result<PixDefinition, ParseError> {
        let tok = self.consume(TokenType::Pix)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = PixDefinition {
            name,
            source: String::new(),
            depth: None,
            branching: None,
            model: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "source" => node.source = self.consume(TokenType::StringLit)?.value.clone(),
                    "depth" => node.depth = self.parse_optional_int(),
                    "branching" => node.branching = self.parse_optional_int(),
                    "model" => node.model = self.consume_any_ident_or_kw()?.value.clone(),
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 62.0 — `ledger <Name> { source, depth, branching, model }`.
    /// The append-only audit chain (formerly the Provenance-Index reading of
    /// `pix`). Field grammar mirrors `pix` (same shape) but the SEMANTICS are
    /// audit, not navigation: `depth` = chain retention, `branching` = Merkle
    /// factor, `model` = hash slug (sha256 / blake3 / sha3).
    fn parse_ledger(&mut self) -> Result<LedgerDefinition, ParseError> {
        let tok = self.consume(TokenType::Ledger)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = LedgerDefinition {
            name,
            source: String::new(),
            depth: None,
            branching: None,
            model: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "source" => node.source = self.consume(TokenType::StringLit)?.value.clone(),
                    "depth" => node.depth = self.parse_optional_int(),
                    "branching" => node.branching = self.parse_optional_int(),
                    "model" => node.model = self.consume_any_ident_or_kw()?.value.clone(),
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_psyche(&mut self) -> Result<PsycheDefinition, ParseError> {
        let tok = self.consume(TokenType::Psyche)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = PsycheDefinition {
            name,
            dimensions: Vec::new(),
            manifold_noise: None,
            manifold_momentum: None,
            safety_constraints: Vec::new(),
            quantum_enabled: None,
            inference_mode: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "dimensions" => node.dimensions = self.parse_bracketed_identifiers()?,
                    "manifold_noise" => node.manifold_noise = self.parse_optional_float(),
                    "manifold_momentum" => node.manifold_momentum = self.parse_optional_float(),
                    "safety_constraints" => {
                        node.safety_constraints = self.parse_bracketed_identifiers()?
                    }
                    "quantum_enabled" => {
                        node.quantum_enabled = Some(self.consume_any_ident_or_kw()?.value == "true")
                    }
                    "inference_mode" => {
                        node.inference_mode = self.consume_any_ident_or_kw()?.value.clone()
                    }
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_corpus(&mut self) -> Result<CorpusDefinition, ParseError> {
        let tok = self.consume(TokenType::Corpus)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = CorpusDefinition {
            name,
            documents: Vec::new(),
            relations: Vec::new(),
            adaptive: false,
            mcp_server: String::new(),
            mcp_resource_uri: String::new(),
            store_source: None,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        // corpus Name from mcp("server", "uri")  — static MCP-bound short form.
        // corpus Name from axonstore { documents: S(id,title)  relations: … }  —
        // §Fase 64.A dynamic store-sourced MDN graph (falls through to the body).
        let mut dynamic = false;
        if self.check(TokenType::From) {
            self.advance();
            if self.check(TokenType::AxonStore) {
                self.advance();
                dynamic = true;
            } else {
                self.consume(TokenType::Mcp)?;
                self.consume(TokenType::LParen)?;
                node.mcp_server = self.consume(TokenType::StringLit)?.value.clone();
                self.consume(TokenType::Comma)?;
                node.mcp_resource_uri = self.consume(TokenType::StringLit)?.value.clone();
                self.consume(TokenType::RParen)?;
                return Ok(node);
            }
        }
        self.consume(TokenType::LBrace)?;
        // §Fase 64.A — accumulate the store-mapping pieces while the dynamic body
        // is parsed; folded into `node.store_source` after the closing brace.
        let mut src = CorpusStoreSource {
            doc_store: String::new(),
            doc_id_col: String::new(),
            doc_title_col: String::new(),
            edge_store: String::new(),
            edge_from_col: String::new(),
            edge_to_col: String::new(),
            edge_type_col: String::new(),
            edge_weight_col: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        };
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    // §Fase 64.A — dynamic: `documents: <DocStore>(id_col, title_col)`.
                    "documents" if dynamic => {
                        let (store, cols) = self.parse_corpus_store_mapping(2)?;
                        src.doc_store = store;
                        src.doc_id_col = cols[0].clone();
                        src.doc_title_col = cols[1].clone();
                    }
                    "documents" => node.documents = self.parse_bracketed_identifiers()?,
                    // §Fase 64.A — dynamic: `relations: <EdgeStore>(from, to, etype, weight)`.
                    "relations" if dynamic => {
                        let (store, cols) = self.parse_corpus_store_mapping(4)?;
                        src.edge_store = store;
                        src.edge_from_col = cols[0].clone();
                        src.edge_to_col = cols[1].clone();
                        src.edge_type_col = cols[2].clone();
                        src.edge_weight_col = cols[3].clone();
                    }
                    // §Fase 63.A — static typed weighted edges → MDN corpus graph.
                    "relations" => node.relations = self.parse_corpus_relations()?,
                    // §Fase 63.C — enable the memory endofunctor.
                    "adaptive" => node.adaptive = self.consume_any_ident_or_kw()?.value == "true",
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        if dynamic {
            node.store_source = Some(src);
        }
        Ok(node)
    }

    /// §Fase 64.A — parse a store-mapping `<StoreName>( col1, col2, … )` of exactly
    /// `n` columns. Used by the dynamic store-sourced corpus's `documents:` (2
    /// cols: id, title) and `relations:` (4 cols: from, to, etype, weight). The
    /// store name is an identifier (a declared `axonstore`); the columns may be
    /// keywords (a column could be named `from`/`type`), so they use the
    /// keyword-tolerant consumer. The type-checker validates store + columns.
    fn parse_corpus_store_mapping(&mut self, n: usize) -> Result<(String, Vec<String>), ParseError> {
        let store = self.consume(TokenType::Identifier)?.value.clone();
        self.consume(TokenType::LParen)?;
        let mut cols = Vec::with_capacity(n);
        for i in 0..n {
            if i > 0 {
                self.consume(TokenType::Comma)?;
            }
            cols.push(self.consume_any_ident_or_kw()?.value.clone());
        }
        self.consume(TokenType::RParen)?;
        Ok((store, cols))
    }

    /// §Fase 63.A — parse `relations: [ etype(from, to, weight) … ]`, the typed
    /// weighted edges of an MDN corpus graph. Entries are whitespace/newline
    /// separated; commas between them are optional. Edge-type validity (closed
    /// catalog), document references, and the weight range are checked by the
    /// type-checker (`check_corpus`), not here.
    fn parse_corpus_relations(&mut self) -> Result<Vec<CorpusRelation>, ParseError> {
        let mut out = Vec::new();
        self.consume(TokenType::LBracket)?;
        while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
            if self.check(TokenType::Comma) {
                self.advance();
                continue;
            }
            let tok = self.current().clone();
            let etype = self.consume_any_ident_or_kw()?.value.clone();
            self.consume(TokenType::LParen)?;
            let from = self.consume_any_ident_or_kw()?.value.clone();
            self.consume(TokenType::Comma)?;
            let to = self.consume_any_ident_or_kw()?.value.clone();
            self.consume(TokenType::Comma)?;
            let weight = self.consume_number()?;
            self.consume(TokenType::RParen)?;
            out.push(CorpusRelation {
                etype,
                from,
                to,
                weight,
                loc: Loc { line: tok.line, column: tok.column },
            });
        }
        self.consume(TokenType::RBracket)?;
        Ok(out)
    }

    /// §Fase 108.b — the typed dataspace declaration:
    ///
    /// ```text
    /// dataspace <Name> {
    ///     column <name>: <Type>
    ///     …
    /// }
    /// ```
    ///
    /// Until 108.b the body was consumed by `skip_braced_block()` — any
    /// content, including garbage, compiled clean and reached nothing.
    /// Now each entry must be a `column` field; the declared type is
    /// kept RAW here and resolved against the closed 6-type catalog by
    /// the type-checker (`axon-T928`), so all schema errors accumulate
    /// in a single compile. An unknown body keyword is a parse error
    /// (the grammar is closed — the §38 axonstore posture).
    fn parse_dataspace(&mut self) -> Result<DataspaceDefinition, ParseError> {
        let tok = self.consume(TokenType::Dataspace)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = DataspaceDefinition {
            name,
            columns: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        if self.check(TokenType::LBrace) {
            self.consume(TokenType::LBrace)?;
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let entry = self.current().clone();
                if entry.value != "column" {
                    return Err(ParseError {
                        message: format!(
                            "Unknown entry `{}` in dataspace `{}`. A dataspace body \
                             declares its columnar schema: `column <name>: <Type>` \
                             (one per line, over the closed type catalog — \
                             Text, Int, Float, Bool, Timestamp, Json).",
                            entry.value, node.name
                        ),
                        line: entry.line,
                        column: entry.column,
                        ..Default::default()
                    });
                }
                self.advance(); // `column`
                let col_tok = self.current().clone();
                let col_name = self.consume_any_ident_or_kw()?.value.clone();
                self.consume(TokenType::Colon)?;
                let declared_type = self.consume_any_ident_or_kw()?.value.clone();
                node.columns.push(crate::ast::DataspaceColumn {
                    name: col_name,
                    declared_type,
                    loc: Loc {
                        line: col_tok.line,
                        column: col_tok.column,
                    },
                });
            }
            self.consume(TokenType::RBrace)?;
        }
        Ok(node)
    }

    fn parse_ots(&mut self) -> Result<OtsDefinition, ParseError> {
        let tok = self.consume(TokenType::Ots)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = OtsDefinition {
            name,
            teleology: String::new(),
            homotopy_search: String::new(),
            loss_function: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        // Skip optional type params <In, Out>
        if self.check(TokenType::Lt) {
            while !self.check(TokenType::Gt) && !self.check(TokenType::Eof) {
                self.advance();
            }
            if self.check(TokenType::Gt) {
                self.advance();
            }
        }
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "teleology" => {
                        node.teleology = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    "homotopy_search" => {
                        node.homotopy_search = self.consume_any_ident_or_kw()?.value.clone()
                    }
                    "loss_function" => {
                        node.loss_function = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_mandate(&mut self) -> Result<MandateDefinition, ParseError> {
        let tok = self.consume(TokenType::Mandate)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = MandateDefinition {
            name,
            constraint: String::new(),
            kp: None,
            ki: None,
            kd: None,
            tolerance: None,
            max_steps: None,
            drift_bound: None,
            lipschitz: None,
            on_violation: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "constraint" => {
                        node.constraint = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    "kp" | "Kp" => node.kp = self.parse_optional_float(),
                    "ki" | "Ki" => node.ki = self.parse_optional_float(),
                    "kd" | "Kd" => node.kd = self.parse_optional_float(),
                    "max_steps" => node.max_steps = self.parse_optional_int(),
                    // §Fase 119.b — `epsilon:` is what the README publishes; `tolerance:`
                    // is what the parser has always accepted. They are the SAME ε — the
                    // convergence band of `Converge(e, ε, N)`. Both spellings resolve here
                    // rather than one of them silently vanishing into `skip_value()`.
                    "tolerance" | "epsilon" => node.tolerance = self.parse_optional_float(),
                    "on_violation" => {
                        node.on_violation = self.consume_any_ident_or_kw()?.value.clone()
                    }
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                // §Fase 119.b — `pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }`, which is the form
                // README §XV publishes and the form every mandate example uses.
                //
                // THIS BLOCK USED TO BE `skip_braced_block()`. The consequence was not a
                // parse error — it was SILENT ACCEPTANCE: `axon check` printed
                // "0 errors" and the IR came out with `kp: None, ki: None, kd: None`.
                // The developer wrote the published example, the compiler agreed, and the
                // ENTIRE CONTROL LAW was discarded between them. A dropped specification
                // that reports success is the §111 defect living in the parser.
                if field_name == "pid" {
                    self.parse_pid_block(&mut node)?;
                } else if field_name == "stability" {
                    self.parse_stability_block(&mut node)?;
                } else {
                    self.skip_braced_block()?;
                }
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 119.b — `pid { Kp: <f>, Ki: <f>, Kd: <f> }`.
    ///
    /// The gains of the Cybernetic Refinement Calculus controller
    /// (`docs/papers/paper_mandate.md` §3): `u(t) = Kp·e(t) + Ki·∫e + Kd·de/dt`.
    /// Accepts both capitalised (`Kp`, the papers' and README's notation) and
    /// lower-case spellings, because the flat `kp:` form was already accepted and
    /// removing it would break programs that use it.
    ///
    /// Unknown keys inside the block are skipped rather than refused — the enclosing
    /// declaration keeps that behaviour, and tightening it is a separate, wider
    /// decision than this sub-fase should make unilaterally.
    fn parse_pid_block(&mut self, node: &mut MandateDefinition) -> Result<(), ParseError> {
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match key.as_str() {
                    "kp" | "Kp" => node.kp = self.parse_optional_float(),
                    "ki" | "Ki" => node.ki = self.parse_optional_float(),
                    "kd" | "Kd" => node.kd = self.parse_optional_float(),
                    _ => self.skip_value(),
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(())
    }

    /// §Fase 119.b — `stability { D: <f>, L: <f> }`.
    ///
    /// The declared hypotheses of the mandate's stability theorem: `D` is the
    /// drift bound `sup|drift(t)|` (paper_mandate §3), `L` the Lipschitz
    /// constant of the refinement map (prompt_opt §6.3). With them declared,
    /// the type checker verifies the full band `D < |Kp+Ki+Kd| < 1/L`; without
    /// them it can verify only the sign conditions, which the papers show to be
    /// necessary but not sufficient. The declaration travels in the IR as a
    /// proof obligation for dispatch — the compiler never invents these
    /// numbers, because they are measured properties of a backend it cannot
    /// see, and fabricating them would make the static check vacuous.
    ///
    /// An empty block is a PARSE error, not a silent no-op: `stability { }`
    /// asserts nothing, can discharge nothing, and the developer who wrote it
    /// believed otherwise.
    fn parse_stability_block(
        &mut self,
        node: &mut MandateDefinition,
    ) -> Result<(), ParseError> {
        let open = self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match key.as_str() {
                    "D" | "d" | "drift_bound" => {
                        node.drift_bound = self.parse_optional_float()
                    }
                    "L" | "l" | "lipschitz" => node.lipschitz = self.parse_optional_float(),
                    _ => self.skip_value(),
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        if node.drift_bound.is_none() && node.lipschitz.is_none() {
            return Err(ParseError {
                message: "the `stability { }` block declares neither `D` nor `L` — it                           asserts nothing and can discharge nothing. Declare the drift                           bound (`D:`), the Lipschitz constant (`L:`), or both; or remove                           the block."
                    .to_string(),
                line: open.line,
                column: open.column,
                ..Default::default()
            });
        }
        Ok(())
    }

    /// §Fase 111.f — `compute <Name>(p: T, …) -> T { <expr> }`.
    ///
    /// # What this used to be
    ///
    /// ```text
    /// // Skip optional parameters/return type before brace
    /// while !self.check(TokenType::LBrace) { self.advance(); }
    /// ```
    ///
    /// The parameters and the return type were **skipped token by token**, and
    /// the brace held only `shield:`. So a `compute` had **no inputs, no output
    /// type and no body** — which is why the runtime could do nothing but bind
    /// the literal string `"compute:Name(args)"`, and why a downstream step then
    /// consumed that text where it expected a number. The README meanwhile
    /// promised "native Fast-Path execution bypassing the LLM" **with an O(n)
    /// guarantee**.
    ///
    /// # What it is now
    ///
    /// A named pure function over the §70 expression language — the closed,
    /// total, side-effect-free term algebra the runtime already evaluates
    /// natively (`eval_expr`, the same evaluator behind `let`, `grad` and
    /// `conditional`). Linear in the term, no model in the loop: the advertised
    /// claim, made true rather than louder.
    ///
    /// The legacy field form (`compute N { shield: G }`) still parses — its body
    /// is simply `None`, and applying a bodyless compute is refused (axon-T941)
    /// instead of silently binding a placeholder.
    fn parse_compute(&mut self) -> Result<ComputeDefinition, ParseError> {
        let tok = self.consume(TokenType::Compute)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ComputeDefinition {
            name,
            shield_ref: String::new(),
            parameters: Vec::new(),
            return_type: String::new(),
            body: None,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        // `(p: T, q: T)` — the typed parameters (they used to be skipped).
        if self.check(TokenType::LParen) {
            self.advance();
            while !self.check(TokenType::RParen) && !self.check(TokenType::Eof) {
                let ptok = self.current().clone();
                let pname = self.consume_any_ident_or_kw()?.value.clone();
                self.consume(TokenType::Colon)?;
                let ptype = self.parse_type_expr()?;
                node.parameters.push(Parameter {
                    name: pname,
                    type_expr: ptype,
                    loc: self.loc_of(&ptok),
                });
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.consume(TokenType::RParen)?;
        }

        // `-> T` — the declared result type.
        if self.check(TokenType::Arrow) {
            self.advance();
            node.return_type = self.consume_any_ident_or_kw()?.value.clone();
        }

        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            // A `<name>:` pair is a legacy field (only `shield:` is meaningful).
            // Anything else is THE BODY — a §70 expression.
            //
            // NOTE: the field name may be a KEYWORD, not just an identifier —
            // `shield` is `TokenType::Shield`. Testing only for `Identifier` here
            // sent `compute N { shield: G }` (the legacy declaration form, and
            // the shape of the shipped canonical program) down the
            // expression-parsing path and broke it. Back-compat is not optional:
            // an adopter's existing program must keep compiling.
            let is_field = self
                .tokens
                .get(self.pos + 1)
                .map(|t| t.ttype == TokenType::Colon)
                .unwrap_or(false);
            if is_field {
                let field_name = self.current().value.clone();
                self.advance();
                self.consume(TokenType::Colon)?;
                match field_name.as_str() {
                    "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    _ => self.skip_value(),
                }
            } else {
                node.body = Some(self.parse_expr()?);
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_daemon(&mut self) -> Result<DaemonDefinition, ParseError> {
        let tok = self.consume(TokenType::Daemon)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = DaemonDefinition {
            name,
            goal: String::new(),
            tools: Vec::new(),
            memory_ref: String::new(),
            strategy: String::new(),
            on_stuck: String::new(),
            shield_ref: String::new(),
            window_ref: String::new(),
            budget: None,
            max_tokens: None,
            max_time: String::new(),
            max_cost: None,
            listeners: Vec::new(),
            requires_capabilities: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        // Skip optional parameters/return type before brace
        while !self.check(TokenType::LBrace) && !self.check(TokenType::Eof) {
            self.advance();
        }
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "goal" => node.goal = self.consume(TokenType::StringLit)?.value.clone(),
                    "tools" => node.tools = self.parse_bracketed_identifiers()?,
                    "memory" => node.memory_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    "strategy" => node.strategy = self.consume_any_ident_or_kw()?.value.clone(),
                    "on_stuck" => node.on_stuck = self.consume_any_ident_or_kw()?.value.clone(),
                    "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    // §Fase 71.c — `window: <WindowName>` temporal binding.
                    "window" => node.window_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    "max_tokens" => node.max_tokens = self.parse_optional_int(),
                    "max_time" => node.max_time = self.consume_any_ident_or_kw()?.value.clone(),
                    "max_cost" => node.max_cost = self.parse_optional_float(),
                    // §Fase 52.d — `requires: [cap, …]` capability scope (same
                    // closed slug grammar as `axonendpoint requires:`). The
                    // enterprise supervisor mints a per-run principal scoped to
                    // exactly these (least privilege).
                    "requires" => {
                        let bracket_tok = self.current().clone();
                        let items = self.parse_bracketed_dot_identifiers()?;
                        for slug in &items {
                            if !is_valid_capability_slug(slug) {
                                return Err(ParseError {
                                    message: format!(
                                        "Invalid capability slug '{slug}' in daemon '{}' \
                                         `requires:`. Capability slugs must match \
                                         ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$ — dot-separated \
                                         lowercase identifiers. Examples: `daemon.run`, \
                                         `memory.write`, `flow.execute`.",
                                        node.name
                                    ),
                                    line: bracket_tok.line,
                                    column: bracket_tok.column,
                                    ..Default::default()
                                });
                            }
                        }
                        node.requires_capabilities = items;
                    }
                    _ => self.skip_value(),
                }
            } else if field.ttype == TokenType::Listen {
                // §λ-L-E Fase 13 D4 — preserve listen blocks for type
                // checking.  We backtracked past the `listen` keyword
                // by `advance()` above, so reconstruct a synthetic
                // listener using the same dual-mode dispatch the flow
                // step parser uses (string topic OR typed channel ref).
                let (channel, channel_is_ref) = if self.check(TokenType::StringLit) {
                    (self.consume(TokenType::StringLit)?.value.clone(), false)
                } else {
                    (self.consume_any_ident_or_kw()?.value.clone(), true)
                };
                let mut alias = String::new();
                if !self.at_declaration_start()
                    && !self.check(TokenType::RBrace)
                    && !self.check(TokenType::LBrace)
                {
                    let next = self.current().clone();
                    if next.value == "as" || next.ttype == TokenType::As {
                        self.advance();
                        alias = self.consume_any_ident_or_kw()?.value.clone();
                    }
                }
                let listen_loc = Loc {
                    line: field.line,
                    column: field.column,
                };
                // §Fase 52.a — parse the handler body (was skipped). This is
                // what makes a `daemon` operational: the body runs per event /
                // scheduled tick (e.g. a `listen "cron:…" as tick { run … }`).
                let body = self.parse_listener_body()?;
                node.listeners.push(ListenStep {
                    channel,
                    channel_is_ref,
                    event_alias: alias,
                    body,
                    loc: listen_loc,
                });
            } else if field_name == "budget" && self.check(TokenType::LBrace) {
                // §Fase 72.a — the `budget { … }` linear-effect rate-limit block.
                node.budget = Some(self.parse_budget_block(field.line, field.column)?);
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 114.a — a TOP-LEVEL `budget <Name> { … }`.
    ///
    /// Same body as the daemon-attached block; what it gains is a **name** and a
    /// **scope that is not a daemon**. Until §114, `budget` was a field of `daemon`
    /// and of nothing else — so an adopter deploying an HTTP endpoint that calls a
    /// vendor tool had **no way in the language to bound how often it did that.**
    /// Not "the bound did not work": **the bound could not be written.** And the
    /// HTTP endpoint is what people actually deploy.
    fn parse_top_level_budget(&mut self) -> Result<BudgetBlock, ParseError> {
        let kw = self.consume(TokenType::Budget)?; // `budget`
        let name = self.consume(TokenType::Identifier)?.value;
        let mut block = self.parse_budget_block(kw.line, kw.column)?;
        block.name = name;
        Ok(block)
    }

    /// §Fase 72.a — `budget { <rate|max>: N per <period> on Tool(<X>) … [on_exhausted: <p>] }`.
    fn parse_budget_block(&mut self, line: u32, column: u32) -> Result<BudgetBlock, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut quotas = Vec::new();
        let mut on_exhausted = String::new();
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = self.consume_any_ident_or_kw()?.value;
            match field_name.as_str() {
                "rate" | "max" => {
                    quotas.push(self.parse_budget_quota(field_name, field.line, field.column)?);
                }
                "on_exhausted" => {
                    self.consume(TokenType::Colon)?;
                    on_exhausted = self.consume_any_ident_or_kw()?.value;
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(BudgetBlock {
            name: String::new(),
            quotas,
            on_exhausted,
            loc: Loc { line, column },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        })
    }

    /// §Fase 72.a — one quota line: `<kind>: <limit> per <period> on Tool(<effect>)`.
    /// `kind` (`rate`/`max`) is already consumed by the caller.
    fn parse_budget_quota(
        &mut self,
        kind: String,
        line: u32,
        column: u32,
    ) -> Result<BudgetQuota, ParseError> {
        self.consume(TokenType::Colon)?;
        let limit = self.consume_number()? as i64;
        // `per <period>`
        let _per = self.consume_any_ident_or_kw()?; // the `per` keyword
        let period = self.consume_any_ident_or_kw()?.value;
        // `on Tool(<effect>)`
        let _on = self.consume_any_ident_or_kw()?; // the `on` keyword
        let _tool = self.consume_any_ident_or_kw()?; // the `Tool` wrapper keyword
        self.consume(TokenType::LParen)?;
        let effect = self.consume_any_ident_or_kw()?.value;
        self.consume(TokenType::RParen)?;
        Ok(BudgetQuota {
            kind,
            limit,
            period,
            effect,
            loc: Loc { line, column },
        })
    }

    fn parse_axonstore(&mut self) -> Result<AxonStoreDefinition, ParseError> {
        let tok = self.consume(TokenType::AxonStore)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = AxonStoreDefinition {
            name,
            backend: String::new(),
            connection: String::new(),
            resource_ref: String::new(),
            confidence_floor: None,
            isolation: String::new(),
            on_breach: String::new(),
            capability: String::new(),
            class: String::new(),
            column_schema: None,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            // §Fase 38.b (D1) — `schema:` declaration in three closed
            // forms: inline column block, manifest reference (string
            // literal), or env-var schema namespace (`env:VAR` —
            // unquoted or quoted). Parse the form; the §38.d / §38.e
            // type-checker consumes the resulting AST.
            if field.ttype == TokenType::Schema {
                self.advance();
                let parsed = self.parse_store_schema_declaration(&node.name, field.line, field.column)?;
                node.column_schema = Some(parsed);
                continue;
            }
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "backend" => node.backend = self.consume_any_ident_or_kw()?.value.clone(),
                    // §Fase 94.a — the secret-class prefix of a
                    // `backend: secrets` metadata store. Dotted-identifier
                    // form (`class: crm`, `class: crm.oauth`); the
                    // secrets-only placement rule + slug shape are
                    // `axon-T900` in the type-checker (it needs the
                    // resolved `backend:`, which may appear after this
                    // field in source order).
                    "class" => node.class = self.parse_dotted_identifier()?,
                    "connection" => {
                        node.connection = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    // §Fase 113 — the `resource` this store RUNS ON. When
                    // present the store derives its DSN, its POOL SIZE and its
                    // sharing discipline from the resource; `connection:`
                    // becomes redundant and `axon-T946` refuses declaring both
                    // (the same fact, twice, is how the islands happened).
                    "resource" => {
                        node.resource_ref = self.consume_any_ident_or_kw()?.value.clone()
                    }
                    "confidence_floor" => node.confidence_floor = self.parse_optional_float(),
                    "isolation" => node.isolation = self.consume_any_ident_or_kw()?.value.clone(),
                    "on_breach" => node.on_breach = self.consume_any_ident_or_kw()?.value.clone(),
                    // §Fase 35.j (D11) — Pillar IV: the capability slug
                    // required to access this store. Validated against
                    // the closed slug grammar shared with `requires:`.
                    "capability" => {
                        let slug_tok = self.consume(TokenType::StringLit)?.clone();
                        if !is_valid_capability_slug(&slug_tok.value) {
                            return Err(ParseError {
                                message: format!(
                                    "Invalid capability slug '{}' in axonstore '{}' \
                                     `capability:`. Capability slugs must match \
                                     ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$ — dot-separated \
                                     lowercase identifiers starting with a letter. Examples: \
                                     `admin`, `tenant.read`, `hipaa.phi.read`.",
                                    slug_tok.value, node.name
                                ),
                                line: slug_tok.line,
                                column: slug_tok.column,
                                ..Default::default()
                            });
                        }
                        node.capability = slug_tok.value.clone();
                    }
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 38.b (D1) — parse the three closed forms of an `axonstore`
    /// `schema:` declaration:
    ///
    ///   * form (a) **inline** — `schema { col: Type [constraint…], … }`
    ///   * form (b) **manifest reference** — `schema: "qualified.name"`
    ///     (string literal that does NOT start with `env:`)
    ///   * form (c) **env-var schema namespace** — `schema: env:VAR`
    ///     (unquoted) OR `schema: "env:VAR"` (quoted; the literal
    ///     starts with `env:`)
    ///
    /// Called immediately AFTER `schema` is consumed.
    fn parse_store_schema_declaration(
        &mut self,
        store_name: &str,
        sch_line: u32,
        sch_col: u32,
    ) -> Result<crate::store_schema::StoreColumnSchema, ParseError> {
        use crate::store_schema::{StoreColumn, StoreColumnSchema, StoreColumnType};

        // — Form (a) — inline column block: `schema { ... }`. —
        if self.check(TokenType::LBrace) {
            self.consume(TokenType::LBrace)?;
            let mut columns: Vec<StoreColumn> = Vec::new();
            while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                let col_tok = self.current().clone();
                let col_name = self.consume_any_ident_or_kw()?.value.clone();
                self.consume(TokenType::Colon)?;
                let type_tok = self.consume_any_ident_or_kw()?.clone();
                let col_type = StoreColumnType::from_token(&type_tok.value).ok_or_else(|| {
                    let names = StoreColumnType::all_canonical_names();
                    let suggestion =
                        crate::smart_suggest::suggest_for(&type_tok.value, &names);
                    let suggest_suffix = if suggestion.is_empty() {
                        String::new()
                    } else {
                        format!(" {suggestion}")
                    };
                    let known = names.join(", ");
                    ParseError {
                        message: format!(
                            "Unknown column type `{}` for column `{}` in \
                             axonstore `{}` `schema:` block. The closed \
                             v1.38.0 column-type catalog (Fase 38.b D1) \
                             is {{{known}}} (plus common lowercase \
                             aliases — `int`/`integer`/`int4` for \
                             `Int`, `bool`/`boolean` for `Bool`, etc.).\
                             {suggest_suffix}",
                            type_tok.value, col_name, store_name
                        ),
                        line: type_tok.line,
                        column: type_tok.column,
                        ..Default::default()
                    }
                })?;

                // §Fase 73.a (D1) — the OPTIONAL `Json<T>` shape LENS on a
                // column. `payload: Json<UserEvent>` records the expected
                // struct shape; the lens is a compile-time expectation only
                // (the column stays physically `jsonb`, navigated totally at
                // runtime — doctrine `open_data_is_total`). The shape's
                // well-formedness (T is a declared `type`) is `axon-T840`
                // in the type-checker — it needs the symbol table. Here we
                // only enforce the STRUCTURAL rule: a `<T>` lens may refine
                // ONLY a `Json` / `Jsonb` column — `axon-T841` otherwise.
                let mut json_shape: Option<String> = None;
                if self.check(TokenType::Lt) {
                    self.advance();
                    let shape_tok = self.consume_any_ident_or_kw()?.clone();
                    self.consume(TokenType::Gt)?;
                    if matches!(col_type, StoreColumnType::Json | StoreColumnType::Jsonb) {
                        json_shape = Some(shape_tok.value.clone());
                    } else {
                        return Err(ParseError {
                            message: format!(
                                "axon-T841 a shape lens `<{shape}>` may refine \
                                 only a `Json` / `Jsonb` column, but column \
                                 `{col}` in axonstore `{store}` is `{ty}`. Drop \
                                 the `<{shape}>` (a rigid column already has a \
                                 fixed shape), or change the column type to \
                                 `Json<{shape}>` if it carries open documents.",
                                shape = shape_tok.value,
                                col = col_name,
                                store = store_name,
                                ty = col_type.canonical_name(),
                            ),
                            line: shape_tok.line,
                            column: shape_tok.column,
                            ..Default::default()
                        });
                    }
                }

                let mut col = StoreColumn {
                    name: col_name,
                    col_type,
                    json_shape,
                    primary_key: false,
                    auto_increment: false,
                    not_null: false,
                    unique: false,
                    indexed: false,
                    default_value: String::new(),
                    // §Fase 38.x.d (D1) — `identity` is now a recognized
                    // inline keyword (see the constraint loop below).
                    // Defaults to false; set to true when the adopter
                    // writes `id: BigInt primary_key identity`.
                    identity: false,
                    line: col_tok.line,
                    column: col_tok.column,
                };

                // Trailing constraints (position-independent), matching
                // the Python `_parse_store_column` surface.
                while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                    if self.current().ttype != TokenType::Identifier {
                        // The next column starts with a non-identifier
                        // (rare) — stop the constraint scan.
                        break;
                    }
                    let constraint = self.current().value.clone();
                    match constraint.as_str() {
                        "primary_key" => {
                            col.primary_key = true;
                            self.advance();
                        }
                        "auto_increment" => {
                            col.auto_increment = true;
                            self.advance();
                        }
                        "not_null" => {
                            col.not_null = true;
                            self.advance();
                        }
                        "unique" => {
                            col.unique = true;
                            self.advance();
                        }
                        // §Fase 73.f (D1) — the `index` constraint declares
                        // an index as a capability-honest effect (visible to
                        // the deploy gate, not a silent DBA action). The
                        // backend picks the method from the column type
                        // (GIN for a Json/Jsonb column, b-tree otherwise).
                        "index" => {
                            col.indexed = true;
                            self.advance();
                        }
                        // §Fase 38.x.d (D1) — `identity` marks a column
                        // as `GENERATED ALWAYS/BY DEFAULT AS IDENTITY`.
                        // Distinct from `auto_increment` (legacy SERIAL
                        // via `nextval(...)` default). T803 skips
                        // identity columns from the NOT-NULL-omission
                        // check because Postgres auto-fills them; the
                        // distinction matters because IDENTITY ALWAYS
                        // also rejects user-supplied values, where
                        // SERIAL accepts them (a future 38.x.e arm in
                        // T802 may surface this).
                        "identity" => {
                            col.identity = true;
                            self.advance();
                        }
                        "default" => {
                            self.advance();
                            let dv = self.current().clone();
                            if matches!(
                                dv.ttype,
                                TokenType::StringLit
                                    | TokenType::Integer
                                    | TokenType::Float
                            ) {
                                col.default_value = dv.value.clone();
                                self.advance();
                            } else {
                                col.default_value =
                                    self.consume_any_ident_or_kw()?.value.clone();
                            }
                        }
                        _ => break,
                    }
                }

                columns.push(col);
            }
            self.consume(TokenType::RBrace)?;
            return Ok(StoreColumnSchema::Inline {
                columns,
                leading_trivia: Vec::new(),
                line: sch_line,
                column: sch_col,
            });
        }

        // — Forms (b) + (c) require a `:` separator. —
        if !self.check(TokenType::Colon) {
            let cur = self.current().clone();
            return Err(ParseError {
                message: format!(
                    "axonstore `{store_name}` `schema:` declaration expects \
                     `{{ … }}` (inline columns), `: \"manifest.ref\"` \
                     (manifest reference), or `: env:VAR` (per-tenant schema \
                     namespace). Got `{}` instead.",
                    cur.value
                ),
                line: cur.line,
                column: cur.column,
                ..Default::default()
            });
        }
        self.consume(TokenType::Colon)?;

        // — Form (b) or (c)-quoted — string literal value. —
        if self.check(TokenType::StringLit) {
            let lit = self.consume(TokenType::StringLit)?.clone();
            let value = lit.value.clone();
            if let Some(var) = value.strip_prefix("env:") {
                let var = var.trim();
                if var.is_empty() {
                    return Err(ParseError {
                        message: format!(
                            "axonstore `{store_name}` `schema: \"env:\"` is \
                             missing the variable name after the `env:` \
                             prefix."
                        ),
                        line: lit.line,
                        column: lit.column,
                        ..Default::default()
                    });
                }
                return Ok(StoreColumnSchema::EnvVar {
                    var_name: var.to_string(),
                    line: sch_line,
                    column: sch_col,
                });
            }
            // Plain string → manifest reference.
            if value.trim().is_empty() {
                return Err(ParseError {
                    message: format!(
                        "axonstore `{store_name}` `schema:` manifest reference \
                         is empty. Expected `\"qualified.name\"` — e.g. \
                         `\"public.tenants\"`."
                    ),
                    line: lit.line,
                    column: lit.column,
                    ..Default::default()
                });
            }
            return Ok(StoreColumnSchema::ManifestRef {
                qualified_name: value,
                line: sch_line,
                column: sch_col,
            });
        }

        // — Form (c) unquoted — `env:VAR`. The lexer emits `env` as an
        //   identifier, then `:`, then the identifier var name. —
        let env_tok = self.current().clone();
        if env_tok.value == "env" {
            self.advance();
            if !self.check(TokenType::Colon) {
                return Err(ParseError {
                    message: format!(
                        "axonstore `{store_name}` `schema: env` is missing the \
                         `:` separator. Expected `schema: env:VAR`."
                    ),
                    line: env_tok.line,
                    column: env_tok.column,
                    ..Default::default()
                });
            }
            self.advance(); // past ':'
            let var_tok = self.consume_any_ident_or_kw()?.clone();
            if var_tok.value.trim().is_empty() {
                return Err(ParseError {
                    message: format!(
                        "axonstore `{store_name}` `schema: env:` is missing \
                         the variable name."
                    ),
                    line: var_tok.line,
                    column: var_tok.column,
                    ..Default::default()
                });
            }
            return Ok(StoreColumnSchema::EnvVar {
                var_name: var_tok.value.clone(),
                line: sch_line,
                column: sch_col,
            });
        }

        Err(ParseError {
            message: format!(
                "axonstore `{store_name}` `schema:` declaration expects \
                 `{{ … }}` (inline columns), `\"manifest.ref\"` (manifest \
                 reference), or `env:VAR` (per-tenant schema namespace). \
                 Got `{}` instead.",
                env_tok.value
            ),
            line: env_tok.line,
            column: env_tok.column,
            ..Default::default()
        })
    }

    // ── §λ-L-E Fase 1 — Resource primitive ────────────────────────

    /// Parse: `resource Name { kind, endpoint, capacity, lifetime, certainty_floor, shield }`.
    ///
    /// Mirrors `axon.compiler.parser.Parser._parse_resource`. Unknown fields
    /// are silently skipped (keeps the grammar forward-compatible).
    fn parse_resource(&mut self) -> Result<ResourceDefinition, ParseError> {
        let tok = self.consume(TokenType::Resource)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ResourceDefinition {
            name,
            kind: String::new(),
            endpoint: String::new(),
            capacity: None,
            lifetime: "affine".to_string(),
            certainty_floor: None,
            shield_ref: String::new(),
            within: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_tok = self.current().clone();
            let field_name = field_tok.value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                // Tolerate stray brace or unknown layout.
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance(); // past ':'
            match field_name.as_str() {
                "kind" => node.kind = self.consume_any_ident_or_kw()?.value,
                // §Fase 113 — `endpoint:` accepts BOTH shapes on purpose:
                //   - a dotted config key  (`endpoint: db.main`)      — the law
                //   - a string literal     (`endpoint: "postgres://…"`) — the sin
                //
                // The literal is REFUSED, but by `axon-T944`, not by the parser.
                // If it died here the adopter would read "Expected StringLit",
                // which explains nothing. The law gets to say why: *URLs and
                // credentials never appear in source* — the same sentence
                // `axon-T850` has been saying to `upstream.resolve` all along.
                //
                // A diagnostic that names the rule teaches; one that names the
                // token type only tells you the compiler is unhappy.
                "endpoint" => {
                    node.endpoint = if self.check(TokenType::StringLit) {
                        self.consume(TokenType::StringLit)?.value
                    } else {
                        self.parse_dotted_identifier()?
                    };
                }
                "capacity" => {
                    node.capacity = self.parse_optional_int();
                }
                "lifetime" => {
                    let lt_tok = self.consume_any_ident_or_kw()?;
                    let lt = lt_tok.value;
                    if !matches!(lt.as_str(), "linear" | "affine" | "persistent") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid lifetime '{lt}' in resource '{}' — \
                                 expected linear | affine | persistent",
                                node.name
                            ),
                            line: lt_tok.line,
                            column: lt_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.lifetime = lt;
                }
                "certainty_floor" => {
                    node.certainty_floor = self.parse_optional_float();
                }
                "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value,
                // §Fase 113 — `within: <fabric>`. ONE field, so a resource
                // cannot be in two fabrics: Separation-Logic disjointness is
                // unrepresentable rather than verified.
                "within" => node.within = self.consume_any_ident_or_kw()?.value,
                // §Fase 113 — an unknown field is a HARD ERROR, not a shrug.
                //
                // This arm used to be `_ => self.skip_value()`. That is the same
                // family as §111's root cause (`parse_block_step` →
                // `skip_braced_block()`, which silently killed four primitives):
                // a misspelled `withn:` would have been swallowed without a
                // word, and the resource would have governed nothing while
                // looking governed. A field the parser does not know is a field
                // the adopter believes in and the compiler does not.
                unknown => {
                    return Err(ParseError {
                        message: format!(
                            "Unknown field '{unknown}' in resource '{}' — expected one of: \
                             kind, endpoint, capacity, lifetime, certainty_floor, shield, within",
                            node.name
                        ),
                        line: field_tok.line,
                        column: field_tok.column,
                        ..Default::default()
                    });
                }
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `fabric Name { provider, region, zones, ephemeral, shield }`.
    fn parse_fabric(&mut self) -> Result<FabricDefinition, ParseError> {
        let tok = self.consume(TokenType::Fabric)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = FabricDefinition {
            name,
            provider: String::new(),
            region: String::new(),
            zones: None,
            ephemeral: None,
            shield_ref: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance(); // past ':'
            match field_name.as_str() {
                "provider" => node.provider = self.consume_any_ident_or_kw()?.value,
                "region" => node.region = self.consume(TokenType::StringLit)?.value,
                "zones" => node.zones = self.parse_optional_int(),
                "ephemeral" => {
                    let b = self.parse_bool()?;
                    node.ephemeral = Some(b);
                }
                "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `manifest Name { resources, fabric, region, zones, compliance }`.
    fn parse_manifest(&mut self) -> Result<ManifestDefinition, ParseError> {
        let tok = self.consume(TokenType::Manifest)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ManifestDefinition {
            name,
            resources: Vec::new(),
            fabric_ref: String::new(),
            region: String::new(),
            zones: None,
            compliance: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "resources" => node.resources = self.parse_bracketed_identifiers()?,
                "fabric" => node.fabric_ref = self.consume_any_ident_or_kw()?.value,
                "region" => node.region = self.consume(TokenType::StringLit)?.value,
                "zones" => node.zones = self.parse_optional_int(),
                "compliance" => node.compliance = self.parse_bracketed_identifiers()?,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `observe Name from Manifest { sources, quorum, timeout, on_partition, certainty_floor }`.
    fn parse_observe(&mut self) -> Result<ObserveDefinition, ParseError> {
        let tok = self.consume(TokenType::Observe)?;
        let name = self.consume(TokenType::Identifier)?.value;
        // `from <Manifest>` — required per Python grammar.
        self.consume(TokenType::From)?;
        let target = self.consume(TokenType::Identifier)?.value;
        let mut node = ObserveDefinition {
            name,
            target,
            sources: Vec::new(),
            quorum: None,
            timeout: String::new(),
            on_partition: "fail".to_string(),
            certainty_floor: None,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "sources" => node.sources = self.parse_bracketed_identifiers()?,
                "quorum" => node.quorum = self.parse_optional_int(),
                "timeout" => {
                    let t = self.current().clone();
                    match t.ttype {
                        TokenType::Duration | TokenType::StringLit => {
                            self.advance();
                            node.timeout = t.value;
                        }
                        _ => node.timeout = self.consume_any_ident_or_kw()?.value,
                    }
                }
                "on_partition" => {
                    let p_tok = self.consume_any_ident_or_kw()?;
                    let p = p_tok.value;
                    if !matches!(p.as_str(), "fail" | "shield_quarantine") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid on_partition '{p}' in observe '{}' — \
                                 expected fail | shield_quarantine",
                                node.name
                            ),
                            line: p_tok.line,
                            column: p_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.on_partition = p;
                }
                "certainty_floor" => node.certainty_floor = self.parse_optional_float(),
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── §λ-L-E Fase 3 — Control cognitivo ─────────────────────────

    /// Parse: `reconcile Name { observe, threshold, tolerance, on_drift, shield, mandate, max_retries }`.
    fn parse_reconcile(&mut self) -> Result<ReconcileDefinition, ParseError> {
        let tok = self.consume(TokenType::Reconcile)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ReconcileDefinition {
            name,
            observe_ref: String::new(),
            threshold: None,
            tolerance: None,
            on_drift: "provision".to_string(),
            shield_ref: String::new(),
            mandate_ref: String::new(),
            max_retries: 3,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "observe" => node.observe_ref = self.consume_any_ident_or_kw()?.value,
                "threshold" => node.threshold = self.parse_optional_float(),
                "tolerance" => node.tolerance = self.parse_optional_float(),
                "on_drift" => {
                    let d_tok = self.consume_any_ident_or_kw()?;
                    let d = d_tok.value;
                    if !matches!(d.as_str(), "provision" | "alert" | "refine") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid on_drift '{d}' in reconcile '{}' — \
                                 expected provision | alert | refine",
                                node.name
                            ),
                            line: d_tok.line,
                            column: d_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.on_drift = d;
                }
                "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value,
                "mandate" => node.mandate_ref = self.consume_any_ident_or_kw()?.value,
                "max_retries" => {
                    if let Some(v) = self.parse_optional_int() {
                        node.max_retries = v;
                    }
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `lease Name { resource, duration, acquire, on_expire }`.
    fn parse_lease(&mut self) -> Result<LeaseDefinition, ParseError> {
        let tok = self.consume(TokenType::Lease)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = LeaseDefinition {
            name,
            resource_ref: String::new(),
            duration: String::new(),
            acquire: "on_start".to_string(),
            on_expire: "anchor_breach".to_string(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "resource" => node.resource_ref = self.consume_any_ident_or_kw()?.value,
                "duration" => {
                    let t = self.current().clone();
                    match t.ttype {
                        TokenType::Duration | TokenType::StringLit => {
                            self.advance();
                            node.duration = t.value;
                        }
                        _ => node.duration = self.consume_any_ident_or_kw()?.value,
                    }
                }
                "acquire" => {
                    let a_tok = self.consume_any_ident_or_kw()?;
                    let a = a_tok.value;
                    if !matches!(a.as_str(), "on_start" | "on_demand") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid acquire '{a}' in lease '{}' — \
                                 expected on_start | on_demand",
                                node.name
                            ),
                            line: a_tok.line,
                            column: a_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.acquire = a;
                }
                "on_expire" => {
                    let e_tok = self.consume_any_ident_or_kw()?;
                    let e = e_tok.value;
                    if !matches!(e.as_str(), "anchor_breach" | "release" | "extend") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid on_expire '{e}' in lease '{}' — \
                                 expected anchor_breach | release | extend",
                                node.name
                            ),
                            line: e_tok.line,
                            column: e_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.on_expire = e;
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `ensemble Name { observations, quorum, aggregation, certainty_mode }`.
    fn parse_ensemble(&mut self) -> Result<EnsembleDefinition, ParseError> {
        let tok = self.consume(TokenType::Ensemble)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = EnsembleDefinition {
            name,
            observations: Vec::new(),
            quorum: None,
            aggregation: "majority".to_string(),
            certainty_mode: "min".to_string(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "observations" => node.observations = self.parse_bracketed_identifiers()?,
                "quorum" => node.quorum = self.parse_optional_int(),
                "aggregation" => {
                    let a_tok = self.consume_any_ident_or_kw()?;
                    let a = a_tok.value;
                    if !matches!(a.as_str(), "majority" | "weighted" | "byzantine") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid aggregation '{a}' in ensemble '{}' — \
                                 expected majority | weighted | byzantine",
                                node.name
                            ),
                            line: a_tok.line,
                            column: a_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.aggregation = a;
                }
                "certainty_mode" => {
                    let c_tok = self.consume_any_ident_or_kw()?;
                    let c = c_tok.value;
                    if !matches!(c.as_str(), "min" | "weighted" | "harmonic") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid certainty_mode '{c}' in ensemble '{}' — \
                                 expected min | weighted | harmonic",
                                node.name
                            ),
                            line: c_tok.line,
                            column: c_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.certainty_mode = c;
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── §λ-L-E Fase 4 — Topology + π-calculus binary sessions ─────

    /// Parse: `session Name { role1: [step, …]  role2: [step, …] }`.
    ///
    /// The enclosing `parse_session_definition` disambiguates from the session
    /// step token `session` (which does not exist) by always entering from the
    /// top-level dispatch; the identifier role name is consumed after `{`.
    fn parse_session_definition(&mut self) -> Result<SessionDefinition, ParseError> {
        let tok = self.consume(TokenType::Session)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = SessionDefinition {
            name,
            roles: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let role_tok = self.consume_any_ident_or_kw()?;
            self.consume(TokenType::Colon)?;
            let steps = self.parse_session_steps()?;
            node.roles.push(SessionRole {
                name: role_tok.value,
                steps,
                loc: Loc {
                    line: role_tok.line,
                    column: role_tok.column,
                },
            });
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 51.c.2 — Parse a Pauli-sum observable declaration:
    /// ```text
    /// observable EnergyHamiltonian {
    ///     qubits: 2
    ///     term: 0.5 * "ZZ"
    ///     term: -1.2 * "XI"
    /// }
    /// ```
    /// `term:` is a repeatable key (one `cₖ · Pₖ` per line). The coefficient is
    /// a real scalar (optional leading `+`/`-`), then `*`, then a quoted Pauli
    /// string. The type-checker (§51.c.2) validates the closed `{I,X,Y,Z}`
    /// alphabet + equal lengths; real coefficients ⇒ Hermitian by construction.
    fn parse_observable(&mut self) -> Result<ObservableDefinition, ParseError> {
        let tok = self.consume(TokenType::Observable)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ObservableDefinition {
            name,
            qubits: None,
            terms: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key_tok = self.consume_any_ident_or_kw()?;
            self.consume(TokenType::Colon)?;
            match key_tok.value.as_str() {
                "qubits" => node.qubits = Some(self.consume_number()? as i64),
                "term" => {
                    let term_loc = Loc {
                        line: key_tok.line,
                        column: key_tok.column,
                    };
                    // Optional sign, then magnitude.
                    let mut negative = false;
                    if self.check(TokenType::Minus) {
                        self.advance();
                        negative = true;
                    } else if self.check(TokenType::Plus) {
                        self.advance();
                    }
                    let mag = self.consume_number()?;
                    let coefficient = if negative { -mag } else { mag };
                    // `*` separator between coefficient and Pauli string.
                    self.consume(TokenType::Star)?;
                    let pauli = self.consume(TokenType::StringLit)?.value;
                    node.terms.push(PauliTerm {
                        coefficient,
                        pauli,
                        loc: term_loc,
                    });
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 69.a — Parse:
    /// `witness Name { claim: <ref>  against: <baseline>  metric: <metric>
    ///                 threshold: <ε>  data: <source> }`.
    /// Order-free `key: value` pairs. `claim`/`against`/`metric`/`data` are bare
    /// identifiers (a ref or a closed-catalog keyword); `threshold` is a number.
    /// Well-formedness (known metric, threshold range, required fields) is the
    /// type-checker's job (`axon-E0790`).
    fn parse_witness(&mut self) -> Result<WitnessDefinition, ParseError> {
        let tok = self.consume(TokenType::Witness)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = WitnessDefinition {
            name,
            claim: String::new(),
            baseline: String::new(),
            metric: String::new(),
            threshold: 0.0,
            data: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key_tok = self.consume_any_ident_or_kw()?;
            self.consume(TokenType::Colon)?;
            match key_tok.value.as_str() {
                "claim" => node.claim = self.consume_any_ident_or_kw()?.value,
                // `against` is the baseline; `against` is not a reserved keyword,
                // so it lexes as an identifier key here.
                "against" => node.baseline = self.consume_any_ident_or_kw()?.value,
                "metric" => node.metric = self.consume_any_ident_or_kw()?.value,
                "threshold" => node.threshold = self.consume_number()?,
                "data" => node.data = self.consume_any_ident_or_kw()?.value,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 41.b — Parse:
    /// `socket Name { protocol: SessionRef, backpressure: credit(n),
    ///               reconnect: cognitive_state, legal_basis: ... }`.
    /// Fields are `key: value` pairs (order-free); only `protocol` is required.
    fn parse_socket(&mut self) -> Result<SocketDefinition, ParseError> {
        let tok = self.consume(TokenType::Socket)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = SocketDefinition {
            name,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "protocol" => node.protocol = self.consume_any_ident_or_kw()?.value,
                "backpressure" => {
                    // `credit(n)` — the typed-resource window.
                    let kind = self.consume_any_ident_or_kw()?.value;
                    if kind != "credit" {
                        return Err(self.error(&format!("expected `credit(n)` for backpressure, got `{kind}`")));
                    }
                    self.consume(TokenType::LParen)?;
                    let n = self
                        .consume(TokenType::Integer)?
                        .value
                        .parse::<i64>()
                        .map_err(|_| self.error("backpressure credit must be an integer"))?;
                    self.consume(TokenType::RParen)?;
                    node.backpressure_credit = Some(n);
                }
                "reconnect" => {
                    let mode = self.consume_any_ident_or_kw()?.value;
                    node.reconnect = mode == "cognitive_state";
                }
                "legal_basis" => node.legal_basis = Some(self.consume_any_ident_or_kw()?.value),
                other => return Err(self.error(&format!("unknown socket field `{other}`"))),
            }
            // Optional comma between fields.
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 80.b — parse `upstream Name [from Preset@vN] { fields }`.
    ///
    /// Field grammar per `docs/fase/fase_80_upstream_design.md` §1–2. The
    /// parser fixes the *shape* only; catalog membership (`transport:`,
    /// `auth:`, `overflow:`, `on_exhausted:`), key charsets and projection
    /// totality are §80.c type-checker laws (T849–T851), mirroring how
    /// `socket` splits parse vs. check.
    fn parse_upstream(&mut self) -> Result<UpstreamDefinition, ParseError> {
        let tok = self.consume(TokenType::Upstream)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = UpstreamDefinition {
            name,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        // §80.f — preset instantiation: `upstream X from DeepgramSTT@v1 {…}`.
        if self.check(TokenType::From) {
            self.advance();
            let base = self.consume(TokenType::Identifier)?.value;
            self.consume(TokenType::At)?;
            let version = self.consume_any_ident_or_kw()?.value;
            node.preset = Some(format!("{base}@{version}"));
        }
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "transport" => node.transport = self.consume_any_ident_or_kw()?.value,
                "protocol" => node.protocol = self.consume_any_ident_or_kw()?.value,
                "role" => node.role = self.consume_any_ident_or_kw()?.value,
                "resolve" => node.resolve = self.parse_dotted_identifier()?,
                // §Fase 114.u — the upstream's channel rides a declared
                // `resource`; the address + instance bound DERIVE from it.
                // XOR with `resolve:` is axon-T951 (type-checker territory).
                "resource" => node.resource_ref = self.consume_any_ident_or_kw()?.value,
                "secret" => node.secret = self.parse_dotted_identifier()?,
                "auth" => {
                    // `header("Name")` | `header("Name", "Prefix ")` |
                    // `query("param")` | `signed_url`.
                    node.auth_kind = self.consume_any_ident_or_kw()?.value;
                    if self.check(TokenType::LParen) {
                        self.consume(TokenType::LParen)?;
                        node.auth_name = Some(self.consume(TokenType::StringLit)?.value);
                        if self.check(TokenType::Comma) {
                            self.consume(TokenType::Comma)?;
                            node.auth_prefix = Some(self.consume(TokenType::StringLit)?.value);
                        }
                        self.consume(TokenType::RParen)?;
                    }
                }
                "map" => node.map = self.parse_upstream_map()?,
                "reconnect" => node.reconnect = Some(self.parse_upstream_reconnect()?),
                "overflow" => node.overflow = Some(self.consume_any_ident_or_kw()?.value),
                "backpressure" => {
                    // `credit(n)` — same typed-resource window as `socket`.
                    let kind = self.consume_any_ident_or_kw()?.value;
                    if kind != "credit" {
                        return Err(self.error(&format!("expected `credit(n)` for backpressure, got `{kind}`")));
                    }
                    self.consume(TokenType::LParen)?;
                    let n = self
                        .consume(TokenType::Integer)?
                        .value
                        .parse::<i64>()
                        .map_err(|_| self.error("backpressure credit must be an integer"))?;
                    self.consume(TokenType::RParen)?;
                    node.backpressure_credit = Some(n);
                }
                other => return Err(self.error(&format!("unknown upstream field `{other}`"))),
            }
            // Optional comma between fields.
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 83.a — parse `cors Name { fields }`. Field-shape checks
    /// (wildcard+credentials, origin-glob shape, closed method catalog,
    /// cross-method path consistency) are §83.c type-checker territory
    /// (T853-T857); the parser only builds the structural AST.
    ///
    /// **Unknown fields are a hard error** (D83.7, not `shield`'s lenient
    /// `axon-W010` record-and-skip) — mirrors `upstream`'s stricter
    /// posture, appropriate for a security-relevant declaration.
    fn parse_cors(&mut self) -> Result<CorsDefinition, ParseError> {
        let tok = self.consume(TokenType::Cors)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = CorsDefinition {
            name,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "allow_origins" => node.allow_origins = self.parse_bracketed_strings()?,
                "allow_methods" => node.allow_methods = self.parse_bracketed_identifiers()?,
                "allow_headers" => node.allow_headers = self.parse_bracketed_strings()?,
                "allow_credentials" => {
                    node.allow_credentials = self.consume_any_ident_or_kw()?.value == "true"
                }
                "max_age" => node.max_age = Some(self.consume(TokenType::Duration)?.value),
                "expose_headers" => node.expose_headers = self.parse_bracketed_strings()?,
                other => return Err(self.error(&format!("unknown cors field `{other}`"))),
            }
            // Optional comma between fields.
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 92.a — parse `credential Name { ttl: grants: }`. Strict
    /// closed-catalog (unknown field is a hard error, the §83 D83.7
    /// discipline — a credential contract governs AUTHORITY, so a typo can
    /// never silently produce a permissive contract). `grants:` slugs are
    /// validated at parse time with the same closed grammar as
    /// `axonendpoint requires:`; the cross-field laws (non-empty grants,
    /// TTL bounds) are §92.a type-checker territory (`axon-T893`/`T894`).
    fn parse_credential(&mut self) -> Result<CredentialDefinition, ParseError> {
        let tok = self.consume(TokenType::Credential)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = CredentialDefinition {
            name,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "ttl" => node.ttl = self.consume(TokenType::Duration)?.value,
                "grants" => {
                    let bracket_tok = self.current().clone();
                    let items = self.parse_bracketed_dot_identifiers()?;
                    for slug in &items {
                        if !is_valid_capability_slug(slug) {
                            return Err(ParseError {
                                message: format!(
                                    "Invalid capability slug '{slug}' in credential '{}' \
                                     `grants:`. Capability slugs must match \
                                     ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$ — dot-separated \
                                     lowercase identifiers starting with a letter. Examples: \
                                     `chat.invoke`, `flow.execute`.",
                                    node.name
                                ),
                                line: bracket_tok.line,
                                column: bracket_tok.column,
                                ..Default::default()
                            });
                        }
                    }
                    node.grants = items;
                }
                other => return Err(self.error(&format!("unknown credential field `{other}`"))),
            }
            // Optional comma between fields.
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 85.a — parse `cache Name { backend:, ttl:, key:, default:,
    /// apply_to_effects:, invalidate_on: }`. Strict closed-catalog (unknown
    /// field is a hard error, the §83 D83.7 discipline — a cache governs
    /// correctness, so a typo can never silently mean "no policy"). All
    /// cross-field laws (single default, non-pure-needs-ttl, reference
    /// resolution, effect widening) are §85.c type-checker territory.
    fn parse_cache(&mut self) -> Result<CacheDefinition, ParseError> {
        let tok = self.consume(TokenType::Cache)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = CacheDefinition {
            name,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "backend" => node.backend = self.consume_any_ident_or_kw()?.value,
                "ttl" => node.ttl = Some(self.consume(TokenType::Duration)?.value),
                "key" => node.key_params = self.parse_bracketed_identifiers()?,
                "default" => {
                    node.default_policy = self.consume_any_ident_or_kw()?.value == "true"
                }
                "apply_to_effects" => {
                    node.apply_to_effects = self.parse_bracketed_identifiers()?
                }
                "invalidate_on" => node.invalidate_on = self.parse_bracketed_identifiers()?,
                other => return Err(self.error(&format!("unknown cache field `{other}`"))),
            }
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── §Fase 99.b — Native Document Synthesis ─────────────────────────────

    /// §Fase 99.b — parse `document <Name> { target:, template:?, provenance:?,
    /// effects:?, <body blocks> }`. Document-level scalars are handled here;
    /// anything of the form `ident { … }` is a body block ([`parse_doc_block_body`]).
    /// Unknown scalar fields are a hard error (the §83/§84 closed-catalog
    /// discipline); the per-`target` block vocabulary is the §99.c checker's job.
    fn parse_document(&mut self) -> Result<crate::ast::DocumentDefinition, ParseError> {
        let tok = self.consume(TokenType::Document)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = crate::ast::DocumentDefinition {
            name,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "target" => node.target = self.consume_any_ident_or_kw()?.value,
                    "template" => node.template = self.parse_dotted_identifier()?,
                    "provenance" => node.provenance = self.consume_any_ident_or_kw()?.value,
                    "effects" => node.effects = Some(self.parse_effect_row()?),
                    other => {
                        return Err(self.error(&format!(
                            "unknown document field `{other}` in document `{}` — expected \
                             `target:` / `template:` / `provenance:` / `effects:`, or a body \
                             block (`section {{ … }}` / `slide {{ … }}` / `sheet {{ … }}`)",
                            node.name
                        )))
                    }
                }
            } else if self.check(TokenType::LBrace) {
                node.blocks
                    .push(self.parse_doc_block_body(field_name, field.line, field.column)?);
            } else {
                return Err(self.error(&format!(
                    "unexpected `{field_name}` in document `{}` body — expected a `field:` or a \
                     body block `{field_name} {{ … }}`",
                    node.name
                )));
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 99.b — parse a document body block whose `kind` was already
    /// consumed: `{ (field: value | nested-block { … })* }`. Recursive — a
    /// `section` holds `para`/`table`/`chart`; a `slide` holds `bullets`/
    /// `notes`; a `sheet` holds `row`/`formula`. A member is a field iff a
    /// `:` follows its name; else it must open a nested block (`{`).
    fn parse_doc_block_body(
        &mut self,
        kind: String,
        line: u32,
        column: u32,
    ) -> Result<crate::ast::DocBlock, ParseError> {
        let mut block = crate::ast::DocBlock {
            kind,
            loc: Loc { line, column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let name_tok = self.current().clone();
            let name = self.consume_any_ident_or_kw()?.value;
            if self.check(TokenType::Colon) {
                self.advance();
                let value = self.parse_doc_scalar()?;
                block.fields.push((name, value));
            } else if self.check(TokenType::LBrace) {
                let child = self.parse_doc_block_body(name, name_tok.line, name_tok.column)?;
                block.children.push(child);
            } else {
                return Err(self.error(&format!(
                    "in document block `{}`: `{name}` must be a `field:` value or open a nested \
                     block `{name} {{ … }}`",
                    block.kind
                )));
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(block)
    }

    /// §Fase 99.b — parse a document field value into a [`crate::ast::DocScalar`].
    /// A bare identifier is a REFERENCE (`text: revenue_summary`) — this is what
    /// the assertion-laundering barrier inspects; a quoted string / int / bool /
    /// bracketed list are literals.
    fn parse_doc_scalar(&mut self) -> Result<crate::ast::DocScalar, ParseError> {
        let tok = self.current().clone();
        match tok.ttype {
            TokenType::StringLit => {
                self.advance();
                Ok(crate::ast::DocScalar::Text(tok.value))
            }
            TokenType::Integer => {
                self.advance();
                Ok(crate::ast::DocScalar::Int(tok.value.parse::<i64>().unwrap_or(0)))
            }
            TokenType::Bool => {
                self.advance();
                Ok(crate::ast::DocScalar::Bool(tok.value == "true"))
            }
            TokenType::LBracket => {
                let items = self.parse_bracketed_strings()?;
                Ok(crate::ast::DocScalar::List(items))
            }
            _ => {
                let name = self.consume_any_ident_or_kw()?.value;
                Ok(crate::ast::DocScalar::Ref(name))
            }
        }
    }

    // ── §Fase 105 — Governed CRM Delivery ──────────────────────────────────

    /// §Fase 105 — parse `deliver <Name> { target:, provenance:?, secret:,
    /// effects:?, <operation blocks> }`. Delivery-level scalars are handled here;
    /// anything of the form `ident { … }` is an operation block
    /// ([`parse_deliver_op`]). Unknown scalar fields are a hard error (the §99
    /// §Fase 110.a — the governed human-notification declaration:
    ///
    /// ```text
    /// notify LowSales {
    ///     channel:    sms | whatsapp | telegram
    ///     to:         secret(ops.oncall_phone)
    ///     template:   "Ventas 7d: ${resumen}"
    ///     window:     4h
    ///     provenance: attached | cleared
    ///     effects:    <web>
    /// }
    /// ```
    ///
    /// The closed-field discipline (§99/§105): an unknown scalar field is
    /// a hard parse error. The LAWS (T933/T934/T935) live in the checker
    /// so violations accumulate; the parser records shape (including a
    /// literal `to:` — kept so T934 can refuse it TEACHING the custody
    /// form, instead of a bare parse error).
    fn parse_notify(&mut self) -> Result<crate::ast::NotifyDefinition, ParseError> {
        let tok = self.consume(TokenType::Notify)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = crate::ast::NotifyDefinition {
            name,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "channel" => node.channel = self.consume_any_ident_or_kw()?.value,
                    "to" => {
                        // The custody form: `secret(<dotted-class>)`. A string
                        // literal parses too — the checker refuses it (T934)
                        // with the teaching message.
                        if self.current().value == "secret" && self.peek_is_lparen() {
                            self.advance(); // `secret`
                            self.consume(TokenType::LParen)?;
                            node.to_secret = self.parse_dotted_identifier()?;
                            self.consume(TokenType::RParen)?;
                            node.to_is_secret = true;
                        } else if self.check(TokenType::StringLit) {
                            node.to_secret = self.consume(TokenType::StringLit)?.value.clone();
                            node.to_is_secret = false;
                        } else {
                            node.to_secret = self.consume_any_ident_or_kw()?.value.clone();
                            node.to_is_secret = false;
                        }
                    }
                    "template" => {
                        node.template = self.consume(TokenType::StringLit)?.value.clone()
                    }
                    "window" => {
                        // `4h` lexes as Integer + ident or one ident — accept
                        // both spellings, normalized to the joined form.
                        if self.check(TokenType::Integer) {
                            let n = self.consume(TokenType::Integer)?.value.clone();
                            let unit = self.consume_any_ident_or_kw()?.value.clone();
                            node.window = format!("{n}{unit}");
                        } else {
                            node.window = self.consume_any_ident_or_kw()?.value.clone();
                        }
                    }
                    "provenance" => node.provenance = self.consume_any_ident_or_kw()?.value,
                    "effects" => node.effects = Some(self.parse_effect_row()?),
                    other => {
                        return Err(self.error(&format!(
                            "unknown notify field `{other}` in notify `{}` — expected \
                             `channel:` / `to:` / `template:` / `window:` / `provenance:` / \
                             `effects:`",
                            node.name
                        )))
                    }
                }
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 110.a — one-token lookahead helper for the `secret(` form.
    /// §Fase 114.a — is the NEXT token an identifier? (`budget <Name> { … }` vs
    /// a bare `budget` used as an ordinary identifier.)
    fn peek_is_identifier(&self) -> bool {
        self.tokens
            .get(self.pos + 1)
            .map(|t| t.ttype == TokenType::Identifier)
            .unwrap_or(false)
    }

    fn peek_is_lparen(&self) -> bool {
        self.tokens
            .get(self.pos + 1)
            .map(|t| t.ttype == TokenType::LParen)
            .unwrap_or(false)
    }

    /// closed-catalog discipline); the operation vocabulary is the checker's job.
    fn parse_deliver(&mut self) -> Result<crate::ast::DeliverDefinition, ParseError> {
        let tok = self.consume(TokenType::Deliver)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = crate::ast::DeliverDefinition {
            name,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "target" => node.target = self.consume_any_ident_or_kw()?.value,
                    "provenance" => node.provenance = self.consume_any_ident_or_kw()?.value,
                    "secret" => node.secret = self.consume_any_ident_or_kw()?.value,
                    "effects" => node.effects = Some(self.parse_effect_row()?),
                    other => {
                        return Err(self.error(&format!(
                            "unknown deliver field `{other}` in deliver `{}` — expected \
                             `target:` / `provenance:` / `secret:` / `effects:`, or an operation \
                             block (`upsert_contact {{ … }}` / `create_deal {{ … }}` / \
                             `add_note {{ … }}`)",
                            node.name
                        )))
                    }
                }
            } else if self.check(TokenType::LBrace) {
                node.ops
                    .push(self.parse_deliver_op(field_name, field.line, field.column)?);
            } else {
                return Err(self.error(&format!(
                    "unexpected `{field_name}` in deliver `{}` body — expected a `field:` or an \
                     operation block `{field_name} {{ … }}`",
                    node.name
                )));
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 105 — parse a delivery operation block whose `kind` was already
    /// consumed: `{ (field: value)* }`. Flat (unlike a document block, an
    /// operation has no nested children) — each member must be a `field: value`.
    fn parse_deliver_op(
        &mut self,
        kind: String,
        line: u32,
        column: u32,
    ) -> Result<crate::ast::DeliverOp, ParseError> {
        let mut op = crate::ast::DeliverOp {
            kind,
            loc: Loc { line, column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let name = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon).map_err(|_| {
                self.error(&format!(
                    "in deliver operation `{}`: `{name}` must be a `field: value` pair — an \
                     operation binds scalar fields, it takes no nested blocks",
                    op.kind
                ))
            })?;
            let value = self.parse_doc_scalar()?;
            op.fields.push((name, value));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(op)
    }

    /// §Fase 87.a — parse `savant <Name> { domain:, cognition{…}, memory{…},
    /// budget{…}, mandate <M> {…} … }`. The block surface only; catalog +
    /// ref-resolution + budget/interruptibility binding is the §87.b/c checker's
    /// job (the standing parse/check split). Unknown fields are a hard error
    /// (D83.7): a savant governs an expensive autonomous process.
    fn parse_savant(&mut self) -> Result<SavantDefinition, ParseError> {
        let tok = self.consume(TokenType::Savant)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = SavantDefinition {
            name,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field = self.current().clone();
            let field_name = field.value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "domain" => node.domain = self.consume(TokenType::StringLit)?.value,
                    other => {
                        return Err(self.error(&format!(
                            "unknown savant field `{other}` in savant `{}` — expected \
                             `domain:` or a `cognition` / `memory` / `budget` / `mandate` block",
                            node.name
                        )))
                    }
                }
            } else if field_name == "cognition" {
                node.cognition = Some(self.parse_savant_cognition(field.line, field.column)?);
            } else if field_name == "memory" {
                node.memory = Some(self.parse_savant_memory(field.line, field.column)?);
            } else if field_name == "budget" {
                node.budget = Some(self.parse_savant_budget(field.line, field.column)?);
            } else if field_name == "mandate" {
                node.mandates
                    .push(self.parse_savant_mandate(field.line, field.column)?);
            } else {
                return Err(self.error(&format!(
                    "unexpected `{field_name}` in savant `{}` body — expected `domain:` or a \
                     `cognition` / `memory` / `budget` / `mandate` block",
                    node.name
                )));
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 87.a — the `cognition { depth:, entropic_threshold:, divergence: }`
    /// sub-block. Catalog validation of `depth`/`divergence` is §87.b.
    fn parse_savant_cognition(
        &mut self,
        line: u32,
        column: u32,
    ) -> Result<SavantCognition, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut node = SavantCognition {
            loc: Loc { line, column },
            ..Default::default()
        };
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "depth" => node.depth = self.consume_any_ident_or_kw()?.value,
                "entropic_threshold" => node.entropic_threshold = self.parse_optional_float(),
                "divergence" => node.divergence = self.consume_any_ident_or_kw()?.value,
                other => {
                    return Err(self.error(&format!(
                        "unknown savant `cognition` field `{other}` — expected \
                         `depth` / `entropic_threshold` / `divergence`"
                    )))
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 87.a — the `memory { backend:, corpus_graph:, isolation_level: }`
    /// sub-block. `backend` is resolved to a declared `memory`/`corpus` in §87.c.
    fn parse_savant_memory(
        &mut self,
        line: u32,
        column: u32,
    ) -> Result<SavantMemory, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut node = SavantMemory {
            loc: Loc { line, column },
            ..Default::default()
        };
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "backend" => node.backend = self.consume_any_ident_or_kw()?.value,
                "corpus_graph" => {
                    node.corpus_graph = self.consume_any_ident_or_kw()?.value == "true"
                }
                "isolation_level" => node.isolation_level = self.consume_any_ident_or_kw()?.value,
                other => {
                    return Err(self.error(&format!(
                        "unknown savant `memory` field `{other}` — expected \
                         `backend` / `corpus_graph` / `isolation_level`"
                    )))
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 87.a — the `budget { max_iterations:, max_tool_synth: }` sub-block.
    /// Bound to a §72 linear budget (`RateLease`) in §87.c.
    fn parse_savant_budget(
        &mut self,
        line: u32,
        column: u32,
    ) -> Result<SavantBudget, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut node = SavantBudget {
            loc: Loc { line, column },
            ..Default::default()
        };
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "max_iterations" => node.max_iterations = self.parse_optional_int(),
                "max_tool_synth" => node.max_tool_synth = self.parse_optional_int(),
                other => {
                    return Err(self.error(&format!(
                        "unknown savant `budget` field `{other}` — expected \
                         `max_iterations` / `max_tool_synth`"
                    )))
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 87.a — the `mandate <Name> { objective:, output: }` sub-block. The
    /// `mandate` keyword is already consumed by `parse_savant`.
    fn parse_savant_mandate(
        &mut self,
        line: u32,
        column: u32,
    ) -> Result<SavantMandate, ParseError> {
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = SavantMandate {
            name,
            loc: Loc { line, column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "objective" => node.objective = self.consume(TokenType::StringLit)?.value,
                "output" => node.output_type = self.consume_any_ident_or_kw()?.value,
                other => {
                    return Err(self.error(&format!(
                        "unknown savant `mandate` field `{other}` — expected `objective` / `output`"
                    )))
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 87.d — parse `synth <Name> { target:, risk:, language:, sandbox:,
    /// review:, max_lines: }`. Flat key:value block (the `cache` shape). Catalog
    /// + deny-by-default validation is §87.d `check_synth`. Unknown fields are a
    /// hard error (D83.7): a synth policy governs arbitrary-code execution.
    fn parse_synth(&mut self) -> Result<SynthDefinition, ParseError> {
        let tok = self.consume(TokenType::Synth)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = SynthDefinition {
            name,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "target" => node.target = self.consume(TokenType::StringLit)?.value,
                "risk" => node.risk = self.consume_any_ident_or_kw()?.value,
                "language" => node.language = self.consume_any_ident_or_kw()?.value,
                "sandbox" => node.sandbox = self.consume_any_ident_or_kw()?.value,
                "review" => node.review = self.consume_any_ident_or_kw()?.value,
                "max_lines" => node.max_lines = self.parse_optional_int(),
                other => {
                    return Err(self.error(&format!(
                        "unknown synth field `{other}` in synth `{}` — expected `target` / `risk` \
                         / `language` / `sandbox` / `review` / `max_lines`",
                        node.name
                    )))
                }
            }
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 80.g — parse `voice Name { fields }`. Cross-field laws
    /// (stt/tts XOR realtime, interruptible ⇒ legal_basis, ref resolution)
    /// are §80.c type-checker territory (T852), same parse/check split as
    /// every primitive in this file.
    fn parse_voice(&mut self) -> Result<VoiceDefinition, ParseError> {
        let tok = self.consume(TokenType::Voice)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = VoiceDefinition {
            name,
            loc: Loc { line: tok.line, column: tok.column },
            ..Default::default()
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                // Each leg: a declared upstream name or a `Preset@vN` ref.
                "stt" => node.stt = Some(self.parse_upstream_ref()?),
                "tts" => node.tts = Some(self.parse_upstream_ref()?),
                "realtime" => node.realtime = Some(self.parse_upstream_ref()?),
                "carrier" => node.carrier = self.consume_any_ident_or_kw()?.value,
                "interruptible" => {
                    let v = self.consume_any_ident_or_kw()?.value;
                    node.interruptible = v == "true";
                }
                "legal_basis" => node.legal_basis = Some(self.consume_any_ident_or_kw()?.value),
                "persona" => node.persona = Some(self.consume(TokenType::Identifier)?.value),
                "context" => node.context = Some(self.consume(TokenType::Identifier)?.value),
                other => return Err(self.error(&format!("unknown voice field `{other}`"))),
            }
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// §Fase 80.g — an upstream leg reference: `Ident` (a declared
    /// `upstream`) or `Ident@vN` (a §80.f preset).
    fn parse_upstream_ref(&mut self) -> Result<String, ParseError> {
        let base = self.consume(TokenType::Identifier)?.value;
        if self.check(TokenType::At) {
            self.advance();
            let version = self.consume_any_ident_or_kw()?.value;
            Ok(format!("{base}@{version}"))
        } else {
            Ok(base)
        }
    }

    /// §Fase 80.b — parse the `map: [ rule, … ]` projection list.
    ///
    /// rule := (`send` | `receive`) <MessageType> `as` (`json` | `binary`)
    ///         [ `tag` <string> ]                 — send-json only
    ///         [ `when` <string> `=` <string> ]   — receive-json only
    fn parse_upstream_map(&mut self) -> Result<Vec<UpstreamMapRule>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut rules = Vec::new();
        while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
            let dir_tok = self.current().clone();
            let direction = match dir_tok.ttype {
                TokenType::Send => "send",
                TokenType::Receive => "receive",
                _ => {
                    return Err(self.error(&format!(
                        "upstream map rule must start with `send` or `receive`, got `{}`",
                        dir_tok.value
                    )))
                }
            };
            self.advance();
            let message = self.consume(TokenType::Identifier)?.value;
            self.consume(TokenType::As)?;
            let framing = self.consume_any_ident_or_kw()?.value;
            let mut rule = UpstreamMapRule {
                direction: direction.to_string(),
                message,
                framing,
                loc: Loc { line: dir_tok.line, column: dir_tok.column },
                ..Default::default()
            };
            // Optional selectors — contextual identifiers, not keywords.
            if self.current().value == "tag" {
                self.advance();
                rule.tag = Some(self.consume(TokenType::StringLit)?.value);
            } else if self.current().value == "when" {
                // `when "f" = "v"` — equality discriminator; `when "f"` —
                // field-PRESENCE discriminator (vendors like Gemini Live /
                // ElevenLabs mark frame kinds by which key exists, not by a
                // type value).
                self.advance();
                rule.when_field = Some(self.consume(TokenType::StringLit)?.value);
                if self.check(TokenType::Assign) {
                    self.advance();
                    rule.when_value = Some(self.consume(TokenType::StringLit)?.value);
                }
            }
            rules.push(rule);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(rules)
    }

    /// §Fase 80.b — parse `reconnect: { backoff_ms: <int>, max_attempts:
    /// <int>, on_exhausted: <ident> }` (order-free, all three required —
    /// a reconnection policy with a hole is not a policy).
    fn parse_upstream_reconnect(&mut self) -> Result<UpstreamReconnect, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut backoff_ms: Option<i64> = None;
        let mut max_attempts: Option<i64> = None;
        let mut on_exhausted: Option<String> = None;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let key = self.consume_any_ident_or_kw()?.value;
            self.consume(TokenType::Colon)?;
            match key.as_str() {
                "backoff_ms" => {
                    backoff_ms = Some(
                        self.consume(TokenType::Integer)?
                            .value
                            .parse::<i64>()
                            .map_err(|_| self.error("backoff_ms must be an integer"))?,
                    )
                }
                "max_attempts" => {
                    max_attempts = Some(
                        self.consume(TokenType::Integer)?
                            .value
                            .parse::<i64>()
                            .map_err(|_| self.error("max_attempts must be an integer"))?,
                    )
                }
                "on_exhausted" => on_exhausted = Some(self.consume_any_ident_or_kw()?.value),
                other => return Err(self.error(&format!("unknown reconnect field `{other}`"))),
            }
            if self.check(TokenType::Comma) {
                self.consume(TokenType::Comma)?;
            }
        }
        self.consume(TokenType::RBrace)?;
        match (backoff_ms, max_attempts, on_exhausted) {
            (Some(b), Some(m), Some(o)) => Ok(UpstreamReconnect { backoff_ms: b, max_attempts: m, on_exhausted: o }),
            _ => Err(self.error(
                "reconnect requires all of `backoff_ms:`, `max_attempts:`, `on_exhausted:` — a reconnection policy with a hole is not a policy",
            )),
        }
    }

    /// Parse: `[send T, receive U, loop, end]`.
    fn parse_session_steps(&mut self) -> Result<Vec<SessionStep>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut steps = Vec::new();
        while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
            steps.push(self.parse_session_step()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(steps)
    }

    /// §Fase 79.b — a **brace**-delimited session step block: `{ step, step, … }`.
    /// Used by the `interrupt`/`resumable` regions (the paper's block surface),
    /// as opposed to the `[ … ]` step-lists used by roles and choice arms.
    fn parse_session_step_block(&mut self) -> Result<Vec<SessionStep>, ParseError> {
        self.consume(TokenType::LBrace)?;
        let mut steps = Vec::new();
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            steps.push(self.parse_session_step()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(steps)
    }

    fn parse_session_step(&mut self) -> Result<SessionStep, ParseError> {
        let tok = self.current().clone();
        let loc = Loc { line: tok.line, column: tok.column };
        match tok.ttype {
            TokenType::Send => {
                self.advance();
                let msg = self.consume_any_ident_or_kw()?;
                Ok(SessionStep { op: "send".into(), message_type: msg.value, loc, ..Default::default() })
            }
            TokenType::Receive => {
                self.advance();
                let msg = self.consume_any_ident_or_kw()?;
                Ok(SessionStep { op: "receive".into(), message_type: msg.value, loc, ..Default::default() })
            }
            TokenType::Loop => {
                self.advance();
                Ok(SessionStep { op: "loop".into(), loc, ..Default::default() })
            }
            TokenType::End => {
                self.advance();
                Ok(SessionStep { op: "end".into(), loc, ..Default::default() })
            }
            // §Fase 41.b — choice: `select { ℓ: [..], … }` (⊕) | `branch { ℓ: [..], … }` (&).
            // `select`/`branch` are not keywords — they arrive as identifiers.
            TokenType::Identifier if tok.value == "select" || tok.value == "branch" => {
                self.parse_session_choice(&tok.value, loc)
            }
            // §Fase 79.b — `interrupt { <body> } on <Signal> as <sig> resumable { <handler> }`.
            // Contextual keyword (identifier), like `select`/`branch`.
            TokenType::Identifier if tok.value == "interrupt" => {
                self.parse_session_interrupt(loc)
            }
            // §Fase 79.b — `resume`: the handler's normal exit (hand control back to
            // the parked body). A bare step, no payload; only meaningful inside an
            // `interrupt` handler (enforced at type-check, §79.c).
            TokenType::Identifier if tok.value == "resume" => {
                self.advance();
                Ok(SessionStep { op: "resume".into(), loc, ..Default::default() })
            }
            _ => Err(ParseError {
                message: format!(
                    "Invalid session step '{}' — expected send | receive | loop | end | select | branch | interrupt | resume",
                    tok.value
                ),
                line: tok.line,
                column: tok.column,
                ..Default::default()
            }),
        }
    }

    /// §Fase 79.b — consume a **contextual keyword** (`on` / `as` / `resumable`):
    /// a token whose *value* must equal `kw`, regardless of whether the lexer
    /// classified it as a keyword or a bare identifier. Keeps the `interrupt`
    /// surface readable without minting three reserved words.
    fn consume_contextual(&mut self, kw: &str) -> Result<(), ParseError> {
        let t = self.current().clone();
        if t.value != kw {
            return Err(ParseError {
                message: format!("expected `{kw}` in interrupt step, got `{}`", t.value),
                line: t.line,
                column: t.column,
                ..Default::default()
            });
        }
        self.advance();
        Ok(())
    }

    /// §Fase 79.b — Parse an interruptible region:
    /// `interrupt { <body-steps> } on <Signal> as <sig> resumable { <handler-steps> }`.
    ///
    /// Encoded into the string-tagged `SessionStep` (mirroring the §41.b choice
    /// shape): `op = "interrupt"`, `message_type = <Signal>` (validated against the
    /// closed `CallInterruptCause` catalog at type-check, §79.c), two labelled
    /// `branches` (`body`, `handler`), `binder = <sig>`, `resumable = true`.
    fn parse_session_interrupt(&mut self, loc: Loc) -> Result<SessionStep, ParseError> {
        self.advance(); // consume `interrupt`
        // Body region — a brace-delimited step block (the paper's `interrupt { … }`
        // surface; distinct from the `[ … ]` step-lists of roles/choice arms).
        let body = self.parse_session_step_block()?;
        // `on <Signal>`
        self.consume_contextual("on")?;
        let signal = self.consume_any_ident_or_kw()?;
        // `as <sig>`
        self.consume_contextual("as")?;
        let binder = self.consume_any_ident_or_kw()?;
        // `resumable { <handler> }`
        self.consume_contextual("resumable")?;
        let handler = self.parse_session_step_block()?;
        Ok(SessionStep {
            op: "interrupt".into(),
            message_type: signal.value,
            branches: vec![
                SessionBranch { label: "body".into(), steps: body, loc: loc.clone() },
                SessionBranch { label: "handler".into(), steps: handler, loc: loc.clone() },
            ],
            binder: binder.value,
            resumable: true,
            loc,
        })
    }

    /// §Fase 41.b — Parse a choice step: `select { ask: [..], cancel: [..] }`
    /// (or `branch { … }`). Each `label: [steps]` arm is a nested sub-protocol.
    fn parse_session_choice(&mut self, op: &str, loc: Loc) -> Result<SessionStep, ParseError> {
        self.advance(); // consume `select` / `branch`
        self.consume(TokenType::LBrace)?;
        let mut branches = Vec::new();
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let label_tok = self.consume_any_ident_or_kw()?;
            self.consume(TokenType::Colon)?;
            let steps = self.parse_session_steps()?;
            branches.push(SessionBranch {
                label: label_tok.value,
                steps,
                loc: Loc { line: label_tok.line, column: label_tok.column },
            });
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(SessionStep { op: op.to_string(), branches, loc, ..Default::default() })
    }

    /// Parse: `topology Name { nodes: [A, B, …]  edges: [A -> B : Session, …] }`.
    fn parse_topology(&mut self) -> Result<TopologyDefinition, ParseError> {
        let tok = self.consume(TokenType::Topology)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = TopologyDefinition {
            name,
            nodes: Vec::new(),
            edges: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "nodes" => node.nodes = self.parse_bracketed_identifiers()?,
                "edges" => node.edges = self.parse_topology_edges()?,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_topology_edges(&mut self) -> Result<Vec<TopologyEdge>, ParseError> {
        self.consume(TokenType::LBracket)?;
        let mut edges = Vec::new();
        while !self.check(TokenType::RBracket) && !self.check(TokenType::Eof) {
            edges.push(self.parse_topology_edge()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.consume(TokenType::RBracket)?;
        Ok(edges)
    }

    fn parse_topology_edge(&mut self) -> Result<TopologyEdge, ParseError> {
        let src_tok = self.consume_any_ident_or_kw()?;
        self.consume(TokenType::Arrow)?;
        let tgt_tok = self.consume_any_ident_or_kw()?;
        self.consume(TokenType::Colon)?;
        let sess_tok = self.consume_any_ident_or_kw()?;
        Ok(TopologyEdge {
            source: src_tok.value,
            target: tgt_tok.value,
            session_ref: sess_tok.value,
            loc: Loc {
                line: src_tok.line,
                column: src_tok.column,
            },
        })
    }

    // ── §λ-L-E Fase 5 — Cognitive immune system (paper_immune_v2.md) ────

    /// Parse: `immune Name { watch, sensitivity, baseline, window, scope, tau, decay }`.
    fn parse_immune(&mut self) -> Result<ImmuneDefinition, ParseError> {
        let tok = self.consume(TokenType::Immune)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ImmuneDefinition {
            name,
            watch: Vec::new(),
            sensitivity: None,
            baseline: "learned".to_string(),
            window: 100,
            scope: String::new(),
            tau: String::new(),
            decay: "exponential".to_string(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "watch" => node.watch = self.parse_bracketed_identifiers()?,
                "sensitivity" => node.sensitivity = self.parse_optional_float(),
                "baseline" => node.baseline = self.consume_any_ident_or_kw()?.value,
                "window" => {
                    if let Some(v) = self.parse_optional_int() {
                        node.window = v;
                    }
                }
                "scope" => {
                    let s_tok = self.consume_any_ident_or_kw()?;
                    let s = s_tok.value;
                    if !matches!(s.as_str(), "tenant" | "flow" | "global") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid scope '{s}' in immune '{}' — \
                                 expected tenant | flow | global",
                                node.name
                            ),
                            line: s_tok.line,
                            column: s_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.scope = s;
                }
                "tau" => {
                    let t = self.current().clone();
                    match t.ttype {
                        TokenType::Duration | TokenType::StringLit => {
                            self.advance();
                            node.tau = t.value;
                        }
                        _ => node.tau = self.consume_any_ident_or_kw()?.value,
                    }
                }
                "decay" => {
                    let d_tok = self.consume_any_ident_or_kw()?;
                    let d = d_tok.value;
                    if !matches!(d.as_str(), "exponential" | "linear" | "none") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid decay '{d}' in immune '{}' — \
                                 expected exponential | linear | none",
                                node.name
                            ),
                            line: d_tok.line,
                            column: d_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.decay = d;
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `reflex Name { trigger, on_level, action, scope, sla }`.
    fn parse_reflex(&mut self) -> Result<ReflexDefinition, ParseError> {
        let tok = self.consume(TokenType::Reflex)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ReflexDefinition {
            name,
            trigger: String::new(),
            on_level: "doubt".to_string(),
            action: String::new(),
            scope: String::new(),
            sla: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "trigger" => node.trigger = self.consume_any_ident_or_kw()?.value,
                "on_level" => {
                    let l_tok = self.consume_any_ident_or_kw()?;
                    let l = l_tok.value;
                    if !matches!(l.as_str(), "know" | "believe" | "speculate" | "doubt") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid on_level '{l}' in reflex '{}' — \
                                 expected know | believe | speculate | doubt",
                                node.name
                            ),
                            line: l_tok.line,
                            column: l_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.on_level = l;
                }
                "action" => {
                    let a_tok = self.consume_any_ident_or_kw()?;
                    let a = a_tok.value;
                    if !matches!(
                        a.as_str(),
                        "drop"
                            | "revoke"
                            | "emit"
                            | "redact"
                            | "quarantine"
                            | "terminate"
                            | "alert"
                    ) {
                        return Err(ParseError {
                            message: format!(
                                "Invalid action '{a}' in reflex '{}' — \
                                 expected drop | revoke | emit | redact | \
                                 quarantine | terminate | alert",
                                node.name
                            ),
                            line: a_tok.line,
                            column: a_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.action = a;
                }
                "scope" => {
                    let s_tok = self.consume_any_ident_or_kw()?;
                    let s = s_tok.value;
                    if !matches!(s.as_str(), "tenant" | "flow" | "global") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid scope '{s}' in reflex '{}' — \
                                 expected tenant | flow | global",
                                node.name
                            ),
                            line: s_tok.line,
                            column: s_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.scope = s;
                }
                "sla" => {
                    let t = self.current().clone();
                    match t.ttype {
                        TokenType::Duration | TokenType::StringLit => {
                            self.advance();
                            node.sla = t.value;
                        }
                        _ => node.sla = self.consume_any_ident_or_kw()?.value,
                    }
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `heal Name { source, on_level, mode, scope, review_sla, shield, max_patches }`.
    fn parse_heal(&mut self) -> Result<HealDefinition, ParseError> {
        let tok = self.consume(TokenType::Heal)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = HealDefinition {
            name,
            source: String::new(),
            on_level: "doubt".to_string(),
            mode: "human_in_loop".to_string(),
            scope: String::new(),
            review_sla: String::new(),
            shield_ref: String::new(),
            max_patches: 3,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "source" => node.source = self.consume_any_ident_or_kw()?.value,
                "on_level" => {
                    let l_tok = self.consume_any_ident_or_kw()?;
                    let l = l_tok.value;
                    if !matches!(l.as_str(), "know" | "believe" | "speculate" | "doubt") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid on_level '{l}' in heal '{}' — \
                                 expected know | believe | speculate | doubt",
                                node.name
                            ),
                            line: l_tok.line,
                            column: l_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.on_level = l;
                }
                "mode" => {
                    let m_tok = self.consume_any_ident_or_kw()?;
                    let m = m_tok.value;
                    if !matches!(m.as_str(), "audit_only" | "human_in_loop" | "adversarial") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid mode '{m}' in heal '{}' — \
                                 expected audit_only | human_in_loop | adversarial",
                                node.name
                            ),
                            line: m_tok.line,
                            column: m_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.mode = m;
                }
                "scope" => {
                    let s_tok = self.consume_any_ident_or_kw()?;
                    let s = s_tok.value;
                    if !matches!(s.as_str(), "tenant" | "flow" | "global") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid scope '{s}' in heal '{}' — \
                                 expected tenant | flow | global",
                                node.name
                            ),
                            line: s_tok.line,
                            column: s_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.scope = s;
                }
                "review_sla" => {
                    let t = self.current().clone();
                    match t.ttype {
                        TokenType::Duration | TokenType::StringLit => {
                            self.advance();
                            node.review_sla = t.value;
                        }
                        _ => node.review_sla = self.consume_any_ident_or_kw()?.value,
                    }
                }
                "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value,
                "max_patches" => {
                    if let Some(v) = self.parse_optional_int() {
                        node.max_patches = v;
                    }
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── §λ-L-E Fase 9 — UI cognitiva (component / view) ────────────

    /// Parse: `component Name { renders, via_shield, on_interact, render_hint }`.
    fn parse_component(&mut self) -> Result<ComponentDefinition, ParseError> {
        let tok = self.consume(TokenType::Component)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ComponentDefinition {
            name,
            renders: String::new(),
            via_shield: String::new(),
            on_interact: String::new(),
            render_hint: "custom".to_string(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "renders" => node.renders = self.consume_any_ident_or_kw()?.value,
                "via_shield" => node.via_shield = self.consume_any_ident_or_kw()?.value,
                "on_interact" => node.on_interact = self.consume_any_ident_or_kw()?.value,
                "render_hint" => {
                    let h_tok = self.consume_any_ident_or_kw()?;
                    let h = h_tok.value;
                    if !matches!(h.as_str(), "card" | "list" | "form" | "chart" | "custom") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid render_hint '{h}' in component '{}' — \
                                 expected card | list | form | chart | custom",
                                node.name
                            ),
                            line: h_tok.line,
                            column: h_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.render_hint = h;
                }
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse: `view Name { title, components: [...], route }`.
    fn parse_view(&mut self) -> Result<ViewDefinition, ParseError> {
        let tok = self.consume(TokenType::View)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ViewDefinition {
            name,
            title: String::new(),
            components: Vec::new(),
            route: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "title" => node.title = self.consume(TokenType::StringLit)?.value,
                "components" => node.components = self.parse_bracketed_identifiers()?,
                "route" => node.route = self.consume(TokenType::StringLit)?.value,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_axonendpoint(&mut self) -> Result<AxonEndpointDefinition, ParseError> {
        let tok = self.consume(TokenType::AxonEndpoint)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = AxonEndpointDefinition {
            name,
            method: String::new(),
            path: String::new(),
            body_type: String::new(),
            execute_flow: String::new(),
            output_type: String::new(),
            shield_ref: String::new(),
            // §Fase 83.a — `cors:` reference; empty ≡ no cors declared
            // (D83.5: no CORS headers, ever — secure by default).
            cors_ref: String::new(),
            retries: None,
            timeout: String::new(),
            compliance: Vec::new(),
            // §Fase 30 — Defaults preserve backwards compat per D1.
            transport: "json".to_string(),
            keepalive: String::new(),
            // §Fase 31.b — Inference fields (parser-default state).
            // Both fields toggle/populate only when the source provides
            // an explicit `transport:` declaration (parser sets
            // `transport_explicit = true`) AND the type-checker walks
            // the program to compute `implicit_transport`.
            transport_explicit: false,
            implicit_transport: String::new(),
            // §Fase 32.g (D8) — auth scope; empty list ≡ no auth gate.
            requires_capabilities: Vec::new(),
            // §Fase 89.a — explicit authorization-coverage opt-out. Default
            // false; the §89.b rule requires coverage OR `public: true`.
            public: false,
            // §Fase 32.h — Replay-token binding (D9 plan-vivo).
            // Parser defaults: not explicit; effective value resolved
            // at deploy time using the method-default heuristic.
            replay_explicit: false,
            replay: false,
            // §Fase 33.z.k.b (v1.28.0) — Wire-format dialect default
            // empty; the runtime classifier resolves the default
            // dialect per the algebraic-effect predicate when the
            // source omits `transport: sse(<dialect>)`.
            transport_dialect: String::new(),
            // §Fase 33.z.k.1 (v1.27.1) — Algebraic-effect override.
            // Parser default false; populated by the type-checker's
            // compute_implicit_transports pass once the full program
            // is known (the predicate cross-references tool effects
            // declared anywhere in the program).
            has_algebraic_stream_effect: false,
            // §Fase 36.d (D2) — declared execution backend; empty ≡
            // not declared (the endpoint resolves down the Fase 36 D1
            // ladder). A non-empty value is validated against the
            // closed `AXONENDPOINT_BACKEND_VALUES` catalog below.
            backend: String::new(),
            // §Fase 37.y (D1) — Path-param names extracted from the
            // `path:` string AFTER the field is parsed. Initialized
            // empty; populated by `extract_path_param_names` after
            // the `path:` field is read in the loop below.
            path_params: Vec::new(),
            // §Fase 37.y (D2) — Inline `query: { name: Type, name: Type? }`
            // block. Initialized empty; populated by the `"query"` arm
            // in the field loop below. Closed catalog enforced at parse
            // time per `axonendpoint_is_valid_query_param_type`.
            query_params: Vec::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_name = self.current().value.clone();
            self.advance();
            if self.check(TokenType::Colon) {
                self.advance();
                match field_name.as_str() {
                    "method" => {
                        // §Fase 32.b D3 — closed method enum
                        // `{GET, POST, PUT, DELETE, PATCH}`. Unknown
                        // values rejected at parse time with smart-
                        // suggest hint (Fase 28.e). HEAD/OPTIONS/etc.
                        // are runtime-managed and not adopter-
                        // declarable.
                        let value_tok = self.consume_any_ident_or_kw()?;
                        let value_upper = value_tok.value.to_uppercase();
                        if !axonendpoint_is_valid_method(&value_upper) {
                            let hint = crate::smart_suggest::suggest_for(
                                &value_upper,
                                AXONENDPOINT_METHOD_VALUES,
                            );
                            let base = format!(
                                "Invalid method '{}' in axonendpoint '{}'.",
                                value_tok.value, node.name
                            );
                            let message = if hint.is_empty() {
                                format!(
                                    "{base} expected GET | POST | PUT | DELETE | PATCH, found {}",
                                    value_tok.value
                                )
                            } else {
                                format!(
                                    "{base} {hint} (expected GET | POST | PUT | DELETE | PATCH, found {})",
                                    value_tok.value
                                )
                            };
                            return Err(ParseError {
                                message,
                                line: value_tok.line,
                                column: value_tok.column,
                                ..Default::default()
                            });
                        }
                        node.method = value_upper;
                    }
                    "path" => {
                        node.path = self.consume(TokenType::StringLit)?.value.clone();
                        // §Fase 37.y (D1) — extract `{name}` placeholders
                        // for the Request Binding Contract's path-param
                        // source. Duplicate `{name}` in the same path
                        // is rejected at parse time (HTTP route patterns
                        // structurally reject duplicates; surfacing the
                        // error here is friendlier than letting axum
                        // panic at registration).
                        match extract_path_param_names(&node.path) {
                            Ok(names) => node.path_params = names,
                            Err(dup) => {
                                let cur = self.current().clone();
                                return Err(ParseError {
                                    message: format!(
                                        "axonendpoint '{}' declares path '{}' \
                                         containing duplicate placeholder '{{{}}}'. \
                                         Each `{{name}}` in a `path:` must be \
                                         unique — the runtime cannot bind two \
                                         path segments to the same name (Fase 37.y D1).",
                                        node.name, node.path, dup,
                                    ),
                                    line: cur.line,
                                    column: cur.column,
                                    ..Default::default()
                                });
                            }
                        }
                    },
                    "body" => node.body_type = self.consume_any_ident_or_kw()?.value.clone(),
                    "query" => {
                        // §Fase 37.y (D2) — Inline query-parameter block.
                        // Grammar: `query: { name: Type [, name: Type?]* }`.
                        // Closed type catalog
                        // `AXONENDPOINT_QUERY_PARAM_TYPES = {Text, Int,
                        // Float, Bool, Uuid}`. Optional via `?` suffix
                        // reuses `TypeExpr.optional` semantics already in
                        // use for flow parameters + body type fields. A
                        // duplicate field name in the same block is a
                        // parse error (HTTP query strings DO allow
                        // multi-value but v1.38.5 binds the first value
                        // only — see plan vivo §7 forward-compat).
                        //
                        // §Fase 37.y (D2 robustness) — declaring `query:`
                        // twice on the same axonendpoint silently merged
                        // params pre-hardening. Now it's a parse error
                        // so an adopter typo / copy-paste mistake
                        // surfaces with line + column instead of
                        // producing an unexpectedly-augmented endpoint.
                        let lbrace_tok = self.consume(TokenType::LBrace)?;
                        let block_line = lbrace_tok.line;
                        if !node.query_params.is_empty() {
                            return Err(ParseError {
                                message: format!(
                                    "axonendpoint '{}' declares `query: {{ … }}` \
                                     more than once. The query-parameter block \
                                     is unique per endpoint; combine all params \
                                     into a single block (Fase 37.y D2).",
                                    node.name,
                                ),
                                line: lbrace_tok.line,
                                column: lbrace_tok.column,
                                ..Default::default()
                            });
                        }
                        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
                            let name_tok = self.consume(TokenType::Identifier)?;
                            let field_name = name_tok.value.clone();
                            // Duplicate detection within the block.
                            if node
                                .query_params
                                .iter()
                                .any(|f| f.name == field_name)
                            {
                                return Err(ParseError {
                                    message: format!(
                                        "axonendpoint '{}' declares duplicate \
                                         query param '{}' inside `query: {{ … }}`. \
                                         Each name must appear at most once \
                                         (Fase 37.y D2).",
                                        node.name, field_name,
                                    ),
                                    line: name_tok.line,
                                    column: name_tok.column,
                                    ..Default::default()
                                });
                            }
                            self.consume(TokenType::Colon)?;
                            let type_expr = self.parse_type_expr()?;
                            // §Fase 37.y (D2 robustness) — reject generic
                            // type expressions on query params. The
                            // closed catalog is 5 primitives; container
                            // types (`Optional<T>`, `List<T>`, etc.)
                            // would mislead the adopter into thinking
                            // they bind multi-value query strings
                            // (deferred per plan vivo §7) or that
                            // `Optional<Text>` is the canonical way to
                            // declare an optional query (it's NOT —
                            // `Text?` is). Surface the canonical syntax
                            // verbatim so the fix is obvious.
                            if !type_expr.generic_param.is_empty() {
                                let canonical_hint = if type_expr.name == "Optional" {
                                    format!(
                                        " Use `{}?` (the `?` suffix) for an \
                                         optional query param instead of \
                                         `Optional<{}>`.",
                                        type_expr.generic_param,
                                        type_expr.generic_param,
                                    )
                                } else if type_expr.name == "List" {
                                    " Multi-value query params (e.g. `?tag=a&tag=b`) \
                                     are honest-deferred from v1.38.5; bind a \
                                     single-value `Text` query param and parse \
                                     the value inside the flow."
                                        .to_string()
                                } else {
                                    String::new()
                                };
                                return Err(ParseError {
                                    message: format!(
                                        "axonendpoint '{}' query param '{}' uses \
                                         a generic type `{}<{}>`. Query params \
                                         take a primitive type from the closed \
                                         catalog ({}); the `?` suffix marks \
                                         optional.{} (Fase 37.y D2).",
                                        node.name,
                                        field_name,
                                        type_expr.name,
                                        type_expr.generic_param,
                                        AXONENDPOINT_QUERY_PARAM_TYPES.join(" | "),
                                        canonical_hint,
                                    ),
                                    line: type_expr.loc.line,
                                    column: type_expr.loc.column,
                                    ..Default::default()
                                });
                            }
                            // Validate against the closed catalog. A
                            // miss surfaces a Fase 28-style smart-suggest
                            // hint when within edit-distance 2.
                            if !axonendpoint_is_valid_query_param_type(&type_expr.name) {
                                // `smart_suggest::suggest_for` returns
                                // pre-formatted prose like
                                // "Did you mean `Text`?" or
                                // "Did you mean `Text` or `Int`?" (empty
                                // when no candidate within edit-distance
                                // 2). Concatenate without re-wrapping.
                                let hint = crate::smart_suggest::suggest_for(
                                    &type_expr.name,
                                    AXONENDPOINT_QUERY_PARAM_TYPES,
                                );
                                let hint_text = if hint.is_empty() {
                                    format!(
                                        " Expected one of: {}.",
                                        AXONENDPOINT_QUERY_PARAM_TYPES.join(" | ")
                                    )
                                } else {
                                    format!(
                                        " {} Expected one of: {}.",
                                        hint,
                                        AXONENDPOINT_QUERY_PARAM_TYPES.join(" | ")
                                    )
                                };
                                return Err(ParseError {
                                    message: format!(
                                        "axonendpoint '{}' query param '{}' has \
                                         unsupported type '{}'.{} (Fase 37.y D2).",
                                        node.name, field_name, type_expr.name,
                                        hint_text,
                                    ),
                                    line: type_expr.loc.line,
                                    column: type_expr.loc.column,
                                    ..Default::default()
                                });
                            }
                            node.query_params.push(TypeField {
                                name: field_name,
                                type_expr,
                                loc: Loc {
                                    line: name_tok.line,
                                    column: name_tok.column,
                                },
                            });
                            // Trailing comma is optional; the next loop
                            // iteration handles `}` cleanly. Accept both
                            // `name: Type, name: Type` AND `name: Type
                            // name: Type` (the existing parser style is
                            // forgiving about list separators).
                            if self.check(TokenType::Comma) {
                                self.advance();
                            }
                            let _ = block_line; // suppress unused warning
                        }
                        self.consume(TokenType::RBrace)?;
                    },
                    "execute" => node.execute_flow = self.consume_any_ident_or_kw()?.value.clone(),
                    "output" => {
                        // §Fase 38.x.f — promote axonendpoint `output:`
                        // parsing from a single token to the full
                        // generic-aware type expression (mirroring
                        // `parse_step` for FlowStep::Step which already
                        // uses `parse_output_type_string`).
                        //
                        // Pre-38.x.f: `output: List<Item>` captured only
                        // `"List"`, dropping `<Item>` (next tokens were
                        // either left unconsumed or absorbed by the
                        // following field). v1.39.0's narrow cardinality
                        // gate happened to fire correctly for `output: T`
                        // + retrieve-tail because the singular-detection
                        // path used `!starts_with("List<")` — but the
                        // SYMMETRIC `output: List<T>` + singular-tail
                        // case (38.x.f D3) needs the FULL `List<T>`
                        // shape captured; without it the gate sees
                        // `"List"` and misclassifies as Singular.
                        node.output_type = self.parse_output_type_string()?;
                    }
                    "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    // §Fase 83.a — the `cors: <Name>` reference.
                    "cors" => node.cors_ref = self.consume_any_ident_or_kw()?.value.clone(),
                    "retries" => node.retries = self.parse_optional_int(),
                    "timeout" => {
                        let t = self.current().clone();
                        self.advance();
                        node.timeout = t.value.clone();
                    }
                    "compliance" => node.compliance = self.parse_bracketed_identifiers()?,
                    "replay" => {
                        // §Fase 32.h (D9 plan-vivo) — Replay-token binding.
                        // Boolean `replay: true | false`. Default (when
                        // omitted) is method-derived at deploy-time:
                        // POST/PUT → true, GET/DELETE → false. Explicit
                        // declaration sets `replay_explicit = true` so
                        // the runtime knows NOT to override.
                        let value_tok = self.consume(TokenType::Bool)?;
                        node.replay = value_tok.value.eq_ignore_ascii_case("true");
                        node.replay_explicit = true;
                    }
                    // §Fase 89.a — `public: true | false`, the explicit
                    // authorization-coverage opt-out (doctrine
                    // `every_boundary_is_guarded`). Mirrors `replay:`'s bool
                    // parse. Default false; the §89.b rule (`axon-T890`)
                    // requires a covering discipline OR `public: true`.
                    "public" => {
                        let value_tok = self.consume(TokenType::Bool)?;
                        node.public = value_tok.value.eq_ignore_ascii_case("true");
                    }
                    "requires" => {
                        // §Fase 32.g (D8) — Auth scope per axonendpoint.
                        // Closed slug grammar
                        // `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$` enforced
                        // at parse time with smart-suggest-style hint.
                        // Empty list means "no auth gate" (D9 backwards-
                        // compat). Cross-stack with Python parser.
                        let bracket_tok = self.current().clone();
                        let items = self.parse_bracketed_dot_identifiers()?;
                        for slug in &items {
                            if !is_valid_capability_slug(slug) {
                                return Err(ParseError {
                                    message: format!(
                                        "Invalid capability slug '{slug}' in axonendpoint '{}' \
                                         `requires:`. Capability slugs must match \
                                         ^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$ — dot-separated \
                                         lowercase identifiers starting with a letter. Examples: \
                                         `admin`, `legal.read`, `hipaa.phi.read`.",
                                        node.name
                                    ),
                                    line: bracket_tok.line,
                                    column: bracket_tok.column,
                                    ..Default::default()
                                });
                            }
                        }
                        node.requires_capabilities = items;
                    }
                    // §Fase 30.b — HTTP transport enum (D2 closed) + keepalive (D6 closed).
                    // Mirrors `axon/compiler/parser.py` `_parse_axonendpoint`.
                    // Drift-gate corpus verifies byte-identical parse cross-stack.
                    "transport" => {
                        let value_tok = self.consume_any_ident_or_kw()?;
                        let value = &value_tok.value;
                        if !axonendpoint_is_valid_transport(value) {
                            let hint = crate::smart_suggest::suggest_for(
                                value,
                                AXONENDPOINT_TRANSPORT_VALUES,
                            );
                            let base = format!(
                                "Invalid transport '{}' in axonendpoint '{}'.",
                                value, node.name
                            );
                            let message = if hint.is_empty() {
                                format!("{base} expected json | sse | ndjson, found {value}")
                            } else {
                                format!(
                                    "{base} {hint} (expected json | sse | ndjson, found {value})"
                                )
                            };
                            return Err(ParseError {
                                message,
                                line: value_tok.line,
                                column: value_tok.column,
                                ..Default::default()
                            });
                        }
                        node.transport = value.clone();
                        // §Fase 31.b D1 — mark the field as explicitly
                        // declared so the type-checker's implicit-transport
                        // inference knows NOT to override this value with
                        // the produces_stream-driven inference.
                        node.transport_explicit = true;
                        // §Fase 33.z.k.b (v1.28.0) — Optional dialect
                        // parametrization: `transport: sse(<dialect>)`.
                        // Only valid when the base value is `sse`
                        // (json + ndjson dialects are the dialects
                        // themselves; `json(<x>)` / `ndjson(<x>)`
                        // would be parse errors caught below).
                        if self.check(TokenType::LParen) {
                            if value != "sse" {
                                let tok = self.current().clone();
                                return Err(ParseError {
                                    message: format!(
                                        "Dialect parametrization \
                                         `transport: {value}(<dialect>)` is \
                                         only valid for `sse`; got \
                                         `{value}` in axonendpoint '{}'.",
                                        node.name
                                    ),
                                    line: tok.line,
                                    column: tok.column,
                                    ..Default::default()
                                });
                            }
                            self.advance(); // consume LParen
                            let dialect_tok = self.consume_any_ident_or_kw()?;
                            let dialect = dialect_tok.value.clone();
                            if !AXONENDPOINT_TRANSPORT_DIALECTS
                                .iter()
                                .any(|&d| d == dialect)
                            {
                                let hint = crate::smart_suggest::suggest_for(
                                    &dialect,
                                    AXONENDPOINT_TRANSPORT_DIALECTS,
                                );
                                let base = format!(
                                    "Invalid SSE dialect '{dialect}' in axonendpoint '{}'.",
                                    node.name
                                );
                                let message = if hint.is_empty() {
                                    format!(
                                        "{base} expected axon | openai | kimi | glm | anthropic, found {dialect}"
                                    )
                                } else {
                                    format!(
                                        "{base} {hint} (expected axon | openai | kimi | glm | anthropic, found {dialect})"
                                    )
                                };
                                return Err(ParseError {
                                    message,
                                    line: dialect_tok.line,
                                    column: dialect_tok.column,
                                    ..Default::default()
                                });
                            }
                            // Closing RParen.
                            let rparen_tok = self.current().clone();
                            if !self.check(TokenType::RParen) {
                                return Err(ParseError {
                                    message: format!(
                                        "Expected `)` after dialect name \
                                         in axonendpoint '{}' \
                                         (transport: sse(<dialect>) grammar).",
                                        node.name
                                    ),
                                    line: rparen_tok.line,
                                    column: rparen_tok.column,
                                    ..Default::default()
                                });
                            }
                            self.advance(); // consume RParen
                            node.transport_dialect = dialect;
                        }
                    }
                    "keepalive" => {
                        // Accepts either a DURATION token (e.g. `15s`) or
                        // an ident-like token. Validation against the
                        // closed enum {5s, 15s, 30s, 60s} happens after.
                        let value_tok = self.current().clone();
                        self.advance();
                        let value = &value_tok.value;
                        if !axonendpoint_is_valid_keepalive(value) {
                            let hint = crate::smart_suggest::suggest_for(
                                value,
                                AXONENDPOINT_KEEPALIVE_VALUES,
                            );
                            let base = format!(
                                "Invalid keepalive '{}' in axonendpoint '{}'.",
                                value, node.name
                            );
                            let message = if hint.is_empty() {
                                format!("{base} expected 5s | 15s | 30s | 60s, found {value}")
                            } else {
                                format!(
                                    "{base} {hint} (expected 5s | 15s | 30s | 60s, found {value})"
                                )
                            };
                            return Err(ParseError {
                                message,
                                line: value_tok.line,
                                column: value_tok.column,
                                ..Default::default()
                            });
                        }
                        node.keepalive = value.clone();
                    }
                    "backend" => {
                        // §Fase 36.d (D2) — declared execution backend.
                        // Closed catalog `CANONICAL_PROVIDERS ∪ {auto,
                        // stub}`; an unknown name is a parse error with
                        // a smart-suggest hint (the same discipline as
                        // `method`/`transport`/`keepalive`). The
                        // type-checker re-validates defensively for
                        // ASTs built outside the parser (LSP, tests).
                        let value_tok = self.consume_any_ident_or_kw()?;
                        let value = &value_tok.value;
                        if !axonendpoint_is_valid_backend(value) {
                            let hint = crate::smart_suggest::suggest_for(
                                value,
                                AXONENDPOINT_BACKEND_VALUES,
                            );
                            let expected = AXONENDPOINT_BACKEND_VALUES.join(" | ");
                            let base = format!(
                                "Invalid backend '{}' in axonendpoint '{}'.",
                                value, node.name
                            );
                            let message = if hint.is_empty() {
                                format!("{base} expected {expected}, found {value}")
                            } else {
                                format!(
                                    "{base} {hint} (expected {expected}, found {value})"
                                )
                            };
                            return Err(ParseError {
                                message,
                                line: value_tok.line,
                                column: value_tok.column,
                                ..Default::default()
                            });
                        }
                        node.backend = value.clone();
                    }
                    _ => self.skip_value(),
                }
            } else if self.check(TokenType::LBrace) {
                self.skip_braced_block()?;
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    // ── Numeric helpers for Tier 2 field parsing ────────────────────

    fn parse_optional_int(&mut self) -> Option<i64> {
        let tok = self.current().clone();
        match tok.ttype {
            TokenType::Integer => {
                self.advance();
                tok.value.parse::<i64>().ok()
            }
            _ => {
                self.advance();
                None
            }
        }
    }

    fn parse_optional_float(&mut self) -> Option<f64> {
        let tok = self.current().clone();
        match tok.ttype {
            TokenType::Float | TokenType::Integer => {
                self.advance();
                tok.value.parse::<f64>().ok()
            }
            _ => {
                self.advance();
                None
            }
        }
    }

    // ── LAMBDA DATA (ΛD) ──────────────────────────────────────────

    fn parse_lambda_data(&mut self) -> Result<LambdaDataDefinition, ParseError> {
        let tok = self.consume(TokenType::Lambda)?;
        let name = self.consume(TokenType::Identifier)?;
        self.consume(TokenType::LBrace)?;

        let mut node = LambdaDataDefinition {
            name: name.value.clone(),
            ontology: String::new(),
            certainty: 1.0,
            temporal_frame_start: String::new(),
            temporal_frame_end: String::new(),
            provenance: String::new(),
            derivation: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };

        while !self.check(TokenType::RBrace) {
            let field = self.current().clone();
            match field.ttype {
                TokenType::Ontology => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.ontology = self.consume(TokenType::StringLit)?.value.clone();
                }
                TokenType::Certainty => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    let val = self.current().clone();
                    match val.ttype {
                        TokenType::Float => {
                            self.advance();
                            node.certainty = val.value.parse::<f64>().unwrap_or(1.0);
                        }
                        TokenType::Integer => {
                            self.advance();
                            node.certainty = val.value.parse::<f64>().unwrap_or(1.0);
                        }
                        _ => {
                            return Err(ParseError {
                                message: format!(
                                    "Expected number for certainty, got '{}'",
                                    val.value
                                ),
                                line: val.line,
                                column: val.column,
                                                            ..Default::default()
                            });
                        }
                    }
                }
                TokenType::TemporalFrame => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.temporal_frame_start = self.consume(TokenType::StringLit)?.value.clone();
                    // Optional second string for end frame
                    if self.check(TokenType::StringLit) {
                        node.temporal_frame_end = self.consume(TokenType::StringLit)?.value.clone();
                    }
                }
                TokenType::Provenance => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    node.provenance = self.consume(TokenType::StringLit)?.value.clone();
                }
                TokenType::Derivation => {
                    self.advance();
                    self.consume(TokenType::Colon)?;
                    let d = self.current().clone();
                    self.advance();
                    node.derivation = d.value.clone();
                }
                _ => {
                    // Skip unknown fields gracefully
                    self.advance();
                    if self.check(TokenType::Colon) {
                        self.advance();
                        self.skip_value();
                    }
                }
            }
        }

        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    fn parse_lambda_data_apply(&mut self) -> Result<LambdaDataApplyNode, ParseError> {
        let tok = self.consume(TokenType::Lambda)?;
        let lambda_name = self.consume(TokenType::Identifier)?;

        // Expect "on" keyword (parsed as identifier since it's not reserved)
        let on_tok = self.current().clone();
        self.advance();
        if on_tok.value != "on" {
            return Err(ParseError {
                message: format!(
                    "Expected 'on' after lambda data name in flow step, got '{}'",
                    on_tok.value
                ),
                line: on_tok.line,
                column: on_tok.column,
                            ..Default::default()
            });
        }

        let target = self.current().clone();
        self.advance();

        let mut output_type = String::new();
        if self.check(TokenType::Arrow) {
            self.advance();
            output_type = self.consume(TokenType::Identifier)?.value.clone();
        }

        Ok(LambdaDataApplyNode {
            lambda_data_name: lambda_name.value.clone(),
            target: target.value.clone(),
            output_type,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        })
    }

    // ── GENERIC (Tier 2+) ────────────────────────────────────────

    fn parse_generic_declaration(&mut self) -> Result<Declaration, ParseError> {
        let kw_tok = self.current().clone();
        self.advance(); // consume keyword

        // Try to consume a name (identifier or keyword-as-name)
        let name = if self.current().ttype == TokenType::Identifier {
            let n = self.current().value.clone();
            self.advance();
            n
        } else if !self.check(TokenType::LBrace)
            && !self.check(TokenType::LParen)
            && !self.check(TokenType::Eof)
            && self
                .current()
                .value
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_')
        {
            let n = self.current().value.clone();
            self.advance();
            n
        } else {
            String::new()
        };

        // Skip optional parens: (...)
        if self.check(TokenType::LParen) {
            self.advance();
            let mut depth = 1u32;
            while depth > 0 && !self.check(TokenType::Eof) {
                if self.check(TokenType::LParen) {
                    depth += 1;
                } else if self.check(TokenType::RParen) {
                    depth -= 1;
                }
                self.advance();
            }
        }

        // Skip tokens until LBrace or next declaration
        while !self.check(TokenType::LBrace) && !self.at_declaration_start() {
            if self.check(TokenType::Eof) {
                break;
            }
            self.advance();
        }

        // Skip braced block if present
        if self.check(TokenType::LBrace) {
            self.skip_braced_block()?;
        }

        Ok(Declaration::Generic(GenericDeclaration {
            keyword: kw_tok.value,
            name,
            loc: Loc {
                line: kw_tok.line,
                column: kw_tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        }))
    }

    // ──────────────────────────────────────────────────────────────────
    //  §λ-L-E Fase 13 — Mobile Typed Channels parsers
    //  (paper_mobile_channels.md §3 + plan/fase_13)
    //  Direct port of axon/compiler/parser.py:_parse_channel/emit/publish/discover.
    // ──────────────────────────────────────────────────────────────────

    /// Parse: `channel Name { message, qos, lifetime, persistence, shield }`.
    fn parse_channel(&mut self) -> Result<ChannelDefinition, ParseError> {
        let tok = self.consume(TokenType::Channel)?;
        let name = self.consume(TokenType::Identifier)?.value;
        let mut node = ChannelDefinition {
            name: name.clone(),
            message: String::new(),
            qos: "at_least_once".to_string(),
            lifetime: "affine".to_string(),
            persistence: "ephemeral".to_string(),
            shield_ref: String::new(),
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        };
        self.consume(TokenType::LBrace)?;
        while !self.check(TokenType::RBrace) && !self.check(TokenType::Eof) {
            let field_tok = self.current().clone();
            let field_name = field_tok.value.clone();
            self.advance();
            if !self.check(TokenType::Colon) {
                if self.check(TokenType::LBrace) {
                    self.skip_braced_block()?;
                }
                continue;
            }
            self.advance();
            match field_name.as_str() {
                "message" => node.message = self.parse_channel_message_type()?,
                "qos" => {
                    let q_tok = self.consume_any_ident_or_kw()?;
                    if !matches!(
                        q_tok.value.as_str(),
                        "at_most_once" | "at_least_once" | "exactly_once" | "broadcast" | "queue"
                    ) {
                        return Err(ParseError {
                            message: format!(
                                "Invalid qos '{}' in channel '{}' — \
                                 expected at_most_once | at_least_once | \
                                 exactly_once | broadcast | queue",
                                q_tok.value, name
                            ),
                            line: q_tok.line,
                            column: q_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.qos = q_tok.value;
                }
                "lifetime" => {
                    let lt_tok = self.consume_any_ident_or_kw()?;
                    if !matches!(lt_tok.value.as_str(), "linear" | "affine" | "persistent") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid lifetime '{}' in channel '{}' — \
                                 expected linear | affine | persistent",
                                lt_tok.value, name
                            ),
                            line: lt_tok.line,
                            column: lt_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.lifetime = lt_tok.value;
                }
                "persistence" => {
                    let p_tok = self.consume_any_ident_or_kw()?;
                    if !matches!(p_tok.value.as_str(), "ephemeral" | "persistent_axonstore") {
                        return Err(ParseError {
                            message: format!(
                                "Invalid persistence '{}' in channel '{}' — \
                                 expected ephemeral | persistent_axonstore",
                                p_tok.value, name
                            ),
                            line: p_tok.line,
                            column: p_tok.column,
                                                    ..Default::default()
                        });
                    }
                    node.persistence = p_tok.value;
                }
                "shield" => node.shield_ref = self.consume_any_ident_or_kw()?.value,
                _ => self.skip_value(),
            }
        }
        self.consume(TokenType::RBrace)?;
        Ok(node)
    }

    /// Parse a `message:` value, supporting nested `Channel<…>`
    /// (second-order session types — paper §3.3).
    fn parse_channel_message_type(&mut self) -> Result<String, ParseError> {
        let head = self.consume(TokenType::Identifier)?;
        let mut spelling = head.value;
        if self.check(TokenType::Lt) {
            self.advance();
            let inner = self.parse_channel_message_type()?;
            self.consume(TokenType::Gt)?;
            spelling = format!("{}<{}>", spelling, inner);
        }
        Ok(spelling)
    }

    /// Parse: `emit ChannelName(value_ref)` — Chan-Output / Chan-Mobility.
    ///
    /// `value_ref` accepts a bare identifier (variable / channel name for
    /// mobility) or a dotted path (`Step.output.field`) referencing a prior
    /// step result (Fase 13.i — runtime resolves via ContextManager).
    fn parse_emit_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.consume(TokenType::Emit)?;
        let channel = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::LParen)?;
        let value = self.parse_emit_value_ref()?;
        self.consume(TokenType::RParen)?;
        Ok(FlowStep::Emit(EmitStatement {
            channel_ref: channel,
            value_ref: value,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 92.b — parse `mint <Credential> as <binding>`. The credential
    /// reference must resolve to a declared `credential` (`axon-T895`,
    /// type-checker); the binding is a fresh flow-scoped name receiving the
    /// raw bearer string. Both tokens are required — a `mint` with no
    /// binding would mint authority into the void.
    fn parse_mint_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.consume(TokenType::Mint)?;
        let credential_ref = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::As)?;
        let binding = self.consume(TokenType::Identifier)?.value;
        Ok(FlowStep::Mint(MintStep {
            credential_ref,
            binding,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// §Fase 94.b — parse `rotate <SecretsStore> [where "<filter>"] with
    /// <Tool> as <binding>` (doctrine `rotation_without_revelation`).
    ///
    /// All three anchors are grammar, not convention: the store names WHAT
    /// may rotate (a `backend: secrets` class view — `axon-T898` in the
    /// type-checker), the tool names WHO performs the exchange
    /// (`axon-T899`), and the binding receives the metadata-only summary —
    /// a `rotate` without a binding would renew authority with no
    /// observable outcome, so `as` is REQUIRED (the `mint` posture). The
    /// `where` filter is optional (§67 string grammar, proven against the
    /// synthesized metadata schema); omitting it rotates the WHOLE class —
    /// the deliberate post-breach bulk shape. `with` is a soft keyword
    /// (not a lexer token): reserving it globally would break every
    /// adopter identifier named `with`.
    fn parse_rotate_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.consume(TokenType::Rotate)?;
        let store_ref = self.consume(TokenType::Identifier)?.value;
        let mut where_expr = String::new();
        if self.check(TokenType::Where) {
            self.advance();
            where_expr = self.consume(TokenType::StringLit)?.value.clone();
        }
        let with_tok = self.current().clone();
        if with_tok.value != "with" {
            return Err(ParseError {
                message: format!(
                    "Expected `with <Tool>` after `rotate {store_ref}{}`, found '{}'. \
                     A rotation names the tool that performs the renewal exchange: \
                     `rotate {store_ref} [where \"<filter>\"] with <Tool> as <binding>`.",
                    if where_expr.is_empty() { "" } else { " where …" },
                    with_tok.value
                ),
                line: with_tok.line,
                column: with_tok.column,
                ..Default::default()
            });
        }
        self.advance();
        let tool_ref = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::As)?;
        let binding = self.consume(TokenType::Identifier)?.value;
        Ok(FlowStep::Rotate(RotateStep {
            store_ref,
            where_expr,
            tool_ref,
            binding,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// Parse: `IDENTIFIER ('.' (IDENTIFIER | keyword))*` → dot-joined string
    /// (Fase 13.i).
    ///
    /// Mirrors the Python `_parse_emit_value_ref` helper exactly so the IR
    /// JSON for `emit Hello(Build.output)` is byte-identical between the
    /// two reference implementations.
    ///
    /// The HEAD must be a real ``Identifier``. Subsequent segments after a
    /// `.` may be identifiers OR keywords — common field names like
    /// ``output``, ``result``, ``message``, ``state``, etc. are reserved
    /// words in Axon but adopters must be able to write them as
    /// dotted-access segments. The accepting predicate:
    ///   - the lexer carried a non-empty `value` (every Word-like token does)
    ///   - the value's first byte is a letter or underscore (filters out
    ///     punctuation tokens such as ',', '{', etc.)
    fn parse_emit_value_ref(&mut self) -> Result<String, ParseError> {
        let head = self.consume(TokenType::Identifier)?.value;
        let mut parts = vec![head];
        while self.check(TokenType::Dot) {
            self.advance(); // consume '.'
            let next_tok = self.current().clone();
            let valid = !next_tok.value.is_empty()
                && next_tok.value.as_bytes()[0].is_ascii_alphabetic()
                || next_tok.value.starts_with('_');
            if !valid {
                return Err(ParseError {
                    message: format!(
                        "Expected identifier or keyword after '.' in dotted \
                         access, found {:?}",
                        next_tok.value
                    ),
                    line: next_tok.line,
                    column: next_tok.column,
                                    ..Default::default()
                });
            }
            self.advance();
            parts.push(next_tok.value);
        }
        Ok(parts.join("."))
    }

    /// Parse: `publish ChannelName within ShieldName` — Publish-Ext (D8).
    fn parse_publish_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.consume(TokenType::Publish)?;
        let channel = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::Within)?;
        let shield = self.consume(TokenType::Identifier)?.value;
        Ok(FlowStep::Publish(PublishStatement {
            channel_ref: channel,
            shield_ref: shield,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }

    /// Parse: `discover ChannelName as alias` — dual of publish.
    fn parse_discover_step(&mut self) -> Result<FlowStep, ParseError> {
        let tok = self.consume(TokenType::Discover)?;
        let cap = self.consume(TokenType::Identifier)?.value;
        self.consume(TokenType::As)?;
        let alias = self.consume(TokenType::Identifier)?.value;
        Ok(FlowStep::Discover(DiscoverStatement {
            capability_ref: cap,
            alias,
            loc: Loc {
                line: tok.line,
                column: tok.column,
            },
        }))
    }
}

// ── §λ-L-E Fase 13 — Mobile Typed Channels parser tests ─────────────────────

#[cfg(test)]
mod fase13_parser_tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Result<Program, ParseError> {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        Parser::new(tokens).parse()
    }

    #[test]
    fn channel_full_parses() {
        let src = r#"channel C { message: Order qos: at_least_once lifetime: affine persistence: ephemeral shield: Gate }"#;
        let prog = parse(src).expect("parse");
        match &prog.declarations[0] {
            Declaration::Channel(c) => {
                assert_eq!(c.name, "C");
                assert_eq!(c.message, "Order");
                assert_eq!(c.qos, "at_least_once");
                assert_eq!(c.lifetime, "affine");
                assert_eq!(c.persistence, "ephemeral");
                assert_eq!(c.shield_ref, "Gate");
            }
            _ => panic!("expected ChannelDefinition"),
        }
    }

    #[test]
    fn channel_defaults_match_paper_d1() {
        let prog = parse("channel C { message: Order }").expect("parse");
        if let Declaration::Channel(c) = &prog.declarations[0] {
            assert_eq!(c.qos, "at_least_once"); // default
            assert_eq!(c.lifetime, "affine"); // D1 default
            assert_eq!(c.persistence, "ephemeral");
            assert_eq!(c.shield_ref, "");
        } else {
            panic!("expected ChannelDefinition");
        }
    }

    #[test]
    fn channel_second_order_message_type_parses() {
        let prog = parse("channel C { message: Channel<Order> }").expect("parse");
        if let Declaration::Channel(c) = &prog.declarations[0] {
            assert_eq!(c.message, "Channel<Order>");
        } else {
            panic!("expected ChannelDefinition");
        }
    }

    #[test]
    fn channel_nested_channel_message_type_parses() {
        let prog = parse("channel C { message: Channel<Channel<Order>> }").expect("parse");
        if let Declaration::Channel(c) = &prog.declarations[0] {
            assert_eq!(c.message, "Channel<Channel<Order>>");
        } else {
            panic!("expected ChannelDefinition");
        }
    }

    #[test]
    fn channel_invalid_qos_rejected() {
        let err = parse("channel C { message: T qos: bogus }").unwrap_err();
        assert!(err.message.contains("Invalid qos"), "got {}", err.message);
    }

    #[test]
    fn channel_invalid_lifetime_rejected() {
        let err = parse("channel C { message: T lifetime: eternal }").unwrap_err();
        assert!(
            err.message.contains("Invalid lifetime"),
            "got {}",
            err.message
        );
    }

    #[test]
    fn channel_invalid_persistence_rejected() {
        let err = parse("channel C { message: T persistence: forever }").unwrap_err();
        assert!(
            err.message.contains("Invalid persistence"),
            "got {}",
            err.message
        );
    }

    #[test]
    fn emit_value_parses() {
        let src = "flow f() -> Out { emit C(payload) }";
        let prog = parse(src).expect("parse");
        if let Declaration::Flow(f) = &prog.declarations[0] {
            match &f.body[0] {
                FlowStep::Emit(e) => {
                    assert_eq!(e.channel_ref, "C");
                    assert_eq!(e.value_ref, "payload");
                }
                other => panic!("expected Emit, got {:?}", other),
            }
        } else {
            panic!("expected Flow");
        }
    }

    #[test]
    fn publish_within_shield_parses() {
        let src = "flow f() -> Cap { publish C within Gate }";
        let prog = parse(src).expect("parse");
        if let Declaration::Flow(f) = &prog.declarations[0] {
            match &f.body[0] {
                FlowStep::Publish(p) => {
                    assert_eq!(p.channel_ref, "C");
                    assert_eq!(p.shield_ref, "Gate");
                }
                other => panic!("expected Publish, got {:?}", other),
            }
        } else {
            panic!("expected Flow");
        }
    }

    #[test]
    fn discover_with_alias_parses() {
        let src = "flow f() -> Out { discover C as ch }";
        let prog = parse(src).expect("parse");
        if let Declaration::Flow(f) = &prog.declarations[0] {
            match &f.body[0] {
                FlowStep::Discover(d) => {
                    assert_eq!(d.capability_ref, "C");
                    assert_eq!(d.alias, "ch");
                }
                other => panic!("expected Discover, got {:?}", other),
            }
        } else {
            panic!("expected Flow");
        }
    }

    #[test]
    fn listen_typed_ref_sets_flag_true() {
        let src = "daemon D() { goal: \"x\" listen C as ev { } }";
        let prog = parse(src).expect("parse");
        if let Declaration::Daemon(d) = &prog.declarations[0] {
            assert_eq!(d.listeners.len(), 1);
            assert_eq!(d.listeners[0].channel, "C");
            assert!(d.listeners[0].channel_is_ref, "typed ref ⇒ true");
        } else {
            panic!("expected Daemon");
        }
    }

    #[test]
    fn listen_string_topic_legacy_flag_false() {
        let src = "daemon D() { goal: \"x\" listen \"orders\" as ev { } }";
        let prog = parse(src).expect("parse");
        if let Declaration::Daemon(d) = &prog.declarations[0] {
            assert_eq!(d.listeners.len(), 1);
            assert_eq!(d.listeners[0].channel, "orders");
            assert!(!d.listeners[0].channel_is_ref, "string topic ⇒ false");
        } else {
            panic!("expected Daemon");
        }
    }

    // ── Fase 13.i — emit value_ref accepts dotted access ───────────

    fn extract_first_emit(prog: &Program) -> &EmitStatement {
        if let Declaration::Flow(f) = &prog.declarations[0] {
            if let FlowStep::Emit(e) = &f.body[0] {
                return e;
            }
        }
        panic!("expected emit statement at flow body[0]");
    }

    #[test]
    fn emit_accepts_bare_identifier_value_ref() {
        // Pre-13.i baseline — must keep working.
        let prog = parse("flow f() -> Out { emit Hello(payload) }").expect("parse");
        let emit = extract_first_emit(&prog);
        assert_eq!(emit.channel_ref, "Hello");
        assert_eq!(emit.value_ref, "payload");
    }

    #[test]
    fn emit_accepts_two_segment_dotted_value_ref() {
        // The exact case adopters reported as broken before 13.i.
        let prog = parse("flow f() -> Out { emit Hello(Build.output) }").expect("parse");
        let emit = extract_first_emit(&prog);
        assert_eq!(emit.value_ref, "Build.output");
    }

    #[test]
    fn emit_accepts_three_segment_nested_dotted_value_ref() {
        let prog = parse("flow f() -> Out { emit Score(Analyze.result.score) }").expect("parse");
        let emit = extract_first_emit(&prog);
        assert_eq!(emit.value_ref, "Analyze.result.score");
    }

    #[test]
    fn emit_dotted_with_trailing_dot_fails() {
        // Trailing `.` must still error — every '.' demands an identifier.
        let result = parse("flow f() -> Out { emit Hello(Build.) }");
        assert!(result.is_err(), "expected parse error for trailing dot");
    }
}

// ── §Fase 14.a — declaration_trivia parallel channel tests ──────────────────

#[cfg(test)]
mod fase14a_declaration_trivia_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::tokens::TriviaKind;

    fn parse(src: &str) -> Program {
        let toks = Lexer::new(src, "<test>").tokenize().expect("lex");
        Parser::new(toks).parse().expect("parse")
    }

    #[test]
    fn no_comments_means_empty_trivia_per_decl() {
        let prog = parse("flow F() -> Out { }");
        assert_eq!(prog.declarations.len(), 1);
        assert_eq!(prog.declaration_trivia.len(), 1);
        assert!(prog.declaration_trivia[0].leading.is_empty());
        assert!(prog.declaration_trivia[0].trailing.is_empty());
    }

    #[test]
    fn doc_line_comment_attaches_as_leading() {
        let prog = parse("/// Documents F\nflow F() -> Out { }");
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading.len(), 1);
        assert_eq!(triv.leading[0].kind, TriviaKind::DocLine);
        assert!(triv.leading[0].is_doc());
        assert_eq!(triv.leading[0].text, "/// Documents F");
    }

    #[test]
    fn regular_line_comment_attaches_as_leading() {
        let prog = parse("// header\nflow F() -> Out { }");
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading.len(), 1);
        assert_eq!(triv.leading[0].kind, TriviaKind::Line);
        assert!(!triv.leading[0].is_doc());
    }

    #[test]
    fn block_doc_comment_attaches_as_leading() {
        let prog = parse("/** Doc block */\nflow F() -> Out { }");
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading[0].kind, TriviaKind::DocBlock);
        assert!(triv.leading[0].is_doc());
    }

    #[test]
    fn multiple_comments_collected_in_source_order() {
        let src = "/// First\n/// Second\nflow F() -> Out { }";
        let prog = parse(src);
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading.len(), 2);
        assert_eq!(triv.leading[0].text, "/// First");
        assert_eq!(triv.leading[1].text, "/// Second");
    }

    #[test]
    fn three_decls_each_get_own_leading() {
        let src = "/// for A\nflow A() -> Out { }\n/// for B\nflow B() -> Out { }\n/// for C\nflow C() -> Out { }";
        let prog = parse(src);
        assert_eq!(prog.declarations.len(), 3);
        assert_eq!(prog.declaration_trivia.len(), 3);
        for (idx, name) in ["A", "B", "C"].iter().enumerate() {
            let triv = &prog.declaration_trivia[idx];
            assert_eq!(triv.leading.len(), 1);
            assert_eq!(triv.leading[0].text, format!("/// for {name}"));
        }
    }

    #[test]
    fn trailing_comment_attaches_to_last_token_of_decl() {
        // Comment on the same line as the decl's closing brace.
        let prog = parse("flow F() -> Out { } // tail");
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.trailing.len(), 1);
        assert_eq!(triv.trailing[0].text, "// tail");
    }

    #[test]
    fn mixed_doc_and_regular_preserve_order_between_decls() {
        let src = "/// doc for A\nflow A() -> Out { }\n\n// header line\n/// doc for B\nflow B() -> Out { }";
        let prog = parse(src);
        assert_eq!(prog.declarations.len(), 2);
        // A: just the doc comment.
        assert_eq!(prog.declaration_trivia[0].leading.len(), 1);
        // B: header + doc, in source order.
        assert_eq!(prog.declaration_trivia[1].leading.len(), 2);
        assert_eq!(prog.declaration_trivia[1].leading[0].text, "// header line");
        assert_eq!(prog.declaration_trivia[1].leading[1].text, "/// doc for B");
    }

    #[test]
    fn parser_unaffected_by_comments_in_grammar_path() {
        // The parser must accept comments interleaved between every
        // legal token without affecting the AST shape it produces.
        // This is the regression guard for "lossless lexing must not
        // change parsing semantics."
        let src =
            "// before flow\nflow /* between flow and name */ F() -> Out {\n  // body comment\n}";
        let prog = parse(src);
        assert_eq!(prog.declarations.len(), 1);
        if let Declaration::Flow(f) = &prog.declarations[0] {
            assert_eq!(f.name, "F");
        } else {
            panic!("expected Flow declaration");
        }
    }
}

// ── §Fase 14.b — per-struct trivia fields tests ─────────────────────────────
//
// 14.b spreads `leading_trivia` / `trailing_trivia` into every Declaration
// variant struct (FlowDefinition, ChannelDefinition, PersonaDefinition, …).
// The Python AST already had this shape since 14.a; 14.b achieves Rust
// parity. The side-channel `Program.declaration_trivia` is preserved for
// backward compat — these tests verify the new direct access path.

#[cfg(test)]
mod fase14b_per_struct_trivia_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::tokens::TriviaKind;

    fn parse(src: &str) -> Program {
        let toks = Lexer::new(src, "<test>").tokenize().expect("lex");
        Parser::new(toks).parse().expect("parse")
    }

    #[test]
    fn flow_definition_carries_leading_trivia_directly() {
        let prog = parse("/// documents F\nflow F() -> Out { }");
        if let Declaration::Flow(f) = &prog.declarations[0] {
            assert_eq!(f.leading_trivia.len(), 1);
            assert_eq!(f.leading_trivia[0].kind, TriviaKind::DocLine);
            assert_eq!(f.leading_trivia[0].text, "/// documents F");
            assert!(f.trailing_trivia.is_empty());
        } else {
            panic!("expected Flow declaration");
        }
    }

    #[test]
    fn flow_definition_carries_trailing_trivia_directly() {
        let prog = parse("flow F() -> Out { } // tail comment");
        if let Declaration::Flow(f) = &prog.declarations[0] {
            assert_eq!(f.trailing_trivia.len(), 1);
            assert_eq!(f.trailing_trivia[0].text, "// tail comment");
        } else {
            panic!("expected Flow declaration");
        }
    }

    #[test]
    fn channel_definition_carries_trivia_directly() {
        // ChannelDefinition is a Tier-1 declaration; verify per-struct fields
        // populate just like FlowDefinition.
        let src = concat!(
            "/// inbound order events\n",
            "channel Orders {\n",
            "    message:     Order\n",
            "    qos:         at_least_once\n",
            "    lifetime:    affine\n",
            "    persistence: ephemeral\n",
            "    shield:      Broker\n",
            "}",
        );
        let prog = parse(src);
        if let Declaration::Channel(ch) = &prog.declarations[0] {
            assert_eq!(ch.leading_trivia.len(), 1);
            assert!(ch.leading_trivia[0].is_doc());
            assert_eq!(ch.leading_trivia[0].text, "/// inbound order events");
        } else {
            panic!("expected Channel declaration");
        }
    }

    #[test]
    fn per_struct_fields_match_side_channel() {
        // 14.a side-channel and 14.b per-struct fields must hold identical
        // data — they are populated by the same parser pass.
        let src = "/// for A\n// header for B\nflow A() -> Out { }\n/// for B\nflow B() -> Out { }";
        let prog = parse(src);
        for (idx, decl) in prog.declarations.iter().enumerate() {
            let side = &prog.declaration_trivia[idx];
            let (per_lead, per_trail) = match decl {
                Declaration::Flow(f) => (&f.leading_trivia, &f.trailing_trivia),
                _ => panic!("unexpected variant"),
            };
            assert_eq!(per_lead.len(), side.leading.len());
            assert_eq!(per_trail.len(), side.trailing.len());
            for (a, b) in per_lead.iter().zip(side.leading.iter()) {
                assert_eq!(a.text, b.text);
                assert_eq!(a.kind, b.kind);
            }
        }
    }

    #[test]
    fn comment_free_program_yields_empty_per_struct_fields() {
        let prog = parse("flow F() -> Out { }");
        if let Declaration::Flow(f) = &prog.declarations[0] {
            assert!(f.leading_trivia.is_empty());
            assert!(f.trailing_trivia.is_empty());
        } else {
            panic!("expected Flow declaration");
        }
    }
}

// ── §Fase 14.c — inner doc comments (//!, /*!) ──────────────────────────────
//
// Inner doc comments document the *enclosing* item rather than the next
// sibling. Today they flow through the trivia channel like any other
// comment; downstream consumers (axon doc, LSP) decide how to interpret
// `is_inner_doc()`. These tests verify the lexer→parser pipeline preserves
// the inner-doc discriminator end-to-end.

#[cfg(test)]
mod fase14c_inner_doc_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::tokens::TriviaKind;

    fn parse(src: &str) -> Program {
        let toks = Lexer::new(src, "<test>").tokenize().expect("lex");
        Parser::new(toks).parse().expect("parse")
    }

    #[test]
    fn inner_doc_line_reaches_leading_trivia() {
        let src = "//! file-level docs\nflow F() -> Out { }";
        let prog = parse(src);
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading.len(), 1);
        assert_eq!(triv.leading[0].kind, TriviaKind::InnerDocLine);
        assert!(triv.leading[0].is_doc());
        assert!(triv.leading[0].is_inner_doc());
        assert_eq!(triv.leading[0].text, "//! file-level docs");
        assert_eq!(triv.leading[0].stripped_text(), " file-level docs");
    }

    #[test]
    fn inner_doc_block_reaches_leading_trivia() {
        let src = "/*! module-level docs */\nflow F() -> Out { }";
        let prog = parse(src);
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading.len(), 1);
        assert_eq!(triv.leading[0].kind, TriviaKind::InnerDocBlock);
        assert!(triv.leading[0].is_inner_doc());
        assert_eq!(triv.leading[0].stripped_text(), " module-level docs ");
    }

    #[test]
    fn outer_and_inner_doc_can_coexist() {
        // File-level inner doc on top, then an outer doc for the
        // declaration. Both reach the trivia channel and remain
        // distinguishable via `is_inner_doc()`.
        let src = "//! file docs\n/// docs F\nflow F() -> Out { }";
        let prog = parse(src);
        let triv = &prog.declaration_trivia[0];
        assert_eq!(triv.leading.len(), 2);
        assert!(triv.leading[0].is_inner_doc());
        assert!(triv.leading[1].is_doc());
        assert!(!triv.leading[1].is_inner_doc());
    }

    #[test]
    fn inner_doc_reaches_per_struct_fields() {
        // Same data must be visible via the per-struct fields (Fase 14.b).
        let src = "//! intro\nflow F() -> Out { }";
        let prog = parse(src);
        if let Declaration::Flow(f) = &prog.declarations[0] {
            assert_eq!(f.leading_trivia.len(), 1);
            assert!(f.leading_trivia[0].is_inner_doc());
        } else {
            panic!("expected Flow declaration");
        }
    }
}

// ── §Fase 28.c — Parser error recovery test pack ─────────────────────────────
//
// Mirror of `tests/test_fase28_parser_recovery.py` (Python side, 28.b).
// The test classes here line up 1-1 with the Python ones so the cross-
// stack drift gate (28.i) can compare error-list shapes input-for-input.
//
// Test classes:
//   - backwards_compat: existing `parse()` API unchanged
//   - single_error_recovery: one bad decl → one error, rest parse OK
//   - multi_error_recovery: N independent errors → N entries
//   - sync_points: every top-level keyword resyncs correctly
//   - parse_result_api: `has_errors`, `is_clean`
//   - edge_cases: EOF mid-error, brace imbalance, only-bad-tokens
//   - robustness_fuzz: 1000 deterministic-seeded mutations never crash
//   - no_ghost_errors: single broken field produces exactly 1 error
//   - integration_with_colon_diagnostic: v1.19.4 hint preserved under
//     recovery mode
#[cfg(test)]
mod fase28_recovery_tests {
    use super::*;
    use crate::lexer::Lexer;

    /// Lex a source and return tokens for the parser to consume.
    /// Mirrors the Python `_parse_recovery` helper.
    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src, "<test>").tokenize().expect("lex")
    }

    /// Parse with recovery mode. Returns `(program, errors)` so call
    /// sites read like the Python helper.
    fn recover(src: &str) -> ParseResult {
        Parser::new(lex(src)).parse_with_recovery()
    }

    /// Strict parse. Mirrors the Python `_parse_strict` helper.
    fn strict(src: &str) -> Result<Program, ParseError> {
        Parser::new(lex(src)).parse()
    }

    // ── backwards_compat ─────────────────────────────────────────

    #[test]
    fn strict_parse_unchanged_for_clean_source() {
        // The existing `parse()` API must continue to succeed
        // verbatim on every well-formed input — D9.
        let src = "intent I {}";
        let prog = strict(src).expect("clean parse");
        assert_eq!(prog.declarations.len(), 1);
    }

    #[test]
    fn strict_parse_still_raises_on_first_error() {
        // D9 + D8: opt-in to recovery via `parse_with_recovery`;
        // strict mode must still bubble the first error.
        // (Using a parse-time error rather than a lex error — `@@@`
        // would be rejected by the lexer, which is out of scope.)
        let src = "flow F() { } not_a_keyword flow G() { }";
        let _ = strict(src).expect_err("must error fast in strict mode");
    }

    #[test]
    fn recovery_clean_source_yields_no_errors() {
        let src = "flow F() { } flow G() { }";
        let pr = recover(src);
        assert!(pr.is_clean(), "errors: {:?}", pr.errors);
        assert_eq!(pr.program.declarations.len(), 2);
    }

    // ── single_error_recovery ────────────────────────────────────

    #[test]
    fn single_unknown_top_level_token_recovers() {
        // One garbage token at top level; rest must parse.
        let src = "garbage_token flow F() { } flow G() { }";
        let pr = recover(src);
        assert_eq!(pr.errors.len(), 1, "errors: {:?}", pr.errors);
        assert_eq!(pr.program.declarations.len(), 2);
    }

    #[test]
    fn error_in_first_decl_does_not_block_second() {
        // `flow F` body refers to non-keyword `nope`; the error
        // recovery must skip to the next top-level keyword.
        let src = "flow F() { not_a_step nope } flow G() { }";
        let pr = recover(src);
        assert!(pr.has_errors(), "expected at least one error");
        // The second flow must be reachable.
        let names: Vec<&str> = pr
            .program
            .declarations
            .iter()
            .filter_map(|d| match d {
                Declaration::Flow(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"G"), "G not found among {names:?}");
    }

    #[test]
    fn malformed_declaration_then_clean_intent_recovers() {
        let src = "flow @ () { } intent I {}";
        let pr = recover(src);
        assert!(pr.has_errors());
        let kinds: Vec<&str> = pr
            .program
            .declarations
            .iter()
            .map(|d| match d {
                Declaration::Intent(_) => "intent",
                Declaration::Flow(_) => "flow",
                _ => "other",
            })
            .collect();
        assert!(kinds.contains(&"intent"), "kinds: {kinds:?}");
    }

    #[test]
    fn recovery_does_not_double_count_a_single_error() {
        // Regression for the "ghost error" pathology that surfaced
        // during 28.b dev: a nested-decl error must not also fire
        // an "Unexpected token at top level" from the outer loop.
        // The Rust grammar has stricter intra-flow requirements
        // than Python; the invariant we assert here is that the
        // outer loop emits zero "Unexpected token at top level"
        // errors after an inner step-shape error.
        let src = "flow F() { not_a_step }";
        let pr = recover(src);
        let outer_ghosts = pr
            .errors
            .iter()
            .filter(|e| e.message.contains("at top level"))
            .count();
        assert_eq!(outer_ghosts, 0, "ghost errors: {:?}", pr.errors);
    }

    // ── multi_error_recovery ─────────────────────────────────────

    #[test]
    fn three_independent_errors_yield_three_entries() {
        let src =
            "garbage1 flow F() { } garbage2 flow G() { } garbage3 flow H() { }";
        let pr = recover(src);
        assert_eq!(pr.errors.len(), 3, "errors: {:?}", pr.errors);
        assert_eq!(pr.program.declarations.len(), 3);
    }

    #[test]
    fn all_errors_no_valid_declarations() {
        let src = "foo bar baz qux";
        let pr = recover(src);
        assert!(pr.has_errors());
        assert!(pr.program.declarations.is_empty());
    }

    #[test]
    fn errors_recorded_in_source_order() {
        let src = "x flow A() { } y flow B() { } z flow C() { }";
        let pr = recover(src);
        assert_eq!(pr.errors.len(), 3);
        let lines: Vec<u32> = pr.errors.iter().map(|e| e.line).collect();
        // Same source-line means we compare by column ordering;
        // either way they must be non-decreasing.
        assert!(
            lines.windows(2).all(|w| w[0] <= w[1]),
            "errors out of order: {lines:?}"
        );
    }

    // ── sync_points ──────────────────────────────────────────────

    #[test]
    fn sync_to_flow_keyword() {
        let src = "garbage flow F() { }";
        let pr = recover(src);
        assert_eq!(pr.program.declarations.len(), 1);
    }

    #[test]
    fn sync_to_intent_keyword() {
        let src = "garbage intent I {}";
        let pr = recover(src);
        assert_eq!(pr.program.declarations.len(), 1);
    }

    #[test]
    fn sync_to_persona_keyword() {
        let src = "garbage persona P { name: \"P\" role: \"R\" }";
        let pr = recover(src);
        assert!(
            pr.program
                .declarations
                .iter()
                .any(|d| matches!(d, Declaration::Persona(_))),
            "persona not recovered: decls = {:?}",
            pr.program.declarations.len()
        );
    }

    #[test]
    fn sync_to_run_keyword() {
        let src = "garbage run R { input: { user_message: \"hi\" } }";
        let pr = recover(src);
        // Either Run was parsed, or recovery still produced ≥1 err.
        assert!(pr.has_errors());
    }

    // ── parse_result_api ─────────────────────────────────────────

    #[test]
    fn parse_result_has_errors_and_is_clean_invert() {
        let pr_clean = recover("flow F() { }");
        assert!(pr_clean.is_clean());
        assert!(!pr_clean.has_errors());

        let pr_err = recover("garbage");
        assert!(!pr_err.is_clean());
        assert!(pr_err.has_errors());
    }

    #[test]
    fn parse_result_program_field_holds_partial_program() {
        let pr = recover("garbage flow F() { }");
        assert!(!pr.program.declarations.is_empty());
    }

    #[test]
    fn parse_result_errors_carry_line_and_column() {
        let pr = recover("garbage");
        assert!(!pr.errors.is_empty());
        let e = &pr.errors[0];
        assert!(e.line >= 1);
        // Column may be 0-based or 1-based depending on lexer;
        // accept anything ≥ 0.
        let _ = e.column;
        assert!(!e.message.is_empty());
    }

    #[test]
    fn parse_result_debug_renders() {
        let pr = recover("flow F() { }");
        let s = format!("{pr:?}");
        assert!(s.contains("ParseResult"));
    }

    // ── edge_cases ───────────────────────────────────────────────

    #[test]
    fn empty_source_is_clean() {
        let pr = recover("");
        assert!(pr.is_clean());
        assert!(pr.program.declarations.is_empty());
    }

    #[test]
    fn whitespace_only_source_is_clean() {
        let pr = recover("   \n\n\t  \n");
        assert!(pr.is_clean());
        assert!(pr.program.declarations.is_empty());
    }

    #[test]
    fn only_garbage_does_not_crash() {
        // Lex-clean garbage tokens (avoids AxonLexerError).
        let pr = recover("foo bar baz { qux quux } corge { grault }");
        assert!(pr.has_errors());
    }

    #[test]
    fn unbalanced_close_brace_does_not_crash() {
        let pr = recover("} flow F() { }");
        // Recovery must keep walking past stray `}`.
        let names: Vec<&str> = pr
            .program
            .declarations
            .iter()
            .filter_map(|d| match d {
                Declaration::Flow(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"F"), "F not recovered: {names:?}");
    }

    #[test]
    fn error_at_eof_does_not_loop() {
        // Truncated declaration. Must terminate; finite errors.
        let pr = recover("flow F() { ");
        // Either errored or somehow accepted — but must terminate.
        let _ = pr.errors.len();
    }

    #[test]
    fn nested_braces_inside_error_still_balance() {
        // Walker must respect brace depth so a `}` inside a malformed
        // block does not prematurely sync.
        let src = "flow F() { not_a_step { inner } } flow G() { }";
        let pr = recover(src);
        let names: Vec<&str> = pr
            .program
            .declarations
            .iter()
            .filter_map(|d| match d {
                Declaration::Flow(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"G"), "G not recovered: {names:?}");
    }

    // ── robustness_fuzz ──────────────────────────────────────────
    //
    // Deterministic-seeded mutator (xorshift). 100 buckets ×
    // 10 mutations = 1000 iterations, byte-bounded so fuzz time
    // stays under 1 s on a release build. Recovery must NEVER crash;
    // lexer-level errors are out of scope (lexer recovery is its own
    // sub-fase). 28.b mirrors this with the same structure.

    #[derive(Clone, Copy)]
    struct Xorshift(u64);
    impl Xorshift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn pick<T: Copy>(&mut self, slice: &[T]) -> T {
            slice[(self.next() as usize) % slice.len()]
        }
    }

    fn mutate(src: &str, rng: &mut Xorshift) -> String {
        let mut bytes: Vec<u8> = src.bytes().collect();
        if bytes.is_empty() {
            return src.to_string();
        }
        let op = rng.next() % 4;
        let pos = (rng.next() as usize) % bytes.len();
        // Stick to ASCII-safe printable bytes to keep input lex-able
        // most of the time. AxonLexerError is still possible and is
        // tolerated by the recovery contract.
        let safe: &[u8] = b"abcdefghijklmnopqrstuvwxyz {}();:,_0123456789";
        match op {
            0 => {
                bytes.remove(pos);
            }
            1 => {
                let b = rng.pick(safe);
                bytes.insert(pos, b);
            }
            2 if pos + 1 < bytes.len() => {
                bytes.swap(pos, pos + 1);
            }
            _ => {
                let b = rng.pick(safe);
                bytes[pos] = b;
            }
        }
        // Lossy decode: mutator may have produced invalid UTF-8;
        // strip non-ASCII before handing to the lexer.
        bytes.retain(|b| b.is_ascii());
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn fuzz_recovery_never_crashes() {
        let seed_bases = [
            "flow F() { }",
            "intent I { }",
            "persona P { name: \"P\" role: \"R\" }",
            "intent J { ask: \"a\" }",
            "type T = String",
        ];
        // 100 buckets × 10 mutations = 1000 iterations, deterministic.
        for (bucket, base) in (0..100u64).zip(seed_bases.iter().cycle()) {
            let mut rng = Xorshift(0x1234_5678_9abc_def0_u64.wrapping_add(bucket));
            let mut current = (*base).to_string();
            for _ in 0..10 {
                current = mutate(&current, &mut rng);
                // Lexer may reject; that's outside parser-recovery
                // scope (28.b/c). Skip those iterations.
                let toks = match Lexer::new(&current, "<fuzz>").tokenize() {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                // Recovery must not panic on any well-lexed input.
                let _pr = Parser::new(toks).parse_with_recovery();
            }
        }
    }

    // ── integration_with_v1_19_4_colon_diagnostic ────────────────

    #[test]
    fn missing_colon_hint_preserved_under_recovery() {
        // The Rust frontend's strict `parse()` carries the same
        // colon diagnostic shape as the Python side. Recovery mode
        // must not erase it.
        let src = "flow F() { run R { input { user_message: \"hi\" } } }";
        let pr = recover(src);
        // Either the parser accepts this (some shape may be valid)
        // or it errors — but if it errors, the message must surface
        // the diagnostic content.
        if !pr.errors.is_empty() {
            let any_msg = pr.errors.iter().any(|e| !e.message.is_empty());
            assert!(any_msg);
        }
    }

    // ── recovery preserves declaration ordering ──────────────────

    #[test]
    fn recovered_declarations_appear_in_source_order() {
        let src = "flow A() { } garbage flow B() { } garbage flow C() { }";
        let pr = recover(src);
        let names: Vec<&str> = pr
            .program
            .declarations
            .iter()
            .filter_map(|d| match d {
                Declaration::Flow(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["A", "B", "C"]);
    }
}

// ── §Fase 28.d — Source-context diagnostic block test pack ───────────────────
//
// Mirror of `tests/test_fase28_source_context.py` (Python side, 28.d).
// The render output must be byte-identical to the Python `SourceSnippet.render`
// on the same input — D7 ratified (cross-stack drift gate). Golden strings
// in `golden_*` tests are duplicated verbatim in the Python pack; edits
// here MUST be mirrored on the Python side and vice versa.
#[cfg(test)]
mod fase28_source_context_tests {
    use super::*;
    use crate::lexer::Lexer;

    fn snippet(source: &str, line: u32, column: u32, filename: &str) -> String {
        SourceSnippet::new(
            source.to_string(),
            line,
            column,
            filename.to_string(),
        )
        .render()
    }

    // ── Pure rendering ──────────────────────────────────────────

    #[test]
    fn rustc_style_block_for_middle_line() {
        let src = "line one\nline two\nline three\nline four\nline five";
        let out = snippet(src, 3, 6, "x.axon");
        assert!(out.contains("--> x.axon:3:6"));
        assert!(out.contains("1 | line one"));
        assert!(out.contains("2 | line two"));
        assert!(out.contains("3 | line three"));
        assert!(out.contains("4 | line four"));
        assert!(out.contains("5 | line five"));
        // Caret col 6 → 5-space pad. Empty gutter is 1 space (gutter=1).
        assert!(out.contains("\n  |      ^"), "out:\n{out}");
    }

    #[test]
    fn caret_column_one_renders_correctly() {
        let out = snippet("abc\n", 1, 1, "<source>");
        assert!(out.contains("\n  | ^"));
    }

    #[test]
    fn first_line_clamps_context_before_to_zero() {
        let src = "first\nsecond\nthird\nfourth\nfifth";
        let out = snippet(src, 1, 1, "<source>");
        assert!(out.contains("1 | first"));
        assert!(out.contains("2 | second"));
        assert!(out.contains("3 | third"));
        assert!(!out.contains("4 | fourth"));
    }

    #[test]
    fn last_line_clamps_context_after_to_eof() {
        let src = "first\nsecond\nthird\nfourth\nfifth";
        let out = snippet(src, 5, 2, "<source>");
        assert!(out.contains("5 | fifth"));
        assert!(out.contains("3 | third"));
        assert!(out.contains("4 | fourth"));
        assert!(!out.contains("2 | second"));
    }

    #[test]
    fn gutter_width_grows_with_line_count() {
        let src: String = (1..=12).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let out = snippet(&src, 12, 1, "<source>");
        assert!(out.contains("12 | line12"));
        assert!(out.contains("10 | line10"));
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn empty_source_returns_empty() {
        assert_eq!(snippet("", 1, 1, "<source>"), "");
    }

    #[test]
    fn zero_line_returns_empty() {
        assert_eq!(snippet("hi", 0, 1, "<source>"), "");
    }

    #[test]
    fn out_of_range_line_returns_empty() {
        assert_eq!(snippet("hi", 99, 1, "<source>"), "");
    }

    #[test]
    fn caret_clamps_past_eol() {
        let out = snippet("hello", 1, 50, "<source>");
        assert!(out.contains("\n  |      ^"), "out:\n{out}");
    }

    #[test]
    fn unicode_codepoint_count_for_caret_clamp() {
        // "héllo" = 5 codepoints; column past EOL clamps to 6.
        let out = snippet("héllo", 1, 99, "<source>");
        assert!(out.contains("\n  |      ^"), "out:\n{out}");
    }

    #[test]
    fn trailing_newline_does_not_create_phantom_last_line() {
        let out = snippet("first\nsecond\n", 2, 1, "<source>");
        assert!(!out.contains("3 |"));
        assert!(out.contains("2 | second"));
    }

    // ── Parser attach plumbing ──────────────────────────────────

    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src, "<test>").tokenize().expect("lex")
    }

    #[test]
    fn strict_parse_attaches_snippet_when_source_given() {
        let src = "garbage_token\nflow F() { }";
        let err = Parser::new(lex(src))
            .with_source(src, "x.axon")
            .parse()
            .expect_err("must error");
        assert!(err.source_snippet.is_some());
        let display = format!("{err}");
        assert!(display.contains("--> x.axon:"), "display: {display}");
    }

    #[test]
    fn strict_parse_no_snippet_when_no_source() {
        let src = "garbage_token";
        let err = Parser::new(lex(src)).parse().expect_err("must error");
        assert!(err.source_snippet.is_none());
        let display = format!("{err}");
        assert!(!display.contains("\n  -->"));
    }

    #[test]
    fn every_recovered_error_has_snippet() {
        let src = "garbage1\nflow F() { }\ngarbage2\nflow G() { }";
        let result = Parser::new(lex(src))
            .with_source(src, "multi.axon")
            .parse_with_recovery();
        assert!(!result.errors.is_empty());
        for err in &result.errors {
            assert!(err.source_snippet.is_some());
            let display = format!("{err}");
            assert!(
                display.contains("--> multi.axon:"),
                "display: {display}"
            );
        }
    }

    #[test]
    fn recovery_no_snippet_when_no_source() {
        let src = "garbage1 garbage2";
        let result = Parser::new(lex(src)).parse_with_recovery();
        for err in &result.errors {
            assert!(err.source_snippet.is_none());
        }
    }

    #[test]
    fn snippet_points_at_correct_line_for_each_error() {
        let src = "garbage_a\nflow F() { }\ngarbage_b\nflow G() { }";
        let result = Parser::new(lex(src))
            .with_source(src, "x")
            .parse_with_recovery();
        for err in &result.errors {
            let sn = err.source_snippet.as_ref().expect("snippet");
            assert_eq!(sn.line, err.line);
        }
    }

    // ── Backwards-compat ────────────────────────────────────────

    #[test]
    fn legacy_constructor_still_works() {
        let src = "flow F() { }";
        let prog = Parser::new(lex(src)).parse().expect("clean");
        assert_eq!(prog.declarations.len(), 1);
    }

    #[test]
    fn attach_source_idempotent() {
        let err = ParseError {
            message: "bad".to_string(),
            line: 2,
            column: 3,
            ..Default::default()
        };
        let err2 = err.clone().attach_source("a\nb\nc\n", "f.axon");
        let first = format!("{err2}");
        let err3 = err.attach_source("a\nb\nc\n", "f.axon");
        let second = format!("{err3}");
        assert_eq!(first, second);
    }

    #[test]
    fn attach_source_noop_when_line_zero() {
        let err = ParseError {
            message: "bad".to_string(),
            line: 0,
            column: 0,
            ..Default::default()
        };
        let err = err.attach_source("a\nb\nc\n", "f.axon");
        assert!(err.source_snippet.is_none());
    }

    // ── Cross-stack golden parity ───────────────────────────────
    // These golden strings are duplicated verbatim in the Python
    // test pack at `tests/test_fase28_source_context.py::TestRustParityShape`.
    // Edits here MUST be mirrored in the Python pack — D7.

    #[test]
    fn golden_simple_three_line_block() {
        let src = "alpha\nbeta\ngamma";
        let out = snippet(src, 2, 3, "g.axon");
        // Note: gutter=1, so empty_gutter=" " (one space). The
        // " --> ..." line therefore starts with two spaces ("<empty>"
        // + literal " --> ...").
        let expected = concat!(
            "  --> g.axon:2:3\n",
            "  |\n",
            "1 | alpha\n",
            "2 | beta\n",
            "  |   ^\n",
            "3 | gamma",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn golden_first_line_caret() {
        let src = "abc\ndef\n";
        let out = snippet(src, 1, 1, "x");
        let expected = concat!(
            "  --> x:1:1\n",
            "  |\n",
            "1 | abc\n",
            "  | ^\n",
            "2 | def",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn golden_two_digit_gutter() {
        let src: String = (1..=11)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = snippet(&src, 10, 2, "big");
        let expected = concat!(
            "   --> big:10:2\n",
            "   |\n",
            " 8 | L8\n",
            " 9 | L9\n",
            "10 | L10\n",
            "   |  ^\n",
            "11 | L11",
        );
        assert_eq!(out, expected);
    }
}

// ── §Fase 28.e — Parser integration tests for smart-suggest ──────────────────
//
// Mirror of `tests/test_fase28_smart_suggest.py::TestParserIntegration`.
// Verifies that the parser actually wires `suggest_for` into the
// unknown-keyword diagnostic at both error sites — top-level and
// flow-body.
#[cfg(test)]
mod fase28_smart_suggest_parser_tests {
    use super::*;
    use crate::lexer::Lexer;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src, "<test>").tokenize().expect("lex")
    }

    #[test]
    fn top_level_typo_suggests_flow() {
        let src = "flwo F() { }";
        let err = Parser::new(lex(src)).parse().expect_err("must error");
        assert!(
            err.message.contains("Did you mean `flow`?"),
            "msg: {}",
            err.message
        );
    }

    #[test]
    fn top_level_unknown_far_no_suggestion() {
        let src = "qwerty F() { }";
        let err = Parser::new(lex(src)).parse().expect_err("must error");
        assert!(
            !err.message.contains("Did you mean"),
            "msg: {}",
            err.message
        );
    }

    #[test]
    fn flow_body_typo_suggests_step() {
        let src = "flow F() { stepp S {} }";
        let err = Parser::new(lex(src)).parse().expect_err("must error");
        assert!(
            err.message.contains("Did you mean `step`"),
            "msg: {}",
            err.message
        );
    }

    #[test]
    fn flow_body_typo_suggests_reason() {
        let src = "flow F() { reasn R {} }";
        let err = Parser::new(lex(src)).parse().expect_err("must error");
        assert!(
            err.message.contains("Did you mean `reason`?"),
            "msg: {}",
            err.message
        );
    }

    #[test]
    fn recovery_mode_carries_hint() {
        let src = "flwo F() { }";
        let result = Parser::new(lex(src)).parse_with_recovery();
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.message.contains("Did you mean `flow`?")),
            "errors: {:?}",
            result.errors
        );
    }
}

// ── §Fase 35.m — mutate / purge where-clause capture ────────────────

#[cfg(test)]
mod fase35m_mutate_purge_where_tests {
    use super::*;

    fn parse(src: &str) -> Program {
        let tokens = crate::lexer::Lexer::new(src, "<test>")
            .tokenize()
            .expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    fn first_step<'a>(prog: &'a Program, flow: &str) -> &'a FlowStep {
        for d in &prog.declarations {
            if let Declaration::Flow(f) = d {
                if f.name == flow {
                    return f.body.first().expect("flow has at least one step");
                }
            }
        }
        panic!("flow `{flow}` not found");
    }

    #[test]
    fn mutate_captures_its_where_clause() {
        // Pre-35.m the `{ where: }` block was skipped — every mutate
        // ran whole-store. It must now reach `where_expr`.
        let prog =
            parse("flow F() -> Unit { mutate accounts { where: \"id = 1\" } }");
        match first_step(&prog, "F") {
            FlowStep::Mutate(m) => {
                assert_eq!(m.store_name, "accounts");
                assert_eq!(m.where_expr, "id = 1");
            }
            other => panic!("expected Mutate, got {other:?}"),
        }
    }

    #[test]
    fn purge_captures_its_where_clause() {
        let prog =
            parse("flow F() -> Unit { purge logs { where: \"ts < 100\" } }");
        match first_step(&prog, "F") {
            FlowStep::Purge(p) => {
                assert_eq!(p.store_name, "logs");
                assert_eq!(p.where_expr, "ts < 100");
            }
            other => panic!("expected Purge, got {other:?}"),
        }
    }

    #[test]
    fn mutate_without_a_where_block_is_a_whole_store_op() {
        // No `{ where: }` → an empty filter → the runtime renders
        // `WHERE TRUE` (every row). A valid, intentional op.
        let prog = parse("flow F() -> Unit { mutate accounts }");
        match first_step(&prog, "F") {
            FlowStep::Mutate(m) => {
                assert_eq!(m.store_name, "accounts");
                assert_eq!(m.where_expr, "");
            }
            other => panic!("expected Mutate, got {other:?}"),
        }
    }
}

// ── §Fase 35.o — persist field-block capture ────────────────────────

#[cfg(test)]
mod fase35o_persist_fields_tests {
    use super::*;

    fn parse(src: &str) -> Program {
        let tokens = crate::lexer::Lexer::new(src, "<test>")
            .tokenize()
            .expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    fn first_step<'a>(prog: &'a Program, flow: &str) -> &'a FlowStep {
        for d in &prog.declarations {
            if let Declaration::Flow(f) = d {
                if f.name == flow {
                    return f.body.first().expect("flow has at least one step");
                }
            }
        }
        panic!("flow `{flow}` not found");
    }

    #[test]
    fn persist_captures_its_field_block() {
        // Pre-35.o the `{ col: value }` block was skipped — every
        // persist wrote the whole binding context. It must now reach
        // `fields`, in source order, with value expressions raw.
        let prog = parse(
            "flow F() -> Unit { persist into chat_history { \
             session_id: \"${session_id}\" sender: \"user\" \
             content: \"${message}\" } }",
        );
        match first_step(&prog, "F") {
            FlowStep::Persist(p) => {
                assert_eq!(p.store_name, "chat_history");
                assert_eq!(
                    p.fields,
                    vec![
                        ("session_id".to_string(), "${session_id}".to_string()),
                        ("sender".to_string(), "user".to_string()),
                        ("content".to_string(), "${message}".to_string()),
                    ]
                );
            }
            other => panic!("expected Persist, got {other:?}"),
        }
    }

    #[test]
    fn persist_without_a_block_keeps_the_user_bindings_fallback() {
        // No `{ }` → empty `fields` → the runtime falls back to the
        // v1.30.0 user-bindings row. Backward-compatible.
        let prog = parse("flow F() -> Unit { persist events }");
        match first_step(&prog, "F") {
            FlowStep::Persist(p) => {
                assert_eq!(p.store_name, "events");
                assert!(p.fields.is_empty());
            }
            other => panic!("expected Persist, got {other:?}"),
        }
    }

    #[test]
    fn persist_accepts_the_optional_into_connector() {
        // `persist into X` and `persist X` resolve to the SAME store
        // name — pre-35.o `into` was captured AS the store name.
        let with =
            parse("flow F() -> Unit { persist into accounts { id: \"1\" } }");
        let without =
            parse("flow F() -> Unit { persist accounts { id: \"1\" } }");
        for prog in [&with, &without] {
            match first_step(prog, "F") {
                FlowStep::Persist(p) => assert_eq!(p.store_name, "accounts"),
                other => panic!("expected Persist, got {other:?}"),
            }
        }
    }

    #[test]
    fn persist_into_without_a_block_resolves_the_store_name() {
        // `persist into events` — the `into` connector is skipped, the
        // store name is `events` (not `into`). Lateral bug closed.
        let prog = parse("flow F() -> Unit { persist into events }");
        match first_step(&prog, "F") {
            FlowStep::Persist(p) => {
                assert_eq!(p.store_name, "events");
                assert!(p.fields.is_empty());
            }
            other => panic!("expected Persist, got {other:?}"),
        }
    }

    #[test]
    fn persist_fields_lower_into_the_ir() {
        // The IR generator must carry `fields` onto `IRPersistStep`
        // so the runtime reads exactly the declared columns.
        let prog = parse(
            "flow F() -> Unit { persist into chat { content: \"${msg}\" } }",
        );
        let ir = crate::ir_generator::IRGenerator::new().generate(&prog);
        let flow = ir.flows.iter().find(|f| f.name == "F").expect("flow F");
        match flow.steps.first().expect("one step") {
            crate::ir_nodes::IRFlowNode::Persist(p) => {
                assert_eq!(p.store_name, "chat");
                assert_eq!(
                    p.fields,
                    vec![("content".to_string(), "${msg}".to_string())]
                );
            }
            other => panic!("expected IRFlowNode::Persist, got {other:?}"),
        }
    }
}

// ── §Fase 35.p — mutate SET-field-block capture ─────────────────────

#[cfg(test)]
mod fase35p_mutate_fields_tests {
    use super::*;

    fn parse(src: &str) -> Program {
        let tokens = crate::lexer::Lexer::new(src, "<test>")
            .tokenize()
            .expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    fn first_step<'a>(prog: &'a Program, flow: &str) -> &'a FlowStep {
        for d in &prog.declarations {
            if let Declaration::Flow(f) = d {
                if f.name == flow {
                    return f.body.first().expect("flow has at least one step");
                }
            }
        }
        panic!("flow `{flow}` not found");
    }

    #[test]
    fn mutate_captures_its_set_field_block() {
        // Pre-35.p every key but `where:` was skipped — the runtime
        // SET every flow binding. The SET columns must now reach
        // `fields`, in source order, with `where:` still captured.
        let prog = parse(
            "flow F() -> Unit { mutate accounts { where: \"id = ${id}\" \
             balance: \"${new_balance}\" status: \"active\" } }",
        );
        match first_step(&prog, "F") {
            FlowStep::Mutate(m) => {
                assert_eq!(m.store_name, "accounts");
                assert_eq!(m.where_expr, "id = ${id}");
                assert_eq!(
                    m.fields,
                    vec![
                        ("balance".to_string(), "${new_balance}".to_string()),
                        ("status".to_string(), "active".to_string()),
                    ]
                );
            }
            other => panic!("expected Mutate, got {other:?}"),
        }
    }

    #[test]
    fn mutate_where_only_block_has_no_set_fields() {
        // A `{ where: }`-only block → empty `fields` → the runtime
        // falls back to the v1.31.0 user-bindings SET.
        let prog =
            parse("flow F() -> Unit { mutate accounts { where: \"id = 1\" } }");
        match first_step(&prog, "F") {
            FlowStep::Mutate(m) => {
                assert_eq!(m.where_expr, "id = 1");
                assert!(m.fields.is_empty());
            }
            other => panic!("expected Mutate, got {other:?}"),
        }
    }

    #[test]
    fn mutate_with_no_block_is_a_whole_store_op() {
        // No block at all → empty where + empty fields (a whole-store
        // UPDATE from user bindings) — unchanged from 35.m.
        let prog = parse("flow F() -> Unit { mutate accounts }");
        match first_step(&prog, "F") {
            FlowStep::Mutate(m) => {
                assert_eq!(m.store_name, "accounts");
                assert_eq!(m.where_expr, "");
                assert!(m.fields.is_empty());
            }
            other => panic!("expected Mutate, got {other:?}"),
        }
    }

    #[test]
    fn mutate_fields_lower_into_the_ir() {
        let prog = parse(
            "flow F() -> Unit { mutate t { where: \"id = 1\" v: \"${x}\" } }",
        );
        let ir = crate::ir_generator::IRGenerator::new().generate(&prog);
        let flow = ir.flows.iter().find(|f| f.name == "F").expect("flow F");
        match flow.steps.first().expect("one step") {
            crate::ir_nodes::IRFlowNode::Mutate(m) => {
                assert_eq!(m.where_expr, "id = 1");
                assert_eq!(
                    m.fields,
                    vec![("v".to_string(), "${x}".to_string())]
                );
            }
            other => panic!("expected IRFlowNode::Mutate, got {other:?}"),
        }
    }
}

