# Interruptible Sessions: A Resumable, Time-Bounded, One-Shot Effect over Session-Typed Dialogue in AXON

**AXON Research Paper — Concept Proposal v1.0 (§Fase 79.a deliverable)**
**Subject:** Algebraic effects & handlers (Plotkin–Pretnar), one-shot delimited continuations, timed session types / timed automata (difference-bound matrices), arithmetic-refined session types (Rast), linear/affine resource discipline
**Integration:** the AXON `session`/`socket` primitive (§41), the credit-refined backpressure index (§41.c), cooperative cancellation (`CancellationFlag`, §33.f), the `cognitive_state` sealed reconnection snapshot (§41.g), the deterministic `ReplayToken` chain (§74.e), and Proof-Carrying Code (§51)

> **Deliverable discipline (§8.7, "paper before parser").** This document is 79.a: a reviewed
> specification with **zero grammar changes**. It fixes the semantics 79.b–79.g implement. Every
> theorem below states its premises explicitly; the two places where a bound rests on an assumption
> the runtime substrate cannot discharge (the per-step latency premise of Theorem 2; the
> flush-boundary definition of "delivered") are named as premises, not hidden — because a
> proof-carrying compiler that ships a bound it cannot keep is the exact W005 failure §77.a fixed
> for shields, and §79 does not repeat it for time.

---

## Abstract

Every production voice-AI stack — Vapi, Retell, Bland, Synthflow, the OSS Vocode, and every
Python-orchestrated agent pipeline (LangGraph, hand-rolled `asyncio`) — handles *barge-in* (the
caller speaking over the agent) the same way: a voice-activity callback fires, application code
cancels the in-flight text-to-speech generation, and whatever context existed mid-utterance is
destroyed. `asyncio.Task.cancel()` does not hand back a resumable value; it tears down state. The
agent's next turn therefore starts from scratch, the reaction latency is a p99 measured in
production rather than a bound proved before deployment, and a disputed call is reconstructible
only from application logs — evidence of intent, not a deterministic replay.

This paper re-founds interruption as a **language feature, not a callback**. We model `interrupt`/`resume`
as a **linear, one-shot algebraic effect** (Plotkin–Pretnar handler semantics), where the captured
continuation is not a general delimited continuation over arbitrary computation — the runtime
substrate (Rust/tokio) has no such primitive, and neither does Python — but a **reified
continuation at the granularity of a session step**: the session-type cursor plus its credit
window, exactly the value AXON's `cognitive_state` snapshot (§41.g) already seals. We prove three
theorems: (1) the interrupt/resume combinator preserves the §41.a **connection law** (duality is
maintained across the interrupted region and its handler's two exits); (2) under a **difference-bound-matrix
(DBM)** timed extension of the session grammar — the same decidable-arithmetic family as §41.c's
Presburger credit checks — the control-plane's *signal → cancellation-acknowledged* reaction path
carries a statically-derived **worst-case step bound**, which becomes a soft-real-time **time**
bound under one explicitly-stated per-step-latency premise, runtime-verified by a fail-closed
watchdog; (3) `resume` restores the pre-interrupt credit window **exactly** — a genuine symmetry,
not an ad-hoc reset — because the handler runs on a separate declared budget. We give the decidable
checking algorithm and its Proof-Carrying-Code class `InterruptibleSessionSoundness`, and we
identify the one genuinely new proof obligation interruption introduces that no prior class covers:
the parked continuation is a **data-at-rest surface**, and its retention must be certified
(`ParkedResidualSoundness`). We keep three things honestly out of scope: hard real-time (impossible
on tokio/Linux), multiparty (3+ role) interruption (an open projection problem), and any claim that
"delivered" means "heard" rather than "flushed to the carrier."

---

## 1. The Gap: Interruption as an Imperative Accident

Let a turn-based dialogue between a caller $c$ and an agent $a$ proceed over a session-typed socket
(§41). During the agent's turn, $a$ streams tokens $t_1, t_2, \dots$ to $c$ over a
credit-flow-controlled `Stream<Token>`. **Barge-in** is the event: $c$ begins speaking at some
point while $t_k$ is in flight.

Every deployed stack responds to barge-in with the same three imperative steps:

$$
\textbf{VAD fires} \;\longrightarrow\; \texttt{task.cancel()} \;\longrightarrow\; \textbf{start next turn from } \varnothing.
$$

Three defects follow, and each is *structural* — a property of the host language's type system, not
a bug in any one product:

**(i) State is destroyed, not captured.** `asyncio.Task.cancel()` raises `CancelledError` inside
the coroutine; the coroutine's frame unwinds; the local context (which token was reached, what the
agent had decided to say) is garbage-collected. There is no value handed back that could resume the
utterance from token $t_k$. The stack *cannot* resume a cut-off sentence because it holds no
resumable object. This is not a missing library — Python has no delimited continuations as a
language primitive (generators are the closest, and they compose with neither linear resource
tracking nor session-typed duality, §3.4).

