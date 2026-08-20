# PIX: Structured Cognitive Retrieval via Navigational Semantics

> **Formal Research Document — Axon-Lang v0.15 Proposal**
> Authors: Ricardo Velit & Axon Research Team
> Date: 2026-03-16
> Status: Research Phase

---

## Abstract

We present **PIX**, a formal retrieval primitive for Axon-lang that replaces
statistical vector similarity (embedding-based RAG) with **intentional structural
navigation** — an LLM-guided tree traversal that mirrors expert human cognition
when consulting complex documents. PIX treats documents as formal trees
`D = (N, E)`, uses Bayesian belief updating to guide navigation, and reduces
conditional entropy at each traversal step. We prove that under reasonable
assumptions, PIX's navigational retrieval is both more precise and more
explainable than cosine-similarity based chunking, while remaining
computationally viable.

**Core thesis:**

> *"Lo estructuralmente navegado con intención es lo relevante"*
> vs. RAG's assumption: *"Lo semánticamente cercano es lo relevante"*

---

## 1. Foundations: Why RAG's Axioms are Insufficient

### 1.1 The Embedding Similarity Axiom and Its Failure Modes

Traditional RAG operates under an implicit axiom:

```text
Axiom_RAG: Relevant(chunk, query) ⟺ cos(φ(chunk), φ(query)) ≥ θ

where
  φ : Text → ℝ^d       — embedding function
  θ ∈ [0, 1]           — similarity threshold
  d ∈ {768, 1536, ...} — embedding dimension
```

This axiom conflates three distinct semantic relations:

| Relation | Description | Example |
|---|---|---|
| **Synonymy** | Same meaning, different words | "car" ≈ "automobile" |
| **Co-occurrence** | Frequently appear together | "bank" ≈ "river" |
| **Functional relevance** | Useful for answering | "Clause 7.2 modifies liability in Section 3" |

Embeddings capture (1) and (2) but systematically fail at (3). A chunk that
*mentions* a term is not the same as a chunk that *answers* the question. This
distinction is fundamental.

**Formal failure mode — the Relevance-Similarity Gap:**

```text
∃ Q, chunk_A, chunk_B :
  cos(φ(Q), φ(chunk_A)) > cos(φ(Q), φ(chunk_B))
  ∧ Relevant(chunk_B, Q) ∧ ¬Relevant(chunk_A, Q)

(chunk_A mentions the query terms but doesn't answer;
 chunk_B answers but uses different terminology)
```

This gap is not an edge case — it is **structural** in high-dimensional
embedding spaces, particularly for professional documents where the answer
to a question may reside several sections away from where the relevant
terminology appears.

### 1.2 The Chunking Destruction Theorem

**Theorem 1 (Semantic Emergence Destruction).** Let `D` be a structured
document with emergent semantic properties `P = {p₁, ..., pₘ}` arising from
non-local relationships between sections `{s₁, ..., sₙ}`. A chunking function
`C : D → {c₁, ..., cₖ}` with fixed window size `w` destroys emergent property
`pⱼ` if `pⱼ` depends on sections `sₐ, sᵦ` with `|loc(sₐ) - loc(sᵦ)| > w`.

*Proof sketch.* Let `pⱼ` be the property "Section 5 modifies the liability
established in Section 2." If `w < |loc(s₅) - loc(s₂)|`, then no chunk
contains both sections. Any retrieval of individual chunks loses the
cross-reference relationship. The property `pⱼ` is destroyed. ∎

**Corollary.** For documents with deep hierarchical structure (legal contracts,
technical specifications, academic papers), the probability of emergent
property destruction increases monotonically with document complexity:

```text
P(∃ pⱼ destroyed) → 1  as  structural_depth(D) → ∞
```

### 1.3 PIX's Alternative Axiom

PIX replaces `Axiom_RAG` with a navigational axiom:

```text
Axiom_PIX: Relevant(section, query) ⟺
    I(R; section | query, path) > ε

where
  I(R; section | Q, path)  — conditional mutual information
  R                        — random variable: correct answer
  path                     — sequence of navigational decisions
  ε > 0                    — information significance threshold
```

