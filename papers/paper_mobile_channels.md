# Mobile Typed Channels for λ-L-E
## π-Calculus Mobility under Epistemic Linear Logic

> [!ABSTRACT]
> We extend the **Cálculo Lambda Lineal Epistémico (λ-L-E)** of [paper_lambda_lineal_epistemico.md](paper_lambda_lineal_epistemico.md) with **mobile typed channels**: a new category of terms `Channel<τ>` that inhabit the linear-affine fragment of the type system and whose handles are first-class values, passable across other channels (π-calculus mobility, Milner 1991), persistable via `axonstore`, and dynamically exposable via capability-gated `publish` / `discover`. The contribution is threefold. (i) A *static* typing discipline under which channel mobility preserves affinity, epistemic monotonicity (ΛD), and Separation Logic disjointness (§3). (ii) A *dynamic* semantics in which β-reduction is extended with scope extrusion (Milner–Parrow–Walker 1992) and in which a receiver's post-extrusion view of a channel carries an envelope whose certainty is bounded above by the publisher's original certainty minus a public-exposure penalty δ_pub (§4). (iii) A **Theorem 6.1 (Mobile Stochastic Degenerative Soundness)** extending Theorem 5.1 of the parent paper: reduction under mobility preserves type, affinity and ΛD monotonicity, *and* is deadlock-free whenever the underlying session-type projection is Honda–Yoshida dual. The calculus subsumes Axon's Fase 4 binary sessions as the immobile fragment and replaces the stringly-typed EventBus of Fase 11.d with a compile-time verifiable event graph.

---

## 1. Motivation

### 1.1 The stringly-typed gap

Axon v1.4.0 exposes reactive pub/sub via the `daemon` / `listen "topic"` pair (Fase 11.d; `axon/compiler/parser.py:2815`). Channel identity is a UTF-8 string consumed at runtime by an `EventBus`. This leaves **five guarantees on the table**:

1. *Producer–consumer schema agreement* — a publisher emitting `Order` and a listener expecting `Invoice` on the same topic typecheck independently.
2. *Static topology* — the compiler cannot draw the graph `daemon_i ↔ topic_k ↔ daemon_j` and therefore cannot verify Honda–Yoshida duality (Fase 4 §4.2) across reactive edges.
3. *Linearity under mobility* — nothing prevents two holders of `"orders.created"` from both consuming a message that should be one-shot.
4. *Capability security* — any caller can `listen` on any topic, because topics are public by construction.
5. *LSP tooling* — topics are opaque to the Language Server (`axon-lsp v0.1.0`): no go-to-definition, no find-references, no rename refactor.

Items 1–5 are precisely the guarantees that λ-L-E asserts for *resources* in Fase 1. The asymmetry is historical, not principled, and this paper removes it.

### 1.2 Thesis

There exist exactly two coherent design points for channels in Axon:

- **Runtime-routed strings** — topics are dynamic, routing is opaque, type system is bypassed. This is Kafka/NATS re-packaged in Axon syntax; it contributes nothing to the formal foundation.
- **Compile-time typed + mobile** — channels are typed terms whose *identities* can still be passed, stored, and published dynamically, exactly because π-calculus mobility (Milner 1991) already separates **static typing** from **dynamic scope extrusion**. This is the design we take.

A third option — a stable hybrid — is incoherent: the untyped escape-hatch drains adoption away from the typed path, leaving the language permanently with the weaker of the two guarantees. Decision D4 of [docs/fase/fase_13_mobile_typed_channels.md](fase_13_mobile_typed_channels.md) therefore accepts a *transitional* hybrid only (v1.4.x dual-mode, v2.0 typed-only).

---

## 2. Syntax (abstract, extending §2 of λ-L-E)

