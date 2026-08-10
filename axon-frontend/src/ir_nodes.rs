//! AXON IR node definitions — direct port of axon/compiler/ir_nodes.py.
//!
//! All nodes serialize to JSON matching the Python IR output format exactly.

#![allow(dead_code)]

use serde::Serialize;

// ── Program root ─────────────────────────────────────────────────────────────

/// §Fase 112.b — `Clone` is additive and every member IR type already derives it.
/// The Cognitive-I/O supervisor owns the compiled program it drives (it outlives
/// the deploy call that built it), and a lifetime-bound supervisor would have to be
/// threaded through `ServerState` for no benefit.
#[derive(Debug, Clone, Serialize)]
pub struct IRProgram {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub personas: Vec<IRPersona>,
    pub contexts: Vec<IRContext>,
    pub anchors: Vec<IRAnchor>,
    pub tools: Vec<IRToolSpec>,
    pub memories: Vec<IRMemory>,
    pub types: Vec<IRType>,
    pub flows: Vec<IRFlow>,
    pub runs: Vec<IRRun>,
    pub imports: Vec<IRImport>,
    pub agents: Vec<IRAgent>,
    pub shields: Vec<IRShield>,
    /// §Fase 71.a — temporal execution-window guards.
    pub windows: Vec<IRWindow>,
    /// §Fase 114.a — top-level `budget` declarations. A daemon's anonymous budget
    /// stays on the daemon; these govern EVERY flow that calls the tools they
    /// name, including the HTTP endpoints adopters actually deploy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub budgets: Vec<IRBudget>,
    pub daemons: Vec<IRDaemon>,
    pub ots_specs: Vec<IROts>,
    pub pix_specs: Vec<IRPix>,
    /// §Fase 62.0 — audit-chain (`ledger`) declarations. Distinct from
    /// `pix_specs` (the retrieval navigator); a ledger binds a hash-linked
    /// recorder to an audited surface.
    pub ledger_specs: Vec<IRLedger>,
    pub corpus_specs: Vec<IRCorpus>,
    pub psyche_specs: Vec<IRPsyche>,
    pub mandate_specs: Vec<IRMandate>,
    pub lambda_data_specs: Vec<IRLambdaData>,
    pub compute_specs: Vec<IRCompute>,
    pub axonstore_specs: Vec<IRAxonStore>,
    pub endpoints: Vec<IRAxonEndpoint>,
    /// §Fase 53 — closed-catalog extension declarations (compiled).
    /// `#[serde(skip)]` so the field is NOT emitted into the IR JSON —
    /// this keeps the static IR-JSON drift-gate fixtures green without
    /// regenerating them (the pattern `dataspace_specs` also used until
    /// §108.b un-skipped it).
    /// The in-memory field feeds the §53.c type-checker + §53.d PCC (both
    /// read `&IRProgram`); soundness invariant #1 holds via SOURCE
    /// re-derivation — both the prover and the verifier read the
    /// source-derived IR, which carries the extensions. §53.x hardening
    /// (optional): un-skip + regenerate fixtures + bind extensions into
    /// the PCC `artifact_digest` (today the digest omits them; the
    /// witness still binds them by re-derivation). Deterministically
    /// sorted by `name` at the end of IR generation (§53.b founder
    /// refinement B) so multi-file declaration order can never perturb
    /// the proof-bundle hash.
    #[serde(skip)]
    pub extensions: Vec<IRExtension>,
    /// §Fase 108.b — the compiled dataspace schemas, SERIALIZED into the
    /// IR JSON (un-skipped). History: this field was `#[serde(skip)]` for
    /// byte-identical parity with the retired Python reference frontend
    /// (§8.2.h.1) — which meant the runtime literally could not see a
    /// declared dataspace (the §108 ground-truth finding). The parity
    /// constraint is gone; the deploy hook walks this field to
    /// instantiate the deterministic columnar engine's stores. Additive
    /// for consumers: no IR deserializer uses `deny_unknown_fields`
    /// (verified 2026-07-12), and `IRProgram` is `Serialize`-only —
    /// consumers re-derive from source.
    pub dataspace_specs: Vec<IRDataspace>,
    /// §λ-L-E Fase 1 — I/O cognitivo primitives (compiled).
    pub resources: Vec<IRResource>,
    pub fabrics: Vec<IRFabric>,
    pub manifests: Vec<IRManifest>,
    pub observations: Vec<IRObserve>,
    /// §λ-L-E Fase 1 (Free Monad root) — populated when the program
    /// declares manifests/observes. `None` ⇒ serialises as `null`
    /// (matches Python when the field is `None`).
    pub intention_tree: Option<IRIntentionTree>,
    /// §λ-L-E Fase 3 — Control cognitivo primitives (compiled).
    pub reconciles: Vec<IRReconcile>,
    pub leases: Vec<IRLease>,
    pub ensembles: Vec<IREnsemble>,
    /// §λ-L-E Fase 4 — Topology + Session (compiled).
    pub sessions: Vec<IRSession>,
    pub topologies: Vec<IRTopology>,
    /// §λ-L-E Fase 5 — Immune system (compiled).
    pub immunes: Vec<IRImmune>,
    pub reflexes: Vec<IRReflex>,
    pub heals: Vec<IRHeal>,
    /// §λ-L-E Fase 9 — UI cognitiva declarativa (compiled).
    pub components: Vec<IRComponent>,
    pub views: Vec<IRView>,
    /// §λ-L-E Fase 13 — Mobile typed channels (compiled).
    pub channels: Vec<IRChannel>,
    /// §Fase 41.b — typed WebSocket transports (compiled). Each carries its
    /// referenced `session` protocol + the credit-window backpressure so
    /// axon-rs can realise the typed endpoint over a `tokio` WebSocket.
    pub sockets: Vec<IRSocket>,
    /// §Fase 51.c.2 — Pauli-sum observable declarations (compiled). Each carries
    /// its real-coefficient × Pauli-string terms so axon-rs can build the
    /// Hermitian measurement operator `M = Σ cₖ Pₖ` a `quant` block measures
    /// against. `#[serde(skip)]` (like `extensions` / `dataspace_specs`) so the
    /// static IR-JSON drift fixtures stay green; the in-memory field feeds the
    /// §51.c.2 checker + the §51.d/e runtime. The checker resolves
    /// `quant(observable: …)` against the AST symbol table, not this field.
    #[serde(skip)]
    pub observables: Vec<IRObservable>,
    /// §Fase 69.a — Advantage-Witness declarations. `skip_serializing_if = empty`
    /// keeps a witness-less program's IR JSON byte-identical (zero IR-SHA drift,
    /// the §52/§67 pattern); when present it rides the IR to the enterprise
    /// deploy/runtime evaluator (§69.b+).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub witnesses: Vec<IRWitness>,
    /// §Fase 80.b — outbound vendor connections (compiled). Each carries its
    /// axon-facing session binding (`protocol`/`role`), the per-tenant config
    /// keys (`resolve`/`secret`), the auth handshake, the total wire↔session
    /// projection (`map`) and the reconnect/overflow policies, so axon-rs can
    /// dial + transcode without vendor-specific code. `skip_serializing_if =
    /// empty` keeps an upstream-less program's IR JSON byte-identical (zero
    /// IR-SHA drift — the standing §76.d discipline).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstreams: Vec<IRUpstream>,
    /// §Fase 83.a — named, referenced browser-origin policies. `skip_serializing_if
    /// = empty` keeps a cors-less program's IR JSON byte-identical (zero IR-SHA
    /// drift — the standing §76.d discipline).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cors_policies: Vec<IRCors>,
    /// §Fase 85.b — named, referenced result-memoization policies. Same
    /// `skip_serializing_if = empty` IR-SHA discipline as `cors_policies`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caches: Vec<IRCache>,
    /// §Fase 92.a — ephemeral-credential contracts (`credential { ttl:
    /// grants: }`), minted at runtime by the `mint` flow verb under the
    /// attenuation law (`authority_only_attenuates`). Same
    /// `skip_serializing_if = empty` IR-SHA discipline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<IRCredential>,
    /// §Fase 87.a — long-horizon autonomous research primitives (compiled). Each
    /// carries its domain, cognition params, memory binding, compute budget and
    /// mandates so the enterprise engine (§87.h+) can drive the active-inference
    /// loop. Same `skip_serializing_if = empty` IR-SHA discipline as `caches`
    /// (a savant-less program's IR JSON stays byte-identical — zero drift).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub savants: Vec<IRSavant>,
    /// §Fase 99.b — compiled document declarations. Same
    /// `skip_serializing_if = empty` IR-SHA discipline (a document-less
    /// program's IR JSON stays byte-identical). Consumed by the runtime
    /// `DocumentRenderer` tool + the `DocumentProvenanceSoundness` PCC class.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<IRDocument>,
    /// §Fase 105 — compiled CRM delivery declarations. Same
    /// `skip_serializing_if = empty` IR-SHA discipline (a delivery-less program's
    /// IR JSON stays byte-identical). Consumed by the runtime delivery dispatch
    /// (`axon::delivery`) + the `DeliveryProvenanceSoundness` PCC class (T920).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deliveries: Vec<IRDeliver>,
    /// §Fase 110 — governed human notifications (the third egress dual).
    pub notifications: Vec<IRNotify>,
    /// §Fase 87.d — dynamic tool-synthesis policies (compiled). Same
    /// `skip_serializing_if = empty` IR-SHA discipline as `savants`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synths: Vec<IRSynth>,
    /// §Fase 88.a — authorization-scope policies (compiled). Same
    /// `skip_serializing_if = empty` IR-SHA discipline as `synths`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<IRScope>,
    /// §Fase 23 — algebraic effect declarations (compiled).
    /// Each declared effect persists into IR so axon-rs can build the
    /// per-effect operation table at startup. The CPS state graph for
    /// perform/handle sites lives inline within IRFlow.steps (each
    /// IRPerform / IRHandlerFrame carries its assigned state_id /
    /// frame_id).
    ///
    /// This Rust port mirrors the Python-side
    /// `IRProgram.effects: tuple[IREffectDeclaration, ...]` field so
    /// the byte-identical structural-parity gate stays green. The Rust
    /// frontend (axon-frontend) does not yet emit Fase 23 IR itself —
    /// the field exists to preserve serialization shape; the actual
    /// algebraic-effects compiler lives on the Python side, and the
    /// Rust runtime (axon-rs/src/effects/) consumes the JSON IR
    /// emitted by Python.
    pub effects: Vec<IREffectDeclaration>,
    /// §Fase 115.e — per-module provenance of a LINKED program: for every
    /// module the linker merged, its path, origin file, both EMS hashes and
    /// the virtual-line window its declarations occupy (the driver renumbers
    /// each module's lines by a base offset so diagnostics and IR
    /// `source_line`s stay globally unambiguous — map back with
    /// `line − line_base`). `skip_serializing_if = empty` keeps every
    /// single-file program's IR JSON byte-identical (zero IR-SHA drift —
    /// the standing §76.d discipline).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<IRModuleProvenance>,
}

/// §Fase 115.e — one linked module's provenance record: the audit chain
/// from a deployed multi-module artifact back to its sources.
#[derive(Debug, Serialize, Clone)]
pub struct IRModuleProvenance {
    /// Dotted module path (`axon.security`).
    pub module: String,
    /// Display origin (file path or bundle key).
    pub origin: String,
    /// SHA-256 of the module's source bytes.
    pub content_hash: String,
    /// SHA-256 of the module's `.axi` interface (comment-stable).
    pub interface_hash: String,
    /// First virtual line assigned to this module by the link renumbering.
    pub line_base: u32,
    /// Number of source lines the module occupies.
    pub line_count: u32,
    /// The module's exported declaration names (deterministic order).
    pub declarations: Vec<String>,
}