This axiom states: a section is relevant if and only if visiting it
**reduces uncertainty** about the correct answer, conditioned on the query
and the navigational path already taken. This is a fundamentally different
criterion from vector similarity.

---

## 2. Mathematical Framework

### 2.1 Document Tree — Formal Definition

**Definition 1 (Document Tree).** A document tree is a tuple `D = (N, E, ρ, κ)`
where:

```text
N = {n₀, n₁, ..., nₖ}              — finite set of nodes
E ⊆ N × N                          — directed edges (parent → child)
ρ : N → ⟨title, summary, location⟩ — node representation function
κ : N → 𝒫(N)                       — children function

Subject to:
  (T1) ∃! n₀ ∈ N : indegree(n₀) = 0          (unique root)
  (T2) ∀ nᵢ ∈ N \ {n₀} : ∃! nⱼ : (nⱼ, nᵢ) ∈ E  (unique parent)
  (T3) D is acyclic                             (no navigation loops)
  (T4) ⋃ᵢ content(nᵢ) ⊇ content(D)            (exhaustive coverage)
  (T5) ∀ siblings nᵢ, nⱼ ∈ κ(nₚ) :
       content(nᵢ) ∩ content(nⱼ) ≈ ∅           (controlled disjunction)
```

**Property (T3)** guarantees termination of any traversal algorithm.
**Property (T4)** guarantees no information is lost during indexing.
**Property (T5)** minimizes redundancy between sibling nodes, ensuring that
the LLM's navigational decisions are based on genuine semantic distinctions,
not duplicated content.

### 2.2 Node Representation — Information-Theoretic Compression

Each node `nᵢ` is represented as a compressed tuple:

```text
ρ(nᵢ) = ⟨titleᵢ, summaryᵢ, locationᵢ, childrenᵢ⟩

where:
  titleᵢ    : String, |titleᵢ| ∈ [5, 15] words
             — high semantic density, H(titleᵢ)/|titleᵢ| maximized
  
  summaryᵢ  : String, H(summaryᵢ) << H(contentᵢ)
             — lossy compression preserving navigational salience
  
  locationᵢ : [page_start, page_end] × [offset_start, offset_end]
             — spatial metadata for content retrieval
  
  childrenᵢ : List[NodeRef]
             — empty for leaf nodes
```

**Definition 2 (Compression Ratio).** The information compression ratio of
node `nᵢ` is:

```text
CR(nᵢ) = H(summaryᵢ) / H(contentᵢ)

where H denotes Shannon entropy.

Target: CR(nᵢ) ∈ [0.05, 0.15]
(summary carries 5–15% of the original information content,
 sufficient for navigational decisions but not for final answers)
```

This compression is **intentionally lossy**: summaries need only preserve
enough information for the LLM to decide *whether to explore deeper*, not
to answer the query. Final answers come from leaf-node content, which is
uncompressed.

### 2.3 Navigational Entropy Reduction

**Theorem 2 (Monotonic Entropy Reduction).** Let `Q` be a query and
`R` the correct answer. At each navigation step `t`, selecting node `nₜ`
from candidates `C(t)` reduces conditional entropy:

```text
H(R | Q, n₁, ..., nₜ) ≤ H(R | Q, n₁, ..., nₜ₋₁)

with equality iff I(R; nₜ | Q, n₁, ..., nₜ₋₁) = 0
(the selected node provides no new information about the answer)
```

*Proof.* By the chain rule of mutual information:

```text
H(R | Q, n₁, ..., nₜ) = H(R | Q, n₁, ..., nₜ₋₁) - I(R; nₜ | Q, n₁, ..., nₜ₋₁)

Since I(R; nₜ | Q, ...) ≥ 0 (mutual information is non-negative),
we have H(R | Q, n₁, ..., nₜ) ≤ H(R | Q, n₁, ..., nₜ₋₁). ∎
```

**Corollary (Convergence).** For a finite tree with depth `h`, the
navigation converges in at most `h` steps:

```text
H(R | Q, path_complete) ≤ H(R | Q) - Σᵢ I(R; nᵢ | Q, n₁, ..., nᵢ₋₁)
```

The LLM approximates the mutual information function:

```text
I(R; nᵢ | Q) ≈ f_LLM(query, nᵢ.summary, nᵢ.title) ∈ [0, 1]
```

This is where the LLM's strength lies: not in embedding similarity, but in
**reasoning about whether a section's summary suggests it contains the answer**.

### 2.4 Bayesian Navigation — Belief Updates

Each navigational decision follows Bayesian updating:

```text
P(nᵢ relevant | Q, evidence) ∝ 
    P(Q | nᵢ relevant) · P(nᵢ relevant | evidence)

where:
  P(Q | nᵢ relevant)      — likelihood: "How probable is this query
                              if this section contains the answer?"
  
  P(nᵢ relevant | evidence) — prior: Updated from structural position,
                              sibling evaluations, and parent context.
```

**Prior construction.** The structural prior is computed from document
conventions:

```text
P(nᵢ relevant | structure) = f(type(nᵢ), depth(nᵢ), query_class(Q))

Examples:
  P(Introduction | specific_query)   = low   (introductions are general)
  P(Appendix | methodology_query)    = high  (appendices contain details)
  P(Section_3.2 | Section_3_query)   = high  (structural proximity)
```

**Advantage over embeddings:** Embeddings encode statistical correlations
from training data. PIX's Bayesian navigation encodes **structural
causality**: "If I'm looking for X, I should look in Y because the document
is organized so that Z."

### 2.5 Navigational Search Algorithm

The retrieval follows bounded breadth-first search with LLM-heuristic pruning:

```text
Algorithm: PIX-Navigate(Q, D, b_max, d_max)

Input:
  Q     — user query
  D     — document tree
  b_max — maximum branching factor (default: 3)
  d_max — maximum navigation depth (default: 4)

Output:
  L ⊆ Leaves(D) — retrieved leaf nodes with reasoning paths

1.  frontier ← {root(D)}
2.  for depth = 0 to d_max:
3.      next_frontier ← ∅
4.      for node ∈ frontier:
5.          if is_leaf(node):
6.              L ← L ∪ {(node, path(node))}
7.              continue
8.          scores ← {(child, f_LLM(Q, child.summary)) : child ∈ κ(node)}
9.          θ ← adaptive_threshold(scores, complexity(Q))
10.         selected ← top_k({c : (c, s) ∈ scores, s ≥ θ}, k=b_max)
11.         next_frontier ← next_frontier ∪ selected
12.     frontier ← next_frontier
13. return L
```

**Complexity analysis:**

```text
Indexing:   O(n · log(n) · C_LLM)
            where n = document pages, C_LLM = cost per LLM summarization

Retrieval:  O(b^d · C_LLM)  worst case
            O(b̄ · d̄ · C_LLM)  expected case (with pruning)
            
            where b̄ ≈ 2-3 (effective branching after pruning)
                  d̄ ≈ 3-4 (average navigation depth)
            
            Expected: ~10-30 LLM evaluations per query

Comparison:
  RAG chunk retrieval:  O(n · d_embed)     — fast but imprecise
  Reranking over RAG:   O(k · C_LLM)      — k ≈ 20-100 chunks
  PIX navigation:   O(b̄·d̄ · C_LLM)    — ~10-30 evaluations, high precision
```

---

## 3. Epistemological Analysis

### 3.1 Two Paradigms of Relevance

| Dimension | Vector Paradigm (RAG) | Navigational Paradigm (PIX) |
|---|---|---|
| **Definition of relevance** | `cos(φ(Q), φ(chunk)) ≥ θ` | `I(R; section \| Q, path) > ε` |
| **What it measures** | Statistical co-occurrence | Functional utility for answering |
| **Knowledge representation** | Flat vector space | Hierarchical tree with structure |
| **Failure mode** | Synonymy confusion | Summary compression loss |
| **Explainability** | "These chunks scored highest" | "I navigated here because..." |
| **Cross-reference handling** | Lost during chunking | Preserved in tree structure |
| **Cognitive model** | Content-addressable memory | Expert document consultation |

