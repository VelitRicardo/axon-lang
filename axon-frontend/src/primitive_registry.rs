//! The closed registry of every primitive AXON exposes as a named
//! language construct.
//!
//! # Why this exists
//!
//! Before §Fase 6.a, the answer to "what primitives does AXON have?"
//! was scattered across the parser dispatch table, the type checker's
//! validation arms, the ℰMCP knowledge corpus, and a half-dozen
//! markdown reference pages. There was no machine-readable canonical
//! list. A new primitive landing in the parser could go undocumented
//! for a release cycle (or three) before someone noticed.
//!
//! `PRIMITIVE_REGISTRY` closes that gap. It is **the** single source
//! of truth for the closed set of primitive names — consumed by:
//!
//! - **ℰMCP coverage gate** — tests under `axon-emcp` that assert
//!   every `Documented` entry has a markdown body in the corpus, AND
//!   that every markdown body has a `Documented` entry here. The
//!   closed set is enforced on BOTH sides; the corpus cannot drift.
//! - **`axon-emcp scaffold-primitive` CLI** — reads the entry, stamps
//!   a markdown skeleton with frontmatter pre-populated from the
//!   registry. Reduces "add new primitive doc" to a 30-second task.
//! - **Future LSP completions / docs site / `axon.primitives()` tool**
//!   — any consumer that needs a deterministic catalogue iterates
//!   over `PRIMITIVE_REGISTRY` directly.
//!
//! # Discipline
//!
//! When a new primitive lands in the parser, the SAME PR adds the
//! entry here AND the markdown doc under
//! `src/knowledge/primitives/<name>.md`. The two are atomic. No
//! orphan parser productions, no orphan corpus entries.
//!
//! For primitives that exist in the parser today but haven't been
//! documented yet, the entry lives here with `doc_status: Pending`.
//! The §Fase 6.b–d roadmap flips each `Pending` → `Documented` as
//! its `.md` lands.

/// One primitive's **shallow** metadata. Deep documentation (grammar,
/// fields, runtime behaviour, examples, see-also) lives in the
/// markdown corpus under `src/knowledge/primitives/<name>.md` —
/// surfaced by the ℰMCP catalogue loader via
/// `axon.primitive_doc(<name>)`. The registry carries only what every
/// consumer needs to identify and classify the primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveInfo {
    /// Canonical name as it appears in source (`persona`, `flow`,
    /// `socket`, `axonendpoint`, …). Doubles as the markdown file
    /// stem (`<name>.md`) and the URL slug for
    /// `axon://primitives/{name}`.
    pub name: &'static str,
    /// Closed-catalogue family. Drives the
    /// `axon.primitives(filter)` facet and the `category:` field of
    /// the corpus frontmatter. Valid values: `"cognition"`,
    /// `"cognitive_io"`, `"data_plane"`, `"session_types"`, `"wire"`,
    /// `"operators"`. Validated against the ℰMCP `Category` enum at
    /// catalog-load time — a category string here that does not
    /// deserialize into a `Category` is a coverage-gate failure.
    pub category: &'static str,
    /// `true` ⇒ this primitive is a top-level declaration (it stands
    /// alone at the program root). `false` ⇒ it only appears nested
    /// inside another construct (e.g. `step` inside a `flow`).
    pub top_level: bool,
    /// The cycle that introduced this primitive (e.g. `"v0.1.0"`,
    /// `"Fase 41.b (v2.3.0)"`). Surfaced verbatim in the corpus
    /// frontmatter `since:` field.
    pub since: &'static str,
    /// One-line summary used by `axon.primitives()` listings and by
    /// the `axon-emcp scaffold-primitive` CLI when stamping a new
    /// doc's frontmatter. Should fit on one line, end with a period.
    pub summary: &'static str,
    /// Whether the primitive has a corresponding markdown doc in the
    /// ℰMCP corpus today. The §Fase 6 plan ships every primitive's
    /// doc in tiers (6.b, 6.c, 6.d); entries flip from `Pending` to
    /// `Documented` as their `.md` lands.
    pub doc_status: DocStatus,
    /// §Fase 114.z — whether this primitive is part of the PUBLIC
    /// PROMISE. `true` ⇒ the anti-drift gate (`advertised.rs`) REQUIRES
    /// a `RuntimeStatus` classification for it AND a `<code>` badge in
    /// the public README — a primitive cannot be part of the promise
    /// without someone stating, on the record, what its runtime does.
    ///
    /// Why this field exists: the §111 gate was 100% README-badge-driven,
    /// so a primitive DISCUSSED but never BADGED escaped classification
    /// entirely — `budget` appeared in the README seventeen times, was
    /// badged zero, and its dead runtime survived every green build until
    /// §114.a. **A gate that guards only the badges guards the
    /// packaging.** The registry is the enumeration source now; the
    /// badges are a projection of it, not the other way around.
    ///
    /// `false` is for primitives the parser accepts that are deliberately
    /// NOT promised (internal mechanisms, or constructs whose public
    /// claims are gated on other doctrine — see `witness`). A `false`
    /// entry must be neither badged nor classified; the gate enforces
    /// both directions.
    pub is_advertised: bool,
}

/// Coverage status for one primitive's documentation. The coverage
/// gate in `axon-emcp` asserts that:
///
/// 1. every `Documented` entry has a `.md` under
///    `src/knowledge/primitives/`;
/// 2. every `.md` under that directory has a `Documented` entry here.
///
/// `Pending` entries are visible in the registry (so the catalogue
/// is honestly complete) but the coverage gate does NOT require
/// their docs yet — that is the §Fase 6.b–d roadmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocStatus {
    /// Documented — has a markdown body in the corpus + passes the
    /// drift-gated canonical-program test where applicable. The
    /// coverage gate requires `<name>.md` to exist.
    Documented,
    /// Pending — the primitive exists in the language but its
    /// markdown doc has not landed yet. The §Fase 6 roadmap names
    /// the cycle (6.b / 6.c / 6.d) that closes the gap.
    Pending,
}

impl DocStatus {
    /// Stringify for diagnostics. Mirrors the `serde` rename rule
    /// the ℰMCP catalogue uses for its own enums.
    pub fn as_str(self) -> &'static str {
        match self {
            DocStatus::Documented => "documented",
            DocStatus::Pending => "pending",
        }
    }
}

