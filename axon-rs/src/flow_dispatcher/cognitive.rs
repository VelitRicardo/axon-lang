//! §Fase 33.y.f — Cognitive primitives (Fase 11 neuro-symbolic).
//!
//! Ten variants graduated in 33.y.f:
//!
//! 1. **`Remember`** — Persist a value to the cognitive memory.
//!    Write-through: always updates `ctx.let_bindings`; when
//!    `ctx.pem_backend` is `Some(_)`, also persists to PEM as a
//!    [`crate::pem::state::MemoryEntry`] in the session's
//!    [`crate::pem::state::CognitiveState`].
//!
//! 2. **`Recall`** — Restore a value from the cognitive memory.
//!    Read-back: when `ctx.pem_backend` is `Some(_)`, restores
//!    `CognitiveState` + searches `short_term_memory` for the
//!    requested key; falls back to `ctx.let_bindings` lookup; binds
//!    the result under the `query` key in `ctx.let_bindings`.
//!
//! 3. **`Forge`** — Payload-free in v1.25.0 IR. Emits canonical
//!    `step_type: "forge"` wire shape (StepStart + StepComplete, 0
//!    tokens). Future IR extensions wire a body via a public helper.
//!
//! 4-10. **`Focus`, `Associate`, `Aggregate`, `Explore`, `Ingest`,
//!    `Navigate`, `Corroborate`** — historically all seven reused the
//!    pure-shape async core. **§Fase 108.a re-founded the five
//!    data-plane verbs** (`focus` / `associate` / `aggregate` /
//!    `explore` / `ingest`): they are relational-algebra operations
//!    over a declared `dataspace`, and an LLM fallthrough would
//!    HALLUCINATE their results — the model *narrating* a load or an
//!    aggregate that never ran (the `mint`/`rotate` argument,
//!    verbatim). Until the deterministic columnar engine port lands
//!    (§108.b–d) each fails CLOSED with
//!    `MissingDependency { name: "dataspace_engine" }` after emitting
//!    its attributable `StepStart` — a refusal, never a narration.
//!    `Navigate` and `Corroborate` (genuinely cognitive) keep the
//!    pure-shape core.
//!
//! # PEM integration
//!
//! The optional `pem_backend` field on `DispatchCtx` carries an
//! `Arc<dyn PersistenceBackend>`. When set, Remember/Recall route
//! through `persist` / `restore` calls; when None, both degrade
//! gracefully to `let_bindings`-only operation (in-memory baseline
//! that matches the canonical adopter unit-test path).
//!
//! D-letter anchors:
//! - **D1** — every cognitive variant has a NAMED async handler;
//!   exhaustive match in `dispatch_node`.
//! - **D3** — cancel checked at every `.await` boundary.
//! - **D6** — pure-shape-routed handlers (Focus/Associate/...) push
//!   StepAuditRecord via the shared core; Remember/Recall do NOT
//!   push audit rows (they're cognitive-state mutations, not
//!   wire-LLM steps).
//! - **D7** — every error case routes through DispatchError; PEM
//!   `persist`/`restore` errors surface as
//!   `DispatchError::BackendError { name: "pem", ... }`.
//! - **D10** — sync-runner parity: Remember binds + Recall reads
//!   via `let_bindings` identically to the principled cognitive-
//!   state semantics the sync runner adopts; PEM write-through is
//!   an enterprise-tier extension (transparent to the wire +
//!   binding semantics).

use crate::flow_dispatcher::pure_shape::{run_pure_shape, PureShapeStep};
use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{
    IRAggregateStep, IRAssociateStep, IRCorroborateStep, IRExploreStep, IRFocusStep,
    IRForgeBlock, IRIngestStep, IRNavigateStep, IRRecallStep, IRRememberStep,
};

// ────────────────────────────────────────────────────────────────────
//  Remember — PEM write-through + let_bindings
// ────────────────────────────────────────────────────────────────────

/// Persist `expression`'s value to the cognitive memory under
/// `memory_target`.
///
/// Resolution order for `expression`:
/// 1. If `expression` is a key in `ctx.let_bindings`, use its value.
/// 2. Otherwise treat `expression` as a literal string.
///
/// Write order:
/// 1. Always insert `value` into `ctx.let_bindings[memory_target]`
///    (in-memory baseline; matches sync-runner semantics).
/// 2. When `ctx.pem_backend` is `Some(_)`, additionally persist
///    the value as a [`crate::pem::state::MemoryEntry`] into the
///    session's `CognitiveState.short_term_memory` (write-through).
///
/// # Wire shape
///
/// Emits StepStart + StepComplete with `step_type: "remember"`.
/// No StepToken (Remember is a cognitive-state mutation, not an
/// LLM dispatch). `tokens_emitted` = 0.
///
/// # Returns
///
/// `NodeOutcome::Completed { output: <resolved-value>,
/// tokens_emitted: 0, step_index: <reserved> }`. The `output`
/// reflects what was bound so downstream `last_output` capture
/// in orchestration handlers (Conditional / ForIn body
/// aggregation) sees the bound value.
pub async fn run_remember(
    node: &IRRememberStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    // Resolve `expression` — let_bindings reference takes priority
    // over literal interpretation.
    let value = ctx
        .let_bindings
        .get(&node.expression)
        .cloned()
        .unwrap_or_else(|| node.expression.clone());

    emit_step_start(ctx, &step_name_for_remember(node), step_index, "remember")?;

    // Always update let_bindings (in-memory baseline).
    ctx.let_bindings
        .insert(node.memory_target.clone(), value.clone());

    // Write-through to PEM when backend is wired. PEM errors
    // surface as DispatchError::BackendError so the SSE handler
    // emits a structured axon.error rather than silently dropping
    // the cognitive state.
    if let Some(backend) = ctx.pem_backend.clone() {
        write_through_pem(&backend, ctx, &node.memory_target, &value).await?;
    }

    emit_step_complete(
        ctx,
        &step_name_for_remember(node),
        step_index,
        &value,
        0,
    )?;

    Ok(NodeOutcome::Completed {
        output: value,
        tokens_emitted: 0,
        step_index,
    })
}

fn step_name_for_remember(node: &IRRememberStep) -> String {
    if node.memory_target.is_empty() {
        "Remember".to_string()
    } else {
        node.memory_target.clone()
    }
}

async fn write_through_pem(
    backend: &std::sync::Arc<dyn crate::pem::PersistenceBackend>,
    ctx: &DispatchCtx,
    key: &str,
    value: &str,
) -> Result<(), DispatchError> {
    use crate::pem::state::{CognitiveState, MemoryEntry};
    use chrono::{Duration as ChronoDuration, Utc};

    // Restore existing state; create a fresh one when not found.
    let mut state = match backend.restore(&ctx.session_id).await {
        Ok(s) => s,
        Err(_) => CognitiveState::new(&ctx.session_id, &ctx.tenant_id, &ctx.flow_name),
    };

    state.short_term_memory.push(MemoryEntry {
        key: key.to_string(),
        payload: serde_json::Value::String(value.to_string()),
        symbolic_refs: Vec::new(),
        stored_at: Utc::now(),
    });
    state.last_updated_at = Utc::now();

    backend
        .persist(&ctx.session_id, &state, ChronoDuration::hours(24))
        .await
        .map_err(|e| DispatchError::BackendError {
            name: "pem".to_string(),
            message: format!("{e:?}"),
        })?;

    Ok(())
}

// ────────────────────────────────────────────────────────────────────
//  Recall — PEM read-back + let_bindings fallback
// ────────────────────────────────────────────────────────────────────

/// Restore a value from the cognitive memory.
///
/// Read order:
/// 1. When `ctx.pem_backend` is `Some(_)`, restore `CognitiveState`
///    + search `short_term_memory` for the latest entry with
///    `key == memory_source`.
/// 2. Otherwise (or when PEM restore returns NotFound / no
///    matching entry), fall back to `ctx.let_bindings[memory_source]`.
/// 3. When neither resolves, the recalled value is the empty string.
///
/// The resolved value is bound under `ctx.let_bindings[query]` so
/// subsequent steps reference it via the adopter-declared name.
///
/// # Wire shape
///
/// Same as Remember: StepStart + StepComplete with `step_type:
/// "recall"`, 0 StepTokens.
pub async fn run_recall(
    node: &IRRecallStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    emit_step_start(ctx, &step_name_for_recall(node), step_index, "recall")?;

    let resolved = resolve_recall_value(node, ctx).await;

    // Bind the recalled value into let_bindings under `query`.
    ctx.let_bindings
        .insert(node.query.clone(), resolved.clone());

    emit_step_complete(
        ctx,
        &step_name_for_recall(node),
        step_index,
        &resolved,
        0,
    )?;

    Ok(NodeOutcome::Completed {
        output: resolved,
        tokens_emitted: 0,
        step_index,
    })
}