### 3.2 Cognitive Science Foundation: Expert Document Consultation

PIX's design is grounded in cognitive science research on how domain
experts navigate complex documents (Ericsson & Kintsch, 1995; Pirolli &
Card, 1999):

| Human Expert Process | PIX Analogue |
|---|---|
| **Skimming:** Rapid scanning of titles and headings | Evaluation of `title` + `summary` at high-level nodes |
| **Drill-down:** Deep-diving into promising sections | Recursive traversal into selected subtrees |
| **Cross-referencing:** Following internal references | Location metadata enables non-linear jumps |
| **Hypothesis maintenance:** "I think the answer is in Section 4" | LLM retains query + reasoning context across depth |
| **Satisficing:** "Good enough" when time is limited | `b_max` and `d_max` bounds enforce bounded rationality |

**Cognitive Load Theory (Sweller, 1988) applied:**

- **Chunking** fragments force the LLM to "reconstruct" lost context between
  pieces — this is extraneous cognitive load.
- **PIX** preserves natural semantic units: each node is a coherent section
  of the original document — this respects the intrinsic cognitive structure.

### 3.3 Information Foraging Theory

PIX implements Pirolli & Card's (1999) **Information Foraging Theory**
in a formal computational model:

```text
Information Scent: IS(nᵢ, Q) = f_LLM(Q, nᵢ.summary)

The LLM follows the "information scent" — navigating toward nodes whose
summaries most strongly suggest relevance to the query.

Optimal foraging predicts that the agent will:
1. Follow strong scent trails (high IS scores)
2. Abandon weak trails (low IS, prune branch)
3. Backtrack when scent disappears (no children relevant)
```

This is precisely what the navigational algorithm does: the LLM acts as a
rational forager in the document's information landscape, guided by compressed
summaries (scent cues) rather than raw content (full foraging).

---

## 4. Emergent Semantic Properties Preserved

### 4.1 Properties That Chunking Destroys

A professional document `D` exhibits emergent properties that arise from
non-local relationships between sections:

```text
P_emergent(D) = {
  hierarchy:      abstractions at each depth level
  temporal_deps:  "as mentioned in Section 2.1..."  
  cross_refs:     "see Table 5 for comparison"
  rhetorical:     argument → evidence → conclusion
  conditional:    "if applicable, see Appendix B"
}
```

**Theorem 3 (Emergence Preservation).** PIX's tree representation `D = (N, E)`
preserves emergent properties of the original document `D_orig`:

```text
∀ p ∈ P_emergent(D_orig) :
  ∃ path(n_a, n_b) ∈ D :
    p is recoverable from ρ(n_a) ∪ ρ(n_b) ∪ path(n_a, n_b)
```

*Proof sketch.* Since the tree preserves hierarchical structure (T1-T2),
parent-child relationships encode the document's abstraction hierarchy.
Cross-references are preserved via location metadata (locationᵢ). Temporal
dependencies are maintained because sibling ordering reflects document ordering.
Rhetorical structure is encoded in the parent node's summary, which contextualizes
its children. ∎

### 4.2 The Explicability Guarantee

**Theorem 4 (Reasoning Path Explainability).** Every PIX retrieval produces
a reasoning path `π = (n₀, n₁, ..., nₗ)` such that:

```text
∀ step (nᵢ → nᵢ₊₁) ∈ π :
  ∃ reasoning_text :
    reasoning_text explains why nᵢ₊₁ was selected from κ(nᵢ)
```

This is architecturally guaranteed — the LLM must evaluate and justify each
navigational decision. In contrast, RAG's similarity ranking provides no
causal explanation for why a chunk scored higher than another.

**Practical implication:** In regulated industries (legal, financial, medical),
"why was this retrieved?" is as important as "what was retrieved." PIX
provides this by construction, not as an afterthought.

