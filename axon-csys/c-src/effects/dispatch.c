/*
 * v1.19.0 — Algebraic effects FSM dispatcher (implementation).
 *
 * Direct port of axon-rs/src/effects/runtime.rs delivering paper section 5.
 * See dispatch.h for the architectural commentary.
 */

#include "dispatch.h"

#include <string.h>

/* ───── Detect whether computed gotos are available ──────────────── */

#if defined(__GNUC__) || defined(__clang__)
#  define AXON_CSYS_USE_COMPUTED_GOTOS 1
#else
#  define AXON_CSYS_USE_COMPUTED_GOTOS 0
#endif

bool axon_csys_effects_uses_computed_gotos(void) {
    return (bool)AXON_CSYS_USE_COMPUTED_GOTOS;
}

/* ───── Internal limits ──────────────────────────────────────────── */

#define AXON_CSYS_MAX_EXEC_STACK   256u
#define AXON_CSYS_MAX_HANDLER_STACK 64u
#define AXON_CSYS_MAX_GLOBALS      128u
#define AXON_CSYS_MAX_CLAUSE_PARAMS  8u

/* ───── Exec stack frame ─────────────────────────────────────────── */

typedef enum {
    EXEC_KIND_BLOCK         = 0,  /* top-level run() block */
    EXEC_KIND_HANDLER_BODY  = 1,  /* body of an active handler frame */
    EXEC_KIND_CLAUSE_BODY   = 2,  /* body of a dispatched clause; must discharge */
} ExecFrameKind;

typedef struct {
    AxonCsysStrSlice key;
    AxonCsysValue    prior_value;
    bool             was_bound;
} ParamSave;

typedef struct {
    ExecFrameKind              kind;
    const AxonCsysInstruction *instructions;
    uint32_t                   instruction_count;
    uint32_t                   ip;
    AxonCsysValue              last_value;

    /* For HANDLER_BODY: the index into handler_stack that this body's
     * frame occupies. Pop together. */
    int32_t handler_stack_idx;

    /* For CLAUSE_BODY: the exec stack index of the perform site's
     * parent block. resume() pops down to (target+1) frames and
     * writes last_value into target's slot. */
    int32_t resume_target_idx;

    /* For CLAUSE_BODY: the handler stack index of the frame this
     * clause belongs to. abort() pops down past this frame. */
    int32_t clause_handler_stack_idx;

    /* For CLAUSE_BODY: parameter saves to restore on exit. */
    ParamSave param_saves[AXON_CSYS_MAX_CLAUSE_PARAMS];
    uint32_t  param_save_count;
} ExecFrame;

typedef struct {
    const AxonCsysFrame *frame;
    int32_t              exec_stack_idx;
} HandlerEntry;

typedef struct {
    AxonCsysStrSlice key;
    AxonCsysValue    value;
    bool             occupied;
} GlobalEntry;

typedef struct {
    /* Stacks. */
    ExecFrame    exec_stack[AXON_CSYS_MAX_EXEC_STACK];
    int32_t      exec_top;  /* -1 when empty; else top index */

    HandlerEntry handler_stack[AXON_CSYS_MAX_HANDLER_STACK];
    int32_t      handler_top;

    /* Globals — flat linear-probing table for the typical case
     * (≤ AXON_CSYS_MAX_GLOBALS bindings). Empty entries have
     * `occupied = false`. */
    GlobalEntry  globals[AXON_CSYS_MAX_GLOBALS];
    uint32_t     global_count;

    /* Wire reference + trace ring. */
    const AxonCsysWire *wire;
    AxonCsysTraceEvent *trace_buffer;
    uint32_t            trace_capacity;
    uint32_t            trace_count;
} Dispatcher;

/* ───── Slice + value helpers ────────────────────────────────────── */

static inline bool slice_eq(AxonCsysStrSlice a, AxonCsysStrSlice b) {
    if (a.len != b.len) return false;
    if (a.len == 0) return true;
    return memcmp(a.ptr, b.ptr, a.len) == 0;
}

/* ───── Globals table — linear probing, no rehash ─────────────────── */

