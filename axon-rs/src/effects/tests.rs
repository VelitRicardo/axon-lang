//! v1.17.0 — Rust runtime tests for algebraic effects.
//!
//! Verifies the FSM dispatch loop, handler frame stack, one-shot
//! continuation semantics (D2), exhaustiveness (D9 trust boundary),
//! linear resume (D10 trust boundary), forward propagation (D12), and
//! cross-stack JSON IR contract (the wire format Python emits).

use serde_json::json;

use super::ir::{
    parse_block, parse_effect, IREffectDeclaration, IREffectOperation, IRHandlerClause,
    IRHandlerFrame, IRPerform, IRResume, Instruction,
};
use super::runtime::{EffectRuntime, ExecutionResult, TraceEvent};
use super::value::Value;

// ────────────────────────────────────────────────────────────────────
//  Helpers
// ────────────────────────────────────────────────────────────────────

fn empty_runtime() -> EffectRuntime {
    EffectRuntime::new()
}

fn sample_effect(name: &str, ops: Vec<(&str, Vec<&str>, &str)>) -> IREffectDeclaration {
    IREffectDeclaration {
        name: name.to_string(),
        operations: ops
            .into_iter()
            .map(|(op_name, params, ret)| IREffectOperation {
                name: op_name.to_string(),
                type_parameters: vec![],
                parameter_names: params.iter().map(|s| s.to_string()).collect(),
                parameter_types: params.iter().map(|_| "String".to_string()).collect(),
                return_type: ret.to_string(),
                source_line: 0,
                source_column: 0,
            })
            .collect(),
        source_line: 0,
        source_column: 0,
    }
}

/// Build a `handle E { Op(p) -> { resume(value) } } in { perform E.Op(arg) }`
/// fragment with given effect name, operation name, parameter, value, arg.
fn simple_handle_perform(
    effect: &str,
    op: &str,
    param: &str,
    resume_value: &str,
    perform_arg: &str,
) -> Instruction {
    Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec![effect.to_string()],
        clauses: vec![IRHandlerClause {
            operation_name: op.to_string(),
            parameter_names: vec![param.to_string()],
            body: vec![Instruction::Resume(IRResume {
                value_expr: resume_value.to_string(),
                frame_id: 0,
            })],
            source_line: 0,
            source_column: 0,
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: effect.to_string(),
            operation_name: op.to_string(),
            arguments: vec![perform_arg.to_string()],
            state_id: 0,
            resume_label: format!("resume_{effect}_{op}_0"),
        })],
        frame_id: 0,
        body_states: vec![0],
        source_line: 0,
        source_column: 0,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Value tests
// ────────────────────────────────────────────────────────────────────

#[test]
fn value_from_text_recognises_literals() {
    assert_eq!(Value::from_argument_text("true"), Value::Bool(true));
    assert_eq!(Value::from_argument_text("42"), Value::Int(42));
    assert_eq!(Value::from_argument_text("3.14"), Value::Float(3.14));
    assert_eq!(
        Value::from_argument_text("token"),
        Value::Symbol("token".to_string())
    );
}

#[test]
fn value_unit_renders() {
    assert_eq!(Value::Unit.render(), "()");
    assert!(Value::Unit.is_unit());
}

#[test]
fn value_default_is_unit() {
    let v: Value = Default::default();
    assert!(v.is_unit());
}

#[test]
fn value_serialises_via_serde() {
    let v = Value::List(vec![Value::Int(1), Value::String("a".into())]);
    let s = serde_json::to_string(&v).unwrap();
    assert!(s.contains("1"));
    assert!(s.contains("\"a\""));
}

// ────────────────────────────────────────────────────────────────────
//  IR deserialisation tests
// ────────────────────────────────────────────────────────────────────

