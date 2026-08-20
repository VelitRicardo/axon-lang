# Cálculo Lambda Lineal Epistémico (λ-L-E)
## Constructive Infrastructure Provisioning via Epistemic Linear Logic

> [!ABSTRACT]
> We present **λ-L-E**, a typed lambda calculus that merges Girard's Linear Logic with an epistemic lattice over computational certainty, yielding the first programming-language framework in which **infrastructure provisioning is a constructive proof of resource existence**. Unlike conventional Infrastructure-as-Code tools (Terraform, Pulumi) which treat state declaratively and detect conflicts at apply time, λ-L-E rejects at *compile time* any program whose resource usage violates affinity, whose aliasing violates Separation Logic's `*` disjointness, or whose belief-evidence gap cannot be closed under shield approval. Every program of type `Manifest ⊸ Evidence` is mechanically a Curry-Howard proof that the requested infrastructure is simultaneously (i) linearly consumable, (ii) regionally disjoint, and (iii) epistemically well-founded. The calculus's soundness is **Theorem 5.1 (Stochastic Degenerative Soundness)**: reduction preserves type *and* certainty with explicit decay.

---

## 1. Introduction

### 1.1 The asymmetry of the state of the art

Imperative cloud orchestrators (Kubernetes operators, Ansible, Chef) compose stateful side-effects with no type-level guarantees. Declarative IaC (Terraform, Pulumi) types *desired state* but not the **certainty** of that state — a Terraform program that compiles may still produce runtime conflicts when two modules reference the same AWS IAM role, because disjointness is not enforced. Both families ignore the epistemological reality that all distributed state is *believed*, never *known* (cf. CAP theorem, PACELC).

### 1.2 The λ-L-E thesis

We claim — and mechanize in the Axon compiler — that:

1. Every computational resource (a database, a TCP socket, a GPU slot, a secret) is fundamentally **linear** in Girard's sense (1987): it is consumed by use and cannot be silently duplicated without cost.
2. Every distributed system holds **partial observations** whose certainty `c ∈ [0.0, 1.0]` must travel with the observation itself, not as an afterthought.
3. These two axioms, combined with Separation Logic's `*` disjointness (O'Hearn-Reynolds 2001), produce a calculus in which **infrastructure provisioning is a proof**, and the proof's soundness is machine-checkable at `axon check`.

The three axioms are realized by the four primitives of Fase 1: `resource`, `fabric`, `manifest`, `observe` (specified in [docs/plan_io_cognitivo.md §5 Fase 1](plan_io_cognitivo.md)).

---

## 2. Syntax (abstract)

```
Type     τ    ::=  Resource<κ>        (regulatory class κ ⊆ Κ)
                |  Fabric<p>          (provider p)
                |  Manifest<τ*, φ>    (resource list, fabric, compliance)
                |  Observe<ψ, Q>      (target manifest ψ, quorum Q)
                |  τ ⊸ τ              (linear implication)
                |  !τ                 (exponential — persistent lifetime)
                |  τ * τ              (separating conjunction)

Lifetime ℓ    ::=  linear | affine | persistent
Cert     c    ::=  ℝ ∩ [0, 1]
Envelope E    ::=  ⟨c, τ_t, ρ, δ⟩    (certainty, time, provenance, derivation)

Term     e    ::=  x | λx:τ.e | e e'
                |  resource k ℓ | fabric p | manifest ψ
                |  observe ψ | reconcile ψ | lease r Δt
                |  let x = e in e'
```

### 2.1 The Epistemic Envelope ΛD

Every term carries a ΛD envelope `E = ⟨c, τ_t, ρ, δ⟩`:

- **c** — certainty in `[0.0, 1.0]`; 1.0 = `know`, 0.0 = `void` (⊥)
- **τ_t** — temporal frame (ISO-8601 UTC); establishes the decay origin
- **ρ** — provenance identifier (handler, sensor, signer)
- **δ** — derivation: `axiomatic | observed | inferred | mutated`

ΛD is not metadata — it is part of the type judgment (§3.3).

### 2.2 The regulatory class κ

κ is a finite set of labels (HIPAA, PCI_DSS, GDPR, SOX, ...). The λ-L-E compiler statically verifies that every resource whose κ is non-empty crosses a shield whose compliance list ⊇ κ. This is **Compile-time Compliance** (plan §6.1); it turns regulation from an audit-time concern into a type-theory concern.

---

## 3. Judgments

### 3.1 Linearity judgment

    Γ, x: Resource<κ, linear>  ⊢  e : τ     ⟹     Γ  ⊢  λx.e  :  Resource<κ, linear> ⊸ τ

