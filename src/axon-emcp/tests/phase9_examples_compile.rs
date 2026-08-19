//! Phase 9 — every example shipped under `knowledge/examples/` must
//! compile end-to-end through the same `axon-frontend` pipeline the
//! `axon` CLI uses, and its frontmatter must parse cleanly.
//!
//! This is the drift gate for `axon.examples`: the moment an example
//! stops parsing the test fails, so the agent never receives a
//! malformed snippet — even mid-refactor.
//!
//! The gate runs in two layers:
//!
//! 1. **Catalog layer** — load the embedded corpus, verify each
//!    expected example slug is present, frontmatter fields populated,
//!    topic is a closed-catalog value.
//! 2. **Compile layer** — every `source` field round-trips through
//!    `axon-frontend`'s lex → parse → type-check pipeline; any
//!    failure surfaces with a structured panic naming the slug + the
//!    failing stage.

use axon_emcp::compiler_pipeline::{run, Outcome};
use axon_emcp::knowledge::{Catalog, ExampleTopic};

/// Canonical list of every example we ship — the third side of the
/// drift gate (`registry ↔ corpus ↔ test`). Adding a new example
/// requires touching all three sides; the test catches the drift if
/// any side is forgotten.
const EXPECTED: &[(&str, ExampleTopic)] = &[
    // Composition (5)
    ("flow_step_basic", ExampleTopic::Composition),
    ("flow_chaining", ExampleTopic::Composition),
    ("tool_use_basic", ExampleTopic::Composition),
    // v2.8.0 — the structured `use <Tool>(k = v, …)` dispatch form.
    ("tool_structured_args", ExampleTopic::Composition),
    ("weave_braid", ExampleTopic::Composition),
    // v2.65.0 — the proof-carrying derivative (grad over the closed Expr).
    ("grad_proof_carrying", ExampleTopic::Composition),
    // v2.19.0 — the quant cognitive primitive feature map.
    ("quant_feature_map", ExampleTopic::Composition),
    // v2.46.0 — declared cognitive time (`now:` on step + context frame).
    ("temporal_cognitive_context", ExampleTopic::Composition),
    // v2.76.0 — the Epistemic Module System: multi-file import + ECC + link.
    ("module_imports", ExampleTopic::Composition),
    // Session types (2)
    ("session_chat_duality", ExampleTopic::SessionTypes),
    ("socket_websocket_chat", ExampleTopic::SessionTypes),
    // Shields (1)
    ("shield_input_output", ExampleTopic::Shields),
    // Effects (1)
    ("lambda_epistemic_stamp", ExampleTopic::Effects),
    // Streaming (1)
    ("streaming_chat", ExampleTopic::Streaming),
    // Data (3)
    ("axonstore_typed", ExampleTopic::Data),
    ("dataspace_basic", ExampleTopic::Data),
    // v2.48.0 — the secret-custody rotation lifecycle (backend: secrets
    // + rotate + tool secret injection; `rotation_without_revelation`).
    ("secret_custody_rotation", ExampleTopic::Data),
    // v2.49.0 — parametric secret injection: one tool, N sub-tenants
    // (`secret_partition:`; `selection_without_revelation`).
    ("secret_partition_multitenant", ExampleTopic::Data),
    // v2.60.0 — governed CRM delivery (acquire→enrich→deliver, provenance intact).
    ("governed_crm_delivery", ExampleTopic::Data),
    // v2.63.0 — the deterministic data plane: ingest → σ → γ, every
    // number COMPUTED (dataspace + the governed pipeline).
    ("governed_data_pipeline", ExampleTopic::Data),
    // v2.66.0 — governed human notification (daemon → aggregate → notify).
    ("governed_notification", ExampleTopic::Data),
    // Agents (2)
    ("agent_react", ExampleTopic::Agents),
    ("reflex_to_immune", ExampleTopic::Agents),
    // Endpoints (2)
    ("axonendpoint_rest", ExampleTopic::Endpoints),
    // v2.38.0 — the named, referenced browser-origin policy.
    ("cors_named_origin_policy", ExampleTopic::Endpoints),
    // v2.46.0 — the ephemeral widget credential (credential + mint + cors).
    ("widget_ephemeral_credential", ExampleTopic::Endpoints),
    // v2.62.0 — HTTP QUERY (RFC 10008): a complex read, safe by compile-time proof.
    ("query_safe_search", ExampleTopic::Endpoints),
    // Memory (1)
    ("memory_scopes", ExampleTopic::Memory),
    // Validation (2)
    ("anchor_validation", ExampleTopic::Validation),
    ("mandate_policy", ExampleTopic::Validation),
];

