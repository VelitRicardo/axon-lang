//! AXON token types and keyword lookup table.
//! Direct port of axon/compiler/tokens.py.

/// A single token produced by the AXON lexer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Token {
    pub ttype: TokenType,
    pub value: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // ── Keywords ──────────────────────────────────────────────────
    Persona,
    Context,
    Intent,
    Flow,
    Reason,
    Anchor,
    Validate,
    Refine,
    Memory,
    Tool,
    Probe,
    Weave,
    Step,
    Type,
    Import,
    Run,
    If,
    Else,
    Use,
    Remember,
    Recall,
    // Epistemic
    Know,
    Believe,
    Speculate,
    Doubt,
    // Parallel / yielding
    Par,
    Consolidate,
    Hibernate,
    Deliberate,
    Consensus,
    // Creative synthesis
    Forge,
    // OTS
    Ots,
    Teleology,
    HomotopySearch,
    LinearConstraints,
    LossFunction,
    // Streaming & effects
    Stream,
    OnChunk,
    OnComplete,
    Effects,
    Pure,
    Network,
    // ── §Fase 120 — algebraic effects (Plotkin/Pretnar), the six keywords
    //    `fase_23` §3.1 specifies. `Effects` above is a DIFFERENT thing: the
    //    `effects: <io, network>` row on a `tool` declaration. `Effect`
    //    (singular) opens a top-level effect DECLARATION. Keeping them as two
    //    tokens is deliberate — collapsing them would make `effect` in a tool
    //    body silently parse as a declaration opener.
    Effect,
    Handle,
    Perform,
    Resume,
    Abort,
    Forward,
    // Agent
    Agent,
    Goal,
    Tools,
    Budget,
    Strategy,
    OnStuck,
    // Shield
    Shield,
    // §Fase 71.a — `window` temporal execution guard (a peer of shield/anchor;
    // gates WHEN a scheduled tick runs, by timezone-aware day/hour windows).
    Window,
    Scan,
    OnBreach,
    Severity,
    Allow,
    Deny,
    Sandbox,
    Quarantine,
    Redact,
    // PIX
    Pix,
    Navigate,
    Drill,
    Trail,
    // §Fase 62.0 — Ledger (audit chain; took over `pix`'s former
    // Provenance-Index role so `pix` is freed for the retrieval navigator).
    Ledger,
    // Psyche
    Psyche,
    Dimensions,
    Manifold,
    Quantum,
    Inference,
    // MDN
    Corpus,
    Corroborate,
    EdgeFilter,
    // Data Science
    Dataspace,
    Ingest,
    Focus,
    /// §Fase 109 — `grad`: the proof-carrying derivative step.
    Grad,
    Associate,
    Aggregate,
    Explore,
    // EMCP
    Mcp,
    Taint,
    // Mandate
    Mandate,
    Constraint,
    Kp,
    Ki,
    Kd,
    Tolerance,
    MaxSteps,
    OnViolation,
    // Daemon
    Daemon,
    Listen,
    BudgetPerEvent,
    // §Fase 87 — the long-horizon autonomous research primitive (governed orchestrator).
    Savant,
    // §Fase 87.d — dynamic tool-synthesis policy (Coder/Reviewer → WASM).
    Synth,
    // Compute
    Compute,
    // §Fase 111 — `Logic` REMOVED. It was a reserved keyword with no parser
    // production, no type-checker arm and no IR node — dead in the ENTIRE
    // frontend, while the README advertised it as primitive #50 ("Compute body
    // scope — arithmetic DSL for pure deterministic transforms").
    // `primitive_registry` deleted its entry back in §Fase 6.d, stating the
    // rule it broke: "registry entries match parser productions one-to-one; an
    // entry without a parser production lies." The keyword outlived the entry
    // by four years and kept `logic` reserved — so an adopter could not even
    // name a variable `logic`. It is now an ordinary identifier again.
    // Lambda Data
    Lambda,
    Ontology,
    Certainty,
    TemporalFrame,
    Provenance,
    Derivation,
    // AxonStore
    AxonStore,
    Schema,
    Persist,
    Retrieve,
    Mutate,
    Purge,
    Transact,
    // AxonEndpoint
    AxonEndpoint,
    // §Fase 53 — Closed-catalog extension mechanism
    Extension,
    // I/O Cognitivo (§λ-L-E Fase 1 — Resources)
    Resource,
    Fabric,
    Manifest,
    Observe,
    // Control Cognitivo (§λ-L-E Fase 3)
    Reconcile,
    Lease,
    Ensemble,
    // Topology & Session (§λ-L-E Fase 4 — π-calculus)
    Topology,
    Session,
    Send,
    Receive,
    Loop,
    End,
    // Immune System (§λ-L-E Fase 5)
    Immune,
    Reflex,
    Heal,
    // UI Cognitiva (§λ-L-E Fase 9 — 100% .axon apps)
    Component,
    View,
    // Mobile Typed Channels (§λ-L-E Fase 13 — π-calc mobility)
    Channel,
    // WebSocket as a cognitive primitive (§Fase 41.b) — the typed-WS
    // transport binding around a `session` protocol.
    Socket,
    // `upstream` (§Fase 80.b) — the dual transport role of `socket`: a
    // persistent, config-resolved, OUTBOUND connection to a third-party
    // service (STT/TTS/realtime vendors), typed by the same §41.a session
    // algebra on the axon-facing side and transcoded to the vendor's wire
    // frames by a declared, compile-time-total projection (`map:`).
    Upstream,
    // `voice` (§Fase 80.g) — the simplicity layer: a top-level declaration
    // that macro-expands (inspectable via `axon desugar`, D80.6) to the
    // primitives already in the language: `ots` codec pair + carrier
    // `session`/`socket` (§79-interruptible when declared) + `upstream`
    // vendor legs. Under 20 lines to a working phone agent for a blessed
    // preset; never a black box.
    Voice,
    // `cors` (§Fase 83.a) — a named, referenced origin-policy declaration
    // (mirrors `shield`'s shape exactly), resolved per `axonendpoint.cors:`
    // reference. Makes the browser-facing origin policy a property of the
    // ENDPOINT, not a single process-wide knob — the only shape that fits
    // a multi-tenant deploy where different bundles need different origins
    // for a path with the same name.
    Cors,
    // `cache` (§Fase 85.a) — a named, referenced result-memoization policy,
    // resolved per `tool.cache:` / `retrieve.cache:` reference (mirrors
    // `cors`'s shape). Cacheability derives from the type system's `pure`
    // proof: a `pure` tool is safe to cache by construction. Distinct from
    // `memory` (conversational recall state) — see cache.md §"What this is NOT".
    Cache,
    // §Fase 99 — `document`: the top-level Native Document Synthesis
    // declaration (docx|pptx|xlsx), the `cache`/`savant` shape. A value LEAVES
    // the epistemic lattice here into a human artifact — the assertion-
    // laundering barrier guards the boundary (D99.1).
    Document,
    // §Fase 105 — `deliver`: the top-level Governed CRM Delivery declaration
    // (the dual of acquisition, §98/§102). A value LEAVES the epistemic lattice
    // here into a system of record — the assertion-laundering barrier in egress
    // form (axon-T920) refuses provenance-stripping delivery of an unshielded
    // flow value (D105.2). The `document`/`cache`/`savant` top-level shape.
    Deliver,
    /// §Fase 110 — `notify`: governed human-notification egress.
    Notify,
    // `quant` as a cognitive primitive (§Fase 51.a) — a flow-body block that
    // projects an MEK semantic tensor into a complex Hilbert space, evolves it
    // under a variational / kernel-feature map, and collapses back to classical
    // silicon. NOT a top-level declaration (lives inside a flow body, like `par`).
    Quant,
    // §Fase 88 — `warden` adversarial-analysis block (flow-body, like `quant`)
    // + `scope` authorization-policy declaration (top-level, like `cache`).
    Warden,
    Scope,
    // §Fase 92 — `credential`: a named ephemeral-credential contract
    // (top-level, the `cors`/`scope` shape): TTL-bounded, capability-
    // attenuated (`authority_only_attenuates` — grants ⊆ the minter's own
    // capabilities at mint). `mint <Credential> as <binding>` is the flow
    // verb that mints one; the binding receives the raw bearer (shown
    // once, never persisted).
    Credential,
    Mint,
    // §Fase 94.b — `rotate <SecretsStore> [where "<filter>"] with <Tool>
    // as <binding>`: the mediated secret-renewal flow verb (doctrine
    // `rotation_without_revelation`). The runtime reveals each matching
    // secret ONLY into the tool exchange and commits the returned value
    // back to custody (CAS, version+1); the binding receives the
    // metadata-only rotation summary — never a value.
    Rotate,
    // `observable` (§Fase 51.c.2) — a top-level Pauli-sum declaration
    // `M = Σ cₖ Pₖ` (real coeffs × Pauli strings ⇒ Hermitian by construction)
    // that a `quant` block measures against.
    Observable,
    // `witness` (§Fase 69.a) — top-level Advantage-Witness declaration: a proof
    // obligation that a primitive's `claim` beats a cheaper `baseline` by a
    // `metric` above a `threshold` on real `data` (axon://logic/no_unwitnessed_advantage).
    Witness,
    // `yield` (§Fase 51.d.2) — the measurement point inside a `quant` block:
    // collapses the evolved amplitudes back to classical silicon. The effect
    // operation whose resolution is a one-shot delimited continuation.
    Yield,
    Emit,
    Publish,
    Discover,
    // Modifiers
    As,
    Within,
    ConstrainedBy,
    OnFailure,
    OutputTo,
    Effort,
    // Contextual
    For,
    In,
    Into,
    Against,
    About,
    From,
    Where,
    Let,
    Return,
    /// Fase 19.e — exit the enclosing for-in body.
    Break,
    /// Fase 19.e — skip to the next iteration of the enclosing for-in body.
    Continue,
    Or,
    /// §Fase 70.a — boolean `and` / `not` operators for the pure expression
    /// engine (peers of the pre-existing `or`; Axon uses word-operators).
    And,
    Not,
    // Field keywords
    Given,
    Ask,
    Output,

    // ── Literals ──────────────────────────────────────────────────
    StringLit,
    Integer,
    Float,
    Bool,
    Duration,
    Identifier,

    // ── Symbols ───────────────────────────────────────────────────
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Dot,
    Arrow,
    DotDot,
    Question,
    At,
    Lt,
    Gt,
    Lte,
    Gte,
    Eq,
    Neq,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    /// §Fase 70.a — modulo operator for the pure expression engine.
    Percent,

    // ── Special ───────────────────────────────────────────────────
    Eof,

    // ── Trivia (Fase 14.a — lossless lexing) ──────────────────────
    // Comment tokens emitted by the lexer instead of being silently
    // stripped. The parser collects them into a parallel `Trivia`
    // array indexed by effective-token position and attaches them to
    // AST nodes as `leading_trivia` / `trailing_trivia` (Roslyn
    // convention). This is what enables LSP hover with docstrings,
    // round-trip-preserving formatters, and rustdoc-style doc
    // generators downstream.
    //
    // Doc-comment heuristic (mirrors the Python lexer):
    //   //   regular line comment
    //   ///  outer doc line comment   (documents the next item)
    //   //!  inner doc line comment   (documents the enclosing item — Fase 14.c)
    //   /*   regular block comment
    //   /**  outer doc block comment  (documents the next item)
    //   /*!  inner doc block comment  (documents the enclosing item — Fase 14.c)
    // ////` (4+ slashes) and `/**/` (empty block) stay regular.
    LineComment,          // //  regular line comment
    BlockComment,         // /* */ regular block comment
    DocLineComment,       // ///  outer doc line comment
    DocBlockComment,      // /** */ outer doc block comment
    InnerDocLineComment,  // //!  inner doc line comment (Fase 14.c)
    InnerDocBlockComment, // /*! */ inner doc block comment (Fase 14.c)
}

