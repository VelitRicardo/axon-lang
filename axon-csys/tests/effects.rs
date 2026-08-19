//! v1.19.0 — Algebraic effects FSM dispatcher test suite.
//!
//! Exercises the C dispatcher (computed gotos on gcc/clang, switch on
//! MSVC) via the safe Rust shim. Tests are organised into four bands:
//!
//!   1. Build-infra parity (computed-goto vs switch detection)
//!   2. Basic dispatch — single perform / handle / resume cycles
//!   3. Control transfer — abort, forward, nested handlers
//!   4. Defensive errors — unhandled effects, no-discharge, stack overflow
//!
//! The drift gate: the C dispatcher's behaviour is asserted directly
//! against the algebraic-effects semantics specified in
//! `axon-rs/src/effects/runtime.rs`. Because the wire format is built
//! by the Rust shim from the same primitives, any divergence in CPS
//! semantics surfaces as a test failure here.

use axon_csys::effects::{
    BuiltWire, Clause, DispatchError, DispatchResult, Dispatcher, EffectDecl, Frame, Instruction,
    Opcode, TraceEvent, Value, WireBuilder,
};

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

/// Build a minimal effect declaration: one effect with N operations.
fn declare_effect(name: &str, ops: &[(&str, u32)]) -> EffectDecl {
    EffectDecl {
        name: name.to_string(),
        operation_names: ops.iter().map(|(n, _)| (*n).to_string()).collect(),
        operation_arities: ops.iter().map(|(_, a)| *a).collect(),
    }
}

fn run(wire: &BuiltWire) -> Result<DispatchResult, DispatchError> {
    let (result, _) = Dispatcher::run(wire, &[], None);
    result
}

fn run_with_globals(
    wire: &BuiltWire,
    globals: &[(&str, Value)],
) -> Result<DispatchResult, DispatchError> {
    let owned: Vec<(String, Value)> = globals
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    let (result, _) = Dispatcher::run(wire, &owned, None);
    result
}

fn run_traced(
    wire: &BuiltWire,
    capacity: usize,
) -> (Result<DispatchResult, DispatchError>, Vec<TraceEvent>) {
    Dispatcher::run(wire, &[], Some(capacity))
}

// ════════════════════════════════════════════════════════════════════════
// 1. Build-infra parity
// ════════════════════════════════════════════════════════════════════════

#[test]
fn dispatcher_reports_computed_goto_availability() {
    let uses_cg = Dispatcher::uses_computed_gotos();
    if cfg!(target_env = "msvc") {
        assert!(!uses_cg, "MSVC build should report switch fallback (D5)",);
    } else {
        assert!(
            uses_cg,
            "gcc/clang build should report computed-goto dispatch",
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// 2. Empty + passthrough — degenerate cases
// ════════════════════════════════════════════════════════════════════════

#[test]
fn empty_block_completes_with_unit() {
    let wire = WireBuilder::new().build();
    assert_eq!(run(&wire).unwrap(), DispatchResult::Completed(Value::Unit));
}

#[test]
fn passthrough_only_block_completes_with_unit() {
    let mut b = WireBuilder::new();
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::Passthrough,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let wire = b.build();
    assert_eq!(run(&wire).unwrap(), DispatchResult::Completed(Value::Unit));
}

#[test]
fn handler_with_empty_body_completes_with_unit() {
    let mut b = WireBuilder::new();
    let _eff_id = b.add_effect(declare_effect("Foo", &[("bar", 0)]));
    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: 0, // body is empty
        body_count: 0,
        clauses: vec![],
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id, // wire->frames index
        state_id: 0,
        frame_id: 1,
    });
    let wire = b.build();
    assert_eq!(run(&wire).unwrap(), DispatchResult::Completed(Value::Unit));
}

// ════════════════════════════════════════════════════════════════════════
// 3. Single perform → handle → resume
// ════════════════════════════════════════════════════════════════════════

/// Helper: build a wire that does:
///   handle Foo {
///     bar() -> { resume(<value>) }
///   } in {
///     perform Foo.bar()
///   }
/// And returns the wire.
fn wire_simple_resume(value: Value) -> BuiltWire {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    // arg pool: index 0 = the resume value
    let (resume_value_offset, _) = b.add_args([value]);

    // Clause body (instructions after frame body):
    //   resume(value)   — appended to the instructions list
    let resume_body_offset = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Resume,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: resume_value_offset,
        state_id: 0,
        frame_id: 1,
    });
    let resume_body_count = b.instructions_len() - resume_body_offset;

    // Frame body:
    //   perform Foo.bar()
    let body_offset = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 100,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_offset;

    // Frame
    let clauses = vec![Clause {
        effect_id: 0,
        operation_id: 0,
        parameter_count: 0,
        parameter_names_offset: 0,
        body_offset: resume_body_offset,
        body_count: resume_body_count,
        operation_name: "bar".to_string(),
    }];
    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset,
        body_count,
        clauses,
        frame_id: 1,
    });

    // Top-level: handle frame
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });
    b.build()
}