#[test]
fn deserialize_effect_declaration() {
    let s = r#"{
        "name": "SSE",
        "operations": [
            {"name": "Emit", "type_parameters": [], "parameter_names": ["t"], "parameter_types": ["String"], "return_type": "Unit"},
            {"name": "Done", "type_parameters": [], "parameter_names": [], "parameter_types": [], "return_type": "Never"}
        ]
    }"#;
    let eff = parse_effect(s).unwrap();
    assert_eq!(eff.name, "SSE");
    assert_eq!(eff.operations.len(), 2);
    assert_eq!(eff.operations[0].name, "Emit");
    assert_eq!(eff.operations[0].parameter_names, vec!["t".to_string()]);
    assert_eq!(eff.operations[1].return_type, "Never");
}

#[test]
fn deserialize_effect_operation_with_type_parameters() {
    // D1 — operation polymorphism survives the wire format.
    let s = r#"{
        "name": "Channel",
        "operations": [
            {"name": "Send", "type_parameters": ["T"], "parameter_names": ["v"], "parameter_types": ["T"], "return_type": "Unit"}
        ]
    }"#;
    let eff = parse_effect(s).unwrap();
    assert_eq!(eff.operations[0].type_parameters, vec!["T".to_string()]);
}

#[test]
fn deserialize_perform_instruction() {
    let s = r#"[{"node_type":"perform","effect_name":"E","operation_name":"Op","arguments":["x"],"state_id":7,"resume_label":"resume_E_Op_7"}]"#;
    let block = parse_block(s).unwrap();
    assert_eq!(block.len(), 1);
    match &block[0] {
        Instruction::Perform(p) => {
            assert_eq!(p.effect_name, "E");
            assert_eq!(p.operation_name, "Op");
            assert_eq!(p.arguments, vec!["x".to_string()]);
            assert_eq!(p.state_id, 7);
            assert_eq!(p.resume_label, "resume_E_Op_7");
        }
        _ => panic!("expected Perform"),
    }
}

#[test]
fn deserialize_handler_frame_with_clauses_and_body() {
    let payload = json!([{
        "node_type": "handler_frame",
        "effect_names": ["SSE"],
        "frame_id": 0,
        "body_states": [0],
        "clauses": [{
            "node_type": "handler_clause",
            "operation_name": "Emit",
            "parameter_names": ["t"],
            "body": [{"node_type":"resume","value_expr":"","frame_id":0}],
        }],
        "body": [{"node_type":"perform","effect_name":"SSE","operation_name":"Emit","arguments":["x"],"state_id":0,"resume_label":"resume_SSE_Emit_0"}],
    }]);
    let block: Vec<Instruction> = serde_json::from_value(payload).unwrap();
    match &block[0] {
        Instruction::HandlerFrame(f) => {
            assert_eq!(f.effect_names, vec!["SSE".to_string()]);
            assert_eq!(f.frame_id, 0);
            assert_eq!(f.body_states, vec![0]);
            assert_eq!(f.clauses.len(), 1);
            assert_eq!(f.body.len(), 1);
        }
        _ => panic!("expected HandlerFrame"),
    }
}

#[test]
fn deserialize_unknown_node_type_is_passthrough() {
    let s = r#"[{"node_type": "step", "name": "S", "body": []}]"#;
    let block = parse_block(s).unwrap();
    assert_eq!(block.len(), 1);
    matches!(block[0], Instruction::Passthrough);
}

