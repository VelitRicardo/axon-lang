//! Phase 2 — every primitive doc shipped under
//! `src/knowledge/primitives/` must be backed by a real `.axon` example
//! that compiles end-to-end through the same `axon-frontend` pipeline
//! the `axon` CLI uses (lex → parse → type-check → IR-generate).
//!
//! These canonical programs are intentionally *minimal*: just enough to
//! exercise the primitive's grammar surface so the docs can never drift
//! from the actual parser. If a primitive's documented grammar stops
//! parsing, this test fails — same gate the language uses internally.
//!
//! The 6 primitives covered here are the core cognitive set an agent
//! touches before anything else: `persona`, `flow`, `step`, `anchor`,
//! `tool`, `reason`. The remaining 60+ primitives will land alongside
//! their own doc entries in later Phase 2 increments.

use axon_emcp::compiler_pipeline::{run, Outcome, Stage};

/// Run a canonical program through the full pipeline and panic with
/// the structured diagnostics if it fails. Agents see the same payload
/// when they call `axon.check`, so a failure here mirrors the failure
/// an agent would surface.
fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => { /* well-formed, that's the whole assertion */ }
        Outcome::Err {
            stage,
            errors,
            warnings,
        } => {
            panic!(
                "{label}: expected well-formed program, got {stage:?} failure:\n\
                 errors   = {errors:#?}\n\
                 warnings = {warnings:#?}\n\
                 source   = {source}"
            );
        }
    }
}

#[test]
fn persona_canonical_program_compiles() {
    // Mirrors the example in `src/knowledge/primitives/persona.md`.
    // Every field documented in the YAML grammar block is exercised.
    let src = r#"
persona LegalExpert {
    domain: ["contract law", "IP", "corporate"]
    tone: precise
    confidence_threshold: 0.85
    cite_sources: true
    language: "en"
    description: "Senior corporate counsel, US/UK common law focus."
}
"#;
    must_compile("persona/canonical", src);
}

#[test]
fn flow_canonical_program_compiles() {
    // Mirrors the example in `src/knowledge/primitives/flow.md`:
    // typed parameters, typed return, two sequenced steps with a
    // forward reference (`Assess` consumes `Extract.output`).
    let src = r#"
type Document {
    body: String
}
type EntityMap {
    entities: String
}
type RiskAnalysis {
    risks: String
}
type ContractAnalysis {
    summary: String
}

flow AnalyzeContract(doc: Document) -> ContractAnalysis {
    step Extract {
        given: doc
        ask: "Extract parties, obligations, dates, penalties"
        output: EntityMap
    }
    step Assess {
        given: Extract.output
        ask: "Identify ambiguous or risky clauses"
        output: RiskAnalysis
    }
}
"#;
    must_compile("flow/canonical", src);
}

#[test]
fn step_canonical_program_compiles() {
    // Mirrors the example in `src/knowledge/primitives/step.md`:
    // a `use <Persona>` header override + `confidence_floor:` body
    // field — the surface most likely to drift if step grammar moves.
    let src = r#"
persona FriendlyAssistant {
    domain: ["greetings"]
    tone: empathetic
}

type Greeting {
    text: String
}

flow GreetUser(name: String) -> Greeting {
    step ComposeGreeting use FriendlyAssistant {
        given: name
        ask: "Write a warm, locale-aware greeting"
        output: Greeting
        confidence_floor: 0.7
    }
}
"#;
    must_compile("step/canonical", src);
}

#[test]
fn anchor_canonical_program_compiles() {
    // Mirrors the example in `src/knowledge/primitives/anchor.md`:
    // `require:` + `confidence_floor:` + `unknown_response:` +
    // `on_violation: raise ...`. The richest field set of any anchor.
    let src = r#"
anchor NoHallucination {
    require: source_citation
    confidence_floor: 0.75
    unknown_response: "I don't have sufficient information."
    on_violation: raise AnchorBreachError
}
"#;
    must_compile("anchor/canonical", src);
}

#[test]
fn tool_canonical_program_compiles() {
    // Mirrors the example in `src/knowledge/primitives/tool.md`:
    // every optional field exercised so the grammar of `tool` (the
    // legacy primitive most likely to grow knobs) is locked in.
    let src = r#"
tool WebSearch {
    provider: http
    max_results: 5
    timeout: 10s
}
"#;
    must_compile("tool/canonical", src);
}

#[test]
fn reason_canonical_program_compiles() {
    // Mirrors the example in `src/knowledge/primitives/reason.md`:
    // `reason` appearing BOTH as a flow-level sibling of `step`
    // (`reason chain_of_thought Frame.output`) AND as a step-level
    // sub-construct (`reason debate` inside `step Decide`).
    let src = r#"
persona Reasoner {
    domain: ["argumentation"]
    tone: precise
}

type Claim {
    text: String
}
type NormalisedClaim {
    text: String
}
type Verdict {
    decision: String
}

flow ResolveAmbiguity(claim: Claim) -> Verdict {
    step Frame {
        given: claim
        ask: "Restate the claim in unambiguous form"
        output: NormalisedClaim
    }
    reason chain_of_thought
    step Decide {
        given: Frame.output
        ask: "Apply the reasoning chain and emit the verdict"
        output: Verdict
        reason debate
    }
}
"#;
    must_compile("reason/canonical", src);
}

/// A negative companion: malformed input from the perspective of one of
/// the Phase 2 primitives MUST still produce a structured diagnostic at
/// the documented stage (parse), so an agent's reflex on `isError:
/// true` fires correctly even on docs-driven examples.
#[test]
fn malformed_persona_surfaces_a_parse_or_lex_diagnostic() {
    // Missing closing brace — must fail at the lex/parse stage with a
    // structured diagnostic the agent can read. We don't pin the exact
    // message (the parser may rephrase it across releases) but we DO
    // pin the stage so docs + diagnostic shape stay aligned.
    let src = r#"
persona Broken {
    domain: ["x"]
    tone: precise
"#;
    match run(src, "persona/malformed") {
        Outcome::Err { stage, errors, .. } => {
            assert!(
                matches!(stage, Stage::Lex | Stage::Parse),
                "malformed persona should fail at lex or parse, got {stage:?}"
            );
            assert!(
                !errors.is_empty(),
                "must surface ≥1 structured diagnostic for the agent"
            );
        }
        Outcome::Ok { .. } => {
            panic!("malformed persona unexpectedly accepted as well-formed");
        }
    }
}