static int32_t globals_find(Dispatcher *d, AxonCsysStrSlice key) {
    for (uint32_t i = 0; i < AXON_CSYS_MAX_GLOBALS; ++i) {
        if (d->globals[i].occupied && slice_eq(d->globals[i].key, key)) {
            return (int32_t)i;
        }
    }
    return -1;
}

static int32_t globals_alloc_slot(Dispatcher *d, AxonCsysStrSlice key) {
    /* Reuse if already bound. */
    int32_t existing = globals_find(d, key);
    if (existing >= 0) return existing;
    for (uint32_t i = 0; i < AXON_CSYS_MAX_GLOBALS; ++i) {
        if (!d->globals[i].occupied) {
            d->globals[i].occupied = true;
            d->globals[i].key = key;
            d->globals[i].value = axon_csys_value_unit();
            d->global_count++;
            return (int32_t)i;
        }
    }
    return -1;  /* table full */
}

/* Resolve a Symbol value against globals. Non-Symbol values pass
 * through unchanged. */
static AxonCsysValue resolve_value(Dispatcher *d, AxonCsysValue v) {
    if (v.tag != AXON_CSYS_VAL_SYMBOL) return v;
    int32_t idx = globals_find(d, v.u.s);
    if (idx >= 0) return d->globals[idx].value;
    return v;
}

/* ───── Trace recording (no-op when capacity == 0) ────────────────── */

static inline void record_trace(
    Dispatcher *d, AxonCsysTraceKind kind,
    uint32_t frame_id, uint32_t effect_id, uint32_t operation_id,
    AxonCsysValue value
) {
    if (d->trace_capacity == 0 || d->trace_buffer == NULL) return;
    if (d->trace_count >= d->trace_capacity) return;  /* silently drop excess */
    AxonCsysTraceEvent *e = &d->trace_buffer[d->trace_count++];
    e->kind         = kind;
    e->frame_id     = frame_id;
    e->effect_id    = effect_id;
    e->operation_id = operation_id;
    e->value        = value;
}

/* ───── Stack push helpers ────────────────────────────────────────── */

static bool exec_push(Dispatcher *d, ExecFrame frame) {
    if (d->exec_top + 1 >= (int32_t)AXON_CSYS_MAX_EXEC_STACK) return false;
    d->exec_top++;
    d->exec_stack[d->exec_top] = frame;
    return true;
}

static bool handler_push(Dispatcher *d, HandlerEntry entry) {
    if (d->handler_top + 1 >= (int32_t)AXON_CSYS_MAX_HANDLER_STACK) return false;
    d->handler_top++;
    d->handler_stack[d->handler_top] = entry;
    return true;
}

/* Restore prior parameter bindings recorded on a CLAUSE_BODY frame
 * just before popping it. */
static void restore_clause_params(Dispatcher *d, const ExecFrame *clause_frame) {
    for (uint32_t i = 0; i < clause_frame->param_save_count; ++i) {
        const ParamSave *ps = &clause_frame->param_saves[i];
        int32_t idx = globals_find(d, ps->key);
        if (ps->was_bound) {
            if (idx >= 0) {
                d->globals[idx].value = ps->prior_value;
            } else {
                int32_t slot = globals_alloc_slot(d, ps->key);
                if (slot >= 0) d->globals[slot].value = ps->prior_value;
            }
        } else {
            if (idx >= 0) {
                d->globals[idx].occupied = false;
                d->global_count--;
            }
        }
    }
}

/* ───── Find handler / clause ─────────────────────────────────────── */

/* Search the handler stack from `start_exclusive - 1` down to 0 for
 * the first frame that handles `effect_id`. Returns -1 if none. */
static int32_t find_handler_index(Dispatcher *d, uint32_t effect_id, int32_t start_exclusive) {
    for (int32_t i = start_exclusive - 1; i >= 0; --i) {
        const AxonCsysFrame *f = d->handler_stack[i].frame;
        for (uint32_t j = 0; j < f->effect_count; ++j) {
            if (f->effect_ids[j] == effect_id) return i;
        }
    }
    return -1;
}

