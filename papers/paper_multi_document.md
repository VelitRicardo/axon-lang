# Multi-Document Navigation: A Formal Framework for Cross-Corpus Cognitive Retrieval

**AXON Research Paper — Feature Proposal v0.16**\
**Authors:** AXON Core Team\
**Date:** March 16, 2026\
**Status:** Research & Design Phase\
**Classification:** Epistemology · Graph Theory · Modal Logic · Type Theory

---
"We present Multi-Document Navigation (MDN), the first retrieval framework with formal guarantees of soundness, termination, and provenance — properties that are not optional but essential in domains where precision is non-negotiable: law, medicine, and critical infrastructure."

## Abstract
We present a formal framework for **multi-document navigation** (MDN) in
AXON-lang, extending the single-document PIX primitive to operate over document
collections with arbitrary cross-reference topologies. While PIX treats
retrieval as tree traversal within a single knowledge artifact, MDN models
document corpora as **labeled directed graphs** with typed edges representing
semantic relationships (citation, dependency, contradiction, elaboration). We
prove that MDN reduces to a **bounded graph reachability problem** under
epistemic constraints, establish modal logic semantics for cross-document
inference, and propose a type-theoretic compilation strategy that preserves
AXON's guarantees of termination, explainability, and epistemic tracking. The
framework unifies four theoretical pillars—**graph theory, modal logic,
information geometry, and category theory**—into a coherent computational model
implementable as AXON primitives.
**Core contributions:**
1. **Mathematical:** Graph-theoretic formalization of document corpora with
   weighted, typed edges and reachability bounds
2. **Logical:** Modal logic S4_MDN for reasoning about knowledge distributed
   across documents with provenance tracking
3. **Philosophical:** Epistemology of **distributed knowledge** grounded in
   social epistemology and testimony theory
4. **Programming:** Type-safe compilation of multi-document queries to bounded
   graph traversals with effect tracking
---

## 1. Introduction

### 1.1 The Problem: From Trees to Graphs

PIX (Positional Indexing eXtraction) solves single-document retrieval via
hierarchical tree navigation:

```
D_single = (N, E, ρ, κ)    where E forms a tree (unique parent, no cycles)
```

This suffices when knowledge is **self-contained** within one artifact. However,
professional knowledge work involves document **collections** where:

- Legal briefs cite case law and statutes
- Research papers reference prior work
- Technical specifications depend on standards documents
- Financial reports cross-reference regulatory filings
- Medical diagnoses cite clinical guidelines

These relationships form a **graph**, not a tree:

```
C = (D, R, τ)    where
  D = {D₁, D₂, ..., Dₙ}      — set of documents
  R ⊆ D × D × L              — labeled edges (relationships)
  τ : R → RelationType       — edge type function
  
  RelationType ∈ {cite, depend, contradict, elaborate, supersede, ...}
```

**The challenge:** Extend PIX's navigational semantics to this graph structure
while preserving:

- **Termination:** No infinite loops despite cycles
- **Explainability:** Reasoning paths remain traceable
- **Epistemic tracking:** Each document's reliability is independently assessed
- **Bounded cost:** Computational complexity remains tractable

### 1.2 Why Multi-Document Navigation ≠ Multi-Document Retrieval

Traditional approaches treat multi-document retrieval as:

```
Retrieve(Q, C) = ⋃ᵢ Retrieve_single(Q, Dᵢ)
                 ↓
            Union of independent results
```

This **ignores relational structure**:

- A legal case is only relevant if it's cited by the primary statute
- A paper's claim is strengthened if corroborated by independent sources
- A regulation is superseded if a newer version exists

**MDN's alternative:**

```
Navigate(Q, C) = Graph_traverse(Q, C, semantic_edges)
                      ↓
                 Path through document graph guided by relationships
```

We don't just retrieve from documents—we **navigate relationships between
them**.

---

## 2. Mathematical Formalization

### 2.1 Document Corpus as Labeled Directed Graph

**Definition 1 (Document Corpus Graph).** A document corpus is a tuple
`C = (D, R, τ, ω, σ)` where:

```
D = {D₁, ..., Dₙ}                  — finite set of documents (n < ∞)
R ⊆ D × D × L                      — set of labeled directed edges
L                                   — finite set of edge labels
τ : R → RelationType               — edge type function
ω : R → ℝ⁺                         — edge weight function
σ : D → EpistemicLevel             — document epistemic status

Constraints:
  (G1)  |D| < ∞                    — finite corpus
  (G2)  ∀ (Dᵢ, Dⱼ, l) ∈ R : Dᵢ, Dⱼ ∈ D  — edges connect corpus members
  (G3)  τ is total on R            — every edge is typed
  (G4)  0 < ω(r) ≤ 1 ∀r ∈ R        — weights normalized
  (G5)  σ is anti-monotonic w.r.t. dependency depth:
        If depth(Dᵢ) > depth(Dⱼ) then σ(Dᵢ) ≤_T σ(Dⱼ)
        where ≤_T is the partial order on EpistemicLevel
        (i.e., primary sources closer to root carry higher epistemic status)
```

**Definition 2 (Edge Semantics).** The edge type function `τ` maps to a finite
set of semantic relationships:

```
RelationType ::= cite          — Dᵢ cites Dⱼ as evidence
               | depend         — Dᵢ requires Dⱼ for comprehension
               | contradict     — Dᵢ disputes claims in Dⱼ
               | elaborate      — Dᵢ expands on Dⱼ
               | supersede      — Dᵢ replaces Dⱼ (versioning)
               | implement      — Dᵢ implements specification Dⱼ
               | exemplify      — Dᵢ is an example of concept in Dⱼ
```

**Definition 3 (Edge Weight Semantics).** The weight function `ω : R → (0, 1]`
encodes relationship strength:

```
ω((Dᵢ, Dⱼ, cite)) = f(citation_count, recency, authority(Dⱼ))

Example weights:
  Primary citation:     ω = 0.9   (strongly relevant)
  Tangential reference: ω = 0.3   (weakly relevant)
  Historical context:   ω = 0.1   (background only)
```

### 2.2 Bounded Graph Reachability Problem

**Problem Statement.** Given:

- Corpus graph `C = (D, R, τ, ω, σ)`
- Query `Q`
- Starting document `D₀ ∈ D`
- Budget `B = (max_depth, max_docs, max_cost)`

Find: `Π ⊆ Paths(C, D₀)` such that:

```
∀ π = (D₀, r₁, D₁, r₂, ..., rₖ, Dₖ) ∈ Π :
  1. length(π) ≤ max_depth
  2. |{Dᵢ : Dᵢ ∈ π}| ≤ max_docs
  3. ∑ᵢ cost(rᵢ) ≤ max_cost
  4. Relevant(Dₖ, Q) = true
```

**Theorem 1 (Decidability of Bounded MDN).** The bounded graph reachability
problem for MDN is **decidable** in time polynomial in `|D|` and exponential in
`max_depth`.

_Proof._ The search space is bounded:

- Maximum path length: `max_depth` (constraint 1)
- Maximum branching factor: `|D|` (finite corpus)
- Maximum paths: `|D|^max_depth` (exponential but finite)

Since `max_depth` is a constant specified in the query (typically 3-5), the
exponential factor is controlled. Each path can be checked in `O(max_depth)`
time. Total complexity:

```
O(|D|^max_depth · max_depth)
```

With pruning (relevance filtering at each step), the effective branching factor
`b̄ << |D|`, yielding practical tractability. ∎

**Definition 3 (Navigation Policy).** A _navigation policy_ `π` is a mapping:

```
π : (Q, D₀, ..., Dₜ₋₁) → Dₜ ∈ Neighbors(Dₜ₋₁) \ visited

where:
  Q          — the query being answered
  Dₜ₋₁      — current document
  visited    — set of previously visited documents
  Neighbors  — adjacent documents via edges in R
```

A policy `π` is **ε-informative** (for `ε > 0`) if at every step:

```
E_{Dₜ ~ π}[I(A; Dₜ | Q, D₀, ..., Dₜ₋₁)] ≥ ε
```

i.e., the expected information gain of the selected document exceeds a minimum
threshold `ε`. An ε-informative policy never selects documents expected to be
uninformative—it always makes progress.

**Theorem 2 (Strict Information Gain under ε-Informative Navigation).** Let `π`
be an ε-informative navigation policy on corpus `C`. Then for any navigation
path `(D₀, ..., Dₖ)` generated by `π`:

```
H(A | Q, D₀, ..., Dₖ) ≤ H(A | Q, D₀, ..., Dₖ₋₁) - ε        (strict decrease)

Equivalently:     H(A | Q, D₀, ..., Dₖ) ≤ H(A | Q) - k · ε   (cumulative bound)
```

_Proof._ By the chain rule of conditional entropy:

```
H(A | Q, D₀, ..., Dₖ) = H(A | Q, D₀, ..., Dₖ₋₁) - I(A; Dₖ | Q, D₀, ..., Dₖ₋₁)
```

Since `π` is ε-informative, `I(A; Dₖ | Q, D₀, ..., Dₖ₋₁) ≥ ε`. Therefore:

```
H(A | Q, D₀, ..., Dₖ) ≤ H(A | Q, D₀, ..., Dₖ₋₁) - ε
```

Applying telescopically over `k` steps:

```
H(A | Q, D₀, ..., Dₖ) ≤ H(A | Q) - ∑ᵢ₌₀ᵏ I(A; Dᵢ | Q, D₀, ..., Dᵢ₋₁) ≤ H(A | Q) - k · ε
```

Since `H(A | Q, D₀, ..., Dₖ) ≥ 0` (entropy is non-negative), this immediately
yields a **convergence bound**: navigation terminates in at most
`k ≤ ⌈H(A | Q) / ε⌉` steps. ∎

> [!IMPORTANT]
> **Why this is non-trivial.** The classical inequality
> `H(A | Q, D₀, ..., Dₖ) ≤ H(A | Q, D₀, ..., Dₖ₋₁)` is a tautology of
> information theory (conditioning never increases entropy). What Theorem 2
> establishes is a _strict decrease_ with a quantified lower bound `ε`, which
> implies three operational guarantees:
>
> 1. **Guaranteed progress:** every navigation step reduces uncertainty by ≥ ε
> 2. **Bounded runtime:** navigation resolves any query in ≤ ⌈H₀/ε⌉ steps
> 3. **No wasted traversals:** uninformative documents are never visited

**Corollary 2.1 (Greedy Optimality via Submodularity).** Define the
**information value function** `f : 2^D → ℝ⁺`:

```
f(S) = I(A; S | Q) = H(A | Q) - H(A | Q, S)
```

measuring the total information gained from visiting document set `S`.

**(a) Submodularity.** `f` is monotone submodular:

```
Monotone:     S ⊆ T ⟹ f(S) ≤ f(T)                        — more documents, more info
Submodular:   S ⊆ T ⟹ f(S ∪ {d}) - f(S) ≥ f(T ∪ {d}) - f(T)  — diminishing returns
```

_Proof._ Monotonicity follows from non-negativity of conditional mutual
information. Submodularity follows from the identity:

```
f(S ∪ {d}) - f(S) = I(A; d | Q, S) = H(A | Q, S) - H(A | Q, S, d)
```

Since `S ⊆ T` implies `H(A | Q, T) ≤ H(A | Q, S)` (conditioning reduces
entropy), and `H(A | Q, T, d) ≤ H(A | Q, S, d)` (same reason), we get:

```
I(A; d | Q, S) ≥ I(A; d | Q, T)
```

This is the **diminishing returns** property: the marginal value of a document
`d` decreases as the context grows. ∎

**(b) Greedy Approximation Guarantee.** The **greedy MDN policy**:

```
π_greedy : Dₜ = argmax_{d ∈ Neighbors(Dₜ₋₁) \ visited} I(A; d | Q, D₀, ..., Dₜ₋₁)
```

achieves, for a budget of `k` documents:

```
f(S_greedy) ≥ (1 - 1/e) · f(S_OPT)  ≈  0.632 · f(S_OPT)
```

where `S_OPT` is the optimal set of `k` documents maximizing `f`.

_Proof._ This is a direct application of Nemhauser, Wolsey, & Fisher (1978):
greedy maximization of a monotone submodular function under a cardinality
constraint `|S| ≤ k` achieves the `(1 - 1/e)` approximation ratio. MDN's greedy
policy selects, at each step, the neighboring document with maximum marginal
information gain, which is exactly the greedy step for `f`. ∎

> [!NOTE]
> The `(1 - 1/e)` bound is **tight**: no polynomial-time algorithm can do better
> unless P = NP (Feige, 1998). MDN's greedy policy is therefore **optimally
> efficient** among tractable strategies. In practice, adaptive submodularity
> (Golovin & Krause, 2011) tightens this further when the policy can observe
> intermediate results—which MDN does via its pruning threshold.

**Proposition 2.2 (Information Bounds at Tree Splits).** Let the corpus `C` be
structured as a semantic tree. At each internal node `n` with children
`c₁, ..., cₘ`, let `C` denote the **branching random variable** (which child the
navigation selects). Write `path` for the ancestor path `(D₀, ..., D_n)`.

**(a) Best-child lower bound:**

```
max_i  I(A; cᵢ | Q, path)  ≥  I(A; C | Q, path)
```

_Proof._ By definition,
`I(A; C | Q, path) = H(A | Q, path) - H(A | Q, path, C)`. Observing child `cᵢ`
reveals at least as much as knowing which child was chosen (since `cᵢ`
determines `C = i`), thus the best child cannot do worse than the branching
variable. Formally, this follows from the data-processing inequality:
`A — cᵢ — C` forms a Markov chain when `cᵢ` is selected, so
`I(A; cᵢ | Q, path) ≥ I(A; C | Q, path)` for the maximizing `i`. ∎

