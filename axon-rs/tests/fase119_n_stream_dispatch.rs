//! §Fase 119.n — **`stream<T>`'s handlers are DISPATCHED**, from SOURCE.
//!
//! This file exists to satisfy law 4 of [`axon_frontend::advertised`]: *"the
//! proof must cite the PATH, not the engine."* `stream` was attested
//! `Real { proof: "tests/fase111_e_stream_runs.rs" }` and that test passed —
//! it exercised §111.e's `body: Vec<FlowStep>`, a shape **no published block and
//! no paper writes**. The engine ran; nothing an adopter could write reached it.
//! So every assertion below starts at `.axon` SOURCE and ends at the wire. A
//! hand-built `IRStep` would prove the same nothing the old gate proved.
//!
//! **The defect being pinned shut.** Before §119.n the step-body parser routed
//! `stream` to `skip_flow_step_structural`. README block 15's `step Stream`
//! reached this dispatcher with `pix_ops=0`, `ask=""`, `output=""` — the
//! `probe`, the `validate` and BOTH `output:` declarations discarded at parse
//! time — and `axon check` reported `0 errors`. The block had already left the
//! §117 ledger on the strength of compiling.
//!
//! **The chunk source.** `on_chunk` runs over the chunks the step's own
//! generation produces (`fase_23` §3.7). A step with no `ask:` has no source, and
//! §3 below pins that it FAILS CLOSED naming what is missing rather than
//! completing empty — an empty completion is indistinguishable from a stream
//! that genuinely had no chunks, and manufacturing that silence is the §112
//! kernel defect wearing a stream's clothes.

use axon::cancel_token::CancellationFlag;
use axon::flow_dispatcher::pure_shape::run_step;
use axon::flow_dispatcher::DispatchCtx;
use axon::flow_execution_event::FlowExecutionEvent;
use axon::ir_nodes::{IRFlowNode, IRStep};
use axon::tool_registry::{ToolEntry, ToolRegistry, ToolSource};
use std::sync::Arc;
use tokio::sync::mpsc;

/// SOURCE → AST → IR → the first `step` node. The whole point of this file.
fn step_from_source(src: &str) -> IRStep {
    let tokens = axon_frontend::lexer::Lexer::new(src, "gate.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("the published surface must parse");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    for flow in &ir.flows {
        for node in &flow.steps {
            if let IRFlowNode::Step(s) = node {
                return s.clone();
            }
        }
    }
    panic!("no step in the generated IR");
}

fn ctx() -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx),
        rx,
    )
}

fn step_start_slugs(rx: &mut mpsc::UnboundedReceiver<FlowExecutionEvent>) -> Vec<String> {
    let mut slugs = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let FlowExecutionEvent::StepStart { step_type, .. } = ev {
            slugs.push(step_type);
        }
    }
    slugs
}

/// A step whose generation IS the stream: it has an `ask:` (the source) and the
/// published handler block.
const WITH_SOURCE: &str = r#"
flow F(x: String) -> Text {
    step Quotes {
        ask: "emit the quote feed"
        stream<QuoteData> {
            on_chunk: {
                probe chunk for [symbol, price]
                output: QuoteSnapshot
            }
            on_complete: {
                output: VerifiedQuote
            }
        }
        output: Text
    }
}
"#;

// ── §1 — the handlers reach the dispatcher at all ────────────────────────────

/// The IR must carry the block off the STEP (not `pix_ops` — a stream handler is
/// not an elevation), or there is nothing for the dispatcher to read.
#[test]
fn s1_the_published_source_lowers_the_block_onto_the_step() {
    let step = step_from_source(WITH_SOURCE);
    let block = step
        .stream
        .as_ref()
        .expect("`stream<T> { … }` in a step body must reach IRStep.stream");
    assert_eq!(block.chunk_type, "QuoteData");
    assert!(block.on_chunk.is_some(), "`on_chunk` must lower");
    assert!(block.on_complete.is_some(), "`on_complete` must lower");
    assert!(
        step.pix_ops.is_empty(),
        "a stream handler is NOT an elevation — filing it under pix_ops would run \
         it before generation and then generate on an empty prompt"
    );
}

/// The reader. `on_chunk` must actually run — this is the assertion whose
/// absence made §111.e's attestation worthless.
#[tokio::test]
async fn s1b_on_chunk_runs_over_the_steps_own_chunks() {
    let (mut c, mut rx) = ctx();
    let step = step_from_source(WITH_SOURCE);

    run_step(&step, &mut c).await.expect("run_step");

    let slugs = step_start_slugs(&mut rx);
    assert!(
        slugs.iter().filter(|s| s.as_str() == "step").count() >= 2,
        "`on_chunk` is a step body and must DISPATCH as one, once per chunk, in \
         addition to the enclosing step. Slugs observed: {slugs:?}"
    );
    assert!(
        slugs.iter().any(|s| s == "probe"),
        "the `probe chunk for […]` written INSIDE the handler must run too — the \
         arm is a real step body, not a skipped brace. Slugs: {slugs:?}"
    );
}