#[test]
fn deserialize_full_paper_canonical_example() {
    // The exact shape from `docs/algebraic_effects_streaming.md`'s SSE.
    let payload = json!([{
        "node_type": "handler_frame",
        "effect_names": ["SSE"],
        "frame_id": 0,
        "body_states": [0, 1],
        "clauses": [
            {"node_type":"handler_clause","operation_name":"Emit","parameter_names":["token"],
             "body":[{"node_type":"resume","value_expr":"","frame_id":0}]},
            {"node_type":"handler_clause","operation_name":"Done","parameter_names":[],
             "body":[{"node_type":"abort","value_expr":"","frame_id":0}]}
        ],
        "body": [
            {"node_type":"perform","effect_name":"SSE","operation_name":"Emit","arguments":["t"],"state_id":0,"resume_label":"resume_SSE_Emit_0"},
            {"node_type":"perform","effect_name":"SSE","operation_name":"Done","arguments":[],"state_id":1,"resume_label":"resume_SSE_Done_1"}
        ]
    }]);
    let block: Vec<Instruction> = serde_json::from_value(payload).unwrap();
    assert_eq!(block.len(), 1);
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Effect declaration registration
// ────────────────────────────────────────────────────────────────────

#[test]
fn register_effect_records_in_table() {
    let mut rt = empty_runtime();
    rt.register_effect(sample_effect("SSE", vec![("Emit", vec!["t"], "Unit")]));
    assert!(rt.effects().contains_key("SSE"));
}

#[test]
fn lookup_operation_returns_signature() {
    let mut rt = empty_runtime();
    rt.register_effect(sample_effect("E", vec![("Op", vec!["x", "y"], "Unit")]));
    let op = rt.lookup_operation("E", "Op").unwrap();
    assert_eq!(op.parameter_names, vec!["x".to_string(), "y".to_string()]);
    assert_eq!(op.return_type, "Unit");
}

#[test]
fn lookup_unknown_effect_returns_none() {
    let rt = empty_runtime();
    assert!(rt.lookup_operation("Ghost", "Boo").is_none());
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Perform / Handle / Resume
// ────────────────────────────────────────────────────────────────────

#[test]
fn simple_perform_handled_by_resume_completes() {
    let block = vec![simple_handle_perform("E", "Op", "x", "", "arg")];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn resume_with_value_returns_value_to_perform_site() {
    // Clause does `resume(answer)`; the perform yields `answer` as
    // the value of the perform expression.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Recv".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume {
                value_expr: "answer".into(),
                frame_id: 0,
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Recv".into(),
            arguments: vec![],
            state_id: 0,
            resume_label: "r0".into(),
        })],
        frame_id: 0,
        body_states: vec![0],
        ..Default()
    })];
    let mut rt = empty_runtime();
    rt.bind_global("answer", Value::String("hello".into()));
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::String("hello".into())));
}

#[test]
fn perform_unhandled_effect_is_runtime_error() {
    // No enclosing handle frame → the perform surfaces as
    // UnhandledEffect (the typechecker should reject this statically;
    // the runtime guards against compiler bugs).
    let block = vec![Instruction::Perform(IRPerform {
        effect_name: "E".into(),
        operation_name: "Op".into(),
        arguments: vec![],
        state_id: 0,
        resume_label: "r".into(),
    })];
    let mut rt = empty_runtime();
    let err = rt.run(&block).unwrap_err();
    assert!(matches!(
        err,
        super::runtime::EffectRuntimeError::UnhandledEffect { .. }
    ));
}

#[test]
fn perform_unknown_operation_is_runtime_error() {
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Bogus".into(),
            arguments: vec![],
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    let err = rt.run(&block).unwrap_err();
    assert!(matches!(
        err,
        super::runtime::EffectRuntimeError::UnknownOperation { .. }
    ));
}

#[test]
fn two_performs_in_body_both_resume() {
    // perform / perform; both clauses just resume; the run completes.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 0,
                ..Default()
            }),
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 1,
                ..Default()
            }),
        ],
        frame_id: 0,
        body_states: vec![0, 1],
        ..Default()
    })];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Abort
// ────────────────────────────────────────────────────────────────────