static const AxonCsysClause *find_clause(
    const AxonCsysFrame *frame, uint32_t effect_id, uint32_t operation_id
) {
    for (uint32_t i = 0; i < frame->clause_count; ++i) {
        const AxonCsysClause *c = &frame->clauses[i];
        if (c->effect_id == effect_id && c->operation_id == operation_id) return c;
    }
    return NULL;
}

/* ───── Bind clause parameters (with save for restore) ────────────── */

static void bind_clause_params(
    Dispatcher *d,
    ExecFrame *clause_frame,
    const AxonCsysClause *clause,
    const AxonCsysValue *args,
    uint32_t args_count
) {
    /* Bind min(parameter_count, args_count) parameters; the IR
     * guarantees these match (typechecker D9). The dispatcher tolerates
     * mismatch by binding the available prefix. */
    uint32_t bind_count = clause->parameter_count < args_count
                        ? clause->parameter_count : args_count;
    if (bind_count > AXON_CSYS_MAX_CLAUSE_PARAMS) {
        bind_count = AXON_CSYS_MAX_CLAUSE_PARAMS;
    }
    clause_frame->param_save_count = 0;
    for (uint32_t i = 0; i < bind_count; ++i) {
        AxonCsysStrSlice key = d->wire->parameter_name_pool[
            clause->parameter_names_offset + i
        ];
        ParamSave *ps = &clause_frame->param_saves[clause_frame->param_save_count++];
        ps->key = key;
        int32_t existing = globals_find(d, key);
        if (existing >= 0) {
            ps->was_bound = true;
            ps->prior_value = d->globals[existing].value;
            d->globals[existing].value = args[i];
        } else {
            ps->was_bound = false;
            ps->prior_value = axon_csys_value_unit();
            int32_t slot = globals_alloc_slot(d, key);
            if (slot >= 0) d->globals[slot].value = args[i];
        }
    }
}

/* ───── Helpers to short-circuit to a final result ─────────────────── */

static AxonCsysDispatchResult result_completed(AxonCsysValue v) {
    AxonCsysDispatchResult r;
    r.kind = AXON_CSYS_RESULT_COMPLETED;
    r.value = v;
    r.error_code = AXON_CSYS_ERR_NONE;
    r.error_effect_id = UINT32_MAX;
    r.error_operation_id = UINT32_MAX;
    return r;
}

static AxonCsysDispatchResult result_aborted(AxonCsysValue v) {
    AxonCsysDispatchResult r;
    r.kind = AXON_CSYS_RESULT_ABORTED;
    r.value = v;
    r.error_code = AXON_CSYS_ERR_NONE;
    r.error_effect_id = UINT32_MAX;
    r.error_operation_id = UINT32_MAX;
    return r;
}

static AxonCsysDispatchResult result_error(
    AxonCsysErrorCode code, uint32_t effect_id, uint32_t operation_id
) {
    AxonCsysDispatchResult r;
    r.kind = AXON_CSYS_RESULT_ERROR;
    r.value = axon_csys_value_unit();
    r.error_code = code;
    r.error_effect_id = effect_id;
    r.error_operation_id = operation_id;
    return r;
}

/* ═══════════════════════════════════════════════════════════════════════
 *  Main dispatch — computed gotos on gcc/clang, switch on MSVC.
 * ═══════════════════════════════════════════════════════════════════════ */