impl IRProgram {
    pub fn new() -> Self {
        IRProgram {
            node_type: "program",
            source_line: 1,
            source_column: 1,
            personas: Vec::new(),
            contexts: Vec::new(),
            anchors: Vec::new(),
            tools: Vec::new(),
            memories: Vec::new(),
            types: Vec::new(),
            flows: Vec::new(),
            runs: Vec::new(),
            imports: Vec::new(),
            agents: Vec::new(),
            shields: Vec::new(),
            windows: Vec::new(),
            budgets: Vec::new(),
            daemons: Vec::new(),
            ots_specs: Vec::new(),
            pix_specs: Vec::new(),
            ledger_specs: Vec::new(),
            corpus_specs: Vec::new(),
            psyche_specs: Vec::new(),
            mandate_specs: Vec::new(),
            lambda_data_specs: Vec::new(),
            compute_specs: Vec::new(),
            axonstore_specs: Vec::new(),
            endpoints: Vec::new(),
            extensions: Vec::new(),
            dataspace_specs: Vec::new(),
            resources: Vec::new(),
            fabrics: Vec::new(),
            manifests: Vec::new(),
            observations: Vec::new(),
            intention_tree: None,
            reconciles: Vec::new(),
            leases: Vec::new(),
            ensembles: Vec::new(),
            sessions: Vec::new(),
            topologies: Vec::new(),
            immunes: Vec::new(),
            reflexes: Vec::new(),
            heals: Vec::new(),
            components: Vec::new(),
            views: Vec::new(),
            channels: Vec::new(),
            sockets: Vec::new(),
            observables: Vec::new(),
            witnesses: Vec::new(),
            upstreams: Vec::new(),
            cors_policies: Vec::new(),
            caches: Vec::new(),
            credentials: Vec::new(),
            savants: Vec::new(),
            documents: Vec::new(),
            deliveries: Vec::new(),
            notifications: Vec::new(),
            synths: Vec::new(),
            scopes: Vec::new(),
            effects: Vec::new(),
            modules: Vec::new(),
        }
    }
}

/// §Fase 51.d.2 — IR for the `yield <expr>` measurement point.
#[derive(Debug, Clone, Serialize)]
pub struct IRYield {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub value_expr: String,
    pub value_kind: String,
}

/// §Fase 51.c.2 — one term `cₖ · Pₖ` of a Pauli-sum observable (compiled).
#[derive(Debug, Clone, Serialize)]
pub struct IRPauliTerm {
    pub coefficient: f64,
    pub pauli: String,
}

/// §Fase 51.c.2 — IR for a Pauli-sum observable `M = Σ cₖ Pₖ`.
#[derive(Debug, Clone, Serialize)]
pub struct IRObservable {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qubits: Option<i64>,
    pub terms: Vec<IRPauliTerm>,
}

/// §Fase 69.a — IR for an Advantage Witness. The deploy/runtime evaluator reads
/// `metric` + `threshold` + `baseline`, computes the metric over `data`, and
/// emits the verdict; a `holds == false` verdict is the honest fail-closed
/// signal (`axon-W007`/`W008`). `claim`/`data` are references resolved per domain.
#[derive(Debug, Clone, Serialize)]
pub struct IRWitness {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub claim: String,
    pub baseline: String,
    pub metric: String,
    pub threshold: f64,
    pub data: String,
}

// ── §Fase 23 — Algebraic effect declarations ─────────────────────────────────
//
// Mirror of Python's `IREffectDeclaration` and `IREffectOperation`
// dataclasses. The Rust frontend (axon-frontend) does not yet emit
// Fase 23 IR itself — these structs exist so the byte-identical
// structural-parity gate stays green when Python emits an empty
// `effects: []` field. The actual algebraic-effects compiler lives on
// the Python side; the Rust runtime (axon-rs/src/effects/) consumes
// the JSON IR via its own deserialize structs.

#[derive(Debug, Serialize, Default, Clone)]
pub struct IREffectDeclaration {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub operations: Vec<IREffectOperation>,
}

impl IREffectDeclaration {
    pub fn new() -> Self {
        Self {
            node_type: "effect_declaration",
            source_line: 0,
            source_column: 0,
            name: String::new(),
            operations: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct IREffectOperation {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub type_parameters: Vec<String>,
    pub parameter_names: Vec<String>,
    pub parameter_types: Vec<String>,
    pub return_type: String,
}

impl IREffectOperation {
    pub fn new() -> Self {
        Self {
            node_type: "effect_operation",
            source_line: 0,
            source_column: 0,
            name: String::new(),
            type_parameters: Vec::new(),
            parameter_names: Vec::new(),
            parameter_types: Vec::new(),
            return_type: String::new(),
        }
    }
}

// ── §λ-L-E Fase 1 — IRResource ──────────────────────────────────────────────

/// Compiled resource declaration — linear/affine infrastructure token.
///
/// Python counterpart: `axon.compiler.ir_nodes.IRResource`.
#[derive(Debug, Clone, Serialize)]
pub struct IRResource {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    /// §Fase 113 — the pool size. Until §113 this was **read by nothing**: every
    /// `postgresql` axonstore in existence got a hardcoded
    /// `MAX_POOL_CONNECTIONS = 10` (`store/postgres_backend.rs`), with no env
    /// var and no source-level knob. `capacity:` is that missing knob, and
    /// wiring it is what makes `resource` a WIRE rather than a LABEL.
    pub capacity: Option<i64>,
    /// §Fase 113 — **how many holders may name this resource** (Linear Logic).
    ///
    /// Not "how long the connection lives" — that is `idle_timeout`, an
    /// operational knob. The Linear-Logic reading is about *sharing*:
    ///
    /// - `linear` — **exactly one** holder, and failing to name it is itself a
    ///   breach (a linear resource must be consumed).
    /// - `affine` — **at most one** holder. It may go unused; **sharing it is a
    ///   breach**.
    /// - `persistent` — the `!` exponential. Freely shared.
    ///
    /// Before §113, two stores shared a connection pool by **accidental DSN
    /// collision** (the registry keys its pool cache on the resolved DSN).
    /// Sharing is now *declared*, and `axon-T945` checks it.
    pub lifetime: String,
    pub certainty_floor: Option<f64>, // c ∈ [0.0, 1.0]
    pub shield_ref: String,
    /// §Fase 113 — the `fabric` this resource lives in (`within: Prod`).
    ///
    /// **One field, therefore Separation-Logic disjointness is UNREPRESENTABLE
    /// rather than verified**: a resource cannot be in two fabrics because
    /// there is no syntax for it. A checked invariant is what you settle for
    /// when you could not make the bad state unwritable; here we could.
    ///
    /// Empty ⇒ no fabric declared. Skip-if-empty ⇒ every pre-§113 program
    /// serializes byte-identically (IR-SHA stability, the §94.a `class`
    /// precedent).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub within: String,
}

impl IRResource {
    pub fn new(name: String, line: u32, column: u32) -> Self {
        IRResource {
            node_type: "resource",
            source_line: line,
            source_column: column,
            name,
            kind: String::new(),
            endpoint: String::new(),
            capacity: None,
            lifetime: "affine".to_string(),
            certainty_floor: None,
            shield_ref: String::new(),
            within: String::new(),
        }
    }
}

// ── §λ-L-E Fase 1 — IRFabric ────────────────────────────────────────────────

/// Compiled fabric declaration — topological substrate for resources.
#[derive(Debug, Clone, Serialize)]
pub struct IRFabric {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub provider: String,
    pub region: String,
    pub zones: Option<i64>,
    pub ephemeral: Option<bool>,
    pub shield_ref: String,
}

// ── §λ-L-E Fase 1 — IRManifest ──────────────────────────────────────────────

/// Compiled manifest declaration — declarative belief about desired shape.
#[derive(Debug, Clone, Serialize)]
pub struct IRManifest {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub resources: Vec<String>,
    pub fabric_ref: String,
    pub region: String,
    pub zones: Option<i64>,
    pub compliance: Vec<String>,
}

// ── §λ-L-E Fase 1 — IRObserve ───────────────────────────────────────────────

// ── §λ-L-E Fase 1 — IRIntentionTree (Free Monad root) ──────────────────────

/// A single operation node in the intention tree.
///
/// Operations are heterogeneous IR nodes (manifests, observes) that the
/// Handler layer (Fase 2) interprets via CPS. The enum is `#[serde(untagged)]`
/// so JSON output is just the inner struct — matching Python's `asdict`
/// behaviour on a polymorphic `tuple[IRNode, ...]`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum IRIntentionOperation {
    Manifest(IRManifest),
    Observe(IRObserve),
}

/// The Free Monad F_Σ(X) — a pure description of I/O intentions. Flat in
/// Fase 1; nested continuations arrive with handlers + reconcile loops.
#[derive(Debug, Clone, Serialize)]
pub struct IRIntentionTree {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub operations: Vec<IRIntentionOperation>,
}

/// Compiled observe declaration — quorum-gated observation with lag τ.
#[derive(Debug, Clone, Serialize)]
pub struct IRObserve {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub target: String,
    pub sources: Vec<String>,
    pub quorum: Option<i64>,
    pub timeout: String,
    pub on_partition: String,
    pub certainty_floor: Option<f64>,
}

// ── §λ-L-E Fase 3 — IRReconcile / IRLease / IREnsemble ──────────────────────

/// Compiled reconcile declaration — free-energy minimizing control loop.
#[derive(Debug, Clone, Serialize)]
pub struct IRReconcile {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub observe_ref: String,
    pub threshold: Option<f64>,
    pub tolerance: Option<f64>,
    pub on_drift: String,
    pub shield_ref: String,
    pub mandate_ref: String,
    pub max_retries: i64,
}

/// Compiled lease declaration — τ-decaying affine resource token.
#[derive(Debug, Clone, Serialize)]
pub struct IRLease {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub resource_ref: String,
    pub duration: String,
    pub acquire: String,
    pub on_expire: String,
}

/// Compiled ensemble declaration — Byzantine quorum aggregator.
#[derive(Debug, Clone, Serialize)]
pub struct IREnsemble {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub observations: Vec<String>,
    pub quorum: Option<i64>,
    pub aggregation: String,
    pub certainty_mode: String,
}

// ── §λ-L-E Fase 4 — IRSession / IRTopology ──────────────────────────────────

/// One operation in a compiled session protocol
/// (send / receive / loop / end / select / branch — §Fase 41.b adds the choices).
#[derive(Debug, Clone, Serialize)]
pub struct IRSessionStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub op: String,
    pub message_type: String,
    /// §Fase 41.b — labelled branches (only for `op == "select" | "branch"`;
    /// §Fase 79.b reuses them for `op == "interrupt"`: `body` + `handler` arms).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub branches: Vec<IRSessionBranch>,
    /// §Fase 79.b — `op == "interrupt"` only: the handler's signal binder
    /// (`... as <sig> ...`). Skip-if-empty ⇒ zero IR-SHA drift for every
    /// non-interrupt step (the §76.d/§77.a additive-only discipline).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub binder: String,
    /// §Fase 79.b — `op == "interrupt"` only: the block declares a `resumable`
    /// handler. Skip-if-false ⇒ byte-identical IR for every other op.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub resumable: bool,
}

/// §Fase 41.b — one labelled arm of a compiled `select`/`branch` choice.
#[derive(Debug, Clone, Serialize)]
pub struct IRSessionBranch {
    pub node_type: &'static str,
    pub label: String,
    pub steps: Vec<IRSessionStep>,
}

/// A role's name and its ordered protocol steps.
#[derive(Debug, Clone, Serialize)]
pub struct IRSessionRole {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub steps: Vec<IRSessionStep>,
}

/// Compiled binary session — exactly two dual roles (verified at type-check).
#[derive(Debug, Clone, Serialize)]
pub struct IRSession {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub roles: Vec<IRSessionRole>,
}

/// Directed, session-typed edge between two topology nodes.
#[derive(Debug, Clone, Serialize)]
pub struct IRTopologyEdge {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub source: String,
    pub target: String,
    pub session_ref: String,
}

/// Compiled topology — typed graph over Axon entities.
#[derive(Debug, Clone, Serialize)]
pub struct IRTopology {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub nodes: Vec<String>,
    pub edges: Vec<IRTopologyEdge>,
}

// ── §λ-L-E Fase 5 — IRImmune / IRReflex / IRHeal ────────────────────────────

/// Compiled immune sensor — KL+FEP anomaly detector descriptor.
#[derive(Debug, Clone, Serialize)]
pub struct IRImmune {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub watch: Vec<String>,
    pub sensitivity: Option<f64>,
    pub baseline: String,
    pub window: i64,
    pub scope: String,
    pub tau: String,
    pub decay: String,
}

/// Compiled reflex — deterministic O(1) motor response descriptor.
#[derive(Debug, Clone, Serialize)]
pub struct IRReflex {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub trigger: String,
    pub on_level: String,
    pub action: String,
    pub scope: String,
    pub sla: String,
}

/// Compiled heal — Linear-Logic one-shot patch kernel descriptor.
#[derive(Debug, Clone, Serialize)]
pub struct IRHeal {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub source: String,
    pub on_level: String,
    pub mode: String,
    pub scope: String,
    pub review_sla: String,
    pub shield_ref: String,
    pub max_patches: i64,
}

// ── §λ-L-E Fase 9 — IRComponent / IRView ────────────────────────────────────

/// Compiled UI component — reusable fragment over a typed data source.
#[derive(Debug, Clone, Serialize)]
pub struct IRComponent {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub renders: String,
    pub via_shield: String,
    pub on_interact: String,
    pub render_hint: String,
}

/// Compiled UI view — top-level screen composing declared components.
#[derive(Debug, Clone, Serialize)]
pub struct IRView {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub title: String,
    pub components: Vec<String>,
    pub route: String,
}

// ── Import ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct IRImport {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub module_path: Vec<String>,
    pub names: Vec<String>,
    /// §Fase 115.e — `true` iff the EMS resolved this import against a
    /// module in the compilation (the fields this paper-era struct always
    /// promised). Skipped when `false` so every pre-§115 program's IR
    /// JSON stays byte-identical (zero IR-SHA drift).
    #[serde(default, skip_serializing_if = "ir_import_unresolved")]
    pub resolved: bool,
    /// §Fase 115.e — the resolved module's `.axi` interface hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_hash: Option<String>,
}

