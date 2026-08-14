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
//! 2. **Nothing advertised may be [`RuntimeStatus::NotImplemented`] or
//!    `RuntimeStatus::Unwired`** … except what is written down in
//!    [`KNOWN_DEBT`], which is a **ratchet**: it may only shrink. A *new*
//!    unkept promise is a red build. That is the whole point — §111's debt
//!    becomes a ledger the compiler enforces, instead of a finding in a
//!    document nobody rereads.
//!
//!    §122.a added `Unwired` to this law rather than beside it (D122.6). The
//!    two states differ in what the fix COSTS — `NotImplemented` needs an
//!    engine, `Unwired` needs a call site — and not at all in what the adopter
//!    experiences. If reclassifying could move a row out from under the
//!    ratchet, the ratchet would measure vocabulary instead of debt.
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
    /// §Fase 122.a — **the engine EXISTS, is tested, and no production path
    /// reaches it.**
    ///
    /// The state this table was missing, and its absence is what produced the
    /// one false `Real` in it. The other five describe degrees of FULFILMENT —
    /// how close the runtime comes to the summary. None described
    /// REACHABILITY. `cache` was attested `Real` because its engine is complete
    /// and correct (content-addressed keys, single-flight, TTL with jitter,
    /// LRU, errors never cached, all with passing tests) and, of the five,
    /// `Real` was the only state that meant *"the engine is good"*. It was not
    /// dishonesty; it was a taxonomy with no box for the truth.
    ///
    /// `engine` names what exists, so a reader can tell a **cable job** from a
    /// **build job** — an `Unwired` row is not a missing feature, it is a
    /// missing call site.
    ///
    /// **It owes a [`KNOWN_DEBT`] entry, exactly like `NotImplemented`**
    /// (D122.6, ratified 2026-08-13). To the adopter the two are
    /// indistinguishable: the primitive does not do what the summary says, and
    /// nothing tells them. Letting `Unwired` dodge Law 2 would make
    /// reclassification an escape hatch from the ratchet — which is the exact
    /// species of drift this file exists to stop. The debt did not grow when
    /// these rows landed; it was **uncovered**. A ledger that stays empty
    /// because a row lies is worth nothing.
    Unwired { engine: &'static str },
    /// §111 has not verified this one. Advertised on trust, and counted: the
    /// unaudited population is pinned below and may only shrink.
    Unaudited,
    /// §Fase 122.a — **`Real`, re-attested under Law 4**: the claim names the
    /// `.axon` FIXTURE a published program could write, and the GATE that
    /// walks that fixture to dispatch.
    ///
    /// Law 4 has been written in this file since §119.f.8 and never ran. The
    /// mechanism of that erosion is the shape of [`Self::Real`]: Law 3 is
    /// conditional (*"if the proof names a test file"*) and `proof` is free
    /// prose, so a citation that names no file evaluates the check to vacuous
    /// truth. `cache`'s proof — *"cacheability derives from the type system's
    /// `effects: pure` proof"* — cited a COMPILE-TIME property as evidence of a
    /// runtime, and passed. Nothing had to be forgotten; a sentence was enough.
    ///
    /// A free-string field breeds an imaginary catalog — the third time this
    /// project has recorded that shape. The fix is the same as always: a closed
    /// pair the build can verify. `fixture` must exist and be `.axon`;
    /// `gate` must exist and must READ that fixture (§122.b).
    ///
    /// `Real { proof }` remains for rows not yet re-attested. Its population is
    /// pinned and **may only shrink**; §122.b drives it to zero. Migrating the
    /// shape and re-attesting forty rows in one commit would mix a mechanical
    /// refactor with forty judgements, and that mixture is where a convenience
    /// `Real` gets in (D122.2).
    Attested {
        /// Repo-relative path to an `.axon` source file a published program
        /// could have written.
        fixture: &'static str,
        /// Repo-relative path to the test that compiles `fixture` and drives it
        /// to dispatch.
        gate: &'static str,
    },
}

impl RuntimeStatus {
    /// Literally `NotImplemented` — nothing else.
    pub fn is_unimplemented(self) -> bool {
        matches!(self, RuntimeStatus::NotImplemented { .. })
    }

