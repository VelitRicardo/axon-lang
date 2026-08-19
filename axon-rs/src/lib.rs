//! AXON runtime library — exposes the full AXON runtime: compiler
//! frontend (re-exported from `axon-frontend`), handlers, runtime
//! primitives, ESK, HTTP/WebSocket servers, persistence, OTS pipelines.
//!
//! Used by the `axon` binary and by integration tests.
//!
//! # Frontend vs runtime
//!
//! v1.4.2 — the compiler frontend (lexer, parser, AST, type checker,
//! IR generator, top-level checker, and the closed catalogs used by the
//! type checker) lives in the sibling crate `axon-frontend`, which has
//! zero runtime dependencies. This crate re-exports those modules
//! transparently so every existing caller (76 call sites across 26
//! files) keeps compiling without changes. The crate `axon-lsp`
//! consumes `axon-frontend` directly, skipping the runtime surface.

// ── v1.4.2 — frontend re-exports (transparent to callers) ───────
pub use axon_frontend::{
    ast,
    checker,
    // v2.76.0 — the Epistemic Module System (resolver · interfaces ·
    // ECC · linker · cache · driver). Re-exported so the CLI and the
    // enterprise workspace reach the module pipeline through the single
    // `axon = …` dep, exactly like the rest of the frontend.
    compilation_cache,
    ems,
    epistemic,
    epistemic_compat,
    ir_generator,
    ir_nodes,
    module_interface,
    module_linker,
    module_resolver,
    legal_basis,
    lexer,
    parser,
    refinement,
    // v2.3.0 — the session-type algebra (duality, regular-coinductive
    // equality, v2.3.0 credit-refined backpressure, v2.3.0 SSE-polarity
    // predicate). Re-exported so downstream consumers (the enterprise
    // server's v2.3.0 WS surface in `axon-enterprise`) reach it via the
    // single `axon = …` workspace dep without an extra `axon-frontend`
    // dependency line.
    session,
    // v2.3.0 — multiparty session types (global types + projection).
    // The orchestration story for n-agent skill/tool topologies: declare
    // a `GlobalType`, project per role, drive each role's binary
    // `SessionType` over the v2.3.0 runtime — composition stays in
    // lock-step by construction.
    multiparty,
    store_introspect,
    store_schema,
    store_schema_manifest,
    stream_effect,
    tokens,
    type_checker,
    // v2.37.0 — the blessed preset catalog + the voice/preset
    // desugar surface (`axon desugar` renders from these).
    upstream_presets,
    voice_desugar,
};

// `ots_catalog` is the compile-time slug catalog; the runtime `ots`
// module (below) re-exports these constants for backward compatibility.

// ── Runtime modules (stay in this crate) ────────────────────────────