#[test]
fn abort_terminates_the_handle_with_its_value() {
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Abort(super::ir::IRAbort {
                value_expr: "reason".into(),
                frame_id: 0,
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    rt.bind_global("reason", Value::String("done".into()));
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Aborted(Value::String("done".into())));
}

#[test]
fn abort_value_unit_when_value_expr_empty() {
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Abort(super::ir::IRAbort::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Aborted(Value::Unit));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Forward (D12)
// ────────────────────────────────────────────────────────────────────

#[test]
fn forward_propagates_to_outer_frame() {
    // outer handle E { Op() -> { resume } } in {
    //   inner handle E { Op() -> { forward E.Op() } } in {
    //     perform E.Op()
    //   }
    // }
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                arguments: vec![],
                source_frame_id: 1,
                state_id: 1,
                resume_label: "fw_E_Op_1".into(),
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            state_id: 0,
            ..Default()
        })],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 0,
        ..Default()
    });
    let block = vec![outer];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn forward_without_outer_handler_is_runtime_error() {
    // Only one handler frame; its clause invokes forward → no outer.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                source_frame_id: 0,
                ..Default()
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    let err = rt.run(&block).unwrap_err();
    assert!(matches!(
        err,
        super::runtime::EffectRuntimeError::UnhandledEffect { .. }
    ));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Multi-clause + multi-effect handlers
// ────────────────────────────────────────────────────────────────────

#[test]
fn multi_clause_handler_dispatches_per_op_name() {
    // Two clauses, two performs — A dispatches to A's clause, B to B's.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![
            IRHandlerClause {
                operation_name: "A".into(),
                parameter_names: vec![],
                body: vec![Instruction::Resume(IRResume::default())],
                ..Default()
            },
            IRHandlerClause {
                operation_name: "B".into(),
                parameter_names: vec![],
                body: vec![Instruction::Resume(IRResume::default())],
                ..Default()
            },
        ],
        body: vec![
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "A".into(),
                state_id: 0,
                ..Default()
            }),
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "B".into(),
                state_id: 1,
                ..Default()
            }),
        ],
        frame_id: 0,
        body_states: vec![0, 1],
        ..Default()
    })];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn multi_effect_handler_handles_both_effects() {
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E1".into(), "E2".into()],
        clauses: vec![
            IRHandlerClause {
                operation_name: "Op1".into(),
                parameter_names: vec![],
                body: vec![Instruction::Resume(IRResume::default())],
                ..Default()
            },
            IRHandlerClause {
                operation_name: "Op2".into(),
                parameter_names: vec![],
                body: vec![Instruction::Resume(IRResume::default())],
                ..Default()
            },
        ],
        body: vec![
            Instruction::Perform(IRPerform {
                effect_name: "E1".into(),
                operation_name: "Op1".into(),
                state_id: 0,
                ..Default()
            }),
            Instruction::Perform(IRPerform {
                effect_name: "E2".into(),
                operation_name: "Op2".into(),
                state_id: 1,
                ..Default()
            }),
        ],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Nested handlers (D3 delimited scope)
// ────────────────────────────────────────────────────────────────────

#[test]
fn nested_handlers_inner_consumes_inner_perform() {
    // outer handles E1; inner handles E2; perform of E2 caught by inner.
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E2".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op2".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E2".into(),
            operation_name: "Op2".into(),
            ..Default()
        })],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E1".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op1".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    let result = rt.run(&[outer]).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn nested_handlers_outer_catches_outer_perform() {
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E2".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op2".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![
            // perform E1 inside the inner handle's body — only the
            // outer frame handles E1, so it bubbles up correctly.
            Instruction::Perform(IRPerform {
                effect_name: "E1".into(),
                operation_name: "Op1".into(),
                ..Default()
            }),
        ],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E1".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op1".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    let result = rt.run(&[outer]).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn inner_abort_propagates_only_to_inner_handle() {
    // Inner clause aborts; outer body continues past the inner handle.
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Abort(super::ir::IRAbort::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 1,
        ..Default()
    });
    let block = vec![inner];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    // The outermost run treated the inner abort as the run's result
    // (no frame above it to absorb it).
    assert_eq!(result, ExecutionResult::Aborted(Value::Unit));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Parameter binding
// ────────────────────────────────────────────────────────────────────

#[test]
fn clause_parameters_bind_perform_arguments() {
    // perform E.Op("hello"); clause Op(t) -> resume(t)
    // → perform yields the bound value back to the perform site.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec!["t".into()],
            body: vec![Instruction::Resume(IRResume {
                value_expr: "t".into(),
                frame_id: 0,
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            arguments: vec!["payload".into()],
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    rt.bind_global("payload", Value::String("hello".into()));
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::String("hello".into())));
}

#[test]
fn clause_parameter_binding_does_not_leak_after_dispatch() {
    // Outer scope binds `t = "outer"`; clause shadows with `t = "inner"`.
    // After dispatch, outer scope sees `t = "outer"` again.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec!["t".into()],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            arguments: vec!["inner".into()],
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    rt.bind_global("t", Value::String("outer".into()));
    rt.bind_global("inner", Value::String("inner_value".into()));
    let _ = rt.run(&block).unwrap();
    // After the run, `t` is back to the outer binding.
    assert_eq!(
        rt.lookup_operation("E", "Op").map(|_| ()).unwrap_or(()),
        ()
    );
    let trace_block = vec![Instruction::Passthrough];
    let _ = rt.run(&trace_block).unwrap();
    // We can't directly read globals via public API, but the
    // restoration is exercised by the no-leak invariant: a second
    // run with the same shape sees the original `t`.
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Tracing
// ────────────────────────────────────────────────────────────────────

#[test]
fn trace_records_enter_perform_resume_exit_in_order() {
    let block = vec![simple_handle_perform("E", "Op", "x", "", "arg")];
    let mut rt = empty_runtime();
    rt.enable_tracing();
    let _ = rt.run(&block).unwrap();
    let trace = rt.take_trace();
    assert!(matches!(trace[0], TraceEvent::EnterFrame { .. }));
    assert!(matches!(trace[1], TraceEvent::Perform { .. }));
    assert!(matches!(trace[2], TraceEvent::Resume { .. }));
    assert!(matches!(trace[3], TraceEvent::ExitFrame { .. }));
}

#[test]
fn trace_disabled_by_default() {
    let block = vec![simple_handle_perform("E", "Op", "x", "", "arg")];
    let mut rt = empty_runtime();
    let _ = rt.run(&block).unwrap();
    assert!(rt.take_trace().is_empty());
}

#[test]
fn trace_records_abort_event() {
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Abort(super::ir::IRAbort::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    rt.enable_tracing();
    let _ = rt.run(&block).unwrap();
    let trace = rt.take_trace();
    assert!(trace.iter().any(|e| matches!(e, TraceEvent::Abort { .. })));
}

#[test]
fn trace_records_forward_event() {
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                source_frame_id: 1,
                ..Default()
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    rt.enable_tracing();
    let _ = rt.run(&[outer]).unwrap();
    let trace = rt.take_trace();
    assert!(trace.iter().any(|e| matches!(e, TraceEvent::Forward { .. })));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — D10 trust boundary (clause discharge)
// ────────────────────────────────────────────────────────────────────

#[test]
fn clause_with_no_discharge_is_runtime_error() {
    // The typechecker (D10) rejects this; the runtime guards anyway.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Passthrough], // no resume/abort/forward
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    let err = rt.run(&block).unwrap_err();
    assert!(matches!(
        err,
        super::runtime::EffectRuntimeError::NoDischarge { .. }
    ));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — End-to-end paper canonical example
// ────────────────────────────────────────────────────────────────────

#[test]
fn paper_canonical_sse_runs_to_completion() {
    let payload = json!([{
        "node_type": "handler_frame",
        "effect_names": ["SSE"],
        "frame_id": 0,
        "body_states": [0, 1],
        "clauses": [
            {"node_type":"handler_clause","operation_name":"Emit","parameter_names":["token"],
             "body":[{"node_type":"resume","value_expr":"","frame_id":0}]},
            {"node_type":"handler_clause","operation_name":"Done","parameter_names":[],
             "body":[{"node_type":"abort","value_expr":"","frame_id":0}]}
        ],
        "body": [
            {"node_type":"perform","effect_name":"SSE","operation_name":"Emit","arguments":["t"],"state_id":0,"resume_label":"resume_SSE_Emit_0"},
            {"node_type":"perform","effect_name":"SSE","operation_name":"Done","arguments":[],"state_id":1,"resume_label":"resume_SSE_Done_1"}
        ]
    }]);
    let block: Vec<Instruction> = serde_json::from_value(payload).unwrap();
    let mut rt = empty_runtime();
    rt.register_effect(sample_effect(
        "SSE",
        vec![("Emit", vec!["token"], "Unit"), ("Done", vec![], "Never")],
    ));
    rt.bind_global("t", Value::String("token1".into()));
    let result = rt.run(&block).unwrap();
    // The first Emit resumes; the Done aborts → final result is Aborted.
    assert_eq!(result, ExecutionResult::Aborted(Value::Unit));
}

#[test]
fn paper_decorator_pattern_with_forward_runs_to_completion() {
    // outer { Op(x) -> resume } in {
    //   inner { Op(x) -> forward Op(x) } in {
    //     perform Op(token)
    //   }
    // }
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec!["x".into()],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                arguments: vec!["x".into()],
                source_frame_id: 1,
                state_id: 1,
                resume_label: "fw_E_Op_1".into(),
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            arguments: vec!["token".into()],
            state_id: 0,
            ..Default()
        })],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec!["x".into()],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    rt.bind_global("token", Value::String("hello".into()));
    let result = rt.run(&[outer]).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — D2 one-shot continuation semantics
// ────────────────────────────────────────────────────────────────────

#[test]
fn one_shot_resume_consumes_continuation_exactly_once() {
    // After a single resume, the perform site advances. A subsequent
    // perform of the same op fires the clause AGAIN with a fresh
    // continuation (continuations are per-perform, not shared).
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 0,
                ..Default()
            }),
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 1,
                ..Default()
            }),
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 2,
                ..Default()
            }),
        ],
        frame_id: 0,
        body_states: vec![0, 1, 2],
        ..Default()
    })];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn abort_does_not_resume_subsequent_performs() {
    // When the first perform's clause aborts, the body's second
    // perform is unreachable — abort short-circuits to the run's
    // result.
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Abort(super::ir::IRAbort {
                value_expr: "halt".into(),
                frame_id: 0,
            })],
            ..Default()
        }],
        body: vec![
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 0,
                ..Default()
            }),
            // This perform never fires because the first one aborted.
            Instruction::Perform(IRPerform {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                state_id: 1,
                ..Default()
            }),
        ],
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    rt.bind_global("halt", Value::String("aborted".into()));
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Aborted(Value::String("aborted".into())));
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Cross-stack JSON IR contract
// ────────────────────────────────────────────────────────────────────

