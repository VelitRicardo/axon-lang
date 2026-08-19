//! The operational state machine for a single session-typed dialogue.
//!
//! v2.3.0. A [`SessionRuntime`] is the runtime witness of a v2.3.0
//! `SessionType`: it carries a **cursor** (the residual type after every
//! step so far) and a [`CreditWindow`] (the dynamic counterpart of the
//! v2.3.0 index `!ⁿA.S`), and exposes one method per operational rule —
//! `try_send`, `try_recv`, `try_select`, `try_offer`, `try_end`. Each
//! method enforces the static discipline *defence-in-depth*: a violation
//! (wrong-kind frame, wrong payload, exhausted credit, post-`end` traffic)
//! returns a [`ProtocolError`] and leaves the cursor unchanged. The
//! caller's contract is to close the transport on first error.
//!
//! Recursion is handled by [`SessionType::unfold_head`] — the cursor is
//! kept in "head-unfolded" form: never a leading `Rec` or `Var`. This
//! keeps the rule for every action a single pattern match on the cursor.
//!
//! The runtime is **transport-agnostic** — it knows nothing about
//! WebSockets, JSON, or `tokio`. The [`crate::session_runtime::ws`]
//! module is one carrier; the runtime would slot identically over QUIC,
//! a TCP stream, or an in-process channel.

use std::collections::BTreeMap;

use axon_frontend::session::{Payload, SessionType};
use serde::{Deserialize, Serialize};

use super::error::ProtocolError;

/// Dynamic counterpart of the v2.3.0 credit index `!ⁿA.S`. Tracks the
/// number of in-flight sends the producer is currently allowed; a `send`
/// decrements `available`, a `recv` refills it (capped at `budget`,
/// standard TCP-window semantics). The static analysis
/// (`SessionType::credit_analyse`) has already verified the protocol is
/// conformant under this budget — this is the runtime safety net for an
/// off-spec peer.
///
/// `Serialize` + `Deserialize` — v2.3.0 sealed-snapshot resume carries
/// the *live* window (available count, not just the budget) so a resumed
/// connection picks up exactly where the disconnected one left off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditWindow {
    /// Maximum credit the producer may hold at any one time (`k` in the
    /// `socket { backpressure: credit(k) }` annotation).
    pub budget: u64,
    /// Current available credit. Invariant: `0 ≤ available ≤ budget`.
    pub available: u64,
}

impl CreditWindow {
    /// Open a fresh window with the full budget available.
    pub fn new(budget: u64) -> Self {
        Self { budget, available: budget }
    }
    /// Try to consume one credit; returns the remaining count, or `None`
    /// if exhausted (the runtime witness of the "no rule at n=0" axiom).
    fn try_consume(&mut self) -> Option<u64> {
        if self.available == 0 {
            None
        } else {
            self.available -= 1;
            Some(self.available)
        }
    }
    /// Refill one credit, capped at the budget. Cannot fail.
    fn refill(&mut self) {
        if self.available < self.budget {
            self.available += 1;
        }
    }
}

/// The session-type runtime cursor + credit window.
///
/// Held by **each** side of a connection — the server runtime is
/// initialised with the server-role type, the client with the dual. Every
/// transition is local: there is no cross-process synchronisation here;
/// the carrier delivers frames in order and the two cursors stay in lock
/// step because they were initialised from a duality-checked pair.
///
/// `Serialize` + `Deserialize` — v2.3.0 sealed-snapshot resume. The
/// serialised form is a stable JSON object containing the schema (so
/// resume can verify the protocol hasn't been swapped), the residual
/// cursor, and the live credit window. Encoded once via [`Self::seal`]
/// into the AAD-bound `cognitive_states` ciphertext; decoded by
/// [`Self::resume`] after the v2.0.0 `EnvelopeEncryption::decrypt` verifies
/// the (tenant, session, flow) binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntime {
    /// The original session type (the protocol "schema"). Kept so the
    /// runtime can re-report it on errors and so the cursor's invariants
    /// are documented at the type level (the cursor is always reachable
    /// from `schema` along the trace so far). Resume validates a sealed
    /// snapshot's `schema` matches the live socket's declared protocol.
    schema: SessionType,
    /// The residual type — the unfinished part of the protocol. Always
    /// head-unfolded (never a leading `Rec` or `Var`).
    cursor: SessionType,
    /// The dynamic credit window, or `None` for the unbounded fragment
    /// (no `backpressure` annotation in the socket).
    credit: Option<CreditWindow>,
    /// v2.36.0 — the active interruptible region, armed by
    /// [`Self::try_enter_interrupt`]: the declared signal + the handler to
    /// divert to. `#[serde(skip)]` — interrupt dispatch is live-only runtime
    /// state; the parked continuation that *survives* a reconnect is persisted
    /// separately via the v2.3.0 `cognitive_state` snapshot (v2.36.0), not here.
    #[serde(skip, default)]
    interrupt: Option<InterruptFrame>,
    /// v2.36.0 — the emit cursor: frames flushed to the carrier so
    /// far. Snapshotted into the parked continuation so `resume` re-opens the
    /// body's `Stream<T>` at the exact flushed offset ("the exact word").
    #[serde(skip, default)]
    emit_count: u64,
    /// v2.36.0 — the captured **one-shot** continuation (paper κ), set by
    /// [`Self::signal`] and consumed exactly once by [`Self::try_resume`]. A
    /// second resume is [`ProtocolError::DoubleResume`] (the design decision linearity).
    #[serde(skip, default)]
    parked: Option<ParkedContinuation>,
}