/// Comment trivia attached to AST nodes (Fase 14.a).
///
/// Parallel of the Python `Trivia` dataclass in `axon/compiler/ast_nodes.py`.
/// Each AST node carries a `leading_trivia` slice (comments preceding
/// the node's first token) and a `trailing_trivia` slice (comments on
/// the same line as the node's last token).
#[derive(Debug, Clone, PartialEq)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub text: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriviaKind {
    Line,
    Block,
    DocLine,
    DocBlock,
    /// Inner doc line comment `//!` — documents the *enclosing* module
    /// or file rather than the next sibling. Fase 14.c.
    InnerDocLine,
    /// Inner doc block comment `/*! … */` — same convention as
    /// `InnerDocLine`. Fase 14.c.
    InnerDocBlock,
}

impl Trivia {
    /// `true` iff this trivia is any kind of doc comment — outer
    /// (`///`, `/** */`) or inner (`//!`, `/*! */`).
    pub fn is_doc(&self) -> bool {
        matches!(
            self.kind,
            TriviaKind::DocLine
                | TriviaKind::DocBlock
                | TriviaKind::InnerDocLine
                | TriviaKind::InnerDocBlock,
        )
    }

    /// `true` iff this trivia is an *inner* doc comment (`//!` or
    /// `/*! … */`). Inner doc comments document the enclosing item.
    pub fn is_inner_doc(&self) -> bool {
        matches!(
            self.kind,
            TriviaKind::InnerDocLine | TriviaKind::InnerDocBlock,
        )
    }