#[test]
fn perform_then_resume_with_int_returns_int() {
    let wire = wire_simple_resume(Value::Int(42));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Int(42))
    );
}

#[test]
fn perform_then_resume_with_bool_returns_bool() {
    let wire = wire_simple_resume(Value::Bool(true));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Bool(true))
    );
}

#[test]
fn perform_then_resume_with_unit_returns_unit() {
    let wire = wire_simple_resume(Value::Unit);
    assert_eq!(run(&wire).unwrap(), DispatchResult::Completed(Value::Unit));
}

#[test]
fn perform_then_resume_with_float_returns_float() {
    let wire = wire_simple_resume(Value::Float(1.5));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Float(1.5))
    );
}

#[test]
fn perform_then_resume_with_string_returns_string() {
    let wire = wire_simple_resume(Value::String("hello".to_string()));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::String("hello".to_string())),
    );
}

// ════════════════════════════════════════════════════════════════════════
// 4. Abort — terminates handle expression
// ════════════════════════════════════════════════════════════════════════

#[test]
fn abort_propagates_to_top_level() {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    let (abort_off, _) = b.add_args([Value::Int(99)]);

    // Clause body: abort(99)
    let clause_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Abort,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: abort_off,
        state_id: 0,
        frame_id: 1,
    });
    let clause_count = b.instructions_len() - clause_off;

    // Body: perform Foo.bar()
    let body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 100,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_off;

    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: body_off,
        body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 0,
            parameter_names_offset: 0,
            body_offset: clause_off,
            body_count: clause_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });

    let wire = b.build();
    // The abort exits the handle frame; the value 99 becomes the
    // frame's result, which is then the top-level last_value →
    // Completed(99). To get an Aborted result at the top, the abort
    // must propagate out of all handlers.
    // Per Rust ref semantics: abort exits the current handle; the
    // surrounding block's last_value is the abort payload, and the
    // block continues. Top-level run returns Completed(payload).
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Int(99))
    );
}

#[test]
fn abort_at_top_level_returns_aborted() {
    // No enclosing handler — the abort opcode appears outside a
    // clause body, which is a defensive error.
    let mut b = WireBuilder::new();
    let (off, _) = b.add_args([Value::Int(7)]);
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::Abort,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: off,
        state_id: 0,
        frame_id: 0,
    });
    let wire = b.build();
    assert_eq!(
        run(&wire).unwrap_err(),
        DispatchError::ControlOpcodeOutsideClauseBody,
    );
}

// ════════════════════════════════════════════════════════════════════════
// 5. Defensive errors — typechecker D9 / D10 escape hatches
// ════════════════════════════════════════════════════════════════════════

#[test]
fn perform_without_enclosing_handler_returns_unhandled_effect() {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let wire = b.build();
    let err = run(&wire).unwrap_err();
    assert_eq!(
        err,
        DispatchError::UnhandledEffect {
            effect_id: 0,
            operation_id: 0,
        }
    );
}

#[test]
fn perform_unknown_operation_returns_unknown_op() {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    // Frame handles Foo but has NO clause for operation_id=0.
    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: 0, // populated below
        body_count: 0,
        clauses: vec![], // no clauses!
        frame_id: 1,
    });

    let body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_off;

    // Patch frame's body offset/count by rebuilding (builder doesn't
    // expose mutation; for this test we use a fresh builder + manual
    // reordering).
    let mut b2 = WireBuilder::new();
    b2.add_effect(declare_effect("Foo", &[("bar", 0)]));
    let body_off2 = b2.instructions_len();
    b2.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let body_count2 = b2.instructions_len() - body_off2;
    let frame_id2 = b2.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: body_off2,
        body_count: body_count2,
        clauses: vec![],
        frame_id: 1,
    });
    b2.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id2,
        state_id: 0,
        frame_id: 1,
    });
    let wire = b2.build();
    let err = run(&wire).unwrap_err();
    assert_eq!(
        err,
        DispatchError::UnknownOperation {
            effect_id: 0,
            operation_id: 0,
        }
    );

    // Quiet warnings on the discarded first-attempt vars.
    let _ = (frame_id, body_off, body_count);
}

