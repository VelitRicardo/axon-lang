//! §Fase 111 — **the anti-drift gate: the public README is a BUILD INPUT.**
//!
//! # Why this exists
//!
//! §111 audited every primitive the public README advertises and found ~22 of
//! ~74 to be aspirational. The obvious question afterwards was: *what gate would
//! have caught them?* The obvious answer — "assert README ⟺ registry ⟺ parser
//! production ⟺ dispatch arm" — is **wrong**, and it is worth being precise
//! about why, because the wrong gate is worse than none: it would have been
//! green.
//!
//! ```text
//!               README   registry   parser   dispatch arm   …and yet
//!   warden        ✓         ✓         ✓          ✓          a no-op
//!   quant         ✓         ✓         ✓          ✓          a no-op
//!   transact      ✓         —         ✓          ✓          no transaction
//!   compute       ✓         ✓         ✓          —          returns a string
//! ```
//!
//! **A presence-only gate catches nothing that matters.** Every serious §111
//! defect had all four boxes ticked. What was missing was never a *symbol* — it
//! was the answer to a question no linter can decide:
//!
//! > **Does the runtime do what the summary promises?**
//!
//! That is not statically inferable, so this gate does not pretend to infer it.
//! It forces a **human to state it, on the record**, and it makes the statement
//! impossible to omit: [`status_of`] has **no default arm**, so a name the README
//! advertises with no entry here is a **build failure**.
//!
//! # The four laws
//!
//! 1. **The README is parsed at test time.** Every `<code>` badge in its header
//!    block must have an entry here. The public promise is not a document that
//!    drifts from the code — it is an *input to the build*.
//! 1b. **(§114.z) The registry is the enumeration source.** Every
//!    `PRIMITIVE_REGISTRY` entry with `is_advertised: true` must have an entry
//!    here AND a `<code>` badge in the README. The §111 gate enumerated only
//!    the badges, so a primitive DISCUSSED but never BADGED escaped
//!    classification entirely — `budget` appeared in the README seventeen
//!    times, was badged zero, and its dead runtime (§114.a) survived every
//!    green build. **A gate that guards only the badges guards the
//!    packaging.** Conversely, an `is_advertised: false` primitive must be
//!    neither badged nor classified: hiding a primitive from the promise is a
//!    deliberate, pinned act.
//! 2. **Nothing advertised may be [`RuntimeStatus::NotImplemented`]** … except
//!    what is written down in [`KNOWN_DEBT`], which is a **ratchet**: it may only
//!    shrink. A *new* unimplemented promise is a red build. That is the whole
//!    point — §111's debt becomes a ledger the compiler enforces, instead of a
//!    finding in a document nobody rereads.
//! 3. **A [`RuntimeStatus::Real`] claim must cite its proof**, and if the proof
//!    names a test file, that file must exist on disk. A claim of reality that
//!    cannot point at the gate proving it is just a nicer-sounding assertion.
//! 4. **The proof must cite the PATH, not the engine** — §Fase 119.f.8, learned
//!    the expensive way. `reason` was attested `Real { proof:
//!    "pure_shape::run_reason" }`. The citation was accurate: the handler
//!    existed, worked, and had its own tests. It was also worthless, because the
//!    grammar that reaches it did not exist — the README's `reason { given ask
//!    depth }` (16 blocks, each its step's entire cognition) was thrown away at
//!    PARSE time, and `axon check` said `0 errors`. §111's own phrase: *motor
//!    real, cable muerto*. A function name proves an engine is there; it says
//!    nothing about whether any published program can call it. **Cite a gate
//!    that walks source → dispatch.** This is the table's blind spot, and it is
//!    why [`KNOWN_DEBT`] being empty is necessary and not sufficient.
//!
//! # How to change this file
//!
//! - **Shipping a primitive?** Flip it to `Real`, cite the gate that proves it,
//!   and delete its `KNOWN_DEBT` row **in the same PR**.
//! - **Retracting one?** Delete it from the README *and* from here.
//! - **Adding a new advertised primitive?** You cannot land it without stating
//!   what its runtime actually does. That is the feature.

#![allow(dead_code)]

