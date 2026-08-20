# Epistemic Module System (EMS) — Research Paper

> **axon-lang v2.76.0** — Separate Compilation and Linking for Cognitive Programming Languages
> July 2026 · Rust implementation (§Fase 115)

---

## Provenance note (read this first)

The EMS was first designed and implemented in **Python** for axon-lang **v0.23.0**
(March 2026). That implementation — `axon/compiler/{module_resolver, interface_generator,
epistemic_compat, compilation_cache}.py` — was retired together with the entire Python
frontend in **Fase 39** ("Pure Silicon Cognition", v2.0.0), and was **not ported** at the
time. Between v2.0.0 and v2.75.0 the `import` statement parsed and lowered to IR, but
**nothing resolved it**: the paper's own two-file example failed `axon check` with
`Undefined persona 'Expert'`.

**§Fase 115 rebuilt the EMS natively in Rust** — this document describes that
implementation, which ships in `axon-frontend` ≥ 1.56.0 / axon-lang ≥ 2.76.0. It is not
a port: the Rust EMS corrects a load-bearing defect in the Python design (§3.6) and
implements the linking phase the Python version listed as *"(future)"*. Every file path,
diagnostic code, and test cited below exists in the shipping toolchain.

---

## Abstract

This document presents the **Epistemic Module System (EMS)**, axon-lang's solution to
separate compilation, cross-file referencing, and multi-module linking. Unlike
traditional module systems designed for value-level programming, EMS operates on
*cognitive compilation units* — files whose exports are personas, anchors, shields,
flows, tools, and resources rather than functions, types, and values.

EMS synthesizes seven state-of-the-art paradigms from programming language theory into a
unified design that is both theoretically grounded and practically functional:

1. **OCaml ML** — Signatures, functors, first-class modules
2. **1ML (Rossberg 2015)** — Core/module unification via System Fω
3. **Haskell Backpack** — Mixin linking + separate type-checking
4. **GHC `.hi` / OCaml `.cmi`** — Interface files for incremental compilation
5. **Zig** — Lazy discovery + build dependency graphs
6. **Nix / Bazel** — Content-addressed hermetic builds with early cutoff
7. **Rust** — Crates + traits as behavioral contracts

The novel contribution is **Epistemic Compatibility Checking (ECC)** — no existing
module system validates *epistemic guarantees* across import boundaries. EMS ensures
that a `know`-level module cannot silently import `speculate`-level definitions,
propagating the epistemic floor across the entire project dependency graph.

The second contribution — new in the Rust implementation — is the **faithful linker**:
the modules' declarations merge at the *AST tier* (topological order, source-map line
renumbering), the full type checker revalidates the linked program once — so every
deep semantic law the language has applies cross-module *by construction* — and IR
generates once over it, producing a single linked `IRProgram` carrying full
declaration bodies and per-module provenance: exactly the artifact the enterprise
runtime already stores and hydrates. Interfaces are the *per-module validation and
caching* contract; the link is the *semantic and execution* contract. Both, not
either (§3.6).

---

## I. Problem Statement: Why axon-lang Needs a Module System

### 1.1 The Decorative-Import Bottleneck

Before §115, the Rust toolchain accepted this program:

```axon
import axon.security.{Expert, NoHallucination}
```

The lexer tokenized it, the parser produced an `ImportNode`, the IR generator lowered it
to `IRImport { module_path, names }` — and **nothing resolved it**. The type checker
matched `Declaration::Import(_) => {}` in both its registration and validation passes;
imported names never entered the symbol table; `ir.imports` was written once and read by
zero lines of code in either repository. Any cross-file reference required duplicate
inline stubs:

```axon
// axon/security.axon — the canonical source
persona Expert {
  domain: ["medicine", "diagnostics"]
  tone: precise
  confidence_threshold: 0.9
}

anchor NoHallucination {
  require: source_citation
  confidence_floor: 0.75
  on_violation: raise AnchorBreachError
}
```

```axon
// consultation.axon — was forced to duplicate everything
persona Expert {
  domain: ["medicine"]
  tone: precise
}

anchor NoHallucination {
  require: source_citation
  on_violation: raise AnchorBreachError
}
```

### 1.2 Four Concrete Problems