**(ii) The reaction time is monitored, not bounded.** The interval from VAD-fire to
cancellation-complete can be logged as a production percentile, but nothing in the type system lets
a compiler discharge "this reaction completes within $N$ ms" as a theorem before deployment. The
guarantee, if any, is empirical and posterior.

**(iii) The exchange is logged, not replayable.** A disputed call is reconstructed from application
logs, which record what the application *believed* happened. There is no cryptographically chained,
deterministic byte-for-byte reconstruction of the actual interrupt/resume transitions.

AXON's thesis, continuing §74 (delivery as a kept promise), §76 (authority as a declared property),
and §77 (a promise that crosses the trust boundary with its own witness): take what the market
ships as imperative plumbing and make it **a property of the type**. Applied here, for the first
time, to *time* and *resumability* inside a live protocol.

---

## 2. Philosophy: Interruption Is a Resumable Effect

The dialogical reading of the socket (§41, Lorenzen–Lorenz; Abramsky game semantics) casts the two
endpoints as Proponent and Opponent and a conformant run as the play of a strategy. Barge-in, under
that reading, is the Opponent seizing the move while the Proponent is mid-assertion. The imperative
stack treats this as an *abort* — the Proponent forfeits the game and a fresh one begins. But an
abort discards a strategy that was, up to token $t_k$, a *winning* one; nothing about the caller's
interjection invalidates the agent's remaining plan.

We therefore posit the **Resumption Identity**, the philosophical content of this fase:

$$
\boxed{\ \text{interruption} \;\equiv\; \text{a captured one-shot continuation} \qquad
\text{not} \qquad \text{a killed process}\ }
$$

An interrupted utterance is a *parked strategy*: a value that can be (a) **resumed** — the
Proponent returns to the exact move it was making — or (b) **superseded** — the handler splices in
new content and the dialogue continues from there. What it is *not* is destroyed. The declared
intent "this region may be interrupted and resumed" is constitutive of the region's type, in the
same way §41 made "what one end sends, the other expects" constitutive of the connection's type.

This is the `interruption_is_a_resumable_effect` doctrine, and §3 makes it a theorem.

---

## 3. The Logical Core: A Linear One-Shot Algebraic Effect

AXON already models temporal I/O as **algebraic effects and handlers** (Plotkin–Pretnar), with the
runtime realizing suspension via one-shot delimited continuations ($\mathcal{S}$/$\mathcal{R}$,
shift/reset) — see the AXON paper *Trascendiendo el Generador Clásico* (§Fase 11 lineage). §79
instantiates that machinery for interruption.

### 3.1 The surface combinator

We extend the session-step grammar of §41 (`send`/`recv`/`select`/`branch`/`rec`/`end`) with one
new step:

```axon
interrupt {
    <body>                       // a session-typed region: the agent's utterance
} on <Signal> as <sig> resumable {
    <handler>                    // runs on interruption; may end in `resume` OR reach `end`
}
```

`<Signal>` is a **new closed catalog**, not a free-form user type:

$$
\mathsf{CallInterruptCause} \;::=\; \mathsf{CallerSpeech} \;\mid\; \mathsf{Dtmf} \;\mid\; \mathsf{SilenceTimeout} \;\mid\; \mathsf{AgentFault}.
$$