AxonCsysDispatchResult axon_csys_effects_run(
    const AxonCsysWire *wire,
    const AxonCsysStrSlice *globals_keys,
    const AxonCsysValue    *globals_values,
    uint32_t                globals_count,
    AxonCsysTraceEvent     *trace_buffer,
    uint32_t                trace_capacity,
    uint32_t               *trace_count_out
) {
    if (wire == NULL) {
        if (trace_count_out != NULL) *trace_count_out = 0;
        return result_error(AXON_CSYS_ERR_INTERNAL, UINT32_MAX, UINT32_MAX);
    }

    Dispatcher d;
    memset(&d, 0, sizeof d);
    d.exec_top    = -1;
    d.handler_top = -1;
    d.wire           = wire;
    d.trace_buffer   = trace_buffer;
    d.trace_capacity = trace_capacity;
    d.trace_count    = 0;

    /* Pre-bind globals from the caller. */
    for (uint32_t i = 0; i < globals_count; ++i) {
        int32_t slot = globals_alloc_slot(&d, globals_keys[i]);
        if (slot >= 0) d.globals[slot].value = globals_values[i];
    }

    /* Helper macro: every return path must update trace_count_out
     * before exiting so the caller knows how many trace events
     * were actually written. */
#define RETURN_RESULT(r) do { \
    if (trace_count_out != NULL) *trace_count_out = d.trace_count; \
    return (r); \
} while (0)

    /* Push the top-level block. */
    ExecFrame top = {0};
    top.kind              = EXEC_KIND_BLOCK;
    top.instructions      = wire->instructions;
    top.instruction_count = wire->instruction_count;
    top.ip                = 0;
    top.last_value        = axon_csys_value_unit();
    top.handler_stack_idx        = -1;
    top.resume_target_idx        = -1;
    top.clause_handler_stack_idx = -1;
    if (!exec_push(&d, top)) {
        RETURN_RESULT(result_error(AXON_CSYS_ERR_STACK_OVERFLOW, UINT32_MAX, UINT32_MAX));
    }

#if AXON_CSYS_USE_COMPUTED_GOTOS
    /* Labels-as-values + computed goto are GNU C extensions; under
     * `-Werror=pedantic` gcc/clang would otherwise reject the
     * `&&label` and `goto *expr` syntax. We use them deliberately
     * (paper section 5 — atomic-jump dispatch) and gate behind
     * `AXON_CSYS_USE_COMPUTED_GOTOS` (which is 0 on MSVC where the
     * extension is unavailable). Suppress the pedantic noise for
     * just this block; the switch fallback below stays portable. */
#  pragma GCC diagnostic push
#  pragma GCC diagnostic ignored "-Wpedantic"
    static void *const opcode_labels[AXON_CSYS_OP_COUNT] = {
        [AXON_CSYS_OP_PASSTHROUGH]   = &&label_passthrough,
        [AXON_CSYS_OP_PERFORM]       = &&label_perform,
        [AXON_CSYS_OP_HANDLER_FRAME] = &&label_handler_frame,
        [AXON_CSYS_OP_RESUME]        = &&label_resume,
        [AXON_CSYS_OP_ABORT]         = &&label_abort,
        [AXON_CSYS_OP_FORWARD]       = &&label_forward,
    };
#  pragma GCC diagnostic pop
#endif

    /* ── Main dispatch loop ──────────────────────────────────────── */