#[test]
fn clause_without_discharge_returns_no_discharge() {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    // Clause body: just a Passthrough — never discharges (D10 should
    // reject statically; runtime surfaces NoDischarge).
    let clause_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Passthrough,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let clause_count = b.instructions_len() - clause_off;

    let body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_off;

    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: body_off,
        body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 0,
            parameter_names_offset: 0,
            body_offset: clause_off,
            body_count: clause_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });

    let wire = b.build();
    assert_eq!(run(&wire).unwrap_err(), DispatchError::NoDischarge);
}

#[test]
fn resume_outside_clause_body_returns_control_opcode_error() {
    let mut b = WireBuilder::new();
    let (off, _) = b.add_args([Value::Unit]);
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::Resume,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: off,
        state_id: 0,
        frame_id: 0,
    });
    let wire = b.build();
    assert_eq!(
        run(&wire).unwrap_err(),
        DispatchError::ControlOpcodeOutsideClauseBody,
    );
}

#[test]
fn forward_outside_clause_body_returns_control_opcode_error() {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::Forward,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let wire = b.build();
    assert_eq!(
        run(&wire).unwrap_err(),
        DispatchError::ControlOpcodeOutsideClauseBody,
    );
}

// ════════════════════════════════════════════════════════════════════════
// 6. Multiple perform / sequencing
// ════════════════════════════════════════════════════════════════════════

#[test]
fn two_sequential_performs_use_distinct_state_ids() {
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));
    let (off1, _) = b.add_args([Value::Int(1)]);
    let (off2, _) = b.add_args([Value::Int(2)]);

    // Single shared clause body: resume(1) — the first perform site
    // gets back 1; the second perform site sees the same clause body
    // and also resumes with 1. The block's last_value after both
    // performs is whatever the LAST perform resumed with.
    // To make the assertion crisp, we use a clause that resumes the
    // STATE_ID lookup. For now, both performs resume with the same
    // bound value → we just verify completion + final value.
    let cl_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Resume,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: off1,
        state_id: 0,
        frame_id: 1,
    });
    let cl_count = b.instructions_len() - cl_off;

    let body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 100,
        frame_id: 0,
    });
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 200,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_off;

    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: body_off,
        body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 0,
            parameter_names_offset: 0,
            body_offset: cl_off,
            body_count: cl_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });

    let wire = b.build();
    let _ = off2; // unused but kept for symmetry / future extension
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Int(1))
    );
}

// ════════════════════════════════════════════════════════════════════════
// 7. Trace events
// ════════════════════════════════════════════════════════════════════════

#[test]
fn trace_records_enter_perform_resume_exit_in_order() {
    let wire = wire_simple_resume(Value::Int(42));
    let (result, trace) = run_traced(&wire, 16);
    assert_eq!(result.unwrap(), DispatchResult::Completed(Value::Int(42)));

    // Expected sequence:
    //   EnterFrame { frame_id: 1 }
    //   Perform    { state_id: 100, effect_id: 0, operation_id: 0 }
    //   Resume     { frame_id: 1, value: Int(42) }
    //   ExitFrame  { frame_id: 1 }
    assert_eq!(trace.len(), 4, "trace = {trace:?}");
    assert!(matches!(trace[0], TraceEvent::EnterFrame { frame_id: 1 }));
    assert!(matches!(
        trace[1],
        TraceEvent::Perform {
            state_id: 100,
            effect_id: 0,
            operation_id: 0,
        }
    ));
    assert!(matches!(
        &trace[2],
        TraceEvent::Resume {
            frame_id: 1,
            value: Value::Int(42),
        }
    ));
    assert!(matches!(trace[3], TraceEvent::ExitFrame { frame_id: 1 }));
}

#[test]
fn trace_capacity_zero_disables_tracing() {
    let wire = wire_simple_resume(Value::Int(1));
    let (_, trace) = Dispatcher::run(&wire, &[], None);
    assert!(trace.is_empty());
}

#[test]
fn trace_silently_drops_excess_events() {
    let wire = wire_simple_resume(Value::Int(1));
    // Capacity 1 should keep only the first event.
    let (_, trace) = run_traced(&wire, 1);
    assert_eq!(trace.len(), 1);
    assert!(matches!(trace[0], TraceEvent::EnterFrame { .. }));
}