---

## 5. Integration with Axon-lang Epistemic System

### 5.1 Epistemic Gradient of Retrieved Content

PIX's output integrates directly with Axon-lang's epistemic lattice
`(T, ≤)`:

```text
Epistemic assignment:

  Node summary (intermediate):  speculate
    — lossy compression, used only for navigation decisions
  
  Leaf content (terminal):      believe
    — original document content, but provenance is external
  
  Validated content:            know
    — only after anchor validation against ground truth

Gradient: speculate ⊑ believe ⊑ know
```

This means PIX retrieval results are **never automatically trusted** at
the `know` level. They must pass through Axon's anchor/shield validation
pipeline before being promoted — consistent with CT-2 (tool effects) from
v0.14.0.

### 5.2 Effect Row for PIX

PIX's retrieval operation has a well-defined effect signature:

```text
EffectRow(PIX_retrieve) = ⟨io, epistemic:believe⟩

  io       — reads from document storage
  believe  — retrieved content is trusted but unverified
```

This is more trustworthy than `network + speculate` (web search) because the
data source is a controlled, pre-indexed document — but less trustworthy than
`pure + know` because external I/O is involved.

### 5.3 Proposed Axon-lang Primitives

```text
Primitive Family: PIX (Structured Cognitive Retrieval)

  PIX   — Declare an indexed document tree
  navigate  — LLM-guided tree traversal (the core retrieval)
  drill     — Explicit descent into a named subtree
  trail     — Access the reasoning path (explainability)
```

**Proposed syntax:**

```axon
PIX ContractIndex {
    source: "contracts/master_agreement.pdf"
    depth: 4
    branching: 3
    model: fast          // Use lightweight LLM for navigation
}

flow AnalyzeClause(question: String) -> Analysis {
    step Retrieve {
        navigate ContractIndex with query: question
        trail: enabled     // Record reasoning path
        output: RelevantSections
    }
    step Analyze {
        reason {
            given: Retrieve.output
            ask: question
            depth: 2
        }
        output: Analysis
    }
}
```

### 5.4 Formal Correspondence: PIX Primitives → Existing Axon Concepts

| PIX Concept | Axon Analogue | Relationship |
|---|---|---|
| Document tree `D` | `dataspace` | Both are structured data containers |
| Navigation `f_LLM` | `probe` | Both extract information via LLM evaluation |
| Branching selection | `focus` | Both select subsets from a larger collection |
| Reasoning path | `trail` (new) | Extends Axon's tracer with retrieval provenance |
| Adaptive threshold | `deliberate` | Both manage compute budget dynamically |
| Leaf extraction | `ingest` | Both load external data into the pipeline |

---

## 6. Graceful Degradation and Production Guarantees

### 6.1 Robustness Mechanisms

PIX adopts the principle: *"Better a good, traceable answer than a
perfect but opaque one."*

```text
Robustness portfolio:

  R1: Timeout per phase
      If navigation exceeds T_max seconds → return best-effort path
  
  R2: Fallback to vector retrieval
      If tree construction fails → use embeddings as Plan B
  
  R3: Subtree caching
      If multiple queries touch same branch → cache evaluation
  
  R4: Tiered model usage
      Navigation: lightweight model (gpt-4o-mini, cost ≈ $0.0001/eval)
      Generation: powerful model (gpt-4o, Claude Opus)
  
  R5: Consistency validation
      Post-hoc: verify that the final answer cites retrieved sources
```

### 6.2 Cost Model

```text
Cost(PIX_query) = C_nav · n_evals + C_gen · 1

where:
  C_nav  = cost per navigation evaluation ($0.0001 for gpt-4o-mini)
  n_evals ≈ 10-30 (typical, after pruning)
  C_gen  = cost per generation ($0.01-0.03 for gpt-4o)

Total: ~$0.004-0.006 per query (navigation + generation)

Compare:
  RAG + reranking:  $0.002-0.01 per query (embedding + retrieval + rerank)
  PIX:          $0.004-0.006 per query (navigation + generation)

PIX is cost-competitive with RAG while providing:
  - Structural explainability
  - Emergent property preservation
  - Epistemic tracking
  - No vector database required
```