```
Type     τ    ::=  …                            (all λ-L-E types)
                |  Channel<τ, q, ℓ, π>          (message type τ, QoS q,
                                                 lifetime ℓ, persistence π)
                |  Capability<c>                (σ-shield gated exposure of c)

QoS      q    ::=  at_most_once                (AMO)
                |  at_least_once               (ALO, default)
                |  exactly_once                (EO — requires replay-token, Fase 11.c)
                |  broadcast                   (fan-out)
                |  queue                       (single-consumer)

Lifetime ℓ    ::=  linear                      (must consume)
                |  affine                      (may drop — default for channels, D1)
                |  persistent                  (`!Channel<τ>`, duplicable)

Persist  π    ::=  ephemeral                   (default)
                |  persistent_axonstore        (materialized in axonstore with τ-decay)

Term     e    ::=  …                            (all λ-L-E terms)
                |  chan c : τ q ℓ π . e        (restriction — ν-binder)
                |  e ⟨ v ⟩ . e'                (output on channel e)
                |  e ( x : τ ) . e'            (input — binds x in e')
                |  publish e within σ          (extrusion gated by shield σ)
                |  discover C as x . e         (dual of publish — dynamic import)
                |  e ∥ e'                      (parallel composition)
                |  ! e                         (replication — banged in Girard's sense)
```

The five additions are exactly Milner's polyadic π-calculus (Milner 1991 §4) with two amendments:

- Each channel carries its ΛD envelope as a type index, so `chan c : Channel<Order, ALO, affine, eph>` is short for `(νc)(c : Channel<Order, ALO, affine, eph> [E_c])`.
- `publish` / `discover` are not primitive in Milner; they materialize the scope-extrusion *rule* (§4.3 below) as a **user-observable** operation, gated by a σ-shield from Fase 6 (D8).

### 2.1 Notational conventions

We write `c : Channel<τ>` and elide q, ℓ, π when unambiguous. We write `c ↦ τ` when the direction is output and `c ↤ τ` for input; duality `c ↦ τ ≡ c̄ ↤ τ` matches Honda–Yoshida. When a channel is stored in `axonstore`, we decorate it `⌈c⌉` for the persistent cell containing the handle.

---

## 3. Judgments (extending §3 of λ-L-E)

### 3.1 Channel linearity (D1)

A channel handle is **affine**: it can be used at most once per linear consumer (`send`, `receive`, `publish`, `store`), may be implicitly dropped, and cannot be aliased via `let`.

    Γ, c: Channel<τ, q, affine, π>  ⊢  e : σ           (no occurrence of c in Γ)
    ─────────────────────────────────────────────       (affine weakening)
    Γ                                  ⊢  e : σ

    Γ ⊢ c : Channel<τ, q, affine, π>     Γ ⊢ v : τ
    ───────────────────────────────────────────────     (Chan-Output)
    Γ ⊢ c⟨v⟩.e  :  ⋄                     [E ⊔ E_c ⊔ E_v]

    Γ, x: τ ⊢ e : σ
    ──────────────────────────────────────────────      (Chan-Input)
    Γ, c: Channel<τ, q, affine, π> ⊢ c(x:τ).e  :  σ

The ⊔ operation on envelopes computes the pointwise minimum of the `c` component and the latest `τ_t` with `δ = mutated` (compare λ-L-E §3.3).

Justification (paper [fase_13_mobile_typed_channels.md §3 D1](fase_13_mobile_typed_channels.md)): linear pleno (must-consume) is too strict for long-lived channels — it would force every `daemon` to explicitly close every listen at program exit, which is pragmatically unacceptable. Affinity preserves no-aliasing (the important property) while permitting drop.

### 3.2 Mobility rule (Scope extrusion, Milner 1991 §4.2)

Mobility is realized by allowing `Channel<…>` values to appear *anywhere* a τ-typed value appears — including inside a send on another channel:

    Γ ⊢ d : Channel<Channel<τ, q', ℓ', π'>, q, ℓ, π>
    Γ ⊢ c : Channel<τ, q', ℓ', π'>
    ─────────────────────────────────────────────────   (Chan-Mobility)
    Γ ⊢ d⟨c⟩ . e  :  ⋄                [E ⊔ E_c ⊔ E_d]