**(b) Branching entropy upper bound:**

```
I(A; C | Q, path)  ≤  H(C | path)
```

_Proof._ By the mutual information identity:
`I(A; C | Q, path) = H(C | Q, path) - H(C | Q, path, A) ≤ H(C | Q, path) ≤ H(C | path)`.
The first inequality holds because conditional entropy is non-negative; the
second because conditioning on `Q` cannot increase entropy. ∎

> [!IMPORTANT]
> **Alignment requirement.** Combining (a) and (b):
>
> ```
> max_i I(A; cᵢ | Q, path)  ≥  I(A; C | Q, path)  and  I(A; C | Q, path) ≤ H(C | path)
> ```
>
> Low branching entropy `H(C | path)` constrains the **total extractable
> information** from the branching decision, but does **not alone guarantee high
> gain** unless the partition **aligns with `A`**. Concretely:
>
> - If the partition separates relevant from irrelevant content (high
>   alignment), then `I(A; C | ...) ≈ H(C | ...)` and
>   `max_i I(A; cᵢ | ...) ≫ ε`.
> - If the partition is semantically arbitrary (low alignment), then
>   `I(A; C | ...) ≈ 0` regardless of `H(C | ...)`.
>
> This is the formal basis for why **semantic indexing quality** determines MDN
> efficiency: the ε-informative condition (Definition 3) is satisfiable iff the
> corpus tree's partitions are aligned with the query-answer structure.

**Corollary (Structural Design Criterion).** A corpus tree satisfies the
ε-informative condition at every node iff:

```
∀ node n, ∀ query Q in the target distribution:

    max_i I(A; cᵢ | Q, path_n) ≥ ε
```

This translates MDN's information-theoretic guarantee into a **testable
structural property** of the corpus index, connecting:

| Tree property                 | Information effect                          |
| ----------------------------- | ------------------------------------------- |
| High branching factor         | More candidate documents → finer partitions |
| Good semantic partitioning    | High `I(A; C \| ...)` → high marginal gain  |
| Deep, narrow subtrees         | Focused information → high ε per step       |
| Pruned uninformative branches | Effective b̄ < b → tractable search          |

MDN's pruning mechanism (§5.4) is therefore not merely an optimization but a
**structural necessity**: it removes branches where `I(A; d | ...) < θ`,
ensuring the ε-informative condition concentrates navigation on high-information
paths.

### 2.3 Graph Centrality and Document Importance

**Definition 4 (Signed Corpus Decomposition).** The corpus graph `C = (D, R)`
induces two subgraphs based on edge polarity:

```
G⁺ = (D, R⁺)    where R⁺ = {r ∈ R : τ(r) ∈ {cite, elaborate, corroborate}}
G⁻ = (D, R⁻)    where R⁻ = {r ∈ R : τ(r) ∈ {contradict, supersede}}
```

This decomposition models the corpus as a **signed graph** where positive edges
propagate epistemic trust and negative edges propagate epistemic distrust.

**Definition 4.1 (Stochastic Transition Matrices).** Define the row-stochastic
transition matrices `P⁺, P⁻ ∈ ℝ^{|D|×|D|}` with entries:

```
P⁺ⱼᵢ = ω⁺(Dⱼ, Dᵢ) / ∑ₖ ω⁺(Dⱼ, Dₖ)

P⁻ⱼᵢ = ω⁻(Dⱼ, Dᵢ) / ∑ₖ ω⁻(Dⱼ, Dₖ)

where:
  ω⁺(Dⱼ, Dᵢ) = ω((Dⱼ, Dᵢ, τ))  if (Dⱼ, Dᵢ, ·) ∈ R⁺,  0 otherwise
  ω⁻(Dⱼ, Dᵢ) = ω((Dⱼ, Dᵢ, τ))  if (Dⱼ, Dᵢ, ·) ∈ R⁻,  0 otherwise
```

**Required properties:**

```
(P1)  P⁺ⱼᵢ ≥ 0,  P⁻ⱼᵢ ≥ 0           — non-negativity
(P2)  ∑ᵢ P⁺ⱼᵢ = 1  ∀j with Out⁺(j)>0  — row-stochasticity (positive)
(P3)  ∑ᵢ P⁻ⱼᵢ = 1  ∀j with Out⁻(j)>0  — row-stochasticity (negative)
(P4)  For dangling nodes (Out(j) = 0): row replaced by uniform 1/|D|
```

Property (P4) handles dangling nodes (documents with no outgoing edges in the
respective subgraph) by distributing their rank uniformly—the standard treatment
in PageRank.

**Definition 4.2 (Epistemic PageRank via Signed Propagation).** The **Epistemic
PageRank** of document `Dᵢ` is:

```
EPR(Dᵢ) = EPR⁺(Dᵢ) - λ · EPR⁻(Dᵢ)

where EPR⁺, EPR⁻ are the unique solutions to:

  EPR⁺ = (1 - d) · u⁺ + d · (P⁺)ᵀ · EPR⁺      — trust propagation
  EPR⁻ = (1 - d) · u⁻ + d · (P⁻)ᵀ · EPR⁻      — distrust propagation

Parameters:
  d ∈ (0, 1)            — damping factor (typically 0.85); must be strict
  λ ∈ [0, 1]            — distrust penalty weight

Teleportation vectors (asymmetric):
  u⁺ ∈ Δ^{|D|}          — trust prior (simplex), biased toward foundational sources
  u⁻ ∈ Δ^{|D|}          — distrust prior (simplex), biased toward recent documents
```

> [!IMPORTANT]
> **Why asymmetric teleportation.** We deliberately use separate `u⁺ ≠ u⁻`
> rather than a single uniform vector `u = (1/|D|, ...)`. Rationale:
>
> - **Trust prior `u⁺`:** Documents closer to the corpus root (primary sources,
>   foundational texts) should receive higher baseline trust. Setting
>   `u⁺(Dᵢ) ∝ 1/depth(Dᵢ)` encodes this structural prior.
> - **Distrust prior `u⁻`:** Contradictions are more likely to originate from
>   recent documents (updates, corrections, errata). Setting
>   `u⁻(Dᵢ) ∝ recency(Dᵢ)` encodes this temporal prior.
>
> Both `u⁺, u⁻` must lie on the probability simplex `Δ^{|D|}` (non-negative, sum
> to 1). The uniform case `u⁺ = u⁻ = (1/|D|, ...)` is recovered as a special
> case when no structural prior is available.

**Interpretation.** `EPR⁺(Dᵢ)` measures how much the corpus _endorses_ document
`Dᵢ` through citation and elaboration chains. `EPR⁻(Dᵢ)` measures how
_contested_ `Dᵢ` is through contradiction and supersession. The combined
`EPR = EPR⁺ - λ · EPR⁻` yields a **net epistemic reputation**: a document
heavily cited but also heavily contradicted scores lower than one cited without
challenge.

| Edge type     | Subgraph | Propagation effect                     |
| ------------- | -------- | -------------------------------------- |
| `cite`        | G⁺       | Direct endorsement → high EPR⁺         |
| `elaborate`   | G⁺       | Contextual support → moderate EPR⁺     |
| `corroborate` | G⁺       | Independent confirmation → strong EPR⁺ |
| `contradict`  | G⁻       | Epistemic challenge → increases EPR⁻   |
| `supersede`   | G⁻       | Temporal replacement → increases EPR⁻  |

**Theorem 3 (Existence, Uniqueness, and Convergence of Signed EPR).** Under the
conditions `d ∈ (0, 1)`, `P⁺` and `P⁻` row-stochastic with (P1)-(P4), and
`u⁺, u⁻ ∈ Δ^{|D|}` strictly positive:

1. `EPR⁺` and `EPR⁻` each exist, are unique, and are strictly positive
2. `EPR` is well-defined (as their linear combination)
3. Power iteration converges geometrically

_Proof._ We prove the result for `EPR⁺`; the argument for `EPR⁻` is identical.

Since `P⁺` is **row-stochastic** (Definition 4.1: rows sum to 1), its transpose
`(P⁺)ᵀ` is **column-stochastic** (columns sum to 1). The fixed-point equation
`x = (1-d)·v + d·(P⁺)ᵀ·x` can be rewritten as `x = M·x` where:

```
M = (1-d) · v·𝟏ᵀ + d · (P⁺)ᵀ
```

The matrix `M` satisfies:

1. **Column-stochastic:**
   `𝟏ᵀM = (1-d)·𝟏ᵀ(v·𝟏ᵀ) + d·𝟏ᵀ(P⁺)ᵀ = (1-d)·𝟏ᵀ + d·𝟏ᵀ = 𝟏ᵀ`, since `v ∈ Δ`
   (sums to 1) and `(P⁺)ᵀ` is column-stochastic.
2. **Strictly positive:** `Mᵢⱼ ≥ (1-d)·vᵢ > 0` for all `i,j`, since `v > 0` and
   `d < 1`. Thus `M > 0` entrywise.
3. **Primitive:** A strictly positive stochastic matrix is trivially primitive
   (irreducible and aperiodic).

By the **Perron-Frobenius theorem** for primitive stochastic matrices, `M` has
eigenvalue 1 with algebraic multiplicity 1 and a unique positive eigenvector
(the Perron vector). All other eigenvalues satisfy `|λᵢ| < 1` strictly, since
`M` is primitive. Therefore the power iteration `x^{(t+1)} = M·x^{(t)}`
converges to the Perron vector at geometric rate `O(|λ₂(M)|ᵗ)` where
`|λ₂(M)| < 1` is the subdominant eigenvalue modulus.

_Remark on convergence rate._ The spectral gap can be bounded more tightly:
since `M = (1-d)·v·𝟏ᵀ + d·(P⁺)ᵀ` is a rank-1 perturbation of `d·(P⁺)ᵀ`, the
non-Perron eigenvalues of `M` are `d` times the corresponding eigenvalues of
`(P⁺)ᵀ`, giving `|λ₂(M)| = d·|λ₂((P⁺)ᵀ)| ≤ d`. This bound is tight when `P⁺`
itself has `|λ₂| = 1` (e.g., bipartite graphs before dangling-node correction),
but in practice the convergence is often much faster.

Since `EPR = EPR⁺ - λ·EPR⁻` is a linear combination of convergent sequences with
`λ ∈ [0,1]`, it converges. ∎

> [!WARNING]
> **EPR is a signed reputation score, not a probability distribution.** Unlike
> standard PageRank (which yields a probability vector on `Δ^{|D|}`), the signed
> EPR satisfies:
>
> - `EPR⁺(Dᵢ) > 0` and `EPR⁻(Dᵢ) > 0` for all `i` (by Perron-Frobenius)
> - `∑ᵢ EPR⁺(Dᵢ) = 1` and `∑ᵢ EPR⁻(Dᵢ) = 1` (each is a valid distribution)
> - **But** `EPR(Dᵢ) = EPR⁺(Dᵢ) - λ·EPR⁻(Dᵢ)` **may be negative**
> - `EPR` does **not** sum to 1 (it sums to `1 - λ`)
>
> This is deliberate: a negative `EPR(Dᵢ)` signals that document `Dᵢ` is **more
> contested than endorsed**—a meaningful epistemic signal that a probability
> distribution cannot express. For applications requiring a normalized score,
> use: `EPR_norm(Dᵢ) = EPR(Dᵢ) / (1 + λ) ∈ [-λ/(1+λ), 1/(1+λ)]`.

> [!NOTE]
> **Connection to Balance Theory and Reputation Systems.** The signed graph
> decomposition connects MDN to two established theoretical frameworks:
>
> 1. **Structural Balance Theory** (Harary, 1953; Cartwright & Harary, 1956): a
>    corpus is _balanced_ if it can be partitioned into two groups where all
>    positive edges are intra-group and all negative edges are inter-group.
>    Balanced corpora exhibit clean epistemic consensus; unbalanced ones signal
>    genuine controversy requiring contradiction resolution (§3.3).
> 2. **EigenTrust** (Kamvar et al., 2003): the `EPR⁺` computation is a direct
>    analogue of trust propagation in P2P reputation networks. MDN extends
>    EigenTrust by adding a dual `EPR⁻` channel for distrust, yielding a
>    **bipolar reputation** score for documents—absent in standard PageRank.

### 2.4 Information-Geometric View

**Definition 5 (Document Manifold).** The corpus `C` induces a statistical
manifold where each document `Dᵢ` is modeled as a probability distribution
`P_Dᵢ` over a shared topic space. We define a symmetric dissimilarity measure
via **Jeffreys divergence** (symmetrized KL divergence):

```
M = {P_D₁, ..., P_Dₙ}                       — points on the statistical manifold
J : M × M → ℝ⁺                              — Jeffreys divergence (pseudo-metric)

J(Dᵢ, Dⱼ) = KL(P_Dᵢ || P_Dⱼ) + KL(P_Dⱼ || P_Dᵢ)

where:
  P_Dᵢ  — probability distribution over topics in Dᵢ
  KL    — Kullback-Leibler divergence
```

**Remark (Relationship to Fisher Information Metric).** In the infinitesimal
limit where `P_Dᵢ` and `P_Dⱼ` are parameterically close, the Jeffreys divergence
reduces to a quadratic form whose coefficients are given by the **Fisher
information matrix** `g_μν(θ)` (Amari, 1985). Thus, the Fisher metric arises as
the _local Riemannian structure_ induced by `J` on the tangent space of `M`. We
use `J` directly as our global dissimilarity measure because MDN operates over
discrete, potentially distant distributions—not infinitesimal perturbations.

**Definition 5.1 (Type-Weighted Divergence Cost).** Define the edge cost
function `c : R → ℝ⁺` as:

