//! Multiparty session types — the global view + projection.
//!
//! v2.3.0 (D6 of the plan vivo, paper section 5). Where the binary algebra
//! [`crate::session`] describes **one endpoint's** view of a two-party
//! dialogue, this module ascends to the **global view**: a [`GlobalType`]
//! `G` declaratively names every message + choice in an n-party protocol
//! (Honda–Yoshida–Carbone, POPL'08), and the [`GlobalType::project`]
//! operator extracts each role's local [`SessionType`] from it. Together
//! they realise the v2.3.0 algebra at scale: one declaration, n
//! cursor-driven runtimes, lock-step by construction.
//!
//! The **safe-realizability gate** ([`GlobalType::project_all`]) is the
//! theorem in code: a global type `G` is *implementable* by independent
//! per-role runtimes iff projection succeeds for every role mentioned —
//! the gate refuses choices a non-participating role couldn't observe
//! (the **merge condition**), self-messages (`p → p`), and free
//! recursion variables. A passing gate is the structural certificate
//! the v2.3.0 enterprise WS surface needs to mount one binding per role
//! and have them stay in lock-step without any cross-role coordination.
//!
//! ### Grammar (paper section 5.1)
//!
//! ```text
//!   G  ::=  end                          — terminated protocol
//!         | p → q : T . G                — p sends T to q, then G
//!         | p → q : { ℓᵢ : Gᵢ }          — p selects ℓᵢ to send to q
//!         | μX. G                        — recursive protocol
//!         | X                            — recursion variable
//! ```
//!
//! ### Projection rules (paper section 5.2)
//!
//! For each role `r`:
//!
//! ```text
//!   end ⌐ r              =  end
//!   (p→q : T . G) ⌐ r    =  !T.(G ⌐ r)       if r = p
//!                        =  ?T.(G ⌐ r)       if r = q
//!                        =  G ⌐ r            otherwise (r not involved)
//!   (p→q : {ℓᵢ:Gᵢ}) ⌐ r  =  ⊕{ℓᵢ : Gᵢ ⌐ r}   if r = p
//!                        =  &{ℓᵢ : Gᵢ ⌐ r}   if r = q
//!                        =  merge_i (Gᵢ ⌐ r) if r ∉ {p, q}
//!   μX.G ⌐ r             =  μX.(G ⌐ r)
//!   X ⌐ r                =  X
//! ```
//!
//! The **merge** of `{Gᵢ ⌐ r}` is defined iff all branches project to
//! `≡`-equivalent session types for `r` — otherwise `r` couldn't tell
//! which arm `p` chose (`r` saw nothing); the protocol is then
//! unrealizable + the gate rejects.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::session::{Payload, SessionType};

/// A participant in a multiparty protocol — an opaque, comparable name.
///
/// `Role("Client")` and `Role("client")` are distinct (case-sensitive,
/// the same discipline v2.3.0's `socket` declaration uses). The wire
/// encoding is the bare string (`#[serde(transparent)]`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Role(pub String);

impl Role {
    pub fn new(name: impl Into<String>) -> Self {
        Role(name.into())
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A global session type — the protocol viewed from above. Each constructor
/// names every participant explicitly so projection has no ambiguity.
///
/// Serialisable so a global type can travel between deployment stages
/// (declaration in source → IR → an enterprise registry that drives the
/// v2.3.0 WS surface). Hashable so a deployment can fingerprint the
/// declared protocol before issuing snapshots that bind to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GlobalType {
    /// `end` — the protocol terminates.
    End,
    /// `p → q : T . G` — role `from` sends a value of type `payload` to
    /// role `to`, then the protocol continues as `cont`. `from ≠ to` is
    /// enforced by the projection gate (a self-message has no operational
    /// meaning at the global layer).
    Message {
        from: Role,
        to: Role,
        payload: Payload,
        cont: Box<GlobalType>,
    },
    /// `p → q : { ℓᵢ : Gᵢ }` — role `from` selects label `ℓᵢ`, sends it
    /// to `to`, the protocol continues as `Gᵢ`. The label set is canonical
    /// (`BTreeMap`); arms must be non-empty.
    Choice {
        from: Role,
        to: Role,
        arms: BTreeMap<String, GlobalType>,
    },
    /// `μX. G` — recursive protocol. Bind `var` in `body`.
    Rec(String, Box<GlobalType>),
    /// `X` — recursion variable. Free vars on the gate's input are an
    /// error ([`ProjectionError::UnboundVariable`]).
    Var(String),
}

impl GlobalType {
    // ── Smart constructors ────────────────────────────────────────────────