This rule is **not admissible from §3.1** as stated — because §3.1's `Γ ⊢ v : τ` premise has to be instantiated with `τ = Channel<…>`, and we must also verify that sending `c` consumes it under affinity. The soundness argument (§6.1) makes this explicit.

### 3.3 Session-type projection (second-order, Honda–Yoshida 1999)

Fase 4 (plan §5 / paper 4) introduced binary session types with duality `(send T) ↔ (receive T)`. We lift duality to channel-carrying sessions:

    Γ ⊢ S = { send Channel<τ>; … }      Γ ⊢ S̄ = { receive Channel<τ>; … }
    ──────────────────────────────────────────────────                  (Sess-Dual-²)
    Γ ⊢ S ⋈ S̄

Formally: duality is defined coinductively over session trees; the second-order rule says that two steps on channels-carrying-channels are dual iff their carried-type steps are themselves dual (a standard Honda–Yoshida induction). This is the connector that makes Fase 4 and Fase 13 a single coherent calculus.

### 3.4 Capability judgment (D8)

`publish` and `discover` are **σ-shield-mediated**:

    Γ ⊢ c : Channel<τ, q, ℓ, π>       Γ ⊢ σ : Shield<κ>     κ ⊇ κ(τ)
    ─────────────────────────────────────────────────────────────────   (Publish)
    Γ ⊢ publish c within σ  :  Capability<c>                            [E_c ⊔ E_σ · (1 − δ_pub)]

    Γ ⊢ C : Capability<c>
    ────────────────────────────────────────────                        (Discover)
    Γ, x: Channel<τ, q, ℓ, π> ⊢ discover C as x . e  :  σ_out

Three invariants fall out of this shape:

- **Compile-time compliance** — a channel carrying PHI cannot be published through a shield that does not cover HIPAA (λ-L-E §6.4, Fase 6.1).
- **Certainty penalty** — publication widens the trusting audience from "the process" to "the subnet / the world", so the published handle's certainty is attenuated by `(1 − δ_pub)` with δ_pub ∈ (0, 1] configured at the shield. Default: δ_pub = 0.05 per extrusion hop (a conservative bound that rules out certainty laundering across publishes).
- **No free publish** — without a shield, `publish` is a compile-time error (paper matches implementation plan 13.b).

### 3.5 Disjointness under channels

Unlike resources, two channels with *different* names are always disjoint. Separation Logic's `*` therefore lifts cheaply:

    Γ ⊢ c₁ ≠ c₂                Γ ⊢ c₁ : Channel<τ₁, …>         Γ ⊢ c₂ : Channel<τ₂, …>
    ──────────────────────────────────────────────────────────────────────────────────────   (Chan-Disjoint)
    Γ ⊢ c₁ * c₂  :  Channel<τ₁, …> * Channel<τ₂, …>

This matters for `manifest`: a manifest that lists two channel handles gets the same compile-time disjointness check as a manifest with two resources (Fase 1.4 IR reuses the same `*` connector).

---

## 4. Operational semantics

### 4.1 Communication rule (π-base)

    c⟨v⟩.P   ∥   c(x:τ).Q       ⟶c       P   ∥   Q[v/x]                          (Comm)
    [E_P]   [E_Q]                         [E_P']   [E_Q'  where E_v.c ≤ E_c.c]

Note the envelope clause: the received value's certainty is bounded by the channel's certainty — a low-trust channel cannot deliver a high-trust observation, in direct analogy with λ-L-E §3.3.

### 4.2 Restriction + structural congruence

    (νc) (P ∥ Q)    ≡    ((νc) P) ∥ Q         if c ∉ fv(Q)                       (Scope-Ext)

This is *the* scope-extrusion rule of Milner; it is what allows `publish` to be implementable without moving code across process boundaries.