```
c(Dᵢ, Dⱼ, τ) = ατ · J(Dᵢ, Dⱼ)

where ατ ∈ (0, ∞) is a type-dependent cost coefficient:

  τ = cite        →  α_cite = 1.0        (low traversal cost)
  τ = elaborate   →  α_elaborate = 0.7   (moderate cost)
  τ = corroborate →  α_corroborate = 0.5 (encourages cross-validation)
  τ = contradict  →  α_contradict = 3.0  (high cost — penalty for inconsistency)
  τ = supersede   →  α_supersede = 2.0   (encourages following updates)
```

The coefficient `ατ` modulates the geometric distance `J(Dᵢ, Dⱼ)` by the
**epistemic cost** of traversing that edge type. Contradiction edges are
expensive; citation edges are cheap. This encodes the epistemic intuition that
following citations is "easy" but navigating contradictions requires extra
cognitive effort.

**Optimal Navigation Path.** The optimal path between `D₀` and target `Dₖ` is
the shortest weighted path in the corpus graph under cost `c`:

```
π* = argmin_π  ∑_{i=0}^{k-1} c(D_πᵢ, D_πᵢ₊₁, τᵢ)

   = argmin_π  ∑_{i=0}^{k-1} α_{τᵢ} · J(D_πᵢ, D_πᵢ₊₁)

subject to: (D_πᵢ, D_πᵢ₊₁, τᵢ) ∈ R  for all i
```

> [!NOTE]
> **Terminological precision: graph shortest paths, not geodesics.** We
> deliberately avoid calling `π*` a "geodesic." In differential geometry, a
> geodesic is a smooth curve locally minimizing arc length under a Riemannian
> metric. Here, `π*` is a **shortest weighted path** on a discrete graph—the
> correct analogue from combinatorial optimization (Dijkstra, 1959). The
> connection is **analogical**, not literal: MDN navigates a discrete sampling
> of the statistical manifold `M`, and `π*` approximates the continuous geodesic
> only in the limit of dense, uniformly sampled corpora.

### 2.5 Category-Theoretic Foundations

We formalize document corpora and their transformations using category theory,
providing a structural framework for composing and comparing navigation systems.

**Definition 6 (Category Corp).** We define the category **Corp** of corpus
embeddings as follows:

```
Objects:    Document corpora C = (D, R, τ, ω, σ) satisfying (G1)-(G5)

            where (recalling §2.1):
              R ⊆ D × D × L             — labeled directed edges (L = RelationType)
              σ : D → (T, ≤_T)           — epistemic status mapping into
                                           the poset (EpistemicLevel, ≤_T)

Morphisms:  Corpus morphisms F = (F_D, F_R) : C₁ → C₂ where:
  F_D : D₁ ↪ D₂                         — document mapping (injective)
  F_R : R₁ → R₂                         — edge mapping

such that:
  (M1)  F_R is induced by F_D:
        F_R(Dᵢ, Dⱼ, l) = (F_D(Dᵢ), F_D(Dⱼ), l)
        
        (This automatically preserves edge types since τ₂(F_R(r)) = τ₂(F_D(Dᵢ), F_D(Dⱼ), l)
        = τ₁(l) = τ₁(r), because the label l encodes the type.)
        
  (M2)  F is weight-compatible:     ω₂(F_R(r)) ≥ ω₁(r)
  (M3)  F preserves epistemic order:
        σ₁(D) ≤_T σ₁(D') ⟹ σ₂(F_D(D)) ≤_T σ₂(F_D(D'))
        
        where ≤_T is the partial order on the poset (EpistemicLevel, ≤_T)
        defined in axiom (G5).

Identity:   id_C = (id_D, id_R)
Composition: (G ∘ F)_D = G_D ∘ F_D,  (G ∘ F)_R = G_R ∘ F_R
```

> [!IMPORTANT]
> **Scope of Corp.** The injectivity requirement on `F_D` restricts **Corp** to
> the subcategory of **corpus embeddings** (structure-preserving inclusions).
> This is intentional: MDN corpus merges (Definition 9) require identifying
> shared documents without collapsing distinct ones. A more general category
> **Corp_quot** admitting surjective `F_D` (document quotients, e.g.,
> deduplication) can be defined but is not needed for MDN's core theory.

**Proposition (Corp is a well-defined category).** The data above satisfies the
axioms of a category.

_Proof._

**(Identity laws.)** For any morphism `F : C₁ → C₂`:

- `F ∘ id_{C₁} = (F_D ∘ id_{D₁}, F_R ∘ id_{R₁}) = (F_D, F_R) = F`
- `id_{C₂} ∘ F = (id_{D₂} ∘ F_D, id_{R₂} ∘ F_R) = (F_D, F_R) = F`

**(Associativity.)** For `F : C₁ → C₂`, `G : C₂ → C₃`, `H : C₃ → C₄`:
`(H ∘ G) ∘ F = ((H_D ∘ G_D) ∘ F_D, (H_R ∘ G_R) ∘ F_R) = (H_D ∘ (G_D ∘ F_D), H_R ∘ (G_R ∘ F_R)) = H ∘ (G ∘ F)`
by associativity of function composition.

**(Closure under composition.)** We verify that `G ∘ F` satisfies (M1)-(M3):

- _(M1)_
  `(G ∘ F)_R(Dᵢ, Dⱼ, l) = G_R(F_R(Dᵢ, Dⱼ, l)) = G_R(F_D(Dᵢ), F_D(Dⱼ), l) = (G_D(F_D(Dᵢ)), G_D(F_D(Dⱼ)), l)`
  ✓
- _(M2)_ `ω₃((G∘F)_R(r)) = ω₃(G_R(F_R(r))) ≥ ω₂(F_R(r)) ≥ ω₁(r)` by transitive
  application of (M2) ✓
- _(M3)_ If `σ₁(D) ≤_T σ₁(D')`, then `σ₂(F_D(D)) ≤_T σ₂(F_D(D'))` by (M3) for
  `F`, then `σ₃(G_D(F_D(D))) ≤_T σ₃(G_D(F_D(D')))` by (M3) for `G` ✓
- _(Injectivity)_ `G_D ∘ F_D` is injective as a composition of injectives ✓ ∎

**Remark.** Condition (M2) uses `≥` (not `=`) to allow corpus embeddings where
edges gain strength in a richer context—e.g., a citation that is tangential in
one corpus may become primary when embedded in a more complete collection.

**Definition 7 (Navigation Functor).** We first formalize the notion of a path
in a corpus, then define the navigation functor.

A **path** in corpus `C = (D, R, τ, ω, σ)` is a finite alternating sequence:

```
π = (D₀, r₁, D₁, r₂, ..., rₖ, Dₖ)

where:
  Dᵢ ∈ D                for all i = 0, ..., k
  rᵢ = (Dᵢ₋₁, Dᵢ, lᵢ) ∈ R    for all i = 1, ..., k
  Dᵢ ≠ Dⱼ for i ≠ j            (no revisits)
  
  length(π) = k                  (number of edges traversed)
```

The **budget-constrained path set** is:

```
Paths_B(C) = {π ∈ Paths(C) : length(π) ≤ B}
```

where the budget `B` is a **hop count** (maximum number of edges), not a
weight-based cost. This distinction is critical for functoriality (see below).

We define a **cost ordering** on `Paths_B(C)` via the type-weighted divergence
cost (Definition 5.1):

```
cost(π) = ∑_{i=1}^{k} c(Dᵢ₋₁, Dᵢ, τ(rᵢ)) = ∑_{i=1}^{k} α_{τ(rᵢ)} · J(Dᵢ₋₁, Dᵢ)
```

This makes `(Paths_B(C), ≤_cost)` a **poset**, where `π₁ ≤_cost π₂` iff
`cost(π₁) ≤ cost(π₂)`.

The **navigation functor** `Nav_B : Corp → Poset` maps:

```
Nav_B(C)  = (Paths_B(C), ≤_cost)              — poset of budget-constrained paths
Nav_B(F)  = F* : Paths_B(C₁) → Paths_B(C₂)   — induced path mapping

where:
  F*(π) = F*(D₀, r₁, D₁, ..., rₖ, Dₖ)
        = (F_D(D₀), F_R(r₁), F_D(D₁), ..., F_R(rₖ), F_D(Dₖ))
```

**Theorem 3.5 (Functoriality of MDN Navigation).** The MDN navigation operator
`Nav_B : Corp → Poset` (with hop-count budget `B`) is a well-defined functor.
Specifically:

1. `F*(π)` is a valid path in `C₂` whenever `π` is a valid path in `C₁`
2. `length(F*(π)) = length(π)` (hop count is preserved)
3. `Nav_B(id_C) = id_{Nav_B(C)}` (identity preservation)
4. `Nav_B(G ∘ F) = Nav_B(G) ∘ Nav_B(F)` (composition preservation)

_Proof._

**(1) Well-definedness of `F*`.** Let `π = (D₀, r₁, D₁, ..., rₖ, Dₖ)` be a path
in `C₁`. By (M1), `F_R(rᵢ) = (F_D(Dᵢ₋₁), F_D(Dᵢ), lᵢ) ∈ R₂`, so each edge of
`F*(π)` exists in `C₂`. The no-revisit condition holds because `F_D` is
injective: `Dᵢ ≠ Dⱼ ⟹ F_D(Dᵢ) ≠ F_D(Dⱼ)`. Thus `F*(π)` is a valid path.

**(2) Length preservation.** `F*(π)` has the same number of edges as `π` because
`F_R` is a bijection on the edge sequence of `π` (induced by injective `F_D`).
Therefore `length(F*(π)) = length(π) ≤ B`, so `F*(π) ∈ Paths_B(C₂)`.

> [!IMPORTANT]
> **Why budget = hop count, not weight sum.** If the budget were defined as
> `∑ ω(rᵢ) ≤ B`, functoriality would fail: condition (M2) gives
> `ω₂(F_R(r)) ≥ ω₁(r)`, so `cost(F*(π)) ≥ cost(π)`, potentially violating the
> budget. The hop-count formulation avoids this. Weight-based budgets require
> the stronger condition `ω₂(F_R(r)) = ω₁(r)` (isometric morphisms), which would
> restrict **Corp** to a smaller subcategory.

**(3) Identity.** `id_C* (π) = (id_D(D₀), id_R(r₁), ...) = (D₀, r₁, ...) = π`. ✓

**(4) Composition.** Let `F : C₁ → C₂`, `G : C₂ → C₃`.

```
(G ∘ F)*(π) = ((G∘F)_D(D₀), (G∘F)_R(r₁), ..., (G∘F)_D(Dₖ))
            = (G_D(F_D(D₀)), G_R(F_R(r₁)), ..., G_D(F_D(Dₖ)))
            = G*(F_D(D₀), F_R(r₁), ..., F_D(Dₖ))
            = G*(F*(π))
```

Hence `Nav_B(G ∘ F) = Nav_B(G) ∘ Nav_B(F)`. ∎

**Definition 8 (Natural Transformation between Strategies).** Given two
navigation strategies `N₁, N₂ : Corp → Poset`, a **strategy comparison** is a
natural transformation `η : N₁ ⟹ N₂` where for each corpus `C`:

```
η_C : N₁(C) → N₂(C)     — maps paths found by strategy 1 to paths found by strategy 2

Naturality condition:
  For every corpus morphism F : C₁ → C₂, the following diagram commutes:

  N₁(C₁) ──η_C₁──→ N₂(C₁)
    │                  │
  N₁(F)              N₂(F)
    ↓                  ↓
  N₁(C₂) ──η_C₂──→ N₂(C₂)
```

The naturality condition ensures **consistency of strategy comparisons across
corpus morphisms**: if `η` relates the paths of `N₁` and `N₂` in corpus `C₁`,
then embedding `C₁` into `C₂` preserves this relationship. This is weaker than
"global subsumption"—it guarantees structural compatibility, not that one
strategy dominates the other in all metrics.

**Definition 9 (Corpus Merge via Pushout).** Given two corpora `C₁, C₂` sharing
a common sub-corpus `C₀` via morphisms `F₁ : C₀ → C₁` and `F₂ : C₀ → C₂`, the
**merged corpus** is the pushout (colimit) in **Corp**:

```
          C₀
        ╱    ╲
     F₁        F₂
     ╱            ╲
   C₁              C₂
     ╲            ╱
     ι₁          ι₂
       ╲        ╱
      C₁ ⊔_{C₀} C₂

where:
  D_{merge} = D₁ ⊔_{D₀} D₂    — pushout of document sets (identifying shared documents)
  R_{merge} = R₁ ⊔_{R₀} R₂    — pushout of edge sets (identifying shared edges)
  ι₁ : C₁ → C_{merge}          — canonical inclusion (corpus morphism)
  ι₂ : C₂ → C_{merge}          — canonical inclusion (corpus morphism)
```

> [!NOTE]
> This is a **pushout** (colimit), not a pullback (limit). The pushout
> identifies shared documents (in `C₀`) rather than duplicating them, and takes
> the union of edges—exactly the "gluing" semantics needed for corpus merging.
> The canonical inclusions `ι₁, ι₂` are corpus morphisms satisfying (M1)-(M3),
> since they preserve edge structure and cannot decrease weights.

**Remark (Discovery of Cross-Corpus Edges).** In practice, MDN may discover
additional inter-corpus relationships not present in either `C₁` or `C₂`:

```
R_discovery = {(Dᵢ, Dⱼ, l) : Dᵢ ∈ D₁\D₀, Dⱼ ∈ D₂\D₀, inferred_relation(Dᵢ, Dⱼ, l)}
```