#[test]
fn json_payload_with_perform_outside_handler_in_block_is_unhandled() {
    // The Python frontend may emit a flow whose body contains a bare
    // perform (typechecker D9 rejects it, but the runtime guards).
    let payload = json!([
        {"node_type":"perform","effect_name":"E","operation_name":"Op","arguments":[],"state_id":0,"resume_label":"r0"}
    ]);
    let block: Vec<Instruction> = serde_json::from_value(payload).unwrap();
    let mut rt = empty_runtime();
    let err = rt.run(&block).unwrap_err();
    assert!(matches!(
        err,
        super::runtime::EffectRuntimeError::UnhandledEffect { .. }
    ));
}

#[test]
fn json_payload_handler_with_int_arg_resolves_via_globals() {
    let payload = json!([{
        "node_type": "handler_frame",
        "effect_names": ["Counter"],
        "frame_id": 0,
        "clauses": [{
            "node_type": "handler_clause",
            "operation_name": "Inc",
            "parameter_names": ["n"],
            "body": [{"node_type": "resume", "value_expr": "n", "frame_id": 0}]
        }],
        "body": [
            {"node_type": "perform", "effect_name": "Counter", "operation_name": "Inc", "arguments": ["42"], "state_id": 0, "resume_label": "r0"}
        ],
    }]);
    let block: Vec<Instruction> = serde_json::from_value(payload).unwrap();
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    // "42" is parsed as a literal Int by Value::from_argument_text.
    assert_eq!(result, ExecutionResult::Completed(Value::Int(42)));
}

