//! §Fase 111.i — **the SessionType compiler.** The declared protocol finally
//! reaches the runtime.
//!
//! # What was missing
//!
//! `session <P> { role A { send Msg … } role B { receive Msg … } }` was
//! **genuinely checked** at compile time — Honda–Vasconcelos duality is *decided*
//! in [`crate::session`] (dual involution, capture-avoiding substitution,
//! coinductive equality), and `check_session_duality` really rejects non-dual
//! roles. That half is real, and §111's audit confirmed it.
//!
//! And then it was **thrown away.** The type-checker lowered the roles into the
//! `SessionType` algebra to prove duality and dropped the result on the floor.
//! `IRProgram.sessions` carried the declarations all the way to the runtime, and
//! **nothing read them.**
//!
//! The enterprise server, which *does* serve the WebSocket wire, wrote its own
//! situation down with admirable candour — under a heading called *"SessionType
//! resolution honesty"*:
//!
//! > *"The `protocol: "ChatProtocol"` string in the IR is treated as an **opaque
//! > identifier**. **Every deployed socket gets the canonical `chat_schema`**. A
//! > future Fase will add the session-type declarations to the IR + a SessionType
//! > compiler that maps the protocol string to a fully-resolved `SessionType`."*
//!
//! So an adopter could declare a protocol, have its duality *proven* at compile
//! time, deploy it — and the runtime would enforce a **hardcoded chat schema**
//! instead. The proof was real. It just wasn't about the thing that ran.
//!
//! **This module is that future Fase.** The declarations were already in the IR;
//! only the compiler was missing.
//!
//! # Fidelity
//!
//! [`session_type_of_role`] mirrors the type-checker's own `lower_session_role` /
//! `lower_session_steps` **step for step** — same recursion point, same terminal
//! ops, same `Interrupt` shape. That is deliberate: if the runtime lowered the
//! protocol even slightly differently from the checker, the compile-time duality
//! proof would be a proof about a *different* protocol than the one enforced —
//! which is exactly the class of defect §111 exists to end.

use crate::ir_nodes::{IRProgram, IRSession, IRSessionRole, IRSessionStep};
use crate::session::{Payload, SessionType};

/// Lower one role's ordered steps into the session algebra.
///
/// Mirrors `type_checker::lower_session_steps` exactly.
fn lower_steps(steps: &[IRSessionStep]) -> SessionType {
    let Some((first, rest)) = steps.split_first() else {
        return SessionType::End;
    };
    match first.op.as_str() {
        "send" => SessionType::send(first.message_type.clone(), lower_steps(rest)),
        "receive" => SessionType::recv(first.message_type.clone(), lower_steps(rest)),
        // The role-level recursion variable — bound by `lower_role` below.
        "loop" => SessionType::var("X"),
        "end" => SessionType::End,
        "select" => SessionType::select(branch_types(first)),
        "branch" => SessionType::branch(branch_types(first)),
        // §Fase 79 — `interrupt { body } on Sig as sig resumable { handler }`
        // lowers to `Intr(sig; B, H)`. Terminal, like select/branch: the region's
        // continuation lives inside its body.
        "interrupt" => {
            let arm = |label: &str| {
                first
                    .branches
                    .iter()
                    .find(|b| b.label == label)
                    .map(|b| lower_steps(&b.steps))
                    .unwrap_or(SessionType::End)
            };
            SessionType::Interrupt {
                signal: Payload::new(first.message_type.clone()),
                body: Box::new(arm("body")),
                handler: Box::new(arm("handler")),
            }
        }
        "resume" => SessionType::Resume,
        // A malformed mid-sequence op cannot reach here from a program that
        // type-checked (`check_session_role` rejects it). Skip it rather than
        // fabricate a step: inventing protocol structure the adopter did not
        // write is the one thing a session compiler must never do.
        _ => lower_steps(rest),
    }
}

fn branch_types(step: &IRSessionStep) -> Vec<(String, SessionType)> {
    step.branches
        .iter()
        .map(|b| (b.label.clone(), lower_steps(&b.steps)))
        .collect()
}