pub mod anchor_checker;
pub mod api_keys;
pub mod audit_trail;
pub mod auth_middleware;
// v2.81.0 — the HTTP server, behind the `server` feature. 29,734 lines of
// `axum` router, and — until this step — the reason every adopter who only
// wanted `axon check` compiled a web framework. `axon serve` STAYS IN `--help`
// under every profile and refuses in writing, naming `axon-server`; see
// `main.rs`. The v2.67.0 doctrine: the advertised surface stays advertised.
#[cfg(feature = "server")]
pub mod axon_server;
pub mod backend;
pub mod backend_error;
pub mod circuit_breaker;
pub mod compiler;
pub mod config_persistence;
pub mod conversation;
pub mod cors;
pub mod cost_estimator;
pub mod daemon;
/// v2.63.0 — the deterministic columnar engine behind `dataspace`
/// (immutable record batches, validity bitmaps, zone maps, provenance).
pub mod dataspace_engine;
/// v2.66.0 — Governed Human Notification: the canonical contract
/// (evidence labels, recipient custody, fail-closed provider port).
pub mod notification;
// v2.81.0 — the sqlx pool builder, behind `postgres`.
#[cfg(feature = "postgres")]
pub mod db_pool;
pub mod deployer;
pub mod emcp;
pub mod event_bus;
/// v2.31.0 — the durable event outbox (the append-only log + processed
/// cursor that makes `emit` survive the consumer being down).
pub mod event_outbox;
/// v1.1.0 — Handler layer (Free Monad + CPS). Port of `axon/runtime/handlers/`.
pub mod handlers;
/// v1.1.0 + 5 runtime primitives. Port of `axon/runtime/` (lease kernel,
/// reconcile loop, ensemble aggregator, immune kernels).
pub mod runtime;
/// v2.3.0 — the **runtime** of a session-typed dialogue. The static
/// algebra (`axon_frontend::session`: duality, regular-coinductive
/// equality, credit-refined backpressure index `!ⁿA.S`) gets a dynamic
/// counterpart here: an operational state machine (`SessionRuntime`)
/// with one method per algebra rule, a wire envelope (`Frame`), and an
/// RFC 6455 WebSocket carrier (`ws::drive`) that runs a session type
/// against a peer. Carrier-agnostic core; the WS layer is one binding.
pub mod session_runtime;
/// v2.37.0 — the `upstream` runtime: the CLIENT dual of the v2.3.0
/// carrier. Dials OUT to a third-party vendor (STT/TTS/realtime speech)
/// over RFC 6455 + TLS, applies the declared auth handshake, transcodes
/// wire↔session per the compiled `map:` projection (T849-total), applies
/// the declared `overflow:` policy when the vendor is the slow side, and
/// reconnects with witnessed, fail-closed exponential backoff. A new
/// vendor is a new DECLARATION, never new Rust code.
pub mod upstream_runtime;
/// v2.4.0 — the `quant` cognitive primitive's RUNTIME: the
/// [`quant::QuantBackend`] port + a usable dense-statevector reference
/// simulator capped at n ≤ 10 (the OSS half; enterprise mounts the QuIDD /
/// VRAM / QPU engine behind the same trait in v2.4.0–i).
pub mod quant;
/// v1.17.0 — Algebraic effects runtime. FSM dispatch loop +
/// handler stack + Free-Monad interpretation of CPS-lowered IR
/// (consumes the JSON IR emitted by the Python frontend in 23.b/c/d).
pub mod effects;
/// v1.18.0 — Native Rust LLM backends. Per-provider async clients
/// behind a `Backend` trait + `Registry`. Per-provider modules
/// (anthropic.rs / openai.rs / gemini.rs / kimi.rs / glm.rs / ollama.rs
/// / openrouter.rs) land in 24.c–24.i; this module ships the shared
/// infra (trait + types + error + retry + observability + locked_model
/// + tokens dispatch).
pub mod backends;
/// v1.31.0 — the Backend Resolution Contract (D1): the pure,
/// deterministic precedence ladder that resolves a flow's execution
/// backend (request → axonendpoint `backend:` → server default →
/// environment-available `auto` → honest failure).
pub mod backend_resolution;
/// v2.22.0 — pure capability-aware model resolution: a step's
/// `requires_context:` + a backend's v2.22.0 model catalog → the smallest model
/// that fits, or honest fail-closed (never a too-small model).
pub mod model_resolution;
/// v2.23.0 — the Advantage Witness: a transversal law
/// (`axon://logic/no_unwitnessed_advantage`). A primitive may not claim an
/// advantage over a cheaper baseline without a machine-checkable witness on real
/// data; the `AdvantageWitness` trait + closed metric catalog + verdict.
pub mod advantage_witness;
/// v2.23.0 — quant as the first Advantage-Witness instance: the amplitude-
/// fidelity ≡ cosine theorem made executable + the `QuantKernelWitness` that
/// fails closed (no advantage over the classical baseline).
pub mod quant_witness;
/// v2.23.0 — the SECOND Advantage-Witness instance (transversality proof):
/// retrieval / navigate via the `ranking_lift` metric over flat cosine retrieval.
pub mod retrieval_witness;
/// v1.2.0 — Epistemic Security Kernel. Port of `axon/runtime/esk/`.
pub mod esk;
/// v2.4.0 — Proof-Carrying Code. apx/axonendpoint carry a portable,
/// machine-checkable proof object an INDEPENDENT verifier checks
/// against the artifact WITHOUT trusting the compiler that produced it
/// (the move from `esk`'s builder-signed attestation to a consumer-
/// verifiable proof). v2.4.0 ships the kernel + the ComplianceCoverage
/// property class.
pub mod pcc;
/// CLI handlers for the ESK audit commands (dossier, sbom, audit, evidence-package).
pub mod audit_cli;
/// v2.4.0 — CLI handlers for the PCC commands (`axon pcc prove` /
/// `axon pcc verify`). Closes the Proof-Carrying Code loop at the
/// command line: generate a proof bundle from source, then
/// independently verify it against a recompile of that source.
pub mod pcc_cli;
pub mod flow_inspect;
/// v1.24.0 — Closed-catalog runtime warnings for the SSE
/// production path. Surfaces `axon-W002 streaming-not-supported`
/// when the async streaming path falls back to legacy synchronous
/// delivery (D5 — no silent degradation).
pub mod runtime_warnings;
/// v1.24.0 — Process-wide runtime opt-in flags. Today carries
/// the `tokenizer_fallback` flag that gates BPE-tokenized chunking
/// on the SSE LEGACY path (D9 — opt-in; defaults OFF for v1.24.0
/// wire byte-compat).
pub mod runtime_flags;
/// v1.24.0 — Streaming-shaped execution plan extractor. Builds
/// `StreamingExecutionPlan` from `.axon` source for the production
/// async SSE path; pre-resolves per-step `BackpressurePolicy` via
/// `stream_effect_dispatcher` so the hot per-chunk loop in
/// `axon_server::server_execute_streaming_async` does not re-walk
/// the AST per chunk. Rejects flows that use 33.x.b-unsupported
/// features (anchors / lambda apply / let bindings / mid-stream
/// use_tool / hibernate / pix) with a closed-catalog `PlanFallback`
/// so the SSE handler can route them to the legacy synchronous path.
pub mod flow_plan;
/// v1.24.0 — Per-IRFlowNode async dispatcher skeleton. Closed-
/// catalog, compiler-enforced exhaustive match over the 45-variant
/// `IRFlowNode` enum. Subsequent steps 33.y.c–j replace the
/// transitional legacy shim with real per-variant async handlers.
/// 33.y.l retires the shim + the `LegacyShimHandled` outcome variant
/// once every IR variant has its real handler.
pub mod flow_dispatcher;
/// v1.24.0 — Streaming via the dispatcher. Lifts
/// `flow_dispatcher::dispatch_node` into the production SSE hot path.
///
/// v2.83.0 — this doc used to describe the graft as pending, behind an
/// `AXON_STREAMING_VIA_DISPATCHER` flag defaulting to OFF. That migration
/// FINISHED: 33.z.c flipped the default and 33.z.e deleted the flag together
/// with the legacy paths. `server_execute_streaming` now calls
/// [`streaming_via_dispatcher::run_streaming_via_dispatcher`]
/// unconditionally, and there is no other streaming entry point.
pub mod streaming_via_dispatcher;
pub mod flow_version;
pub mod epistemic_capture;
pub mod exec_context;
pub mod graceful_shutdown;
pub mod graph_export;
pub mod health_check;
pub mod hooks;
pub mod http_tool;
/// v2.53.0 — the deterministic OOXML writer (DOCX/PPTX/XLSX) behind the
/// `DocumentRenderer` native tool. Byte-deterministic + provenance-embedding.
#[cfg(feature = "documents")]
pub mod ooxml;
/// v2.54.0 — the read-only filesystem capability + path sandbox.
pub mod fs_sandbox;
/// v2.54.0 — the OOXML reader: bounded, born-Untrusted, Parsed text tree.
#[cfg(feature = "documents")]
pub mod ooxml_read;
/// v2.54.0 — the surgical edit engine + per-part hash manifest.
#[cfg(feature = "documents")]
pub mod ooxml_edit;
/// v2.54.0 — the `Inferred`-extraction contract: the `ExtractionEngine`
/// trait, the born-`Inferred` span with measured confidence, and the
/// confidence-floor quarantine gate. The producers v2.54.0 left the class without.
pub mod extraction;
/// v2.54.0 — the IDP-E recognizer kernel: the deterministic geometry+topology
/// engine (Otsu → cubical β₀/β₁ → geometric discrimination → reading order →
/// pix-navigable canonical tree). Reads a bounded PGM/PBM raster; real image
/// decode is the sidecar (v2.54.0). Scoped to clean machine-print.
pub mod idpe;
/// v2.54.0 — the active-inference foveation planner: spend the recognizer on
/// the highest-information-scent regions until the answer resolves or the budget
/// (v2.28.0) is exhausted; every foveation is a replayable `ledger` trail entry.
pub mod foveation;
/// v2.54.0 — the IDP-E image front-end: deterministic Perona-Malik anisotropic
/// diffusion (Catté-regularised) + Gabor phase-tensor orientation energy that
/// clean and analyse a raster before recognition. The CVE-prone image DECODE is
/// isolated in the sidecar binary (`src/bin/idpe_sidecar.rs`), which feeds this
/// front-end already-decoded grayscale — hostile bytes never reach the runtime.
pub mod idpe_frontend;
pub mod inspect;
/// v2.52.0 — Native Web Acquisition runtime (`scrape_http` / `scrape_dom` /
/// `scrape_crawl`); born-Untrusted content, pluggable stealth fetcher.
pub mod scrape_tool;
/// v2.58.0 — Governed Contact Enrichment (`scrape_enrich`): structured
/// contact lookup via a pluggable enterprise provider; results born Inferred
/// (≤ believe-ceiling) + Untrusted. OSS default = typed refusal (no fabrication).
pub mod enrichment;
/// v2.77.0 — axon-agora governed social connectors (`agora_linkedin` /
/// `agora_facebook` / `agora_instagram` / `agora_tiktok`): the first official
/// library of axon-lang. Per-platform pluggable `SocialConnector` cores; every
/// result born Untrusted. OSS default = typed refusal (no fabrication).
pub mod agora_runtime;
/// v2.77.0 — the agora OAuth token-refresh orchestration (the OSS core the
/// enterprise v2.4.0 daemon drives): enumerate → decide → exchange → atomically
/// persist, closing the rotating-refresh-token trap. Clock injected; the vault
/// is the `SecretCustody` port.
pub mod agora_refresh;
/// v2.60.0 — Governed CRM Delivery (`deliver`): the egress-dual of acquisition.
/// Canonical, idempotent CRM operations delivered via a pluggable enterprise
/// transducer; each field carries its epistemic provenance or the author
/// vouched (T920). OSS default = typed refusal (no fabricated receipt).
pub mod delivery;
pub mod lambda_data;
pub mod lambda_runtime;
pub mod logging;
// v2.81.0 — `sqlx::migrate!` is a compile-time macro over `./migrations`.
#[cfg(feature = "postgres")]
pub mod migrations;
pub mod output;
pub mod mdn;
pub mod mdn_memory;
pub mod mdn_provenance;
pub mod parallel;
pub mod pix_mdn_pcc;
pub mod pix_navigator;
pub mod plan_diff;
pub mod plan_export;
pub mod rate_limiter;
pub mod request_binding;
pub mod request_log;
pub mod request_middleware;
pub mod repl;
pub mod replay;
// v2.81.0 — an `axum::middleware::from_fn` handler end to end (request
// span, trace-id header, latency record). Nothing in it survives without the
// framework, so it is gated whole rather than split.
#[cfg(feature = "server")]
pub mod request_tracing;
// v1.23.0 — Body schema validation for first-class axonendpoint
// routes. `route_schema` hosts the pure `validate_body` primitive +
// `collect_type_table` walker. The fallback handler in `axon_server`
// consults the table at request time per (method, path).
pub mod route_schema;
// v1.23.0 — Idempotency-Key store for POST/PUT axonendpoint routes.
// Stripe-compatible. Cross-tenant isolation via (client_id, path, key)
// composite key. 24h default retention. Same-key-different-body
// returns 422 per industry convention.
pub mod idempotency;
// v1.23.0 — Auth scope (capability subset matching) for first-class
// axonendpoint routes. `requires: [admin, legal.read, ...]` declarations
// gate dispatch on declared_requires ⊆ token_capabilities. Closed slug
// grammar shared with `axon_frontend::parser`. Mirror of Python
// `_is_valid_capability_slug`.
pub mod auth_scope;
// v1.23.0 — Replay-token binding for first-class axonendpoint routes.
// Append-only log keyed by trace_id; populated on every successful 2xx
// POST/PUT where `replay:` resolves to true. `GET /v1/replay/<trace_id>`
// returns the original request body + response body + metadata for
// regulatory audit (PCI DSS Req 10, FedRAMP AU-2, FRE 502, 21 CFR Part 11).
pub mod axonendpoint_replay;
// v1.24.0 — Layer 1: flow execution event stream. Closed catalog of
// {FlowStart, StepStart, StepToken, StepComplete, FlowComplete,
// FlowError} per D2. Consumed by execute_sse_handler (33.c) for live
// SSE forwarding; cross-stack drift-gated against the Python mirror.
pub mod flow_execution_event;
pub mod resilient_backend;
pub mod retry_policy;
pub mod runner;
// v2.81.0 — the version string in a leaf module with no dependencies. It
// used to live in `runner`, which put the whole flow executor (sqlx, reqwest,
// tokio, axum, axon-csys) into the reachable set of every compiler-side
// subcommand that wanted a string literal. See `version.rs`.
pub mod version;
// v2.81.0 — the ingest provenance lattice, dependency-free. See the module
// docs: it lived in `ooxml_read` and dragged the OOXML surface behind it.
pub mod ingest_provenance;
// v2.81.0 — THE THIRD INSTANCE OF THE SMELL, and the largest. The flow
// execution RESULT (`ServerExecutionResult`, `EnforcementSummaryWire`) lived in
// `axon_server`, so `flow_dispatcher`, `streaming_via_dispatcher` and
// `wire_envelope` — the core execution path — could not name their own output
// without the HTTP server. Both are pure data; `axon_server` re-exports them.
pub mod execution_result;
// v2.81.0 — `parse_truthy_env`, the cross-stack truthy contract shared
// with the Python CLI. It reads an env var; it lived in `axon_server`, and
// `main.rs` called it there while building `ServerConfig`.
pub mod env_flags;
// v2.81.0 / the design decision — the pinned-connection PORT. The executor used to name
// `sqlx::pool::PoolConnection<sqlx::Postgres>` in its own signatures, threading a
// concrete database type through `runner` -> `flow_dispatcher` ->
// `streaming_via_dispatcher`, i.e. the cognition path. It now names `PinnedConn`
// and cannot reach the driver at all. See the module docs for why a newtype beat
// a trait (the executor never calls a method on a pin — it only holds one).
pub mod pinned_conn;
// v2.0.0 — public shield-scanner registration hook. OSS ships no
// scanners (identity); enterprise vertical crates register HIPAA/legal/AML
// scanners here at boot. The `shield apply` handler consults it.
pub mod shield_registry;
/// v2.67.0 — the Cognitive-I/O supervisor: the loop that instantiates the
/// declared λ-L-E dataflow graph (`observe` → {`ensemble`, `immune`} →
/// {`reflex`, `heal`}, plus `reconcile`) and drives it. The language was complete
/// and the kernels took the IR directly; **nobody had ever built the loop.**
pub mod cognitive_io_supervisor;
/// v2.67.0 — the source adapter registry: what an `observe` actually looks at.
/// **Deny-by-default** — an unregistered source is UNKNOWN, not healthy, and the
/// observation refuses rather than fabricating a reading.
pub mod source_registry;
pub mod server_config;
pub mod server_metrics;
pub mod session_scope;
pub mod session_store;
pub mod step_deps;
pub mod storage;
// v2.81.0 — the tenant-scoped RLS storage layer. `storage.rs` (the port +
// the in-memory backend) stays in every build; only this implementation goes.
#[cfg(feature = "postgres")]
pub mod storage_postgres;
// v1.30.0 — the `axonstore` cognitive data plane runtime. 35.b ships
// `store::filter` (the parameterized where-expression compiler).
pub mod resource_lease;
pub mod resource_resolver;
pub mod store;
pub mod stdlib;
// v2.81.0 — tenant EXTRACTION (JWKS verification + the axum middleware
// that resolves a tenant from an inbound request) is server code and is gated as
// such. Tenant IDENTITY — the task-local, `TenantPlan`, `TenantContext`,
// `current_tenant_id`, `scope_tenant` — moved to `tenant_context` below, because
// `storage_postgres` reads it in 31 places to build the RLS `SET LOCAL` of every
// query and should never have needed a web framework to do it. `tenant`
// re-exports all of it, so `axon::tenant::current_tenant_id` still resolves.
#[cfg(feature = "server")]
pub mod tenant;
/// v2.81.0 — tenant identity, dependency-free. See the module docs.
pub mod tenant_context;
pub mod tenant_secrets;
// v1.4.0 — JWT signature verification + JWKS client. Used by
// tenant::tenant_extractor_middleware when AXON_JWT_JWKS_URL is set.
pub mod jwt_verifier;
// v1.4.0 runtime — `trust_verifiers` holds the runtime
// implementations that the compiler recognises; `stream_runtime` is
// the Stream<T> channel with policy dispatch. The compile-time
// `refinement` and `stream_effect` catalogs live in `axon-frontend`.
pub mod trust_verifiers;
pub mod stream_runtime;
// v1.24.0 — Stream-effect dispatcher (Layer 4 of the v1.24.0 cycle).
// Bridges the `effects: <stream:<policy>>` declarations on tool
// definitions to actual runtime backpressure behavior on the SSE
// wire. The dispatcher itself is a thin composition over
// `stream_runtime::Stream<T>` (which carries the policy semantics)
// and the AST resolver (which extracts the declared policy from the
// tool referenced by each step).
pub mod stream_effect_dispatcher;
// v1.24.0 — Cooperative cancellation primitives (D6 cancel-safety).
// `CancellationFlag` + `CancelOnDrop` are the building blocks that
// bind SSE response lifetime to the executor's spawn_blocking task:
// when the wire client disconnects, the consumer cancels the flag,
// which the producer observes between event emissions and exits
// early instead of running the flow to completion against a dropped
// channel.
pub mod cancel_token;
pub mod channel_semaphore;
// v1.28.0 — Wire-format adapter framework.
// `wire_format` defines the WireFormatAdapter trait + per-dialect
// adapters (axon / openai / anthropic). The SSE producer in
// `axon_server::execute_sse_handler` uses `select_adapter(dialect)`
// to translate internal FlowExecutionEvents into the dialect-
// specific wire shape adopters' SDKs expect.
//
// v2.81.0 — behind the `server` feature. Every adapter builds
// `axum::response::sse::Event`, and its only consumer is the SSE producer in
// `axon_server`. D: GATE, do not define our own event type — an SSE `Event` is
// four fields, but inventing a parallel one with a single consumer would add an
// abstraction to avoid a dependency that the only caller already has. If a
// non-HTTP dialect consumer ever appears, that is the moment to own the type.
#[cfg(feature = "server")]
pub mod wire_format;
// v2.0.0 — Pure Silicon Cognition wire envelope. The canonical
// `FlowEnvelope` payload for `transport: json` axonendpoint responses
// + legacy `POST /v1/execute`. Isomorphic serialization of the
// ψ-vector `⟨T, V, E⟩`. See `docs/cycle/pure_silicon_cognition.md`.
pub mod wire_envelope;
// v2.0.0 — Wire envelope producer helpers. Closed-taxonomy
// translators from runtime execution metadata into the wire envelope's
// epistemic fields (`provenance_chain` + `blame_attribution`).
pub mod wire_envelope_producers;
// v2.0.0 — Rust CLI binary parity. New subcommands that closed
// the gap vs the Python CLI (`axon parse` aggregator + `axon fmt`
// round-trip formatter).
pub mod cli_parse;
pub mod cli_fmt;
// v1.4.0 — Zero-Copy Multimodal Buffers.
// `buffer` defines ZeroCopyBuffer (Arc<[u8]>-backed) + BufferKind
// (open registry) + BufferPool (slab allocator with per-tenant
// soft-limit accounting). `ingest` hosts the network deposit paths
// (multipart/form-data streaming parser, WebSocket binary-frame
// accumulator) that populate buffers without intermediate copies.
pub mod buffer;
pub mod ingest;
// v1.4.0 runtime — `replay_token` hosts ReplayToken canonical
// hashing + pluggable ReplayLog + ReplayExecutor for re-running from
// any token. The compile-time `legal_basis` catalog lives in
// `axon-frontend`.
pub mod replay_token;
// v1.4.0 — Stateful PEM over WebSocket. `pem::state`
// defines CognitiveState with Q32.32 fixed-point float encoding
// so density-matrix round-trips are bit-identical across reconnects.
// `pem::continuity_token` is an HMAC-signed handshake that proves
// a reconnecting client is the original party. `pem::backend`
// exposes the PersistenceBackend async trait + in-memory impl;
// production uses axon_enterprise::cognitive_states (Postgres +
// envelope encryption).
/// v2.83.0 — the `mandate` enforcement engine: the closed loop that
/// refuses to release any output its constraint set rejects.
pub mod mandate_engine;
/// v2.83.0 — the name-keyed `ots` transformer registry (shield_registry's
/// proven shape). An unregistered ots REFUSES at dispatch — a transformation
/// that transforms nothing is the v2.67.0 F18 lie.
pub mod ots_registry;
/// v2.83.0 — `hibernate`: the parking lot, the continuation id, and the
/// lazy-expiry timeout. The flow HALTS; resume rides `emit`.
pub mod hibernation;
pub mod pem;
// v1.4.0 — Ontological Tool Synthesis binary pipelines.
// `ots::pipeline` hosts Transformer trait + TransformerRegistry +
// Dijkstra-based path search. `ots::native` seeds μ-law ↔ PCM16
// + resample (8k/16k/48k ladder). `ots::subprocess::ffmpeg` is
// the subprocess fallback with warm-pool + availability detection.
// The compile-time slug catalog lives in `axon-frontend::ots_catalog`.
pub mod ots;
pub mod tool_executor;
pub mod tool_registry;
// v1.29.0 — Tool trait + ToolChunk closed-catalog
// surface for tools-as-stream-producers. Bridges adopter-source
// `effects: <stream:<policy>>` declarations into the runtime via
// the dispatcher's per-chunk wire emission path (v1.29.0 lands
// the wiring; this module is the structural foundation).
pub mod tool_trait;
// v1.29.0 — Bridge from ToolEntry (registry shape) to
// Tool trait impls (dispatcher's streaming surface). The dispatcher's
// `pure_shape::run_step` calls `tool_dispatch_bridge::resolve_streaming_tool`
// for is_streaming-flagged tools + drains the resulting Stream<ToolChunk>
// chunk-by-chunk into the wire.
pub mod tool_dispatch_bridge;
// v2.39.0 — Remote Hands runtime: pure argv render + confirmation-hash
// binding + output bounding + the axon⇄agent wire protocol.
pub mod technician_dispatch;
// v2.40.0 — result-memoization cache core: content-addressed keys,
// in-process LRU tier with single-flight + TTL jitter + size bound, the
// `CacheBackend` trait (enterprise injects Redis), and policy resolution.
pub mod cache_runtime;
// v2.41.0 — the mathematical core of `forge` Directed Creative Synthesis:
// Boden profiles, NCD novelty (the computable Kolmogorov-novelty proxy),
// best-of-N selection, and fail-closed verification.
pub mod forge;
// v2.42.0 — the `HolographBackend` port + the OSS reference HRR codec
// (circular-convolution binding via a self-contained radix-2 FFT), the
// `savant` long-horizon memory-compression layer (paper section 5).
pub mod holograph;
// v2.42.0 — the remaining `savant` runtime ports + OSS reference impls:
// `inference` (classical VFE/EFE active inference, no advantage claim),
// `topology` (Vietoris–Rips β₀/β₁ + PHC-proxy centrality), and `synth`
// (deny-by-default dynamic tool synthesis — the Extism executor is enterprise).
pub mod inference;
pub mod synth;
pub mod topology;
// v2.43.0 — the `WardenBackend` port + the OSS reference static analyzer
// (attested `Vulnerability` findings; authorization + deny-by-default enforced;
// paraconsistent finding-validator). The enterprise LLM engine mounts v2.43.0.
pub mod warden;
pub mod tool_validator;
pub mod trace_export;
pub mod trace_store;
pub mod trace_stats;
pub mod tracer;
pub mod version_diff;
pub mod webhook_delivery;
pub mod webhooks;
/// v2.46.0 — the `CredentialMinter` port behind the `mint` flow verb
/// (attenuated, TTL-bounded ephemeral credentials; fail-closed when absent).
pub mod credential_minter;
/// v2.48.0 — the `SecretCustody` port behind the `backend: secrets`
/// metadata store, the `rotate` verb and the `tool { secret: }` injection
/// (`rotation_without_revelation`; fail-closed when absent).
pub mod secret_custody;
/// v2.46.0 — declared cognitive time: the runtime half of `now:` (one
/// capture per run, deterministic prompt line, envelope record).
pub mod temporal_context;
/// v2.27.0 — the runtime for the `window` temporal execution guard
/// (timezone-aware `is_in_window` / `next_window_open` via chrono-tz).
pub mod window;