/// Serde helper: skip `resolved` while it is `false`.
fn ir_import_unresolved(resolved: &bool) -> bool {
    !*resolved
}

// ── Persona ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRPersona {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub domain: Vec<String>,
    pub tone: String,
    pub confidence_threshold: Option<f64>,
    pub cite_sources: Option<bool>,
    pub refuse_if: Vec<String>,
    pub language: String,
    pub description: String,
}

// ── Context ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRContext {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub memory_scope: String,
    pub language: String,
    pub depth: String,
    pub max_tokens: Option<i64>,
    pub temperature: Option<f64>,
    pub cite_sources: Option<bool>,
    /// §Fase 91.a — the frame's declared cognitive timezone (IANA name).
    /// Elided when absent → pre-§91 context IR JSON stays byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_tz: Option<String>,
}

// ── Anchor ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRAnchor {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub description: String,
    pub require: String,
    pub reject: Vec<String>,
    pub enforce: String,
    pub confidence_floor: Option<f64>,
    pub unknown_response: String,
    pub on_violation: String,
    pub on_violation_target: String,
}

// ── Tool ─────────────────────────────────────────────────────────────────────

/// §Fase 58.c — one typed parameter of a tool's input schema (the IR mirror of
/// the AST `Parameter`). `type_name` is the flattened BASE type string
/// (`String`, `List<String>`); optionality (`T?`) is carried in `optional`, so
/// `required` is derivable with no parallel bool (§58 D1, single source of
/// truth). Lossless round-trip is gated in §58.i.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IRToolParam {
    pub name: String,
    pub type_name: String,
    pub optional: bool,
}

/// §Fase 58.c — one bound keyword argument of a `use Tool(k = v, …)` call (the
/// IR mirror of `UseArgs::Named`). `value` is an expression string (the
/// frontend has no structured `Expr`). The runtime (§58.e) assembles these into
/// the structured JSON request body.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IRNamedArg {
    pub name: String,
    pub value: String,
    /// §Fase 60 — `"literal"` or `"reference"` (classified by `parse_let_atom`).
    /// A `"reference"` value (a bare identifier or `Step.output`) is resolved at
    /// runtime against the bindings (flow-param / `let` / step output), like a
    /// `let` reference — instead of being passed as the literal name (the pre-60
    /// bug). `"literal"` values keep `${…}` interpolation + typed coercion.
    pub value_kind: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct IRToolSpec {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub provider: String,
    pub max_results: Option<i64>,
    pub filter_expr: String,
    pub timeout: String,
    pub runtime: String,
    /// §Fase 114.c — the `resource` this tool's channel runs on. Empty ⇒ legacy
    /// form. Skip-if-empty ⇒ every pre-§114 tool serializes byte-identically.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resource_ref: String,
    pub sandbox: Option<bool>,
    pub input_schema: Vec<String>,
    pub output_schema: String,
    /// §Fase 58.c — the tool's typed INPUT SCHEMA (D1). Distinct from the §32
    /// `input_schema`/`output_schema` validation hints (those say HOW to
    /// validate raw output: JSON/number/…); these are the caller↔tool TYPE
    /// contract the type-checker enforces (§58.d) and the runtime binds
    /// structured args against (§58.e). Empty for a schema-less tool (D5).
    pub parameters: Vec<IRToolParam>,
    /// §Fase 58.c — the tool's declared OUTPUT type (D8), so `${Step.output}`
    /// is typed. `None` when undeclared. Single source of truth (lives here,
    /// not denormalised onto each call site).
    pub output_type: Option<String>,
    /// §Fase 116.a (D116.9) — the authorization scopes this tool's operation
    /// requires: flat capability atoms, the `credential.grants` (§92) /
    /// `requires_capabilities` (§51.x) vocabulary. `axon-T956` enforces subset
    /// coverage at compile; the PCC `ScopeCoverage` witness rides the linked
    /// IR. Elided when empty — every pre-§116 tool serializes byte-identically
    /// (IR-SHA stability), and stored FlowIr hydrates via `default` (§49.f
    /// mirror discipline).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// §Fase 94.c — the per-tenant secret KEY injected into every dispatch
    /// under the reserved `axon_secret` request field (resolved against the
    /// tenant's custody at `use` time; the flow never touches the value).
    /// Elided when empty — every pre-§94 tool serializes byte-identically
    /// (IR-SHA stability).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
    /// §Fase 95.a — the `secret_partition:` parameter name whose runtime
    /// value is appended as a single segment to `secret` at dispatch
    /// (`selection_without_revelation`). Elided when empty, so every §94 and
    /// pre-§94 tool serializes byte-identically (IR-SHA stability). The
    /// class prefix lives in `secret`; this only names the dynamic segment
    /// source — no value ever rides the IR.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret_partition: String,
    pub effect_row: Vec<String>,
    /// §Fase 84.b — Remote Hands. All three fields are `skip_serializing_if`
    /// so a program using none of them serialises **byte-identically** to the
    /// pre-§84 IR (the §76.d IR-SHA / additive-only gate — no drift for the
    /// entire existing corpus).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    /// §Fase 85.b — the cache-policy reference (a declared `cache` name, or the
    /// `none` opt-out sentinel). Empty ⇒ module-default-governed. Elided when
    /// empty (IR-SHA stable for cache-less programs).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cache: String,
    /// §Fase 98.b — the closed-catalog web-acquisition config. `None` for
    /// every non-scrape tool, and `skip_serializing_if` so the entire
    /// pre-§98 corpus serialises byte-identically (the §76.d IR-SHA /
    /// additive-only gate). Present ⇒ this tool acquires open-web content
    /// (born Untrusted, D98.1) and its `effect_row` carries `web`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrape: Option<IRScrapeSpec>,
}

/// §Fase 98.b — the IR mirror of `ast::ScrapeSpec`. Every field is
/// `skip_serializing_if` on its empty/none form so a minimal `scrape: {}`
/// and each partially-populated block serialise deterministically with no
/// null noise, keeping the IR-SHA additive.
#[derive(Debug, Serialize, Clone)]
pub struct IRScrapeSpec {
    pub node_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impersonate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_wait: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respect_robots: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extract: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_floor: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub follow: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub politeness: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checkpoint: String,
}

// ── Memory ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct IRMemory {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub store: String,
    pub backend: String,
    pub retrieval: String,
    pub decay: String,
}

// ── Type ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRTypeField {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub type_name: String,
    pub generic_param: String,
    pub optional: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct IRType {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub fields: Vec<IRTypeField>,
    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
    pub where_expression: String,
    /// §ESK Fase 6.1 — κ regulatory class.
    pub compliance: Vec<String>,
}

// ── Flow ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRParameter {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub type_name: String,
    pub generic_param: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDataEdge {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub source_step: String,
    pub target_step: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub persona_ref: String,
    pub given: String,
    pub ask: String,
    pub use_tool: Option<serde_json::Value>,
    pub probe: Option<serde_json::Value>,
    pub reason: Option<serde_json::Value>,
    pub weave: Option<serde_json::Value>,
    pub output_type: String,
    pub confidence_floor: Option<f64>,
    pub navigate_ref: String,
    pub apply_ref: String,
    /// §Fase 68.b — the step's model-capability requirement (context window in
    /// tokens). `skip_serializing_if = Option::is_none` keeps every pre-§68 step's
    /// IR JSON byte-identical (no IR-SHA drift, D68.4); a legacy IR deserialises
    /// to `None` → the §68.c resolver picks the backend default exactly as today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_context: Option<u32>,
    /// §Fase 91.a — the step's declared cognitive timezone (IANA name). The
    /// runtime renders the run's captured instant in this zone into the step's
    /// cognitive context. Elided when absent → every pre-§91 step's IR JSON is
    /// byte-identical (no IR-SHA drift); legacy IR → `None` → no injection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_tz: Option<String>,
    /// §Fase 119 (D119.4) — governance applications scoped to this step.
    /// Elided when empty so every pre-§119 program's IR JSON stays
    /// byte-identical (no IR-SHA drift — the §68.b/§91.a discipline).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guards: Vec<IRStepGuard>,
    /// §Fase 119.f — the PIX verbs written as statements in this step's body
    /// (`navigate` / `drill` / `trail` / `validate`), in source order. They
    /// are ELEVATIONS: dispatch runs them before the step generates, so each
    /// `as:` binding is in scope for the step's `ask:`. Elided when empty so
    /// every pre-§119.f program's IR JSON is byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pix_ops: Vec<IRFlowNode>,
    /// §Fase 119.n — a `stream<T> { … }` written in this step's body. NOT a
    /// `pix_ops` entry: those are elevations that run BEFORE generation, while a
    /// stream handler runs DURING it and this step's output IS the stream.
    /// Elided when absent, so every pre-§119.n program's IR JSON is
    /// byte-identical and a legacy IR still deserialises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<Box<IRStreamBlock>>,
    pub body: Vec<serde_json::Value>,
}