| Problem | Impact |
|---------|--------|
| **DRY Violation** | Every file needing `Expert` must re-declare it. 10 files = 10 copies. |
| **Silent Divergence** | Stubs desynchronize from canonical definitions. The copy above already lost `"diagnostics"` from the domain and the anchor's `confidence_floor`. |
| **Scaling Barrier** | Multi-agent systems with dozens of `.axon` files are impractical. Each file is an island. |
| **Laundered Holes** | The type checker's soft acceptance of unknown types was justified by a comment — *"may come from imports"* — citing a mechanism that did not exist. A named excuse kept a real hole open. |

### 1.3 Why Not a Simple `#include`?

A naive textual inclusion would solve DRY but introduce worse problems:

- **Compilation cost**: Including all transitive dependencies means recompiling everything on any change.
- **Name pollution**: All symbols from included files flood the namespace.
- **Circular dependencies**: No protection against `A includes B includes A`.
- **No semantic boundary**: Cannot validate epistemic compatibility.

This argument is also why EMS **refuses non-selective imports** (§3.8): `import a.b`
without a `{…}` selector is `#include` wearing a module system's clothes.

---

## II. Research Foundation: Seven Paradigms Analyzed

### 2.1 OCaml ML Module System — Signatures, Functors, First-Class Modules

**Source**: Xavier Leroy et al., "The OCaml system" (INRIA); Robert Harper & John Mitchell, "On the type structure of Standard ML" (1993).

OCaml's signatures describe what a module *must* provide without revealing
implementation. **What we take**: *Cognitive Signatures* — interfaces that declare the
epistemic properties a module guarantees (persona domains, anchor constraints, shield
scan categories) without exposing prompt text or step logic.

### 2.2 1ML (Rossberg 2015) — Unification via System Fω

**Source**: Andreas Rossberg, "1ML — Core and modules united" (ICFP 2015).

1ML's thesis: the distinction between "core language" and "module language" is
artificial. **What we take**: axon-lang has NO separate module declaration language. An
imported persona IS a persona. `import axon.security.{NoHallucination}` makes
`NoHallucination` available as if declared locally — no wrappers, no adapters, no
module-level indirection.

### 2.3 Haskell Backpack — Mixin Linking + Separate Type-Checking

**Source**: Scott Kilpatrick et al., "Backpack: Retrofitting Haskell with interfaces" (POPL 2014); Edward Z. Yang, "Backpack to work" (PhD thesis, 2017).

Backpack separates compilation into a wiring phase (which unit provides which
signature) and a checking phase (validate each unit against wired signatures).
**What we take**: two-phase separation — Discovery + Interface generation produce the
wiring; Resolution + Full IR type-check against it.

### 2.4 GHC `.hi` / OCaml `.cmi` — Interface Files

**Source**: GHC User's Guide §4.7 "Recompilation checking"; OCaml Manual §13 "Separate compilation."

GHC's ABI hash enables content-based recompilation avoidance: if the interface hash
hasn't changed, downstream modules skip recompilation. **What we take**: `.axi` (AXON
Interface) files — JSON-serialized cognitive signatures with a dual-hash scheme
(`content_hash` / `interface_hash`) enabling GHC-style early cutoff (§3.4).

### 2.5 Zig — Lazy Discovery + Build DAGs

**Source**: Andrew Kelley, "Zig Language Reference" §29 "Build system."

Zig's compiler builds the dependency graph from `@import` edges without fully compiling
anything until needed. **What we take**: Phase 0 discovers the DAG with a **lexer-level
scan** of `import` statements — tokens only, no AST construction, no type checking. (The
Python EMS used a regex; the Rust EMS uses the real lexer, so discovery can never
disagree with the parser about what an import is.)

### 2.6 Nix / Bazel — Content-Addressed Hermetic Builds

**Source**: Eelco Dolstra, "The Purely Functional Software Deployment Model" (PhD thesis, 2006); Bazel documentation, "Remote caching" (2024).

A build is a pure function from inputs to outputs; artifacts are keyed by content hash;
identical inputs guarantee identical outputs; early cutoff skips downstream rebuilds
when outputs are unchanged. **What we take**: the `CompilationCache` key is
`SHA-256(source) + sorted dependency interface hashes` (§3.7).