/// The chunk is BOUND, under the name README block 15 uses.
#[tokio::test]
async fn s1c_the_chunk_is_bound_as_chunk() {
    let (mut c, _rx) = ctx();
    let step = step_from_source(WITH_SOURCE);

    run_step(&step, &mut c).await.expect("run_step");

    assert!(
        c.let_bindings.contains_key("chunk"),
        "`on_chunk`'s body writes `probe chunk for […]`, so the chunk must be \
         bound as `chunk` before the arm dispatches. Bindings: {:?}",
        c.let_bindings.keys().collect::<Vec<_>>()
    );
}

// ── §2 — on_complete closes the stream and owns the output ───────────────────

#[tokio::test]
async fn s2_on_complete_runs_and_binds_the_accumulation() {
    let (mut c, _rx) = ctx();
    let step = step_from_source(WITH_SOURCE);

    run_step(&step, &mut c).await.expect("run_step");

    assert!(
        c.let_bindings.contains_key("complete"),
        "`on_complete` must see the accumulated stream bound as `complete`. \
         Bindings: {:?}",
        c.let_bindings.keys().collect::<Vec<_>>()
    );
}

// ── §3 — no source FAILS CLOSED ──────────────────────────────────────────────

/// README block 15's exact shape: handlers, and nothing producing chunks. It used
/// to compile to an EMPTY step and complete successfully.
#[tokio::test]
async fn s3_a_stream_with_no_source_refuses_and_names_what_is_missing() {
    let src = r#"
flow MonitorMarket(sector: String) -> MarketReport {
    step Stream {
        stream<QuoteData> {
            on_chunk: {
                probe chunk for [symbol, price, volume]
                output: QuoteSnapshot
            }
        }
    }
}
"#;
    let (mut c, _rx) = ctx();
    let step = step_from_source(src);

    let err = run_step(&step, &mut c)
        .await
        .expect_err("a stream with no chunk source must REFUSE, not complete empty");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("QuoteData"),
        "the diagnostic must name the declared chunk type: {msg}"
    );
    assert!(
        msg.contains("ask:"),
        "and must name the source the author has to supply: {msg}"
    );
}

// ── §4 — the chunk source can be a streaming TOOL ────────────────────────────
//
// README block 15's own shape: `tool MarketFeed` feeding `stream<QuoteData>`.
// The tool's chunks are the stream; `on_chunk` runs once per TOOL chunk, and the
// handler rides the drain that already enforces the tool's declared
// `backpressure:` policy rather than a private copy of it.

fn tool_entry(name: &str, provider: &str, effect_row: Vec<&str>, is_streaming: bool) -> ToolEntry {
    ToolEntry {
        name: name.into(),
        provider: provider.into(),
        timeout: String::new(),
        runtime: String::new(),
        resource_ref: String::new(),
        substrate: None,
        capacity: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: effect_row.into_iter().map(String::from).collect(),
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: ToolSource::Program,
        is_streaming,
        scrape: None,
    }
}

fn ctx_with(entries: Vec<ToolEntry>) -> (DispatchCtx, mpsc::UnboundedReceiver<FlowExecutionEvent>) {
    let mut reg = ToolRegistry::new();
    for e in entries {
        reg.register(e);
    }
    let (tx, rx) = mpsc::unbounded_channel();
    (
        DispatchCtx::new("F", "stub", "", CancellationFlag::new(), tx)
            .with_tool_registry(Arc::new(reg)),
        rx,
    )
}

/// Block 15's shape, completed: the feed is BOUND to the step with `apply:`.
const TOOL_SOURCED: &str = r#"
flow MonitorMarket(sector: String) -> MarketReport {
    step Quotes {
        apply: MarketFeed
        ask: "start the quote feed"
        stream<QuoteData> {
            on_chunk: {
                probe chunk for [symbol, price, volume]
                output: QuoteSnapshot
            }
            on_complete: {
                output: VerifiedQuote
            }
        }
        output: MarketReport
    }
}
"#;