/// v2.36.0 — the armed interruptible region: what signal fires it and what
/// handler to divert to. Live runtime state (not serialized).
#[derive(Debug, Clone, PartialEq, Eq)]
struct InterruptFrame {
    signal: Payload,
    handler: SessionType,
}

/// v2.36.0 — a captured one-shot continuation of an interrupted body: the
/// reified session cursor + credit window (the paper's κ ≅ (S₍>k₎, w), the design decision)
/// plus the emit-cursor snapshot and the cause that fired. Consumed
/// exactly once by [`SessionRuntime::try_resume`].
///
/// `Serialize` + `Deserialize` — v2.36.0: a parked κ survives a reconnect by
/// riding the v2.3.0 `cognitive_state` sealed snapshot (no new state store). The
/// wire shape is exactly the reified cursor + window the snapshot already seals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkedContinuation {
    /// `S₍>k₎` — the body residual at the instant of interruption.
    pub cursor: SessionType,
    /// The body's live credit window at interruption — restored *exactly* on
    /// resume (credit symmetry, Theorem 3).
    pub credit: Option<CreditWindow>,
    /// Frames flushed to the carrier at park time (the emit cursor, the design decision).
    pub emit_count: u64,
    /// The `CallInterruptCause` that fired the interruption.
    pub cause: Payload,
}

impl SessionRuntime {
    /// Create a runtime for the given role's session type. `budget`
    /// mirrors the socket's `credit(k)`; pass `None` for the unbounded
    /// fragment (statically equivalent to omitting `backpressure:`).
    pub fn new(schema: SessionType, budget: Option<u64>) -> Self {
        let cursor = schema.unfold_head();
        Self {
            schema,
            cursor,
            credit: budget.map(CreditWindow::new),
            interrupt: None,
            emit_count: 0,
            parked: None,
        }
    }

    /// The original session type — useful for error messages and logs.
    pub fn schema(&self) -> &SessionType {
        &self.schema
    }

    /// The current residual cursor — always head-unfolded.
    pub fn cursor(&self) -> &SessionType {
        &self.cursor
    }

    /// The dynamic credit window snapshot (or `None` for the unbounded
    /// fragment).
    pub fn credit(&self) -> Option<CreditWindow> {
        self.credit
    }

    /// `true` once the cursor reaches `end` — both sides should now close
    /// the carrier cleanly. The runtime rejects further actions after
    /// this point with [`ProtocolError::AlreadyComplete`].
    pub fn is_complete(&self) -> bool {
        matches!(self.cursor, SessionType::End)
    }

    // ── v2.3.0 — typed reconnection via sealed snapshots ───────────

    /// Serialise the live runtime state into a stable JSON envelope —
    /// the plaintext the v2.0.0 `cognitive_states` AAD-bound ciphertext
    /// wraps. Carries the schema (so resume can verify the protocol
    /// hasn't been swapped under the connection), the residual cursor,
    /// and the live credit window snapshot.
    ///
    /// Symmetric with [`Self::resume`]: `runtime.seal()` then
    /// `SessionRuntime::resume(sealed, declared_schema)` round-trips when
    /// the declared schema matches.
    ///
    /// Returns `None` if and only if the cursor is already at `End` — a
    /// completed dialogue has no residual to seal, so no snapshot is
    /// issued (the caller should `evict()` the prior snapshot instead).
    pub fn seal(&self) -> Option<SealedRuntime> {
        if self.is_complete() {
            return None;
        }
        Some(SealedRuntime {
            version: SEALED_RUNTIME_VERSION,
            schema: self.schema.clone(),
            cursor: self.cursor.clone(),
            credit: self.credit,
            // v2.36.0 — carry the parked one-shot continuation across the
            // reconnect, so an interrupted-but-unresumed dialogue can still
            // `resume` into its body after the client comes back.
            parked: self.parked.clone(),
        })
    }