The catalog is closed for the same reason every AXON catalog is (`qos`, `on_stuck`, `sign`,
backpressure policy): `axon check` needs a **finite exhaustiveness surface** and the PCC needs a
**finite proof obligation** (D79.2).

### 3.2 Operational semantics: the one-shot effect

Write the interruptible region as an effect operation $\mathsf{intr}$ delimited by its handler.
Following Plotkin–Pretnar, evaluation of the body under a handler is

$$
\mathcal{H}\big[\, \mathcal{E}[\mathsf{perform}\ \mathsf{intr}(s)] \,\big]
\;\longrightarrow\;
\texttt{handler}\big(s,\; \kappa\big), \qquad \kappa \equiv \lambda x.\,\mathcal{E}[x],
$$

where the signal value $s : \mathsf{CallInterruptCause}$ arrives from the peer, $\mathcal{E}$ is the
evaluation context of the body **at the session step where the effect fired**, and $\kappa$ is the
captured continuation. The handler may:

- **resume:** invoke $\kappa$ once — control returns to $\mathcal{E}$, the body continues from its
  exact session-step cursor;
- **supersede:** run to its own $\mathbf{end}$ without invoking $\kappa$ — the body's continuation is
  discarded (its linear capabilities released, §3.5).

**Linearity forces one-shot (D79.1).** AXON's resource discipline is affine-by-default and linear
for session channels: a channel capability cannot be duplicated. A *multi-shot* continuation would
invoke $\kappa$ more than once, duplicating the linear channel capability captured inside
$\mathcal{E}$ — ill-typed by construction. Hence $\kappa$ is invoked **at most once**; a second
`resume` is a linear-type violation (caught statically where possible, and as a runtime error
otherwise, §6). One-shot is not a simplification of convenience; it is the *only* discipline the
type theory admits, and it is exactly the discipline the compiled `shift`/`reset` machinery already
implements.

### 3.3 Duality and the connection-law preservation theorem

The §41.a connection law requires a well-formed connection's two endpoints to carry dual types:
`peer ≡ selfᐧ⊥` (implemented as `SessionType::is_dual_to`, `session.rs:245`). We must give the dual
of the new step and show the law is preserved.

Let $B$ be the session type of `<body>` and $H$ the session type of `<handler>`. The handler has, by
§3.5, **two exits**: a *resume exit* whose continuation type is $B$'s continuation (call it $B_{>k}$,
the residual of $B$ after the fired step $k$), and an *abandon exit* of type $\mathbf{end}$. Define
the interrupt combinator's type

$$
\mathsf{Intr}(B, H)\quad\text{with}\quad H = H_{\mathrm{res}} \;\&\; H_{\mathrm{abd}}, \qquad H_{\mathrm{res}}\text{ continues as } B_{>k},\ H_{\mathrm{abd}} = \mathbf{end}.
$$

The dual is taken structurally, swapping polarities in $B$ and $H$ and preserving the interrupt
scaffold:

$$
\mathsf{Intr}(B, H)^{\perp} \;=\; \mathsf{Intr}\big(B^{\perp},\, H^{\perp}\big), \qquad
(H_{\mathrm{res}} \;\&\; H_{\mathrm{abd}})^{\perp} = H_{\mathrm{res}}^{\perp} \;\oplus\; H_{\mathrm{abd}}^{\perp}.
$$

Intuitively: the peer of an endpoint that *may be interrupted while sending* is an endpoint that
*may interrupt while receiving* — the interrupt polarity flips exactly as send/recv does, and the
handler's external-choice-of-two-exits ($\&$) dualizes to the peer's internal choice ($\oplus$) of
which exit it drives.

> **Theorem 1 (Connection-law preservation).** *Let a connection have endpoints typed $S$ and
> $\bar S$ with $\bar S \equiv S^{\perp}$, and let $S$ contain an interruptible region
> $\mathsf{Intr}(B, H)$. Then the connection obtained by the interrupt/resume reduction of §3.2
> again has dual endpoints: at every reachable state — body in progress, handler on the resume
> exit, handler on the abandon exit — the residual endpoint types remain dual. Consequently the
> §41.a deadlock-freedom and conformance guarantee (Theorem 1 of the §41 paper, transported from
> Caires–Pfenning cut elimination) is preserved.*