fn step_name_for_recall(node: &IRRecallStep) -> String {
    if node.query.is_empty() {
        "Recall".to_string()
    } else {
        node.query.clone()
    }
}

async fn resolve_recall_value(node: &IRRecallStep, ctx: &DispatchCtx) -> String {
    // 1. PEM read-back if backend is wired.
    if let Some(backend) = &ctx.pem_backend {
        if let Ok(state) = backend.restore(&ctx.session_id).await {
            // Find the LATEST entry with matching key (short_term_memory
            // accumulates over time; newest takes precedence).
            if let Some(entry) = state
                .short_term_memory
                .iter()
                .rev()
                .find(|e| e.key == node.memory_source)
            {
                if let serde_json::Value::String(s) = &entry.payload {
                    return s.clone();
                }
                // Non-string payload — canonical JSON serialization.
                return entry.payload.to_string();
            }
        }
    }

    // 2. let_bindings fallback.
    ctx.let_bindings
        .get(&node.memory_source)
        .cloned()
        .unwrap_or_default()
}

// ────────────────────────────────────────────────────────────────────
//  Forge — payload-free wire shape
// ────────────────────────────────────────────────────────────────────

/// Forge handler. In v1.25.0 the IR variant is payload-free so
/// this emits the canonical `step_type: "forge"` wire shape
/// (StepStart + StepComplete, 0 tokens). Future IR extensions
/// (a Fase 33.y.f.2 follow-up that adds a body via the AST/IR)
/// wire a recursive `dispatch_body` call from `run_forge`.
/// §Fase 86 — one LLM pass of the forge pipeline at an explicit temperature.
/// Returns the phase's produced text (empty on a non-Completed outcome).
async fn forge_phase(
    ctx: &mut DispatchCtx,
    name: &str,
    prompt: String,
    temperature: f64,
) -> Result<String, DispatchError> {
    let shape = PureShapeStep {
        name: name.to_string(),
        user_prompt: prompt,
        framing_addendum: Some(
            "You are inside a directed creative-synthesis pipeline (`forge`). Produce vivid, \
             concrete, ORIGINAL conceptual content — never a hedge or a restatement."
                .into(),
        ),
        kind_slug: "forge",
        tools: Vec::new(),
        requires_context: None,
        temperature: Some(temperature),
        now_tz: None,
        stream_on_chunk: None,
    };
    match run_pure_shape(shape, ctx).await? {
        NodeOutcome::Completed { output, .. } => Ok(output),
        _ => Ok(String::new()),
    }
}

/// §Fase 86 — Directed Creative Synthesis. The real Poincaré-Hadamard-Wallas
/// four-phase pipeline (replacing the pre-§86 no-op stub), with a **measured,
/// fail-closed novelty guarantee** (D86.4/D86.6):
///
/// 1. **Preparation** — expand the seed into its OBVIOUS reading `B` (low τ) —
///    the "known" we measure novelty against.
/// 2. **Incubation** — `depth` speculative iterations at τ_eff = τ_base·(0.5 +
///    0.5·novelty), each pushing further past the obvious.
/// 3. **Illumination** — `branches` crystallizations; each branch's novelty is
///    MEASURED as ν = NCD(B, branch) — the computable Kolmogorov-novelty proxy.
/// 4. **Verification** — select the argmax-utility branch and enforce the
///    novelty floor **fail-closed**: a derivative result (ν < floor) is NEVER
///    returned as creative — the forge fails with a structured error.
///
/// Honest v1 scope (D86.7): the runtime hard gate is the measured novelty
/// floor; the `constraints:` anchor is statically validated (T871) and its
/// coherence floor is read here, with per-branch coherence set to 1.0 pending
/// the live anchor-confidence judge (§5 deferred). The novelty guarantee — the
/// genuinely novel contribution — is fully enforced.
pub async fn run_forge(
    node: &IRForgeBlock,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;

    let mode = if node.mode.is_empty() {
        "exploratory"
    } else {
        node.mode.as_str()
    };
    let depth = node.depth.max(1) as usize;
    let branches = node.branches.max(1) as usize;
    let nov_floor = crate::forge::novelty_floor(node.novelty);
    let tau_base = crate::forge::boden_profile(mode).tau_base;
    let tau_incubate = crate::forge::incubation_temperature(mode, node.novelty);
    let out_type = if node.output_type.is_empty() {
        "concept".to_string()
    } else {
        node.output_type.clone()
    };

    // Coherence floor from the declared `constraints:` anchor (0.0 ⇒ none).
    let coherence_floor = ctx
        .anchors
        .iter()
        .find(|a| a.name == node.constraints_ref)
        .and_then(|a| a.confidence_floor)
        .unwrap_or(0.0);

    // Phase 1 — Preparation: the obvious/conventional baseline B.
    let baseline = forge_phase(
        ctx,
        &node.name,
        format!(
            "Expand this creative seed into its CONVENTIONAL, obvious interpretation — the first \
             associations most people make. Seed: \"{}\". Be concrete, but deliberately \
             conventional; this is the baseline we will surpass.",
            node.seed
        ),
        0.3,
    )
    .await?;

    // Phase 2 — Incubation: `depth` speculative iterations at τ_eff.
    let mut incubated = baseline.clone();
    for i in 0..depth {
        incubated = forge_phase(
            ctx,
            &node.name,
            format!(
                "Speculatively explore FAR beyond the obvious — past cliché into unexpected \
                 territory (iteration {}/{}). Seed: \"{}\". Obvious baseline to surpass: {}. \
                 Prior exploration: {}. Go further; break the expected frame.",
                i + 1,
                depth,
                node.seed,
                baseline,
                incubated
            ),
            tau_incubate,
        )
        .await?;
    }

    // Phase 3 — Illumination: `branches` crystallizations; measure ν = NCD(B,·).
    let mut candidates: Vec<crate::forge::Branch> = Vec::with_capacity(branches);
    for _ in 0..branches {
        let output = forge_phase(
            ctx,
            &node.name,
            format!(
                "Crystallize a single, coherent, GENUINELY NOVEL {} from this incubated \
                 exploration. Seed: \"{}\". Exploration: {}. Deliver the finished creative \
                 concept — surprising yet coherent.",
                out_type, node.seed, incubated
            ),
            tau_base,
        )
        .await?;
        let novelty = crate::forge::novelty_score(&baseline, &output);
        candidates.push(crate::forge::Branch {
            output,
            coherence: 1.0, // §5 deferred: live per-branch anchor-confidence judge
            novelty,
        });
    }

    // Phase 4 — Verification: select + fail-closed novelty gate.
    let winner_idx = crate::forge::select_illumination(&candidates, coherence_floor, nov_floor);
    let verdict = crate::forge::verify(winner_idx.map(|i| &candidates[i]), nov_floor);
    ctx.step_counter += 1;

    match verdict {
        crate::forge::ForgeVerdict::Accepted {
            output,
            novelty,
            coherence: _,
        } => {
            emit_step_start(ctx, "Forge", step_index, "forge")?;
            emit_step_complete(ctx, "Forge", step_index, &output, 0)?;
            let _ = novelty; // measured; surfaced via the enterprise audit (§86.g)
            Ok(NodeOutcome::Completed {
                output,
                tokens_emitted: 0,
                step_index,
            })
        }
        crate::forge::ForgeVerdict::Rejected(reason) => {
            let detail = match &reason {
                crate::forge::ForgeRejection::NoveltyFloorBreached { measured, floor } => format!(
                    "best branch novelty {:.3} < floor {:.3} — the synthesis was too derivative \
                     of the obvious reading of the seed",
                    measured, floor
                ),
                crate::forge::ForgeRejection::NoFeasibleBranch => {
                    "no illumination branch satisfied the constraints anchor".to_string()
                }
            };
            // Fail-closed: a forge that cannot clear its floor errors LOUDLY —
            // never a silent derivative/empty result (D86.6).
            Err(DispatchError::BackendError {
                name: "forge".to_string(),
                message: format!("{}: {}", reason.slug(), detail),
            })
        }
    }
}