    /// Body of the comment with marker prefixes/suffixes removed.
    /// Useful for LSP hover rendering.
    pub fn stripped_text(&self) -> &str {
        match self.kind {
            TriviaKind::DocLine => self.text.strip_prefix("///").unwrap_or(&self.text),
            TriviaKind::InnerDocLine => self.text.strip_prefix("//!").unwrap_or(&self.text),
            TriviaKind::Line => self.text.strip_prefix("//").unwrap_or(&self.text),
            TriviaKind::DocBlock => {
                let s = self.text.strip_prefix("/**").unwrap_or(&self.text);
                s.strip_suffix("*/").unwrap_or(s)
            }
            TriviaKind::InnerDocBlock => {
                let s = self.text.strip_prefix("/*!").unwrap_or(&self.text);
                s.strip_suffix("*/").unwrap_or(s)
            }
            TriviaKind::Block => {
                let s = self.text.strip_prefix("/*").unwrap_or(&self.text);
                s.strip_suffix("*/").unwrap_or(s)
            }
        }
    }
}

pub fn keyword_type(word: &str) -> TokenType {
    match word {
        "persona" => TokenType::Persona,
        "context" => TokenType::Context,
        "intent" => TokenType::Intent,
        "flow" => TokenType::Flow,
        "reason" => TokenType::Reason,
        "anchor" => TokenType::Anchor,
        "validate" => TokenType::Validate,
        "refine" => TokenType::Refine,
        "memory" => TokenType::Memory,
        "tool" => TokenType::Tool,
        "probe" => TokenType::Probe,
        "weave" => TokenType::Weave,
        "step" => TokenType::Step,
        "type" => TokenType::Type,
        "import" => TokenType::Import,
        "run" => TokenType::Run,
        "if" => TokenType::If,
        "else" => TokenType::Else,
        "use" => TokenType::Use,
        "remember" => TokenType::Remember,
        "recall" => TokenType::Recall,
        "know" => TokenType::Know,
        "believe" => TokenType::Believe,
        "speculate" => TokenType::Speculate,
        "doubt" => TokenType::Doubt,
        "par" => TokenType::Par,
        "consolidate" => TokenType::Consolidate,
        "hibernate" => TokenType::Hibernate,
        "deliberate" => TokenType::Deliberate,
        "consensus" => TokenType::Consensus,
        "forge" => TokenType::Forge,
        "ots" => TokenType::Ots,
        "teleology" => TokenType::Teleology,
        "homotopy_search" => TokenType::HomotopySearch,
        "linear_constraints" => TokenType::LinearConstraints,
        "loss_function" => TokenType::LossFunction,
        "stream" => TokenType::Stream,
        "on_chunk" => TokenType::OnChunk,
        "on_complete" => TokenType::OnComplete,
        "effects" => TokenType::Effects,
        "pure" => TokenType::Pure,
        "network" => TokenType::Network,
        "agent" => TokenType::Agent,
        "goal" => TokenType::Goal,
        "tools" => TokenType::Tools,
        "budget" => TokenType::Budget,
        "strategy" => TokenType::Strategy,
        "on_stuck" => TokenType::OnStuck,
        "shield" => TokenType::Shield,
        // §Fase 71.a — temporal execution-window guard.
        "window" => TokenType::Window,
        "scan" => TokenType::Scan,
        "on_breach" => TokenType::OnBreach,
        "severity" => TokenType::Severity,
        "allow" => TokenType::Allow,
        "deny" => TokenType::Deny,
        "sandbox" => TokenType::Sandbox,
        "quarantine" => TokenType::Quarantine,
        "redact" => TokenType::Redact,
        "pix" => TokenType::Pix,
        "ledger" => TokenType::Ledger,
        "navigate" => TokenType::Navigate,
        "drill" => TokenType::Drill,
        "trail" => TokenType::Trail,
        "psyche" => TokenType::Psyche,
        "dimensions" => TokenType::Dimensions,
        "manifold" => TokenType::Manifold,
        "quantum" => TokenType::Quantum,
        "inference" => TokenType::Inference,
        "corpus" => TokenType::Corpus,
        "corroborate" => TokenType::Corroborate,
        "edge_filter" => TokenType::EdgeFilter,
        "dataspace" => TokenType::Dataspace,
        "ingest" => TokenType::Ingest,
        "focus" => TokenType::Focus,
        "grad" => TokenType::Grad,
        "associate" => TokenType::Associate,
        "aggregate" => TokenType::Aggregate,
        "explore" => TokenType::Explore,
        "mcp" => TokenType::Mcp,
        "taint" => TokenType::Taint,
        "mandate" => TokenType::Mandate,
        "constraint" => TokenType::Constraint,
        "kp" => TokenType::Kp,
        "ki" => TokenType::Ki,
        "kd" => TokenType::Kd,
        "tolerance" => TokenType::Tolerance,
        "max_steps" => TokenType::MaxSteps,
        "on_violation" => TokenType::OnViolation,
        "daemon" => TokenType::Daemon,
        "listen" => TokenType::Listen,
        "budget_per_event" => TokenType::BudgetPerEvent,
        "compute" => TokenType::Compute,
        // §Fase 111 — "logic" is no longer a keyword (see the TokenType enum).
        "lambda" => TokenType::Lambda,
        "ontology" => TokenType::Ontology,
        "certainty" => TokenType::Certainty,
        "temporal_frame" => TokenType::TemporalFrame,
        "provenance" => TokenType::Provenance,
        "derivation" => TokenType::Derivation,
        "axonstore" => TokenType::AxonStore,
        "schema" => TokenType::Schema,
        "persist" => TokenType::Persist,
        "retrieve" => TokenType::Retrieve,
        "mutate" => TokenType::Mutate,
        "purge" => TokenType::Purge,
        "transact" => TokenType::Transact,
        "axonendpoint" | "axpoint" => TokenType::AxonEndpoint,
        // §Fase 53 — Closed-catalog extension mechanism
        "extension" => TokenType::Extension,
        // I/O Cognitivo (§λ-L-E Fase 1 — Resources)
        "resource" => TokenType::Resource,
        "fabric" => TokenType::Fabric,
        "manifest" => TokenType::Manifest,
        "observe" => TokenType::Observe,
        // Control Cognitivo (§λ-L-E Fase 3)
        "reconcile" => TokenType::Reconcile,
        "lease" => TokenType::Lease,
        "ensemble" => TokenType::Ensemble,
        // Topology & Session (§λ-L-E Fase 4 — π-calculus)
        "topology" => TokenType::Topology,
        "session" => TokenType::Session,
        "send" => TokenType::Send,
        "receive" => TokenType::Receive,
        "loop" => TokenType::Loop,
        "end" => TokenType::End,
        // Immune System (§λ-L-E Fase 5)
        "immune" => TokenType::Immune,
        "reflex" => TokenType::Reflex,
        "heal" => TokenType::Heal,
        // UI Cognitiva (§λ-L-E Fase 9)
        "component" => TokenType::Component,
        "view" => TokenType::View,
        // Mobile Typed Channels (§λ-L-E Fase 13)
        "channel" => TokenType::Channel,
        // WebSocket as a cognitive primitive (§Fase 41.b)
        "socket" => TokenType::Socket,
        // `upstream` (§Fase 80.b) — outbound vendor connection.
        "upstream" => TokenType::Upstream,
        // `voice` (§Fase 80.g) — the inspectable voice-agent sugar.
        "voice" => TokenType::Voice,
        // `cors` (§Fase 83.a) — the named origin-policy declaration.
        "cors" => TokenType::Cors,
        // `cache` (§Fase 85.a) — the named result-memoization policy.
        "cache" => TokenType::Cache,
        "document" => TokenType::Document,
        // `deliver` (§Fase 105) — the Governed CRM Delivery declaration.
        "deliver" => TokenType::Deliver,
        "notify" => TokenType::Notify,
        // `savant` (§Fase 87.a) — long-horizon autonomous research primitive.
        "savant" => TokenType::Savant,
        // `synth` (§Fase 87.d) — dynamic tool-synthesis policy.
        "synth" => TokenType::Synth,
        // `warden` (§Fase 88.a) — adversarial security-analysis flow-body block.
        "warden" => TokenType::Warden,
        // `scope` (§Fase 88.a) — authorization-scope policy declaration.
        "scope" => TokenType::Scope,
        // `credential` (§Fase 92.a) — ephemeral-credential contract declaration.
        "credential" => TokenType::Credential,
        // `mint` (§Fase 92.b) — the credential-minting flow verb.
        "mint" => TokenType::Mint,
        // `rotate` (§Fase 94.b) — the mediated secret-renewal flow verb.
        "rotate" => TokenType::Rotate,
        // `quant` as a cognitive primitive (§Fase 51.a) — flow-body block.
        "quant" => TokenType::Quant,
        // `observable` (§Fase 51.c.2) — top-level Pauli-sum declaration.
        "observable" => TokenType::Observable,
        // `witness` (§Fase 69.a) — Advantage-Witness declaration.
        "witness" => TokenType::Witness,
        // `yield` (§Fase 51.d.2) — quant measurement point.
        "yield" => TokenType::Yield,
        // §Fase 120 — algebraic effects. `effect` (singular) is the top-level
        // declaration; `effects` (plural, above) stays the tool effect-row
        // field. The two are distinguished by the `match` arm, not by context.
        "effect" => TokenType::Effect,
        "handle" => TokenType::Handle,
        "perform" => TokenType::Perform,
        "resume" => TokenType::Resume,
        "abort" => TokenType::Abort,
        "forward" => TokenType::Forward,
        "emit" => TokenType::Emit,
        "publish" => TokenType::Publish,
        "discover" => TokenType::Discover,
        "as" => TokenType::As,
        "within" => TokenType::Within,
        "constrained_by" => TokenType::ConstrainedBy,
        "on_failure" => TokenType::OnFailure,
        "output_to" => TokenType::OutputTo,
        "effort" => TokenType::Effort,
        "for" => TokenType::For,
        "in" => TokenType::In,
        "into" => TokenType::Into,
        "against" => TokenType::Against,
        "about" => TokenType::About,
        "from" => TokenType::From,
        "where" => TokenType::Where,
        "let" => TokenType::Let,
        "return" => TokenType::Return,
        "break" => TokenType::Break,
        "continue" => TokenType::Continue,
        "or" => TokenType::Or,
        // §Fase 70.a — boolean operators for the pure expression engine.
        "and" => TokenType::And,
        "not" => TokenType::Not,
        "given" => TokenType::Given,
        "ask" => TokenType::Ask,
        "output" => TokenType::Output,
        "true" | "false" => TokenType::Bool,
        _ => TokenType::Identifier,
    }
}

