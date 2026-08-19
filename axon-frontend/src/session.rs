//! Session types — the algebra of typed bidirectional dialogue.
//!
//! §Fase 41.a — *WebSocket as a Cognitive Primitive*. This module is the pure
//! mathematical core of the paper (`docs/paper_websocket_cognitive_primitive.md`):
//! the session-type grammar (§3.1), the **duality** involution `(·)⊥` (§3.2),
//! the **regular-coinductive equality** for recursive (`μ`) types, and the
//! **connection law** — a connection with endpoints typed `S` and `T` is
//! well-formed iff `T ≡ S⊥`. Grounded in Caires & Pfenning's Curry–Howard
//! correspondence (session types ARE intuitionistic linear-logic propositions),
//! it is the static guarantee RFC 6455 lacks: *what one end sends, the other
//! expects* — making a dialogue **deadlock-free and protocol-conformant by
//! construction**, not by per-message runtime validation.
//!
//! This is the pure algebra only: no parser/AST (Fase 41.b), no runtime (41.d),
//! no multiparty projection (41.h). The payload carried by `send`/`recv` is an
//! opaque [`Payload`] (a canonical type name); 41.b binds it to the real AST
//! value types — the duality + equality algebra here depends only on payload
//! *equality*, never on payload structure, so it is decoupled by construction.
//!
//! §Fase 41.c — **credit-refined backpressure** (D2 of the plan vivo, §4.2 of
//! the paper). `Send` / `Recv` now carry an optional credit index `n: u64`
//! (`!ⁿA.S` / `?ⁿA.S`); `None` is the unbounded fragment (`!∞A.S`, the algebra
//! before 41.c). The "send at n = 0 has no typing rule" axiom is implemented by
//! [`SessionType::has_send_at_zero`] (an explicit `!⁰A.S` in the type is
//! unprovable) and by the **Presburger-decidable** flow analysis
//! [`SessionType::credit_analyse`], which — given a socket budget `k` — checks
//! that every send fires at an available credit `> 0` (no rule at n=0) and that
//! every recursive body is **sustainable** (per-iteration net send count
//! `Δ = #send − #recv ≤ 0`, the loop-fixpoint inequality). All constraints are
//! linear over the naturals → decidable in the theory of Presburger arithmetic.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The value type carried by a `send`/`recv`. Opaque at this layer (a canonical
/// type name); Fase 41.b replaces it with the real AST value type. Duality and
/// equality treat it nominally — only `Payload == Payload` matters.
///
/// `#[serde(transparent)]` — the JSON encoding is the bare type-name string
/// (the wire shape the §Fase 41.g sealed-snapshot serialiser depends on);
/// `Payload("Msg")` ↔ `"Msg"` on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Payload(pub String);

impl Payload {
    pub fn new(name: impl Into<String>) -> Self {
        Payload(name.into())
    }
}

impl fmt::Display for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A session type — the protocol of one endpoint of a connection (§3.1 of the
/// paper). `Select`/`Branch` carry their labelled continuations in a `BTreeMap`
/// so the label set is canonically ordered (deterministic duality + equality).
///
/// `Serialize` + `Deserialize` — §Fase 41.g sealed-snapshot resume needs the
/// residual cursor + the protocol schema serialisable. The encoding is
/// stable across the algebra layer + the enterprise persistence layer: the
/// same JSON shape goes into the AAD-bound `cognitive_states` ciphertext
/// and comes back out via [`SessionRuntime::resume`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionType {
    /// `end` — the dialogue is complete.
    End,
    /// `!ⁿA.S` — send a value of type `A`, then behave as `S`. The optional
    /// `credit` is the Fase 41.c index `n` (paper §4.2): `Some(n)` types a send
    /// that *requires* `n > 0` available credit (the "no rule at n = 0" axiom
    /// makes `Some(0)` unprovable); `None` is the unbounded fragment `!∞A.S`.
    Send {
        payload: Payload,
        credit: Option<u64>,
        cont: Box<SessionType>,
    },
    /// `?ⁿA.S` — receive a value of type `A`, then behave as `S`. Symmetric to
    /// [`SessionType::Send`]: the index `n` bounds the receiver-side window.
    Recv {
        payload: Payload,
        credit: Option<u64>,
        cont: Box<SessionType>,
    },
    /// `⊕{ℓᵢ:Sᵢ}` — internal choice: this endpoint *selects* a label.
    Select(BTreeMap<String, SessionType>),
    /// `&{ℓᵢ:Sᵢ}` — external choice: this endpoint *offers* the branches.
    Branch(BTreeMap<String, SessionType>),
    /// `μX.S` — recursive session (equirecursive: `μX.S ≡ S[μX.S/X]`).
    Rec(String, Box<SessionType>),
    /// `X` — a recursion variable (bound by an enclosing `Rec`).
    Var(String),
    /// §Fase 79 — `Intr(sig; B, H)` — an **interruptible region** (paper
    /// *Interruptible Sessions* §3.3). `body` (`B`) is the interruptible
    /// protocol; on the `signal` (a closed `CallInterruptCause` cause) the
    /// `handler` (`H`) runs. `H` is a **two-exit** construct: its normal exit is
    /// [`SessionType::Resume`] (control returns to `B`'s residual), its
    /// abandon exit is `End`. Duality is `Intr(sig; B, H)⊥ = Intr(sig; B⊥, H⊥)`
    /// (Theorem 1: the connection law is preserved across both exits).
    Interrupt {
        signal: Payload,
        body: Box<SessionType>,
        handler: Box<SessionType>,
    },
    /// §Fase 79 — the interrupt handler's **normal exit**: hand control back to
    /// the parked body's residual (`resume`). A self-dual leaf, like `End`
    /// (resumption is symmetric — the peer receives control back). Only
    /// well-formed inside an [`SessionType::Interrupt`] handler (checked at
    /// §79.c); the abandon exit is the ordinary `End`.
    Resume,
}

impl SessionType {
    // ── Smart constructors (ergonomic + keep call sites readable) ──────────