    /// **The predicate Law 2 turns on** (§122.a).
    ///
    /// Both states owe a ledger entry, because both present the adopter with
    /// the same thing: a promise the runtime does not keep, silently.
    /// `NotImplemented` has no engine; `Unwired` has one nothing calls. The
    /// distinction tells a maintainer what the fix costs; it tells an adopter
    /// nothing, and Law 2 is written for the adopter.
    pub fn owes_a_debt_entry(self) -> bool {
        matches!(
            self,
            RuntimeStatus::NotImplemented { .. } | RuntimeStatus::Unwired { .. }
        )
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
    // §Fase 122.b — was the bare module name `flow_dispatcher::pure_shape`, one
    // of the 38 rows Law 3 never checked. The fixture below was added by §122.b
    // and declares a `persona` alongside the `mandate` that governs its output.
    ("persona", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/13_compliance_persona_mandate.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("intent", Unaudited),
    // §Fase 122.b — FIRST TWO RE-ATTESTATIONS. Both used to cite a module
    // (`flow_dispatcher::dispatch_node`, `flow_dispatcher::pure_shape`) — an
    // ENGINE citation, the shape §119.f.8 proved worthless, and one Law 3 never
    // checked because it names no test file. The parity corpus is the honest
    // replacement: `fase33z_d_parity_corpus` reads each `.axon` fixture from
    // disk, drives it through BOTH execution paths (sync runner + async
    // dispatcher) under the stub backend, and pins the results byte-equal. The
    // input is source an adopter could have written; the gate walks it to
    // dispatch. That is Law 4 satisfied, not asserted.
    ("flow", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/banking/01_canonical_loan_decision.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
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
    // §Fase 122.b — §119.f.8's semantic gates (`fase119_f8_reason_block`,
    // `fase119_f8_reason_prompt_is_built`) still hold and still run; what they
    // could not do is satisfy Law 4, because they build their source inline and
    // no file on disk holds it. The parity corpus does: a published-shaped
    // program carrying a `reason` block, executed on both paths.
    ("reason", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/09_content_moderation_reason.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — was the bare engine name `anchor_checker`. The fixture below
    // was ADDED by §122.b for exactly this: every gate proving `anchor`, `probe`
    // and `weave` built its source inline, so no file on disk held a program
    // using them and the rows could only cite an engine. The corpus gate
    // discovers it by directory walk and drives it through both paths.
    ("anchor", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/12_legal_intake_probe_weave_anchor.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — was the bare engine name `pure_shape::run_refine`.
    ("refine", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/18_refine_grad_compute.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("memory", Real { proof: "cognitive::run_remember / run_recall (PEM write-through)" }),
    // §Fase 122.b — was the bare engine name `lambda_tools::dispatch_use_tool_real`.
    // The gate that DID read a file for `tool` (`fase54_c_tool_dispatch_example`)
    // only parses and type-checks the canonical example — the front half. This
    // fixture is dispatched.
    ("tool", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/15_tool_budget_governance.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — see the `anchor` note above; same fixture, same gate.
    ("probe", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/12_legal_intake_probe_weave_anchor.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 119.f.9 — see rule 4 above: this proof used to name the handler
    // alone, and the handler was reachable only from a braced form no published
    // block writes. Worse, it sent the model the source NAMES, so even when
    // reached it synthesised over identifiers instead of outputs.
    // §Fase 122.b — §119.f.9's gates (`fase119_f9_weave_statement`,
    // `fase119_f9_weave_prompt_is_built`) still hold: grammar → IR → resolved
    // prompt, over the engine `pure_shape::run_weave`. They build their source
    // inline, so they cannot satisfy Law 4. Same fixture as `anchor`/`probe`.
    ("weave", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/12_legal_intake_probe_weave_anchor.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — was `pure_shape::run_validate`, a bare engine name and one
    // of the 38 rows Law 3 never checked (no `tests/` token ⇒ vacuous). §121's
    // own gates prove the scoring; this proves a published program reaches it.
    ("validate", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/04_capability_mediation_validate.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("context", Unaudited),
    // ── Epistemic scopes
    ("know", Unaudited),
    ("believe", Unaudited),
    ("speculate", Unaudited),
    ("doubt", Unaudited),
    // ── Concurrency & continuation
    // §Fase 122.b — real fan-out via `join_all` (§65) stays the mechanism; the
    // citation now names a program that uses it and the gate that runs it.
    ("par", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/banking/04_par_concurrent_checks.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
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
    // §Fase 122.a — both were `Unaudited`, which was honest in §111 and is no
    // longer: they HAVE been audited now (2026-08-13), and the finding is
    // reachability, not uncertainty. `Unwired` is strictly more informative.
    ("savant", Unwired { engine: "holograph::ReferenceHolographCodec (HRR, FFT, dim-capped) + inference::ReferenceInference (VFE/EFE) + topology::ReferenceTopology (Betti/PHC) — three tested engines with ZERO callers outside their own tests. `cognition.depth:` is read only by the PCC (axon-T876, catalog validity) and by `main.rs` to print it; HOLOGRAPH_DIM_CAP is never exercised. Wiring active inference is its own fase, not a cable" }),
    ("synth", Unwired { engine: "synth::DenyByDefaultSynth — the OSS reference, which refuses BY DESIGN (§87.j: OSS never executes synthesised code). No mount point exists, so no flow reaches it. NOTE: cabling it in OSS yields FailsClosed, not Real — the enterprise Extism/gVisor executor is what makes it Real, and it needs the sandbox reachable at all" }),
    // §Fase 122.b — §111.c's nine joints all still run, and all nine hand-build
    // `IRScope`/`IRWarden` in Rust: a proof of the HANDLER. The gate now also
    // compiles a fixture through the real pipeline and dispatches the node the
    // COMPILER produced, against the same `ReferenceStaticWarden` the runner
    // mounts. Perturbing the DECLARATION kills it three ways — a target outside
    // `targets:`, an empty `approver:`, and `depth: live_network` above the OSS
    // ceiling — so the authorization envelope really comes from the source.
    ("warden", Attested {
        fixture: "tests/fixtures/fase122_b_warden/authorized_audit.axon",
        gate: "tests/fase111_c_warden_wired.rs",
    }),
    // §Fase 122.b — the scope catalog under test is the one the compiler emitted
    // from the fixture's `scope InternalAudit { … }`, not a Rust literal. See the
    // `warden` note above; the same three perturbations kill this citation.
    ("scope", Attested {
        fixture: "tests/fixtures/fase122_b_warden/authorized_audit.axon",
        gate: "tests/fase111_c_warden_wired.rs",
    }),
    // ── Effects & streaming
    // §Fase 119.n — `Real` → `Partial`, and the DOWNGRADE is the honest move.
    //
    // The old proof read `tests/fase111_e_stream_runs.rs (§111.e — the body is
    // parsed, lowered and EXECUTED)`. Every word of it was true, and it violated
    // law 4 above: §111.e gave `StreamBlock` a `body: Vec<FlowStep>` and proved
    // THAT runs — but no published block and no paper writes that shape. The
    // specified surface (`fase_23` §3.7, whose D8 promises "backward compat 100%,
    // cero cambios en `.axon` source files") is `stream<T> { on_chunk on_complete }`,
    // and README block 15 publishes exactly it. Measured: at flow level the
    // published body was a hard parse error; in a step body the whole block was
    // discarded, so block 15's `step Stream` dispatched with `pix_ops=0`,
    // `ask=""`, `output=""` — an EMPTY step whose `Stream.output` the next step
    // reasoned over, and which had left the §117 ledger on the strength of
    // compiling. The `reason` defect one fase later, in the table that recorded it.
    ("stream", Partial { gap: "§119.n made the PUBLISHED surface real: `stream<T> { on_chunk: … on_complete: … }` parses at both the step-body and flow positions (it was a hard parse error at flow level and a SILENT DISCARD in a step body), the `<T>` chunk type reaches the IR instead of being eaten by the argument-skip loop, and each arm is a step body so its `output:` lands. BOTH chunk sources are wired: a streaming TOOL bound with `apply:` (README block 15's own shape — the handler rides `unified_stream_handler`, so it sees the chunks the declared `backpressure:` policy let through, never the raw source), and the step's own generation (fase_23 §3.7's `stream<Token>` case, hooked into `drain_direct`). Neither has a private drain, so a handler cannot drift from the stream it handles. `on_complete` binds the accumulation as `complete` and its output becomes the step's. An `apply:` that is not a registered streaming tool is REFUSED, never demoted to the LLM path — demoting would stream something the author never named (§112). §119.n.3 added the THIRD arm, `on_error:`, run when the SOURCE fails with the failure bound as `error`; the step then completes with that arm's output (a recovery the author declared, not a swallowed error) and `on_complete` does NOT run, because a stream that broke did not close. It is deliberately NOT a catch-all: a failure raised by the author's own `on_chunk` is tagged and excluded, since a broken handler that silently catches itself and reports the stream as healthy is worse than one that crashes; cancellation is not a failure either. `on_chunk` is SEQUENTIAL and INTERLEAVED — one run per chunk, during the stream, gated by a wire-trace test that a buffered implementation would fail (it passes every other assertion in the file). Gates: axon-frontend/tests/fase119_n_stream_handlers.rs (12, source→AST→IR) + axon-rs/tests/fase119_n_stream_dispatch.rs (16, source→dispatch, incl. README block 15 VERBATIM executing); 7/7 mutations killed. THE GAP: `stream<τ>` is NOT desugared to `effect _StreamBuiltin<T>` + `handle … in …`, so fase_23's D8 claim of one-shot delimited continuations under this primitive is still future work and the algebraic `perform Stream.Yield` bridge stays orthogonal. A handler slower than the producer backpressures the whole step — correct, but a performance property an adopter should be told about. And `on_error` cannot RESUME the stream: it runs once, after the source is already gone" }),
    // §Fase 122.b — `parse_effect_row` + the type checker are the engines; §85's
    // cacheability derivation reads the `effects: pure` proof. Verified on this
    // fixture by perturbation: `effects: <not_an_effect>` fails it with the
    // closed catalog named — "Valid: io, network, pure, random, storage, stream,
    // trust, sensitive, legal, ots, web" — so the row really is decided here.
    ("effects", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/19_run_effects_compliance.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 120 — the algebraic-effect system. `effects` (plural, above) is a
    // DIFFERENT thing that shares a word: the `effects: <io, network>` row on a
    // tool declaration. These three are Plotkin/Pretnar handlers.
    //
    // ⚠️ What was measured before §120, and why it is the sharpest case law 4
    // has produced. `axon-rs/src/effects/` is a real 590-line FSM — handler
    // stack, one-shot delimited continuations (D2), forward-to-outer (D12) —
    // shipped in §Fase 23 with 49 green tests. It was never `Unaudited` and
    // never in `KNOWN_DEBT`, because it was never ADVERTISED: no README badge,
    // no registry entry, nothing to classify. And so nobody ever asked the
    // question that mattered, which is not *"does the engine work?"* (it did)
    // but *"can any program reach it?"*. The answer was no, in the most total
    // way this repo has recorded: ZERO lexer tokens, no AST, no IR emission,
    // and `EffectRuntime::new()` with both call sites inside `#[cfg(test)]`.
    // An unadvertised subsystem is invisible to every gate in this file.
    ("effect", Real { proof: "axon-frontend/tests/fase120_a_effect_grammar.rs (29, source→AST→IR: the declaration lands in IRProgram::effects, the field §23 created and nothing ever wrote to) + axon-rs/tests/fase120_c_effect_dispatch.rs (15, source→dispatch)" }),
    ("handle", Real { proof: "axon-rs/tests/fase120_c_effect_dispatch.rs §1-§9 — source→dispatch: the `in { … }` body EXECUTES, a clause body executes, `resume` returns control to the perform site, a clause with no resume aborts (fase_23 §3.1's published `Done()` shape), an abort reaches the frame that OWNS it under nesting, `forward` delegates to the outer frame, and a clause binder does not leak into the continuation" }),
    ("perform", Real { proof: "axon-rs/tests/fase120_c_effect_dispatch.rs §2/§8/§10 (the clause sees the RESOLVED value; an unhandled perform FAILS CLOSED; a step-body perform runs AFTER generation over the step's own output) + axon-frontend/tests/fase120_a_effect_grammar.rs §4/§5 (D120.2 bare-name resolution, ambiguity refused naming both candidates, and D9 exhaustiveness INTERPROCEDURALLY at every entry point incl. axonendpoint)" }),
    ("@contract_tool", Unaudited),
    ("@csp_tool", Unaudited),
    // ── Knowledge navigation (PIX · MDN)
    ("pix", Real { proof: "pix_navigator (embeddings-free structural index)" }),
    ("navigate", Partial { gap: "§111 F11 — three REAL deterministic engines (MDN store-sourced, MDN in-memory, PIX), BUT with no indexable source in scope it falls back to an LLM prompt that INSTRUCTS the model to fabricate a provenance trail. The one live §108 left in the tree" }),
    ("drill", Partial { gap: "§111 — real subtree navigation when a source is in scope; degrades to a placeholder string otherwise" }),
    ("trail", Partial { gap: "§111 — reads the real breadcrumb `navigate` seeds; falls back to a placeholder when no navigate ran. Inherits F11: a trail harvested from the LLM fallback is confabulation wearing an audit's clothes" }),
    // §Fase 122.b — `mdn` (signed Epistemic PageRank, §62–§64) is the engine and
    // has its own unit coverage; this citation is the PATH: a published program
    // declaring a `corpus`, compiled from disk and executed.
    ("corpus", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/06_mdn_navigate_graph.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // ── Advanced cognition & trust
    ("psyche", Unaudited),
    ("ots", Partial { gap: "§119.c made the dispatch layer REAL: `apply_ots_to_target` (identity, §111 F18) is deleted; `run_ots_apply` resolves the declaration, consults the name-keyed `ots_registry` (shield_registry's shape — the hook that F18 said did not exist, now tested by registering a transformer and watching it fire), propagates transformer refusals, and REFUSES an unregistered name (identity under a transformation's name fabricates a result). Step-scoped `ots X on y -> b` elevates before generation. THE GAP: no OSS transformer registers by default — any generic default would be the identity lie again — so OSS `ots` refuses at runtime until the media pipeline (crate::ots) or an enterprise vertical registers; the paper's JIT synthesis pipeline remains future work" }),
    ("mcp", Unaudited),
    // §Fase 122.b — §119.b's account stays true and is worth keeping: the closed
    // loop runs through production dispatch via `mandate_engine.rs` (e = 1 − CSR
    // from `pem::semantic_validator`, PID from `pem::density_matrix`,
    // Converge(e,ε,N) with Breach fail-closed, declared-D premise discharge,
    // |C|=0 refused before any token), driven by `run_mandate_apply` + the
    // D119.4 step guard (buffered — attempts never stream). Tier 1 logit-bias
    // bans on the OpenAI-compat wire, Tier 2 CSP feedback everywhere. Gates:
    // mandate_engine (16), algebraic_handlers MandateApply incl. the F18
    // regression (an unresolved name REFUSES, never identity),
    // openai_compat logit_bias wire, fase119_b3_stability_band.rs,
    // fase119_b_mandate_grammar.rs — 9/9 dispatch-chain mutations killed.
    // None of those reads a file, so none could satisfy Law 4. This does.
    ("mandate", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/13_compliance_persona_mandate.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — §119.c's account stays true and is worth keeping here: the
    // ΛD engine (`lambda_data.rs`: ψ = ⟨T,V,E⟩, four invariants, compose with
    // c_out ≤ min(c_i); `lambda_runtime.rs`: Theorem 5.1 enforced at apply, only
    // raw data may carry c = 1.0) runs on the PRODUCTION dispatch path.
    // `apply_lambda_data` — the §111 F18 placeholder string — is deleted, an
    // undeclared lambda REFUSES, and the step-scoped form elevates BEFORE prompt
    // interpolation. Gates: the lambda_tools elevation suite incl. the F18
    // regression + fase119_c_apply_grammar.rs; mutations C1/C2/C5 killed.
    // The citation below is the one Law 4 asks for: source on disk → dispatch.
    ("lambda", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/08_rate_limiting_lambda.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // ── Deterministic compute
    // §Fase 119.o — the proof now cites a gate whose INPUT is `.axon` source
    // (law 4). The §111.f citation stayed, because it is still true and still
    // the engine's own gate — but on its own it was the §119.n shape: it proved
    // `compute` evaluates a §70 expression while the form every published block
    // writes (`input:` / `output:` / `logic { let … return }`) did not parse at
    // all. Three of the four README computes were in the §117 ledger for it.
    // §Fase 122.b — `fase111_f_compute_real.rs` stays the semantic gate (§111.f
    // deleted the placeholder string that a downstream step consumed as if it
    // were a number). It builds its source inline, and until this fixture NO
    // `.axon` in the repository declared a `compute` at all — the third such
    // primitive, with `json` and `budget`.
    ("compute", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/18_refine_grad_compute.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
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
        // §Fase 122.b — the readings that drive the immune's baseline come through this
    // `observe` declaration, sampling a source adapter the gate registers. A
    // refused observation feeds nothing downstream (deny-by-default).
    ("observe", Attested {
        fixture: "tests/fixtures/fase122_b_cognitive_io/immune_reflex_heal.axon",
        gate: "tests/fase112_d_immune_fires.rs",
    }),
    // §Fase 122.b — §112.e already deploys through the real path and measures
    // REAL Jaccard drift between the manifest's desired shape and the observed
    // world (before it, the loop compared the belief against itself and reported
    // 0.0 for every world). The fixture makes that program a file. Three
    // perturbations kill it: widening `tolerance:` past the measured drift,
    // changing `on_drift:` to `noop`, and shrinking the manifest — the last
    // proving the drift is computed against the DECLARATION, not a constant.
    ("reconcile", Attested {
        fixture: "tests/fixtures/fase122_b_cognitive_io/reconcile_drift.axon",
        gate: "tests/fase112_e_reconcile_drift.rs",
    }),
    ("lease", Real { proof: "tests/fase113_d_lease_breach_fires.rs (§113.d — post-expiry USE of a leased store is the CT-2 Anchor Breach, and all three `on_expire` policies are honoured. The kernel was NEVER broken: it had no SUBJECT. A flow could not USE a resource, so a guarantee about post-expiry use was VACUOUS — unviolatable, and therefore unkeepable. §113 made the store operation the use)" }),
    // §Fase 122.b — §112.c exercised `ensemble` ONLY as a refusal (an unsatisfiable
    // quorum must refuse the deploy, because a half-instantiated immune system is
    // worse than none — it looks like one). That law stays, and it left the path
    // the CLAIM is about — aggregating observations actually TAKEN — with no
    // program on disk. This fixture is the satisfiable case.
    //
    // Writing it surfaced a law worth knowing: the type checker refuses a Byzantine
    // ensemble over a single observation ("Byzantine quorum requires >= 2").
    // Byzantine agreement over one voter is not agreement, and the compiler says so
    // rather than deploying something that merely looks like a quorum.
    //
    // Perturbations that kill it: a quorum above the declared observations, and
    // pointing an observation at a source nobody registered — the latter proving
    // the aggregator counts only observations that were really taken.
    ("ensemble", Attested {
        fixture: "tests/fixtures/fase122_b_cognitive_io/ensemble_quorum.axon",
        gate: "tests/fase112_c_cognitive_io_deploy.rs",
    }),
    // §Fase 122.a — ADJUDICATED, because two different things in this codebase
    // are called "topology" and the §122 audit tripped over it. This badge is
    // the SESSION-TYPE liveness checker (Honda), which is real and compile-time.
    // It is NOT `topology.rs`'s `TopologyBackend`/`ReferenceTopology` (Betti
    // numbers / persistent homology), which is one of `savant`'s three unwired
    // engines and is carried by that row. The proof below does not overclaim —
    // it names exactly what it means — but a reader scanning for "topology" will
    // find the unwired engine first, so the disambiguation lives here.
    // §Fase 122.b — the session family and `topology` are COMPILE-time claims
    // (`check_session_duality` decides dual involution with capture-avoiding
    // substitution and coinductive equality; `check_topology_liveness` is a DFS
    // gray/black cycle detector emitting a Honda-liveness violation). The corpus
    // gate COMPILES every fixture before executing it, so both deductions run on
    // this file on every corpus run — which makes a compiled fixture the RIGHT
    // KIND of citation here, not a weaker substitute for a dispatch one.
    //
    // Verified by breaking it, not by assuming: flipping ONE arm of `Buyer`'s
    // `select` from `send` to `receive` fails the gate with the full session
    // type and its dual printed —
    //   `!Quote.?Settlement.+{accept: ?Settlement.end, …}` vs the expected
    //   `?Quote.!Settlement.&{accept: !Settlement.end, …}`
    // — so the duality is genuinely DECIDED arm-for-arm on this fixture, not
    // merely parsed.
    ("topology", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("session", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("send", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("receive", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("select", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("branch", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
        // §Fase 122.b — §112.d already deploys through the REAL `POST /v1/deploy`,
    // so the path was never in doubt; what it lacked was a program on DISK, which
    // is what lets a build catch the citation going stale. The KL sensor detects a
    // real deviation from a LEARNED baseline (wiring it exposed a kernel bug that
    // made it structurally blind). Perturbing the fixture's `window: 4` kills the
    // test — the declared learning window is honoured.
    ("immune", Attested {
        fixture: "tests/fixtures/fase122_b_cognitive_io/immune_reflex_heal.axon",
        gate: "tests/fase112_d_immune_fires.rs",
    }),
        // §Fase 122.b — see `immune`. Perturbing the fixture's `action: quarantine`
    // kills the test: the reflex takes the action the ADOPTER declared, not a
    // default, and its firing carries an HMAC-signed trace.
    //
    // NOTE for a future mutation attempt: loosening `on_level` does NOT kill it,
    // and that is correct, not a hole. `level_at_least` is `observed >= threshold`
    // over know(0) < believe < speculate < doubt(3) — severity order — so a lower
    // threshold fires on a MORE severe classification. Discrimination in the other
    // direction is what `a_steady_system_stays_quiet_once_the_baseline_is_learned`
    // pins.
    ("reflex", Attested {
        fixture: "tests/fixtures/fase122_b_cognitive_io/immune_reflex_heal.axon",
        gate: "tests/fase112_d_immune_fires.rs",
    }),
        // §Fase 122.b — see `immune`. The HealKernel is registered from the compiled
    // IR and renders a decision under the fixture's declared `mode: audit_only`.
    ("heal", Attested {
        fixture: "tests/fixtures/fase122_b_cognitive_io/immune_reflex_heal.axon",
        gate: "tests/fase112_d_immune_fires.rs",
    }),
    ("compliance", Real { proof: "tests/fase117_a_regulated_boundary_coverage.rs (§117.a — `axon-T957` finally scopes the ESK Fase 6.1 κ-coverage law to the `axonendpoint`, the surface the README has advertised since v1.x. It is a real SET DIFFERENCE over the κ of the boundary's `body:`/`output:` types against the declared shield's `compliance:` — NOT the `!compliance.is_empty()` presence check §111 F16 condemned. The endpoint's own `compliance:` list does NOT satisfy it: a label is not a control. The §111 diagnosis was right that the law existed only for `component`; what it missed is that the promise was VACUOUS, not weak — the same shape `lease` had before §113 gave it a subject)" }),
    ("component", Partial { gap: "§111 — the compile-time shield-coverage law over regulated κ IS genuinely enforced (a real set difference). But the component renders NOTHING; the README itself defers the renderer" }),
    ("view", Partial { gap: "§111 — only referential integrity is checked. No `route` check, no session-typed-reactivity check, and it renders nothing" }),
    // ── Enterprise I/O (Fases 80–85)
    // §Fase 122.a — was `Real { proof: "cache_runtime — cacheability derives
    // from the type system's `effects: pure` proof (§85)" }`. That proof cited
    // a COMPILE-TIME property as evidence of a runtime, named no test file, and
    // so passed Law 3 vacuously and Law 4 not at all. Measured 2026-08-13:
    // `CacheRuntime::dispatch` has ZERO production callers (only its own
    // tests), `CacheBackend` has no enterprise implementor, and fase_85's own
    // plan records §85.h as "deferred, not built". `backend:`, `ttl:`,
    // `key_params:`, `invalidate_on:` and `default_policy:` are inert in BOTH
    // flavours — `backend: redis` is not a downgrade to in-process, there is no
    // cache. Shipped as `Real` in 2.88.0; this is the row with public exposure.
    ("cache", Unwired { engine: "cache_runtime::CacheRuntime::dispatch — content-addressed key, single-flight, TTL+jitter, LRU eviction, errors never cached, with its own passing tests. The seam was designed for a call site it never got (§85.h). Cabling it is §122.d, the cheapest cable in its class" }),
    ("voice", Unaudited),
    ("shell", Unaudited),
    ("path rewrite", Unaudited),
    ("PASETO", Unaudited),
    // ── Session types (§41)
    ("socket", Real { proof: "tests/fase111_i_socket_served.rs (§111.i — the OSS server now SERVES the session-typed WebSocket at GET /ws/{socket}, and enforces the protocol the adopter DECLARED: the missing SessionType compiler lands in session_runtime::compile. An unresolvable protocol is REFUSED, never substituted — enterprise used to hand every socket a hardcoded chat schema, so a protocol proven dual at compile time had a different one enforced at runtime)" }),
    // §Fase 122.b — the surface forms the README badges separately from the bare
    // names above. The same fixture carries all four literally: `send Quote`,
    // `receive Settlement`, and the labelled `select`/`branch` whose arms the
    // duality check pairs one for one.
    ("send T", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("receive T", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("select {ℓᵢ:…}", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("branch {ℓᵢ:…}", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
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
    // §Fase 122.b — see the `flow` note above; same fixture, same gate. The
    // fixture is a single-`step` flow, so the byte-equal pin IS the step
    // executor's output on both paths.
    ("step", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/banking/01_canonical_loan_decision.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — §65's unified executor (runner + execute_server_flow, binding
    // flow+persona+context) is what every gate that runs a program goes through,
    // which made the old citation true and unfalsifiable at once. This fixture
    // carries a literal top-level `run` statement, and perturbation confirms it
    // is resolved: `run NoSuchFlowAtAll()` fails with "Undefined flow
    // 'NoSuchFlowAtAll' in run statement".
    ("run", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/19_run_effects_compliance.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — `type_checker` (structural + refinement validation) and
    // `store_schema` (§38, declared shapes pinned to SQL column types) are the
    // engines. The fixture below DECLARES types and is compiled from disk by the
    // gate before it executes, so the front half of the path is exercised on
    // source an adopter could have written.
    ("type", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/06_mdn_navigate_graph.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // §Fase 122.b — `fase73_c_json_accessors.rs` + `fase73_e_json_lens.rs` stay
    // the semantic gates for §73's totality (navigation yields null rather than
    // panicking; coercions are honest about a type mismatch). Neither reads a
    // file. The fixture carries both surfaces — open `Json` and the `Json<T>`
    // lens — and is the first published program in the repo to use either.
    ("json", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/16_json_open_navigation.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // The audit chain + π-calc channel family.
    ("ledger", Unaudited),
    ("channel", Partial { gap: "§114.z — durable persistence (§74 outbox) and the σ-shield egress gate (§114, tests/fase114_channel_shield_egress.rs) are proven; `qos:`/`lifetime:` enforcement is unaudited" }),
    // §Fase 122.b — the previous citation (`tests/fase114_channel_shield_egress.rs`)
    // is the SEMANTIC gate and still runs: the channel's σ-shield scans on every
    // path by IR-resolution, and a Reject fails closed before any routing. But
    // its dispatch half hand-builds `IREmit`, so it proves the handler, not the
    // path — the §119.f.8 distinction. The parity corpus supplies the path.
    ("emit", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/banking/09_credit_score_reasoning.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("publish", Unaudited),
    ("discover", Unaudited),
    // Governance operators.
    // §Fase 122.b — `fase114_a_budget_governs_the_canonical_path.rs` (9/9) still
    // proves the ceiling binds on the canonical `use Tool(…)` path, and stays
    // the semantic gate. What it cannot do is satisfy Law 4: it builds its
    // source inline. §114.a rescued `budget` from being README prose that
    // governed nothing — and it was STILL true, until this fixture, that no
    // published `.axon` in the repo declared one. That is the same shape one
    // level up: a primitive whose runtime is real and whose surface nobody
    // writes.
    ("budget", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/15_tool_budget_governance.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("window", Unaudited),
    ("cors", Unaudited),
    // The governed-egress trio (§105 · §99/§106 · §110) — none was ever badged.
    // §Fase 122.b — verified by perturbation on this fixture: 
    // `target: notaformat` fails it (§99's serializer catalog is decided, not
    // parsed).
    ("document", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/20_document_notify_egress.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    ("deliver", Real { proof: "delivery.rs + axon-T920 (§105 — provenance travels or the delivery refuses; first top-level egress integrated in the executor, D105.7(B))" }),
    // §Fase 122.b — verified by perturbation on this fixture: 
    // `window: 0m` fails it — §110's mandatory positive window. All five
    // §110 laws fired when the block was first written wrong, and the fixture
    // keeps their diagnostics as comments.
    ("notify", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/20_document_notify_egress.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
    // Credentials + custody verbs.
    ("credential", Real { proof: "tests/fase92_mint_runtime.rs (§92 — grants ⊆ minter, TTL ≤ 24h; axon-T893–T896)" }),
    ("mint", Real { proof: "tests/fase92_mint_runtime.rs (§92 — fail-closed without a minter port; the raw bearer is shown once and never persisted, axon-T896)" }),
    ("rotate", Real { proof: "tests/fase94_custody_runtime.rs (§94 — rotation_without_revelation: ENUMERATE+ROTATE(CAS)+USE; no term evaluates to a secret value)" }),
    // Session types + quantum bridge.
    ("upstream", Real { proof: "tests/fase80_d_upstream_e2e.rs (§80 — config-resolved dial, declared auth, credit flow-control against a real WS server)" }),
    // §Fase 122.b — §111.d's gates all still run and all hand-build `IRQuant`:
    // a proof of the HANDLER. The gate now also compiles a fixture and dispatches
    // the node the COMPILER lowered, against the same `ReferenceSimulator` the
    // runner mounts. The expected value is ANALYTIC — |0⟩ under Pauli-Z gives
    // ⟨Z⟩ = +1 exactly — so no placeholder satisfies it. Perturbation kills it
    // three ways: flipping the term's sign, renaming the observable, and rotating
    // the carrier off the ground state.
    ("quant", Attested {
        fixture: "tests/fixtures/fase122_b_quant/ground_state_measurement.axon",
        gate: "tests/fase111_d_quant_wired.rs",
    }),
    // §Fase 122.b — the observable under test is the one the compiler emitted from
    // `observable GroundEnergy { qubits: 1  term: 1.0 * \"Z\" }`, and the result
    // must NAME it. Renaming the reference in the `quant` header kills the
    // citation; so does flipping the declared term's coefficient.
    ("observable", Attested {
        fixture: "tests/fixtures/fase122_b_quant/ground_state_measurement.axon",
        gate: "tests/fase111_d_quant_wired.rs",
    }),
    // Symbolic differentiation.
    // §Fase 122.b — `fase109_grad_runtime.rs` stays the semantic gate (it deploys
    // through the REAL /v1/deploy + /v1/execute and checks the evaluated
    // derivative). It builds its source inline. `grad` is SYMBOLIC and happens at
    // COMPILE time — the gradient IS IR under the PCC GradientSoundness witness —
    // so the corpus gate's compile pass is where the claim is exercised.
    ("grad", Attested {
        fixture: "tests/fixtures/fase33z_parity_corpus/cross_vertical/18_refine_grad_compute.axon",
        gate: "tests/fase33z_d_parity_corpus.rs",
    }),
];

/// **The ratchet.** Every advertised name whose runtime is
/// [`RuntimeStatus::NotImplemented`] or `RuntimeStatus::Unwired` — i.e. every
/// promise the README makes today that the code does not keep.
///
/// This list may only ever **shrink**. Adding a name to it requires editing this
/// file, which is the point: a new unkept promise cannot land quietly. Removing
/// one means you either implemented it or retracted it from the README — both
/// good outcomes, and both must happen in the same PR as the deletion.
///
/// §111 inherited every one of these. They are the fase's open debt, and now the
/// compiler holds the ledger.
///
/// # §Fase 122.a — this ledger went 0 → 3, and that is the ratchet WORKING
///
/// §119 drove it to empty and that was true of everything the table classified
/// honestly. It was not true of the table. `cache` was attested `Real` on a
/// proof that named a compile-time property, so its dead runtime was never
/// counted; `savant` and `synth` sat under `Unaudited`, which said *nobody
/// looked* — accurate in §111, and no longer accurate once someone did.
///
/// **The debt did not grow. It was uncovered.** Three engines that exist, pass
/// their own tests, and are reachable from no published program. Recording them
/// is the entire thesis of §122 applied to the file that carries it: a ledger
/// that stays empty because a row lies is worth less than one that admits three.
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
    //  §111 opened this ledger with 22 advertised primitives that were not
    //  real. §111.f paid `compute`. §112 paid six of the Cognitive-I/O block.
    //  §113 paid `resource`/`lease`. §114 paid the governed channel. §119 paid
    //  the last five — `mandate` (b), `lambda` + `ots` (c), `hibernate` (d),
    //  `fabric` (e), all on 2026-08-07 — and the list reached EMPTY.
    //
    //  §Fase 122.a re-opened it at THREE, and the reason is the point of the
    //  whole fase: the list was empty of everything this table classified
    //  honestly, and the table had one row that did not. Each entry below is an
    //  `Unwired` engine — complete, tested, and reachable from no published
    //  program. NONE of them is new work discovered by §122; all three predate
    //  it and were simply not counted.
    //
    //  A KNOWN_DEBT of 0 or 3 does NOT mean every other primitive is Real.
    //  Several are `Partial` with the gap NAMED — a bounded, published limit an
    //  adopter can read and plan around, which is the whole point of the
    //  distinction.
    // ═══════════════════════════════════════════════════════════════════
    //
    // §Fase 122.a — measured 2026-08-13. `CacheRuntime::dispatch` has zero
    // production callers, `CacheBackend` has no enterprise implementor, and
    // fase_85 records §85.h as "deferred, not built". Nothing memoises
    // anything. It LEAVES when §122.d gives the seam its call site — the
    // cheapest cable in the class, because the engine is already there.
    "cache",
    // §Fase 122.a — three tested engines (HRR codec, VFE/EFE inference,
    // Betti/PHC topology), zero callers outside their own tests. It LEAVES when
    // active inference is wired, which is a fase, not a cable.
    "savant",
    // §Fase 122.a — `DenyByDefaultSynth` exists and has no mount point. NOTE
    // the honest ceiling: cabling it in OSS yields `FailsClosed` (§87.j — OSS
    // never executes synthesised code), not `Real`. It leaves this ledger for
    // `FailsClosed`, which is an acceptable state to advertise from.
    "synth",
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

    /// **LAW 2 — the ratchet.** Nothing advertised may be `NotImplemented` OR
    /// `Unwired` unless it is in `KNOWN_DEBT`, and that list may only shrink.
    ///
    /// This is the law that makes §111's findings durable. A *new* unkept promise
    /// is a red build. The existing ones are a ledger the compiler holds — not a
    /// paragraph in a document nobody rereads.
    ///
    /// §122.a widened it from `is_unimplemented()` to `owes_a_debt_entry()`
    /// (D122.6). Had `Unwired` been added beside this law instead of inside it,
    /// the state would have been an escape hatch: any `NotImplemented` row could
    /// be relabelled out from under the ratchet, and the ratchet would measure
    /// vocabulary instead of debt.
    #[test]
    fn no_new_unkept_promises() {
        let debt: HashSet<&str> = KNOWN_DEBT.iter().copied().collect();
        let undeclared: Vec<&str> = ADVERTISED
            .iter()
            .filter(|(_, s)| s.owes_a_debt_entry())
            .map(|(n, _)| *n)
            .filter(|n| !debt.contains(*n))
            .collect();
        assert!(
            undeclared.is_empty(),
            "these advertised primitives are NotImplemented/Unwired and are NOT in KNOWN_DEBT: {undeclared:?}\n\n\
             You have advertised a promise the code does not keep. Either implement it, wire it, \
             retract it from the README, or (if you are knowingly shipping the gap) add it to \
             KNOWN_DEBT with its finding — so it is a LEDGER ENTRY and not a lie."
        );
    }

    /// The ratchet's teeth: `KNOWN_DEBT` may only shrink. Bump this DOWN when a
    /// debt is paid; a build that needs it bumped UP is telling you something.
    ///
    /// §122.a TIGHTENED the cap 14 → 3. The old cap was slack left over from
    /// §111's opening balance: with the list at zero, eleven rows could have
    /// been added before any gate complained. A ratchet with slack is a ratchet
    /// that has stopped measuring. The three entries are the `Unwired` engines
    /// §122.a uncovered; the next move is 3 → 2 when §122.d cables `cache`.
    #[test]
    fn the_debt_ledger_only_shrinks() {
        assert!(
            KNOWN_DEBT.len() <= 3,
            "KNOWN_DEBT grew to {} — the ratchet only turns one way. §122.a re-baselined it at 3 \
             (the `Unwired` engines it uncovered) and TIGHTENED the cap to match; a new entry is \
             not a row to add, it is a bug to fix.",
            KNOWN_DEBT.len()
        );
    }

    /// **§122.a — the `Unwired` population is pinned, and may only shrink.**
    ///
    /// Three, measured 2026-08-13: `cache` · `savant` · `synth`. Each is an
    /// engine that exists, passes its own tests, and is reachable from no
    /// published program.
    ///
    /// This pin exists so the state cannot become a comfortable parking spot.
    /// `Unwired` is the honest label for a cable that was never run — it is not
    /// a place to move rows that have become inconvenient to defend as `Real`.
    /// The only direction is down, and down means a call site landed.
    #[test]
    fn the_unwired_population_only_shrinks() {
        let n = ADVERTISED
            .iter()
            .filter(|(_, s)| matches!(s, Unwired { .. }))
            .count();
        assert!(
            n <= 3,
            "the Unwired population grew to {n} — §122.a measured 3 (cache · savant · synth). \
             A new Unwired row means a primitive was advertised with an engine nobody calls; \
             wire it or retract it."
        );
    }

    /// Every `Unwired` row must NAME the engine that exists. The name is what
    /// separates a cable job from a build job — without it the state degrades
    /// into a politer `NotImplemented`, which is not what it is for.
    #[test]
    fn every_unwired_row_names_its_engine() {
        for (name, status) in ADVERTISED {
            if let Unwired { engine } = status {
                assert!(
                    !engine.trim().is_empty(),
                    "`{name}` is Unwired with no engine named — say what exists, or the row \
                     cannot be told apart from NotImplemented"
                );
            }
        }
    }

    /// Every debt entry must actually BE a debt — no padding the ledger with
    /// names that are fine, which would let a real one hide among them.
    #[test]
    fn the_debt_ledger_has_no_padding() {
        for name in KNOWN_DEBT {
            let s = status_of(name)
                .unwrap_or_else(|| panic!("KNOWN_DEBT names `{name}`, which is not advertised"));
            assert!(
                s.owes_a_debt_entry(),
                "`{name}` is in KNOWN_DEBT but its status is {s:?} — if it was implemented, wired, \
                 or retracted, delete its ledger row in the same PR."
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

    /// **§122.a — LAW 4's shape.** An `Attested` claim must name a `.axon`
    /// FIXTURE that exists and a GATE that exists.
    ///
    /// This is the half of Law 4 that the SHAPE can enforce. The other half —
    /// that the gate actually READS that fixture — is §122.b, and it is the
    /// half that makes the citation mean something. Landing the shape first is
    /// deliberate (D122.2): re-attesting sixty-four rows is sixty-four
    /// judgements, and mixing them into a mechanical refactor is how a
    /// convenience `Real` gets in.
    #[test]
    fn every_attested_claim_names_a_fixture_and_a_gate_that_exist() {
        for (name, status) in ADVERTISED {
            if let Attested { fixture, gate } = status {
                assert!(
                    fixture.ends_with(".axon"),
                    "`{name}` is Attested but its fixture `{fixture}` is not a `.axon` source. \
                     Law 4 wants the input a published program could have written — a test file \
                     proves the engine, not the path."
                );
                for (label, path) in [("fixture", fixture), ("gate", gate)] {
                    let found = [repo_root().join(path)]
                        .into_iter()
                        .chain([
                            repo_root().join("axon-rs").join(path),
                            repo_root().join("axon-frontend").join(path),
                        ])
                        .any(|p| p.exists());
                    assert!(
                        found,
                        "`{name}` is Attested and cites {label} `{path}`, which does not exist. \
                         A citation that cannot be run is not a proof."
                    );
                }
            }
        }
    }

    /// **§122.b — LAW 4, ENFORCED.** The cited gate must actually reach the
    /// cited fixture, and the fixture must be a program the compiler accepts.
    ///
    /// # What this proves, and what it does not
    ///
    /// Enforced here, mechanically:
    ///
    /// 1. the fixture **compiles** — a `.axon` that no longer parses is not a
    ///    program any adopter could have written, and a citation to it is
    ///    stale. This is the check that catches rot;
    /// 2. the gate **names a path that reaches the fixture** — the fixture
    ///    itself, or a directory under `fixtures/` that contains it (the
    ///    corpus-driven idiom `fase116_laws_corpus` and `fase33z_d_parity_corpus`
    ///    already use). A gate naming only `tests/fixtures` proves nothing and
    ///    is rejected: the path must be at least one segment deeper.
    ///
    /// **Not** enforced, and it must be said plainly, because a ratchet that is
    /// believed to prove more than it does is the exact defect this file
    /// exists to close: nothing here decides whether the gate *meaningfully
    /// exercises the primitive*. That stays a human attestation — the premise
    /// of [`RuntimeStatus`] from the beginning, *because no linter can decide
    /// it*. What §122.b removes is the ability to cite something that cannot be
    /// run, or that has quietly stopped compiling. It makes the CITATION
    /// verifiable. It does not make the JUDGEMENT automatic.
    #[test]
    fn every_attested_gate_reaches_a_fixture_that_still_compiles() {
        for (name, status) in ADVERTISED {
            let Attested { fixture, gate } = status else {
                continue;
            };
            // (1) The fixture is a program the compiler still accepts.
            let fixture_path = resolve_repo_path(fixture)
                .unwrap_or_else(|| panic!("`{name}` cites fixture `{fixture}`, which is missing"));
            let src = std::fs::read_to_string(&fixture_path)
                .unwrap_or_else(|e| panic!("`{name}`: cannot read `{fixture}`: {e}"));
            let tokens = crate::lexer::Lexer::new(&src, fixture)
                .tokenize()
                .unwrap_or_else(|e| {
                    panic!(
                        "`{name}` cites fixture `{fixture}`, which no longer LEXES: {e:?}\n\
                         A citation to a program the compiler rejects is stale."
                    )
                });
            crate::parser::Parser::new(tokens).parse().unwrap_or_else(|e| {
                panic!(
                    "`{name}` cites fixture `{fixture}`, which no longer PARSES: {e:?}\n\
                     A citation to a program the compiler rejects is stale."
                )
            });

            // (2) The gate names a path that reaches it.
            let gate_path = resolve_repo_path(gate)
                .unwrap_or_else(|| panic!("`{name}` cites gate `{gate}`, which is missing"));
            let gate_src = std::fs::read_to_string(&gate_path)
                .unwrap_or_else(|e| panic!("`{name}`: cannot read gate `{gate}`: {e}"));
            assert!(
                gate_mentions_a_path_reaching(&gate_src, fixture),
                "`{name}` is Attested, but gate `{gate}` never names a path that reaches fixture \
                 `{fixture}`.\n\n\
                 This is LAW 4. A gate that does not read the fixture proves the ENGINE, not the \
                 PATH — §119.f.8's lesson, and the hole `cache` went through. Name the fixture (or \
                 the corpus directory that holds it) in the gate, or cite a gate that does."
            );
        }
    }

    /// Resolve a repo-relative path, tolerating the two crate roots the table's
    /// citations use.
    fn resolve_repo_path(p: &str) -> Option<std::path::PathBuf> {
        [
            repo_root().join(p),
            repo_root().join("axon-rs").join(p),
            repo_root().join("axon-frontend").join(p),
        ]
        .into_iter()
        .find(|c| c.exists())
    }

    /// True when `gate_src` names the fixture, or a directory under a
    /// `fixtures` segment that contains it.
    ///
    /// The depth rule is the load-bearing part: naming `tests/fixtures` alone
    /// would let any gate claim any fixture, so an ancestor only counts when it
    /// is at least one segment deeper than the `fixtures` directory itself.
    fn gate_mentions_a_path_reaching(gate_src: &str, fixture: &str) -> bool {
        let fixture = fixture.replace('\\', "/");
        if gate_src.contains(&fixture) {
            return true;
        }
        let segments: Vec<&str> = fixture.split('/').collect();
        let Some(fx) = segments.iter().position(|s| *s == "fixtures") else {
            return false;
        };
        // Ancestors strictly deeper than `.../fixtures`, longest first.
        (fx + 2..segments.len())
            .rev()
            .any(|end| gate_src.contains(&segments[..end].join("/")))
    }

    /// §122.b — the depth rule inside [`gate_mentions_a_path_reaching`] is the
    /// load-bearing half, so it gets its own test rather than trust.
    ///
    /// Without it, a gate that merely says `tests/fixtures` anywhere in its
    /// source could claim EVERY fixture in the repo — which would make Law 4
    /// enforceable in form and vacuous in fact, the precise failure this whole
    /// fase exists to end.
    #[test]
    fn naming_the_fixtures_root_alone_does_not_reach_a_fixture() {
        let fixture = "tests/fixtures/fase33z_parity_corpus/banking/01_canonical_loan_decision.axon";

        // Too shallow: the corpus root is not named.
        assert!(!gate_mentions_a_path_reaching("reads tests/fixtures/", fixture));
        assert!(!gate_mentions_a_path_reaching("reads tests/", fixture));
        // An unrelated corpus does not reach it either.
        assert!(!gate_mentions_a_path_reaching(
            "reads tests/fixtures/fase116_laws",
            fixture
        ));
        // Deep enough: the corpus directory that actually holds it.
        assert!(gate_mentions_a_path_reaching(
            "reads tests/fixtures/fase33z_parity_corpus/",
            fixture
        ));
        // And the fixture named outright.
        assert!(gate_mentions_a_path_reaching(
            &format!("reads {fixture}"),
            fixture
        ));
        // A fixture outside any `fixtures/` directory must be named exactly.
        assert!(!gate_mentions_a_path_reaching(
            "reads examples/",
            "examples/loan.axon"
        ));
    }

    /// **§122.a — the legacy `Real { proof }` population is pinned, and may only
    /// shrink.** §122.b drives it to zero.
    ///
    /// **Sixty-nine** rows still carry the free-prose citation whose conditional
    /// check let `cache` through: Law 3 only verifies a proof that happens to
    /// name a test file, and Law 4 — written since §119.f.8 — never ran at all.
    /// Every row that migrates to `Attested` is one that has been re-attested
    /// against a fixture, by hand, on purpose.
    ///
    /// The number going UP means a new claim of reality was made in the old,
    /// unverifiable shape.
    #[test]
    fn the_legacy_real_population_only_shrinks() {
        let n = ADVERTISED
            .iter()
            .filter(|(_, s)| matches!(s, Real { .. }))
            .count();
        assert!(
            n <= 25,
            "the legacy `Real {{ proof }}` population grew to {n} — §122.a measured 69 and §122.b \
             drives it to zero (69 → 25 so far). A NEW claim of reality must be \
             `Attested {{ fixture, gate }}`: name the `.axon` a published program could write, \
             and the gate that runs it."
        );
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
