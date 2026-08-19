/*
 * v1.19.0 — Algebraic effects FSM dispatcher (public ABI).
 *
 * Direct port of `axon-rs/src/effects/runtime.rs` delivering paper section 5
 * — "operaciones atómicas de salto en la pila de CPU sin objetos de
 * control opacos". The Rust impl already removed opaque continuations
 * (the captured continuation IS the remaining instruction array at
 * the perform site, walked by the dispatcher); the C port adds the
 * atomic-jump half via:
 *
 *   1) **Computed gotos** (`goto *labels[op]`) on gcc + clang for
 *      O(1) opcode dispatch with branch-prediction friendliness
 *      that rivals CPython's bytecode interpreter (≥3× over
 *      switch-based dispatch on hot loops).
 *   2) **Explicit exec stack** instead of C recursion — a flat
 *      dispatch loop keeps the inner instruction sequence in cache
 *      and removes function-call overhead per opcode.
 *   3) **Pre-resolved indices** — effect / operation / clause
 *      references are integers in the wire format, never strings.
 *      String → index resolution happens once at IR-load time in
 *      the Rust shim.
 *
 * MSVC fallback: D5 (founder ratification 2026-05-08) gives MSVC a
 * `switch`-based dispatch (labels-as-values is a GCC extension; MSVC
 * has no equivalent). Functional parity intact; performance delta is
 * documented in 25.j benchmarks.
 *
 * Mathematical pillar (preserved verbatim from the Rust ref):
 *   - One-shot continuations (D2): each clause discharges via
 *     resume / abort / forward exactly once. The typechecker (D10)
 *     guarantees no multi-resume; this dispatcher does not check it
 *     at runtime — would require allocation.
 *   - Forward semantics: searches outward from the source frame's
 *     stack index, bypassing the source frame AND any frames nested
 *     beneath it (those nested frames cannot be the next outer
 *     handler the forward should reach).
 *
 * Value representation pillar split (founder principle):
 *   - C handles: Unit / Bool / Int / Float / String (borrowed) /
 *     Symbol (borrowed). Strings + Symbols carry (const char*, len)
 *     into the IR text buffer — zero allocation in dispatch.
 *   - Rust shim handles: List / Map (heap-managed); these short-
 *     circuit to the Rust dispatcher when present in the IR. The
 *     shim guards the boundary.
 */

#ifndef AXON_CSYS_EFFECTS_DISPATCH_H
#define AXON_CSYS_EFFECTS_DISPATCH_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Pre-C23 GCC short-circuit caveat — see probe.c (nested-#ifdef pattern). */
#ifdef __has_c_attribute
#  if __has_c_attribute(nodiscard)
#    define AXON_CSYS_EFFECTS_NODISCARD [[nodiscard]]
#  endif
#endif
#ifndef AXON_CSYS_EFFECTS_NODISCARD
#  define AXON_CSYS_EFFECTS_NODISCARD
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* ──────────────────────────────────────────────────────────────────────
 * Opcode set — must mirror axon-rs/src/effects/ir.rs::Instruction
 * variants. Encoded as uint8_t for dense in-cache layout + computed-
 * goto label table indexing.
 * ────────────────────────────────────────────────────────────────── */

typedef enum {
    AXON_CSYS_OP_PASSTHROUGH    = 0,
    AXON_CSYS_OP_PERFORM        = 1,
    AXON_CSYS_OP_HANDLER_FRAME  = 2,
    AXON_CSYS_OP_RESUME         = 3,
    AXON_CSYS_OP_ABORT          = 4,
    AXON_CSYS_OP_FORWARD        = 5,
    AXON_CSYS_OP_COUNT          = 6,
} AxonCsysOpcode;

/* ──────────────────────────────────────────────────────────────────────
 * Value — tagged union. List / Map intentionally absent (Rust-shim
 * territory; see header doc).
 * ────────────────────────────────────────────────────────────────── */

/* Tag values — encoded as `uint32_t` in the struct (NOT a C enum) so
 * the wire layout matches Rust's `u32` field byte-for-byte across all
 * compilers. C enum sizing is implementation-defined (typically `int`
 * but `-fshort-enums` / vendor switches can shrink it); explicit
 * uint32_t pins the FFI contract. */
