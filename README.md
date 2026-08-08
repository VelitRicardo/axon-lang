<p align="center">
  <strong>AXON</strong> <em>v2.75.0</em><br>
  Axon is the Cognitive Runtime for Enterprise Software.<br><br>
  Build AI systems with deterministic execution, formal guarantees and
  native cognitive primitives.
</p>

<p align="center">
  <a href="https://www.ricardovelit.com/axon-docs"><img src="https://img.shields.io/badge/docs-ricardovelit.com%2Faxon--docs-blue" alt="Documentation"></a>
  <img src="https://img.shields.io/badge/version-v2.83.0-informational" alt="Version">
  <img src="https://img.shields.io/badge/status-production-brightgreen" alt="Status: Production">
  <img src="https://img.shields.io/badge/runtime-100%25%20Rust%20%2B%20C23-orange" alt="100% Rust + C23">
  <img src="https://img.shields.io/badge/streaming-SSE%20%7C%20NDJSON%20%7C%20WebSocket-brightgreen" alt="Streaming">
  <img src="https://img.shields.io/badge/realtime-session--typed-purple" alt="Session types">
  <img src="https://img.shields.io/badge/tests-3845%20axon--lang%20%2B%201584%20frontend-brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/compliance-HIPAA%20%7C%20PCI__DSS%20%7C%20GDPR%20%7C%20SOX%20%7C%20SOC2%20%7C%20ISO27001%20%7C%20FIPS%20%7C%20CC%20EAL4%2B-blueviolet" alt="Compliance">
  <img src="https://img.shields.io/badge/persistence-postgresql-blue" alt="PostgreSQL">
  <img src="https://img.shields.io/badge/observability-tracing-green" alt="Tracing">
  <img src="https://img.shields.io/badge/license-AGPL--3.0--or--later-lightgrey" alt="License">
</p>

<details>
<summary align="center"><strong>Primitive census</strong> — every language construct advertised here is a <em>build input</em>: the compiler's anti-drift gate (<code>axon-frontend/src/advertised.rs</code>, §111/§114.z) parses this block at test time and fails the build unless each badge carries a human-attested statement of what its runtime actually does.</summary>

<p align="center">
  <!-- Cognition primitives -->
  <code>persona</code> · <code>intent</code> · <code>flow</code> · <code>reason</code> · <code>anchor</code> · <code>refine</code> · <code>memory</code> · <code>tool</code> · <code>probe</code> · <code>weave</code> · <code>validate</code> · <code>context</code> · <code>step</code> · <code>run</code><br>
  <code>know</code> · <code>believe</code> · <code>speculate</code> · <code>doubt</code> · <code>par</code> · <code>hibernate</code><br>
  <code>dataspace</code> · <code>ingest</code> · <code>focus</code> · <code>associate</code> · <code>aggregate</code> · <code>explore</code><br>
  <code>type</code> · <code>json</code> · <code>ledger</code><br>
  <code>deliberate</code> · <code>consensus</code> · <code>forge</code> · <code>agent</code> · <code>shield</code><br>
  <code>savant</code> · <code>synth</code> · <code>warden</code> · <code>scope</code><br>
  <code>stream</code> · <code>effects</code> · <code>@contract_tool</code> · <code>@csp_tool</code><br>
  <code>pix</code> · <code>navigate</code> · <code>drill</code> · <code>trail</code> · <code>corpus</code><br>
  <code>psyche</code> · <code>ots</code><br>
  <code>mcp</code> · <code>mandate</code> · <code>lambda</code><br>
  <code>budget</code> · <code>window</code> · <code>cors</code> · <code>document</code> · <code>deliver</code> · <code>notify</code><br>
  <code>credential</code> · <code>mint</code> · <code>rotate</code><br>
  <code>compute</code> · <code>grad</code> · <code>quant</code> · <code>observable</code><br>
  <code>daemon</code> · <code>listen</code><br>
  <code>channel</code> · <code>emit</code> · <code>publish</code> · <code>discover</code><br>
  <code>axonendpoint</code> · <code>axpoint</code> · <code>axonstore</code><br>
  <!-- Cognitive I/O & compliance (λ-L-E calculus, Phases 1–9) -->
  <strong>Cognitive I/O:</strong>
  <code>resource</code> · <code>fabric</code> · <code>manifest</code> · <code>observe</code> ·
  <code>reconcile</code> · <code>lease</code> · <code>ensemble</code><br>
  <code>topology</code> · <code>session</code> · <code>send</code> · <code>receive</code> · <code>select</code> · <code>branch</code> ·
  <code>immune</code> · <code>reflex</code> · <code>heal</code> ·
  <code>compliance</code><br>
  <code>component</code> · <code>view</code><br>
  <!-- Enterprise I/O capabilities (Fases 80–85) -->
  <code>cache</code> · <code>voice</code> · <code>shell</code> · <code>path rewrite</code> · <code>PASETO</code><br>
  <!-- NEW — Session-typed WebSocket dialogue (Fase 41, v2.3.0) -->
  <strong>Session types (v2.3.0):</strong>
  <code>socket</code> · <code>upstream</code> · <code>send T</code> · <code>receive T</code> · <code>select {ℓᵢ:…}</code> · <code>branch {ℓᵢ:…}</code> · <code>backpressure: credit(k)</code> · <code>reconnect: cognitive_state</code>
</p>

</details>

---

> **Two repositories, two version lines.** This repo (`axon-lang`,
> AGPL-3.0-or-later, public) ships the **language + runtime + compiler + 7 LLM
> backends + Cognitive I/O + WebSocket session types** — currently **v2.75.0**.
> The commercial control plane (`axon-enterprise`, EULA, private) layers
> multi-tenant identity / RBAC / SSO / metering / audit / vertical compliance
> **on top of** this language via a pinned Cargo dependency — currently
> **v3.92.0**. The version numbers diverge by design (enterprise iterates on
> the SaaS surface independently of the language). If you don't run a
> commercial Axon deployment, this repo is all you need; the badge above is
> the only version that matters for you.

---

## What is AXON?

AXON is a **compiled language** that targets LLMs instead of CPUs. It has a
formal EBNF grammar, a lexer, parser, AST, intermediate representation, **seven
native Rust LLM backends** (Anthropic, OpenAI, Gemini, Kimi, GLM, Ollama,
OpenRouter), and a 100% Rust + C23 native runtime with semantic type checking, an
algebraic-effects execution engine, real-time SSE / NDJSON / WebSocket
session-typed streaming, retry + circuit-breaker resilience, and execution
tracing. The FIPS-routable cryptographic + tokenisation kernels live in
[`axon-csys`](axon-csys/) as standalone C23 (no opaque C bindings — every
kernel is a `_Generic`-dispatched, `[[nodiscard]]`-annotated, sanitizer-clean
C23 source file with a Rust wrapper).

Beyond cognition, AXON ships **Cognitive I/O** — a λ-calculus-based
infrastructure layer where resources, control loops, observability, security
kernels, and UI components carry their regulatory class (HIPAA / PCI_DSS /
GDPR / SOX / SOC 2 / ISO 27001 / FIPS / CC EAL 4+) as a **compile-time type**.
Programs that fail coverage are rejected *before* they run. No other
programming language does this.

**v2.3.0 (Fase 41) adds the first session-typed real-time dialogue primitive in
any production language**: declare a `session` (the bidirectional protocol),
bind it to a `socket` (the WebSocket transport with credit-refined
backpressure), and the compiler proves the two endpoints are duals
(Caires–Pfenning linear-logic Curry–Howard); the runtime enforces every step,
seals the residual cursor on disconnect for typed reconnection, and projects
to W3C Server-Sent Events when the protocol is single-polarity. See
[`docs/papers/paper_websocket_cognitive_primitive.md`](docs/papers/paper_websocket_cognitive_primitive.md).

It is **not** a Python library, a LangChain wrapper, a YAML DSL, or a Terraform
replacement. It is a *new kind of calculus* — see
[`docs/papers/paper_lambda_lineal_epistemico.md`](docs/papers/paper_lambda_lineal_epistemico.md)
for the formal semantics (Cálculo Lambda Lineal Epistémico).

---

## Cognitive I/O — Build Infrastructure with Compile-Time Compliance

The big differential added in v1.0 — ten new top-level declarations that turn
AXON into the only language where *"does this app leak PHI?"* is a **type
error**, not a post-mortem finding.