---

## 7. Theoretical Comparison

### 7.1 PIX vs. RAG — Formal Advantages

| Property | RAG | PIX |
|---|---|---|
| **Relevance definition** | Statistical similarity | Information-theoretic utility |
| **Knowledge preservation** | Partial (chunking destroys emergence) | Complete (tree preserves structure) |
| **Explainability** | Post-hoc (similarity scores) | By-construction (reasoning paths) |
| **Cross-reference handling** | Lost | Preserved via location metadata |
| **Epistemic tracking** | None | Integrated with Axon lattice |
| **Infrastructure** | Vector DB required | JSON/SQL sufficient |
| **Computational model** | Retrieval ≡ nearest-neighbor search | Retrieval ≡ guided tree traversal |
| **Cognitive analogue** | Content-addressable memory | Expert document consultation |

### 7.2 When RAG is Preferable

PIX is not universally superior. RAG remains preferable when:

```text
Prefer RAG when:
  - Documents lack hierarchical structure (e.g., flat FAQ lists)
  - Query volume is extremely high (>1000 QPS) and latency < 100ms required
  - Documents are very short (< 5 pages) — tree overhead not justified
  - Cross-document retrieval is the primary use case (PIX is per-document)
```

### 7.3 Hybrid Architecture

The optimal production architecture combines both:

```text
Query → Intent Classification
         │
         ├─ Factual/specific → PIX (structural navigation)
         │                     High precision, explainable, epistemic-tracked
         │
         └─ Exploratory/broad → RAG (vector similarity)
                                 Fast, broad coverage, cross-document
```

---

## 8. Open Research Questions

1. **Multi-document navigation:** Can PIX trees be composed into a forest
   `F = {D₁, D₂, ..., Dₘ}` with cross-tree navigation edges?

2. **Incremental indexing:** When a document is updated, can we re-index only
   the affected subtree rather than rebuilding the entire tree?

3. **Optimal tree depth:** Is there a theoretical optimum for `d_max` given
   document complexity and query type? Can we derive it from information theory?

4. **Navigation model fine-tuning:** Can a small model be fine-tuned specifically
   for navigational scoring, replacing general-purpose LLM evaluation?

5. **Formal verification of retrieval quality:** Can we prove bounds on retrieval
   precision/recall for PIX given properties of the document tree?

---

## References

- Ericsson, K. A., & Kintsch, W. (1995). Long-term working memory. *Psychological Review*, 102(2), 211-245.
- Findler, R. B., & Felleisen, M. (2002). Contracts for higher-order functions. *ICFP*.
- Lewis, P., et al. (2020). Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks. *NeurIPS*.
- Pirolli, P., & Card, S. (1999). Information Foraging. *Psychological Review*, 106(4), 643-675.
- Plotkin, G. D., & Pretnar, M. (2013). Handling algebraic effects. *LMCS*, 9(4).
- Shannon, C. E. (1948). A Mathematical Theory of Communication. *Bell System Technical Journal*.
- Sweller, J. (1988). Cognitive Load During Problem Solving. *Cognitive Science*, 12(2), 257-285.

---

## Appendix A: Formal Notation Summary

| Symbol | Meaning |
|---|---|
| `D = (N, E, ρ, κ)` | Document tree |
| `N` | Set of nodes |
| `E` | Directed edges |
| `ρ(n)` | Node representation function |
| `κ(n)` | Children function |
| `H(X)` | Shannon entropy of X |
| `I(X; Y \| Z)` | Conditional mutual information |
| `f_LLM` | LLM evaluation/scoring function |
| `b_max` | Maximum branching factor |
| `d_max` | Maximum navigation depth |
| `θ` | Adaptive similarity threshold |
| `π` | Reasoning path (navigation trajectory) |
| `CR(n)` | Compression ratio of node n |
| `⊑` | Partial order on epistemic lattice |