### 2.7 Rust — Crates + Traits as Behavioral Contracts

**Source**: The Rust Reference, "Crates and source files"; "The Rust Programming Language" §10 "Traits."

**What we take**: *cognitive behavioral contracts*. An anchor set
(`{NoHallucination, NoBias}`) functions like a trait bound — a module exporting these
anchors certifies compliance with those guarantees, checkable at the import boundary.

---

## III. EMS Architecture: How It Works

### 3.1 Compilation Pipeline (5 Phases + Cache)

```
Phase 0: DISCOVERY    ─── module_resolver.rs ─────────────
         Lexer-level import scan → dependency DAG
         Kahn topological sort · cycle refusal (axon-T955)

Phase 1: INTERFACE    ─── module_interface.rs ────────────
         Each module: lex → renumber lines by base offset
         (the source map) → parse → CognitiveInterface (.axi)
         content_hash · interface_hash · epistemic floor

Phase 2: COMPAT       ─── epistemic_compat.rs ────────────
         ECC across every import edge
         (axon-T954 error · axon-W017 warning)

Phase 3: RESOLVE      ─── type_checker.rs (module mode) ──
         Per-module validation: imported names REGISTER in
         the symbol table with their exported kinds
         (axon-T953 completeness + collision law)
         · skipped on a cache validation hit (§3.7)

Phase 4: LINK         ─── module_linker.rs ───────────────
         The modules' ASTs merge into ONE Program
         (topological order, entry last, dep `run`s dropped)
         → merged revalidation: the full TypeChecker runs
           ONCE over the linked program — every deep law,
           cross-module, by construction
         → ir_generator.rs runs ONCE over the linked program
           (full declaration bodies + module provenance)
         → the deployable artifact

Cache:   compilation_cache.rs — content-addressed validation
         skip, early cutoff, atomic writes, self-healing (§3.7)

Driver:  ems.rs — the one orchestration both production
         surfaces call (CLI entry file · enterprise bundle)
```

All module machinery lives in `axon-frontend` under its zero-runtime-dependency
discipline: SHA-256 is the crate's existing hand-rolled FIPS 180-4 implementation
(`sha256_hex`, §Fase 38 precedent), serialization is the existing `serde_json`, and the
resolver API is **in-memory-first** (a `ModuleSet` maps module paths to sources) so the
LSP and the enterprise loader can resolve without a filesystem. The filesystem walk is
one constructor on top.

### 3.2 Phase 0 — Dependency Discovery (`module_resolver.rs`)

The `ModuleResolver` builds a DAG of module dependencies using a **lexer-level scanner**
(`scan_imports`): it tokenizes the source with the real AXON lexer and extracts
top-level `import` statements without constructing an AST. Discovery can therefore never
recognize a different import grammar than the parser does — the class of drift a regex
scanner invites.

The DAG is topologically sorted with **Kahn's algorithm**; ties are broken by module
path ordering, so the topological order is **deterministic** regardless of discovery
order. If the sorted result does not include all nodes, at least one **cycle** exists
and resolution refuses with `axon-T955`, naming the full cycle path
(`a.b → c.d → a.b`). Cognitive cycles are semantic paradoxes — a persona cannot depend
on an anchor that depends on that persona's definition.

**Module path resolution** (D115.8):

```
import axon.security.{NoHallucination}
                 ↓
<modules-root>/axon/security.axon
```

where `<modules-root>` defaults to the entry file's directory and is overridable via
`--modules-root` / `AXON_MODULES_ROOT`.

### 3.3 Phase 1 — Interface Generation (`module_interface.rs`)

The `InterfaceGenerator` extracts the **public surface** of a compiled module into a
`CognitiveInterface` — the `.axi`. Every top-level *named* declaration is exportable
(D115.3): personas, contexts, anchors, flows, types, tools, shields, resources,
channels, sockets, upstreams, memories, daemons, and the rest of the named surface. The
signature captures what an importer must know to type-check and to trust; it hides
implementation:

| Primitive | Signature captures | Signature hides |
|-----------|-------------------|-----------------|
| **Persona** | name, domain, tone, confidence_threshold | description text |
| **Anchor** | name, constraint_hash, on_violation | require/reject text |
| **Flow** | name, params, output_type, step_count | step bodies, ask text |
| **Shield** | name, scan categories, on_breach | redact rules, strategy |
| **Tool** | name, effects, risk, provider | runtime path, secret ref |
| **Resource** | name, kind, capacity, lifetime | endpoint literal |

#### Dual hashes (the GHC ABI-hash scheme)

1. **`content_hash`** = SHA-256(source bytes) — changes on ANY edit.
2. **`interface_hash`** = SHA-256(canonical `.axi` JSON, hash field excluded) — changes
   only when the PUBLIC surface changes.

Adding a comment to `security.axon` changes `content_hash` but not `interface_hash` —
downstream modules skip recompilation (**early cutoff**, §3.7).

### 3.4 Epistemic Floor Computation

Each module's **epistemic floor** is computed from its content:

```
Rules (highest level wins):
  anchors present             → KNOW (4)
  epistemic block `know`      → KNOW (4)
  shields present             → BELIEVE (3)
  epistemic block `believe`   → BELIEVE (3)
  epistemic block `doubt`     → DOUBT (2)
  epistemic block `speculate` → SPECULATE (1)
  none of the above           → UNSPECIFIED (0)
```

The floor is the **maximum epistemic guarantee** the module can offer. A module with
anchors is inherently `know`-level: anchors enforce factual constraints at breach-error
severity. The `know | believe | doubt | speculate` block grammar is the language's
existing epistemic surface — the floor derives from declarations that already exist, not
from a new annotation an author could forget.

### 3.5 Phase 2 — Epistemic Compatibility (`epistemic_compat.rs`)

The `EpistemicCompatChecker` validates every import edge against the **Epistemic
Compatibility Principle**:

$$\forall\, \text{import}(M_a \leftarrow M_b): \text{floor}(M_b) \geq \text{floor}(M_a)\ \lor\ \text{acknowledged\_downgrade}(M_a, M_b)$$

**Compatibility Matrix** (importer row, imported column):

| Importer ↓ \ Imported → | know | believe | doubt | speculate |
|---|---|---|---|---|
| **know** | ✅ OK | ⚠️ W017 | ⚠️ W017 | ❌ T954 |
| **believe** | ✅ OK | ✅ OK | ⚠️ W017 | ⚠️ W017 |
| **doubt** | ✅ OK | ✅ OK | ✅ OK | ⚠️ W017 |
| **speculate** | ✅ OK | ✅ OK | ✅ OK | ✅ OK |

- **Gap = floor(importer) − floor(imported).** Gap ≥ 3 → error `axon-T954`. Gap 1–2 →
  warning `axon-W017`. Gap ≤ 0 → OK. `UNSPECIFIED` floors are neutral (no edge fires).
- `axon check --strict` escalates W017 to an error — the CI posture.
- **`@allow_downgrade`** on the import statement is the explicit valve:

```axon
import creative.brainstorm.{WildIdeas} @allow_downgrade
```

  It silences W017 and downgrades T954 to W017 — the downgrade still *appears* in
  output; it can be acknowledged, never hidden.

**Why this matters**: a `know`-level medical-diagnosis module that silently imports
`speculate`-level creative personas would execute speculative reasoning where factual
rigor was expected. No linter, test, or traditional type system catches this. ECC
catches it at compile time, and the valve leaves an audit trail instead of a silence.

### 3.6 Phase 3 — Resolution, and the stub defect the Rust EMS corrects

In module mode (`TypeChecker::set_module_context`), every imported name registers in
the symbol table with its exported kind — so all ~71 reference-resolution sites the
checker already has (runs, tools naming resources, ingest targets, …) resolve an
imported symbol exactly like a local one. The `axon-T953` law family enforces:

- the imported **module resolves** to a file in the ModuleSet;
- every imported **name exists** among that module's exports (the message carries the
  module's real export list);
- **no collision** — an imported name may not shadow a local declaration or another
  import (D115.5: no shadowing, ever);
- imports are **selective** (`{…}` required) and **unscoped** (`@scope` is reserved,
  refused with `axon-T953` until a package registry exists — a form that parses but has
  no semantics is refused loudly, per the §111 posture).