/// Returns true for keywords that open a top-level declaration.
/// Used by the structural declaration counter (depth-0 scan).
pub fn is_declaration_keyword(tt: &TokenType) -> bool {
    matches!(
        tt,
        TokenType::Persona
            | TokenType::Flow
            | TokenType::Anchor
            | TokenType::Context
            | TokenType::Intent
            | TokenType::Type
            | TokenType::Memory
            | TokenType::Tool
            | TokenType::Probe
            | TokenType::Weave
            | TokenType::Agent
            | TokenType::Shield
            | TokenType::Pix
            | TokenType::Ledger
            | TokenType::Psyche
            | TokenType::Corpus
            | TokenType::Dataspace
            | TokenType::Mcp
            | TokenType::Daemon
            | TokenType::Compute
            | TokenType::Mandate
            | TokenType::Lambda
            | TokenType::AxonStore
            | TokenType::AxonEndpoint
            | TokenType::Extension
            | TokenType::Import
            | TokenType::Run
            // §Fase 120 — `effect E { Op(p: T) -> R }`, the algebraic-effect
            // declaration. A peer of `tool` / `persona` / `anchor`, per
            // `fase_23` §3.1's placement ("top-level, like tool/persona/anchor").
            | TokenType::Effect
            // §λ-L-E Fase 1–5 — I/O cognitivo, control, topology, immune
            | TokenType::Resource
            | TokenType::Fabric
            | TokenType::Manifest
            | TokenType::Observe
            | TokenType::Reconcile
            | TokenType::Lease
            | TokenType::Ensemble
            | TokenType::Topology
            | TokenType::Session
            | TokenType::Immune
            | TokenType::Reflex
            | TokenType::Heal
            // §λ-L-E Fase 9 — UI cognitiva
            | TokenType::Component
            | TokenType::View
            // §λ-L-E Fase 13 — Mobile typed channels
            | TokenType::Channel
            // §Fase 41.b — typed WebSocket transport
            | TokenType::Socket
            // §Fase 80.b — outbound vendor connection (the client dual of socket)
            | TokenType::Upstream
            // §Fase 80.g — the voice-agent simplicity layer
            | TokenType::Voice
            // §Fase 83.a — the named origin-policy declaration
            | TokenType::Cors
            // §Fase 85.a — the named result-memoization policy declaration
            | TokenType::Cache
            | TokenType::Document
            // §Fase 105 — the Governed CRM Delivery declaration
            | TokenType::Deliver
            // §Fase 87.a — the long-horizon autonomous research primitive
            | TokenType::Savant
            // §Fase 87.d — dynamic tool-synthesis policy
            | TokenType::Synth
            // §Fase 88.a — the authorization-scope policy declaration
            | TokenType::Scope
            // §Fase 51.c.2 — Pauli-sum observable declaration
            | TokenType::Observable
            // §Fase 69.a — Advantage-Witness declaration
            | TokenType::Witness
    )
}