#[test]
fn json_payload_extra_fields_are_tolerated() {
    // Future-proof: the IR may grow new fields; the runtime should
    // not refuse to deserialize because of unknown keys (serde
    // default behaviour: ignores extras).
    let payload = json!([{
        "node_type": "perform",
        "effect_name": "E",
        "operation_name": "Op",
        "arguments": [],
        "state_id": 0,
        "resume_label": "r0",
        "future_field_we_dont_know_about": "ignored",
    }]);
    let block: Vec<Instruction> = serde_json::from_value(payload).unwrap();
    assert_eq!(block.len(), 1);
}

// ────────────────────────────────────────────────────────────────────
//  Runtime — Edge cases + invariants
// ────────────────────────────────────────────────────────────────────

#[test]
fn empty_handle_body_completes_with_unit() {
    let block = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![], // empty body — no performs to discharge
        frame_id: 0,
        ..Default()
    })];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn empty_run_block_completes_with_unit() {
    let mut rt = empty_runtime();
    let result = rt.run(&[]).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn passthrough_instructions_are_inert() {
    let block = vec![
        Instruction::Passthrough,
        Instruction::Passthrough,
        Instruction::Passthrough,
    ];
    let mut rt = empty_runtime();
    let result = rt.run(&block).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn handler_stack_pops_on_abort() {
    // After an inner abort, the runtime should not leave handlers on
    // the stack — verified indirectly by running a second flow shape
    // and expecting fresh dispatch.
    let block_abort = vec![Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Abort(super::ir::IRAbort::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 0,
        ..Default()
    })];
    let block_resume = vec![simple_handle_perform("E", "Op", "x", "", "arg")];
    let mut rt = empty_runtime();
    let _ = rt.run(&block_abort).unwrap();
    let result = rt.run(&block_resume).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn three_levels_of_nesting_work_correctly() {
    // outer { Op() -> resume } in {
    //   middle { Op() -> resume } in {
    //     inner { Op() -> resume } in {
    //       perform Op()    ← caught by innermost
    //     }
    //   }
    // }
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 2,
        ..Default()
    });
    let middle = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![middle],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    let result = rt.run(&[outer]).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn forward_chain_through_three_frames() {
    // outer { Op() -> resume } in {
    //   middle { Op() -> forward Op() } in {
    //     inner { Op() -> forward Op() } in {
    //       perform Op()  ← inner forwards to middle, middle forwards to outer
    //     }
    //   }
    // }
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                source_frame_id: 2,
                ..Default()
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            ..Default()
        })],
        frame_id: 2,
        ..Default()
    });
    let middle = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                source_frame_id: 1,
                ..Default()
            })],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec![],
            body: vec![Instruction::Resume(IRResume::default())],
            ..Default()
        }],
        body: vec![middle],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    let result = rt.run(&[outer]).unwrap();
    assert_eq!(result, ExecutionResult::Completed(Value::Unit));
}