dispatch:
    {
        ExecFrame *cur = &d.exec_stack[d.exec_top];

        /* End-of-block handling. */
        if (cur->ip >= cur->instruction_count) {
            switch (cur->kind) {
                case EXEC_KIND_CLAUSE_BODY: {
                    /* Walked off the end without resume/abort/forward.
                     * The typechecker D10 should reject this. */
                    AxonCsysErrorCode err = AXON_CSYS_ERR_NO_DISCHARGE;
                    RETURN_RESULT(result_error(err, UINT32_MAX, UINT32_MAX));
                }
                case EXEC_KIND_HANDLER_BODY: {
                    AxonCsysValue v = cur->last_value;
                    int32_t fid = -1;
                    if (cur->handler_stack_idx >= 0
                        && cur->handler_stack_idx <= d.handler_top) {
                        fid = (int32_t)d.handler_stack[cur->handler_stack_idx]
                                       .frame->frame_id;
                        d.handler_top = cur->handler_stack_idx - 1;
                    }
                    d.exec_top--;
                    if (fid >= 0) {
                        record_trace(&d, AXON_CSYS_TRACE_EXIT_FRAME,
                                     (uint32_t)fid, UINT32_MAX, UINT32_MAX,
                                     axon_csys_value_unit());
                    }
                    if (d.exec_top < 0) RETURN_RESULT(result_completed(v));
                    d.exec_stack[d.exec_top].last_value = v;
                    d.exec_stack[d.exec_top].ip++;
                    goto dispatch;
                }
                case EXEC_KIND_BLOCK: {
                    AxonCsysValue v = cur->last_value;
                    d.exec_top--;
                    if (d.exec_top < 0) RETURN_RESULT(result_completed(v));
                    d.exec_stack[d.exec_top].last_value = v;
                    d.exec_stack[d.exec_top].ip++;
                    goto dispatch;
                }
            }
        }

        const AxonCsysInstruction *instr = &cur->instructions[cur->ip];

#if AXON_CSYS_USE_COMPUTED_GOTOS
        if (instr->opcode >= AXON_CSYS_OP_COUNT) {
            RETURN_RESULT(result_error(AXON_CSYS_ERR_INTERNAL, UINT32_MAX, UINT32_MAX));
        }
        /* Computed-goto dispatch — GNU C extension; suppress
         * `-Wpedantic` for the single statement. */
#  pragma GCC diagnostic push
#  pragma GCC diagnostic ignored "-Wpedantic"
        goto *opcode_labels[instr->opcode];
#  pragma GCC diagnostic pop
#else
        switch (instr->opcode) {
            case AXON_CSYS_OP_PASSTHROUGH:   goto label_passthrough;
            case AXON_CSYS_OP_PERFORM:       goto label_perform;
            case AXON_CSYS_OP_HANDLER_FRAME: goto label_handler_frame;
            case AXON_CSYS_OP_RESUME:        goto label_resume;
            case AXON_CSYS_OP_ABORT:         goto label_abort;
            case AXON_CSYS_OP_FORWARD:       goto label_forward;
            default:
                RETURN_RESULT(result_error(AXON_CSYS_ERR_INTERNAL, UINT32_MAX, UINT32_MAX));
        }
#endif

label_passthrough: {
            cur->ip++;
            goto dispatch;
        }

label_perform: {
            int32_t hidx = find_handler_index(&d, instr->effect_id, d.handler_top + 1);
            if (hidx < 0) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_UNHANDLED_EFFECT,
                                           instr->effect_id, instr->operation_id));
            }
            const AxonCsysFrame *frame = d.handler_stack[hidx].frame;
            const AxonCsysClause *clause =
                find_clause(frame, instr->effect_id, instr->operation_id);
            if (clause == NULL) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_UNKNOWN_OPERATION,
                                           instr->effect_id, instr->operation_id));
            }

            record_trace(&d, AXON_CSYS_TRACE_PERFORM,
                         instr->state_id, instr->effect_id, instr->operation_id,
                         axon_csys_value_unit());

            /* Resolve args (Symbols against current globals). */
            AxonCsysValue resolved_args[AXON_CSYS_MAX_CLAUSE_PARAMS];
            uint32_t arg_n = instr->args_count;
            if (arg_n > AXON_CSYS_MAX_CLAUSE_PARAMS) arg_n = AXON_CSYS_MAX_CLAUSE_PARAMS;
            for (uint32_t i = 0; i < arg_n; ++i) {
                resolved_args[i] = resolve_value(
                    &d, d.wire->arg_pool[instr->args_offset + i]
                );
            }

            int32_t perform_site_idx = d.exec_top;

            ExecFrame clause_frame = {0};
            clause_frame.kind = EXEC_KIND_CLAUSE_BODY;
            clause_frame.instructions = wire->instructions + clause->body_offset;
            clause_frame.instruction_count = clause->body_count;
            clause_frame.ip = 0;
            clause_frame.last_value = axon_csys_value_unit();
            clause_frame.handler_stack_idx = -1;
            clause_frame.resume_target_idx = perform_site_idx;
            clause_frame.clause_handler_stack_idx = hidx;
            clause_frame.param_save_count = 0;
            if (!exec_push(&d, clause_frame)) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_STACK_OVERFLOW,
                                           UINT32_MAX, UINT32_MAX));
            }
            ExecFrame *new_clause_frame = &d.exec_stack[d.exec_top];
            bind_clause_params(&d, new_clause_frame, clause, resolved_args, arg_n);
            goto dispatch;
        }