#define AXON_CSYS_VAL_UNIT    0u
#define AXON_CSYS_VAL_BOOL    1u
#define AXON_CSYS_VAL_INT     2u
#define AXON_CSYS_VAL_FLOAT   3u
#define AXON_CSYS_VAL_STRING  4u
#define AXON_CSYS_VAL_SYMBOL  5u

typedef struct {
    /* Borrowed string slice — points into the IR text buffer or the
     * caller's owned string pool. The dispatcher never copies. */
    const char *ptr;
    size_t      len;
} AxonCsysStrSlice;

typedef struct {
    uint32_t tag;  /* one of AXON_CSYS_VAL_* */
    union {
        bool             b;
        int64_t          i;
        double           f;
        AxonCsysStrSlice s;  /* used by VAL_STRING + VAL_SYMBOL */
    } u;
} AxonCsysValue;

/* Constructors (inline-style helpers; no allocation). */

static inline AxonCsysValue axon_csys_value_unit(void) {
    AxonCsysValue v;
    v.tag = AXON_CSYS_VAL_UNIT;
    v.u.i = 0;
    return v;
}

static inline AxonCsysValue axon_csys_value_bool(bool b) {
    AxonCsysValue v;
    v.tag = AXON_CSYS_VAL_BOOL;
    v.u.b = b;
    return v;
}

static inline AxonCsysValue axon_csys_value_int(int64_t i) {
    AxonCsysValue v;
    v.tag = AXON_CSYS_VAL_INT;
    v.u.i = i;
    return v;
}

static inline AxonCsysValue axon_csys_value_float(double f) {
    AxonCsysValue v;
    v.tag = AXON_CSYS_VAL_FLOAT;
    v.u.f = f;
    return v;
}

static inline AxonCsysValue axon_csys_value_string(const char *ptr, size_t len) {
    AxonCsysValue v;
    v.tag = AXON_CSYS_VAL_STRING;
    v.u.s.ptr = ptr;
    v.u.s.len = len;
    return v;
}

static inline AxonCsysValue axon_csys_value_symbol(const char *ptr, size_t len) {
    AxonCsysValue v;
    v.tag = AXON_CSYS_VAL_SYMBOL;
    v.u.s.ptr = ptr;
    v.u.s.len = len;
    return v;
}

/* ──────────────────────────────────────────────────────────────────────
 * Wire format — the dispatcher operates on flattened arrays of these
 * structs. The Rust shim builds the arrays from the JSON IR; the
 * dispatcher walks them.
 *
 * Indices are 32-bit unsigned. -1 sentinels are encoded as UINT32_MAX
 * to keep the layout tight.
 * ────────────────────────────────────────────────────────────────── */

#define AXON_CSYS_INDEX_NONE UINT32_MAX

/* One instruction in the flattened block. Index fields are packed for
 * cache density. */
typedef struct {
    /* Opcode (one of AXON_CSYS_OP_*). */
    uint8_t  opcode;

    /* Reserved padding for future flags / 16-bit alignment. */
    uint8_t  _pad[3];

    /* For PERFORM + FORWARD:
     *   `effect_id`    — index into wire->effects table.
     *   `operation_id` — index into effects[effect_id].operations.
     *   `args_count`   — number of args.
     *   `args_offset`  — offset into wire->arg_pool.
     * For HANDLER_FRAME:
     *   `effect_id`    — UNUSED (the frame's effect_names live in the frame_id->frame map).
     *   `operation_id` — UNUSED.
     *   `args_count`   — UNUSED.
     *   `args_offset`  — index into wire->frames table for the frame body + clauses.
     * For RESUME / ABORT:
     *   `effect_id`    — UNUSED.
     *   `operation_id` — UNUSED.
     *   `args_count`   — 1 if value_expr present, else 0.
     *   `args_offset`  — offset into wire->arg_pool for the value expression
     *                    (a single string or pre-evaluated value).
     * For PASSTHROUGH: all fields unused.
     */
    uint32_t effect_id;
    uint32_t operation_id;
    uint32_t args_count;
    uint32_t args_offset;

    /* CPS state coordinate (for trace events / observability). */
    uint32_t state_id;

    /* For HANDLER_FRAME + FORWARD: the frame_id from the IR. */
    uint32_t frame_id;
} AxonCsysInstruction;