The soft-type discipline is **preserved** (D115.6): ad-hoc type names remain accepted,
because that is house idiom — but the excuse comment now cites a mechanism that exists,
and explicitly imported names are checked for real.

**The stub defect, and how the Rust design dissolves it.** The Python EMS injected
signature *stubs* into the IR. A stub of a flow has no steps; a program that ran an
imported flow would have executed an empty shell. That defect was invisible precisely
because nothing downstream ever consumed the resolution. The Rust EMS does not inject
at all: the linker merges the modules' **ASTs** (§3.8), the full type checker
revalidates the linked program once — body-dependent laws (a tool call validated
against its imported tool's parameter schema, a store proof against an imported
store's columns) fire cross-module *by construction*, with no per-primitive injection
matrix to drift — and the IR generator runs once over the linked program.
`resolved_persona`, `resolved_anchors`, and `resolved_flow` on an `IRRun` therefore
receive **full nodes** because the declarations are simply *there*. `IRImport` gains
`resolved: bool` + `interface_hash: Option<String>` — the fields this paper's Python
edition described, now real and serialized.

A consequence worth stating: per-module compilation artifacts never embed foreign
bodies. All cross-module body resolution happens at link time, which is what makes
the cache's early cutoff *sound* — nothing a dependent caches can go stale when a
dependency's body changes (§3.7).

### 3.7 The Compilation Cache (`compilation_cache.rs`)

Content-addressed, Nix/Bazel model:

```
module_key  = SHA-256(source bytes) ⊕ sorted(dependency interface_hashes)
project_key = SHA-256(sorted (module, content_hash) pairs)
```

**What is cached — stated precisely, so the claim can be true.** The unit of
persisted skip is a module's **validation** (the type-check — by far the expensive
pass; parse and IR generation are the cheap, total passes and always re-run, because
IR nodes are Serialize-only by design: consumers re-derive from source). Only CLEAN
validations are ever recorded — a failing module re-validates every run, so
diagnostics always re-emit from source, never from a replay.

**On-disk layout** (beside the entry file):

```
.axon_cache/
  ├── interfaces/     # .axi files, one per module
  └── manifest.json   # schema_version + per-module keys + project entry
```