/// §Fase 119 (D119.4) — one step-scoped governance application. For a
/// `mandate` guard, dispatch must run the §119.b control loop over THIS
/// step's generation, with the mandate's declared `(D, L)` obligations.
#[derive(Debug, Clone, Serialize)]
pub struct IRStepGuard {
    pub kind: String,
    pub name: String,
    pub target: String,
    pub binding: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRFlow {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub parameters: Vec<IRParameter>,
    pub return_type_name: String,
    pub return_type_generic: String,
    pub return_type_optional: bool,
    pub steps: Vec<IRFlowNode>,
    pub edges: Vec<IRDataEdge>,
    pub execution_levels: Vec<Vec<String>>,
}

// ── Run ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRRun {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub flow_name: String,
    pub arguments: Vec<String>,
    pub persona_name: String,
    pub context_name: String,
    pub anchor_names: Vec<String>,
    pub on_failure: String,
    pub on_failure_params: Vec<Vec<String>>,
    pub output_to: String,
    pub effort: String,
    pub resolved_flow: Option<IRFlow>,
    pub resolved_persona: Option<IRPersona>,
    pub resolved_context: Option<IRContext>,
    pub resolved_anchors: Vec<IRAnchor>,
}

// ── Lambda Data (ΛD) — Epistemic State Vectors ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRLambdaData {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub ontology: String,             // T — ontological type
    pub certainty: f64,               // c ∈ [0,1]
    pub temporal_frame_start: String, // τ_start
    pub temporal_frame_end: String,   // τ_end
    pub provenance: String,           // ρ — EntityRef origin
    pub derivation: String,           // δ ∈ Δ
}

#[derive(Debug, Clone, Serialize)]
pub struct IRLambdaDataApply {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub lambda_data_name: String, // reference to declared ΛD
    pub target: String,           // expression being bound
    pub output_type: String,      // result type after binding
}

// ── Flow step IR nodes ──────────────────────────────────────────────────────

/// Polymorphic flow body node — serializes via #[serde(untagged)] so each
/// variant emits its inner struct's JSON (with its own `node_type` field).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum IRFlowNode {
    Step(IRStep),
    Probe(IRProbe),
    Reason(IRReasonStep),
    Validate(IRValidateStep),
    Refine(IRRefineStep),
    Weave(IRWeaveStep),
    UseTool(IRUseToolStep),
    Remember(IRRememberStep),
    Recall(IRRecallStep),
    Conditional(IRConditional),
    ForIn(IRForIn),
    Let(IRLetBinding),
    Return(IRReturnStep),
    /// Fase 19.e — exit the enclosing for-in body. Payload-free;
    /// the runner translates it into a sentinel that terminates the
    /// loop. Parser scope check guarantees this only appears inside
    /// a for-in body.
    Break(IRBreakStep),
    /// Fase 19.e — skip to the next iteration of the enclosing for-in
    /// body. Same shape as Break — payload-free, sentinel-driven at
    /// runtime.
    Continue(IRContinueStep),
    LambdaDataApply(IRLambdaDataApply),
    Par(IRParallelBlock),
    Hibernate(IRHibernateStep),
    Deliberate(IRDeliberateBlock),
    Consensus(IRConsensusBlock),
    Forge(IRForgeBlock),
    /// §Fase 109 — the proof-carrying derivative step.
    Grad(IRGradStep),
    Focus(IRFocusStep),
    Associate(IRAssociateStep),
    Aggregate(IRAggregateStep),
    Explore(IRExploreStep),
    Ingest(IRIngestStep),
    ShieldApply(IRShieldApplyStep),
    Stream(IRStreamBlock),
    Navigate(IRNavigateStep),
    Drill(IRDrillStep),
    Trail(IRTrailStep),
    Corroborate(IRCorroborateStep),
    OtsApply(IROtsApplyStep),
    MandateApply(IRMandateApplyStep),
    ComputeApply(IRComputeApplyStep),
    /// §Fase 119.m.3 — `<Agent>(arg, …)`. The 46th variant, and the one that
    /// makes §119.m.1's bounded control loop reachable from source.
    AgentCall(IRAgentCall),
    Listen(IRListenStep),
    DaemonStep(IRDaemonStepNode),
    /// §λ-L-E Fase 13 — π-calc output prefix (Chan-Output / Chan-Mobility).
    Emit(IREmit),
    /// §Fase 92.b — ephemeral-credential minting (attenuated, TTL-bounded).
    Mint(IRMintStep),
    /// §Fase 94.b — mediated secret renewal (`rotation_without_revelation`).
    Rotate(IRRotateStep),
    /// §λ-L-E Fase 13 — capability extrusion (Publish-Ext).
    Publish(IRPublish),
    /// §λ-L-E Fase 13 — dual of publish (typed handle import).
    Discover(IRDiscover),
    Persist(IRPersistStep),
    Retrieve(IRRetrieveStep),
    Mutate(IRMutateStep),
    Purge(IRPurgeStep),
    Transact(IRTransactBlock),
    /// §Fase 88.a — the `warden` adversarial security-analysis block.
    Warden(IRWarden),
    /// §Fase 51.a — the `quant` cognitive block (Hilbert-space projection).
    Quant(IRQuant),
    /// §Fase 51.d.2 — the `yield` measurement point inside a `quant` block.
    Yield(IRYield),
    /// §Fase 52.c — `run <Flow>(args)` flow-step: invoke a declared flow from a
    /// body (a daemon listen handler). Reuses [`IRRun`] (the top-level run IR).
    Run(IRRun),
}

#[derive(Debug, Clone, Serialize)]
pub struct IRProbe {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRReasonStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub strategy: String,
    pub target: String,
    /// §Fase 119.f.8 — the evidence the deliberation reasons OVER, resolved
    /// against the flow bindings at dispatch. Elided when empty so every
    /// pre-§119.f.8 program's IR JSON is byte-identical (no IR-SHA drift).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given: String,
    /// §Fase 119.f.8 — the deliberation's question. The README writes it in
    /// every one of its sixteen `reason { … }` blocks and, before this fase, it
    /// reached the model in none of them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ask: String,
    /// §Fase 119.f.8 — the declared deliberation depth. Consumed by the
    /// dispatch FRAMING, like `strategy`; not a runtime iteration bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRValidateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
    pub rule: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRRefineStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRWeaveStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub sources: Vec<String>,
    pub target: String,
    pub format_type: String,
    pub priority: Vec<String>,
    pub style: String,
    /// §Fase 119.f.9 — the parts the synthesis must contain
    /// (`include: [summary, risks, recommendations]`). Elided when empty so
    /// every pre-§119.f.9 program's IR JSON is byte-identical (no IR-SHA drift).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRUseToolStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub tool_name: String,
    pub argument: String,
    /// §Fase 58.c — the bound keyword args of `use Tool(k = v, …)` (W1: the
    /// structured args survive to the IR, no longer collapsed to one opaque
    /// string). Empty for the legacy single-`on <arg>` form (`argument`
    /// carries that, D5).
    pub named_args: Vec<IRNamedArg>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRRememberStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub expression: String,
    pub memory_target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRRecallStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub query: String,
    pub memory_source: String,
}

/// §Fase 70.a — the lowered form of a pure expression (`Expr`). Carried in the
/// IR for conditions the legacy `(condition, op, value)` triple cannot express.
/// Operators are canonical lowercase strings so the JSON is stable + readable;
/// the runtime evaluator (§70.f) matches on them. Externally-tagged by `kind`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IRExpr {
    /// A typed literal.
    Lit { lit: IRExprLit },
    /// A reference to a binding / dotted path.
    Ref { path: String },
    /// Unary op — `op ∈ {neg, not}`.
    Unary { op: String, operand: Box<IRExpr> },
    /// Binary op — `op ∈ {add,sub,mul,div,mod,eq,ne,lt,le,gt,ge,and,or}`.
    Binary {
        op: String,
        lhs: Box<IRExpr>,
        rhs: Box<IRExpr>,
    },
    /// §Fase 70.c — a closed-catalog builtin call. `args[0]` is the receiver.
    /// `builtin ∈ {length,count,is_empty,is_null,contains,starts_with,ends_with}`.
    Call {
        builtin: String,
        args: Vec<IRExpr>,
    },
    /// §Fase 70.d — field access on a non-reference base (the JSONB seam).
    Field {
        base: Box<IRExpr>,
        field: String,
    },
    /// §Fase 70.d — index access `base[index]`.
    Index {
        base: Box<IRExpr>,
        index: Box<IRExpr>,
    },
    /// §Fase 119.o — `let <name> = <value>` scoped over `<body>`. The lowered
    /// form of a `logic { let … return … }` chain: one `Let` per binding,
    /// nested, so each bound term is evaluated exactly once and shadowing falls
    /// out of the nesting.
    Let {
        name: String,
        value: Box<IRExpr>,
        body: Box<IRExpr>,
    },
}