    /// `!A.S` — unbounded send (`credit = None`, the pre-41.c fragment).
    pub fn send(payload: impl Into<String>, then: SessionType) -> Self {
        SessionType::Send {
            payload: Payload::new(payload),
            credit: None,
            cont: Box::new(then),
        }
    }
    /// `?A.S` — unbounded receive (`credit = None`).
    pub fn recv(payload: impl Into<String>, then: SessionType) -> Self {
        SessionType::Recv {
            payload: Payload::new(payload),
            credit: None,
            cont: Box::new(then),
        }
    }
    /// `!ⁿA.S` — credit-refined send (Fase 41.c, paper §4.2). The continuation
    /// `then` runs in the same window — the budget is global to the socket; the
    /// `n` here is the *snapshot* of available credit demanded at this step.
    pub fn send_credit(payload: impl Into<String>, n: u64, then: SessionType) -> Self {
        SessionType::Send {
            payload: Payload::new(payload),
            credit: Some(n),
            cont: Box::new(then),
        }
    }
    /// `?ⁿA.S` — credit-refined receive (Fase 41.c).
    pub fn recv_credit(payload: impl Into<String>, n: u64, then: SessionType) -> Self {
        SessionType::Recv {
            payload: Payload::new(payload),
            credit: Some(n),
            cont: Box::new(then),
        }
    }
    pub fn select(branches: impl IntoIterator<Item = (String, SessionType)>) -> Self {
        SessionType::Select(branches.into_iter().collect())
    }
    pub fn branch(branches: impl IntoIterator<Item = (String, SessionType)>) -> Self {
        SessionType::Branch(branches.into_iter().collect())
    }
    pub fn rec(var: impl Into<String>, body: SessionType) -> Self {
        SessionType::Rec(var.into(), Box::new(body))
    }
    pub fn var(name: impl Into<String>) -> Self {
        SessionType::Var(name.into())
    }

    // ── Duality (§3.2): the involution that swaps the two sides ────────────

    /// The dual `S⊥`: swaps `send`↔`recv` and `select`↔`branch`, recursing into
    /// continuations; `end`, `Rec` binders and `Var`s are preserved. Payloads
    /// **and** the credit index `n` are unchanged — `(!ⁿA.S)⊥ = ?ⁿA.S⊥` (same
    /// `A`, same `n`, opposite direction). Symmetric credit is the standard
    /// credit-flow semantics (Rast lineage): the sender's window-of-n is
    /// exactly what the receiver-side is sized to absorb.
    pub fn dual(&self) -> SessionType {
        match self {
            SessionType::End => SessionType::End,
            SessionType::Send { payload, credit, cont } => SessionType::Recv {
                payload: payload.clone(),
                credit: *credit,
                cont: Box::new(cont.dual()),
            },
            SessionType::Recv { payload, credit, cont } => SessionType::Send {
                payload: payload.clone(),
                credit: *credit,
                cont: Box::new(cont.dual()),
            },
            SessionType::Select(m) => SessionType::Branch(dual_map(m)),
            SessionType::Branch(m) => SessionType::Select(dual_map(m)),
            SessionType::Rec(x, b) => SessionType::Rec(x.clone(), Box::new(b.dual())),
            SessionType::Var(x) => SessionType::Var(x.clone()),
            // §Fase 79 — Intr(sig; B, H)⊥ = Intr(sig; B⊥, H⊥); Resume self-dual
            // (Theorem 1: connection law preserved across both handler exits).
            SessionType::Interrupt { signal, body, handler } => SessionType::Interrupt {
                signal: signal.clone(),
                body: Box::new(body.dual()),
                handler: Box::new(handler.dual()),
            },
            SessionType::Resume => SessionType::Resume,
        }
    }

    // ── Equirecursive unfolding + capture-stopping substitution ────────────

    /// Substitute the free variable `var` by `repl`. Stops at a shadowing
    /// `Rec(var, …)` (the inner binder re-captures the name).
    fn subst(&self, var: &str, repl: &SessionType) -> SessionType {
        match self {
            SessionType::End => SessionType::End,
            SessionType::Send { payload, credit, cont } => SessionType::Send {
                payload: payload.clone(),
                credit: *credit,
                cont: Box::new(cont.subst(var, repl)),
            },
            SessionType::Recv { payload, credit, cont } => SessionType::Recv {
                payload: payload.clone(),
                credit: *credit,
                cont: Box::new(cont.subst(var, repl)),
            },
            SessionType::Select(m) => SessionType::Select(subst_map(m, var, repl)),
            SessionType::Branch(m) => SessionType::Branch(subst_map(m, var, repl)),
            SessionType::Rec(x, b) => {
                if x == var {
                    self.clone() // shadowed — leave the inner Rec untouched
                } else {
                    SessionType::Rec(x.clone(), Box::new(b.subst(var, repl)))
                }
            }
            SessionType::Var(x) => {
                if x == var {
                    repl.clone()
                } else {
                    self.clone()
                }
            }
            // §Fase 79 — substitution descends into both interrupt sub-protocols;
            // Resume is a leaf (no free vars of its own).
            SessionType::Interrupt { signal, body, handler } => SessionType::Interrupt {
                signal: signal.clone(),
                body: Box::new(body.subst(var, repl)),
                handler: Box::new(handler.subst(var, repl)),
            },
            SessionType::Resume => SessionType::Resume,
        }
    }

    /// Unfold every *leading* `Rec` so the head constructor is exposed:
    /// `μX.S ↦ S[μX.S/X]`, repeated. Terminates for **contractive** types
    /// (a guard appears under each `Rec` before the variable recurs).
    ///
    /// Public so the 41.d runtime can drive the session-type cursor over a
    /// live connection: after every operational step the continuation is
    /// re-unfolded so the cursor never carries a leading `Rec` for the
    /// state machine to interpret.
    pub fn unfold_head(&self) -> SessionType {
        let mut t = self.clone();
        while let SessionType::Rec(x, b) = t {
            let whole = SessionType::Rec(x.clone(), b.clone());
            t = b.subst(&x, &whole);
        }
        t
    }

    // ── Regular-coinductive equality ──────────────────────────────────────