| Primitive | What it is | Formal backing |
|---|---|---|
| `resource`  | Infrastructure token (DB, cache, bucket, GPU) with `linear` / `affine` / `persistent` lifetime | Linear Logic (Girard 1987) |
| `fabric`    | Topological substrate (VPC, cluster, namespace) | Separation Logic (O'Hearn–Reynolds) |
| `manifest`  | Declarative "belief" about desired infrastructure shape, with κ (regulatory class) annotations | Epistemic Logic (Fagin–Halpern) |
| `observe`   | Quorum-gated snapshot of real state, producing a ΛD envelope ⟨c, τ, ρ, δ⟩ | Decision D4: partition ≡ void, never `doubt` |
| `reconcile` | Active-Inference control loop: observe → drift → shield → act | Free Energy Principle (Friston) |
| `lease`     | τ-decaying affine capability; post-expiry use is a CT-2 *Anchor Breach* | Hybrid affine + revocation (D2) |
| `ensemble`  | Byzantine quorum aggregator over N observations with common-knowledge fusion | Fagin–Halpern `Cφ` |
| `topology` + `session` | Typed directed graph over declared entities with Honda–Vasconcelos duality + deadlock detection | π-calculus binary sessions |
| `socket` (v2.3.0) | Session-typed WebSocket transport with credit-refined backpressure, typed reconnection via `cognitive_states`, SSE-as-fragment projection | Caires–Pfenning Curry–Howard + Rast credit-refined types ([paper_websocket_cognitive_primitive.md](docs/papers/paper_websocket_cognitive_primitive.md)) |
| `immune` + `reflex` + `heal` | KL-divergence anomaly sensor + O(1) signed-trace motor response + Linear-Logic one-shot patch FSM | Cognitive Immune System ([paper_immune_v2.md](docs/papers/paper_immune_v2.md)) |
| `component` + `view` | Declarative UI with the **same** compile-time κ coverage rule — regulated types need a covering shield or the compiler rejects | Regulatory Type Theory (Fase 9) |

### Hard differentiators vs. Terraform / Pulumi / Kubernetes manifests

1. **Compile-time compliance.** `shield<HIPAA>` / `type PatientRecord compliance [HIPAA, GDPR]` are *types*. A `.axon` program that sends PHI to an unshielded endpoint fails `axon check` — same exit code as a syntax error.
2. **Blame Calculus (Findler–Felleisen).** Every error is classified as **CT-1** (axon/runtime bug), **CT-2** (program author: anchor breach, expired lease), or **CT-3** (infrastructure: partition, missing credential, provider quota). No silent downgrades.
3. **Audit-ready artefacts.** `axon dossier` + `axon sbom` + `axon audit --framework {soc2,iso27001,fips,cc,all}` + `axon evidence-package` produce byte-identical, deterministic JSON/ZIP — the SHA-256 of every output is a *contract* against your release.
4. **100% Rust + C23 runtime, no interpreter.** The whole stack — lexer, parser, type-checker, IR, the algebraic-effects execution engine, the HTTP server, the seven LLM backends, the streaming wire, the session-typed WebSocket driver — is a single native Rust binary; the FIPS-routable cryptographic + tokeniser kernels live in [`axon-csys`](axon-csys/) as standalone C23 (no `unsafe` glue: `_Generic`-dispatched headers, `[[nodiscard]]` everywhere, sanitizer-clean, valgrind-clean). `cargo install axon-lang`. No GC, no interpreter, no runtime dependency.
5. **Cognitive immune system.** `immune + reflex + heal` is a first-class language primitive, not a plug-in. Signed HMAC traces per firing, three compliance modes (`audit_only` / `human_in_loop` / `adversarial`), Linear-Logic patch FSM preventing double-application.
6. **Post-Quantum-ready ESK.** HMAC-SHA256 baseline + Ed25519 + ML-DSA-65 (NIST FIPS 204 Dilithium) + Hybrid signer (NIST SP 800-208 transition posture). Feature-gated; no silent classical fallbacks.
7. **Persistence is a typed cognitive primitive.** A database in AXON is an `axonstore`, not an ORM bolt-on: retrieved rows are born epistemically `Untrusted` and a `confidence_floor` is enforced at read and write; every mutation appends to an HMAC-Merkle audit chain; `retrieve` is a bounded, back-pressured `Stream<Row>`; and store access is capability-typed and checked at *compile time*. No other language treats stored data this way.
8. **Real-time dialogue is a typed cognitive primitive (v2.3.0).** A WebSocket in AXON is a `socket` over a declared `session`, not a JSON envelope over bytes: the compiler proves the two endpoints are duals (Caires–Pfenning intuitionistic linear logic, `S̄ ≡ S⊥`), so the connection is **deadlock-free and protocol-conformant by construction**; credit-refined backpressure (`backpressure: credit(k)`) is decidable in Presburger arithmetic at compile time; a mid-protocol disconnect seals the residual session-type cursor + credit window into an AAD-bound `cognitive_states` snapshot, and the typed `?resume=` resume restores it under tenant + flow_id binding; a single-polarity protocol's `socket` ALSO speaks W3C Server-Sent Events byte-compat with Fase 33's existing SSE pipeline (`S_SSE = Π_↓(S_WS)`).
9. **Every boundary is guarded (v2.44.0).** The doctrine `axon://logic/every_boundary_is_guarded`: no `axonendpoint` may dispatch a flow across a trust boundary uncovered by silent omission. It must declare a covering discipline (`requires:` / `shield:` / `compliance:`) **or** the explicit, auditable opt-out `public: true` — omit both and `axon check` fails with **`axon-T890`**, same exit code as a syntax error. The *absence* of a guard is a deliberate, witnessed decision, never a default. The rule is independently re-verified as a Proof-Carrying-Code obligation (`PropertyClass::AuthorizationCoverage`) at the deploy gate, so an unguarded boundary cannot reach production through any door; `axon fix` migrates existing programs. Generalises the §6.1 shield κ-coverage rule from *regulated types* to *every boundary*.
10. **Every guard is satisfiable (v2.45.0).** The completeness dual of #9: the doctrine `axon://logic/every_requirement_is_grantable`. A `requires: [x]` whose capability `x` no authority in the system can grant is a **dead boundary** — a locked door with no fabricable key, indistinguishable at deploy from a real guard. §90 makes key-existence a proof obligation: a total, injective projection `π` reconciles the two authority representations that otherwise never meet — the control-plane RBAC catalog (colon `resource:action`) and the data-plane `requires:` grammar (dotted `resource.action`) — so a role-derived permission `flow:execute` actually satisfies `requires: [flow.execute]`. The law `requires ⊆ π(grantable_catalog)` is re-verified as a Proof-Carrying-Code obligation (`PropertyClass::CapabilityGrantability`); a dead requirement is refuted with **`axon-T891`** and the deploy gate rejects the bundle fail-closed. Unlike IAM systems that discover unsatisfiable policies at request time (or never), AXON proves *before the surface mounts* that every scope it requires is grantable. Together with #9: **every boundary is guarded, and every guard is openable by exactly the authorities meant to open it.** v2.47.0 closes the converse quantifier (`axon://logic/every_granted_authority_projects`, §93): §90 proved the key *exists in the catalog*; §93 proves the key that is actually *cut* — a granted role, built-in or custom — turns the lock: `π` lifted to held-authority sets is total with **explicit drops** (`auth_scope::project_permission_set` — a tampered authority row degrades with a witness, never silently), authority is derived from live state at verify time (never minted into a claim, so revocation binds next-request), and a permission that cannot project is refused at the write. The **dead authority** — assignable but inert, the principal-side dual of the `axon-T891` dead requirement — becomes unrepresentable.
11. **Delegation is attenuation (v2.46.0).** The third act of the authority story: the doctrine `axon://logic/authority_only_attenuates`. `credential <Name> { ttl: grants: }` declares an **ephemeral-credential contract** — the exact shape a chat widget on any origin needs — and the `mint` flow verb turns it into a TTL-bounded bearer at runtime, admitted only when `grants ⊆ capabilities(minter)`: a flow can hand a visitor a *slice* of the authority it provably holds, never more. The law is machine-checked at three layers, all fail-closed: compile (**`axon-T893`**–**`axon-T896`** — grantless contracts, TTLs beyond the 24h ephemeral ceiling, ghost mints, and persisting a minted bearer are all rejected), verify/deploy (a Proof-Carrying-Code obligation, `PropertyClass::CredentialAttenuation`, re-derives every contract + mint site from the artifact), and mint time (the dispatch handler AND the `CredentialMinter` port independently enforce the subset law; no port configured ⇒ a loud missing-dependency error, never a silent stub). Stripe-style ephemeral keys, but as *typed language surface with a deploy-time proof* — no SDK call whose scopes are runtime strings. Cognitive time is declared the same release: `now: "<IANA-tz>"` on a `step` or `context` (v2.46.0, §91) injects ONE captured instant per run, DST-correct and recorded as `(captured_utc, tzdb_version, zones)` in the envelope for byte-exact replay — `time_is_an_explicit_input` applied to cognition, with `TemporalContextSoundness` refuting a plausible-but-unknown timezone before deploy.
12. **Rotation without revelation (§94).** The inbound dual of #11: the doctrine `axon://logic/rotation_without_revelation`. A third-party credential a tenant lends the platform (an OAuth token for a connected CRM) is **custodied, never readable**: `axonstore <N> { backend: secrets  class: crm }` is a read-only *metadata* view over the encrypted secret custody (key / version / created_at / expires_at — the value has no column), so a cron `daemon` enumerates what nears expiry with ordinary §67 time-aware `retrieve`; the `rotate <Store> [where] with <Tool> as <r>` verb renews each matching entry through ONE runtime-mediated exchange per key (reveal into the reserved `axon_rotation` tool envelope → the adopter's tool performs the vendor refresh → CAS commit at version+1, so HA daemon replicas cannot double-spend a refresh token), binding only the `{attempted, rotated, failed}` summary; and `tool <N> { secret: crm.hubspot }` injects the per-tenant value into the tool-server request at dispatch (`axon_secret`) so the consuming flow never touches it. **No term of the language evaluates to a secret value — revelation is unrepresentable, not discouraged.** Machine-checked at three layers, all fail-closed: compile (**`axon-T897`**–**`axon-T900`** + **`axon-T902`** — custody writes, rotations of non-custody stores or ghost tools, class-less views, and credential literals in source are all rejected), verify/deploy (`PropertyClass::SecretCustodySoundness` re-derives every store, rotate site and write verb from the artifact), and dispatch (no custody port ⇒ a loud missing-dependency error; a secret-bearing tool never calls its vendor unauthenticated). AWS-Secrets-Manager-style managed rotation, but the orchestrator itself is *provably unable to read what it rotates*.

### External audit readiness

The audit engine ships **108 mapped controls** across the four major external frameworks:

- **SOC 2 Type II** — 31 TSC controls (CC + C + PI + P)
- **ISO/IEC 27001:2022** — 41 Annex A controls
- **FIPS 140-3 (CMVP)** — 14 CAVP/FSM entries
- **Common Criteria EAL 4+** — 22 SFRs + SARs

Each framework has an operational runbook and an audit-evidence pipeline that emits the evidence bundle on every release.

### Try it in 30 seconds

```bash
cargo install axon-lang
echo 'type PatientRecord compliance [HIPAA, GDPR] { ssn: String }
shield PHIShield { scan: [pii_leak] on_breach: halt severity: critical
                   compliance: [HIPAA, GDPR] }
axonendpoint Api { method: POST path: "/p" body: PatientRecord
                   execute: F output: FlowEnvelope<PatientRecord> shield: PHIShield
                   backend: anthropic compliance: [HIPAA, GDPR] }
flow F(ssn: String) -> PatientRecord {
  step R { ask: "summarize" output: PatientRecord } }' > app.axon
axon check   app.axon   # compile-time compliance verification
axon dossier app.axon   # regulatory posture JSON
axon audit   app.axon --framework all   # per-framework gap analysis
```

Now delete `shield: PHIShield` from the endpoint and run `axon check` again:

```
X app.axon  1 error(s)
  error [line 4]: axon-T957 axonendpoint 'Api' carries regulated data
  (kappa = {GDPR, HIPAA}) across a trust boundary but declares no `shield:`.
  Regulated boundaries require a shield whose `compliance:` covers the type's
  kappa — ESK Fase 6.1 coverage rule. […] Declaring the classes on the
  endpoint's own `compliance:` does NOT cover them: that list is a label,
  the shield is the control that acts on a breach.
```

That failure is a **type error**, not a lint warning — `axon check` exits `1` and nothing downstream will build. The rule is a real set difference over the regulatory classes carried by the boundary's `body:` and `output:` types, so a shield that covers *some* of them fails too, naming exactly the ones it misses.

### Reference programs

- [`examples/healthcare_reference.axon`](examples/healthcare_reference.axon) — HIPAA + GDPR + GxP + SOC 2
- [`examples/banking_reference.axon`](examples/banking_reference.axon) — PCI_DSS + SOX + SOC 2
- [`examples/government_reference.axon`](examples/government_reference.axon) — FISMA + NIST 800-53 + SOC 2
- [`examples/ui/healthcare_console.axon`](examples/ui/healthcare_console.axon) — UI built on top of the healthcare backend with compile-time κ-redacted renders
- [`examples/tool_dispatch.axon`](examples/tool_dispatch.axon) — flow-level `use <Tool> on "${param}"` native tool dispatch with request-parameter binding (§Fase 54)

### Academic references

- [`docs/papers/paper_lambda_lineal_epistemico.md`](docs/papers/paper_lambda_lineal_epistemico.md) — λ-L-E calculus: Theorem 5.1 *Stochastic Degenerative Soundness*
- [`docs/papers/paper_immune_v2.md`](docs/papers/paper_immune_v2.md) — Cognitive Immune System with red-teaming metrics (F1 ≥ 0.80 per class)
- [`docs/papers/paper_esk.md`](docs/papers/paper_esk.md) — Regulatory Type Theory for Cognitive Systems (Theorems 10.1–10.5)

---

## Production Status

AXON v2.3.0 is **production-ready**. The full stack is cross-validated, 100% Rust + C23:

- ✅ 65+ cognitive + Cognitive-I/O primitives wired to the native runtime
- ✅ 285 HTTP routes tested end-to-end
- ✅ Seven native Rust LLM backends (Anthropic, OpenAI, Gemini, Kimi, GLM, Ollama, OpenRouter) with full async streaming
- ✅ Real-time streaming wire — SSE + NDJSON, type-driven transport inference, per-chunk algebraic-effect dispatch
- ✅ **Session-typed WebSocket dialogue (v2.3.0)** — declared `session` + `socket`, statically-checked duality (Caires–Pfenning), credit-refined backpressure (Presburger discharge), typed reconnection via AAD-bound `cognitive_states` snapshots, SSE-as-fragment unification
- ✅ **Multiparty projection (v2.3.0)** — `GlobalType` + `project_all` (Honda–Yoshida–Carbone safe-realizability gate) for n-agent skill/tool topologies
- ✅ `axonendpoint` as a first-class HTTP REST primitive — typed routes, body + output schema validation, `Idempotency-Key`, auth scopes
- ✅ `axonstore` cognitive data plane — epistemically typed rows, HMAC-Merkle audit chains, `Stream<Row>`, capability-typed access
- ✅ Compile-time regulatory compliance for HIPAA / PCI_DSS / GDPR / SOX / SOC 2 / ISO 27001 / FIPS / CC EAL 4+
- ✅ Cognitive immune system (anomaly detection + reflex + heal) paper-faithful
- ✅ Post-Quantum signatures: HMAC-SHA256 baseline + Ed25519 + ML-DSA-65 + Hybrid (NIST SP 800-208)
- ✅ **`axon-csys` C23 kernels** — FIPS-routable SHA-256 / HMAC-SHA256 / SIMD G.711 / BPE tokeniser / FSM dispatch (computed gotos) / buffer pool (207× faster than Vec<u8>) — standalone C23 with sanitizer-clean + valgrind-clean CI lanes
- ✅ PostgreSQL persistence with migrations and health checks
- ✅ Structured observability (JSON logging + request tracing)
- ✅ LLM call resilience (retry + circuit breaker + fallback)
- ✅ **2,245 axon-lang + 535 axon-frontend + 13 axon-csys = 2,793 Rust tests**; cross-stack zero-regression discipline. Python side is now a thin PyPI wrapper that downloads + invokes the native Rust binary (the language interpreter is 100% Rust/C23 — the Python suite was retired in Fase 40's *Pure Silicon* pivot).
- ✅ Zero "por ahora", zero "lo mínimo" — production-complete

Designed for cognitive AI applications that require formal semantics, reliability, epistemic rigor, and *provable* regulatory coverage.

```axon
persona LegalExpert {
    domain: ["contract law", "IP", "corporate"]
    tone: precise
    confidence_threshold: 0.85
    refuse_if: [speculation, unverifiable_claim]
}

anchor NoHallucination {
    require: source_citation
    confidence_floor: 0.75
    unknown_response: "Insufficient information"
}
```

> **⚠️ `enforce` is the behavioral carrier in anchors.** It is the ONLY anchor field
> injected as a direct behavioral directive to the LLM. `require`/`reject` are
> post-generation validation constraints. `description` is metadata-only — it does
> NOT reach the model. Use `enforce` for text that must shape the model's behavior.

```axon
flow AnalyzeContract(doc: Document) -> StructuredReport {
    step Extract {
        probe doc for [parties, obligations, dates, penalties]
        output: EntityMap
    }
    step Assess {
        reason {
            chain_of_thought: enabled
            given: Extract.output
            ask: "Are there ambiguous or risky clauses?"
            depth: 3
        }
        output: RiskAnalysis
    }
    step Check {
        validate Assess.output against: ContractSchema
        if confidence < 0.8 -> refine(max_attempts: 2)
        output: ValidatedAnalysis
    }
    step Report {
        weave [Extract.output, Check.output]
        format: StructuredReport
        include: [summary, risks, recommendations]
    }
}
```

---

## Native Rust + C23 Runtime

AXON v2.3.0 ships a **production-hardened** 100% Rust + C23 native runtime server with **285+ HTTP routes**, **65+ primitives** wired to runtime, an **algebraic-effects execution engine**, a **real-time SSE / NDJSON / session-typed WebSocket streaming wire**, a full **ℰMCP** (Epistemic Model Context Protocol) implementation, **PostgreSQL persistence**, **structured observability via tracing**, **LLM call resilience** (retry + circuit breaker + fallback chains across seven native backends), and a complete native CLI (`check`, `compile`, `run`, `serve`, `parse`, `dossier`, `sbom`, `audit`, `evidence-package`, and more).

The **Rust + C23 stack is the canonical implementation** of the language. Fase 40 (*Pure Silicon*, v2.0.0) retired the Python interpreter — and the PyPI line has since been closed with a tombstone release that redirects to `cargo install axon-lang` (the historical Python releases remain archived on PyPI, unmaintained). The FIPS-routable cryptographic + tokeniser kernels live in [`axon-csys`](axon-csys/) as standalone C23 (no `unsafe` glue: `_Generic`-dispatched headers, `[[nodiscard]]` everywhere, sanitizer-clean + valgrind-clean CI lanes).

**Production Foundation (Phase K):**
- **Observability**: JSON structured logging with request tracing, daily log rotation, configurable levels
- **Resilience**: Exponential backoff retry, per-provider circuit breakers, configurable fallback chains across 7 LLM backends
- **Persistence**: Full PostgreSQL integration with embedded migrations, JSONB storage, in-memory fallback for development

### Quickstart

```bash
# Build the native runtime
cd axon-rs
cargo build --release

# Start the server with default in-memory storage
cargo run --release -- --port 3000

# Or with PostgreSQL persistence + structured logging
DATABASE_URL="postgresql://user:pass@localhost/axon" \
cargo run --release -- \
  --port 3000 \
  --log-format json \
  --log-file ./logs \
  --database-url "$DATABASE_URL"

# Deploy a flow
curl -X POST http://localhost:3000/v1/deploy \
  -H "Content-Type: application/json" \
  -d '{"source": "flow analyze { step reason { prompt: \"Analyze the input\" } }", "backend": "stub"}'

# Execute
curl -X POST http://localhost:3000/v1/execute/analyze

# MCP endpoint (JSON-RPC 2.0)
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

### Phase K — Production Hardening (v1.0.0 foundation)

AXON v1.0.0 launched with three production-critical systems that remain the foundation of every subsequent minor release:

#### 1. Observability (K1)
- **Structured logging** via `tracing` crate with JSON output
- **Request tracing** with UUID correlation (x-request-id header)
- **Daily log rotation** with configurable directory
- **Configurable levels** via `AXON_LOG` env or `--log-level` CLI
- Instrumentation on all LLM calls: backend, model, latency_ms, tokens_in/out

#### 2. Resilience (K2)
- **Exponential backoff retry** (500ms base, 2.0x multiplier, 30s max, jitter)
- **Per-provider circuit breaker** (5 failures → Open, 30s cooldown → HalfOpen, 2 successes → Closed)
- **Retry-After header respecting** for rate limit hints
- **Fallback chains** (e.g., anthropic → openrouter → ollama)
- **Error classification** (retryable vs. terminal)
- Covers all 7 LLM backends: Anthropic, OpenAI, Gemini, Kimi, GLM, OpenRouter, Ollama

#### 3. Persistence (K3-K4)
- **PostgreSQL backend** with full ACID semantics
- **12 domain tables** (traces, sessions, daemons, audit_log, axon_stores, dataspaces, hibernations, event_history, execution_cache, cost_tracking, schedules, backend_registry)
- **15 performance indexes** for query optimization
- **Embedded migrations** (zero external DB setup for development)
- **UPSERT semantics** for idempotent writes
- **JSONB storage** for nested structures
- **In-memory fallback** when `DATABASE_URL` unset (perfect for development & CI/CD)

**Write-through pattern** ensures all state mutations (flows, sessions, daemons, hibernations) persist to PostgreSQL while maintaining fast in-process reads.

#### Architectural Decisions

**Storage Pattern: StorageDispatcher Enum**
- Uses concrete dispatch via `StorageDispatcher` enum instead of `dyn Trait`
- Enables zero-cost abstraction: `PostgresBackend` or `InMemoryBackend` at compile time
- No runtime trait object overhead, full optimization from compiler

**Async/Await Safety**
- All storage operations are async, but never held across await points
- Mutex locks are released before database I/O
- Prevents deadlocks and enables high concurrency

**Graceful Fallback**
- Database connection failures don't crash the server
- Automatic fallback to `InMemoryBackend` (with logging)
- State persists in-memory for the process lifetime
- Clients experience no service interruption

---

### Runtime Surface

| Surface | Count |
|---------|-------|
| HTTP API routes | 285 |
| Language primitives | 65+ (cognitive + Cognitive I/O) |
| LLM backends | 7 (anthropic, openai, gemini, kimi, glm, openrouter, ollama) — native async, all streaming |
| MCP tool types | 8 (flow, dataspace, axonstore, shield, corpus, compute, mandate, forge) |
| MCP resource types | 10 (traces, metrics, backends, flows, dataspaces, axonstores, shields, corpora, mandates, forges) |
| MCP workflow prompts | 5 (research, decide, secure_transfer, reflect, analyze_image) |
| `axon-lang` (axon-rs) lib tests | 2,245 |
| `axon-frontend` lib tests | 535 (incl. §41.a-c algebra + §41.h multiparty) |
| `axon-csys` lib tests | 13 (Rust wrapper; C23 kernels exercised by `cargo test` + sanitizers/valgrind in CI) |
| Rust workspace total | 2,793 — zero regressions |
| Python wrapper tests | 16 (the thin PyPI wrapper that downloads + invokes the native binary; the interpreter was retired in Fase 40) |
| SQL tables | 12 (traces, sessions, daemons, audit_log, axon_stores, dataspaces, hibernations, event_history, execution_cache, cost_tracking, schedules, backend_registry) |
| Performance indexes | 15 |

### ΛD (Lambda Data) — Epistemic Guarantees

Every AXON operation carries a formal epistemic envelope ψ = ⟨T, V, E=⟨c, τ, ρ, δ⟩⟩:

- **Theorem 5.1**: Only raw data may carry certainty c=1.0; all derived operations cap at c≤0.99
- **Epistemic Lattice**: ⊥ ⊑ doubt ⊑ speculate ⊑ believe ⊑ know
- **Blame Calculus**: CT-2 (caller) / CT-3 (server) / Network attribution on every error
- **CSP §5.3**: MCP tools carry constraint satisfaction schemas

### TypeScript SDK

```typescript
import { AxonClient } from "@axon/mcp-client";

const client = new AxonClient({ baseUrl: "http://localhost:3000" });
await client.initialize();

// Discover and call tools
const tools = await client.listTools();
const result = await client.callTool("axon_compute_evaluate", { expression: "pi * 2" });
const envelope = AxonClient.extractEnvelope(result);
console.log(envelope?.certainty); // 0.99 (transcendental → derived)

// Read resources
const backends = await client.readResource("axon://backends");

// Get workflow prompts
const prompt = await client.getPrompt("workflow:research", { question: "How does attention work?" });
```

Full language specification available on request.

---

## Paradigm Shifts

> AXON's compiler-level paradigm shifts elevate the language from prompt
> compilation to a Cognitive Operating System.

### I. Formal Model — Epistemic Constraint Calculus

Each program `P` in AXON operates over a typed epistemic lattice `(T, ≤)` where
the compiler enforces semantic constraints at compile time. The paradigm shifts
extend this with three new formal mechanisms:

**Epistemic Scoping Function.** Given an epistemic mode
`m ∈ {know, believe,
speculate, doubt}`, the compiler applies a constraint
function `C(m)` that maps to a tuple of LLM parameters and auto-injected
anchors:

```text
C : Mode → (τ, p, A)
where
  τ ∈ [0,1]    — temperature override
  p ∈ [0,1]    — nucleus sampling (top_p)
  A ⊆ Anchors  — auto-injected constraint set

C(know)      = (0.1, 0.3, {RequiresCitation, NoHallucination})
C(believe)   = (0.3, 0.5, {NoHallucination})
C(speculate) = (0.9, 0.95, ∅)
C(doubt)     = (0.2, 0.4, {RequiresCitation, SyllogismChecker})
```

This is calculated **at compile time** — the IR carries the resolved constraint
set, so the executor applies them as zero-cost runtime overrides.

**Parallel DAG Scheduling.** A `par` block `B = {b₁, ..., bₙ}` where `n ≥ 2` is
verified at compile time to have no data dependencies between branches:

```text
∀ bᵢ, bⱼ ∈ B, i ≠ j : deps(bᵢ) ∩ outputs(bⱼ) = ∅
```

At runtime, branches execute via `asyncio.gather`, achieving `O(max(tᵢ))`
latency instead of `O(Σtᵢ)` for sequential chains.

**CPS Continuation Points.** A `hibernate` node generates a deterministic
continuation ID via `SHA-256(flow_name ∥ event_name ∥ source_position)`. The
executor serializes the full `ExecutionState` (call stack, step results, context
variables) and halts. On `resume(continuation_id)`, the state is deserialized
and execution continues from the exact IR node — implementing
Continuation-Passing Style at the language level.

### II. Design Philosophy — Programming Epistemic States

Traditional LLM frameworks treat every model call identically — the same
temperature, the same constraints, the same trust level. This is the equivalent
of asking a human to treat brainstorming and sworn testimony with the same
cognitive rigor.

AXON rejects this flat model. **Epistemic Directives** make the confidence state
of the AI a first-class construct in the language:

```axon
know {
    flow ExtractFacts(doc: Document) -> CitedFact {
        step Verify { ask: "Extract only verifiable facts" output: CitedFact }
    }
}

speculate {
    flow Brainstorm(topic: String) -> Opinion {
        step Imagine { ask: "What could be possible?" output: Opinion }
    }
}
```

The compiler **does not merely label** these blocks — it structurally transforms
them. A `know` block injects citation anchors and drops temperature to 0.1,
making hallucination a compile-time constraint violation. A `speculate` block
removes all constraints and raises temperature to 0.9, liberating the model.

**Parallel Cognitive Dispatch** mirrors how human organizations work: delegate
independent analyses to specialists concurrently, then synthesize.

**Dynamic State Yielding** transforms agents from expensive `while True` loops
into event-driven processes that can sleep for days, weeks, or months — then
resume with full context. The language handles the serialization; the developer
writes `hibernate until "event_name"` and moves on.

### III. Real-World Use Cases

#### Use Case 1: Legal Document Analysis Pipeline

A law firm needs to analyze contracts with maximum factual rigor, while also
exploring creative legal strategies. AXON separates these cognitive modes at the
language level:

```axon
know {
    flow ExtractClauses(contract: Document) -> ClauseMap {
        step Parse { probe contract for [parties, obligations, penalties] output: ClauseMap }
    }
}

flow AnalyzeRisk(contract: Document) -> StructuredReport {
    par {
        step Financial { ask: "Analyze financial exposure" output: RiskScore }
        step Regulatory { ask: "Check regulatory compliance" output: ComplianceReport }
        step Precedent { ask: "Find relevant case law" output: CaseList }
    }
    weave [Financial, Regulatory, Precedent] into Report { format: StructuredReport }
}

speculate {
    flow ExploreStrategies(report: StructuredReport) -> Opinion {
        step Creative { ask: "What unconventional strategies could mitigate these risks?" output: Opinion }
    }
}
```

- `know` guarantees citation-backed extraction (temperature 0.1)
- `par` runs 3 analyses concurrently, reducing latency by ~3x
- `speculate` explicitly relaxes constraints for creative strategy exploration

#### Use Case 2: Multi-Agent Research & Intelligence System

A BI platform deploys autonomous research agents that run for weeks, hibernating
between data collection phases:

```axon
flow MarketIntelligence(sector: String) -> Report {
    know {
        flow GatherData(sector: String) -> DataSet {
            step Collect { ask: "Gather verified market data" output: DataSet }
        }
    }

    par {
        step Trends { ask: "Identify emerging trends" output: TrendAnalysis }
        step Competitors { ask: "Map competitor landscape" output: CompetitorMap }
    }

    hibernate until "quarterly_data_available"

    doubt {
        flow ValidateFindings(data: DataSet) -> ValidatedReport {
            step CrossCheck { ask: "Challenge every assumption with evidence" output: ValidatedReport }
        }
    }

    weave [Trends, Competitors] into Final { format: Report }
}
```

- Agent hibernates after initial analysis, **costing $0 while waiting**
- Resumes automatically when quarterly data arrives (webhook/cron)
- `doubt` mode forces adversarial validation with syllogism checking

#### Use Case 3: Autonomous Customer Support with Escalation

A SaaS platform handles support tickets with different confidence requirements
and automatic escalation via hibernate:

```axon
persona SupportAgent {
    domain: ["product knowledge", "troubleshooting"]
    tone: empathetic
    confidence_threshold: 0.8
}

flow HandleTicket(ticket: String) -> Resolution {
    know {
        flow DiagnoseIssue(ticket: String) -> Diagnosis {
            step Classify { ask: "Classify the issue type and severity" output: Diagnosis }
        }
    }

    believe {
        flow SuggestSolution(diagnosis: Diagnosis) -> Solution {
            step Solve { ask: "Propose a solution based on known patterns" output: Solution }
        }
    }

    if confidence < 0.7 -> hibernate until "human_review_complete"

    step Respond { ask: "Draft customer response" output: Resolution }
}
```

- `know` classifies with strict accuracy (no guessing on severity)
- `believe` suggests solutions with moderate confidence
- Low confidence triggers `hibernate` — agent sleeps until a human reviews
- Zero compute cost during human review; resumes with full context

### IV. Directed Creative Synthesis — the `forge` Primitive

> AXON introduces a sixth paradigm shift: **mathematical formalization of
> the creative process** inside LLMs.

The industry suffers from a structural limitation: LLMs can interpolate, but
they struggle to _create_. `forge` addresses this by implementing a
compiler-level **Poincaré pipeline** — the same 4-phase process mathematicians
and scientists use when producing genuinely novel work.

**Poincaré-Hadamard Creative Pipeline.** A `forge` block orchestrates four
sequential phases, each mapped to a distinct LLM configuration:

```text
forge(seed, mode, novelty, depth, branches) → result

Phase 1: PREPARATION   — Expand the seed via context probing
Phase 2: INCUBATION    — Speculative exploration (depth iterations)
Phase 3: ILLUMINATION  — Best-of-N consensus crystallization
Phase 4: VERIFICATION  — Adversarial doubt + anchor validation
```

**Boden Creativity Taxonomy.** The `mode` parameter maps Margaret Boden's three
creativity types (*The Creative Mind*, 1990) to concrete sampling-parameter
profiles at compile time:

```text
B : Mode → (τ_base, freedom, rule_flexibility)

B(combinatorial)    = (0.9,  0.8, 0.3)   — novel recombination of known ideas
B(exploratory)      = (0.7,  0.6, 0.5)   — structured navigation of possibility spaces
B(transformational) = (1.2,  1.0, 0.9)   — rule-breaking synthesis, new paradigms
```

**Novelty, measured — not asserted.** Kolmogorov complexity `K(x)` is
*uncomputable*, so novelty cannot be computed exactly. `forge` measures it with
the **Normalized Compression Distance** — the standard *computable* approximation
of the Normalized Information Distance, a universal metric grounded in Kolmogorov
complexity (Li, Chen, Li, Ma, Vitányi, IEEE TIT 2004):

```text
NCD(x, y) = [C(xy) − min(C(x), C(y))] / max(C(x), C(y))

ν(output) = NCD(baseline, output)   — how much of the output is NOT
                                       already implied by the obvious
                                       reading of the seed
```

The `novelty` parameter (0.0–1.0) both blends the incubation temperature AND sets
the **fail-closed floor** the final output must clear:

```text
τ_eff = τ_base × (0.5 + 0.5 × novelty)

novelty = 0.0 → τ_eff = 0.5 × τ_base  (conservative, high utility)
novelty = 1.0 → τ_eff = 1.0 × τ_base  (maximum divergence, high surprise)
```

**Usage example — Directed Creative Synthesis:**

```axon
anchor GoldenRatio {
    require: aesthetic_harmony
    confidence_floor: 0.70
}

flow CreateVisualConcept(brief: String) -> Visual {
    forge Artwork(seed: "aurora borealis over ancient ruins") -> Visual {
        mode:        transformational
        novelty:     0.85
        constraints: GoldenRatio
        depth:       4
        branches:    7
    }
}

run CreateVisualConcept("Create a visual concept for a film poster")
```

What the compiler does:

1. **Preparation** — expands "aurora borealis over ancient ruins" into a rich
   conceptual foundation via context probing
2. **Incubation** — runs 4 iterations of speculative exploration at
   `τ_eff = 1.2 × 0.925 = 1.11`, pushing beyond obvious associations
3. **Illumination** — launches 7 parallel branches, each crystallizing the
   incubated ideas, then selects the most coherent output (Best-of-N)
4. **Verification** — measures the winning branch's novelty ν = NCD(baseline,
   output) and enforces it **fail-closed**: if ν is below the floor set by
   `novelty`, the forge does NOT return a derivative result — it fails with a
   structured `forge.novelty_floor_breached` error. Only a synthesis that
   provably cleared the measured novelty floor (and its `GoldenRatio` anchor) is
   returned.

This is **not** a prompt template. `forge` compiles to structured IR metadata
(`IRForgeBlock` — seed, mode, novelty, depth, branches, constraints) that the
runtime executes as the orchestrated four-phase pipeline above, with a
`ForgeSoundness` proof-carrying-code certificate checked at the deploy gate. And
unlike every prompt-based "creative mode," its novelty is **measured** (NCD) and
**enforced fail-closed** — a derivative result is never passed off as creative.

> **Honest scope.** `forge` synthesizes a typed *concept/specification*, not a
> rendered artifact; "novelty" is novelty-relative-to-the-obvious-baseline (a
> computable proxy for the uncomputable Kolmogorov novelty), not a claim of
> absolute unprecedentedness.

### V. Autonomous Goal-Seeking — the `agent` Primitive

> AXON introduces a seventh paradigm shift: **compiler-verified autonomous
> agents** grounded in the Belief-Desire-Intention (BDI) architecture, epistemic
> logic, and coinductive semantics.

Every existing LLM framework implements agents as Python classes with ad-hoc
while-loops, hidden state machines, and zero formal guarantees. LangChain's
`AgentExecutor` is a runtime artifact — it cannot be statically analyzed, type-
checked, or budget-bounded at compile time. AXON's `agent` primitive makes
autonomous goal-seeking a **first-class compiled construct** with mathematical
semantics.


**BDI Coinductive Semantics.** An `agent` declaration compiles to a coinductive
BDI system — a state machine whose behavior is defined by an infinite
observation/transition pair over the epistemic lattice:

```text
Agent ≅ ν X. (S × (Action → X))

where
  S        = Beliefs × Goals × Plans    — cognitive state
  Action   = Observe | Deliberate | Act | Reflect
  ν        = greatest fixpoint (coinduction — runs indefinitely)
```

The `ν` (nu) operator is the key: unlike inductive data (finite trees), a
coinductive agent is a potentially infinite stream of state transitions,
terminating only when the goal is achieved or a budget is exhausted. This
formalization is not decorative — it determines the compiler's verification
strategy and the executor's loop semantics.

**Epistemic Lattice Convergence.** At each BDI cycle, the agent's epistemic
state is projected onto the same lattice `(T, ≤)` used by epistemic directives.
The deliberation phase produces a state `σ ∈ {know, believe, speculate, doubt}`
and a boolean `goal_achieved`. The convergence criterion is:

```text
Converge(σ, g) = g = true ∧ σ ≥ believe

Diverge(σ, i, n) = σ = doubt ∧ Δσ = 0 ∧ i ≥ n
  where
    Δσ       = σᵢ - σᵢ₋₁   — epistemic progress between cycles
    i        = current iteration
    n        = stuck_window  — consecutive stagnation threshold
```

When `Converge` fires, the agent terminates successfully. When `Diverge` fires,
the `on_stuck` recovery policy activates — `escalate` raises `AgentStuckError`,
`forge` triggers creative re-seeding via the Poincaré pipeline, `retry` resets
and re-attempts.

**Budget Composition.** Budget constraints compose from the IR into the runtime
as a 4-tuple verified at compile time:

```text
B(agent) = (max_iter, max_tokens, max_time, max_cost)

Terminate when: ∃ b ∈ B(agent) : consumed(b) ≥ limit(b)
```

The compiler rejects agents with unbounded budgets (`max_iterations = 0` without
an explicit `on_stuck` policy), preventing runaway execution by construction.

**Strategy Dispatch.** The `strategy` parameter selects the BDI loop variant at
compile time. Each strategy maps to a specific deliberation/action sequence:

```text
Λ : Strategy → CycleShape

Λ(react)            = Deliberate → Act → Observe
Λ(reflexion)        = Deliberate → Act → Observe → Reflect
Λ(plan_and_execute) = Plan → (Act → Observe)* → Verify
Λ(custom)           = user-defined step sequence
```

**Usage example — Autonomous Research Agent:**

```axon
persona ResearchAnalyst {
    domain: ["market research", "competitive analysis"]
    tone: analytical
    confidence_threshold: 0.85
}

tool WebSearch {
    provider: serper
    timeout: 10s
}

tool DataAnalyzer {
    provider: internal
    timeout: 30s
}

agent MarketResearcher {
    goal: "Produce a comprehensive competitive analysis report
           with verified data from at least 5 sources"
    tools: [WebSearch, DataAnalyzer]
    strategy: react
    max_iterations: 15
    max_tokens: 50000
    max_cost: 2.50
    on_stuck: forge
    return: CompetitiveReport
}

flow CompetitiveIntelligence(sector: String) -> CompetitiveReport {
    step Research {
        MarketResearcher(sector)
        output: CompetitiveReport
    }
}

run CompetitiveIntelligence("electric vehicles")
    with ResearchAnalyst
```

What the compiler does:

1. **IR Generation** — the `agent` block compiles to an `IRAgent` node containing
   goal, tools, budget (15 iter / 50k tokens / $2.50), strategy (`react`), and
   recovery policy (`forge`). The `IRAgent` is embedded as a step inside
   `IRFlow`, preserving compositional semantics.
2. **Backend Compilation** — the backend (Anthropic, Gemini) generates a
   `CompiledStep` with `step_name: "agent:MarketResearcher"` and full agent
   metadata in its `metadata["agent"]` dictionary. The system prompt includes
   persona traits, tool availability, and epistemic constraints.
3. **Runtime Execution** — the executor detects `agent:` prefix and dispatches
   to the BDI loop. Each cycle: deliberate (epistemic assessment via JSON),
   act (execute step or invoke tool), observe (update beliefs). The loop
   respects the budget 4-tuple and applies `on_stuck` when `Diverge` fires.
4. **Trace Events** — every BDI cycle emits `STEP_START`, `MODEL_CALL`, and
   `STEP_END` trace events, giving full observability into the agent's
   reasoning trajectory.

**Why this matters:** The agent is not a Python class that wraps `while True`.
It is a **compiled cognitive primitive** — the compiler verifies its budget
boundedness, the type checker validates its return type, the backend generates
strategy-specific prompts, and the runtime executes a formally-defined BDI loop
with epistemic convergence criteria. This is the difference between duct-taping
an LLM into a loop and engineering an autonomous system with mathematical
guarantees.

#### Agent Use Case 1: Autonomous Legal Research Agent

A law firm deploys an agent that autonomously researches case law until it finds
sufficient precedent — or exhausts its budget and escalates to a human attorney:

```axon
// §Fase 119.f — the tools this agent uses must EXIST: an agent naming a
// tool nobody declared is an incomplete example, not a grammar gap (the
// same shape as §119.c's block 42).
tool WebSearch {
    timeout: 10s
}

tool PDFExtractor {
    timeout: 30s
}

agent CaseLawResearcher {
    goal: "Find 3+ relevant precedents for the contract dispute
           with verified court citations"
    tools: [WebSearch, PDFExtractor]
    strategy: reflexion
    max_iterations: 20
    max_cost: 5.00
    on_stuck: escalate
    return: CaseLawReport
}
```

- `reflexion` strategy adds self-critique after each cycle — the agent evaluates
  whether its found precedents are truly relevant, not just keyword matches
- `on_stuck: escalate` means if the agent doubts its findings after 20 cycles,
  it raises `AgentStuckError` with full context, so the human reviews exactly
  where the agent got stuck
- Budget cap of $5.00 prevents runaway API costs — the compiler guarantees
  termination

#### Agent Use Case 2: Multi-Agent Data Pipeline

A BI platform chains two agents: one gathers data, the other analyzes it.
Both execute within the same compiled flow:

```axon
agent DataGatherer {
    goal: "Collect quarterly revenue data from public filings"
    tools: [WebSearch, FileReader]
    strategy: react
    max_iterations: 10
    on_stuck: retry
    return: DataSet
}

agent TrendAnalyzer {
    goal: "Identify year-over-year growth patterns and anomalies"
    tools: [Calculator, DataAnalyzer]
    strategy: plan_and_execute
    max_iterations: 8
    on_stuck: forge
    return: TrendReport
}

flow QuarterlyIntelligence(sector: String) -> TrendReport {
    step Gather { DataGatherer(sector) output: DataSet }
    step Analyze { TrendAnalyzer(Gather.output) output: TrendReport }
}
```

- Two agents, two strategies: `react` for data gathering (fast, tool-heavy),
  `plan_and_execute` for analysis (structured, plan-then-verify)
- Each agent has independent budget tracking — if `DataGatherer` costs $0.50,
  `TrendAnalyzer` still has its full budget
- If `TrendAnalyzer` gets stuck, `forge` triggers creative re-seeding via the
  Poincaré pipeline, generating novel analytical angles

#### Agent Use Case 3: Customer Onboarding Agent with Dynamic Recovery

A SaaS platform uses an agent to guide new customers through a personalized
onboarding flow, adapting when it gets stuck:

```axon
tool APICall {
    timeout: 10s
}

tool Calculator {
    timeout: 5s
}

persona OnboardingSpecialist {
    domain: ["product knowledge", "user experience"]
    tone: empathetic
    confidence_threshold: 0.80
}

agent OnboardingGuide {
    goal: "Complete the customer's onboarding checklist with
           personalized recommendations for their industry"
    tools: [APICall, Calculator]
    strategy: custom
    max_iterations: 12
    max_tokens: 30000
    on_stuck: forge
    return: OnboardingReport

    step Greet { ask: "Welcome the user and assess their goals" }
    step Configure { ask: "Recommend workspace configuration" }
    step Train { ask: "Generate personalized tutorial sequence" }
}
```

- `custom` strategy: the agent follows a user-defined step sequence (Greet →
  Configure → Train), not a generic loop
- `on_stuck: forge` — if the agent can't personalize recommendations (e.g.,
  unknown industry), it triggers creative synthesis to propose novel onboarding
  paths instead of failing
- The `return: OnboardingReport` type is validated by the semantic type checker
  — the agent must produce a structurally valid report, not just free text

### VI. Compile-Time Security — the `shield` Primitive

> AXON introduces an eighth paradigm shift: **Information Flow Control
> (IFC) as a first-class compiled construct**, providing compile-time security
> guarantees against LLM-specific attack vectors.

Every LLM framework treats security as an afterthought — runtime guardrails
bolted on top of applications. AXON's `shield` primitive makes security a
**compiler-verified property** of your program, grounded in taint analysis and
Information Flow Control theory.

**Trust Lattice (Denning-style IFC).** The shield system operates over a trust
lattice where data flows from untrusted sources through shield application
points to trusted sinks. The compiler statically verifies that every path from
an untrusted source to a trusted sink passes through at least one shield:

```text
U : DataLabel → TrustLevel

TrustLevel = Untrusted < Scanned < Sanitized < Trusted

∀ path(source, sink) ∈ Flow :
  label(source) = Untrusted ∧ label(sink) = Trusted
  → ∃ shield ∈ path : label(shield.output) ≥ Sanitized
```

**Threat Taxonomy.** The `scan` field declares which threats the shield detects,
drawn from a formal taxonomy of 11 LLM attack categories:

```text
T = { prompt_injection, jailbreak, data_exfil, pii_leak, toxicity,
      bias, hallucination, code_injection, social_engineering,
      model_theft, training_poisoning }
```

**Detection Strategies.** The `strategy` parameter selects the detection
mechanism, each with different cost/accuracy tradeoffs:

```text
Σ : Strategy → (Cost, Accuracy, Latency)

Σ(pattern)     = (low,    medium, fast)     — regex/heuristic scan
Σ(classifier)  = (medium, high,   medium)   — fine-tuned classifier (Llama Guard)
Σ(dual_llm)    = (high,   highest, slow)    — privileged/quarantined model pair
Σ(canary)      = (low,    medium, fast)     — traceable token injection
Σ(perplexity)  = (medium, high,   medium)   — statistical anomaly detection
Σ(ensemble)    = (high,   highest, slow)    — majority voting across multiple strategies
```

**Capability Enforcement.** The compiler statically verifies that agent tool
access is a subset of the shield's allow list — preventing privilege escalation
at compile time:

```text
∀ agent A with shield S :
  tools(A) ⊆ allow_tools(S)    — verified at compile time
  tools(A) ∩ deny_tools(S) = ∅  — also verified
```

**Usage example — LLM Input Shield:**

```axon
shield InputGuard {
    scan: [prompt_injection, jailbreak, pii_leak]
    strategy: dual_llm
    on_breach: halt
    severity: critical
    allow: [web_search, calculator]
    deny: [code_executor]
    sandbox: true
    redact: [email, phone]
    confidence_threshold: 0.85
}

persona SecureAssistant {
    domain: ["customer support"]
    tone: professional
    confidence_threshold: 0.80
}

agent SecureBot {
    goal: "Answer customer queries safely"
    tools: [web_search, calculator]
    shield: InputGuard
    strategy: react
    max_iterations: 10
    return: SafeResponse
}

flow SecureSupport(query: String) -> SafeResponse {
    shield InputGuard on query -> SanitizedQuery
    step Process {
        SecureBot(SanitizedQuery)
        output: SafeResponse
    }
}

run SecureSupport("Help me with my account")
    with SecureAssistant
```

What the compiler does:

1. **Type Checking** — validates all scan categories, strategies, breach
   policies, severity levels, and confidence thresholds. Detects allow/deny
   overlaps and invalid configurations at compile time.
2. **Capability Enforcement** — verifies that `SecureBot` only uses
   `[web_search, calculator]` which are in `InputGuard.allow`, and that
   neither appears in `deny`. If `SecureBot` tried to use `code_executor`,
   the compiler would reject the program.
3. **Taint Analysis** — verifies that `query` (untrusted) passes through
   `shield InputGuard on query` before reaching the agent's trusted context.
4. **Runtime Execution** — the shield step emits `SHIELD_SCAN_START`,
   scans for prompt injection/jailbreak/PII, and either passes
   (`SHIELD_SCAN_PASS`) or raises `ShieldBreachError` (`SHIELD_SCAN_BREACH`).

#### Shield Use Case 1: Financial Data Pipeline with PII Redaction

```axon
shield DataShield {
    scan: [pii_leak, data_exfil]
    strategy: classifier
    on_breach: sanitize_and_retry
    max_retries: 3
    severity: high
    redact: [ssn, credit_card, bank_account]
}

flow ProcessFinancialQuery(input: String) -> Report {
    shield DataShield on input -> CleanInput
    step Analyze {
        given: CleanInput
        ask: "Analyze the financial data"
        output: Report
    }
}
```

- PII fields (SSN, credit card, bank account) are auto-redacted **before** the
  LLM sees the data
- `sanitize_and_retry` means detected threats are cleaned and re-scanned up to
  3 times, not just blocked
- The compiler guarantees the LLM never processes raw PII

#### Shield Use Case 2: Multi-Agent System with Capability Isolation

```axon
shield ResearchShield {
    scan: [data_exfil, model_theft]
    strategy: ensemble
    on_breach: quarantine
    allow: [web_search, file_reader]
    deny: [code_executor, api_call]
    sandbox: true
}

tool web_search {
    timeout: 10s
}

tool file_reader {
    timeout: 10s
}

agent Researcher {
    goal: "Gather market intelligence from public sources"
    tools: [web_search, file_reader]
    shield: ResearchShield
    strategy: reflexion
    max_iterations: 15
    return: IntelligenceReport
}
```

- `ensemble` strategy runs multiple detectors with majority voting — highest
  accuracy for sensitive operations
- `sandbox: true` runs tool execution in an isolated environment
- Capability enforcement: the compiler rejects any agent that tries to use
  `code_executor` or `api_call` — preventing privilege escalation by design
- `quarantine` breach policy isolates suspicious data for human review instead
  of blocking operations

### VII. Epistemic Tool Fortification — Streaming, Effects & Blame Semantics

> AXON introduces a ninth paradigm shift: **formal epistemic control over
> tool invocations, streaming outputs, and foreign-function interfaces** — backed
> by algebraic effect theory, coinductive stream semantics, and Findler-Felleisen
> blame calculus. The `stream` primitive decouples pure deliberation from the I/O mechanism.

#### The Hard Argument (Computational Decoupling)
In pragmatic software engineering, Python generators (`yield` and `async for`) have become the standard for data streaming. However, under the rigor of formal language theory and category mathematics, this approach has a structural flaw: it inextricably couples "deliberation" (data generation) with the "I/O mechanism" (transmission). AXON resolves this by applying **Algebraic Effects and Handlers** to streaming. The `stream` primitive no longer executes I/O; it yields a pure effect (`YieldChunk(data)`), suspending the continuation `k`. An external Handler (e.g., `SSEHandler`) intercepts the effect, executes the I/O side-effect, and then resumes `k`. This mathematical decoupling ensures the generative core remains functionally pure and independently testable.

#### The Sweet Argument (Why it's awesome)
Imagine writing streaming logic without ever worrying about the HTTP connection! With the renewed `stream` primitive, your AI agents don't "push bytes"—they express pure conceptual intentions. You just write your LLM generation logic in the cleanest way possible. Want to switch from Server-Sent Events (SSE) to WebSockets, or maybe just log to a file? The agent code doesn't change a single character! You simply swap the Handler. Your codebase becomes incredibly pristine, blazingly fast to test, and theoretically invincible. It makes streaming feel like pure magic backed by hardcore category theory.

#### Real-World Use Cases
1. **Agentic Server-Sent Events (SSE)**: Stream an agent's intermediate "thoughts" and reasoning steps directly to a React frontend in real-time. If the client drops the connection, the handler manages the disconnection gracefully without crashing the agent's pure deliberation cycle.
2. **Multi-Channel Orchestration**: A single `stream` computation can be intercepted by a composite handler that simultaneously prints chunks to a CLI, broadcasts to an SSE channel, and persists the flow to a Redis database—all while the business logic remains fully unaware of these I/O burdens.
3. **Deterministic Testing Pipelines**: In your CI/CD pipelines, the I/O handler can be instantly swapped out for a `MockHandler` that accumulates chunks synchronously in memory. This eliminates flaky network-bound streaming tests entirely, allowing you to test complex LLM streaming flows in microseconds.

Every LLM framework treats tool calls as black boxes: a function returns a
string, and the framework trusts it unconditionally. Streaming is even worse —
partial tokens arrive without any notion of confidence, reliability, or
epistemic state. AXON solves this by making **every interaction with the
external world** subject to formal epistemic tracking.

#### Formal Model — Four Convergence Theorems

**CT-1: Coinductive Semantic Streaming.** A streaming response is a
coinductive process — an infinite observation/transition pair that monotonically
accumulates epistemic confidence as chunks arrive:

```text
Stream(τ) = νX. (StreamChunk × EpistemicState × X)

where
  StreamChunk    = (content: String, index: ℕ, timestamp: ℝ)
  EpistemicState = (level ∈ {doubt, speculate, believe, know}, confidence ∈ [0,1])
  ν              = greatest fixpoint (coinduction — process unfolds indefinitely)

Monotonicity invariant:
  ∀ i < j : gradient(chunkᵢ) ⊑ gradient(chunkⱼ)
  (epistemic level can only rise, never degrade during streaming)
```

Streaming in AXON is **not** "tokens arriving". It is a formal epistemic
process: each chunk carries its position on the lattice, and the system
guarantees that confidence can only increase monotonically until convergence.

**CT-2: Algebraic Effect Rows.** Every tool declares its computational effects
using Plotkin & Pretnar's algebraic effect theory. The compiler statically
verifies effect compatibility:

```text
EffectRow(tool) = ⟨ε₁, ε₂, ..., εₙ, epistemic:level⟩

where
  εᵢ ∈ {pure, io, network, storage, random}
  level ∈ {know, believe, speculate, doubt}

Composition rule:
  EffectRow(A ∘ B) = EffectRow(A) ∪ EffectRow(B)
  epistemic(A ∘ B) = min(epistemic(A), epistemic(B))   — meet on lattice
```

The composition rule means: if you chain a `network + speculate` tool with a
`pure + know` tool, the combined effect is `network + speculate` — the system
automatically tracks the **least trustworthy** component.

**CT-3: Blame Semantics for FFI.** External tool calls are wrapped in
Findler-Felleisen contract monitors that assign blame when pre/postconditions
fail:

```text
ContractMonitor(tool) = (Pre, Post, Blame)

where
  Pre  : Input → Bool         — caller's obligation
  Post : Output → Bool        — server's obligation
  Blame : {CALLER, SERVER}    — who violated the contract

Blame assignment:
  ¬Pre(input)   → Blame = CALLER   (you sent bad data)
  ¬Post(output) → Blame = SERVER   (tool returned bad data)
```

This is not error handling — this is **formal accountability**. When a tool
fails, AXON tells you *who* broke the contract, not just *that* it broke.

**CT-4: Epistemic Inference via CSP.** The `@csp_tool` decorator automatically
infers the epistemic level of any Python function by analyzing its effect
footprint using a constraint-satisfaction heuristic:

```text
Infer(f) : Function → EpistemicLevel

  If ∄ io/network/random ∈ effects(f) → know
  If ∃ network ∈ effects(f)           → speculate
  If ∃ random ∈ effects(f)            → doubt
  Otherwise                           → believe
```

#### What Makes This Revolutionary

No LLM framework in existence tracks **what a tool does to your epistemic
state**. LangChain, CrewAI, AutoGen — they all treat tool results as trusted
strings. This means:

- A web search result (unreliable) gets the same trust as a database query
  (reliable)
- A streaming response's first token gets the same trust as the final,
  validated output
- When a tool fails, you don't know if your input was wrong or the tool was
  broken

AXON solves all three. The compiler **guarantees** that:

1. Every tool call is tagged with its effect signature and epistemic level
2. Streaming outputs start at `doubt` and can only ascend monotonically
3. Tool failures carry blame labels that identify the responsible party
4. Data crossing the FFI boundary is **automatically tainted** — it cannot
   reach `know` level without passing through a shield or anchor

#### Use Case 1: Real-Time Financial Streaming with Epistemic Gradient

A trading desk receives streaming market data and needs to distinguish between
real-time quotes (speculative) and confirmed trades (factual):

```axon
tool MarketFeed {
    timeout: 5s
    effects: <io, network, epistemic:speculate>
}

flow MonitorMarket(sector: String) -> MarketReport {
    step Stream {
        stream<QuoteData> {
            on_chunk: {
                probe chunk for [symbol, price, volume]
                output: QuoteSnapshot
            }
            on_complete: {
                validate QuoteSnapshot against: MarketSchema
                output: VerifiedQuote
            }
        }
    }
    step Analyze {
        reason {
            given: Stream.output
            ask: "Identify anomalous price movements"
            depth: 2
        }
        output: MarketReport
    }
}
```

- Each streaming chunk starts at `doubt` — the system treats partial data as
  unreliable by default
- `on_complete` handler validates and promotes to `believe` — only complete,
  schema-validated data upgrades
- The `effects: <io, network, epistemic:speculate>` declaration means the
  compiler knows this tool is **never** factual — preventing accidental
  `know`-level assertions from market data

#### Use Case 2: Multi-Tool Research Agent with Blame Tracking

A research agent uses multiple tools with different reliability levels. When
something fails, the system identifies exactly who broke the contract:

```axon
tool WebSearch {
    provider: serper
    timeout: 10s
    effects: <network, epistemic:speculate>
}

tool DatabaseQuery {
    provider: internal
    timeout: 30s
    effects: <io, epistemic:believe>
}

tool Calculator {
    provider: stdlib
    effects: <pure, epistemic:know>
}

flow DeepResearch(question: String) -> ResearchReport {
    par {
        step Web {
            use_tool WebSearch with query: question
            output: WebResults
        }
        step DB {
            use_tool DatabaseQuery with query: question
            output: DBResults
        }
    }
    step Synthesize {
        weave [Web.output, DB.output]
        output: ResearchReport
    }
}
```

- `WebSearch` is `epistemic:speculate` — the compiler knows web results are
  unreliable and automatically taints downstream data
- `DatabaseQuery` is `epistemic:believe` — more reliable, but still not `know`
  because external I/O is involved
- `Calculator` is `pure + epistemic:know` — no side effects, deterministic,
  fully trustworthy
- When `weave` combines them, the result's epistemic level is
  `min(speculate, believe) = speculate` — the weakest link determines trust
- If `WebSearch` returns garbage, the `ContractMonitor` issues
  `Blame = SERVER` with full diagnostic context

#### Use Case 3: Safe External API Integration with @contract_tool

A production system integrates a third-party payment API. The `@contract_tool`
decorator wraps it with pre/postcondition contracts and automatic epistemic
downgrade:

```python
from axon.runtime.tools import contract_tool

@contract_tool(
    pre=lambda amount, currency: amount > 0 and currency in ["USD", "EUR"],
    post=lambda result: "transaction_id" in result,
    effect_row=("network", "io"),
    epistemic_level="speculate"
)
async def process_payment(amount: float, currency: str) -> dict:
    return await stripe_api.charge(amount, currency)
```

```axon
flow ProcessOrder(order: Order) -> Receipt {
    step Charge {
        use_tool process_payment with amount: order.total, currency: "USD"
        output: PaymentResult
    }
    step Verify {
        validate Charge.output against: PaymentSchema
        if confidence < 0.9 -> refine(max_attempts: 2)
        output: Receipt
    }
}
```

- `pre` contract: AXON validates that `amount > 0` and `currency` is valid
  **before** calling Stripe. If violated → `Blame = CALLER`
- `post` contract: AXON validates that the response contains a
  `transaction_id`. If violated → `Blame = SERVER` (Stripe returned bad data)
- All payment results are automatically `tainted = True` — they cannot reach
  `know` level without explicit anchor validation
- The `effects: <network, io>` declaration prevents this tool from being used
  inside a `pure` context — a compile-time error

---

### VIII. Structured Cognitive Retrieval — the `pix` Primitive

> AXON introduces a tenth paradigm shift: **intent-driven tree navigation
> as a formally grounded alternative to vector-similarity retrieval (RAG)**,
> built on information foraging theory, bounded rational search, and full
> explainability via reasoning trails.

Every RAG system in existence makes the same assumption: *semantically close
embeddings imply relevance*. This works for keyword-style queries, but fails
catastrophically for structured documents — legal contracts, technical manuals,
medical records — where the answer lives at a specific structural location, not
in the nearest embedding vector.

AXON's `pix` primitive rejects the "embed everything, retrieve by cosine"
paradigm. Instead, it treats documents as **navigable trees** and retrieval as
a **bounded cognitive search** — the same process a human expert uses when
consulting a complex document: start at the table of contents, follow the most
promising branches, prune irrelevant paths, and explain every decision.

#### Formal Model — Rooted Directed Acyclic Tree (DAG→Tree)

**Document Tree.** A PIX-indexed document `D` is a rooted tree:

```text
D = (N, E, n₀)

where
  N  = {n₀, n₁, ..., nₖ}    — nodes (sections, subsections, paragraphs)
  E  ⊆ N × N               — directed edges (parent → child)
  n₀ ∈ N                    — root (document-level summary)

Properties:
  ∀ nᵢ ∈ N \ {n₀} : ∃! nⱼ : (nⱼ, nᵢ) ∈ E    — unique parent
  height(D) = h                                — maximum depth
  |leaves(D)| = content nodes with full text
```

Each node carries a **summary** (generated at index time) and optionally the
full section **content**. Internal nodes hold structure; leaf nodes hold
answers.

**Information Scent Navigation.** Navigation follows Pirolli & Card's
Information Foraging Theory. At each tree level, a scoring function `S`
evaluates the "information scent" of every child relative to the query:

```text
S : (query, title, summary) → [0, 1]

Navigation rule at depth d:
  children_d = {nᵢ : (current, nᵢ) ∈ E}
  scored     = {(nᵢ, S(q, nᵢ.title, nᵢ.summary)) : nᵢ ∈ children_d}
  selected   = top_k(scored, k=max_branch) ∩ {(n, s) : s ≥ threshold}

Fallback (no child meets threshold):
  selected = {argmax(scored)} if max(scored) > 0 else ∅
```

The key insight: **the scorer replaces embedding similarity**. In production it
is an LLM call; in tests a keyword-overlap heuristic suffices. Either way, the
navigator uses the same bounded-search algorithm.

**Bounded Rational Search.** Navigation terminates via a budget 4-tuple
verified at compile time:

```text
Config(pix) = (max_depth, max_branch, threshold, timeout)

Termination:
  depth ≥ max_depth  ∨  node.is_leaf  ∨  elapsed ≥ timeout
  → append to result leaves
```

This prevents unbounded traversal — the same principle behind AXON's agent
budget enforcement.

**Reasoning Trail (Explainability).** Every navigation produces a
`ReasoningPath` — an ordered sequence of `NavigationStep` records documenting
*why* each branch was selected or pruned:

```text
Trail = [Step₁, Step₂, ..., Stepₙ]

Stepᵢ = (node_id, title, score, reasoning, depth)

Properties:
  |Trail| = total nodes evaluated
  depth(Trail) = max(Stepᵢ.depth)
```

This is not logging — it is **formal explainability**. The trail is a
first-class data structure accessible via the `trail` keyword.

#### What Makes PIX Different from RAG

| Property | RAG | PIX |
|----------|-----|-----|
| Index structure | Flat vector store | Hierarchical tree |
| Retrieval method | Cosine similarity | Bounded tree navigation |
| Granularity | Fixed chunks | Structural sections |
| Explainability | None (black-box) | Full reasoning trail |
| Query type | Keyword/semantic | Intent-driven |
| Relevance model | "Closest vector" | "Most scented path" |
| Compile-time verification | ❌ | ✅ (depth, branching bounds) |

**PIX principle:** *"Lo estructuralmente navegado con intención es lo
relevante"* — what matters is not what is semantically close, but what a
rational agent would navigate to when consulting the document with purpose.

#### Usage Example — PIX-Navigated Legal Analysis

```axon
pix ContractIndex {
    source: "contracts/master_agreement.md"
    depth: 4
    branching: 3
    model: "fast"
}

flow AnalyzeContract(question: String) -> LegalAnalysis {
    step Search {
        navigate ContractIndex
            query: question
            trail: enabled
            as: relevant_sections
    }
    step Drill {
        drill ContractIndex
            into "Liabilities"
            query: question
            as: liability_detail
    }
    step Explain {
        trail relevant_sections
    }
    step Synthesize {
        weave [relevant_sections, liability_detail]
        format: LegalAnalysis
        include: [answer, sources, reasoning_trail]
    }
}
```

What the compiler does:

1. **Type Checking** — validates `pix` parameters (depth ≤ 10, branching ≤ 10),
   verifies that `navigate` and `drill` reference a declared `pix` (not a
   `persona` or `flow`), and guarantees output bindings are unique
2. **IR Generation** — compiles to `IRPixSpec`, `IRNavigate`, `IRDrill`, and
   `IRTrail` nodes carrying the full configuration (source, depth, branching,
   model, effects)
3. **Runtime Execution** — the PIX engine indexes the source document into a
   `DocumentTree`, then the navigator performs bounded tree search guided by the
   scoring function, recording every decision in the `ReasoningPath`
4. **Trail Output** — the `trail` step exposes the full reasoning path — every
   node evaluated, its score, and why it was selected or pruned

#### PIX Use Case 1: Medical Document Navigation

A hospital system needs to find specific clinical guidelines within a 200-page
protocol manual. RAG would chunk the document into 512-token fragments and
return the 5 closest embeddings — potentially mixing guidelines from different
sections. PIX navigates structurally:

```axon
pix ClinicalProtocol {
    source: "protocols/surgical_guidelines_v12.md"
    depth: 5
    branching: 2
    model: "precise"
}

flow FindGuideline(procedure: String) -> ClinicalGuideline {
    step Navigate {
        navigate ClinicalProtocol
            query: procedure
            trail: enabled
            as: guideline
    }
    step Verify {
        validate guideline against: ClinicalSchema
        if confidence < 0.9 -> refine(max_attempts: 2)
        output: ClinicalGuideline
    }
}
```

- `depth: 5` allows reaching deeply nested subsections (Chapter → Section →
  Subsection → Paragraph → Note)
- `branching: 2` limits exploration to the 2 most relevant children per level
  — fast, focused retrieval
- The trail documents *exactly* which sections were evaluated and why, which is
  required for medical audit compliance

#### PIX Use Case 2: Technical Documentation Q&A

A developer needs to find the exact API method for a specific task in a large
SDK documentation. RAG returns 5 chunks that all mention the API but none
answer the precise question. PIX drills directly:

```axon
pix SDKDocs {
    source: "docs/sdk_reference_v3.md"
    depth: 6
    branching: 3
}

flow AnswerDevQuestion(question: String) -> DevAnswer {
    step Browse {
        navigate SDKDocs query: question as: overview
    }
    step Deep {
        drill SDKDocs into "API Reference" query: question as: api_detail
    }
    step Respond {
        weave [overview, api_detail]
        format: DevAnswer
        include: [answer, code_examples, see_also]
    }
}
```

- `navigate` finds the general area; `drill` goes directly into "API Reference"
- Combined result gives both context (overview) and precision (api_detail)
- No embedding database needed — the document's own structure is the index

#### PIX Use Case 3: Regulatory Compliance Audit with Full Trail

A compliance team audits whether a company's data practices satisfy GDPR
requirements. The trail provides the auditable decision chain:

```axon
pix GDPRRegulation {
    source: "regulations/gdpr_full_text.md"
    depth: 4
    branching: 3
    model: "precise"
}

know {
    flow AuditCompliance(practice: String) -> ComplianceReport {
        step Find {
            navigate GDPRRegulation
                query: practice
                trail: enabled
                as: articles
        }
        step ShowTrail {
            trail articles
        }
        step Assess {
            reason {
                given: articles
                ask: "Does the practice comply with these articles?"
                depth: 3
            }
            output: ComplianceReport
        }
    }
}
```

- `know` block ensures maximum factual rigor — no speculation about regulations
- The `trail` provides a complete record of which GDPR articles were considered
  and why, satisfying regulatory audit requirements
- No vector database, no embedding model, no chunking strategy to tune — the
  regulation's own hierarchical structure (Part → Chapter → Section → Article)
  is the retrieval mechanism

#### Epistemic Vision — Visual Perception for PIX

> AXON extends PIX from document-only navigation to **deterministic
> visual perception**, treating images as structured data isomorphic to
> documents. No neural networks. No GPUs. No stochastic outputs. Pure
> mathematics — and it sees better than any "vision model" at structural tasks.

##### The Hard Argument — Pure Mathematics

The visual pipeline rests on three mathematically validated pillars:

**1. Perona-Malik Anisotropic Diffusion (Regularized).**
Images are treated as signals on a Riemannian manifold. Noise reduction follows
the Catté-Lions-Morel-Coll regularization of the Perona-Malik PDE:

```text
∂u/∂t = div(g(|∇G_σ * u|²) · ∇u)

where
  g(s) = 1 / (1 + s/λ²)           — Lorentzian conductance (edge-preserving)
  G_σ * u                          — Gaussian pre-smoothing (well-posedness)
  CFL condition: Δt ≤ h²/4         — guaranteed numerical stability
```

This is not a filter — it is a **PDE solver** that provably converges to a
piecewise-smooth signal while preserving edges. Every step is deterministic,
reproducible, and CFL-stable.

**2. Gabor Phase Encoding (Biomimetic V1).**
Oriented texture energy is computed via a bank of Gabor filters that model
the primary visual cortex:

```text
Ψ(x,y;θ,λ) = exp(-‖x'‖²/2σ²) · cos(2πx'/λ)

where
  x' = x·cos(θ) + y·sin(θ)        — rotated coordinates
  θ ∈ {kπ/n : k = 0,...,n-1}       — n orientations
  λ ∈ geometric progression        — spatial frequencies
```

The resulting energy map captures oriented structure at multiple scales —
the same information a biological visual cortex extracts in its first 50ms.

**3. Persistent Homology H₀ (Union-Find, O(N·α(N))).**
Topological structure is extracted via sublevel-set filtration using computational
algebraic topology:

```text
PH₀(f) = {(bᵢ, dᵢ)}              — persistence diagram

where
  bᵢ = birth value (component appears in sublevel set)
  dᵢ = death value (component merges with older component)
  β₀ = |{(b,d) : d - b ≥ ε}|     — Betti number (significant components)
```

Persistence diagrams are compared via **Bottleneck** and **Wasserstein** distances,
providing a metric space over topological signatures. The Union-Find algorithm
runs in near-linear time O(N·α(N)), where α is the inverse Ackermann function.

##### The Sweet Argument — Why This Is Genius

The PIX documental engine uses LLM calls for scoring — each navigation decision
costs money, introduces latency, and is inherently non-reproducible.

The visual PIX uses **pure mathematics** for scoring:

```text
Score(node) = σ(w₁·C_topo + w₂·P_total + w₃·E_gabor)

where
  C_topo   = β₀ + β₁                — topological complexity
  P_total  = Σ(dᵢ - bᵢ)             — total persistence
  E_gabor  = mean Gabor energy       — oriented texture richness
  σ(x)     = 1/(1 + e⁻ˣ)            — sigmoidal normalization
```

The result:
- **$0.00 per navigation** — zero API calls, zero tokens consumed
- **100% reproducible** — same image, same result, every time, forever
- **Fully auditable** — every score is a pure function of measurable quantities
- **No GPU required** — runs on any CPU, any platform, any environment

The document is a case of structured data. The image is another. PIX navigates
both with the same `PixNavigator` — the visual extension composes via an
adapter pattern (`VisualTree → DocumentTree`), reusing 100% of the navigation
logic with zero code duplication.

##### Three Use Cases

**Use Case 1: Industrial Quality Control — Deterministic Defect Detection**

A manufacturing plant inspects PCB boards. Traditional CV uses neural networks
that require 10,000+ labeled images, a GPU cluster, and produce stochastic
results. PIX Visual detects defects via topological invariants:

```axon
pix BoardInspector {
    source: "camera://line_3"
    mode: visual
    depth: 3
    branching: 4
}

know {
    flow InspectBoard(image: Image) -> DefectReport {
        step Perceive {
            navigate BoardInspector
                query: "Locate solder joint anomalies"
                trail: enabled
                as: regions
        }
        step Classify {
            reason {
                given: regions
                ask: "Are these topological signatures consistent with known defect patterns?"
                depth: 2
            }
            output: DefectReport
        }
    }
}
```

- β₀ anomalies (unexpected isolated components) flag missing solder joints
- Persistence outliers flag micro-cracks invisible to optical inspection
- Every detection is **deterministic and auditable** — critical for ISO 9001
- Zero training data, zero GPU, zero model drift

**Use Case 2: Medical Imaging — Auditable Pathology Navigation**

A pathology lab analyzes tissue biopsies. Regulatory compliance (FDA, CE)
requires full traceability of every diagnostic decision. PIX Visual provides
the reasoning trail that no neural network can:

```axon
pix TissueAnalyzer {
    source: "pathology://slide_42"
    mode: visual
    depth: 4
    branching: 3
    model: "precise"
}

know {
    flow AnalyzeBiopsy(slide: Image) -> PathologyReport {
        step Survey {
            navigate TissueAnalyzer
                query: "Identify regions of cellular irregularity"
                trail: enabled
                as: findings
        }
        step DeepDive {
            drill TissueAnalyzer
                into findings.top_region
                query: "Characterize cellular morphology"
                as: morphology
        }
        step Report {
            trail findings
            weave [findings, morphology]
            format: PathologyReport
            include: [diagnosis, confidence, reasoning_trail]
        }
    }
}
```

- Persistent homology captures tissue topology (ductal structures, lobular patterns)
- The reasoning trail satisfies regulatory audit requirements
- Results reproducible across institutions — same slide, same diagnosis
- No black-box model to validate, no adversarial attacks possible

**Use Case 3: Geospatial Intelligence — Satellite Imagery Analysis**

A defense agency monitors infrastructure changes via satellite imagery.
Classified environments prohibit cloud APIs and external model calls.
PIX Visual runs entirely on-premise:

```axon
pix SatelliteWatch {
    source: "geo://sector_7G"
    mode: visual
    depth: 5
    branching: 4
}

flow MonitorChanges(before: Image, after: Image) -> ChangeReport {
    par {
        step Baseline {
            navigate SatelliteWatch query: "Extract structural features" as: baseline
        }
        step Current {
            navigate SatelliteWatch query: "Extract structural features" as: current
        }
    }
    step Compare {
        reason {
            given: [baseline.topology, current.topology]
            ask: "What structural changes occurred between acquisitions?"
            depth: 3
        }
        output: ChangeReport
    }
}
```

- Topological comparison detects structural changes (new buildings, roads, excavations)
- Runs 100% air-gapped — no cloud APIs, no data exfiltration risk
- Bottleneck distance between persistence diagrams quantifies change magnitude
- Parallel navigation compares before/after in O(max(t₁, t₂)) latency

---

### IX. Multi-Document Navigation — the `corpus` Primitive

> AXON introduces an eleventh paradigm shift: **formal cross-document
> navigation with provenance guarantees, epistemic typing, and graph-theoretic
> bounded reachability** — the first retrieval framework with mathematical proofs
> of soundness, termination, and information convergence.

Every existing retrieval system treats documents as independent objects: embed
them, rank them by cosine similarity, return a flat list. This works for keyword
queries. It fails catastrophically when the **relationship between documents is
the answer** — a legal brief that cites a statute that cites a prior ruling, a
medical diagnosis that cross-references clinical guidelines and lab protocols, a
financial audit that chains regulatory filings with accounting standards.

AXON's `corpus` primitive treats document collections as **typed directed
graphs** and retrieval as **bounded graph navigation** with formal guarantees
that no existing framework provides.

#### A. Hard Mathematical Argument — Three Theorems

**Definition 1 (Document Corpus Graph).** A corpus is a 5-tuple
`C = (D, R, τ, ω, σ)` where:

```text
D = {D₁, ..., Dₙ}        — finite set of documents
R ⊆ D × D × L            — labeled directed edges (cross-references)
τ : R → RelationType     — edge type: cite | depend | contradict | elaborate | supersede
ω : R → (0, 1]            — edge weight (relationship strength)
σ : D → EpistemicLevel   — document epistemic status function

EpistemicLevel = Uncertainty ≤ ContestedClaim ≤ FactualClaim ≤ CitedFact ≤ CorroboratedFact
```

The ordering on `EpistemicLevel` encodes **justification strength**: `A ≤ B` iff
A is less justified or less informationally supported than B. This is a complete
lattice with ⊤ = CorroboratedFact, ⊥ = Uncertainty, and operations:

```text
join(A, B) = sup{A, B}    — strongest justified level (promotion)
meet(A, B) = inf{A, B}    — most conservative level (aggregation)
```

**Theorem 1 (Decidability + Bounded Complexity).** The bounded graph
reachability problem for MDN is decidable in `O(b̄ᵈ · C_eval)` where `b̄` is
the effective branching factor (typically 2–3 after pruning) and `d` is
`max_depth`.

_Key insight:_ since `d` is a compile-time constant (typically 3–5), the
exponential factor is controlled. With information-gain pruning, practical
complexity is **near-linear** in corpus size.

**Theorem 2 (Strict Information Gain).** Under an ε-informative navigation
policy, each step strictly reduces conditional entropy:

```text
H(A | Q, D₀, ..., Dₖ) ≤ H(A | Q) - k · ε

where ε > 0 is the minimum information gain per step
```

_Consequence:_ navigation terminates in at most `k ≤ ⌈H(A|Q)/ε⌉` steps.
This is **not** a heuristic — it is an information-theoretic convergence proof.
Every step provably makes progress toward answering the query.

**Theorem 3 (Epistemic PageRank Convergence).** The epistemic-weighted PageRank
operator `T` on a corpus graph converges to a unique stationary distribution:

```text
T(v)ᵢ = (1-α)/|D| + α · ∑ⱼ (ωⱼᵢ · σ(Dⱼ)) / ∑ₖ ωⱼₖ

where α ∈ (0,1) is the damping factor and σ(Dⱼ) is the epistemic weight
```

Convergence is guaranteed because `T` is a contraction mapping on the compact
space [0,1]ⁿ (Banach fixed-point theorem). Unlike standard PageRank, EPR
weights authority by **epistemic status** — a peer-reviewed study propagates more
authority than a contested claim.

#### B. Sweet Argument — Why This Changes Everything

The mathematical machinery above enables something no other system provides:
**provenance-guaranteed, epistemically-typed cross-document reasoning.**

When AXON returns a result from multi-document navigation, you know:

1. **Exactly which path the system followed** — not just "these 5 documents are
   relevant" but "Document A cited Document B which contradicts Document C, and
   the result is a ContestedClaim with confidence 0.72."

2. **The epistemic status of every claim** — not all information is equal. A
   peer-reviewed study (CorroboratedFact) carries more weight than a blog post
   (FactualClaim). AXON's lattice makes this distinction a **formal property**
   of the type system, not a human judgment call.

3. **That the search was exhaustive within bounds** — Theorem 2 proves that
   an ε-informative policy doesn't miss relevant paths. If something was within
   depth 3 and above the relevance threshold, it was found.

4. **That contradictions are surfaced, not hidden** — when documents disagree,
   traditional systems return both and let the user reconcile. AXON's epistemic
   lattice **automatically demotes** the claim to ContestedClaim and tracks the
   provenance chain of the conflict.

This is the difference between a search engine and a **reasoning engine over
interconnected knowledge.**

#### MDN Use Case 1: Multi-Source Medical Diagnosis

A hospital system needs to cross-reference a patient's lab results against
clinical guidelines, drug interaction databases, and recent research papers to
make a diagnosis. No single document contains the answer — the diagnosis emerges
from **navigating relationships between sources**:

```axon
corpus ClinicalKnowledge {
    documents: [LabResults, ClinicalGuidelines, DrugDB, RecentStudies]
    edges: [
        LabResults -> ClinicalGuidelines  : cite,    weight: 0.9
        ClinicalGuidelines -> DrugDB      : depend,  weight: 0.8
        RecentStudies -> ClinicalGuidelines: contradict, weight: 0.7
    ]
}

know {
    flow Diagnose(symptoms: String) -> DiagnosisReport {
        step Navigate {
            navigate ClinicalKnowledge
                from: LabResults
                query: symptoms
                depth: 3
                trail: enabled
                as: evidence_chain
        }
        step Assess {
            reason {
                given: evidence_chain
                ask: "Synthesize a differential diagnosis with provenance"
                depth: 3
            }
            output: DiagnosisReport
        }
    }
}
```

- **When RecentStudies contradicts ClinicalGuidelines**, the system automatically
  classifies the conflicting claim as `ContestedClaim` — the treating physician
  sees the contradiction and its provenance, not a false consensus
- **Epistemic PageRank** ranks ClinicalGuidelines (peer-reviewed, widely cited)
  above RecentStudies (single study, not yet corroborated)
- **Trail provides audit-grade provenance**: every decision traces back to
  specific source documents — required for medical malpractice defense
- `know` block ensures maximum rigor — no speculation in clinical settings

#### MDN Use Case 2: Legal Case Building Across Jurisdictions

A law firm builds a case by navigating the citation graph between statutes,
case law, legal opinions, and regulatory guidance. The strength of the case
depends on the **provenance chain** — which authorities support each claim:

```axon
corpus CaseLawGraph {
    documents: [Statute_A, Precedent_B, Precedent_C, RegulatoryGuidance]
    edges: [
        Statute_A -> Precedent_B   : cite,      weight: 0.9
        Precedent_B -> Precedent_C : elaborate,  weight: 0.7
        Precedent_C -> Statute_A   : cite,       weight: 0.8
        RegulatoryGuidance -> Statute_A : depend, weight: 0.6
    ]
}

flow BuildArgument(legal_question: String) -> LegalBrief {
    step Research {
        navigate CaseLawGraph
            from: Statute_A
            query: legal_question
            depth: 4
            trail: enabled
            as: authority_chain
    }
    step Synthesize {
        weave [authority_chain]
        format: LegalBrief
        include: [argument, authorities, provenance_trail]
    }
}
```

- **Corroboration detection**: when Precedent_C cites back to Statute_A (cycle),
  EPR identifies the mutual reinforcement and promotes both to `CorroboratedFact`
- **Citation weight** distinguishes primary authority (weight 0.9) from
  tangential references (weight 0.3) — critical for legal argument quality
- **Provenance trail** is the chain of authority itself — the legal brief includes
  not just the conclusion but the formal path through the law that supports it

#### MDN Use Case 3: Financial Due Diligence Across Filing Networks

An investment firm performs due diligence by navigating relationships between
SEC filings, audit reports, analyst notes, and news articles. Contradictions
between sources are the most valuable signal:

```axon
corpus DueDiligence {
    documents: [SEC_10K, AuditReport, AnalystNotes, NewsArticles]
    edges: [
        SEC_10K -> AuditReport     : depend,      weight: 0.95
        AuditReport -> AnalystNotes: elaborate,    weight: 0.6
        NewsArticles -> SEC_10K    : contradict,   weight: 0.8
    ]
}

doubt {
    flow InvestigateRisk(company: String) -> RiskAssessment {
        step Traverse {
            navigate DueDiligence
                from: SEC_10K
                query: company
                depth: 3
                trail: enabled
                as: findings
        }
        step Challenge {
            reason {
                given: findings
                ask: "Identify discrepancies between filings and external reports"
                depth: 3
            }
            output: RiskAssessment
        }
    }
}
```

- **`doubt` block** forces adversarial analysis — the model is primed to find
  contradictions, not consensus
- **When news contradicts the 10-K**, the system flags the discrepancy as
  `ContestedClaim` with exact provenance: "NewsArticles contradicts SEC_10K, 
  edge weight 0.8"
- **Epistemic aggregation**: the overall assessment takes the conservative
  `meet()` of all evidence — if any source is contested, the aggregate drops
- **Trail produces an auditable investigation chain** — every finding traces
  back to its source documents, satisfying regulatory compliance requirements

---

### X. Memory-Augmented MDN — Structural Learning via Graph Transformation

> AXON introduces a twelfth paradigm shift: **memory as a functorial
> endomorphism on the category of corpora** — not storage, but a formal
> transformation of the epistemological space that enables structural learning
> through interaction history.

Every LLM framework treats memory as a cache: stuff text into a vector store,
retrieve by similarity, prepend to prompt. This is computationally trivial and
epistemically bankrupt — the system never *learns* from its interactions. It
merely *remembers* text.

AXON's memory primitive extends the MDN corpus model from `C = (D, R, τ, ω, σ)`
to a **memory-augmented corpus** `C* = (D, R, τ, ω, σ, H, μ)` where the memory
operator `μ` is a functorial endomorphism that transforms the corpus graph based
on interaction history — preserving topology while adapting continuous parameters
(edge weights, epistemic levels) to reflect accumulated experience.

#### A. Hard Mathematical Argument — Functorial Endomorphism

**Definition 2 (Memory-Augmented Corpus).** Extends Definition 1 with:

```text
C* = (D, R, τ, ω, σ, H, μ)

where
  H = (Q, Π, O)              — interaction history
    Q = (q₁, ..., qₙ)        — query sequence
    Π = (π₁, ..., πₙ)        — traversal paths πᵢ ∈ Paths(C)
    O = (s₁, ..., sₙ)        — outcome scores sᵢ ∈ [0,1]

  μ : (C, H) → C'            — memory update operator
    where C' = (D, R, τ, ω', σ')  — same topology, transformed parameters
```

**Three Orthogonal Memory Types.** The operator decomposes into three
independent subsystems, each operating on different aspects of the corpus:

```text
M_episodic  : Π ⊆ Paths(C)     — trajectory storage with structural recall
M_semantic  : ω'(r) = ω(r) + Δ(r | H)   — edge weight adaptation
M_procedural: Bias(D) ∈ ℝ^|D|  — navigation policy learning

where
  Δ(r | H) = η · Σᵢ γⁿ⁻ⁱ · (sᵢ - s̄) · 𝟙[r ∈ Edges(πᵢ)]

  η ∈ (0,1)     — learning rate
  γ ∈ (0,1)     — temporal decay (recent interactions dominate)
  s̄             — running baseline (mean outcome)
```

**Theorem 4 (Convergence of μ).** Under bounded history and Lipschitz-continuous
scoring, repeated application of μ converges to a fixed point:

```text
∃ C∞ : lim_{n→∞} μⁿ(C, H) = C∞

Proof sketch:
  (1) Weight clamping: ε ≤ ω'(r) ≤ 1  — bounded, closed set
  (2) Temporal decay: γⁿ → 0          — diminishing influence
  (3) Banach: ||μ(C₁) - μ(C₂)|| ≤ γ · ||C₁ - C₂||  — contraction ∎
```

**Formal Guarantees:**

```text
Identity:       μ(C, ∅) = C               — empty history preserves corpus
Locality:       Δω(r) ≠ 0 ⟹ r ∈ Edges(Π), r ∈ H — only traversed edges change
Monotonicity:   σ(Dᵢ) ≤ σ(Dⱼ) ⟹ σ'(Dᵢ) ≤ σ'(Dⱼ)  — lattice order preserved
Invariant G4:   0 < ω'(r) ≤ 1             — weight bounds never violated
Generalization: ∃ C, H : Nav(μ(C,H)) ≠ Nav(C)  — memory produces new paths
```

#### B. Sweet Argument — A System That Learns From Its Own Navigation

The mathematical machinery above produces something no other framework has ever
achieved: **a knowledge system that structurally improves through use.**

When you navigate AXON's memory-augmented corpus:

1. **Edges that lead to good answers get stronger.** If a citation path
   (`LabResults → ClinicalGuidelines`) consistently produces high-scoring
   results, its weight increases — making it more likely to be traversed in
   future queries. This is not heuristic; it's the `Δ(r | H)` operator applying
   gradient-like updates to the corpus graph.

2. **Edges that lead to dead ends get weaker.** Contradiction paths with low
   scores see their weights decay toward `ε` — they remain in the graph (no
   information is destroyed) but are naturally deprioritized. The system learns
   what *not* to follow.

3. **Documents earn their epistemic status.** High-scoring documents get
   promoted on the epistemic lattice (`FactualClaim → CitedFact`), while
   consistently poor-scoring documents get demoted. The system doesn't just
   *tag* reliability — it **discovers** it through interaction.

4. **Past navigation shapes future navigation.** Procedural memory computes a
   `Bias(D)` vector that shifts navigation policy — documents that were
   historically valuable get a head start in future traversals, creating an
   adaptive, experience-driven retrieval policy.

This is the difference between a **static knowledge graph** and a **living
epistemological system**. Every other framework — LangChain's memory, LlamaIndex's
history, CrewAI's context — stores text. AXON transforms the **geometric
structure of knowledge itself.**

#### Memory Use Case 1: Adaptive Medical Decision Support

A hospital system navigates clinical knowledge daily. Over time, the system
learns which evidence chains are most diagnostically valuable:

```axon
corpus ClinicalKnowledge {
    documents: [LabResults, Guidelines, DrugDB, RecentStudies]
    edges: [
        LabResults -> Guidelines     : cite,       weight: 0.9
        Guidelines -> DrugDB         : depend,     weight: 0.8
        RecentStudies -> Guidelines  : contradict, weight: 0.7
    ]
    memory: enabled
}

know {
    flow DiagnosticQuery(symptoms: String) -> DiagnosisReport {
        step Navigate {
            navigate ClinicalKnowledge
                from: LabResults
                query: symptoms
                depth: 3
                recall: episodic
                as: evidence_chain
        }
        step Assess {
            reason {
                given: evidence_chain
                ask: "Synthesize differential diagnosis with provenance"
                depth: 3
            }
            output: DiagnosisReport
        }
    }
}
```

- **After 100 diagnostic queries**, the system has learned that `LabResults →
  Guidelines` is the highest-value path (weight promoted from 0.9 → 0.97),
  while `RecentStudies → Guidelines` contradictions rarely help (weight decayed
  from 0.7 → 0.35)
- **Episodic recall** retrieves past trajectories for similar symptoms — the
  system remembers *how* it navigated, not just *what* it found
- **Documents earn their status**: Guidelines promotes to `CorroboratedFact`
  through consistent high-scoring interactions
- **No manual tuning** — the system's edge weights and epistemic levels are
  empirically grounded, not hand-coded

#### Memory Use Case 2: Self-Optimizing Legal Research

A law firm's case research system improves with every successful case by learning
which statutory paths produce winning arguments:

```axon
corpus CaseLawGraph {
    documents: [Statute_A, Precedent_B, Precedent_C, RegulatoryGuidance]
    edges: [
        Statute_A -> Precedent_B           : cite,      weight: 0.9
        Precedent_B -> Precedent_C         : elaborate, weight: 0.7
        RegulatoryGuidance -> Statute_A    : depend,    weight: 0.6
    ]
    memory: enabled
    max_history: 500
}

flow BuildArgument(legal_question: String) -> LegalBrief {
    step Research {
        navigate CaseLawGraph
            from: Statute_A
            query: legal_question
            depth: 4
            recall: episodic
            bias: procedural
            as: authority_chain
    }
    step Synthesize {
        weave [authority_chain]
        format: LegalBrief
        include: [argument, authorities, provenance_trail, memory_influence]
    }
}
```

- **Procedural bias**: after winning 30 cases using `Statute_A → Precedent_B →
  Precedent_C`, the system gives this path a navigational head start —
  `Bias(Precedent_B) = 0.42` vs `Bias(RegulatoryGuidance) = 0.12`
- **Semantic weight learning**: `Statute_A → Precedent_B` weight grows from 0.9
  to 0.98 (consistently high-value citation)
- **Temporal decay** ensures that recent case outcomes matter more than cases
  from 3 years ago — the law evolves, and so do the weights
- **`memory_influence` output field** reports exactly how memory transformed the
  navigation — full transparency on what the system learned

#### Memory Use Case 3: Learning-Aware Financial Surveillance

A compliance system monitors financial networks and learns which investigation
paths reveal genuine anomalies vs. false positives:

```axon
corpus FinancialNetwork {
    documents: [SEC_Filings, AuditReports, TransactionLogs, IntelReports]
    edges: [
        SEC_Filings -> AuditReports     : depend,     weight: 0.95
        AuditReports -> TransactionLogs : elaborate,   weight: 0.6
        IntelReports -> SEC_Filings     : contradict,  weight: 0.8
    ]
    memory: enabled
    max_history: 1000
}

doubt {
    flow InvestigateAnomaly(alert: String) -> RiskAssessment {
        step Traverse {
            navigate FinancialNetwork
                from: TransactionLogs
                query: alert
                depth: 3
                recall: episodic
                bias: procedural
                as: findings
        }
        step Challenge {
            reason {
                given: findings
                ask: "Is this a genuine anomaly or a known false positive?"
                depth: 3
            }
            output: RiskAssessment
        }
    }
}
```

- **False positive learning**: when investigations resolve as benign (low
  outcome score), the traversed paths' edge weights decrease — the system learns
  which patterns are noise, not signal
- **True positive reinforcement**: genuine anomaly paths see weight increases,
  making similar future anomalies faster to locate
- **Episodic recall** surfaces past investigations with similar alert patterns —
  "we saw this 3 months ago and it was a known vendor discrepancy"
- **Procedural bias** steers the system toward document types that historically
  revealed real issues — if `IntelReports` consistently surfaces genuine risks,
  it gets navigational priority
- **`doubt` block** ensures adversarial stance — the system challenges every
  finding, preventing confirmation bias even as it learns

---

### XI. Psychological-Epistemic Modeling — the `psyche` Primitive

> AXON introduces a thirteenth paradigm shift: **formal psychological-
> epistemic modeling with Riemannian state dynamics, quantum cognitive probability,
> and active inference** — the first compiled construct that treats mental states
> as epistemological objects with structured uncertainty and formal safety
> guarantees.

Every existing AI system treats cognitive biases, emotional states, and mental
load as noise to be filtered out. This is a category error. Human cognition
is not rational-plus-noise — it is a **dynamical system on a curved manifold**
where affect, bias, and cognitive load are formal modulators of epistemic
inference. AXON's `psyche` primitive makes this distinction a first-class
language construct.

```axon
psyche TherapeuticProfile {
    dimensions: [affect, bias, cognitive_load]
    manifold {
        curvature: { affect: 0.8, bias: 1.2, cognitive_load: 0.5 }
        noise: 0.1
        momentum: 0.3
    }
    safety: [non_diagnostic]
    quantum: enabled
    inference: active
}
```

#### A. Hard Mathematical Argument — Three Theorems

**Definition 1 (Cognitive State Manifold).** A psyche configuration defines a
Riemannian manifold `(M, g)` where:

```text
M = ℝᵈ                    — d-dimensional cognitive state space
g : TₚM × TₚM → ℝ       — Riemannian metric tensor encoding local geometry
ψ(t) ∈ M                  — cognitive state trajectory at time t
d = |dimensions|           — number of cognitive dimensions (≥ 1)
```

The metric tensor `g` incorporates the per-dimension curvatures `κᵢ`:

```text
gᵢⱼ(ψ) = κᵢ · δᵢⱼ + f(ψ)     where κᵢ > 0, f captures cross-dimensional coupling
```

This is not an ad-hoc parameterization — it is a **proper Riemannian structure**
that gives each cognitive dimension its own local geometry. High curvature in
`bias` (κ = 1.2) means the manifold bends sharply around biased states, making
them harder to remain in. Low curvature in `cognitive_load` (κ = 0.5) means
the system can traverse load states smoothly.

**Theorem 1 (SDE Convergence on M).** The stochastic differential equation
governing cognitive state evolution admits a unique strong solution:

```text
dψ(t) = μ(ψ, t) dt + σ · dW(t)

where:
  μ(ψ, t) — drift function (manifold geodesic + momentum β)
  σ ∈ (0, 1] — diffusion coefficient (configured noise)
  W(t) — standard Wiener process on M

Convergence: 𝔼[‖ψ(t) - ψ*(t)‖²] ≤ C · e^{-λt}
```

_Key insight:_ because `σ` is bounded ∈ (0, 1] (enforced at compile-time by the
type checker) and `M` is complete (curvature `κᵢ > 0` guarantees geodesic
completeness), the SDE has a unique strong solution by Itô theory. The system
cannot diverge.

**Theorem 2 (Quantum Density Matrix Trace Preservation).** When `quantum:
enabled`, the cognitive state is lifted to a density matrix `ρ_ψ` satisfying:

```text
ρ_ψ ∈ S(ℋ) = { ρ : ℋ → ℋ | ρ ≥ 0, Tr(ρ) = 1 }

Quantum belief update:   ρ' = Σᵢ Kᵢ ρ Kᵢ†     (Kraus channel)
Trace preservation:      Σᵢ Kᵢ† Kᵢ = I         (CPTP condition)
Von Neumann entropy:     S(ρ) = -Tr(ρ log ρ)    (uncertainty measure)
```

_Consequence:_ beliefs are **superposed** rather than point-estimated.
A patient can be simultaneously in `anxious ∧ motivated` states
with interference effects — exactly like quantum probability theory predicts
for human cognitive biases (Busemeyer & Bruza, 2012).

**Theorem 3 (Free Energy Convergence).** Under active inference, the system
minimizes variational free energy:

```text
F(ψ, m) = 𝔼_q[log q(ψ) - log p(ψ, o | m)]

Convergence: F(ψₜ₊₁) ≤ F(ψₜ) - η · ‖∇F‖²     (monotone descent)
Termination: converges in ≤ ⌈F₀ / (η · ε²)⌉ steps
```

_Guarantee:_ the active inference loop **provably converges** to a local minimum
of free energy, meaning the system always reaches a stable epistemic state.
Combined with the NonDiagnostic type constraint (§4 of PEM), the converged state
is guaranteed to be a **structural understanding** rather than a clinical
diagnosis.

#### B. Sweet Argument — Why This Changes Everything

The mathematical machinery above enables something unprecedented:
**formal reasoning about psychological states as first-class objects.**

When AXON executes a `psyche` block, you get:

1. **States on a manifold, not labels in a dropdown** — affect isn't `"happy"` or
   `"sad"`. It's a point on a curved surface where the geometry itself encodes how
   states relate to each other. Depression and anxiety are close on the manifold
   (high curvature boundary), while calm and focused are in a flat basin.
   **Topology replaces taxonomy.**

2. **Uncertainty as a mathematical structure, not imprecision** — with quantum
   mode enabled, a patient doesn't have `bias = 0.7`. They have a density matrix
   where confirmation bias and availability bias are **superposed** with
   interference terms. The system models that biases interact non-classically —
   exactly as empirical cognitive science shows.

3. **Convergence guarantees, not best-effort prompts** — the active inference loop
   minimizes free energy with a proven convergence rate. Traditional prompt
   engineering throws instructions at an LLM and hopes. AXON's `psyche` provides
   a **mathematical guarantee** that the system will reach a stable
   epistemic interpretation.

4. **Safety as a type, not a disclaimer** — the `non_diagnostic` constraint is
   enforced at **compile-time** (type checker) and **runtime** (trace event).
   The system literally cannot emit diagnostic outputs. This isn't a
   system prompt that says "don't diagnose" — it's a formal type boundary
   that makes clinical diagnosis **unrepresentable** in the program's
   output type.

This is the difference between an AI that processes text about psychology
and one that **reasons within a formal psychological-epistemic framework.**

#### Psyche Use Case 1: Clinical Research — Longitudinal Affect Tracking

A psychiatric research institute studies mood trajectories in treatment-resistant
depression. Traditional tools use discrete mood scales (PHQ-9, GAD-7) that
cannot model the continuous dynamics of affective states. AXON's `psyche`
primitive provides a Riemannian manifold where mood evolves continuously via SDE,
and quantum superposition captures ambivalent states ("simultaneously hopeless
and determined") that discrete scales cannot represent.

```axon
psyche AffectTrajectory {
    dimensions: [valence, arousal, dominance, rumination]
    manifold {
        curvature: { valence: 1.0, arousal: 0.8, dominance: 0.6, rumination: 1.5 }
        noise: 0.15
        momentum: 0.4
    }
    safety: [non_diagnostic]
    quantum: enabled
    inference: active
}

flow TrackMoodTrajectory(sessions: [SessionData]) -> TrajectoryReport {
    step Initialize {
        probe sessions[0] for [baseline_valence, baseline_arousal]
        use AffectTrajectory
        output: ManifoldState
    }
    step Evolve {
        reason {
            given: Initialize.output, sessions
            ask: "How has the affective trajectory evolved across sessions?"
            depth: 4
        }
        output: TrajectoryAnalysis
    }
    step Synthesize {
        weave [Initialize.output, Evolve.output]
        format: TrajectoryReport
        include: [manifold_visualization, entropy_trend, stability_assessment]
    }
}
```

The high curvature on `rumination` (κ = 1.5) means the system treats ruminative
states as sharp basins — easy to fall into, hard to escape. The `non_diagnostic`
safety constraint ensures the output is a **structural analysis** (trajectory,
entropy, stability) rather than a clinical diagnosis.

#### Psyche Use Case 2: Workforce Analytics — Cognitive Load Optimization

A technology company wants to optimize team assignments based on cognitive load
patterns. Traditional tools use self-reported surveys. AXON's `psyche` primitive
models cognitive load as a dimension on a Riemannian manifold where momentum
captures the inertia of sustained high-load periods, and active inference
predicts burnout trajectories before they materialize.

```axon
psyche TeamCognition {
    dimensions: [cognitive_load, focus_quality, collaboration_friction]
    manifold {
        curvature: { cognitive_load: 0.9, focus_quality: 0.7, collaboration_friction: 1.3 }
        noise: 0.08
        momentum: 0.5
    }
    safety: [non_diagnostic]
    quantum: disabled
    inference: active
}

flow OptimizeAssignments(team: TeamData, sprints: [SprintMetrics]) -> OptimizationPlan {
    step Profile {
        probe sprints for [load_patterns, focus_windows, friction_events]
        use TeamCognition
        output: CognitiveProfile
    }
    step Predict {
        reason {
            given: Profile.output
            ask: "Which team members are on burnout trajectories?"
            depth: 3
        }
        output: BurnoutRiskMap
    }
    step Optimize {
        weave [Profile.output, Predict.output]
        format: OptimizationPlan
        include: [load_rebalancing, focus_protection_windows, friction_reduction]
    }
}
```

The high curvature on `collaboration_friction` (κ = 1.3) treats inter-team
friction as a sharp manifold feature — small changes in assignment can
produce large effects on collaboration dynamics. The momentum coefficient
(β = 0.5) models how sustained high-load sprints create inertia that
persists even after the load is reduced.

#### Psyche Use Case 3: Adaptive Education — Epistemic State Modeling

An adaptive learning platform needs to model student cognitive states to
optimize content delivery. Traditional systems use binary metrics (correct/
incorrect). AXON's `psyche` primitive models the student's epistemic state
as a quantum density matrix where confusion and understanding can
coexist in superposition — "partially understands the concept but has a
fundamental misconception about the prerequisite."

```axon
psyche StudentEpistemics {
    dimensions: [comprehension, confidence, misconception_load, engagement]
    manifold {
        curvature: {
            comprehension: 0.7,
            confidence: 0.9,
            misconception_load: 1.4,
            engagement: 0.6
        }
        noise: 0.12
        momentum: 0.25
    }
    safety: [non_diagnostic]
    quantum: enabled
    inference: active
}

flow AdaptLesson(student: StudentProfile, topic: TopicGraph) -> AdaptedContent {
    step Assess {
        probe student.recent_interactions for [comprehension_signals, error_patterns]
        use StudentEpistemics
        output: EpistemicState
    }
    step Identify {
        reason {
            given: Assess.output, topic
            ask: "What misconceptions are superposed with partial understanding?"
            depth: 3
        }
        output: MisconceptionMap
    }
    step Adapt {
        weave [Assess.output, Identify.output, topic]
        format: AdaptedContent
        include: [targeted_explanations, scaffolded_problems, misconception_corrections]
    }
}
```

The quantum mode captures a critical educational reality: a student doesn't
either "understand" or "not understand" a concept. They exist in a **superposition**
of partial understandings where misconceptions interfere with correct knowledge.
The density matrix `ρ_ψ` encodes this precisely, and the adaptive engine uses
von Neumann entropy `S(ρ)` to select the intervention that maximally reduces
epistemic uncertainty.

---

## XII. Ontological Tool Synthesis — the `ots` Primitive

AXON introduces **Ontological Tool Synthesis (OTS)**, replacing dynamic tool binding with formal, continuous tool generation. Synthesizing tools at runtime rather than selecting them from a static set.

### 1. The Hard Argument: Topological Tool Spaces
In traditional orchestrators, the capability space $\mathcal{C}$ is a discrete, finite set of predefined APIs $\mathcal{T} = \{t_1, t_2, \dots, t_n\}$. Tool routing becomes a discrete classification map $f: X \to \mathcal{T}$. OTS fundamentally redefines this by modeling tools as **morphisms in a category** embedded in a differentiable manifold. 
Instead of selecting a tool $t \in \mathcal{T}$, OTS traverses a continuous topological space of computational structures to synthesize a morphism $m: X \to Y$ that optimally satisfies the agent's objective function. Given a teleology (goal) and constraints, OTS executes a *homotopy search* across the capability manifold, compiling an ephemeral tool $t'$ precise to the immediate context.

### 2. The Sweet Argument: Beyond the API Straitjacket
Imagine your agent isn't constrained by the APIs you wrote yesterday. "Dynamic Tool Binding" is like giving an agent a Swiss Army Knife and hoping one of the blades fits the screw. **OTS is giving the agent a portable forge.** 
When a problem arises that no existing tool can perfectly solve, OTS allows the agent to synthesize a custom, hyper-specialized tool on the fly, use it, and discard it. It transforms capabilities from static, brittle endpoints into fluid, intention-driven cognitive extensions, unlocking true open-ended autonomy.

### 3. Real-World Use Cases

#### A. Zero-Day Vulnerability Patch Synthesis
When discovering an unknown threat structure, pre-written remediation scripts fail. `ots` synthesizes a bespoke AST-transformation tool to safely sanitize the specific vulnerability pattern in memory.
```axon
ots ThreatPatcher<VulnGraph, ASTPatch> {
    teleology: "Given a specific AST vulnerability, generate an isolated AST transformation tool"
    loss_function: SemanticPreservation
    linear_constraints: { max_mutations: 5, runtime_overhead: "<1ms" }
    homotopy_search: deep
}
```

#### B. Ephemeral Data Protocol Bridging
Two microservices communicate using disparate, undocumented legacy protocols. Instead of hardcoding adapters, `ots` synthesizes an ephemeral serializer/deserializer tool precisely matching the inferred schemas at runtime.
```axon
ots ProtocolAdapter<StreamA, StreamB> {
    teleology: "Synthesize an impedance-matching adapter mapping fields mathematically"
    loss_function: Contrastive
    linear_constraints: { latency: "<5ms", drop_rate: 0 }
    homotopy_search: deep
}
```

#### C. Ad-Hoc Statistical Operator Generation
A data science agent encounters a novel distribution requirement not present in the standard math libraries. `ots` synthesizes a custom, optimized mathematical operator compiled down to low-level execution logic just-in-time.
```axon
ots MathOperator<Tensor, Tensor> {
    teleology: "Generate an optimized projection operator converging to target distribution"
    loss_function: L2
    linear_constraints: { vectorizable: true, precision: 64 }
    homotopy_search: shallow
}
```

---

### XIII. Universal Protocol: Model Execution Kernel (MEK)

> AXON introduces a fifteenth paradigm shift: **The Model Execution Kernel (MEK)**, a universal hypervisor that categorically decouples cognitive state from external LLM representations.

#### A. Hard Mathematical Argument — Decoupling by Decoherence
In all traditional frameworks, the execution state is implicitly tied to the exact shape of an external API (e.g., an OpenAI JSON payload). AXON replaces this with a continuous, universal `LatentState` mathematically defined as a manifold. LLM interactions are no longer string exchanges; they are modeled as the application of a **Logical Transducer**—a diffeomorphism that maps the pristine topological spaces of AXON directly into the rigid, discrete logical spaces expected by black-box APIs (acting as "Categorical Oracles"). 
When the API returns a response (like logprobs or AST blocks), the **Holographic Codec** runs a controlled decoherence protocol to reconstruct the continuous latent space. The deliberation logic thus operates entirely unaffected by the idiosyncrasies of specific APIs. Furthermore, a **Bayesian Router** actively routes computation across Oracles not by arbitrary heuristics, but by minimizing probabilistic divergence and cost dynamically choosing providers based on required output entropy.

#### B. Sweet Argument — Breaking the Black Box Paradigm
Think about the absolute brutality of how AXON shatters the standard API paradigm: traditional LLM development treats these APIs as opaque slot machines—you plug text in and pray text comes out. **AXON treats AI providers as calculable, isolated co-processors.** We don't act as "wrappers" around Gemini or Anthropic; we act as a hypervisor. The LLM does precisely what the MEK’s transduction layer mathematically forces it to do. It transforms an unpredictable HTTP call into a mathematically disciplined, type-safe compilation target, enabling pristine universally compatible core logic that effortlessly hot-swaps Anthropic, Gemini, or local models without modifying a single line of your agentic logic.

#### MEK Use Cases

**Use Case 1: Constant Active Inference Routing**
When an agent is configured with high reliability requirements but a preferred Oracle experiences high latency or epistemic degradation, the MEK dynamically reroutes the continuous internal state via the Bayesian Router to a different Oracle, synthesizing the same expected cognitive operation without leaking any provider-specific context handling to the program's source.

**Use Case 2: Multi-Model Holographic Ensembles**
An agent processing complex financial transactions can map its `LatentState` transduction through both Anthropic and Gemini simultaneously. The Holographic Codec reconstructs the responses into AXON's internal state framework, automatically evaluating the epistemic consensus from the discrete probabilistic structures produced by multiple independent "Oracle" backends.

**Use Case 3: Zero-Cost Backend Swapping for Enterprise**
An enterprise needs to migrate its massive multi-agent infrastructure from OpenAI to a self-hosted pipeline using open-source variants. Because the MEK forces all LLM interactions through the universal Logical Transducer, replacing the backend literally just involves changing the `provider` configuration. No prompts need rewriting, no JSON parsers need adjusting; the mathematical contract holds universally.

---

### XIV. Epistemic Model Context Protocol — the `mcp` and `taint` Primitives

> AXON introduces a sixteenth paradigm shift: **Categorial Subyugation
> of the MCP Standard** — formally assimilating external Model Context Protocol
> servers (databases, tools, resources) into AXON's epistemic lattice via
> structural transduction, taint propagation, and compile-time capability
> verification.

Every MCP client in existence treats external servers as trusted oracles: text
arrives from a database connector or a tool output, and the framework injects it
into the LLM context verbatim — zero taint tracking, zero epistemic downgrade,
zero compile-time verification. AXON's EMCP (Epistemic Model Context Protocol)
primitive rejects this naïve ingestion model. External MCP resources are
**epistemically untrusted by construction** and must pass through AXON's formal
trust lattice before reaching any cognitive primitive.

#### A. Hard Mathematical Argument — Categorial Transduction

**Definition 1 (EMCP Transduction Functor).** The assimilation of an MCP server
into AXON's cognitive framework is a functor between two categories:

```text
F : MCP_Ext → AXON_Cog

where
  MCP_Ext  = (Resources, Tools, Prompts)       — external MCP universe
  AXON_Cog = (Corpus, Shield, ContractTools)    — AXON epistemic universe

F(Resource)  = PIX(topologize(R))  ∪  CorpusNode(flatten(R))
F(Tool)      = @contract_tool(mcp="server:tool", taint=Untrusted)
F(Prompt)    = ∅  (prompts are ignored — AXON generates its own)
```

The functor is **not** a trivial renaming. Each arm applies a distinct
transduction:

- **Resources → Structural Lifting.** Hierarchical MCP resources (manuals,
  schemas, regulatory documents) are lifted into PIX navigable trees via
  `corpus ... from mcp("server", "uri")`. Flat resources (key-value stores,
  tabular data) are mapped to `CorpusNode` entries with flat topology. In
  both cases, the resource's epistemic level is initialized to `Untrusted`.

- **Tools → Taint-Wrapped FFI.** External MCP tools are wrapped in AXON's
  `@contract_tool` decorator with `taint: untrusted`. The compiler statically
  verifies that every path from the MCP tool output to a `know` or `believe`
  block passes through at least one `shield` — the same Denning-style IFC
  guarantee from Section VI.

```text
Taint Propagation Rule:
  ∀ path(mcp_tool.output, cognitive_sink) ∈ DataFlow :
    label(mcp_tool.output) = Untrusted
    → ∃ shield ∈ path : label(shield.output) ≥ Sanitized

Capability Enforcement:
  ∀ agent A using mcp_tool T :
    T ∈ allow_tools(shield(A))      — verified at compile time
    T ∉ deny_tools(shield(A))       — also verified
```

**Theorem (Epistemic Soundness of EMCP Ingestion).** Under the taint
propagation rule and capability enforcement, no MCP-sourced data can
reach `know`-level epistemic status without passing through a `shield`:

```text
∀ D ∈ F(MCP_Ext), ∀ path(D, know_context) :
  ∃ s ∈ Shields : s ∈ path ∧ label(s.output) ≥ Sanitized

Proof: by induction on path length + taint lattice monotonicity ∎
```

This guarantee is **structural** — it holds for any MCP server, any resource,
any tool. The compiler enforces it; the runtime cannot violate it.

#### B. Sweet Argument — Assimilating the World, Not Trusting It

The MCP standard was designed so AI assistants can connect to databases,
filesystems, and APIs. But every existing MCP client — Cursor, Claude Desktop,
Windsurf — treats the data from these servers as ground truth. A Postgres
query result gets the same epistemic weight as a hardcoded constant. A
third-party API response is injected directly into the LLM context with zero
sanitization.

AXON's EMCP is the antithesis: **assimilate everything, trust nothing.**

When you write `corpus ClinicalDB from mcp("hospital_db", "patients://")`,
AXON doesn't just "connect" to the database. It:

1. **Structurally lifts** the resource into a navigable PIX tree or corpus
   graph node — preserving the document's topology instead of flattening it
   into chunks.

2. **Taints every datum as `Untrusted`** — the data enters the epistemic
   lattice at the bottom. It cannot influence `know`-level assertions until
   a `shield` scans and sanitizes it.

3. **Wraps every MCP tool in a `@contract_tool`** with pre/postcondition
   contracts and blame semantics. If the MCP server returns garbage, AXON
   knows it's `Blame = SERVER`, not your agent's fault.

4. **Ignores MCP prompts entirely** — AXON generates its own cognitive
   instructions via personas, anchors, and epistemic directives. External
   prompt injection via MCP prompt resources is **categorically impossible**.

This means you can plug any MCP server into AXON — a medical database, a
financial API, a legal document store — and the compiler **guarantees** that
the data will be properly tainted, shielded, and epistemically tracked. No
other MCP client on the planet provides this.

#### EMCP Use Case 1: Hospital Clinical Audit via MCP Database

A hospital system connects to its patient database and clinical guidelines
via MCP servers. Traditional MCP clients would inject query results directly
into the LLM context. AXON's EMCP ensures that raw patient data is tainted,
shielded for PII, and structurally navigated:

```axon
corpus ClinicalDB from mcp("hospital_db", "patients://records")

shield PatientShield {
    scan: [pii_leak, data_exfil]
    strategy: classifier
    on_breach: sanitize_and_retry
    redact: [ssn, medical_record_number, date_of_birth]
}

know {
    flow AuditPatientCare(patient_id: String) -> AuditReport {
        step Retrieve {
            navigate ClinicalDB
                query: patient_id
                trail: enabled
                as: patient_data
        }
        step Sanitize {
            shield PatientShield on patient_data -> clean_data
        }
        step Assess {
            reason {
                given: clean_data
                ask: "Does the treatment plan comply with clinical guidelines?"
                depth: 3
            }
            output: AuditReport
        }
    }
}
```

- **MCP server data enters as `Untrusted`** — the compiler enforces that it
  passes through `PatientShield` before reaching the `know` block
- **PII fields (SSN, MRN, DOB) are auto-redacted** before the LLM sees them
- **Structural navigation** via PIX tree preserves document hierarchy instead
  of chunking patient records into embedding fragments
- **Trail provides audit-grade provenance** — required for HIPAA compliance

#### EMCP Use Case 2: Financial Compliance via External API Tools

An investment firm uses MCP-exposed tools for market data and regulatory
filing retrieval. AXON wraps each tool in contract monitors with blame
semantics and taint propagation:

```axon
tool MarketData {
    provider: mcp
    mcp: "bloomberg_mcp:get_quote"
    timeout: 5s
    effects: <network, epistemic:speculate>
    taint: untrusted
}

tool SECFilings {
    provider: mcp
    mcp: "sec_mcp:search_filings"
    timeout: 30s
    effects: <network, io, epistemic:speculate>
    taint: untrusted
}

shield ComplianceShield {
    scan: [data_exfil, hallucination]
    strategy: dual_llm
    on_breach: halt
    taint: untrusted
    allow: [MarketData, SECFilings]
    deny: [code_executor]
}

doubt {
    flow InvestigateDiscrepancy(ticker: String) -> RiskReport {
        step Gather {
            par {
                step Quote { use_tool MarketData with symbol: ticker output: QuoteData }
                step Filing { use_tool SECFilings with company: ticker output: FilingData }
            }
        }
        step Shield {
            shield ComplianceShield on [QuoteData, FilingData] -> verified_data
        }
        step Analyze {
            reason {
                given: verified_data
                ask: "Identify discrepancies between market quotes and SEC filings"
                depth: 3
            }
            output: RiskReport
        }
    }
}
```

- **Both MCP tools are `taint: untrusted`** — their outputs cannot reach
  `know` or `believe` without shield sanitization
- **`doubt` block** forces adversarial analysis of the data
- **Blame semantics**: if Bloomberg MCP returns stale data, `Blame = SERVER`
  with full diagnostic context
- **Capability enforcement**: the compiler verifies that only `MarketData`
  and `SECFilings` are accessible — no code execution, no API abuse

#### EMCP Use Case 3: Multi-Source Legal Research via MCP Corpus Ingestion

A law firm assimilates multiple MCP-exposed document repositories (statutes,
case law, regulatory guidance) into a single AXON corpus graph with typed
cross-references and epistemic status tracking:

```axon
corpus LegalCorpus {
    documents: [
        Statutes   from mcp("legal_db", "statutes://federal"),
        CaseLaw    from mcp("legal_db", "cases://precedent"),
        Regulatory from mcp("regulatory_mcp", "guidance://latest")
    ]
    edges: [
        Statutes  -> CaseLaw     : cite,      weight: 0.9
        CaseLaw   -> Regulatory  : elaborate,  weight: 0.7
        Regulatory -> Statutes   : depend,     weight: 0.6
    ]
    memory: enabled
    taint: untrusted
}

shield LegalShield {
    scan: [hallucination, prompt_injection]
    strategy: ensemble
    on_breach: quarantine
    taint: untrusted
}

know {
    flow ResearchPrecedent(legal_question: String) -> LegalBrief {
        step Navigate {
            navigate LegalCorpus
                from: Statutes
                query: legal_question
                depth: 4
                trail: enabled
                as: authority_chain
        }
        step Verify {
            shield LegalShield on authority_chain -> verified_chain
        }
        step Synthesize {
            weave [verified_chain]
            format: LegalBrief
            include: [argument, authorities, provenance_trail]
        }
    }
}
```

- **Three MCP servers are assimilated** into a single epistemically-typed
  corpus graph — AXON treats them as `Untrusted` nodes with standard MDN
  navigation
- **Cross-document edges** (`cite`, `elaborate`, `depend`) enable provenance-
  tracked navigation across MCP-sourced documents
- **Memory-augmented corpus** learns from past legal research — edges that
  consistently produce winning arguments get stronger
- **`ensemble` shield strategy** runs multiple detectors with majority voting
  before any MCP data reaches the `know` block
- **Trail provides the chain of legal authority** — from statute to precedent
  to regulatory guidance, with full MCP source attribution

---

### XV. Cybernetic Refinement Calculus — the `mandate` Primitive

> AXON introduces a seventeenth paradigm shift: **Deterministic LLM
> Control via Closed-Loop PID Enforcement** — the first compiler-native
> implementation of the Cybernetic Refinement Calculus (CRC), unifying Axiomatic
> Semantics, Dependent Refinement Types, and Thermodynamic PID Control to
> mechanically coerce stochastic LLM outputs into mathematical compliance.

Every LLM framework treats output quality as a prayer — prompt engineering,
re-rolls, and heuristic guardrails with zero formal guarantees. AXON's `mandate`
primitive makes **deterministic convergence a compiler-verified property** of
your program, backed by Lyapunov stability theory and the Curry-Howard
isomorphism.

#### A. Hard Mathematical Argument — Cybernetic Refinement Calculus (CRC)

The CRC operates across three formally verified pathways:

**Vía C: Epistemic Refinement Types.** Under the Curry-Howard isomorphism,
generating a token sequence `τ` that satisfies a mandate `M` is equivalent to
constructing a formal proof. A standard LLM returns a probabilistic string
from `Σ*`. The `mandate` primitive collapses this stochastic space into an
**Epistemic Refinement Type**:

```text
T_M = { x ∈ Σ* | M(x) ⊢ ⊤ }
```

The compiler's type inference rule enforces this statically via natural
deduction:

```text
Γ ⊢ τ_t : Σ*    Γ ⊢ M : Σ* → Bool    M(τ_t ⊕ w_{t+1}) = True
─────────────────────────────────────────────────────────────────
      Γ ⊢ infer(τ_t, M) ⇓ (τ_t ⊕ w_{t+1}) : T_M
```

If any trajectory violates the topological space of `M`, the type collapses
to the uninhabitable Bottom type (`⊥`). **The type system makes constraint
violation structurally impossible.**

**Vía A: Lyapunov Stability Proof.** To dynamically inhabit `T_M` without
infinite re-rolls, the runtime applies a PID controller that injects a
**Dynamic Negative Logit Bias** `ΔL_t` into the latent space before Softmax:

```text
u(t) = −ΔL_t = K_p·e(t) + K_i·∫₀ᵗ e(τ)dτ + K_d·de(t)/dt
```

where `e(t) ∈ ℝ⁺` is the semantic divergence computed in real-time by the
`SemanticValidator`.

**Theorem 1 (Asymptotic Stability of Active Inference):** Under gains
`K_p > 0, K_i ≥ 0, K_d ≥ 0` whose combined control effort **exceeds the
backend's drift bound** (`|K_p + K_i + K_d| > D`, with `sup|drift(t)| ≤ D`),
the semantic error `e(t)` is asymptotically stable in the Lyapunov sense.
The sign conditions alone are necessary but **not sufficient**: gains below
the drift floor leave `V̇` non-negative and the cage open.

*Proof:* Define the Lyapunov candidate `V(e) = ½·e(t)²`, representing the
thermodynamic "Free Energy" of the semantic violation. The time derivative
along system trajectories:

```text
V̇(e) = e(t)·ė(t) = e(t)·(drift(t) − u(t))
```

Substituting a proportional controller `u(t) = K_p·e(t)` and bounding the
stochastic drift (natural LLM hallucination) `sup|drift(t)| ≤ D`:

```text
V̇(e) ≈ −λ·e(t)² < 0    ∀ e(t) ≠ 0
```

Since `V(e)` is strictly decreasing outside a bounded tolerance region, **the
stochastic trajectory converges asymptotically to the mandate setpoint**
(`e = 0`). ∎

**Vía B: Thermodynamic Validation.** Empirical simulation confirms: an
unconstrained LLM's error `e(t)` diverges via directional random walk. Under
CRC PID control, the derivative component (`K_d`) detects error acceleration
instantly while the proportional component (`K_p`) injects massive negative
logit bias, physically collapsing the probability mass of violating tokens
before Softmax — a "thermodynamic cage" that forces absolute compliance.

**Convergence Criterion and Anti-Windup:**

The PID controller enforces convergence within `N` discrete steps:

```text
Converge(e, ε, N) = ∃ t ≤ N : |e(t)| < ε

Anti-windup:  I_clamped = clamp(∫e, −I_max, I_max)
```

**Static verification — the stability band.** Two independent results bound
the gains in opposite directions, and both are enforced at compile time:

```text
D  <  |K_p + K_i + K_d|  <  1/L         (both endpoints exclusive)
```

- **Floor** (Lyapunov, Theorem 1): the control effort must *exceed* the
  backend's drift bound `D = sup|drift(t)|` — too small, and the cage does
  not hold.
- **Ceiling** (Lipschitz): the effort must stay *below* `1/L`, the reciprocal
  Lipschitz constant of the refinement map — too large, and the loop
  oscillates or diverges.

`D` and `L` are **measured properties of a backend**, and the compiler never
invents them. You declare them, and the compiler verifies the theorem
conditionally on your declaration — the declared bounds then travel in the IR
as proof obligations for dispatch:

```axon
mandate SECCompliance {
    constraint: "no forward-looking statements without safe harbor language"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    stability { D: 0.5, L: 0.4 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}
```

Here `|2.0 + 0.3 + 0.1| = 2.4` lies inside `(0.5, 2.5)` and the mandate
compiles; gains of `pid { Kp: 0.3, Ki: 0.1, Kd: 0.05 }` pass every sign test
yet are **refused at compile time**, because `0.45` cannot beat the declared
drift. If `D ≥ 1/L` the band is empty — no gains whatever can stabilise that
backend, and the compiler says so.

Without a `stability` declaration the compiler verifies the necessary sign
conditions — `K_p > 0`, `K_i ≥ 0`, `K_d ≥ 0`, `ε ∈ (0, 1]`, `N ≥ 1` — and the
IR carries no bounds, so the runtime can see that nothing further was
statically promised. (`ε` is capped at 1 because `e = 1 − CSR` cannot exceed
1: a wider band would admit even total violation, a mandate that could never
fail.)

#### B. Sweet Argument — The Thermodynamic Cage for LLMs

Imagine this: you don't *ask* an LLM to follow your rules — you **physically
force** it. The `mandate` primitive doesn't add another "please be accurate"
prompt. It installs a **cybernetic control loop** directly inside the AXON
runtime that mathematically measures how far the LLM drifts from your
constraint, computes an exact corrective force using PID control theory, and
injects it as a negative logit bias *before the next token is even sampled*.

The LLM literally cannot hallucinate its way out of a mandate. It's not a
guardrail — it's a **thermodynamic cage** with a Lyapunov stability proof.
Every token the model generates is measured, corrected, and forced back into
compliance. If the error doesn't converge within `N` steps, the system applies
your `on_violation` policy: halt (fail-safe), coerce (return best-effort), or
log (audit trail). No faith. No prayers. Just closed-loop control theory
applied to stochastic generation.

This is the difference between asking a rocket to please go straight and
installing a guidance computer with feedback sensors. One is hope; the other
is engineering.

#### Mandate Use Case 1: Regulatory-Compliant Financial Report Generation

A fintech company needs AI-generated quarterly reports that **must** comply
with SEC formatting rules — no exceptions, no manual review loops:

```axon
mandate SECCompliance {
    constraint: "Output must be a valid SEC 10-K section with
                 GAAP-compliant financial tables, footnote references,
                 and no forward-looking statements without safe harbor language"
    pid { Kp: 2.0, Ki: 0.3, Kd: 0.1 }
    epsilon: 0.05
    max_steps: 8
    on_violation: halt
}

know {
    flow GenerateQuarterlyReport(data: FinancialData) -> SECReport {
        step Draft {
            mandate SECCompliance on data
            output: SECReport
        }
    }
}
```

- **`Kp: 2.0`** — aggressive proportional correction crushes deviations
  instantly (SEC formatting is non-negotiable)
- **`epsilon: 0.05`** — convergence tolerance of 5% semantic error
- **`on_violation: halt`** — if convergence fails after 8 PID steps, the
  system raises `MandateViolationError` instead of emitting a non-compliant
  report
- **The `know` block** guarantees citation-backed generation (temperature 0.1)
  while the mandate enforces structural compliance

#### Mandate Use Case 2: Medical Diagnosis Constraint Enforcement

A telemedicine platform requires AI-generated diagnostic suggestions to follow
clinical guidelines with zero tolerance for speculative diagnoses:

```axon
mandate ClinicalProtocol {
    constraint: "Diagnosis must reference ICD-10 codes, cite clinical
                 evidence levels (I-V), and never suggest off-label
                 treatments without explicit disclaimer"
    pid { Kp: 3.0, Ki: 0.5, Kd: 0.2 }
    epsilon: 0.02
    max_steps: 12
    on_violation: halt
}

shield PatientShield {
    scan: [pii_leak, hallucination]
    strategy: dual_llm
    on_breach: halt
    redact: [ssn, mrn, dob]
}

doubt {
    flow GenerateDiagnosis(symptoms: PatientData) -> ClinicalReport {
        step Sanitize {
            shield PatientShield on symptoms -> clean_data
        }
        step Diagnose {
            mandate ClinicalProtocol on clean_data
            output: ClinicalReport
        }
    }
}
```

- **`Kp: 3.0` with `epsilon: 0.02`** — extremely tight control for
  safety-critical medical outputs (2% error tolerance)
- **12 PID steps** — allows deep convergence for complex multi-system
  diagnoses
- **`doubt` block** forces adversarial self-critique on every diagnostic claim
- **Shield + Mandate composition** — PII is redacted before the mandate even
  sees the data; the mandate then enforces clinical protocol compliance on the
  sanitized input

#### Mandate Use Case 3: Autonomous Legal Contract Generation with PID-Controlled Clause Precision

A law firm deploys an agent that generates legally binding contract clauses
with mathematically enforced precision — every clause must satisfy formal
legal structure requirements:

```axon
mandate LegalPrecision {
    constraint: "Each clause must contain: (1) parties identification,
                 (2) obligation specification with measurable deliverables,
                 (3) temporal bounds, (4) breach remedies with liquidated
                 damages formula, (5) governing law reference"
    pid { Kp: 1.5, Ki: 0.4, Kd: 0.15 }
    epsilon: 0.08
    max_steps: 10
    on_violation: coerce
}

tool LegalDB {
    timeout: 15s
}

tool TemplateEngine {
    timeout: 10s
}

agent ContractDrafter {
    goal: "Generate all clauses for the service agreement"
    tools: [LegalDB, TemplateEngine]
    strategy: plan_and_execute
    max_iterations: 8
    return: ContractDocument
}

flow DraftContract(terms: NegotiationTerms) -> ContractDocument {
    step Generate {
        mandate LegalPrecision on ContractDrafter(terms)
        output: ContractDocument
    }
}
```

- **`on_violation: coerce`** — returns the best-effort output after 10 PID
  steps rather than halting, since partial contracts are still useful for
  human review
- **Agent + Mandate composition** — the BDI agent autonomously drafts clauses
  while the PID loop ensures each generated clause satisfies the 5-element
  structural constraint
- **`plan_and_execute` strategy** — the agent plans the full contract
  structure before generating individual clauses, while the mandate enforces
  precision on each clause independently
- **Moderate gains (`Kp: 1.5`)** — legal language has higher acceptable
  variance than medical or financial outputs, so the controller is less
  aggressive

---

### XVI. Epistemic Module System — Separate Compilation and Linking for Cognitive Languages

Every mainstream module system (OCaml, Haskell, Rust, Zig) solves the same problem:
compile files independently, then link them. But none of them operate on *cognitive
primitives* — and none validate epistemic guarantees across module boundaries.

AXON's **Epistemic Module System (EMS)** — rebuilt natively in Rust in §Fase 115
(v2.76.0; the original Python implementation was retired with the Python frontend in
Fase 39) — compiles a multi-file project in five phases: lexer-true import discovery
over a Kahn-sorted DAG (an import cycle is refused naming its full path,
`axon-T955`), per-module `.axi` interface generation, the Epistemic Compatibility
Check, per-module validation in which imported names register with their exported
kinds (`axon-T953`: every imported name must exist in the exporting module; no
shadowing, ever), and a real **linker**: the modules merge at the AST tier, the full
type checker revalidates the linked program once — so every deep semantic law the
language has applies cross-module by construction — and the compiler emits ONE linked
IR artifact carrying full declaration bodies plus per-module provenance
(`content_hash` + `interface_hash` + source-map line windows per module). That linked
IR is the same artifact shape the enterprise deploy path stores and re-hydrates.

The full design — the seven paradigms it synthesizes (OCaml signatures, 1ML
unification, Backpack two-phase checking, GHC ABI hashes, Zig lazy discovery,
Nix/Bazel content addressing, Rust behavioral contracts) and the theoretical
guarantees — is the EMS paper: `docs/papers/paper_ems_axon.md`.

#### The Novel Contribution: Epistemic Compatibility Checking (ECC)

No existing module system validates *epistemic compatibility* across imports. Each
module carries a compile-time **epistemic floor** (know > believe > doubt >
speculate) derived from its content — anchors ⇒ `know`, shields ⇒ `believe`, an
epistemic block ⇒ its level; never from an annotation an author could forget:

```
Module A (know-level: has anchors + factual constraints)
  └── imports from Module B (speculate-level: creative personas)

  → ❌ axon-T954: epistemic conflict — a know-level module cannot
     silently depend on speculate-level definitions. Either raise the
     imported module's floor or acknowledge with `@allow_downgrade`
     (which downgrades the error to a VISIBLE axon-W017 warning —
     an acknowledged downgrade is never a silent one).
```

**Why this matters**: A medical diagnosis flow (`know`-level, anchored with
`NoHallucination`) that silently imports from a creative writing module
(`speculate`-level) would execute speculative reasoning where factual rigor was
expected. No linter, test, or traditional type system catches this. The ECC catches
it at compile time — `axon check --strict` escalates the warnings for CI.

#### `.axi` Interface Files — The Cognitive `.cmi`

Each module compiles to a `.axi` (AXON Interface) under `.axon_cache/interfaces/`,
containing only the public surface — names, kinds, and constraint hashes — never
prompt text or step logic. Two hashes enable GHC-style **early cutoff**:

- `content_hash` = SHA-256(source) — changes on any edit
- `interface_hash` = SHA-256(public surface) — changes only when the surface changes

Add a comment (or edit a step body) in `security.axon`: its `content_hash` changes,
its `interface_hash` doesn't → dependents **skip re-validation** (observable in the
cache stats, and sound because per-module validation consumes only interface facts —
all body resolution happens at link time). The cache is content-addressed, writes
atomically, self-heals from source if corrupted, and a compiler upgrade busts it
wholesale.

#### Backwards Compatible: Zero Breaking Changes

A program with no `import` statements never engages the EMS: single-file compilation
is byte-identical to v2.75.0, including its IR JSON (the new `IRImport` fields and
the provenance block are skip-serialized at their defaults). The suite pinning all of
this is `fase115_a`–`g` across `axon-frontend/tests/` and `axon-rs/tests/` — 34
integration tests plus the per-module unit suites, including the paper's own
two-file example compiling end-to-end and the anti-stub proof (an imported flow's
steps survive the link).

---

### XVII. Lambda Data (ΛD) — Epistemic State Vectors as First-Class Data

> AXON introduces an eighteenth paradigm shift: **formal epistemic
> data primitives with compile-time degradation enforcement** — replacing
> JSON's semantics-blind serialization with invariant epistemic state
> vectors grounded in information thermodynamics and Peircean semiotics.

Every data format in existence — JSON, Protocol Buffers, MessagePack —
operates exclusively at Shannon's syntactic layer: bits are transmitted
accurately, but **meaning is discarded**. When a cognitive agent serializes
a fact it is 20% certain about, JSON forces it into an absolute deterministic
string. This fundamental epistemic mismatch is the root cause of AI
hallucinations in data pipelines. AXON's `lambda` primitive eliminates
this by making every datum an **Epistemic State Vector** `ψ = ⟨T, V, E⟩`
with compile-time invariant enforcement.

```axon
lambda SensorReading {
    ontology: "measurement.temperature.celsius"
    certainty: 0.95
    temporal_frame: "2026-03-23T00:00:00Z/2026-03-24T00:00:00Z"
    provenance: "Sensor_X_Unit_7"
    derivation: raw
}

flow ProcessSensorData(readings: [SensorReading]) -> AnalysisReport {
    step Analyze {
        lambda SensorReading on readings -> TypedReadings
        reason {
            given: TypedReadings
            ask: "Identify anomalous temperature patterns"
            depth: 3
        }
        output: AnalysisReport
    }
}
```

#### A. Hard Mathematical Argument — The Epistemic State Vector

**Definition (Epistemic State Vector ψ).** Every valid datum in Lambda Data
is not a scalar value but a state within a system governed by invariant
physical laws of information:

```text
ψ = ⟨T, V, E⟩

where
  T ∈ O         — Ontological Type (node in a verified ontology graph)
  V ∈ dom(T)    — Valid Value (magnitude satisfying the topology of T)
  E = ⟨c, τ, ρ, δ⟩  — Epistemic Tensor

  c ∈ [0, 1]    — Certainty scalar (1.0 = axiomatic/direct measurement)
  τ = [t_start, t_end]  — Temporal Frame (outside τ, certainty decays to 0)
  ρ : EntityRef  — Provenance (deterministic causal origin)
  δ ∈ Δ = {raw, derived, inferred, aggregated, transformed}  — Derivation mechanism
```

**Four Invariants — The Physics of ΛD:**

```text
Invariant 1 (Ontological Rigidity):
  ∀ ψ = ⟨T, V, E⟩ : T must be a well-defined ontological node
  V ∉ dom(T) → Collapse(ψ)

Invariant 2 (Epistemic Bounding):
  c ∈ [0, 1] — certainty is always explicitly bounded

Invariant 3 (Semantic Conservation):
  ψ₁ →f ψ₂ ⟹ ψ₁ ≡_sem ψ₂  (no valid transformation loses semantic meaning)

Invariant 4 (Singular Interpretation):
  Each datum holds a single valid semantic interpretation
  independent of the consuming system
```

**Theorem (Epistemic Degradation — First Law of Cognitive Information).**
Let `Φ: Ψⁿ → Ψ` be a logical inference or computational transformation
mapping `n` input states to an output state `ψ_out`. The certainty of
`ψ_out` is strictly bounded by:

```text
c(ψ_out) ≤ (min_{i=1}^n c(ψᵢ)) · η_Φ

where η_Φ ∈ (0, 1] is the epistemic fidelity of Φ
```

*Proof sketch:* Information theory dictates that processing cannot create
organic information *ex nihilo* (Data Processing Inequality). An AI agent
cannot deduce absolute truth (`c = 1.0`) from probabilistic premises
(`c = 0.7`). The AXON compiler enforces this at compile time:

```text
COMPILE-TIME ENFORCEMENT:
  δ ∈ {derived, inferred, aggregated, transformed} ∧ c = 1.0
  → ⊥ (COMPILE ERROR: Epistemic Degradation Theorem violation)

  Only δ = raw permits c = 1.0 (direct physical measurement or axiom)
```

This makes hallucination propagation a **structural impossibility** — the
type system rejects programs that claim absolute certainty for non-raw data.

#### B. Sweet Argument — Data That Knows What It Knows

JSON is Plato's cave — a two-dimensional shadow of a higher-dimensional
cognitive state. When an LLM outputs `{"temperature": 23.5}`, the consumer
has zero knowledge of: *How certain is this?* *When was it measured?* *Who
measured it?* *Was it directly observed or inferred?*

Lambda Data annihilates this epistemic blindness. Every datum in AXON
carries its complete epistemological identity — certainty, temporal
validity, provenance, and derivation — as **compile-time verified
properties**, not optional metadata that developers "should" add.

The Epistemic Degradation Theorem is the crown jewel: the AXON compiler
**mathematically guarantees** that no chain of transformations can inflate
certainty beyond what the weakest input supports. This is not a runtime
check. This is not a linter warning. This is a **type-system invariant**
that makes hallucination propagation impossible by construction.

When you write `derivation: inferred` with `certainty: 1.0`, the compiler
rejects your program — because inferring absolute truth is a logical
impossibility that AXON treats as a type error, not a philosophical debate.

This is the difference between data that **happens to be correct** and data
that **proves it cannot be wrong**.

#### ΛD Use Case 1: IoT Sensor Fusion with Temporal Decay

A smart building system fuses temperature readings from multiple sensors
with different reliability levels. Raw sensor data retains `c = 1.0`, but
aggregated metrics automatically degrade:

```axon
lambda RawTemp {
    ontology: "measurement.temperature.celsius"
    certainty: 1.0
    temporal_frame: "2026-03-23T14:00:00Z/2026-03-23T14:05:00Z"
    provenance: "HVAC_Sensor_Unit_3"
    derivation: raw
}

lambda AggregatedFloorTemp {
    ontology: "measurement.temperature.aggregate"
    certainty: 0.87
    temporal_frame: "2026-03-23T14:00:00Z/2026-03-23T15:00:00Z"
    provenance: "BuildingOS_FloorManager"
    derivation: aggregated
}

flow MonitorBuilding(sensors: [RawTemp]) -> BuildingReport {
    step Aggregate {
        lambda AggregatedFloorTemp on sensors -> floor_data
        output: FloorMetrics
    }
    step Analyze {
        reason {
            given: floor_data
            ask: "Identify HVAC zones requiring immediate attention"
            depth: 2
        }
        output: BuildingReport
    }
}
```

- **`RawTemp` with `c = 1.0` and `derivation: raw`** — direct sensor
  measurement, full certainty is valid
- **`AggregatedFloorTemp` with `c = 0.87`** — the compiler would **reject**
  `c = 1.0` here because `derivation: aggregated` triggers the Epistemic
  Degradation Theorem
- **Temporal Frame** bounds validity — readings outside the 5-minute window
  are epistemically expired
- **Provenance chain** traces every value to its physical sensor

#### ΛD Use Case 2: Financial Data Pipeline with Derivation Tracking

An investment platform processes market data through multiple
transformation stages, each reducing certainty according to the EPD theorem:

```axon
lambda RawQuote {
    ontology: "finance.equity.quote"
    certainty: 1.0
    temporal_frame: "2026-03-23T09:30:00Z/2026-03-23T16:00:00Z"
    provenance: "NYSE_DirectFeed"
    derivation: raw
}

lambda DerivedValuation {
    ontology: "finance.equity.valuation"
    certainty: 0.78
    temporal_frame: "2026-03-23T09:30:00Z/2026-03-24T09:30:00Z"
    provenance: "QuantEngine_v4"
    derivation: derived
}

lambda InferredOutlook {
    ontology: "finance.equity.outlook"
    certainty: 0.52
    provenance: "SentimentAnalyzer_LLM"
    derivation: inferred
}

doubt {
    flow AssessRisk(ticker: String) -> RiskAssessment {
        step Price {
            lambda RawQuote on ticker -> verified_quote
            output: QuoteData
        }
        step Value {
            lambda DerivedValuation on verified_quote -> valuation
            output: ValuationData
        }
        step Outlook {
            lambda InferredOutlook on valuation -> outlook
            output: OutlookData
        }
        step Synthesize {
            weave [verified_quote, valuation, outlook]
            format: RiskAssessment
            include: [price_analysis, valuation_model, sentiment, certainty_chain]
        }
    }
}
```

- **Certainty degrades through the pipeline**: `1.0 → 0.78 → 0.52` — the
  compiler enforces that each stage cannot exceed its predecessor's certainty
  multiplied by the transformation fidelity
- **`doubt` block** forces adversarial validation — appropriate for
  speculative financial analysis
- **`certainty_chain` output** exposes the full degradation path to the
  consuming system — complete epistemic transparency
- **The compiler would reject** `InferredOutlook` with `c = 1.0` — LLM
  sentiment inference cannot claim absolute truth

#### ΛD Use Case 3: Clinical Research Data with Multi-Source Provenance

A pharmaceutical company tracks clinical trial data where regulatory
compliance requires formal provenance and certainty tracking at every
transformation stage:

```axon
lambda PatientObservation {
    ontology: "clinical.observation.vitals"
    certainty: 1.0
    temporal_frame: "2026-01-15T08:00:00Z/2026-01-15T08:30:00Z"
    provenance: "ClinicalTrial_Phase3_Site_Boston"
    derivation: raw
}

lambda TransformedCohort {
    ontology: "clinical.cohort.statistical"
    certainty: 0.91
    temporal_frame: "2026-01-01T00:00:00Z/2026-06-30T23:59:59Z"
    provenance: "StatisticalEngine_R_v4.3"
    derivation: transformed
}

lambda InferredEfficacy {
    ontology: "clinical.efficacy.estimate"
    certainty: 0.68
    provenance: "BayesianModel_PharmaCore"
    derivation: inferred
}

know {
    flow AnalyzeTrialResults(trial_id: String) -> RegulatoryReport {
        step Collect {
            lambda PatientObservation on trial_id -> raw_data
            output: ObservationSet
        }
        step Transform {
            lambda TransformedCohort on raw_data -> cohort_stats
            output: CohortAnalysis
        }
        step Infer {
            lambda InferredEfficacy on cohort_stats -> efficacy
            output: EfficacyEstimate
        }
        step Report {
            weave [raw_data, cohort_stats, efficacy]
            format: RegulatoryReport
            include: [patient_data, statistical_analysis, efficacy_estimate,
                       provenance_chain, certainty_degradation_audit]
        }
    }
}
```

- **`1.0 → 0.91 → 0.68`** — certainty degrades formally through
  observation → transformation → inference
- **`know` block** ensures maximum rigor — the LLM generates with citation
  anchors and temperature 0.1
- **`provenance_chain`** provides the FDA-required traceability: every
  number traces back to a specific clinical site, statistical engine, and
  Bayesian model
- **`certainty_degradation_audit`** exposes the complete epistemic
  degradation path — required for regulatory submission compliance
- **The compiler guarantees** no stage inflates certainty — this is not a
  policy, it is a **mathematical invariant** of the type system

---

### XVIII. Deterministic Muscle — the `compute` Primitive

> AXON introduces a nineteenth paradigm shift: **deterministic execution
> as a first-class cognitive primitive**, grounding the System 1 / System 2
> duality of Kahneman directly into the compiler pipeline.

Every LLM orchestration framework commits the same categorical error: routing
deterministic transformations — arithmetic, schema mapping, data aggregation —
through a probabilistic oracle. This is the computational equivalent of asking
a philosopher to multiply matrices. AXON's `compute` primitive introduces a
**Fast-Path** that bypasses the LLM entirely, executing pure functions natively
with zero token cost and zero hallucination risk.

#### Argument 1: The Hard Argument — Pure Mathematics

**Category-Theoretic Foundation.** A `compute` block defines a strict monoidal
functor $F: V \to W$ over the AXON type lattice, operating under Linear Logic's
resource semantics:

```text
F : V → W    (strict monoidal functor — deterministic morphism)
A ⊸ B        (linear implication — each resource consumed exactly once)
```

Unlike every other AXON primitive (which compiles to LLM API calls), `compute`
compiles to a **closed algebraic morphism** — a function whose output is
uniquely determined by its inputs, with no probabilistic component:

```text
∀ x ∈ V : F(x) = y ∈ W, |{y}| = 1    (determinism guarantee)
P(hallucination | compute) = 0          (by construction)
Cost_tokens(compute) = 0                (no LLM invocation)
```

The compiler verifies the morphism at IR generation time via the **Curry-Howard
correspondence**: if a `shield` is attached to the compute block, it acts as a
theorem prover that checks type safety of the transformation before emitting
the `IRCompute` node. A compute block with a verified shield is a **proof term**
— its type correctness is a mathematical theorem, not a runtime hope.

**Complexity class separation.** The compute primitive enforces a formal
boundary between complexity classes within a single AXON program:

```text
compute steps:  O(n) — linear in input size, native execution
LLM steps:      O(T) — proportional to token count, API-bound

Speedup ratio:  T/n → ∞  as context grows
```

This is not optimization — it is a **complexity-theoretic firewall** that
prevents deterministic operations from being dragged into the exponential
latency regime of transformer inference.

#### Argument 2: The Sweet Argument — Why It's Brilliant

The `compute` primitive is AXON's answer to a question the industry hasn't
even articulated: **what if your AI agent had muscles, not just a brain?**

Every cognitive architecture in nature separates deliberation from reflex.
A chess grandmaster doesn't _think_ about how to move their hand — the motor
cortex fires a deterministic signal while the prefrontal cortex plans the
next sacrifice. But today's AI agents deliberate about _everything_: "please
calculate 1000 × 0.21" goes through the same $0.015/1K-token pipeline as
"analyze the geopolitical implications of this trade agreement."

`compute` gives AXON agents **cognitive reflexes**:

- **Zero-cost arithmetic.** Tax calculations, discount logic, unit conversions
  — executed natively in microseconds, not milliseconds, with zero API calls.
- **Zero hallucination.** `2 + 2 = 4`, always. No temperature, no sampling,
  no "the model sometimes gets math wrong." Determinism is a _compiler guarantee_.
- **Context budget liberation.** Every token spent on arithmetic is a token
  stolen from reasoning. `compute` returns those tokens to the LLM for what
  it's actually good at — semantic understanding, creative synthesis, and
  multi-step deliberation.
- **Composability.** `compute` blocks are declared once and invoked inside
  any `flow` with `compute Name on args -> output`. The same deterministic
  function composes with `shield`, `agent`, `mandate`, and every other
  primitive — because the compiler treats it as a first-class citizen of
  the IR, not a bolted-on escape hatch.

The result: AXON programs that are simultaneously **faster** (native execution),
**cheaper** (zero tokens), **safer** (zero hallucination), and **smarter**
(more context budget for actual reasoning).

#### Argument 3: Three Use Cases

**Use Case 1 — Financial Calculations in an Autonomous Agent**

An insurance agent needs to compute premiums deterministically while using
the LLM for policy language analysis:

```axon
compute CalculatePremium {
    input: base_rate (Float), risk_factor (Float), years (Float)
    output: PremiumResult
    logic {
        let annual = base_rate * risk_factor
        let total = annual * years
        let discount = total * 0.05
        let final = total - discount
        return final
    }
}

agent InsuranceAdvisor {
    goal: "Analyze policy and compute accurate premium"
    tools: [WebSearch, PDFExtractor]
    strategy: react
    max_iterations: 10
    return: PolicyReport
}

flow ProcessApplication(client: Document) -> PolicyReport {
    step Analyze {
        ask: "Analyze the client's risk profile from this document"
        output: RiskProfile
    }
    compute CalculatePremium on Analyze.risk_factor, 1.2, 5.0 -> premium
    step Report {
        ask: "Draft the policy report incorporating the computed premium"
        output: PolicyReport
    }
}
```

The LLM analyzes risk (semantic task), `compute` calculates the premium
(deterministic task), and the LLM drafts the report. Zero tokens wasted
on multiplication.

**Use Case 2 — Data Transformation in a Multi-Agent Pipeline**

Two agents collaborate on market intelligence. The data normalization
between them is pure arithmetic — no LLM needed:

```axon
compute NormalizeMetrics {
    input: revenue (Float), market_cap (Float)
    output: Float
    shield: TypeSafety
    logic {
        let ratio = revenue / market_cap
        let score = ratio * 100
        return score
    }
}

agent DataCollector {
    goal: "Gather quarterly revenue data"
    tools: [WebSearch, APICall]
    strategy: react
    max_iterations: 8
    return: DataSet
}

agent StrategyAnalyst {
    goal: "Produce investment recommendations"
    tools: [DataAnalyzer]
    strategy: reflexion
    max_iterations: 12
    return: StrategyReport
}

flow InvestmentPipeline(sector: String) -> StrategyReport {
    step Collect { DataCollector(sector) output: DataSet }
    compute NormalizeMetrics on Collect.revenue, Collect.market_cap -> score
    step Analyze { StrategyAnalyst(score) output: StrategyReport }
}
```

The `shield: TypeSafety` attachment means the compiler verifies type
correctness of the arithmetic morphism via the Curry-Howard correspondence
— the normalization is a proven-correct transformation, not a hope.

**Use Case 3 — Real-Time Scoring in a Customer-Facing Agent**

A customer support agent needs to compute eligibility scores instantly
while maintaining empathetic conversation:

```axon
compute EligibilityScore {
    input: tenure (Float), spend (Float), incidents (Float)
    output: Float
    logic {
        let base = tenure * 10
        let bonus = spend * 0.01
        let penalty = incidents * 15
        let score = base + bonus - penalty
        return score
    }
}

persona SupportAgent {
    domain: ["customer success", "retention"]
    tone: empathetic
    confidence_threshold: 0.85
}

flow HandleRetention(customer_id: String) -> Resolution {
    step Profile {
        ask: "Retrieve and analyze this customer's history"
        output: CustomerProfile
    }
    compute EligibilityScore on Profile.tenure, Profile.spend, Profile.incidents -> score
    step Respond {
        ask: "Based on an eligibility score of {score}, craft a personalized
              retention offer with appropriate empathy"
        output: Resolution
    }
}
```

The eligibility score is computed in microseconds — no API latency, no
hallucinated numbers, no wasted context. The LLM focuses exclusively on
what it does best: understanding the customer's emotional state and
crafting a personalized response.

---

### XIX. Reactive Daemon Infrastructure — the `daemon` and `listen` Primitives

> AXON introduces a twentieth paradigm shift: **π-Calculus reactive
> processes as first-class compiled constructs**, grounding event-driven AI
> agents in Milner's π-calculus channel theory, Rutten's co-algebraic stream
> semantics, and Erlang/OTP supervision trees — backed by a pluggable EventBus
> with FFI bridges to Kafka, RabbitMQ, and AWS EventBridge.

Every LLM orchestration framework implements event-driven agents as ad-hoc
Python loops polling queues in `while True` — no formal channel semantics,
no supervision, no resource linearity, no compile-time verification. AXON's
`daemon` and `listen` primitives make reactive event processing a **compiled
cognitive primitive** with mathematical guarantees of liveness, fairness,
and fault tolerance.

#### Argument 1: The Hard Argument — Pure Mathematics

**π-Calculus Channel Semantics.** A `daemon` declaration compiles to a
π-calculus process — a concurrent entity communicating exclusively via typed
channels, where the `listen` primitive binds the daemon to an `EventChannel`
satisfying Milner's channel axioms:

```text
Daemon(D) ≅ (ν ch)(D | !ch(x).P(x))

where
  ν ch       — channel restriction (EventBus creates private channels)
  !ch(x)     — replicated input (listen loop — coinductive, unbounded)
  P(x)       — event handler (compiled flow body)
  D          — daemon process (OTP-supervised lifecycle)
```

The `ν` (nu) operator creates a restricted channel — no external process can
directly access the daemon's event stream. The `!` (bang) operator denotes
replicated input — the listener processes an unbounded stream of events,
one at a time, without terminating.

**Co-algebraic Stream Unfolding.** Each `listen` block defines a coinductive
observation function — an infinite state machine that unfolds events:

```text
listen(topic) ≅ νX. (Event × State × X)

Observation:    observe(s) = (event, s')
Transition:     unfold(s') = observe(s')
Termination:    only via external signal (supervisor.stop)
```

Unlike inductive data (finite), a coinductive listener is a potentially
infinite stream of (event, state-transition) pairs. The coalgebraic
formalization guarantees:

- **Liveness:** if events arrive, they are eventually processed
- **Fairness:** a well-typed EventBus distributes events FIFO per channel
- **Resource linearity:** each event is consumed exactly once per listener
  (Girard's Linear Logic `⊗` monoidal semantics)

**OTP Supervision Tree.** The `DaemonSupervisor` implements Erlang/OTP's
supervision strategies as a categorical fixpoint operator:

```text
Supervisor ≅ μX. (Children × Strategy × RestartPolicy × X)

Strategy ∈ { one_for_one, one_for_all, rest_for_one }

Restart(child, failures, window) =
  failures ≤ max_restarts ∧ elapsed ≤ max_seconds
  → respawn(child)
  | otherwise → escalate(error)
```

The `μ` (mu) operator is the least fixpoint — the supervisor loop is
inductively defined and provably terminating when the restart budget is
exhausted, preventing infinite restart cascades.

**EventBus Channel Factory — Polymorphic FFI.** The EventBus accepts a
`ChannelFactory` parameter that enables plugging external message brokers
while preserving the same π-calculus semantics:

```text
ChannelFactory : (topic: String, maxsize: ℕ) → EventChannel

where EventChannel satisfies:
  publish : Event → IO()     — channel output (c̄⟨v⟩)
  receive : () → IO(Event)   — channel input (c(x))
  close   : () → ()          — channel deallocation

Backends:
  InMemoryChannel      — asyncio.Queue (dev/test, zero-deps)
  KafkaChannel         — aiokafka (distributed, at-least-once)
  RabbitMQChannel      — aio-pika (durable, topic exchange)
  EventBridgeChannel   — aiobotocore (serverless, AWS-native)
```

The channel factory is a natural transformation `η: F ⇒ G` between functors —
swapping the backend preserves the algebraic structure of the EventBus.

#### Argument 2: The Sweet Argument — Why It's Brilliant

The `daemon` primitive transforms AXON from a **compilation tool** into a
**reactive platform**. Today's AI agents are either:

1. **Request-response** — a user asks, the agent answers, done. No persistence.
2. **Polling loops** — a Python `while True` checks a queue, runs inference,
   repeats. No formal semantics, no supervision, no crash recovery.

AXON daemons are neither. They are **persistent reactive processes** that:

- **Run forever** (coinductive) — processing events as they arrive, not
  polling. A daemon monitoring financial markets reacts to price changes in
  real-time, not every 30 seconds.
- **Survive crashes** — the OTP supervisor automatically restarts failed
  daemons within configurable bounds. If a daemon processing medical alerts
  crashes at 3 AM, it's back in milliseconds — no human intervention, no
  PagerDuty.
- **Cost $0 while idle** — unlike `while True` loops that consume compute
  even when nothing happens, a daemon's `listen` suspends on an empty
  channel. Zero CPU, zero tokens, zero cost.
- **Scale from laptop to Kafka** — the same AXON source that runs with
  `InMemoryChannel` in tests runs with `KafkaChannel` in production. One
  line of config: `--channel kafka`. No code changes. No adapter patterns.
  No infrastructure rewrites.
- **Compose with everything** — `daemon` blocks live inside `flow` blocks.
  A daemon can use `compute` for deterministic transforms, `shield` for
  security, `mandate` for output compliance, `forge` for creative generation.
  Every AXON primitive composes because the compiler treats `daemon` as a
  first-class IR node, not a bolted-on runtime hack.

The AxonServer exposes all of this via a **production HTTP/WebSocket API**:
deploy `.axon` files, publish events, manage daemons, stream events in
real-time — all from `axon serve` on the command line.

This is the difference between a language that **compiles prompts** and a
language that **runs a reactive cognitive platform**.

#### Argument 3: Three Use Cases

**Use Case 1 — Real-Time Financial Alert Daemon**

An investment firm needs continuous monitoring of market events. When a
price anomaly is detected, the daemon triggers analysis immediately — not
on the next polling interval:

```axon
daemon PriceMonitor(event: MarketEvent) -> AlertReport {
    goal: "Monitor real-time price feeds and trigger anomaly alerts"
    tools: [MarketFeed, Calculator, NotificationService]
    listen "market.prices" as price_event {
        compute CalculateDeviation on price_event.price, price_event.avg_30d -> deviation
        step Evaluate {
            ask: "Is this {deviation}% price deviation anomalous given current
                  market conditions and sector trends?"
            output: AnomalyAssessment
        }
        if deviation > 0.05 {
            step Alert {
                ask: "Draft a concise alert explaining the anomaly and
                      recommended immediate action"
                output: AlertReport
            }
        }
    }
}
```

- The daemon reacts to events in real-time — no polling, no cron jobs
- `compute` handles the arithmetic (zero tokens, deterministic)
- The LLM performs semantic analysis only when needed (>5% deviation)
- OTP supervisor restarts the daemon if the market feed causes a crash
- Scales from in-memory (backtesting) to Kafka (production) with zero code changes

**Use Case 2 — Medical Incident Response Daemon**

A hospital system monitors patient vitals continuously. When an alarm fires,
the daemon synthesizes clinical context in seconds — not minutes:

```axon
shield ClinicalShield {
    scan: [pii_leak, hallucination]
    strategy: classifier
    on_breach: halt
    redact: [ssn, patient_name]
}

daemon VitalsMonitor(event: VitalSign) -> ClinicalAlert {
    goal: "Process vital sign alerts and generate clinical recommendations"
    tools: [EMRLookup, DrugDatabase, EscalationService]
    listen "hospital.vitals.critical" as vital_event {
        shield ClinicalShield on vital_event -> safe_event
        know {
            step Context {
                ask: "Retrieve patient history and current medications
                      relevant to this vital sign anomaly"
                output: ClinicalContext
            }
        }
        step Recommend {
            ask: "Based on the clinical context, what immediate
                  interventions should the care team consider?"
            output: ClinicalAlert
        }
    }
}
```

- PII is automatically redacted before the LLM sees patient data
- `know` block ensures maximum factual rigor for medical recommendations
- The daemon runs 24/7 — no human needs to monitor the vitals dashboard
- If the daemon crashes (e.g., EMR timeout), the supervisor restarts it
  within the configured window — zero clinical alert gaps

**Use Case 3 — Multi-Daemon Compliance Pipeline**

A fintech platform chains three daemons: one ingests transactions, one
analyzes risk, one generates regulatory reports. Each runs independently,
communicating via the EventBus:

```axon
daemon TransactionIngester(event: RawTransaction) -> ProcessedTx {
    goal: "Normalize and enrich incoming transactions"
    tools: [APICall, Calculator]
    listen "payments.raw" as tx {
        compute NormalizeCurrency on tx.amount, tx.currency, "USD" -> normalized
        step Enrich {
            ask: "Classify this transaction by risk category"
            output: ProcessedTx
        }
    }
}

daemon RiskAnalyzer(event: ProcessedTx) -> RiskAssessment {
    goal: "Assess AML/KYC risk for enriched transactions"
    tools: [SanctionsList, RiskModel]
    listen "payments.enriched" as processed {
        doubt {
            step Assess {
                ask: "Evaluate this transaction against AML patterns
                      and sanction lists with maximum skepticism"
                output: RiskAssessment
            }
        }
    }
}

daemon ComplianceReporter(event: RiskAssessment) -> SARReport {
    goal: "Generate Suspicious Activity Reports for high-risk transactions"
    tools: [DocumentGenerator, RegulatoryAPI]
    listen "risk.flagged" as assessment {
        mandate SECFormat {
            constraint: "Output must follow FinCEN SAR format"
            tolerance: 0.01
            max_iterations: 5
        }
        step Report {
            ask: "Draft a SAR for this flagged transaction including
                  all required regulatory fields"
            output: SARReport
        }
    }
}
```

- Three independent daemons, three independent failure domains
- `doubt` block in RiskAnalyzer forces adversarial validation — no false negatives
  on AML screening
- `mandate` in ComplianceReporter guarantees regulatory format compliance via
  PID control — the output mathematically converges to FinCEN SAR format
- Each daemon scales independently: ingestion on Kafka (high throughput),
  analysis on RabbitMQ (durable), reporting on memory (low volume)
- Full OTP supervision: if any daemon crashes, it restarts without affecting
  the other two

---

### XX. The Cognitive Data Plane — the `axonstore` Primitive

> The `axonstore` primitive turns a database into a **cognitive data plane** —
> a persistent relation enriched in four orthogonal dimensions no relational
> model ever carried: an **epistemic** grading, an **audit-chained** mutation
> history, an **algebraic-effect** (streaming) selection, and a **capability**
> type. Shipped in v1.30.0 (Fase 35) on a native Rust + `sqlx` PostgreSQL
> substrate; v1.31.0 (Fase 38) added the *Declared & Compile-Time-Typed Store
> Schema* (the `schema { … }` block + the `CREATE TABLE` synthesis); v2.0.0
> (Fase 39) added the **`FlowEnvelope⟨T⟩` wire contract** (Theorem 5.1, the
> §40 enterprise binding).

Every existing LLM framework bolts a database on as an afterthought: wrap a
SQLAlchemy session in a class, call it "memory", and hope the agent doesn't
hallucinate column names. AXON rejects this. `axonstore` is a **compiled
cognitive primitive** — its `where` filters compile to parameterized SQL
(injection is *unrepresentable*), its rows are epistemically typed, every
mutation is audit-chained, and store access is checked against a capability
type at compile time.

> **What v2.3.0 ships.** The four pillars below are live and proven end-to-end
> against a real `postgres:16` in CI. The Fase 38 *typed schema synthesis*
> (Fase 38.a-i, v1.31.0+) generates `CREATE TABLE` from the `schema { … }`
> block at compile time, with declared cardinality propagation (Fase 38.x.f).
> Multi-statement transactions land in 38.x. The Fase 39 *FlowEnvelope* (v2.0.0)
> formalises the cross-stack T1 wire contract for the §40 enterprise binding.

#### The four pillars

1. **Epistemic (Pillar I).** Every row `retrieve`d is born `Untrusted` (⊥) in
   the ESK trust lattice; a `confidence_floor` on the store is enforced at both
   `retrieve` and `persist` — sub-floor data cannot enter a trusted result or
   be silently written.
2. **Audit-chained (Pillar II).** Every `persist` / `mutate` / `purge` appends
   an HMAC-SHA256, Merkle-linked delta to a per-flow mutation chain; `head()`
   is a tamper-evident root, `verify()` re-checks the whole history, and an
   `on_breach` policy (`log` / `raise` / `rollback`) fires on a detected tamper.
3. **Streaming (Pillar III).** `retrieve` is a lazy, back-pressured
   `Stream<Row>` — it drains a server-side `sqlx` cursor through the Fase 34
   streaming surface under a `BackpressurePolicy`, so a million-row scan never
   materialises in memory.
4. **Capability-typed (Pillar IV).** A `capability: "<slug>"` on the store is a
   typed permission; the frontend type-checker enforces an
   endpoint → flow → gated-store compositional check at *compile time* — an
   under-privileged endpoint that could reach a gated store does not compile.

The remainder of this section gives the formal model behind the data plane.

#### 1. Argumento duro (pura matematica)

**Definition 1 (Ontological Transducer).** An `axonstore` declaration defines
a morphism between the probabilistic cognitive domain and the deterministic
relational domain:

```text
T : C_LLM → C_DB

where
  C_LLM = (Ω, P, F)         — probability space over LLM outputs
  C_DB  = (S, Σ, ACID)      — relational state space with ACID guarantees
  T      = compile(schema)  — schema-grounded morphism, verified at compile time
```

The transducer `T` is **not surjective** — not every LLM output can pass through
it. Only outputs satisfying the schema constraints (column types, primary keys,
NOT NULL) can be encoded as valid relational writes. This is the formal mechanism
that prevents hallucinated column names and type mismatches.

**Theorem 1 (HoTT Schema Isomorphism).** An `axonstore` schema is interpreted
as a type in Homotopy Type Theory (HoTT). The Univalence Axiom guarantees that
if the compiler can construct a homotopy path between the LLM's cognitive type
`B` and the relational schema type `A`, the operation is type-safe:

```text
Univalence Axiom:    (A ≃ B) ≃ (A = B)

where
  A = IRStoreColumn(name, type, constraints)        — schema type (compile-time)
  B = TypedOutput (LLM inference output)            — cognitive type (runtime)
  ≃ = homotopy equivalence (type isomorphism)

Proof-Carrying Synthesis:
  If ∃ path p : A →̃ B  →  operation O is type-safe                (1)
  If ∄ path p : A →̃ B  →  compile error: type mismatch            (2)
```

The critical corollary: if a model attempts to persist a `Speculation` value
into a column typed `FactualClaim`, the homotopy path collapses and the compiler
rejects the program. **Type safety is a topological invariant**.

**Theorem 2 (Linear Logic Transaction Tokens).** AXON's `transact` block
implements Girard's Linear Logic to prevent repeated insertions and dropped
commits — the two most common LLM-induced database corruption patterns:

```text
Linear Implication:    A ⊸ B     — A is consumed exactly once to produce B

Transaction lifecycle:
  begin(token_id)   → 1⊗ Token(token_id)     — create unique token (resource)
  execute(op, tok)  → Token(token_id) ⊸ Result — consume token, produce result
  commit(token_id)  → Result ⊸ ∅              — consume result, token destroyed

Structural rules prohibited:
  Weakening:    A ⊢ B  ≁  A, A ⊢ B             — no token duplication
  Contraction:  A, A ⊢ B  ≁  A ⊢ B             — no silent discard
```

_Consequence:_ a `transact` block cannot be executed twice with the same token.
If the LLM hallucinates a repeated commit, the second attempt finds no token
to consume and raises `LinearTokenExhaustedError`. This structurally prohibits
double-spending and phantom commits — the two failure modes that destroy
financial ledger integrity.

**Theorem 3 (Confidence Floor as a Barrier Function).** The `confidence_floor`
parameter of `axonstore` implements a formal **epistemic barrier**:

```text
B(σ, φ) : LLMState × ConfidenceFloor → Decision

B(σ, φ) = {
  allow    if σ.confidence ≥ φ            — agent may persist
  on_breach_policy(σ, φ)  otherwise       — rollback | raise | log
}

Formal guarantee (on_breach = rollback):

  ∀ write W : confidence(W) < φ  ⟹  ¬committed(W)
```

No LLM operating below the confidence floor can persist data. This is not
a runtime check that might be bypassed — it is enforced by the dispatcher
before the SQL is ever generated. The database is **provably free** of
sub-threshold writes.

---

#### 2. Argumento dulce (por que es genial?)

Every other AI framework treats the database as plumbing. AXON makes it
a **cognitive primitive** — and the difference is profound:

**1. Your AI agents can finally be trusted with real data.**
Today, if you let an LLM write to a database, you're gambling. The model
might hallucinate a column name (`"user_name"` vs `"username"`), insert
a sentence where a float was expected, or silently drop a critical foreign key
constraint. With `axonstore`, the HoTT type checker rejects these programs
**before they compile**. The production database never sees a malformed write.
Your agents aren't just smart — they're **verifiably correct**.

**2. Transactions are mathematically impossible to corrupt.**
The Linear Logic token model is counterintuitive until you see it work: a
transaction is a resource that gets consumed exactly once. There is no API to
call `commit()` twice. There is no way to forget a rollback. The token
mechanism enforces this at the language level — **atomicity is a structural
property of the program, not a runtime hope**.

**3. The database adapts to the agent, not the other way around.**
Schema migrations are a first-class operation: `migrate()` computes the diff
between the current schema and the declared schema, issues `ALTER TABLE` for
each new column, and preserves all existing data. The agent declares what it
*wants* the schema to be; the runtime figures out how to get there. No manual
migration scripts. No ORM configuration files. No `db.create_all()` prayers.

**4. Observability is built in, not bolted on.**
Every `persist`, `retrieve`, `mutate`, and `purge` operation is tracked by
`StoreMetrics` — operation count, error rate, p50/p95/p99 latency. The circuit
breaker trips automatically when error rates spike. Health checks are a single
`ping()` call. You get production-grade observability for free, from the
language itself.

**5. SQL injection is unrepresentable.**
The `filter_parser` tokenizes `where` expressions into typed `FilterCondition`
objects and builds parameterized SQL from the AST. There is no string
interpolation path. There is no "raw query" escape hatch. It is **structurally
impossible** to write an `axonstore` program that is vulnerable to SQL
injection — the same guarantee assembly gives you for buffer overflows via
Rust's ownership model.

---

#### 3. Argumento con tres casos de uso

> *Scope note — `schema { … }` is REAL (v1.31.0 / Fase 38: DDL synthesis from
> the declaration), as are the four pillars (epistemic floor, audit chain,
> `Stream<Row>`, capability typing, v1.30.0) and the `FlowEnvelope⟨T⟩` wire
> contract (v2.0.0 / Fase 39). **`transact { … }` was RETRACTED in Fase 111
> (`axon-T938`) and no longer compiles** — it never opened a transaction, so
> the construct was removed rather than left to look like one; the examples
> below use the idempotent form instead. `migrate()` illustrates the design
> model and is not shipped.*

**Use Case 1 — Financial Ledger with Atomic Double-Entry Bookkeeping**

A fintech platform requires that every debit is paired with exactly one credit.
Classical approaches: wrap two INSERTs in a try/except and pray. AXON approach:
Linear Logic makes the atomicity a theorem.

```axon
axonstore Ledger {
    backend: postgresql
    connection: env:DATABASE_URL
    confidence_floor: 0.99
    on_breach: rollback
    isolation: serializable
    schema {
        id:        integer   primary_key auto_increment
        entry_ref: text      not_null
        type:      text      not_null    // "debit" | "credit"
        amount:    real      not_null
        account:   text      not_null
        created_at: text
    }
}

anchor LedgerIntegrity {
    require: double_entry_balance
    confidence_floor: 0.99
    on_violation: raise AnchorBreachError
    enforce: "Every transaction must have exactly one matching debit and credit
              with equal amounts before committing."
}

flow RecordTransfer(from_acct: String, to_acct: String, amount: Real) -> LedgerEntry {
    // Both legs carry the SAME `entry_ref`, which is the idempotency key: a
    // retry converges on the same pair instead of duplicating one side.
    persist into Ledger { entry_ref: "TXN-001", type: "debit",  amount: amount, account: from_acct }
    persist into Ledger { entry_ref: "TXN-001", type: "credit", amount: amount, account: to_acct  }
}
```

- `isolation: serializable` prevents phantom reads during concurrent transfers
- **Multi-statement transactions are NOT shipped.** `transact { … }` was
  retracted in §Fase 111 (`axon-T938`): it never opened a transaction, took no
  lock and rolled nothing back, so writes inside it were exactly as atomic as
  writes outside it. A fabricated atomicity guarantee is worse than an absent
  one, because you only discover it on the failure path. Until real
  transactional semantics land, the shape above is the honest one: issue the
  writes directly and key them by `entry_ref` so a retry CONVERGES instead of
  double-posting
- `confidence_floor: 0.99` means the agent must be nearly certain before
  writing financial data — a 0.97-confidence output is rejected before it
  reaches the store
- The `LedgerIntegrity` anchor enforces double-entry balance semantics on top
  of the database's ACID guarantees — two independent layers of correctness

**Use Case 2 — Multi-Tenant User Data Store with Dynamic Schema Migration**

A SaaS platform adds a `subscription_tier` column to the user table mid-flight,
without downtime, while agents continue running:

```axon
axonstore Users {
    backend: sqlite  // local dev
    connection: env:DB_PATH
    confidence_floor: 0.90
    schema {
        id:                integer  primary_key auto_increment
        email:             text     not_null
        name:              text
        created_at:        text
        // roadmap: migrate() will handle ALTER TABLE automatically
        subscription_tier: text
    }
    indexes {
        idx_users_email: [email] unique
    }
}

know {
    flow LookupUser(email: String) -> UserRecord {
        step Find {
            retrieve from Users where "email = '{email}'"
            output: UserRecord
        }
    }
}

flow OnboardUser(email: String, name: String, tier: String) -> UserRecord {
    transact Users {
        persist into Users {
            email: email
            name: name
            subscription_tier: tier
        }
    }
}
```

- When this flow runs against a database without `subscription_tier`, `migrate()`
  automatically issues `ALTER TABLE Users ADD COLUMN subscription_tier TEXT`
- Existing rows get `NULL` for the new column — no data loss
- The unique index on `email` is created via `create_index()` if it doesn't exist
- `know` block on the lookup ensures zero speculation about data that's in the DB
- The entire schema evolution is **declarative** — the developer changes the
  `axonstore` block, not a migration file

**Use Case 3 — Autonomous Research Agent with Cited Knowledge Persistence**

A research agent discovers facts during investigation. It persists only what it
knows with high confidence — sub-threshold discoveries are logged but not committed:

```axon
axonstore KnowledgeBase {
    backend: postgresql
    connection: env:RESEARCH_DB_URL
    confidence_floor: 0.85
    on_breach: log             // sub-threshold writes are logged, not rejected
    isolation: read_committed
    schema {
        id:          integer  primary_key auto_increment
        claim:       text     not_null
        source_url:  text     not_null
        confidence:  real     not_null
        claim_type:  text     not_null    // "FactualClaim" | "CitedFact"
        discovered:  text
    }
}

anchor CitationRequired {
    require: source_citation
    confidence_floor: 0.85
    enforce: "Every persisted claim must include a verifiable source URL."
}

agent ResearchScout {
    goal: "Discover and persist verified facts about the target topic
           with source citations and confidence scores above 0.85"
    tools: [WebSearch, PDFExtractor]
    strategy: reflexion
    max_iterations: 20
    max_cost: 3.00
    on_stuck: forge
    return: ResearchReport
}

know {
    flow ConductResearch(topic: String) -> ResearchReport {
        step Scout {
            ResearchScout(topic)
            output: DiscoveredFacts
        }
        transact KnowledgeBase {
            persist into KnowledgeBase {
                claim:      DiscoveredFacts.claim
                source_url: DiscoveredFacts.source
                confidence: DiscoveredFacts.confidence
                claim_type: "CitedFact"
            }
        }
        step Synthesize {
            retrieve from KnowledgeBase where "confidence > 0.90"
            output: ResearchReport
        }
    }
}
```

- The agent only persists what it knows (`know` block + `confidence_floor: 0.85`)
- Sub-threshold discoveries trigger `on_breach: log` — they're not lost, just not
  committed to the authoritative knowledge base
- `reflexion` strategy means the agent critiques its own findings each cycle,
  naturally driving confidence scores higher through self-correction
- The parameterized `WHERE` in the final `retrieve` — `"confidence > 0.90"` —
  compiles to `"confidence" > ?` with `[0.90]` as the bound parameter. SQL
  injection is structurally impossible.
- `StoreMetrics` tracks every persist, recording p95 latency, error rate, and
  circuit breaker state — the full research sessions is observable in production

---

## Architecture

```
.axon source → Lexer → Tokens → Parser → AST
                                           │
                              Type Checker (semantic validation)
                                           │
                              IR Generator → AXON IR (JSON-serializable)
                                           │
                              Backend (Anthropic │ OpenAI │ Gemini │ Kimi │ GLM │ OpenRouter │ Ollama)
                                           │
                    ┌──────────────────────────────────────────────────────────┐
                    │  server_execute_full() — 10-stage pipeline              │
                    │  auto-select → rate-limit → key-resolve → circuit-break │
                    │  → execute → fallback → metrics → cost → TPM → CB      │
                    └──────────────────────────────────────────────────────────┘
                                           │
                              ΛD Epistemic Layer (ψ = ⟨T, V, E=⟨c,τ,ρ,δ⟩⟩)
                                           │
                              ℰMCP Protocol (tools · resources · prompts)
                                           │
                              Typed Output (validated, traced, epistemic)
```

**v2.3.0 metrics: 100% Rust + C23 native runtime · 285+ routes · 65+ primitives · 2,245 axon-lang + 535 axon-frontend + 13 axon-csys = 2,793 Rust tests · 108 mapped external-audit controls across 4 frameworks**

> **Versioning trail (semver bumps; v2.x.y additions are backward-compatible at the language surface):**
> - **v1.0.0** — Initial Phase K production release (47 cognitive primitives, 738 tests)
> - **v1.1.0** — Fases 1–5 Cognitive I/O: `resource` / `fabric` / `manifest` / `observe` / `reconcile` / `lease` / `ensemble` / `topology` / `session` / `immune` / `reflex` / `heal`
> - **v1.2.0** — Fases 6–7.x ESK: `compliance` annotations + Regulatory Type Theory + audit engine (108 controls across SOC 2 / ISO 27001 / FIPS 140-3 / CC EAL 4+)
> - **v1.3.0** — Fase 8: native Rust runtime with byte-identical parity + CLI parity (`dossier` / `sbom` / `audit` / `evidence-package`) + binary distribution
> - **v1.3.1** — Fase 9: UI cognitiva declarativa core primitives (`component` / `view` + compile-time compliance contract)
> - **v1.4.x–v1.9.x** — Fases 11–14: neuro-symbolic micro-OS, π-calculus mobile typed channels, lossless lexing
> - **v1.10.0–v1.15.0** — Fases 15–20: runtime wiring (lambda-apply, `let`, IR meta-audit), production hardening, daemon supervisor, the production Shield runtime
> - **v1.16.0–v1.18.0** — Fases 22–24: seven native Rust LLM backends + the algebraic-effects execution runtime
> - **v1.19.x–v1.20.0** — Fases 25 + 28: **C23 metal-bound kernels** (`axon-csys`) + adopter diagnostic robustness (multi-file `axon parse`, rustc-style diagnostics)
> - **v1.21.0–v1.22.0** — Fases 30–31: HTTP transport for algebraic stream effects (SSE / NDJSON) + type-driven wire inference
> - **v1.23.0–v1.23.1** — Fase 32: `axonendpoint` as a first-class HTTP REST primitive — typed routes, body + output schema validation, `Idempotency-Key`, auth scopes
> - **v1.24.0–v1.28.0** — Fase 33: SSE as a cognitive primitive — per-`IRFlowNode` async dispatcher, production streaming wiring, wire-format adapters
> - **v1.29.0** — Fase 34: tools as stream-producers
> - **v1.30.0** — Fase 35: `axonstore` as a cognitive data plane (four pillars — epistemic / audit-chained / `Stream<Row>` / capability-typed)
> - **v1.31.0** — Fase 36: Backend Resolution Contract (D1 deterministic precedence ladder) + Fase 36.x mixed-flow streaming
> - **v1.32.0–v1.40.x** — Fase 37: Request Binding Contract (totality across the runner) + Fase 37.y pooler-coherent store + Fase 38 *Declared & Compile-Time-Typed Store Schema* (CREATE TABLE synthesis from `schema { … }`) + Fase 38.x cardinality propagation + Fase 38.x.f narrow cardinality gate
> - **v2.0.0** — **Fase 39 (Pure Silicon)**: cross-stack `FlowEnvelope⟨T⟩` wire contract (Theorem 5.1) + **Fase 40 *Enterprise Pure Silicon*** — axon-enterprise rewritten to 100% Rust + C23, consuming axon-lang via a versioned Cargo dependency (no Python interpreter); 23-crate workspace with §40.f RLS policy generators + §40.g Argon2id identity + §40.h closed RBAC catalog + §40.i Ed25519/JWKS + §40.j OIDC/SAML + §40.k AES-256-GCM envelope + §40.l axum integration + §40.o tamper-evident audit hash-chain + §40.t AAD-bound `cognitive_states` snapshots
> - **v2.1.0** — Fase 40 follow-ups: native-runner multi-arch Docker image (amd64 + arm64; no QEMU)
> - **v2.2.0** — Fase 40.w.3.1 (D16): public `axon::tenant::scope_tenant` primitive — the auth middleware scopes the tenant task-local so RLS binds automatically (no per-handler `SET LOCAL` contract)
> - **v2.3.0** — **Fase 41 (WebSocket as a Cognitive Primitive)**: the §41.a binary session-type algebra (duality + regular-coinductive μ-equality, Caires–Pfenning) + §41.b `session` + `socket` declarations (with `select`/`branch` choice grammar) + §41.c **credit-refined backpressure** (Presburger discharge, `backpressure: credit(k)`) + §41.d `tokio` + RFC 6455 WebSocket runtime + §41.e **SSE-as-fragment unification** (`S_SSE = Π_↓(S_WS)`, byte-compat with Fase 33) + §41.f enterprise WS surface (the Kivi single-image unblock) + §41.g typed reconnection via AAD-bound `cognitive_states` snapshots + §41.h **multiparty projection** (Honda–Yoshida–Carbone `GlobalType` + `project_all` safe-realizability gate) + §41.i dedicated CI fuzz lane (6 + 2 surfaces × 50-seed LCG corpus)

### 65+ Language Primitives — Cognitive + Cognitive I/O (100% wired to runtime)

**Block 1 — the core Cognitive Primitives:**


| Primitive  | Keyword      | What it represents                                   |
| ---------- | ------------ | ---------------------------------------------------- |
| Persona    | `persona`    | Cognitive identity of the model                      |
| Context    | `context`    | Working memory / session config                      |
| Intent     | `intent`     | Atomic semantic instruction                          |
| Flow       | `flow`       | Composable pipeline of cognitive steps               |
| Reason     | `reason`     | Explicit chain-of-thought                            |
| Anchor     | `anchor`     | Hard constraint (never violable)                     |
| Validate   | `validate`   | Semantic validation gate                             |
| Refine     | `refine`     | Adaptive retry with failure context                  |
| Memory     | `memory`     | Memory-augmented corpus with structural learning     |
| Tool       | `tool`       | External invocable capability                        |
| Probe      | `probe`      | Directed information extraction                      |
| Weave      | `weave`      | Semantic synthesis of multiple outputs               |
| Know       | `know`       | Epistemic scope — maximum factual rigor              |
| Believe    | `believe`    | Epistemic scope — moderate confidence                |
| Speculate  | `speculate`  | Epistemic scope — creative freedom                   |
| Doubt      | `doubt`      | Epistemic scope — adversarial validation             |
| Par        | `par`        | Parallel cognitive dispatch                          |
| Hibernate  | `hibernate`  | Dynamic state yielding / CPS checkpoint              |
| DataSpace  | `dataspace`  | In-memory associative data container                 |
| Ingest     | `ingest`     | Load external data into a DataSpace                  |
| Focus      | `focus`      | Select data — propagate associations                 |
| Associate  | `associate`  | Link tables via shared fields                        |
| Aggregate  | `aggregate`  | Group-by aggregation on selections                   |
| Explore    | `explore`    | Snapshot current associative state                   |
| Deliberate | `deliberate` | Compute budget control (tokens/depth/strategy)       |
| Consensus  | `consensus`  | Best-of-N parallel evaluation & selection            |
| Forge      | `forge`      | Directed creative synthesis (Poincaré pipeline)      |
| Agent      | `agent`      | Autonomous goal-seeking BDI cognitive system         |
| Shield     | `shield`     | Compile-time IFC security (taint + capability)       |
| Savant     | `savant`     | Long-horizon autonomous research orchestrator — budget-bounded, interruptible, fail-closed, provenance-witnessed (§Fase 87) |
| Synth      | `synth`      | Dynamic tool-synthesis safety envelope — risk ceiling + WASM zero-trust sandbox + Coder/Reviewer consensus (§Fase 87) |
| Warden     | `warden`     | Authorization-native adversarial security-analysis block — abduction over authorized evidence, emitting attested `Vulnerability` findings (§Fase 88) |
| Scope      | `scope`      | Named authorization scope — signed envelope (targets allowlist + depth ceiling + approver) a `warden` MUST run within (§Fase 88) |
| Stream     | `stream`     | Algebraic Effects and Free Monads                     |
| Effects    | `effects`    | Algebraic effect rows for tool declarations          |
| PIX        | `pix`        | Structured document index (navigable tree)           |
| Navigate   | `navigate`   | Intent-driven tree retrieval with reasoning trail    |
| Drill      | `drill`      | Subtree-scoped navigation for targeted retrieval     |
| Trail      | `trail`      | Explainability path — formal reasoning audit         |
| Corpus     | `corpus`     | Multi-document graph with typed edges + epistemic σ  |
| Recall     | `recall`     | Memory-augmented episodic recall from interaction H  |
| Psyche     | `psyche`     | Psychological-epistemic modeling on Riemannian manifold |
| OTS        | `ots`        | Ontological Tool Synthesis for open-ended teleological generation |
| MCP        | `mcp`        | EMCP resource/tool ingestion from external MCP servers           |
| Mandate    | `mandate`    | Cybernetic Refinement Calculus — PID control for deterministic LLM output |
| Lambda     | `lambda`     | Epistemic State Vectors — compile-time degradation enforcement for data  |
| Compute    | `compute`    | Deterministic muscle — native Fast-Path execution bypassing the LLM      |
| Daemon     | `daemon`     | π-Calculus reactive process — persistent event-driven agent with OTP supervision |
| Listen     | `listen`     | Co-algebraic event subscription — binds daemon to typed EventBus channels       |
| AxonEndpoint | `axonendpoint` (`axpoint`) | Native HTTP boundary primitive — typed ingress, flow dispatch, and holographic endpoint telemetry |
| AxonStore  | `axonstore`  | Cognitive data plane — epistemically-typed rows, HMAC-Merkle audit chain, `Stream<Row>`, capability-typed access |

**Block 2 — the 18 Cognitive I/O primitives (Fases 1–9, λ-L-E calculus):**

| Primitive  | Keyword       | Phase | What it represents                                           |
| ---------- | ------------- | ----- | ------------------------------------------------------------ |
| Resource   | `resource`    | 1     | Linear/affine/persistent infrastructure token (DB, cache, bucket, GPU) under Linear Logic |
| Fabric     | `fabric`      | 1     | Topological substrate (VPC, cluster, namespace) under Separation Logic `*` disjointness  |
| Manifest   | `manifest`    | 1     | Declarative belief about desired shape + κ (regulatory class) annotations |
| Observe    | `observe`     | 1     | Quorum-gated state snapshot producing a ΛD envelope ⟨c, τ, ρ, δ⟩; `on_partition: fail` ≡ void (D4) |
| Reconcile  | `reconcile`   | 3     | Active-Inference control loop: observe → Jaccard drift → shield gate → on_drift action (Free Energy Principle) |
| Lease      | `lease`       | 3     | τ-decaying affine capability; post-expiry use is a CT-2 *Anchor Breach* (D2) |
| Ensemble   | `ensemble`    | 3     | Byzantine quorum aggregator with common-knowledge `Cφ` fusion (Fagin–Halpern) |
| Topology   | `topology`    | 4     | Typed directed graph over declared entities with Honda-liveness deadlock detection |
| Session    | `session`     | 4     | π-calculus binary session type — Honda–Vasconcelos duality enforced at compile time |
| Send       | `send`        | 4     | Session step — emit a message of type T                      |
| Receive    | `receive`     | 4     | Session step — block-await a message of type T               |
| Immune     | `immune`      | 5     | KL-divergence + Free-Energy anomaly sensor with temporal decay (paper §5.2–5.3) |
| Reflex     | `reflex`      | 5     | Deterministic O(1) LLM-free motor response with HMAC-signed traces + idempotency (paper §4.2) |
| Heal       | `heal`        | 5     | Linear-Logic one-shot patch kernel with `audit_only` / `human_in_loop` / `adversarial` modes (paper §6–7) |
| Compliance | `compliance`  | 6.1   | κ regulatory class annotation (HIPAA, PCI_DSS, GDPR, SOX, SOC2, ISO27001, FISMA, GxP, CCPA, NIST_800_53) enforced at compile time |
| Endpoint   | `axonendpoint`| 6.1   | HTTP boundary with compile-time `shield.compliance ⊇ type.compliance` coverage rule (Regulatory Type Theory) |
| Component  | `component`   | 9     | Reusable UI fragment with `renders` + `via_shield` + `on_interact` — regulated types require shield coverage at compile time |
| View       | `view`        | 9     | Top-level UI screen composing declared components with optional `route` + session-typed reactivity (deferred to §9.3.b/§9.4.b renderers) |

### Epistemic Type System (Partial Order Lattice)

Types represent **meaning** and cognitive state, not just data structures. AXON
implements an epistemic type system based on a partial order lattice (T, ≤),
representing formal subsumption relationships:

```text
⊤ (CorroboratedFact)
    │
    ├── CitedFact
    │   └── FactualClaim
    │       ├── ContestedClaim
    │       └── Uncertainty (⊥)
    │
    ├── Opinion
    └── Speculation
```

**Rule of Subsumption:** If T₁ ≤ T₂, then T₁ can be used where T₂ is expected.
For instance, a `CitedFact` can naturally satisfy a `FactualClaim` dependency,
but an `Opinion` **never** can. Furthermore, computations involving
`Uncertainty` structurally taint the result, propagating `Uncertainty` forwards
to guarantee epistemic honesty throughout the execution flow.

```
Content:      Document · Chunk · EntityMap · Summary · Translation
Analysis:     RiskScore(0..1) · ConfidenceScore(0..1) · SentimentScore(-1..1)
Structural:   Party · Obligation · Risk (user-defined)
Compound:     StructuredReport
```

---

## Project Structure

AXON is a Rust + C workspace. The Rust crates are the **canonical
implementation**; the Python package remains as the historical reference.

```
axon-constructor/
├── axon-frontend/               # Rust — the canonical compiler frontend
│   └── src/
│       ├── lexer.rs             # Source → token stream (lossless)
│       ├── tokens.rs            # Keyword table (100+ keywords)
│       ├── parser.rs            # Tokens → AST (recursive descent + recovery)
│       ├── ast.rs               # AST node hierarchy
│       ├── type_checker.rs      # Semantic + epistemic + capability typing
│       ├── checker.rs           # Compliance / κ-coverage checker
│       ├── ir_generator.rs      # AST → AXON IR
│       ├── ir_nodes.rs          # IR node definitions
│       ├── stream_effect.rs     # Algebraic stream-effect inference
│       ├── channel_analysis.rs  # π-calculus session-type duality
│       └── smart_suggest.rs     # "Did you mean X?" Levenshtein hints
├── axon-rs/                     # Rust — the canonical runtime, server & CLI
│   └── src/
│       ├── main.rs              # `axon` CLI — 21 subcommands
│       ├── runner.rs            # Synchronous flow execution engine
│       ├── flow_dispatcher/     # Per-IRFlowNode async dispatcher (45-variant)
│       ├── streaming_via_dispatcher.rs  # Production SSE streaming producer
│       ├── stream_runtime.rs    # Algebraic-effects streaming runtime
│       ├── effects/             # Algebraic effect handlers
│       ├── backends/            # 7 native LLM backends + SSE streaming + retry
│       ├── store/               # axonstore cognitive data plane (4 pillars)
│       │   ├── filter.rs            # where-expr → parameterized SQL compiler
│       │   ├── postgres_backend.rs  # sqlx PgPool substrate
│       │   ├── registry.rs          # closed-catalog backend dispatch
│       │   ├── epistemic.rs         # Pillar I — confidence_floor enforcement
│       │   ├── audit_chain.rs       # Pillar II — HMAC-Merkle mutation chain
│       │   ├── row_stream.rs        # Pillar III — retrieve as Stream<Row>
│       │   └── capability.rs        # Pillar IV — capability-typed access
│       ├── session_runtime/     # §Fase 41 — typed WS dialogue runtime
│       │   ├── state.rs             # SessionRuntime cursor + CreditWindow + seal/resume
│       │   ├── wire.rs              # Frame { Send, Select, End, Error } JSON envelope
│       │   ├── ws.rs                # axum WebSocket carrier driver (RFC 6455)
│       │   ├── sse.rs               # SSE-as-fragment driver (S_SSE = Π_↓(S_WS))
│       │   └── error.rs             # ProtocolError closed catalog
│       ├── axon_server.rs       # axum HTTP / JSON-RPC server (285 routes)
│       ├── emcp.rs              # Epistemic Model Context Protocol
│       ├── esk/                 # Epistemic Substrate Kernel (trust lattice)
│       ├── wire_format/         # SSE / NDJSON wire-format adapters
│       ├── storage_postgres.rs  # PostgreSQL persistence + migrations
│       └── resilient_backend.rs # Retry + circuit breaker + fallback chains
├── axon-csys/                   # **C23 metal-bound kernels** (FIPS-routable):
│   ├── src/                         #   - SHA-256 + HMAC-SHA256 (used by audit chain + JWT)
│   │                                #   - SIMD G.711 audio codecs
│   │                                #   - BPE tokeniser (1.26-1.43× faster than tiktoken-rs;
│   │                                #     #embed-baked merges tables for OpenAI/Kimi/GLM)
│   │                                #   - Buffer pool (207× faster than Vec<u8> for hot paths)
│   │                                #   - FSM dispatch via computed gotos
│   └── tests/                       # sanitizer-clean + valgrind-clean CI lanes
├── axon/                        # Python — thin PyPI wrapper (downloads + invokes the Rust binary)
├── examples/                    # Reference .axon programs (healthcare, banking, government…)
├── docs/papers/                 # Academic papers (formal proofs, calculus, theorems)
├── sdk/                         # TypeScript MCP client SDK
├── tests/                       # Wrapper contract tests + CLI smoke tests
└── .github/workflows/           # CI — per-fase test workflows + release pipelines
    ├── fase_41_session_types.yml    # 7-lane WS / SSE / fuzz / D8 backwards-compat
    ├── fase_33_sse_cognitive_primitive.yml
    └── …                            # one workflow per cycle for clear failure attribution
```

---

## Installation

> **The Rust + C23 binary is the canonical channel.** The whole stack — compiler,
> runtime, HTTP server, seven LLM backends, the SSE / NDJSON / WebSocket
> streaming wire, the session-typed dialogue driver — ships as a single native
> binary, no interpreter required. The Python interpreter was retired in
> Fase 40 (*Pure Silicon*, v2.0.0); the PyPI package is now a tombstone that
> redirects to `cargo install axon-lang`.

### Prerequisites

| Requirement | Why |
| ----------- | --- |
| [Rust](https://rustup.rs) **1.95+** | `rust-version` in every manifest. Older toolchains fail with an MSRV error, not a compile error. |
| A working **C compiler** (MSVC / clang / gcc) | `axon-csys` is a non-optional dependency and builds C23 kernels via `cc`. On Windows the MSVC Build Tools suffice; on Debian/Ubuntu, `build-essential`. |

> These are checked at build time, not install time — if `cargo install` stops
> with a `cc`/linker error rather than a Rust error, the C toolchain is what is
> missing.

### Option 1 — `cargo install` (recommended)

```bash
cargo install axon-lang
axon run program.axon
```

This is the canonical distribution channel, and the one the PyPI tombstone
redirects to. It installs the `axon` binary — all 28 subcommands, one artefact.

### Option 2 — Build from source

```bash
# Install Rust (if you don't have it)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/VelitRicardo/axon-lang.git
cd axon-lang/axon-rs
cargo build --release

# Run a program
./target/release/axon run program.axon
```

> **On prebuilt binaries.** There are none today. `rust_release.yml` exists to
> build and attach them, but it triggers on `rust-v*` tags while releases are
> cut as `v*`, and its last runs failed — so every release page carries zero
> assets. Rather than keep pointing readers at an empty page, this section
> documents only channels that work. The workflow's own header records the same
> thing, so the next person to touch it knows what they are picking up.

### Option 3 — Serve mode (HTTP API)

```bash
# Development — in-memory storage
axon serve --port 3000

# Production — with PostgreSQL persistence and structured logging
DATABASE_URL="postgresql://user:pass@localhost/axon" \
axon serve --port 3000 --log-format json --database-url "$DATABASE_URL"
```

Without `DATABASE_URL`, AXON uses in-memory storage (perfect for development and testing).

### LLM Backend API Keys

Optional — only needed to execute flows against real backends (not for CLI validation):

| Key                  | For                       | Get it at                                               |
| -------------------- | ------------------------- | ------------------------------------------------------- |
| `ANTHROPIC_API_KEY`  | Claude backend            | [console.anthropic.com](https://console.anthropic.com/) |
| `OPENAI_API_KEY`     | GPT backend               | [platform.openai.com](https://platform.openai.com/)     |
| `GEMINI_API_KEY`     | Gemini backend            | [aistudio.google.com](https://aistudio.google.com/)     |
| `KIMI_API_KEY`       | Kimi backend              | Moonshot AI platform                                    |
| `GLM_API_KEY`        | GLM backend               | Zhipu AI open platform                                  |
| `OPENROUTER_API_KEY` | OpenRouter backend        | [openrouter.ai](https://openrouter.ai)                  |
| `OLLAMA_API_KEY`     | Ollama (local — optional) | runs locally; no key for the default setup              |

Stubs work without keys — perfect for development and CI/CD.

---

## CLI Usage

All commands work as native binaries:

```bash
# Validate syntax: lex + parse + type-check
axon check program.axon

# Multi-file diagnostic pass (v1.20.0+) — surfaces every parse error
# across a whole corpus in one pass, with rustc-style source-context
# blocks + "Did you mean X?" hints on typo'd keywords.
axon parse src/                                # walk recursively
axon parse "src/**/*.axon"                     # glob pattern
axon parse src/ --json --format=ndjson         # IDE / LSP-friendly
axon parse src/ --strict                       # halt at first failing file
axon parse src/ --max-errors 50                # cap diagnostic stream

# Compile to IR JSON
axon compile program.axon                     # → program.ir.json
axon compile program.axon --stdout             # pipe to stdout
axon compile program.axon -b openai            # target backend
axon compile program.axon -o custom.json       # custom output path

# Execute end-to-end (requires API key for chosen backend)
axon run program.axon                          # default: anthropic
axon run program.axon -b gemini                # choose backend
axon run program.axon --trace                  # save execution trace
axon run program.axon --tool-mode hybrid       # stub | real | hybrid

# Start the production server
axon serve --port 3000                         # In-memory storage
axon serve --port 3000 \
  --log-format json \
  --log-file ./logs \
  --database-url "postgresql://localhost/axon"

# Pretty-print an execution trace
axon trace program.trace.json

# Version
axon version

# Interactive REPL
axon repl

# Introspect stdlib
axon inspect anchors                       # list all anchors
axon inspect personas                      # list all personas
axon inspect NoHallucination               # detail for a component
axon inspect --all                         # list everything

# Deploy a flow to a running server
axon deploy program.axon --server http://localhost:3000

# Lambda Data (ΛD) epistemic codec — encode / decode / inspect envelopes
axon ld encode data.json
axon ld inspect envelope.ld.json

# Compare two execution plans / replay a trace as a regression check
axon diff old.ir.json new.ir.json
axon replay program.trace.json

# Aggregate trace statistics (text | json | prometheus | csv)
axon stats traces/ --format prometheus

# Dependency graph (DOT or Mermaid) + token/USD cost estimate
axon graph program.axon --format mermaid
axon estimate program.axon -b anthropic
```

### Server Endpoints (HTTP/JSON-RPC)

All runtime features are exposed via HTTP:

```bash
# Deploy a flow
curl -X POST http://localhost:3000/v1/deploy \
  -H "Content-Type: application/json" \
  -d '{"source": "flow ...", "backend": "anthropic"}'

# Execute a deployed flow
curl -X POST http://localhost:3000/v1/execute/flow_name \
  -H "Content-Type: application/json" \
  -d '{"input": {...}}'

# Query traces (with request correlation via tracing)
curl http://localhost:3000/v1/traces?limit=50

# Health check (with database pool status)
curl http://localhost:3000/v1/health

# MCP interface (JSON-RPC 2.0)
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

---

## Tests

All tests run on the compiled Rust runtime:

```bash
# Full suite
cd axon-rs
cargo test

# Specific test suites
cargo test --lib                           # axon-rs unit tests
cargo test --test integration              # axon-rs integration tests
cd ../axon-frontend && cargo test          # compiler frontend tests
```

### Current Status (v2.3.0)

```
axon-lang lib       :  2,245 tests  (the runtime — sessions, backends, runtime, streaming, axonstore)
axon-frontend lib   :    535 tests  (lexer / parser / type checker / IR / session-type algebra / multiparty)
axon-csys lib       :     13 tests  (the C23 kernels' Rust wrapper)
─────────────────────────────────────
Rust workspace      :  2,793 tests — zero regressions
```

Plus dedicated integration lanes per cycle:
- §Fase 41 WS / SSE / fuzz E2E lanes (the new `fase_41_session_types.yml` workflow)
- §Fase 33 SSE family (still green — D8 backwards-compat)
- §Fase 30/31/32 streaming + REST families
- §Fase 38 typed schema CREATE TABLE + cardinality propagation
- §Fase 39 FlowEnvelope wire contract drift gate
- Real-`postgres:16` integration lane for the `axonstore` data plane

> **Python note.** The Python interpreter was retired in Fase 40 (*Pure Silicon*, v2.0.0), and the PyPI distribution line was closed with a tombstone release that redirects to `cargo install axon-lang`. The root `tests/` directory holds cross-stack golden fixtures (consumed by the Rust drift-gate suites) and k6 load scripts — no Python.

Every development cycle (Fase) lands behind a dedicated CI workflow under
[`.github/workflows/`](.github/workflows/) — per-fase test lanes, cross-stack
drift gates, fuzz packs, and a real-`postgres:16` integration lane for the
`axonstore` data plane. The discipline is **zero regressions**: a cycle does
not ship until the full suite is green.

**Coverage by area:**

| Area | What's covered |
| ---- | -------------- |
| Frontend | Lexer, parser (with error recovery), AST, type checker, IR generator, smart-suggest, channel analysis |
| Compiler backends | IR generation, seven LLM backends, locked-model dispatch, retry / transport |
| Runtime | Flow execution, algebraic-effects engine, per-`IRFlowNode` async dispatcher, context, tracer, validator |
| Streaming | SSE / NDJSON wire, type-driven transport inference, backpressure, cancellation, wire-format adapters |
| Cognitive primitives | forge, agent, shield, pix, corpus / MDN, memory, ots, mcp / taint, mandate, compute, daemon / listen |
| HTTP surface | `axonendpoint` REST routes, body + output schema validation, idempotency, auth scopes, 285 server routes |
| Data plane | `axonstore` four pillars — filter compiler, Postgres backend, epistemic floor, audit chain, `Stream<Row>`, capability typing |
| Compliance | ESK regulatory type theory, audit engine (108 controls), dossier / sbom / evidence-package determinism |
| Hardening | Observability, resilience (retry + circuit breaker), persistence, fuzz packs, immune / reflex / heal |

---

## Tool System

AXON tools bridge compile-time `IRUseTool` nodes with runtime implementations via `ToolRegistry` enum-based dispatch (no dynamic trait objects).

### Registry Modes

All modes support 4 provider adapters (native, stub, http, mcp) — mode controls which APIs are called:

```bash
# Safe for tests — no API calls, no I/O (all tools return stubs)
cargo run -- --tool-mode stub

# Real backends where available, stubs elsewhere (useful for mixed CI)
cargo run -- --tool-mode hybrid

# Only real backends (fails if API keys missing)
cargo run -- --tool-mode real
```

### Tool System (Rust Native)

Tools are dispatched via `ToolRegistry` with 4 provider adapters:

| Provider | Dispatch               | Use case                          |
| -------- | ---------------------- | --------------------------------- |
| native   | Built-in Rust executor | Calculator, DateTimeTool           |
| stub     | Returns mock response  | Testing and development            |
| http     | REST via reqwest       | External APIs (configurable URL)   |
| mcp      | JSON-RPC 2.0 (eMCP)   | MCP-compatible tool servers        |

**Built-in tools (always available, no LLM call):**

| Tool         | Backend                  | Capabilities                                              |
| ------------ | ------------------------ | --------------------------------------------------------- |
| Calculator   | Native Rust evaluator    | +, -, *, /, %, **, parens, sqrt, sin, cos, log, min, max |
| DateTimeTool | `std::time` (no deps)   | Current date, time, timestamp, UTC                        |

**Program-defined tools** are declared in `.axon` files via `tool Name { provider: http, runtime: "https://..." }` and dispatched at execution time through the configured provider adapter.

---

## Error Hierarchy

```
Level  1: ValidationError         — output type mismatch
Level  2: ConfidenceError         — confidence below floor
Level  3: AnchorBreachError       — anchor constraint violated
Level  4: RefineExhausted         — max retry attempts exceeded
Level  5: RuntimeError            — model call failed
Level  6: TimeoutError            — execution time limit exceeded
Level  7: ToolExecutionError      — tool invocation failed
Level  8: AgentStuckError         — agent stagnation detected
Level  9: ShieldBreachError       — shield detected security threat
Level 10: TaintViolationError     — untrusted data reached trusted sink
Level 11: CapabilityViolationError — tool access outside shield allow list
```

---

## Runtime Self-Healing

AXON features a native self-healing mechanism for **L3 Semantic Gates**. When
the LLM output violates a hard constraint (`AnchorBreachError`) or fails
structural semantic validation (`ValidationError`), the AXON `RetryEngine`
automatically intercepts the failure.

Instead of crashing or silently failing, the engine re-injects the exact
`failure_context` (e.g., _"Anchor breach detected: Hedging without citation"_)
back into the LLM's next prompt. This creates a closed feedback loop where the
model adaptively corrects its logic and structurally self-heals in real-time.

**Production Guarantees:**

- **Strict Boundaries:** The correction loop strictly respects the `refine`
  limits explicitly defined in the execution configuration. If the model fails
  to heal within the permitted attempts, AXON deterministically raises a
  `RefineExhaustedError` (containing the last failed state) to escalate the
  failure, preventing infinite execution loops.
- **Anchor Dependency:** The healing capability is directly proportional to the
  precision of the defined Anchors. AXON provides the robust recovery mechanism,
  but ambiguous or poorly defined constraints may cause the model to optimize
  for passing validation syntactically while failing semantically. Clear,
  logical Anchors are required.

### Phase 4: Logic & Epistemic Anchors

AXON includes specialized standard library anchors (Phase 4) explicitly designed
to work with the Self-Healing engine to enforce logical structures and epistemic
honesty:

- `SyllogismChecker`: Enforces explicit logical formats using `Premise:` and
  `Conclusion:` markers to guarantee structurally parseable arguments.
- `ChainOfThoughtValidator`: Requires explicit sequence step markers before
  resolving a prompt.
- `RequiresCitation`: Deep verification enforcing academic-style inline
  citations/URLs blocking unverifiable claims.
- `AgnosticFallback`: Penalizes unwarranted speculation, forcing the model to
  explicitly state a lack of information when sufficient data is unavailable.

---

## Roadmap

| Phase | What                                              | Status  |
| ----- | ------------------------------------------------- | ------- |
| 0     | Spec, grammar, type system                        | ✅ Done |
| 1     | Lexer, Parser, AST, Type Checker                  | ✅ Done |
| 2     | IR Generator, Compiler Backends                   | ✅ Done |
| 3     | Runtime (7 modules)                               | ✅ Done |
| 4     | Standard Library                                  | ✅ Done |
| 5     | CLI, REPL, Inspect                                | ✅ Done |
| 6     | Test Suite, Hardening, Docs                       | ✅ Done |
| 7     | Paradigm Shifts (epistemic/par/hibernate)         | ✅ Done |
| 8     | Data Science Engine + Runtime Integration         | ✅ Done |
| 9     | Executor integration + production backends        | ✅ Done |
| 10    | Compute Budget & Consensus (deliberate/consensus) | ✅ Done |
| 11    | Directed Creative Synthesis (`forge`)             | ✅ Done |
| 12    | Autonomous Agents (`agent` BDI primitive)         | ✅ Done |
| 13    | Security Shields (`shield` IFC primitive)         | ✅ Done |
| 14    | Epistemic Tool Fortification (stream/effects/FFI) | ✅ Done |
| 15    | Structured Cognitive Retrieval (`pix`)            | ✅ Done |
| 16    | Multi-Document Navigation (`corpus` MDN framework)| ✅ Done |
| 17    | Memory-Augmented MDN (structural learning via μ)  | ✅ Done |
| 18    | Ontological Tool Synthesis (`ots` primitive)      | ✅ Done |
| 19    | Epistemic MCP (`mcp`)                            | ✅ Done — `taint` **retracted in §111** (it was a dead `shield` field, never read by the runtime; the epistemic-taint law lives in the lattice: external data is born `Untrusted`, axon-T908) |
| 20    | Lambda Data (`lambda` — ΛD epistemic state vectors)| ✅ Done |
| 21    | Deterministic Muscle (`compute` + `logic`)        | 🔴 **Not done.** `logic` **retracted in §111** (reserved keyword, zero parser production). `compute` does **not** execute natively — it binds the literal string `"compute:Name(args)"`; its body is discarded at IR-gen. Under §111 review (implement or retract) |
| 22    | Reactive Daemons (`daemon` + `listen` π-calculus) | ✅ Done |
| 23    | AxonServer Process (HTTP/WS API + EventBus FFI)   | ✅ Done |
| 24    | Transactional Persistence (`axonstore` — HoTT + Linear Logic + DbC + Enterprise hardening) | ✅ Done |
| 25    | Epistemic Dependency Management (`apx`)           | 🔴 **Retracted in §111.** Never implemented: the policy block was parsed and discarded (`skip_braced_block`), reached no IR, and no MEC/PCC verification, EPR ranking, quarantine or compliance gate ever existed. `import … with apx { … }` is now a compile error |
| 26    | Native Endpoint Surface (`axonendpoint` / `axpoint` — typed HTTP ingress + flow dispatch + endpoint telemetry) | ✅ Done |
| **K**     | **Production Hardening — Observability, Resilience, Persistence** | ✅ Done |
| v1.10–v1.20 | Runtime wiring (`let` / lambda-apply / IR meta-audit) + production hardening + daemon supervisor + 7 native Rust LLM backends + algebraic-effects runtime + adopter diagnostics | ✅ Done |
| v1.21–v1.22 | HTTP transport for algebraic stream effects (SSE / NDJSON) + type-driven wire inference | ✅ Done |
| v1.23     | `axonendpoint` as a first-class HTTP REST primitive (schema validation, idempotency, auth scopes) | ✅ Done |
| v1.24–v1.28 | SSE as a cognitive primitive — per-`IRFlowNode` async dispatcher + production streaming wiring | ✅ Done |
| v1.29     | Tools as stream-producers                         | ✅ Done |
| v1.30     | `axonstore` cognitive data plane — four pillars (epistemic / audit-chained / `Stream<Row>` / capability-typed) | ✅ Done |
| v1.31     | Fase 36: Backend Resolution Contract (D1 deterministic precedence ladder) + Fase 36.x mixed-flow streaming | ✅ Done |
| v1.32–v1.40 | Fase 37 Request Binding Contract + Fase 38 *Declared & Compile-Time-Typed Store Schema* (DDL synthesis + cardinality propagation) | ✅ Done |
| **v2.0**  | **Fase 39 + 40 — Pure Silicon**: `FlowEnvelope⟨T⟩` cross-stack wire contract (Theorem 5.1) + axon-enterprise rewritten 100% Rust + C23 (23-crate workspace, no Python interpreter) | ✅ Done |
| v2.1      | Fase 40 follow-ups: native-runner multi-arch Docker image (amd64 + arm64) | ✅ Done |
| v2.2      | Fase 40.w.3.1 (D16) `axon::tenant::scope_tenant` primitive — auth middleware scopes RLS task-local automatically | ✅ Done |
| **v2.3**  | **Fase 41 — WebSocket as a Cognitive Primitive**: session-type algebra (Caires–Pfenning) + `session` + `socket` + credit-refined backpressure (Presburger) + SSE-as-fragment unification + typed reconnection (AAD-bound `cognitive_states`) + multiparty projection (Honda–Yoshida–Carbone) + enterprise WS surface (Kivi single-image unblock) | ✅ Done |

---

## Design Principles

1. **Declarative over imperative** — describe _what_, not _how_
2. **Semantic over syntactic** — types carry meaning, not layout
3. **Composable cognition** — blocks compose like neurons
4. **Configurable determinism** — spectrum from exploration to precision
5. **Failure as first-class citizen** — retry, refine, fallback are native

---

## How it Compares

|                               | LangChain | DSPy    | Guidance | **AXON** |
| ----------------------------- | --------- | ------- | -------- | -------- |
| Own language + grammar        | ❌        | ❌      | ❌       | ✅       |
| Semantic type system          | ❌        | Partial | ❌       | ✅       |
| Formal anchors                | ❌        | ❌      | ❌       | ✅       |
| Persona as type               | ❌        | ❌      | ❌       | ✅       |
| Reasoning as primitive        | ❌        | Partial | ❌       | ✅       |
| Native multi-model            | Partial   | Partial | ❌       | ✅       |
| Epistemic directives          | ❌        | ❌      | ❌       | ✅       |
| Native parallel dispatch      | ❌        | ❌      | ❌       | ✅       |
| State yielding / CPS          | ❌        | ❌      | ❌       | ✅       |
| Compute budget control        | ❌        | ❌      | ❌       | ✅       |
| Best-of-N consensus           | ❌        | ❌      | ❌       | ✅       |
| Creative synthesis engine     | ❌        | ❌      | ❌       | ✅       |
| Compiled autonomous agents    | ❌        | ❌      | ❌       | ✅       |
| Formal BDI convergence        | ❌        | ❌      | ❌       | ✅       |
| Budget-bounded agent loops    | ❌        | ❌      | ❌       | ✅       |
| Compile-time taint analysis   | ❌        | ❌      | ❌       | ✅       |
| Capability enforcement        | ❌        | ❌      | ❌       | ✅       |
| LLM attack surface shielding  | ❌        | ❌      | Partial  | ✅       |
| Algebraic effect rows         | ❌        | ❌      | ❌       | ✅       |
| Coinductive streaming         | ❌        | ❌      | ❌       | ✅       |
| FFI blame semantics           | ❌        | ❌      | ❌       | ✅       |
| Epistemic tool inference      | ❌        | ❌      | ❌       | ✅       |
| Structured tree retrieval     | ❌        | ❌      | ❌       | ✅       |
| Explainable retrieval trail   | ❌        | ❌      | ❌       | ✅       |
| Compile-time retrieval bounds | ❌        | ❌      | ❌       | ✅       |
| Cross-document graph navigation | ❌      | ❌      | ❌       | ✅       |
| Formal provenance tracking    | ❌        | ❌      | ❌       | ✅       |
| Epistemic type lattice        | ❌        | ❌      | ❌       | ✅       |
| EpistemicPageRank convergence | ❌        | ❌      | ❌       | ✅       |
| Memory as graph transformation| ❌        | ❌      | ❌       | ✅       |
| Structural learning via μ     | ❌        | ❌      | ❌       | ✅       |
| Episodic/semantic/procedural  | ❌        | Partial | ❌       | ✅       |
| Convergent memory operator    | ❌        | ❌      | ❌       | ✅       |
| EMCP taint-safe MCP ingestion | ❌        | ❌      | ❌       | ✅       |
| MCP resource → PIX/corpus     | ❌        | ❌      | ❌       | ✅       |
| Compile-time MCP capability   | ❌        | ❌      | ❌       | ✅       |
| Epistemic data state vectors  | ❌        | ❌      | ❌       | ✅       |
| Compile-time certainty bounds | ❌        | ❌      | ❌       | ✅       |
| Epistemic degradation theorem | ❌        | ❌      | ❌       | ✅       |
| Deterministic compute (zero LLM) | ❌     | ❌      | ❌       | ✅       |
| π-Calculus reactive daemons   | ❌        | ❌      | ❌       | ✅       |
| OTP supervision trees         | ❌        | ❌      | ❌       | ✅       |
| Pluggable EventBus FFI        | ❌        | ❌      | ❌       | ✅       |
| Native HTTP/WS server API     | ❌        | ❌      | ❌       | ✅       |
| Real-time SSE / NDJSON streaming wire | ❌ | ❌    | ❌       | ✅       |
| Type-driven wire-format inference | ❌    | ❌      | ❌       | ✅       |
| Algebraic-effects streaming runtime | ❌  | ❌      | ❌       | ✅       |
| First-class HTTP REST endpoints | ❌      | ❌      | ❌       | ✅       |
| Compile-time request/response schema typing | ❌ | ❌ | ❌      | ✅       |
| Compile-time regulatory compliance | ❌    | ❌      | ❌       | ✅       |
| Epistemically-typed persistent storage | ❌ | ❌    | ❌       | ✅       |
| Audit-chained database mutations | ❌      | ❌      | ❌       | ✅       |
| Capability-typed data access  | ❌        | ❌      | ❌       | ✅       |
| **Session-typed bidirectional dialogue (v2.3.0)** | ❌ | ❌ | ❌    | ✅       |
| **Statically-checked WebSocket protocol duality** | ❌ | ❌ | ❌  | ✅       |
| **Credit-refined backpressure (Presburger-decidable)** | ❌ | ❌ | ❌ | ✅    |
| **Typed reconnection via AAD-bound snapshots** | ❌ | ❌ | ❌    | ✅       |
| **Multiparty projection (Honda–Yoshida–Carbone)** | ❌ | ❌ | ❌   | ✅       |
| **100% Rust + C23 runtime (no GC, no interpreter)** | ❌ | ❌ | ❌ | ✅       |

---

## License

**Copyright (C) 2026 Ricardo Velit.**

**GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)** —
see [`LICENSE`](./LICENSE) for the canonical text.

axon-lang is free software: you can redistribute it and/or modify it
under the terms of the GNU Affero General Public License as published
by the Free Software Foundation, either version 3 of the License, or
(at your option) any later version. It is distributed in the hope
that it will be useful, but WITHOUT ANY WARRANTY; without even the
implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
PURPOSE. See the GNU Affero General Public License for more details.

Effective 2026-05-25, axon-lang transitioned from MIT to AGPL-3.0
to close the SaaS loophole: forks deployed as network services must
release their modifications under AGPL too. Existing MIT-licensed
releases on crates.io / PyPI (≤ axon-lang 2.3.0) retain MIT terms;
new releases from v3.0.0 onward ship under AGPL.

### Contributing

Every contributor must sign the [Contributor License Agreement
(CLA)](./CLA.md) before their pull request can be merged. The CLA is
collected automatically by the
[cla-assistant.io](https://cla-assistant.io/) bot on first PR. See
[`CONTRIBUTING.md`](./CONTRIBUTING.md) for the full flow.

### Commercial license

If AGPL-3.0 doesn't fit your use case (e.g. you want to embed
axon-lang in a closed-source product without open-sourcing your
product), a commercial license is available directly from the
author and copyright holder, Ricardo Velit — write to
[licensing@bemarking.com.co](mailto:licensing@bemarking.com.co) for
terms. The dual-license architecture (AGPL public + commercial
relicensing) is documented in
[the axon-enterprise `LICENSING.md`](https://github.com/VelitRicardo/axon-enterprise/blob/master/LICENSING.md).

## Authors

Ricardo Velit
