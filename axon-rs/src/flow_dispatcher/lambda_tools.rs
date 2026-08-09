//! §Fase 33.y.j — Lambda + UseTool. The final 2 variants needed
//! to reach 45/45 IRFlowNode graduation.
//!
//! Two variants graduated in 33.y.j:
//!
//! - **`LambdaDataApply`** (Fase 15 ΛD apply) — apply a named
//!   lambda data structure to a target expression. Sync runner
//!   walks a CPS dispatcher mapping lambda data structures to
//!   their result expressions. OSS reference impl uses the public
//!   helper [`apply_lambda_data`] which returns a canonical
//!   `"lambda:<name>(<target>)"` placeholder; enterprise R&D
//!   (axon_enterprise lambda runtime) wires the real CPS
//!   dispatcher.
//!
//! - **`UseTool`** (Fase 22 mid-step tool dispatch) — invoke a
//!   named tool with an argument. The full
//!   `ChatRequest.tools` cross-cutting plumb-through (D8) lands
//!   in 33.y.k as a cross-cutting fix that extends the
//!   `pure_shape` core. 33.y.j ships the OSS reference impl via
//!   the public helper [`invoke_tool`] which returns a canonical
//!   `"tool:<name>(<argument>)"` placeholder; enterprise R&D
//!   wires the real Fase 22 tool registry + dispatch.
//!
//! After 33.y.j: 45/45 IRFlowNode variants graduated. The legacy
//! `shim` becomes structurally unreachable from `dispatch_node`;
//! 33.y.l explicitly retires it.
//!
//! # D-letter anchors
//!
//! - **D1** — both variants have NAMED async handlers; the
//!   exhaustive match in `dispatch_node` reaches 45/45 graduation.
//! - **D3** — cancel checked at every `.await` boundary.
//! - **D7** — every error case routes through `DispatchError`;
//!   OSS helpers cannot fail (placeholder semantics); enterprise
//!   overrides surface `BackendError` for real lambda/tool
//!   runtime errors.
//! - **D8 (preview)** — UseTool is the 33.y.k cross-cutting
//!   anchor. 33.y.j ships the wire shape + helper surface; 33.y.k
//!   plumbs `ChatRequest.tools` through every `pure_shape`-routed
//!   handler so adopters declaring `apply: <tool>` see real
//!   tool-call events on the wire.
//! - **D10** — sync-runner parity: lambda apply + tool invocation
//!   produce deterministic placeholders for OSS path; enterprise
//!   integration preserves the SAME wire envelope (only inner
//!   content differs).

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{IRLambdaDataApply, IRUseToolStep};

// ────────────────────────────────────────────────────────────────────
//  Public helpers (enterprise hooks override these)
// ────────────────────────────────────────────────────────────────────

// §Fase 119.c — `apply_lambda_data` was DELETED here.
//
// It returned the string `"lambda:<name>(<target>)"` (§111 F18) while the
// REAL evaluator — `lambda_runtime::build_psi`, with Theorem 5.1 enforced at
// apply time — already existed on the sync runner path (Fase 15.c), which
// production stopped taking at the §65 unified-executor cutover. The engine
// was real; the live wire bound a placeholder. `run_lambda_data_apply` now
// drives the same evaluator, with D10 parity: both paths bind the serialized
// ψ, or refuse.

/// Invoke a tool with an argument. OSS default: resolves
/// `argument` through `ctx.let_bindings` (literal if missing) +
/// returns canonical `"tool:<name>(<resolved_argument>)"`.
/// Enterprise overrides hook the Fase 22 tool registry +
/// per-provider dispatch (Anthropic / OpenAI / etc.). The D8
/// cross-cutting fix (33.y.k) extends `pure_shape::run_pure_shape`
/// to plumb `ChatRequest.tools` so `apply: <tool>` on a Step
/// activates real upstream tool-calling on the wire.
pub fn invoke_tool(tool_name: &str, argument: &str, ctx: &DispatchCtx) -> String {
    let resolved_argument = ctx
        .let_bindings
        .get(argument)
        .cloned()
        .unwrap_or_else(|| argument.to_string());
    format!("tool:{tool_name}({resolved_argument})")
}