    /// Equirecursive equality: `S ≡ T` iff their infinite unfoldings coincide.
    /// Decided by the standard coinductive algorithm — assume the pair equal,
    /// unfold leading `Rec`s, compare heads, recurse; a re-encountered pair is
    /// discharged by the assumption (the greatest fixed point). Terminates
    /// because a regular type has finitely many distinct sub-pairs.
    pub fn equiv(&self, other: &SessionType) -> bool {
        let mut assumed: Vec<(SessionType, SessionType)> = Vec::new();
        equiv_inner(self, other, &mut assumed)
    }

    /// The **connection law** (§3.2): a connection whose two endpoints are typed
    /// `self` and `peer` is well-formed iff `peer ≡ self⊥`. Symmetric up to
    /// involutivity (`(S⊥)⊥ ≡ S`).
    pub fn is_dual_to(&self, peer: &SessionType) -> bool {
        peer.equiv(&self.dual())
    }

    // ── Fase 41.c — credit-refined backpressure (D2, paper §4.2) ────────────

    /// Stamp every (recursively-reachable) `Send` and `Recv` with the credit
    /// index `n`. Idempotent on already-stamped types. Used by the type
    /// checker to lift the socket's `backpressure: credit(k)` annotation onto
    /// the bare session protocol so the algebra-level analysis can discharge
    /// the constraint.
    pub fn with_credit(&self, n: u64) -> SessionType {
        match self {
            SessionType::End => SessionType::End,
            SessionType::Send { payload, cont, .. } => SessionType::Send {
                payload: payload.clone(),
                credit: Some(n),
                cont: Box::new(cont.with_credit(n)),
            },
            SessionType::Recv { payload, cont, .. } => SessionType::Recv {
                payload: payload.clone(),
                credit: Some(n),
                cont: Box::new(cont.with_credit(n)),
            },
            SessionType::Select(m) => SessionType::Select(
                m.iter().map(|(l, s)| (l.clone(), s.with_credit(n))).collect(),
            ),
            SessionType::Branch(m) => SessionType::Branch(
                m.iter().map(|(l, s)| (l.clone(), s.with_credit(n))).collect(),
            ),
            SessionType::Rec(x, b) => SessionType::Rec(x.clone(), Box::new(b.with_credit(n))),
            SessionType::Var(x) => SessionType::Var(x.clone()),
            // §Fase 79 — stamp both sub-protocols; the handler's disjoint-budget
            // symmetry (Thm 3) is enforced structurally by the checker, not here.
            SessionType::Interrupt { signal, body, handler } => SessionType::Interrupt {
                signal: signal.clone(),
                body: Box::new(body.with_credit(n)),
                handler: Box::new(handler.with_credit(n)),
            },
            SessionType::Resume => SessionType::Resume,
        }
    }

    /// The "no rule at n = 0" axiom (paper §4.2): an explicit `!⁰A.S` in the
    /// type is **unprovable** — there is no typing rule for a send at zero
    /// available credit. Returns the offending payload of the first such send
    /// (in a deterministic left-to-right walk) if any.
    ///
    /// Decidable in linear time over the type structure.
    pub fn has_send_at_zero(&self) -> Option<Payload> {
        match self {
            SessionType::End => None,
            SessionType::Send { payload, credit: Some(0), .. } => Some(payload.clone()),
            SessionType::Send { cont, .. } | SessionType::Recv { cont, .. } => cont.has_send_at_zero(),
            SessionType::Select(m) | SessionType::Branch(m) => {
                m.values().find_map(|s| s.has_send_at_zero())
            }
            SessionType::Rec(_, b) => b.has_send_at_zero(),
            SessionType::Var(_) => None,
            // §Fase 79 — an explicit `!⁰A.S` anywhere in either sub-protocol.
            SessionType::Interrupt { body, handler, .. } => {
                body.has_send_at_zero().or_else(|| handler.has_send_at_zero())
            }
            SessionType::Resume => None,
        }
    }

    /// Decide the **credit conformance** of `self` against a budget `k`
    /// (the socket's `backpressure: credit(k)` window). This is the
    /// Presburger discharge — the constraints are linear arithmetic over the
    /// naturals, so satisfiability is decidable; the algorithm here is the
    /// direct fixpoint formulation specialised to closed, contractive session
    /// types (Rast lineage, §4.2 of the paper).
    ///
    /// The check fires three kinds of error:
    ///
    /// 1. **Send at zero** — an explicit `!⁰A.S` in the type. Unprovable by
    ///    construction (no typing rule applies).
    /// 2. **Burst overflow** — a straight-line send burst exceeding the
    ///    available window. With initial budget `k`, the abstract trace must
    ///    never reach `available_credit < 0` at a send.
    /// 3. **Loop unsustainability** — a recursive body whose per-iteration net
    ///    send count `Δ = #send − #recv` is strictly positive: each iteration
    ///    drains the window, so unbounded iteration is unsound under *any*
    ///    finite budget. (`Δ ≤ 0` is the Presburger fixpoint inequality.)
    ///
    /// Returns `Ok(())` if the protocol is conformant, or [`CreditError`] with
    /// the offending witness. Total over closed, contractive session types.
    pub fn credit_analyse(&self, budget: u64) -> Result<(), CreditError> {
        if let Some(p) = self.has_send_at_zero() {
            return Err(CreditError::SendAtZero { payload: p });
        }
        // Initial window = full budget. The walker tracks the minimum
        // available credit reachable along any execution path; if at any send
        // it would fall below 0 → BurstOverflow. Recursive bodies are
        // discharged by the Δ ≤ 0 fixpoint inequality.
        let _final = credit_walk(self, budget as i64, budget as i64)?;
        Ok(())
    }