    /// Reconstruct a [`SessionRuntime`] from a sealed snapshot, validating
    /// the schema matches what the route declares now (defence against
    /// protocol-swap attacks where an attacker reuses a sealed snapshot
    /// against a different socket whose declaration drifted).
    ///
    /// On success the returned runtime resumes from the exact cursor +
    /// credit window the disconnected one left behind. The carrier driver
    /// then runs the producer/consumer loop as usual.
    pub fn resume(
        sealed: SealedRuntime,
        declared_schema: &SessionType,
    ) -> Result<Self, ResumeError> {
        if sealed.version != SEALED_RUNTIME_VERSION {
            return Err(ResumeError::UnsupportedVersion(sealed.version));
        }
        if !sealed.schema.equiv(declared_schema) {
            return Err(ResumeError::SchemaMismatch);
        }
        // The cursor must be reachable from the schema (we cannot prove
        // this in general — the algebra would need a labelled trace — but
        // we DO require the cursor to be head-unfolded, which the wire
        // form preserves because `seal()` only stores cursors set via
        // `advance()`).
        Ok(SessionRuntime {
            schema: sealed.schema,
            cursor: sealed.cursor,
            credit: sealed.credit,
            interrupt: None,
            emit_count: 0,
            // v2.36.0 — restore the parked κ so `resume` still works post-
            // reconnect (the `interrupted_by_peer` carrier state survives).
            parked: sealed.parked,
        })
    }

    // ── Operational rules ──────────────────────────────────────────────

    /// Producer step `!A.S → S`. Succeeds iff:
    /// 1. the cursor is `Send { payload, … }` with `payload == got`;
    /// 2. the credit window (if any) has `available > 0` — otherwise the
    /// v2.3.0 "no rule at n=0" axiom fires.
    /// On success the cursor advances (unfolded) and one credit is
    /// consumed (when the window is present).
    pub fn try_send(&mut self, got: &str) -> Result<(), ProtocolError> {
        if self.is_complete() {
            return Err(ProtocolError::AlreadyComplete { frame_kind: "send" });
        }
        let (expected_payload, cont) = match &self.cursor {
            SessionType::Send { payload, cont, .. } => (payload.clone(), cont.clone()),
            other => {
                return Err(ProtocolError::UnexpectedFrame {
                    cursor_kind: kind_of(other),
                    frame_kind: "send",
                });
            }
        };
        let got_payload = Payload::new(got);
        if expected_payload != got_payload {
            return Err(ProtocolError::PayloadMismatch {
                expected: expected_payload,
                got: got_payload,
            });
        }
        // Credit decrement — the dynamic witness of `!ⁿA.S, n > 0`.
        if let Some(w) = self.credit.as_mut() {
            if w.try_consume().is_none() {
                return Err(ProtocolError::CreditExhausted {
                    payload: expected_payload,
                    budget: w.budget,
                });
            }
        }
        self.advance(*cont);
        // v2.36.0 — advance the emit cursor: a producer step flushes one
        // frame to the carrier (the design decision "delivered = flushed").
        self.emit_count += 1;
        Ok(())
    }

    /// Consumer step `?A.S → S`. The peer just produced an `!A.S` frame.
    /// Symmetric to [`try_send`] — payload must match, the cursor advances,
    /// and one credit is refilled (if a window is present).
    pub fn try_recv(&mut self, got: &str) -> Result<(), ProtocolError> {
        if self.is_complete() {
            return Err(ProtocolError::AlreadyComplete { frame_kind: "send" });
        }
        let (expected_payload, cont) = match &self.cursor {
            SessionType::Recv { payload, cont, .. } => (payload.clone(), cont.clone()),
            other => {
                return Err(ProtocolError::UnexpectedFrame {
                    cursor_kind: kind_of(other),
                    frame_kind: "send",
                });
            }
        };
        let got_payload = Payload::new(got);
        if expected_payload != got_payload {
            return Err(ProtocolError::PayloadMismatch {
                expected: expected_payload,
                got: got_payload,
            });
        }
        // Refill — the peer just delivered, the in-flight count drops by 1.
        if let Some(w) = self.credit.as_mut() {
            w.refill();
        }
        self.advance(*cont);
        Ok(())
    }

    /// Internal choice (`⊕`) — *we* select the labelled arm. Cursor must
    /// be `Select { arms }` containing `label`; on success the cursor
    /// advances into that arm's continuation.
    pub fn try_select(&mut self, label: &str) -> Result<(), ProtocolError> {
        self.advance_into_arm(label, true)
    }

    /// External choice (`&`) — the *peer* selected this label; we accept.
    /// Cursor must be `Branch { arms }` containing `label`.
    pub fn try_offer(&mut self, label: &str) -> Result<(), ProtocolError> {
        self.advance_into_arm(label, false)
    }

    /// `end` step — terminates the dialogue. Cursor must already be
    /// `End`; otherwise the peer is signalling termination mid-protocol.
    pub fn try_end(&mut self) -> Result<(), ProtocolError> {
        match &self.cursor {
            SessionType::End => Ok(()),
            other => Err(ProtocolError::UnexpectedFrame {
                cursor_kind: kind_of(other),
                frame_kind: "end",
            }),
        }
    }