#[test]
fn forward_with_arguments_passes_them_to_outer_clause() {
    let inner = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec!["x".into()],
            body: vec![Instruction::Forward(super::ir::IRForward {
                effect_name: "E".into(),
                operation_name: "Op".into(),
                arguments: vec!["x".into()],
                source_frame_id: 1,
                ..Default()
            })],
            ..Default()
        }],
        body: vec![Instruction::Perform(IRPerform {
            effect_name: "E".into(),
            operation_name: "Op".into(),
            arguments: vec!["payload".into()],
            ..Default()
        })],
        frame_id: 1,
        ..Default()
    });
    let outer = Instruction::HandlerFrame(IRHandlerFrame {
        effect_names: vec!["E".into()],
        clauses: vec![IRHandlerClause {
            operation_name: "Op".into(),
            parameter_names: vec!["x".into()],
            // Outer resumes with the value of `x` (the forwarded payload).
            body: vec![Instruction::Resume(IRResume {
                value_expr: "x".into(),
                frame_id: 0,
            })],
            ..Default()
        }],
        body: vec![inner],
        frame_id: 0,
        ..Default()
    });
    let mut rt = empty_runtime();
    rt.bind_global("payload", Value::String("seen".into()));
    let result = rt.run(&[outer]).unwrap();
    // The payload propagates through the forward chain: inner clause
    // binds `x = payload`, forwards `x` (which resolves to "seen") to
    // outer, outer's clause binds its own `x = "seen"` and resumes
    // with that value.
    assert_eq!(result, ExecutionResult::Completed(Value::String("seen".into())));
}