    /// `from → to : payload . cont`.
    pub fn message(
        from: impl Into<String>,
        to: impl Into<String>,
        payload: impl Into<String>,
        cont: GlobalType,
    ) -> Self {
        GlobalType::Message {
            from: Role::new(from),
            to: Role::new(to),
            payload: Payload(payload.into()),
            cont: Box::new(cont),
        }
    }
    /// `from → to : { ℓ : G, … }`.
    pub fn choice(
        from: impl Into<String>,
        to: impl Into<String>,
        arms: impl IntoIterator<Item = (String, GlobalType)>,
    ) -> Self {
        GlobalType::Choice {
            from: Role::new(from),
            to: Role::new(to),
            arms: arms.into_iter().collect(),
        }
    }
    /// `μX. G`.
    pub fn rec(var: impl Into<String>, body: GlobalType) -> Self {
        GlobalType::Rec(var.into(), Box::new(body))
    }
    /// `X`.
    pub fn var(name: impl Into<String>) -> Self {
        GlobalType::Var(name.into())
    }

    // ── The roles a global type mentions ──────────────────────────────────

    /// Every participant named anywhere in `self`. Used by
    /// [`Self::project_all`] to drive the per-role projection loop.
    pub fn roles(&self) -> BTreeSet<Role> {
        let mut out = BTreeSet::new();
        collect_roles(self, &mut out);
        out
    }

    // ── Projection (the section 5.2 operator) ────────────────────────────────────

    /// Project `self` to the local session type role `r` is expected to
    /// run. Returns the binary [`SessionType`] the v2.3.0 algebra +
    /// v2.3.0 runtime consume.
    ///
    /// Total over closed, contractive global types; rejects only if the
    /// gate fires:
    /// - [`ProjectionError::SelfMessage`] — `from == to` somewhere;
    /// - [`ProjectionError::MergeFailed`] — a non-participating role
    ///   couldn't observe the choice (arms diverge);
    /// - [`ProjectionError::EmptyChoice`] — `Choice { arms: {} }`;
    /// - [`ProjectionError::UnboundVariable`] — `Var(x)` without an
    ///   enclosing `Rec(x, _)` on the path.
    pub fn project(&self, r: &Role) -> Result<SessionType, ProjectionError> {
        project_inner(self, r)
    }

    /// Project for every role this global type mentions. A `Result::Ok`
    /// is the **safe-realizability certificate**: every endpoint has a
    /// well-defined local protocol, and any compliant per-role runtime
    /// composes into a faithful realisation of `self`.
    pub fn project_all(&self) -> Result<BTreeMap<Role, SessionType>, ProjectionError> {
        let mut out = BTreeMap::new();
        for role in self.roles() {
            let local = self.project(&role)?;
            out.insert(role, local);
        }
        Ok(out)
    }