/// §Fase 70.a — a literal inside an [`IRExpr`].
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "ty", rename_all = "snake_case")]
pub enum IRExprLit {
    Int { value: i64 },
    Float { value: f64 },
    Bool { value: bool },
    Str { value: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct IRConditional {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub condition: String,
    pub comparison_op: String,
    pub comparison_value: String,
    pub then_body: Vec<IRFlowNode>,
    pub else_body: Vec<IRFlowNode>,
    pub conditions: Vec<(String, String, String)>,
    pub conjunctor: String,
    /// §Fase 70.a — the lowered expression form, present only for conditions
    /// the legacy triple cannot express. `skip_serializing_if` keeps the IR
    /// JSON (and its SHA) byte-identical for every pre-§70 program.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cond: Option<IRExpr>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRForIn {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub variable: String,
    pub iterable: String,
    pub body: Vec<IRFlowNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRLetBinding {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
    pub value: String,
    /// Fase 17.a — preserves parser tokenization intent.
    /// One of "literal" | "reference" | "expression".
    pub value_kind: String,
    /// §Fase 70.f — the lowered expression form of the value, present only for
    /// `value_kind == "expression"`. The runtime evaluates it instead of
    /// treating the value string as an opaque literal. `skip_serializing_if`
    /// keeps the IR byte-identical for every literal / reference let.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_ast: Option<IRExpr>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRReturnStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub value_expr: String,
}

/// Fase 19.e — `break` keyword IR node. Payload-free (the runner
/// raises a sentinel; no value is carried). Mirrors Python's
/// ``IRBreak`` (axon/compiler/ir_nodes.py).
#[derive(Debug, Clone, Serialize)]
pub struct IRBreakStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
}

/// Fase 19.e — `continue` keyword IR node. Same shape as
/// ``IRBreakStep``; the runner uses a different sentinel type to
/// distinguish loop-exit (break) from iteration-skip (continue).
#[derive(Debug, Clone, Serialize)]
pub struct IRContinueStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRParallelBlock {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    /// §Fase 65 — the concurrent branches lowered from the AST `par { … }`.
    /// Each branch is a flow-IR body run concurrently by the dispatcher's
    /// `run_branches_concurrently`. `skip_serializing_if = "Vec::is_empty"` so a
    /// payload-free / empty `par` serializes byte-identically to the pre-§65
    /// shape (D5 back-compat); a `par` with real branches carries them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<Vec<IRFlowNode>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRHibernateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub event_name: String,
    pub timeout: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDeliberateBlock {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRConsensusBlock {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
}

/// §Fase 86 — the compiled Directed Creative Synthesis block. This IS the
/// "structured IR metadata that the runtime executes as an orchestrated
/// pipeline" the README always claimed — pre-§86 it carried only a source
/// location. New fields are `skip_serializing_if`-elided so a program with no
/// `forge` stays IR-SHA stable.
#[derive(Debug, Clone, Serialize, Default)]
pub struct IRForgeBlock {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub seed: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "is_default_novelty")]
    pub novelty: f64,
    #[serde(default, skip_serializing_if = "is_one_i64")]
    pub depth: i64,
    #[serde(default, skip_serializing_if = "is_one_i64")]
    pub branches: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub constraints_ref: String,
}

fn is_default_novelty(v: &f64) -> bool {
    (*v - 0.5).abs() < f64::EPSILON
}
fn is_one_i64(v: &i64) -> bool {
    *v == 1
}

/// §Fase 109.a — the proof-carrying derivative. `original` is the
/// differentiated `let`'s expression; `derivatives[i]` = ∂original/∂wrt[i],
/// SIMPLIFIED (D109.4) — computed at compile time by the symbolic
/// differentiator and re-derived at deploy by PCC `GradientSoundness`.
/// `original: None` / empty `derivatives` only in a stale artifact — the
/// runtime fails CLOSED on it and the PCC refutes it.
#[derive(Debug, Clone, Serialize)]
pub struct IRGradStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    /// The prior rich `let` differentiated.
    pub target: String,
    pub wrt: Vec<String>,
    /// Result binding (empty ⇒ `d_<target>`).
    pub output: String,
    pub original: Option<IRExpr>,
    pub derivatives: Vec<IRExpr>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRFocusStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub expression: String,
    /// §Fase 108.d — the data-plane `where:` (D108.9; empty ⇒ no filter).
    pub where_expr: String,
    /// §Fase 108.d — π: projected columns (empty ⇒ all).
    pub select: Vec<String>,
    /// §Fase 108.d — result binding (`as:`; empty ⇒ the dataspace name).
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRAssociateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub left: String,
    pub right: String,
    pub using_field: String,
    /// §Fase 108.d — result binding (`as:`; empty ⇒ `<L>_<R>`).
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRAggregateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
    pub group_by: Vec<String>,
    pub alias: String,
    /// §Fase 108.d — the closed aggregate catalog entries, raw
    /// (`count`, `sum(score)`, …) — canonical spelling, T930-validated.
    pub compute: Vec<String>,
    /// §Fase 108.d — the data-plane `where:` (D108.9).
    pub where_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRExploreStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
    pub limit: Option<i64>,
    /// §Fase 108.d — result binding (`as:`; empty ⇒ the target).
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRIngestStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub source: String,
    pub target: String,
    /// §Fase 108.c — the declared wire format (`csv` | `json`, validated
    /// by axon-T929). Empty only in a pre-108.c artifact — the runtime
    /// handler fails CLOSED on it.
    pub format: String,
    /// §Fase 108.c — bounds enforced on the raw byte stream BEFORE
    /// parsing (§100). `None` ⇒ the engine's conservative defaults.
    pub max_bytes: Option<u64>,
    pub max_rows: Option<u64>,
}

/// §Fase 114.w — a shield's compiled BREACH POLICY, resolved onto the nodes
/// that enforce it (`IRShieldApplyStep` / `IREmit`) at LOWERING — the same
/// discipline as `IREmit.shield_ref`: the policy rides the artifact, so every
/// dispatch path honors it by construction (no per-ctx shield map a forgotten
/// site could miss). Before §114.w the whole `on_breach:` catalog
/// (`halt|sanitize_and_retry|escalate|quarantine|deflect`) was documented,
/// parsed, type-checked — and the runtime always `halt`ed.
#[derive(Debug, Clone, Serialize)]
pub struct IRBreachPolicy {
    /// The declared policy (validated against `VALID_ON_BREACH_POLICIES`).
    pub on_breach: String,
    /// The quarantine SINK name (`on_breach: quarantine` requires it, axon-T952).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quarantine: String,
    /// The canned safe reply (`on_breach: deflect` requires it, axon-T952).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub deflect_message: String,
    /// Fields masked by `sanitize_and_retry` (requires ≥ 1, axon-T952).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redact: Vec<String>,
    /// Re-scan budget for `sanitize_and_retry` (parser default: 3).
    pub max_retries: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRShieldApplyStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub shield_name: String,
    pub target: String,
    pub output_type: String,
    /// §Fase 114.w — the named shield's breach policy, resolved at lowering.
    /// `None` ⇒ the shield declares no `on_breach:` (halt, the fail-closed
    /// default) — and every pre-§114.w program serializes byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breach_policy: Option<IRBreachPolicy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRStreamBlock {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    /// §Fase 111.e — the block's lowered body. ADDITIVE: `skip_serializing_if`
    /// elides it when empty, so every pre-111 program's IR JSON stays
    /// byte-identical and a legacy IR deserialises to an empty body (which then
    /// executes as a no-op, exactly as before — no silent behaviour change for
    /// an artifact compiled by an older frontend).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<IRFlowNode>,
    /// §Fase 119.n — the `<T>` in `stream<T>`: the CHUNK type. Elided when the
    /// block was written without one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chunk_type: String,
    /// §Fase 119.n — `on_chunk: { … }`, lowered. Run once per chunk with the
    /// chunk bound as `chunk`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_chunk: Option<Box<IRStep>>,
    /// §Fase 119.n — `on_complete: { … }`, lowered. Run once, after the source
    /// closes, with the accumulated stream bound as `complete`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_complete: Option<Box<IRStep>>,
    /// §Fase 119.n.3 — `on_error: { … }`, lowered. Run when the SOURCE fails,
    /// with the failure bound as `error`. Never fires for a failure of the
    /// author's own handlers, nor for cancellation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_error: Option<Box<IRStep>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRNavigateStep {
    /// §Fase 119.f — per-navigation depth override → `NavConfig.d_max`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<i64>,
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub pix_ref: String,
    pub corpus_ref: String,
    pub query: String,
    pub trail_enabled: bool,
    pub output_name: String,
    /// §Fase 63.B — MDN corpus-graph navigation: the seed document (`from:`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub seed: String,
    /// §Fase 63.B — MDN navigation budget (`budget:` = max documents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<i64>,
    /// §Fase 66 (Q2) — column-scope filter for a `corpus from axonstore`. A raw
    /// filter expr threaded to `read_all_store_rows` → `stream_retrieve` for
    /// BOTH the documents and edges stores, so the sourced MDN graph is scoped
    /// to a sub-tenant column (`where: "tenant_id == '${tenant_id}'"`). The
    /// §37.d filter compiler resolves `${name}` → `$N` bind params (injection-
    /// safe). Empty = no column filter (axon-tenant RLS scope only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub where_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDrillStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub pix_ref: String,
    pub subtree_path: String,
    pub query: String,
    pub output_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRTrailStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub navigate_ref: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRCorroborateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub navigate_ref: String,
    pub output_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IROtsApplyStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub ots_name: String,
    pub target: String,
    pub output_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRMandateApplyStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub mandate_name: String,
    pub target: String,
    pub output_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRAgentCall {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    /// The declared `agent` this call invokes. Resolved against
    /// `DispatchCtx::agent_specs` at dispatch; an unresolved name fails CLOSED
    /// (the §111.f compute doctrine — an empty catalog runs nothing).
    pub agent_name: String,
    /// Positional §119.f.10 subjects. Resolved against the flow bindings at
    /// dispatch, so `TrendAnalyzer(Gather.output)` hands the agent the prior
    /// step's VALUE.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRComputeApplyStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub compute_name: String,
    pub arguments: Vec<String>,
    pub output_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRListenStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub channel: String,
    /// §λ-L-E Fase 13 D4 — true ⇒ `channel` is a declared
    /// `IRChannel` ref; false ⇒ legacy string topic.
    pub channel_is_ref: bool,
    pub event_alias: String,
    /// §Fase 52.a — the handler body's lowered flow-steps, executed per event /
    /// scheduled tick by the §52.c runtime. `skip_serializing_if` keeps a
    /// bodyless `listen`'s JSON byte-identical to the pre-§52.a shape (D8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<IRFlowNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDaemonStepNode {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub daemon_ref: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRPersistStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub store_name: String,
    /// §Fase 35.o — declared `{ col: value }` field block (value
    /// expressions kept raw; interpolated at runtime). Empty ⇒ the
    /// runtime writes the flow's user bindings (v1.30.0 fallback).
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRRetrieveStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub store_name: String,
    pub where_expr: String,
    pub alias: String,
    /// §Fase 67.b — `order_by:` clause (raw `"col [asc|desc], …"`).
    /// `skip_serializing_if` empty so a store that doesn't order never
    /// perturbs the serialized IR bytes (the §52 brief-#33 /
    /// [[feedback-boot-hydrate-self-heal]] no-drift discipline).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub order_by: String,
    /// §Fase 67.b — `limit:` clause (raw `"100"` or `"${max}"`).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub limit_expr: String,
    /// §Fase 76.d — `aggregate:` clause (raw, closed catalog: `count` /
    /// `sum(col)` / `avg(col)` / `min(col)` / `max(col)`).
    /// `skip_serializing_if` empty so a non-aggregating retrieve never
    /// perturbs the serialized IR bytes (the same §67.b no-drift
    /// discipline — zero IR-SHA drift for existing programs).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub aggregate: String,
    /// §Fase 76.d — `group_by:` clause (raw `"col, col2"`).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub group_by: String,
    /// §Fase 85.b — `cache:` reference (a declared `cache` name). Empty ⇒
    /// uncached. Elided when empty (IR-SHA stable for cache-less retrieves).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub cache: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRMutateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub store_name: String,
    pub where_expr: String,
    /// §Fase 35.p — declared `{ col: value }` SET assignments (value
    /// expressions kept raw; interpolated at runtime). Empty ⇒ the
    /// runtime writes the flow's user bindings (v1.31.0 fallback).
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRPurgeStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub store_name: String,
    pub where_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRTransactBlock {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
}

/// §Fase 88.a — IR for the `warden` adversarial-analysis block. Carries the
/// target, the mandatory `scope_ref`, and the recursively-lowered body so the
/// enterprise engine (§88.f) can drive the analysis and §88.c can enforce the
/// authorization discipline.
#[derive(Debug, Clone, Serialize)]
pub struct IRWarden {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub target: String,
    pub scope_ref: String,
    /// Nested flow-body IR (recursively lowered).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<IRFlowNode>,
}

/// §Fase 88.a — IR for a `scope` authorization-policy declaration.
#[derive(Debug, Clone, Serialize)]
pub struct IRScope {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub targets: Vec<String>,
    pub depth: String,
    pub approver: String,
}

/// §Fase 51.a — IR for the `quant` cognitive block (Hilbert-space projection).
/// Mirrors `ast::QuantBlock`. Optional attributes serialize only when present
/// (`skip_serializing_if`) so a bare `quant {}` lowers to a minimal node and
/// the JSON stays diff-stable. The body lowers recursively, like `par` branches.
#[derive(Debug, Clone, Serialize)]
pub struct IRQuant {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    /// Encoding scheme surface spelling (`amplitude` | `angle`); `None` = default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// Referenced `Observable` (Pauli-sum) name; `None` if unspecified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observable: Option<String>,
    /// Register width n; `None` = inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qubits: Option<i64>,
    /// Variational circuit depth L; `None` = backend default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<i64>,
    /// Projected-kernel bandwidth γ (D7); `None` = backend default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<f64>,
    /// §Fase 69.c — data re-uploading layers L (`None`/`1` = no re-uploading).
    /// `skip_serializing_if` keeps a non-re-uploading block's IR byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reupload: Option<i64>,
    /// Algebraic-effect backend tag (`quant_sim` | `qpu_native`).
    pub effect: String,
    /// Nested flow-body IR (recursively lowered).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<IRFlowNode>,
}

// ── Tier 2 IR nodes ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IRAgent {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub goal: String,
    pub tools: Vec<String>,
    pub memory_ref: String,
    pub strategy: String,
    pub on_stuck: String,
    pub shield_ref: String,
    pub max_iterations: Option<i64>,
    pub max_tokens: Option<i64>,
    pub max_time: String,
    pub max_cost: Option<f64>,
}

/// §Fase 71.a — the lowered temporal execution-window guard. The runtime
/// (§71.b) evaluates `is_in_window(now, tz, allow)`; the daemon binding +
/// coalesced defer ledger are §71.c/d.
#[derive(Debug, Clone, Serialize)]
pub struct IRWindow {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub timezone: String,
    pub allow: Vec<IRWindowSpan>,
    /// §Fase 71.e — excluded dates (holidays): ISO `YYYY-MM-DD` literals. A tick
    /// whose local date is in this set is OUTSIDE regardless of the hour spans.
    /// `skip_serializing_if` keeps a holiday-less window's JSON byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    pub on_outside: String,
}

/// §Fase 71.a — one allowed day/hour span.
#[derive(Debug, Clone, Serialize)]
pub struct IRWindowSpan {
    pub day_start: String,
    pub day_end: String,
    pub hour_start: i64,
    pub hour_end: i64,
}

/// §Fase 72.a — the `budget { … }` linear-effect rate limit lowered to IR. Each
/// quota gates a declared tool's dispatch on a renewable token bucket (the §72.b
/// `RateLease`); `on_exhausted` is the exhaustion policy.
#[derive(Debug, Clone, Serialize)]
pub struct IRBudget {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    /// §Fase 114.a — the name of a **top-level** `budget`. Empty ⇒ the anonymous
    /// daemon-attached form. Skip-if-empty ⇒ every pre-§114 program serializes
    /// byte-identically (IR-SHA stability).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub quotas: Vec<IRBudgetQuota>,
    /// `block` (fail-closed) | `defer` (reschedule via the §71 defer ledger) |
    /// `shed` (skip the call). An omitted policy lowers to `block` (the safe
    /// fail-closed default).
    pub on_exhausted: String,
}

/// §Fase 72.a — one quota: `<kind>: <limit> per <period> on Tool(<effect>)`.
#[derive(Debug, Clone, Serialize)]
pub struct IRBudgetQuota {
    /// `rate` (renewable bucket) | `max` (windowed hard cap, no intra-window refill).
    pub kind: String,
    /// Token allowance per period (> 0, validated by `axon-T831`).
    pub limit: i64,
    /// `second` | `minute` | `hour` | `day` (closed catalog, `axon-T832`).
    pub period: String,
    /// The declared tool this quota governs (`on Tool(X)`; resolved by `axon-T830`).
    pub effect: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRShield {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub scan: Vec<String>,
    pub strategy: String,
    pub on_breach: String,
    pub severity: String,
    pub quarantine: String,
    /// §8.2.h.3 — Python emits concrete 0, not null. AST keeps `Option<i64>`
    /// so the parser can distinguish "not set"; IR lowering collapses.
    pub max_retries: i64,
    pub confidence_threshold: f64,
    pub allow_tools: Vec<String>,
    pub deny_tools: Vec<String>,
    pub sandbox: bool,
    pub redact: Vec<String>,
    pub log: String,
    pub deflect_message: String,
    // `taint` exists on `ShieldDefinition` (AST) but Python's reference
    // IRShield doesn't emit it. Hidden from JSON output for §8.2.h parity.
    #[serde(skip)]
    pub taint: String,
    /// §ESK Fase 6.1 — covered regulatory classes for this shield.
    pub compliance: Vec<String>,
    /// §Fase 77.a — egress signing algorithm (`hmac_sha256`; empty = the
    /// shield does not sign). Elided from JSON when empty so every pre-§77
    /// program's IR stays byte-identical (zero IR-SHA drift).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sign: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRPix {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub source: String,
    pub depth: Option<i64>,
    pub branching: Option<i64>,
    pub model: String,
}

/// §Fase 62.0 — the audit-chain (`ledger`) IR node. Same shape as [`IRPix`]
/// but a DISTINCT node (`node_type: "ledger"`): a ledger binds a hash-linked
/// recorder to an audited surface (`source`), retaining `depth` rows under a
/// `branching`-factor Merkle tree, hashed with `model`. Kept separate from
/// `IRPix` so the navigator and the audit chain never alias on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct IRLedger {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub source: String,
    pub depth: Option<i64>,
    pub branching: Option<i64>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRPsyche {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub dimensions: Vec<String>,
    pub manifold_noise: Option<f64>,
    pub manifold_momentum: Option<f64>,
    pub safety_constraints: Vec<String>,
    pub quantum_enabled: Option<bool>,
    pub inference_mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRCorpus {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub documents: Vec<String>,
    /// §Fase 63.A — typed weighted edges. Non-empty ⇒ this corpus is an MDN
    /// graph `C = (D, R, τ, ω, σ)`; the runtime builds an `mdn::Corpus` from it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<IRCorpusRelation>,
    /// §Fase 63.C — `adaptive: true` enables the memory endofunctor on this
    /// corpus's navigations.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adaptive: bool,
    pub mcp_server: String,
    pub mcp_resource_uri: String,
    /// §Fase 64.A — when present, this is a DYNAMIC store-sourced MDN graph: the
    /// documents and typed edges are rows in two declared `axonstore`s and the
    /// runtime builds the `mdn::Corpus` from the live rows at navigate-time
    /// (per-tenant, growing). Absent ⇒ the static §63 corpus (byte-identical IR).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_source: Option<IRCorpusStoreSource>,
}

/// §Fase 63.A — a lowered MDN corpus-graph edge `(from, to, τ, ω)`.
#[derive(Debug, Clone, Serialize)]
pub struct IRCorpusRelation {
    pub etype: String,
    pub from: String,
    pub to: String,
    pub weight: f64,
}

/// §Fase 64.A — the lowered store-mapping of a dynamic, `axonstore`-sourced MDN
/// corpus graph. `doc_store(doc_id, doc_title)` maps rows → nodes;
/// `edge_store(edge_from, edge_to, edge_type, edge_weight)` maps rows → typed
/// weighted edges. The runtime (§64.B) reads these stores tenant-scoped at
/// navigate-time to build the `mdn::Corpus`.
#[derive(Debug, Clone, Serialize)]
pub struct IRCorpusStoreSource {
    pub doc_store: String,
    pub doc_id: String,
    pub doc_title: String,
    pub edge_store: String,
    pub edge_from: String,
    pub edge_to: String,
    pub edge_type: String,
    pub edge_weight: String,
}

/// §Fase 108.b — one compiled dataspace column. `column_type` is the
/// CANONICAL catalog name (`Text` / `Int` / `Float` / `Bool` /
/// `Timestamp` / `Json`) — aliases are resolved at IR generation, so
/// every downstream consumer (the engine's deploy hook, the §108.d PCC
/// class) reads one spelling.
#[derive(Debug, Clone, Serialize)]
pub struct IRDataspaceColumn {
    pub name: String,
    pub column_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDataspace {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    /// §Fase 108.b — the typed columnar schema (canonical type names).
    pub columns: Vec<IRDataspaceColumn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IROts {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub teleology: String,
    pub homotopy_search: String,
    pub loss_function: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRMandate {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub constraint: String,
    pub kp: Option<f64>,
    pub ki: Option<f64>,
    pub kd: Option<f64>,
    pub tolerance: Option<f64>,
    pub max_steps: Option<i64>,
    /// §Fase 119.b — the declared stability hypotheses `(D, L)`, carried to the
    /// runtime as PROOF OBLIGATIONS: the static band check was conditional on
    /// them, so dispatch must verify them against the measured backend or
    /// refuse. `None` means nothing was statically promised — absence is
    /// visible, never defaulted.
    pub drift_bound: Option<f64>,
    pub lipschitz: Option<f64>,
    pub on_violation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRCompute {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub shield_ref: String,
    /// §Fase 111.f — typed parameters. ADDITIVE (`skip_serializing_if`), so every
    /// pre-111 program's IR JSON stays byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<IRParameter>,
    /// §Fase 111.f — the declared result type.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub return_type: String,
    /// §Fase 111.f — the lowered §70 expression the runtime evaluates natively.
    /// `None` ⇒ the apply is refused (axon-T941): a compute with no body cannot
    /// compute, and binding a placeholder string in its place is how the old
    /// runtime handed a downstream step the text `"compute:Name(args)"` where it
    /// expected a number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<IRExpr>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDaemon {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub goal: String,
    pub tools: Vec<String>,
    pub memory_ref: String,
    pub strategy: String,
    pub on_stuck: String,
    pub shield_ref: String,
    /// §Fase 71.c — the `window:` temporal binding (a `window` primitive name).
    /// Empty ⇒ no temporal guard; `skip_serializing_if` keeps a windowless
    /// daemon's JSON byte-identical (D8 zero-drift).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub window_ref: String,
    /// §Fase 72.a — the `budget { … }` linear-effect rate limit. `None` ⇒ no
    /// budget; `skip_serializing_if` keeps a budgetless daemon's JSON
    /// byte-identical (D8 zero-drift).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<IRBudget>,
    pub max_tokens: Option<i64>,
    pub max_time: String,
    pub max_cost: Option<f64>,
    /// §Fase 52.a — the daemon's `listen` listeners (channel + alias + handler
    /// body). Pre-§52.a these were DROPPED at lowering (the IR daemon carried no
    /// listeners at all); now they survive so the §52.c runtime can mount + run
    /// them and the §52.d enterprise supervisor can extract them per-tenant.
    /// `skip_serializing_if` keeps a listenerless daemon's JSON unchanged (D8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<IRListenStep>,
    /// §Fase 52.d — the capability scope the daemon's runs are confined to
    /// (`requires: [cap, …]`). The enterprise supervisor mints a per-run
    /// principal scoped to exactly these. `skip_serializing_if` keeps a
    /// requires-less daemon's JSON byte-identical (D8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_capabilities: Vec<String>,
}

// ── §Fase 87 — the long-horizon autonomous research primitive ────────────────

/// §Fase 87.a — a compiled `savant` (long-horizon autonomous research
/// primitive). A governed orchestrator: the IR carries the declared surface so
/// the enterprise active-inference engine (§87.h+) can drive the FEP loop, and
/// the §87.c checker can bind `memory` to a declared store, `budget` to a §72
/// linear budget, and the body to a §79 interruptible session.
#[derive(Debug, Clone, Serialize)]
pub struct IRSavant {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cognition: Option<IRSavantCognition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<IRSavantMemory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<IRSavantBudget>,
    pub mandates: Vec<IRSavantMandate>,
}

/// §Fase 87.a — the compiled `cognition { … }` sub-block (active-inference
/// engine parameters).
#[derive(Debug, Clone, Serialize)]
pub struct IRSavantCognition {
    /// `standard | deep | hyper` — HRR dimensionality tier (validated §87.b).
    pub depth: String,
    /// Expected-Free-Energy convergence bound (`> 0`, §87.b). `None` ⇒ default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entropic_threshold: Option<f64>,
    /// `low | med | high` — explore/exploit balance (validated §87.b).
    pub divergence: String,
}

/// §Fase 87.a — the compiled `memory { … }` sub-block (retention binding).
#[derive(Debug, Clone, Serialize)]
pub struct IRSavantMemory {
    /// A declared `memory`/`corpus` name (resolved §87.c). Empty ⇒ ephemeral.
    pub backend: String,
    /// Whether to index the corpus as a simplicial-complex graph (topological
    /// β_n reading).
    pub corpus_graph: bool,
    /// Per-tenant tensor partitioning level (enforced by the enterprise engine).
    pub isolation_level: String,
}

/// §Fase 87.a — the compiled `budget { … }` sub-block (compute ceiling, bound to
/// a §72 linear budget in §87.c).
#[derive(Debug, Clone, Serialize)]
pub struct IRSavantBudget {
    /// Hard ceiling on FEP-loop iterations before the savant pauses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<i64>,
    /// Hard ceiling on `synth` (§87.d) tool-creation events per mandate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_synth: Option<i64>,
}

/// §Fase 87.a — a compiled `mandate <Name> { … }` sub-block (one research goal).
#[derive(Debug, Clone, Serialize)]
pub struct IRSavantMandate {
    pub name: String,
    pub objective: String,
    pub output_type: String,
}

// ── §Fase 99.b — Native Document Synthesis IR ─────────────────────────────────

/// §Fase 99.b — a compiled document declaration. The runtime `DocumentRenderer`
/// tool serialises this to deterministic OOXML bytes (§99.e); the
/// `DocumentProvenanceSoundness` PCC class (§99.d) re-derives the barrier from
/// it. `blocks` is the closed-catalog body tree.
#[derive(Debug, Clone, Serialize)]
pub struct IRDocument {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    /// `docx | pptx | xlsx`.
    pub target: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub template: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provenance: String,
    /// The propagated effect row (`io`, `storage`, `sensitive:*`, `legal:*`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_row: Vec<String>,
    /// §Fase 99.d — the enclosing `epistemic { mode: … }` at compile time
    /// (`believe`/`know` vouch the whole document is ≥ believe, satisfying the
    /// assertion-laundering barrier without per-field `attribute:`). Empty at
    /// top level. Recorded so the `DocumentProvenanceSoundness` PCC class
    /// re-derives the barrier identically (no false refutation).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub epistemic_mode: String,
    pub blocks: Vec<IRDocBlock>,
}

/// §Fase 99.b — one compiled document block. `fields` preserves declaration
/// order (a `Vec` of `(name, value)`), so the render is deterministic.
#[derive(Debug, Clone, Serialize)]
pub struct IRDocBlock {
    pub kind: String,
    pub fields: Vec<IRDocField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<IRDocBlock>,
}

/// §Fase 99.b — a compiled `(field, value)` pair. `kind` tags the value shape
/// (`text`|`ref`|`list`|`int`|`bool`) so the renderer + the barrier can
/// discriminate a literal from a flow-value reference without re-parsing.
#[derive(Debug, Clone, Serialize)]
pub struct IRDocField {
    pub name: String,
    /// `text | ref | list | int | bool`.
    pub kind: &'static str,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

// ── §Fase 105 — Governed CRM Delivery IR ─────────────────────────────────────

/// §Fase 105 — a compiled delivery declaration. The runtime delivery dispatch
/// (`axon::delivery`) transduces this to the configured CRM engine; the
/// `DeliveryProvenanceSoundness` PCC class (T920) re-derives the barrier from it.
/// `ops` is the closed-catalog operation list.
/// §Fase 110 — the compiled `notify` declaration. `epistemic_mode`
/// records the enclosing vouch (the §99.d/§105 discipline) so T933
/// re-derives identically at deploy (PCC `NotificationProvenanceSoundness`).
#[derive(Debug, Clone, Serialize)]
pub struct IRNotify {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub channel: String,
    /// The §94 secret-class ref (the recipient value NEVER rides the IR).
    pub to_secret: String,
    pub template: String,
    pub window: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provenance: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub epistemic_mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRDeliver {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    /// `crm`.
    pub target: String,
    /// `attached | cleared` (empty ⇒ `attached`). How field provenance crosses
    /// the boundary (D105.2) — the T920 barrier's subject.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provenance: String,
    /// The per-tenant credential key (§94 custody — resolved at dispatch, never
    /// in cognition).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
    /// The propagated effect row (`web`, `sensitive:*`, `legal:*`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_row: Vec<String>,
    /// The enclosing `epistemic { mode: … }` at compile time (`believe`/`know`
    /// vouch the delivered values are ≥ believe, satisfying the T920 barrier for
    /// a `provenance: cleared` delivery). Empty at top level. Recorded so the
    /// `DeliveryProvenanceSoundness` PCC class re-derives the barrier identically.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub epistemic_mode: String,
    pub ops: Vec<IRDeliverOp>,
}

/// §Fase 105 — one compiled delivery operation. `fields` preserves declaration
/// order (a `Vec` of `(name, value)`) so the transduced request is deterministic.
/// Reuses [`IRDocField`] — the same `(name, kind, value)` shape a document block
/// field carries, so the barrier + transducer discriminate a literal from a
/// flow-value `ref` without re-parsing.
#[derive(Debug, Clone, Serialize)]
pub struct IRDeliverOp {
    /// `upsert_contact | create_deal | add_note`.
    pub kind: String,
    pub fields: Vec<IRDocField>,
}

/// §Fase 87.d — a compiled `synth` dynamic tool-synthesis policy. The IR carries
/// the safety envelope so the enterprise Extism/WASM executor (§87.j) enforces
/// it; OSS ships a deny-by-default `SynthBackend` that refuses execution.
#[derive(Debug, Clone, Serialize)]
pub struct IRSynth {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub target: String,
    pub risk: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    pub sandbox: String,
    /// `required | none`; an omitted policy lowers to `required` (fail-closed).
    pub review: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<i64>,
}

// ── §Fase 53 — Closed-catalog extension mechanism ────────────────────────────

/// §Fase 53 — one compiled member of an `extension`. For `effects`
/// the `name` is a provenance base; `default_confidence` is a CEILING
/// (§53.d tainted-overriding). Metadata is elided from JSON when absent
/// so the serialised shape stays minimal once the `extensions` field is
/// un-skipped alongside the Python IR mirror.
#[derive(Debug, Clone, Serialize)]
pub struct IRExtensionMember {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantics: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_confidence: Option<f64>,
}

/// §Fase 53 — a compiled `extension` declaration. Rides in the IR (and,
/// once un-skipped, the proof bundle) so an independent PCC verifier
/// re-derives `is_known_base` against the artifact's own extensions
/// (soundness invariant #1). `category` ∈ {`effects`, `scan`} — the
/// type-checker (§53.c) enforces the closed category + no-shadowing +
/// provenance-class invariants before this IR is trusted.
#[derive(Debug, Clone, Serialize)]
pub struct IRExtension {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub category: String,
    pub members: Vec<IRExtensionMember>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IRAxonStore {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub backend: String,
    /// The DSN. **This is the field that actually runs.**
    ///
    /// `connection:` → `resolve_dsn` → a real sqlx `PgPool`. It is the sole DSN
    /// source for every store op in every deployed flow; there is no
    /// global-pool fallback. §113's census established this, and it is why
    /// §113 is delicate: moving authority to `resource` moves it *away* from
    /// the one field that governs anything, *toward* the half that governs
    /// nothing. A `resource:` that merely renames this string would be the
    /// nominal link — wired and hollow.
    ///
    /// §113: still parsed, but **deprecated in favour of [`Self::resource_ref`]**,
    /// and a store declared this way is INELIGIBLE for `lease` / `observe` /
    /// `reconcile`. *You cannot govern what you did not declare.*
    pub connection: String,
    /// §Fase 113 — the `resource` this store runs on (`axonstore U { resource: Db }`).
    ///
    /// When present, the store DERIVES its DSN (`resource.endpoint`), its pool
    /// size (`resource.capacity` — a knob that did not exist before §113; the
    /// pool was hardcoded at 10), and its sharing discipline
    /// (`resource.lifetime`, `axon-T941`) from the resource. **That derivation
    /// — not the reference — is what makes this real.**
    ///
    /// Empty ⇒ the legacy un-resourced form. Skip-if-empty ⇒ every pre-§113
    /// store serializes byte-identically (the §94.a `class` precedent).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resource_ref: String,
    pub confidence_floor: Option<f64>,
    pub isolation: String,
    pub on_breach: String,
    /// §Fase 35.j (D11) — Pillar IV: the capability slug required to
    /// access this store (empty = no gate).
    pub capability: String,
    /// §Fase 94.a — the secret-class prefix of a `backend: secrets`
    /// metadata store (`rotation_without_revelation`). Non-empty ⇔
    /// `backend == "secrets"` (both directions enforced by `axon-T900`
    /// before the IR ships). Elided from the wire when empty — every
    /// pre-§94 store serializes byte-identically (IR-SHA stability).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// §Fase 38.b (D1) — the OPTIONAL column-schema declaration. Three
    /// closed forms (inline / manifest-ref / env-var). `None` means the
    /// 37.x runtime+deploy path applies verbatim (D5 absolute). The
    /// §38.d / §38.e type-checker proves every store reference against
    /// this when present. §Fase 94.a: for a `backend: secrets` store the
    /// generator synthesizes the FIXED metadata schema here (the artifact
    /// is self-describing; PCC and the deploy gate re-derive against it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_schema: Option<IRStoreColumnSchema>,
}

/// §Fase 38.b (D1) — IR mirror of [`crate::store_schema::StoreColumnSchema`].
/// Serializes as a tagged union: `{"form": "inline" | "manifest_ref" |
/// "env_var", …}`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum IRStoreColumnSchema {
    Inline { columns: Vec<IRStoreColumn> },
    ManifestRef { qualified_name: String },
    EnvVar { var_name: String },
}

/// §Fase 38.b (D1) — IR mirror of [`crate::store_schema::StoreColumn`].
/// The serialized `col_type` is the canonical PascalCase name (e.g.
/// `"Uuid"`, `"Int"`, `"Timestamptz"`).
#[derive(Debug, Clone, Serialize)]
pub struct IRStoreColumn {
    pub name: String,
    pub col_type: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub primary_key: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_increment: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub not_null: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
    /// §Fase 38.x.c (D2, D5) — `true` iff the column is declared with
    /// `GENERATED ALWAYS AS IDENTITY` or `GENERATED BY DEFAULT AS
    /// IDENTITY`. Distinct from `auto_increment` (legacy SERIAL via
    /// `nextval(...)` default). `skip_serializing_if` keeps IR JSON
    /// byte-identical to v1.38.2 for any column where `identity = false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub identity: bool,
    /// §Fase 73.f (D1) — `true` iff the column carries the `index`
    /// declaration. Surfaced into the IR so the deployment layer (the
    /// enterprise deploy gate) SEES the index as a declared capability and
    /// can materialize it (a GIN path index for a `Json`/`Jsonb` column, a
    /// b-tree otherwise) — never a silent out-of-band DBA action.
    /// `skip_serializing_if` keeps IR JSON byte-identical for any column
    /// where `indexed = false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub indexed: bool,
    /// §Fase 73.g (D1) — the OPTIONAL `Json<T>` shape-lens struct name on a
    /// `Json`/`Jsonb` column (`payload: Json<UserEvent>` → `Some("UserEvent")`).
    /// Surfaced into the IR so the PCC `JsonShapeSoundness` proof can
    /// RE-DERIVE, from the artifact alone, that every lens shape resolves
    /// to a declared struct `type` — the §73.a/§73.e lens well-formedness
    /// made an independently-verifiable proof object. `skip_serializing_if`
    /// keeps IR JSON byte-identical for any column with no shape lens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_shape: Option<String>,
}

#[inline]
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize)]
pub struct IRAxonEndpoint {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub method: String,
    pub path: String,
    pub body_type: String,
    pub execute_flow: String,
    pub output_type: String,
    pub shield_ref: String,
    /// §8.2.h.3 — Python emits concrete `0`; AST stays `Option<i64>`.
    pub retries: i64,
    pub timeout: String,
    /// §ESK Fase 6.1 — κ regulatory class on the boundary.
    pub compliance: Vec<String>,
    /// §Fase 37.y (D1) — Path parameter names extracted from the
    /// `path:` string. Mirrors `AxonEndpointDefinition.path_params`.
    /// **`skip_serializing_if = Vec::is_empty`** so a pre-v1.38.5 IR
    /// JSON snapshot (without the field) is byte-identical to a
    /// v1.38.5 IR JSON for the same endpoint — D5 backwards-compat
    /// absolute. The runtime + adopter tools that consume the IR
    /// JSON parse `path_params` as an absent key → empty Vec via
    /// serde's `default` semantics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_params: Vec<String>,
    /// §Fase 37.y (D2) — Query parameters from the inline
    /// `query: { … }` block. Mirrors `AxonEndpointDefinition.query_params`
    /// using `IRTypeField` (shared with body type fields → uniform
    /// downstream tooling). **`skip_serializing_if = Vec::is_empty`**
    /// — same D5 IR-JSON byte-identity guarantee as `path_params`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_params: Vec<IRTypeField>,
    /// §Fase 51.x — capability scopes the request bearer must hold
    /// (the `requires: [scope.dotted]` declaration, §Fase 32.g). Mirror
    /// of `AxonEndpointDefinition.requires_capabilities`, lowered into
    /// the IR so the PCC CapabilityContainment property can prove that
    /// the stores this endpoint's flow reaches are all covered by the
    /// declared requires. **`skip_serializing_if = Vec::is_empty`** so a
    /// pre-§51.x IR-JSON snapshot (no `requires:`) stays byte-identical
    /// (D5 backwards-compat — empty key parses back to empty Vec).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_capabilities: Vec<String>,
    /// §Fase 83.a — the `cors: <Name>` reference, or `""` when absent
    /// (D83.5: no CORS headers, ever). NEW field on an EXISTING struct —
    /// `skip_serializing_if` (not `shield_ref`'s bare/always-emitted
    /// historical shape) so a cors-less endpoint's IR stays byte-identical
    /// to pre-§83 (zero IR-SHA drift — the standing §76.d discipline).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cors_ref: String,
    /// §Fase 89.a — the explicit authorization-coverage opt-out lowered into
    /// the IR so the enterprise runtime (§89.d) and the PCC
    /// `AuthorizationCoverage` witness (§89.c) can read it. `false` (the
    /// default + the common case) elides from JSON via `is_false` so a
    /// pre-§89 IR-JSON snapshot stays byte-identical (zero IR-SHA drift —
    /// the standing §76.d discipline).
    #[serde(default, skip_serializing_if = "is_false")]
    pub public: bool,
}

// ── §λ-L-E Fase 13 — Mobile Typed Channels IR ───────────────────────────────

/// Compiled `channel Name { … }` declaration.
///
/// Direct port of `axon.compiler.ir_nodes.IRChannel`.  Lives in
/// `IRProgram.channels`; emit/publish/discover reductions embed in
/// their containing flow/listener (paper §3 + §4 — π-calc prefix
/// discipline preserved structurally, not lifted to top-level ops).
#[derive(Debug, Clone, Serialize)]
pub struct IRChannel {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub message: String, // surface spelling — Order | Channel<Order> | …
    pub qos: String,
    pub lifetime: String,
    pub persistence: String,
    pub shield_ref: String,
    /// §Fase 77.b — non-empty ⇒ some `publish <this> within <Shield>` site
    /// referenced a SIGNING shield: the channel is an EGRESS channel and
    /// its durable events are signed-deliverable to registered external
    /// subscribers under this algorithm (first publish site wins;
    /// deterministic — the catalog has one algorithm in v1). Elided from
    /// JSON when empty (zero IR-SHA drift for pre-§77 programs).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub egress_sign: String,
}

/// §Fase 41.b — compiled typed WebSocket transport. `protocol` names the
/// `session` it carries; `backpressure_credit` is the typed-resource window
/// (`null` if unspecified). axon-rs realises the endpoint over a `tokio` WS,
/// crediting/decrementing the window per §4.2 of the paper.
#[derive(Debug, Clone, Serialize)]
pub struct IRSocket {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub protocol: String,
    pub backpressure_credit: Option<i64>,
    pub reconnect: bool,
    pub legal_basis: Option<String>,
}

/// §Fase 80.b — compiled outbound vendor connection (the client dual of
/// [`IRSocket`]). `protocol`/`role` bind the axon-facing session interface;
/// `resolve`/`secret` are per-tenant config keys (never literals — T850);
/// `map` is the compile-time-total wire↔session projection (T849). Optional
/// fields elide when absent so the IR shape is purely additive.
#[derive(Debug, Clone, Serialize)]
pub struct IRUpstream {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub transport: String,
    pub protocol: String,
    pub role: String,
    pub resolve: String,
    /// §Fase 114.u — the `resource` this upstream's channel rides. When set,
    /// `resolve` above was DERIVED from the resource's `endpoint` at LOWERING
    /// (the §114 shield-egress discipline: derivation stamped into the
    /// artifact reaches every dial path by construction — no per-site wiring
    /// to forget) and `capacity` below carries the resource's bound. Elided
    /// when empty — every pre-§114.u upstream serializes byte-identically.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resource_ref: String,
    /// §Fase 114.u — **max concurrent connection INSTANCES** of this upstream
    /// (from `resource.capacity`). Frames are already flow-controlled by
    /// `backpressure_credit`; this bounds CONNECTIONS. The runtime holds a
    /// per-process semaphore permit for the life of each dialed handle —
    /// the same in-memory/per-process bound §114.e documented for tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<i64>,
    pub secret: String,
    pub auth_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_prefix: Option<String>,
    pub map: Vec<IRUpstreamMapRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect: Option<IRUpstreamReconnect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backpressure_credit: Option<i64>,
    /// §80.f — the `Preset@vN` reference this declaration was expanded from
    /// (provenance for the compliance reviewer); absent for hand-written ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

/// §Fase 80.b — one compiled `map:` projection rule.
#[derive(Debug, Clone, Serialize)]
pub struct IRUpstreamMapRule {
    pub node_type: &'static str,
    pub direction: String,
    pub message: String,
    pub framing: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_value: Option<String>,
}

/// §Fase 80.b — compiled reconnect policy (all three fields required by the
/// parser — a reconnection policy with a hole is not a policy).
#[derive(Debug, Clone, Serialize)]
pub struct IRUpstreamReconnect {
    pub backoff_ms: i64,
    pub max_attempts: i64,
    pub on_exhausted: String,
}

/// §Fase 83.a — a named, referenced browser-origin policy. Mirrors
/// `IRShield`'s field-for-field shape; consumed by `IRAxonEndpoint.cors_ref`.
/// Wildcard+credentials (T853), origin-glob shape (T854), and closed-method
/// (T855) violations are all rejected before this node is ever lowered — the
/// checker re-derives the same closed catalogs at deploy time (§83.c,
/// `CorsPolicyConsistency`), so an IR that reaches the runtime is already
/// proven consistent.
#[derive(Debug, Clone, Serialize)]
pub struct IRCors {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    pub allow_origins: Vec<String>,
    pub allow_methods: Vec<String>,
    pub allow_headers: Vec<String>,
    pub allow_credentials: bool,
    /// Duration literal (`"3600s"`) — same string-carries-the-unit
    /// convention as `axonendpoint.timeout`; the consumer (enterprise's
    /// dynamic CORS middleware) parses it into seconds at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expose_headers: Vec<String>,
}

/// §Fase 85.b — compiled `cache` policy. The checker (§85.c) re-derives the
/// same laws at the deploy gate (`CacheSoundness`), so an IR that reaches the
/// runtime is already proven sound (one default max, non-pure ⇒ finite ttl,
/// references resolve). Every optional field is `skip_serializing_if` so a
/// bundle using `cache` only pays IR bytes for what it declares, and a bundle
/// with no `cache` never emits a `caches` key (IR-SHA stable, §76.d).
#[derive(Debug, Clone, Serialize)]
pub struct IRCache {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    /// `"redis"` | `"in_process"`; empty ⇒ runtime default (`in_process`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub backend: String,
    /// Duration literal (`"10s"`) — same string-carries-the-unit convention as
    /// `cors.max_age`. `None` ⇒ cache-forever (sound only for a `pure` cache).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
    /// The parameter-name subset forming the key; empty ⇒ all bound params.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_params: Vec<String>,
    /// `true` ⇒ auto-covers every eligible tool (at most one per module).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default_policy: bool,
    /// Effect classes this cache memoises; empty ⇒ `["pure"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apply_to_effects: Vec<String>,
    /// Channel names whose `emit` flushes this cache's namespace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalidate_on: Vec<String>,
}

/// Compiled emit step — `c⟨v⟩.P` (Chan-Output / Chan-Mobility).
///
/// `value_is_channel = true` ⇒ resolved at lowering time as a channel
/// handle (second-order mobility, paper §3.2); the runtime dispatches
/// on this flag without re-resolving symbols.
#[derive(Debug, Clone, Serialize)]
pub struct IREmit {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub channel_ref: String,
    pub value_ref: String,
    pub value_is_channel: bool,
    /// §Fase 114 (owed) — the σ-shield the target `channel` declares
    /// (`channel C { … shield: S }`), RESOLVED here at lowering (Phase 0
    /// pre-pass, like `IRPublish.sign`) so the runtime `run_emit` scans the
    /// emitted value through S on EVERY dispatch path without re-deriving the
    /// channel↔shield map. Empty ⇒ an unshielded channel (byte-identical to a
    /// pre-§114 emit: `skip_serializing_if` elides it → zero IR-SHA drift for
    /// programs whose channels declare no shield).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shield_ref: String,
    /// §Fase 114.w — the σ-shield's breach policy, resolved at lowering beside
    /// `shield_ref` (same Phase 0 pre-pass). `None` ⇒ no `on_breach:` declared
    /// (halt, the fail-closed default); elided → zero IR-SHA drift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breach_policy: Option<IRBreachPolicy>,
}

/// §Fase 92.a — compiled `credential` contract. The TTL is carried as
/// SECONDS (converted at lowering from the duration literal) so every
/// consumer — the OSS minter port, the enterprise PASETO minter, the
/// deploy gate — shares one arithmetic-ready representation.
#[derive(Debug, Clone, Serialize)]
pub struct IRCredential {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub name: String,
    /// The bearer lifetime in seconds (from the `ttl:` duration literal;
    /// `0` = unparseable, rejected by `axon-T894` before the IR ships).
    pub ttl_secs: u64,
    /// The capability slugs the minted bearer carries (validated dotted
    /// slugs; non-empty per `axon-T893`).
    pub grants: Vec<String>,
}

/// §Fase 92.b — compiled `mint <Credential> as <binding>` step. The runtime
/// resolves the contract, enforces the attenuation law
/// (`grants ⊆ capabilities(minter)`, fail-closed), mints via the
/// `CredentialMinter` port, and binds the raw bearer under `binding`.
#[derive(Debug, Clone, Serialize)]
pub struct IRMintStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub credential_ref: String,
    pub binding: String,
}

/// §Fase 94.b — compiled `rotate <SecretsStore> [where "…"] with <Tool>
/// as <binding>` step. The runtime enumerates the custody entries of the
/// store's class matching `where_expr` (whole class when empty), performs
/// ONE mediated exchange per key through the named tool (reveal → tool
/// renews → CAS commit at version+1), and binds the METADATA-ONLY
/// summary. Fail-closed without a custody port; each per-key failure
/// degrades with a witness, never destructively.
#[derive(Debug, Clone, Serialize)]
pub struct IRRotateStep {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub store_ref: String,
    /// §67 metadata filter; empty = the whole class (elided from the wire).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub where_expr: String,
    pub tool_ref: String,
    pub binding: String,
}

/// Compiled publish step — capability extrusion (Publish-Ext, paper §4.3).
#[derive(Debug, Clone, Serialize)]
pub struct IRPublish {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub channel_ref: String,
    pub shield_ref: String,
    /// §Fase 77.b — the referenced shield's `sign:` algorithm, RESOLVED at
    /// lowering (order-independent pre-pass over every declared shield).
    /// Non-empty ⇒ this publish is an EGRESS declaration: the channel's
    /// events are signed-deliverable to registered external subscribers.
    /// Elided from JSON when empty (zero IR-SHA drift for pre-§77 programs).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sign: String,
}

/// Compiled discover step — dual of publish.
#[derive(Debug, Clone, Serialize)]
pub struct IRDiscover {
    pub node_type: &'static str,
    pub source_line: u32,
    pub source_column: u32,
    pub capability_ref: String,
    pub alias: String,
}