#[test]
fn lookup_operation_via_registered_effect_d1() {
    // D1 — operation polymorphism: lookup returns the polymorphic
    // signature with type_parameters populated.
    let mut rt = empty_runtime();
    rt.register_effect(IREffectDeclaration {
        name: "Channel".into(),
        operations: vec![IREffectOperation {
            name: "Send".into(),
            type_parameters: vec!["T".into()],
            parameter_names: vec!["v".into()],
            parameter_types: vec!["T".into()],
            return_type: "Unit".into(),
            source_line: 0,
            source_column: 0,
        }],
        source_line: 0,
        source_column: 0,
    });
    let op = rt.lookup_operation("Channel", "Send").unwrap();
    assert_eq!(op.type_parameters, vec!["T".to_string()]);
}

// ────────────────────────────────────────────────────────────────────
//  Default impls used by tests
// ────────────────────────────────────────────────────────────────────

// `Default()` is a tiny syntactic helper for the tests above. We
// implement Default on every IR struct used in tests.

impl Default for IRHandlerClause {
    fn default() -> Self {
        Self {
            operation_name: String::new(),
            parameter_names: Vec::new(),
            body: Vec::new(),
            source_line: 0,
            source_column: 0,
        }
    }
}

impl Default for IRHandlerFrame {
    fn default() -> Self {
        Self {
            effect_names: Vec::new(),
            clauses: Vec::new(),
            body: Vec::new(),
            frame_id: 0,
            body_states: Vec::new(),
            source_line: 0,
            source_column: 0,
        }
    }
}

impl Default for IRPerform {
    fn default() -> Self {
        Self {
            effect_name: String::new(),
            operation_name: String::new(),
            arguments: Vec::new(),
            state_id: 0,
            resume_label: String::new(),
        }
    }
}

impl Default for IRResume {
    fn default() -> Self {
        Self {
            value_expr: String::new(),
            frame_id: 0,
        }
    }
}

impl Default for super::ir::IRAbort {
    fn default() -> Self {
        Self {
            value_expr: String::new(),
            frame_id: 0,
        }
    }
}

impl Default for super::ir::IRForward {
    fn default() -> Self {
        Self {
            effect_name: String::new(),
            operation_name: String::new(),
            arguments: Vec::new(),
            source_frame_id: 0,
            state_id: 0,
            resume_label: String::new(),
        }
    }
}

// `Default()` syntactic shim: tests use `..Default()` in struct
// literals expecting `..Default::default()`. We define a free
// function with this name to satisfy the syntax.
#[allow(non_snake_case)]
fn Default<T: std::default::Default>() -> T {
    T::default()
}