### 4.3 Publish as materialized extrusion (D8)

    (νc) P   ∥   publish c within σ     ⟶p      (νc) (P ∥ C_c_σ)                 (Publish-Ext)

where `C_c_σ` is a capability cell: a freshly generated witness whose possession lets another process `discover C_c_σ as d . Q` bind `d ≔ c`.  The reduction is the operational analogue of §3.2's Chan-Mobility, but with the scope boundary **externalized** through σ's handler (ESK, Fase 6).

### 4.4 τ-decay on mobile channels

The channel's envelope decays as in λ-L-E §4.2. When `c_at(now) = 0`, any subsequent `c⟨v⟩` or `c(x)` raises `LeaseExpiredError` (Fase 3.2, CT-2). The interesting case is a *published* handle: because its envelope was already attenuated by δ_pub at §3.4, it decays to void sooner than the original, matching the intuition that published credentials should expire before local ones.

---

## 5. Interaction with existing λ-L-E machinery

### 5.1 Channels as resources

A `Channel<τ>` with `lifetime = linear` and `persistence = persistent_axonstore` is, up to isomorphism, a `Resource<κ_chan, linear>` where κ_chan is the compliance class of the transported messages. All Fase 1 rules apply: it participates in manifests, can be observed (via `ensemble` aggregation over multiple subscribers), and can hold a lease (Fase 3.2).

### 5.2 Network partition (D4) on mobile channels

A partition during `c⟨v⟩` raises `NetworkPartitionError` (CT-3). Per λ-L-E §4.3, this is ⊥, not `doubt`. Under mobility, the subtlety is that the *sender* caller gets CT-3, but a *published* downstream holder of the same handle gets `LeaseExpiredError` (CT-2) because the handle's envelope decays mechanically to void. This preserves Blame Calculus separation.

### 5.3 Ensemble over channels (Cφ)

The `observe` ensemble aggregator (Fase 3.3) generalizes to channels: an ensemble of N listeners subscribing to the same `Channel<τ>` produces a `HealthReport [⟨c_agg, τ_t, ρ_ens, δ⟩]` where c_agg is the Byzantine-quorum minimum-weighted combiner. This yields common-knowledge over channel traffic, not just over static state — a prerequisite for the reactive audit semantics of Fase 11.c replay tokens.

### 5.4 Replay on channels

Every `Chan-Input` reduction emits a `ReplayToken` (Fase 11.c). A replayed program re-executes the input rule against the recorded token, preserving deterministic causality across mobile channels. The published-handle penalty δ_pub is also recorded, so replay with a different δ_pub is a detected divergence (not silently accepted).

---

## 6. Soundness

### 6.1 Theorem — Mobile Stochastic Degenerative Soundness

Let `⊢ e : τ [E]` in the calculus of §2. If `e ⟶* e'` under the reductions of §4 (β from λ-L-E, plus Comm, Scope-Ext, Publish-Ext), then:

1. **Type preservation:** `⊢ e' : τ [E']`.
2. **Affine preservation:** the number of distinct holders of any `Channel<τ, q, affine, π>` handle is ≤ 1 at every intermediate configuration, *up to* `Capability<c>` materialized by Publish-Ext.
3. **Envelope monotonicity:** `E'.c ≤ E.c`, with the additional inequality that for every `publish … within σ` traversal, certainty strictly decreases by at least `δ_pub(σ)`.
4. **Deadlock freedom:** if every binary session projected from the program graph is Honda–Yoshida dual (§3.3), then `e'` is either a value or admits a Comm reduction — i.e. the network never wedges.

**Proof (sketch).**

(1) by structural induction on reductions, reusing the λ-L-E case set; the new cases are Comm, Scope-Ext, Publish-Ext. Comm is standard (Milner 1991, Theorem 2). Scope-Ext is a structural congruence — preserves types by α-equivalence. Publish-Ext requires the Capability<c> type to be stable under reduction, which is immediate from (Publish).