*Proof sketch.* Duality is an involution defined by structural recursion (`SessionType::dual`,
`session.rs:157`); we extend it with the two clauses above and check involutivity
$\mathsf{Intr}(B,H)^{\perp\perp} = \mathsf{Intr}(B,H)$ (immediate, since $B^{\perp\perp}=B$,
$H^{\perp\perp}=H$, and $\&/\oplus$ are involutive). For preservation across reduction we case on
the three reachable states. **Body in progress:** the interrupt scaffold is inert; duality is that
of $B$ against $B^{\perp}$, which holds by §41. **Resume exit:** the handler yields to $B_{>k}$; the
peer's dual internal choice yields to $B_{>k}^{\perp}$; the residual pair is $(B_{>k},
B_{>k}^{\perp})$, dual by §41. **Abandon exit:** both sides reach $\mathbf{end}$, trivially dual.
Since every reachable state has dual residuals, cut elimination applies at each, and global progress
holds throughout. $\qquad\blacksquare$

The load-bearing consequence: **interruption does not open a deadlock**. A cut-off utterance that
neither resumes nor terminates is not a reachable well-typed state — the handler's two exits are
total over the abandon/resume dichotomy, and both are dual.

### 3.4 What is actually captured: the reified session cursor (D79.9)

It must be stated with precision, or the claim boomerangs against AXON itself. The runtime is
Rust/tokio, which — like Python — has **no** native primitive to capture the continuation of an
arbitrary `async fn` at an arbitrary `await` point. §79 does **not** claim one. The captured
$\kappa$ is not a general delimited continuation over the body's intermediate computation; it is a
**reified continuation at session-step granularity**: the pair

$$
\kappa \;\cong\; \big(\underbrace{S_{>k}}_{\text{residual session cursor}},\; \underbrace{w}_{\text{credit window}}\big),
$$

