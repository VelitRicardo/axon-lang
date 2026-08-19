//! v2.36.0 — the interruptible-session runtime dispatch.
//!
//! Exercises the operational core the 79.a paper fixes, on the transport-
//! agnostic `SessionRuntime`:
//! - enter a region, fire a signal → the body's exact residual is captured as a
//!   one-shot continuation and control diverts to the handler;
//! - `resume` restores the body cursor, the **exact** pre-interrupt credit
//! window (Theorem 3), and the emit cursor;
//! - a second `resume` is a linear-type runtime error;
//! - the fail-closed WCET watchdog trips on a bound breach;
//! - the abandon exit terminates at `end`, releasing the continuation.

use axon::session::{Payload, SessionType};
use axon::session_runtime::{ProtocolError, SessionRuntime};

/// `Intr(CallerSpeech; !Token.end, !Ack.resume)` — an agent utterance of one
/// token, interruptible by caller speech; the handler acks then resumes.
fn agent_one() -> SessionType {
    SessionType::Interrupt {
        signal: Payload::new("CallerSpeech"),
        body: Box::new(SessionType::send("Token", SessionType::End)),
        handler: Box::new(SessionType::send("Ack", SessionType::Resume)),
    }
}

/// `Intr(CallerSpeech; !Token.!Token.end, !Ack.resume)` — a two-token body, so
/// we can interrupt mid-body (after one flushed token) and check the emit +
/// credit cursors restore exactly.
fn agent_two() -> SessionType {
    SessionType::Interrupt {
        signal: Payload::new("CallerSpeech"),
        body: Box::new(SessionType::send(
            "Token",
            SessionType::send("Token", SessionType::End),
        )),
        handler: Box::new(SessionType::send("Ack", SessionType::Resume)),
    }
}

#[test]
fn enter_signal_resume_round_trips_the_body() {
    let mut rt = SessionRuntime::new(agent_one(), None);
    rt.try_enter_interrupt().expect("enter the interruptible region");
    assert!(rt.interrupt_armed());

    // Barge-in before the token is flushed.
    rt.signal("CallerSpeech", 4).expect("fire the signal");
    assert!(rt.parked().is_some(), "the body residual is captured");

    // Handler: ack, then resume.
    rt.try_send("Ack").expect("handler flushes Ack");
    rt.try_resume().expect("resume back into the body");
    assert!(rt.parked().is_none(), "the one-shot continuation is consumed");

    // Back in the body: the deferred token now flushes, then end.
    rt.try_send("Token").expect("body resumes from its exact residual");
    assert!(rt.is_complete());
}

#[test]
fn resume_restores_the_exact_credit_window_theorem_3() {
    let mut rt = SessionRuntime::new(agent_two(), Some(4));
    rt.try_enter_interrupt().unwrap();
    rt.try_send("Token").unwrap(); // body flushes one token: window 4 → 3
    assert_eq!(rt.credit().unwrap().available, 3);

    rt.signal("CallerSpeech", 4).unwrap(); // parks window = 3
    rt.try_send("Ack").unwrap(); // handler debits the FORKED window: 3 → 2
    assert_eq!(rt.credit().unwrap().available, 2);

    rt.try_resume().unwrap();
    // Symmetry: the body window returns to EXACTLY its pre-interrupt value (3),
    // untouched by the handler's own send.
    assert_eq!(
        rt.credit().unwrap().available,
        3,
        "resume restores the pre-interrupt credit window exactly (Theorem 3)"
    );
}

#[test]
fn resume_restores_the_emit_cursor_d79_10() {
    let mut rt = SessionRuntime::new(agent_two(), None);
    rt.try_enter_interrupt().unwrap();
    rt.try_send("Token").unwrap(); // emit cursor 0 → 1
    assert_eq!(rt.emit_count(), 1);

    rt.signal("CallerSpeech", 4).unwrap(); // parks emit = 1
    rt.try_send("Ack").unwrap(); // handler flush: live emit 1 → 2
    assert_eq!(rt.emit_count(), 2);

    rt.try_resume().unwrap();
    assert_eq!(
        rt.emit_count(),
        1,
        "the body's stream re-opens at the exact flushed offset"
    );
}

#[test]
fn double_resume_is_a_linear_type_error() {
    // A `Resume` cursor with no captured continuation (the un-parked case).
    let mut rt = SessionRuntime::new(SessionType::Resume, None);
    assert!(
        matches!(rt.try_resume(), Err(ProtocolError::DoubleResume)),
        "resume with no parked continuation is a linear-type violation"
    );
}

#[test]
fn watchdog_trips_fail_closed_on_a_zero_bound() {
    let mut rt = SessionRuntime::new(agent_one(), None);
    rt.try_enter_interrupt().unwrap();
    match rt.signal("CallerSpeech", 0) {
        Err(ProtocolError::WatchdogBreach { bound: 0, actual: 1 }) => {}
        other => panic!("expected fail-closed WatchdogBreach, got {other:?}"),
    }
    assert!(
        rt.interrupt_armed(),
        "a watchdog fault never silently loses the armed region"
    );
    assert!(rt.parked().is_none(), "no continuation captured on a breach");
}

#[test]
fn abandon_exit_terminates_and_releases() {
    let mut rt = SessionRuntime::new(agent_one(), None);
    rt.try_enter_interrupt().unwrap();
    rt.signal("CallerSpeech", 4).unwrap();
    assert!(rt.parked().is_some());

    rt.abandon(); // TTL expiry → the abandon exit
    assert!(rt.is_complete(), "abandon drives the region to `end`");
    assert!(rt.parked().is_none(), "the one-shot continuation is released");
}

#[test]
fn signal_mismatch_leaves_the_region_armed() {
    let mut rt = SessionRuntime::new(agent_one(), None);
    rt.try_enter_interrupt().unwrap();
    match rt.signal("Dtmf", 4) {
        Err(ProtocolError::SignalMismatch { .. }) => {}
        other => panic!("expected SignalMismatch, got {other:?}"),
    }
    assert!(rt.interrupt_armed(), "a non-matching signal does not disarm the region");
    assert!(rt.parked().is_none());
}

#[test]
fn signal_without_arming_is_rejected() {
    let mut rt = SessionRuntime::new(agent_one(), None);
    // Cursor is at the Interrupt node but the region is not yet entered.
    assert!(matches!(
        rt.signal("CallerSpeech", 4),
        Err(ProtocolError::NoInterruptArmed)
    ));
}