**Laws (all observable via `CacheStats` — the tests' witness):**

1. Source hash changed → module re-validates.
2. Any dependency's `interface_hash` changed → module re-validates.
3. Both match a recorded clean validation → **validation hit** (skip).
4. A dependency edited without changing its public surface (comment or body-only
   edit) keeps its `interface_hash` → dependents still hit (**early cutoff**) —
   sound because per-module validation consumes only interface facts (§3.6).
5. Writes are atomic (temp file + rename). A corrupted or unreadable cache is not an
   error: it **self-heals** by re-deriving from source (the boot-hydrate doctrine —
   a cache is never the source of truth).
6. `manifest.json` pins `schema_version` + `.axi` format + compiler version; any
   mismatch busts the cache wholesale rather than risking stale-shape reads.
7. The merged revalidation (§3.8) re-runs whenever ANY module changed. It is skipped
   only when every module hit AND the manifest's *project entry* proves this exact
   content set previously passed the merged gate — a prior cross-module failure
   clears the entry, so its error can never vanish behind per-module hits.

### 3.8 Phase 4 — Linking (`module_linker.rs`) — REAL, and the production artifact

The linker merges the modules' **ASTs** into one `Program`, then the full checker and
the IR generator each run once over it:

- **Merge order is the topological order** (dependencies first, entry last); within a
  module, declaration order — the linked program is a **pure function of the module
  set** (deterministic bytes, stable hashes; the enterprise `ir_sha256` dedupe anchor
  relies on exactly this).
- **A dependency's top-level `run` statements do not link** — a library's runs are its
  own demos; only the entry orchestrates execution.
- **Module reachability prunes; declarations do not** (v1): a module nobody imports
  never loads, but every declaration of a reachable module links. There is no
  tree-shaking in v1 — selectivity is enforced at the *reference* level by the
  `axon-T953` import laws, not by pruning. Stated plainly so the artifact's contents
  are never a surprise.
- **Name collisions are refused** — an imported name colliding with a local or another
  import is `axon-T953` at the import site; two linked modules declaring the same
  top-level name collide at the merged registration pass (global uniqueness; there is
  no "last one wins").
- **Source map**: each module's tokens are renumbered into a disjoint virtual line
  window before parsing (the rustc `Span` idea), so every diagnostic of the merged
  revalidation and every IR `source_line` maps back to `(file, local line)` via the
  provenance windows.
- The linked program carries **module provenance** (`IRProgram.modules`): for each
  contributing module, its path, origin, `content_hash`, `interface_hash`, virtual-line
  window and export names — the audit chain from deployed artifact back to sources.

**This is the production story.** The enterprise deploy path
(`POST /api/v1/tenant/flows`) compiles source to a `FlowIr` and stores the IR as the
source of truth; container boot re-hydrates from stored IR without recompiling. §115
extends the deploy surface with a **bundle** body — `{ entry, files: {path → source} }`,
fail-closed limits — that runs this same in-memory EMS and stores the **linked**
program. One artifact shape, single-file or multi-module; the hydrate path is unchanged.

### 3.9 Grammar surface (complete)

```
import a.b                      → axon-T953 (selective import required)
import a.b.{X, Y}               → resolves <root>/a/b.axon, imports X, Y
import a.b.{X} @allow_downgrade → same + ECC valve (§3.5)
import @scope.pkg.{X}           → axon-T953 (reserved for package registry)
import a.b.{X} with apx { … }   → RETRACTED §111 (unchanged; refused loudly)
```

---

## IV. Why It's Functional: Theoretical Guarantees

### 4.1 Soundness

- **Signature monotonicity**: a `CognitiveInterface` only contains what a module
  actually declares; if `NoHallucination` appears in the `.axi`, it was compiled from a
  real `anchor`.
- **No stub execution**: injection uses full IR nodes (§3.6) — the linked program's
  behavior is the behavior of the sources, not of a shell.
- **Backwards compatibility (absolute)**: without a registry, `TypeChecker` and
  `IRGenerator` behave byte-identically to v2.75.0. Single-file compilation is
  untouched; the EMS engages only when an entry file declares imports.

### 4.2 Completeness

- Topological ordering guarantees every dependency's interface and IR exist before any
  dependent compiles — no forward references.
- Every explicitly imported name is verified to exist (`axon-T953`).
- **Honest caveat**: the house *soft-type* discipline (undeclared ad-hoc type names are
  accepted) is deliberately preserved. EMS closes the import hole; it does not flip a
  language-wide idiom. The completeness law is about what you *import*, not about every
  free name.

### 4.3 Termination

- Kahn's algorithm refuses cycles before compilation begins (`axon-T955`).
- The DAG is finite (bounded by the module set); resolution is a single topological
  pass — no fixpoint iteration.

### 4.4 Determinism

- Content-addressed keys; `sorted(interface_hashes)`; BTreeMap-ordered exports;
  topological tie-breaking by module path. Same inputs ⇒ same linked bytes ⇒ same
  hashes — the property the enterprise dedupe anchor (`ir_sha256`) relies on.

---

## V. Test Validation

The §115 suite — all shipping, all against the real pipeline (`ems::compile_module_set`
/ the CLI entry points), no mocks of the EMS by the EMS. Integration files below;
each EMS module additionally carries in-module unit tests (resolver 8 · interfaces 7 ·
ECC 6 · cache 4).

| Test file | Tests | Covers |
|-----------|-------|--------|
| `axon-frontend/tests/module_resolver.rs` | 3 | cycle refusal through the driver (named path), diamond links the shared module once, scan/parser parity over every accepted import form |
| `axon-frontend/tests/interfaces.rs` | 4 | the KIND-PARITY gate (run-family + T950 resource kinds, cross-module kind-mismatch blame), `.axi` persistence + roundtrip + body-hiding |
| `axon-frontend/tests/epistemic_compat.rs` | 3 | T954 fails the compile, `@allow_downgrade` compiles with a VISIBLE warning, gap-1 warns |
| `axon-frontend/tests/registry_typecheck.rs` | 9 | the paper's example compiles; T953 family: missing module, missing export (with export list), local + cross-import collision, selective-required, `@scope` refusal; selectivity law; soft types stay soft |
| `axon-frontend/tests/link_faithful.rs` | 6 | full persona body survives (the anti-stub proof), imported flow steps survive, provenance + `IRImport.resolved`, deterministic bytes, dep-`run` exclusion, cross-module deep check at the merged gate with entry-file line mapping |
| `axon-frontend/tests/cache_laws.rs` | 5 | cold/warm/source-invalidation, early cutoff counted, dep-interface invalidation, corruption self-heal, a failing project never caches its way to green |
| `axon-rs/tests/cli_multifile.rs` | 4 | `check` + `compile` e2e on a real on-disk project (warm cache pass included), T953 exit codes, single-file programs bypass the EMS with zero IR drift |
| enterprise `fase115_h_bundle_deploy.rs` | — | bundle → linked FlowIr stored; limits fail closed; single-source path unchanged |

Every `.axon` snippet in this paper is itself compiled by `axon check` in the docs
verification lane (the published-grammar-must-compile doctrine).

---

## VI. Comparison with Existing Systems

| Feature | OCaml | Haskell | Rust | Zig | **axon-lang EMS** |
|---------|-------|---------|------|-----|------------------|
| Interface files | `.cmi` | `.hi` | — | — | **`.axi`** |
| Content-addressed cache | ✗ | Partial | ✗ | ✗ | **✓ (Nix-style)** |
| Early cutoff | ✗ | ✓ (ABI hash) | ✗ | ✗ | **✓** |
| Lexer-true lazy discovery | ✗ | ✗ | ✗ | ✓ | **✓** |
| Cycle detection | ✓ | ✓ | ✓ | ✓ | **✓ (named cycle path)** |
| Deterministic linked artifact | — | — | ✓ | ✓ | **✓ (+ module provenance)** |
| Epistemic compatibility | ✗ | ✗ | ✗ | ✗ | **✓ (novel)** |
| Zero-breaking-change retrofit | — | ✓ (Backpack) | — | — | **✓** |

---

## VII. File Map

```
axon-frontend/src/
  ├── module_resolver.rs       # Phase 0: lexer scan + DAG + Kahn + cycles (T955)
  ├── module_interface.rs      # Phase 1: .axi signatures + floors + ModuleRegistry
  ├── epistemic_compat.rs      # Phase 2: ECC matrix (T954/W017)
  ├── module_linker.rs         # Phase 4: AST merge + source map + provenance
  ├── compilation_cache.rs     # validation-skip cache + early cutoff + project entry
  ├── ems.rs                   # the driver: Phases 0–4, one orchestration
  ├── type_checker.rs          # module mode: set_module_context (T953)
  ├── ir_generator.rs          # with_import_resolution (IRImport marks)
  ├── ir_nodes.rs              # IRImport.resolved/interface_hash + IRModuleProvenance
  ├── ast.rs                   # declaration_surface (the export surface, exhaustive)
  ├── parser.rs                # @allow_downgrade annotation (§3.5)
  └── checker.rs               # `axon check` EMS lane (file-qualified diagnostics)

axon-rs/src/
  ├── compiler.rs              # `axon compile` EMS lane (linked-IR emission)
  └── runner.rs                # `axon run` EMS lane (linked-IR execution)

tests/  (see §V table)
```

---

## References

1. Leroy, X. (2000). "A modular module system." *Journal of Functional Programming*, 10(3), 269–303.
2. Rossberg, A. (2015). "1ML — Core and modules united." *ACM SIGPLAN Notices*, 50(9), 35–47.
3. Kilpatrick, S., Dreyer, D., Peyton Jones, S., Marlow, S. (2014). "Backpack: Retrofitting Haskell with interfaces." *POPL 2014*.
4. Yang, E. Z. (2017). "Backpack to work: Towards practical mixin linking for Haskell." PhD thesis, Stanford University.
5. Dolstra, E. (2006). "The Purely Functional Software Deployment Model." PhD thesis, Utrecht University.
6. Harper, R., Mitchell, J. C. (1993). "On the type structure of Standard ML." *ACM TOPLAS*, 15(2), 211–252.
7. Kelley, A. (2024). "Zig Language Reference." https://ziglang.org/documentation/
8. The Bazel Authors (2024). "Remote caching." https://bazel.build/remote/caching
9. NIST (2015). "FIPS 180-4: Secure Hash Standard." (the in-crate `sha256_hex` reference)