/// The `stub_stream` provider yields 3 non-empty chunks + a terminator, so
/// `on_chunk` must dispatch exactly 3 times — once per TOOL chunk.
#[tokio::test]
async fn s4_on_chunk_runs_once_per_tool_chunk() {
    let (mut c, mut rx) = ctx_with(vec![tool_entry(
        "MarketFeed",
        "stub_stream",
        vec!["stream:drop_oldest"],
        true,
    )]);
    let step = step_from_source(TOOL_SOURCED);

    run_step(&step, &mut c).await.expect("run_step");

    let slugs = step_start_slugs(&mut rx);
    let probes = slugs.iter().filter(|s| s.as_str() == "probe").count();
    assert_eq!(
        probes, 3,
        "the tool yields 3 non-empty chunks, so the `probe` inside `on_chunk` must \
         run 3 times — once per chunk, during the stream. Slugs: {slugs:?}"
    );
}

/// An `apply:` that resolves to a NON-streaming tool must refuse, not fall back
/// to the step's own generation. Falling back would stream the model's words
/// while the program says it is streaming the feed.
#[tokio::test]
async fn s4b_a_non_streaming_apply_is_refused_not_demoted() {
    let (mut c, _rx) = ctx_with(vec![tool_entry("MarketFeed", "http", vec!["io"], false)]);
    let step = step_from_source(TOOL_SOURCED);

    let err = run_step(&step, &mut c)
        .await
        .expect_err("a non-streaming tool produces one value, not a sequence");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("MarketFeed"),
        "the diagnostic must name the tool: {msg}"
    );
    assert!(
        msg.contains("not a STREAMING tool"),
        "and say why it cannot be the source: {msg}"
    );
}

/// A source named but not mounted. Refuses rather than running handlers over a
/// producer that is not there.
#[tokio::test]
async fn s4c_an_unregistered_apply_is_refused() {
    let (mut c, _rx) = ctx_with(vec![]);
    let step = step_from_source(TOOL_SOURCED);

    let err = run_step(&step, &mut c)
        .await
        .expect_err("nothing produces chunks when the tool is not registered");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("MarketFeed") && msg.contains("no such tool is registered"),
        "the diagnostic must name the missing source: {msg}"
    );
}

// ── §5 — README block 15, VERBATIM, RUNS ─────────────────────────────────────

/// The whole point, end to end. This is the published block character-for-
/// character, and before §119.n it reached the dispatcher as an EMPTY step.
///
/// §119.n also completed the example: it declared `tool MarketFeed` and never
/// bound it, so the block named no source. `apply: MarketFeed` (plus the
/// `stream:` effect that makes the feed a streaming tool) is the §119.m.3
/// repair — an incomplete example, not a grammar gap.
const README_BLOCK_15_VERBATIM: &str = r#"
tool MarketFeed {
    timeout: 5s
    effects: <io, network, epistemic:speculate, stream:drop_oldest>
}

flow MonitorMarket(sector: String) -> MarketReport {
    step Stream {
        apply: MarketFeed
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
"#;

#[tokio::test]
async fn s5_readme_block_15_verbatim_runs_with_the_feed_mounted() {
    let (mut c, mut rx) = ctx_with(vec![tool_entry(
        "MarketFeed",
        "stub_stream",
        vec!["io", "network", "epistemic:speculate", "stream:drop_oldest"],
        true,
    )]);
    let step = step_from_source(README_BLOCK_15_VERBATIM);

    run_step(&step, &mut c)
        .await
        .expect("README block 15's first step must RUN, not arrive empty");

    let slugs = step_start_slugs(&mut rx);
    assert!(
        slugs.iter().any(|s| s == "probe"),
        "block 15's `probe chunk for [...]` must reach the wire. Before §119.n this \
         step dispatched with pix_ops=0, ask=\"\", output=\"\" and `axon check` said \
         0 errors. Slugs: {slugs:?}"
    );
    assert!(
        slugs.iter().any(|s| s == "validate"),
        "and its `validate ... against:` in `on_complete` must run when the stream \
         closes. Slugs: {slugs:?}"
    );
    assert!(
        c.let_bindings.contains_key("Stream"),
        "`step Analyze` reasons over `Stream.output`, so the stream step must bind \
         its name. Bindings: {:?}",
        c.let_bindings.keys().collect::<Vec<_>>()
    );
}

/// The §119.n position claim, stated negatively: the block must never again be
/// silently dropped. If a future refactor sends `stream` back to
/// `skip_flow_step_structural`, `IRStep.stream` goes `None` and the step runs as
/// a plain generation — green everywhere except here.
#[test]
fn s3b_the_block_is_never_silently_discarded() {
    let step = step_from_source(WITH_SOURCE);
    assert!(
        step.stream.is_some(),
        "REGRESSION: the `stream` block was discarded at parse time again. This is \
         the §111 silent-drop shape — the program still compiles with 0 errors and \
         the step does nothing."
    );
}