// ────────────────────────────────────────────────────────────────────
//  LambdaDataApply (Fase 15 ΛD apply)
// ────────────────────────────────────────────────────────────────────

/// §Fase 119.c — `lambda <Name> on <target> [-> <binding>]`: ΛD elevation,
/// REAL on the production dispatch path.
///
/// Resolves the declaration from `ctx.lambda_data_specs` (fail closed —
/// an unresolvable ΛD elevates nothing, so nothing is bound), resolves the
/// target through `ctx.let_bindings`, enforces Theorem 5.1 at apply time
/// (`lambda_runtime::enforce_theorem_5_1`, inside `build_psi`: only `raw`
/// data may carry c = 1.0), and binds the serialized ψ = ⟨T, V, E⟩ — the
/// SAME shape the sync runner has always bound (D10 parity).
pub async fn run_lambda_data_apply(
    node: &IRLambdaDataApply,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.lambda_data_name.is_empty() {
        "LambdaApply".to_string()
    } else {
        node.lambda_data_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "lambda_data_apply")?;

    let psi_json = elevate_lambda(&node.lambda_data_name, &node.target, ctx)?;

    let output_key = if !node.output_type.is_empty() {
        node.output_type.clone()
    } else if !node.target.is_empty() {
        format!("{}_lambda_applied", node.target)
    } else {
        String::new()
    };
    if !output_key.is_empty() {
        ctx.let_bindings.insert(output_key, psi_json.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &psi_json, 0, true)?;

    Ok(NodeOutcome::Completed {
        output: psi_json,
        tokens_emitted: 0,
        step_index,
    })
}

/// §Fase 119.c — the shared ΛD elevation: declaration → ψ → serialized JSON.
///
/// Used by the flow-level node above and by the step-scoped `lambda` guard
/// in `pure_shape::run_step` (README blocks 46-47's position), which runs it
/// BEFORE prompt interpolation so the elevated binding is in scope for the
/// step's `ask`.
pub(crate) fn elevate_lambda(
    lambda_name: &str,
    target: &str,
    ctx: &DispatchCtx,
) -> Result<String, DispatchError> {
    let blame = format!("lambda:{lambda_name}");
    let spec = ctx
        .lambda_data_specs
        .iter()
        .find(|l| l.name == lambda_name)
        .ok_or_else(|| DispatchError::BackendError {
            name: blame.clone(),
            message: format!(
                "lambda '{lambda_name}' is not in the compiled program's \u{039b}D \
                 catalog; nothing can be elevated, so nothing is bound. (The type \
                 checker rejects unknown names statically \u{2014} an empty catalog \
                 here usually means the deploy path did not attach \
                 `lambda_data_specs`.)"
            ),
        })?;

    let snapshot = crate::lambda_runtime::SpecSnapshot {
        name: spec.name.clone(),
        ontology: spec.ontology.clone(),
        certainty: spec.certainty,
        temporal_frame_start: spec.temporal_frame_start.clone(),
        temporal_frame_end: spec.temporal_frame_end.clone(),
        provenance: spec.provenance.clone(),
        derivation: spec.derivation.clone(),
    };
    let resolved_target = ctx
        .let_bindings
        .get(target)
        .cloned()
        .unwrap_or_else(|| target.to_string());

    let psi = crate::lambda_runtime::build_psi(
        &snapshot,
        serde_json::Value::String(resolved_target),
    )
    .map_err(|e| DispatchError::BackendError {
        name: blame,
        message: e.to_string(),
    })?;

    serde_json::to_string(&psi).map_err(|e| DispatchError::BackendError {
        name: format!("lambda:{lambda_name}"),
        message: format!("\u{03c8} serialization failed: {e}"),
    })
}

pub async fn run_use_tool(
    node: &IRUseToolStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.tool_name.is_empty() {
        "UseTool".to_string()
    } else {
        node.tool_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "use_tool")?;

    // ── §Fase 114.a — THE LIVE BUG, CLOSED ───────────────────────────────────
    //
    // This is the canonical tool-call path. It is the one `advertised.rs` cites
    // as PROOF that `tool` is Real (`lambda_tools::dispatch_use_tool_real`).
    //
    // **It had no budget gate.** `grep -c budget` on this file returned 0.
    //
    // So `budget { rate: 100 per minute on Tool(Search) }` parsed, type-checked
    // (T830–T834), built a real fail-closed token bucket — and then governed
    // NOTHING in an ordinary flow. The gate existed, but only on the streaming
    // path, only inside a daemon, only on the enterprise supervisor.
    //
    // An adopter declared a budget over their vendor and the compiler said yes.
    //
    // Charged BEFORE the dispatch, deliberately: a budget charged after the call
    // bounds nothing — the vendor was already hit and the money already spent.
    // Over-emission must be impossible by construction, not merely reported.
    match crate::flow_dispatcher::budget_gate::charge(ctx, &node.tool_name)? {
        crate::flow_dispatcher::budget_gate::BudgetGrant::Granted => {}
        // `shed` — the call is NOT made and the flow continues with empty output.
        // Marked in the audit trail: a skipped call that leaves no trace is
        // indistinguishable from a call that returned nothing.
        crate::flow_dispatcher::budget_gate::BudgetGrant::Shed { .. } => {
            if !node.tool_name.is_empty() {
                ctx.let_bindings
                    .insert(format!("{}_result", node.tool_name), String::new());
            }
            emit_step_complete(ctx, &step_name, step_index, "", 0, true)?;
            return Ok(NodeOutcome::Completed {
                output: String::new(),
                tokens_emitted: 0,
                step_index,
            });
        }
    }

    // §Fase 114.f — charge the tool's channel lease BEFORE the call. A post-expiry
    // vendor call is a CT-2 Anchor Breach (the τ-decaying affine capability was
    // used past expiry), exactly as a post-expiry store op is in §113.d — a tool
    // call is a *use* of the resource just as a store op is. Charged before the
    // dispatch: a breach reported after the vendor was hit bounds nothing.
    if let Some(breach) = charge_tool_lease(node, ctx) {
        let step_index_now = step_index;
        emit_step_complete(ctx, &step_name, step_index_now, &breach, 0, false)?;
        if !node.tool_name.is_empty() {
            ctx.let_bindings
                .insert(format!("{}_result", node.tool_name), breach.clone());
        }
        return Ok(NodeOutcome::Completed {
            output: breach,
            tokens_emitted: 0,
            step_index,
        });
    }

    // §Fase 114.e — hold a channel permit ACROSS the call, so at most
    // `resource.capacity` calls are in flight against this tool's channel at once.
    //
    // §114.d derived the capacity; this is what turns the number into a bound.
    // Acquired BEFORE the dispatch and held (the guard lives to the end of the
    // scope) until the call returns — a bound applied after the call bounds
    // nothing, the vendor was already hit. A tool whose resource has no capacity
    // (or no resource) acquires nothing and proceeds unbounded, as before.
    let _channel_permit = acquire_channel_permit(node, ctx).await;

    // §Fase 58.f.2 — attempt a real dispatch; honor cancel observed
    // while the (potentially blocking, network-bound) call ran.
    let (result, success) = match dispatch_use_tool_real(node, ctx).await {
        Some(tool_result) => (tool_result.output, tool_result.success),
        // D5 — no registry / unregistered / LLM-routed provider →
        // the canonical placeholder, unchanged from pre-58.
        None => (invoke_tool(&node.tool_name, &node.argument, ctx), true),
    };
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }

    if !node.tool_name.is_empty() {
        ctx.let_bindings
            .insert(format!("{}_result", node.tool_name), result.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &result, 0, success)?;

    Ok(NodeOutcome::Completed {
        output: result,
        tokens_emitted: 0,
        step_index,
    })
}

/// §Fase 58.f.2 — attempt a REAL tool dispatch on the streaming path.
///
/// Returns `Some(ToolResult)` when `ctx.tool_registry` resolves the
/// tool to a locally-dispatchable provider; `None` when there is no
/// registry, the tool is unregistered, or its provider falls through
/// to the LLM — the caller then keeps the canonical placeholder (D5).
///
/// The structured `use Tool(k = v, …)` body is assembled with the
/// SAME `(name, type)` coercion the synchronous server path applies
/// (`runner::build_structured_tool_body`, §58.e), reading the typed
/// schema carried on the [`crate::tool_registry::ToolEntry`] (§58.f.2
/// piece 1). Interpolation of arg values mirrors the sync path's
/// [`crate::exec_context::ExecContext::interpolate`] via the shared
/// `interpolate_vars` helper over `ctx.let_bindings`.
///
/// `registry.dispatch` uses a blocking `reqwest` client for the
/// `http` / `mcp` providers; calling it directly inside the tokio
/// runtime would panic, so the dispatch runs on the blocking pool via
/// `spawn_blocking` (D6). The request-scoped registry is `Arc`-cloned
/// into the task — never a shared mutable global (D10).
/// §Fase 114.f — charge a tool's channel lease by tool NAME, so BOTH the
/// canonical `use Tool(…)` path and the streaming step-tool path share it.
///
/// Returns `Some(breach_message)` when the lease over the tool's resource has
/// expired (the CT-2 Anchor Breach) — the caller must NOT make the call. `None` ⇒
/// no lease governs this channel, or it is live.
pub fn charge_tool_lease_by_name(tool_name: &str, ctx: &DispatchCtx) -> Option<String> {
    let leases = ctx.tool_leases.as_ref()?;
    let registry = ctx.tool_registry.as_ref()?;
    let entry = registry.get(tool_name)?;
    if entry.resource_ref.is_empty() {
        // An un-resourced tool is ineligible for lease governance — you cannot
        // govern what you did not declare (§113's ratified posture).
        return None;
    }
    match leases.charge(&entry.resource_ref) {
        Ok(()) => None,
        Err(breach) => Some(breach.to_string()),
    }
}

fn charge_tool_lease(node: &IRUseToolStep, ctx: &DispatchCtx) -> Option<String> {
    charge_tool_lease_by_name(&node.tool_name, ctx)
}

/// §Fase 114.e — acquire a concurrency permit for a tool's channel by NAME, so
/// both tool paths share it. The returned guard bounds simultaneous calls while it
/// lives; `None` ⇒ the tool names no capacity-bounded resource (unbounded, pre-§114).
pub async fn acquire_channel_permit_by_name(
    tool_name: &str,
    ctx: &DispatchCtx,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    let sems = ctx.channel_semaphores.as_ref()?;
    let registry = ctx.tool_registry.as_ref()?;
    let entry = registry.get(tool_name)?;
    if entry.resource_ref.is_empty() {
        return None;
    }
    let sem = sems.for_resource(&entry.resource_ref)?;
    // `acquire_owned` waits if the channel is at capacity — which is exactly the
    // bound: the N+1th concurrent call parks until one of the N in flight returns.
    sem.acquire_owned().await.ok()
}

async fn acquire_channel_permit(
    node: &IRUseToolStep,
    ctx: &DispatchCtx,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    acquire_channel_permit_by_name(&node.tool_name, ctx).await
}

async fn dispatch_use_tool_real(
    node: &IRUseToolStep,
    ctx: &DispatchCtx,
) -> Option<crate::tool_executor::ToolResult> {
    let registry = ctx.tool_registry.clone()?;
    // Resolve the typed input schema for coercion (+ the §94.c secret
    // key). The borrows end here (cloned) so `registry` can move into
    // `spawn_blocking`.
    let entry = registry.get(&node.tool_name)?;
    let parameters = entry.parameters.clone();
    let secret_key = entry.secret.clone();
    // §Fase 95.a — the partition parameter name (`selection_without_revelation`).
    // Empty ⇒ the §94 static-key path, unchanged.
    let secret_partition = entry.secret_partition.clone();

    // Assemble the request argument: a structured JSON body for the
    // keyword form (D2), or the interpolated single argument for the
    // legacy `on <arg>` form (D5).
    let mut argument = if node.named_args.is_empty() {
        crate::exec_context::interpolate_vars(&node.argument, &ctx.let_bindings)
    } else {
        let interpolated: Vec<(String, String)> = node
            .named_args
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    // §Fase 60 — resolve by value_kind: a `"reference"` (bare
                    // identifier / `Step.output`) is a binding lookup, not a
                    // literal name; `"literal"` keeps `${…}` interpolation.
                    crate::exec_context::resolve_named_arg_value(
                        &a.value,
                        &a.value_kind,
                        &ctx.let_bindings,
                    ),
                )
            })
            .collect();
        crate::runner::build_structured_tool_body(&interpolated, &parameters)
    };

    // §Fase 94.c — dispatch injection (`rotation_without_revelation`):
    // a tool declaring `secret: <key>` gets the per-tenant custody value
    // injected under the reserved `axon_secret` field of its structured
    // body. Fail-CLOSED at every fork: no custody port, a custody
    // refusal, or a non-JSON legacy body all fail the dispatch with a
    // witness — a declared injection is never silently skipped (the tool
    // would call its vendor unauthenticated and 401 confusingly, or
    // worse, fall back to ambient credentials).
    if !secret_key.is_empty() {
        let refuse = |message: String| {
            Some(crate::tool_executor::ToolResult {
                success: false,
                output: message,
                tool_name: node.tool_name.clone(),
            })
        };
        let Some(custody) = ctx.secret_custody.clone() else {
            return refuse(format!(
                "tool '{}' declares `secret: {}` but no secret_custody port is \
                 configured — injection fails closed (never an unauthenticated \
                 vendor call)",
                node.tool_name, secret_key
            ));
        };
        // The body must be a JSON object BEFORE the reveal now: §95 reads the
        // partition segment out of it to resolve the custody key, and the
        // reveal must target the resolved (per-sub-tenant) key, not the class
        // key. Non-structured argument ⇒ fail closed (unchanged §94 reason).
        let mut body: serde_json::Value = match serde_json::from_str(&argument) {
            Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
            _ => {
                return refuse(format!(
                    "tool '{}' declares `secret:` but was invoked with a \
                     non-structured argument — secret injection requires the \
                     `use {}(k = v, …)` keyword form (the body must be a JSON \
                     object to carry the reserved `axon_secret` field)",
                    node.tool_name, node.tool_name
                ))
            }
        };
        // §Fase 95.a — resolve the custody KEY (`selection_without_revelation`).
        // With no partition this is the §94 static class key. With a partition,
        // the caller-supplied value of that parameter is appended as ONE key
        // segment: `crm.hubspot` + `.` + `acme` → `crm.hubspot.acme`. The
        // segment is charset-validated to a single dot-free lowercase run, so
        // the resolved key can NEVER escape the tool's declared class (no `.`
        // to widen the prefix, no `/`/`:` to reach a URL). Fail-closed at every
        // fork: a missing, non-string, empty, or ill-charactered discriminator
        // refuses the dispatch with a witness — a program NEVER spends the
        // wrong tenant's credential, and never reaches for a key outside class.
        let resolved_key = if secret_partition.is_empty() {
            secret_key.clone()
        } else {
            let segment = match body.get(&secret_partition) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(_) | None => {
                    return refuse(format!(
                        "tool '{}' declares `secret_partition: {}` but the call did \
                         not bind that parameter to a string value — the partition \
                         segment is unresolved, so the per-sub-tenant secret key \
                         cannot be addressed (injection fails closed)",
                        node.tool_name, secret_partition
                    ))
                }
            };
            if segment.is_empty() || !is_key_segment(&segment) {
                return refuse(format!(
                    "tool '{}' partition value for `{}` is not a valid key segment \
                     ('{}') — a partition segment is a single non-empty run of \
                     `[a-z0-9_-]` (no `.`, no `/`, no `:`, no uppercase) so the \
                     resolved custody key can never leave the `{}` class. \
                     Injection fails closed.",
                    node.tool_name, secret_partition, segment, secret_key
                ));
            }
            format!("{secret_key}.{segment}")
        };
        let revealed = match custody.reveal_for_dispatch(&ctx.tenant_id, &resolved_key).await
        {
            Ok(r) => r,
            Err(e) => {
                return refuse(format!(
                    "tool '{}' secret injection refused by custody: {e}",
                    node.tool_name
                ))
            }
        };
        body["axon_secret"] = serde_json::Value::String(revealed.value.clone());
        drop(revealed);
        argument = body.to_string();
    }

    let tool_name = node.tool_name.clone();
    let registry_for_task = registry.clone();
    match tokio::task::spawn_blocking(move || {
        registry_for_task.dispatch(&tool_name, &argument)
    })
    .await
    {
        Ok(opt) => opt,
        // A join failure (panic in the blocking task) surfaces as a
        // failed ToolResult rather than propagating a panic to the
        // dispatcher — the consumer sees a clean error, never a hang.
        Err(join_err) => Some(crate::tool_executor::ToolResult {
            success: false,
            output: format!(
                "tool '{}' dispatch task failed: {join_err}",
                node.tool_name
            ),
            tool_name: node.tool_name.clone(),
        }),
    }
}