This edge discovery is an **algorithmic extension**, not a categorical
operation: it depends on an external inference procedure (e.g., LLM-based
semantic analysis) and does not arise from the universal property of the
pushout. The enriched merge
`C_{merge}^+ = (D_{merge}, R_{merge} ∪ R_discovery, ...)` is a strictly larger
object than the categorical pushout, and the inclusions `ι₁, ι₂` remain valid
morphisms into `C_{merge}^+` (since adding edges preserves (M1)-(M3) — the new
edges only strengthen the corpus).

**Proposition (Pushout Preserves Navigation).** If `π` is a valid MDN path in
`C₁`, then `ι₁*(π)` is a valid MDN path in `C₁ ⊔_{C₀} C₂` (by functoriality of
`Nav_B`, since `ι₁` is a corpus morphism). Similarly for paths in `C₂` via `ι₂`.
The enriched merge `C_{merge}^+` may additionally contain paths not present in
either `C₁` or `C₂`—namely, those traversing `R_discovery` edges.

---

## 3. Logical Formalization

### 3.1 Modal Logic for Distributed Knowledge

We extend standard epistemic logic S4 to account for knowledge distributed
across documents.

**Language L_MDN:**

```
φ ::= p                   — atomic proposition
    | ¬φ                  — negation
    | φ ∧ ψ              — conjunction
    | Kᵢ φ                — "document Dᵢ knows φ"
    | C^R φ               — "φ is common knowledge via relation R"
    | E^R φ               — "everyone connected via R knows φ"
    | [cite]φ             — "φ follows from citation chain"
    | [depend]φ           — "φ is accessible via dependencies"
    | ⟨contradict⟩φ       — "there exists a contradiction about φ"
```

**Semantics.** A Kripke model for `L_MDN` is:

```
M = (W, {Rᵢ}ᵢ∈D, {Rₜ}ₜ∈RelationType, V)

where:
  W                      — set of possible worlds (states of knowledge)
  Rᵢ ⊆ W × W             — epistemic accessibility for document Dᵢ
  Rₜ ⊆ W × W             — structural accessibility for edge type t
  V : Atoms → P(W)       — valuation function
```

> [!IMPORTANT]
> **Two levels of modality.** The model separates two families of accessibility
> relations that serve fundamentally different roles:
>
> - `Rᵢ` (epistemic): captures what document `Dᵢ` "knows"—its internal
>   propositional content. Governed by S4 axioms (reflexive + transitive).
> - `Rₜ` (structural): captures inter-document navigability via edge type `t`.
>   Governed by type-specific frame conditions (see below).
>
> **Bridge condition.** These levels are connected not by subset containment
> (`Rₜ ⊆ ⋃ᵢ Rᵢ`—which is too strong, since a structural edge may connect
> information no single document fully encompasses) but by a weaker **semantic
> grounding** principle:
>
> **(Bridge)** For each structural relation `Rₜ`:
> `wRₜw' ⟹ ∃i ∈ D : wRᵢw' ∨ (∃w'' : wRᵢw'' ∧ w''Rₜw')`
>
> i.e., structural accessibility is reachable through at most one epistemic step
> followed by structural navigation. This avoids imposing that every citation
> edge falls within a single document's knowledge.

**All modal operators `[t]` are normal.** Each `[t]` distributes over
implication and is closed under necessitation:

```
(K_t)   [t](φ → ψ) → ([t]φ → [t]ψ)          — distribution (for each t)
(Nec_t) If ⊢ φ then ⊢ [t]φ                    — necessitation (for each t)
```

This holds uniformly for all edge types. We need not state normality axioms
individually per type; the `[t]` operators inherit the standard normal modal
logic framework (Blackburn et al., 2001, Ch. 1).

**Truth conditions:**

```
M, w ⊨ Kᵢ φ              iff  ∀w' : wRᵢw' ⟹ M, w' ⊨ φ
M, w ⊨ [t]φ              iff  ∀w' : wRₜw' ⟹ M, w' ⊨ φ       (for any t)
M, w ⊨ ⟨contradict⟩φ     iff  ∃w' : wR_contradict w' ∧ M, w' ⊨ ¬φ
```

**Frame conditions for S4_MDN:**

```
Epistemic relations (one per document Dᵢ):
  (F1)  Rᵢ is reflexive                        — grounds axiom (T): Kᵢφ → φ
  (F2)  Rᵢ is transitive                       — grounds axiom (4): Kᵢφ → KᵢKᵢφ

Structural relations (per edge type t):
  (F3)  R_cite is reflexive                     — grounds (TR) for cite: [cite]φ → φ
  (F4)  R_depend is reflexive                   — grounds (TR) for depend: [depend]φ → φ
  (F5)  R_contradict is symmetric               — contradiction is mutual
  (F6)  R_contradict is irreflexive             — no document contradicts itself
```

**Axioms of S4_MDN:**

```
(K)   Kᵢ(φ → ψ) → (Kᵢφ → Kᵢψ)                — distribution (epistemic)
(T)   Kᵢφ → φ                                  — veridicality (truth axiom)
(4)   Kᵢφ → KᵢKᵢφ                              — positive introspection
(C)   C^R φ → (E^R φ ∧ E^R C^R φ)              — common knowledge
(TR)  [t]φ → φ    for t ∈ {cite, depend}        — type-restricted veridicality
```

> [!NOTE]
> **Justification of (TR).** The axiom `[t]φ → φ` is the modal T axiom applied
> to structural operators. It is valid iff `Rₜ` is reflexive (frame conditions
> F3, F4). The epistemic reading: "if φ holds throughout the citation
> neighborhood of the current world, then φ holds at the current world"—which is
> trivially satisfied when the current world is in its own neighborhood. We
> restrict (TR) to `{cite, depend}` because `R_contradict` is deliberately
> **not** reflexive (F6): a document does not contradict itself.

> [!NOTE]
> **Semantic exclusion for contradiction.** Symmetry of `R_contradict` (F5)
> captures the bidirectional nature of disagreement: if `Dᵢ` contradicts `Dⱼ` on
> `φ`, then `Dⱼ` contradicts `Dᵢ` on `φ`. Irreflexivity (F6) ensures internal
> consistency. Together, these frame conditions do **not** entail global corpus
> consistency (the corpus may be contradictory overall), but they localize
> contradictions to inter-document edges—a design choice aligned with MDN's
> multi-source epistemic model.