// ────────────────────────────────────────────────────────────────────
//  Cognitive-framing handlers (7) — reuse pure_shape async core
// ────────────────────────────────────────────────────────────────────

/// Focus handler — narrow attention to an expression. Reuses the
/// pure-shape async core with the focus framing addendum.
/// §Fase 108.d — shared plumbing for the four query verbs: resolve the
/// engine port (fail CLOSED — the §108.a floor), run the CPU-bound
/// query under `spawn_blocking` (Brief #63), bind the result envelope
/// under the step's output name, and emit the completion event.
/// The envelope is DETERMINISTIC: `{rows, taint, stats}` — pruning is
/// observable, taint is explicit, and a downstream step reasons over
/// computed data, never a narration.
async fn run_dataspace_query<F>(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_type: &str,
    step_index: usize,
    query: F,
) -> Result<NodeOutcome, DispatchError>
where
    F: FnOnce(
            &crate::dataspace_engine::DataspaceEngine,
        ) -> Result<crate::dataspace_engine::QueryOutput, String>
        + Send
        + 'static,
{
    let engine = ctx
        .dataspace_engine
        .clone()
        .ok_or(DispatchError::MissingDependency {
            name: "dataspace_engine",
        })?;
    let step_type_owned = step_type.to_string();
    let out = tokio::task::spawn_blocking(move || {
        let guard = engine.read().expect("dataspace engine lock poisoned");
        query(&guard)
    })
    .await
    .map_err(|e| DispatchError::BackendError {
        name: "dataspace_engine".to_string(),
        message: format!("{step_type_owned} worker task failed: {e}"),
    })?
    .map_err(|e| DispatchError::BackendError {
        name: "dataspace_engine".to_string(),
        message: format!("{step_type} refused: {e}"),
    })?;
    let envelope = serde_json::json!({
        "rows": out.rows,
        "taint": crate::dataspace_engine::taint_str(out.taint),
        "stats": out.stats,
    })
    .to_string();
    ctx.let_bindings
        .insert(step_name.to_string(), envelope.clone());
    emit_step_complete(ctx, step_name, step_index, &envelope, 0)?;
    Ok(NodeOutcome::Completed {
        output: envelope,
        tokens_emitted: 0,
        step_index,
    })
}

/// §Fase 108.a — the honesty floor. `focus` is a DATA-PLANE verb:
/// σ_φ ∘ π_v over a declared `dataspace` (§108.d). The previous
/// behaviour prompted the model to "narrow scope … surface what
/// matters most" — i.e. to NARRATE a selection that never scanned a
/// row. No engine port ⇒ fail CLOSED, never narrate (the
/// `mint`/`rotate` posture). The real relational handler replaces
/// this refusal in §108.d.
pub async fn run_focus(
    node: &IRFocusStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    let name = if node.output.is_empty() {
        if node.expression.is_empty() {
            "Focus".to_string()
        } else {
            node.expression.clone()
        }
    } else {
        node.output.clone()
    };
    emit_step_start(ctx, &name, step_index, "focus")?;
    let target = node.expression.clone();
    let where_expr = node.where_expr.clone();
    let select = node.select.clone();
    let bindings = ctx.let_bindings.clone();
    run_dataspace_query(ctx, &name, "focus", step_index, move |engine| {
        let store = engine.store(&target).ok_or_else(|| {
            format!(
                "'{target}' is not an instantiated dataspace — the compile-time \
                 axon-T930 check did not run over this IR (stale or hand-edited \
                 artifact)"
            )
        })?;
        crate::dataspace_engine::focus_query(store, &where_expr, &select, &bindings)
    })
    .await
}

/// §Fase 108.a — the honesty floor. `associate` is a DATA-PLANE verb:
/// a hash equi-join across two declared dataspaces (§108.d). The
/// previous behaviour prompted the model to "find the meaningful
/// relationship" — a join whose matches were INVENTED, not computed.
/// No engine port ⇒ fail CLOSED, never narrate. The real join handler
/// replaces this refusal in §108.d.
pub async fn run_associate(
    node: &IRAssociateStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    let name = if !node.output.is_empty() {
        node.output.clone()
    } else if node.left.is_empty() {
        "Associate".to_string()
    } else {
        format!("{}_{}", node.left, node.right)
    };
    emit_step_start(ctx, &name, step_index, "associate")?;
    let left = node.left.clone();
    let right = node.right.clone();
    let using = node.using_field.clone();
    run_dataspace_query(ctx, &name, "associate", step_index, move |engine| {
        let l = engine.store(&left).ok_or_else(|| {
            format!("'{left}' is not an instantiated dataspace (stale IR — axon-T930)")
        })?;
        let r = engine.store(&right).ok_or_else(|| {
            format!("'{right}' is not an instantiated dataspace (stale IR — axon-T930)")
        })?;
        if using.is_empty() {
            return Err(
                "associate declares no `using <column>` — a keyless join is a \
                 cartesian product, which is refused (stale IR — axon-T930)"
                    .to_string(),
            );
        }
        crate::dataspace_engine::associate_query(l, r, &using)
    })
    .await
}

/// §Fase 108.a — the honesty floor. `aggregate` is a DATA-PLANE verb:
/// γ (group + the closed aggregate catalog) over a declared dataspace
/// (§108.d). The previous behaviour prompted the model to "group +
/// summarize" — an aggregate is a NUMBER, and a narrated number is a
/// fabricated one. No engine port ⇒ fail CLOSED, never narrate. The
/// real γ handler replaces this refusal in §108.d.
pub async fn run_aggregate(
    node: &IRAggregateStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    let name = if !node.alias.is_empty() {
        node.alias.clone()
    } else if node.target.is_empty() {
        "Aggregate".to_string()
    } else {
        node.target.clone()
    };
    emit_step_start(ctx, &name, step_index, "aggregate")?;
    let target = node.target.clone();
    let where_expr = node.where_expr.clone();
    let group_by = node.group_by.clone();
    let compute_raw = node.compute.clone();
    let bindings = ctx.let_bindings.clone();
    run_dataspace_query(ctx, &name, "aggregate", step_index, move |engine| {
        let store = engine.store(&target).ok_or_else(|| {
            format!("'{target}' is not an instantiated dataspace (stale IR — axon-T930)")
        })?;
        // An aggregate with no computes counts rows — the smallest
        // honest default (`count` is total for any schema).
        let computes: Vec<crate::dataspace_engine::AggregateSpec> = if compute_raw.is_empty() {
            vec![crate::dataspace_engine::AggregateSpec::parse("count")?]
        } else {
            compute_raw
                .iter()
                .map(|c| crate::dataspace_engine::AggregateSpec::parse(c))
                .collect::<Result<_, _>>()?
        };
        crate::dataspace_engine::aggregate_query(
            store,
            &where_expr,
            &group_by,
            &computes,
            &bindings,
        )
    })
    .await
}

/// §Fase 108.a — the honesty floor. `explore` is a DATA-PLANE verb: a
/// deterministic profile (row/null counts, per-column min/max, schema)
/// over a declared dataspace (§108.d). The previous behaviour prompted
/// the model to "sample broadly" over data it had never seen. No
/// engine port ⇒ fail CLOSED, never narrate. The real profile handler
/// replaces this refusal in §108.d.
pub async fn run_explore(
    node: &IRExploreStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    let name = if !node.output.is_empty() {
        node.output.clone()
    } else if node.target.is_empty() {
        "Explore".to_string()
    } else {
        node.target.clone()
    };
    emit_step_start(ctx, &name, step_index, "explore")?;
    let target = node.target.clone();
    run_dataspace_query(ctx, &name, "explore", step_index, move |engine| {
        let store = engine.store(&target).ok_or_else(|| {
            format!("'{target}' is not an instantiated dataspace (stale IR — axon-T930)")
        })?;
        Ok(crate::dataspace_engine::explore_profile(store))
    })
    .await
}