(2) by induction on the typing derivation. Only (Chan-Output), (Chan-Mobility) and (Publish) consume a channel handle; each is gated on `Γ ⊢ c : Channel<…>` on the LHS and each strictly removes c from Γ on the RHS (weakening is still available for unused handles). Publish-Ext creates a `Capability<c>` but does **not** re-introduce c into the sender's Γ — the sender has traded its handle for the capability witness.

(3) envelope monotonicity: λ-L-E §5.1 clauses (i)–(iii) still apply; the new clause is Publish-Ext which multiplies c by `(1 − δ_pub)` with δ_pub > 0, therefore strictly decreases c on every publish.

(4) deadlock freedom. Assume every binary projection is dual (§3.3). A stuck state would require two inputs facing each other on the same channel with no matching output, or two outputs racing for one input. Honda duality over the projected session tree contradicts both: an `input Channel<τ>` must pair with an `output Channel<τ>` by (Sess-Dual-²). Since the second-order rule composes coinductively, the pairing extends through mobility. A network-level partition (D4) is a CT-3 exception, not a deadlock — it propagates and blames, it does not silently wedge. □

### 6.2 Corollary — No certainty laundering across public exposure

No program can expose a `Channel<τ, q, ℓ, π>` via `publish` without a measurable certainty penalty. Consequently, a resource's trust level is a lower bound on the worst-case view held by any published downstream — a strictly stronger property than the parent paper's §5.2 ("no silent upgrade"), now lifted from computation to *communication*.

### 6.3 Corollary — Static topology

The set of all channel names and their carried types is statically extractable from the program. Therefore:

- axon-lsp can render the full reactive graph.
- `axon check` can verify Honda duality on all reactive edges (existing Fase 4 machinery, now reused).
- Dead-channel detection (declared and never consumed) is a compile-time warning.

This is the capability that string-topics structurally deny.

---

## 7. Correspondence with existing calculi

### 7.1 Mobile channels vs Milner's π-calculus (1991)

Milner's polyadic π has `c⟨v̄⟩.P`, `c(x̄).P`, `(νc)P`, `!P`, `P ∥ Q`. λ-L-E's mobile channels add:

- Envelope-typed channels `Channel<τ, q, ℓ, π>` — the carrier is not untyped but annotated with ΛD.
- Linearity/affinity discipline — Milner's π is unconstrained; Axon's is not.
- Capability extrusion via shields — Milner had no security layer.

### 7.2 vs Honda–Yoshida session types (1999)

HY introduces duality + progress for binary sessions. Fase 4 implemented HY for static topologies. Fase 13 extends HY's session-type judgment to **channel-carrying** sessions via the Sess-Dual-² rule (§3.3). This is the second-order extension sketched in HY 1999 §6, mechanized in Axon for the first time in a production-grade compiler.

### 7.3 vs Mezzo / Rust's ownership

Mezzo and Rust both track affinity of references. λ-L-E differs on two axes: (i) the epistemic envelope ΛD has no equivalent — Rust cannot express "certainty ≤ 0.95" at the type level, and (ii) Rust has no scope extrusion; channels in Rust (mpsc) are affine but not mobile-as-values.

### 7.4 vs Erlang / Elixir / Akka

Actor mailboxes in the Erlang family are mobile (PIDs are first-class) but **untyped**. λ-L-E's mobile channels are typed + affine + envelope-aware. The performance story is expected to be comparable (mailbox ≈ channel queue); the formal story is strictly stronger.

### 7.5 vs stringly-typed EventBus (Axon v1.4.0 Fase 11.d)

This paper subsumes the current `daemon`/`listen "topic"` mechanism. Strings remain accessible in v1.4.x under dual-mode (D4) and emit a deprecation warning. In v2.0 strings are removed; the only channels are the typed-mobile ones of §2.

---