which is *exactly* the value the `cognitive_state` snapshot already seals (§41.g: "the residual
session-type cursor + live credit window", `socket.md:94`). This is what makes resumability
**checkable** on a substrate with no continuation primitive, and it is the crux of the fase, not a
footnote:

> **Body-alignment restriction (premise of resumability).** `resume` is sound **only** from a
> position expressible as a session step. A resume point interior to a non-session computation — a
> `let`/`Expr` evaluation (§70), a tool-call in flight — resumes from the *enclosing* session step,
> not mid-expression. 79.b's grammar makes a non-session-step-aligned resume point unrepresentable;
> 79.c rejects any that slip through. All theorems here are stated *under this premise*.

Python's generators are the closest primitive it has; they reify a state machine but compose with
neither linear channel tracking nor session-typed duality, so they cannot carry $S_{>k}$ as a typed,
dual, credit-indexed object. Resumable interruption is, for Python, not a missing library but a
missing type system.

### 3.5 The two-exit handler and abandonment (D79.11a)

A `resumable` handler is a **two-exit construct**. Its *normal* exit is `resume` (invoke $\kappa$,
return to $B_{>k}$). Its *abandon* exit is a distinct terminal $\mathbf{end}$, taken when the parked
continuation is discarded — in particular on **TTL expiry** (§79.e): an un-resumed continuation
cannot dangle forever under the affine-by-default discipline, so on expiry the runtime drives the
handler's abandon exit, releasing $\kappa$'s linear capabilities exactly once. Theorem 1 already
requires *both* exits to be dual; 79.c discharges the duality obligation for each. The naive
"handler then resume" model is incorrect precisely because it omits the abandon exit — and an
omitted abandon exit is where a linear capability would leak.

---

## 4. Timing: A DBM-Refined Timed Session Extension

The second property — a bound on reaction time — requires adding *clocks* to the session grammar. We
do this in the **difference-bound-matrix (DBM)** style of timed-automata verification (Dill 1989;
UPPAAL), the same decidable-arithmetic family as §41.c's Presburger credit analysis. This is a
continuation of AXON's existing decidable-arithmetic proof technology, not a new paradigm bolted on.

### 4.1 Clock constraints on the reaction path

Introduce a single clock $x$ reset at the instant the interrupt signal is admitted. Annotate the
control-plane's reaction region — from signal admission to the handler becoming schedulable — with a
guard $x \le \delta_{\max}$. The residual of a timed session type is a DBM: a system of
difference constraints $x_i - x_j \le c_{ij}$ over clocks, closed under the operations (reset,
elapse, conjoin-guard) whose emptiness/entailment is decidable in $O(n^3)$ (Floyd–Warshall
canonicalization). For §79 v1 the clock set is a **singleton** ($n=1$), so the machinery collapses
to interval reasoning; the DBM framing is chosen so the multi-clock generalization (nested
interrupts, deferred) is a widening of the same decidable core, not a rewrite.

### 4.2 The reaction path and the worst-case bound

Let the **reaction path** be the sequence of session transitions the runtime itself performs from
signal admission to handler-entry:

$$
\pi \;=\; \big[\text{admit } s\big] \to \big[\text{capture } \kappa\big] \to \big[\text{fire } \texttt{CancellationFlag}\big] \to \big[\text{tear down in-flight } \mathsf{Stream}\big] \to \big[\text{enter handler}\big].
$$

Let $N(\pi)$ be the number of transitions in $\pi$ — a **discrete** quantity the DBM analysis bounds
statically from the region's step structure (it does not depend on payload sizes or peer behavior).
Crucially, $N(\pi)$ **excludes** LLM inference and network latency: those are not transitions the
runtime owns, and the bound explicitly does not cover them.

> **Theorem 2 (Reaction bound).** *For an interruptible region whose reaction path $\pi$ is
> statically bounded by $N(\pi) \le N$ transitions, and **under the premise** that each transition
> completes within wall-clock $\delta$ (i.e. $\mathrm{per\text{-}step\ latency} \le \delta$), the
> control-plane's signal → cancellation-acknowledged latency is bounded by $N \cdot \delta$.*

The theorem has two layers, and honesty lives in keeping them distinct:

1. **The discrete layer** ($N(\pi) \le N$) is a genuine static theorem, discharged by the DBM check
   exactly as §41.c discharges credit constraints. It is machine-checkable and holds unconditionally.
2. **The time layer** ($\le N\cdot\delta$) rests on the **explicit premise** $\text{per-step
   latency} \le \delta$. This premise is precisely what tokio-on-Linux **cannot** discharge: a
   single transition — say the `Stream` teardown, which runs `CancelOnDrop::drop` glue — may block
   in a syscall or on a lock, and the OS scheduler may preempt the reactor. The premise is stated,
   not smuggled. Absent it, "$N\cdot\delta$" would be "$N \times$ an unproven constant," a circular
   proof.

### 4.3 Why this is soft-real-time, claimed honestly (D79.5)

Because the time layer rests on an unprovable-in-general premise, §79 does **not** claim a hard
real-time guarantee. It claims: (a) a *statically-derived discrete bound* $N$ on the reaction path,
proved; and (b) a *runtime-verified soft bound* — a **fail-closed watchdog** asserts $x \le
\delta_{\max}$ in production, and a breach does not silently degrade: it trips a fault, audited
(§79.d, §79.g). The watchdog is the operational enforcement of the premise the static proof cannot
close. Overclaiming a hard bound would be the "checker that accepts what the runtime cannot keep"
failure §77.a fixed for shields (W005); §79 does not repeat it. The shipped guarantee, stated
without inflation: *signal → acknowledged in $\le N$ transitions, and $\le N\cdot\delta$ time under a
$\delta$-bounded-step premise the watchdog enforces fail-closed.*

---

## 5. Credit Symmetry Under Resume

The third property: `resume` restores the pre-interrupt **credit window** (§41.c) exactly. Let $w$
be the available credit at the fired step $k$ (the abstract window of `credit_walk`,
`session.rs:467`).

> **Theorem 3 (Credit symmetry).** *If the handler runs on a separate declared credit budget
> $w_H$ disjoint from the body's window $w$, then invoking $\kappa$ (`resume`) returns the body to
> exactly $w$ — the credit account is symmetric under interrupt/resume: the pre-interrupt and
> post-resume windows are identical.*

*Proof.* The body's window $w$ is a function of the residual session type $S_{>k}$ and the socket's
`credit(k)` budget, both captured verbatim in $\kappa$ (§3.4). The handler, running on the disjoint
budget $w_H$, performs sends/recvs that debit/credit $w_H$, never $w$ — this is the *separation*
that makes the symmetry real rather than an accounting fiction. Invoking $\kappa$ reinstates
$(S_{>k}, w)$ unchanged; therefore the post-resume window equals the pre-interrupt window. $\;\blacksquare$

**Why separation is required (D79.11b).** The doctrine permits the handler to *splice in new
content* on the same socket — the caller's turn continues from spliced material. That spliced content
consumes credit. If the handler drew from $w$, resume could not restore $w$ "exactly"; the symmetry
would be false. Running the handler on a separate declared budget $w_H$ is therefore not a
convenience — it is the condition under which Theorem 3 is *true*. This is a strictly cleaner
invariant than plain abort (which restores nothing and proves nothing): resumption is symmetric, and
we prove it as such.

**Fallback (D79.4).** If the 79.a → 79.c mechanization shows Theorem 3's separation does not close
for *unbounded* credit, v1 narrows to `backpressure: credit(1)` sessions (named in the fase's §5
deferred scope), rather than shipping unproven accounting. The mechanization's job is to confirm the
separation discharges for the general `credit(k)` case; the paper's obligation is to state the
theorem and its premise precisely, which it does.

---

## 6. The Decidable Checking Algorithm and the PCC Class

### 6.1 `Check` for an interruptible session

Given a socket whose protocol contains $\mathsf{Intr}(B,H)$ regions, the compiler runs:

$$
\textsc{Check}_{\mathsf{intr}}(S) = \begin{cases}
\mathbf{1.} & \text{dual-check } \bar S \stackrel{?}{\equiv} S^{\perp} \text{ with the extended } (\cdot)^{\perp} \text{ (§3.3), both handler exits;}\\
\mathbf{2.} & \text{exhaustiveness: } \langle\mathsf{Signal}\rangle \in \mathsf{CallInterruptCause} \text{ (closed catalog);}\\
\mathbf{3.} & \text{credit symmetry: handler budget } w_H \text{ disjoint from body } w \text{ (§5);}\\
\mathbf{4.} & \text{DBM discharge: reaction-path clock guard } x \le \delta_{\max} \text{ satisfiable given } N(\pi) \text{ (§4);}\\
\mathbf{5.} & \text{body-alignment: every } \texttt{resume} \text{ point is session-step-aligned (§3.4).}
\end{cases}
$$

Steps 1–2 and 5 are linear in $|S|$ (regular-coinductive equality for $\mu$-types, `session.rs:237`);
step 3 is the disjointness check on two credit walks; step 4 is DBM canonicalization, $O(n^3)$ with
$n=1$. The whole pipeline is **decidable and terminating** — admissible as a compiler pass. A session
violating any clause is rejected at `axon check`.

### 6.2 The PCC class `InterruptibleSessionSoundness`

AXON's Proof-Carrying Code engine (§51, `axon-rs/src/pcc/`) emits, per certified property, a portable
`ProofTerm { property, artifact_digest, witness, axon_version }` that an **independent** checker
re-derives (`check_proof` → `Verified | Refuted | DigestMismatch | UnknownProperty`), rejecting a
forged witness by recomputation (D51.2). §79 adds one property class, mirroring the existing dozen
(`ComplianceCoverage`, `EffectRowSoundness`, …, `ChannelEgressSoundness`):

$$
\mathsf{PropertyClass}\ {+}{=}\ \mathsf{InterruptibleSessionSoundness}, \qquad \text{slug} = \texttt{"interruptible\_session\_soundness"}.
$$

Its witness records, per interruptible region, the machine-checkable facts of §6.1: the handler is
reachable and dual **under both exits**; the signal is catalog-closed; the credit account is
symmetric (handler budget disjoint); the reaction-path clock bound is satisfiable given $N(\pi)$; the
resume point is session-step-aligned. The **checker re-derives each** from the artifact IR — it does
not trust that the type-checker ran (PCC independence, §51). A forged witness (e.g. claiming
`dual_under_both_exits: true` for a handler missing its abandon exit) is Refuted by recomputation,
exactly as the existing classes reject their forgeries.

---

## 7. The Parked Continuation Is a Data-at-Rest Surface (`ParkedResidualSoundness`)

This is the one obligation interruption introduces that **no prior class covers**, and it is the
reason the unified certificate (§79.f) must *compose a new proof*, not merely conjoin existing ones.

When a body is interrupted and parked (§79.e), its reified continuation $\kappa = (S_{>k}, w)$ is
**persisted at rest** into the `cognitive_state` store for the TTL window, so it survives a reconnect.
But $S_{>k}$ may carry, in its residual payload types, **PII-bearing values** from the body. The §77
shield reasons about *channel egress* — data crossing the trust boundary on the wire — not about a
*snapshot at rest* of a parked continuation. So a certificate that merely ANDs (session soundness ∧
shield soundness ∧ budget soundness) would certify "this call is sound" while silently opening a new
retention surface the shield never inspected.

We therefore add a fourth, genuinely new obligation:

> **`ParkedResidualSoundness`.** For every interruptible region whose continuation may be parked: (a)
> the parked snapshot's payload types lie within the flow's declared PII/retention envelope; (b) the
> snapshot TTL does not exceed the socket's `legal_basis` retention ceiling; (c) the AAD binding of
> the `cognitive_state` seal (§41.g: keyed by `(tenant_id, session_id, socket_name,
> subject_user_id)`) covers **every** field carried across the park (no field escapes the
> authenticated envelope).

This obligation is a first-class member of the `CallSoundnessCertificate` (§79.f), not an emergent
conjunction (D79.8). The composition is *necessary* (three orthogonal proofs) but not *sufficient*;
the interaction term — residual-at-rest — is the part only §79 can see.

---

## 8. Realization and Honest Scope

**What is composition of established results:** algebraic effects & handlers (Plotkin–Pretnar); the
one-shot delimited-continuation compilation AXON already ships (§Fase 11 lineage); timed automata /
DBMs (Dill; UPPAAL); arithmetic-refined session types (Rast) already realized as §41.c's credit
index; the session-type core and connection law (Caires–Pfenning) already realized in
`session.rs`; the `cognitive_state` sealed snapshot (§41.g); the `ReplayToken` chain (§74.e); and
the PCC kernel (§51).

**What is the contribution:** their *fusion into one language feature* that (a) makes interruption a
**resumable one-shot effect** whose duality preserves the connection law (Theorem 1); (b) makes the
control-plane reaction path carry a **statically-derived discrete bound**, lifted to a
watchdog-enforced soft-time bound under a stated premise (Theorem 2); (c) makes `resume`
**credit-symmetric** by construction (Theorem 3); (d) certifies all of it — plus the new
data-at-rest obligation — as **one deploy-time proof** (§6, §7); and (e) makes every transition a
receipt in the deterministic replay chain. To our knowledge no deployed voice stack, and no
general-purpose language used to build one, ships any of (a)–(d); the field cancels and restarts.

**Honestly out of scope** (named, not omitted):

- **Hard real-time (RTOS-level).** Impossible on tokio/Linux; a *permanent ceiling*, not a "not
  yet" (D79.5). No theorem here implies one.
- **Multiparty (3+ role) interruption.** Projecting a resumable, timed, effectful combinator across
  3+ roles via Honda–Yoshida–Carbone is an open problem this fase does not build on (D79.7). A
  proof-carrying compiler that shipped an unproven multiparty case would undermine the very property
  it sells. v1 is **binary** (2-role), single-level, single-signal.
- **"Delivered" = flushed-to-carrier, not heard (D79.10).** The "resume from the exact word" claim
  is honored by snapshotting the `Stream` **emit cursor** (frames flushed to the socket) alongside
  the session cursor. The runtime cannot observe what the caller's device rendered — TTS audio is
  commonly pre-buffered client-side — so the guarantee is defined against the flush boundary the
  runtime *can* observe.

## 9. Falsifiable Claims (the engineering must honor the math)

1. Every accepted interruptible session is duality-checked under **both** handler exits; a handler
   missing its abandon exit is rejected.
2. `resume` is invocable **at most once**; a second `resume` is a linear-type error, statically
   where derivable and at runtime otherwise.
3. The reaction-path **discrete** bound $N(\pi)$ is discharged statically; a region whose clock
   guard is unsatisfiable given $N(\pi)$ fails `axon check`.
4. The production watchdog trips **fail-closed** on a bound breach (never silent degradation), and
   the breach is audited.
5. `resume` restores the pre-interrupt credit window exactly; the handler cannot draw from the
   body's window.
6. A parked continuation carrying a field outside the flow's PII/retention envelope, or with a TTL
   exceeding the `legal_basis` ceiling, fails `ParkedResidualSoundness`.
7. Every interrupt/resume/abandon transition replays deterministically, byte-for-byte, from the
   audit log alone.

## 10. Pillar Trace

- **MATHEMATICS.** Interruption is an involutive extension of session duality (Theorem 1); the
  reaction bound is a DBM-decidable arithmetic obligation (Theorem 2); credit symmetry is an
  equality of abstract windows (Theorem 3). Each is a statement in a decidable theory, not a runtime
  hope.
- **LOGIC.** The one-shot discipline is *forced* by linearity — a multi-shot continuation duplicates
  a linear capability and is ill-typed. The handler's two exits are total over the resume/abandon
  dichotomy; no interrupted-and-dangling state is well-typed.
- **PHILOSOPHY.** An interrupted utterance is a parked strategy, not a forfeited game; declared
  resumability is constitutive of the region's type. Interruption becomes *Zuhandenheit* — lawful
  conduct is the type, not a runtime callback asserted post hoc.
- **COMPUTING.** The captured continuation is the reified session cursor + credit window — the value
  `cognitive_state` already seals — so resumability is realizable on a substrate (Rust/tokio) with no
  native continuation primitive, via the `CancellationFlag` (§33.f) and one-shot `shift`/`reset` the
  runtime already compiles.

## 11. Conclusion

For the whole short history of voice AI, interruption has been an imperative accident: a callback
that cancels a task and throws away the agent's mind. AXON does not patch the callback; it re-founds
interruption as a **resumable, time-bounded, credit-symmetric, one-shot effect** whose soundness —
including the data-at-rest surface it opens — is a single deploy-time proof, and whose every
transition is a receipt in a deterministic replay chain. The captured continuation is narrow by
necessity — a reified session cursor, not a general delimited continuation — and that narrowness is
exactly what makes it *checkable* on a runtime whose host language, like Python's, has no
continuation primitive of its own. What Python lacks here is not a library; it is a type system. A
call built on §79 can prove, before it ever connects, that it will resume from the exact word it
flushed to the caller, react within a bounded number of transitions, keep its credit account
symmetric, retain no more than it declared, and reconstruct the entire exchange byte-for-byte
afterward. That is the guarantee the market cannot currently express — and the one this fase makes a
property of the type.

---

*AXON Research Paper — §Fase 79.a. Reviewed specification; zero grammar changes. Fixes the semantics
implemented by §79.b (grammar/AST/IR), §79.c (type-checker + `InterruptibleSessionSoundness`), §79.d
(runtime + fail-closed watchdog), §79.e (parked-continuation persistence), §79.f
(`CallSoundnessCertificate` + `ParkedResidualSoundness`), and §79.g (audit + replay). Theorems 1–3
and the honest-scope ceilings of §8 are the contract those sub-fases must honor.*