#[cfg(test)]
mod tests_lang_extensions {
    //! Regression tests for the Fase 1–5 keyword additions.
    use super::*;

    fn check(word: &str, expected: TokenType) {
        assert_eq!(keyword_type(word), expected, "keyword '{word}'");
    }

    #[test]
    fn fase1_io_cognitivo_keywords() {
        check("resource", TokenType::Resource);
        check("fabric", TokenType::Fabric);
        check("manifest", TokenType::Manifest);
        check("observe", TokenType::Observe);
    }

    #[test]
    fn fase3_control_keywords() {
        check("reconcile", TokenType::Reconcile);
        check("lease", TokenType::Lease);
        check("ensemble", TokenType::Ensemble);
    }

    #[test]
    fn fase4_topology_and_session_keywords() {
        check("topology", TokenType::Topology);
        check("session", TokenType::Session);
        check("send", TokenType::Send);
        check("receive", TokenType::Receive);
        check("loop", TokenType::Loop);
        check("end", TokenType::End);
    }

    #[test]
    fn fase5_immune_keywords() {
        check("immune", TokenType::Immune);
        check("reflex", TokenType::Reflex);
        check("heal", TokenType::Heal);
    }

    #[test]
    fn new_decl_keywords_are_declaration_level() {
        for tt in [
            TokenType::Resource,
            TokenType::Fabric,
            TokenType::Manifest,
            TokenType::Observe,
            TokenType::Reconcile,
            TokenType::Lease,
            TokenType::Ensemble,
            TokenType::Topology,
            TokenType::Session,
            TokenType::Immune,
            TokenType::Reflex,
            TokenType::Heal,
        ] {
            assert!(
                is_declaration_keyword(&tt),
                "{tt:?} must be a top-level decl"
            );
        }
    }

