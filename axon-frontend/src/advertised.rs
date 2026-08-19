//! v2.67.0 — **the anti-drift gate: the public README is a BUILD INPUT.**
//!
//! # Why this exists
//!
//! v2.67.0 audited every primitive the public README advertises and found ~22 of
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
//! **A presence-only gate catches nothing that matters.** Every serious v2.67.0
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
//! 1b. **(v2.69.0) The registry is the enumeration source.** Every
//!    `PRIMITIVE_REGISTRY` entry with `is_advertised: true` must have an entry
//! here AND a `<code>` badge in the README. The v2.67.0 gate enumerated only
//!    the badges, so a primitive DISCUSSED but never BADGED escaped
//!    classification entirely — `budget` appeared in the README seventeen
//! times, was badged zero, and its dead runtime (v2.69.0) survived every
//!    green build. **A gate that guards only the badges guards the
//!    packaging.** Conversely, an `is_advertised: false` primitive must be
//!    neither badged nor classified: hiding a primitive from the promise is a
//!    deliberate, pinned act.
//! 2. **Nothing advertised may be [`RuntimeStatus::NotImplemented`] or
//!    `RuntimeStatus::Unwired`** … except what is written down in
//!    [`KNOWN_DEBT`], which is a **ratchet**: it may only shrink. A *new*
//! unkept promise is a red build. That is the whole point — v2.67.0's debt
//!    becomes a ledger the compiler enforces, instead of a finding in a
//!    document nobody rereads.
//!
//! v2.89.0 added `Unwired` to this law rather than beside it. The
//!    two states differ in what the fix COSTS — `NotImplemented` needs an
//!    engine, `Unwired` needs a call site — and not at all in what the adopter
//!    experiences. If reclassifying could move a row out from under the
//!    ratchet, the ratchet would measure vocabulary instead of debt.
//! 3. **A [`RuntimeStatus::Real`] claim must cite its proof**, and if the proof
//!    names a test file, that file must exist on disk. A claim of reality that
//!    cannot point at the gate proving it is just a nicer-sounding assertion.
//! 4. **The proof must cite the PATH, not the engine** — v2.83.0, learned
//!    the expensive way. `reason` was attested `Real { proof:
//!    "pure_shape::run_reason" }`. The citation was accurate: the handler
//!    existed, worked, and had its own tests. It was also worthless, because the
//!    grammar that reaches it did not exist — the README's `reason { given ask
//!    depth }` (16 blocks, each its step's entire cognition) was thrown away at
//! PARSE time, and `axon check` said `0 errors`. v2.67.0's own phrase: *motor
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
    /// evidence — a test file, or the cycle that shipped it.
    Real { proof: &'static str },
    /// It delivers, with a documented gap versus the advertised claim. The gap
    /// is named so it cannot rot into a silent lie.
    Partial { gap: &'static str },
    /// It REFUSES — at compile time or at dispatch. Honest by construction: an
    /// adopter is told, loudly, rather than handed silence. This is the v2.63.0
    /// posture, and it is an acceptable state to advertise from, because the
    /// adopter cannot be *fooled* by it.
    FailsClosed { diagnostic: &'static str },
    /// The runtime does NOT do what the summary promises, and says nothing about
    /// it. **This may not be advertised** unless it is in [`KNOWN_DEBT`].
    NotImplemented { finding: &'static str },
    /// v2.89.0 — **the engine EXISTS, is tested, and no production path
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
    /// (ratified 2026-08-13). To the adopter the two are
    /// indistinguishable: the primitive does not do what the summary says, and
    /// nothing tells them. Letting `Unwired` dodge Law 2 would make
    /// reclassification an escape hatch from the ratchet — which is the exact
    /// species of drift this file exists to stop. The debt did not grow when
    /// these rows landed; it was **uncovered**. A ledger that stays empty
    /// because a row lies is worth nothing.
    Unwired { engine: &'static str },
    /// v2.67.0 has not verified this one. Advertised on trust, and counted: the
    /// unaudited population is pinned below and may only shrink.
    Unaudited,
    /// v2.89.0 — **`Real`, re-attested under Law 4**: the claim names the
    /// `.axon` FIXTURE a published program could write, and the GATE that
    /// walks that fixture to dispatch.
    ///
    /// Law 4 has been written in this file since v2.83.0 and never ran. The
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
    /// `gate` must exist and must READ that fixture (v2.89.0).
    ///
    /// `Real { proof }` remains for rows not yet re-attested. Its population is
    /// pinned and **may only shrink**; v2.89.0 drives it to zero. Migrating the
    /// shape and re-attesting forty rows in one commit would mix a mechanical
    /// refactor with forty judgements, and that mixture is where a convenience
    /// `Real` gets in.
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

    /// **The predicate Law 2 turns on** (v2.89.0).
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
/// Verdicts sourced from the v2.67.0 Gate-1/Gate-2 audit (see the cycle's topic
/// file). Where v2.67.0 did not reach, the entry is honestly `Unaudited` rather
/// than optimistically `Real` — an unchecked box is not a passing one.
pub const ADVERTISED: &[(&str, RuntimeStatus)] = &[
    // ── Cognitive core — LLM-driven, and honestly so. Calling the model IS
    // their nature; that is not the defect v2.67.0 hunts.
    // v2.89.0 — was the bare module name `flow_dispatcher::pure_shape`, one
    // of the 38 rows Law 3 never checked. The fixture below was added by v2.89.0
    // and declares a `persona` alongside the `mandate` that governs its output.
    ("persona", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/13_compliance_persona_mandate.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("intent", Unaudited),
    // v2.89.0 — FIRST TWO RE-ATTESTATIONS. Both used to cite a module
    // (`flow_dispatcher::dispatch_node`, `flow_dispatcher::pure_shape`) — an
    // ENGINE citation, the shape v2.83.0 proved worthless, and one Law 3 never
    // checked because it names no test file. The parity corpus is the honest
    // replacement: `parity_corpus` reads each `.axon` fixture from
    // disk, drives it through BOTH execution paths (sync runner + async
    // dispatcher) under the stub backend, and pins the results byte-equal. The
    // input is source an adopter could have written; the gate walks it to
    // dispatch. That is Law 4 satisfied, not asserted.
    ("flow", Attested {
        fixture: "tests/fixtures/parity_corpus/banking/01_canonical_loan_decision.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.83.0 — the proof used to read `pure_shape::run_reason`, and it was
    // TRUE and USELESS. The handler existed and worked; the grammar that reaches
    // it did not. The README's `reason { given ask depth }` — 16 blocks, each one
    // its step's entire cognition — was discarded at PARSE time, so this "Real"
    // attested an engine no published program could call.
    //
    // ⚠️ The lesson is about THIS TABLE, not about `reason`: a proof that names a
    // FUNCTION proves the engine exists. It does not prove any published program
    // reaches it. Cite the gate that walks the path — grammar to dispatch — and
    // the attestation means what a reader thinks it means.
    // v2.89.0 — v2.83.0's semantic gates (`reason_block`,
    // `reason_prompt_is_built`) still hold and still run; what they
    // could not do is satisfy Law 4, because they build their source inline and
    // no file on disk holds it. The parity corpus does: a published-shaped
    // program carrying a `reason` block, executed on both paths.
    ("reason", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/09_content_moderation_reason.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — was the bare engine name `anchor_checker`. The fixture below
    // was ADDED by v2.89.0 for exactly this: every gate proving `anchor`, `probe`
    // and `weave` built its source inline, so no file on disk held a program
    // using them and the rows could only cite an engine. The corpus gate
    // discovers it by directory walk and drives it through both paths.
    ("anchor", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/12_legal_intake_probe_weave_anchor.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — was the bare engine name `pure_shape::run_refine`.
    ("refine", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/18_refine_grad_compute.axon",
        gate: "tests/parity_corpus.rs",
    }),
        // v2.89.0 — was `cognitive::run_remember / run_recall`. This citation proves
    // the DECLARATION path and says so: the PEM write-through stays proven by v2.67.0's
    // gates. The fixture's `store: session` and `retrieval: semantic` reach the IR,
    // and both closed catalogs are watched refusing a value outside them.
    ("memory", Attested {
        fixture: "tests/fixtures/declarations/memory_pix_store.axon",
        gate: "tests/declaration_laws.rs",
    }),
    // v2.89.0 — was the bare engine name `lambda_tools::dispatch_use_tool_real`.
    // The gate that DID read a file for `tool` (`tool_dispatch_example`)
    // only parses and type-checks the canonical example — the front half. This
    // fixture is dispatched.
    ("tool", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/15_tool_budget_governance.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — see the `anchor` note above; same fixture, same gate.
    ("probe", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/12_legal_intake_probe_weave_anchor.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.83.0 — see rule 4 above: this proof used to name the handler
    // alone, and the handler was reachable only from a braced form no published
    // block writes. Worse, it sent the model the source NAMES, so even when
    // reached it synthesised over identifiers instead of outputs.
    // v2.89.0 — v2.83.0's gates (`weave_statement`,
    // `weave_prompt_is_built`) still hold: grammar → IR → resolved
    // prompt, over the engine `pure_shape::run_weave`. They build their source
    // inline, so they cannot satisfy Law 4. Same fixture as `anchor`/`probe`.
    ("weave", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/12_legal_intake_probe_weave_anchor.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — was `pure_shape::run_validate`, a bare engine name and one
    // of the 38 rows Law 3 never checked (no `tests/` token ⇒ vacuous). v2.88.0's
    // own gates prove the scoring; this proves a published program reaches it.
    ("validate", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/04_capability_mediation_validate.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("context", Unaudited),
    // ── Epistemic scopes
    ("know", Unaudited),
    ("believe", Unaudited),
    ("speculate", Unaudited),
    ("doubt", Unaudited),
    // ── Concurrency & continuation
    // v2.89.0 — real fan-out via `join_all` (v2.15.0) stays the mechanism; the
    // citation now names a program that uses it and the gate that runs it.
    ("par", Attested {
        fixture: "tests/fixtures/parity_corpus/banking/04_par_concurrent_checks.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("hibernate", Partial { gap: "v2.83.0 made the suspension REAL on the production dispatch path: the walk HALTS at the hibernate point (the task dies — zero further compute, zero tokens, the primitive's economic guarantee), the continuation parks under the README's SHA-256(flow ∥ event ∥ position) id with the remaining nodes + bindings + catalogs, `emit <channel>` WAKES matching continuations (fire-and-forget resume through the real dispatcher), a late resume is REFUSED by lazy expiry (no timer burns while asleep), and `hibernate until \"event\"` — the README's own spelling — parses. Gates: tests/hibernate.rs (halt counted in StepCompletes, park, emit-wake e2e, nested refusal) + hibernation unit suite; 4/4 mutations killed incl. the F20 walk-keeps-going reinjection. THE GAP: the parking lot is in-process (the pem::InMemoryBackend discipline) — a continuation does not survive process restart, so README's sleep-for-months across restarts is the durable-store enterprise catch-up (cognitive_states), and chained hibernation inside a resumed continuation is not yet defined" }),
    // ── Deterministic data plane (v2.63.0)
        // v2.89.0 — v2.63.0's finding is what this citation keeps closed:`dataspace` was
    // declared, type-checked and carried into the IR while `dataspace_specs` was
    // `#[serde(skip)]`-hidden and read by NOTHING. The declared dataspace reached no
    // runtime state, so this whole verb family was NARRATION — the program would
    // have sent "You are ingesting…" to a language model and returned prose.
    //
    // Group C was scoped as needing a live database. Measured: it does not. The v2.63.0
    // engine is first-party COLUMNAR and in-process, and the server's deploy path
    // builds it; what cannot reach it is the PARITY corpus, which passes `None` and
    // therefore fails these verbs closed BY DESIGN. "The parity harness cannot drive
    // it" is not "it needs Postgres".
    //
    // The gate deploys the fixture, executes it, and reads the ENGINE afterwards:
    // three CSV rows land as three typed rows, born `Untrusted` (v2.52.0) with a 64-hex
    // witness hash, and the `Float` column reads back as a float. Four perturbations
    // kill it: the declared column TYPE, the `max_rows` limit, selecting an
    // undeclared column in `focus`, and joining `associate` on the wrong key.
    ("dataspace", Attested {
        fixture: "tests/fixtures/dataspace/lead_pipeline.axon",
        gate: "tests/dataspace_from_disk.rs",
    }),
        // v2.89.0 — see the `dataspace` note above; same fixture and gate. The
    // execute-success assertion is what makes this verb's presence a claim rather
    // than decoration: v2.63.0's verbs fail CLOSED without the engine, so a green
    // execute is what says it ran.
    ("ingest", Attested {
        fixture: "tests/fixtures/dataspace/lead_pipeline.axon",
        gate: "tests/dataspace_from_disk.rs",
    }),
        // v2.89.0 — see the `dataspace` note above; same fixture and gate. The
    // execute-success assertion is what makes this verb's presence a claim rather
    // than decoration: v2.63.0's verbs fail CLOSED without the engine, so a green
    // execute is what says it ran.
    ("focus", Attested {
        fixture: "tests/fixtures/dataspace/lead_pipeline.axon",
        gate: "tests/dataspace_from_disk.rs",
    }),
        // v2.89.0 — see the `dataspace` note above; same fixture and gate. The
    // execute-success assertion is what makes this verb's presence a claim rather
    // than decoration: v2.63.0's verbs fail CLOSED without the engine, so a green
    // execute is what says it ran.
    ("associate", Attested {
        fixture: "tests/fixtures/dataspace/lead_pipeline.axon",
        gate: "tests/dataspace_from_disk.rs",
    }),
        // v2.89.0 — see the `dataspace` note above; same fixture and gate. The
    // execute-success assertion is what makes this verb's presence a claim rather
    // than decoration: v2.63.0's verbs fail CLOSED without the engine, so a green
    // execute is what says it ran.
    ("aggregate", Attested {
        fixture: "tests/fixtures/dataspace/lead_pipeline.axon",
        gate: "tests/dataspace_from_disk.rs",
    }),
        // v2.89.0 — see the `dataspace` note above; same fixture and gate. The
    // execute-success assertion is what makes this verb's presence a claim rather
    // than decoration: v2.63.0's verbs fail CLOSED without the engine, so a green
    // execute is what says it ran.
    ("explore", Attested {
        fixture: "tests/fixtures/dataspace/lead_pipeline.axon",
        gate: "tests/dataspace_from_disk.rs",
    }),
    // ── Budget, selection & synthesis
    ("deliberate", FailsClosed { diagnostic: "axon-T939 (v2.67.0) — the body is discarded at parse time; no budget was ever controlled" }),
    ("consensus", FailsClosed { diagnostic: "axon-T940 (v2.67.0) — no votes, no aggregation, no candidates" }),
    ("forge", Partial { gap: "v2.67.0 F17 — the Poincaré pipeline + NCD novelty gate are real, but `coherence` is hardcoded to 1.0, so a declared `constraints:` coherence floor is INERT" }),
    // v2.83.0 — `Unaudited` → `Partial`. It was the worst state in this
    // table: v2.67.0 never checked it, and when v2.83.0 finally did there was nothing
    // to check — eleven declared `IRAgent` fields, and no executor anywhere in
    // the dispatcher. It now RUNS, with `strategy:` as a control policy
    // and a termination proof per strategy.
    //
    // `Partial`, not `Real`, and the gap is one specific thing rather than a
    // hedge: `strategy: custom` means "the control policy is the `step` list
    // written inside the agent block", and `AgentDefinition` has no body field,
    // so the parser discards those steps. Dispatch REFUSES `custom` by name
    // instead of substituting another policy. `on_stuck: hibernate` is refused
    // for the same reason in the other direction — v2.83.0 parks a flow walk's
    // remaining nodes and an agent loop has no such list.
    // 4.1.0 — the two halves the v2.83.0 row named as gaps are closed:
    // `custom` executes the step sequence written inside the agent block
    // (the parser keeps it now), and the COMPILER refuses what the loop used
    // to refuse at dispatch (axon-T1216..T1220: the termination bound, the
    // body/strategy agreement, the undeclared callee, the return type, the
    // duration grammar). `max_time` is enforced, `return:` is validated. The
    // fixture is the README's own agent-in-a-step shape, read from disk and
    // driven through BOTH doors (the default server engine and the CLI stub
    // walker) by the gate — which is how the gate found the default server
    // engine had never attached the agent catalog at all. `on_stuck:
    // hibernate` remains refused by name — an agent loop has no
    // remaining-node list to park — and the refusal is documented behaviour,
    // not a gap.
    ("agent", Attested {
        fixture: "tests/fixtures/agent/react_agent_in_step.axon",
        gate: "tests/agent_loop_closes_its_promises.rs",
    }),
    ("shield", Partial { gap: "v2.67.0 — the scanner registry IS consulted and a Reject really fails the step, but OSS ships ZERO registered scanners, so an OSS adopter's shield is an identity pass-through until one is mounted" }),
    // ── Security & autonomous analysis
    // v2.89.0 — both were `Unaudited`, which was honest in v2.67.0 and is no
    // longer: they HAVE been audited now (2026-08-13), and the finding is
    // reachability, not uncertainty. `Unwired` is strictly more informative.
    ("savant", Unwired { engine: "holograph::ReferenceHolographCodec (HRR, FFT, dim-capped) + inference::ReferenceInference (VFE/EFE) + topology::ReferenceTopology (Betti/PHC) — three tested engines with ZERO callers outside their own tests. `cognition.depth:` is read only by the PCC (axon-T876, catalog validity) and by `main.rs` to print it; HOLOGRAPH_DIM_CAP is never exercised. Wiring active inference is its own cycle, not a cable" }),
    ("synth", Unwired { engine: "synth::DenyByDefaultSynth — the OSS reference, which refuses BY DESIGN (v2.42.0: OSS never executes synthesised code). No mount point exists, so no flow reaches it. NOTE: cabling it in OSS yields FailsClosed, not Real — the enterprise Extism/gVisor executor is what makes it Real, and it needs the sandbox reachable at all" }),
    // v2.89.0 — v2.67.0's nine joints all still run, and all nine hand-build
    // `IRScope`/`IRWarden` in Rust: a proof of the HANDLER. The gate now also
    // compiles a fixture through the real pipeline and dispatches the node the
    // COMPILER produced, against the same `ReferenceStaticWarden` the runner
    // mounts. Perturbing the DECLARATION kills it three ways — a target outside
    // `targets:`, an empty `approver:`, and `depth: live_network` above the OSS
    // ceiling — so the authorization envelope really comes from the source.
    //
    // v2.89.0 adds the SECOND proof, and it is about timing rather than
    // dispatch: `tests/deploy_gate.rs` deploys a program whose `warden`
    // block sits after two emitting steps, and the mounted backend's inability to
    // reach `depth: live_network` now refuses at /v1/deploy — before the flow acts
    // — instead of on the seventh step, after it already had. The citation below
    // stays pointed at v2.67.0 because that is the gate that proves the engine
    // ANALYSES; v2.89.0 proves only that an unreachable one is refused in time.
    // Two properties, two gates; collapsing them would lose the first.
    ("warden", Attested {
        fixture: "tests/fixtures/warden/authorized_audit.axon",
        gate: "tests/warden_wired.rs",
    }),
    // v2.89.0 — the scope catalog under test is the one the compiler emitted
    // from the fixture's `scope InternalAudit { … }`, not a Rust literal. See the
    // `warden` note above; the same three perturbations kill this citation.
    ("scope", Attested {
        fixture: "tests/fixtures/warden/authorized_audit.axon",
        gate: "tests/warden_wired.rs",
    }),
    // ── Effects & streaming
    // v2.83.0 — `Real` → `Partial`, and the DOWNGRADE is the honest move.
    //
    // The old proof read `tests/stream_runs.rs (v2.67.0 — the body is
    // parsed, lowered and EXECUTED)`. Every word of it was true, and it violated
    // law 4 above: v2.67.0 gave `StreamBlock` a `body: Vec<FlowStep>` and proved
    // THAT runs — but no published block and no paper writes that shape. The
    // specified surface (`the design plan` section 3.7, whose D8 promises "backward compat 100%,
    // cero cambios en `.axon` source files") is `stream<T> { on_chunk on_complete }`,
    // and README block 15 publishes exactly it. Measured: at flow level the
    // published body was a hard parse error; in a step body the whole block was
    // discarded, so block 15's `step Stream` dispatched with `pix_ops=0`,
    // `ask=""`, `output=""` — an EMPTY step whose `Stream.output` the next step
    // reasoned over, and which had left the v2.81.0 ledger on the strength of
    // compiling. The `reason` defect one cycle later, in the table that recorded it.
    ("stream", Partial { gap: "v2.83.0 made the PUBLISHED surface real: `stream<T> { on_chunk: … on_complete: … }` parses at both the step-body and flow positions (it was a hard parse error at flow level and a SILENT DISCARD in a step body), the `<T>` chunk type reaches the IR instead of being eaten by the argument-skip loop, and each arm is a step body so its `output:` lands. BOTH chunk sources are wired: a streaming TOOL bound with `apply:` (README block 15's own shape — the handler rides `unified_stream_handler`, so it sees the chunks the declared `backpressure:` policy let through, never the raw source), and the step's own generation (the design plan's `stream<Token>` case, hooked into `drain_direct`). Neither has a private drain, so a handler cannot drift from the stream it handles. `on_complete` binds the accumulation as `complete` and its output becomes the step's. An `apply:` that is not a registered streaming tool is REFUSED, never demoted to the LLM path — demoting would stream something the author never named (v2.67.0). v2.83.0 added the THIRD arm, `on_error:`, run when the SOURCE fails with the failure bound as `error`; the step then completes with that arm's output (a recovery the author declared, not a swallowed error) and `on_complete` does NOT run, because a stream that broke did not close. It is deliberately NOT a catch-all: a failure raised by the author's own `on_chunk` is tagged and excluded, since a broken handler that silently catches itself and reports the stream as healthy is worse than one that crashes; cancellation is not a failure either. `on_chunk` is SEQUENTIAL and INTERLEAVED — one run per chunk, during the stream, gated by a wire-trace test that a buffered implementation would fail (it passes every other assertion in the file). Gates: axon-frontend/tests/stream_handlers.rs (12, source→AST→IR) + axon-rs/tests/stream_dispatch.rs (16, source→dispatch, incl. README block 15 VERBATIM executing); 7/7 mutations killed. THE GAP: `stream<τ>` is NOT desugared to `effect _StreamBuiltin<T>` + `handle … in …`, so the design plan's D8 claim of one-shot delimited continuations under this primitive is still future work and the algebraic `perform Stream.Yield` bridge stays orthogonal. A handler slower than the producer backpressures the whole step — correct, but a performance property an adopter should be told about. And `on_error` cannot RESUME the stream: it runs once, after the source is already gone" }),
    // v2.89.0 — `parse_effect_row` + the type checker are the engines; v2.40.0's
    // cacheability derivation reads the `effects: pure` proof. Verified on this
    // fixture by perturbation: `effects: <not_an_effect>` fails it with the
    // closed catalog named — "Valid: io, network, pure, random, storage, stream,
    // trust, sensitive, legal, ots, web" — so the row really is decided here.
    ("effects", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/19_run_effects_compliance.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.87.0 — the algebraic-effect system. `effects` (plural, above) is a
    // DIFFERENT thing that shares a word: the `effects: <io, network>` row on a
    // tool declaration. These three are Plotkin/Pretnar handlers.
    //
    // ⚠️ What was measured before v2.87.0, and why it is the sharpest case law 4
    // has produced. `axon-rs/src/effects/` is a real 590-line FSM — handler
    // stack, one-shot delimited continuations (D2), forward-to-outer (D12) —
    // shipped in v1.17.0 with 49 green tests. It was never `Unaudited` and
    // never in `KNOWN_DEBT`, because it was never ADVERTISED: no README badge,
    // no registry entry, nothing to classify. And so nobody ever asked the
    // question that mattered, which is not *"does the engine work?"* (it did)
    // but *"can any program reach it?"*. The answer was no, in the most total
    // way this repo has recorded: ZERO lexer tokens, no AST, no IR emission,
    // and `EffectRuntime::new()` with both call sites inside `#[cfg(test)]`.
    // An unadvertised subsystem is invisible to every gate in this file.
        // v2.89.0 — cited against the PUBLISHED EXAMPLE, not a fixture written for
    // the gate. That is deliberate and stronger: `examples/algebraic_effects.axon`
    // is what this repository shows an adopter, so if it rots the row goes red, and
    // it is the file people actually read. `examples_compile` already
    // proves it COMPILES; v2.87.0's dispatch gate runs the machine from inline source.
    // This joins the two ends — the published program, dispatched.
    //
    // v2.87.0's finding: the Plotkin/Pretnar engine from v1.17.0 (590 lines, 49 tests) had
    // ZERO lexer tokens. There was no grammar. It was invisible to every gate
    // because it was never ADVERTISED — no badge, nothing to classify.
    //
    // Renaming the declared `effect` or the `handle` target kills the citation.
    ("effect", Attested {
        fixture: "examples/algebraic_effects.axon",
        gate: "tests/effects_from_the_example.rs",
    }),
        // v2.89.0 — see the `effect` note above; same published example and gate.
    ("handle", Attested {
        fixture: "examples/algebraic_effects.axon",
        gate: "tests/effects_from_the_example.rs",
    }),
        // v2.89.0 — see the `effect` note above; same published example and gate.
    ("perform", Attested {
        fixture: "examples/algebraic_effects.axon",
        gate: "tests/effects_from_the_example.rs",
    }),
    ("@contract_tool", Unaudited),
    ("@csp_tool", Unaudited),
    // ── Knowledge navigation (PIX · MDN)
        // v2.89.0 — was `pix_navigator`. Declaration path only; v2.13.0 owns the
    // navigation itself. The declared `depth: 4` reaches the IR — v2.83.0 found
    // `pix_ops` being read by nobody, so a declared field arriving at a consumer is
    // exactly the property worth pinning here.
    ("pix", Attested {
        fixture: "tests/fixtures/declarations/memory_pix_store.axon",
        gate: "tests/declaration_laws.rs",
    }),
    ("navigate", Partial { gap: "v2.67.0 F11 — three REAL deterministic engines (MDN store-sourced, MDN in-memory, PIX), BUT with no indexable source in scope it falls back to an LLM prompt that INSTRUCTS the model to fabricate a provenance trail. The one live v2.63.0 left in the tree" }),
    ("drill", Partial { gap: "v2.67.0 — real subtree navigation when a source is in scope; degrades to a placeholder string otherwise" }),
    ("trail", Partial { gap: "v2.67.0 — reads the real breadcrumb `navigate` seeds; falls back to a placeholder when no navigate ran. Inherits F11: a trail harvested from the LLM fallback is confabulation wearing an audit's clothes" }),
    // v2.89.0 — `mdn` (signed Epistemic PageRank, v2.12.0–v2.14.0) is the engine and
    // has its own unit coverage; this citation is the PATH: a published program
    // declaring a `corpus`, compiled from disk and executed.
    ("corpus", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/06_mdn_navigate_graph.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // ── Advanced cognition & trust
    ("psyche", Unaudited),
    ("ots", Partial { gap: "v2.83.0 made the dispatch layer REAL: `apply_ots_to_target` (identity, v2.67.0 F18) is deleted; `run_ots_apply` resolves the declaration, consults the name-keyed `ots_registry` (shield_registry's shape — the hook that F18 said did not exist, now tested by registering a transformer and watching it fire), propagates transformer refusals, and REFUSES an unregistered name (identity under a transformation's name fabricates a result). Step-scoped `ots X on y -> b` elevates before generation. THE GAP: no OSS transformer registers by default — any generic default would be the identity lie again — so OSS `ots` refuses at runtime until the media pipeline (crate::ots) or an enterprise vertical registers; the paper's JIT synthesis pipeline remains future work" }),
    ("mcp", Unaudited),
    // v2.89.0 — v2.83.0's account stays true and is worth keeping: the closed
    // loop runs through production dispatch via `mandate_engine.rs` (e = 1 − CSR
    // from `pem::semantic_validator`, PID from `pem::density_matrix`,
    // Converge(e,ε,N) with Breach fail-closed, declared-D premise discharge,
    // |C|=0 refused before any token), driven by `run_mandate_apply` + the
    // the design decision step guard (buffered — attempts never stream). Tier 1 logit-bias
    // bans on the OpenAI-compat wire, Tier 2 CSP feedback everywhere. Gates:
    // mandate_engine (16), algebraic_handlers MandateApply incl. the F18
    // regression (an unresolved name REFUSES, never identity),
    // openai_compat logit_bias wire, stability_band.rs,
    // mandate_grammar.rs — 9/9 dispatch-chain mutations killed.
    // None of those reads a file, so none could satisfy Law 4. This does.
    ("mandate", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/13_compliance_persona_mandate.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — v2.83.0's account stays true and is worth keeping here: the
    // ΛD engine (`lambda_data.rs`: ψ = ⟨T,V,E⟩, four invariants, compose with
    // c_out ≤ min(c_i); `lambda_runtime.rs`: Theorem 5.1 enforced at apply, only
    // raw data may carry c = 1.0) runs on the PRODUCTION dispatch path.
    // `apply_lambda_data` — the v2.67.0 F18 placeholder string — is deleted, an
    // undeclared lambda REFUSES, and the step-scoped form elevates BEFORE prompt
    // interpolation. Gates: the lambda_tools elevation suite incl. the F18
    // regression + apply_grammar.rs; mutations C1/C2/C5 killed.
    // The citation below is the one Law 4 asks for: source on disk → dispatch.
    ("lambda", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/08_rate_limiting_lambda.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // ── Deterministic compute
    // v2.83.0 — the proof now cites a gate whose INPUT is `.axon` source
    // (law 4). The v2.67.0 citation stayed, because it is still true and still
    // the engine's own gate — but on its own it was the v2.83.0 shape: it proved
    // `compute` evaluates a v2.26.0 expression while the form every published block
    // writes (`input:` / `output:` / `logic { let … return }`) did not parse at
    // all. Three of the four README computes were in the v2.81.0 ledger for it.
    // v2.89.0 — `compute_real.rs` stays the semantic gate (v2.67.0
    // deleted the placeholder string that a downstream step consumed as if it
    // were a number). It builds its source inline, and until this fixture NO
    // `.axon` in the repository declared a `compute` at all — the third such
    // primitive, with `json` and `budget`.
    ("compute", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/18_refine_grad_compute.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // ── Reactive processes & platform boundary
    // v2.89.0 — was the bare module name `daemon.rs`. The delivery proof is real
    // and stays (it lives in that module's unit tests), but it builds its source
    // inline, so nothing on disk held the program. This citation drives v2.31.0's
    // headline from a fixture: a producer emits on a typed channel, the bus — built
    // FROM the compiled IR, so the channel is registered because the PROGRAM
    // declares it — carries the event, and the consumer daemon's `listen` body runs
    // with the payload bound. v2.4.0's `axon-W009` said that delivery did not exist.
    //
    // Emptying the `listen` body or renaming the daemon kills the citation.
    ("daemon", Attested {
        fixture: "tests/fixtures/daemon/intent_learner.axon",
        gate: "tests/daemon_delivery.rs",
    }),
    ("listen", Partial { gap: "v2.67.0 F7 — TWO disjoint paths sharing one keyword. Inside a DAEMON: real (v2.31.0 outbox → deliver_typed_event → execute_server_flow). Inside a FLOW BODY: binds the canned string \"(awaiting <channel>)\". Sub-gap: a daemon listener body executes ONLY `run <Flow>` steps; any other step type is silently dropped" }),
        // v2.89.0 — was the bare engine name `axon_server`. The fixture is DEPLOYED
    // and its route FETCHED. Perturbing the declared `path:` kills it, and so does
    // swapping `execute:` — but only after the test was strengthened to assert the
    // step name that belongs to the declared flow. The first version checked only
    // that `a` flow ran, which does not discriminate between two flows of the same
    // shape; the mutation found that weakness in the TEST.
    ("axonendpoint", Attested {
        fixture: "tests/fixtures/endpoint/served_routes.axon",
        gate: "tests/default_on_and_tool_call.rs",
    }),
        // v2.89.0 — `axpoint` claims to be an ALIAS of `axonendpoint` (same
    // TokenType). That is a claim only a program can check, so the fixture writes
    // one route each way and the gate fetches BOTH, asserting each runs the flow it
    // declares and not the other's.
    ("axpoint", Attested {
        fixture: "tests/fixtures/endpoint/served_routes.axon",
        gate: "tests/default_on_and_tool_call.rs",
    }),
        // v2.89.0 — was `wire_integrations::run_{persist,retrieve,mutate,purge}`.
    // `backend: in_memory` follows v2.67.0's precedent deliberately: this proves the
    // DECLARED surface — the closed backend catalog, the isolation level, and the
    // v1.31.0 inline schema riding the artifact — not the Postgres wire, which
    // `typed_store_integration` owns. A store whose declared shape reaches no consumer
    // is the v2.67.0 shape, so `column_schema` being present is the assertion.
    ("axonstore", Attested {
        fixture: "tests/fixtures/declarations/memory_pix_store.axon",
        gate: "tests/declaration_laws.rs",
    }),
    // ── Cognitive I/O (λ-L-E, v1.1.0–v1.3.1)
    //
    // v2.67.0 F14 + the v1.4.0 REFRAME: this family is not merely unwired — it is
    // UNREACHABLE BY CONSTRUCTION. There is no FlowStep, no IRFlowNode and no
    // daemon field that can consume any of them. The kernels are real,
    // well-written and unit-tested; the language never grew a way to reach them.
    // Founder-ratified: designing that consumption surface is v2.67.0+.
    // v2.89.0 — v2.67.0's flagship is the difference between the resource being a
    // WIRE and being a LABEL: before it, `capacity:` was declared, type-checked,
    // lowered into the IR, advertised in the README as a pool cap — and read by
    // nothing. Every pool was 10, always, for everyone. Its gates hand-build
    // `IRResource`/`IRAxonStore`, which proves the REGISTRY; this fixture is the
    // program an adopter writes, and the store deliberately declares no
    // `connection:` so the DSN can only come from the resource's config key
    // (axon-T944). Three perturbations kill it: changing the declared `capacity:`,
    // changing the `endpoint:` config key, and pointing `resource:` at a name that
    // does not resolve.
    ("resource", Attested {
        fixture: "tests/fixtures/resource/pooled_store.axon",
        gate: "tests/resource_is_a_wire.rs",
    }),
    ("fabric", Partial { gap: "section 119.e made provider/region DECIDE things, on the line the knowledge doc draws (fabric describes what the runtime expects to find; it is explicitly NOT infrastructure-as-code, so `provision` is not what it owed). COMPILE TIME: `axon-E041` refuses a region that cannot belong to the declared provider (the doc's own `provider: aws region: \"eastus\"` example) and `axon-E042` refuses a compliance obligation the substrate cannot satisfy (the doc's own \"a GDPR-tagged manifest deployed to a non-EU region is rejected\") — with an UNDETERMINABLE jurisdiction counted as a violation, never a pass. RUNTIME: a tool whose resource lives `within` a fabric carries that fabric's (provider, region) on its binding and into its audit row — the doc's compliance propagation, made a fact — and a `within:` naming an undeclared fabric REFUSES the binding. Gates: axon-frontend/tests/fabric_substrate.rs (10) + substrate unit suite (11) + axon-rs/tests/fabric_runtime.rs (5); 6/6 mutations killed. THE GAP: the provider catalog is open by design, so a provider whose region shape this compiler does not know is accepted UNVALIDATED (visible via ProviderShape::Unvalidated) — and `zones`/`ephemeral`/`shield:` are still consumed by nothing, so a fabric-level shield does not yet wrap resource acquisition" }),
    ("manifest", Partial { gap: "v2.67.0 F14 — the κ/compliance half IS genuinely consumed (it feeds attestation + the audit scorer); the \"desired shape\" half is dead" }),
        // v2.89.0 — the readings that drive the immune's baseline come through this
    // `observe` declaration, sampling a source adapter the gate registers. A
    // refused observation feeds nothing downstream (deny-by-default).
    ("observe", Attested {
        fixture: "tests/fixtures/cognitive_io/immune_reflex_heal.axon",
        gate: "tests/immune_fires.rs",
    }),
    // v2.89.0 — v2.67.0 already deploys through the real path and measures
    // REAL Jaccard drift between the manifest's desired shape and the observed
    // world (before it, the loop compared the belief against itself and reported
    // 0.0 for every world). The fixture makes that program a file. Three
    // perturbations kill it: widening `tolerance:` past the measured drift,
    // changing `on_drift:` to `noop`, and shrinking the manifest — the last
    // proving the drift is computed against the DECLARATION, not a constant.
    ("reconcile", Attested {
        fixture: "tests/fixtures/cognitive_io/reconcile_drift.axon",
        gate: "tests/reconcile_drift.rs",
    }),
    // v2.89.0 — the sharpest case in the whole v2.67.0 line, and worth restating
    // because this fixture is what makes it checkable from source: the kernel was
    // NEVER broken. `LeaseKernel` had acquire/use_token/release, the CT-2 Anchor
    // Breach and all three `on_expire` policies, and its unit tests passed for
    // years. IT HAD NO SUBJECT — the README promised that using a resource after
    // expiry is a breach, in a language where a flow could not USE a resource at
    // all. A guarantee about using a thing that cannot be used is not weak; it is
    // VACUOUS: unviolatable, and therefore unkeepable. v2.67.0 made the store
    // operation the use.
    //
    // Perturbing the DECLARATION kills it: widening `duration:` past the clock we
    // advance, and changing `on_expire:` away from `anchor_breach`.
    ("lease", Attested {
        fixture: "tests/fixtures/resource/leased_store.axon",
        gate: "tests/lease_breach_fires.rs",
    }),
    // v2.89.0 — v2.67.0 exercised `ensemble` ONLY as a refusal (an unsatisfiable
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
        fixture: "tests/fixtures/cognitive_io/ensemble_quorum.axon",
        gate: "tests/cognitive_io_deploy.rs",
    }),
    // v2.89.0 — ADJUDICATED, because two different things in this codebase
    // are called "topology" and the v2.89.0 audit tripped over it. This badge is
    // the SESSION-TYPE liveness checker (Honda), which is real and compile-time.
    // It is NOT `topology.rs`'s `TopologyBackend`/`ReferenceTopology` (Betti
    // numbers / persistent homology), which is one of `savant`'s three unwired
    // engines and is carried by that row. The proof below does not overclaim —
    // it names exactly what it means — but a reader scanning for "topology" will
    // find the unwired engine first, so the disambiguation lives here.
    // v2.89.0 — the session family and `topology` are COMPILE-time claims
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
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("session", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("send", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("receive", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("select", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("branch", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
        // v2.89.0 — v2.67.0 already deploys through the REAL `POST /v1/deploy`,
    // so the path was never in doubt; what it lacked was a program on DISK, which
    // is what lets a build catch the citation going stale. The KL sensor detects a
    // real deviation from a LEARNED baseline (wiring it exposed a kernel bug that
    // made it structurally blind). Perturbing the fixture's `window: 4` kills the
    // test — the declared learning window is honoured.
    ("immune", Attested {
        fixture: "tests/fixtures/cognitive_io/immune_reflex_heal.axon",
        gate: "tests/immune_fires.rs",
    }),
        // v2.89.0 — see `immune`. Perturbing the fixture's `action: quarantine`
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
        fixture: "tests/fixtures/cognitive_io/immune_reflex_heal.axon",
        gate: "tests/immune_fires.rs",
    }),
        // v2.89.0 — see `immune`. The HealKernel is registered from the compiled
    // IR and renders a decision under the fixture's declared `mode: audit_only`.
    ("heal", Attested {
        fixture: "tests/fixtures/cognitive_io/immune_reflex_heal.axon",
        gate: "tests/immune_fires.rs",
    }),
    // v2.89.0 — DOWNGRADED from `Real`, deliberately, and this is the row that
    // shows the ratchet doing what it was built for.
    //
    // Measured 2026-08-14: `compliance: [NOT_A_FRAMEWORK]` on an `axonendpoint`
    // COMPILES CLEAN. In the same fixture, `effects: <not_an_effect>` is refused
    // with its closed catalog named. `compliance:` is a free-string field.
    //
    // It could have been attested — a fixture declaring it compiles and passes —
    // and that citation would have proven the field EXISTS, not that it GOVERNS.
    // Writing it would have been the exact species of claim v2.89.0 exists to remove,
    // committed by the cycle that removes it. So the row states the gap instead.
    //
    // This is the fourth recorded instance in this project of "a free string field
    // breeds an imaginary catalog", and the most exposed: the values are `PCI_DSS`,
    // `SOX`, `HIPAA` — read by a regulated adopter as an assertion. The ESK maps
    // `shield.scan` to ISO 27001 A.8.23 with `CompileTime` evidence, which is
    // honest about its own strength; this field has no such qualifier.
    //
    // Closing it is compiler work — a closed catalog keyed to what the runtime
    // reaches, and a decision about what declaring a framework OBLIGES — not
    // fixture work. A dedicated cycle is scheduled after v2.89.0's release.
    // v4.0.0 — the FIRST half of that gap is closed, and the row stays
    // `Partial` because the second half is not.
    //
    // What changed: `compliance:` is a closed catalog (`axon-T1214`).
    // `[NOT_A_FRAMEWORK]` no longer compiles, on any of the four declarations
    // that carry the field, and a typo gets its suggestion.
    //
    // Worth recording WHY the law had been missing, because it was not an
    // oversight: the canonical registry and its validating predicate already
    // existed — in `axon-rs::esk::compliance`, which is DOWNSTREAM of the type
    // checker (`axon-frontend` depends on `serde` and nothing else). The
    // catalog was on the wrong side of the dependency edge, so the rule the ESK
    // paper states as compile-time could not be written where it belonged. It
    // had been true in the retired Python interpreter — the paper still cites
    // that test by name — and was lost in the Rust rewrite. v4.0.0 moved the
    // vocabulary up; the metadata stayed down, with a test pinning the two.
    //
    // A side effect worth naming: `axon-T957` compares these strings as a set
    // difference, so a `type` labelled `[HIPPA]` used to be perfectly covered
    // by a shield labelled `[HIPPA]`. Closing the vocabulary makes that shape
    // unrepresentable, so the coverage law keeps its exact logic and stops
    // being satisfiable by nonsense. T957 was not edited.
    //
    // v4.0.0 — the second half is closed, and the row is `Attested`.
    //
    // The three defects the v4.0.0 gap named, each with its fix:
    //
    // 1. THE CHANNEL SEAM. `TypedEventBus::from_ir_program` handed both
    //    production callers (`daemon.rs`, `runner.rs`) the permissive
    //    `|_,_| true` — a shield covering NOTHING satisfied a channel
    //    carrying PHI at `publish`, the operation that extrudes a capability
    // to outside parties (D8). v4.0.0 derives the predicate from the SAME
    //    IR the bus registers channels from, and adds the missing dual of
    //    T957 at CHECK time: `axon-T1215` refuses the uncovered DECLARATION
    // (a mid-flow refusal is the v2.89.0 shape). Both doors are exercised
    //    from the same `.axon` sources in the cited gate. T1215's first
    //    catch was our own published example: `mobile_channels.axon`
    //    documented "compliance MUST cover" in a comment and then declared a
    //    second-order channel relaying κ=PCI_DSS with no shield at all.
    //
    // 2. PRESENCE SCORING. The audit engine granted
    //    `has_compliance_annotation` on `!compliance.is_empty()` anywhere,
    //    and on that feature the risk register asserted MITIGATION and the
    //    gap analyzer marked C1.1/P1.1/P6.1/A.5.34 satisfied. The feature is
    //    now `compliance_coverage_holds` — granted iff at least one
    //    regulated boundary exists and EVERY one names a shield whose κ
    //    covers the data's κ (`esk::audit_engine::coverage`, one definition
    //    shared by both scorers). A label nothing carries grants nothing.
    //
    // 3. THE VOCABULARY (v4.0.0, recorded above): closed catalog, T1214.
    //
    // What a declared framework now OBLIGES, stated precisely: κ classes are
    // machine-checked coverage obligations — refused at declaration when
    // uncovered (T957 endpoints, T1215 channels), refused at publish for IR
    // that never met the checker, and scored by the audit engine only when
    // the rule HOLDS. What it still does not oblige, deliberately: the
    // SEMANTIC content of each regulation (what HIPAA requires operationally)
    // is the adopter's shield/flow design, not something a compiler can
    // infer — the ESK maps controls to evidence with that qualifier stated.
    ("compliance", Attested { fixture: "tests/fixtures/channel_kappa/phi_relay.axon", gate: "tests/channel_kappa.rs" }),
    ("component", Partial { gap: "v2.67.0 — the compile-time shield-coverage law over regulated κ IS genuinely enforced (a real set difference). But the component renders NOTHING; the README itself defers the renderer" }),
    ("view", Partial { gap: "v2.67.0 — only referential integrity is checked. No `route` check, no session-typed-reactivity check, and it renders nothing" }),
    // ── Enterprise I/O (v2.37.0–v2.40.0)
    // v2.89.0 — was `Real { proof: "cache_runtime — cacheability derives
    // from the type system's `effects: pure` proof (v2.40.0)" }`. That proof cited
    // a COMPILE-TIME property as evidence of a runtime, named no test file, and
    // so passed Law 3 vacuously and Law 4 not at all. Measured 2026-08-13:
    // `CacheRuntime::dispatch` has ZERO production callers (only its own
    // tests), `CacheBackend` has no enterprise implementor, and the design plan's own
    // plan records v2.40.0 as "deferred, not built". `backend:`, `ttl:`,
    // `key_params:`, `invalidate_on:` and `default_policy:` are inert in BOTH
    // flavours — `backend: redis` is not a downgrade to in-process, there is no
    // cache. Shipped as `Real` in 2.88.0; this is the row with public exposure.
    //
    // v2.89.0 — CABLED. The fixture deploys a `pure` tool under a default
    // cache and calls it twice; the gate counts hits on a real upstream and
    // finds ONE. It also pins the three properties a memoiser is worthless or
    // dangerous without: `cache: none` still computes twice, a FAILED call is
    // never stored, an `emit` on an `invalidate_on:` channel flushes,
    // and a second TENANT does not read the first one's entry.
    //
    // Worth recording that v2.89.0 called this "the cheapest cable in the
    // class" and was wrong on every count. `cache` had THREE declared
    // consumers, not one: `use Tool(…)`, a streaming `apply:` tool, and
    // `retrieve … cache:` — the last of which had lived in the IR since v2.40.0
    // with no reader. The streaming one cannot be memoised at all (a chunk
    // sequence is not a value), so v2.89.0 added `axon-T1213` to refuse the
    // combination rather than accept a declaration it could not honour. And
    // the cross-tenant assertion could not even be made honestly until
    // v2.89.0 fixed the tenant that two of the three execution doors were
    // losing — see `cache_hits.rs`.
    ("cache", Attested {
        fixture: "tests/fixtures/cache/memoised_pure_tool.axon",
        gate: "tests/cache_hits.rs",
    }),
    ("voice", Unaudited),
    ("shell", Unaudited),
    ("path rewrite", Unaudited),
    ("PASETO", Unaudited),
    // ── Session types (v2.3.0)
    // v2.89.0 — v2.67.0 already deploys through the real path and boots a real
    // TCP listener; its only Law 4 gap was that the program was a Rust `const`.
    // The finding it closed is why the citation matters: the OSS server had NO
    // WebSocket route — not a stub, none — while the README led with session-typed
    // dialogue as the headline feature; and when a route appeared, every socket got
    // a hardcoded canonical chat schema, so the protocol proven at compile time was
    // not the protocol enforced.
    //
    // Perturbing the fixture kills it two ways: breaking the session's duality
    // (`send Fill` -> `send Order` in the broker's arm), and pointing the socket at
    // an undeclared protocol — which must REFUSE rather than substitute.
    ("socket", Attested {
        fixture: "tests/fixtures/socket/trade_dialogue.axon",
        gate: "tests/socket_served.rs",
    }),
    // v2.89.0 — the surface forms the README badges separately from the bare
    // names above. The same fixture carries all four literally: `send Quote`,
    // `receive Settlement`, and the labelled `select`/`branch` whose arms the
    // duality check pairs one for one.
    ("send T", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("receive T", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("select {ℓᵢ:…}", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("branch {ℓᵢ:…}", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/17_session_duality_topology.axon",
        gate: "tests/parity_corpus.rs",
    }),
        // v2.89.0 — the v2.37.0 upstream fixture DECLARES `backpressure: credit(8)` and
    // narrowing it to `credit(3)` kills that gate, so the declared credit window is
    // what the session runtime honours.
    ("backpressure: credit(k)", Attested {
        fixture: "tests/fixtures/upstream/stt_dialogue.axon",
        gate: "tests/upstream_e2e.rs",
    }),
    ("reconnect: cognitive_state", Unaudited),
    // ── v2.69.0 — the classifications the badge-only gate never asked for.
    //
    // Twenty-two primitives were part of the public promise (registered,
    // corpus-documented, or discussed in the README at length — `step` 136
    // mentions, `type` 79, `document` 25, `budget` 16) and had NO entry here,
    // because the old gate enumerated only the README's `<code>` badges. A
    // gate that guards only the badges guards the packaging. The registry is
    // the enumeration source now (`is_advertised`), and every one of these is
    // classified with v2.67.0's own discipline: Real only with a citable gate,
    // Partial only with a named gap, Unaudited where nobody has looked — an
    // unchecked box is not a passing one.
    //
    // The language core — exercised by essentially every gate in both repos.
    // v2.89.0 — see the `flow` note above; same fixture, same gate. The
    // fixture is a single-`step` flow, so the byte-equal pin IS the step
    // executor's output on both paths.
    ("step", Attested {
        fixture: "tests/fixtures/parity_corpus/banking/01_canonical_loan_decision.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — v2.15.0's unified executor (runner + execute_server_flow, binding
    // flow+persona+context) is what every gate that runs a program goes through,
    // which made the old citation true and unfalsifiable at once. This fixture
    // carries a literal top-level `run` statement, and perturbation confirms it
    // is resolved: `run NoSuchFlowAtAll()` fails with "Undefined flow
    // 'NoSuchFlowAtAll' in run statement".
    ("run", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/19_run_effects_compliance.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — `type_checker` (structural + refinement validation) and
    // `store_schema` (v1.31.0, declared shapes pinned to SQL column types) are the
    // engines. The fixture below DECLARES types and is compiled from disk by the
    // gate before it executes, so the front half of the path is exercised on
    // source an adopter could have written.
    ("type", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/06_mdn_navigate_graph.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — `json_accessors.rs` + `json_lens.rs` stay
    // the semantic gates for v2.26.0's totality (navigation yields null rather than
    // panicking; coercions are honest about a type mismatch). Neither reads a
    // file. The fixture carries both surfaces — open `Json` and the `Json<T>`
    // lens — and is the first published program in the repo to use either.
    ("json", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/16_json_open_navigation.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // The audit chain + π-calc channel family.
    ("ledger", Unaudited),
    ("channel", Partial { gap: "v2.69.0 — durable persistence (v2.31.0 outbox) and the σ-shield egress gate (v2.69.0, tests/channel_shield_egress.rs) are proven; `qos:`/`lifetime:` enforcement is unaudited" }),
    // v2.89.0 — the previous citation (`tests/channel_shield_egress.rs`)
    // is the SEMANTIC gate and still runs: the channel's σ-shield scans on every
    // path by IR-resolution, and a Reject fails closed before any routing. But
    // its dispatch half hand-builds `IREmit`, so it proves the handler, not the
    // path — the v2.83.0 distinction. The parity corpus supplies the path.
    ("emit", Attested {
        fixture: "tests/fixtures/parity_corpus/banking/09_credit_score_reasoning.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("publish", Unaudited),
    ("discover", Unaudited),
    // Governance operators.
    // v2.89.0 — `budget_governs_the_canonical_path.rs` (9/9) still
    // proves the ceiling binds on the canonical `use Tool(…)` path, and stays
    // the semantic gate. What it cannot do is satisfy Law 4: it builds its
    // source inline. v2.69.0 rescued `budget` from being README prose that
    // governed nothing — and it was STILL true, until this fixture, that no
    // published `.axon` in the repo declared one. That is the same shape one
    // level up: a primitive whose runtime is real and whose surface nobody
    // writes.
    ("budget", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/15_tool_budget_governance.axon",
        gate: "tests/parity_corpus.rs",
    }),
    ("window", Unaudited),
    ("cors", Unaudited),
    // The governed-egress trio (v2.60.0 · v2.53.0/v2.62.0 · v2.66.0) — none was ever badged.
    // v2.89.0 — verified by perturbation on this fixture: 
    // `target: notaformat` fails it (v2.53.0's serializer catalog is decided, not
    // parsed).
    ("document", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/20_document_notify_egress.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // v2.89.0 — was `delivery.rs + axon-T920`, a module plus a code.
    //
    // The boundary is stated precisely, because overstating it would be the very
    // defect this cycle closes: this citation proves the DECLARATION path (source →
    // type checker → IR), NOT the egress. `deliver` cannot be dispatched from a
    // fixture at all — `secret:` is a per-tenant key under v2.48.0 custody (axon-T923),
    // so no fixture without a mounted minter reaches the wire. The egress stays
    // proven by v2.60.0's own runtime gates.
    //
    // What the fixture does carry is the substance an adopter meets first, each law
    // watched refusing on the file's own text: T921 (`target:` is a closed
    // system-of-record class), T924 twice — the delivery must declare the `web` it
    // crosses, AND binding `sensitive:*` into a system of record is further
    // processing so the legal basis is not decoration — and T926 (every
    // operation carries an idempotency key).
    ("deliver", Attested {
        fixture: "tests/fixtures/deliver/crm_handoff.axon",
        gate: "tests/deliver_laws.rs",
    }),
    // v2.89.0 — verified by perturbation on this fixture: 
    // `window: 0m` fails it — v2.66.0's mandatory positive window. All five
    // v2.66.0 laws fired when the block was first written wrong, and the fixture
    // keeps their diagnostics as comments.
    ("notify", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/20_document_notify_egress.axon",
        gate: "tests/parity_corpus.rs",
    }),
    // Credentials + custody verbs.
        // v2.89.0 — v2.46.0's gates hand-build `IRCredential`. The contract under test
    // is now the one the compiler emitted from `credential WidgetSession { ttl: 15m
    // grants: [chat.invoke] }`. Perturbation kills it three ways: changing the
    // declared `grants:`, renaming the `as` binding, and — after the helper was
    // fixed to run the TYPE CHECKER — pushing `ttl:` past the 24h ceiling, which
    // now fails with `axon-T894` naming the v2.46.0 service-account surface instead.
    ("credential", Attested {
        fixture: "tests/fixtures/credential/visitor_session.axon",
        gate: "tests/mint_runtime.rs",
    }),
        // v2.89.0 — the flow-body verb, from source: `mint WidgetSession as tok`.
    // The bearer is bound under the name the SOURCE chose, its grants come from the
    // DECLARED contract, and the test still pins the property that matters — the
    // raw token rides neither the outcome nor the wire.
    ("mint", Attested {
        fixture: "tests/fixtures/credential/visitor_session.axon",
        gate: "tests/mint_runtime.rs",
    }),
    // v2.89.0 — a DECLARATION-path citation, and the boundary is structural
    // rather than a testing gap: v2.48.0's runtime gate drives a local tool server on
    // an ephemeral port, and a tool endpoint has no config-key indirection the way
    // v2.37.0 requires for `upstream` — so a static file cannot name it without
    // bending. `rotation_without_revelation` (ENUMERATE + ROTATE(CAS) + USE, no
    // term evaluating to a secret) stays proven by v2.48.0's own gates.
    //
    // What the file carries are the two laws that make the verb governable, each
    // watched refusing: `axon-T898` — the target must be a `backend: secrets`
    // store, because you cannot rotate what is not custody — and `axon-T899` — the
    // exchange tool must be declared, not ad-hoc.
    ("rotate", Attested {
        fixture: "tests/fixtures/credential/rotate_crm_class.axon",
        gate: "tests/declaration_laws.rs",
    }),
    // Session types + quantum bridge.
    // v2.89.0 — v2.37.0's gates hand-build `IRUpstream`, which proves the DIAL.
    // This citation compiles the declaration an adopter writes and dials the same
    // real WebSocket vendor with it. A static fixture works here for the reason
    // `rotate` cannot yet use one: `resolve:`/`secret:` are CONFIG KEYS by v2.37.0's
    // rule, never literals, so the vendor's ephemeral address is supplied by the
    // resolver and nothing about the program bends to the test.
    //
    // Three perturbations kill it: changing the declared `auth:` from header to
    // query (the secret must ride where the DECLARATION says), narrowing the
    // `backpressure: credit(n)` window, and changing the `when` clause that
    // classifies the inbound frame — the last proving the vendor's reply is typed
    // by the fixture's `map:`, not guessed.
    ("upstream", Attested {
        fixture: "tests/fixtures/upstream/stt_dialogue.axon",
        gate: "tests/upstream_e2e.rs",
    }),
    // v2.89.0 — v2.67.0's gates all still run and all hand-build `IRQuant`:
    // a proof of the HANDLER. The gate now also compiles a fixture and dispatches
    // the node the COMPILER lowered, against the same `ReferenceSimulator` the
    // runner mounts. The expected value is ANALYTIC — |0⟩ under Pauli-Z gives
    // ⟨Z⟩ = +1 exactly — so no placeholder satisfies it. Perturbation kills it
    // three ways: flipping the term's sign, renaming the observable, and rotating
    // the carrier off the ground state.
    ("quant", Attested {
        fixture: "tests/fixtures/quant/ground_state_measurement.axon",
        gate: "tests/quant_wired.rs",
    }),
    // v2.89.0 — the observable under test is the one the compiler emitted from
    // `observable GroundEnergy { qubits: 1  term: 1.0 * \"Z\" }`, and the result
    // must NAME it. Renaming the reference in the `quant` header kills the
    // citation; so does flipping the declared term's coefficient.
    // v2.89.0 — as with `warden`, the deploy gate is the timing proof: an
    // `observable` declaring more qubits than the mounted simulator can hold is
    // refused at /v1/deploy. Worth recording WHY that test was hard to write —
    // the first draft declared `qubits: 24` with a one-qubit Pauli string and was
    // refused by `axon-E0785` (declared width must match the terms) before the
    // capacity gate ran at all. The two laws are complementary: E0785 asks whether
    // the DECLARATION is coherent with itself, v2.89.0 asks whether a coherent
    // declaration is REALISABLE on the backend actually mounted.
    ("observable", Attested {
        fixture: "tests/fixtures/quant/ground_state_measurement.axon",
        gate: "tests/quant_wired.rs",
    }),
    // Symbolic differentiation.
    // v2.89.0 — `grad_runtime.rs` stays the semantic gate (it deploys
    // through the REAL /v1/deploy + /v1/execute and checks the evaluated
    // derivative). It builds its source inline. `grad` is SYMBOLIC and happens at
    // COMPILE time — the gradient IS IR under the PCC GradientSoundness witness —
    // so the corpus gate's compile pass is where the claim is exercised.
    ("grad", Attested {
        fixture: "tests/fixtures/parity_corpus/cross_vertical/18_refine_grad_compute.axon",
        gate: "tests/parity_corpus.rs",
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
/// v2.67.0 inherited every one of these. They are the cycle's open debt, and now the
/// compiler holds the ledger.
///
/// # v2.89.0 — this ledger went 0 → 3, and that is the ratchet WORKING
///
/// v2.83.0 drove it to empty and that was true of everything the table classified
/// honestly. It was not true of the table. `cache` was attested `Real` on a
/// proof that named a compile-time property, so its dead runtime was never
/// counted; `savant` and `synth` sat under `Unaudited`, which said *nobody
/// looked* — accurate in v2.67.0, and no longer accurate once someone did.
///
/// **The debt did not grow. It was uncovered.** Three engines that exist, pass
/// their own tests, and are reachable from no published program. Recording them
/// is the entire thesis of v2.89.0 applied to the file that carries it: a ledger
/// that stays empty because a row lies is worth less than one that admits three.
pub const KNOWN_DEBT: &[&str] = &[
    // Tier 4 — implement or retract.
    // `compute` LEFT this ledger in v2.67.0: it is now a real pure function over
    // the v2.26.0 expression language. This is the ratchet turning — the only
    // direction it turns.
    // `hibernate` LEFT this ledger in v2.83.0 (2026-08-07): the walk halts,
    // the continuation parks under the published SHA-256 id, emit wakes it,
    // and lazy expiry refuses lateness. Fifth turn of the ratchet.
    //
    // `mandate` LEFT this ledger in v2.83.0 (2026-08-07): the enforcement loop
    // runs through production dispatch, fails closed, and its name resolving
    // to nothing is a refusal — the exact call that used to be the identity
    // passthrough is now the F18 regression test. Second turn of the ratchet.
    //
    // `lambda` and `ots` LEFT in v2.83.0 (2026-08-07), the same day: the ΛD
    // engine reached the production dispatch path (Real), and ots's dispatch
    // layer + registry hook became real with an honestly-named gap (Partial:
    // no OSS transformer registers by default — a generic default would be
    // the identity lie F18 named). Third and fourth turns.
    // The Cognitive-I/O block.
    //
    // v2.67.0 PAID SIX OF THESE. `observe` · `ensemble` · `immune` · `reflex` · `heal` ·
    // `reconcile` are now driven by the CognitiveIoSupervisor and each cites a gate
    // that proves it runs through the REAL deploy path.
    //
    // My v2.67.0 diagnosis of this family was WRONG in a way worth remembering: I called
    // it a language-design problem ("no verb can reach them"). The language was
    // already complete — the declarations form a dataflow graph and reference each
    // other, and the kernels took the compiled IR directly. **Nobody had ever built
    // the loop.**
    //
    // v2.67.0 PAID TWO MORE. `resource` and `lease` left this ledger.
    //
    // `resource` now governs what actually runs: an `axonstore { resource: Db }`
    // DERIVES its DSN and its POOL SIZE from the resource (`capacity: 20` ⇒ twenty
    // connections — before v2.67.0 that field was read by zero lines of code in either
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
    // kept. v2.67.0 made the store operation the use, and the breach finally fires.
    //
    // ── AND `fabric` STAYS. ────────────────────────────────────────────────────
    //
    // This is the ratchet doing its job on the cycle that was in a position to fudge
    // it. `within:` gave `fabric` subjects, and Separation-Logic disjointness is now
    // unrepresentable by construction (one field ⇒ a resource cannot be in two
    // fabrics). It would be *easy* to call that Real.
    //
    // But `provider` / `region` / `zones` are consumed by NOTHING at runtime — in
    // `LiveHandler::provision` the parameter is literally `_fabrics`, underscored,
    // and that function refuses anyway. A `fabric` still governs nothing that runs.
    //
    // Calling it Real here is exactly the lie v2.67.0 exists to prevent, told by the
    // person who wrote the gate. The ledger only shrinks by what is PROVEN.
    // v2.83.0 (2026-08-07) — `fabric` LEFT this ledger, and with it the
    // ledger itself: provider/region now refuse an impossible substrate and an
    // unsatisfiable obligation at compile time, and reach the running channel
    // and its audit row. SIXTH AND LAST TURN.
    //
    // ═══════════════════════════════════════════════════════════════════
    // v2.67.0 opened this ledger with 22 advertised primitives that were not
    // real. v2.67.0 paid `compute`. v2.67.0 paid six of the Cognitive-I/O block.
    // v2.67.0 paid `resource`/`lease`. v2.69.0 paid the governed channel. v2.83.0 paid
    //  the last five — `mandate` (b), `lambda` + `ots` (c), `hibernate` (d),
    //  `fabric` (e), all on 2026-08-07 — and the list reached EMPTY.
    //
    // v2.89.0 re-opened it at THREE, and the reason is the point of the
    // whole cycle: the list was empty of everything this table classified
    //  honestly, and the table had one row that did not. Each entry below is an
    //  `Unwired` engine — complete, tested, and reachable from no published
    // program. NONE of them is new work discovered by v2.89.0; all three predate
    //  it and were simply not counted.
    //
    //  A KNOWN_DEBT of 0 or 3 does NOT mean every other primitive is Real.
    //  Several are `Partial` with the gap NAMED — a bounded, published limit an
    //  adopter can read and plan around, which is the whole point of the
    //  distinction.
    // ═══════════════════════════════════════════════════════════════════
    //
    // v2.89.0 — `cache` LEFT this ledger. The seam has its call sites (both
    // of them), and `cache_hits.rs` counts one upstream hit for two
    // identical calls, through the real deploy + execute handlers.
    //
    // v2.89.0's entry said it would leave via "the cheapest cable in the class,
    // because the engine is already there". The engine was indeed already
    // there and the estimate was still wrong: three consumers rather than one,
    // a seam whose synchronous shape did not fit the async dispatch path, a
    // combination the language had to start refusing (`axon-T1213`), and a
    // tenant that two of the three execution doors were losing outright
    // (v2.89.0). Left as a note to the next reader of this file: an entry
    // that estimates its own cost is estimating something nobody has measured.
    //
    // v2.89.0 — three tested engines (HRR codec, VFE/EFE inference,
    // Betti/PHC topology), zero callers outside their own tests. It LEAVES when
    // active inference is wired, which is a cycle, not a cable.
    "savant",
    // v2.89.0 — `DenyByDefaultSynth` exists and has no mount point. NOTE
    // the honest ceiling: cabling it in OSS yields `FailsClosed` (v2.42.0 — OSS
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
             That omission is exactly how v2.67.0 happened: `warden` and `quant` had a README \
             badge, a registry entry, a parser production AND a dispatch arm — and were no-ops.",
            missing.len(),
            missing.join("\n  ")
        );
    }

    /// **LAW 1b (v2.69.0) — the registry is the enumeration source.**
    ///
    /// Every `PRIMITIVE_REGISTRY` entry with `is_advertised: true` must be
    /// classified here. This is the law the badge-only gate lacked: `budget`
    /// was discussed in the README seventeen times, badged zero, tracked in NO
    /// table — and its dead runtime (v2.69.0) survived every green build.
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
             The registry is the enumeration source (v2.69.0). A primitive that is part of \
             the public promise cannot exist without someone stating, on the record, what \
             its runtime does. That omission is exactly how `budget` (v2.69.0) happened.",
            missing.len(),
            missing.join("\n  ")
        );
    }

    /// **LAW 1b, visibility half (v2.69.0)** — an advertised primitive must be
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

    /// **LAW 1b, negative half (v2.69.0)** — an `is_advertised: false` registry
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
    /// This is the law that makes v2.67.0's findings durable. A *new* unkept promise
    /// is a red build. The existing ones are a ledger the compiler holds — not a
    /// paragraph in a document nobody rereads.
    ///
    /// v2.89.0 widened it from `is_unimplemented()` to `owes_a_debt_entry()`
    ///. Had `Unwired` been added beside this law instead of inside it,
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
    /// v2.89.0 TIGHTENED the cap 14 → 3. The old cap was slack left over from
    /// v2.67.0's opening balance: with the list at zero, eleven rows could have
    /// been added before any gate complained. A ratchet with slack is a ratchet
    /// that has stopped measuring. The three entries are the `Unwired` engines
    /// v2.89.0 uncovered; the next move is 3 → 2 when v2.89.0 cables `cache`.
    #[test]
    fn the_debt_ledger_only_shrinks() {
        assert!(
            KNOWN_DEBT.len() <= 2,
            "KNOWN_DEBT grew to {} — the ratchet only turns one way. v2.89.0 re-baselined it at 3 \
             (the `Unwired` engines it uncovered) and TIGHTENED the cap to match; v2.89.0 cabled \
             `cache` and took it to 2. A new entry is not a row to add, it is a bug to fix.",
            KNOWN_DEBT.len()
        );
    }

    /// **v2.89.0 — the `Unwired` population is pinned, and may only shrink.**
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
            n <= 2,
            "the Unwired population grew to {n} — v2.89.0 measured 3 (cache · savant · synth) \
             and v2.89.0 cabled `cache`, leaving 2. A new Unwired row means a primitive was \
             advertised with an engine nobody calls; wire it or retract it."
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

    /// **v2.89.0 — LAW 4's shape.** An `Attested` claim must name a `.axon`
    /// FIXTURE that exists and a GATE that exists.
    ///
    /// This is the half of Law 4 that the SHAPE can enforce. The other half —
    /// that the gate actually READS that fixture — is v2.89.0, and it is the
    /// half that makes the citation mean something. Landing the shape first is
    /// deliberate: re-attesting sixty-four rows is sixty-four
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

    /// **v2.89.0 — LAW 4, ENFORCED.** The cited gate must actually reach the
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
    ///    corpus-driven idiom `laws_corpus` and `parity_corpus`
    ///    already use). A gate naming only `tests/fixtures` proves nothing and
    ///    is rejected: the path must be at least one segment deeper.
    ///
    /// **Not** enforced, and it must be said plainly, because a ratchet that is
    /// believed to prove more than it does is the exact defect this file
    /// exists to close: nothing here decides whether the gate *meaningfully
    /// exercises the primitive*. That stays a human attestation — the premise
    /// of [`RuntimeStatus`] from the beginning, *because no linter can decide
    /// it*. What v2.89.0 removes is the ability to cite something that cannot be
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
                 PATH — v2.83.0's lesson, and the hole `cache` went through. Name the fixture (or \
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

    /// **v2.89.0 — the README's VERSION is a build input too.**
    ///
    /// This file has parsed the README as a build input since v2.67.0 — for the
    /// primitive badges. Nobody pinned the version, and it drifted until the
    /// README disagreed with itself: the title said `v2.75.0`, the badge said
    /// `v2.85.0`, the prose said `v2.75.0`, and the shipped crate was `2.88.0`.
    /// Three numbers in one file, none of them right.
    ///
    /// That is the same species of unverified public claim v2.89.0 spent itself
    /// removing from the ledger, sitting two screens above the ledger. The gate
    /// costs two lines and closes it: every `vX.Y.Z` the header block asserts
    /// about THIS crate must equal `axon-rs/Cargo.toml`'s version.
    ///
    /// Scope note — deliberate, and narrower than the first draft: only the
    /// title, the badges, and the two-repositories note are checked. The
    /// primitive-census `<details>` block sits between them and carries
    /// HISTORICAL labels ("Session types (v2.3.0)"), which say when a feature
    /// shipped and must never move. The first version of this gate swept the
    /// whole header and immediately flagged those — correctly refusing to let
    /// me conflate "what ships now" with "when this arrived".
    #[test]
    fn the_readme_header_states_the_version_the_crate_actually_is() {
        let cargo = std::fs::read_to_string(repo_root().join("axon-rs/Cargo.toml"))
            .expect("axon-rs/Cargo.toml must be readable");
        let version = cargo
            .lines()
            .find_map(|l| l.strip_prefix("version = \""))
            .and_then(|r| r.split('"').next())
            .expect("axon-rs/Cargo.toml must declare a version");

        let readme = std::fs::read_to_string(repo_root().join("README.md"))
            .expect("README.md must be readable");
        let header_end = readme
            .find("## What is AXON?")
            .expect("the README must still have its `What is AXON?` heading");
        // Everything above the prose, MINUS the primitive-census block, whose
        // version labels are historical.
        let (census_start, census_end) = (
            readme.find("<details>").unwrap_or(header_end),
            readme
                .find("</details>")
                .map(|i| i + "</details>".len())
                .unwrap_or(header_end),
        );
        let header = format!(
            "{}{}",
            &readme[..census_start.min(header_end)],
            &readme[census_end.min(header_end)..header_end]
        );
        // …and MINUS the `axon-enterprise` note, which states a DIFFERENT
        // product's version.
        //
        // That note has always been there; it was invisible to this gate only
        // because the gate matched the literal `"v2."` while enterprise was on
        // `v3.x`. The README says the two lines "diverge by design", and at
        // axon-lang v3.0.0 they stopped diverging in the MAJOR — so a purely
        // lexical scan can no longer tell `v3.0.0` (this crate) from
        // `v3.100.0` (the control plane). The exclusion has to be named rather
        // than emergent, exactly like the census block above.
        //
        // The note is a blockquote, so it ends at the first blank line.
        let header: String = match header.find("axon-enterprise") {
            None => header,
            Some(at) => {
                let tail_from = header[at..]
                    .find("\n\n")
                    .map(|i| at + i)
                    .unwrap_or(header.len());
                // Rewind to the start of the blockquote that mentions it.
                let quote_from = header[..at].rfind("\n>").map(|i| i + 1).unwrap_or(at);
                format!("{}{}", &header[..quote_from], &header[tail_from..])
            }
        };
        let header = header.as_str();

        let expected = format!("v{version}");
        // v2.89.0 wrote this anchor as the literal `"v2."`, and v3.0.0 is
        // where that came due: the header stated `v3.0.0`, the scan found zero
        // `v2.` claims, and the gate failed with "the header must state the
        // version somewhere" — reporting a restructured README when nothing had
        // moved. A gate keyed to the CURRENT major cannot survive the next one,
        // so it now matches any `vN.N.N` and compares every one it finds.
        let claims: Vec<&str> = header
            .match_indices('v')
            .filter(|(i, _)| header[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
            .map(|(i, _)| {
                let rest = &header[i..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'v'))
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .filter(|c| c.matches('.').count() == 2)
            .collect();

        assert!(
            !claims.is_empty(),
            "the README header must state the version somewhere — if this fires, the \
             header was restructured and this gate needs its anchor updated"
        );
        for c in &claims {
            assert_eq!(
                *c, expected,
                "the README header claims `{c}` but axon-rs/Cargo.toml is `{version}`. \
                 Every version the header asserts about THIS crate must match the crate. \
                 (Historical `vX.Y.Z (cycle N) adds…` references live BELOW the header and \
                 are deliberately out of scope — they say when something shipped.)"
            );
        }
    }

    /// v2.89.0 — the depth rule inside [`gate_mentions_a_path_reaching`] is the
    /// load-bearing half, so it gets its own test rather than trust.
    ///
    /// Without it, a gate that merely says `tests/fixtures` anywhere in its
    /// source could claim EVERY fixture in the repo — which would make Law 4
    /// enforceable in form and vacuous in fact, the precise failure this whole
    /// cycle exists to end.
    #[test]
    fn naming_the_fixtures_root_alone_does_not_reach_a_fixture() {
        let fixture = "tests/fixtures/parity_corpus/banking/01_canonical_loan_decision.axon";

        // Too shallow: the corpus root is not named.
        assert!(!gate_mentions_a_path_reaching("reads tests/fixtures/", fixture));
        assert!(!gate_mentions_a_path_reaching("reads tests/", fixture));
        // An unrelated corpus does not reach it either.
        assert!(!gate_mentions_a_path_reaching(
            "reads tests/fixtures/laws",
            fixture
        ));
        // Deep enough: the corpus directory that actually holds it.
        assert!(gate_mentions_a_path_reaching(
            "reads tests/fixtures/parity_corpus/",
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

    /// **v2.89.0 — the legacy `Real { proof }` population is pinned, and may only
    /// shrink.** v2.89.0 drives it to zero.
    ///
    /// **Sixty-nine** rows still carry the free-prose citation whose conditional
    /// check let `cache` through: Law 3 only verifies a proof that happens to
    /// name a test file, and Law 4 — written since v2.83.0 — never ran at all.
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
            n <= 0,
            "the legacy `Real {{ proof }}` population grew to {n} — v2.89.0 measured 69 and v2.89.0 \
             drives it to zero (69 → 0). A NEW claim of reality must be \
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

    /// The unaudited population is pinned. v2.67.0 could not reach everything, and
    /// says so; but the number may only go down. An unchecked box is not a
    /// passing one.
    ///
    /// v2.69.0 RE-BASELINED this pin 18 → 23 — not because auditing went
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
        // v2.89.0 — restored to 23 after a blind string replace walked this pin
        // down alongside the legacy-`Real` pin, which happened to hold the same
        // number. The Unaudited population is 20 today (`savant` and `synth` left it
        // for `Unwired` in v2.89.0); it is NOT re-baselined here, because a pin that
        // moves as a side effect of editing another pin has stopped being a ratchet.
        assert!(
            n <= 23,
            "the Unaudited population grew to {n} — v2.67.0 left 18, v2.69.0's census widening \
             re-baselined at 23. Auditing is the only direction."
        );
    }
}