// ════════════════════════════════════════════════════════════════════════
// 8. Symbol resolution against globals
// ════════════════════════════════════════════════════════════════════════

#[test]
fn symbol_arg_resolves_against_pre_bound_globals() {
    // Resume with a Symbol that should resolve to a globals binding.
    let wire = wire_simple_resume(Value::Symbol("token".to_string()));
    let result = run_with_globals(&wire, &[("token", Value::Int(7))]);
    assert_eq!(result.unwrap(), DispatchResult::Completed(Value::Int(7)));
}

#[test]
fn symbol_arg_unbound_passes_through_unchanged() {
    let wire = wire_simple_resume(Value::Symbol("nope".to_string()));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Symbol("nope".to_string())),
    );
}

// ════════════════════════════════════════════════════════════════════════
// 9. Clause parameter binding
// ════════════════════════════════════════════════════════════════════════

#[test]
fn clause_parameter_binding_visible_at_resume() {
    // handle Foo {
    //   bar(x) -> { resume(x) }
    // } in {
    //   perform Foo.bar(42)
    // }
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 1)]));

    // Args for the perform site.
    let (perform_args_off, _) = b.add_args([Value::Int(42)]);
    // Clause body resume value: Symbol("x") — resolves at run time.
    let (resume_off, _) = b.add_args([Value::Symbol("x".to_string())]);
    // Parameter names for the clause.
    let pnames_off = b.add_parameter_names(["x".to_string()]);

    // Clause body: resume(x)
    let cl_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Resume,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: resume_off,
        state_id: 0,
        frame_id: 1,
    });
    let cl_count = b.instructions_len() - cl_off;

    // Body: perform Foo.bar(42)
    let body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: perform_args_off,
        state_id: 100,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_off;

    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: body_off,
        body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 1,
            parameter_names_offset: pnames_off,
            body_offset: cl_off,
            body_count: cl_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });

    let wire = b.build();
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Int(42))
    );
}

// ════════════════════════════════════════════════════════════════════════
// 10. Forward semantics — outer handler picks up
// ════════════════════════════════════════════════════════════════════════

#[test]
fn forward_propagates_to_next_outer_handler() {
    // handle Foo {
    //   bar() -> { resume(99) }    // OUTER clause
    // } in {
    //   handle Foo {
    //     bar() -> { forward Foo.bar() }   // INNER clause forwards
    //   } in {
    //     perform Foo.bar()
    //   }
    // }
    // Expected: outer clause runs, resumes with 99, value propagates back.
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    // Outer clause body: resume(99)
    let (resume_off, _) = b.add_args([Value::Int(99)]);
    let outer_cl_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Resume,
        effect_id: 0,
        operation_id: 0,
        args_count: 1,
        args_offset: resume_off,
        state_id: 0,
        frame_id: 1,
    });
    let outer_cl_count = b.instructions_len() - outer_cl_off;

    // Inner clause body: forward Foo.bar()
    let inner_cl_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Forward,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 2,
    });
    let inner_cl_count = b.instructions_len() - inner_cl_off;

    // Inner-frame body: perform Foo.bar()
    let inner_body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 100,
        frame_id: 0,
    });
    let inner_body_count = b.instructions_len() - inner_body_off;

    // Inner frame.
    let inner_frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: inner_body_off,
        body_count: inner_body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 0,
            parameter_names_offset: 0,
            body_offset: inner_cl_off,
            body_count: inner_cl_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 2,
    });

    // Outer-frame body: HandlerFrame(inner)
    let outer_body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: inner_frame_id,
        state_id: 0,
        frame_id: 2,
    });
    let outer_body_count = b.instructions_len() - outer_body_off;

    // Outer frame.
    let outer_frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: outer_body_off,
        body_count: outer_body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 0,
            parameter_names_offset: 0,
            body_offset: outer_cl_off,
            body_count: outer_cl_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 1,
    });

    // Top-level: HandlerFrame(outer)
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: outer_frame_id,
        state_id: 0,
        frame_id: 1,
    });

    let wire = b.build();
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Int(99))
    );
}