    /// Enumerate the **recurring paths** of `self` w.r.t. recursion variable
    /// `x` — every trace from the root that reaches `Var(x)`. Each path is
    /// reported as `(#send, #recv)`; terminating paths (reaching `End` or a
    /// different free variable) are dropped (they don't iterate, so they
    /// don't constrain unbounded sustainability). Shadowing `Rec(x, …)` cuts
    /// the descent — references inside refer to the inner binder.
    ///
    /// Total in time linear in the size of `self`; the path count is bounded
    /// by the number of leaves of the choice tree.
    pub fn recurring_paths(&self, x: &str) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        recurring_paths_into(self, x, 0, 0, &mut out);
        out
    }

    /// Worst-case (maximum-Δ) recurring path of `self` w.r.t. `x`. Used by
    /// the type checker to report the offending iteration count. Returns
    /// `(0, 0)` if there are no recurring paths.
    pub fn credit_delta(&self, x: &str) -> (u64, u64) {
        self.recurring_paths(x)
            .into_iter()
            .max_by_key(|(s, r)| *s as i64 - *r as i64)
            .unwrap_or((0, 0))
    }

    // ── Fase 41.e — SSE-as-fragment unification (D3, paper §4.4) ─────────

    /// True iff `self` lies in the **SSE producer fragment**: the
    /// connection only sends to its peer. Concretely the type contains
    /// only `End`, `Send`, internal-`Select`, `Rec`, and `Var` — no
    /// `Recv` (would mean the producer expects client input) and no
    /// `Branch` (would mean the producer offers a choice the client
    /// picks). For such a type the §4.4 identity `S_SSE = Π↓(S_WS)`
    /// holds with `Π↓ = id`: the protocol *is already* the SSE fragment,
    /// runnable over W3C SSE without WebSocket bidirectionality.
    ///
    /// Total over closed, contractive session types; linear in the size
    /// of `self`.
    pub fn projects_to_sse(&self) -> bool {
        self.has_polarity(Polarity::Producer)
    }

    /// Dual of [`projects_to_sse`] — the **SSE consumer fragment**: the
    /// connection only receives from its peer (`End`, `Recv`,
    /// external-`Branch`, `Rec`, `Var`). The §4.4 theorem
    /// `Π↓(S)⊥ = Π↑(S⊥)` ties this to `projects_to_sse` via duality:
    /// `S.projects_to_sse() ⇔ S.dual().projects_to_sse_consumer()`.
    pub fn projects_to_sse_consumer(&self) -> bool {
        self.has_polarity(Polarity::Consumer)
    }

    /// Unified polarity test. The two SSE fragments are exactly the two
    /// inhabitants of [`Polarity`]: `Producer = !/⊕/end/μ/var-only` and
    /// `Consumer = ?/&/end/μ/var-only`.
    pub fn has_polarity(&self, p: Polarity) -> bool {
        match (self, p) {
            (SessionType::End, _) => true,
            (SessionType::Var(_), _) => true,
            (SessionType::Send { cont, .. }, Polarity::Producer) => cont.has_polarity(p),
            (SessionType::Recv { cont, .. }, Polarity::Consumer) => cont.has_polarity(p),
            (SessionType::Select(arms), Polarity::Producer) => {
                arms.values().all(|s| s.has_polarity(p))
            }
            (SessionType::Branch(arms), Polarity::Consumer) => {
                arms.values().all(|s| s.has_polarity(p))
            }
            (SessionType::Rec(_, body), _) => body.has_polarity(p),
            // Wrong-polarity head: `Send` in Consumer fragment, `Recv` in
            // Producer fragment, `Branch` in Producer fragment, `Select`
            // in Consumer fragment. Each one immediately disqualifies the
            // type from the single-polarity SSE projection.
            _ => false,
        }
    }
}

/// Which side of an SSE-projectable connection a session type describes
/// — the **producer** (server-side, only sends/selects) or the
/// **consumer** (client-side, only receives/branches). Used by
/// [`SessionType::has_polarity`] to discharge the §4.4 SSE-fragment
/// predicate `Π↓(S_WS) = S_SSE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    /// Server-side: only outbound actions (`Send`, `Select`).
    Producer,
    /// Client-side: only inbound actions (`Recv`, `Branch`).
    Consumer,
}

impl Polarity {
    /// The dual of this polarity — used to express the connection-law
    /// preservation: an SSE-projectable session has a Producer role and
    /// a Consumer role, related by duality.
    pub fn flip(self) -> Self {
        match self {
            Polarity::Producer => Polarity::Consumer,
            Polarity::Consumer => Polarity::Producer,
        }
    }
}

/// The Presburger discharge's negative verdict — the witness of an
/// unconformant credit constraint. Surfaced verbatim by the type checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreditError {
    /// An explicit `!⁰A.S` — the "no rule at n=0" axiom rejects it.
    SendAtZero { payload: Payload },
    /// A straight-line send burst exceeds the budget `k`: at the offending
    /// send the abstract credit window would fall below 0.
    BurstOverflow { payload: Payload, budget: u64, burst: u64 },
    /// A recursive body has Δ > 0 (per iteration drains the window): no finite
    /// budget makes unbounded iteration sound.
    LoopUnsustainable { sends_per_iter: u64, recvs_per_iter: u64 },
}

impl fmt::Display for CreditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreditError::SendAtZero { payload } => {
                write!(f, "send `{payload}` at credit n=0 has no typing rule")
            }
            CreditError::BurstOverflow { payload, budget, burst } => write!(
                f,
                "credit-window overflow at send `{payload}`: the protocol requires a \
                 send-burst of {burst} but the socket's `credit({budget})` cannot absorb it"
            ),
            CreditError::LoopUnsustainable { sends_per_iter, recvs_per_iter } => write!(
                f,
                "recursive body is unsustainable: Δ = {sends_per_iter} - {recvs_per_iter} > 0 \
                 (no finite credit window keeps unbounded iteration in flight)"
            ),
        }
    }
}