    /// Is `self` safely realisable? Convenience wrapper around
    /// [`Self::project_all`] for callers that only care about the gate
    /// verdict (the type-checker's safety predicate).
    pub fn is_safely_realizable(&self) -> bool {
        self.project_all().is_ok()
    }
}

/// Negative verdict of the projection / safe-realizability gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    /// `from == to` for some message or choice — protocols at the
    /// global layer don't model self-talk (it would have no observable
    /// effect; the v2.3.0 algebra has no operational rule for it).
    SelfMessage { role: Role },
    /// `Choice { arms: {} }` — a choice with no arms has no projection
    /// for the chooser (no internal-choice arm to select).
    EmptyChoice { from: Role, to: Role },
    /// The merge condition failed: a role `r` that doesn't participate
    /// in a choice `p → q` saw arms that project to non-equivalent local
    /// types, so it cannot observe which branch `p` chose. The protocol
    /// is unrealizable.
    MergeFailed {
        /// The role whose projection diverged across arms.
        role: Role,
        /// The two labels whose continuations disagree (deterministic
        /// pick: alphabetically first divergent pair).
        labels: (String, String),
        /// The two projections that fail to merge — useful for the
        /// type-checker's diagnostic.
        left: Box<SessionType>,
        right: Box<SessionType>,
    },
    /// `Var(x)` reached outside any enclosing `Rec(x, _)`. The global
    /// type is open and has no closed-form projection.
    UnboundVariable(String),
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectionError::SelfMessage { role } => write!(
                f,
                "global protocol has a self-message at role `{role}` — \
                 a global type's projection has no rule for `p → p`"
            ),
            ProjectionError::EmptyChoice { from, to } => write!(
                f,
                "global choice `{from} → {to}` has no arms — \
                 the projection for `{from}` is the empty internal-choice ⊕{{}}"
            ),
            ProjectionError::MergeFailed { role, labels, left, right } => write!(
                f,
                "global type is not safely realizable: role `{role}` cannot \
                 observe the choice — arm `{}` projects to `{left}` and arm `{}` \
                 projects to `{right}` (they must be ≡ for {role} to stay in step)",
                labels.0, labels.1
            ),
            ProjectionError::UnboundVariable(var) => write!(
                f,
                "global type has a free recursion variable `{var}` — \
                 no enclosing `μ{var}. _` binds it"
            ),
        }
    }
}

impl std::error::Error for ProjectionError {}

// ── Internal helpers ──────────────────────────────────────────────────────

fn collect_roles(g: &GlobalType, out: &mut BTreeSet<Role>) {
    match g {
        GlobalType::End | GlobalType::Var(_) => {}
        GlobalType::Message { from, to, cont, .. } => {
            out.insert(from.clone());
            out.insert(to.clone());
            collect_roles(cont, out);
        }
        GlobalType::Choice { from, to, arms } => {
            out.insert(from.clone());
            out.insert(to.clone());
            for g in arms.values() {
                collect_roles(g, out);
            }
        }
        GlobalType::Rec(_, body) => collect_roles(body, out),
    }
}