/// Does this role's protocol loop back to its start?
fn contains_loop(steps: &[IRSessionStep]) -> bool {
    steps.iter().any(|s| {
        s.op == "loop" || s.branches.iter().any(|b| contains_loop(&b.steps))
    })
}

/// Compile one declared role into its [`SessionType`].
///
/// A role that loops is wrapped in a single role-level `μX` — the loop-back point
/// — exactly as the type-checker does when it proves duality.
pub fn session_type_of_role(role: &IRSessionRole) -> SessionType {
    let body = lower_steps(&role.steps);
    if contains_loop(&role.steps) {
        SessionType::rec("X", body)
    } else {
        body
    }
}

/// Resolve the schema a **server-side** endpoint of `session_name` must enforce.
///
/// A binary session declares exactly two dual roles (the type-checker guarantees
/// it). Convention: the **first** declared role is the server's; its dual is the
/// client's. The two are *proven* dual at compile time, so which one we take is a
/// convention rather than a semantic choice — but the server must take the same
/// one every time, or a client written against the published protocol would face
/// its own type instead of its dual.
///
/// Returns `None` when the session is not declared, or declares no roles. The
/// caller **refuses** — it does not substitute a default protocol. Serving a
/// schema the adopter did not write is precisely the defect this module fixes.
pub fn server_schema(ir: &IRProgram, session_name: &str) -> Option<SessionType> {
    let session: &IRSession = ir.sessions.iter().find(|s| s.name == session_name)?;
    let role = session.roles.first()?;
    Some(session_type_of_role(role))
}

/// Resolve the schema for a declared `socket`, via the `session` its `protocol:`
/// names.
///
/// `None` ⇒ the socket names a protocol that is not declared. The route refuses
/// the upgrade rather than falling back to a canonical shape.
pub fn schema_for_socket(ir: &IRProgram, socket_name: &str) -> Option<SessionType> {
    let socket = ir.sockets.iter().find(|s| s.name == socket_name)?;
    server_schema(ir, &socket.protocol)
}