/* Pre-resolved handler frame metadata.
 *
 * Each frame lists the effect IDs it handles + the body instruction
 * span + a per-(effect_id, operation_id) clause index. The dispatcher
 * uses these to find the matching clause in O(1) after locating the
 * frame.
 */
typedef struct {
    /* Effect IDs this frame handles. */
    const uint32_t *effect_ids;
    uint32_t        effect_count;

    /* Body instruction span (offset + count into wire->instructions). */
    uint32_t        body_offset;
    uint32_t        body_count;

    /* Clause table: clauses[i].operation_id_global is the operation
     * (effect_id<<16 | operation_id) that this clause handles. */
    const struct AxonCsysClause *clauses;
    uint32_t                     clause_count;

    /* IR-side frame_id (used in trace events). */
    uint32_t frame_id;
} AxonCsysFrame;

/* Pre-resolved handler clause metadata. */
typedef struct AxonCsysClause {
    /* Effect this clause handles. */
    uint32_t effect_id;

    /* Operation within that effect. */
    uint32_t operation_id;

    /* Number of operation parameters bound by this clause; the
     * dispatcher copies args_count values from the perform site into
     * the clause's parameter slots before running the body. */
    uint32_t parameter_count;

    /* Offset into wire->parameter_name_pool for the parameter names
     * (used to bind into the globals table at perform time). */
    uint32_t parameter_names_offset;

    /* Body instruction span. */
    uint32_t body_offset;
    uint32_t body_count;

    /* Operation name for trace events (borrowed slice). */
    AxonCsysStrSlice operation_name;
} AxonCsysClause;

/* Effect declaration metadata — operation arity + parameter types
 * (used by the dispatcher to validate the wire shape on entry). */
typedef struct {
    AxonCsysStrSlice    name;
    const AxonCsysStrSlice *operation_names;  /* parallel array */
    const uint32_t          *operation_arities; /* parallel array */
    uint32_t                operation_count;
} AxonCsysEffectDecl;

/* The full wire-format input to the dispatcher. The Rust shim builds
 * this from the JSON IR; the dispatcher walks it without ever
 * touching strings (except for the borrowed slices in trace events). */
typedef struct {
    /* Top-level instruction block. */
    const AxonCsysInstruction *instructions;
    uint32_t                   instruction_count;

    /* Frame table, indexed by `instruction.args_offset` for HANDLER_FRAME ops. */
    const AxonCsysFrame *frames;
    uint32_t             frame_count;

    /* Effect declaration table, indexed by `instruction.effect_id`. */
    const AxonCsysEffectDecl *effects;
    uint32_t                  effect_count;

    /* Argument pool — used by perform / forward / resume / abort to
     * carry argument lists. Indexed by `instruction.args_offset`,
     * count = `instruction.args_count`. */
    const AxonCsysValue *arg_pool;
    uint32_t             arg_pool_size;

    /* Parameter name pool — strings used by clause parameter binding.
     * Indexed by `clause.parameter_names_offset`, count =
     * `clause.parameter_count`. */
    const AxonCsysStrSlice *parameter_name_pool;
    uint32_t                parameter_name_pool_size;
} AxonCsysWire;

/* ──────────────────────────────────────────────────────────────────────
 * Dispatch result + error surface
 * ────────────────────────────────────────────────────────────────── */

typedef enum {
    AXON_CSYS_RESULT_COMPLETED = 0,  /* block ran to completion; value is in `value` */
    AXON_CSYS_RESULT_ABORTED   = 1,  /* abort propagated to top; value is the abort payload */
    AXON_CSYS_RESULT_ERROR     = 2,  /* runtime error; see `error_code` */
} AxonCsysResultKind;