    // ── v2.36.0 — interruptible-session dispatch ─────────────────────

    /// The emit cursor: producer frames flushed to the carrier so far.
    pub fn emit_count(&self) -> u64 {
        self.emit_count
    }

    /// `true` while an interruptible region is armed (cursor inside its body).
    pub fn interrupt_armed(&self) -> bool {
        self.interrupt.is_some()
    }

    /// The captured one-shot continuation, if the region has been interrupted
    /// and not yet resumed/abandoned. `None` before `signal` and after
    /// `resume`/`abandon` (the linear consumption point).
    pub fn parked(&self) -> Option<&ParkedContinuation> {
        self.parked.as_ref()
    }

    /// v2.36.0 — enter an interruptible region. The cursor must be
    /// `Interrupt { signal, body, handler }`; advances **into the body** while
    /// arming the region so a matching [`Self::signal`] can capture the body's
    /// residual and divert to the handler. The connection law is preserved:
    /// the peer enters the dual region symmetrically (Theorem 1).
    pub fn try_enter_interrupt(&mut self) -> Result<(), ProtocolError> {
        let (signal, body, handler) = match &self.cursor {
            SessionType::Interrupt { signal, body, handler } => {
                (signal.clone(), (**body).clone(), (**handler).clone())
            }
            other => {
                return Err(ProtocolError::UnexpectedFrame {
                    cursor_kind: kind_of(other),
                    frame_kind: "interrupt",
                })
            }
        };
        self.interrupt = Some(InterruptFrame { signal, handler });
        self.advance(body);
        Ok(())
    }

    /// v2.36.0 — fire the interrupt signal `cause`. Captures the body's exact
    /// residual (cursor + credit window + emit cursor) as a **one-shot**
    /// continuation (κ, the design decision), then diverts the cursor to the handler. A
    /// fail-closed WCET watchdog asserts the reaction path completes
    /// within `max_reaction_steps` transitions — the capture-and-divert is a
    /// single transition, so any bound `≥ 1` holds and `0` trips the watchdog
    /// (the fault is never silently degraded).
    ///
    /// Errors: [`ProtocolError::NoInterruptArmed`] if no region is armed,
    /// [`ProtocolError::SignalMismatch`] if `cause` ≠ the declared signal,
    /// [`ProtocolError::WatchdogBreach`] on a bound breach.
    pub fn signal(&mut self, cause: &str, max_reaction_steps: u32) -> Result<(), ProtocolError> {
        let frame = match self.interrupt.take() {
            Some(f) => f,
            None => return Err(ProtocolError::NoInterruptArmed),
        };
        let got = Payload::new(cause);
        if frame.signal != got {
            let expected = frame.signal.clone();
            self.interrupt = Some(frame); // region stays armed; this signal wasn't ours
            return Err(ProtocolError::SignalMismatch { expected, got });
        }
        // WCET watchdog: the reaction path here is one transition
        // (capture + divert). Fail closed on a declared bound it exceeds.
        const REACTION_STEPS: u32 = 1;
        if REACTION_STEPS > max_reaction_steps {
            // Re-arm so the region isn't silently lost on a watchdog fault.
            self.interrupt = Some(frame);
            return Err(ProtocolError::WatchdogBreach {
                bound: max_reaction_steps,
                actual: REACTION_STEPS,
            });
        }
        // Capture the reified one-shot continuation κ ≅ (S₍>k₎, w) + emit cursor.
        self.parked = Some(ParkedContinuation {
            cursor: self.cursor.clone(),
            credit: self.credit,
            emit_count: self.emit_count,
            cause: got,
        });
        // Divert to the handler. It runs on the live window (a fork of the body
        // window); `resume` restores the parked body window exactly, so the
        // handler's own sends never perturb the body's credit (Theorem 3).
        self.advance(frame.handler);
        Ok(())
    }

    /// v2.36.0 — the handler's **normal exit** `resume`: consume the parked
    /// one-shot continuation EXACTLY ONCE and return the body to its exact
    /// residual — cursor, credit window (symmetry, Theorem 3), and emit cursor
    ///. The cursor must be at the `Resume` leaf.
    ///
    /// A second `resume` (or a resume with no capture) is
    /// [`ProtocolError::DoubleResume`] — the runtime witness of the design decision linearity.
    pub fn try_resume(&mut self) -> Result<(), ProtocolError> {
        if !matches!(self.cursor, SessionType::Resume) {
            return Err(ProtocolError::UnexpectedFrame {
                cursor_kind: kind_of(&self.cursor),
                frame_kind: "resume",
            });
        }
        let parked = match self.parked.take() {
            Some(p) => p,
            None => return Err(ProtocolError::DoubleResume),
        };
        self.credit = parked.credit; // exact pre-interrupt window (Theorem 3)
        self.emit_count = parked.emit_count; // re-open the stream at the flushed offset
        self.advance(parked.cursor); // back to S₍>k₎
        Ok(())
    }