/// The socket's declared backpressure credit (§41.c `!ⁿA.S`), if any.
/// `None` ⇒ the unbounded fragment.
pub fn credit_for_socket(ir: &IRProgram, socket_name: &str) -> Option<u64> {
    ir.sockets
        .iter()
        .find(|s| s.name == socket_name)
        .and_then(|s| s.backpressure_credit)
        .map(|c| if c < 0 { 0 } else { c as u64 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_nodes::{IRSessionBranch, IRSocket};

    fn step(op: &str, msg: &str) -> IRSessionStep {
        IRSessionStep {
            node_type: "session_step",
            source_line: 0,
            source_column: 0,
            op: op.into(),
            message_type: msg.into(),
            branches: Vec::new(),
            binder: String::new(),
            resumable: false,
        }
    }

    fn role(name: &str, steps: Vec<IRSessionStep>) -> IRSessionRole {
        IRSessionRole {
            node_type: "session_role",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            steps,
        }
    }

    fn program(sessions: Vec<IRSession>, sockets: Vec<IRSocket>) -> IRProgram {
        let mut ir = IRProgram::new();
        ir.sessions = sessions;
        ir.sockets = sockets;
        ir
    }

    fn session(name: &str, roles: Vec<IRSessionRole>) -> IRSession {
        IRSession {
            node_type: "session",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            roles,
        }
    }

    fn socket(name: &str, protocol: &str, credit: Option<i64>) -> IRSocket {
        IRSocket {
            node_type: "socket",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            protocol: protocol.into(),
            backpressure_credit: credit,
            reconnect: false,
            legal_basis: None,
        }
    }

    /// The load-bearing test: the schema the runtime enforces is **the one the
    /// adopter declared** — not a canonical chat shape.
    #[test]
    fn the_declared_protocol_is_what_compiles() {
        let s = session(
            "Trade",
            vec![
                role("broker", vec![step("receive", "Order"), step("send", "Fill"), step("end", "")]),
                role("client", vec![step("send", "Order"), step("receive", "Fill"), step("end", "")]),
            ],
        );
        let ir = program(vec![s], vec![socket("Wire", "Trade", None)]);

        let schema = schema_for_socket(&ir, "Wire").expect("the declared protocol must resolve");
        let expected = SessionType::recv(
            "Order",
            SessionType::send("Fill", SessionType::End),
        );
        assert_eq!(
            schema, expected,
            "the runtime must enforce `?Order.!Fill.end` — the protocol the adopter WROTE. \
             Enterprise substituted a hardcoded chat schema here, so a proven-dual protocol was \
             deployed and a different one was enforced"
        );
    }

    /// And it is genuinely the DUAL of the client's — which is the entire point of
    /// a session type. If the server compiled its own role's *client* view, a
    /// conforming client would meet its own type instead of its dual and deadlock.
    #[test]
    fn the_server_schema_is_dual_to_the_client_role() {
        let broker = role(
            "broker",
            vec![step("receive", "Order"), step("send", "Fill"), step("end", "")],
        );
        let client = role(
            "client",
            vec![step("send", "Order"), step("receive", "Fill"), step("end", "")],
        );
        let server_ty = session_type_of_role(&broker);
        let client_ty = session_type_of_role(&client);
        assert!(
            server_ty.is_dual_to(&client_ty),
            "the compiled roles must be dual — the same law the type-checker proves"
        );
    }

    /// A looping role compiles to `μX. …` — the recursion point the checker uses.
    #[test]
    fn a_looping_role_gets_its_recursion_point() {
        let r = role(
            "echo",
            vec![step("receive", "Msg"), step("send", "Msg"), step("loop", "")],
        );
        let ty = session_type_of_role(&r);
        assert_eq!(
            ty,
            SessionType::rec(
                "X",
                SessionType::recv("Msg", SessionType::send("Msg", SessionType::var("X")))
            )
        );
    }

    /// Labelled choice survives the round trip.
    #[test]
    fn select_and_branch_compile_their_arms() {
        let mut sel = step("select", "");
        sel.branches = vec![
            IRSessionBranch {
                node_type: "session_branch",
                label: "buy".into(),
                steps: vec![step("send", "Buy"), step("end", "")],
            },
            IRSessionBranch {
                node_type: "session_branch",
                label: "quit".into(),
                steps: vec![step("end", "")],
            },
        ];
        let ty = session_type_of_role(&role("r", vec![sel]));
        match ty {
            SessionType::Select(arms) => {
                assert_eq!(arms.len(), 2);
                assert_eq!(arms["buy"], SessionType::send("Buy", SessionType::End));
                assert_eq!(arms["quit"], SessionType::End);
            }
            other => panic!("expected Select, got {other:?}"),
        }
    }

    /// **The refusal.** A socket naming an undeclared protocol resolves to
    /// nothing — and the caller must refuse rather than substitute a default.
    /// Serving a schema the adopter did not write is the defect this module fixes;
    /// re-introducing it as a "safe fallback" would undo the whole thing.
    #[test]
    fn an_undeclared_protocol_resolves_to_nothing() {
        let ir = program(vec![], vec![socket("Wire", "GhostProtocol", None)]);
        assert!(schema_for_socket(&ir, "Wire").is_none());
        assert!(schema_for_socket(&ir, "NoSuchSocket").is_none());
    }

    /// The declared backpressure credit (§41.c) reaches the runtime too.
    #[test]
    fn the_declared_credit_reaches_the_runtime() {
        let ir = program(vec![], vec![socket("Wire", "P", Some(8))]);
        assert_eq!(credit_for_socket(&ir, "Wire"), Some(8));
        // A negative credit is clamped to 0 — the "no rule at n = 0" axiom makes
        // it unprovable, which is the correct, refusing behaviour.
        let ir2 = program(vec![], vec![socket("Wire", "P", Some(-3))]);
        assert_eq!(credit_for_socket(&ir2, "Wire"), Some(0));
    }
}
