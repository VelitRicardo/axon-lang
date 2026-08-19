//! v2.36.0 — Proof-Carrying Code for interruptible sessions.
//!
//! The v2.36.0 type-checker's interrupt validation (closed CallInterruptCause
//! catalog, both arms present, two-exit handler) is now an INDEPENDENTLY
//! VERIFIABLE proof: `InterruptibleSessionSoundness`. The checker re-derives
//! every fact from the IR session steps — never trusting the compiler —
//! and a forged witness is rejected by recomputation.
//!
//! Properties pinned: (1) a well-formed dual interrupt session generates
//! per-region proofs the independent checker VERIFIES; (2) a forged witness is
//! Refuted; (3) digest binding holds; (4) the wire slug is stable.

use axon::pcc::{
    check_proof, generate_interruptible_session_soundness_proofs, CheckOutcome, PropertyClass,
    Witness,
};

const VERSION: &str = "2.36.0-test";

/// A barge-in turn typed as two **dual** roles: the agent may be interrupted
/// while sending tokens (handler acks then resumes); the caller is the dual
/// (interrupted while receiving). This type-checks clean under the v2.3.0
/// connection law extended with `Intr(sig; B, H)⊥ = Intr(sig; B⊥, H⊥)`.
const BARGE_IN: &str = r#"
session VoiceTurn {
    agent: [
        interrupt {
            send Token
        } on CallerSpeech as cause resumable {
            send Ack,
            resume
        }
    ]
    caller: [
        interrupt {
            receive Token
        } on CallerSpeech as cause resumable {
            receive Ack,
            resume
        }
    ]
}
"#;

fn compile(src: &str) -> axon::ir_nodes::IRProgram {
    let (_program, ir) = axon::flow_plan::compile_source_to_ir(src, "<test>.axon")
        .expect("interrupt session must compile clean (dual + catalog-valid)");
    ir
}

#[test]
fn dual_interrupt_session_generates_proofs_that_verify() {
    let ir = compile(BARGE_IN);
    let proofs = generate_interruptible_session_soundness_proofs(&ir, VERSION);
    // One interrupt region per role (agent + caller) => two proofs.
    assert_eq!(
        proofs.len(),
        2,
        "both roles declare an interrupt region => one proof each"
    );
    for p in &proofs {
        assert_eq!(
            check_proof(p, &ir),
            CheckOutcome::Verified,
            "a well-formed interrupt region must be certified sound"
        );
    }
}

#[test]
fn witness_records_the_soundness_facts() {
    let ir = compile(BARGE_IN);
    let proofs = generate_interruptible_session_soundness_proofs(&ir, VERSION);
    let Witness::InterruptibleSessionSoundness(w) = &proofs[0].witness else {
        panic!("expected an InterruptibleSessionSoundness witness");
    };
    assert_eq!(w.session_name, "VoiceTurn");
    assert_eq!(w.signal, "CallerSpeech");
    assert!(w.signal_in_catalog);
    assert!(w.has_body);
    assert!(w.has_handler);
    assert!(w.handler_reaches_exit);
}

/// ADVERSARIAL: a forged witness claiming the signal is in-catalog for
/// a cause that is not (or flipping any re-derived fact) is REJECTED — the
/// checker recomputes from the artifact and finds the witness lies.
#[test]
fn forged_witness_is_rejected() {
    let ir = compile(BARGE_IN);
    let mut proofs = generate_interruptible_session_soundness_proofs(&ir, VERSION);
    if let Witness::InterruptibleSessionSoundness(ref mut w) = proofs[0].witness {
        // The lie: claim a bogus signal is what was certified.
        w.signal = "TotallyBogusCause".to_string();
        w.signal_in_catalog = true;
    }
    match check_proof(&proofs[0], &ir) {
        CheckOutcome::Refuted { .. } => {}
        other => panic!("expected forged-witness Refuted, got {other:?}"),
    }
}

/// ADVERSARIAL: flipping `handler_reaches_exit` to hide a two-exit defect is
/// caught by the artifact re-derivation (the witness disagrees).
#[test]
fn forged_handler_exit_flag_is_rejected() {
    let ir = compile(BARGE_IN);
    let mut proofs = generate_interruptible_session_soundness_proofs(&ir, VERSION);
    if let Witness::InterruptibleSessionSoundness(ref mut w) = proofs[0].witness {
        w.handler_reaches_exit = !w.handler_reaches_exit;
    }
    match check_proof(&proofs[0], &ir) {
        CheckOutcome::Refuted { reason } => {
            assert!(reason.contains("disagrees with artifact"), "got: {reason}");
        }
        other => panic!("expected Refuted, got {other:?}"),
    }
}

/// the design decision — a proof minted for program A must not verify against program B.
#[test]
fn digest_mismatch_rejected() {
    let ir_a = compile(BARGE_IN);
    let proofs = generate_interruptible_session_soundness_proofs(&ir_a, VERSION);

    // Program B: a second, differently-named session changes the IR digest.
    let ir_b = compile(
        r#"
session VoiceTurn {
    agent: [
        interrupt {
            send Token
        } on CallerSpeech as cause resumable {
            send Ack,
            resume
        }
    ]
    caller: [
        interrupt {
            receive Token
        } on CallerSpeech as cause resumable {
            receive Ack,
            resume
        }
    ]
}
session Extra {
    a: [ send M, end ]
    b: [ receive M, end ]
}
"#,
    );
    assert_eq!(check_proof(&proofs[0], &ir_b), CheckOutcome::DigestMismatch);
}

#[test]
fn slug_is_stable() {
    assert_eq!(
        PropertyClass::InterruptibleSessionSoundness.slug(),
        "interruptible_session_soundness"
    );
}