/// §Fase 108.c — governed ingest, REAL. Loads source bytes (resolved
/// from the flow's in-scope bindings) into a DECLARED dataspace through
/// the deterministic loaders:
///
/// 1. **Fail CLOSED without the engine port** (the §108.a honesty
///    floor — never an LLM narration; pre-108.a this handler literally
///    prompted the model to "map the source's structure… preserve
///    fidelity").
/// 2. **Bounds BEFORE parse** (§100): `limits { max_bytes, max_rows }`
///    (or the conservative defaults) are enforced on the raw stream
///    before a byte is interpreted.
/// 3. **Type refusal, not coercion** (D108.7): a value that does not
///    fit its declared column refuses the whole batch, naming
///    row + column.
/// 4. **Born-Untrusted provenance** (§98): the batch is stamped
///    source + sha256 + ingested_at + `EpistemicTaint::Untrusted`
///    at construction — no unstamped batch can exist.
/// 5. **CPU work off the async runtime** (Brief #63): parse + typing
///    run under `spawn_blocking`.
pub async fn run_ingest(
    node: &IRIngestStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;
    let name = if node.target.is_empty() {
        "Ingest".to_string()
    } else {
        node.target.clone()
    };
    emit_step_start(ctx, &name, step_index, "ingest")?;

    // (1) The engine port — absent means fail CLOSED, never narrate.
    let engine = ctx
        .dataspace_engine
        .clone()
        .ok_or(DispatchError::MissingDependency {
            name: "dataspace_engine",
        })?;

    // A pre-108.c artifact carries no `format:` — the compile-time
    // axon-T929 check did not run over this IR. Fail closed, loudly.
    let format = crate::dataspace_engine::IngestFormat::from_declared(&node.format)
        .ok_or_else(|| DispatchError::BackendError {
            name: "dataspace_engine".to_string(),
            message: format!(
                "ingest into '{}' declares format '{}' — not the closed loader \
                 catalog {{csv, json}}. The compile-time axon-T929 check did not \
                 run over this IR (stale or hand-edited artifact).",
                node.target,
                if node.format.is_empty() { "<unset>" } else { &node.format }
            ),
        })?;

    // Resolve the source bytes from the flow's bindings (a prior `let`,
    // step result, or tool output binds them). Missing binding = fail
    // closed: an ingest NEVER invents its input.
    let raw: String = ctx
        .let_bindings
        .get(&node.source)
        .cloned()
        .ok_or_else(|| DispatchError::BackendError {
            name: "dataspace_engine".to_string(),
            message: format!(
                "ingest source '{}' resolves to no in-scope binding — bind the raw \
                 content first (a `let`, a step result, or a tool output).",
                node.source
            ),
        })?;

    // The declared schema (the store was instantiated at deploy).
    let schema: Vec<(String, crate::dataspace_engine::ColumnType)> = {
        let guard = engine.read().expect("dataspace engine lock poisoned");
        let store = guard
            .store(&node.target)
            .ok_or_else(|| DispatchError::BackendError {
                name: "dataspace_engine".to_string(),
                message: format!(
                    "ingest targets '{}', which is not an instantiated dataspace — \
                     the compile-time axon-T929 check did not run over this IR \
                     (stale or hand-edited artifact).",
                    node.target
                ),
            })?;
        store.schema().to_vec()
    };

    let limits = crate::dataspace_engine::IngestLimits {
        max_bytes: node
            .max_bytes
            .unwrap_or(crate::dataspace_engine::IngestLimits::default().max_bytes),
        max_rows: node
            .max_rows
            .unwrap_or(crate::dataspace_engine::IngestLimits::default().max_rows),
    };

    // (4) Provenance — stamped before the batch exists. `ingested_at`
    // is wall-clock bookkeeping (the same audit surface as the wire
    // events' now_ms), NOT a cognitive input (§91 stays intact).
    let provenance = crate::dataspace_engine::BatchProvenance {
        source: node.source.clone(),
        source_sha256: {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(raw.as_bytes());
            format!("{:x}", h.finalize())
        },
        ingested_at: chrono::Utc::now().to_rfc3339(),
        taint: crate::emcp::EpistemicTaint::Untrusted,
    };

    // (2)+(3)+(5) — bounds, parse, type: CPU-bound, off the runtime.
    let schema_for_parse = schema.clone();
    let batch = tokio::task::spawn_blocking(move || {
        crate::dataspace_engine::ingest_bytes(
            &schema_for_parse,
            format,
            raw.as_bytes(),
            &limits,
            provenance,
        )
    })
    .await
    .map_err(|e| DispatchError::BackendError {
        name: "dataspace_engine".to_string(),
        message: format!("ingest worker task failed: {e}"),
    })?
    .map_err(|e| DispatchError::BackendError {
        name: "dataspace_engine".to_string(),
        message: format!("ingest into '{}' refused: {e}", node.target),
    })?;

    let rows = batch.len();
    let sha = batch.provenance().source_sha256.clone();
    {
        let mut guard = engine.write().expect("dataspace engine lock poisoned");
        let store = guard
            .store_mut(&node.target)
            .ok_or_else(|| DispatchError::BackendError {
                name: "dataspace_engine".to_string(),
                message: format!("dataspace '{}' vanished mid-ingest", node.target),
            })?;
        store.append(batch).map_err(|e| DispatchError::BackendError {
            name: "dataspace_engine".to_string(),
            message: format!("ingest into '{}' refused at append: {e}", node.target),
        })?;
    }

    // Deterministic summary — bound under the step name so downstream
    // steps can reference the load; NO row data leaks into cognition.
    let output = serde_json::json!({
        "dataspace": node.target,
        "rows": rows,
        "source_sha256": sha,
        "taint": "untrusted",
    })
    .to_string();
    ctx.let_bindings.insert(name.clone(), output.clone());
    emit_step_complete(ctx, &name, step_index, &output, 0)?;
    Ok(NodeOutcome::Completed {
        output,
        tokens_emitted: 0,
        step_index,
    })
}

/// §Fase 62.A — resolve the source document a `navigate`/`drill` indexes, from
/// the in-scope bindings (the established PIX convention — the document/corpus
/// content lives under a binding seeded by a prior `ingest`/`let`, the same way
/// `drill` reads `__pix_<ref>_<path>`). Tries the corpus binding, the explicit
/// `__pix_<pix>_source` key, then the pix-named binding.
pub(crate) fn resolve_pix_source(corpus_ref: &str, pix_ref: &str, ctx: &DispatchCtx) -> Option<String> {
    let mut keys: Vec<String> = Vec::new();
    if !corpus_ref.is_empty() {
        keys.push(corpus_ref.to_string());
    }
    if !pix_ref.is_empty() {
        keys.push(format!("__pix_{pix_ref}_source"));
        keys.push(pix_ref.to_string());
    }
    for k in keys {
        if let Some(v) = ctx.let_bindings.get(&k) {
            if !v.trim().is_empty() {
                return Some(v.clone());
            }
        }
    }
    None
}