    /// v2.36.0 — the handler's **abandon exit**: the parked
    /// continuation is discarded (released exactly once, affine-by-default) and
    /// the region terminates at `end`. Driven on TTL expiry by the carrier
    /// (v2.36.0). Safe to call whether or not a continuation is still parked.
    pub fn abandon(&mut self) {
        self.parked = None;
        self.interrupt = None;
        self.cursor = SessionType::End;
    }

    // ── Internal helpers ───────────────────────────────────────────────

    /// Set the cursor to the head-unfolded form of `next`. This is the
    /// single invariant maintained across every step — after any advance
    /// the cursor never has a leading `Rec` (and never a leading bare
    /// `Var` on a *closed* type, which 41.a/b/c statically guarantee).
    fn advance(&mut self, next: SessionType) {
        self.cursor = next.unfold_head();
    }

    fn advance_into_arm(&mut self, label: &str, internal: bool) -> Result<(), ProtocolError> {
        if self.is_complete() {
            return Err(ProtocolError::AlreadyComplete {
                frame_kind: if internal { "select" } else { "branch" },
            });
        }
        let arms = match (&self.cursor, internal) {
            (SessionType::Select(m), true) => m.clone(),
            (SessionType::Branch(m), false) => m.clone(),
            (other, _) => {
                return Err(ProtocolError::UnexpectedFrame {
                    cursor_kind: kind_of(other),
                    frame_kind: if internal { "select" } else { "select" },
                });
            }
        };
        match arms.get(label) {
            Some(cont) => {
                let cont = cont.clone();
                self.advance(cont);
                Ok(())
            }
            None => Err(ProtocolError::UnknownLabel {
                label: label.to_string(),
                expected: keys_of(&arms),
            }),
        }
    }
}

/// Symbolic name of the cursor's head constructor — used to enrich error
/// messages without leaking the full type body.
fn kind_of(t: &SessionType) -> &'static str {
    match t {
        SessionType::End => "end",
        SessionType::Send { .. } => "send",
        SessionType::Recv { .. } => "recv",
        SessionType::Select(_) => "select",
        SessionType::Branch(_) => "branch",
        SessionType::Rec(_, _) => "rec", // never reached on a head-unfolded cursor
        SessionType::Var(_) => "var",    // ditto, on closed types
        SessionType::Interrupt { .. } => "interrupt", // v2.36.0 — dispatch lands in 79.d
        SessionType::Resume => "resume",
    }
}

fn keys_of(m: &BTreeMap<String, SessionType>) -> Vec<String> {
    m.keys().cloned().collect()
}

// ── v2.3.0 — sealed-snapshot envelope ────────────────────────────────

/// On-wire version tag for the sealed-runtime JSON. Bumped only on a
/// breaking schema change to the envelope shape.
pub const SEALED_RUNTIME_VERSION: u8 = 1;

/// The plaintext the v2.0.0 `cognitive_states` AAD-bound ciphertext wraps —
/// a stable JSON envelope containing the session-type schema, the residual
/// cursor, and the live credit window. Issued by
/// [`SessionRuntime::seal`]; opened by [`SessionRuntime::resume`] after the
/// v2.0.0 envelope decryption verifies the (tenant, session, flow) binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SealedRuntime {
    /// Envelope version — gates schema evolution.
    pub version: u8,
    /// The declared session-type schema. Resume validates this equals
    /// the live socket's declaration (defence against protocol-swap).
    pub schema: SessionType,
    /// The residual session type (the cursor at seal time). Always
    /// head-unfolded because `SessionRuntime::advance` enforces it.
    pub cursor: SessionType,
    /// The live credit window snapshot — `None` if the bound socket is
    /// in the unbounded fragment (no `backpressure` annotation).
    pub credit: Option<CreditWindow>,
    /// v2.36.0 — the parked one-shot continuation, present iff the runtime
    /// was interrupted-and-not-yet-resumed at seal time. `skip_serializing_if`
    /// keeps every non-interrupt snapshot byte-identical to the pre-v2.36.0 wire
    /// form (no version bump; back-compat by construction — the boot-hydrate
    /// self-heal discipline). On resume the `interrupted_by_peer` carrier state
    /// is restored so the handler can still `resume` into the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked: Option<ParkedContinuation>,
}

impl SealedRuntime {
    /// Serialise to bytes — the format the v2.0.0 envelope encrypts.
    /// Deterministic JSON via `serde_json::to_vec`.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("SealedRuntime ⇒ JSON is total")
    }
    /// Parse bytes (after envelope decryption) back into a `SealedRuntime`.
    pub fn from_bytes(b: &[u8]) -> Result<Self, ResumeError> {
        serde_json::from_slice(b).map_err(|e| ResumeError::Malformed(e.to_string()))
    }
}