/// §Fase 95.a — is `s` a single custody-key SEGMENT? A segment is a
/// non-empty run of `[a-z0-9_-]` — deliberately a SUBSET of the T850 key
/// charset (`[a-z0-9_.-]`) with the dot REMOVED: the dot is the class
/// separator, so a partition segment that contained one could widen the
/// resolved key past the tool's compile-time-pinned class. No `/`, no `:`,
/// no uppercase either — a URL or credential can never pass as a segment.
/// (Emptiness is checked by the caller so the refusal message can name it.)
fn is_key_segment(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

// ────────────────────────────────────────────────────────────────────
//  Wire-event helpers
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
    success: bool,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.to_string(),
            step_index,
            success,
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

    // §Fase 119.c — `apply_lambda_data_literal_target` and
    // `apply_lambda_data_resolves_target_through_bindings` were DELETED
    // here. They pinned the placeholder string "lambda:<name>(<target>)" in
    // green — the §111 F18 shape, tested and passing — while the real
    // evaluator sat unreached on the dead sync path. The elevation suite
    // below replaces them.

    fn lambda_spec(name: &str, certainty: f64, derivation: &str) -> crate::ir_nodes::IRLambdaData {
        crate::ir_nodes::IRLambdaData {
            node_type: "lambda_data",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            ontology: "measurement.temperature.celsius".into(),
            certainty,
            temporal_frame_start: "2026-01-01T00:00:00Z".into(),
            temporal_frame_end: "2026-12-31T23:59:59Z".into(),
            provenance: "Sensor_X".into(),
            derivation: derivation.into(),
        }
    }

    #[tokio::test]
    async fn a_declared_lambda_elevates_to_a_real_psi_not_a_placeholder() {
        // THE §111 F18 regression test for lambda: this used to bind the
        // string "lambda:SensorReading(21.5)".
        let (mut ctx, _rx) = fresh_ctx();
        ctx.lambda_data_specs =
            std::sync::Arc::new(vec![lambda_spec("SensorReading", 0.95, "raw")]);
        ctx.let_bindings.insert("reading".into(), "21.5".into());
        let node = IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: "SensorReading".into(),
            target: "reading".into(),
            output_type: "typed".into(),
        };
        run_lambda_data_apply(&node, &mut ctx).await.expect("elevates");
        let bound = ctx.let_bindings.get("typed").expect("psi bound").clone();
        let psi: serde_json::Value = serde_json::from_str(&bound).expect("psi is JSON");
        assert_eq!(psi["T"], "measurement.temperature.celsius");
        assert_eq!(psi["V"], "21.5");
        assert_eq!(psi["E"]["c"], 0.95);
        assert_eq!(psi["E"]["rho"], "Sensor_X");
        assert!(
            !bound.contains("lambda:"),
            "the placeholder string shape must be dead: {bound}"
        );
    }

    #[tokio::test]
    async fn theorem_5_1_is_enforced_at_apply_time_on_the_dispatch_path() {
        // c = 1.0 with derivation 'inferred' violates the Epistemic
        // Degradation Theorem: only raw data may carry absolute certainty.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.lambda_data_specs =
            std::sync::Arc::new(vec![lambda_spec("Overclaim", 1.0, "inferred")]);
        let node = IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: "Overclaim".into(),
            target: "x".into(),
            output_type: "out".into(),
        };
        let err = run_lambda_data_apply(&node, &mut ctx)
            .await
            .err()
            .expect("theorem violation refuses");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "lambda:Overclaim");
                assert!(message.contains("Theorem 5.1"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(
            ctx.let_bindings.get("out").is_none(),
            "nothing binds on a refused elevation"
        );
    }

    #[tokio::test]
    async fn an_undeclared_lambda_fails_closed_never_placeholder() {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: "ghost".into(),
            target: "x".into(),
            output_type: "out".into(),
        };
        let err = run_lambda_data_apply(&node, &mut ctx)
            .await
            .err()
            .expect("undeclared refuses");
        match err {
            DispatchError::BackendError { name, message } => {
                assert_eq!(name, "lambda:ghost");
                assert!(message.contains("nothing is bound"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_lambda_step_guard_elevates_before_the_prompt_interpolates() {
        // README blocks 46-47's shape: the elevated binding must be in scope
        // for the step's ask. The stub backend answers the generation; the
        // assertion is on the BINDING the guard produced.
        let (mut ctx, _rx) = fresh_ctx();
        ctx.lambda_data_specs =
            std::sync::Arc::new(vec![lambda_spec("RawQuote", 0.9, "inferred")]);
        ctx.let_bindings.insert("ticker".into(), "ACME".into());
        let step = crate::ir_nodes::IRStep {
            node_type: "step",
            source_line: 0,
            source_column: 0,
            name: "Price".into(),
            persona_ref: String::new(),
            given: String::new(),
            ask: "assess ${verified_quote}".into(),
            use_tool: None,
            probe: None,
            reason: None,
            weave: None,
            output_type: String::new(),
            confidence_floor: None,
            navigate_ref: String::new(),
            apply_ref: String::new(),
            requires_context: None,
            now_tz: None,
            pix_ops: Vec::new(),
            stream: None,
            guards: vec![crate::ir_nodes::IRStepGuard {
                kind: "lambda".into(),
                name: "RawQuote".into(),
                target: "ticker".into(),
                binding: "verified_quote".into(),
            }],
            body: Vec::new(),
        };
        super::super::pure_shape::run_step(&step, &mut ctx)
            .await
            .expect("guarded step runs via the stub");
        let bound = ctx.let_bindings.get("verified_quote").expect("elevated");
        let psi: serde_json::Value = serde_json::from_str(bound).expect("json");
        assert_eq!(psi["V"], "ACME");
        assert_eq!(psi["E"]["delta"], "inferred");
    }

    // ── invoke_tool ──────────────────────────────────────────────────

    #[test]
    fn invoke_tool_literal_argument() {
        let (ctx, _rx) = fresh_ctx();
        assert_eq!(
            invoke_tool("calculator", "2+2", &ctx),
            "tool:calculator(2+2)"
        );
    }

    #[test]
    fn invoke_tool_resolves_argument_through_bindings() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert("query".into(), "weather today".into());
        assert_eq!(
            invoke_tool("web_search", "query", &ctx),
            "tool:web_search(weather today)"
        );
    }

    // ── LambdaDataApply ──────────────────────────────────────────────

    #[tokio::test]
    /// §Fase 119.c — these two tests used to pin the PLACEHOLDER in green:
    /// `"lambda:transform(raw)"` bound as if it were an elevation. They now
    /// declare the lambdas they apply and assert the wire shape over a real ψ.
    async fn run_lambda_data_apply_binds_under_output_type() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.lambda_data_specs =
            std::sync::Arc::new(vec![lambda_spec("transform", 0.8, "derived")]);
        ctx.let_bindings.insert("input_data".into(), "raw".into());
        let node = IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: "transform".into(),
            target: "input_data".into(),
            output_type: "transformed".into(),
        };
        let outcome = run_lambda_data_apply(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, .. } => {
                let psi: serde_json::Value = serde_json::from_str(&output).expect("ψ JSON");
                assert_eq!(psi["V"], "raw");
                assert_eq!(psi["E"]["c"], 0.8);
                assert_eq!(tokens_emitted, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert!(ctx.let_bindings.get("transformed").is_some());
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "lambda_data_apply");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_lambda_data_apply_canonical_fallback() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.lambda_data_specs =
            std::sync::Arc::new(vec![lambda_spec("norm", 0.8, "derived")]);
        let node = IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: "norm".into(),
            target: "doc".into(),
            output_type: String::new(),
        };
        run_lambda_data_apply(&node, &mut ctx).await.unwrap();
        let bound = ctx
            .let_bindings
            .get("doc_lambda_applied")
            .expect("canonical fallback key");
        let psi: serde_json::Value = serde_json::from_str(bound).expect("ψ JSON");
        assert_eq!(psi["V"], "doc", "unresolved binding elevates the literal");
    }

    // ── UseTool ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_use_tool_binds_under_canonical_result_key() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.let_bindings.insert("input".into(), "5+3".into());
        let node = IRUseToolStep {
            node_type: "use_tool",
            source_line: 0,
            source_column: 0,
            tool_name: "calculator".into(),
            argument: "input".into(),
            named_args: Vec::new(),
        };
        let outcome = run_use_tool(&node, &mut ctx).await.unwrap();
        match outcome {
            NodeOutcome::Completed { output, tokens_emitted, .. } => {
                assert_eq!(output, "tool:calculator(5+3)");
                assert_eq!(tokens_emitted, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(
            ctx.let_bindings.get("calculator_result").unwrap(),
            "tool:calculator(5+3)"
        );
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "use_tool");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── Cancel guards ────────────────────────────────────────────────

    #[tokio::test]
    async fn lambda_and_use_tool_short_circuit_on_cancel() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let lambda = IRLambdaDataApply {
            node_type: "lambda_data_apply",
            source_line: 0,
            source_column: 0,
            lambda_data_name: "x".into(),
            target: "y".into(),
            output_type: "z".into(),
        };
        assert!(matches!(
            run_lambda_data_apply(&lambda, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));

        let ut = IRUseToolStep {
            node_type: "use_tool",
            source_line: 0,
            source_column: 0,
            tool_name: "x".into(),
            argument: "y".into(),
            named_args: Vec::new(),
        };
        assert!(matches!(
            run_use_tool(&ut, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));
    }
}