/// Navigate handler — the PIX retrieval navigator (paper
/// `paper_pix_formal_research.md`).
///
/// §Fase 62.A.2: when the referenced document/corpus is in scope, this runs the
/// REAL navigator (`crate::pix_navigator`): index the source into a tree, then a
/// bounded BFS whose branch selection approximates `I(R; node | Q, path)` —
/// embeddings-free, with a recorded reasoning path. It binds the retrieved leaf
/// content under `output_name`, seeds `__navigate_<output>_trail` with the real
/// path (so a later `trail` reads it), and seeds `__pix_<pix>_<title-path>` per
/// leaf (so a later `drill` resolves it).
///
/// When NO indexable source is in scope, it falls back (D5 graceful) to the
/// cognitive-framing shape so pre-§62 flows keep working unchanged.
pub async fn run_navigate(
    node: &IRNavigateStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    let query = crate::exec_context::interpolate_vars(&node.query, &ctx.let_bindings);

    // ── §Fase 64.B — DYNAMIC store-sourced MDN corpus-graph navigation ─────
    // When the navigate ref names a `corpus … from axonstore { … }`, build the
    // MDN graph from the LIVE store rows (tenant-scoped) at navigate-time and
    // navigate it. The graph grows as the stores grow — no redeploy. Tenant
    // isolation is INHERITED: the reads reuse the flow's connection-pinned,
    // RLS-scoped store connection (`read_all_store_rows`), never a fresh one.
    if let Some(src) = ctx.mdn_store_sources.get(&node.pix_ref).cloned() {
        let step_index = ctx.step_counter;
        ctx.step_counter += 1;
        let out_name = if node.output_name.is_empty() {
            "Navigate".to_string()
        } else {
            node.output_name.clone()
        };
        emit_step_start(ctx, &out_name, step_index, "navigate")?;

        // Read both backing stores tenant-scoped (RLS scopes to the axon-tenant).
        // §Fase 66 (Q2) — the SAME `where:` column-scope filter is applied to
        // BOTH the documents and edges stores, so the sourced MDN graph (docs +
        // edges) is scoped to one sub-tenant column. Empty `where_expr` keeps the
        // §64 behavior byte-identical (RLS scope only).
        let doc_rows = crate::flow_dispatcher::wire_integrations::read_all_store_rows(
            ctx,
            &src.doc_store,
            &node.where_expr,
        )
        .await?;
        let edge_rows = crate::flow_dispatcher::wire_integrations::read_all_store_rows(
            ctx,
            &src.edge_store,
            &node.where_expr,
        )
        .await?;

        // §Fase 64.C — when this store-sourced corpus is `adaptive`, the memory
        // endofunctor's ω reinforcement is PERSISTED back to the edge store after
        // the navigation (the plan is computed in the arm below, then written via
        // the atomic relative UPDATE once the read borrows are released).
        let adaptive = ctx.mdn_adaptive.contains(&node.pix_ref);
        let mut reinforcement: Vec<(String, String, String, f64)> = Vec::new();

        let content = match (doc_rows, edge_rows) {
            (Some(drows), Some(erows)) => {
                let (docs, edges) =
                    crate::flow_dispatcher::wire_integrations::extract_corpus_rows(
                        &drows, &erows, &src,
                    );
                match crate::mdn::Corpus::from_rows(&docs, &edges) {
                    Ok(corpus) => {
                        // Seed: the `from:` document by title, else the lowest id.
                        let seed = corpus
                            .documents()
                            .into_iter()
                            .find(|d| d.title == node.seed)
                            .map(|d| d.id)
                            .or_else(|| corpus.documents().into_iter().map(|d| d.id).min())
                            .unwrap_or(0);
                        let budget = crate::mdn::NavBudget {
                            max_docs: node.budget.map(|b| b.max(1) as usize).unwrap_or(5),
                            epsilon: 1e-6,
                        };
                        let gain = crate::mdn::LexicalGain::new(&corpus);
                        let r = crate::mdn::navigate_corpus(&corpus, &query, seed, &budget, &gain);
                        let trail = r
                            .trail
                            .iter()
                            .filter_map(|(id, g)| {
                                corpus.document(*id).map(|d| format!("{} (Δ={:.2})", d.title, g))
                            })
                            .collect::<Vec<_>>()
                            .join(" → ");
                        ctx.let_bindings
                            .insert(format!("__navigate_{out_name}_trail"), trail);

                        // §Fase 64.C — record this navigation's outcome into the
                        // corpus's in-flow history and plan the per-edge ω
                        // reinforcement to persist. `Δ = η·(s_o − s̄)` (relative,
                        // paper Def 6): a single outcome ⇒ s_o = s̄ ⇒ Δ = 0 ⇒ no
                        // write — reinforcement accrues once the corpus has seen
                        // multiple, varied interactions.
                        if adaptive {
                            let denom = r.selected.len().max(1) as f64;
                            let score = (r.total_gain / denom).clamp(0.0, 1.0);
                            let params = crate::mdn_memory::MemoryParams::default();
                            let s_bar = {
                                let mut hist = ctx.mdn_histories.lock().unwrap();
                                let h = hist.entry(node.pix_ref.clone()).or_default();
                                let t = h.outcomes.len() as u64;
                                h.record(crate::mdn_memory::Outcome {
                                    query: query.clone(),
                                    path: r.selected.clone(),
                                    score,
                                    timestamp: t,
                                });
                                h.mean_score()
                            };
                            reinforcement =
                                crate::flow_dispatcher::wire_integrations::plan_edge_reinforcements(
                                    &corpus, &r.selected, &docs, score, s_bar, params.eta,
                                );
                        }

                        r.selected
                            .iter()
                            .filter_map(|id| corpus.document(*id))
                            .map(|d| d.title.clone())
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                    // Empty graph (no documents persisted yet) — an empty result,
                    // not an error (a living corpus starts empty).
                    Err(_) => String::new(),
                }
            }
            // A non-Postgres-backed store can't hold typed rows — nothing to
            // navigate. Honest degrade to an empty result.
            _ => String::new(),
        };
        if !node.output_name.is_empty() {
            ctx.let_bindings.insert(node.output_name.clone(), content.clone());
        }

        // §Fase 64.C — persist the endofunctor's reinforcement to the edge store
        // via the atomic, relative UPDATE (tenant-scoped, best-effort).
        if !reinforcement.is_empty() {
            let eps = crate::mdn_memory::MemoryParams::default().epsilon;
            crate::flow_dispatcher::wire_integrations::persist_reinforcements(
                ctx,
                &src.edge_store,
                &src.edge_weight,
                &src.edge_from,
                &src.edge_to,
                &src.edge_type,
                &reinforcement,
                eps,
            )
            .await?;
        }

        emit_step_complete(ctx, &out_name, step_index, &content, 0)?;
        return Ok(NodeOutcome::Completed {
            output: content,
            tokens_emitted: 0,
            step_index,
        });
    }

    // ── §Fase 63.B — MDN corpus-graph navigation ──────────────────────────
    // When the navigate ref names a built MDN corpus graph (a `corpus` with
    // `relations:`), navigate the GRAPH: ε-informative greedy over reachable
    // documents, scored by the deterministic LexicalGain (signed EPR rides the
    // same `mdn::Corpus`). Embeddings-free.
    if let Some(corpora) = ctx.mdn_corpora.clone() {
        if let Some(base) = corpora.get(&node.pix_ref) {
            let step_index = ctx.step_counter;
            ctx.step_counter += 1;
            let out_name = if node.output_name.is_empty() {
                "Navigate".to_string()
            } else {
                node.output_name.clone()
            };
            emit_step_start(ctx, &out_name, step_index, "navigate")?;

            // §Fase 63.C — when the corpus is `adaptive`, deform it by the memory
            // endofunctor over the accumulated history (semantic ω reinforcement
            // + procedural bias) BEFORE navigating; otherwise navigate the base.
            let adaptive = ctx.mdn_adaptive.contains(&node.pix_ref);
            let effective: crate::mdn::Corpus = if adaptive {
                let hist = ctx.mdn_histories.lock().unwrap();
                let h = hist.get(&node.pix_ref).cloned().unwrap_or_default();
                crate::mdn_memory::apply_memory(base, &h, &crate::mdn_memory::MemoryParams::default())
            } else {
                base.clone()
            };

            // Seed: the `from:` document by title, else the lowest doc id.
            let seed = effective
                .documents()
                .into_iter()
                .find(|d| d.title == node.seed)
                .map(|d| d.id)
                .or_else(|| effective.documents().into_iter().map(|d| d.id).min())
                .unwrap_or(0);
            let budget = crate::mdn::NavBudget {
                max_docs: node.budget.map(|b| b.max(1) as usize).unwrap_or(5),
                epsilon: 1e-6,
            };
            let gain = crate::mdn::LexicalGain::new(&effective);
            let r = crate::mdn::navigate_corpus(&effective, &query, seed, &budget, &gain);

            let content = r
                .selected
                .iter()
                .filter_map(|id| effective.document(*id))
                .map(|d| d.title.clone())
                .collect::<Vec<_>>()
                .join("\n");
            if !node.output_name.is_empty() {
                ctx.let_bindings.insert(node.output_name.clone(), content.clone());
            }
            let trail = r
                .trail
                .iter()
                .filter_map(|(id, g)| effective.document(*id).map(|d| format!("{} (Δ={:.2})", d.title, g)))
                .collect::<Vec<_>>()
                .join(" → ");
            ctx.let_bindings.insert(format!("__navigate_{out_name}_trail"), trail);

            // §Fase 63.C — record this navigation into the adaptive corpus's
            // memory (episodic trajectory + an outcome scored by the information
            // gained), so subsequent navigations learn from it.
            if adaptive {
                let denom = r.selected.len().max(1) as f64;
                let score = (r.total_gain / denom).clamp(0.0, 1.0);
                let mut hist = ctx.mdn_histories.lock().unwrap();
                let h = hist.entry(node.pix_ref.clone()).or_default();
                let t = h.outcomes.len() as u64;
                h.record(crate::mdn_memory::Outcome {
                    query: query.clone(),
                    path: r.selected.clone(),
                    score,
                    timestamp: t,
                });
            }

            emit_step_complete(ctx, &out_name, step_index, &content, 0)?;
            return Ok(NodeOutcome::Completed {
                output: content,
                tokens_emitted: 0,
                step_index,
            });
        }
    }

    // ── Real navigation path (PIX) ────────────────────────────────────────
    if let Some(source) = resolve_pix_source(&node.corpus_ref, &node.pix_ref, ctx) {
        if let Ok(tree) = crate::pix_navigator::index_markdown(&source) {
            let step_index = ctx.step_counter;
            ctx.step_counter += 1;
            let out_name = if node.output_name.is_empty() {
                "Navigate".to_string()
            } else {
                node.output_name.clone()
            };
            emit_step_start(ctx, &out_name, step_index, "navigate")?;

            // §Fase 119.f — the declared per-navigation `depth:` DECIDES the
            // bounded-rationality depth bound. A field consumed by nothing
            // would be the §111 defect; this is the consumer.
            let cfg = crate::pix_navigator::NavConfig {
                d_max: node
                    .depth
                    .filter(|d| *d > 0)
                    .map(|d| d as usize)
                    .unwrap_or(crate::pix_navigator::NavConfig::default().d_max),
                ..crate::pix_navigator::NavConfig::default()
            };
            let scorer = crate::pix_navigator::LexicalScorer::default();
            let result = crate::pix_navigator::pix_navigate(&tree, &query, &cfg, &scorer);

            let content = result
                .leaves
                .iter()
                .map(|l| l.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");

            if !node.output_name.is_empty() {
                ctx.let_bindings.insert(node.output_name.clone(), content.clone());
            }
            // Seed the reasoning trail (paper Theorem 4 — explainability).
            let trail = crate::pix_navigator::pix_trail(&tree, &result).join(" | ");
            ctx.let_bindings
                .insert(format!("__navigate_{out_name}_trail"), trail);
            // Seed drill keys: each leaf is reachable by its dotted title path.
            if !node.pix_ref.is_empty() {
                for l in &result.leaves {
                    let path_titles: Vec<String> = l
                        .path
                        .iter()
                        .filter_map(|id| tree.node(*id))
                        .filter(|n| n.title != "root")
                        .map(|n| n.title.to_lowercase())
                        .collect();
                    ctx.let_bindings.insert(
                        format!("__pix_{}_{}", node.pix_ref, path_titles.join(".")),
                        l.content.clone(),
                    );
                }
            }

            emit_step_complete(ctx, &out_name, step_index, &content, 0)?;
            return Ok(NodeOutcome::Completed {
                output: content,
                tokens_emitted: 0,
                step_index,
            });
        }
    }

    // ── Fallback (D5) — no indexable source in scope ──────────────────────
    let trail_clause = if node.trail_enabled { " (with trail)" } else { "" };
    let shape = PureShapeStep {
        name: if node.output_name.is_empty() {
            "Navigate".to_string()
        } else {
            node.output_name.clone()
        },
        user_prompt: format!(
            "Navigate corpus `{}` via PIX `{}` for query: {}{}",
            node.corpus_ref, node.pix_ref, query, trail_clause
        ),
        framing_addendum: Some(
            "You are navigating a PIX retrieval index. Trace your reasoning path; surface the document regions you crossed.".into(),
        ),
        kind_slug: "navigate",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
        stream_on_chunk: None,
    };
    run_pure_shape(shape, ctx).await
}

/// Corroborate handler — cross-validate a navigation result against
/// the referenced `navigate_ref`.
pub async fn run_corroborate(
    node: &IRCorroborateStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    let shape = PureShapeStep {
        name: if node.output_name.is_empty() {
            "Corroborate".to_string()
        } else {
            node.output_name.clone()
        },
        user_prompt: format!("Corroborate navigation result `{}`", node.navigate_ref),
        framing_addendum: Some(
            "You are corroborating. Cross-validate independently; surface agreement strength + disagreements.".into(),
        ),
        kind_slug: "corroborate",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
        stream_on_chunk: None,
    };
    run_pure_shape(shape, ctx).await
}

// ────────────────────────────────────────────────────────────────────
//  Wire-event helpers (shared with Remember/Recall/Forge)
// ────────────────────────────────────────────────────────────────────

fn emit_step_start(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    step_type: &str,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.to_string(),
            step_index,
            step_type: step_type.to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
}

fn emit_step_complete(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    full_output: &str,
    tokens_output: u64,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.to_string(),
            step_index,
            success: true,
            full_output: full_output.to_string(),
            tokens_input: 0,
            tokens_output,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
}

// ────────────────────────────────────────────────────────────────────
//  Unit tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;
    
    use crate::pem::InMemoryBackend;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn fresh_ctx() -> (
        DispatchCtx,
        mpsc::UnboundedReceiver<FlowExecutionEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let ctx = DispatchCtx::new(
            "TestFlow",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        );
        (ctx, rx)
    }

    // ── Remember ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_remember_literal_value_binds_to_let_bindings() {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "us-east-1".into(),
            memory_target: "region".into(),
        };
        let outcome = run_remember(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, .. } => {
                assert_eq!(output, "us-east-1");
                assert_eq!(tokens_emitted, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(ctx.let_bindings.get("region").unwrap(), "us-east-1");
    }

    #[tokio::test]
    async fn run_remember_resolves_expression_through_let_bindings() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("upstream".into(), "computed-X".into());
        let node = IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "upstream".into(),
            memory_target: "snapshot".into(),
        };
        run_remember(&node, &mut ctx).await.unwrap();
        assert_eq!(ctx.let_bindings.get("snapshot").unwrap(), "computed-X");
    }

    #[tokio::test]
    async fn run_remember_with_pem_persists_to_backend() {
        let backend: Arc<dyn crate::pem::PersistenceBackend> =
            Arc::new(InMemoryBackend::default());
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new(
            "F",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        )
        .with_pem(backend.clone())
        .with_session_id("session-1");

        let node = IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "persisted-value".into(),
            memory_target: "key1".into(),
        };
        run_remember(&node, &mut ctx).await.unwrap();

        // Verify PEM has the entry.
        let state = backend.restore("session-1").await.unwrap();
        assert_eq!(state.short_term_memory.len(), 1);
        assert_eq!(state.short_term_memory[0].key, "key1");
    }

    // ── Recall ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_recall_from_let_bindings_when_no_pem() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("region".into(), "us-east-1".into());
        let node = IRRecallStep {
            node_type: "recall",
            source_line: 0,
            source_column: 0,
            query: "current_region".into(),
            memory_source: "region".into(),
        };
        let outcome = run_recall(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => {
                assert_eq!(output, "us-east-1");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(
            ctx.let_bindings.get("current_region").unwrap(),
            "us-east-1"
        );
    }

    #[tokio::test]
    async fn run_recall_from_pem_when_backend_set() {
        let backend: Arc<dyn crate::pem::PersistenceBackend> =
            Arc::new(InMemoryBackend::default());
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new(
            "F",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        )
        .with_pem(backend.clone())
        .with_session_id("sess");

        // Plant a memory entry via Remember.
        run_remember(
            &IRRememberStep {
                node_type: "remember",
                source_line: 0,
                source_column: 0,
                expression: "value-from-pem".into(),
                memory_target: "pem_key".into(),
            },
            &mut ctx,
        )
        .await
        .unwrap();

        // Now Recall via PEM.
        let outcome = run_recall(
            &IRRecallStep {
                node_type: "recall",
                source_line: 0,
                source_column: 0,
                query: "recalled".into(),
                memory_source: "pem_key".into(),
            },
            &mut ctx,
        )
        .await
        .unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => {
                assert_eq!(output, "value-from-pem");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_recall_missing_key_returns_empty_string() {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRRecallStep {
            node_type: "recall",
            source_line: 0,
            source_column: 0,
            query: "x".into(),
            memory_source: "never_set".into(),
        };
        let outcome = run_recall(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => assert_eq!(output, ""),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Forge ─────────────────────────────────────────────────────────

    /// §Fase 86 — the fail-closed guarantee. The `stub` backend returns the
    /// SAME `"(stub)"` for every phase, so every illumination branch is
    /// identical to the obvious baseline ⇒ measured novelty NCD ≈ 0, below the
    /// floor. The forge MUST refuse to pass off a derivative result as creative
    /// and fail loudly (D86.6) — never a silent empty/derivative output.
    #[tokio::test]
    async fn run_forge_fails_closed_on_derivative_output() {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRForgeBlock {
            node_type: "forge",
            name: "Artwork".into(),
            seed: "aurora borealis over ancient ruins".into(),
            output_type: "Visual".into(),
            mode: "transformational".into(),
            novelty: 0.85,
            depth: 2,
            branches: 3,
            ..Default::default()
        };
        let result = run_forge(&node, &mut ctx).await;
        match result {
            Err(DispatchError::BackendError { name, message }) => {
                assert_eq!(name, "forge");
                assert!(
                    message.contains("forge.novelty_floor_breached"),
                    "expected novelty-floor rejection, got: {message}"
                );
            }
            other => panic!("expected a fail-closed forge rejection, got {other:?}"),
        }
    }

    // ── Cognitive framing handlers ────────────────────────────────────

    #[tokio::test]
    async fn run_focus_refuses_without_dataspace_engine() {
        // §Fase 108.a — the honesty floor: a `focus` with no engine port
        // REFUSES (MissingDependency), it does not narrate a selection.
        // The refusal is attributable: StepStart is emitted first.
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRFocusStep {
            node_type: "focus",
            source_line: 0,
            source_column: 0,
            expression: "key_insight".into(),
            where_expr: String::new(),
            select: Vec::new(),
            output: String::new(),
        };
        let err = run_focus(&node, &mut ctx).await.unwrap_err();
        assert!(
            matches!(err, DispatchError::MissingDependency { name: "dataspace_engine" }),
            "got: {err:?}"
        );
        match rx.try_recv().unwrap() {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "focus");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_associate_refuses_without_dataspace_engine() {
        // §Fase 108.a — a join whose matches would be invented is refused.
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRAssociateStep {
            node_type: "associate",
            source_line: 0,
            source_column: 0,
            left: "A".into(),
            right: "B".into(),
            using_field: "id".into(),
            output: String::new(),
        };
        let err = run_associate(&node, &mut ctx).await.unwrap_err();
        assert!(
            matches!(err, DispatchError::MissingDependency { name: "dataspace_engine" }),
            "got: {err:?}"
        );
        match rx.try_recv().unwrap() {
            FlowExecutionEvent::StepStart { step_type, step_name, .. } => {
                assert_eq!(step_type, "associate");
                // §108.d — the default output binding is `<L>_<R>` (a
                // BINDABLE identifier, unlike the old display arrow).
                assert_eq!(step_name, "A_B");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_aggregate_refuses_without_dataspace_engine() {
        // §Fase 108.a — an aggregate is a NUMBER; a narrated number is a
        // fabricated one. No engine ⇒ refusal.
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRAggregateStep {
            node_type: "aggregate",
            source_line: 0,
            source_column: 0,
            target: "events".into(),
            group_by: vec!["region".into()],
            alias: "by_region".into(),
            compute: Vec::new(),
            where_expr: String::new(),
        };
        let err = run_aggregate(&node, &mut ctx).await.unwrap_err();
        assert!(
            matches!(err, DispatchError::MissingDependency { name: "dataspace_engine" }),
            "got: {err:?}"
        );
        match rx.try_recv().unwrap() {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "aggregate");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_explore_refuses_without_dataspace_engine() {
        // §Fase 108.a — a profile over data never seen is a fabrication.
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRExploreStep {
            node_type: "explore",
            source_line: 0,
            source_column: 0,
            target: "hypothesis_space".into(),
            limit: Some(5),
            output: String::new(),
        };
        let err = run_explore(&node, &mut ctx).await.unwrap_err();
        assert!(
            matches!(err, DispatchError::MissingDependency { name: "dataspace_engine" }),
            "got: {err:?}"
        );
        match rx.try_recv().unwrap() {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "explore");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── §108.c — governed ingest, REAL ─────────────────────────────

    fn engine_with_leads() -> crate::dataspace_engine::SharedDataspaceEngine {
        let spec = axon_frontend::ir_nodes::IRDataspace {
            node_type: "dataspace",
            source_line: 1,
            source_column: 1,
            name: "Leads".to_string(),
            columns: vec![
                axon_frontend::ir_nodes::IRDataspaceColumn {
                    name: "email".into(),
                    column_type: "Text".into(),
                },
                axon_frontend::ir_nodes::IRDataspaceColumn {
                    name: "score".into(),
                    column_type: "Float".into(),
                },
            ],
        };
        std::sync::Arc::new(std::sync::RwLock::new(
            crate::dataspace_engine::DataspaceEngine::from_ir(&[spec]).unwrap(),
        ))
    }

    fn ingest_node(source: &str, format: &str) -> IRIngestStep {
        IRIngestStep {
            node_type: "ingest",
            source_line: 0,
            source_column: 0,
            source: source.into(),
            target: "Leads".into(),
            format: format.into(),
            max_bytes: None,
            max_rows: None,
        }
    }

    #[tokio::test]
    async fn run_ingest_loads_real_rows_with_untrusted_provenance() {
        let (mut ctx, _rx) = fresh_ctx();
        let engine = engine_with_leads();
        ctx = ctx.with_dataspace_engine(engine.clone());
        ctx.let_bindings.insert(
            "raw_leads".into(),
            "email,score\na@x.com,0.9\nb@x.com,0.4\n".into(),
        );
        let outcome = run_ingest(&ingest_node("raw_leads", "csv"), &mut ctx)
            .await
            .unwrap();
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, .. } => {
                assert_eq!(tokens_emitted, 0, "no LLM anywhere in an ingest");
                assert!(output.contains("\"rows\":2"), "{output}");
                assert!(output.contains("untrusted"), "{output}");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        let guard = engine.read().unwrap();
        let store = guard.store("Leads").unwrap();
        assert_eq!(store.row_count(), 2, "the rows are REAL");
        assert_eq!(
            store.batches()[0].provenance().taint,
            crate::emcp::EpistemicTaint::Untrusted,
            "born-Untrusted"
        );
        assert_eq!(store.batches()[0].provenance().source_sha256.len(), 64);
        // The summary binds under the step name for downstream refs.
        assert!(ctx.let_bindings.get("Leads").unwrap().contains("rows"));
    }

    #[tokio::test]
    async fn run_ingest_missing_source_binding_fails_closed() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx = ctx.with_dataspace_engine(engine_with_leads());
        let err = run_ingest(&ingest_node("nowhere", "csv"), &mut ctx)
            .await
            .unwrap_err();
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(message.contains("no in-scope binding"), "{message}");
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_ingest_type_mismatch_refuses_and_appends_nothing() {
        let (mut ctx, _rx) = fresh_ctx();
        let engine = engine_with_leads();
        ctx = ctx.with_dataspace_engine(engine.clone());
        ctx.let_bindings.insert(
            "raw_leads".into(),
            "email,score\na@x.com,not_a_float\n".into(),
        );
        let err = run_ingest(&ingest_node("raw_leads", "csv"), &mut ctx)
            .await
            .unwrap_err();
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(message.contains("`score`"), "names the column: {message}");
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
        assert_eq!(
            engine.read().unwrap().store("Leads").unwrap().row_count(),
            0,
            "a refused batch appends NOTHING (all-or-nothing)"
        );
    }

    #[tokio::test]
    async fn run_ingest_bounds_refuse_before_parse() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx = ctx.with_dataspace_engine(engine_with_leads());
        ctx.let_bindings
            .insert("raw_leads".into(), "\"".repeat(4096));
        let mut node = ingest_node("raw_leads", "csv");
        node.max_bytes = Some(64);
        let err = run_ingest(&node, &mut ctx).await.unwrap_err();
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(
                    message.contains("BEFORE parse"),
                    "bounds precede parsing: {message}"
                );
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_ingest_stale_ir_without_format_fails_closed() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx = ctx.with_dataspace_engine(engine_with_leads());
        ctx.let_bindings.insert("raw_leads".into(), "email,score\n".into());
        let err = run_ingest(&ingest_node("raw_leads", ""), &mut ctx)
            .await
            .unwrap_err();
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(message.contains("axon-T929"), "names the law: {message}");
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_ingest_refuses_without_dataspace_engine() {
        // §Fase 108.a — the load-bearing refusal: an `ingest` with no
        // engine port MUST NOT ask the model to pretend it loaded the
        // data (assertion-laundering; the mint/rotate argument verbatim).
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRIngestStep {
            node_type: "ingest",
            source_line: 0,
            source_column: 0,
            source: "external_api".into(),
            target: "raw".into(),
            format: "json".into(),
            max_bytes: None,
            max_rows: None,
        };
        let err = run_ingest(&node, &mut ctx).await.unwrap_err();
        assert!(
            matches!(err, DispatchError::MissingDependency { name: "dataspace_engine" }),
            "got: {err:?}"
        );
        match rx.try_recv().unwrap() {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "ingest");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_navigate_mdn_graph_when_ref_is_a_corpus() {
        // §Fase 63.B — a `navigate <corpus>` over a built MDN graph runs real
        // ε-informative graph navigation: from the seed, follow the edge to the
        // query-relevant document, not the irrelevant one. No LLM, no embeddings.
        use std::collections::HashMap;
        use std::sync::Arc;
        let corpus = crate::mdn::Corpus::from_declaration(
            &[
                "intro overview".to_string(),
                "liability limitation cap".to_string(),
                "termination notice".to_string(),
            ],
            &[
                ("cite".into(), "intro overview".into(), "liability limitation cap".into(), 0.9),
                ("cite".into(), "intro overview".into(), "termination notice".into(), 0.9),
            ],
        )
        .unwrap();
        let mut map = HashMap::new();
        map.insert("Sessions".to_string(), corpus);
        let (ctx, _rx) = fresh_ctx();
        let mut ctx = ctx.with_mdn_corpora(Arc::new(map));

        let node = IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: "Sessions".into(),
            corpus_ref: String::new(),
            query: "liability cap".into(),
            trail_enabled: true,
            output_name: "hits".into(),
            seed: "intro overview".into(),
            budget: Some(3),
            where_expr: String::new(),
        };
        let outcome = run_navigate(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => {
                assert!(output.contains("liability limitation cap"), "got: {output}");
                assert!(!output.contains("termination notice"), "uninformative doc not visited");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert!(ctx.let_bindings.get("hits").unwrap().contains("liability"));
        assert!(ctx.let_bindings.contains_key("__navigate_hits_trail"));
        // A non-adaptive corpus records no memory.
        assert!(ctx.mdn_histories.lock().unwrap().is_empty(), "non-adaptive records nothing");
    }

    #[tokio::test]
    async fn run_navigate_store_sourced_degrades_gracefully_without_postgres() {
        // §Fase 64.B — a `corpus … from axonstore` registered in
        // `mdn_store_sources`, navigated WITHOUT a Postgres backend, must
        // degrade to an empty result (no rows to read) rather than panic, and
        // still bind its output + complete the step. The full live-graph path is
        // exercised by the Postgres CI lane (no in-process DB here).
        use std::collections::HashMap;
        use std::sync::Arc;
        let mut sources = HashMap::new();
        sources.insert(
            "LtmGraph".to_string(),
            crate::ir_nodes::IRCorpusStoreSource {
                doc_store: "LtmSummaries".into(),
                doc_id: "id".into(),
                doc_title: "summary".into(),
                edge_store: "LtmEdges".into(),
                edge_from: "from_id".into(),
                edge_to: "to_id".into(),
                edge_type: "etype".into(),
                edge_weight: "weight".into(),
            },
        );
        let (ctx, _rx) = fresh_ctx();
        let mut ctx = ctx.with_mdn_store_sources(Arc::new(sources));

        let node = IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: "LtmGraph".into(),
            corpus_ref: String::new(),
            query: "anything".into(),
            trail_enabled: true,
            output_name: "hits".into(),
            seed: String::new(),
            budget: Some(5),
            where_expr: String::new(),
        };
        let outcome = run_navigate(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => {
                assert_eq!(output, "", "no Postgres backend → empty live graph");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        // The store-sourced branch took priority over the PIX/framing fallback
        // and bound the (empty) output.
        assert_eq!(ctx.let_bindings.get("hits").map(String::as_str), Some(""));
    }

    #[tokio::test]
    async fn run_navigate_adaptive_corpus_accumulates_memory() {
        // §Fase 63.C — navigations over an `adaptive` corpus apply the memory
        // endofunctor and record their trajectory, so the corpus learns.
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;
        let corpus = crate::mdn::Corpus::from_declaration(
            &["intro overview".to_string(), "liability cap".to_string()],
            &[("cite".into(), "intro overview".into(), "liability cap".into(), 0.5)],
        )
        .unwrap();
        let mut map = HashMap::new();
        map.insert("Mem".to_string(), corpus);
        let mut adaptive = HashSet::new();
        adaptive.insert("Mem".to_string());
        let (ctx, _rx) = fresh_ctx();
        let mut ctx = ctx
            .with_mdn_corpora(Arc::new(map))
            .with_mdn_adaptive(Arc::new(adaptive));

        let node = IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: "Mem".into(),
            corpus_ref: String::new(),
            query: "liability".into(),
            trail_enabled: false,
            output_name: "hits".into(),
            seed: "intro overview".into(),
            budget: Some(3),
            where_expr: String::new(),
        };
        // Two navigations accumulate two episodic outcomes.
        run_navigate(&node, &mut ctx).await.unwrap();
        run_navigate(&node, &mut ctx).await.unwrap();
        let hist = ctx.mdn_histories.lock().unwrap();
        assert_eq!(
            hist.get("Mem").map(|h| h.outcomes.len()),
            Some(2),
            "the adaptive corpus recorded both navigations"
        );
        // The recorded trajectory is the navigated path.
        assert!(hist.get("Mem").unwrap().outcomes[0].path.contains(&0));
    }

    #[tokio::test]
    async fn run_navigate_emits_navigate_slug() {
        // No indexable source in scope → falls back to the framing shape, which
        // still emits the `navigate` wire slug (D5 graceful degradation).
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: "main_pix".into(),
            corpus_ref: "law_corpus".into(),
            query: "interpret_clause".into(),
            trail_enabled: true,
            output_name: "nav_result".into(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
        };
        run_navigate(&node, &mut ctx).await.unwrap();
        let ev = rx.try_recv().unwrap();
        match ev {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "navigate");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_navigate_real_indexes_and_retrieves_embeddings_free() {
        // §Fase 62.A.2 — with the source document in scope, `navigate` runs the
        // REAL navigator: index → bounded BFS → retrieve the answering section,
        // bind it, and seed the reasoning trail. No LLM, no embeddings.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert(
            "ContractDoc".into(),
            "# Liability\n## Limitation\nLiability is capped at the contract value.\n\
             # Termination\n## Notice\nEither party may terminate with thirty days notice."
                .into(),
        );
        let node = IRNavigateStep {
            depth: None,
            node_type: "navigate",
            source_line: 0,
            source_column: 0,
            pix_ref: "ContractIndex".into(),
            corpus_ref: "ContractDoc".into(),
            query: "what is the liability limitation cap".into(),
            trail_enabled: true,
            output_name: "sections".into(),
            seed: String::new(),
            budget: None,
            where_expr: String::new(),
        };
        let outcome = run_navigate(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, .. } => {
                assert!(
                    output.contains("capped at the contract value"),
                    "expected the Limitation section, got: {output}"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        // Output bound under the declared name + reasoning trail seeded.
        assert!(ctx.let_bindings.get("sections").unwrap().contains("capped"));
        assert!(ctx.let_bindings.contains_key("__navigate_sections_trail"));
    }

    #[tokio::test]
    async fn run_corroborate_emits_corroborate_slug() {
        let (mut ctx, mut rx) = fresh_ctx();
        let node = IRCorroborateStep {
            node_type: "corroborate",
            source_line: 0,
            source_column: 0,
            navigate_ref: "nav_result".into(),
            output_name: "validated".into(),
        };
        run_corroborate(&node, &mut ctx).await.unwrap();
        let ev = rx.try_recv().unwrap();
        match ev {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "corroborate");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── Cancel guards ────────────────────────────────────────────────

    #[tokio::test]
    async fn every_cognitive_handler_short_circuits_on_cancel() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        // Remember
        let r = IRRememberStep {
            node_type: "remember",
            source_line: 0,
            source_column: 0,
            expression: "x".into(),
            memory_target: "y".into(),
        };
        assert!(matches!(
            run_remember(&r, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));

        // Recall
        let r = IRRecallStep {
            node_type: "recall",
            source_line: 0,
            source_column: 0,
            query: "q".into(),
            memory_source: "k".into(),
        };
        assert!(matches!(
            run_recall(&r, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));

        // Forge
        assert!(matches!(
            run_forge(
                &IRForgeBlock {
                    node_type: "forge",
                    source_line: 0,
                    source_column: 0,
                ..Default::default()
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));

        // Cognitive-framing handlers — all go through run_pure_shape
        // which has its own cancel guard.
        assert!(matches!(
            run_focus(
                &IRFocusStep {
                    node_type: "focus",
                    source_line: 0,
                    source_column: 0,
                    expression: "x".into(),
                    where_expr: String::new(),
                    select: Vec::new(),
                    output: String::new(),
                },
                &mut ctx,
            )
            .await,
            Err(DispatchError::UpstreamCancelled)
        ));
    }
}