A linear resource binds once, consumed once. Affine is the weakening-permitted variant (O'Hearn-Reynolds); persistent is the exponential `!Resource<κ>`.

### 3.2 Disjointness judgment (Separation Logic)

    Γ ⊢  r₁ * r₂  :  Manifest<[r₁, r₂], φ>     ⟺     name(r₁) ≠ name(r₂)

The `*` connector lifts to manifest composition: resources listed in a single manifest inhabit disjoint regions of the infrastructure heap. Violation is a compile error (Fase 1 `TestManifestValidation.test_manifest_duplicate_resource_rejected_separation_logic`).

### 3.3 Epistemic judgment

    Γ ⊢ e : τ [E]    where    E = ⟨c, τ_t, ρ, δ⟩

`c` propagates **monotonically**: any computation consuming a term with c=k yields a term with c ≤ k. This is the lattice rule

    ⊥ ⊑ doubt ⊑ speculate ⊑ believe ⊑ know ⊑ ⊤

and the certainty carried by the output of an operator is bounded above by the **minimum** certainty of its inputs.

---

## 4. Reduction (β and τ-decay)

### 4.1 β-reduction is provisioning

    (λx:Resource<k,ℓ>. e) r    ⟶β    e[r/x]

At runtime the β-step invokes the Handler (Fase 2, Decision D1 — Free Monad + Handler). The Handler materializes the resource and the returned value carries a newly stamped ΛD envelope.

### 4.2 τ-decay is lease expiration

For any term `e : τ [⟨c, τ_t, ρ, δ⟩]` and wall-clock time `now`:

    c_at(now) = c · f(now − τ_t,  half_life)

with `f ∈ {exp, linear, constant}` depending on the decay mode. When `c_at(now) = 0`, the term is void (⊥) and any subsequent use triggers `LeaseExpiredError` (CT-2 Anchor Breach, Fase 3.2).

### 4.3 D4 — Partition as ⊥, not as doubt

Following Fase 3 Decision D4 (plan §2): a network partition during observation does NOT decay certainty to `doubt`. It raises a structural **CT-3 exception** (`NetworkPartitionError`). Rationale: doubt is a *conclusion from evidence*; a partition is the *absence of evidence* — an ontological void, not an epistemological uncertainty.

---

## 5. Soundness

### 5.1 Theorem — Stochastic Degenerative Soundness

Let `⊢ e : τ [E]`. If `e ⟶* e'`, then:

1. `⊢ e' : τ [E']` (type preservation), and
2. `E'.c ≤ E.c` (certainty monotonicity — the reduction may only destroy, never create, certainty without external evidence).

**Proof sketch.** Structural induction on the reduction relation. β-reduction preserves type by standard argument; the certainty constraint follows because every rule either (i) copies an envelope unchanged (identity), (ii) combines two envelopes by taking the pointwise minimum of their c components (conjunction), or (iii) applies a decay function f that is monotonic non-increasing in elapsed time. □

### 5.2 Corollary — No silent upgrade

No program can promote a term from `doubt` to `believe` without adding a new observation. This rules out the "launder the uncertainty" failure mode characteristic of tools that silently assume the latest cache is authoritative.

---

## 6. Correspondence with existing calculi

### 6.1 λ-L-E vs Girard's linear lambda (1987)

Girard's linear lambda has `⊸`, `⊗`, `!`. λ-L-E adds:
- regulatory class κ as a type parameter (implicit product: `Resource<κ> ≅ Resource × κ`);
- ΛD envelope as an effect-type annotation (a restricted indexed monad);
- Separation Logic's `*` for manifest composition.

### 6.2 λ-L-E vs Separation Logic (O'Hearn-Reynolds 2001)

SL's heap split `h = h₁ ⨄ h₂` lifts to: manifests with disjoint resources correspond to heap fragments that can be reasoned about independently. A manifest `M = [r₁, r₂]` has heap invariant `Inv(r₁) * Inv(r₂)` iff the affinity/linearity rules hold (Fase 1 post-pass `_check_resource_linearity()`).

### 6.3 λ-L-E vs Epistemic Logic (Fagin-Halpern-Moses-Vardi 1995)

EL's common-knowledge operator `Cφ` reads: *everyone knows that everyone knows … that φ*. λ-L-E realizes `Cφ` operationally as the output of an `ensemble` (Fase 3.3): a Byzantine quorum over N observers produces a HealthReport whose certainty is the fused minimum/weighted/harmonic aggregate of the individual c's. Below quorum ⇒ `InfrastructureBlameError` (CT-3), which is `¬Cφ` encoded as a structural exception.

### 6.4 Curry-Howard for infrastructure

Type | Proposition | Program | Proof
-----|-------------|---------|------
`Manifest<[r], φ>` | *exists a disjoint tuple of resources inhabiting φ* | the AST representing that manifest | the compiled IR generator's output
`τ ⊸ σ` | *consuming τ yields σ* | a λ-L-E function | the Free-Monad handler's β-step
`Observe<ψ, Q>` | *the ensemble Q over ψ has a consistent snapshot* | an observe declaration | the EnsembleAggregator outcome under quorum

### 6.5 The λ-L-E **twist** — stochastic proofs

Conventional Curry-Howard yields boolean proofs: either a term inhabits the type or it does not. λ-L-E produces **stochastic** proofs: the same term inhabits the type with certainty `c`. A program that compiles is a proof that certainty can be at least `c_min` at apply time; whether the actual runtime reaches that floor depends on the Handler's execution and the decay clock.

This is the *academic contribution*: a calculus in which correctness and confidence are the same algebraic object.

---

## 7. Relation to the Axon implementation

λ-L-E is not a paper abstraction — every rule above has a mechanized counterpart in the Axon compiler and runtime:

| λ-L-E construct | Axon code |
|---|---|
| `Resource<κ, ℓ>` | [ast_nodes.py `ResourceDefinition`](../axon/compiler/ast_nodes.py) |
| `Manifest<r*, φ>` | [ast_nodes.py `ManifestDefinition`](../axon/compiler/ast_nodes.py) |
| `*` disjointness check | [type_checker.py `_check_manifest`](../axon/compiler/type_checker.py) + `_check_resource_linearity()` |
| ΛD envelope `⟨c, τ_t, ρ, δ⟩` | [handlers/base.py `LambdaEnvelope`](../axon/runtime/handlers/base.py) |
| β-reduction as provisioning | [handlers/base.py `Handler.interpret`](../axon/runtime/handlers/base.py) |
| τ-decay | [lease_kernel.py `LeaseToken.envelope`](../axon/runtime/lease_kernel.py) |
| Partition = CT-3 | [handlers/base.py `NetworkPartitionError`](../axon/runtime/handlers/base.py) |
| Common knowledge `Cφ` | [ensemble_aggregator.py `EnsembleAggregator`](../axon/runtime/ensemble_aggregator.py) |
| Stochastic Degenerative Soundness | enforced by the combination of type_checker + certainty-preserving handler contract |

---

## 8. Worked example

```axon
type PHI compliance [HIPAA] { ssn: String }

resource PatientDb {
  kind: postgres
  lifetime: linear
  certainty_floor: 0.95
}

fabric ClinicalVPC {
  provider: aws
  region: "us-east-1"
}

manifest ProductionHealthcare {
  resources: [PatientDb]
  fabric: ClinicalVPC
  compliance: [HIPAA]
}

observe Health from ProductionHealthcare {
  sources: [prometheus, cloudwatch, healthcheck]
  quorum: 2
}
```

The compiler proves:
1. `PatientDb` is linear and appears in exactly one manifest (**linearity**).
2. `ProductionHealthcare` has a disjoint resource list (**separation**).
3. Any endpoint consuming PHI must gate it through a `shield<HIPAA>` (**compile-time compliance**).
4. `Health` produces a ΛD envelope whose certainty is a Byzantine-quorum aggregate over two of three observers (**Cφ**).

Every guarantee is *mechanical* — no runtime, no audit, no human review.

---

## 9. Related work

- Girard, J.-Y. (1987). *Linear Logic*. Theoretical Computer Science 50.
- O'Hearn, P.W. & Reynolds, J.C. (2001). *Separation Logic: A Logic for Shared Mutable Data Structures*.
- Fagin, R., Halpern, J.Y., Moses, Y., Vardi, M.Y. (1995). *Reasoning About Knowledge*.
- Findler, R.B. & Felleisen, M. (2002). *Contracts for Higher-Order Functions* — blame calculus.
- Friston, K. (2010). *The Free-Energy Principle: a unified brain theory?* — reused operationally in `immune`.
- Honda, K., Vasconcelos, V.T., Kubo, M. (1998). *Language primitives and type discipline for structured communication-based programming* — session types for Fase 4.
- Milner, R. (1999). *Communicating and Mobile Systems: The π-Calculus*.

---

## 10. Status

| Component | File | Tests |
|---|---|---|
| Linearity + separation | `axon/compiler/type_checker.py` | `tests/test_type_checker.py::TestResourcePrimitives` (14) |
| ΛD envelope | `axon/runtime/handlers/base.py` | `tests/test_handlers_base.py::TestLambdaEnvelope` (6) |
| τ-decay lease kernel | `axon/runtime/lease_kernel.py` | `tests/test_phase3_runtime.py::TestLeaseKernel` (9) |
| Cφ common knowledge | `axon/runtime/ensemble_aggregator.py` | `tests/test_phase3_runtime.py::TestEnsembleAggregator` (8) |
| Partition = CT-3 (D4) | `handlers/base.NetworkPartitionError` | `tests/test_handlers_base.py` (2) |
| Compile-time compliance | `axon/compiler/type_checker._check_regulatory_compliance()` | `tests/test_phase6_language.py::TestComplianceCoverage` (7) |

> **Paper status:** v1.0 — Formal foundation established; mechanization complete; Theorem 5.1 stated and supported by the type-checker + handler invariants. A fully paper-proof mechanization in Coq/Lean is reserved for future work.

> **Authored by:** AXON Language Team