typedef enum {
    AXON_CSYS_ERR_NONE                       = 0,
    AXON_CSYS_ERR_UNHANDLED_EFFECT           = 1,
    AXON_CSYS_ERR_UNKNOWN_OPERATION          = 2,
    AXON_CSYS_ERR_NO_DISCHARGE               = 3,
    AXON_CSYS_ERR_FORWARD_WITHOUT_OUTER      = 4,
    AXON_CSYS_ERR_CONTROL_OPCODE_OUT_OF_BODY = 5,
    AXON_CSYS_ERR_STACK_OVERFLOW             = 6,
    AXON_CSYS_ERR_INTERNAL                   = 99,
} AxonCsysErrorCode;

typedef struct {
    AxonCsysResultKind kind;
    AxonCsysValue      value;       /* meaningful for COMPLETED + ABORTED */
    AxonCsysErrorCode  error_code;  /* meaningful for ERROR */
    /* When error_code = UNHANDLED_EFFECT or UNKNOWN_OPERATION, these
     * carry the offending IDs back to the Rust shim for diagnostic
     * stringification. UINT32_MAX if not applicable. */
    uint32_t           error_effect_id;
    uint32_t           error_operation_id;
} AxonCsysDispatchResult;

/* ──────────────────────────────────────────────────────────────────────
 * Trace event surface — opt-in per-call. The dispatcher writes events
 * into a caller-provided ring buffer; if `event_capacity == 0` the
 * dispatcher emits no events (the hot-path fast-path).
 * ────────────────────────────────────────────────────────────────── */

typedef enum {
    AXON_CSYS_TRACE_ENTER_FRAME = 0,
    AXON_CSYS_TRACE_EXIT_FRAME  = 1,
    AXON_CSYS_TRACE_PERFORM     = 2,
    AXON_CSYS_TRACE_RESUME      = 3,
    AXON_CSYS_TRACE_ABORT       = 4,
    AXON_CSYS_TRACE_FORWARD     = 5,
} AxonCsysTraceKind;

typedef struct {
    AxonCsysTraceKind kind;
    /* Common: frame_id for ENTER/EXIT, source_frame_id for FORWARD,
     * resume/abort frame_id, perform state_id. */
    uint32_t          frame_id;
    /* Effect / operation IDs (where applicable). UINT32_MAX otherwise. */
    uint32_t          effect_id;
    uint32_t          operation_id;
    /* Value carried (resume/abort). Unit otherwise. */
    AxonCsysValue     value;
} AxonCsysTraceEvent;

/* ──────────────────────────────────────────────────────────────────────
 * Dispatch entry point
 * ────────────────────────────────────────────────────────────────── */

/* Execute the wire's top-level instruction block.
 *
 * `globals` + `globals_count`: read-only pre-bound symbol table for
 * resolving Symbol-typed args. The dispatcher does NOT mutate this
 * table; clause parameter binding uses an internal scratch table.
 *
 * `trace_buffer` + `trace_capacity` + `trace_count_out`: optional
 * trace ring. If `trace_buffer == NULL` OR `trace_capacity == 0`,
 * tracing is disabled (fast path). Otherwise the dispatcher writes
 * up to `trace_capacity` events; the count actually written is
 * stored in `*trace_count_out`. Excess events are silently dropped.
 *
 * Returns the dispatch result by value. */
AXON_CSYS_EFFECTS_NODISCARD
AxonCsysDispatchResult axon_csys_effects_run(
    const AxonCsysWire *wire,
    const AxonCsysStrSlice *globals_keys,
    const AxonCsysValue    *globals_values,
    uint32_t                globals_count,
    AxonCsysTraceEvent     *trace_buffer,
    uint32_t                trace_capacity,
    uint32_t               *trace_count_out
);

/* Reports whether this build of axon-csys uses computed gotos for the
 * effects dispatcher. Returns true on gcc/clang, false on MSVC (D5
 * fallback to switch dispatch). Used by tests + benchmarks. */
AXON_CSYS_EFFECTS_NODISCARD
bool axon_csys_effects_uses_computed_gotos(void);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_EFFECTS_DISPATCH_H */
