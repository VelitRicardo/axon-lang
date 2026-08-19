//! v2.36.0 (OSS core) — the unified `CallSoundnessCertificate` +
//! `ParkedResidualSoundness`.
//!
//! Interruption opens a data-at-rest surface (the parked κ persisted into the
//! v2.3.0 cognitive_state snapshot). The certificate composes, for one socket
//! bundle: interruptible-session soundness + the NEW parked-residual obligation
//! + the socket's resource bound — one deploy-time artifact whose overall
//! verdict is "can this call ever misbehave" (the design decision: composition, not mere
//! conjunction). The enterprise serving endpoint (v2.36.0 ENT) rides this core.

use axon::pcc::{
    check_call_soundness_certificate, check_proof, generate_call_soundness_certificate,
    generate_parked_residual_soundness_proofs, CheckOutcome, PropertyClass, Witness,
};

const VERSION: &str = "2.36.0-test";

/// A voice call: a dual interrupt session bound to a socket that declares
/// `reconnect: cognitive_state` (the park is AAD-bound + recoverable) and a
/// `legal_basis` (the at-rest retention is governed).
const SOUND_CALL: &str = r#"
session VoiceTurn {
    agent: [
        interrupt { send Token } on CallerSpeech as cause resumable { send Ack, resume }
    ]
    caller: [
        interrupt { receive Token } on CallerSpeech as cause resumable { receive Ack, resume }
    ]
}
socket VoiceCall {
    protocol: VoiceTurn
    backpressure: credit(8)
    reconnect: cognitive_state
    legal_basis: legitimate_interest
}
"#;

/// The same call, but the socket omits `reconnect` and `legal_basis` — it parks
/// a possibly-PII-bearing residual with no sealed store and no retention ceiling.
/// Type-checks (both fields are optional), but the certificate must NOT verify.
const UNGOVERNED_CALL: &str = r#"
session VoiceTurn {
    agent: [
        interrupt { send Token } on CallerSpeech as cause resumable { send Ack, resume }
    ]
    caller: [
        interrupt { receive Token } on CallerSpeech as cause resumable { receive Ack, resume }
    ]
}
socket VoiceCall {
    protocol: VoiceTurn
    backpressure: credit(8)
}
"#;

fn compile(src: &str) -> axon::ir_nodes::IRProgram {
    let (_p, ir) = axon::flow_plan::compile_source_to_ir(src, "<test>.axon").expect("compile");
    ir
}

#[test]
fn sound_call_certificate_verifies() {
    let ir = compile(SOUND_CALL);
    let cert = generate_call_soundness_certificate("VoiceCall", &ir, VERSION)
        .expect("the socket exists");
    assert_eq!(cert.socket_name, "VoiceCall");
    assert_eq!(cert.session_name, "VoiceTurn");

    // The composition carries the genuinely-new parked-residual member alongside
    // the interruptible-session + resource members.
    assert!(
        cert.proofs
            .iter()
            .any(|p| p.property == PropertyClass::ParkedResidualSoundness),
        "the certificate composes the NEW parked-residual obligation"
    );
    assert!(cert
        .proofs
        .iter()
        .any(|p| p.property == PropertyClass::InterruptibleSessionSoundness));

    assert_eq!(
        check_call_soundness_certificate(&cert, &ir),
        CheckOutcome::Verified,
        "a governed, dual, catalog-valid call is certified sound"
    );
}

#[test]
fn ungoverned_park_fails_the_certificate() {
    let ir = compile(UNGOVERNED_CALL);
    let cert = generate_call_soundness_certificate("VoiceCall", &ir, VERSION).unwrap();
    // The parked-residual member refutes → the whole certificate is not Verified.
    assert!(
        matches!(
            check_call_soundness_certificate(&cert, &ir),
            CheckOutcome::Refuted { .. }
        ),
        "a socket that parks a residual with no reconnect/legal_basis must fail the certificate"
    );
}

#[test]
fn parked_residual_witness_records_the_governance_facts() {
    let ir = compile(SOUND_CALL);
    let proofs = generate_parked_residual_soundness_proofs(&ir, VERSION);
    assert_eq!(proofs.len(), 1, "one socket carries an interruptible session");
    let Witness::ParkedResidualSoundness(w) = &proofs[0].witness else {
        panic!("expected a ParkedResidualSoundness witness");
    };
    assert_eq!(w.socket_name, "VoiceCall");
    assert!(w.session_has_interrupt);
    assert!(w.reconnect_cognitive_state);
    assert!(w.legal_basis_declared);
    assert_eq!(check_proof(&proofs[0], &ir), CheckOutcome::Verified);
}

/// ADVERSARIAL: a forged witness hiding the missing governance is caught
/// by re-derivation.
#[test]
fn forged_parked_residual_witness_rejected() {
    let ir = compile(UNGOVERNED_CALL);
    let mut proofs = generate_parked_residual_soundness_proofs(&ir, VERSION);
    if let Witness::ParkedResidualSoundness(ref mut w) = proofs[0].witness {
        w.reconnect_cognitive_state = true; // the lie
        w.legal_basis_declared = true; // the lie
    }
    match check_proof(&proofs[0], &ir) {
        CheckOutcome::Refuted { reason } => {
            assert!(reason.contains("disagrees with artifact"), "got: {reason}");
        }
        other => panic!("expected forged-witness Refuted, got {other:?}"),
    }
}

#[test]
fn slug_is_stable() {
    assert_eq!(
        PropertyClass::ParkedResidualSoundness.slug(),
        "parked_residual_soundness"
    );
}