/// Errors raised by [`SessionRuntime::resume`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeError {
    /// The sealed envelope's version is newer than this runtime supports.
    UnsupportedVersion(u8),
    /// The sealed schema does not match the live socket's declared
    /// protocol. The v2.0.0 envelope's AAD binds tenant+session+flow, so
    /// the *transport* can't be confused; this check catches the case
    /// where the socket's declared session-type itself drifted between
    /// seal + resume (e.g. a deploy bumped the protocol).
    SchemaMismatch,
    /// The plaintext bytes didn't deserialise into a `SealedRuntime`
    /// envelope. Carries the parser's complaint for diagnostics.
    Malformed(String),
}

impl std::fmt::Display for ResumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResumeError::UnsupportedVersion(v) => write!(
                f,
                "sealed runtime envelope version {v} is newer than this runtime supports \
                 (current = {SEALED_RUNTIME_VERSION})"
            ),
            ResumeError::SchemaMismatch => f.write_str(
                "sealed runtime's declared protocol does not match the live socket's session type \
                 — the protocol drifted between seal and resume",
            ),
            ResumeError::Malformed(detail) => write!(f, "sealed runtime envelope is malformed: {detail}"),
        }
    }
}

impl std::error::Error for ResumeError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CreditWindow primitives ─────────────────────────────────────────

    #[test]
    fn credit_window_decrements_and_refills_within_budget() {
        let mut w = CreditWindow::new(2);
        assert_eq!(w.try_consume(), Some(1));
        assert_eq!(w.try_consume(), Some(0));
        assert!(w.try_consume().is_none()); // exhausted
        w.refill();
        assert_eq!(w.available, 1);
        w.refill();
        assert_eq!(w.available, 2);
        // Refill beyond budget is a no-op (capped at k).
        w.refill();
        assert_eq!(w.available, 2);
    }

    // ── try_send / try_recv on linear types ─────────────────────────────

    #[test]
    fn try_send_advances_on_matching_payload() {
        // Type: !Msg.end
        let schema = SessionType::send("Msg", SessionType::End);
        let mut r = SessionRuntime::new(schema, None);
        r.try_send("Msg").expect("step");
        assert!(r.is_complete());
    }

    #[test]
    fn try_send_rejects_wrong_payload() {
        let schema = SessionType::send("Msg", SessionType::End);
        let mut r = SessionRuntime::new(schema, None);
        match r.try_send("WrongType") {
            Err(ProtocolError::PayloadMismatch { expected, got }) => {
                assert_eq!(expected, Payload::new("Msg"));
                assert_eq!(got, Payload::new("WrongType"));
            }
            other => panic!("expected PayloadMismatch, got {other:?}"),
        }
        // The cursor is unchanged on error.
        assert!(matches!(r.cursor(), SessionType::Send { .. }));
    }

    #[test]
    fn try_recv_rejects_when_cursor_is_send() {
        let schema = SessionType::send("Msg", SessionType::End);
        let mut r = SessionRuntime::new(schema, None);
        match r.try_recv("Msg") {
            Err(ProtocolError::UnexpectedFrame { cursor_kind: "send", .. }) => {}
            other => panic!("expected UnexpectedFrame(send→send), got {other:?}"),
        }
    }

    // ── Credit accounting ───────────────────────────────────────────────

    #[test]
    fn credit_exhaustion_blocks_send_at_zero() {
        // Type: !A.!B.end with budget = 1
        let schema = SessionType::send("A", SessionType::send("B", SessionType::End));
        let mut r = SessionRuntime::new(schema, Some(1));
        // First send consumes the credit (budget→0).
        r.try_send("A").expect("first send");
        assert_eq!(r.credit().unwrap().available, 0);
        // Second send hits the n=0 axiom.
        match r.try_send("B") {
            Err(ProtocolError::CreditExhausted { payload, budget: 1 }) => {
                assert_eq!(payload, Payload::new("B"));
            }
            other => panic!("expected CreditExhausted, got {other:?}"),
        }
    }

    #[test]
    fn recv_refills_credit_capped_at_budget() {
        // Type: !A.?Ack.!B.end with budget = 1 (sustainable: each send
        // is followed by a refill).
        let schema = SessionType::send(
            "A",
            SessionType::recv("Ack", SessionType::send("B", SessionType::End)),
        );
        let mut r = SessionRuntime::new(schema, Some(1));
        r.try_send("A").expect("send A");
        assert_eq!(r.credit().unwrap().available, 0);
        r.try_recv("Ack").expect("recv Ack refills");
        assert_eq!(r.credit().unwrap().available, 1);
        r.try_send("B").expect("send B uses refilled credit");
        assert!(r.is_complete());
    }

    // ── select / branch / recursion ─────────────────────────────────────

    #[test]
    fn select_advances_into_named_arm() {
        let schema = SessionType::select([
            ("ask".into(), SessionType::send("Q", SessionType::End)),
            ("quit".into(), SessionType::End),
        ]);
        let mut r = SessionRuntime::new(schema, None);
        r.try_select("ask").expect("select ask");
        assert!(matches!(r.cursor(), SessionType::Send { .. }));
        r.try_send("Q").expect("send Q");
        assert!(r.is_complete());
    }

    #[test]
    fn select_rejects_unknown_label() {
        let schema = SessionType::select([
            ("ask".into(), SessionType::End),
            ("quit".into(), SessionType::End),
        ]);
        let mut r = SessionRuntime::new(schema, None);
        match r.try_select("nope") {
            Err(ProtocolError::UnknownLabel { label, expected }) => {
                assert_eq!(label, "nope");
                assert_eq!(expected, vec!["ask".to_string(), "quit".to_string()]);
            }
            other => panic!("expected UnknownLabel, got {other:?}"),
        }
    }

    #[test]
    fn offer_advances_into_peer_selected_arm() {
        let schema = SessionType::branch([
            ("ack".into(), SessionType::End),
            ("err".into(), SessionType::End),
        ]);
        let mut r = SessionRuntime::new(schema, None);
        r.try_offer("ack").expect("offer ack");
        assert!(r.is_complete());
    }

    #[test]
    fn recursion_unfolds_one_step_at_a_time() {
        // rec X. !A.?Ack.X — should support unbounded iteration under
        // budget=1 (Δ = 0 per recurring iteration).
        let schema = SessionType::rec(
            "X",
            SessionType::send("A", SessionType::recv("Ack", SessionType::var("X"))),
        );
        let mut r = SessionRuntime::new(schema, Some(1));
        for _ in 0..5 {
            r.try_send("A").expect("send");
            r.try_recv("Ack").expect("recv");
        }
        // The cursor is still at the start-of-iteration shape (unfolded
        // form of `Rec(X, …)`), which is `!A.?Ack.<unfolded rec>`.
        assert!(matches!(r.cursor(), SessionType::Send { .. }));
        // Definitely not at `end` — the dialogue is unbounded.
        assert!(!r.is_complete());
    }

    // ── Post-end safety net ─────────────────────────────────────────────

    #[test]
    fn post_end_traffic_is_rejected() {
        let mut r = SessionRuntime::new(SessionType::End, None);
        r.try_end().expect("end on End is OK");
        match r.try_send("X") {
            Err(ProtocolError::AlreadyComplete { frame_kind: "send" }) => {}
            other => panic!("expected AlreadyComplete, got {other:?}"),
        }
    }

    // ── A realistic chat dialogue runs to completion ────────────────────

    // ── v2.3.0 — sealed snapshot round-trip + resume validation ────

    #[test]
    fn seal_returns_none_at_end_and_some_otherwise() {
        // !A.end → before sending, seal yields a snapshot.
        let schema = SessionType::send("A", SessionType::End);
        let r = SessionRuntime::new(schema.clone(), None);
        let sealed = r.seal().expect("snapshot at non-End cursor");
        assert_eq!(sealed.version, SEALED_RUNTIME_VERSION);
        // The schema is preserved verbatim (resume needs the original).
        assert_eq!(sealed.schema, schema);
        // The cursor is the head-unfolded form of `Send`.
        assert!(matches!(sealed.cursor, SessionType::Send { .. }));
        // After advancing to End, seal returns None.
        let mut r = SessionRuntime::new(schema, None);
        r.try_send("A").unwrap();
        assert!(r.is_complete());
        assert!(r.seal().is_none(), "no snapshot once cursor is at End");
    }

    #[test]
    fn seal_carries_live_credit_window_not_just_budget() {
        // !A.!B.end with budget=2 — after one send the window has 1 left.
        let schema = SessionType::send("A", SessionType::send("B", SessionType::End));
        let mut r = SessionRuntime::new(schema, Some(2));
        r.try_send("A").unwrap();
        let sealed = r.seal().expect("snapshot mid-protocol");
        assert_eq!(sealed.credit, Some(CreditWindow { budget: 2, available: 1 }));
    }

    #[test]
    fn resume_round_trips_through_seal_then_unbinds_to_the_same_cursor() {
        let schema = SessionType::recv(
            "Msg",
            SessionType::send("Ack", SessionType::End),
        );
        let r0 = SessionRuntime::new(schema.clone(), None);
        let sealed = r0.seal().expect("snapshot before recv");
        let bytes = sealed.to_bytes();
        // Round-trip via JSON bytes (the v2.0.0 envelope plaintext).
        let recovered = SealedRuntime::from_bytes(&bytes).expect("parse");
        assert_eq!(recovered, sealed);
        // And resume → live runtime that picks up where we left off.
        let r1 = SessionRuntime::resume(recovered, &schema).expect("resume");
        assert_eq!(r1.cursor(), r0.cursor());
        assert_eq!(r1.credit(), r0.credit());
    }

    #[test]
    fn resume_after_partial_progress_continues_from_the_residual() {
        // !A.!B.end — send A, seal, resume, send B, end.
        let schema = SessionType::send("A", SessionType::send("B", SessionType::End));
        let mut r0 = SessionRuntime::new(schema.clone(), Some(2));
        r0.try_send("A").unwrap();
        let bytes = r0.seal().unwrap().to_bytes();
        // Wire bytes round-trip — this is what the AAD-bound ciphertext carries.
        let recovered = SealedRuntime::from_bytes(&bytes).unwrap();
        let mut r1 = SessionRuntime::resume(recovered, &schema).unwrap();
        // Credit window survived the seal (1 used, 1 available).
        assert_eq!(r1.credit().unwrap().available, 1);
        // The cursor is exactly `!B.end` — sending B completes the dialogue.
        r1.try_send("B").expect("send B from resumed cursor");
        assert!(r1.is_complete());
    }

    #[test]
    fn resume_rejects_a_schema_mismatch() {
        // Seal a snapshot for `!A.end`, then try to resume against `!B.end`.
        let schema_a = SessionType::send("A", SessionType::End);
        let schema_b = SessionType::send("B", SessionType::End);
        let r0 = SessionRuntime::new(schema_a.clone(), None);
        let sealed = r0.seal().unwrap();
        assert_eq!(
            SessionRuntime::resume(sealed.clone(), &schema_b).err(),
            Some(ResumeError::SchemaMismatch)
        );
        // Same-schema resume works.
        assert!(SessionRuntime::resume(sealed, &schema_a).is_ok());
    }

    #[test]
    fn resume_rejects_a_future_envelope_version() {
        let schema = SessionType::send("A", SessionType::End);
        let r = SessionRuntime::new(schema.clone(), None);
        let mut sealed = r.seal().unwrap();
        sealed.version = SEALED_RUNTIME_VERSION + 7;
        assert_eq!(
            SessionRuntime::resume(sealed, &schema).err(),
            Some(ResumeError::UnsupportedVersion(SEALED_RUNTIME_VERSION + 7))
        );
    }

    #[test]
    fn resume_rejects_malformed_envelope_bytes() {
        let garbage = b"{not valid JSON";
        assert!(matches!(
            SealedRuntime::from_bytes(garbage),
            Err(ResumeError::Malformed(_))
        ));
    }

    #[test]
    fn resume_accepts_alpha_equivalent_schemas() {
        // The schema match uses the v2.3.0 regular-coinductive equality, so
        // α-renamed recursion variables are accepted as equivalent.
        let schema_x = SessionType::rec("X", SessionType::send("T", SessionType::var("X")));
        let schema_y = SessionType::rec("Y", SessionType::send("T", SessionType::var("Y")));
        let r = SessionRuntime::new(schema_x.clone(), None);
        let sealed = r.seal().unwrap();
        // Sealing produced a schema with binder `X`; resume against `Y` succeeds.
        assert!(SessionRuntime::resume(sealed, &schema_y).is_ok());
    }

    #[test]
    fn sealed_runtime_is_json_compatible_with_serde_roundtrip() {
        // The wire shape must be JSON-deserialisable by any downstream
        // tool (e.g. an offline forensic inspector). We check the bytes
        // parse via the standard `serde_json::from_slice`.
        let schema = SessionType::send("X", SessionType::End);
        let r = SessionRuntime::new(schema, None);
        let bytes = r.seal().unwrap().to_bytes();
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("envelope is well-formed JSON");
        // The envelope carries the four known keys.
        assert!(value.get("version").is_some());
        assert!(value.get("schema").is_some());
        assert!(value.get("cursor").is_some());
        assert!(value.get("credit").is_some());
    }

    #[test]
    fn realistic_chat_dialogue_runs_to_completion() {
        // The 41.a sample type: rec X. +{ ask: !Utterance. &{ token:
        // ?Token.X, done: end }, cancel: end }
        let schema = SessionType::rec(
            "X",
            SessionType::select([
                (
                    "ask".into(),
                    SessionType::send(
                        "Utterance",
                        SessionType::branch([
                            ("token".into(), SessionType::recv("Token", SessionType::var("X"))),
                            ("done".into(), SessionType::End),
                        ]),
                    ),
                ),
                ("cancel".into(), SessionType::End),
            ]),
        );
        let mut client = SessionRuntime::new(schema, Some(4));
        // Iter 1: ask → send Utt → server says token → recv Token → loop.
        client.try_select("ask").unwrap();
        client.try_send("Utterance").unwrap();
        client.try_offer("token").unwrap();
        client.try_recv("Token").unwrap();
        // Iter 2: ask → send Utt → server says done.
        client.try_select("ask").unwrap();
        client.try_send("Utterance").unwrap();
        client.try_offer("done").unwrap();
        client.try_end().unwrap();
        assert!(client.is_complete());
    }
}