/// The closed catalogue — **91 primitives, all Documented** (§6.d reached
/// 100% at 66; the §114.z census backfilled 25 parser realities and its
/// docs-tier paid their corpus docs, restoring 100% at the new, honest
/// denominator). Ordered by category for readability. Consumers must not
/// depend on declaration order; they iterate and filter.
///
/// §Fase 114.z made this table the ENUMERATION SOURCE of the
/// anti-drift gate (`advertised.rs`): every `is_advertised` entry must
/// carry a `RuntimeStatus` classification and a README badge. See the
/// census section at the bottom for what was missing and why.
pub const PRIMITIVE_REGISTRY: &[PrimitiveInfo] = &[
    // ── Cognition ─────────────────────────────────────────────────────
    PrimitiveInfo {
        name: "persona",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "Declares the identity, expertise, and refusal posture an agent adopts when executing a flow.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "context",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "Declares the conversational frame — memory scope, depth, max tokens, temperature — a flow operates within.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "flow",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "The orchestration primitive — a typed, ordered composition of cognitive steps with parameters and a return type.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "anchor",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "A typed grounding constraint — declares the conditions a flow's outputs MUST satisfy, with a structured violation policy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "tool",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "A declarative binding for an external capability (search, web fetch, code interpreter, …) callable from within a flow.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "intent",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "A declarative target outcome — what the flow is trying to achieve, separately from how it gets there.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "memory",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "Declares a typed memory store — session, persistent, vector — for cross-step state with retrieval + decay semantics.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "agent",
        category: "cognition",
        top_level: true,
        since: "Fase 18",
        summary: "An orchestrated cognitive entity — composes personas, tools, contexts under a coordination strategy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "run",
        category: "cognition",
        top_level: true,
        since: "v0.1.0",
        summary: "Binds a flow to a persona, context, and anchors — the statement that EXECUTES a declared flow.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "step",
        category: "cognition",
        top_level: false,
        since: "v0.1.0",
        summary: "A single cognitive operation inside a flow — typed input (given), prompt (ask), and typed output.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "reason",
        category: "cognition",
        top_level: false,
        since: "v0.1.0",
        summary: "An explicit-reasoning operation — declares HOW the model should think (chain-of-thought, debate, …).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "probe",
        category: "cognition",
        top_level: false,
        since: "v0.1.0",
        summary: "A diagnostic / probing operation inside a step — emits observations without changing the trajectory.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "validate",
        category: "cognition",
        top_level: false,
        since: "v0.1.0",
        summary: "Enforces a typed invariant on a step's output before subsequent steps consume it.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "refine",
        category: "cognition",
        top_level: false,
        since: "v0.1.0",
        summary: "Iteratively improves a candidate output via a declared refinement strategy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "weave",
        category: "cognition",
        top_level: false,
        since: "v0.1.0",
        summary: "Multi-thread reasoning braid — composes multiple sub-derivations into a unified conclusion.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // ── Cognitive I/O ─────────────────────────────────────────────────
    PrimitiveInfo {
        name: "resource",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "Declares an external compute/storage resource (database, S3, ML endpoint) consumable by a flow.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "fabric",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "The cloud-substrate declaration: provider/region are cross-validated at compile time (an impossible region and an unsatisfiable compliance obligation are both refused) and travel with every channel bound to a resource placed `within` it, into its audit row.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "manifest",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "Bundles resources + fabric + compliance tags into a deployable, audit-tracked unit.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "observe",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "Declares an observability surface — sources, quorum, timeout, certainty floor, partition policy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "reconcile",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "A typed reconciliation loop — observes drift against a manifest and applies bounded corrections.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "lease",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "Time-bounded resource acquisition with typed expiry, renewal, and revocation semantics.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "ensemble",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 6",
        summary: "Coordinates multiple cognitive entities under a consensus or quorum protocol with structured tie-breaking.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "immune",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 19",
        summary: "Continuous-monitoring agent that learns a baseline + emits epistemic-level signals on anomalies.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "reflex",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 19",
        summary: "An automatic-response trigger bound to an immune system's level — fires structured actions on threshold breach.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "heal",
        category: "cognitive_io",
        top_level: true,
        since: "Fase 19",
        summary: "A recovery routine bound to an immune system's level — runs scoped repairs, often human-in-the-loop.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // ── Data plane ────────────────────────────────────────────────────
    PrimitiveInfo {
        name: "type",
        category: "data_plane",
        top_level: true,
        since: "v0.1.0",
        summary: "Declares a structured data type with optional refinements, ranges, where clauses, and compliance tags.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "json",
        category: "data_plane",
        top_level: false,
        since: "Fase 73",
        summary: "The open, semi-structured value type — a totally-navigable JSON document, refinable by an optional `Json<T>` shape lens, total and honest always.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "axonstore",
        category: "data_plane",
        top_level: true,
        since: "Fase 36",
        summary: "A typed, audit-chained data store — relational backend, isolation level, encryption, retention, on-breach policy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "dataspace",
        category: "data_plane",
        top_level: true,
        since: "Fase 36",
        summary: "A declared analytical data container — the deterministic data plane's store (§108). Typed columnar schema (`column <name>: <Type>` over the closed 6-type catalog, axon-T928), instantiated in the columnar engine at deploy; governed `ingest` (§108.c) + the relational query verbs (§108.d) land next.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "corpus",
        category: "data_plane",
        top_level: true,
        since: "Fase 36",
        summary: "A retrieval-ready collection of documents — backs RAG and grounded retrieval with citation provenance.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "pix",
        category: "data_plane",
        top_level: true,
        since: "Fase 19",
        summary: "PIX retrieval navigator — an embeddings-free structural index navigated by conditional-mutual-information descent (no vector store). Consumed by navigate/drill/trail.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "ledger",
        category: "data_plane",
        top_level: true,
        since: "Fase 62",
        summary: "Audit chain — an append-only, hash-linked record of every state transition over a bound surface, tamper-evident by construction (formerly the Provenance-Index reading of pix).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // §Fase 111 — `transact` REMOVED (retracted, axon-T938).
    //
    // Its summary here read: "wraps multiple data-plane mutations in a single
    // transactional unit with rollback semantics." None of that was true. The
    // runtime inserted an unread marker string (`__txn_active = "true"`),
    // `TransactBlock` carried only a source location so the block's body was
    // never even lowered into the IR, and no transaction was opened, no lock
    // taken, nothing rolled back. We shipped it in a knowledge template.
    //
    // The registry's own rule — "an entry without a parser production lies" —
    // was too weak: `transact` HAD a parser production. §111 widens it: an
    // entry whose SUMMARY the runtime does not honour lies just as loudly, and
    // is more dangerous, because a rollback that does not happen is only
    // discovered on the failure path. A retracted primitive is not an exposed
    // primitive; it leaves the closed set, and `axon check` teaches the truth
    // via T938 instead.
    // ── Session types (§Fase 41) ──────────────────────────────────────
    PrimitiveInfo {
        name: "session",
        category: "session_types",
        top_level: true,
        since: "Fase 41.a (v2.3.0)",
        summary: "Declares the typed bidirectional dialogue protocol a socket carries — §41 algebra (send/receive/select/branch/loop/end).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "socket",
        category: "session_types",
        top_level: true,
        since: "Fase 41.b (v2.3.0)",
        summary: "Session-typed WebSocket transport with credit-refined backpressure, typed reconnection, and SSE-as-fragment projection.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "upstream",
        category: "session_types",
        top_level: true,
        since: "Fase 80.b (v2.37.0)",
        summary: "Outbound vendor connection (the client dual of socket): config-resolved dial, declared auth, and a compile-time-total wire↔session projection — a new vendor is a declaration, not new code.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "voice",
        category: "session_types",
        top_level: true,
        since: "Fase 80.g (v2.37.0)",
        summary: "The voice-agent simplicity layer: macro-expands (inspectable via axon desugar) to ots codecs + a carrier session/socket + upstream vendor legs — a blessed-preset phone agent in under 20 lines.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // ── Wire ──────────────────────────────────────────────────────────
    PrimitiveInfo {
        name: "axonendpoint",
        category: "wire",
        top_level: true,
        since: "Fase 32",
        summary: "HTTP REST primitive — exposes a flow on a typed route with body/output schemas, transport classification, and compliance.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "axpoint",
        category: "wire",
        top_level: true,
        since: "Fase 32",
        summary: "Lightweight axonendpoint — for simple request/response flows without the full request-binding schema scaffolding.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "daemon",
        category: "wire",
        top_level: true,
        since: "Fase 16",
        summary: "A long-lived, supervised cognitive process — reacts to events on declared listeners with structured restart semantics.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "mcp",
        category: "wire",
        top_level: true,
        since: "Fase 33+",
        summary: "Declares an outbound MCP server binding — turns axon into an MCP client of another server.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // §Fase 6.c — `taint` was registered in 6.a as a wire primitive
    // but has no parser production (the lexer recognises the
    // keyword token; no `parse_taint` exists; the language treats
    // `taint` only as part of the epistemic-uncertainty lattice
    // wording in `epistemic.rs`). Registry is the source of truth
    // for what the parser actually accepts as a top-level
    // declaration; an entry without a parser production lies. We
    // remove it here. If a future Fase introduces `taint <Name> {
    // ... }`, re-add an entry with that Fase's `since:` tag.
    PrimitiveInfo {
        name: "listen",
        category: "wire",
        top_level: false,
        since: "Fase 16",
        summary: "A flow/daemon-body listener — binds to an event source and dispatches typed messages downstream.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // §Fase 77 — the π-calc channel quartet, undocumented since Fase 13
    // (Kivi brief #51 §B.2: `axon.primitive_doc` answered *unknown
    // primitive* for constructs the parser accepts). Registered +
    // documented together, per the atomicity discipline above.
    PrimitiveInfo {
        name: "channel",
        category: "wire",
        top_level: true,
        since: "Fase 13",
        summary: "A typed π-calculus channel — message type, qos, lifetime, persistence, and shield gate for in-process delivery and signed external egress.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "emit",
        category: "wire",
        top_level: false,
        since: "Fase 13",
        summary: "The π-calculus output prefix — emits a typed value onto a channel; durable channels append to the at-least-once outbox.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "publish",
        category: "wire",
        top_level: false,
        since: "Fase 13",
        summary: "Capability extrusion — publishes a shield-gated channel for discovery; under a signing shield it declares the channel for signed webhook egress.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "discover",
        category: "wire",
        top_level: false,
        since: "Fase 13",
        summary: "The dual of publish — imports a previously published channel capability under a local alias.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // ── Operators ─────────────────────────────────────────────────────
    PrimitiveInfo {
        name: "shield",
        category: "operators",
        top_level: true,
        since: "Fase 20",
        summary: "A composable defence layer — scans inputs/outputs for declared threats with a structured on-breach policy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "window",
        category: "operators",
        top_level: true,
        since: "Fase 71",
        summary: "A timezone-aware temporal execution guard — gates a scheduled daemon's ticks to allowed day/hour spans (minus holiday dates) with a skip/warn/defer policy.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "cors",
        category: "operators",
        top_level: true,
        since: "Fase 83 (v2.38.0)",
        summary: "A named, referenced browser-origin policy — `axonendpoint.cors:` resolves it dynamically per tenant at the enterprise HTTP edge, secure by default when absent.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "cache",
        category: "operators",
        top_level: true,
        since: "Fase 85 (v2.40.0)",
        summary: "A named, referenced result-memoization policy — cacheability derives from the type system's `effects: pure` proof; `tool.cache:` / `retrieve.cache:` opt in, a single `default: true` auto-covers every pure tool, and a non-pure cache must carry a finite `ttl:`.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "document",
        category: "operators",
        top_level: true,
        since: "Fase 99 (v2.53.0)",
        summary: "Native Document Synthesis — a declarative, compile-time-validated DOCX/PPTX/XLSX structure. The egress boundary where a value LEAVES the epistemic lattice into a human artifact: the assertion-laundering barrier refuses a flow value below `believe` in an assertive slot without `attribute:` or a shield. Byte-deterministic output; provenance travels inside the artifact.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "deliver",
        category: "operators",
        top_level: true,
        since: "Fase 105 (v2.60.0)",
        summary: "Governed CRM Delivery — the egress-dual of acquisition. A declarative, compile-time-validated write of assertions into a system of record (a CRM): idempotent operations (`upsert_contact`/`create_deal`/`add_note`), per-tenant credentials via §94 custody, `web` effect. The provenance-stripping barrier (T920) refuses a `provenance: cleared` delivery of an unshielded flow value — a vendor guess must arrive in the CRM labeled as a guess, not laundered into a fact.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "mandate",
        category: "operators",
        top_level: true,
        since: "Fase 21",
        // §Fase 119.b — THIS SUMMARY USED TO READ: "A typed approval
        // requirement — gates a flow's execution on a capability check +
        // optional segregation of duties." That is a DIFFERENT PRIMITIVE. It
        // described §90 grantability and §102 segregation of duties, neither of
        // which `mandate` has ever done, and it cost two wrong scopes before
        // anyone opened `docs/papers/paper_mandate.md`. The registry is the
        // artefact §111 built to stop primitives from being misdescribed, and
        // nothing audits that a summary matches the README — so this one rotted
        // in the one file least likely to be doubted.
        summary: "A constraint the model cannot evade — a closed-loop PID controller over the semantic error e = 1 − CSR that refines generation until |e| < ε within N steps, or fails closed.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "compute",
        category: "operators",
        top_level: true,
        since: "Fase 17",
        summary: "Binds a flow to a specific compute backend — model selection, effort hint, parallelism, deterministic seed.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "lambda",
        category: "operators",
        top_level: true,
        since: "Fase 15",
        summary: "An anonymous, typed function bound to a flow's data plane — supports lambda apply semantics for inline composition.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "forge",
        category: "operators",
        top_level: false,
        since: "Fase 18 (stub); Fase 86 (v2.41.0)",
        summary: "Directed Creative Synthesis — a flow-body block running the Poincaré-Hadamard four-phase creative process under a measured, fail-closed novelty guarantee (NCD), so an LLM can genuinely CREATE, not just interpolate.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "ots",
        category: "operators",
        top_level: true,
        since: "Fase 11",
        summary: "One-shot transform — a closed-catalogue media transformation (audio, image, format) with native/ffmpeg backend dispatch.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "psyche",
        category: "operators",
        top_level: true,
        since: "Fase 14",
        summary: "Declares the psychological model a persona enacts — beliefs, desires, traits, behavioural disposition.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "observable",
        category: "operators",
        top_level: true,
        since: "Fase 51 (v2.19.0)",
        summary: "Declares a Hermitian observable — a Pauli-sum M = Σ cₖ Pₖ — measured by a quant block to collapse a Hilbert-space state back to a classical expectation.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "quant",
        category: "operators",
        top_level: false,
        since: "Fase 51 (v2.19.0)",
        summary: "A flow-body block that lifts a continuous carrier tensor into a finite Hilbert space, evolves it, and yields the expectation of a declared observable (the cognitive↔quantum bridge; OSS simulator capped at n≤10).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "savant",
        category: "operators",
        top_level: true,
        since: "Fase 87",
        summary: "The long-horizon autonomous research primitive — a governed ORCHESTRATOR that composes memory/corpus, budget, quant, forge and daemon into a budget-bounded, interruptible, fail-closed, provenance-witnessed research loop. Enterprise-exclusive at scale; the keyword + type discipline + PCC live in OSS.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "synth",
        category: "operators",
        top_level: true,
        since: "Fase 87",
        summary: "A dynamic tool-synthesis policy — the safety envelope (risk ceiling, source language, mandatory WASM zero-trust sandbox, Coder/Reviewer consensus) under which a savant may synthesise and run a tool at runtime. OSS disciplines the policy and ships a deny-by-default backend; the Extism executor is enterprise.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "warden",
        category: "operators",
        top_level: false,
        since: "Fase 88",
        summary: "An adversarial security-analysis flow-body block — `warden(<target>) within <Scope>` audits a target under a paraconsistent adversarial framing (abduction over authorized evidence), emitting attested `Vulnerability` findings (a witness, not LLM prose). Authorization-native: the `within <Scope>` clause is mandatory (fail-closed). The active auditor of a TARGET, distinct from `shield` (the passive I/O firewall of the AGENT).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "scope",
        category: "operators",
        top_level: true,
        since: "Fase 88",
        summary: "A named authorization scope — the signed envelope (`targets` allowlist + `depth` ceiling + `approver`) a `warden` analysis MUST run `within`. The load-bearing safety construct that makes adversarial analysis a governed, auditable, fail-closed capability rather than an unscoped weapon.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "credential",
        category: "operators",
        top_level: true,
        since: "Fase 92",
        summary: "A named ephemeral-credential contract — `ttl:` (≤ 24h, axon-T894) + `grants:` (dotted capability slugs, axon-T893). `mint <Name> as <binding>` mints a TTL-bounded bearer carrying exactly the grants, admitted only when grants ⊆ capabilities(minter) — `authority_only_attenuates`, the delegation dual of the enterprise service account.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "mint",
        category: "wire",
        top_level: false,
        since: "Fase 92",
        summary: "The credential-minting flow verb — `mint <Credential> as <binding>` mints a declared ephemeral contract at runtime (fail-closed without a minter port) and binds the raw bearer, shown once and never persisted (axon-T896).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "rotate",
        category: "wire",
        top_level: false,
        since: "Fase 94",
        summary: "The mediated secret-renewal flow verb — `rotate <SecretsStore> [where \"<filter>\"] with <Tool> as <binding>` renews every custody entry of a `backend: secrets` store's class matching the §67 filter through ONE runtime-mediated exchange per key (reveal → tool renews → CAS commit at version+1), binding the metadata-only summary. `rotation_without_revelation`: no term evaluates to a secret value; fail-closed without a custody port. Target must be a secrets store (axon-T898), the tool declared (axon-T899).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // §Fase 6.d — `logic` was registered in 6.a as an operators
    // primitive but has NO parser production (the lexer recognises
    // the `logic` keyword token; no `parse_logic` exists; the
    // top-level dispatch in `parse_declaration` has no `Logic`
    // arm). Same situation `taint` was in at 6.c. Discipline
    // applies: registry entries match parser productions
    // one-to-one. We remove `logic` here; if a future Fase
    // introduces `logic <Name> { … }`, re-add the entry with
    // that Fase's `since:` tag.
    //
    // ══ §Fase 114.z — THE CENSUS BACKFILL ═══════════════════════════════
    //
    // The registry claimed to be "the single source of truth for the
    // closed set of primitive names" while **25 named constructs with
    // live parser productions were tracked NOWHERE** — most sharply
    // `budget`, which was in neither this registry nor the `ADVERTISED`
    // table, appeared in the README seventeen times with zero badges,
    // and whose dead runtime (§114.a) therefore survived every green
    // build. A single-source-of-truth that lacks entries is not one yet;
    // §114.z audits parser dispatch (`parse_declaration` +
    // `parse_flow_step`) against this table and closes the census.
    //
    // Entries landed as `DocStatus::Pending` in the census and were flipped
    // to `Documented` by the §114.z docs-tier: every one of the 25 now has a
    // corpus doc whose body is verified against the runtime audit (several
    // honestly document a refusal — deliberate/consensus FailsClosed — or a
    // KNOWN_DEBT gap — hibernate; a doc that overclaims is the §112.f
    // defect, and these were written not to).
    //
    // DELIBERATELY EXCLUDED from the census, with reasons:
    //   - usage-verbs of registered primitives — `use` (tool),
    //     `remember`/`recall` (memory), `persist`/`retrieve`/`mutate`/
    //     `purge` (axonstore), `yield` (quant), and the apply-forms
    //     (`shield`/`ots`/`mandate`/`compute` on a target, `run` as a
    //     flow-step): their doc home is the parent primitive's `.md`.
    //   - structural control flow — `if`/`for`/`let`/`return`/`break`/
    //     `continue`/`import`: syntax, not primitives. (§115 note: the
    //     exclusion stays correct for `import` — it has no runtime
    //     kernel — but as of the Rust EMS it is REAL syntax: the
    //     compiler resolves it, T953/T954/T955 govern it, and the
    //     README's §XVI section documents the subsystem. Through
    //     v2.75.0 this exclusion quietly covered a statement nothing
    //     resolved; the census exclusion was correct, the SILENCE
    //     around the dead subsystem was the §115 finding.)
    //   - retracted constructs — `transact` (axon-T938) and
    //     `corroborate`: they parse only so the checker can teach the
    //     retraction; a retracted primitive is not an exposed one.
    //
    // ── §114.z census — top-level declarations ──────────────────────────
    PrimitiveInfo {
        name: "budget",
        category: "operators",
        top_level: true,
        since: "Fase 72 (daemon-attached); top-level Fase 114.a (v2.69.0)",
        summary: "A declared spend ceiling over cognition and tool use — enforced on the canonical `use Tool(…)` path (§114.a); attachable to a daemon or declared top-level.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "notify",
        category: "operators",
        top_level: true,
        since: "Fase 110 (v2.66.0)",
        summary: "Governed human notification — the third egress dual (deliver §105 · document §106 · notify §110): spends HUMAN ATTENTION, carries lineage or refuses, at-most-once-per-window across replicas.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "topology",
        category: "session_types",
        top_level: true,
        since: "λ-L-E Fase 4",
        summary: "Declares a process-graph topology whose liveness is checked by a genuine DFS cycle detector (Honda liveness — a narrow sufficient condition, honestly scoped).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "component",
        category: "wire",
        top_level: true,
        since: "λ-L-E Fase 9",
        summary: "A compliance-bound UI component — the compile-time shield-coverage law over regulated κ IS enforced (a real set difference); rendering is deferred (§111).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "view",
        category: "wire",
        top_level: true,
        since: "λ-L-E Fase 9",
        summary: "A UI view declaration — referential integrity is checked; route dispatch and session-typed reactivity are deferred (§111).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "witness",
        category: "operators",
        top_level: true,
        since: "Fase 69.a",
        summary: "The advantage-witness declaration (§69) — INTERNAL: benchmark and advantage claims wait for the Sandbox (§101), so this is deliberately NOT part of the advertised surface.",
        doc_status: DocStatus::Documented,
        is_advertised: false,
    },
    PrimitiveInfo {
        name: "extension",
        category: "operators",
        top_level: true,
        since: "Brief #15 → Fase 53",
        summary: "The closed-catalog extension mechanism (#15→§53) — an internal composition mechanism, deliberately not advertised as a primitive.",
        doc_status: DocStatus::Documented,
        is_advertised: false,
    },
    // ── §114.z census — the epistemic lattice scopes ────────────────────
    PrimitiveInfo {
        name: "know",
        category: "cognition",
        top_level: true,
        since: "v0.1.0 (epistemic lattice)",
        summary: "Epistemic scope — stamps its block's derivations with the `know` level of the uncertainty lattice (the strongest claim).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "believe",
        category: "cognition",
        top_level: true,
        since: "v0.1.0 (epistemic lattice)",
        summary: "Epistemic scope — stamps its block's derivations with the `believe` level of the uncertainty lattice (the egress floor for assertive slots, §99).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "speculate",
        category: "cognition",
        top_level: true,
        since: "v0.1.0 (epistemic lattice)",
        summary: "Epistemic scope — stamps its block's derivations with the `speculate` level of the uncertainty lattice.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "doubt",
        category: "cognition",
        top_level: true,
        since: "v0.1.0 (epistemic lattice)",
        summary: "Epistemic scope — stamps its block's derivations with the `doubt` level of the uncertainty lattice (the weakest claim).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // ── §114.z census — flow-body cognitive verbs ───────────────────────
    PrimitiveInfo {
        name: "par",
        category: "cognition",
        top_level: false,
        since: "pre-§65; real fan-out Fase 65",
        summary: "Concurrent fan-out — executes its branches in parallel (a real `join_all`, §65) and joins their results; branch tool calls ride the §114.e channel semaphore.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "hibernate",
        category: "cognition",
        top_level: false,
        since: "pre-§111 (introduction unrecorded)",
        summary: "Suspend-until: the flow HALTS at the hibernate point (zero further compute and tokens), the continuation parks under a deterministic SHA-256 id, `emit` on the awaited channel resumes the remaining steps, and a resume after the declared timeout is refused.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "deliberate",
        category: "cognition",
        top_level: false,
        since: "pre-§111 (introduction unrecorded)",
        summary: "Budget-bounded deliberation — FAILS CLOSED (axon-T939, §111): refused at check time rather than silently discarded; no budget was ever controlled by the old placeholder.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "consensus",
        category: "cognition",
        top_level: false,
        since: "pre-§111 (introduction unrecorded)",
        summary: "Multi-candidate consensus — FAILS CLOSED (axon-T940, §111): refused at check time; no votes, no aggregation, no candidates in the old placeholder.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "stream",
        category: "operators",
        top_level: false,
        since: "pre-§111; body executed since Fase 111.e",
        summary: "Algebraic-effects stream block — the body is parsed, lowered and EXECUTED over the Free-Monad CPS handler runtime (§111.e; it used to be discarded at parse time).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    // ── §Fase 120 — algebraic effects (Plotkin/Pretnar) ──────────────
    //
    // THREE entries, not six. `resume` / `abort` / `forward` are the discharge
    // forms of a handler CLAUSE and are a compile error anywhere else
    // (axon-T967) — they have no independent existence to register, unlike
    // `emit`/`publish`/`discover`, which are legal in any flow body. Listing
    // them would inflate the catalog with names an adopter cannot use on their
    // own, and a catalog that overstates what is reachable is the defect this
    // registry exists to prevent.
    PrimitiveInfo {
        name: "effect",
        category: "operators",
        top_level: true,
        since: "Fase 120",
        summary: "Algebraic effect declaration (Plotkin/Pretnar) — `effect E { Op(p: T) -> R }`. A CLOSED operation catalog: a bare `perform Op(x)` resolves against it, and an operation two effects both declare is refused naming both (axon-T964) rather than picked.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "handle",
        category: "operators",
        top_level: false,
        since: "Fase 120",
        summary: "Delimited handler scope — `handle E { Op(p) -> { … } } in { … }`. The clause body is ORDINARY flow steps, so a handler composes with every primitive. Discharged inside a clause by `resume(v)` (one-shot, D2), `abort(v)`, `forward E.Op(args)` (D12), or by running off the end (an implicit abort, per fase_23 §3.1).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "perform",
        category: "operators",
        top_level: false,
        since: "Fase 120",
        summary: "Raise an algebraic effect — `perform Emit(x)` or `perform SSE.Emit(x)`. Legal at flow level and in a step body (where it runs AFTER generation, so it can perform the step's own output). An undischarged perform is a COMPILE error (axon-T966, fase_23 D9): there is no runtime fallback.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "grad",
        category: "operators",
        top_level: false,
        since: "Fase 109 (v2.65.0)",
        summary: "The proof-carrying derivative — SYMBOLIC differentiation over the §70 expression language at compile time; the gradient IS IR (PCC GradientSoundness) and the runtime only evaluates.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "navigate",
        category: "data_plane",
        top_level: false,
        since: "Fases 62–65 (PIX·MDN program)",
        summary: "Knowledge navigation — three REAL deterministic engines (MDN store-sourced, MDN in-memory, PIX); with no indexable source in scope it falls back to an LLM prompt (§111 F11, the named gap).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "drill",
        category: "data_plane",
        top_level: false,
        since: "Fases 62–65 (PIX·MDN program)",
        summary: "Subtree descent under a prior navigate — real navigation when a source is in scope; degrades to a placeholder otherwise (§111).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "trail",
        category: "data_plane",
        top_level: false,
        since: "Fases 62–65 (PIX·MDN program)",
        summary: "Reads the breadcrumb a navigate seeded — a real provenance trail when navigation was deterministic; inherits §111 F11 on the LLM fallback path.",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "ingest",
        category: "data_plane",
        top_level: false,
        since: "pre-§108; made real Fase 108.c (v2.63.0)",
        summary: "Governed ingestion into a dataspace — bounds-BEFORE-parse, sha256 provenance, born-Untrusted taint (§108.c; the pre-§108 placeholder hallucinated success).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "focus",
        category: "data_plane",
        top_level: false,
        since: "Fase 108.d (v2.63.0)",
        summary: "The dataspace selection-projection verb (σ∘π) over the first-party columnar engine (§108.d).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "associate",
        category: "data_plane",
        top_level: false,
        since: "Fase 108.d (v2.63.0)",
        summary: "The dataspace join verb (⋈) — a hash equi-join over the columnar engine that REFUSES a keyless join (§108.d).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "aggregate",
        category: "data_plane",
        top_level: false,
        since: "Fase 108.d (v2.63.0)",
        summary: "The dataspace aggregation verb (γ) over the first-party columnar engine (§108.d).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
    PrimitiveInfo {
        name: "explore",
        category: "data_plane",
        top_level: false,
        since: "Fase 108.d (v2.63.0)",
        summary: "The dataspace profiling verb — zone-map statistics over declared columns (§108.d).",
        doc_status: DocStatus::Documented,
        is_advertised: true,
    },
];

/// Lookup one primitive by canonical name. O(n) over the 54-entry
/// table — n is small, the linear scan beats any hash overhead.
/// Returns `None` for unknown names; callers surface a structured
/// "unknown primitive" diagnostic.
pub fn find(name: &str) -> Option<&'static PrimitiveInfo> {
    PRIMITIVE_REGISTRY.iter().find(|i| i.name == name)
}

/// Filter the registry by closed-catalogue category. Returns an
/// iterator so callers can chain into collectors of their choice.
/// An unknown category returns an empty iterator — by design;
/// validation against the closed set is the caller's responsibility.
///
/// Lifetime `'a` ties the returned iterator to the input string so
/// edition-2021 implicit-capture rules accept the closure's borrow.
pub fn by_category<'a>(category: &'a str) -> impl Iterator<Item = &'static PrimitiveInfo> + 'a {
    PRIMITIVE_REGISTRY.iter().filter(move |i| i.category == category)
}

/// Filter the registry by documentation status. The §Fase 6 coverage
/// gate uses `with_status(DocStatus::Documented)` to know which
/// entries MUST have a corresponding `.md`.
pub fn with_status(status: DocStatus) -> impl Iterator<Item = &'static PrimitiveInfo> {
    PRIMITIVE_REGISTRY.iter().filter(move |i| i.doc_status == status)
}

/// Count primitives by `(category, doc_status)`. Used in tests + the
/// `axon-emcp` coverage gate's diagnostic output so a gate failure
/// surfaces a structured "what's missing" report rather than an
/// opaque assertion.
pub fn coverage_summary() -> CoverageSummary {
    let mut summary = CoverageSummary::default();
    for info in PRIMITIVE_REGISTRY {
        match info.doc_status {
            DocStatus::Documented => summary.documented += 1,
            DocStatus::Pending => summary.pending += 1,
        }
    }
    summary.total = PRIMITIVE_REGISTRY.len();
    summary
}

/// Aggregate documentation-coverage counts. Used by the §Fase 6
/// coverage gate + future telemetry to surface "we have N
/// primitives, M documented, N-M pending" at a glance.
#[derive(Debug, Default, Clone, Copy)]
pub struct CoverageSummary {
    pub total: usize,
    pub documented: usize,
    pub pending: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The valid category strings — must match the closed set in
    /// the ℰMCP `Category` enum. The coverage gate test in
    /// `axon-emcp` does the cross-check; here we just validate
    /// shape locally so the registry is self-consistent without
    /// needing the downstream crate.
    const VALID_CATEGORIES: &[&str] = &[
        "cognition",
        "cognitive_io",
        "data_plane",
        "session_types",
        "wire",
        "operators",
    ];

    #[test]
    fn registry_contains_the_expected_count() {
        // Total count is pinned — a regression that drops a primitive
        // surfaces as a smaller catalogue. §Fase 6.c removed `taint`
        // (47→46); §Fase 6.d removed `logic` (46→45). Both were lex-
        // recognised keywords with NO parser production, and the
        // "registry entries = parser productions" discipline meant
        // they could not stay. Future Fases that add or remove
        // primitives update this assertion in the same PR.
        // §Fase 62.0 added `ledger` (the audit chain) as a distinct top-level
        // primitive when `pix` was reassigned to the retrieval navigator (45→46).
        // §Fase 51 (v2.19.0) added `observable` + `quant` (the Hilbert-space
        // cognitive primitive + its Hermitian-observable companion) → 46→48.
        // §Fase 71 added `window` (the temporal execution guard) → 48→49.
        // §Fase 73 added `json` (the open semi-structured value type) → 49→50.
        // §Fase 77 added the π-calc channel quartet `channel` / `emit` /
        // `publish` / `discover` (parsed since Fase 13, undocumented until
        // Kivi brief #51 §B.2 caught the gap) → 50→54.
        // §Fase 80.b added `upstream` (the outbound vendor connection,
        // the client dual of `socket`) → 54→55.
        // §Fase 80.g added `voice` (the inspectable voice-agent sugar) → 55→56.
        // §Fase 83 added `cors` (the named, referenced browser-origin
        // policy) → 56→57.
        // §Fase 85 added `cache` (the named, referenced result-memoization
        // policy) → 57→58.
        // §Fase 87 added `savant` (the long-horizon autonomous research
        // primitive) + `synth` (the dynamic tool-synthesis policy) → 58→60.
        // §Fase 88 added `warden` (the adversarial security-analysis block) +
        // `scope` (the authorization-scope policy) → 60→62.
        // §Fase 92 added `credential` (the ephemeral-credential contract) +
        // `mint` (the minting flow verb) → 62→64.
        // §Fase 94 added `rotate` (the mediated secret-renewal verb) → 64→65.
        // §Fase 99 added `document` (Native Document Synthesis) → 65→66.
        // §Fase 105 added `deliver` (Governed CRM Delivery, egress-dual) → 66→67.
        // §Fase 111 RETRACTED `transact` (it never opened a transaction) → 67→66.
        // §Fase 114.z CENSUS BACKFILL → 66→91: twenty-five constructs with
        // live parser productions were tracked NOWHERE (sharpest: `budget`,
        // whose dead runtime survived every green build because no table
        // knew it existed). All 25 enter as `Pending`; see the census
        // section's exclusion list for what deliberately stays out.
        // §Fase 120 added `effect` / `handle` / `perform` → 91→94. The §23
        // algebraic-effects runtime existed all along and was tracked NOWHERE
        // for the same reason `budget` was not: no badge, no registry row,
        // nothing to classify — so no gate could ask whether a program could
        // reach it. The answer was no, totally: zero lexer tokens.
        assert_eq!(
            PRIMITIVE_REGISTRY.len(),
            94,
            "PRIMITIVE_REGISTRY count drift — add/remove the primitive intentionally + update this assertion"
        );
    }

    /// §Fase 114.z — the advertised polarity is pinned. `is_advertised:
    /// false` is an EXCEPTIONAL state (an internal mechanism, or a
    /// construct whose public claims are gated on other doctrine); every
    /// new primitive is part of the promise unless this list is
    /// deliberately grown in the same PR.
    #[test]
    fn non_advertised_entries_are_exactly_the_deliberate_exceptions() {
        let hidden: HashSet<&str> = PRIMITIVE_REGISTRY
            .iter()
            .filter(|i| !i.is_advertised)
            .map(|i| i.name)
            .collect();
        let expected: HashSet<&str> = [
            // §69 — benchmark/advantage claims wait for the Sandbox (§101).
            "witness",
            // #15→§53 — internal composition mechanism, not a promised primitive.
            "extension",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            hidden, expected,
            "is_advertised polarity drift — hiding a primitive from the promise \
             (or promising a hidden one) is a deliberate act; update this pin in the same PR"
        );
    }

    #[test]
    fn every_entry_has_a_non_empty_name_and_summary() {
        for info in PRIMITIVE_REGISTRY {
            assert!(!info.name.is_empty(), "empty name in registry");
            assert!(
                !info.summary.is_empty(),
                "primitive `{}` has an empty summary — scaffold + listings would be unhelpful",
                info.name
            );
            assert!(
                !info.since.is_empty(),
                "primitive `{}` has an empty since — corpus frontmatter would be invalid",
                info.name
            );
        }
    }

    #[test]
    fn every_entry_has_a_valid_category() {
        for info in PRIMITIVE_REGISTRY {
            assert!(
                VALID_CATEGORIES.contains(&info.category),
                "primitive `{}` has invalid category `{}` — valid: {VALID_CATEGORIES:?}",
                info.name, info.category
            );
        }
    }

    #[test]
    fn primitive_names_are_unique() {
        let mut seen: HashSet<&str> = HashSet::new();
        for info in PRIMITIVE_REGISTRY {
            assert!(
                seen.insert(info.name),
                "duplicate primitive name in registry: `{}`",
                info.name
            );
        }
    }

    #[test]
    fn documented_tier_matches_phase_6d_baseline_full_coverage() {
        // §Fase 6.d baseline (post-Tier-3): **45 primitives — every
        // entry in the registry is Documented (100% coverage)**.
        //
        //   Tier 0 (Fase 5,    7): persona, flow, step, anchor, tool, reason, socket
        //   Tier 1 (Fase 6.b, 10): context, intent, memory, agent, probe,
        //                          validate, refine, weave, type, run
        //   Tier 2 (Fase 6.c, 12): resource, fabric, manifest, observe,
        //                          reconcile, lease, ensemble, session,
        //                          axonstore, dataspace, corpus, pix
        //   Tier 3 (Fase 6.d, 16): axonendpoint, axpoint, daemon, mcp,
        //                          listen, shield, mandate, compute,
        //                          lambda, forge, ots, psyche, immune,
        //                          reflex, heal, transact
        //
        // Tier 2 ships 12 (not 13) — `taint` removed in 6.c (no
        // parser production). Tier 3 ships 16 (not 17) — `logic`
        // removed in 6.d (same reason). The discipline holds: every
        // entry in the registry has a `.md` AND a parser production.
        // A regression that flips a Documented entry back to Pending
        // surfaces here.
        let documented: HashSet<&str> = with_status(DocStatus::Documented)
            .map(|i| i.name)
            .collect();
        let expected: HashSet<&str> = [
            // Tier 0
            "persona", "flow", "step", "anchor", "tool", "reason", "socket",
            // Tier 1
            "context", "intent", "memory", "agent", "probe", "validate",
            "refine", "weave", "type", "run",
            // Tier 2
            "resource", "fabric", "manifest", "observe", "reconcile",
            "lease", "ensemble", "session", "axonstore", "dataspace",
            "corpus", "pix", "ledger",
            // Tier 3
            "axonendpoint", "axpoint", "daemon", "mcp", "listen",
            "shield", "mandate", "compute", "lambda", "forge", "ots",
            "psyche", "immune", "reflex", "heal",
            // §Fase 51 (v2.19.0)
            "observable", "quant",
            // §Fase 71 — the temporal execution-window guard.
            "window",
            // §Fase 73 — the open semi-structured value type.
            "json",
            // §Fase 77 — the π-calc channel quartet (Kivi brief #51 §B.2).
            "channel", "emit", "publish", "discover",
            // §Fase 80.b/80.g — the outbound vendor connection + the
            // inspectable voice-agent sugar.
            "upstream", "voice",
            // §Fase 83 — the named, referenced browser-origin policy.
            "cors",
            // §Fase 85 — the named, referenced result-memoization policy.
            "cache",
            // §Fase 99 — Native Document Synthesis (DOCX/PPTX/XLSX egress).
            "document",
            // §Fase 105 — Governed CRM Delivery (the egress-dual of acquisition).
            "deliver",
            // §Fase 87 — the long-horizon autonomous research primitive + its
            // dynamic tool-synthesis policy.
            "savant", "synth",
            // §Fase 88 — the adversarial security-analysis block + its
            // authorization-scope policy.
            "warden", "scope",
            // §Fase 92 — the ephemeral-credential contract + its minting verb.
            "credential", "mint",
            // §Fase 94 — the mediated secret-renewal verb.
            "rotate",
            // §Fase 114.z census + docs-tier — the 25 backfilled entries,
            // each documented against the runtime audit (not aspiration).
            "budget", "notify", "topology", "component", "view",
            "witness", "extension",
            "know", "believe", "speculate", "doubt",
            "par", "hibernate", "deliberate", "consensus", "stream",
            "grad", "navigate", "drill", "trail",
            "ingest", "focus", "associate", "aggregate", "explore",
            // §Fase 120 — algebraic effects, reachable from source at last.
            "effect", "handle", "perform",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            documented, expected,
            "Documented set drift — Fase 6.d baseline + §51 quant/observable + §71 window (full coverage)"
        );
    }

    #[test]
    fn coverage_summary_is_arithmetic() {
        let s = coverage_summary();
        // §Fase 62.0: 45 → 46 with `ledger` (audit chain) split out from `pix`.
        // §Fase 51 (v2.19.0): 46 → 48 with `observable` + `quant`.
        // §Fase 71: 48 → 49 with `window` (the temporal execution guard).
        // §Fase 73: 49 → 50 with `json` (the open semi-structured value type).
        // §Fase 77: 50 → 54 with the π-calc channel quartet
        // (`channel` / `emit` / `publish` / `discover`).
        // §Fase 80.b: 54 → 55 with `upstream`; §80.g: 55 → 56 with `voice`.
        // §Fase 83: 56 → 57 with `cors`.
        // §Fase 85: 57 → 58 with `cache`.
        // §Fase 87: 58 → 60 with `savant` + `synth`.
        // §Fase 88: 60 → 62 with `warden` + `scope`.
        // §Fase 92: 62 → 64 with `credential` + `mint`.
        // §Fase 94: 64 → 65 with `rotate` (the mediated secret-renewal verb).
        // §Fase 99: 65 → 66 with `document` (Native Document Synthesis).
        // §Fase 105: 66 → 67 with `deliver` (Governed CRM Delivery).
        // §Fase 111: 67 → 66 — `transact` retracted (axon-T938).
        // §Fase 114.z: 66 → 91 — the census backfill (entered Pending).
        // The §114.z docs-tier then paid the debt: all 25 census docs
        // landed (verified against the runtime audit), restoring 100%
        // coverage at the NEW, honest denominator. A future drop is a
        // regression the gate catches.
        // §Fase 120: 91 → 94 with `effect` / `handle` / `perform` — the
        // algebraic-effect constructs. The §23 runtime existed and the language
        // had no grammar for it at all, so `EffectRuntime` was constructible
        // only from its own tests.
        assert_eq!(s.total, 94);
        assert_eq!(s.documented + s.pending, s.total);
        assert_eq!(s.documented, 94);
        assert_eq!(s.pending, 0);
    }

    #[test]
    fn find_resolves_documented_and_pending_entries() {
        assert_eq!(find("persona").map(|i| i.name), Some("persona"));
        assert_eq!(find("axonendpoint").map(|i| i.name), Some("axonendpoint"));
        assert!(find("does_not_exist").is_none());
    }

    #[test]
    fn by_category_filters_to_the_named_family() {
        let cog: Vec<&str> = by_category("cognition").map(|i| i.name).collect();
        assert!(cog.contains(&"persona"));
        assert!(cog.contains(&"flow"));
        assert!(!cog.contains(&"socket"), "socket is session_types, not cognition");
        assert!(!cog.contains(&"axonendpoint"), "axonendpoint is wire, not cognition");
    }

    #[test]
    fn nested_primitives_carry_top_level_false() {
        // The "lives only inside a parent" set — coverage gate cross-
        // checks against `axon://grammar/top_level` which is the
        // human-readable mirror of this same polarity.
        for nested in ["step", "reason", "probe", "validate", "refine", "weave",
                       "listen", "forge", "quant",
                       // §Fase 77 — the channel quartet's nested members.
                       "emit", "publish", "discover",
                       // §Fase 114.z census — flow-body cognitive verbs.
                       "par", "hibernate", "deliberate", "consensus", "stream",
                       "grad", "navigate", "drill", "trail", "ingest",
                       "focus", "associate", "aggregate", "explore",
                       // §Fase 120 — `handle` and `perform` live in a flow or
                       // step body; only `effect` is a top-level declaration.
                       "handle", "perform"] {
            let info = find(nested).expect("must be in registry");
            assert!(
                !info.top_level,
                "primitive `{}` should be nested (top_level: false)",
                info.name
            );
        }
    }
}