    #[test]
    fn session_body_keywords_are_not_declaration_level() {
        for tt in [
            TokenType::Send,
            TokenType::Receive,
            TokenType::Loop,
            TokenType::End,
        ] {
            assert!(
                !is_declaration_keyword(&tt),
                "{tt:?} is a session-body step, not a top-level decl"
            );
        }
    }

    #[test]
    fn unknown_words_fall_back_to_identifier() {
        assert_eq!(keyword_type("frobnicate"), TokenType::Identifier);
        assert_eq!(keyword_type("PatientRecord"), TokenType::Identifier);
    }

    // §λ-L-E Fase 13 — Mobile typed channels.

    #[test]
    fn fase13_channel_keywords() {
        check("channel", TokenType::Channel);
        check("emit", TokenType::Emit);
        check("publish", TokenType::Publish);
        check("discover", TokenType::Discover);
    }

    #[test]
    fn fase13_channel_is_top_level_decl() {
        assert!(
            is_declaration_keyword(&TokenType::Channel),
            "channel must be a top-level decl"
        );
    }

    #[test]
    fn fase13_emit_publish_discover_are_flow_steps_not_decls() {
        for tt in [TokenType::Emit, TokenType::Publish, TokenType::Discover] {
            assert!(
                !is_declaration_keyword(&tt),
                "{tt:?} is a flow-step reduction, not a top-level decl"
            );
        }
    }
}