/// What the RUNTIME does — as attested by a human, because no linter can decide
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    /// The runtime delivers what the README's summary promises. MUST cite the
    /// evidence — a test file, or the fase that shipped it.
    Real { proof: &'static str },
    /// It delivers, with a documented gap versus the advertised claim. The gap
    /// is named so it cannot rot into a silent lie.
    Partial { gap: &'static str },
    /// It REFUSES — at compile time or at dispatch. Honest by construction: an
    /// adopter is told, loudly, rather than handed silence. This is the §108
    /// posture, and it is an acceptable state to advertise from, because the
    /// adopter cannot be *fooled* by it.
    FailsClosed { diagnostic: &'static str },
    /// The runtime does NOT do what the summary promises, and says nothing about
    /// it. **This may not be advertised** unless it is in [`KNOWN_DEBT`].
    NotImplemented { finding: &'static str },
    /// §111 has not verified this one. Advertised on trust, and counted: the
    /// unaudited population is pinned below and may only shrink.
    Unaudited,
}

impl RuntimeStatus {
    /// The predicate law 2 turns on.
    pub fn is_unimplemented(self) -> bool {
        matches!(self, RuntimeStatus::NotImplemented { .. })
    }
}

use RuntimeStatus::*;

/// Every `<code>` badge the README's header block advertises, and what its
/// runtime actually does.
///
/// Verdicts sourced from the §111 Gate-1/Gate-2 audit (see the fase's topic
/// file). Where §111 did not reach, the entry is honestly `Unaudited` rather
/// than optimistically `Real` — an unchecked box is not a passing one.
pub const ADVERTISED: &[(&str, RuntimeStatus)] = &[
    // ── Cognitive core — LLM-driven, and honestly so. Calling the model IS
    //    their nature; that is not the defect §111 hunts.
    ("persona", Real { proof: "flow_dispatcher::pure_shape" }),
    ("intent", Unaudited),
    ("flow", Real { proof: "flow_dispatcher::dispatch_node (exhaustive, zero catch-alls)" }),
    // §Fase 119.f.8 — the proof used to read `pure_shape::run_reason`, and it was
    // TRUE and USELESS. The handler existed and worked; the grammar that reaches
    // it did not. The README's `reason { given ask depth }` — 16 blocks, each one
    // its step's entire cognition — was discarded at PARSE time, so this "Real"
    // attested an engine no published program could call.
    //
    // ⚠️ The lesson is about THIS TABLE, not about `reason`: a proof that names a
    // FUNCTION proves the engine exists. It does not prove any published program
    // reaches it. Cite the gate that walks the path — grammar to dispatch — and
    // the attestation means what a reader thinks it means.
    ("reason", Real { proof: "fase119_f8_reason_block + fase119_f8_reason_prompt_is_built (grammar → IR → prompt); engine pure_shape::run_reason" }),
    ("anchor", Real { proof: "anchor_checker" }),
    ("refine", Real { proof: "pure_shape::run_refine" }),
    ("memory", Real { proof: "cognitive::run_remember / run_recall (PEM write-through)" }),
    ("tool", Real { proof: "lambda_tools::dispatch_use_tool_real (§58)" }),
    ("probe", Real { proof: "pure_shape::run_probe" }),
    // §Fase 119.f.9 — see rule 4 above: this proof used to name the handler
    // alone, and the handler was reachable only from a braced form no published
    // block writes. Worse, it sent the model the source NAMES, so even when
    // reached it synthesised over identifiers instead of outputs.
    ("weave", Real { proof: "fase119_f9_weave_statement + fase119_f9_weave_prompt_is_built (grammar → IR → resolved prompt); engine pure_shape::run_weave" }),
    ("validate", Real { proof: "pure_shape::run_validate" }),
    ("context", Unaudited),
    // ── Epistemic scopes
    ("know", Unaudited),
    ("believe", Unaudited),
    ("speculate", Unaudited),
    ("doubt", Unaudited),
    // ── Concurrency & continuation
    ("par", Real { proof: "parallel::run_par (real fan-out via join_all, §65)" }),
    ("hibernate", Partial { gap: "§119.d made the suspension REAL on the production dispatch path: the walk HALTS at the hibernate point (the task dies — zero further compute, zero tokens, the primitive's economic guarantee), the continuation parks under the README's SHA-256(flow ∥ event ∥ position) id with the remaining nodes + bindings + catalogs, `emit <channel>` WAKES matching continuations (fire-and-forget resume through the real dispatcher), a late resume is REFUSED by lazy expiry (no timer burns while asleep), and `hibernate until \"event\"` — the README's own spelling — parses. Gates: tests/fase119_d_hibernate.rs (halt counted in StepCompletes, park, emit-wake e2e, nested refusal) + hibernation unit suite; 4/4 mutations killed incl. the F20 walk-keeps-going reinjection. THE GAP: the parking lot is in-process (the pem::InMemoryBackend discipline) — a continuation does not survive process restart, so README's sleep-for-months across restarts is the durable-store enterprise catch-up (cognitive_states), and chained hibernation inside a resumed continuation is not yet defined" }),
    // ── Deterministic data plane (§108)
    ("dataspace", Real { proof: "dataspace_engine (columnar, first-party)" }),
    ("ingest", Real { proof: "cognitive::run_ingest — bounds-before-parse, sha256 + Untrusted taint (§108)" }),
    ("focus", Real { proof: "dataspace_engine::focus_query (σ∘π)" }),
    ("associate", Real { proof: "dataspace_engine::associate_query (hash equi-join; refuses a keyless join)" }),
    ("aggregate", Real { proof: "dataspace_engine::aggregate_query (γ)" }),
    ("explore", Real { proof: "dataspace_engine::explore_profile (zone-map stats)" }),
    // ── Budget, selection & synthesis
    ("deliberate", FailsClosed { diagnostic: "axon-T939 (§111) — the body is discarded at parse time; no budget was ever controlled" }),
    ("consensus", FailsClosed { diagnostic: "axon-T940 (§111) — no votes, no aggregation, no candidates" }),
    ("forge", Partial { gap: "§111 F17 — the Poincaré pipeline + NCD novelty gate are real, but `coherence` is hardcoded to 1.0, so a declared `constraints:` coherence floor is INERT" }),
    // §Fase 119.m.1 — `Unaudited` → `Partial`. It was the worst state in this
    // table: §111 never checked it, and when §119 finally did there was nothing
    // to check — eleven declared `IRAgent` fields, and no executor anywhere in
    // the dispatcher. It now RUNS, with `strategy:` as a control policy
    // (D119.5) and a termination proof per strategy.
    //
    // `Partial`, not `Real`, and the gap is one specific thing rather than a
    // hedge: `strategy: custom` means "the control policy is the `step` list
    // written inside the agent block", and `AgentDefinition` has no body field,
    // so the parser discards those steps. Dispatch REFUSES `custom` by name
    // instead of substituting another policy. `on_stuck: hibernate` is refused
    // for the same reason in the other direction — §119.d parks a flow walk's
    // remaining nodes and an agent loop has no such list.
    ("agent", Partial {
        gap: "§119.m.1 — react / reflexion / plan_and_execute run with per-strategy \
              termination proofs (gate: fase119_m1_agent_executor.rs). `strategy: custom` \
              is REFUSED, not substituted: its policy is the `step` list inside the agent \
              block and `AgentDefinition` has no body field, so the parser drops it. \
              `on_stuck: hibernate` is refused likewise — an agent loop has no \
              remaining-node list for §119.d to park.",
    }),
    ("shield", Partial { gap: "§111 — the scanner registry IS consulted and a Reject really fails the step, but OSS ships ZERO registered scanners, so an OSS adopter's shield is an identity pass-through until one is mounted" }),
    // ── Security & autonomous analysis
    ("savant", Unaudited),
    ("synth", Unaudited),
    ("warden", Real { proof: "tests/fase111_c_warden_wired.rs (§111.c — real attested findings, verify()-gated, body runs, fail-closed at 6 joints)" }),
    ("scope", Real { proof: "tests/fase111_c_warden_wired.rs — the scope catalog is resolved at dispatch and the allowlist enforced (the check §88.c deferred)" }),
    // ── Effects & streaming
    ("stream", Real { proof: "tests/fase111_e_stream_runs.rs (§111.e — the body is parsed, lowered and EXECUTED; it used to be discarded at parse time)" }),
    ("effects", Real { proof: "parse_effect_row + type_checker; §85 `cache` derives cacheability from the `effects: pure` proof" }),
    ("@contract_tool", Unaudited),
    ("@csp_tool", Unaudited),
    // ── Knowledge navigation (PIX · MDN)
    ("pix", Real { proof: "pix_navigator (embeddings-free structural index)" }),
    ("navigate", Partial { gap: "§111 F11 — three REAL deterministic engines (MDN store-sourced, MDN in-memory, PIX), BUT with no indexable source in scope it falls back to an LLM prompt that INSTRUCTS the model to fabricate a provenance trail. The one live §108 left in the tree" }),
    ("drill", Partial { gap: "§111 — real subtree navigation when a source is in scope; degrades to a placeholder string otherwise" }),
    ("trail", Partial { gap: "§111 — reads the real breadcrumb `navigate` seeds; falls back to a placeholder when no navigate ran. Inherits F11: a trail harvested from the LLM fallback is confabulation wearing an audit's clothes" }),
    ("corpus", Real { proof: "mdn (signed Epistemic PageRank; §62–§64)" }),
    // ── Advanced cognition & trust
    ("psyche", Unaudited),
    ("ots", Partial { gap: "§119.c made the dispatch layer REAL: `apply_ots_to_target` (identity, §111 F18) is deleted; `run_ots_apply` resolves the declaration, consults the name-keyed `ots_registry` (shield_registry's shape — the hook that F18 said did not exist, now tested by registering a transformer and watching it fire), propagates transformer refusals, and REFUSES an unregistered name (identity under a transformation's name fabricates a result). Step-scoped `ots X on y -> b` elevates before generation. THE GAP: no OSS transformer registers by default — any generic default would be the identity lie again — so OSS `ots` refuses at runtime until the media pipeline (crate::ots) or an enterprise vertical registers; the paper's JIT synthesis pipeline remains future work" }),
    ("mcp", Unaudited),
    ("mandate", Real { proof: "§119.b — the closed loop runs through production dispatch: axon-rs/src/mandate_engine.rs (e = 1 − CSR from pem::semantic_validator, PID from pem::density_matrix, Converge(e,ε,N) with Breach fail-closed, declared-D premise discharge, |C|=0 refused before any token) driven by run_mandate_apply + the D119.4 step guard (buffered — attempts never stream). Tier 1 logit-bias bans on the OpenAI-compat wire, Tier 2 CSP feedback everywhere. Gates: mandate_engine tests (16), algebraic_handlers MandateApply tests (incl. the F18 regression: an unresolved name REFUSES, never identity), openai_compat logit_bias wire tests, fase119_b3_stability_band.rs, fase119_b_mandate_grammar.rs — 9/9 dispatch-chain mutations killed" }),
    ("lambda", Real { proof: "§119.c — the ΛD engine (lambda_data.rs: ψ = ⟨T,V,E⟩, four invariants, compose with c_out ≤ min(c_i); lambda_runtime.rs: Theorem 5.1 enforced at apply — only raw data may carry c = 1.0) now runs on the PRODUCTION dispatch path: `apply_lambda_data` (the placeholder string, §111 F18) is deleted, `run_lambda_data_apply` builds ψ via build_psi with D10 runner parity, an undeclared lambda REFUSES, and the step-scoped `lambda X on y -> b` (README blocks 46-47) elevates BEFORE prompt interpolation. Gates: lambda_tools elevation suite incl. the F18 regression + fase119_c_apply_grammar.rs; mutations C1/C2/C5 killed" }),
    // ── Deterministic compute
    ("compute", Real { proof: "tests/fase111_f_compute_real.rs (§111.f — a named PURE FUNCTION over the §70 expression language, evaluated natively by eval_expr: linear in the term, ZERO tokens, no model in the loop. Every failure refuses rather than binding a string that looks like a number)" }),
    // ── Reactive processes & platform boundary
    ("daemon", Real { proof: "daemon.rs — OTP-style supervision + §74 durable event delivery" }),
    ("listen", Partial { gap: "§111 F7 — TWO disjoint paths sharing one keyword. Inside a DAEMON: real (§74 outbox → deliver_typed_event → execute_server_flow). Inside a FLOW BODY: binds the canned string \"(awaiting <channel>)\". Sub-gap: a daemon listener body executes ONLY `run <Flow>` steps; any other step type is silently dropped" }),
    ("axonendpoint", Real { proof: "axon_server — typed routes, body/output schemas, Idempotency-Key (§32/§37)" }),
    ("axpoint", Real { proof: "an ALIAS of axonendpoint — same TokenType (tokens.rs)" }),
    ("axonstore", Real { proof: "wire_integrations::run_{persist,retrieve,mutate,purge} — parameterized SQL, capability gate, epistemic floor, HMAC-Merkle audit chain" }),
    // ── Cognitive I/O (λ-L-E, Fases 1–9)
    //
    // §111 F14 + the §11 REFRAME: this family is not merely unwired — it is
    // UNREACHABLE BY CONSTRUCTION. There is no FlowStep, no IRFlowNode and no
    // daemon field that can consume any of them. The kernels are real,
    // well-written and unit-tested; the language never grew a way to reach them.
    // Founder-ratified: designing that consumption surface is §112+.
    ("resource", Real { proof: "tests/fase113_c_resource_is_a_wire.rs (§113.c — an `axonstore { resource: Db }` DERIVES its DSN and its POOL SIZE from the resource: `capacity: 20` produces twenty connections. Before §113 `capacity` was read by zero lines of code in either repo while every pool sat at a hardcoded 10, and `lifetime` — sold as Linear Logic — was read by nothing at all)" }),
    ("fabric", Partial { gap: "§119.e made provider/region DECIDE things, on the line the knowledge doc draws (fabric describes what the runtime expects to find; it is explicitly NOT infrastructure-as-code, so `provision` is not what it owed). COMPILE TIME: `axon-E041` refuses a region that cannot belong to the declared provider (the doc's own `provider: aws region: \"eastus\"` example) and `axon-E042` refuses a compliance obligation the substrate cannot satisfy (the doc's own \"a GDPR-tagged manifest deployed to a non-EU region is rejected\") — with an UNDETERMINABLE jurisdiction counted as a violation, never a pass. RUNTIME: a tool whose resource lives `within` a fabric carries that fabric's (provider, region) on its binding and into its audit row — the doc's compliance propagation, made a fact — and a `within:` naming an undeclared fabric REFUSES the binding. Gates: axon-frontend/tests/fase119_e_fabric_substrate.rs (10) + substrate unit suite (11) + axon-rs/tests/fase119_e_fabric_runtime.rs (5); 6/6 mutations killed. THE GAP: the provider catalog is open by design, so a provider whose region shape this compiler does not know is accepted UNVALIDATED (visible via ProviderShape::Unvalidated) — and `zones`/`ephemeral`/`shield:` are still consumed by nothing, so a fabric-level shield does not yet wrap resource acquisition" }),
    ("manifest", Partial { gap: "§111 F14 — the κ/compliance half IS genuinely consumed (it feeds attestation + the audit scorer); the \"desired shape\" half is dead" }),
    ("observe", Real { proof: "tests/fase112_c_cognitive_io_deploy.rs (§112.a/c — a real Handler reaches a real target through a deny-by-default SourceRegistry; an observation that cannot be taken REFUSES. The only prior Handler returned certainty 1.0 unconditionally, without going anywhere)" }),
    ("reconcile", Real { proof: "tests/fase112_e_reconcile_drift.rs (§112.e — REAL Jaccard drift between the manifest's desired shape and the world's actual shape. It used to compare the belief against ITSELF: when evidence was missing it defaulted to the manifest, so drift was structurally always 0.0)" }),
    ("lease", Real { proof: "tests/fase113_d_lease_breach_fires.rs (§113.d — post-expiry USE of a leased store is the CT-2 Anchor Breach, and all three `on_expire` policies are honoured. The kernel was NEVER broken: it had no SUBJECT. A flow could not USE a resource, so a guarantee about post-expiry use was VACUOUS — unviolatable, and therefore unkeepable. §113 made the store operation the use)" }),
    ("ensemble", Real { proof: "tests/fase112_c_cognitive_io_deploy.rs (§112.b/c — the EnsembleAggregator is instantiated from the IR at deploy and aggregates only observations ACTUALLY TAKEN; a refused source is absent, not present-and-failing, which is what lets its quorum gate work honestly)" }),
    ("topology", Real { proof: "type_checker::check_topology_liveness — a genuine DFS gray/black cycle detector emitting a Honda-liveness violation (narrow sufficient condition, but real)" }),
    ("session", Real { proof: "type_checker::check_session_duality → session.rs (dual involution, capture-avoiding substitution, coinductive equality). Duality is genuinely DECIDED, not faked" }),
    ("send", Real { proof: "lowered into the session algebra; an unmatched send fails the duality check" }),
    ("receive", Real { proof: "lowered into the session algebra; dual of send" }),
    ("select", Real { proof: "session.rs dual_map — internal/external choice duality is really checked arm-for-arm" }),
    ("branch", Real { proof: "session.rs dual_map — the dual of select" }),
    ("immune", Real { proof: "tests/fase112_d_immune_fires.rs (§112.d — the KL sensor detects a REAL deviation from a LEARNED baseline. Wiring it exposed a kernel bug that made it structurally blind: an unseen symbol was scored against the baseline's MINIMUM probability, so a perfectly stable baseline could never register an anomaly at all)" }),
    ("reflex", Real { proof: "tests/fase112_d_immune_fires.rs (§112.d — a real anomaly FIRES the declared action with an HMAC-signed trace, within its sla:, and the same signature does not re-fire. No reflex may fire while the baseline is still being learned — that would be a false positive by construction)" }),
    ("heal", Real { proof: "tests/fase112_d_immune_fires.rs (§112.d — the HealKernel is registered from the IR and renders a decision under its declared mode: on a real health report)" }),
    ("compliance", Real { proof: "tests/fase117_a_regulated_boundary_coverage.rs (§117.a — `axon-T957` finally scopes the ESK Fase 6.1 κ-coverage law to the `axonendpoint`, the surface the README has advertised since v1.x. It is a real SET DIFFERENCE over the κ of the boundary's `body:`/`output:` types against the declared shield's `compliance:` — NOT the `!compliance.is_empty()` presence check §111 F16 condemned. The endpoint's own `compliance:` list does NOT satisfy it: a label is not a control. The §111 diagnosis was right that the law existed only for `component`; what it missed is that the promise was VACUOUS, not weak — the same shape `lease` had before §113 gave it a subject)" }),
    ("component", Partial { gap: "§111 — the compile-time shield-coverage law over regulated κ IS genuinely enforced (a real set difference). But the component renders NOTHING; the README itself defers the renderer" }),
    ("view", Partial { gap: "§111 — only referential integrity is checked. No `route` check, no session-typed-reactivity check, and it renders nothing" }),
    // ── Enterprise I/O (Fases 80–85)
    ("cache", Real { proof: "cache_runtime — cacheability derives from the type system's `effects: pure` proof (§85)" }),
    ("voice", Unaudited),
    ("shell", Unaudited),
    ("path rewrite", Unaudited),
    ("PASETO", Unaudited),
    // ── Session types (§41)
    ("socket", Real { proof: "tests/fase111_i_socket_served.rs (§111.i — the OSS server now SERVES the session-typed WebSocket at GET /ws/{socket}, and enforces the protocol the adopter DECLARED: the missing SessionType compiler lands in session_runtime::compile. An unresolvable protocol is REFUSED, never substituted — enterprise used to hand every socket a hardcoded chat schema, so a protocol proven dual at compile time had a different one enforced at runtime)" }),
    ("send T", Real { proof: "the session algebra's send, carrying a payload type" }),
    ("receive T", Real { proof: "the session algebra's receive" }),
    ("select {ℓᵢ:…}", Real { proof: "session.rs Select — labelled internal choice" }),
    ("branch {ℓᵢ:…}", Real { proof: "session.rs Branch — labelled external choice" }),
    ("backpressure: credit(k)", Real { proof: "session_runtime credit window + the PCC credit-positivity witness" }),
    ("reconnect: cognitive_state", Unaudited),
    // ── §Fase 114.z — the classifications the badge-only gate never asked for.
    //
    // Twenty-two primitives were part of the public promise (registered,
    // corpus-documented, or discussed in the README at length — `step` 136
    // mentions, `type` 79, `document` 25, `budget` 16) and had NO entry here,
    // because the old gate enumerated only the README's `<code>` badges. A
    // gate that guards only the badges guards the packaging. The registry is
    // the enumeration source now (`is_advertised`), and every one of these is
    // classified with §111's own discipline: Real only with a citable gate,
    // Partial only with a named gap, Unaudited where nobody has looked — an
    // unchecked box is not a passing one.
    //
    // The language core — exercised by essentially every gate in both repos.
    ("step", Real { proof: "flow_dispatcher::pure_shape — the step executor every flow runs through (the same shared seam §114's channel guards read)" }),
    ("run", Real { proof: "§65 unified executor — runner + execute_server_flow bind flow+persona+context and execute; every fase gate that runs a program runs through it" }),
    ("type", Real { proof: "type_checker (structural + refinement validation) + store_schema (§38 — declared shapes pinned to SQL column types)" }),
    ("json", Real { proof: "tests/fase73_c_json_accessors.rs + tests/fase73_e_json_lens.rs (§73 — total navigation; honest fail-closed accessor semantics pinned in axon-rs orchestration tests)" }),
    // The audit chain + π-calc channel family.
    ("ledger", Unaudited),
    ("channel", Partial { gap: "§114.z — durable persistence (§74 outbox) and the σ-shield egress gate (§114, tests/fase114_channel_shield_egress.rs) are proven; `qos:`/`lifetime:` enforcement is unaudited" }),
    ("emit", Real { proof: "tests/fase114_channel_shield_egress.rs (§114 — the channel's shield scans on EVERY path by IR-resolution; Reject fails closed before any routing) + §74 durable outbox delivery" }),
    ("publish", Unaudited),
    ("discover", Unaudited),
    // Governance operators.
    ("budget", Real { proof: "tests/fase114_a_budget_governs_the_canonical_path.rs (§114.a — the ceiling binds on the canonical `use Tool(…)` path; before, the field was README prose that governed nothing)" }),
    ("window", Unaudited),
    ("cors", Unaudited),
    // The governed-egress trio (§105 · §99/§106 · §110) — none was ever badged.
    ("document", Real { proof: "axon-T916 (§99 — compile-time-validated, byte-deterministic OOXML synthesis; the assertion-laundering barrier refuses a sub-believe value in an assertive slot)" }),
    ("deliver", Real { proof: "delivery.rs + axon-T920 (§105 — provenance travels or the delivery refuses; first top-level egress integrated in the executor, D105.7(B))" }),
    ("notify", Real { proof: "notification.rs + axon-T933/T934/T935 (§110 — carried lineage or refusal; at-most-once-per-window attention ledger across replicas, ENT mig 033)" }),
    // Credentials + custody verbs.
    ("credential", Real { proof: "tests/fase92_mint_runtime.rs (§92 — grants ⊆ minter, TTL ≤ 24h; axon-T893–T896)" }),
    ("mint", Real { proof: "tests/fase92_mint_runtime.rs (§92 — fail-closed without a minter port; the raw bearer is shown once and never persisted, axon-T896)" }),
    ("rotate", Real { proof: "tests/fase94_custody_runtime.rs (§94 — rotation_without_revelation: ENUMERATE+ROTATE(CAS)+USE; no term evaluates to a secret value)" }),
    // Session types + quantum bridge.
    ("upstream", Real { proof: "tests/fase80_d_upstream_e2e.rs (§80 — config-resolved dial, declared auth, credit flow-control against a real WS server)" }),
    ("quant", Real { proof: "tests/fase111_d_quant_wired.rs (§111.d — E = ⟨ψ|M|ψ⟩ by real arithmetic; the body RUNS; `depth:` is refused rather than fabricating the physics)" }),
    ("observable", Real { proof: "tests/fase111_d_quant_wired.rs (§111.d — the DECLARED observable is resolved and measured; no resolvable M ⇒ refusal, because an expectation without an observable is a category error)" }),
    // Symbolic differentiation.
    ("grad", Real { proof: "tests/fase109_grad_runtime.rs (§109 — SYMBOLIC differentiation at compile time; the gradient IS IR under the PCC GradientSoundness witness; the runtime only evaluates)" }),
];

/// **The ratchet.** Every advertised name whose runtime is
/// [`RuntimeStatus::NotImplemented`] — i.e. every promise the README makes today
/// that the code does not keep.
///
/// This list may only ever **shrink**. Adding a name to it requires editing this
/// file, which is the point: a new unkept promise cannot land quietly. Removing
/// one means you either implemented it or retracted it from the README — both
/// good outcomes, and both must happen in the same PR as the deletion.
///
/// §111 inherited every one of these. They are the fase's open debt, and now the
/// compiler holds the ledger.
pub const KNOWN_DEBT: &[&str] = &[
    // Tier 4 — implement or retract.
    // `compute` LEFT this ledger in §111.f: it is now a real pure function over
    // the §70 expression language. This is the ratchet turning — the only
    // direction it turns.
    // `hibernate` LEFT this ledger in §119.d (2026-08-07): the walk halts,
    // the continuation parks under the published SHA-256 id, emit wakes it,
    // and lazy expiry refuses lateness. Fifth turn of the ratchet.
    //
    // `mandate` LEFT this ledger in §119.b (2026-08-07): the enforcement loop
    // runs through production dispatch, fails closed, and its name resolving
    // to nothing is a refusal — the exact call that used to be the identity
    // passthrough is now the F18 regression test. Second turn of the ratchet.
    //
    // `lambda` and `ots` LEFT in §119.c (2026-08-07), the same day: the ΛD
    // engine reached the production dispatch path (Real), and ots's dispatch
    // layer + registry hook became real with an honestly-named gap (Partial:
    // no OSS transformer registers by default — a generic default would be
    // the identity lie F18 named). Third and fourth turns.
    // The Cognitive-I/O block.
    //
    // §112 PAID SIX OF THESE. `observe` · `ensemble` · `immune` · `reflex` · `heal` ·
    // `reconcile` are now driven by the CognitiveIoSupervisor and each cites a gate
    // that proves it runs through the REAL deploy path.
    //
    // My §111 diagnosis of this family was WRONG in a way worth remembering: I called
    // it a language-design problem ("no verb can reach them"). The language was
    // already complete — the declarations form a dataflow graph and reference each
    // other, and the kernels took the compiled IR directly. **Nobody had ever built
    // the loop.**
    //
    // §113 PAID TWO MORE. `resource` and `lease` left this ledger.
    //
    // `resource` now governs what actually runs: an `axonstore { resource: Db }`
    // DERIVES its DSN and its POOL SIZE from the resource (`capacity: 20` ⇒ twenty
    // connections — before §113 that field was read by zero lines of code in either
    // repository while every pool sat at a hardcoded 10), and `lifetime` decides how
    // many holders may name it (`axon-T945`).
    //
    // `lease` was the sharpest case in the whole line, and worth remembering: its
    // kernel was never broken. `LeaseKernel` had `acquire`, `use_token`, `release`,
    // the CT-2 Anchor Breach and all three `on_expire` policies, and its unit tests
    // passed for years. **IT HAD NO SUBJECT.** The README promised that using a
    // resource after expiry is a breach — in a language where a flow could not USE a
    // resource at all. A guarantee about using a thing that cannot be used is not a
    // weak guarantee; it is a VACUOUS one. It cannot be violated, and so it cannot be
    // kept. §113 made the store operation the use, and the breach finally fires.
    //
    // ── AND `fabric` STAYS. ────────────────────────────────────────────────────
    //
    // This is the ratchet doing its job on the fase that was in a position to fudge
    // it. `within:` gave `fabric` subjects, and Separation-Logic disjointness is now
    // unrepresentable by construction (one field ⇒ a resource cannot be in two
    // fabrics). It would be *easy* to call that Real.
    //
    // But `provider` / `region` / `zones` are consumed by NOTHING at runtime — in
    // `LiveHandler::provision` the parameter is literally `_fabrics`, underscored,
    // and that function refuses anyway. A `fabric` still governs nothing that runs.
    //
    // Calling it Real here is exactly the lie §111.g exists to prevent, told by the
    // person who wrote the gate. The ledger only shrinks by what is PROVEN.
    // §Fase 119.e (2026-08-07) — `fabric` LEFT this ledger, and with it the
    // ledger itself: provider/region now refuse an impossible substrate and an
    // unsatisfiable obligation at compile time, and reach the running channel
    // and its audit row. SIXTH AND LAST TURN.
    //
    // ═══════════════════════════════════════════════════════════════════
    //  THE RATCHET IS EMPTY.
    //
    //  §111 opened it with 22 advertised primitives that were not real, and
    //  built this list as the ledger of what the language owed. §111.f paid
    //  `compute`. §112 paid six of the Cognitive-I/O block. §113 paid
    //  `resource`/`lease`. §114 paid the governed channel. §119 paid the last
    //  five: `mandate` (b), `lambda` + `ots` (c), `hibernate` (d), `fabric`
    //  (e) — all on 2026-08-07.
    //
    //  An empty KNOWN_DEBT does NOT mean every primitive is Real. Several are
    //  `Partial` with the gap NAMED in `advertised.rs` — which is the whole
    //  point of the distinction: `Partial` is a bounded, published limit an
    //  adopter can read and plan around; `NotImplemented` was an advertised
    //  promise with nothing behind it. There is none of the latter left.
    //
    //  KEEP THIS LIST EMPTY. Adding a name back means the language advertised
    //  something it does not have — which is exactly the state §111 existed to
    //  end, and the gate below will make that addition loud.
    // ═══════════════════════════════════════════════════════════════════
];

/// Look up what an advertised name's runtime actually does.
///
/// **No default arm, deliberately.** An unknown name returns `None`, and the
/// gate below turns that into a build failure — so a primitive cannot be added
/// to the README without someone stating what it does.
pub fn status_of(name: &str) -> Option<RuntimeStatus> {
    ADVERTISED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;

    /// The repo root, from `axon-frontend/`.
    fn repo_root() -> &'static Path {
        Path::new("..")
    }

    /// Parse the README's header badge block — the public promise, as published.
    fn readme_advertised() -> Vec<String> {
        let readme = std::fs::read_to_string(repo_root().join("README.md"))
            .expect("the public README must be readable — it is an INPUT to this build");
        let start = readme
            .find("<!-- Cognition primitives -->")
            .expect("the README's primitive badge block must be findable");
        let end = readme[start..]
            .find("</p>")
            .expect("the badge block must terminate")
            + start;
        let block = &readme[start..end];

        let mut out = Vec::new();
        let mut rest = block;
        while let Some(i) = rest.find("<code>") {
            rest = &rest[i + 6..];
            let j = rest.find("</code>").expect("unterminated <code> badge");
            out.push(rest[..j].to_string());
            rest = &rest[j + 7..];
        }
        out
    }

    /// **LAW 1 — the README is a build input.**
    ///
    /// Every badge it advertises must be classified here. A primitive cannot be
    /// added to the public promise without someone stating, on the record, what
    /// its runtime actually does.
    #[test]
    fn every_advertised_name_is_classified() {
        let missing: Vec<String> = readme_advertised()
            .into_iter()
            .filter(|n| status_of(n).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "the README advertises {} name(s) with NO entry in `ADVERTISED`:\n  {}\n\n\
             You cannot advertise a primitive without stating what its runtime does. \
             That omission is exactly how §111 happened: `warden` and `quant` had a README \
             badge, a registry entry, a parser production AND a dispatch arm — and were no-ops.",
            missing.len(),
            missing.join("\n  ")
        );
    }

    /// **LAW 1b (§114.z) — the registry is the enumeration source.**
    ///
    /// Every `PRIMITIVE_REGISTRY` entry with `is_advertised: true` must be
    /// classified here. This is the law the badge-only gate lacked: `budget`
    /// was discussed in the README seventeen times, badged zero, tracked in NO
    /// table — and its dead runtime (§114.a) survived every green build.
    #[test]
    fn every_advertised_registry_primitive_is_classified() {
        let missing: Vec<&str> = crate::primitive_registry::PRIMITIVE_REGISTRY
            .iter()
            .filter(|i| i.is_advertised)
            .map(|i| i.name)
            .filter(|n| status_of(n).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "{} registry primitive(s) are `is_advertised` with NO entry in `ADVERTISED`:\n  {}\n\n\
             The registry is the enumeration source (§114.z). A primitive that is part of \
             the public promise cannot exist without someone stating, on the record, what \
             its runtime does. That omission is exactly how `budget` (§114.a) happened.",
            missing.len(),
            missing.join("\n  ")
        );
    }

    /// **LAW 1b, visibility half (§114.z)** — an advertised primitive must be
    /// VISIBLE in the promise: every `is_advertised` registry entry carries a
    /// `<code>` badge in the README's header block. Classification without
    /// visibility would let the promise live only inside this file.
    #[test]
    fn every_advertised_registry_primitive_is_badged() {
        let badges: HashSet<String> = readme_advertised().into_iter().collect();
        let unbadged: Vec<&str> = crate::primitive_registry::PRIMITIVE_REGISTRY
            .iter()
            .filter(|i| i.is_advertised)
            .map(|i| i.name)
            .filter(|n| !badges.contains(*n))
            .collect();
        assert!(
            unbadged.is_empty(),
            "these `is_advertised` registry primitives have NO `<code>` badge in the README: \
             {unbadged:?}\nAdd the badge (and its classification) or flip `is_advertised` — \
             deliberately, in the same PR."
        );
    }

    /// **LAW 1b, negative half (§114.z)** — an `is_advertised: false` registry
    /// primitive must be neither badged nor classified. If either exists, the
    /// polarity flag is lying about the promise.
    #[test]
    fn non_advertised_registry_primitives_are_neither_badged_nor_classified() {
        let badges: HashSet<String> = readme_advertised().into_iter().collect();
        for info in crate::primitive_registry::PRIMITIVE_REGISTRY {
            if info.is_advertised {
                continue;
            }
            assert!(
                !badges.contains(info.name),
                "`{}` is `is_advertised: false` but carries a README badge — the flag lies",
                info.name
            );
            assert!(
                status_of(info.name).is_none(),
                "`{}` is `is_advertised: false` but is classified in ADVERTISED — \
                 either advertise it deliberately or remove the classification",
                info.name
            );
        }
    }

    /// The reverse: no zombie entries. If a name leaves the README (retracted),
    /// it must leave this table too — otherwise the table slowly becomes a
    /// museum and stops describing the promise.
    #[test]
    fn every_classified_name_is_still_advertised() {
        let advertised: HashSet<String> = readme_advertised().into_iter().collect();
        let zombies: Vec<&str> = ADVERTISED
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !advertised.contains(*n))
            .collect();
        assert!(
            zombies.is_empty(),
            "these names are classified here but are NO LONGER advertised in the README: {zombies:?}\n\
             Retracting a primitive means deleting it from BOTH."
        );
    }

    /// **LAW 2 — the ratchet.** Nothing advertised may be `NotImplemented`
    /// unless it is in `KNOWN_DEBT`, and that list may only shrink.
    ///
    /// This is the law that makes §111's findings durable. A *new* unkept promise
    /// is a red build. The existing ones are a ledger the compiler holds — not a
    /// paragraph in a document nobody rereads.
    #[test]
    fn no_new_unkept_promises() {
        let debt: HashSet<&str> = KNOWN_DEBT.iter().copied().collect();
        let undeclared: Vec<&str> = ADVERTISED
            .iter()
            .filter(|(_, s)| s.is_unimplemented())
            .map(|(n, _)| *n)
            .filter(|n| !debt.contains(*n))
            .collect();
        assert!(
            undeclared.is_empty(),
            "these advertised primitives are NotImplemented and are NOT in KNOWN_DEBT: {undeclared:?}\n\n\
             You have advertised a promise the code does not keep. Either implement it, retract it \
             from the README, or (if you are knowingly shipping the gap) add it to KNOWN_DEBT with \
             its finding — so it is a LEDGER ENTRY and not a lie."
        );
    }

    /// The ratchet's teeth: `KNOWN_DEBT` may only shrink. Bump this DOWN when a
    /// debt is paid; a build that needs it bumped UP is telling you something.
    #[test]
    fn the_debt_ledger_only_shrinks() {
        assert!(
            KNOWN_DEBT.len() <= 14,
            "KNOWN_DEBT grew to {} — the ratchet only turns one way. §111 inherited 14 unkept \
             promises; a new one is not an entry to add, it is a bug to fix.",
            KNOWN_DEBT.len()
        );
    }

    /// Every debt entry must actually BE a debt — no padding the ledger with
    /// names that are fine, which would let a real one hide among them.
    #[test]
    fn the_debt_ledger_has_no_padding() {
        for name in KNOWN_DEBT {
            let s = status_of(name)
                .unwrap_or_else(|| panic!("KNOWN_DEBT names `{name}`, which is not advertised"));
            assert!(
                s.is_unimplemented(),
                "`{name}` is in KNOWN_DEBT but its status is {s:?} — if it was implemented or \
                 retracted, delete its ledger row in the same PR."
            );
        }
    }

    /// **LAW 3 — a claim of reality must cite the gate that proves it.**
    ///
    /// And where the proof names a test file, that file must exist. A `Real`
    /// claim pointing at nothing is just a nicer-sounding assertion — which is
    /// precisely what the README's feature table was.
    #[test]
    fn every_real_claim_cites_a_proof_that_exists() {
        for (name, status) in ADVERTISED {
            if let Real { proof } = status {
                assert!(
                    !proof.trim().is_empty(),
                    "`{name}` claims Real with an empty proof"
                );
                if let Some(path) = proof.split_whitespace().find(|t| t.starts_with("tests/")) {
                    let full = repo_root().join("axon-rs").join(path);
                    let alt = repo_root().join("axon-frontend").join(path);
                    assert!(
                        full.exists() || alt.exists(),
                        "`{name}` claims Real and cites `{path}`, which does not exist. \
                         A proof that cannot be run is not a proof."
                    );
                }
            }
        }
    }

    /// Partial / FailsClosed entries must NAME the gap or the diagnostic. An
    /// unnamed gap is how a documented compromise rots into an undocumented lie.
    #[test]
    fn every_partial_and_failsclosed_names_its_gap() {
        for (name, status) in ADVERTISED {
            match status {
                Partial { gap } => assert!(
                    !gap.trim().is_empty(),
                    "`{name}` is Partial with no stated gap — name it, or it will rot"
                ),
                FailsClosed { diagnostic } => assert!(
                    !diagnostic.trim().is_empty(),
                    "`{name}` FailsClosed with no diagnostic named"
                ),
                _ => {}
            }
        }
    }

    /// The unaudited population is pinned. §111 could not reach everything, and
    /// says so; but the number may only go down. An unchecked box is not a
    /// passing one.
    ///
    /// §114.z RE-BASELINED this pin 18 → 23 — not because auditing went
    /// backwards, but because the DENOMINATOR grew: the census forced a
    /// classification for 22 primitives the badge-only gate never asked about,
    /// and five of them (`ledger` · `publish` · `discover` · `window` ·
    /// `cors`) are honestly Unaudited — their existing gates pin grammar/IR,
    /// not runtime behaviour, and claiming more without looking would be the
    /// exact lie this file exists to prevent. From 23, the only direction is
    /// down.
    #[test]
    fn the_unaudited_population_only_shrinks() {
        let n = ADVERTISED
            .iter()
            .filter(|(_, s)| matches!(s, Unaudited))
            .count();
        assert!(
            n <= 23,
            "the Unaudited population grew to {n} — §111 left 18, §114.z's census widening \
             re-baselined at 23. Auditing is the only direction."
        );
    }
}