fn project_inner(g: &GlobalType, r: &Role) -> Result<SessionType, ProjectionError> {
    match g {
        GlobalType::End => Ok(SessionType::End),
        GlobalType::Message { from, to, payload, cont } => {
            if from == to {
                return Err(ProjectionError::SelfMessage { role: from.clone() });
            }
            let k = project_inner(cont, r)?;
            if r == from {
                Ok(SessionType::Send {
                    payload: payload.clone(),
                    credit: None,
                    cont: Box::new(k),
                })
            } else if r == to {
                Ok(SessionType::Recv {
                    payload: payload.clone(),
                    credit: None,
                    cont: Box::new(k),
                })
            } else {
                // r is uninvolved — the message is invisible to it.
                Ok(k)
            }
        }
        GlobalType::Choice { from, to, arms } => {
            if from == to {
                return Err(ProjectionError::SelfMessage { role: from.clone() });
            }
            if arms.is_empty() {
                return Err(ProjectionError::EmptyChoice {
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            // Project every arm for `r`.
            let mut arm_projections: Vec<(&String, SessionType)> = Vec::with_capacity(arms.len());
            for (label, arm) in arms {
                let local = project_inner(arm, r)?;
                arm_projections.push((label, local));
            }
            if r == from {
                // Internal choice — keep the label set, project per arm.
                let map: BTreeMap<String, SessionType> = arm_projections
                    .into_iter()
                    .map(|(l, s)| (l.clone(), s))
                    .collect();
                Ok(SessionType::Select(map))
            } else if r == to {
                // External choice — the receiver offers all arms.
                let map: BTreeMap<String, SessionType> = arm_projections
                    .into_iter()
                    .map(|(l, s)| (l.clone(), s))
                    .collect();
                Ok(SessionType::Branch(map))
            } else {
                // r is uninvolved — every arm MUST project to the same
                // type for r (otherwise r would have to know which arm
                // was selected, but r saw nothing). This is the merge
                // condition; the verdict is the canonical safety gate.
                let mut iter = arm_projections.into_iter();
                let (first_label, first_proj) =
                    iter.next().expect("non-empty arms (checked above)");
                let canonical = first_proj;
                let canonical_label = first_label.clone();
                for (label, proj) in iter {
                    if !canonical.equiv(&proj) {
                        return Err(ProjectionError::MergeFailed {
                            role: r.clone(),
                            labels: ordered_pair(canonical_label.clone(), label.clone()),
                            left: Box::new(canonical),
                            right: Box::new(proj),
                        });
                    }
                }
                Ok(canonical)
            }
        }
        GlobalType::Rec(var, body) => {
            // Non-participation rule (standard MPST): if `r` does not
            // appear anywhere in the body, the recursion is invisible to
            // `r` and the projection is `End`. This handles `μX. X` and
            // the more common case where the loop touches a strict
            // subset of the participants — the rest just terminate.
            let mut body_roles = BTreeSet::new();
            collect_roles(body, &mut body_roles);
            if !body_roles.contains(r) {
                return Ok(SessionType::End);
            }
            let inner = project_inner(body, r)?;
            // Drop the wrapper if the body's projection doesn't actually
            // recur (the var is gone after the per-message elision
            // sequence above). Keeps the projected session type minimal.
            if contains_var(&inner, var) {
                Ok(SessionType::Rec(var.clone(), Box::new(inner)))
            } else {
                Ok(inner)
            }
        }
        GlobalType::Var(var) => Ok(SessionType::Var(var.clone())),
    }
}

/// Does `t` contain a free occurrence of `Var(var)` (i.e. not shadowed by
/// an inner `Rec(var, _)`)? Used to elide vacuous `Rec` wrappers when a
/// role's projection never recurses.
fn contains_var(t: &SessionType, var: &str) -> bool {
    match t {
        SessionType::End => false,
        SessionType::Send { cont, .. } | SessionType::Recv { cont, .. } => {
            contains_var(cont, var)
        }
        SessionType::Select(m) | SessionType::Branch(m) => {
            m.values().any(|s| contains_var(s, var))
        }
        SessionType::Rec(y, b) if y == var => false, // shadowed
        SessionType::Rec(_, b) => contains_var(b, var),
        SessionType::Var(x) => x == var,
        // v2.36.0 — a free var may occur in either interrupt sub-protocol.
        SessionType::Interrupt { body, handler, .. } => {
            contains_var(body, var) || contains_var(handler, var)
        }
        SessionType::Resume => false,
    }
}

/// Canonicalised pair (alphabetical) for deterministic diagnostics.
fn ordered_pair(a: String, b: String) -> (String, String) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers for readable tests ───────────────────────────────────────

    fn r(s: &str) -> Role {
        Role::new(s)
    }

    // ── 1. Sender / receiver / non-participant projection rules ───────────

    #[test]
    fn project_sender_yields_send() {
        // A → B : Msg . end
        let g = GlobalType::message("A", "B", "Msg", GlobalType::End);
        assert_eq!(
            g.project(&r("A")).unwrap(),
            SessionType::send("Msg", SessionType::End)
        );
    }

    #[test]
    fn project_receiver_yields_recv() {
        let g = GlobalType::message("A", "B", "Msg", GlobalType::End);
        assert_eq!(
            g.project(&r("B")).unwrap(),
            SessionType::recv("Msg", SessionType::End)
        );
    }

    #[test]
    fn projection_for_uninvolved_role_skips_the_message() {
        // A → B : Msg . end — role C is never mentioned, projects to End.
        let g = GlobalType::message("A", "B", "Msg", GlobalType::End);
        assert_eq!(g.project(&r("C")).unwrap(), SessionType::End);
        // Two-message protocol where C is also uninvolved.
        let g2 = GlobalType::message(
            "A",
            "B",
            "Msg",
            GlobalType::message("B", "A", "Ack", GlobalType::End),
        );
        assert_eq!(g2.project(&r("C")).unwrap(), SessionType::End);
    }

    // ── 2. Choice projection rules ───────────────────────────────────────

    #[test]
    fn project_chooser_yields_internal_choice() {
        // A → B : { yes: end, no: end } — A picks → ⊕.
        let g = GlobalType::choice(
            "A",
            "B",
            [("yes".into(), GlobalType::End), ("no".into(), GlobalType::End)],
        );
        let p = g.project(&r("A")).unwrap();
        assert!(matches!(p, SessionType::Select(_)));
    }

    #[test]
    fn project_offerer_yields_external_choice() {
        let g = GlobalType::choice(
            "A",
            "B",
            [("yes".into(), GlobalType::End), ("no".into(), GlobalType::End)],
        );
        let p = g.project(&r("B")).unwrap();
        assert!(matches!(p, SessionType::Branch(_)));
    }

    #[test]
    fn merge_condition_passes_when_uninvolved_role_sees_equivalent_arms() {
        // A → B : { yes: C → D : T . end,  no: C → D : T . end }
        // role C sees both arms project to !T.end (same), merge OK.
        let arm = GlobalType::message("C", "D", "T", GlobalType::End);
        let g = GlobalType::choice(
            "A",
            "B",
            [("yes".into(), arm.clone()), ("no".into(), arm)],
        );
        let pc = g.project(&r("C")).unwrap();
        assert_eq!(pc, SessionType::send("T", SessionType::End));
        // D too — receiver-side same projection.
        let pd = g.project(&r("D")).unwrap();
        assert_eq!(pd, SessionType::recv("T", SessionType::End));
    }

    #[test]
    fn merge_condition_fails_when_uninvolved_role_sees_divergent_arms() {
        // A → B : { yes: C → D : T . end,  no: end }
        // Arm `yes`: C projects to !T.end. Arm `no`: C projects to end. Diverge.
        let g = GlobalType::choice(
            "A",
            "B",
            [
                ("no".into(), GlobalType::End),
                (
                    "yes".into(),
                    GlobalType::message("C", "D", "T", GlobalType::End),
                ),
            ],
        );
        match g.project(&r("C")) {
            Err(ProjectionError::MergeFailed { role, labels, .. }) => {
                assert_eq!(role, r("C"));
                // The labels are reported in alphabetical order.
                assert_eq!(labels, ("no".into(), "yes".into()));
            }
            other => panic!("expected MergeFailed for C, got {other:?}"),
        }
    }

    // ── 3. Recursion + the elision optimisation ──────────────────────────

    #[test]
    fn recursion_round_trips_through_projection_for_an_iterating_role() {
        // μX. A → B : T . X — A and B iterate forever; C doesn't.
        let g = GlobalType::rec(
            "X",
            GlobalType::message("A", "B", "T", GlobalType::var("X")),
        );
        // A's projection: rec X. !T.X
        let pa = g.project(&r("A")).unwrap();
        assert_eq!(
            pa,
            SessionType::rec("X", SessionType::send("T", SessionType::var("X")))
        );
        // B's projection: rec X. ?T.X
        let pb = g.project(&r("B")).unwrap();
        assert_eq!(
            pb,
            SessionType::rec("X", SessionType::recv("T", SessionType::var("X")))
        );
    }

    #[test]
    fn projection_elides_vacuous_rec_for_a_non_iterating_role() {
        // μX. A → B : T . X — C never participates, so its projection
        // should collapse to End (no need to wrap in rec — the var would
        // not occur inside, and our optimisation drops the wrapper).
        let g = GlobalType::rec(
            "X",
            GlobalType::message("A", "B", "T", GlobalType::var("X")),
        );
        let pc = g.project(&r("C")).unwrap();
        // Sanity: the projected type is not a vacuous rec.
        assert!(!matches!(pc, SessionType::Rec(_, _)));
    }

    // ── 4. Gate rejections ───────────────────────────────────────────────

    #[test]
    fn self_message_is_rejected() {
        let g = GlobalType::message("A", "A", "T", GlobalType::End);
        match g.project(&r("B")) {
            Err(ProjectionError::SelfMessage { role }) => assert_eq!(role, r("A")),
            other => panic!("expected SelfMessage, got {other:?}"),
        }
    }

    #[test]
    fn empty_choice_is_rejected() {
        let g = GlobalType::choice("A", "B", []);
        match g.project(&r("A")) {
            Err(ProjectionError::EmptyChoice { from, to }) => {
                assert_eq!(from, r("A"));
                assert_eq!(to, r("B"));
            }
            other => panic!("expected EmptyChoice, got {other:?}"),
        }
    }

    // ── 5. roles() + project_all() ───────────────────────────────────────

    #[test]
    fn roles_collects_every_participant() {
        // Three-role chat: User → Agent → Tool → Agent → User.
        let g = GlobalType::message(
            "User",
            "Agent",
            "Query",
            GlobalType::message(
                "Agent",
                "Tool",
                "Sub",
                GlobalType::message(
                    "Tool",
                    "Agent",
                    "Resp",
                    GlobalType::message("Agent", "User", "Reply", GlobalType::End),
                ),
            ),
        );
        let roles = g.roles();
        assert_eq!(roles, [r("Agent"), r("Tool"), r("User")].into_iter().collect());
    }

    #[test]
    fn project_all_succeeds_on_a_safely_realizable_three_role_protocol() {
        let g = GlobalType::message(
            "User",
            "Agent",
            "Query",
            GlobalType::message(
                "Agent",
                "Tool",
                "Sub",
                GlobalType::message(
                    "Tool",
                    "Agent",
                    "Resp",
                    GlobalType::message("Agent", "User", "Reply", GlobalType::End),
                ),
            ),
        );
        let all = g.project_all().expect("safely realizable");
        assert_eq!(all.len(), 3);
        assert!(g.is_safely_realizable());
        // Spot-check the User projection: !Query.?Reply.end.
        let pu = &all[&r("User")];
        assert_eq!(
            pu,
            &SessionType::send("Query", SessionType::recv("Reply", SessionType::End))
        );
        // Tool: ?Sub.!Resp.end.
        let pt = &all[&r("Tool")];
        assert_eq!(
            pt,
            &SessionType::recv("Sub", SessionType::send("Resp", SessionType::End))
        );
        // Agent: ?Query.!Sub.?Resp.!Reply.end (the orchestrator's view).
        let pa = &all[&r("Agent")];
        assert_eq!(
            pa,
            &SessionType::recv(
                "Query",
                SessionType::send(
                    "Sub",
                    SessionType::recv("Resp", SessionType::send("Reply", SessionType::End))
                )
            )
        );
    }

    #[test]
    fn is_safely_realizable_rejects_the_diverging_choice() {
        let g = GlobalType::choice(
            "A",
            "B",
            [
                ("yes".into(), GlobalType::message("C", "D", "T", GlobalType::End)),
                ("no".into(), GlobalType::End),
            ],
        );
        assert!(!g.is_safely_realizable());
    }

    // ── 6. Serde round-trip — the wire shape downstream tools consume ────

    #[test]
    fn global_type_round_trips_through_json() {
        let g = GlobalType::rec(
            "X",
            GlobalType::choice(
                "Client",
                "Server",
                [
                    (
                        "ask".into(),
                        GlobalType::message(
                            "Server",
                            "Client",
                            "Token",
                            GlobalType::var("X"),
                        ),
                    ),
                    ("cancel".into(), GlobalType::End),
                ],
            ),
        );
        let json = serde_json::to_string(&g).unwrap();
        let back: GlobalType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, g);
    }

    // ── 7. The Kivi paper-style three-role chat with a recursion ─────────

    #[test]
    fn kivi_three_role_recursive_chat_projects_per_role() {
        // The v2.3.0 motivating example — orchestrator + skill + user, a
        // genuinely safely-realizable three-role loop (every participant
        // observes every iteration, no hidden choice):
        //
        //   μX. User → Agent : Utterance .
        //       Agent → Skill : Sub .
        //       Skill → Agent : Resp .
        //       Agent → User : Reply .
        //       X
        //
        // Adding a "done" arm a non-participant role couldn't observe
        // (e.g. an `Agent → User : {more: …, done: end}` where Skill is
        // silent on the choice) is correctly REJECTED by the gate — see
        // `merge_condition_fails_…` above. To support termination across
        // all three, the protocol must propagate the choice signal to
        // every active role (e.g. Agent → Skill : {more, done} before
        // the outer choice); we exercise that pattern in
        // `safely_propagated_choice_projects_for_every_role` below.
        let g = GlobalType::rec(
            "X",
            GlobalType::message(
                "User",
                "Agent",
                "Utterance",
                GlobalType::message(
                    "Agent",
                    "Skill",
                    "Sub",
                    GlobalType::message(
                        "Skill",
                        "Agent",
                        "Resp",
                        GlobalType::message("Agent", "User", "Reply", GlobalType::var("X")),
                    ),
                ),
            ),
        );
        let all = g.project_all().expect("3-role chat is safely realizable");
        assert_eq!(all.len(), 3);
        // Each role iterates — the recursion is preserved per-role.
        for role in [r("User"), r("Agent"), r("Skill")] {
            assert!(
                matches!(all[&role], SessionType::Rec(_, _)),
                "{role} should iterate (got {})",
                all[&role]
            );
        }
        // Skill's body: ?Sub.!Resp.X (the choice/Reply are uninvolved
        // for Skill, so they elide).
        let skill = &all[&r("Skill")];
        if let SessionType::Rec(_, body) = skill {
            assert_eq!(
                **body,
                SessionType::recv("Sub", SessionType::send("Resp", SessionType::var("X")))
            );
        }
    }

    #[test]
    fn safely_realizable_choice_projects_for_every_role() {
        // The canonical "uniform-continuation choice" — both arms have
        // identical observable behaviour for every non-participating
        // role, so the merge condition is trivially satisfied. Agent
        // decides whether to acknowledge User; either way, Agent
        // delivers `T` to Skill.
        //
        //   Agent → User : { ack: Agent → Skill : T . end,
        //                    nak: Agent → Skill : T . end }
        //
        // Roles + projections:
        //   - Agent: ⊕{ ack: !T.end, nak: !T.end }     (chooser)
        //   - User : &{ ack: end,    nak: end }        (offerer)
        //   - Skill: ?T.end                            (uninvolved in
        //                                              outer choice;
        //                                              merge of both
        //                                              arms = ?T.end)
        let g = GlobalType::choice(
            "Agent",
            "User",
            [
                (
                    "ack".into(),
                    GlobalType::message("Agent", "Skill", "T", GlobalType::End),
                ),
                (
                    "nak".into(),
                    GlobalType::message("Agent", "Skill", "T", GlobalType::End),
                ),
            ],
        );
        let all = g.project_all().expect("uniform-continuation choice is realizable");
        assert_eq!(all.len(), 3);
        // Skill's projection collapses the choice (both arms project to
        // the same `?T.end` for Skill — merge passes silently).
        assert_eq!(
            all[&r("Skill")],
            SessionType::recv("T", SessionType::End)
        );
        // Agent is the chooser → internal choice.
        assert!(matches!(all[&r("Agent")], SessionType::Select(_)));
        // User is the offerer → external choice.
        assert!(matches!(all[&r("User")], SessionType::Branch(_)));
    }

    #[test]
    fn contains_var_handles_shadowing_correctly() {
        // rec Y. send T . var X  — outer X is free; rec Y body has X as free.
        let t = SessionType::rec(
            "Y",
            SessionType::send("T", SessionType::var("X")),
        );
        assert!(contains_var(&t, "X"));
        // rec X. send T . var X  — X is bound inside; from outside, no X.
        let t2 = SessionType::rec(
            "X",
            SessionType::send("T", SessionType::var("X")),
        );
        assert!(!contains_var(&t2, "X"));
    }
}