## 8. Relation to the Axon implementation

λ-L-E Mobile Channels is **not** speculative: every rule in §3–§4 compiles to a mechanical check in the Axon implementation (phase-gated per Fase 13.a–f of the plan).

| λ-L-E-M construct | Axon code (planned) |
|---|---|
| `Channel<τ, q, ℓ, π>` | `axon/compiler/ast_nodes.py::ChannelDefinition` (Fase 13.a) |
| (Chan-Output) / (Chan-Input) | `axon/compiler/type_checker.py::_check_channel_ops` (13.b) |
| (Chan-Mobility) | `axon/compiler/type_checker.py::_check_channel_mobility` (13.b) |
| (Sess-Dual-²) | extension of `_check_session_duality` (Fase 4) — (13.b) |
| (Publish) / (Discover) | `axon/compiler/type_checker.py::_check_capability` + shield coverage gate (13.b) |
| (Comm) reduction | `axon/runtime/channels/typed_bus.py` (13.d) |
| (Scope-Ext) | `axon/runtime/channels/scope.py` (13.d) — α-renaming in handle registry |
| (Publish-Ext) | ESK + `axon/runtime/channels/capability.py` (13.d) |
| δ_pub envelope penalty | `axon/runtime/handlers/base.py::LambdaEnvelope.publish_hop` (13.d) |
| Rust parity | `axon-frontend/src/**` + `axon-rs/src/runtime/channels/**` (13.f) |
| Paper-mechanized tests | `tests/test_paper_mobile_channels.py` — one test per theorem clause (13.h) |

---

## 9. Worked example

```axon
type Order compliance [PCI_DSS] {
    id: String
    amount: Money
    customer_ref: String
}

# Declare the public broker channel — carries orders
channel OrdersCreated {
    message: Order
    qos: at_least_once
    lifetime: affine
    persistence: ephemeral
}

# Declare the meta-channel — carries OrdersCreated handles (mobility)
channel BrokerHandoff {
    message: Channel<Order, at_least_once, affine, ephemeral>
    qos: exactly_once
    lifetime: affine
    persistence: persistent_axonstore
}

# Shield gate: PCI_DSS-covering σ required for publish
shield PublicBroker {
    scope: [OrdersCreated]
    compliance: [PCI_DSS]
    severity: high
    delta_pub: 0.05              # certainty penalty on every extrusion
}

# Consumer — receives a channel via BrokerHandoff, listens on it
daemon OrderConsumer {
    listen BrokerHandoff as ch {
        listen ch as order {
            process(order)        # `order: Order` is statically typed
        }
    }
}

# Producer — creates OrdersCreated, emits it via BrokerHandoff, publishes
flow hand_off() -> Capability<OrdersCreated> {
    emit BrokerHandoff(OrdersCreated)                   # (Chan-Mobility)
    publish OrdersCreated within PublicBroker           # (Publish-Ext)
}
```

**What the compiler proves at `axon check`:**

1. `OrdersCreated` is affine and appears in at most one simultaneous consumer (Chan-Output linearity).
2. `BrokerHandoff` is correctly parameterized over `Channel<Order, …>` — second-order session type (Sess-Dual-²).
3. `PublicBroker.compliance ⊇ κ(Order) = {PCI_DSS}` (Compile-time Compliance, §3.4).
4. The published handle's envelope decays by at least 0.05 on extrusion (Theorem 6.1 clause 3).
5. Deadlock freedom across `OrderConsumer ↔ hand_off` (Theorem 6.1 clause 4) — verified by Honda-Yoshida duality over the reactive edge.

Every guarantee is **mechanical** — checked by extensions to `type_checker.py` and `ir_generator.py`, without any runtime, audit, or human review.

---

## 10. Related work