label_handler_frame: {
            uint32_t frame_idx = instr->args_offset;
            if (frame_idx >= wire->frame_count) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_INTERNAL, UINT32_MAX, UINT32_MAX));
            }
            const AxonCsysFrame *frame = &wire->frames[frame_idx];
            HandlerEntry he = { .frame = frame, .exec_stack_idx = d.exec_top + 1 };
            if (!handler_push(&d, he)) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_STACK_OVERFLOW,
                                           UINT32_MAX, UINT32_MAX));
            }
            int32_t hidx = d.handler_top;

            record_trace(&d, AXON_CSYS_TRACE_ENTER_FRAME,
                         frame->frame_id, UINT32_MAX, UINT32_MAX,
                         axon_csys_value_unit());

            ExecFrame body = {0};
            body.kind                     = EXEC_KIND_HANDLER_BODY;
            body.instructions             = wire->instructions + frame->body_offset;
            body.instruction_count        = frame->body_count;
            body.ip                       = 0;
            body.last_value               = axon_csys_value_unit();
            body.handler_stack_idx        = hidx;
            body.resume_target_idx        = -1;
            body.clause_handler_stack_idx = -1;
            body.param_save_count         = 0;
            if (!exec_push(&d, body)) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_STACK_OVERFLOW,
                                           UINT32_MAX, UINT32_MAX));
            }
            goto dispatch;
        }

label_resume: {
            /* `cur` MUST be a CLAUSE_BODY frame. The typechecker D10
             * should reject Resume in any other position; we surface
             * a CONTROL_OPCODE_OUT_OF_BODY error if it appears in a
             * non-clause block. */
            if (cur->kind != EXEC_KIND_CLAUSE_BODY) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_CONTROL_OPCODE_OUT_OF_BODY,
                                           UINT32_MAX, UINT32_MAX));
            }
            AxonCsysValue v = (instr->args_count > 0)
                ? resolve_value(&d, d.wire->arg_pool[instr->args_offset])
                : axon_csys_value_unit();

            record_trace(&d, AXON_CSYS_TRACE_RESUME,
                         instr->frame_id, UINT32_MAX, UINT32_MAX, v);

            int32_t target_idx = cur->resume_target_idx;
            restore_clause_params(&d, cur);
            d.exec_top--;  /* pop the clause body */

            if (target_idx < 0 || target_idx > d.exec_top) {
                /* Should not happen for well-formed IR. */
                RETURN_RESULT(result_error(AXON_CSYS_ERR_INTERNAL, UINT32_MAX, UINT32_MAX));
            }
            d.exec_stack[target_idx].last_value = v;
            d.exec_stack[target_idx].ip++;
            goto dispatch;
        }

label_abort: {
            if (cur->kind != EXEC_KIND_CLAUSE_BODY) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_CONTROL_OPCODE_OUT_OF_BODY,
                                           UINT32_MAX, UINT32_MAX));
            }
            AxonCsysValue v = (instr->args_count > 0)
                ? resolve_value(&d, d.wire->arg_pool[instr->args_offset])
                : axon_csys_value_unit();

            record_trace(&d, AXON_CSYS_TRACE_ABORT,
                         instr->frame_id, UINT32_MAX, UINT32_MAX, v);

            int32_t handler_idx = cur->clause_handler_stack_idx;
            restore_clause_params(&d, cur);

            /* Pop down to (and including) the handler frame's body. */
            if (handler_idx < 0 || handler_idx > d.handler_top) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_INTERNAL, UINT32_MAX, UINT32_MAX));
            }
            int32_t handler_body_exec_idx = d.handler_stack[handler_idx].exec_stack_idx;

            /* Emit EXIT_FRAME for the handler we're popping. */
            uint32_t fid = d.handler_stack[handler_idx].frame->frame_id;
            record_trace(&d, AXON_CSYS_TRACE_EXIT_FRAME,
                         fid, UINT32_MAX, UINT32_MAX, axon_csys_value_unit());

            /* Pop exec frames down to handler_body_exec_idx - 1
             * (one below the handler body — the parent block of the
             * handler frame). The clause itself was already popped
             * by the d.exec_top-- below. */
            d.exec_top = handler_body_exec_idx - 1;
            d.handler_top = handler_idx - 1;

            if (d.exec_top < 0) {
                /* Top-level abort. */
                RETURN_RESULT(result_aborted(v));
            }
            d.exec_stack[d.exec_top].last_value = v;
            d.exec_stack[d.exec_top].ip++;
            goto dispatch;
        }