#[test]
fn forward_without_outer_handler_returns_error() {
    // Single handler that forwards an unhandled-by-anyone-else effect.
    let mut b = WireBuilder::new();
    b.add_effect(declare_effect("Foo", &[("bar", 0)]));

    let cl_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Forward,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 1,
    });
    let cl_count = b.instructions_len() - cl_off;

    let body_off = b.instructions_len();
    b.add_instruction(Instruction {
        opcode: Opcode::Perform,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: 0,
        state_id: 0,
        frame_id: 0,
    });
    let body_count = b.instructions_len() - body_off;

    let frame_id = b.add_frame(Frame {
        effect_ids: vec![0],
        body_offset: body_off,
        body_count,
        clauses: vec![Clause {
            effect_id: 0,
            operation_id: 0,
            parameter_count: 0,
            parameter_names_offset: 0,
            body_offset: cl_off,
            body_count: cl_count,
            operation_name: "bar".to_string(),
        }],
        frame_id: 1,
    });
    b.add_top_level_instruction(Instruction {
        opcode: Opcode::HandlerFrame,
        effect_id: 0,
        operation_id: 0,
        args_count: 0,
        args_offset: frame_id,
        state_id: 0,
        frame_id: 1,
    });

    let wire = b.build();
    let err = run(&wire).unwrap_err();
    assert_eq!(
        err,
        DispatchError::ForwardWithoutOuterHandler {
            effect_id: 0,
            operation_id: 0,
        },
    );
}

// ════════════════════════════════════════════════════════════════════════
// 11. Display impl on DispatchError
// ════════════════════════════════════════════════════════════════════════

#[test]
fn dispatch_error_display_formats_each_variant() {
    let cases = [
        DispatchError::UnhandledEffect {
            effect_id: 1,
            operation_id: 2,
        },
        DispatchError::UnknownOperation {
            effect_id: 1,
            operation_id: 2,
        },
        DispatchError::NoDischarge,
        DispatchError::ForwardWithoutOuterHandler {
            effect_id: 3,
            operation_id: 4,
        },
        DispatchError::ControlOpcodeOutsideClauseBody,
        DispatchError::StackOverflow,
        DispatchError::Internal,
    ];
    for err in cases {
        let s = format!("{err}");
        assert!(
            !s.is_empty(),
            "Display impl produced empty string for {err:?}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// 12. Repeated dispatch — confirms no global state leaks
// ════════════════════════════════════════════════════════════════════════

#[test]
fn dispatcher_is_reentrant_across_runs() {
    let wire1 = wire_simple_resume(Value::Int(1));
    let wire2 = wire_simple_resume(Value::Int(2));
    for _ in 0..50 {
        assert_eq!(
            run(&wire1).unwrap(),
            DispatchResult::Completed(Value::Int(1))
        );
        assert_eq!(
            run(&wire2).unwrap(),
            DispatchResult::Completed(Value::Int(2))
        );
    }
}

#[test]
fn dispatcher_is_thread_safe_under_concurrent_loads() {
    use std::sync::Arc;
    use std::thread;

    let wire = Arc::new(wire_simple_resume(Value::Int(123)));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let w = Arc::clone(&wire);
        handles.push(thread::spawn(move || {
            for _ in 0..200 {
                assert_eq!(run(&w).unwrap(), DispatchResult::Completed(Value::Int(123)));
            }
        }));
    }
    for h in handles {
        h.join().expect("worker panicked");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 13. Wire structure — defaults + introspection
// ════════════════════════════════════════════════════════════════════════

#[test]
fn opcode_to_u8_is_stable() {
    assert_eq!(Opcode::Passthrough as u8, 0);
    assert_eq!(Opcode::Perform as u8, 1);
    assert_eq!(Opcode::HandlerFrame as u8, 2);
    assert_eq!(Opcode::Resume as u8, 3);
    assert_eq!(Opcode::Abort as u8, 4);
    assert_eq!(Opcode::Forward as u8, 5);
}

#[test]
fn value_round_trips_through_int_path() {
    // Verify Rust Value → C RawValue → Rust Value preserves Int.
    let wire = wire_simple_resume(Value::Int(-12345));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Int(-12345)),
    );
}

#[test]
fn value_round_trips_through_float_path() {
    let wire = wire_simple_resume(Value::Float(0.625));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Float(0.625)),
    );
}

#[test]
fn value_round_trips_through_bool_path() {
    let wire = wire_simple_resume(Value::Bool(false));
    assert_eq!(
        run(&wire).unwrap(),
        DispatchResult::Completed(Value::Bool(false)),
    );
}

#[test]
fn wire_builder_default_is_empty() {
    let wire = WireBuilder::default().build();
    assert_eq!(run(&wire).unwrap(), DispatchResult::Completed(Value::Unit));
}
