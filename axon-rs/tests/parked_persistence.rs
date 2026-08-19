//! v2.36.0 — parked-continuation persistence across a reconnect.
//!
//! The parked one-shot continuation (v2.36.0 κ) rides the existing v2.3.0
//! `cognitive_state` sealed snapshot — no new state store. Two properties:
//! (1) an interrupted-but-unresumed dialogue seals, round-trips through the
//! snapshot wire form, resumes, and can STILL `resume` into its body; (2) a
//! non-interrupt snapshot carries no `parked` key — byte-identical to the
//! pre-v2.36.0 wire form (boot-hydrate self-heal / back-compat, no version bump).

use axon::session::{Payload, SessionType};
use axon::session_runtime::{SealedRuntime, SessionRuntime};

fn agent_one() -> SessionType {
    SessionType::Interrupt {
        signal: Payload::new("CallerSpeech"),
        body: Box::new(SessionType::send("Token", SessionType::End)),
        handler: Box::new(SessionType::send("Ack", SessionType::Resume)),
    }
}

#[test]
fn parked_continuation_survives_seal_resume() {
    let mut rt = SessionRuntime::new(agent_one(), None);
    rt.try_enter_interrupt().unwrap();
    rt.signal("CallerSpeech", 4).unwrap(); // interrupted: cursor at handler, κ parked

    // Seal mid-handler (client dropped while the agent was handling barge-in).
    let sealed = rt.seal().expect("an interrupted dialogue has a residual to seal");
    assert!(sealed.parked.is_some(), "the sealed snapshot carries the parked κ");

    // Round-trip through the snapshot wire form (the cognitive_state plaintext).
    let json = serde_json::to_string(&sealed).expect("serialize");
    let restored: SealedRuntime = serde_json::from_str(&json).expect("deserialize");

    // Reconnect: resume the runtime from the snapshot.
    let mut rt2 = SessionRuntime::resume(restored, &agent_one()).expect("resume from snapshot");

    // The handler can still finish and `resume` into the body — the barge-in is
    // recoverable across the reconnect.
    rt2.try_send("Ack").expect("handler resumes post-reconnect");
    rt2.try_resume().expect("the parked κ survived the reconnect");
    rt2.try_send("Token").expect("body resumes from its exact residual");
    assert!(rt2.is_complete());
}

#[test]
fn non_interrupt_snapshot_has_no_parked_key() {
    let mut rt = SessionRuntime::new(
        SessionType::send("M", SessionType::recv("N", SessionType::End)),
        None,
    );
    rt.try_send("M").unwrap();
    let sealed = rt.seal().expect("mid-protocol residual");
    assert!(sealed.parked.is_none());

    let json = serde_json::to_string(&sealed).expect("serialize");
    assert!(
        !json.contains("parked"),
        "no interrupt ⇒ no `parked` key in the snapshot wire form: {json}"
    );
}