label_forward: {
            if (cur->kind != EXEC_KIND_CLAUSE_BODY) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_CONTROL_OPCODE_OUT_OF_BODY,
                                           UINT32_MAX, UINT32_MAX));
            }
            int32_t source_handler_idx = cur->clause_handler_stack_idx;

            record_trace(&d, AXON_CSYS_TRACE_FORWARD,
                         (uint32_t)source_handler_idx,
                         instr->effect_id, instr->operation_id,
                         axon_csys_value_unit());

            int32_t outer_idx = find_handler_index(&d, instr->effect_id,
                                                   source_handler_idx);
            if (outer_idx < 0) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_FORWARD_WITHOUT_OUTER,
                                           instr->effect_id, instr->operation_id));
            }
            const AxonCsysFrame *outer_frame = d.handler_stack[outer_idx].frame;
            const AxonCsysClause *outer_clause =
                find_clause(outer_frame, instr->effect_id, instr->operation_id);
            if (outer_clause == NULL) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_UNKNOWN_OPERATION,
                                           instr->effect_id, instr->operation_id));
            }

            /* Resolve forward args (Symbols against globals BEFORE
             * we restore the source clause's bindings). */
            AxonCsysValue resolved_args[AXON_CSYS_MAX_CLAUSE_PARAMS];
            uint32_t arg_n = instr->args_count;
            if (arg_n > AXON_CSYS_MAX_CLAUSE_PARAMS) arg_n = AXON_CSYS_MAX_CLAUSE_PARAMS;
            for (uint32_t i = 0; i < arg_n; ++i) {
                resolved_args[i] = resolve_value(
                    &d, d.wire->arg_pool[instr->args_offset + i]
                );
            }

            /* The new clause's resume_target stays the same as the
             * source clause's — resume in the outer clause should
             * land at the original perform site. */
            int32_t new_resume_target = cur->resume_target_idx;

            /* Restore + pop source clause. */
            restore_clause_params(&d, cur);
            d.exec_top--;

            /* Push outer clause body. */
            ExecFrame outer_clause_frame = {0};
            outer_clause_frame.kind = EXEC_KIND_CLAUSE_BODY;
            outer_clause_frame.instructions = wire->instructions + outer_clause->body_offset;
            outer_clause_frame.instruction_count = outer_clause->body_count;
            outer_clause_frame.ip = 0;
            outer_clause_frame.last_value = axon_csys_value_unit();
            outer_clause_frame.handler_stack_idx = -1;
            outer_clause_frame.resume_target_idx = new_resume_target;
            outer_clause_frame.clause_handler_stack_idx = outer_idx;
            outer_clause_frame.param_save_count = 0;
            if (!exec_push(&d, outer_clause_frame)) {
                RETURN_RESULT(result_error(AXON_CSYS_ERR_STACK_OVERFLOW,
                                           UINT32_MAX, UINT32_MAX));
            }
            ExecFrame *new_outer_frame = &d.exec_stack[d.exec_top];
            bind_clause_params(&d, new_outer_frame, outer_clause,
                               resolved_args, arg_n);
            goto dispatch;
        }
    }

    /* Unreachable — every dispatch path either RETURN_RESULTs or
     * `goto dispatch`s back to the loop top. The compiler hint below
     * tells MSVC + gcc + clang that this point cannot be reached, so
     * the function does not need a trailing return statement and
     * -Werror won't trip on C4702 / -Wreturn-type. If a future
     * refactor breaks the loop invariant, runtime behaviour is
     * undefined — but the dispatch loop is small enough that the
     * invariant is auditable in one read. */
#if defined(_MSC_VER) && !defined(__clang__)
    __assume(0);
#elif defined(__GNUC__) || defined(__clang__)
    __builtin_unreachable();
#endif

#undef RETURN_RESULT
}