/// Abstract-interpretation walker for the credit constraint. `available` is the
/// current window snapshot; `budget` is the maximum (recv refills are capped at
/// budget, the standard credit-flow semantics). Returns the available credit
/// at the end of the executed branch (the *minimum* across choice arms so the
/// caller sees the worst-case continuation).
fn credit_walk(t: &SessionType, available: i64, budget: i64) -> Result<i64, CreditError> {
    match t {
        SessionType::End => Ok(available),
        SessionType::Send { payload, cont, .. } => {
            let next = available - 1;
            if next < 0 {
                return Err(CreditError::BurstOverflow {
                    payload: payload.clone(),
                    budget: budget as u64,
                    burst: (budget - available + 1) as u64,
                });
            }
            credit_walk(cont, next, budget)
        }
        SessionType::Recv { cont, .. } => {
            // A recv refills one credit, capped at the budget (TCP-window
            // semantics: the receiver never accumulates more than `k`).
            let next = (available + 1).min(budget);
            credit_walk(cont, next, budget)
        }
        SessionType::Select(m) | SessionType::Branch(m) => {
            // Each arm must be conformant on its own; the conservative
            // post-state is the minimum (worst case) across arms.
            let mut worst = available;
            for arm in m.values() {
                let post = credit_walk(arm, available, budget)?;
                if post < worst {
                    worst = post;
                }
            }
            Ok(worst)
        }
        SessionType::Rec(x, body) => {
            // Loop sustainability (Presburger fixpoint): for every recurring
            // path back to `Var(x)`, the per-iteration net send count must
            // satisfy `Δ = #send − #recv ≤ 0`. A non-recurring arm (one that
            // terminates in `end`) is exempt — it executes at most once.
            // If *any* recurring path has Δ > 0, the window strictly drains
            // on that iteration and no finite `k` is sufficient → reject.
            for (s, r) in body.recurring_paths(x) {
                if s > r {
                    return Err(CreditError::LoopUnsustainable {
                        sends_per_iter: s,
                        recvs_per_iter: r,
                    });
                }
            }
            // Walk one iteration so a burst inside the body is surfaced even
            // when the loop is sustainable on net (Δ ≤ 0 doesn't bound peak).
            credit_walk(body, available, budget)
        }
        SessionType::Var(_) => {
            // Recursion re-entry: nothing further to walk on this iteration;
            // the fixpoint check above already vetted sustainability.
            Ok(available)
        }
        // §Fase 79 — the body runs on the socket window; the handler runs on a
        // separate declared budget (Thm 3 / D79.11b) and does not debit this
        // window. On `resume` the window returns to its pre-interrupt state, so
        // the region's post-state is exactly the body's normal completion.
        SessionType::Interrupt { body, .. } => credit_walk(body, available, budget),
        SessionType::Resume => Ok(available),
    }
}

/// Enumerate `(#send, #recv)` for every path from `t` that reaches `Var(x)`
/// (the loop-recurring traces). Paths that hit `End` or a free `Var(y≠x)` are
/// dropped — they exit the loop, not iterate. A shadowing `Rec(x, _)` cuts the
/// descent (the inner binder re-captures the name). Total in linear time.
fn recurring_paths_into(t: &SessionType, x: &str, s: u64, r: u64, out: &mut Vec<(u64, u64)>) {
    match t {
        SessionType::End => {} // terminates — not a recurring path
        SessionType::Var(y) if y == x => out.push((s, r)),
        SessionType::Var(_) => {} // a free var that isn't our loop's
        SessionType::Send { cont, .. } => recurring_paths_into(cont, x, s + 1, r, out),
        SessionType::Recv { cont, .. } => recurring_paths_into(cont, x, s, r + 1, out),
        SessionType::Select(m) | SessionType::Branch(m) => {
            // Each arm is its own trace — descend into all of them.
            for arm in m.values() {
                recurring_paths_into(arm, x, s, r, out);
            }
        }
        SessionType::Rec(y, body) if y != x => recurring_paths_into(body, x, s, r, out),
        SessionType::Rec(_, _) => {} // shadows x — its inner Var refers to itself
        // §Fase 79 — descend into the body (its sends/recvs count toward the loop
        // Δ); the handler's disjoint budget is analysed separately, and Resume is
        // a return-to-body marker, not a fresh recurring path.
        SessionType::Interrupt { body, .. } => recurring_paths_into(body, x, s, r, out),
        SessionType::Resume => {}
    }
}

fn dual_map(m: &BTreeMap<String, SessionType>) -> BTreeMap<String, SessionType> {
    m.iter().map(|(l, s)| (l.clone(), s.dual())).collect()
}

fn subst_map(m: &BTreeMap<String, SessionType>, var: &str, repl: &SessionType) -> BTreeMap<String, SessionType> {
    m.iter().map(|(l, s)| (l.clone(), s.subst(var, repl))).collect()
}

fn equiv_inner(s: &SessionType, t: &SessionType, assumed: &mut Vec<(SessionType, SessionType)>) -> bool {
    // Coinduction: a pair we are already proving equal is taken as equal.
    if assumed.iter().any(|(x, y)| x == s && y == t) {
        return true;
    }
    assumed.push((s.clone(), t.clone()));

    match (s.unfold_head(), t.unfold_head()) {
        (SessionType::End, SessionType::End) => true,
        (
            SessionType::Send { payload: a, credit: ca, cont: sk },
            SessionType::Send { payload: b, credit: cb, cont: tk },
        ) => a == b && ca == cb && equiv_inner(&sk, &tk, assumed),
        (
            SessionType::Recv { payload: a, credit: ca, cont: sk },
            SessionType::Recv { payload: b, credit: cb, cont: tk },
        ) => a == b && ca == cb && equiv_inner(&sk, &tk, assumed),
        (SessionType::Select(m1), SessionType::Select(m2)) => equiv_maps(&m1, &m2, assumed),
        (SessionType::Branch(m1), SessionType::Branch(m2)) => equiv_maps(&m1, &m2, assumed),
        // A bare `Var` survives unfolding only if it is free (open type); compare
        // nominally. Closed, contractive types never reach this with a head Var.
        (SessionType::Var(x), SessionType::Var(y)) => x == y,
        // §Fase 79 — two interrupt regions are equivalent iff same signal cause
        // and equivalent body + handler. Required so `is_dual_to` recognizes the
        // Intr(sig;B,H) / Intr(sig;B⊥,H⊥) pair.
        (
            SessionType::Interrupt { signal: a, body: b1, handler: h1 },
            SessionType::Interrupt { signal: b, body: b2, handler: h2 },
        ) => a == b && equiv_inner(&b1, &b2, assumed) && equiv_inner(&h1, &h2, assumed),
        (SessionType::Resume, SessionType::Resume) => true,
        _ => false,
    }
}

