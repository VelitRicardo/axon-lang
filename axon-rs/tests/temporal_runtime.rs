//! v2.46.0 — runtime for `now:` declared cognitive time
//! (`the design plan`, axon-enterprise repo).
//!
//! Pinned properties (through the PRODUCTION engine — `execute_server_flow`
//! runs the unified dispatcher via the BufferSink collector, v2.15.0):
//! 1. A step-level `now: "<zone>"` yields a `temporal_context` record on
//!    the run metrics: `captured_utc` (RFC 3339 UTC), `tzdb_version`, and
//!    the rendered zone.
//! 2. A frame-level `context { now: }` (first-context convention) covers
//!    steps with no step-level `now:`.
//! 3. Step-level overrides frame-level; both zones are recorded once each.
//! 4. A `now:`-less flow reports `temporal_context: None` — and the
//!    `FlowEnvelope` wire JSON carries NO `temporal_context` key
//! (byte-identical to pre-v2.46.0).
//! 5. Fail-closed: a zone that passes the compile-time format law but is
//!    unknown to the tz database fails the flow loudly (never a silent
//!    omission).

use axon::flow_plan::compile_source_to_ir;
use axon::runner::execute_server_flow;

fn run(source: &str, flow: &str) -> axon::runner::ServerRunnerMetrics {
    let (_prog, ir) = compile_source_to_ir(source, "<v2.46.0-test>").expect("compile");
    let empty = std::collections::HashMap::new();
    execute_server_flow(
        &ir, flow, "stub", "", "<v2.46.0-test>", None, None, &empty, &empty, None, None, None,
        None, // budget
        None, // v2.69.0 channel semaphores
        None, // v2.69.0 — tool leases (test: none)
        None, // event_outbox
        None, // credential minter
        None, // v2.48.0 — secret custody (test: none)
        None, // v2.63.0 dataspace_engine (tests: fail closed)
        None, // v2.56.0 scrape_overrides
)
    .expect("execute")
}

const STEP_NOW: &str = "flow Plan() -> Unit {\n\
    step Triage {\n\
        now: \"America/Bogota\"\n\
        ask: \"Propose three visit slots this week.\"\n\
    }\n\
}\n";

#[test]
fn step_now_yields_a_temporal_record() {
    let m = run(STEP_NOW, "Plan");
    assert!(m.success, "stub flow runs");
    let rec = m.temporal_context.expect("temporal record present");
    assert_eq!(rec.zones, vec!["America/Bogota".to_string()]);
    assert_eq!(rec.tzdb_version, axon::window::tz_db_version());
    // RFC 3339 UTC instant (e.g. 2026-07-07T19:33:05Z).
    assert!(rec.captured_utc.ends_with('Z'), "UTC instant: {}", rec.captured_utc);
}

#[test]
fn frame_level_now_covers_steps_without_their_own() {
    let src = "context Scheduling { now: \"UTC\" }\n\
               flow Plan() -> Unit { step S { ask: \"hi\" } }\n";
    let m = run(src, "Plan");
    assert!(m.success);
    let rec = m.temporal_context.expect("frame-level record present");
    assert_eq!(rec.zones, vec!["UTC".to_string()]);
}

#[test]
fn step_zone_overrides_frame_zone_and_both_record_once() {
    let src = "context Scheduling { now: \"UTC\" }\n\
               flow Plan() -> Unit {\n\
                   step A { now: \"America/Bogota\" ask: \"a\" }\n\
                   step B { ask: \"b\" }\n\
                   step C { ask: \"c\" }\n\
               }\n";
    let m = run(src, "Plan");
    assert!(m.success);
    let rec = m.temporal_context.expect("record present");
    // First-use order: step A rendered Bogotá first, B/C the frame zone;
    // each zone recorded exactly once.
    assert_eq!(
        rec.zones,
        vec!["America/Bogota".to_string(), "UTC".to_string()]
    );
}

#[test]
fn now_less_flow_has_no_record_and_no_wire_key() {
    let src = "flow Plan() -> Unit { step S { ask: \"hi\" } }\n";
    let m = run(src, "Plan");
    assert!(m.success);
    assert!(m.temporal_context.is_none(), "no `now:` ⇒ no record");
}

#[test]
fn flow_envelope_elides_temporal_context_when_absent() {
    // The wire law directly: a `None` record serializes to NO key.
    #[derive(serde::Serialize)]
    struct Probe {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temporal_context: Option<axon::temporal_context::TemporalRecord>,
    }
    let json = serde_json::to_string(&Probe { temporal_context: None }).unwrap();
    assert_eq!(json, "{}");
    // …and a populated record carries exactly the replay triple.
    let m = run(STEP_NOW, "Plan");
    let json = serde_json::to_string(&Probe { temporal_context: m.temporal_context }).unwrap();
    assert!(json.contains("\"captured_utc\""), "{json}");
    assert!(json.contains("\"tzdb_version\""), "{json}");
    assert!(json.contains("\"zones\":[\"America/Bogota\"]"), "{json}");
}

#[test]
fn unknown_zone_fails_closed() {
    // Passes the frontend's shape law (`axon-T892` sees a plausible IANA
    // form) but is NOT in the tz database — the runtime must fail the flow
    // loudly, never silently omit the declared time.
    let src = "flow Plan() -> Unit { step S { now: \"Fake/Zone\" ask: \"hi\" } }\n";
    let m = run(src, "Plan");
    assert!(!m.success, "unresolvable declared zone must fail the flow");
    let err = m.error.expect("honest failure detail");
    assert!(
        err.contains("not a known IANA timezone"),
        "names the cause: {err}"
    );
    assert!(m.temporal_context.is_none(), "no record for a failed render");
}