/// Phase 9 catalog gate — every expected example is embedded, its
/// frontmatter is well-formed, and the topic matches the closed
/// catalog. A regression here means a contributor renamed / removed
/// an example without updating `EXPECTED`.
#[test]
fn embedded_corpus_contains_every_phase_9_example() {
    let cat = Catalog::load_embedded().expect("embedded corpus must load");
    for (slug, topic) in EXPECTED {
        let e = cat
            .example(slug)
            .unwrap_or_else(|| panic!("example `{slug}` must be embedded"));
        assert_eq!(&e.name, slug, "{slug}: slug drift");
        assert_eq!(e.topic, *topic, "{slug}: topic drift");
        assert!(!e.title.is_empty(), "{slug}: title empty");
        assert!(!e.summary.is_empty(), "{slug}: summary empty");
        assert!(
            !e.source.is_empty(),
            "{slug}: source empty — axon.examples would return nothing"
        );
        assert!(
            !e.primitives.is_empty(),
            "{slug}: primitives list empty — primitive-filter queries would never hit this entry"
        );
    }
    assert_eq!(
        cat.example_count(),
        EXPECTED.len(),
        "example count drift — add the new example to EXPECTED, or remove the orphan from disk"
    );
}

/// Phase 9 compile gate — every embedded example's source compiles
/// clean through the same `axon-frontend` pipeline the `axon` CLI
/// uses. A failure here means the snippet would be returned to the
/// agent but would NOT compile if the agent tried to use it — the
/// exact failure mode this gate exists to prevent.
#[test]
fn every_embedded_example_source_compiles_clean() {
    let cat = Catalog::load_embedded().expect("embedded corpus must load");
    for e in cat.examples() {
        let filename = format!("{}.md", e.name);
        match run(&e.source, &filename) {
            Outcome::Ok { .. } => { /* compiles clean — the whole assertion */ }
            Outcome::Err { stage, errors, warnings } => panic!(
                "example `{}` failed at {stage:?}:\n\
                 errors   = {errors:#?}\n\
                 warnings = {warnings:#?}\n\
                 source   =\n{}\n",
                e.name, e.source,
            ),
        }
    }
}

/// Phase 9 topic gate — every closed `ExampleTopic` is represented
/// by AT LEAST one example. The point of the topic catalogue is to
/// let the agent navigate the corpus by axis; an empty topic axis
/// degrades the discovery surface silently.
#[test]
fn every_topic_has_at_least_one_example() {
    let cat = Catalog::load_embedded().expect("embedded corpus must load");
    for topic in ExampleTopic::all() {
        let count = cat.examples_of(*topic).count();
        assert!(
            count >= 1,
            "topic `{}` has zero examples — every closed topic must be populated",
            topic.as_str()
        );
    }
}

/// Phase 9 primitive-filter gate — `examples_using(primitive)` is
/// the lookup the agent uses to find "how do I use weave?". This test
/// spot-checks that the canonical primitives an agent asks about
/// FIRST are reachable through that lookup. Adding a new must-cover
/// primitive lands as a new row here.
#[test]
fn canonical_primitives_are_reachable_via_examples_using() {
    let cat = Catalog::load_embedded().expect("embedded corpus must load");
    let must_cover: &[&str] = &[
        // Tier 0 baseline — the primitives an agent always looks up
        // before composing anything.
        "persona", "flow", "step", "anchor", "tool",
        // Tier 1 — the named "do something distinctive" primitives.
        "shield", "weave", "memory", "lambda", "mandate",
        // Phase 9 also surfaces session/socket/agent/axonendpoint —
        // the primitives that anchor the four large topic axes.
        "session", "socket", "agent", "axonendpoint",
        // v2.19.0 — the quant cognitive primitive + its observable.
        // An adopter asking "how do I write a quant block?" MUST hit an example
        // (the brief-#29 failure mode: 0 examples → adopter guesses the grammar).
        "quant", "observable",
    ];
    for prim in must_cover {
        let count = cat.examples_using(prim).count();
        assert!(
            count >= 1,
            "primitive `{prim}` has zero examples — every must-cover primitive must be reachable via examples_using()"
        );
    }
}