fn equiv_maps(
    m1: &BTreeMap<String, SessionType>,
    m2: &BTreeMap<String, SessionType>,
    assumed: &mut Vec<(SessionType, SessionType)>,
) -> bool {
    // Same label set, and equal continuations label-by-label.
    if m1.len() != m2.len() || !m1.keys().all(|l| m2.contains_key(l)) {
        return false;
    }
    m1.iter().all(|(l, s1)| equiv_inner(s1, &m2[l], assumed))
}

impl fmt::Display for SessionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionType::End => f.write_str("end"),
            SessionType::Send { payload, credit, cont } => match credit {
                Some(n) => write!(f, "!^{n}{payload}.{cont}"),
                None => write!(f, "!{payload}.{cont}"),
            },
            SessionType::Recv { payload, credit, cont } => match credit {
                Some(n) => write!(f, "?^{n}{payload}.{cont}"),
                None => write!(f, "?{payload}.{cont}"),
            },
            SessionType::Select(m) => write_choice(f, "+", m),
            SessionType::Branch(m) => write_choice(f, "&", m),
            SessionType::Rec(x, b) => write!(f, "rec {x}.{b}"),
            SessionType::Var(x) => f.write_str(x),
            SessionType::Interrupt { signal, body, handler } => {
                write!(f, "intr[{signal}]({body} ; {handler})")
            }
            SessionType::Resume => f.write_str("resume"),
        }
    }
}