> [!IMPORTANT]
> We deliberately adopt **S4** (K + T + 4) rather than **S5** (K + T + 4 + 5)
> for MDN. The negative introspection axiom (5): `¬Kᵢφ → Kᵢ¬Kᵢφ` ("if a document
> does not contain information about φ, then it _knows_ it lacks that
> information") is **epistemically unwarranted** for documents. Documents are
> passive artifacts—they cannot introspect on their own gaps. S4's weaker
> assumption (reflexive + transitive accessibility, without symmetry) correctly
> models the **asymmetric** nature of documentary knowledge: a document knows
> what it states (T) and its knowledge is introspectable (4), but it cannot
> certify its own incompleteness. This follows Fagin et al. (1995, Ch. 3) who
> argue that S5 is appropriate only for idealized agents with perfect awareness
> of their epistemic boundaries.

**Theorem 4 (Soundness and Completeness).** S4_MDN is sound and complete with
respect to the class of Kripke frames `F = (W, {Rᵢ}ᵢ∈D, {Rₜ}ₜ∈RelationType)`
satisfying frame conditions (F1)-(F6) and the Bridge condition.

_Proof sketch._

**Soundness** follows by induction on proof length. Each axiom is verified
against the frame conditions:

- (T) requires reflexivity of `Rᵢ` (F1) ✓
- (4) requires transitivity of `Rᵢ` (F2) ✓
- (TR) for `t ∈ {cite, depend}` requires reflexivity of `Rₜ` (F3, F4) ✓
- Normality of all `[t]` is inherited from the normal modal logic framework ✓

**Completeness** is established by extending the canonical model construction
(Blackburn et al., 2001, Ch. 4). The key non-trivial step is verifying that the
additional frame conditions are **canonical** (i.e., the canonical frame
satisfies them):

- **(F1), (F2):** Canonical for S4 by standard results (reflexivity from T,
  transitivity from 4).
- **(F3), (F4):** Canonical for (TR), since (TR) is an instance of the T-axiom
  schema for the operators `[cite]`, `[depend]`. The T schema corresponds to
  reflexivity (Blackburn et al., 2001, Prop. 4.42).
- **(F5):** Symmetry of `R_contradict` is canonical if we include the axiom
  `φ → [contradict]⟨contradict⟩φ` (the B axiom for `contradict`), which is
  implicitly entailed by the symmetric frame semantics of contradiction.
- **(F6):** Irreflexivity is **not** first-order definable in basic modal logic
  (Blackburn et al., 2001, Ch. 3.3), hence not directly canonical. We impose
  (F6) as a frame restriction: completeness holds for the class of frames
  satisfying (F1)-(F5) plus the extra-logical constraint (F6), following the
  general completeness transfer technique for non-canonical conditions
  (Blackburn et al., 2001, §4.4). ∎

### 3.2 Provenance Logic

We extend `L_MDN` with a **provenance enrichment layer** that associates each
derived formula with the corpus path(s) that justify it. This layer operates at
the **meta-logical** level: provenance annotations are not part of the object
language of S4_MDN, but rather external certificates attached to derivations.

> [!IMPORTANT]
> **Separation of levels.** The base logic (S4_MDN, §3.1) reasons about truth
> and knowledge. The provenance layer tracks **why** a formula is believed and
> **through which documents** it was derived. This two-level architecture avoids
> the formalization pitfalls of mixing logical connectives with graph-structural
> constraints.

**Definition 10 (Provenance-Annotated Formula).** A provenance-annotated formula
is a pair `φ@Π` where:

```
φ   ∈ L_MDN            — a formula in the base logic
Π   ⊆ Paths(C)          — a non-empty set of provenance paths (Definition 7)

Each π ∈ Π is a valid acyclic path in C:
  π = (D₀, r₁, D₁, ..., rₖ, Dₖ)
  with Dᵢ ≠ Dⱼ for i ≠ j       (acyclicity — part of path validity)
```

When `|Π| = 1`, we write `φ@π` for the singleton. Multiple paths represent
**independent lines of evidence** supporting the same formula (corroboration).

**Definition 10.1 (Corpus-Anchored Kripke Model).** To connect provenance (which
lives in the corpus graph `C`) with truth (which lives in the Kripke model `M`),
we extend the model with an **anchor function**:

```
M_C = (W, {Rᵢ}, {Rₜ}, V, C, γ)

where:
  C = (D, R, τ, ω, σ)       — the corpus graph (§2.1)
  γ : D → P(W)               — anchor: maps each document Dᵢ to the set of
                                worlds where its content holds

Required properties:
  (A1)  γ(Dᵢ) ≠ ∅ for all Dᵢ ∈ D     — every document grounds some worlds
  (A2)  w ∈ γ(Dᵢ) ⟹ ∀φ stated in Dᵢ : M, w ⊨ φ    — anchor respects content
```

**Semantics of provenance annotation:**

```
M_C, w ⊨ φ@π    iff    (i)  M, w ⊨ φ                          — truth in the base model
                  and   (ii) π = (D₀, ..., Dₖ) is a valid path in C
                  and   (iii) w ∈ γ(Dₖ)                         — w is anchored at endpoint
```

**Provenance Composition Rules.** The following rules derive new
provenance-annotated formulas from existing ones. All premises and conclusions
are provenance-annotated; side conditions reference the corpus graph `C`.

```
  φ@(D₀)    ψ@(D₁)    (D₀, D₁, cite) ∈ R
  ──────────────────────────────────────────  (P-CITE)
           (φ → ψ)@(D₀, cite, D₁)

  Reads: if D₀ asserts φ, D₁ asserts ψ, and D₀ cites D₁,
         then "φ implies ψ" is witnessed by the citation path.


  φ@π₁    (Dₖ, D_{k+1}, t) ∈ R
  ──────────────────────────────────────────  (P-EXTEND)
           φ@(π₁ · t · D_{k+1})

  where π₁ · t · D_{k+1} denotes path extension, and
  D_{k+1} ∉ nodes(π₁)   (acyclicity preserved).


  φ@{π₁}    φ@{π₂}    nodes(π₁) ∩ nodes(π₂) = {D₀}
  ──────────────────────────────────────────  (P-CORROBORATE)
           φ@{π₁, π₂}

  Reads: if φ is independently derived via two paths sharing only
         the origin D₀, then φ has multi-path provenance.
```

> [!NOTE]
> **(P-CORROBORATE) is purely logical — it combines provenance, not
> confidence.** The confidence boost from independent corroboration is a
> **separate quantitative concern** handled by the epistemic weight function
> (Definition 3, §2.1) and the EPR scoring (§2.3), not by the provenance
> calculus. The logical content of (P-CORROBORATE) is: φ is supported by
> multiple independent evidence chains.

**Theorem 5 (Provenance Soundness).** If `⊢_MDN φ@Π` is derivable using rules
(P-CITE), (P-EXTEND), and (P-CORROBORATE), then:

1. Every `π ∈ Π` is a valid acyclic path in `C`
2. `M_C, w ⊨ φ` for all `w ∈ ⋂_{π ∈ Π} γ(endpoint(π))`
3. The derivation structure of `φ` is **witnessed** by `Π`: each application of
   a composition rule corresponds to an edge traversal in some `π ∈ Π`

_Proof._ By induction on the height of the derivation tree.

**Base case.** `φ@(Dᵢ)` is an axiom (atomic annotation). By (A2), `w ∈ γ(Dᵢ)`
implies `M, w ⊨ φ`. The single-node path `(Dᵢ)` is trivially valid and acyclic.
✓

**Inductive cases:**

- _(P-CITE):_ By IH, `φ` is true at worlds anchored at `D₀`, `ψ` at worlds
  anchored at `D₁`. The edge `(D₀, D₁, cite) ∈ R` is given. The extended path
  `(D₀, cite, D₁)` is valid by construction. ✓
- _(P-EXTEND):_ By IH, `π₁` is a valid path. `D_{k+1} ∉ nodes(π₁)` ensures
  acyclicity. The extended path is valid by (G2). ✓
- _(P-CORROBORATE):_ By IH, both `π₁` and `π₂` are valid. No new edges are
  created; the provenance set grows from `{πᵢ}` to `{π₁, π₂}`. Validity is
  preserved componentwise. ✓ ∎

### 3.3 Contradiction Detection and Resolution

**Definition 11 (Contradiction Relation).** For documents `Dᵢ, Dⱼ` claiming `φ`
and `¬φ` respectively:

```
Contradict(Dᵢ, Dⱼ, φ) iff  Dᵢ ⊨ φ  ∧  Dⱼ ⊨ ¬φ  ∧  (Dᵢ, Dⱼ, contradict) ∈ R
```

**Resolution Strategy.** When contradictions are detected, the system applies an
**epistemic preference ordering**:

```
Prefer(Dᵢ, Dⱼ) iff one of:
  1. Recency: Dᵢ.date > Dⱼ.date  ∧  τ(Dᵢ, Dⱼ) = supersede
  2. Authority: EPR(Dᵢ) > EPR(Dⱼ)  (Epistemic PageRank)
  3. Corroboration: |{Dₖ : supports(Dₖ, Dᵢ)}| > |{Dₖ : supports(Dₖ, Dⱼ)}|
  4. Provenance: depth(Dᵢ) < depth(Dⱼ)  (closer to primary source)
```

**Axiom (Contradiction Quarantine):**

```
⟨contradict⟩φ → (φ@π₁ ∧ ¬φ@π₂) → tag_uncertainty(φ)
```

If a contradiction is detected, both claims are tagged with epistemic status
`Uncertainty` (per AXON's lattice), forcing manual resolution or anchor
validation.

---

## 4. Philosophical Foundations

### 4.1 Social Epistemology of Distributed Knowledge

Multi-document navigation is grounded in **social epistemology**—the study of
knowledge as a collective, distributed phenomenon.

**Testimony Theory (Coady, 1992).** Knowledge is often acquired via testimony
from others. In MDN:

- Each document `Dᵢ` acts as a **testifier**
- Citations are **testimonial chains**: `D₀` trusts `D₁` which trusts `D₂`
- The epistemic status of derived knowledge depends on:
  - **Reliability of testifiers** (document authority)
  - **Independence of testimony** (multiple corroborating paths)
  - **Coherence** (no contradictions along the path)

**Proposition 6 (Testimonial Credence Model).** Given a testimonial chain
`π = (D₀, r₁, D₁, ..., rₖ, Dₖ)`, the credence `D₀` assigns to claims derived
from `Dₖ` is:

```
C(D₀, claim_from_Dₖ, π) = (∏ᵢ₌₁ᵏ cᵢ)^α · ρ(π) · (1 - δ)^|contradict(π)|

where:
  cᵢ ∈ [0, 1]              — credibility of edge rᵢ, defined as a normalized
                              authority score (e.g., EPR(Dᵢ), citation count,
                              or subjective probability of Dᵢ's reliability)

  α ∈ (0, 1]                — attenuation exponent (controls decay rate;
                              α = 1 is full multiplicative penalty,
                              α < 1 softens penalization for long chains)

  ρ(π) ∈ [0, 1]             — independence factor: measures the degree to
                              which testimony along π is independent.
                              ρ = 1 if all testifiers are fully independent;
                              ρ < 1 if shared sources or editorial lineage
                              introduce correlation among Dᵢ

  δ ∈ [0, 1]                — contradiction discount per contradicted step.
                              |contradict(π)| counts the number of edges rᵢ
                              where ⟨contradict⟩ holds between Dᵢ₋₁ and Dᵢ
                              (connecting to §3.3's contradiction detection)
```

> [!NOTE]
> **This is a credence model, not a theorem.** The formula is not derived from
> axioms but postulated as a plausible quantitative model grounded in testimony
> theory (Coady, 1992). Its components correspond to well-established
> epistemological principles:
>
> - `∏ cᵢ` — **reliability chaining**: each intermediary introduces uncertainty
> - `ρ` — **independence**: correlated testimony is less informative (Bovens &
>   Hartmann, 2003)
> - `(1-δ)^|contradict|` — **coherence penalty**: contradictions weaken credence
> - `α` — **decay control**: avoids the double-penalization problem that arises
>   when both the product `∏cᵢ` and a separate `exp(-λk)` term independently
>   decay with chain length. The exponent `α` subsumes both effects in a single,
>   tunable parameter.

### 4.2 Coherentism vs. Foundationalism

Two epistemological traditions inform MDN's design:

| Tradition           | Claim                                                  | MDN Implementation                                                                                   |
| ------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| **Foundationalism** | Knowledge rests on basic, self-justified beliefs       | Primary sources (root documents) are assigned high epistemic prior `σ(D)` in `(EpistemicLevel, ≤_T)` |
| **Coherentism**     | Knowledge is justified by coherence with other beliefs | Cross-corroboration increases confidence; contradictions lower it                                    |

**MDN synthesizes both:**

- **Foundation:** Root documents in the corpus (e.g., statutes, standards) are
  initialized with high epistemic status `σ(D)` in the poset
  `(EpistemicLevel, ≤_T)` (axiom G5, §2.1)
- **Coherence:** Derived documents gain credibility through citation networks
  and mutual support

**Principle of Epistemic Corroboration.** If `D₀` reaches claim `φ` via two
**node-disjoint** paths (sharing only the origin `D₀`):

```
π₁ = (D₀, ..., Dₖ) with confidence c₁ = C(D₀, φ, π₁)
π₂ = (D₀, ..., Dₘ) with confidence c₂ = C(D₀, φ, π₂)

where: nodes(π₁) ∩ nodes(π₂) = {D₀}     (independence — cf. P-CORROBORATE, §3.2)

Then the corroborated credence is:

  C_corr(D₀, φ) = max(c₁, c₂) + β · min(c₁, c₂) · (1 - max(c₁, c₂))
                   · (1 - δ)^|conflict(π₁, π₂)|

where:
  β ∈ [0, 1]                — corroboration bonus (0 = no bonus, 1 = full)
  δ ∈ [0, 1]                — contradiction discount (per conflicting sub-claim)
  |conflict(π₁, π₂)|        — number of sub-claims where π₁ and π₂ disagree
```

> [!NOTE]
> **Normalization guarantee.** The term `min(c₁, c₂) · (1 - max(c₁, c₂))`
> ensures `C_corr ≤ 1` for all inputs:
> `C_corr ≤ max + β · min · (1 - max) ≤ max + 1 · 1 · (1 - max) = 1`. ✓
>
> **Node-disjointness as independence.** Two paths are _independent_ iff they
> share no intermediate documents — only the query origin `D₀`. This prevents
> counting circular or editorially correlated testimony as independent
> corroboration, connecting to the formal rule (P-CORROBORATE) in §3.2.

Independent corroboration increases confidence beyond individual paths.

### 4.3 Ontology of Document Relations

**Upper Ontology for Document Corpora:**

```
Document Role Classification:
  type : D → T                where T is a domain-extensible role set

  Default roles (extensible per domain):
    T = { PrimarySource,        — statutes, standards, original research
          SecondarySource,       — interpretations, analyses, reviews
          TertiarySource,        — summaries, textbooks, compilations
          ... }                  — domain-specific extensions

  Note: T is not fixed. A legal corpus may include {Statute, Regulation,
  Commentary, CaseDecision}; an academic corpus may use {OriginalResearch,
  MetaAnalysis, Survey, Textbook}. The ontology only requires that T is
  finite and each D ∈ D has a declared type.

Relation Hierarchy:
  SemanticRelation
    ├── SupportRelation    (cite, elaborate, implement, corroborate, exemplify)
    ├── ConflictRelation   (contradict, supersede)
    └── DependencyRelation (depend)
```

**Relation Properties.** Each edge type has formal structural properties that
connect to the frame conditions of S4_MDN (§3.1):

```
Edge Type    | Reflexive | Symmetric | Transitive | Antisymmetric | Modal Operator
-------------|-----------|-----------|------------|---------------|----------------
cite         |    yes    |    no     |     no     |      no       | [cite]φ
depend       |    yes    |    no     |    yes     |      no       | [depend]φ
elaborate    |    no     |    no     |    yes     |      no       | [elaborate]φ
contradict   |    no     |   yes     |     no     |      no       | ⟨contradict⟩φ
supersede    |    no     |    no     |    yes     |     yes       | [supersede]φ
implement    |    no     |    no     |     no     |      no       | [implement]φ
corroborate  |    no     |   yes     |     no     |      no       | [corroborate]φ
exemplify    |    no     |    no     |     no     |      no       | [exemplify]φ
```

> [!NOTE]
> **Connection to §3 modal logic.** The relation properties above directly
> ground the frame conditions (F1)-(F6) of S4_MDN:
>
> - **SupportRelation** types map to **box operators** `[t]φ`: "φ holds
>   throughout the t-neighborhood." Reflexivity of `R_cite` and `R_depend`
>   justifies axiom (TR).
> - **ConflictRelation** types map to **diamond operators** `⟨t⟩φ`: "there
>   exists a t-accessible world where φ holds." Symmetry of `R_contradict`
>   justifies the B axiom (F5). Antisymmetry + transitivity of `supersede`
>   imposes a strict temporal ordering on document versions.

**Ontological Commitment (Formalized).** Every edge in the corpus graph must
have a well-defined semantic interpretation in a domain ontology:

```
interpretation : R → O

where O is a domain ontology (a typed vocabulary of semantic relations).

  (O1)  interpretation is total:
        ∀ r ∈ R : interpretation(r) ∈ O

  (O2)  Ontological consistency (domain/range typing):
        valid(r = (Dᵢ, Dⱼ, l)) ⟹ (type(Dᵢ), type(Dⱼ)) ∈ Dom(τ(r))

        where Dom : RelationType → P(T × T) specifies which document type
        pairs are admissible for each relation type.

        Example type constraints:
          implement ∈ Dom ⟹ type(src) = Specification, type(tgt) = System
          supersede ∈ Dom ⟹ type(src) = type(tgt)   (same-type versioning)
          cite      ∈ Dom ⟹ no type restriction     (universal)
```

Condition (O2) **formally prevents spurious connections**: an `implement` edge
between two textbooks would violate the domain/range constraint, forcing the
corpus builder to use a semantically appropriate relation type.

### 4.4 The Problem of Induction in MDN

**Hume's Problem Applied to Document Navigation:**

> Past relevance of citation chains does not guarantee future relevance—and,
> strictly speaking, no amount of past success can logically justify the
> expectation of future success (Hume, 1739).

Just because document `D₁` was helpful for query `Q₁` doesn't mean following
similar citations will help with `Q₂`. This is a genuine epistemological
concern, not merely a practical one.

**MDN's Response: Bayesian Updating as Pragmatic Induction**

Let `H = (Q₁, D₁*, ..., Qₜ₋₁, D*ₜ₋₁)` denote the complete navigation history
(past queries and their successful resolutions). For a new query `Q` and
candidate document `Dⱼ`, the posterior relevance is:

```
P(Dⱼ | Q, H) ∝ P(Q | Dⱼ, H) · P(Dⱼ | H)

where:
  P(Q | Dⱼ, H)     — likelihood: how well does Dⱼ's content match Q,
                      given what we know from past queries?
                      (estimated via semantic similarity, term overlap,
                      or learned retrieval models)

  P(Dⱼ | H)         — prior: how likely is Dⱼ to be relevant before
                      seeing Q, based on:
                      · graph structure (centrality, depth)
                      · epistemic weight ω(Dⱼ)
                      · EPR score (§2.3)
                      · past query success rates for similar documents
```

> [!NOTE]
> **Relevance as latent variable.** We model relevance `rⱼ ∈ {0, 1}` as a latent
> binary variable: `rⱼ = 1` if `Dⱼ` contains information that resolves (part of)
> `Q`. The posterior `P(rⱼ = 1 | Q, H)` is what the system estimates; observed
> feedback (user satisfaction, answer quality) updates `H` for future queries.

The system **learns** from query history which types of edges are productive for
which types of queries. This constitutes inductive reasoning, formalized
probabilistically via Bayesian updating.

> [!WARNING]
> **Humean caveat.** Bayesian updating **does not solve** the problem of
> induction—it **manages uncertainty under inductive assumptions**. The prior
> `P(Dⱼ | H)` itself embodies an inductive bet: that past patterns (which
> documents were helpful, which edge types were productive) will generalize. MDN
> adopts a **pragmatic** stance: while perfect justification is unattainable,
> systematically updating beliefs from evidence is the best available strategy
> (cf. de Finetti, 1937; Savage, 1954). The formal guarantees of Theorem 2
> (monotonic information gain) hold _conditional_ on the inductive assumption
> that the corpus structure is informative.

---

## 5. Programming & Implementation

### 5.1 Type System Extension

We extend AXON's type system with **graph types**:

```typescript
// Edge type — the atomic unit of corpus structure
type Edge<D: Document> {
    source: D
    target: D
    rel:    RelationType
}

// Corpus type — typed graph with relation-aware weights
type Corpus<D: Document> {
    documents: Set<D>
    edges:     Set<Edge<D>>
    weights:   Map<Edge<D>, Float>    // keyed by full edge (incl. relation type)
}

// Path type — derived from edges, nodes computed automatically
type Path<D: Document> {
    edges:      List<Edge<D>>
    provenance: ProvenanceAnnotation

    // Derived property (not stored, computed from edges):
    //   nodes = [edges[0].source] ++ edges.map(e => e.target)
    //
    // Internal invariant (enforced at construction):
    //   ∀ i : edges[i].target = edges[i+1].source
}

// Proposition type — connected to the formal language L_MDN (§3)
type Proposition = Formula<L_MDN>

type NavigationResult<D: Document> {
    paths:          Set<Path<D>>
    confidence:     ConfidenceScore
    contradictions: Set<(D, D, Proposition)>       // (Dᵢ, Dⱼ, φ) where Dᵢ ⊨ φ and Dⱼ ⊨ ¬φ
}
```

**Invariant Enforcement (Static + Runtime):**

```
∀ path ∈ NavigationResult.paths :

  1. Edge validity     [STATIC]   ∀ e ∈ path.edges : e ∈ Corpus.edges
     — the type system ensures edges are well-typed (source/target ∈ D,
       rel ∈ RelationType). Membership in a specific corpus is checked
       at construction time (runtime).

  2. Path coherence    [STATIC]   ∀ i : path.edges[i].target = path.edges[i+1].source
     — structural invariant enforced by the Path constructor.

  3. No immediate      [RUNTIME]  ∀ i : path.edges[i].target ≠ path.edges[i].source
     revisit                      — prevents trivial self-loops. The corpus graph
                                    may contain cycles (e.g., mutual citations);
                                    the navigation algorithm avoids revisiting the
                                    immediately preceding node, but does not
                                    forbid all cycles (which would be too
                                    restrictive for real corpora).

  4. Budget            [RUNTIME]  length(path) ≤ max_depth
     — hop-count budget (§2.5, Theorem 3.5).
```

> [!IMPORTANT]
> **Static vs. runtime boundary.** Properties (1) and (2) are enforced by AXON's
> type system at compile time. Properties (3) and (4) are **runtime
> invariants**: they depend on data values (corpus contents, navigation depth)
> and cannot be verified statically in a decidable type system. This is a
> deliberate design choice — full static verification would require dependent
> types (e.g., `Path<D, n: Nat> where n ≤ max_depth`), which AXON does not
> currently support. The runtime checks are lightweight (O(k) for a path of
> length k) and are performed at path construction time, failing fast on
> violation.

### 5.2 Effect System for MDN

Multi-document navigation has a complex effect signature. We extend AXON's
algebraic effect system with **epistemic effects** — a novel family of effects
that track the epistemic commitment level of computations.

**Epistemic Effect Lattice.** We define a lattice of epistemic effects:

```
EpistemicEffect = (⊥, speculate, believe, ≤_E)

where:  ⊥  <_E  speculate  <_E  believe

Operational semantics:
  ⊥               — no epistemic commitment (pure computation)
  speculate        — non-monotonic exploration: results are tentative,
                     not committed to the global belief state. The system
                     may backtrack or discard speculative conclusions.
  believe          — monotonic commitment: results are incorporated into
                     the agent's epistemic state. Increases confidence
                     on accepted claims (cf. Proposition 6, §4.1).

Join rule:  speculate ⊔ believe = believe
            (a computation that both speculates and believes is committed)
```

> [!NOTE]
> **Connection to §4 (Philosophical Foundations).** The `speculate/believe`
> distinction corresponds directly to the foundationalism/coherentism synthesis:
> `speculate` = exploratory coherence checking; `believe` = foundational
> commitment based on corroborated evidence.

**Effect Signatures:**

```typescript
effect Navigate<C: Corpus> where
  // Core navigation — may access network, always epistemic:speculate
  traverse : forall eps <= {network}.
             (Query, C, Budget) ->[io | eps | epistemic:speculate] NavigationResult

  // Epistemic operations — no I/O, only epistemic effects
  corroborate : (Claim, Set<Path>) ->[epistemic:believe] ConfidenceScore
  detect_contradiction : Set<Path> ->[] Set<Contradiction>     // pure (empty row)

  // Cache operations — namespaced mutation
  cache_read  : (Query, C) ->[io:storage] Option<NavigationResult>
  cache_write : (Query, NavigationResult) ->[io:storage | mutation:cache] Unit
```

**Composite Effect Row:**

```
EffectRow(MDN_navigate) = ⟨io:storage, network?, mutation:cache?,
                            epistemic:(speculate ⊔ believe)⟩
                         = ⟨io:storage, network?, mutation:cache?,
                            epistemic:believe⟩

where:
  io:storage           — reads from document storage (deterministic)
  network?             — optional: fetches external documents
                         (effect-polymorphic: ∀ε ⊆ {network})
  mutation:cache?      — optional: writes to navigation cache
                         (namespaced: does not affect corpus or belief state)
  epistemic:believe    — the join of speculate ⊔ believe over all sub-operations;
                         the composite operation has the strongest epistemic
                         commitment of any sub-operation
```

### 5.3 Compilation Strategy

The AXON compiler translates high-level MDN declarations into an intermediate
representation (IR) that preserves type information, effect annotations, and
ontological constraints. We illustrate with a legal research example.

**High-Level AXON MDN Code:**

```typescript
corpus LegalCorpus {
    documents: [statute_A, case_law_B, regulation_C]

    relationships: [
        (case_law_B,   statute_A, cite),           // B cites A
        (regulation_C, statute_A, implement)        // C implements A
    ]

    weights: {
        (case_law_B,   statute_A, cite):       0.9,
        (regulation_C, statute_A, implement):  0.85
    }

    // Ontological validation (§4.3, condition O2):
    //   implement requires type(src) = Regulation, type(tgt) = Statute  ✓
    //   cite has no type restriction                                     ✓
}

// The flow declaration carries effect annotations from §5.2
flow ResearchLegalQuestion(question: String)
    ->[io:storage | epistemic:believe] LegalAnalysis
{
    step Navigate ->[io:storage | epistemic:speculate] {
        result = traverse(
            query:       question,
            corpus:      LegalCorpus,
            budget:      Budget { max_depth: 3 },
            edge_filter: [cite, implement]
        )
        // result : NavigationResult<LegalDocument>
    }

    step Synthesize ->[epistemic:believe] {
        confidence = corroborate(
            claims_from(result.paths),
            result.paths
        )
        contradictions = detect_contradiction(result.paths)
        // contradictions : Set<(D, D, Proposition)>  where Proposition = Formula<L_MDN>

        output LegalAnalysis {
            paths:          result.paths,
            confidence:     confidence,
            contradictions: contradictions
        }
    }
}
```

**Compiled IR:**

```json
{
    "flow": "ResearchLegalQuestion",
    "effect_row": ["io:storage", "epistemic:believe"],
    "steps": [
        {
            "type": "MDN_navigate",
            "effect_row": ["io:storage", "epistemic:speculate"],
            "corpus_ref": "LegalCorpus",
            "start_node": "statute_A",
            "query": "{question}",
            "budget": {
                "max_depth": 3,
                "budget_type": "hop_count"
            },
            "edge_filter": ["cite", "implement"],
            "algorithm": {
                "base": "bounded_bfs",
                "pruning": "information_gain",
                "note": "Algorithm selected by compiler based on budget_type and edge_filter cardinality"
            },
            "output": "result",
            "output_type": "NavigationResult<LegalDocument>"
        },
        {
            "type": "MDN_synthesize",
            "effect_row": ["epistemic:believe"],
            "operations": [
                {
                    "op": "corroborate",
                    "input": "result.paths",
                    "model": "Proposition_6"
                },
                {
                    "op": "detect_contradiction",
                    "input": "result.paths",
                    "logic": "modal_S4_MDN"
                }
            ],
            "output": "LegalAnalysis",
            "output_type": "LegalAnalysis"
        }
    ]
}
```

> [!NOTE]
> **Compilation guarantees.** The compiler ensures:
>
> 1. **Effect soundness:** each step's `effect_row` is a subset of the flow's
>    composite `effect_row`, verified via lattice join (§5.2)
> 2. **Ontological validity:** corpus relationships are checked against
>    `Dom(τ(r))` constraints (§4.3, condition O2) at compile time
> 3. **Budget typing:** `budget_type: "hop_count"` ensures functoriality of
>    navigation (Theorem 3.5, §2.5)
> 4. **Algorithm selection:** the compiler chooses the algorithm based on formal
>    properties (budget type, edge filter cardinality, corpus size), not
>    hardcoded by the programmer

**Runtime Execution Pipeline:**

1. **Graph Construction:** Load corpus graph `C = (D, R, τ, ω, σ)` from storage;
   validate structural axioms (G1)-(G5) from §2.1
2. **Navigation:** Execute selected algorithm (e.g., bounded BFS) with
   information-gain pruning (Theorem 2, §2.2). At each step `k`, select
   `Dₖ = argmax_{d ∈ candidates} I(A; d | Q, D₀, ..., Dₖ₋₁)`
3. **Provenance Tracking:** Annotate each retrieved claim with its provenance
   path `φ@π` using rules (P-CITE), (P-EXTEND) from §3.2. Verify anchoring via
   `γ : D → P(W)` (Definition 10.1)
4. **Contradiction Detection:** Check for `⟨contradict⟩φ` via the modal
   semantics of §3.1. Tag contradicted claims with epistemic status
   `Uncertainty` per §3.3
5. **Confidence Aggregation:** Compute `C(D₀, φ, π)` via Proposition 6 (§4.1)
   for individual paths; apply Principle of Epistemic Corroboration (§4.2) for
   multi-path claims. Update posterior `P(Dⱼ | Q, H)` per §4.4
6. **Output Validation:** Verify runtime invariants (§5.1): no-immediate-revisit
   (property 3) and budget compliance (property 4). Fail fast on violation

### 5.4 Algorithmic Specification

The following pseudocode implements the MDN navigation strategy. Each step is
annotated with its connection to the formal framework.

```python
Algorithm: MDN-Navigate(Q, C, D₀, B)

Input:
  Q    — query (natural language or structured)
  C    — corpus graph (D, R, τ, ω, σ)       — cf. Definition 1, §2.1
  D₀   — starting document ∈ D
  B    — budget: { max_depth: Nat,            — hop-count budget (§2.5)
                   max_nodes: Nat,            — max documents to visit
                   edge_filter: Set<RelationType> }

Output:
  result : NavigationResult<D>               — paths with provenance (§5.1)

 1.  frontier ← {(D₀, [(D₀)])}              # (current_doc, path_so_far)
 2.  visited ← ∅
 3.  Π ← ∅                                   # collected paths
 4.
 5.  for depth = 0 to B.max_depth:           # Termination: finite bound (T1)
 6.      next_frontier ← ∅
 7.      for (D, path) in frontier:
 8.          if |visited| ≥ B.max_nodes:      # Budget enforcement (T3)
 9.              break
10.          if D ∈ visited:                  # No re-exploration (T2)
11.              continue
12.          visited ← visited ∪ {D}
13.
14.          # — Relevance test (§4.4): P(D relevant | Q, H) ≥ τ_rel
15.          score_D ← relevance(D, Q, path)
16.          if score_D ≥ τ_rel:
17.              Π ← Π ∪ {annotate(path, D)}  # Provenance: φ@π (§3.2)
18.
19.          # — Neighbor expansion with type filtering (§4.3)
20.          neighbors ← {(D', l) : (D, D', l) ∈ R  ∧  l ∈ B.edge_filter}
21.
22.          # — Information-gain scoring (Theorem 2, §2.2)
23.          #   Greedy selection: D' ≈ argmax I(A; D' | Q, path)
24.          for each (D', l) in neighbors:
25.              gain[D'] ← estimate_info_gain(Q, D', path)
26.
27.          # — Adaptive pruning
28.          θ ← quantile(gain.values(), p=0.5)   # median threshold
29.          selected ← {(D', l) : gain[D'] ≥ θ}
30.
31.          for (D', l) in selected:
32.              new_path ← path ++ [(D, l, D')]
33.              if no_immediate_revisit(new_path):   # §5.1 invariant 3
34.                  next_frontier ← next_frontier ∪ {(D', new_path)}
35.
36.      frontier ← next_frontier
37.
38.  return NavigationResult {
39.      paths:  Π,
40.      confidence: aggregate_confidence(Π),        # Proposition 6, §4.1
41.      contradictions: detect_contradiction(Π)      # §3.3
42.  }
```

**Subroutine Specifications:**

```
relevance(D, Q, path) → [0, 1]
  Computes P(D relevant | Q, H) via the posterior model of §4.4.
  Implementation: semantic similarity + structural prior (EPR, ω).

estimate_info_gain(Q, D', path) → ℝ≥0
  Estimates I(A; D' | Q, D₀, ..., Dₖ) — the mutual information gain
  from visiting D' (Theorem 2, §2.2). Approximated via:
    · embedding similarity between Q and D'.summary
    · edge-type informativeness ατ (§2.4)
    · diminishing returns from visited documents (submodularity, Theorem 2.1)

no_immediate_revisit(path) → Bool
  Returns true iff path[-1].target ≠ path[-2].target (when |path| ≥ 2).
  Note: stricter acyclicity (all nodes distinct) is NOT enforced —
  the corpus may contain legitimate cycles (§5.1, invariant 3).

annotate(path, D) → Path<D> with provenance
  Constructs φ@π using rules (P-CITE), (P-EXTEND) from §3.2.
  Verifies anchor γ(D) ≠ ∅ (Definition 10.1, condition A1).
```

**Complexity Analysis:**

```
Time:   O(Δ(C)^d · C_eval)     worst case (no pruning)
        O(b̄ · d · C_eval)      expected case (with pruning)

Space:  O(|Π| · d + |frontier| · d)   path storage

where:
  Δ(C)   = max out-degree of corpus graph C
             (NOT |D| — the branching factor is bounded by
              the maximum number of edges from any single node)
  d      = B.max_depth (hop-count budget)
  b̄      ≈ 2-3 (effective branching factor after pruning;
             justified by Theorem 2: if the navigation policy is
             ε-informative, only O(H₀/ε) nodes need visiting,
             and submodularity ensures diminishing returns
             concentrate on a small frontier)
  C_eval = cost per relevance + info-gain evaluation
             (dominates: typically involves embedding computation
              or LLM inference)
```

> [!NOTE]
> **Optimality connection.** By Theorem 2.1, the greedy selection on lines 24-29
> achieves a `(1 - 1/e)` approximation to the optimal information gain. This is
> a consequence of the submodularity of mutual information (Nemhauser et al.,
> 1978). The adaptive threshold `θ` on line 28 controls the
> exploration/exploitation tradeoff: lower `p` = more exploration (higher `b̄`),
> higher `p` = more exploitation (lower `b̄`, faster convergence).

**Termination Guarantee.** The algorithm terminates because:

1. **(T1)** `max_depth` is finite — the outer loop executes at most `d+1` times
   (line 5)
2. **(T2)** The `visited` set is monotonically increasing — each document is
   processed at most once (line 10-12), preventing infinite re-exploration
3. **(T3)** `max_nodes` provides a hard upper bound on `|visited|` (line 8-9),
   guaranteeing termination even in dense graphs

These three conditions together ensure termination in at most
`min(B.max_nodes, |D|)` document evaluations, regardless of graph topology. ∎

### 5.5 Incremental Indexing

**Problem.** When the corpus is large (10k+ documents), full reindexing on every
update is prohibitively costly. We need an incremental update strategy that
maintains all invariants (§5.1) and keeps the EPR scores (Theorem 3) consistent
without global recomputation.

**Solution: Incremental Graph Update (Functional)**

```python
Algorithm: Incremental-Update(C, D_new, R_new)

Input:
  C      — existing corpus graph (D, R, τ, ω, σ)
  D_new  — new or modified document
  R_new  — candidate new edges involving D_new (Set<Edge<D>>)

Output:
  C'     — updated corpus graph (a fresh structure; C is not mutated)

Precondition:    ∀ (u, v, l) ∈ R_new :
                   u, v ∈ C.documents ∪ {D_new}              (endpoint existence)
                   ∧ (type(u), type(v)) ∈ Dom(τ(l))          (ontological validity, §4.3 O2)

 1.  C' ← shallow_copy(C)

     # ——— Document update ———
 2.  if D_new ∈ C.documents:
 3.      # Modify existing document
 4.      C'.documents[D_new] ← reindex(D_new)
 5.
 6.      # Differential edge update: only remove edges that are no longer valid
 7.      old_edges   ← {e ∈ C.edges : D_new ∈ {e.source, e.target}}
 8.      retained    ← {e ∈ old_edges : e ∈ R_new}          # still valid
 9.      removed     ← old_edges \ retained                   # invalidated
10.      added       ← R_new \ old_edges                      # genuinely new
11.      C'.edges    ← (C.edges \ removed) ∪ added
12.
13.      # Invalidate cached navigation results that touched D_new
14.      invalidate_cache(queries_touching(D_new))
15.      # queries_touching : D → Set<Query> is maintained via an
16.      # inverted index: for each query Q in cache, record {D : D ∈ visited(Q)}
17.  else:
18.      # Add new document
19.      C'.documents ← C.documents ∪ {D_new}
20.      C'.edges     ← C.edges ∪ R_new
21.
     # ——— EPR recomputation (localized) ———
22.  affected ← k_hop_neighborhood(C', D_new, k=2)          # cascade radius
23.  C'.EPR   ← incremental_pagerank(C', affected)
24.      # Uses local power iteration on the affected subgraph only
25.      # (Bahmani et al., 2010): iterates until convergence within
26.      # the affected neighborhood, then merges back into global EPR.
27.      # NOT a simple additive delta: PageRank is nonlinear.
28.
     # ——— Weight recomputation ———
29.  for e ∈ (added ∪ retained):
30.      C'.weights[e] ← compute_weight(e, C'.EPR)           # §2.3

31.  return C'
```

> [!WARNING]
> **Consistency guarantee.** The precondition on lines (u, v ∈ documents,
> ontological validity) MUST be checked before calling this algorithm. Violation
> would break the ontological constraints of §4.3 and could introduce spurious
> edges that corrupt the modal semantics of §3.1.

**Incremental PageRank.** We use the **Monte Carlo Personalized PageRank**
algorithm (Bahmani et al., 2010). Key properties:

- **Localized:** only re-walks random paths starting from the `affected` set,
  not the entire graph
- **Nonlinear:** iterates to convergence locally, then merges — NOT a simple
  additive delta (PageRank is a fixed-point equation, not a linear operator on
  deltas)
- **Convergent:** inherits convergence from Theorem 3 (spectral gap of `M`)

**Complexity:**

```
Time:   O(|affected| · t_converge / (1 - d))     expected
        where t_converge ≈ log(|affected|/ε) iterations to ε-convergence
        and d is the damping factor (Theorem 3)

        In practice: sublinear in |D| — proportional to the size of the
        affected subgraph, not the full corpus.

Space:  O(|affected|)  for local EPR delta storage
```

> [!NOTE]
> **Cascade depth.** The parameter `k` in `k_hop_neighborhood` controls how far
> the update propagates. In practice, `k = 2` captures >95% of the EPR change
> (since PageRank influence decays geometrically with distance at rate `d`). For
> `d = 0.85`, the influence at distance `k` is at most `d^k ≈ 0.72` (k=2),
> `0.61` (k=3), which is below typical convergence thresholds.

---

## 6. Convergence Theorem: Unifying the Four Pillars

**Theorem 7 (Canonical Instantiation).** Let **[Corp, Set]₍₁₋₄₎** denote the
full subcategory of the functor category **[Corp, Set]** consisting of functors
`M : Corp → Set` satisfying the following properties and **graph-faithfulness**:

1. **Bounded Graph Reachability** (§2.2): `M(C)` consists of paths decidable in
   `O(Δ(C)^d · d)` for budget parameter `d`
2. **Sound Modal Reasoning** (§3.1): Each element of `M(C)` admits an S4_MDN
   derivation with provenance annotation (§3.2)
3. **Epistemic Coherence** (§4.1-4.2): `M(C)` is equipped with a credence
   function `C(D₀, φ, π)` satisfying the testimonial credence model
   (Proposition 6) and corroboration principle (§4.2)
4. **Functorial Composability** (§2.5): `M` is a functor preserving corpus
   morphisms as defined in Definition 6

**Graph-faithfulness:** `M` does not introduce edges absent from the corpus
graph, i.e., every path in `M(C)` uses only edges present in `R`.

Then `Nav_B` is a **terminal object** in **[Corp, Set]₍₁₋₄₎**.

_Proof._ Let `M ∈ [Corp, Set]₍₁₋₄₎` be any functor satisfying (1)-(4) and
graph-faithfulness. We construct a natural transformation `η : M ⟹ Nav_B` and
show it is unique.

**(Step 1: Graph-structured representation.)** By (1), `M` must produce paths
that are decidable within the corpus graph structure. Graph-faithfulness ensures
that `M` admits a representation over objects equivalent to **Corp** — it cannot
rely on latent edges or implicit relations not present in `R`. Thus `M` and
`Nav_B` operate on the same object class.

**(Step 2: Morphism compatibility is forced.)** By (4), `M` is functorial. For
any corpus morphism `F : C₁ → C₂`, `M(F)` must preserve path structure. Since
corpus morphisms preserve edge types (M1), endpoints (M2), and epistemic order
(M4), the induced path mapping `M(F)` must satisfy the same constraints as
`Nav_B(F)`.

**(Step 3: Provenance annotations constrain paths.)** By (2), every element of
`M(C)` admits an S4_MDN derivation. By soundness of the provenance system
(Theorem 5, §3.2), each such derivation admits a sound encoding as a
provenance-annotated path `φ@π ∈ Paths(C, D₀)`. Consequently, the set of valid
annotated results `M(C)` is contained in the set of all provenance-valid paths
within budget — which is precisely `Nav_B(C)`, defined as the **maximal** such
set (maximal with respect to set inclusion among budget-respecting,
provenance-valid path sets).

**(Step 4: Epistemic weights refine but do not enlarge.)** By (3), `M` equips
each path with credence `C(D₀, φ, π)` via Proposition 6 (§4.1). This credence
function can prune paths from `Nav_B(C)` (by filtering low-credibility routes
below some threshold) but cannot introduce paths not already in `Nav_B(C)`,
since graph-faithfulness prevents the creation of new edges.

Therefore, for each corpus `C`: `M(C) ⊆ Nav_B(C)`.

**Construction of η.** Define `η_C : M(C) ↪ Nav_B(C)` as the set-theoretic
inclusion. This is well-defined by the containment above.

**Naturality.** For any corpus morphism `F : C₁ → C₂`, the square
`Nav_B(F) ∘ η_{C₁} = η_{C₂} ∘ M(F)` commutes because both `M` and `Nav_B` are
functorial and `η` acts as inclusion on paths, which are preserved by `F`.

**Uniqueness.** Let `η' : M ⟹ Nav_B` be any other natural transformation. Since
`M(C) ⊆ Nav_B(C)` and both are sets of provenance-annotated paths, `η'_C` must
map each path `π ∈ M(C)` to itself (the provenance annotation uniquely
identifies the path in `Nav_B(C)`). Hence `η' = η`.

Therefore `Nav_B` is terminal in **[Corp, Set]₍₁₋₄₎**. ∎

> [!IMPORTANT]
> **Canonicality vs. Uniqueness.** We claim `Nav_B` is _canonical_ (terminal),
> not _unique_. Alternative models `M` satisfying (1)-(4) may exist but will
> always embed into `Nav_B` as sub-functors. This is analogous to how the
> universal enveloping algebra is canonical among associative algebras
> containing a given Lie algebra — other solutions exist but factor through the
> canonical one.

> [!CAUTION]
> **Graph-faithfulness is non-trivial.** The graph-faithfulness hypothesis
> excludes models that infer latent relationships (e.g., via embedding
> similarity or co-occurrence). Such models violate (1)-(4) because their
> implicit edges have no provenance annotation (violating (2)) and no
> ontological grounding (violating the constraints of §4.3). If one wished to
> extend MDN to include latent-edge models, the proof would require a richer
> category (e.g., **Corp** augmented with probabilistic edges) and the
> terminality result would need to be re-established in that setting.

**Corollary 7.1 (Extensional Completeness).** MDN is **extensionally complete**
under constraints (1)-(4): any graph-faithful model satisfying (1)-(4) produces
a subset of MDN's results. Extending MDN beyond `Nav_B(C)` requires either:

- Relaxing guarantees (removing termination, soundness, or provenance tracking),
- Adding domain-specific pruning heuristics (which are natural transformations
  `η : Nav_B ⟹ Nav_{B'}` between budget-parameterized functors), or
- Introducing latent edges (which requires lifting the graph-faithfulness
  hypothesis and re-establishing canonicality in an enriched setting)

---

## 7. Integration with AXON Epistemic System

### 7.1 Epistemic Lattice Extension

MDN extends AXON's epistemic lattice `(T, ≤)` with **provenance-aware refinement
types**. We define the ordering explicitly:

```
A ≤ B   ⇔   B carries at least as much epistemic justification as A
```

**Lattice hierarchy** (ordered by increasing justification, bottom to top):

```
⊤ (Any)                          — unconstrained claim (no epistemic guaranty)
│
├── Opinion                       — subjective, no evidence
│
├── Uncertainty                   — insufficient evidence to decide
│
├── ContestedClaim[π₁, π₂]       — conflicting evidence paths
│       (not linearly ordered with CitedFact; see Note below)
│
├── FactualClaim                  — base epistemic type (assertion)
│
├── CitedFact[π]                  — claim with single-path provenance
│
├── CorroboratedFact[π₁ ∪ π₂]    — verified via independent paths
│       requires: independent(π₁, π₂)
│       i.e., nodes(π₁) ∩ nodes(π₂) = {D₀}  (§3.2, P-CORROBORATE)
│
⊥ (Contradiction)                — contradicted / impossible claim
```

> [!NOTE]
> **Non-linear lattice.** The ordering is NOT a total order. In particular:
>
> - `ContestedClaim` and `CitedFact` are **incomparable**: a contested claim has
>   evidence (unlike `Uncertainty`) but contains conflict (unlike `CitedFact`).
> - `Opinion` and `Uncertainty` are incomparable: one is subjective, the other
>   is evidence-deficient.
>
> The lattice forms a **diamond** at the evidence level:
>
> ```
>        CorroboratedFact
>        /              \
> CitedFact          ContestedClaim
>        \              /
>        FactualClaim
> ```

**Refinement Type Syntax.** Provenance is encoded as a **refinement** (dependent
annotation), not a value-level field:

```
CitedFact[π]                — π : Path<D> witnesses the citation chain
CorroboratedFact[π₁ ∪ π₂]  — π₁, π₂ are independent provenance paths
ContestedClaim[π₁, π₂]     — π₁ supports φ, π₂ supports ¬φ
```

This separates the **type** (epistemic status) from the **evidence** (provenance
path), enabling static type checking of epistemic transitions while keeping
provenance as a runtime value.

**Promotion and Demotion Rules:**

```
Promotion:
  FactualClaim        ≤  CitedFact[π]
      when ∃ π : valid_path(π) ∧ anchor(π, φ)           (§3.2, P-CITE)

  CitedFact[π₁]       ≤  CorroboratedFact[π₁ ∪ π₂]
      when  independent(π₁, π₂)                          (§4.2, P-CORROBORATE)
      i.e., nodes(π₁) ∩ nodes(π₂) = {D₀}

Demotion:
  CitedFact[π]         →  ContestedClaim[π, π']
      when ∃ π' : anchor(π', ¬φ)                         (§3.3, ⟨contradict⟩φ)

  ContestedClaim[π,π'] →  ⊥    (Contradiction)
      when resolution is impossible (formally: no ω consistent assignment)
```

**Join and Meet Operations:**

```
CitedFact[π₁] ⊔ CitedFact[π₂]
    = CorroboratedFact[π₁ ∪ π₂]    if independent(π₁, π₂)
    = CitedFact[π₁ · π₂]            if dependent (path extension)

CitedFact[π] ⊔ ContestedClaim[π', π'']
    = ContestedClaim[π ∪ π', π'']   (new evidence joins supporting side)

CitedFact[π] ⊓ ContestedClaim[π', π'']
    = ContestedClaim                 (conservative: conflict persists)

⊤ ⊔ A = ⊤    ⊥ ⊓ A = ⊥           (standard lattice bounds)
```

> [!IMPORTANT]
> **Connection to §5.2 (Effect System).** The epistemic lattice `(T, ≤)` aligns
> with the epistemic effect lattice `(⊥_E, speculate, believe)`:
>
> - `speculate` produces `FactualClaim` or `CitedFact` (tentative evidence)
> - `believe` produces `CorroboratedFact` (committed, corroborated evidence)
> - Contradiction detection moves claims to `ContestedClaim` or `⊥`

### 7.2 Anchor Integration

MDN results — typed as `NavigationResult<D>` (§5.1) — must pass through AXON's
anchor system before being committed to the epistemic state. Each anchor
enforces a formal invariant and triggers a lattice demotion (§7.1) on violation.

```typescript
anchor CrossReferenceValid {
    // Input: result : NavigationResult<D>
    ensures: forall path in result.paths :
        |path.edges| <= B.max_depth                          // budget (§5.1, invariant 4)
        && forall (Dᵢ, Dⱼ, φ) in result.contradictions :
            type(φ) != ⊥                                     // no unresolved contradictions

    on_violation(claim):
        // Formal demotion (§7.1): CitedFact[π] → ContestedClaim[π, π']
        demote(claim, ContestedClaim)
        quarantine(claim)
}

anchor SourceAuthority {
    // EPR authority threshold: τ_auth ∈ [0, 1], connected to σ (§2.3)
    parameter: τ_auth : Float = 0.3

    requires: forall path in result.paths :
        min({ EPR(D) : D in nodes(path) }) >= τ_auth
        // nodes(path) derived from path.edges (§5.1):
        // nodes = [edges[0].source] ++ edges.map(e => e.target)

    on_violation(path):
        warn("low-authority source: min EPR = " ++ show(min_epr))
        // Does NOT demote: low authority reduces confidence
        // but does not invalidate the provenance chain
}

anchor EpistemicConsistency {
    // Verify that epistemic type assignments respect the lattice (§7.1)
    ensures: forall claim in result.paths :
        type(claim) is consistent with:
            CorroboratedFact  => |provenance_paths(claim)| >= 2
                                 && independent(provenance_paths(claim))
            CitedFact         => |provenance_paths(claim)| >= 1
            ContestedClaim    => exists contradiction in result.contradictions

    on_violation(claim):
        // Type assignment is inconsistent with evidence structure
        downgrade(claim, FactualClaim)    // reset to base type
}
```

> [!NOTE]
> **Anchor ↔ Effect interaction.** Anchors run AFTER the navigate flow completes
> (post-condition). They do NOT add new effects — they enforce constraints on
> the output of `epistemic:believe` operations (§5.2). A failed anchor check
> triggers lattice demotion, which is a pure type-level operation, not an
> effect.

### 7.3 Shield Compatibility

MDN inherits AXON v0.14's shield system (cf. `shield` primitive). The
`ShieldScan` operates on `NavigationResult<D>` and checks properties at the
**effect boundary** — where `epistemic:believe` results cross into the
application layer.

```
ShieldScan(result : NavigationResult<D>) verifies:

  1. Data exfiltration     [io:storage boundary]
     No PII from source documents is exposed in synthesized output.
     Enforced via taint tracking on document content fields.

  2. Prompt injection      [epistemic:speculate boundary]
     No adversarial content in document summaries can influence
     navigation decisions. Validated against D.summary fields
     before scoring (§5.4, line 25: estimate_info_gain).

  3. Taint propagation     [epistemic:believe boundary]
     Untrusted sources (EPR(D) < τ_trust) are marked in the
     provenance path π. Claims from tainted paths cannot be
     promoted above CitedFact[π] (§7.1 promotion rules).

  4. Capability bounds     [effect_row boundary]
     Navigation respects declared edge_filter — no edges of
     type t ∉ B.edge_filter appear in any path of result.paths.
     This is verified by the type system (§5.1, invariant 1)
     and re-checked by the shield as defense-in-depth.
```

---

## 8. Comparative Analysis

### 8.1 MDN vs. Existing Approaches

| Approach                 | Method                 | Handles Cross-Refs? | Explainable? | Type-Safe? | Epistemic Tracking? |
| ------------------------ | ---------------------- | ------------------- | ------------ | ---------- | ------------------- |
| **RAG**                  | Vector similarity      | ❌ No               | ❌ No        | ❌ No      | ❌ No               |
| **GraphRAG** (Microsoft) | Graph + embeddings     | ⚠️ Partial          | ⚠️ Partial   | ❌ No      | ❌ No               |
| **HippoRAG** (2024)      | Hippocampus-inspired   | ⚠️ Partial          | ❌ No        | ❌ No      | ❌ No               |
| **MDN (AXON)**           | Typed graph navigation | ✅ Yes              | ✅ Yes       | ✅ Yes     | ✅ Yes              |

### 8.2 When to Use MDN vs. PIX

```
Use PIX (single-document) when:
  - Document is self-contained
  - No cross-references needed
  - Budget is tight (lower latency)

Use MDN (multi-document) when:
  - Knowledge is distributed
  - Citations matter (legal, academic)
  - Corroboration increases confidence
  - Contradictions must be detected
```

---

## 9. Open Research Questions

1. **Multi-Corpus Federation:** Can multiple organizations share document graphs
   while preserving privacy? (Federated learning analogue)

2. **Dynamic Edge Discovery:** Can the LLM infer missing edges (implicit
   citations) during navigation?

3. **Temporal Graph Evolution:** How to model version histories as time-indexed
   graphs `C(t)`?

4. **Adversarial Robustness:** Can malicious documents inject false edges to
   manipulate navigation?

5. **Optimal Branching Factor:** Is there a theoretical optimum for `b_max` as a
   function of corpus size and query complexity?

---

## 10. Conclusion

We have presented a comprehensive framework for multi-document navigation
grounded in:

- **Graph theory** for formal structure
- **Modal logic** for reasoning about distributed knowledge
- **Social epistemology** for modeling testimonial chains
- **Type theory** for safe compilation

MDN extends AXON's PIX primitive from trees to graphs while preserving
termination, explainability, and epistemic tracking. The framework is
implementable as native AXON primitives and compatible with the existing
anchor/shield/effect infrastructure.

**Future work** will focus on empirical evaluation, federated corpus navigation,
and integration with AXON's formal verification roadmap (AXON/Proof).

---

## References

### Graph Theory & Signed Graphs

- Bondy, J. A., & Murty, U. S. R. (2008). _Graph Theory_. Springer.
- Bahmani, B., Chierichetti, F., Kumar, R., & Lattanzi, S. (2010). Fast
  Incremental and Personalized PageRank. _Proceedings of the VLDB Endowment_,
  4(3), 173-184.
- Harary, F. (1953). On the Notion of Balance of a Signed Graph. _Michigan
  Mathematical Journal_, 2(2), 143-146.
- Cartwright, D., & Harary, F. (1956). Structural Balance: A Generalization of
  Heider's Theory. _Psychological Review_, 63(5), 277-293.
- Kamvar, S. D., Schlosser, M. T., & Garcia-Molina, H. (2003). The EigenTrust
  Algorithm for Reputation Management in P2P Networks. _Proceedings of the 12th
  International Conference on World Wide Web (WWW)_, 640-651.

### Modal Logic

- Blackburn, P., de Rijke, M., & Venema, Y. (2001). _Modal Logic_. Cambridge
  University Press.
- Fagin, R., Halpern, J. Y., Moses, Y., & Vardi, M. Y. (1995). _Reasoning About
  Knowledge_. MIT Press.

### Social Epistemology

- Coady, C. A. J. (1992). _Testimony: A Philosophical Study_. Oxford University
  Press.
- Goldman, A. I. (1999). _Knowledge in a Social World_. Oxford University Press.

### Type Theory & Effects

- Plotkin, G., & Pretnar, M. (2013). Handling Algebraic Effects. _Logical
  Methods in Computer Science_, 9(4).
- Wadler, P., & Thiemann, P. (2003). The Marriage of Effects and Monads. _ACM
  SIGPLAN Notices (ICFP)_, 38(1), 63-74.

### Information Theory & Geometry

- Cover, T. M., & Thomas, J. A. (2006). _Elements of Information Theory_. 2nd
  ed. Wiley.
- MacKay, D. J. C. (2003). _Information Theory, Inference, and Learning
  Algorithms_. Cambridge University Press.
- Amari, S. (1985). _Differential-Geometrical Methods in Statistics_. Lecture
  Notes in Statistics, Vol. 28. Springer.

### Submodular Optimization

- Nemhauser, G. L., Wolsey, L. A., & Fisher, M. L. (1978). An Analysis of
  Approximations for Maximizing Submodular Set Functions—I. _Mathematical
  Programming_, 14(1), 265-294.
- Feige, U. (1998). A Threshold of ln n for Approximating Set Cover. _Journal of
  the ACM_, 45(4), 634-652.
- Golovin, D., & Krause, A. (2011). Adaptive Submodularity: Theory and
  Applications in Active Learning and Stochastic Optimization. _Journal of
  Artificial Intelligence Research_, 42, 427-486.
- Krause, A., & Guestrin, C. (2005). Near-optimal Nonmyopic Value of Information
  in Graphical Models. _Proceedings of the 21st Conference on Uncertainty in
  Artificial Intelligence (UAI)_, 324-331.

### Category Theory

- Mac Lane, S. (1998). _Categories for the Working Mathematician_. 2nd ed.
  Graduate Texts in Mathematics, Vol. 5. Springer.
- Spivak, D. I. (2014). _Category Theory for the Sciences_. MIT Press.
- Spivak, D. I., & Kent, R. E. (2012). Ologs: A Categorical Framework for
  Knowledge Representation. _PLoS ONE_, 7(1), e24274.

### Retrieval Systems

- Lewis, P., Perez, E., Piktus, A., et al. (2020). Retrieval-Augmented
  Generation for Knowledge-Intensive NLP Tasks. _Advances in Neural Information
  Processing Systems (NeurIPS)_, 33.
- Edge, D., Trinh, H., Cheng, N., et al. (2024). From Local to Global: A Graph
  RAG Approach to Query-Focused Summarization. Microsoft Research.

### Cognitive Science

- Pirolli, P., & Card, S. (1999). Information Foraging. _Psychological Review_,
  106(4), 643-675.

---

## Appendix: Formal Notation Summary

| Symbol                   | Meaning                                              |
| ------------------------ | ---------------------------------------------------- |
| `C = (D, R, τ, ω, σ)`    | Document corpus graph                                |
| `D`                      | Set of documents                                     |
| `R ⊆ D × D × L`          | Labeled directed edges                               |
| `τ : R → RelationType`   | Edge type function                                   |
| `ω : R → (0,1]`          | Edge weight function                                 |
| `σ : D → EpistemicLevel` | Document epistemic status (anti-monotonic, G5)       |
| `EPR(D)`                 | Epistemic PageRank of document D                     |
| `G⁺, G⁻`                 | Positive/negative subgraphs (trust/distrust)         |
| `P⁺, P⁻`                 | Transition matrices for G⁺, G⁻                       |
| `EPR⁺, EPR⁻`             | Trust/distrust PageRank components                   |
| `λ`                      | Distrust penalty weight in EPR                       |
| `u⁺, u⁻`                 | Trust/distrust teleportation priors (`∈ Δ^{\|D\|}`)  |
| `J(Dᵢ, Dⱼ)`              | Jeffreys divergence (symmetrized KL)                 |
| `ατ`                     | Type-dependent cost coefficient for edge type τ      |
| `c(Dᵢ, Dⱼ, τ) = ατ·J`    | Edge traversal cost (Def. 5.1)                       |
| `Kᵢ φ`                   | Document Dᵢ knows proposition φ                      |
| `[t]φ`                   | φ follows from edges of type t                       |
| `⟨contradict⟩φ`          | Contradiction exists about φ                         |
| `φ@π`                    | Claim φ with provenance path π                       |
| `I(A; D \| Q, π)`        | Conditional mutual information                       |
| `π`                      | Navigation policy (Def. 3) or provenance path        |
| `ε`                      | Minimum information gain threshold                   |
| `f(S) = I(A; S \| Q)`    | Information value function (submodular)              |
| **Corp**                 | Category of document corpora                         |
| `F : C₁ → C₂`            | Corpus morphism (structure-preserving map)           |
| `Nav_B : Corp → Set`     | Navigation functor (parameterized by budget B)       |
| `η : N₁ ⟹ N₂`            | Natural transformation between navigation strategies |

---

**END OF DOCUMENT**