- Milner, R. (1991). *The Polyadic π-Calculus: a Tutorial*. LFCS Edinburgh.
- Milner, R., Parrow, J., Walker, D. (1992). *A Calculus of Mobile Processes, Parts I and II*. Information and Computation 100(1).
- Honda, K., Vasconcelos, V.T., Kubo, M. (1998). *Language primitives and type discipline for structured communication-based programming*. ESOP.
- Honda, K., Yoshida, N., Carbone, M. (2008). *Multiparty Asynchronous Session Types*. POPL.
- Pierce, B.C. (2002). *Types and Programming Languages* — chapters on π-calculus typing.
- Gay, S., Hole, M. (2005). *Subtyping for Session Types in the π-Calculus*. Acta Informatica.
- Turner, D.N. (1996). *The Polymorphic Pi-Calculus: Theory and Implementation*. PhD, LFCS.
- Caires, L., Pfenning, F. (2010). *Session Types as Intuitionistic Linear Propositions*. CONCUR.
- Tov, J.A., Pucella, R. (2011). *Practical Affine Types*. POPL.
- Axon reference: [paper_lambda_lineal_epistemico.md](paper_lambda_lineal_epistemico.md) — parent calculus.
- Axon reference: `docs/paper_session_types_axon.md` *(Fase 4 formalization; if not yet extracted as its own paper, see `plan_io_cognitivo.md` §Fase 4)*.

---

## 11. Status

| Section | Mechanization | Tests |
|---|---|---|
| §3.1 Channel affinity | `type_checker.py::_check_channel_affinity` (Fase 13.b) | `tests/test_type_checker.py::TestChannelAffinity` (target ≥ 15) |
| §3.2 Mobility | `type_checker.py::_check_channel_mobility` (13.b) | `tests/test_type_checker.py::TestChannelMobility` (target ≥ 10) |
| §3.3 Second-order sessions | extension of `_check_session_duality` (13.b) | `tests/test_type_checker.py::TestSecondOrderSessions` (target ≥ 8) |
| §3.4 Capability | `type_checker.py::_check_capability` (13.b) | `tests/test_type_checker.py::TestChannelCapability` (target ≥ 10) |
| §4 Reduction | `axon/runtime/channels/` (13.d) | `tests/test_typed_channels.py` (target ≥ 50) |
| §6.1 Theorem (clause 1–3) | enforced by combination of 13.b + 13.d | `tests/test_paper_mobile_channels.py::TestMobileSoundness` |
| §6.1 Theorem (clause 4) | reuse Fase 4 deadlock detection, extended coinductively | `tests/test_paper_mobile_channels.py::TestDeadlockFreedom` |

All rows `[PLANNED]` as of 2026-04-24 — the paper precedes the code by Fase-13 convention.

---

## Appendix A — Design decisions (sign-off 2026-04-24)

The nine fundational decisions for Fase 13 are fixed in [docs/fase/fase_13_mobile_typed_channels.md §3](fase_13_mobile_typed_channels.md). They are (in shorthand): D1 affine handles, D2 first-class mobility, D3 schema-typed messages, D4 dual-mode transition to v2.0 typed-only, D5 Python-then-Rust ordering, D6 integration with existing primitives (manifest/session/axonstore/daemon), D7 per-channel QoS, D8 capability-gated publish, D9 paper precedes parser. This document is the realization of D9.

## Appendix B — Open questions (deferred to Fase 14+)

- **Multiparty session types** (Honda–Yoshida–Carbone 2008) — Fase 13 covers binary only; n-party is future work.
- **Polymorphism over QoS** — can a flow be generic over `q ∈ QoS`? Current answer: no, QoS is part of the channel type. If demand arises, explore bounded QoS polymorphism à la subtyping.
- **Distributed extrusion across cluster boundaries** — current Publish-Ext is process-local with capability escape; cross-cluster requires Raft/CRDT coordination, explicitly out of scope (Fase 13 §10 R5 in plan).
- **Substructural interaction with Immune (Fase 5)** — can `observe` be defined over channel traffic beyond the §5.3 ensemble sketch? Open.
