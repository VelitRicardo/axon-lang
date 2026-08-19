//! Operational protocol errors raised by the §Fase 41.d session-typed
//! WebSocket runtime — every variant is a runtime witness of a static
//! discipline that the connection must respect on every transition.
//!
//! When the static type checker (`axon-frontend`, §41.b/c) has already
//! validated the bound `session` + `socket { credit }`, a [`ProtocolError`]
//! at runtime can only fire because the **peer** sent a frame that diverges
//! from the conformant trace (or because a malformed frame entered the
//! decoder). The carrier (WebSocket) closes with code `1002 protocol error`
//! when one of these is observed; the error is recorded verbatim in the
//! close-reason payload so the peer can diagnose the divergence.

use std::fmt;

use axon_frontend::session::Payload;

/// Runtime protocol violation — the peer's next frame is inconsistent with
/// the session-type cursor or with the credit window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The cursor expected a `send`/`recv` of payload `expected` but the
    /// peer's frame announced payload `got`. The static type discipline is
    /// violated — at runtime this means the peer is not running the dual
    /// of our declared role.
    PayloadMismatch { expected: Payload, got: Payload },
    /// The cursor expected an operation of one kind (e.g. `recv`) but the
    /// peer's frame announced another (e.g. `select`). The connection state
    /// machine has no transition rule for the observed input.
    UnexpectedFrame {
        cursor_kind: &'static str,
        frame_kind: &'static str,
    },
    /// The cursor is at an internal/external choice and the peer's label is
    /// not in the type's arm set. Lists the declared labels so the peer can
    /// recover.
    UnknownLabel { label: String, expected: Vec<String> },
    /// A `send` was attempted at zero available credit — the §Fase 41.c
    /// "no rule at `n = 0`" axiom (paper §4.2) projected onto the runtime.
    /// Static analysis (`credit_analyse`) catches this at compile time
    /// when the declared protocol demands more than `k` sends in a burst;
    /// at runtime it is the dynamic-safety net for an off-spec peer.
    CreditExhausted { payload: Payload, budget: u64 },
    /// The cursor has reached `end` but the peer sent more data, or the
    /// peer requested an action while we already closed our half.
    AlreadyComplete { frame_kind: &'static str },
    /// The frame did not parse as a well-formed AXON session-typed
    /// envelope (malformed JSON, unknown `kind`, missing required field).
    /// Carries the raw payload for diagnostics.
    MalformedFrame(String),
    /// The transport (WebSocket) returned an I/O error or was closed
    /// abruptly mid-dialogue.
    Transport(String),
    /// §Fase 79.d — a `signal` fired but no interruptible region is armed
    /// (the cursor is not inside an `interrupt { … }` body).
    NoInterruptArmed,
    /// §Fase 79.d — the fired signal's cause does not match the armed
    /// region's declared `on <Signal>`.
    SignalMismatch { expected: Payload, got: Payload },
    /// §Fase 79.d — the fail-closed WCET watchdog (D79.5): the reaction path
    /// (signal → cancellation-acknowledged) took more transitions than the
    /// statically-declared bound. A breach never silently degrades — it trips
    /// this fault, which the carrier audits.
    WatchdogBreach { bound: u32, actual: u32 },
    /// §Fase 79.d — `resume` invoked on an already-consumed (or never
    /// captured) one-shot continuation — the linear-type violation of D79.1,
    /// caught at runtime even if it somehow slipped the static check.
    DoubleResume,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::PayloadMismatch { expected, got } => write!(
                f,
                "payload mismatch: cursor expected `{expected}`, peer sent `{got}` \
                 (the connection is not dual to the declared role)"
            ),
            ProtocolError::UnexpectedFrame { cursor_kind, frame_kind } => write!(
                f,
                "unexpected frame: cursor is at `{cursor_kind}`, peer sent `{frame_kind}` \
                 — the state machine has no transition for this input"
            ),
            ProtocolError::UnknownLabel { label, expected } => write!(
                f,
                "unknown choice label `{label}` — declared labels: {}",
                expected.join(", ")
            ),
            ProtocolError::CreditExhausted { payload, budget } => write!(
                f,
                "credit exhausted on `send {payload}` at window n = 0 \
                 (budget = {budget})"
            ),
            ProtocolError::AlreadyComplete { frame_kind } => write!(
                f,
                "dialogue already at `end`; peer sent `{frame_kind}` post-termination"
            ),
            ProtocolError::MalformedFrame(detail) => write!(f, "malformed frame: {detail}"),
            ProtocolError::Transport(detail) => write!(f, "transport error: {detail}"),
            ProtocolError::NoInterruptArmed => write!(
                f,
                "signal fired but no interruptible region is armed (cursor not inside \
                 an `interrupt` body)"
            ),
            ProtocolError::SignalMismatch { expected, got } => write!(
                f,
                "interrupt signal mismatch: region declares `on {expected}`, got `{got}`"
            ),
            ProtocolError::WatchdogBreach { bound, actual } => write!(
                f,
                "WCET watchdog breach: reaction path took {actual} transitions, declared \
                 bound is {bound} (fail-closed)"
            ),
            ProtocolError::DoubleResume => write!(
                f,
                "`resume` on an already-consumed one-shot continuation \
                 (linear-type violation)"
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl ProtocolError {
    /// A compact identifier suitable for the WebSocket close-frame reason
    /// payload (RFC 6455 §5.5.1 caps the reason at 123 bytes UTF-8 — keep
    /// these stable, short and machine-readable).
    pub fn code(&self) -> &'static str {
        match self {
            ProtocolError::PayloadMismatch { .. } => "payload_mismatch",
            ProtocolError::UnexpectedFrame { .. } => "unexpected_frame",
            ProtocolError::UnknownLabel { .. } => "unknown_label",
            ProtocolError::CreditExhausted { .. } => "credit_exhausted",
            ProtocolError::AlreadyComplete { .. } => "already_complete",
            ProtocolError::MalformedFrame(_) => "malformed_frame",
            ProtocolError::Transport(_) => "transport",
            ProtocolError::NoInterruptArmed => "no_interrupt_armed",
            ProtocolError::SignalMismatch { .. } => "signal_mismatch",
            ProtocolError::WatchdogBreach { .. } => "watchdog_breach",
            ProtocolError::DoubleResume => "double_resume",
        }
    }
}