fn write_choice(f: &mut fmt::Formatter<'_>, sym: &str, m: &BTreeMap<String, SessionType>) -> fmt::Result {
    write!(f, "{sym}{{")?;
    for (i, (l, s)) in m.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{l}: {s}")?;
    }
    f.write_str("}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helpers for readable session-type literals.
    fn sel(pairs: &[(&str, SessionType)]) -> SessionType {
        SessionType::select(pairs.iter().map(|(l, s)| (l.to_string(), s.clone())))
    }
    fn brn(pairs: &[(&str, SessionType)]) -> SessionType {
        SessionType::branch(pairs.iter().map(|(l, s)| (l.to_string(), s.clone())))
    }

    // ── Duality basics ─────────────────────────────────────────────────────

    #[test]
    fn dual_swaps_send_recv_and_keeps_payload() {
        let s = SessionType::send("Int", SessionType::End);
        assert_eq!(s.dual(), SessionType::recv("Int", SessionType::End));
        // The payload (`Int`) is unchanged — only the direction flips.
        assert_eq!(SessionType::recv("Int", SessionType::End).dual(), s);
    }

    #[test]
    fn dual_swaps_select_branch() {
        let s = sel(&[("a", SessionType::End), ("b", SessionType::send("T", SessionType::End))]);
        let d = s.dual();
        assert!(matches!(d, SessionType::Branch(_)));
        // …and the continuations are dualised too.
        assert_eq!(d, brn(&[("a", SessionType::End), ("b", SessionType::recv("T", SessionType::End))]));
    }

    // ── Involutivity: (S⊥)⊥ ≡ S — the cornerstone of the connection law ─────

    #[test]
    fn duality_is_an_involution() {
        let samples = vec![
            SessionType::End,
            SessionType::send("A", SessionType::recv("B", SessionType::End)),
            sel(&[("x", SessionType::End), ("y", SessionType::recv("Q", SessionType::End))]),
            // recursive: rec X. !Msg. &{ more: X, done: end }
            SessionType::rec(
                "X",
                SessionType::send("Msg", brn(&[("more", SessionType::var("X")), ("done", SessionType::End)])),
            ),
        ];
        for s in samples {
            assert!(s.dual().dual().equiv(&s), "(S⊥)⊥ ≢ S for {s}");
        }
    }

    // ── The connection law: S is dual to S⊥, and only to S⊥ ─────────────────

    #[test]
    fn connection_law_holds_for_dual_and_fails_otherwise() {
        let s = SessionType::send("Q", SessionType::recv("R", SessionType::End));
        assert!(s.is_dual_to(&s.dual()), "a session must be dual to its own dual");
        // Not dual to itself (it sends where the peer must receive).
        assert!(!s.is_dual_to(&s));
        // Not dual to a peer with a mismatched payload.
        let wrong = SessionType::recv("Q", SessionType::send("WRONG", SessionType::End));
        assert!(!s.is_dual_to(&wrong));
    }

    // ── Regular-coinductive equality: fold/unfold + α-renaming ──────────────

    #[test]
    fn equirecursive_fold_unfold_equality() {
        // μX. !A.X  ≡  !A.(μX. !A.X)   — one unfolding is equal.
        let folded = SessionType::rec("X", SessionType::send("A", SessionType::var("X")));
        let unfolded = SessionType::send("A", folded.clone());
        assert!(folded.equiv(&unfolded));
        assert!(unfolded.equiv(&folded));
    }

    #[test]
    fn equality_is_insensitive_to_bound_variable_name() {
        let x = SessionType::rec("X", SessionType::send("A", SessionType::var("X")));
        let y = SessionType::rec("Y", SessionType::send("A", SessionType::var("Y")));
        assert!(x.equiv(&y), "α-equivalent recursive sessions must be equal");
    }

    #[test]
    fn equality_reflexive_and_rejects_real_differences() {
        let s = sel(&[("a", SessionType::send("T", SessionType::End)), ("b", SessionType::End)]);
        assert!(s.equiv(&s));
        // Direction differs.
        assert!(!SessionType::send("T", SessionType::End).equiv(&SessionType::recv("T", SessionType::End)));
        // Payload differs.
        assert!(!SessionType::send("A", SessionType::End).equiv(&SessionType::send("B", SessionType::End)));
        // Label set differs.
        let s2 = sel(&[("a", SessionType::send("T", SessionType::End)), ("c", SessionType::End)]);
        assert!(!s.equiv(&s2));
        // Choice kind differs (select vs branch).
        assert!(!sel(&[("a", SessionType::End)]).equiv(&brn(&[("a", SessionType::End)])));
    }

    #[test]
    fn connection_law_holds_for_recursive_dialogue() {
        // A realistic chat dialogue: rec X. +{ ask: !Utterance. &{ token: ?Token.X, done: end }, cancel: end }
        let client = SessionType::rec(
            "X",
            sel(&[
                (
                    "ask",
                    SessionType::send(
                        "Utterance",
                        brn(&[("token", SessionType::recv("Token", SessionType::var("X"))), ("done", SessionType::End)]),
                    ),
                ),
                ("cancel", SessionType::End),
            ]),
        );
        // The server endpoint is the structural dual; the law must accept it,
        // and reject the (non-dual) identical copy.
        assert!(client.is_dual_to(&client.dual()));
        assert!(!client.is_dual_to(&client));
        // equiv terminates on this recursive type (no stack blow-up).
        assert!(client.equiv(&client));
    }

    #[test]
    fn display_is_readable() {
        let s = SessionType::send("Int", SessionType::recv("Bool", SessionType::End));
        assert_eq!(s.to_string(), "!Int.?Bool.end");
        assert_eq!(SessionType::rec("X", SessionType::var("X")).to_string(), "rec X.X");
    }

    // ── Fase 41.c — credit-refined backpressure (D2) ─────────────────────────

    #[test]
    fn dual_preserves_credit_index() {
        // (!ⁿA.S)⊥ = ?ⁿA.S⊥ — same credit, opposite direction.
        let s = SessionType::send_credit("Msg", 7, SessionType::End);
        assert_eq!(s.dual(), SessionType::recv_credit("Msg", 7, SessionType::End));
        // Round-trip preserves the credit through both polarities.
        assert!(s.dual().dual().equiv(&s));
    }

    #[test]
    fn equality_distinguishes_credit_index() {
        // Different numeric credit ⇒ structurally distinct types.
        let a = SessionType::send_credit("T", 1, SessionType::End);
        let b = SessionType::send_credit("T", 2, SessionType::End);
        assert!(!a.equiv(&b));
        // Unbounded (credit=None) is distinct from any stamped credit.
        let unbounded = SessionType::send("T", SessionType::End);
        assert!(!a.equiv(&unbounded));
    }

    #[test]
    fn with_credit_stamps_every_send_and_recv() {
        let bare = SessionType::send(
            "A",
            SessionType::recv("B", SessionType::send("C", SessionType::End)),
        );
        let stamped = bare.with_credit(4);
        let expected = SessionType::send_credit(
            "A",
            4,
            SessionType::recv_credit("B", 4, SessionType::send_credit("C", 4, SessionType::End)),
        );
        assert_eq!(stamped, expected);
        // Idempotent on already-stamped.
        assert_eq!(stamped.with_credit(4), stamped);
    }

    #[test]
    fn has_send_at_zero_finds_the_unprovable_send() {
        let bad = SessionType::recv(
            "Q",
            SessionType::send_credit("Boom", 0, SessionType::End),
        );
        assert_eq!(bad.has_send_at_zero(), Some(Payload::new("Boom")));
        // A protocol with no `!⁰…` is clean.
        let ok = SessionType::send_credit("A", 3, SessionType::End);
        assert_eq!(ok.has_send_at_zero(), None);
        // Sends inside choice arms are reached.
        let choice = sel(&[("ask", SessionType::send_credit("X", 0, SessionType::End))]);
        assert_eq!(choice.has_send_at_zero(), Some(Payload::new("X")));
    }

    // ── The Presburger discharge: credit_analyse(budget) ────────────────────

    #[test]
    fn credit_analyse_accepts_a_straight_line_protocol_within_budget() {
        // Two consecutive sends; budget = 2 ⇒ enough window.
        let s = SessionType::send("A", SessionType::send("B", SessionType::End));
        assert!(s.credit_analyse(2).is_ok());
    }

    #[test]
    fn credit_analyse_rejects_burst_overflow() {
        // Three sends in a row; budget = 2 ⇒ the third send hits available = 0.
        let s = SessionType::send(
            "A",
            SessionType::send("B", SessionType::send("C", SessionType::End)),
        );
        match s.credit_analyse(2) {
            Err(CreditError::BurstOverflow { payload, budget: 2, .. }) => {
                assert_eq!(payload, Payload::new("C"));
            }
            other => panic!("expected BurstOverflow, got {other:?}"),
        }
    }

    #[test]
    fn credit_analyse_rejects_explicit_send_at_zero() {
        let s = SessionType::send_credit("X", 0, SessionType::End);
        match s.credit_analyse(8) {
            Err(CreditError::SendAtZero { payload }) => assert_eq!(payload, Payload::new("X")),
            other => panic!("expected SendAtZero, got {other:?}"),
        }
    }

    #[test]
    fn credit_analyse_rejects_unsustainable_loop() {
        // rec X. !A.!B.?Ack.X — Δ = 2 - 1 = 1 > 0; no finite budget keeps
        // unbounded iteration in flight.
        let s = SessionType::rec(
            "X",
            SessionType::send(
                "A",
                SessionType::send("B", SessionType::recv("Ack", SessionType::var("X"))),
            ),
        );
        match s.credit_analyse(100) {
            Err(CreditError::LoopUnsustainable { sends_per_iter: 2, recvs_per_iter: 1 }) => {}
            other => panic!("expected LoopUnsustainable(2,1), got {other:?}"),
        }
    }

    #[test]
    fn credit_analyse_accepts_a_balanced_loop() {
        // rec X. !A.?Ack.X — Δ = 1 - 1 = 0; sustainable under budget ≥ 1.
        let s = SessionType::rec(
            "X",
            SessionType::send("A", SessionType::recv("Ack", SessionType::var("X"))),
        );
        assert!(s.credit_analyse(1).is_ok());
        assert!(s.credit_analyse(8).is_ok());
    }

    #[test]
    fn credit_analyse_walks_choice_arms_worst_case() {
        // +{ ask: !A.!B.end, quit: end } — the ask arm needs window 2.
        let s = sel(&[
            ("ask", SessionType::send("A", SessionType::send("B", SessionType::End))),
            ("quit", SessionType::End),
        ]);
        assert!(s.credit_analyse(2).is_ok()); // both arms fit
        assert!(matches!(
            s.credit_analyse(1),
            Err(CreditError::BurstOverflow { .. })
        ));
    }

    #[test]
    fn credit_delta_counts_per_iteration() {
        // rec X. !A.!B.?Ack.X — the body's single recurring path has Δ = (2, 1).
        let body = SessionType::send(
            "A",
            SessionType::send("B", SessionType::recv("Ack", SessionType::var("X"))),
        );
        assert_eq!(body.credit_delta("X"), (2, 1));
        // Non-recurring tail (no Var(X)) yields no recurring paths → (0, 0).
        let non_recurring = SessionType::send("A", SessionType::End);
        assert_eq!(non_recurring.credit_delta("X"), (0, 0));
        // Choice: only the recurring arm contributes; `cancel: end` is exempt.
        let body_chat = sel(&[
            (
                "ask",
                SessionType::send("U", SessionType::recv("Tok", SessionType::var("X"))),
            ),
            ("cancel", SessionType::End),
        ]);
        assert_eq!(body_chat.credit_delta("X"), (1, 1));
    }

    // ── Fase 41.e — SSE-as-fragment unification (D3) ─────────────────────

    #[test]
    fn pure_send_chain_is_in_the_sse_producer_fragment() {
        // !A.!B.end — the canonical SSE shape: a server emits a sequence
        // of events, no client input.
        let s = SessionType::send("A", SessionType::send("B", SessionType::End));
        assert!(s.projects_to_sse());
        // Its dual is the consumer-side, in the dual SSE fragment.
        assert!(s.dual().projects_to_sse_consumer());
    }

    #[test]
    fn any_recv_disqualifies_the_producer_fragment() {
        // Even one `Recv` makes the type two-polarity.
        let s = SessionType::send("Q", SessionType::recv("Ack", SessionType::End));
        assert!(!s.projects_to_sse());
        // …and the dual still has the offending direction, just flipped.
        assert!(!s.dual().projects_to_sse_consumer());
    }

    #[test]
    fn branch_disqualifies_the_producer_fragment() {
        // Branch = client picks. The producer (server) cannot offer one.
        let s = SessionType::branch([("ack".into(), SessionType::End)]);
        assert!(!s.projects_to_sse());
        // The dual is a Select, which IS in the producer fragment.
        assert!(s.dual().projects_to_sse());
    }

    #[test]
    fn select_is_in_the_producer_fragment_iff_all_arms_are() {
        // ⊕{a: !A.end, b: end} — pure server-side internal choice.
        let ok = sel(&[
            ("a", SessionType::send("A", SessionType::End)),
            ("b", SessionType::End),
        ]);
        assert!(ok.projects_to_sse());
        // ⊕{a: !A.end, b: ?Q.end} — one arm asks for client input,
        // disqualifies the entire choice.
        let bad = sel(&[
            ("a", SessionType::send("A", SessionType::End)),
            ("b", SessionType::recv("Q", SessionType::End)),
        ]);
        assert!(!bad.projects_to_sse());
    }

    #[test]
    fn recursive_sse_token_stream_is_in_the_producer_fragment() {
        // rec X. !Token.X — an unbounded SSE token stream. The canonical
        // example: every Fase 33 server-token stream is exactly this
        // type (modulo the closing `end` we typically wrap it in).
        let s = SessionType::rec(
            "X",
            SessionType::send("Token", SessionType::var("X")),
        );
        assert!(s.projects_to_sse());
        // …and the dual SSE-consumer view is `rec X. ?Token.X`.
        assert!(s.dual().projects_to_sse_consumer());
    }

    #[test]
    fn the_two_polarities_partition_the_sse_projectable_space() {
        // For every S, S.projects_to_sse() ⇔ S.dual().projects_to_sse_consumer().
        // We sample a handful of producer-side shapes + the negative cases.
        let samples_producer: Vec<SessionType> = vec![
            SessionType::End,
            SessionType::send("A", SessionType::End),
            sel(&[("x", SessionType::End), ("y", SessionType::send("T", SessionType::End))]),
            SessionType::rec("X", SessionType::send("T", SessionType::var("X"))),
        ];
        for s in samples_producer {
            assert!(s.projects_to_sse(), "{s} should project to SSE producer");
            assert!(s.dual().projects_to_sse_consumer(), "{s}⊥ should project to SSE consumer");
        }
        let samples_non_sse: Vec<SessionType> = vec![
            SessionType::recv("A", SessionType::send("B", SessionType::End)),
            SessionType::send("A", SessionType::recv("B", SessionType::End)),
            brn(&[("x", SessionType::End)]),
        ];
        for s in samples_non_sse {
            assert!(!s.projects_to_sse(), "{s} should NOT project to SSE producer");
        }
    }

    #[test]
    fn polarity_flip_is_an_involution() {
        assert_eq!(Polarity::Producer.flip(), Polarity::Consumer);
        assert_eq!(Polarity::Consumer.flip(), Polarity::Producer);
        for p in [Polarity::Producer, Polarity::Consumer] {
            assert_eq!(p.flip().flip(), p);
        }
    }

    #[test]
    fn credit_analyse_is_total_on_realistic_chat_dialogue() {
        // The 41.a chat sample: rec X. +{ ask: !Utterance. &{ token: ?Token.X,
        // done: end }, cancel: end }. Worst-case arm has Δ = 1 - 1 = 0.
        let client = SessionType::rec(
            "X",
            sel(&[
                (
                    "ask",
                    SessionType::send(
                        "Utterance",
                        brn(&[
                            ("token", SessionType::recv("Token", SessionType::var("X"))),
                            ("done", SessionType::End),
                        ]),
                    ),
                ),
                ("cancel", SessionType::End),
            ]),
        );
        assert!(client.credit_analyse(4).is_ok());
        // The dual receiver also conforms — symmetric credit.
        assert!(client.dual().credit_analyse(4).is_ok());
    }
}
